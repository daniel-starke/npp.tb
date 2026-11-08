//! Class diagrams (`classDiagram.jison`, `classDb.ts`, `classRenderer-v3-unified.ts`), laid out
//! with dagre.

mod db;
mod parser;

use super::common::CommonFields;
use super::label::encode_entities;
use super::rendering::{self, ClassBody, ClassMember, DiagramStyle, Edge, LayoutData, Node};
use super::rendering::{Pattern, Point};
use super::svg::{self, Anchor, Baseline, Frame, TextRun};
use super::text::Font;
use super::{Context, Diagram, Error, Result};
use db::{ClassDb, Classifier, Member, RelationType};

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	let mut common = CommonFields::new(context);
	let mut db = ClassDb::default();
	parser::parse(&code, &mut common, &mut db)
		.map_err(|error| context.error_at(error.line, error.message))?;
	if db.relations.len() > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} edges found, but the limit is {}.",
			db.relations.len(),
			context.limits.max_edges
		)));
	}
	let data = layout_data(&db, context);
	let (body, mut bbox) = rendering::render(context, &data)?;
	let mut content = svg::Svg::new();
	content.append(body);
	if let Some(title) = common.title.as_deref().filter(|title| !title.is_empty()) {
		// No rule styles `classDiagramTitleText`: the text has the font and fill of the SVG.
		// The renderer reads the state diagram's configuration.
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
			svg.attr("class", "classDiagramTitleText");
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
	Ok(svg::document(context, "classDiagram", &frame, &common.accessibility, content))
}

fn class_member(member: &Member) -> ClassMember {
	ClassMember {
		text: member.text.clone(),
		is_italic: member.classifier == Classifier::Abstract,
		is_underlined: member.classifier == Classifier::Static,
	}
}

/// `getData`: namespaces as clusters, classes as `classBox` nodes, notes with their edges,
/// interfaces as invisible nodes, and the relations.
fn layout_data(db: &ClassDb, context: &Context) -> LayoutData {
	let look = context.look;
	let is_hierarchical = context.config_bool("class", "hierarchicalNamespaces", true);
	let padding = |default| context.config_number("class", "padding", default);
	let mut nodes = Vec::new();
	for namespace in &db.namespaces {
		if !is_hierarchical && !namespace.explicit {
			continue;
		}
		nodes.push(Node {
			id: namespace.id.clone(),
			dom_id: namespace.id.clone(),
			label: if is_hierarchical { namespace.label.clone() } else { namespace.id.clone() },
			is_group: true,
			padding: padding(16.0),
			shape: "rect".to_owned(),
			look,
			parent_id: namespace.parent.clone().filter(|_| is_hierarchical),
			..Node::default()
		});
	}
	let parent_of = |parent: &Option<String>| match is_hierarchical {
		true => parent.clone(),
		false => db.explicit_ancestor(parent.as_deref()),
	};
	for (index, class) in db.classes.iter().enumerate() {
		nodes.push(Node {
			id: class.id.clone(),
			dom_id: class.dom_id.clone(),
			label: class.text.clone(),
			is_markdown: true,
			parent_id: parent_of(&class.parent),
			css_styles: class.styles.clone(),
			css_classes: class.css_classes.clone(),
			look,
			link: class.link.clone(),
			link_target: class.link_target.clone(),
			tooltip: class.tooltip.clone(),
			shape: "classBox".to_owned(),
			color_index: Some(index),
			class_body: Some(ClassBody {
				annotation: class.annotations.first().cloned(),
				members: class.members.iter().map(class_member).collect(),
				methods: class.methods.iter().map(class_member).collect(),
			}),
			..Node::default()
		});
	}
	let mut edges = Vec::new();
	let note_bkg = context.theme.css("noteBkgColor");
	let note_border = context.theme.css("noteBorderColor");
	for (index, note) in db.notes.iter().enumerate() {
		nodes.push(Node {
			id: note.id.clone(),
			dom_id: note.id.clone(),
			label: note.text.clone(),
			is_markdown: true,
			shape: "note".to_owned(),
			padding: padding(6.0),
			css_styles: vec![
				"text-align: left".to_owned(),
				"white-space: nowrap".to_owned(),
				format!("fill: {note_bkg}"),
				format!("stroke: {note_border}"),
			],
			look,
			parent_id: parent_of(&note.parent),
			// Notes have no `cssClasses`; `getNodeClasses` writes them as `undefined`.
			css_classes: "undefined".to_owned(),
			..Node::default()
		});
		if let Some(class) = note.class.as_deref().and_then(|class| db.class(class)) {
			edges.push(Edge {
				id: format!("edgeNote{index}"),
				start: note.id.clone(),
				end: class.id.clone(),
				classes: "relation".to_owned(),
				arrow_type_start: "none".to_owned(),
				arrow_type_end: "none".to_owned(),
				style: vec!["fill: none".to_owned()],
				pattern: Pattern::Dotted,
				look,
				..Edge::default()
			});
		}
	}
	for interface in &db.interfaces {
		nodes.push(Node {
			id: interface.id.clone(),
			dom_id: interface.id.clone(),
			label: interface.label.clone(),
			shape: "rect".to_owned(),
			css_styles: vec!["opacity: 0;".to_owned()],
			look,
			..Node::default()
		});
	}
	for (index, relation) in db.relations.iter().enumerate() {
		edges.push(Edge {
			id: format!("id_{}_{}_{}", relation.id1, relation.id2, index + 1),
			start: relation.id1.clone(),
			end: relation.id2.clone(),
			label: relation.title.clone().unwrap_or_default(),
			is_markdown: true,
			labelpos: "c",
			classes: "relation".to_owned(),
			arrow_type_start: RelationType::arrow_marker(relation.type1).to_owned(),
			arrow_type_end: RelationType::arrow_marker(relation.type2).to_owned(),
			start_label_right: relation.title1.clone(),
			end_label_left: relation.title2.clone(),
			pattern: if relation.is_dotted { Pattern::Dashed } else { Pattern::Solid },
			look,
			..Edge::default()
		});
	}
	LayoutData {
		nodes,
		edges,
		direction: db.direction.unwrap_or("TB").to_owned(),
		node_spacing: rendering::state_spacing(context, "nodeSpacing"),
		rank_spacing: rendering::state_spacing(context, "rankSpacing"),
		diagram_type: "classDiagram",
		// Edges take their curve from the flowchart configuration.
		config_section: "flowchart",
		style: DiagramStyle::Class,
		markers: &["aggregation", "extension", "composition", "dependency", "lollipop"],
	}
}
