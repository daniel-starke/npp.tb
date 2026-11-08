//! Requirement diagrams (`requirementDiagram.jison`, `requirementDb.ts`,
//! `requirementRenderer.ts`), laid out with dagre.

mod db;
mod parser;

use super::common::CommonFields;
use super::label::encode_entities;
use super::rendering::{self, DiagramStyle, Edge, LayoutData, Node, Pattern, Point};
use super::svg::{self, Anchor, Baseline, Frame, TextRun};
use super::text::Font;
use super::{Context, Diagram, Error, Look, Result};
use db::RequirementDb;

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	let mut common = CommonFields::new(context);
	let mut db = RequirementDb::default();
	parser::parse(&code, &mut common, &mut db)
		.map_err(|error| context.error_at(error.line, error.message))?;
	if db.relations.len() > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} edges found, but the limit is {}.",
			db.relations.len(),
			context.limits.max_edges
		)));
	}
	// Mermaid fails with a TypeError on relationships of unknown names.
	let names = db.relations.iter().flat_map(|relation| [&relation.src, &relation.dst]);
	if let Some(unknown) = names.into_iter().find(|name| !db.knows(name)) {
		return Err(Error::new(format!(
			"The relationship refers to \"{unknown}\", which is no requirement or element."
		)));
	}
	let data = layout_data(&db, context);
	let (body, mut bbox) = rendering::render(context, &data)?;
	let mut content = svg::Svg::new();
	content.append(body);
	if let Some(title) = common.title.as_deref().filter(|title| !title.is_empty()) {
		// No rule styles `requirementDiagramTitleText`: the text has the font and fill of the
		// SVG. The renderer reads the state diagram's configuration.
		let font = Font::new(context.font_family(), context.font_size());
		let x = if bbox.is_empty() { 0.0 } else { bbox.min.x + bbox.width() / 2.0 };
		let y = -context.config_number("state", "titleTopMargin", 25.0);
		let run = TextRun {
			x,
			y,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: title,
		};
		content.text(run, |svg| {
			svg.attr("class", "requirementDiagramTitleText");
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
	Ok(svg::document(context, "requirement", &frame, &common.accessibility, content))
}

/// `getData`: the requirements and elements as `requirementBox` nodes, the relationships as
/// edges.
fn layout_data(db: &RequirementDb, context: &Context) -> LayoutData {
	let look = context.look;
	let nodes = db
		.requirements
		.iter()
		.chain(&db.elements)
		.enumerate()
		.map(|(index, item)| Node {
			id: item.name.clone(),
			dom_id: item.name.clone(),
			label: item.name.clone(),
			is_markdown: true,
			css_styles: item.css_styles.clone(),
			css_classes: item.classes.join(" "),
			shape: "requirementBox".to_owned(),
			look,
			color_index: Some(index),
			requirement: Some(rendering::RequirementBody {
				kind: item.requirement_type,
				fields: item.fields.values.clone(),
			}),
			..Node::default()
		})
		.collect();
	let edges = db
		.relations
		.iter()
		.enumerate()
		.map(|(index, relation)| {
			let is_contains = relation.kind == "contains";
			Edge {
				id: format!("{}-{}-{index}", relation.src, relation.dst),
				start: relation.src.clone(),
				end: relation.dst.clone(),
				label: format!("&lt;&lt;{}&gt;&gt;", relation.kind),
				is_markdown: true,
				pattern: if is_contains { Pattern::Solid } else { Pattern::Dashed },
				arrow_type_start: if is_contains { "requirement_contains" } else { "" }.to_owned(),
				arrow_type_end: if is_contains { "" } else { "requirement_arrow" }.to_owned(),
				style: vec![
					"fill:none".to_owned(),
					if is_contains { "" } else { "stroke-dasharray: 10,7" }.to_owned(),
				],
				classes: "relationshipLine".to_owned(),
				labelpos: "c",
				look,
				..Edge::default()
			}
		})
		.collect();
	LayoutData {
		nodes,
		edges,
		direction: db.direction.unwrap_or("TB").to_owned(),
		node_spacing: rendering::state_spacing(context, "nodeSpacing"),
		rank_spacing: rendering::state_spacing(context, "rankSpacing"),
		diagram_type: "requirement",
		// Edges take their curve from the flowchart configuration.
		config_section: "flowchart",
		style: DiagramStyle::Requirement,
		markers: match look {
			Look::Neo => &["requirement_contains_neo", "requirement_arrow_neo"],
			_ => &["requirement_contains", "requirement_arrow"],
		},
	}
}
