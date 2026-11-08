//! State diagrams (`stateDiagram.jison`, `stateDb.ts`, `dataFetcher.ts`,
//! `stateRenderer-v3-unified.ts`), laid out with dagre.

mod db;
mod parser;

use super::common::CommonFields;
use super::label::encode_entities;
use super::rendering::{self, DiagramStyle, Edge, LayoutData, Node, Pattern, Point};
use super::svg::{self, Anchor, Baseline, Frame, TextRun};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Error, Look, Result};
use db::{NodeData, StateDb};

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	let mut common = CommonFields::new(context);
	let root = parser::parse(&code, &mut common)
		.map_err(|error| context.error_at(error.line, error.message))?;
	let db = StateDb::new(root, context.look).map_err(Error::new)?;
	if db.edges.len() > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} edges found, but the limit is {}.",
			db.edges.len(),
			context.limits.max_edges
		)));
	}
	let data = layout_data(&db, context)?;
	let (body, mut bbox) = rendering::render(context, &data)?;
	let mut content = svg::Svg::new();
	content.append(body);
	if let Some(title) = common.title.as_deref().filter(|title| !title.is_empty()) {
		let font = Font::new(context.font_family(), 18.0);
		let x = if bbox.is_empty() { 0.0 } else { bbox.min.x + bbox.width() / 2.0 };
		let y = -context.config_number("state", "titleTopMargin", 25.0);
		let fill = context.theme.css("textColor");
		let run = TextRun {
			x,
			y,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: title,
		};
		content.text(run, |svg| {
			svg.attr("class", "statediagramTitleText").attr_if("fill", &fill);
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
		use_max_width: context.config_bool("state", "useMaxWidth", true),
	};
	Ok(svg::document(context, "stateDiagram", &frame, &common.accessibility, content))
}

/// The registry shape of a node, or Mermaid's error for shapes it does not know.
fn shape_of(node: &NodeData) -> Result<String> {
	let shape = match node.shape.as_deref() {
		None => return Err(Error::new("No such shape: undefined. Please check your syntax.")),
		Some(db::SHAPE_DIVIDER) if !node.is_group => {
			return Err(Error::new("No such shape: divider. Please check your syntax."));
		}
		Some(db::SHAPE_STATE) if node.is_rounded => "roundedRect",
		Some(db::SHAPE_STATE) => "squareRect",
		Some(shape) => shape,
	};
	Ok(shape.to_owned())
}

/// A positive number of the configuration: the diagram's setting, else `default`.
fn spacing(context: &Context, key: &str) -> f64 {
	let positive = |value: Option<&Value>| value.and_then(Value::as_number).filter(|n| *n > 0.0);
	positive(context.config.get(key))
		.or_else(|| positive(context.config_value("state", key)))
		.unwrap_or(50.0)
}

/// `getData`: the nodes and edges of the database for the layout.
fn layout_data(db: &StateDb, context: &Context) -> Result<LayoutData> {
	let look = context.look;
	let wrapping_width = context.config_number("state", "wrappingWidth", 120.0);
	let min_width = context.config_number("state", "minNodeWidth", 120.0);
	let note_padding = context.config_number("flowchart", "padding", 15.0);
	let mut nodes = Vec::with_capacity(db.nodes.len());
	for node in &db.nodes {
		let shape = shape_of(node)?;
		let link = db.links.iter().find(|(id, _, _)| *id == node.id);
		let unquote = |text: &str| text.trim_matches('"').to_owned();
		nodes.push(Node {
			id: node.id.clone(),
			dom_id: node.dom_id.clone(),
			label: node.label.clone(),
			is_markdown: true,
			parent_id: node.parent_id.clone(),
			padding: node.padding.unwrap_or(note_padding),
			css_styles: node.css_styles.clone(),
			css_compiled_styles: node.css_compiled_styles.clone(),
			css_classes: node.css_classes.clone(),
			dir: node.dir.clone(),
			look,
			link: link.map(|(_, url, _)| unquote(url)),
			link_target: link.map(|_| "_blank".to_owned()),
			tooltip: link
				.map(|(_, _, tooltip)| unquote(tooltip))
				.filter(|tooltip| !tooltip.is_empty()),
			is_group: node.is_group,
			min_width: (!node.is_group).then_some(min_width),
			color_index: node.color_index,
			description: node.description.clone(),
			wrapping_width: Some(wrapping_width),
			// `.statediagram-state rect.basic { rx: 5px }`, which misses the paths of roughjs.
			// Composite states are rounded (`rx: 10`).
			corner_radius: (look != Look::HandDrawn || node.is_group).then_some(5.0),
			shape,
			..Node::default()
		});
	}
	let arrow = if look == Look::Neo { "arrow_barb_neo" } else { "arrow_barb" };
	let edges = db
		.edges
		.iter()
		.map(|edge| Edge {
			id: edge.id.clone(),
			start: edge.start.clone(),
			end: edge.end.clone(),
			label: edge.label.clone(),
			is_markdown: true,
			pattern: if edge.is_note_edge { Pattern::Dashed } else { Pattern::Solid },
			arrow_type_start: String::new(),
			arrow_type_end: if edge.is_note_edge { String::new() } else { arrow.to_owned() },
			style: vec!["fill:none".to_owned()],
			classes: match edge.is_note_edge {
				true => "transition note-edge".to_owned(),
				false => "transition".to_owned(),
			},
			labelpos: "c",
			look,
			..Edge::default()
		})
		.collect();
	Ok(LayoutData {
		nodes,
		edges,
		direction: db.direction.clone(),
		node_spacing: spacing(context, "nodeSpacing"),
		rank_spacing: spacing(context, "rankSpacing"),
		diagram_type: "stateDiagram",
		config_section: "state",
		style: DiagramStyle::State,
		markers: if look == Look::Neo { &["barbNeo"] } else { &["barb"] },
	})
}
