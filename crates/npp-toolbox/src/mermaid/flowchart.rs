//! Flowcharts (`flow.jison`, `flowDb.ts`, `flowRenderer-v3-unified.ts`), laid out with dagre.

pub(super) mod db;
pub(super) mod lexer;
pub(super) mod parser;

use std::collections::{HashMap, HashSet};

use super::common::CommonFields;
use super::icons;
use super::image;
use super::label::encode_entities;
use super::rendering::{
	self, BBox, Edge, LayoutData, Node, NodeIcon, NodeImage, Pattern, Point, Thickness,
};
use super::svg::{self, Anchor, Baseline, Frame, TextRun};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Look, Result};
use db::{FlowDb, LabelType, Stroke};

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	render_kind(code, context, false)
}

/// Swimlane diagrams: flowcharts whose default layout is `swimlane` (`swimlanesDiagram.ts`).
pub fn render_swimlanes(code: &str, context: &mut Context) -> Result<Diagram> {
	render_kind(code, context, true)
}

/// The layout of the diagram after `resolveAppearance`: the diagram's section, the general
/// setting, then the diagram's default. Only `swimlane` changes the layout, everything else is
/// laid out with dagre.
fn uses_swimlane_layout(context: &Context, is_swimlane_diagram: bool) -> bool {
	let section = if is_swimlane_diagram { "swimlane" } else { "flowchart" };
	let configured = [context.config.path(&[section, "layout"]), context.config.get("layout")]
		.into_iter()
		.flatten()
		.find_map(Value::as_str);
	match configured {
		Some(layout) => layout == "swimlane",
		None => is_swimlane_diagram,
	}
}

fn render_kind(code: &str, context: &mut Context, is_swimlane_diagram: bool) -> Result<Diagram> {
	// `flowParser.ts` and `Diagram.ts`: entities, whitespace after `}` and a final line end.
	let code = encode_entities(code).replace("}\r\n", "}\n") + "\n";
	let mut db = FlowDb::new(context.limits.max_edges);
	let mut common = CommonFields::new(context);
	parser::parse(&code, parser::Database::Flowchart(&mut db), &mut common)
		.map_err(|error| context.error_at(error.line, error.message))?;
	if context.config_bool("flowchart", "inheritDir", false) {
		let direction = db.direction.clone();
		for subgraph in &mut db.subgraphs {
			if subgraph.dir.is_none() {
				subgraph.dir = direction.clone();
			}
		}
	}
	let mut data = layout_data(&db, context);
	if is_swimlane_diagram {
		data.diagram_type = "swimlane";
	}
	let (body, bbox) = match uses_swimlane_layout(context, is_swimlane_diagram) {
		true => rendering::swimlane::render(context, &data, is_swimlane_diagram)?,
		false => rendering::render(context, &data)?,
	};
	let role = if is_swimlane_diagram { "swimlane" } else { "flowchart-v2" };
	Ok(document(context, body, bbox, &common, "flowchart", role))
}

/// The diagram around the rendered graph, for flowcharts and agentflows: the title, the look's
/// definitions and the frame, configured in `section`.
pub(super) fn document(
	context: &mut Context,
	body: svg::Svg,
	mut bbox: BBox,
	common: &CommonFields,
	section: &str,
	role: &str,
) -> Diagram {
	let mut content = svg::Svg::new();
	content.append(body);
	if let Some(title) = common.title.as_deref().filter(|title| !title.is_empty()) {
		let font = Font::new(context.font_family(), 18.0);
		let x = if bbox.is_empty() { 0.0 } else { bbox.min.x + bbox.width() / 2.0 };
		let y = -context.config_number(section, "titleTopMargin", 25.0);
		let fill = context.theme.css("textColor");
		let run = TextRun {
			x,
			y,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: title,
		};
		let class = format!("{section}TitleText");
		content.text(run, |svg| {
			svg.attr("class", &class).attr_if("fill", &fill);
		});
		bbox.include_centered_text(context, title, &font, Point { x, y });
	}
	rendering::write_look_defs(&mut content, context);
	let padding = context.config_number(section, "diagramPadding", 8.0);
	let (x, y) = if bbox.is_empty() { (0.0, 0.0) } else { (bbox.min.x, bbox.min.y) };
	let frame = Frame {
		x: x - padding,
		y: y - padding,
		width: bbox.width() + padding * 2.0,
		height: bbox.height() + padding * 2.0,
		use_max_width: context.config_bool(section, "useMaxWidth", true),
	};
	svg::document(context, role, &frame, &common.accessibility, content)
}

/// The colour slots of the subgraphs in source order: a pre-order walk of the containment
/// forest given by `parent_of`.
pub(super) fn declaration_order<'a>(
	subgraphs: &'a [db::SubGraph],
	parent_of: &HashMap<&'a str, &'a str>,
) -> HashMap<&'a str, usize> {
	let mut children_of: HashMap<&str, Vec<&str>> = HashMap::new();
	for subgraph in subgraphs {
		if let Some(parent) = parent_of.get(subgraph.id.as_str()) {
			children_of.entry(parent).or_default().push(&subgraph.id);
		}
	}
	let mut order: HashMap<&str, usize> = HashMap::new();
	let mut stack: Vec<&str> = subgraphs
		.iter()
		.rev()
		.filter(|subgraph| !parent_of.contains_key(subgraph.id.as_str()))
		.map(|subgraph| subgraph.id.as_str())
		.collect();
	while let Some(id) = stack.pop() {
		if order.contains_key(id) {
			continue;
		}
		let next = order.len();
		order.insert(id, next);
		if let Some(children) = children_of.get(id) {
			stack.extend(children.iter().rev());
		}
	}
	order
}

/// A vertex with the id of a subgraph styles the subgraph's node. False if there is none.
pub(super) fn style_subgraph_node(db: &FlowDb, nodes: &mut [Node], vertex: &db::Vertex) -> bool {
	let Some(node) = nodes.iter_mut().find(|node| node.id == vertex.id) else { return false };
	node.css_styles = vertex.styles.clone();
	node.css_compiled_styles = db.compiled_styles(&vertex.classes);
	node.css_classes = vertex.classes.join(" ");
	true
}

/// The layout edge of `raw` from `start` to `end`, with the arrows of `destructEdgeType`.
pub(super) fn layout_edge(
	db: &FlowDb,
	raw: &db::Edge,
	(start, end): (&str, &str),
	(start_arrow, end_arrow): (String, String),
	default_curve: Option<&String>,
	look: Look,
) -> Edge {
	let mut styles = db.default_style.clone().unwrap_or_default();
	if let Some(style) = &raw.style {
		styles.extend(style.iter().cloned());
	}
	let is_invisible = raw.stroke == Stroke::Invisible;
	let has_no_arrows = is_invisible || raw.kind.as_deref() == Some("arrow_open");
	Edge {
		id: raw.id.clone(),
		start: start.to_owned(),
		end: end.to_owned(),
		label: raw.text.clone(),
		is_markdown: raw.label_type == LabelType::Markdown,
		thickness: match raw.stroke {
			Stroke::Thick => Thickness::Thick,
			Stroke::Invisible => Thickness::Invisible,
			_ => Thickness::Normal,
		},
		pattern: if raw.stroke == Stroke::Dotted { Pattern::Dotted } else { Pattern::Solid },
		arrow_type_start: if has_no_arrows { "none".to_owned() } else { start_arrow },
		arrow_type_end: if has_no_arrows { "none".to_owned() } else { end_arrow },
		style: styles,
		css_compiled_styles: db.compiled_styles(&raw.classes),
		classes: if is_invisible {
			String::new()
		} else {
			"edge-thickness-normal edge-pattern-solid flowchart-link".to_owned()
		},
		minlen: raw.length,
		labelpos: "c",
		curve: raw
			.interpolate
			.clone()
			.or_else(|| db.default_interpolate.clone())
			.or_else(|| default_curve.cloned()),
		look,
		animate: raw.animate == Some(true),
		animation: raw.animation.clone(),
		..Edge::default()
	}
}

/// `getTypeFromVertex`.
fn shape_of(vertex: &db::Vertex) -> String {
	if vertex.img.is_some() {
		return "imageSquare".to_owned();
	}
	if vertex.icon.is_some() {
		return match vertex.form.as_deref() {
			Some("circle") => "iconCircle",
			Some("square") => "iconSquare",
			Some("rounded") => "iconRounded",
			_ => "icon",
		}
		.to_owned();
	}
	match vertex.kind.as_deref() {
		None | Some("square") => "squareRect".to_owned(),
		Some("round") => "roundedRect".to_owned(),
		Some(other) => other.to_owned(),
	}
}

/// The image of `imageSquare` and its size. Images other than `data:` URIs are assumed square,
/// as their natural size is unknown without loading them.
fn node_image(
	vertex: &db::Vertex,
	wrapping_width: f64,
	images: &dyn image::ImageDecoder,
) -> Option<NodeImage> {
	const UNKNOWN_SIZE: (f64, f64) = (100.0, 100.0);
	let href = vertex.img.clone()?;
	let (natural_width, natural_height) =
		image::natural_size(&href, images).unwrap_or(UNKNOWN_SIZE);
	let aspect_ratio = natural_width / natural_height;
	let has_label = vertex.text.as_deref().is_some_and(|text| !text.is_empty());
	let raw_width = vertex.asset_width.unwrap_or(natural_width).max(if has_label {
		wrapping_width
	} else {
		0.0
	});
	let is_constrained = vertex.constraint.as_deref() == Some("on");
	let width = match (is_constrained, vertex.asset_height) {
		(true, Some(height)) => height * aspect_ratio,
		_ => raw_width,
	};
	let height = match is_constrained {
		true => width / aspect_ratio,
		false => vertex.asset_height.unwrap_or(natural_height),
	};
	Some(NodeImage { href, width, height, is_label_on_top: vertex.pos.as_deref() == Some("t") })
}

/// The icon of the `icon` shapes: `getIconSVG` wants its pack in the name.
fn node_icon(vertex: &db::Vertex) -> Option<NodeIcon> {
	const DEFAULT_SIZE: f64 = 48.0;
	if vertex.img.is_some() {
		return None;
	}
	let reference = vertex.icon.as_deref()?;
	Some(NodeIcon {
		glyph: icons::registered_glyph(reference, Some("")),
		size: f64::max(
			vertex.asset_height.unwrap_or(DEFAULT_SIZE),
			vertex.asset_width.unwrap_or(DEFAULT_SIZE),
		),
		is_label_on_top: vertex.pos.as_deref() == Some("t"),
	})
}

/// `destructEdgeType`.
fn arrow_types(kind: Option<&str>) -> (String, String) {
	match kind {
		Some(kind @ ("arrow_point" | "arrow_circle" | "arrow_cross")) => {
			("none".to_owned(), kind.to_owned())
		}
		Some(kind @ ("double_arrow_point" | "double_arrow_circle" | "double_arrow_cross")) => {
			let arrow = kind.replacen("double_", "", 1);
			(arrow.clone(), arrow)
		}
		_ => ("none".to_owned(), "arrow_point".to_owned()),
	}
}

/// `getData`: the nodes, clusters and edges of the database for the layout.
fn layout_data(db: &FlowDb, context: &Context) -> LayoutData {
	let look = context.look;
	let padding = context.config_number("flowchart", "padding", 15.0);
	let min_width = context.config_number("flowchart", "minNodeWidth", 120.0);
	let wrapping_width = context.config_number("flowchart", "wrappingWidth", 120.0);
	let subgraphs = &db.subgraphs;
	let is_subgraph = |id: &str| db.is_subgraph(id);
	let mut subgraph_parent: HashMap<&str, &str> = HashMap::new();
	for subgraph in subgraphs {
		for child in &subgraph.nodes {
			if is_subgraph(child) {
				subgraph_parent.insert(child, &subgraph.id);
			}
		}
	}
	let declaration_index = declaration_order(subgraphs, &subgraph_parent);
	// `@{ view: collapsed }` draws a subgraph as one node. Its members are hidden, and edges to
	// them end at the outermost collapsed subgraph around them.
	let collapsed: HashSet<&str> = subgraphs
		.iter()
		.filter(|subgraph| {
			let view = subgraph.metadata.as_ref().and_then(|metadata| metadata.get("view"));
			view.and_then(Value::as_str) == Some("collapsed")
		})
		.map(|subgraph| subgraph.id.as_str())
		.collect();
	let outermost_collapsed = |id: &str| -> Option<&str> {
		let mut result = None;
		let mut seen = HashSet::new();
		let mut current = Some(id);
		while let Some(id) = current.filter(|id| seen.insert(*id)) {
			if let Some(&id) = collapsed.get(id) {
				result = Some(id);
			}
			current = subgraph_parent.get(id).copied();
		}
		result
	};
	let mut collapsed_ancestor: HashMap<&str, &str> = HashMap::new();
	for subgraph in subgraphs {
		let Some(ancestor) = outermost_collapsed(&subgraph.id) else { continue };
		if subgraph.id != ancestor {
			collapsed_ancestor.insert(&subgraph.id, ancestor);
		}
		for child in subgraph.nodes.iter().filter(|child| *child != ancestor) {
			collapsed_ancestor.insert(child, ancestor);
		}
	}
	let is_hidden = |id: &str| collapsed_ancestor.contains_key(id);
	let mut parent_of: HashMap<&str, &str> = HashMap::new();
	let mut is_group: HashSet<&str> = HashSet::new();
	for subgraph in subgraphs.iter().rev().filter(|subgraph| !is_hidden(&subgraph.id)) {
		if !subgraph.nodes.is_empty() {
			is_group.insert(&subgraph.id);
		}
		for id in &subgraph.nodes {
			parent_of.insert(id, &subgraph.id);
		}
	}
	let mut nodes: Vec<Node> = Vec::new();
	for subgraph in subgraphs.iter().rev().filter(|subgraph| !is_hidden(&subgraph.id)) {
		let is_collapsed = collapsed.contains(subgraph.id.as_str());
		nodes.push(Node {
			id: subgraph.id.clone(),
			dom_id: subgraph.id.clone(),
			label: subgraph.title.clone(),
			is_markdown: subgraph.label_type == LabelType::Markdown,
			parent_id: parent_of.get(subgraph.id.as_str()).map(|parent| (*parent).to_owned()),
			padding: 8.0,
			css_compiled_styles: db.compiled_styles(&subgraph.classes),
			css_classes: subgraph.classes.join(" "),
			dir: subgraph.dir.clone(),
			look,
			is_group: !is_collapsed,
			shape: if is_collapsed { "collapsedGroup" } else { "rect" }.to_owned(),
			color_index: declaration_index.get(subgraph.id.as_str()).copied(),
			..Node::default()
		});
	}
	for vertex in db.vertices.iter().filter(|vertex| !is_hidden(&vertex.id)) {
		if style_subgraph_node(db, &mut nodes, vertex) {
			continue;
		}
		let mut classes = vec!["default", "node"];
		classes.extend(vertex.classes.iter().map(String::as_str));
		let group = is_group.contains(vertex.id.as_str());
		nodes.push(Node {
			id: vertex.id.clone(),
			dom_id: vertex.dom_id.clone(),
			label: vertex.text.clone().unwrap_or_default(),
			is_markdown: vertex.label_type == LabelType::Markdown,
			parent_id: parent_of.get(vertex.id.as_str()).map(|parent| (*parent).to_owned()),
			padding,
			css_styles: vertex.styles.clone(),
			css_compiled_styles: db.compiled_styles(&classes),
			css_classes: format!("default {}", vertex.classes.join(" ")),
			dir: vertex.dir.clone(),
			look,
			link: vertex.link.clone(),
			link_target: vertex.link_target.clone(),
			tooltip: db.tooltip(&vertex.id).map(str::to_owned),
			is_group: group,
			shape: if group { "rect".to_owned() } else { shape_of(vertex) },
			min_width: (!group).then_some(min_width),
			image: if group { None } else { node_image(vertex, wrapping_width, context.images) },
			icon: if group { None } else { node_icon(vertex) },
			..Node::default()
		});
	}
	let default_curve = context.config_text("flowchart", "curve");
	let edges = db
		.edges
		.iter()
		.filter_map(|raw| {
			let start = collapsed_ancestor.get(raw.start.as_str()).copied().unwrap_or(&raw.start);
			let end = collapsed_ancestor.get(raw.end.as_str()).copied().unwrap_or(&raw.end);
			// Edges inside a collapsed subgraph disappear; other self loops stay.
			if start == end && (is_hidden(&raw.start) || is_hidden(&raw.end)) {
				return None;
			}
			let arrows = arrow_types(raw.kind.as_deref());
			Some(layout_edge(db, raw, (start, end), arrows, default_curve.as_ref(), look))
		})
		.collect();
	LayoutData {
		nodes,
		edges,
		direction: db.direction.clone().unwrap_or_else(|| "TB".to_owned()),
		node_spacing: context.config_number("flowchart", "nodeSpacing", 50.0),
		rank_spacing: context.config_number("flowchart", "rankSpacing", 50.0),
		diagram_type: "flowchart-v2",
		config_section: "flowchart",
		style: rendering::DiagramStyle::Flowchart,
		markers: &["point", "circle", "cross"],
	}
}
