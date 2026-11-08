//! Mindmaps (`mindmap.jison`, `mindmapDb.ts`, `mindmapRenderer.ts`, `styles.ts`, the shapes
//! `defaultMindmapNode` and `mindmapCircle`, and the cose-bilkent renderer of
//! `rendering-util/layout-algorithms/cose-bilkent`).
//!
//! Mermaid lays mindmaps out with cose-bilkent unless the document names another registered
//! layout; here they always use cose-bilkent.

use super::common::sanitize_text;
use super::kanban::{Token, lex};
use super::label::LabelBox;
use super::layout::cose_bilkent;
use super::rendering::edges::{self, EdgeEnd, EdgeRoute};
use super::rendering::intersect::Bounds;
use super::rendering::markers::{MarkerColors, Markers};
use super::rendering::shapes::{self, Shape};
use super::rendering::sketch;
use super::rendering::{
	self, BBox, DiagramStyle, Edge, Node, Point, Styles, Theme, translate, write_label,
	write_look_defs,
};
use super::svg::{Frame, Svg, document, number};
use super::value::Value;
use super::{Context, Diagram, Error, Look, Result};

/// `nodeType`: `NO_BORDER` shares the value of `DEFAULT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeType {
	Default,
	RoundedRect,
	Rect,
	Circle,
	Cloud,
	Bang,
	Hexagon,
}

impl NodeType {
	/// `getType` of the start and end of a node.
	fn of(start: &str, end: &str) -> NodeType {
		match start {
			"[" => NodeType::Rect,
			"(" if end == ")" => NodeType::RoundedRect,
			"(" | ")" => NodeType::Cloud,
			"((" => NodeType::Circle,
			"))" => NodeType::Bang,
			"{{" => NodeType::Hexagon,
			_ => NodeType::Default,
		}
	}
}

#[derive(Clone, Debug)]
struct MindmapNode {
	level: i64,
	description: String,
	kind: NodeType,
	children: Vec<usize>,
	padding: f64,
	class: Option<String>,
	section: Option<usize>,
}

/// `MindmapDB`: the nodes in the order they were added, the root first.
struct MindmapDb {
	nodes: Vec<MindmapNode>,
	base_level: i64,
	padding: f64,
}

impl MindmapDb {
	/// `addNode`.
	fn add_node(
		&mut self,
		level: usize,
		description: &str,
		kind: NodeType,
	) -> std::result::Result<(), String> {
		let mut level = level as i64;
		let is_root = self.nodes.is_empty();
		if is_root {
			self.base_level = level;
			level = 0;
		} else {
			level -= self.base_level;
		}
		let padding = match kind {
			NodeType::RoundedRect | NodeType::Rect | NodeType::Hexagon => self.padding * 2.0,
			_ => self.padding,
		};
		let node = MindmapNode {
			level,
			description: sanitize_text(description),
			kind,
			children: Vec::new(),
			padding,
			class: None,
			section: None,
		};
		let index = self.nodes.len();
		match self.nodes.iter().rposition(|candidate| candidate.level < level) {
			Some(parent) => self.nodes[parent].children.push(index),
			None if is_root => {}
			None => {
				return Err(format!(
					"There can be only one root. No parent could be found for (\"{}\")",
					node.description
				));
			}
		}
		self.nodes.push(node);
		Ok(())
	}

	/// `decorateNode` of the last node.
	fn decorate(&mut self, class: Option<&str>, setting: &str) -> std::result::Result<(), String> {
		let Some(node) = self.nodes.last_mut() else {
			return Err(format!("Cannot set properties of undefined (setting '{setting}')"));
		};
		if let Some(class) = class {
			node.class = Some(sanitize_text(class));
		}
		Ok(())
	}

	/// `assignSections`: the children of the root count the sections, their descendants inherit
	/// them; by depth with `colorBy: depth`.
	fn assign_sections(&mut self, by_depth: bool) {
		const SECTIONS: usize = 12 - 1;
		let mut stack: Vec<(usize, Option<usize>, usize)> = vec![(0, None, 0)];
		while let Some((index, section, depth)) = stack.pop() {
			let is_root = self.nodes[index].level == 0;
			self.nodes[index].section = match is_root {
				true => None,
				false if by_depth => Some((depth - 1) % SECTIONS),
				false => section,
			};
			for (position, &child) in self.nodes[index].children.iter().enumerate().rev() {
				let child_section = if is_root { Some(position % SECTIONS) } else { section };
				stack.push((child, child_section, depth + 1));
			}
		}
	}

	/// The nodes in the order of `flattenNodes`, and the edges of `generateEdges` as parent and
	/// child: both depth first.
	fn preorder(&self) -> (Vec<usize>, Vec<(usize, usize)>) {
		let mut order = Vec::with_capacity(self.nodes.len());
		let mut edges = Vec::with_capacity(self.nodes.len().saturating_sub(1));
		let mut stack: Vec<(usize, Option<usize>)> = vec![(0, None)];
		while let Some((index, parent)) = stack.pop() {
			order.push(index);
			// `generateEdges` writes the edge to a child right before the edges below it.
			if let Some(parent) = parent {
				edges.push((parent, index));
			}
			stack
				.extend(self.nodes[index].children.iter().rev().map(|&child| (child, Some(index))));
		}
		(order, edges)
	}
}

/// `mindmap.jison` on the tokens of the shared lexer.
fn parse(code: &str, context: &Context) -> Result<MindmapDb> {
	let tokens = lex(code, context, "mindmap")?;
	let mut index = 0;
	let token = |index: usize| &tokens[index.min(tokens.len() - 1)].0;
	let line_of = |index: usize| tokens[index.min(tokens.len() - 1)].1;
	let unexpected = |index: usize, expected: &str| {
		let found = match token(index) {
			Token::End => "the end of the diagram".to_owned(),
			Token::NewLine | Token::SpaceLine => "the end of the line".to_owned(),
			Token::SpaceList(_) => "indentation".to_owned(),
			Token::NodeId(text) | Token::NodeDescription(text) => format!("{text:?}"),
			other => format!("{other:?}"),
		};
		context.error_at(line_of(index), format!("expected {expected}, found {found}"))
	};
	while matches!(token(index), Token::SpaceLine | Token::NewLine) {
		index += 1;
	}
	if *token(index) != Token::Kanban {
		return Err(unexpected(index, "`mindmap`"));
	}
	index += 1;
	if *token(index) == Token::NewLine {
		index += 1;
	}
	let padding = context.config_number("mindmap", "padding", 10.0);
	let mut db = MindmapDb {
		nodes: Vec::new(),
		base_level: 0,
		padding: if padding.is_finite() { padding.clamp(0.0, 10_000.0) } else { 10.0 },
	};
	// Mermaid draws nothing for a mindmap without nodes.
	if *token(index) == Token::End {
		return Ok(db);
	}
	loop {
		let line = line_of(index);
		let level = match token(index) {
			Token::SpaceList(level) => {
				index += 1;
				Some(*level)
			}
			_ => None,
		};
		match token(index).clone() {
			Token::SpaceLine if level.is_none() => index += 1,
			Token::Icon(_) => {
				db.decorate(None, "icon").map_err(|message| context.error_at(line, message))?;
				index += 1;
			}
			Token::Class(class) => {
				db.decorate(Some(&class), "class")
					.map_err(|message| context.error_at(line, message))?;
				index += 1;
			}
			Token::NodeId(_) | Token::NodeStart(_) => {
				let (description, kind) = match token(index).clone() {
					Token::NodeId(id) => match token(index + 1).clone() {
						Token::NodeStart(start) => {
							let Token::NodeDescription(description) = token(index + 2).clone()
							else {
								return Err(unexpected(index + 2, "a node description"));
							};
							let Token::NodeEnd(end) = token(index + 3).clone() else {
								return Err(unexpected(index + 3, "the end of the node"));
							};
							index += 4;
							(description, NodeType::of(&start, &end))
						}
						_ => {
							index += 1;
							(id, NodeType::Default)
						}
					},
					Token::NodeStart(start) => {
						let Token::NodeDescription(description) = token(index + 1).clone() else {
							return Err(unexpected(index + 1, "a node description"));
						};
						let Token::NodeEnd(end) = token(index + 2).clone() else {
							return Err(unexpected(index + 2, "the end of the node"));
						};
						index += 3;
						(description, NodeType::of(&start, &end))
					}
					_ => unreachable!(),
				};
				db.add_node(level.unwrap_or(0), &description, kind)
					.map_err(|message| context.error_at(line, message))?;
			}
			_ if level.is_some() => {}
			_ => return Err(unexpected(index, "a node")),
		}
		match token(index) {
			Token::NewLine | Token::SpaceLine => index += 1,
			Token::End => break,
			_ => return Err(unexpected(index, "the end of the line")),
		}
		while *token(index) == Token::NewLine {
			index += 1;
		}
		if *token(index) == Token::End {
			break;
		}
	}
	Ok(db)
}

/// The paint of a node and its edge from the rules of `styles.ts`.
struct SectionPaint {
	fill: String,
	/// The stroke of neo shapes.
	stroke: String,
	label: String,
	line_stroke: String,
	line_width: &'static str,
}

struct Paint<'a, 'b> {
	context: &'a Context<'b>,
	color_limit: usize,
	is_neo: bool,
	uses_gradient: bool,
	stroke_width: String,
	filter: String,
}

impl Paint<'_, '_> {
	fn color(&self, key: &str) -> String {
		self.context.theme.css(key)
	}

	/// The rules of `.section-{section}`, `None` for the root (`section-root section--1`).
	fn node(&self, section: Option<usize>) -> SectionPaint {
		let theme = self.context.theme_name.as_str();
		let is_redux = theme == "redux" || theme == "redux-dark";
		let index = section.map_or(0, |section| section + 1);
		let has_rules = index < self.color_limit;
		let rule = |key: &str| {
			if has_rules { self.color(&format!("{key}{index}")) } else { String::new() }
		};
		let label_index = if theme == "neutral" { 1 } else { index };
		let mut paint = match (section, self.is_neo) {
			(None, false) => SectionPaint {
				fill: self.color("git0"),
				stroke: String::new(),
				label: match theme.contains("redux") {
					true => self.color("nodeBorder"),
					false => self.color("gitBranchLabel0"),
				},
				line_stroke: rule("cScaleInv"),
				line_width: "3",
			},
			(Some(_), false) => SectionPaint {
				fill: rule("cScale"),
				stroke: String::new(),
				label: rule("cScaleLabel"),
				line_stroke: rule("cScaleInv"),
				line_width: "3",
			},
			(_, true) => SectionPaint {
				fill: match (section, is_redux || theme == "neutral") {
					(None, _) if theme.contains("redux") => self.color("mainBkg"),
					(None, _) => self.color("git0"),
					(Some(_), true) => self.color("mainBkg"),
					(Some(_), false) => rule("cScale"),
				},
				stroke: if is_redux { self.color("nodeBorder") } else { rule("cScale") },
				label: match (section, is_redux, theme.contains("redux")) {
					(None, _, true) | (Some(_), true, _) => self.color("nodeBorder"),
					_ if has_rules || section.is_none() => {
						self.color(&format!("cScaleLabel{label_index}"))
					}
					_ => String::new(),
				},
				line_stroke: rule("cScaleInv"),
				line_width: "3",
			},
		};
		if self.uses_gradient {
			paint.line_width = "0";
			if self.is_neo && has_rules {
				paint.stroke = format!("url(#{}-gradient)", self.context.id);
				paint.fill = self.color("mainBkg");
			}
		}
		paint
	}

	/// The stroke and its width of an edge to a child in `section` below `depth`
	/// (`.section-edge-N`, `.edge-depth-N`).
	fn edge(&self, section: Option<usize>, depth: i64) -> (String, String) {
		let theme = self.context.theme_name.as_str();
		let stroke = match section.map(|section| section + 1).filter(|&i| i < self.color_limit) {
			Some(_) if self.is_neo && (theme.contains("redux") || theme == "neo-dark") => {
				self.color("nodeBorder")
			}
			Some(index) => self.color(&format!("cScale{index}")),
			None => String::new(),
		};
		let index = depth + 1;
		let width = match index >= 0 && (index as usize) < self.color_limit {
			true if self.is_neo => (10 - (index - 1) * 2).max(2),
			// A negative width is invalid and leaves `.edge { stroke-width: 3 }`.
			true => (17 - 3 * index).max(-1),
			false => -1,
		};
		let width = if width < 0 { 3 } else { width };
		(stroke, width.to_string())
	}
}

/// The node shape names of `flattenNodes`.
fn shape_name(kind: NodeType, theme: &str) -> &'static str {
	match kind {
		NodeType::Circle => "mindmapCircle",
		NodeType::Rect => "rect",
		NodeType::RoundedRect => "rounded",
		NodeType::Cloud => "cloud",
		NodeType::Bang => "bang",
		NodeType::Hexagon => "hexagon",
		NodeType::Default if theme.to_lowercase().contains("redux") => "rounded",
		NodeType::Default => "defaultMindmapNode",
	}
}

/// `defaultMindmapNode`: a rectangle with rounded corners (neo: only the top ones) and a line
/// along its bottom.
fn default_node_shape(label: &LabelBox, padding: f64, is_neo: bool) -> (Shape, Point, Point) {
	let half_padding = padding / 2.0;
	let w = label.width + 8.0 * half_padding;
	let h = label.height + 2.0 * half_padding;
	const RD: f64 = 5.0;
	let d = match is_neo {
		true => format!(
			"M{} {} v{} q0,-{RD} {RD},-{RD} h{} q{RD},0 {RD},{RD} v{} H{} Z",
			number(-w / 2.0),
			number(h / 2.0 - RD),
			number(-h + 2.0 * RD),
			number(w - 2.0 * RD),
			number(h - RD),
			number(-w / 2.0)
		),
		false => format!(
			"M{} {} v{} q0,-{RD} {RD},-{RD} h{} q{RD},0 {RD},{RD} v{} q0,{RD} {},{RD} h{} q{},0 {},{} Z",
			number(-w / 2.0),
			number(h / 2.0 - RD),
			number(-h + 2.0 * RD),
			number(w - 2.0 * RD),
			number(h - 2.0 * RD),
			number(-RD),
			number(-(w - 2.0 * RD)),
			number(-RD),
			number(-RD),
			number(-RD)
		),
	};
	let mut bbox = BBox::default();
	bbox.include_rect(-w / 2.0, -h / 2.0, w, h);
	let shape = Shape {
		parts: vec![shapes::Part::Path { d, offset: Point::default(), class: "node-bkg node-0" }],
		bbox,
		intersect: shapes::Intersect::Rect,
		label_center: Point::default(),
	};
	(shape, Point { x: -w / 2.0, y: h / 2.0 }, Point { x: w / 2.0, y: h / 2.0 })
}

/// A laid out node: its shape and label, and the bottom line of default nodes.
struct Drawing {
	node: Node,
	shape: Shape,
	label: LabelBox,
	line: Option<(Point, Point)>,
	section: Option<usize>,
}

/// `intersectLineEllipse` of cytoscape's default node, a 30 pixel circle, and
/// `shortenIntersection` without arrows: where an edge from `from` ends at the node on `center`.
fn edge_end(center: Point, from: Point) -> Point {
	const RADIUS: f64 = 30.0 / 2.0;
	let (disp_x, disp_y) = ((center.x - from.x) / RADIUS, (center.y - from.y) / RADIUS);
	let length = (disp_x * disp_x + disp_y * disp_y).sqrt();
	let proportion = (length - 1.0) / length;
	if proportion.is_nan() || proportion < 0.0 {
		return center;
	}
	let intersect = Point {
		x: (center.x - from.x) * proportion + from.x,
		y: (center.y - from.y) * proportion + from.y,
	};
	let (dx, dy) = (intersect.x - from.x, intersect.y - from.y);
	let length = (dx * dx + dy * dy).sqrt();
	let ratio = (length - 0.0) / length;
	match ratio.is_finite() {
		true => Point { x: from.x + ratio * dx, y: from.y + ratio * dy },
		false => intersect,
	}
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let mut db = parse(code, context)?;
	if db.nodes.len().saturating_sub(1) > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} edges found, but the limit is {}.",
			db.nodes.len() - 1,
			context.limits.max_edges
		)));
	}
	let use_max_width = context.config_bool("mindmap", "useMaxWidth", true);
	let mut body = Svg::new();
	let mut bbox = BBox::default();
	if !db.nodes.is_empty() {
		db.assign_sections(context.config_text("mindmap", "colorBy").as_deref() == Some("depth"));
		draw(context, &db, &mut body, &mut bbox);
	}
	let padding = db.padding;
	let (x, y, width, height) = match bbox.is_empty() {
		true => (0.0, 0.0, 0.0, 0.0),
		false => (bbox.min.x, bbox.min.y, bbox.width(), bbox.height()),
	};
	let frame = Frame {
		x: x - padding,
		y: y - padding,
		width: width + 2.0 * padding,
		height: height + 2.0 * padding,
		use_max_width,
	};
	Ok(document(context, "mindmap", &frame, &Default::default(), body))
}

fn draw(context: &mut Context, db: &MindmapDb, body: &mut Svg, bbox: &mut BBox) {
	let max_node_width = context.config_number("mindmap", "maxNodeWidth", 200.0);
	let max_node_width =
		if max_node_width.is_finite() { max_node_width.clamp(0.0, 100_000.0) } else { 200.0 };
	let wrapping_width = context.config_number("flowchart", "wrappingWidth", 120.0);
	let look = context.look;
	let is_neo = look == Look::Neo;
	let theme_name = context.theme_name.clone();
	let (order, edges) = db.preorder();
	let (font, _) = rendering::label_font(context, &Styles::default());
	let theme = Theme::new(context, DiagramStyle::Flowchart);

	// `insertNode` of every node for its size.
	let mut drawings: Vec<Drawing> = Vec::with_capacity(order.len());
	for &index in &order {
		let mindmap_node = &db.nodes[index];
		let shape = shape_name(mindmap_node.kind, &theme_name);
		let mut classes = match index {
			0 => "mindmap-node section-root section--1".to_owned(),
			_ => match mindmap_node.section {
				Some(section) => format!("mindmap-node section-{section}"),
				None => "mindmap-node".to_owned(),
			},
		};
		if let Some(class) = &mindmap_node.class {
			classes.push(' ');
			classes.push_str(class);
		}
		// `draw` of the renderer: rounded and square nodes wrap without a width.
		let (padding, width) = match shape {
			"rounded" => (15.0, 0.0),
			"rect" => (10.0, 0.0),
			"hexagon" => (mindmap_node.padding, 0.0),
			_ => (mindmap_node.padding, max_node_width),
		};
		let wrap = if width != 0.0 { width } else { wrapping_width };
		let mut node = Node {
			id: index.to_string(),
			dom_id: format!("node_{index}"),
			label: mindmap_node.description.clone(),
			is_markdown: true,
			padding,
			css_classes: classes,
			look,
			shape: shape.to_owned(),
			..Node::default()
		};
		let (shape, label, line) = match shape {
			"defaultMindmapNode" => {
				let label =
					rendering::layout_label(context, &node.label, true, &font, Some(wrap), 0.0);
				let (shape, from, to) = default_node_shape(&label, padding, is_neo);
				(shape, label, Some((from, to)))
			}
			name => {
				// `mindmapCircle` passes its whole padding, where `circle` takes half of it.
				if name == "mindmapCircle" {
					node.shape = "circle".to_owned();
					node.padding = padding * 2.0;
				}
				let text = node.label.clone();
				let (shape, label) = rendering::layout_shape(
					context,
					&node,
					(&text, &font, &Styles::default()),
					(Some(wrap), 0.0),
					false,
					DiagramStyle::Flowchart,
				);
				let styles = Styles::default();
				let shape =
					sketch::apply(shape, context, &node, &styles, &theme, DiagramStyle::Flowchart);
				node.shape = name.to_owned();
				(shape, label, None)
			}
		};
		drawings.push(Drawing { node, shape, label, line, section: mindmap_node.section });
	}

	// `executeCoseBilkentLayout` on the sizes `getBBox` gives in single precision.
	let position_of: std::collections::HashMap<usize, usize> =
		order.iter().enumerate().map(|(position, &index)| (index, position)).collect();
	let sizes: Vec<(f64, f64)> = drawings
		.iter()
		.map(|drawing| {
			let bbox = rendering::node_box(&drawing.shape, &drawing.label);
			(f64::from(bbox.width() as f32), f64::from(bbox.height() as f32))
		})
		.collect();
	let layout_edges: Vec<(usize, usize)> =
		edges.iter().map(|(parent, child)| (position_of[parent], position_of[child])).collect();
	let centers = cose_bilkent::layout(&sizes, &layout_edges);

	let paint = Paint {
		context,
		color_limit: context
			.theme
			.get("THEME_COLOR_LIMIT")
			.and_then(Value::as_number)
			.filter(|limit| limit.is_finite() && *limit >= 0.0)
			.map_or(12, |limit| limit.min(1000.0) as usize),
		is_neo,
		uses_gradient: matches!(context.theme.get("useGradient"), Some(Value::Bool(true))),
		stroke_width: context.theme.text("strokeWidth").unwrap_or_else(|| "2".to_owned()),
		filter: theme.neo_filter.clone(),
	};
	let node_paints: Vec<SectionPaint> = drawings
		.iter()
		.enumerate()
		.map(|(position, drawing)| {
			paint.node(if order[position] == 0 { None } else { drawing.section })
		})
		.collect();
	let edge_paints: Vec<(String, String)> = edges
		.iter()
		.map(|&(parent, child)| paint.edge(db.nodes[child].section, db.nodes[parent].level + 1))
		.collect();
	let stroke_width = paint.stroke_width.clone();
	let filter = paint.filter.clone();

	body.open("g").end();
	let colors = MarkerColors {
		is_extension_filled: false,
		line: theme.line_color.clone(),
		edge: theme.edge_stroke.clone(),
		main_bkg: theme.plain_main_bkg.clone(),
		stroke_width: theme.stroke_width.clone(),
	};
	let mut markers = Markers::write(body, &context.id, "mindmap", &["point"], colors);
	body.open("g").attr("class", "subgraphs").close();
	body.open("g").attr("class", "edgePaths").end();
	for (number, &(parent, child)) in edges.iter().enumerate() {
		let (from, to) = (position_of[&parent], position_of[&child]);
		let (source, target) = (centers[from], centers[to]);
		let start = edge_end(source, target);
		let end = edge_end(target, source);
		let middle = Point {
			x: (start.x + end.x + start.x + end.x) / 4.0,
			y: (start.y + end.y + start.y + end.y) / 4.0,
		};
		let section = db.nodes[child].section;
		let mut classes = "edge".to_owned();
		if let Some(section) = section {
			classes.push_str(&format!(" section-edge-{section}"));
		}
		classes.push_str(&format!(" edge-depth-{}", db.nodes[parent].level + 1));
		let edge = Edge {
			id: format!("edge_{parent}_{child}"),
			start: parent.to_string(),
			end: child.to_string(),
			classes,
			curve: Some("basis".to_owned()),
			look,
			paint: Some(edge_paints[number].clone()),
			..Edge::default()
		};
		let route = EdgeRoute {
			jumps: None,
			points: vec![start, middle, end],
			label_position: None,
			from_cluster: None,
			to_cluster: None,
		};
		let end_of = |center: Point| EdgeEnd {
			bounds: Bounds { x: center.x, y: center.y, width: 0.0, height: 0.0 },
			intersect: None,
		};
		edges::write_edge(
			body,
			bbox,
			context,
			&edge,
			&route,
			&end_of(source),
			&end_of(target),
			&mut markers,
			&theme,
			"basis",
		);
	}
	body.close_tag("g");
	body.open("g").attr("class", "nodes").end();
	// `getNodeClasses`.
	let node_class = if look == Look::HandDrawn { "rough-node" } else { "node" };
	for (position, drawing) in drawings.iter().enumerate() {
		let center = centers[position];
		let node_paint = &node_paints[position];
		let node = &drawing.node;
		body.open("g")
			.attr("class", &format!("{node_class} {}", node.css_classes))
			.attr("id", &format!("{}-{}", context.id, node.dom_id))
			.attr("data-look", look.data_name())
			.attr("transform", &translate(center.x, center.y));
		if is_neo {
			body.attr_if("filter", &filter);
		}
		body.end();
		let shape_theme = Theme {
			main_bkg: node_paint.fill.clone(),
			neo_stroke: node_paint.stroke.clone(),
			node_stroke_width: if is_neo { format!("{stroke_width}px") } else { String::new() },
			neo_filter: filter.clone(),
			..Theme::default()
		};
		match &drawing.line {
			Some((from, to)) => {
				if let Some(shapes::Part::Path { d, class, .. }) = drawing.shape.parts.first() {
					body.open("path").attr("class", class).attr_if("fill", &node_paint.fill);
					if is_neo {
						body.attr_if("stroke", &node_paint.stroke)
							.attr("stroke-width", &format!("{stroke_width}px"));
					}
					body.attr("d", d).close();
				}
				body.open("line")
					.attr("class", "node-line-")
					.attr_if("stroke", &node_paint.line_stroke)
					.attr("stroke-width", node_paint.line_width)
					.number_attr("x1", from.x)
					.number_attr("y1", from.y)
					.number_attr("x2", to.x)
					.number_attr("y2", to.y)
					.close();
			}
			None if look == Look::HandDrawn && !node_paint.fill.is_empty() => {
				// `.section-N path` fills the paths of the sketch.
				let mut shape = drawing.shape.clone();
				sketch::fill_all_paths(&mut shape, &node_paint.fill);
				shapes::write(body, &shape, node, &Styles::default(), &shape_theme);
			}
			None => shapes::write(body, &drawing.shape, node, &Styles::default(), &shape_theme),
		}
		body.open("g").attr("class", "label").end();
		write_label(body, context, &drawing.label, drawing.shape.label_center, &node_paint.label);
		body.close_tag("g").close_tag("g");
		bbox.include(&rendering::node_box(&drawing.shape, &drawing.label), center.x, center.y);
	}
	body.close_tag("g");
	markers.write_colored(body, "mindmap");
	body.close_tag("g");
	write_look_defs(body, context);
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn db(code: &str) -> Result<MindmapDb> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_the_tree() {
		let db = db("mindmap\n  root((mindmap))\n    Origins\n      Long history\n      ::icon(fa fa-book)\n    id)I am a cloud(\n      id))I am a bang((\n    b[sq]\n    :::urgent large\n    c(round)\n    d{{hex}}\n").unwrap();
		let descriptions: Vec<_> = db.nodes.iter().map(|node| node.description.as_str()).collect();
		assert_eq!(
			descriptions,
			[
				"mindmap",
				"Origins",
				"Long history",
				"I am a cloud",
				"I am a bang",
				"sq",
				"round",
				"hex"
			]
		);
		let kinds: Vec<_> = db.nodes.iter().map(|node| node.kind).collect();
		use NodeType::*;
		assert_eq!(kinds, [Circle, Default, Default, Cloud, Bang, Rect, RoundedRect, Hexagon]);
		assert_eq!(db.nodes[0].children, [1, 3, 5, 6, 7]);
		assert_eq!(db.nodes[5].class.as_deref(), Some("urgent large"));
		assert_eq!(db.nodes[5].padding, 20.0);
		let (order, edges) = db.preorder();
		assert_eq!(order, [0, 1, 2, 3, 4, 5, 6, 7]);
		assert_eq!(edges[..3], [(0, 1), (1, 2), (0, 3)]);
	}

	#[test]
	fn reports_what_mermaid_rejects() {
		let error = db("mindmap\n  root\n  second\n").err().unwrap();
		assert_eq!(
			error.message,
			"There can be only one root. No parent could be found for (\"second\")"
		);
		assert_eq!(error.line, Some(3));
		assert!(db("mindmap\n::icon(x)\n").is_err());
		assert!(db("mindmap\n  a(b\n").is_err());
		assert!(db("graph\n  a\n").is_err());
	}

	#[test]
	fn assigns_sections_like_mermaid() {
		let mut db = db("mindmap\nr\n a\n  a1\n b\n c\n").unwrap();
		db.assign_sections(false);
		let sections: Vec<_> = db.nodes.iter().map(|node| node.section).collect();
		assert_eq!(sections, [None, Some(0), Some(0), Some(1), Some(2)]);
		db.assign_sections(true);
		let sections: Vec<_> = db.nodes.iter().map(|node| node.section).collect();
		assert_eq!(sections, [None, Some(0), Some(1), Some(0), Some(0)]);
	}

	#[test]
	fn ends_edges_at_cytoscapes_default_node() {
		let end = edge_end(Point { x: 0.0, y: 0.0 }, Point { x: 30.0, y: 40.0 });
		assert!((end.x - 9.0).abs() < 1e-12 && (end.y - 12.0).abs() < 1e-12);
		let inside = edge_end(Point { x: 0.0, y: 0.0 }, Point { x: 3.0, y: 4.0 });
		assert_eq!(inside, Point { x: 0.0, y: 0.0 });
	}
}
