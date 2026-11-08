//! `flowDb.ts`: the vertices, edges, classes and subgraphs the parser reports, and the layout
//! data built from them (`getData`). Agentflow's database (`agentflowDb.ts`) keeps its graph in a
//! `FlowDb` too and shares the actions that both run alike.

use std::collections::HashMap;

use super::super::common::sanitize_url;
use super::super::value::{Value, parse_yaml};
use super::parser::GraphDb;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelType {
	#[default]
	Text,
	String,
	Markdown,
}

impl LabelType {
	/// `sanitizeNodeLabelType`: unknown types are Markdown.
	pub fn from_name(name: Option<&str>) -> LabelType {
		match name {
			Some("text") => LabelType::Text,
			Some("string") => LabelType::String,
			_ => LabelType::Markdown,
		}
	}
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlowText {
	pub text: String,
	pub label_type: LabelType,
}

#[derive(Clone, Debug, Default)]
pub struct Vertex {
	pub id: String,
	pub dom_id: String,
	pub text: Option<String>,
	pub label_type: LabelType,
	/// The shape keyword of the source (`square`, `round`, …) or a shape name of `@{}`.
	pub kind: Option<String>,
	pub styles: Vec<String>,
	pub classes: Vec<String>,
	pub dir: Option<String>,
	pub link: Option<String>,
	pub link_target: Option<String>,
	pub icon: Option<String>,
	pub form: Option<String>,
	pub pos: Option<String>,
	pub img: Option<String>,
	pub constraint: Option<String>,
	pub asset_width: Option<f64>,
	pub asset_height: Option<f64>,
	/// An agentflow `connector`.
	pub is_connector: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stroke {
	#[default]
	Normal,
	Thick,
	Dotted,
	Invisible,
	Invalid,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Link {
	/// `arrow_point`, `double_arrow_circle`, … or `INVALID`.
	pub kind: String,
	pub stroke: Stroke,
	pub length: usize,
	pub text: Option<FlowText>,
	pub id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Edge {
	pub start: String,
	pub end: String,
	pub kind: Option<String>,
	pub text: String,
	pub label_type: LabelType,
	pub stroke: Stroke,
	pub length: usize,
	pub id: String,
	pub is_user_defined_id: bool,
	pub interpolate: Option<String>,
	pub style: Option<Vec<String>>,
	pub classes: Vec<String>,
	pub animate: Option<bool>,
	pub animation: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FlowClass {
	pub styles: Vec<String>,
	pub text_styles: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SubGraph {
	pub id: String,
	pub nodes: Vec<String>,
	pub title: String,
	pub classes: Vec<String>,
	pub dir: Option<String>,
	pub label_type: LabelType,
	pub metadata: Option<Value>,
}

/// A statement's contribution to the node list of an enclosing subgraph.
#[derive(Clone, Debug, PartialEq)]
pub enum Member {
	Id(String),
	Direction(String),
}

#[derive(Default)]
pub struct FlowDb {
	pub vertices: Vec<Vertex>,
	vertex_index: HashMap<String, usize>,
	pub edges: Vec<Edge>,
	pub default_interpolate: Option<String>,
	pub default_style: Option<Vec<String>>,
	pub classes: Vec<FlowClass>,
	class_index: HashMap<String, usize>,
	pub subgraphs: Vec<SubGraph>,
	subgraph_lookup: HashMap<String, usize>,
	tooltips: HashMap<String, String>,
	sub_count: usize,
	vertex_counter: usize,
	pub direction: Option<String>,
	pub max_edges: usize,
	/// The start of the vertex dom ids: `flowchart` or `agentflow`.
	dom_id_prefix: &'static str,
}

/// Text of a YAML value like JavaScript's `String(value)` for the scalars of shape data.
pub fn scalar(value: &Value) -> Option<String> {
	match value {
		Value::String(text) => Some(text.clone()),
		Value::Number(number) => Some(super::super::color::js_number(*number)),
		Value::Bool(value) => Some(value.to_string()),
		_ => None,
	}
}

pub fn is_truthy(value: Option<&Value>) -> bool {
	match value {
		None | Some(Value::Null) => false,
		Some(Value::Bool(value)) => *value,
		Some(Value::Number(number)) => *number != 0.0 && !number.is_nan(),
		Some(Value::String(text)) => !text.is_empty(),
		Some(_) => true,
	}
}

/// The text of a label, trimmed and without the quotes around it.
pub fn unquote(text: &str) -> String {
	let text = text.trim();
	match text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
		true => text[1..text.len() - 1].to_owned(),
		false => text.to_owned(),
	}
}

/// The direction and the distinct, non-blank node ids of a subgraph's statements.
pub fn collect_members(members: &[Member]) -> (Option<String>, Vec<String>) {
	let mut dir = None;
	let mut nodes: Vec<String> = Vec::new();
	for member in members {
		match member {
			Member::Direction(direction) => dir = Some(direction.clone()),
			Member::Id(id) if id.trim().is_empty() => {}
			Member::Id(id) => {
				if !nodes.contains(id) {
					nodes.push(id.clone());
				}
			}
		}
	}
	(dir, nodes)
}

impl FlowDb {
	pub fn new(max_edges: usize) -> Self {
		Self::with_dom_id_prefix(max_edges, "flowchart")
	}

	pub fn with_dom_id_prefix(max_edges: usize, dom_id_prefix: &'static str) -> Self {
		Self { max_edges, dom_id_prefix, ..Self::default() }
	}

	pub fn vertex(&self, id: &str) -> Option<&Vertex> {
		self.vertex_index.get(id).map(|&index| &self.vertices[index])
	}

	pub fn vertex_mut(&mut self, id: &str) -> Option<&mut Vertex> {
		self.vertex_index.get(id).map(|&index| &mut self.vertices[index])
	}

	/// The dom id the next new vertex `id` gets.
	pub fn next_dom_id(&self, id: &str) -> String {
		format!("{}-{id}-{}", self.dom_id_prefix, self.vertex_counter)
	}

	/// `Map.set`: replaces the vertex of the same id in its place or appends it; counts it.
	pub fn set_vertex(&mut self, vertex: Vertex) {
		match self.vertex_index.get(&vertex.id) {
			Some(&index) => self.vertices[index] = vertex,
			None => {
				self.vertex_index.insert(vertex.id.clone(), self.vertices.len());
				self.vertices.push(vertex);
			}
		}
		self.vertex_counter += 1;
	}

	/// The part of `addVertex` both dialects share: creates the vertex `id` if it is new, counts
	/// the call and sets its text, shape keyword and styles.
	pub fn update_vertex(
		&mut self,
		id: &str,
		text: Option<&FlowText>,
		kind: Option<&str>,
		styles: Option<&[String]>,
	) -> Option<&mut Vertex> {
		if !self.vertex_index.contains_key(id) {
			let vertex =
				Vertex { id: id.to_owned(), dom_id: self.next_dom_id(id), ..Vertex::default() };
			self.vertex_index.insert(id.to_owned(), self.vertices.len());
			self.vertices.push(vertex);
		}
		self.vertex_counter += 1;
		let vertex = self.vertex_mut(id)?;
		match text {
			Some(text) => {
				vertex.label_type = text.label_type;
				vertex.text = Some(unquote(&text.text));
			}
			None if vertex.text.is_none() => vertex.text = Some(id.to_owned()),
			None => {}
		}
		if let Some(kind) = kind {
			vertex.kind = Some(kind.to_owned());
		}
		if let Some(styles) = styles {
			vertex.styles.extend(styles.iter().cloned());
		}
		Some(vertex)
	}

	/// Shape data of a subgraph: merged into its metadata. False if `id` is no subgraph.
	pub fn merge_subgraph_metadata(&mut self, id: &str, doc: &Value) -> bool {
		let Some(&index) = self.subgraph_lookup.get(id) else { return false };
		let subgraph = &mut self.subgraphs[index];
		match &mut subgraph.metadata {
			Some(existing) => existing.merge(doc),
			None => subgraph.metadata = Some(doc.clone()),
		}
		true
	}

	/// Shape data of an edge: its animation and curve, the texts made by `scalar`. False if `id`
	/// is no edge.
	pub fn apply_edge_metadata(
		&mut self,
		id: &str,
		doc: Option<&Value>,
		scalar: fn(&Value) -> Option<String>,
	) -> bool {
		let Some(edge) = self.edges.iter_mut().find(|edge| edge.id == id) else { return false };
		if let Some(doc) = doc {
			if let Some(animate) = doc.get("animate") {
				edge.animate = Some(is_truthy(Some(animate)));
			}
			if let Some(animation) = doc.get("animation").and_then(scalar) {
				edge.animation = Some(animation);
			}
			if let Some(curve) = doc.get("curve").and_then(scalar) {
				edge.interpolate = Some(curve);
			}
		}
		true
	}

	/// The id of a new subgraph: its own or `subGraph<n>`; counts the subgraph.
	pub fn subgraph_id(&mut self, id: Option<String>) -> String {
		let id = id.unwrap_or_else(|| format!("subGraph{}", self.sub_count));
		self.sub_count += 1;
		id
	}

	/// Makes `subgraphs[index]` the subgraph of `id`.
	pub fn index_subgraph(&mut self, id: &str, index: usize) {
		self.subgraph_lookup.insert(id.to_owned(), index);
	}

	pub fn bounds_error(&self, index: usize) -> String {
		format!(
			"The index {index} for linkStyle is out of bounds. Valid indices for linkStyle are between 0 and {}. (Help: Ensure that the index is within the range of existing edges.)",
			self.edges.len() as isize - 1
		)
	}

	fn add_single_link(
		&mut self,
		start: &str,
		end: &str,
		link: &Link,
		id: Option<&str>,
	) -> Result<(), String> {
		let mut edge = Edge {
			start: start.to_owned(),
			end: end.to_owned(),
			interpolate: self.default_interpolate.clone(),
			..Edge::default()
		};
		if let Some(text) = &link.text {
			edge.text = unquote(&text.text);
			edge.label_type = text.label_type;
		}
		edge.kind = Some(link.kind.clone());
		edge.stroke = link.stroke;
		edge.length = link.length.min(10);
		match id {
			Some(id) if !self.edges.iter().any(|edge| edge.id == id) => {
				edge.id = id.to_owned();
				edge.is_user_defined_id = true;
			}
			_ => {
				let existing = self
					.edges
					.iter()
					.filter(|other| other.start == start && other.end == end)
					.count();
				let counter = if existing == 0 { 0 } else { existing + 1 };
				edge.id = format!("L_{start}_{end}_{counter}");
			}
		}
		if self.edges.len() >= self.max_edges {
			return Err(format!(
				"Edge limit exceeded. {} edges found, but the limit is {}.",
				self.edges.len(),
				self.max_edges
			));
		}
		self.edges.push(edge);
		Ok(())
	}

	pub fn class(&self, id: &str) -> Option<&FlowClass> {
		self.class_index.get(id).map(|&index| &self.classes[index])
	}

	pub fn is_subgraph(&self, id: &str) -> bool {
		self.subgraph_lookup.contains_key(id)
	}

	pub fn subgraph(&self, id: &str) -> Option<&SubGraph> {
		self.subgraph_lookup.get(id).map(|&index| &self.subgraphs[index])
	}

	pub fn tooltip(&self, id: &str) -> Option<&str> {
		self.tooltips.get(id).map(String::as_str)
	}

	/// `getCompiledStyles`: the styles of the classes, then their text styles.
	pub fn compiled_styles<S: AsRef<str>>(&self, classes: &[S]) -> Vec<String> {
		let mut compiled = Vec::new();
		for class in classes {
			if let Some(class) = self.class(class.as_ref()) {
				compiled.extend(class.styles.iter().map(|style| style.trim().to_owned()));
				compiled.extend(class.text_styles.iter().map(|style| style.trim().to_owned()));
			}
		}
		compiled
	}
}

impl GraphDb for FlowDb {
	/// `addVertex`. `metadata` is the text of `@{ … }`.
	fn add_vertex(
		&mut self,
		id: &str,
		text: Option<&FlowText>,
		kind: Option<&str>,
		styles: Option<&[String]>,
		metadata: Option<&str>,
	) -> Result<(), String> {
		if id.trim().is_empty() {
			return Ok(());
		}
		let doc = match metadata {
			Some(metadata) => {
				let yaml = match metadata.contains('\n') {
					true => format!("{metadata}\n"),
					false => format!("{{\n{metadata}\n}}"),
				};
				Some(parse_yaml(&yaml).ok_or_else(|| format!("Invalid shape data for {id}"))?)
			}
			None => None,
		};
		if let Some(doc) = &doc
			&& self.merge_subgraph_metadata(id, doc)
		{
			return Ok(());
		}
		if self.apply_edge_metadata(id, doc.as_ref(), scalar) {
			return Ok(());
		}
		let Some(vertex) = self.update_vertex(id, text, kind, styles) else { return Ok(()) };
		let Some(doc) = doc else { return Ok(()) };
		if let Some(shape) = doc.get("shape").and_then(scalar) {
			if shape != shape.to_lowercase() || shape.contains('_') {
				return Err(format!("No such shape: {shape}. Shape names should be lowercase."));
			}
			if !super::super::rendering::is_shape_name(&shape) {
				return Err(format!("No such shape: {shape}."));
			}
			vertex.kind = Some(shape);
		}
		let label = doc.get("label").and_then(scalar);
		if let Some(label) = label.as_ref().filter(|label| !label.is_empty()) {
			vertex.text = Some(label.clone());
			vertex.label_type = LabelType::from_name(doc.get("labelType").and_then(Value::as_str));
		}
		let label_is_blank = label.as_ref().is_none_or(|label| label.trim().is_empty());
		if let Some(icon) = doc.get("icon").and_then(scalar).filter(|icon| !icon.is_empty()) {
			vertex.icon = Some(icon);
			if label_is_blank && vertex.text.as_deref() == Some(id) {
				vertex.text = Some(String::new());
			}
		}
		if let Some(form) = doc.get("form").and_then(scalar).filter(|form| !form.is_empty()) {
			vertex.form = Some(form);
		}
		if let Some(pos) = doc.get("pos").and_then(scalar).filter(|pos| !pos.is_empty()) {
			vertex.pos = Some(pos);
		}
		if let Some(img) = doc.get("img").and_then(scalar).filter(|img| !img.is_empty()) {
			vertex.img = Some(img);
			if label_is_blank && vertex.text.as_deref() == Some(id) {
				vertex.text = Some(String::new());
			}
		}
		if let Some(constraint) = doc.get("constraint").and_then(scalar).filter(|c| !c.is_empty()) {
			vertex.constraint = Some(constraint);
		}
		let number = |key: &str| {
			is_truthy(doc.get(key))
				.then(|| doc.get(key).and_then(scalar))
				.flatten()
				.and_then(|text| text.trim().parse::<f64>().ok())
		};
		if let Some(width) = number("w") {
			vertex.asset_width = Some(width);
		}
		if let Some(height) = number("h") {
			vertex.asset_height = Some(height);
		}
		Ok(())
	}

	/// `addLink` for every pair of the node groups; the user's edge id goes to the last start and
	/// first end.
	fn add_link(&mut self, starts: &[String], ends: &[String], link: &Link) -> Result<(), String> {
		let id = link.id.as_ref().map(|id| id.replacen('@', "", 1));
		for start in starts {
			for end in ends {
				let is_last_start = Some(start) == starts.last();
				let is_first_end = Some(end) == ends.first();
				let id = if is_last_start && is_first_end { id.as_deref() } else { None };
				self.add_single_link(start, end, link, id)?;
			}
		}
		Ok(())
	}

	/// `updateLinkInterpolate`; `None` is `default`.
	fn update_link_interpolate(
		&mut self,
		positions: &[Option<usize>],
		interpolate: &str,
	) -> Result<(), String> {
		for position in positions {
			match position {
				None => self.default_interpolate = Some(interpolate.to_owned()),
				Some(index) => {
					if let Some(edge) = self.edges.get_mut(*index) {
						edge.interpolate = Some(interpolate.to_owned());
					}
				}
			}
		}
		Ok(())
	}

	/// `updateLink`; `None` is `default`.
	fn update_link(
		&mut self,
		positions: &[Option<usize>],
		styles: &[String],
	) -> Result<(), String> {
		for position in positions {
			match position {
				None => self.default_style = Some(styles.to_vec()),
				Some(index) => {
					let error = self.bounds_error(*index);
					let Some(edge) = self.edges.get_mut(*index) else { return Err(error) };
					let mut style = styles.to_vec();
					if !style.is_empty() && !style.iter().any(|s| s.starts_with("fill")) {
						style.push("fill:none".to_owned());
					}
					edge.style = Some(style);
				}
			}
		}
		Ok(())
	}

	/// `addClass`: `classDef` with comma separated ids; `\,` is a comma in a value.
	fn add_class(&mut self, ids: &str, styles: &[String]) {
		let joined = styles.join(",").replace("\\,", "§§§").replace(',', ";").replace("§§§", ",");
		let style: Vec<String> = joined.split(';').map(str::to_owned).collect();
		for id in ids.split(',') {
			let index = match self.class_index.get(id) {
				Some(&index) => index,
				None => {
					self.class_index.insert(id.to_owned(), self.classes.len());
					self.classes.push(FlowClass::default());
					self.classes.len() - 1
				}
			};
			let class = &mut self.classes[index];
			for s in &style {
				if s.contains("color") {
					class.text_styles.push(s.replacen("fill", "bgFill", 1));
				}
				class.styles.push(s.clone());
			}
		}
	}

	/// `setDirection`.
	fn set_direction(&mut self, direction: &str) {
		let mut direction = direction.trim().to_owned();
		if direction.contains('<') {
			direction = "RL".to_owned();
		}
		if direction.contains('^') {
			direction = "BT".to_owned();
		}
		if direction.contains('>') {
			direction = "LR".to_owned();
		}
		if direction.contains('v') {
			direction = "TB".to_owned();
		}
		if direction == "TD" {
			direction = "TB".to_owned();
		}
		self.direction = Some(direction);
	}

	/// `setClass` for vertices, edges and subgraphs.
	fn set_class(&mut self, ids: &str, class: &str) {
		for id in ids.split(',') {
			if let Some(vertex) = self.vertex_mut(id) {
				vertex.classes.push(class.to_owned());
			}
			if let Some(edge) = self.edges.iter_mut().find(|edge| edge.id == id) {
				edge.classes.push(class.to_owned());
			}
			if let Some(&index) = self.subgraph_lookup.get(id) {
				self.subgraphs[index].classes.push(class.to_owned());
			}
		}
	}

	fn set_tooltip(&mut self, ids: &str, tooltip: &str) {
		for id in ids.split(',') {
			self.tooltips.insert(id.to_owned(), tooltip.to_owned());
		}
	}

	/// `setClickEvent`: callbacks need `securityLevel: loose`, which documents cannot set, so
	/// only the class remains.
	fn set_click_event(&mut self, ids: &str) {
		self.set_class(ids, "clickable");
	}

	fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
		for id in ids.split(',') {
			if let Some(vertex) = self.vertex_mut(id) {
				let url = link.trim();
				vertex.link = (!url.is_empty()).then(|| sanitize_url(url));
				vertex.link_target = target.map(str::to_owned);
			}
		}
		self.set_class(ids, "clickable");
	}

	/// `addSubGraph`. `id` is `None` for `subgraph` without a name; `title_is_id` when the title
	/// is the same text as the id (then ids with whitespace are generated).
	fn add_subgraph(
		&mut self,
		id: Option<&FlowText>,
		members: &[Member],
		title: Option<&FlowText>,
		title_is_id: bool,
	) -> String {
		let mut subgraph_id = id.map(|id| id.text.trim().to_owned());
		let title_text = title.map(|title| title.text.clone()).unwrap_or_default();
		if title_is_id && title_text.contains(char::is_whitespace) {
			subgraph_id = None;
		}
		let (dir, nodes) = collect_members(members);
		let id = self.subgraph_id(subgraph_id);
		let label_type = title.map_or(LabelType::Markdown, |title| title.label_type);
		// `makeUniq`: nodes already in a subgraph stay there.
		let nodes: Vec<String> = nodes
			.into_iter()
			.filter(|node| !self.subgraphs.iter().any(|subgraph| subgraph.nodes.contains(node)))
			.filter(|node| *node != id)
			.collect();
		let index = match self.subgraphs.iter().position(|subgraph| subgraph.id == id) {
			Some(index) => {
				self.subgraphs[index].nodes.extend(nodes);
				index
			}
			None => {
				self.subgraphs.push(SubGraph {
					id: id.clone(),
					nodes,
					title: title_text.trim().to_owned(),
					classes: Vec::new(),
					dir,
					label_type,
					metadata: None,
				});
				self.subgraphs.len() - 1
			}
		};
		self.index_subgraph(&id, index);
		id
	}
}

/// `destructLink`: the arrow type, stroke and length of a link and its start (`--`, `==`, `-.`).
pub fn destruct_link(end: &str, start: Option<&str>) -> Link {
	let info = destruct_end_link(end);
	let Some(start) = start.filter(|start| !start.is_empty()) else { return info };
	let (mut kind, stroke) = destruct_start_link(start);
	if stroke != info.stroke {
		return Link { kind: "INVALID".to_owned(), stroke: Stroke::Invalid, ..info };
	}
	if kind == "arrow_open" {
		kind = info.kind.clone();
	} else {
		if kind != info.kind {
			return Link { kind: "INVALID".to_owned(), stroke: Stroke::Invalid, ..info };
		}
		kind = format!("double_{kind}");
	}
	if kind == "double_arrow" {
		kind = "double_arrow_point".to_owned();
	}
	Link { kind, stroke, length: info.length, text: None, id: None }
}

fn destruct_start_link(text: &str) -> (String, Stroke) {
	let mut text = text.trim();
	let kind = match text.chars().next() {
		Some('<') => "arrow_point",
		Some('x') => "arrow_cross",
		Some('o') => "arrow_circle",
		_ => "arrow_open",
	};
	if kind != "arrow_open" {
		text = &text[1..];
	}
	let mut stroke = Stroke::Normal;
	if text.contains('=') {
		stroke = Stroke::Thick;
	}
	if text.contains('.') {
		stroke = Stroke::Dotted;
	}
	(kind.to_owned(), stroke)
}

fn destruct_end_link(text: &str) -> Link {
	let text = text.trim();
	let mut line = &text[..text.len().saturating_sub(1)];
	let mut kind = "arrow_open".to_owned();
	let last = text.chars().next_back();
	let double_start = match last {
		Some('x') => Some(('x', "arrow_cross")),
		Some('>') => Some(('<', "arrow_point")),
		Some('o') => Some(('o', "arrow_circle")),
		_ => None,
	};
	if let Some((start, arrow)) = double_start {
		kind = arrow.to_owned();
		if text.starts_with(start) {
			kind = format!("double_{kind}");
			line = line.get(1..).unwrap_or("");
		}
	}
	let mut stroke = Stroke::Normal;
	let mut length = line.chars().count().saturating_sub(1);
	if line.starts_with('=') {
		stroke = Stroke::Thick;
	}
	if line.starts_with('~') {
		stroke = Stroke::Invisible;
	}
	let dots = line.matches('.').count();
	if dots > 0 {
		stroke = Stroke::Dotted;
		length = dots;
	}
	Link { kind, stroke, length, text: None, id: None }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn destructs_links_like_mermaid() {
		let link = destruct_link("-->", None);
		assert_eq!(
			(link.kind.as_str(), link.stroke, link.length),
			("arrow_point", Stroke::Normal, 1)
		);
		let link = destruct_link("<--->", None);
		assert_eq!((link.kind.as_str(), link.length), ("double_arrow_point", 2));
		let link = destruct_link("-.->", None);
		assert_eq!(
			(link.kind.as_str(), link.stroke, link.length),
			("arrow_point", Stroke::Dotted, 1)
		);
		let link = destruct_link("==x", None);
		assert_eq!((link.kind.as_str(), link.stroke), ("arrow_cross", Stroke::Thick));
		let link = destruct_link("~~~", None);
		assert_eq!((link.kind.as_str(), link.stroke), ("arrow_open", Stroke::Invisible));
		let link = destruct_link("-->", Some("<-- "));
		assert_eq!(link.kind, "double_arrow_point");
		let link = destruct_link("-->", Some("x-- "));
		assert_eq!(link.kind, "INVALID");
		assert_eq!(destruct_link("---", Some("-- ")).kind, "arrow_open");
	}

	#[test]
	fn generates_edge_ids_and_subgraphs() {
		let mut db = FlowDb::new(500);
		let link = destruct_link("-->", None);
		db.add_link(&["A".into()], &["B".into()], &link).unwrap();
		db.add_link(&["A".into()], &["B".into()], &link).unwrap();
		db.add_link(&["A".into()], &["B".into()], &Link { id: Some("e1@".into()), ..link.clone() })
			.unwrap();
		let ids: Vec<&str> = db.edges.iter().map(|edge| edge.id.as_str()).collect();
		assert_eq!(ids, ["L_A_B_0", "L_A_B_2", "e1"]);
		let members =
			[Member::Id("A".into()), Member::Direction("LR".into()), Member::Id("A".into())];
		let title = FlowText { text: "one two".into(), label_type: LabelType::Text };
		let id = db.add_subgraph(Some(&title), &members, Some(&title), true);
		assert_eq!(id, "subGraph0");
		assert_eq!(db.subgraphs[0].nodes, ["A"]);
		assert_eq!(db.subgraphs[0].dir.as_deref(), Some("LR"));
		assert_eq!(sanitize_url(" javascript:alert(1)"), "about:blank");
	}
}
