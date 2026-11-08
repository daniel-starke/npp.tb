//! Agentflow diagrams (`agentflow.jison`, `agentflowDb.ts`, `shapes.ts`, `colorSlots.ts`,
//! `renderer.ts`, `styles.ts`), laid out with dagre. The grammar is the flowchart's with `flow`,
//! `connector` and `global` blocks, so the flowchart parser reads it in its agentflow dialect.

pub(super) mod db;

use std::collections::{HashMap, HashSet};

use super::common::CommonFields;
use super::flowchart::db::{self as flow_db, LabelType};
use super::flowchart::{self, parser};
use super::label::encode_entities;
use super::rendering::{self, LayoutData, Node};
use super::value::Value;
use super::{Context, Diagram, Result};
use db::{AgentflowDb, resolve_shape_alias};

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	// `Diagram.ts` and `agentflowParser.ts`: entities, a final line end and no blanks between a
	// closing `}` and the line end.
	let code = strip_blanks_after_braces(&(encode_entities(code) + "\n"));
	let inherit_dir = context.config_bool("flowchart", "inheritDir", false);
	let mut db = AgentflowDb::new(context.limits.max_edges, inherit_dir);
	let mut common = CommonFields::new(context);
	parser::parse(&code, parser::Database::Agentflow(&mut db), &mut common)
		.map_err(|error| context.error_at(error.line, error.message))?;
	let data = layout_data(&db, context);
	check_renderable(&data)?;
	let (body, bbox) = rendering::render(context, &data)?;
	Ok(flowchart::document(context, body, bbox, &common, "agentflow", "agentflow"))
}

/// The graphs Mermaid's renderer fails on: an edge to the reserved id `connectors` ends at no
/// node, and dagre's wrapper draws a flow container without children as a node, whose shapes
/// have no `flowGroup`.
fn check_renderable(data: &LayoutData) -> Result<()> {
	let is_node = |id: &str| data.nodes.iter().any(|node| node.id == id);
	if data.edges.iter().any(|edge| !is_node(&edge.start) || !is_node(&edge.end)) {
		return Err(super::Error::new("Cannot read properties of undefined (reading 'shape')"));
	}
	let has_children =
		|id: &str| data.nodes.iter().any(|node| node.parent_id.as_deref() == Some(id));
	if data.nodes.iter().any(|node| node.shape == "flowGroup" && !has_children(&node.id)) {
		return Err(super::Error::new("No such shape: flowGroup. Please check your syntax."));
	}
	Ok(())
}

/// `src.replace(/}[^\S\n]*\n/g, '}\n')`.
fn strip_blanks_after_braces(code: &str) -> String {
	let mut output = String::with_capacity(code.len());
	let mut rest = code;
	while let Some(index) = rest.find('}') {
		output.push_str(&rest[..=index]);
		rest = &rest[index + 1..];
		let blanks = rest.find(|c: char| !c.is_whitespace() || c == '\n').unwrap_or(rest.len());
		if rest[blanks..].starts_with('\n') {
			rest = &rest[blanks..];
		}
	}
	output.push_str(rest);
	output
}

/// The shapes v0.8.1 removed (`REMOVED_SHAPES`) and the ones it allows (`ALLOWED_SHAPES`); all
/// others are drawn as the default `roundedRect`.
const ALLOWED_SHAPES: [&str; 13] = [
	"roundedRect",
	"subroutine",
	"subprocess",
	"subproc",
	"framed-rectangle",
	"lean-right",
	"diamond",
	"lin-doc",
	"lined-document",
	"hexagon",
	"hex",
	"connector",
	"collapsedGroup",
];

/// `getTypeFromVertex` and `normaliseNodeShapes`: the drawn shape of a vertex.
fn shape_of(vertex: &flow_db::Vertex) -> String {
	let resolved = vertex.kind.as_deref().map(resolve_shape_alias);
	match resolved {
		Some(shape) if ALLOWED_SHAPES.contains(&shape) => shape.to_owned(),
		_ => "roundedRect".to_owned(),
	}
}

/// `KIND_SLOT`: the palette slot of every vertex kind.
const KIND_SLOTS: [&str; 7] =
	["tool", "task", "decision", "input", "refdoc", "connector", "action"];

/// `deriveVertexKind`: what a vertex is, which picks its palette slot.
fn kind_of(vertex: &flow_db::Vertex) -> &'static str {
	if vertex.is_connector {
		return "connector";
	}
	let kind = vertex.kind.as_deref();
	if matches!(kind, Some("subroutine" | "subprocess" | "subproc" | "framed-rectangle" | "tool")) {
		return "tool";
	}
	match kind.map(resolve_shape_alias) {
		Some("hexagon" | "hex") => "action",
		Some("lean-right" | "lean_right") => "input",
		Some("lin-doc" | "lined-document") => "refdoc",
		Some("diamond") => "decision",
		_ => "task",
	}
}

/// `containerSlot`: containers cycle the palette slots above the ones of the kinds.
fn container_slot(index: usize, palette_length: usize) -> usize {
	let kinds = KIND_SLOTS.len();
	let slot = kinds + index % palette_length.saturating_sub(kinds).max(1);
	if palette_length > 0 { slot % palette_length } else { slot }
}

/// `destructEdgeType`.
fn arrow_types(kind: Option<&str>) -> (String, String) {
	let kind = kind.unwrap_or_default();
	match kind {
		"arrow_point" | "arrow_circle" | "arrow_cross" | "arrow_hierarchy" => {
			("none".to_owned(), kind.to_owned())
		}
		"double_arrow_point" | "double_arrow_circle" | "double_arrow_cross" => {
			let arrow = kind.replacen("double_", "", 1);
			(arrow.clone(), arrow)
		}
		_ => ("none".to_owned(), "arrow_point".to_owned()),
	}
}

/// The members a collapsed container hides, each with the outermost collapsed container that
/// stands for it (`collectDescendants`). Containment may be cyclic, so every container is
/// visited once.
fn collapsed_members(db: &AgentflowDb) -> HashMap<&str, &str> {
	let mut ancestor_of: HashMap<&str, &str> = HashMap::new();
	for subgraph in &db.flow.subgraphs {
		let is_collapsed = subgraph.metadata.as_ref().and_then(|metadata| metadata.get("view"));
		if is_collapsed.and_then(Value::as_str) != Some("collapsed")
			|| ancestor_of.contains_key(subgraph.id.as_str())
		{
			continue;
		}
		let ancestor = subgraph.id.as_str();
		let mut seen: HashSet<&str> = HashSet::from([ancestor]);
		// The recursion of `collectDescendants` as a stack of containers and their next child.
		let mut stack: Vec<(&flow_db::SubGraph, usize)> = vec![(subgraph, 0)];
		while let Some((container, index)) = stack.pop() {
			let Some(child) = container.nodes.get(index) else { continue };
			stack.push((container, index + 1));
			let child = child.as_str();
			if child == ancestor || seen.contains(child) {
				continue;
			}
			ancestor_of.insert(child, ancestor);
			if seen.insert(child)
				&& let Some(nested) = db.flow.subgraph(child)
			{
				stack.push((nested, 0));
			}
		}
	}
	ancestor_of
}

/// `getData`: the nodes, clusters and edges of the database for the layout.
fn layout_data(agentflow: &AgentflowDb, context: &Context) -> LayoutData {
	let db = &agentflow.flow;
	let look = context.look;
	let padding = context.config_number("flowchart", "padding", 15.0);
	let padding = if padding == 0.0 || padding.is_nan() { 8.0 } else { padding };
	let wrapping_width = Some(context.config_number("agentflow", "wrappingWidth", 120.0));
	let min_width = Some(context.config_number("agentflow", "minNodeWidth", 120.0));
	let subgraphs = &db.subgraphs;
	let hidden = collapsed_members(agentflow);
	let is_hidden = |id: &str| hidden.contains_key(id);
	// Parents: inner containers first, a nesting that would close a containment cycle dropped.
	let mut parent_of: HashMap<&str, &str> = HashMap::new();
	let mut is_group: HashSet<&str> = HashSet::new();
	for subgraph in subgraphs.iter().rev().filter(|subgraph| !is_hidden(&subgraph.id)) {
		if !subgraph.nodes.is_empty() {
			is_group.insert(&subgraph.id);
		}
		for id in &subgraph.nodes {
			let mut seen = HashSet::new();
			let mut current = Some(subgraph.id.as_str());
			let mut closes_cycle = false;
			while let Some(container) = current.filter(|container| seen.insert(*container)) {
				if container == id {
					closes_cycle = true;
					break;
				}
				current = parent_of.get(container).copied();
			}
			if !closes_cycle {
				parent_of.insert(id, &subgraph.id);
			}
		}
	}
	let is_collapsed = |subgraph: &flow_db::SubGraph| {
		let view = subgraph.metadata.as_ref().and_then(|metadata| metadata.get("view"));
		view.and_then(Value::as_str) == Some("collapsed")
	};
	let mut nodes: Vec<Node> = Vec::new();
	for subgraph in subgraphs.iter().rev().filter(|subgraph| !is_hidden(&subgraph.id)) {
		let collapsed = is_collapsed(subgraph);
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
			is_group: !collapsed,
			shape: if collapsed { "collapsedGroup" } else { "flowGroup" }.to_owned(),
			..Node::default()
		});
	}
	for vertex in db.vertices.iter().filter(|vertex| !is_hidden(&vertex.id)) {
		if flowchart::style_subgraph_node(db, &mut nodes, vertex) {
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
			look,
			link: vertex.link.clone(),
			link_target: vertex.link_target.clone(),
			tooltip: db.tooltip(&vertex.id).map(str::to_owned),
			is_group: group,
			shape: if group { "rect".to_owned() } else { shape_of(vertex) },
			min_width: if group { None } else { min_width },
			wrapping_width: if group { None } else { wrapping_width },
			..Node::default()
		});
	}
	assign_color_slots(db, context, &parent_of, &mut nodes);
	let default_curve = context.config_text("flowchart", "curve");
	let edges = db
		.edges
		.iter()
		.filter_map(|raw| {
			let start = hidden.get(raw.start.as_str()).copied().unwrap_or(&raw.start);
			let end = hidden.get(raw.end.as_str()).copied().unwrap_or(&raw.end);
			// Collapsing makes no self loops; the ones of the source stay.
			if start == end && raw.start != raw.end {
				return None;
			}
			let arrows = arrow_types(raw.kind.as_deref());
			Some(flowchart::layout_edge(
				db,
				raw,
				(start, end),
				arrows,
				default_curve.as_ref(),
				look,
			))
		})
		.collect();
	LayoutData {
		nodes,
		edges,
		direction: db.direction.clone().unwrap_or_else(|| "TB".to_owned()),
		node_spacing: positive_or(context.config_number("agentflow", "nodeSpacing", 50.0), 50.0),
		rank_spacing: positive_or(context.config_number("agentflow", "rankSpacing", 50.0), 50.0),
		diagram_type: "agentflow",
		config_section: "flowchart",
		style: rendering::DiagramStyle::Agentflow,
		markers: &["point", "circle", "cross"],
	}
}

/// JavaScript's `value || fallback` of a number.
fn positive_or(value: f64, fallback: f64) -> f64 {
	if value == 0.0 || value.is_nan() { fallback } else { value }
}

/// `assignColorSlots`: containers cycle palette slots in declaration order, the other nodes
/// take the slot of their kind.
fn assign_color_slots(
	db: &flow_db::FlowDb,
	context: &Context,
	parent_of: &HashMap<&str, &str>,
	nodes: &mut [Node],
) {
	let order = flowchart::declaration_order(&db.subgraphs, parent_of);
	let palette_length = match context.theme.get("borderColorArray") {
		Some(Value::Array(colors)) => colors.len(),
		_ => 0,
	};
	let mut fallback = order.len();
	for node in nodes {
		if node.is_group || node.shape == "collapsedGroup" {
			let index = order.get(node.id.as_str()).copied().unwrap_or_else(|| {
				fallback += 1;
				fallback - 1
			});
			node.color_index = Some(container_slot(index, palette_length));
			continue;
		}
		let Some(vertex) = db.vertex(&node.id) else { continue };
		let kind = kind_of(vertex);
		node.kind_slot = KIND_SLOTS.iter().position(|slot| *slot == kind);
		node.css_classes = format!("{} af-kind-{kind}", node.css_classes.trim()).trim().to_owned();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn strips_blanks_after_braces() {
		assert_eq!(strip_blanks_after_braces("a@{ x }  \n\nb}\t\nc} d"), "a@{ x }\n\nb}\nc} d");
	}

	#[test]
	fn assigns_shapes_and_kinds_like_agentflow() {
		let vertex = |kind: Option<&str>| flow_db::Vertex {
			kind: kind.map(str::to_owned),
			..flow_db::Vertex::default()
		};
		assert_eq!(shape_of(&vertex(None)), "roundedRect");
		assert_eq!(shape_of(&vertex(Some("subroutine"))), "subroutine");
		assert_eq!(shape_of(&vertex(Some("lean_right"))), "roundedRect");
		assert_eq!(shape_of(&vertex(Some("cylinder"))), "roundedRect");
		assert_eq!(shape_of(&vertex(Some("lin-doc"))), "lin-doc");
		assert_eq!(kind_of(&vertex(Some("lean_right"))), "input");
		assert_eq!(kind_of(&vertex(Some("subroutine"))), "tool");
		assert_eq!(kind_of(&vertex(Some("square"))), "task");
		assert_eq!(container_slot(0, 12), 7);
		assert_eq!(container_slot(5, 12), 7);
		assert_eq!(container_slot(1, 3), 1);
		assert_eq!(container_slot(2, 0), 7);
	}
}
