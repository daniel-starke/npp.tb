//! Use case diagrams (`usecase.chevrotain.ts`, `usecaseDb.ts`, `usecaseRenderer.ts`,
//! `styles.ts`), laid out with dagre like the flowchart.
//!
//! Mermaid 12.2 lays them out with ELK by default; until ELK is ported, dagre lays out every use
//! case diagram, as with `layout: dagre`.

mod json;
mod lexer;
mod model;
mod parser;

use super::common::sanitize_text;
use super::icons;
use super::label::{self, encode_entities};
use super::rendering::shapes::usecase::{ActorVariant, UsecaseNode};
use super::rendering::{self, DiagramStyle, Edge, LayoutData, Node, Pattern};
use super::svg::{self, Accessibility, Frame};
use super::text::Font;
use super::{Context, Diagram, Error, Result};
use model::{ActorType, BoundaryType, Model};
use parser::{ArrowType, LabelType, RelationshipType, UseCaseShape};

pub use json::js_number_string;

const SECTION: &str = "usecase";

/// `usecaseNodeDomId`: the id of an element without the diagram prefix.
fn dom_id(id: &str) -> String {
	let mut safe = String::with_capacity(id.len());
	let mut in_run = false;
	for character in id.chars() {
		if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
			safe.push(character);
			in_run = false;
		} else if !in_run {
			safe.push('_');
			in_run = true;
		}
	}
	let safe = safe.trim_matches('_');
	if safe.is_empty() { "element".to_owned() } else { safe.to_owned() }
}

/// `escapePlainLabel`.
fn escape_plain(label: &str) -> String {
	label.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `escapeMarkdownMarkers`.
fn escape_markdown_markers(label: &str) -> String {
	let mut escaped = String::with_capacity(label.len());
	for character in label.chars() {
		if matches!(character, '*' | '[' | '\\' | ']' | '_' | '`') {
			escaped.push('\\');
		}
		escaped.push(character);
	}
	escaped
}

/// `getAccessibleLabel`: the words of a Markdown label without its formatting.
fn accessible_label(text: &str, label_type: LabelType) -> String {
	if label_type != LabelType::Markdown {
		return label::decode_entities_to_text(text);
	}
	label::label_lines(text, true)
		.iter()
		.map(|line| {
			line.iter()
				.map(|run| run.text.trim())
				.filter(|word| !word.is_empty())
				.collect::<Vec<_>>()
				.join(" ")
		})
		.collect::<Vec<_>>()
		.join("\n")
}

/// `getCompiledStyles`: the `default` class and the classes, a later property replacing an earlier
/// one in its place.
fn compiled_styles(model: &Model, classes: &[String]) -> Vec<String> {
	let mut compiled: Vec<(String, String)> = Vec::new();
	for class in std::iter::once("default").chain(classes.iter().map(String::as_str)) {
		let Some((_, styles)) = model.class_defs.iter().find(|(id, _)| id == class) else {
			continue;
		};
		for raw in styles {
			let style = raw.trim();
			let property = style.split_once(':').map_or(style, |(property, _)| property).trim();
			if property.is_empty() {
				continue;
			}
			match compiled.iter_mut().find(|(known, _)| known == property) {
				Some(entry) => entry.1 = style.to_owned(),
				None => compiled.push((property.to_owned(), style.to_owned())),
			}
		}
	}
	compiled.into_iter().map(|(_, style)| style).collect()
}

/// The font of a label class of the configuration: `<prefix>FontSize`, `…Family`, `…Weight`.
fn configured_font(context: &Context, prefix: &str, size: f64) -> Font {
	let size_key = format!("{prefix}FontSize");
	let size = match context.config_value(SECTION, &size_key) {
		Some(value) => value
			.to_text()
			.and_then(|text| super::text::parse_font_size(&text, size))
			.filter(|size| size.is_finite() && *size > 0.0)
			.unwrap_or(size),
		None => size,
	};
	let family = context
		.config_text(SECTION, &format!("{prefix}FontFamily"))
		.filter(|family| !family.is_empty())
		.unwrap_or_else(|| "\"Open Sans\", sans-serif".to_owned());
	let mut font = Font::new(family, size);
	match context.config_text(SECTION, &format!("{prefix}FontWeight")).as_deref() {
		Some("bold" | "bolder") => font.weight = 700,
		Some(weight) => {
			if let Ok(weight) = weight.parse::<u16>() {
				font.weight = weight;
			}
		}
		None => {}
	}
	font
}

/// The label of an element as the renderer gets it: sanitized, and plain text escaped.
fn node_label(text: &str, label_type: LabelType) -> String {
	let sanitized = sanitize_text(text);
	match label_type {
		LabelType::Text => escape_plain(&sanitized),
		LabelType::Markdown => sanitized,
	}
}

fn theme_color(context: &Context, keys: &[&str]) -> String {
	keys.iter().find_map(|key| context.theme.text(key)).unwrap_or_default()
}

/// `getData` and `prepareUsecaseLayoutData`.
fn layout_data(model: &Model, context: &Context) -> LayoutData {
	let look = context.look;
	let wrapping_width = context.config_number(SECTION, "wrappingWidth", 120.0);
	let min_width = context.config_number(SECTION, "minNodeWidth", 120.0);
	let primary_text = theme_color(context, &["primaryTextColor"]);
	let actor_font = configured_font(context, "actor", 14.0);
	let usecase_font = configured_font(context, "usecase", 12.0);
	let text_of = |id: &str| -> (String, LabelType) {
		let element = model.actors.iter().chain(&model.use_cases).find(|element| element.id == id);
		match element {
			Some(element) => (element.label.text.clone(), element.label.label_type),
			None => (id.to_owned(), LabelType::Text),
		}
	};
	let accessible_of = |id: &str| {
		let (text, label_type) = text_of(id);
		accessible_label(&sanitize_text(&text), label_type)
	};
	let base_node = |id: &str, label: String, is_markdown: bool, shape: &str| Node {
		id: id.to_owned(),
		dom_id: format!("usecase-{}", dom_id(id)),
		label,
		is_markdown,
		look,
		shape: shape.to_owned(),
		wrapping_width: Some(wrapping_width),
		width_in_columns: 1.0,
		..Node::default()
	};
	let attributes = |id: &str, kind: &'static str, name: String| {
		vec![
			("data-usecase-id", id.to_owned()),
			("data-usecase-kind", kind.to_string()),
			("role", "img".to_owned()),
			("aria-label", name),
		]
	};
	let mut nodes = Vec::new();
	let mut color_index = 0;
	for actor in &model.actors {
		let variant = match actor.actor_type {
			ActorType::Normal => ActorVariant::Normal,
			ActorType::Hollow => ActorVariant::Hollow,
			ActorType::Awesome => ActorVariant::Awesome,
			ActorType::Icon => ActorVariant::Icon,
		};
		let shape = match variant {
			ActorVariant::Normal => "usecaseActor",
			ActorVariant::Hollow => "usecaseActorHollow",
			ActorVariant::Awesome => "usecaseActorAwesome",
			ActorVariant::Icon => "usecaseActorIcon",
		};
		let is_markdown = actor.label.label_type == LabelType::Markdown;
		let label = node_label(&actor.label.text, actor.label.label_type);
		let stereotype = actor.stereotype.as_deref().map(sanitize_text);
		let name = {
			let variant_name = if variant == ActorVariant::Normal {
				String::new()
			} else {
				format!("{} ", variant.name())
			};
			let business = if actor.business { "business " } else { "" };
			let stereotype = stereotype
				.as_ref()
				.map(|stereotype| format!(", stereotype {stereotype}"))
				.unwrap_or_default();
			let text = accessible_label(&sanitize_text(&actor.label.text), actor.label.label_type);
			format!("{business}{variant_name}actor {text}{stereotype}")
		};
		let mut classes = vec![
			"default".to_owned(),
			"usecase-actor".to_owned(),
			format!("usecase-actor-{}", variant.name()),
		];
		if actor.business {
			classes.push("usecase-business".to_owned());
		}
		classes.extend(actor.classes.iter().cloned());
		classes.push("usecase-actor-variant".to_owned());
		classes.push(format!("usecase-actor-{}", variant.name()));
		nodes.push(Node {
			parent_id: actor.parent.clone(),
			padding: 10.0,
			css_styles: actor.styles.clone(),
			css_compiled_styles: compiled_styles(model, &actor.classes),
			css_classes: classes.join(" "),
			usecase: Some(Box::new(UsecaseNode {
				font: Some(actor_font.clone()),
				label_color: theme_color(context, &["actorTextColor", "primaryTextColor"]),
				variant,
				icon: actor
					.icon
					.as_deref()
					.and_then(|icon| icons::registered_glyph(icon, Some("fa"))),
				business: actor.business,
				stereotype: stereotype.map(|stereotype| escape_plain(&stereotype)),
				color_slot: Some(color_index),
				attributes: attributes(&actor.id, "actor", name),
				..UsecaseNode::default()
			})),
			..base_node(&actor.id, label, is_markdown, shape)
		});
		color_index += 1;
	}
	for use_case in &model.use_cases {
		let is_ellipse = use_case.shape == UseCaseShape::Ellipse;
		let shape = match (is_ellipse, use_case.business) {
			(true, true) => "usecaseBusiness",
			(true, false) => "usecaseEllipse",
			(false, _) => "rect",
		};
		let stereotype = use_case.stereotype.as_deref().map(sanitize_text);
		let name = {
			let business = if use_case.business { "business " } else { "" };
			let stereotype_text = stereotype
				.as_ref()
				.map(|stereotype| format!(", stereotype {stereotype}"))
				.unwrap_or_default();
			let text =
				accessible_label(&sanitize_text(&use_case.label.text), use_case.label.label_type);
			format!("{business}use case {text}{stereotype_text}")
		};
		let mut label = node_label(&use_case.label.text, use_case.label.label_type);
		let mut is_markdown = use_case.label.label_type == LabelType::Markdown;
		let mut separate_stereotype =
			stereotype.as_ref().map(|stereotype| escape_plain(stereotype));
		if shape != "usecaseBusiness"
			&& let Some(stereotype) = separate_stereotype.take()
		{
			let label_text =
				if is_markdown { label.clone() } else { escape_markdown_markers(&label) };
			label = format!("«{}»<br/>{label_text}", escape_markdown_markers(&stereotype));
			is_markdown = true;
		}
		let mut classes = vec![
			"default".to_owned(),
			"usecase-element".to_owned(),
			format!("usecase-{}", if is_ellipse { "ellipse" } else { "rect" }),
		];
		if use_case.business {
			classes.push("usecase-business".to_owned());
		}
		classes.extend(use_case.classes.iter().cloned());
		if shape == "usecaseBusiness" {
			classes.push("usecase-business-shape".to_owned());
		}
		nodes.push(Node {
			parent_id: use_case.parent.clone(),
			padding: if is_ellipse { 20.0 } else { 10.0 },
			css_styles: use_case.styles.clone(),
			css_compiled_styles: compiled_styles(model, &use_case.classes),
			css_classes: classes.join(" "),
			min_width: Some(min_width),
			usecase: Some(Box::new(UsecaseNode {
				font: Some(usecase_font.clone()),
				label_color: primary_text.clone(),
				business: use_case.business,
				stereotype: separate_stereotype,
				color_slot: Some(color_index),
				attributes: attributes(&use_case.id, "usecase", name),
				..UsecaseNode::default()
			})),
			..base_node(&use_case.id, label, is_markdown, shape)
		});
		color_index += 1;
	}
	for note in &model.notes {
		let label = node_label(&note.label.text, note.label.label_type);
		let name = format!(
			"Note for {}: {}",
			accessible_of(&note.target),
			accessible_label(&sanitize_text(&note.label.text), note.label.label_type)
		);
		nodes.push(Node {
			padding: 10.0,
			css_compiled_styles: compiled_styles(model, &[]),
			css_classes: "default usecase-note".to_owned(),
			min_width: Some(min_width),
			usecase: Some(Box::new(UsecaseNode {
				label_color: theme_color(context, &["noteTextColor"]),
				attributes: attributes(&note.id, "note", name),
				..UsecaseNode::default()
			})),
			..base_node(&note.id, label, note.label.label_type == LabelType::Markdown, "note")
		});
	}
	for json_node in &model.jsons {
		let rows = json::flatten_rows(&json_node.value, &|cell| sanitize_text(cell));
		let label = escape_plain(&sanitize_text(&json_node.id));
		let summary: Vec<String> =
			rows.iter().map(|row| format!("{}: {}", row.accessible_key, row.value)).collect();
		let name = match summary.is_empty() {
			true => sanitize_text(&json_node.id),
			false => format!("{}: {}", sanitize_text(&json_node.id), summary.join("; ")),
		};
		let mut classes = vec!["default".to_owned(), "usecase-json-table".to_owned()];
		classes.extend(json_node.classes.iter().cloned());
		classes.push("usecase-json-table".to_owned());
		nodes.push(Node {
			padding: 10.0,
			css_styles: json_node.styles.clone(),
			css_compiled_styles: compiled_styles(model, &json_node.classes),
			css_classes: classes.join(" "),
			min_width: Some(min_width),
			usecase: Some(Box::new(UsecaseNode {
				label_color: primary_text.clone(),
				json_rows: rows
					.into_iter()
					.map(|row| (escape_plain(&row.key), escape_plain(&row.value)))
					.collect(),
				attributes: attributes(&json_node.id, "json", name),
				..UsecaseNode::default()
			})),
			..base_node(&json_node.id, label, false, "usecaseJsonTable")
		});
	}
	for (index, boundary) in model.boundaries.iter().enumerate() {
		let kind = boundary.boundary_type.name();
		let name = format!(
			"{kind} system boundary {}",
			accessible_label(&sanitize_text(&boundary.label.text), boundary.label.label_type)
		);
		let mut classes = vec![
			"default".to_owned(),
			"system-boundary".to_owned(),
			format!("system-boundary-{kind}"),
		];
		classes.extend(boundary.classes.iter().cloned());
		nodes.push(Node {
			padding: 20.0,
			css_styles: boundary.styles.clone(),
			css_compiled_styles: compiled_styles(model, &boundary.classes),
			css_classes: classes.join(" "),
			is_group: true,
			usecase: Some(Box::new(UsecaseNode {
				is_package: boundary.boundary_type == BoundaryType::Package,
				color_slot: Some(index),
				attributes: attributes(&boundary.id, "boundary", name),
				..UsecaseNode::default()
			})),
			..base_node(
				&boundary.id,
				node_label(&boundary.label.text, boundary.label.label_type),
				boundary.label.label_type == LabelType::Markdown,
				"usecaseSystemBoundary",
			)
		});
	}
	let stroke_width =
		format!("{}px", context.theme.text("strokeWidth").unwrap_or_else(|| "1".to_owned()));
	let line_color = theme_color(context, &["lineColor"]);
	let mut edges = Vec::new();
	for relationship in &model.relationships {
		let (start_arrow, end_arrow) = match relationship.relationship_type {
			RelationshipType::Include | RelationshipType::Extend => ("none", "arrow_point"),
			RelationshipType::Generalization => ("none", "extension"),
			RelationshipType::Association => match relationship.arrow_type {
				ArrowType::SolidArrow => ("none", "arrow_point"),
				ArrowType::BackArrow => ("arrow_point", "none"),
				ArrowType::CircleArrow => ("none", "arrow_circle"),
				ArrowType::CrossArrow => ("none", "arrow_cross"),
				ArrowType::CircleArrowReversed => ("arrow_circle", "none"),
				ArrowType::CrossArrowReversed => ("arrow_cross", "none"),
				ArrowType::LineSolid => ("none", "none"),
			},
		};
		let (label, label_type) = match relationship.relationship_type {
			RelationshipType::Generalization => (None, LabelType::Text),
			RelationshipType::Include | RelationshipType::Extend => {
				(Some(relationship.relationship_type.name().to_owned()), LabelType::Text)
			}
			RelationshipType::Association => match &relationship.label {
				Some(label) => (Some(label.text.clone()), label.label_type),
				None => (None, LabelType::Text),
			},
		};
		let accessible_relation = match (&label, relationship.relationship_type) {
			(Some(text), RelationshipType::Association) => {
				format!("association {}", accessible_label(&sanitize_text(text), label_type))
			}
			_ => relationship.relationship_type.name().to_owned(),
		};
		let name = format!(
			"{accessible_relation} from {} to {}",
			accessible_of(&relationship.source),
			accessible_of(&relationship.target)
		);
		let is_dotted = matches!(
			relationship.relationship_type,
			RelationshipType::Include | RelationshipType::Extend
		);
		let stroke = match relationship.relationship_type {
			RelationshipType::Include => theme_color(context, &["usecaseIncludeLine", "lineColor"]),
			RelationshipType::Extend => theme_color(context, &["usecaseExtendLine", "lineColor"]),
			_ => line_color.clone(),
		};
		let mut classes = vec![
			"default".to_owned(),
			"relationship".to_owned(),
			format!("relationship-{}", relationship.relationship_type.name()),
		];
		classes.extend(relationship.classes.iter().cloned());
		edges.push(Edge {
			id: relationship.id.clone(),
			start: relationship.source.clone(),
			end: relationship.target.clone(),
			label: label.map(|label| node_label(&label, label_type)).unwrap_or_default(),
			is_markdown: label_type == LabelType::Markdown,
			pattern: if is_dotted { Pattern::Dotted } else { Pattern::Solid },
			arrow_type_start: start_arrow.to_owned(),
			arrow_type_end: end_arrow.to_owned(),
			style: relationship.styles.clone(),
			css_compiled_styles: compiled_styles(model, &relationship.classes),
			classes: classes.join(" "),
			minlen: relationship.minlen,
			labelpos: "c",
			look,
			animate: relationship.animate,
			animation: relationship.animation.clone(),
			paint: Some((stroke, stroke_width.clone())),
			dasharray: is_dotted.then_some("3"),
			dom_id: Some(format!("usecase-{{}}-{}", dom_id(&relationship.id))),
			attributes: vec![
				("data-usecase-id", relationship.id.clone()),
				("data-usecase-kind", "relationship".to_owned()),
				("role", "img".to_owned()),
				("aria-label", name),
			],
			..Edge::default()
		});
	}
	for note in &model.notes {
		let id = format!("{}-edge", note.id);
		edges.push(Edge {
			id: id.clone(),
			start: note.id.clone(),
			end: note.target.clone(),
			pattern: Pattern::Dotted,
			arrow_type_start: "none".to_owned(),
			arrow_type_end: "none".to_owned(),
			css_compiled_styles: compiled_styles(model, &[]),
			classes: "default relationship relationship-note".to_owned(),
			minlen: 1,
			labelpos: "c",
			look,
			paint: Some((line_color.clone(), stroke_width.clone())),
			dasharray: Some("3"),
			dom_id: Some(format!("usecase-{{}}-{}", dom_id(&id))),
			attributes: vec![
				("data-usecase-id", id.clone()),
				("data-usecase-kind", "note-connector".to_owned()),
				("aria-hidden", "true".to_owned()),
			],
			..Edge::default()
		});
	}
	LayoutData {
		nodes,
		edges,
		direction: model.direction.to_owned(),
		node_spacing: context.config_number(SECTION, "nodeSpacing", 50.0),
		rank_spacing: context.config_number(SECTION, "rankSpacing", 50.0),
		diagram_type: "usecase",
		config_section: "flowchart",
		style: DiagramStyle::Usecase,
		markers: &["point", "circle", "cross", "extension"],
	}
}

/// The start of the code after its leading whitespace, like `trimStart`.
fn trimmed_start(code: &str) -> usize {
	code.len() - code.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}').len()
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	// `Diagram.fromText`: entities and a final line end.
	let code = encode_entities(code) + "\n";
	let lines = lexer::LineIndex::new(&code);
	let tokens = lexer::tokenize(&code, trimmed_start(&code), &lines).map_err(|error| {
		let character = code[error.offset..].chars().next().unwrap_or(' ');
		context.error_at(error.line, format!("Unexpected character '{character}'"))
	})?;
	let drafts = parser::parse(&code, &tokens)
		.map_err(|error| context.error_at(error.line, error.message))?;
	let model =
		model::build(&drafts).map_err(|error| context.error_at(error.line, error.message))?;
	let edge_count = model.relationships.len() + model.notes.len();
	if edge_count > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {edge_count} edges found, but the limit is {}.",
			context.limits.max_edges
		)));
	}
	let data = layout_data(&model, context);
	let (body, bbox) = rendering::render(context, &data)?;
	let mut content = svg::Svg::new();
	content.append(body);
	rendering::write_look_defs(&mut content, context);
	let padding = context.config_number(SECTION, "diagramPadding", 20.0);
	let (x, y) = if bbox.is_empty() { (0.0, 0.0) } else { (bbox.min.x, bbox.min.y) };
	let frame = Frame {
		x: x - padding,
		y: y - padding,
		width: bbox.width() + padding * 2.0,
		height: bbox.height() + padding * 2.0,
		use_max_width: context.config_bool(SECTION, "useMaxWidth", true),
	};
	let accessibility = Accessibility {
		title: model.acc_title.clone().unwrap_or_default(),
		description: model.acc_description.clone().unwrap_or_default(),
	};
	Ok(svg::document(context, "usecase", &frame, &accessibility, content))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mermaid::text::testing::FixedMeasure;

	fn parse(code: &str) -> std::result::Result<Model, (usize, String)> {
		let code = encode_entities(code) + "\n";
		let lines = lexer::LineIndex::new(&code);
		let tokens = lexer::tokenize(&code, trimmed_start(&code), &lines)
			.map_err(|error| (error.line, "lexer".to_owned()))?;
		let drafts = parser::parse(&code, &tokens).map_err(|error| (error.line, error.message))?;
		model::build(&drafts).map_err(|error| (error.line, error.message))
	}

	#[test]
	fn parses_the_documented_syntax() {
		let model = parse(
			"usecase-beta\ndirection LR\nactor Customer(\"Customer\")\nsystemBoundary \"Order system\"\n  Checkout(\"Place order\")\nend\nCustomer --> Checkout\n",
		)
		.unwrap();
		assert_eq!(model.actors.len(), 1);
		assert_eq!(model.use_cases[0].parent.as_deref(), Some("Order_system"));
		assert_eq!(model.relationships[0].arrow_type, ArrowType::SolidArrow);
		let model = parse(
			"usecase-beta\nactor Customer\nCheckout\nPayment\nCustomer opens@-- \"starts checkout\" ---> Checkout\nCheckout payment@..> : include Payment\nopens@{ animation: fast }\n",
		)
		.unwrap();
		assert_eq!(model.relationships[0].id, "opens");
		assert_eq!(model.relationships[0].minlen, 2);
		assert_eq!(model.relationships[0].animation.as_deref(), Some("fast"));
		assert_eq!(model.relationships[1].relationship_type, RelationshipType::Include);
		let model = parse("usecase-beta\nA -- files R&D report --> B\nB <-- x -- C\n").unwrap();
		assert_eq!(model.relationships[0].label.as_ref().unwrap().text, "files R&D report");
		assert_eq!(model.relationships[1].arrow_type, ArrowType::BackArrow);
		// Unquoted labels keep their inner spacing; `1mg` is an id.
		let model = parse("usecase-beta\n1mg[Costs 19.95 EUR — VAT]\n").unwrap();
		assert_eq!(model.use_cases[0].label.text, "Costs 19.95 EUR — VAT");
		assert_eq!(model.use_cases[0].shape, UseCaseShape::Rect);
	}

	#[test]
	fn rejects_like_mermaid() {
		let error = |code: &str| parse(code).unwrap_err();
		assert_eq!(error("usecase-beta\nA --> B C\n").0, 2);
		assert_eq!(error("usecase-beta\npackage Foo\n").0, 2);
		assert_eq!(
			error("usecase-beta\nA --> B\n\nnote for X \"t\"\n").1,
			"Note target 'X' is unresolved"
		);
		assert!(
			error("usecase-beta\nactor A\nA ..> : include B\n").1.contains("include relationship")
		);
		assert!(error("usecase-beta\nX(\"a\")\nX(\"b\")\n").1.contains("conflicting labels"));
		assert!(
			error("usecase-beta\nactor A@{ icon: x, type: hollow }\n").1.contains("cannot combine")
		);
		assert_eq!(
			error("usecase-beta\nA@{ business: true }\n").1,
			"Metadata target 'A' is unresolved"
		);
		// `encodeEntities` drops the last `;` of a style with a color; other `;` are errors.
		assert!(parse("usecase-beta\nclassDef x fill:#fff;stroke:#000\n").is_ok());
		assert_eq!(error("usecase-beta\nclassDef x fill:red;stroke:blue\n").0, 2);
		assert!(parse("usecase-beta\nA@{ business: true }\nA\n").is_ok());
		assert!(parse("usecase-beta\nsystemBoundary b\nA\n").is_err());
		assert!(
			parse(
				"usecase-beta\njson P@{ \"a\": [1, {\"b\": null}] }:::x\nclassDef x stroke:red\n"
			)
			.is_ok()
		);
	}

	#[test]
	fn renders_use_cases() {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let diagram = render(
			"usecase-beta\nactor A@{ business: true } <<Staff>>\nactor H@{ type: hollow }\nsystemBoundary s[\"System\"]@{ type: package }\n  U(\"Use\")@{ business: true } <<Core>>\n  R[Rect] <<Rec>>\nend\njson J@{ \"k\": [\"v\", 2] }\nnote for U \"a note\"\nA --> U\nH -- label --> R\nU ..> : extend R\nA --|> H\nR --> J\n",
			&mut context,
		)
		.unwrap();
		assert!(diagram.svg.contains("usecase-actor-stick"));
		assert!(diagram.svg.contains("boundary-tab"));
		assert!(diagram.svg.contains("usecase-json-key-cell"));
	}
}
