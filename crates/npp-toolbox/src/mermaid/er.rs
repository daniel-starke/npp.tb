//! Entity relationship diagrams (`erDiagram.jison`, `erDb.ts`, `erRenderer-unified.ts`), laid
//! out with dagre.

mod db;
mod parser;

use std::collections::HashMap;

use super::common::CommonFields;
use super::label::encode_entities;
use super::rendering::{self, DiagramStyle, Edge, LayoutData, Node, Pattern, Point};
use super::svg::{self, Anchor, Baseline, Frame, TextRun};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Error, Look, Result};
use db::ErDb;

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	let mut common = CommonFields::new(context);
	let mut db = ErDb::default();
	parser::parse(&code, &mut common, &mut db)
		.map_err(|error| context.error_at(error.line, error.message))?;
	if db.relationships.len() > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} edges found, but the limit is {}.",
			db.relationships.len(),
			context.limits.max_edges
		)));
	}
	let data = layout_data(&db, context);
	let (body, mut bbox) = rendering::render(context, &data)?;
	let mut content = svg::Svg::new();
	content.append(body);
	if let Some(title) = common.title.as_deref().filter(|title| !title.is_empty()) {
		// No rule styles `erDiagramTitleText`: the text has the font and fill of the SVG.
		let font = Font::new(context.font_family(), context.font_size());
		let x = if bbox.is_empty() { 0.0 } else { bbox.min.x + bbox.width() / 2.0 };
		let y = -context.config_number("er", "titleTopMargin", 25.0);
		let run = TextRun {
			x,
			y,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: title,
		};
		content.text(run, |svg| {
			svg.attr("class", "erDiagramTitleText");
		});
		bbox.include_centered_text(context, title, &font, Point { x, y });
	}
	rendering::write_look_defs(&mut content, context);
	const PADDING: f64 = 8.0;
	let (x, y) = if bbox.is_empty() { (0.0, 0.0) } else { (bbox.min.x, bbox.min.y) };
	let frame = Frame {
		x: x - PADDING,
		y: y - PADDING,
		width: bbox.width() + PADDING * 2.0,
		height: bbox.height() + PADDING * 2.0,
		use_max_width: context.config_bool("er", "useMaxWidth", true),
	};
	Ok(svg::document(context, "er", &frame, &common.accessibility, content))
}

/// The spacing of the layout: the top-level setting, else the diagram's, else `default`.
fn spacing(context: &Context, key: &str, default: f64) -> f64 {
	let positive = |value: Option<&Value>| value.and_then(Value::as_number).filter(|n| *n != 0.0);
	positive(context.config.get(key))
		.or_else(|| positive(context.config_value("er", key)))
		.unwrap_or(default)
}

/// `getData`: the subgraphs (last first) and entities as nodes, the relationships as edges.
fn layout_data(db: &ErDb, context: &Context) -> LayoutData {
	let look = context.look;
	let mut parents: HashMap<&str, &str> = HashMap::new();
	for subgraph in db.subgraphs.iter().rev() {
		for id in &subgraph.nodes {
			parents.insert(id, &subgraph.id);
		}
	}
	let parent_of = |id: &str| parents.get(id).map(|parent| (*parent).to_owned());
	let mut nodes = Vec::with_capacity(db.subgraphs.len() + db.entities.len());
	for subgraph in db.subgraphs.iter().rev() {
		nodes.push(Node {
			id: subgraph.id.clone(),
			dom_id: subgraph.id.clone(),
			label: subgraph.title.clone(),
			is_markdown: true,
			parent_id: parent_of(&subgraph.id),
			padding: 8.0,
			css_compiled_styles: db.compiled_styles(subgraph.classes.iter().map(String::as_str)),
			css_styles: subgraph.css_styles.clone(),
			css_classes: subgraph.classes.join(" "),
			shape: "rect".to_owned(),
			dir: subgraph.dir.clone(),
			is_group: true,
			look,
			..Node::default()
		});
	}
	let mut color_index = 0;
	for entity in &db.entities {
		if db.subgraphs.iter().any(|subgraph| subgraph.id == entity.label) {
			continue;
		}
		let label = if entity.alias.is_empty() { &entity.label } else { &entity.alias };
		nodes.push(Node {
			id: entity.id.clone(),
			dom_id: entity.id.clone(),
			label: label.clone(),
			is_markdown: true,
			parent_id: parent_of(&entity.label),
			css_compiled_styles: db.compiled_styles(entity.css_classes.split(' ')),
			css_styles: entity.css_styles.clone(),
			css_classes: entity.css_classes.clone(),
			shape: "erBox".to_owned(),
			look,
			color_index: Some(color_index),
			attributes: entity
				.attributes
				.iter()
				.map(|attribute| {
					[
						attribute.type_name.clone(),
						attribute.name.clone(),
						attribute.keys.join(","),
						attribute.comment.clone(),
					]
				})
				.collect(),
			..Node::default()
		});
		color_index += 1;
	}
	let edges = db
		.relationships
		.iter()
		.enumerate()
		.map(|(index, relationship)| Edge {
			id: format!("id_{}_{}_{index}", relationship.entity_a, relationship.entity_b),
			start: relationship.entity_a.clone(),
			end: relationship.entity_b.clone(),
			label: relationship.role.clone(),
			is_markdown: true,
			pattern: if relationship.spec.is_identifying {
				Pattern::Solid
			} else {
				Pattern::Dashed
			},
			arrow_type_start: relationship.spec.card_b.arrow_type().to_owned(),
			arrow_type_end: relationship.spec.card_a.arrow_type().to_owned(),
			classes: "relationshipLine".to_owned(),
			labelpos: "c",
			curve: Some("basis".to_owned()),
			look,
			..Edge::default()
		})
		.collect();
	LayoutData {
		nodes,
		edges,
		direction: db.direction.clone(),
		node_spacing: spacing(context, "nodeSpacing", 140.0),
		rank_spacing: spacing(context, "rankSpacing", 80.0),
		diagram_type: "er",
		config_section: "er",
		style: DiagramStyle::Er,
		markers: match look {
			Look::Neo => {
				&["only_one_neo", "zero_or_one_neo", "one_or_more_neo", "zero_or_more_neo"]
			}
			_ => &["only_one", "zero_or_one", "one_or_more", "zero_or_more"],
		},
	}
}
