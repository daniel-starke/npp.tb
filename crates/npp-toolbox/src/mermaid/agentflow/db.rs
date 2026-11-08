//! `agentflowDb.ts`: the vertices, connectors, edges, classes and containers the parser reports,
//! kept in the flowchart's database (`flowchart::db`), which runs the actions both share.
//! Diagnostics, element mappings and the semantic model are for editor tooling and not ported.

use std::collections::HashSet;

use super::super::flowchart::db::{
	FlowDb, FlowText, LabelType, Link, Member, Stroke, SubGraph, Vertex, collect_members, is_truthy,
};
use super::super::flowchart::parser::GraphDb;
use super::super::value::{Value, parse_yaml};

/// `SHAPE_ALIASES` of `shapes.ts`: author-friendly names of Mermaid shapes.
const SHAPE_ALIASES: [(&str, &str); 7] = [
	("task", "roundedRect"),
	("tool", "subroutine"),
	("input", "lean-right"),
	("decision", "diamond"),
	("refdoc", "lin-doc"),
	("action", "hexagon"),
	("round", "rect"),
];

/// `resolveShapeAlias`.
pub fn resolve_shape_alias(shape: &str) -> &str {
	SHAPE_ALIASES.iter().find(|(alias, _)| *alias == shape).map_or(shape, |(_, name)| name)
}

/// The graph of a diagram: the flowchart's database with agentflow's own rules on top.
pub struct AgentflowDb {
	/// The vertices, connectors, edges, classes and `flow` containers.
	pub flow: FlowDb,
	global_nodes: HashSet<String>,
	/// `flowchart.inheritDir`: containers without a direction take the diagram's.
	inherit_dir: bool,
}

/// Text of a YAML value like JavaScript's `String(value)`.
fn scalar(value: &Value) -> Option<String> {
	value.to_text()
}

/// `JSON.stringify` of the scalars a `shape` can hold.
fn json_text(value: &Value) -> String {
	match value {
		Value::String(text) => format!("\"{text}\""),
		Value::Null => "null".to_owned(),
		Value::Array(_) => "[…]".to_owned(),
		Value::Object(_) => "{…}".to_owned(),
		other => other.to_text().unwrap_or_default(),
	}
}

/// `stripLineTrailingCommas`: the commas at the ends of the lines of a multi-line `@{ … }`, which
/// block YAML does not allow; commas in quotes, flow collections and block scalars stay.
fn strip_line_trailing_commas(body: &str) -> String {
	let mut in_single = false;
	let mut in_double = false;
	let mut flow_depth = 0usize;
	let mut block_scalar_indent: Option<usize> = None;
	let mut out: Vec<String> = Vec::new();
	for line in body.split('\n') {
		let indent = line.len() - line.trim_start().len();
		if let Some(block_indent) = block_scalar_indent {
			if line.trim().is_empty() || indent > block_indent {
				out.push(line.to_owned());
				continue;
			}
			block_scalar_indent = None;
		}
		let mut comment_start = None;
		let bytes = line.as_bytes();
		let mut index = 0;
		while index < bytes.len() {
			let byte = bytes[index];
			if in_double {
				if byte == b'\\' {
					index += 1;
				} else if byte == b'"' {
					in_double = false;
				}
			} else if in_single {
				if byte == b'\'' {
					in_single = false;
				}
			} else if byte == b'"' {
				in_double = true;
			} else if byte == b'\'' {
				in_single = true;
			} else if byte == b'#' && (index == 0 || matches!(bytes[index - 1], b' ' | b'\t')) {
				comment_start = Some(index);
				break;
			} else if byte == b'[' || byte == b'{' {
				flow_depth += 1;
			} else if byte == b']' || byte == b'}' {
				flow_depth = flow_depth.saturating_sub(1);
			}
			index += 1;
		}
		if in_single || in_double || flow_depth > 0 {
			out.push(line.to_owned());
			continue;
		}
		let (code, comment) = line.split_at(comment_start.unwrap_or(line.len()));
		if opens_block_scalar(code) {
			block_scalar_indent = Some(indent);
			out.push(line.to_owned());
			continue;
		}
		// `code.replace(/,([\t ]*)$/, '$1')`.
		let trimmed = code.trim_end_matches([' ', '\t']);
		let code = match trimmed.strip_suffix(',') {
			Some(before) => format!("{before}{}", &code[trimmed.len()..]),
			None => code.to_owned(),
		};
		out.push(code + comment);
	}
	out.join("\n")
}

/// `/:\s*[>|][\d+-]*\s*$/`: `key: |` or `key: >` opens a block scalar.
fn opens_block_scalar(code: &str) -> bool {
	let code = code.trim_end();
	let code = code.trim_end_matches(|c: char| c.is_ascii_digit() || c == '+' || c == '-');
	let Some(code) = code.strip_suffix(['>', '|']) else { return false };
	code.trim_end().ends_with(':')
}

/// The YAML of `@{ … }`: the single-line form is a flow mapping without its braces.
fn parse_metadata(metadata: &str) -> Option<Value> {
	let is_multiline = metadata.contains('\n');
	let yaml = match is_multiline {
		true => format!("{metadata}\n"),
		false => format!("{{\n{metadata}\n}}"),
	};
	let doc = parse_yaml(&yaml).or_else(|| {
		let stripped = strip_line_trailing_commas(metadata);
		(is_multiline && stripped != metadata).then(|| parse_yaml(&(stripped + "\n"))).flatten()
	})?;
	// A whitespace-only document is `null`, which counts as `{}`.
	Some(match doc {
		Value::Null => Value::Object(Vec::new()),
		other => other,
	})
}

impl AgentflowDb {
	pub fn new(max_edges: usize, inherit_dir: bool) -> Self {
		Self {
			flow: FlowDb::with_dom_id_prefix(max_edges, "agentflow"),
			global_nodes: HashSet::new(),
			inherit_dir,
		}
	}

	pub fn is_connector(&self, id: &str) -> bool {
		self.flow.vertex(id).is_some_and(|vertex| vertex.is_connector)
	}
}

impl GraphDb for AgentflowDb {
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
		let mut doc = match metadata {
			Some(metadata) => {
				Some(parse_metadata(metadata).ok_or_else(|| format!("Invalid metadata for {id}"))?)
			}
			None => None,
		};
		let authored_shape = doc
			.as_ref()
			.and_then(|doc| doc.get("shape"))
			.and_then(Value::as_str)
			.map(str::to_owned);
		if let (Some(doc), Some(authored)) = (&mut doc, &authored_shape) {
			let resolved = resolve_shape_alias(authored);
			if resolved != authored {
				let resolved = Value::String(resolved.to_owned());
				doc.merge(&Value::Object(vec![("shape".to_owned(), resolved)]));
			}
		}
		if let Some(doc) = &doc
			&& self.flow.merge_subgraph_metadata(id, doc)
		{
			return Ok(());
		}
		// Metadata of connectors stays on the connector; it carries no presentation.
		if self.is_connector(id) && doc.is_some() {
			return Ok(());
		}
		// The id of the former connectors group stays reserved.
		if id == "connectors" {
			return Ok(());
		}
		if self.flow.apply_edge_metadata(id, doc.as_ref(), scalar) {
			return Ok(());
		}
		let Some(vertex) = self.flow.update_vertex(id, text, kind, styles) else { return Ok(()) };
		let Some(doc) = doc else { return Ok(()) };
		if let Some(shape) = doc.get("shape").filter(|shape| is_truthy(Some(shape))) {
			let Some(authored) = authored_shape else {
				return Err(format!("No such shape: {}.", json_text(shape)));
			};
			if authored != authored.to_lowercase() || authored.contains('_') {
				return Err(format!("No such shape: {authored}. Shape names should be lowercase."));
			}
			let resolved = resolve_shape_alias(&authored);
			if !super::super::rendering::is_shape_name(resolved) {
				return Err(format!("No such shape: {resolved}."));
			}
			vertex.kind = Some(resolved.to_owned());
		}
		if let Some(label) = doc.get("label").filter(|label| is_truthy(Some(label))) {
			vertex.text = Some(scalar(label).unwrap_or_default());
			vertex.label_type = LabelType::from_name(doc.get("labelType").and_then(Value::as_str));
		}
		Ok(())
	}

	fn add_link(&mut self, starts: &[String], ends: &[String], link: &Link) -> Result<(), String> {
		self.flow.add_link(starts, ends, link)
	}

	/// `destructLink` of agentflow: `-->`, `--x`, `-.-` and `-- text -->`, without double arrows.
	fn destruct_link(&self, end: &str, start: Option<&str>) -> Link {
		let text = end.trim();
		let (kind, line) = match text.chars().next_back() {
			Some('x') => ("arrow_cross", &text[..text.len() - 1]),
			Some('>') => ("arrow_point", &text[..text.len() - 1]),
			_ => ("arrow_open", text),
		};
		let dots = line.matches('.').count();
		let (stroke, length) = match dots {
			0 => (Stroke::Normal, line.chars().count().saturating_sub(1)),
			dots => (Stroke::Dotted, dots),
		};
		let start_stroke = start.map(|start| match start.contains('.') {
			true => Stroke::Dotted,
			false => Stroke::Normal,
		});
		if start_stroke.is_some_and(|start_stroke| start_stroke != stroke) {
			return Link {
				kind: "INVALID".to_owned(),
				stroke: Stroke::Invalid,
				length,
				text: None,
				id: None,
			};
		}
		Link { kind: kind.to_owned(), stroke, length, text: None, id: None }
	}

	/// Unlike the flowchart's, rejects the indices of missing edges.
	fn update_link_interpolate(
		&mut self,
		positions: &[Option<usize>],
		interpolate: &str,
	) -> Result<(), String> {
		for position in positions {
			match position {
				None => self.flow.default_interpolate = Some(interpolate.to_owned()),
				Some(index) => match self.flow.edges.get_mut(*index) {
					Some(edge) => edge.interpolate = Some(interpolate.to_owned()),
					None => return Err(self.flow.bounds_error(*index)),
				},
			}
		}
		Ok(())
	}

	fn update_link(
		&mut self,
		positions: &[Option<usize>],
		styles: &[String],
	) -> Result<(), String> {
		self.flow.update_link(positions, styles)
	}

	fn add_class(&mut self, ids: &str, styles: &[String]) {
		self.flow.add_class(ids, styles);
	}

	fn set_direction(&mut self, direction: &str) {
		self.flow.set_direction(direction);
	}

	fn set_class(&mut self, ids: &str, class: &str) {
		self.flow.set_class(ids, class);
	}

	fn set_tooltip(&mut self, ids: &str, tooltip: &str) {
		self.flow.set_tooltip(ids, tooltip);
	}

	fn set_click_event(&mut self, ids: &str) {
		self.flow.set_click_event(ids);
	}

	fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
		self.flow.set_link(ids, link, target);
	}

	/// A `flow` container. Unlike a subgraph, a container declared again takes the new title,
	/// direction and members.
	fn add_subgraph(
		&mut self,
		id: Option<&FlowText>,
		members: &[Member],
		title: Option<&FlowText>,
		_title_is_id: bool,
	) -> String {
		let (mut dir, nodes) = collect_members(members);
		if dir.is_none() && self.inherit_dir {
			dir = self.flow.direction.clone();
		}
		let id = self.flow.subgraph_id(id.map(|id| id.text.trim().to_owned()));
		let title_text = title.map(|title| title.text.trim().to_owned()).unwrap_or_default();
		let label_type = title.map_or(LabelType::Markdown, |title| title.label_type);
		// A container is never its own member, globally scoped nodes are nobody's, and nodes
		// already in a container stay there (`makeUniq`).
		let subgraphs = &mut self.flow.subgraphs;
		let nodes: Vec<String> = nodes
			.into_iter()
			.filter(|node| *node != id && !self.global_nodes.contains(node))
			.filter(|node| !subgraphs.iter().any(|subgraph| subgraph.nodes.contains(node)))
			.collect();
		let index = match subgraphs.iter().position(|subgraph| subgraph.id == id) {
			Some(index) => {
				let existing = &mut subgraphs[index];
				for node in nodes {
					if !existing.nodes.contains(&node) {
						existing.nodes.push(node);
					}
				}
				if !title_text.is_empty() {
					existing.title = title_text;
				}
				if dir.is_some() {
					existing.dir = dir;
				}
				index
			}
			None => {
				subgraphs.push(SubGraph {
					id: id.clone(),
					nodes,
					title: title_text,
					classes: Vec::new(),
					dir,
					label_type,
					metadata: None,
				});
				subgraphs.len() - 1
			}
		};
		self.flow.index_subgraph(&id, index);
		id
	}

	fn add_connector(&mut self, id: &FlowText, title: &FlowText) -> String {
		let id = id.text.trim();
		if id.is_empty() {
			return String::new();
		}
		let title = title.text.trim();
		let text = if title.is_empty() { id } else { title };
		if self.is_connector(id) {
			if let Some(vertex) = self.flow.vertex_mut(id).filter(|_| !title.is_empty()) {
				vertex.text = Some(text.to_owned());
			}
			return id.to_owned();
		}
		self.flow.set_vertex(Vertex {
			id: id.to_owned(),
			dom_id: self.flow.next_dom_id(id),
			text: Some(text.to_owned()),
			label_type: LabelType::Text,
			is_connector: true,
			..Vertex::default()
		});
		id.to_owned()
	}

	fn add_global(&mut self, members: &[Member]) {
		for member in members {
			if let Member::Id(id) = member {
				let id = id.trim();
				if !id.is_empty() {
					self.global_nodes.insert(id.to_owned());
				}
			}
		}
		let global_nodes = &self.global_nodes;
		for subgraph in &mut self.flow.subgraphs {
			subgraph.nodes.retain(|node| !global_nodes.contains(node));
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn strips_trailing_commas_of_block_metadata() {
		let body = " shape: tool,\n params: \"a, b\",\n note: |\n   one,\n two: [1, 2], # c,\n";
		assert_eq!(
			strip_line_trailing_commas(body),
			" shape: tool\n params: \"a, b\"\n note: |\n   one,\n two: [1, 2] # c,\n"
		);
		assert!(parse_metadata("\n shape: task,\n label: Hi,\n").is_some());
		assert_eq!(parse_metadata("\n"), Some(Value::Object(Vec::new())));
	}

	#[test]
	fn destructs_agentflow_links() {
		let db = AgentflowDb::new(500, false);
		let link = db.destruct_link("-->", None);
		assert_eq!(
			(link.kind.as_str(), link.stroke, link.length),
			("arrow_point", Stroke::Normal, 1)
		);
		let link = db.destruct_link(" --x ", None);
		assert_eq!((link.kind.as_str(), link.stroke), ("arrow_cross", Stroke::Normal));
		let link = db.destruct_link("-..-", None);
		assert_eq!(
			(link.kind.as_str(), link.stroke, link.length),
			("arrow_open", Stroke::Dotted, 2)
		);
		let link = db.destruct_link("--->", Some("-- "));
		assert_eq!((link.kind.as_str(), link.length), ("arrow_point", 2));
	}
}
