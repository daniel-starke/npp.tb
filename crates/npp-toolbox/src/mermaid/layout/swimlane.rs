//! Mermaid's swimlane layout (`rendering-util/layout-algorithms/swimlanes/`: `layoutCore.ts`,
//! `pipeline.ts`, `helpers.ts`, `phase0.helpers.ts`, `phase1.cycles.ts`, `config.ts`): a
//! Sugiyama layering where every top-level group is a lane, followed by the orthogonal edge
//! router and its post-processing passes.
//!
//! Ids are ordered like Mermaid orders them: `localeCompare` where it uses it
//! (`common::locale_compare`), code units where it uses `<` or a plain `sort()`.

mod coordinates;
mod detour;
mod endpoints;
mod geometry;
mod labels;
mod layering;
mod materialized;
mod nudging;
mod ordering;
mod post;
mod router;

use std::collections::HashMap;

pub use super::dagre::Point;

/// `Array.prototype.sort` with a comparator that returns an ordering.
fn js_sort<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
	super::super::common::v8_sort(items, |a, b| compare(a, b) as i8 as f64);
}

/// `PRECISION.EPSILON`.
const EPSILON: f64 = 1e-6;

/// Layout direction; `BT` and `RL` lay out like `TB` (Mermaid's swimlanes only special-case `LR`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
	#[default]
	TopBottom,
	BottomTop,
	LeftRight,
	RightLeft,
}

impl Direction {
	pub fn parse(text: &str) -> Self {
		match text {
			"LR" => Self::LeftRight,
			"RL" => Self::RightLeft,
			"BT" => Self::BottomTop,
			_ => Self::TopBottom,
		}
	}

	pub fn is_left_right(self) -> bool {
		self == Self::LeftRight
	}

	fn is_horizontal(self) -> bool {
		matches!(self, Self::LeftRight | Self::RightLeft)
	}
}

/// The title band of a top-level lane (`groupTitleRect`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleRect {
	pub left: f64,
	pub right: f64,
	pub top: f64,
	pub bottom: f64,
}

/// A node of Mermaid's `LayoutData` as far as the layout reads and writes it.
#[derive(Clone, Debug, Default)]
pub struct Node {
	pub id: String,
	pub parent_id: Option<String>,
	pub is_group: bool,
	pub shape: String,
	pub width: Option<f64>,
	pub height: Option<f64>,
	pub x: Option<f64>,
	pub y: Option<f64>,
	pub padding: Option<f64>,
	/// The node of an edge label (`edge-label-…`).
	pub is_edge_label: bool,
	pub layer: Option<usize>,
	pub order: Option<usize>,
	/// Top of the content of all lanes (`swimlaneContentTop`).
	pub content_top: Option<f64>,
	pub title_rect: Option<TitleRect>,
}

/// An edge of Mermaid's `LayoutData` as far as the layout reads and writes it.
#[derive(Clone, Debug, Default)]
pub struct Edge {
	pub id: String,
	pub start: String,
	pub end: String,
	/// The label node the edge runs through (`labelNodeId`).
	pub label_node_id: Option<String>,
	/// Layering-only edges from and to label nodes (`isLayoutOnly`).
	pub is_layout_only: bool,
	pub points: Option<Vec<Point>>,
	pub curve: Option<String>,
}

/// The settings `runSwimlaneLayoutCore` reads from the configuration.
#[derive(Clone, Copy, Debug)]
pub struct Config {
	/// `flowchart.nodeSpacing`.
	pub node_gap: f64,
	/// `flowchart.rankSpacing`.
	pub layer_gap: f64,
	pub ignore_cross_lane_edges: bool,
	pub optimize_ranks_by_crossings: bool,
	pub automatic_lane_ordering: bool,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			node_gap: 40.0,
			layer_gap: 100.0,
			ignore_cross_lane_edges: true,
			optimize_ranks_by_crossings: true,
			automatic_lane_ordering: false,
		}
	}
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
	pub nodes: Vec<Node>,
	pub edges: Vec<Edge>,
	pub direction: Direction,
	pub config: Config,
}

/// `runSwimlaneLayoutCore`: positions the nodes and lanes and routes the edges. Layout-only
/// edges are removed afterwards.
pub fn run(layout: &mut Layout) {
	let graph = Graph::of_layout(layout);
	let config = layout.config;
	let (ordered, coordinates) = sugiyama_layout(&graph, layout, config);
	write_back(&graph, layout, &ordered, &coordinates);
	for edge in &mut layout.edges {
		edge.points = None;
	}
	router::route_edges_orthogonal(layout);
	for edge in &mut layout.edges {
		if edge.curve.as_deref().is_none_or(|curve| curve == "basis") {
			edge.curve = Some("rounded".to_string());
		}
	}
	post::post_process(layout);
}

/// Mermaid's `nodeByIdMap` of the post-processing: the last node of every id, iterated in
/// the order the ids first appear.
pub(super) struct NodeMap {
	pub order: Vec<usize>,
	pub by_id: HashMap<String, usize>,
}

impl NodeMap {
	fn of(nodes: &[Node]) -> Self {
		let mut by_id: HashMap<String, usize> = HashMap::new();
		let mut first: Vec<&str> = Vec::new();
		for (index, node) in nodes.iter().enumerate() {
			if by_id.insert(node.id.clone(), index).is_none() {
				first.push(&node.id);
			}
		}
		let order = first.iter().map(|id| by_id[*id]).collect();
		Self { order, by_id }
	}
}

/// Node of the layering graph: a layout node or a dummy of a long edge.
#[derive(Clone, Debug)]
struct GraphNode {
	id: String,
	parent_id: Option<String>,
	is_group: bool,
	is_dummy: bool,
	width: f64,
	height: f64,
}

/// `EdgeRef`: endpoints are indices into `Graph::arena`.
#[derive(Clone, Debug)]
struct EdgeRef {
	id: String,
	source: usize,
	target: usize,
	/// Index of the layout edge.
	reference: usize,
}

/// Mermaid's `Graph` view after `normalizeGraph`.
#[derive(Clone, Debug)]
struct Graph {
	arena: Vec<GraphNode>,
	by_id: HashMap<String, usize>,
	/// Node order: `nodeById` insertion order.
	nodes: Vec<usize>,
	edges: Vec<EdgeRef>,
	/// Top-level lane of every arena node (`createTopLaneResolver`).
	lane_of: Vec<Option<usize>>,
	lane_names: Vec<String>,
	/// Top-level groups in reversed source order (`buildTopLaneOrder`).
	source_lane_order: Vec<usize>,
}

impl Graph {
	/// `toGraphView` followed by `normalizeGraph`: edges with unknown endpoints and labelled
	/// edges (laid out through their label node) are left out.
	fn of_layout(layout: &Layout) -> Self {
		let mut graph = Graph {
			arena: Vec::with_capacity(layout.nodes.len()),
			by_id: HashMap::with_capacity(layout.nodes.len()),
			nodes: Vec::with_capacity(layout.nodes.len()),
			edges: Vec::new(),
			lane_of: Vec::new(),
			lane_names: Vec::new(),
			source_lane_order: Vec::new(),
		};
		for node in &layout.nodes {
			let graph_node = GraphNode {
				id: node.id.clone(),
				parent_id: node.parent_id.clone(),
				is_group: node.is_group,
				is_dummy: false,
				width: node.width.unwrap_or(0.0),
				height: node.height.unwrap_or(0.0),
			};
			match graph.by_id.get(&node.id) {
				// `Map.set` replaces the value and keeps the key's position.
				Some(&index) => graph.arena[index] = graph_node,
				None => {
					graph.by_id.insert(node.id.clone(), graph.arena.len());
					graph.nodes.push(graph.arena.len());
					graph.arena.push(graph_node);
				}
			}
		}
		let mut seen = std::collections::HashSet::new();
		for (reference, edge) in layout.edges.iter().enumerate() {
			if edge.label_node_id.is_some() {
				continue;
			}
			let (Some(&source), Some(&target)) =
				(graph.by_id.get(&edge.start), graph.by_id.get(&edge.end))
			else {
				continue;
			};
			if !seen.insert((edge.id.as_str(), source, target)) {
				continue;
			}
			graph.edges.push(EdgeRef { id: edge.id.clone(), source, target, reference });
		}
		graph.resolve_lanes();
		let mut lanes = Vec::new();
		for node in &layout.nodes {
			if node.is_group && node.parent_id.is_none() {
				let lane = graph.lane_index(&node.id);
				if !lanes.contains(&lane) {
					lanes.push(lane);
				}
			}
		}
		lanes.reverse();
		graph.source_lane_order = lanes;
		graph
	}

	fn lane_index(&mut self, name: &str) -> usize {
		match self.lane_names.iter().position(|lane| lane == name) {
			Some(index) => index,
			None => {
				self.lane_names.push(name.to_string());
				self.lane_names.len() - 1
			}
		}
	}

	/// `buildTopLaneMap`: the outermost ancestor of each node. A parent cycle has no lane
	/// (Mermaid would overflow its stack).
	fn resolve_lanes(&mut self) {
		self.lane_of = vec![None; self.arena.len()];
		for index in 0..self.arena.len() {
			let mut current = index;
			let mut outermost: Option<&str> = None;
			let mut steps = 0;
			while let Some(parent) = self.arena[current].parent_id.as_deref() {
				outermost = Some(parent);
				steps += 1;
				match self.by_id.get(parent) {
					Some(&next) if steps <= self.arena.len() => current = next,
					Some(_) => {
						outermost = None;
						break;
					}
					None => break,
				}
			}
			if let Some(name) = outermost.map(str::to_string) {
				self.lane_of[index] = Some(self.lane_index(&name));
			}
		}
	}

	fn id(&self, node: usize) -> &str {
		&self.arena[node].id
	}

	fn compare_ids(&self, a: usize, b: usize) -> std::cmp::Ordering {
		super::super::common::locale_compare(self.id(a), self.id(b))
	}

	fn incoming(&self, node: usize) -> impl Iterator<Item = &EdgeRef> {
		self.edges.iter().filter(move |edge| edge.target == node)
	}

	/// `buildSuccessorMap`, successors sorted with `localeCompare` (`buildSortedSuccessorMap`).
	fn sorted_successors(&self) -> Vec<Vec<usize>> {
		let mut successors = vec![Vec::new(); self.arena.len()];
		for edge in &self.edges {
			successors[edge.source].push(edge.target);
		}
		for list in &mut successors {
			npp::sort::sort_by(&mut list[..], |&a, &b| self.compare_ids(a, b));
		}
		successors
	}

	fn in_degrees(&self) -> Vec<usize> {
		let mut degrees = vec![0; self.arena.len()];
		for edge in &self.edges {
			degrees[edge.target] += 1;
		}
		degrees
	}

	/// `sortedZeroInDegreeNodes`: sources by id, unconnected nodes keep their source order.
	fn sorted_sources(&self, degrees: &[usize]) -> Vec<usize> {
		let mut sorted: Vec<usize> =
			self.nodes.iter().copied().filter(|&node| degrees[node] == 0).collect();
		npp::sort::sort_by(&mut sorted[..], |&a, &b| self.compare_ids(a, b));
		let mut linked = vec![false; self.arena.len()];
		for edge in &self.edges {
			linked[edge.source] = true;
			linked[edge.target] = true;
		}
		let is_unconnected = |node: usize| !linked[node] && !self.arena[node].is_group;
		let mut unconnected =
			self.nodes.iter().copied().filter(|&node| degrees[node] == 0 && is_unconnected(node));
		sorted
			.into_iter()
			.map(|node| match is_unconnected(node) {
				true => unconnected.next().unwrap_or(node),
				false => node,
			})
			.collect()
	}

	/// `topoSortIfAcyclic` (Kahn, ready nodes kept sorted by code units).
	fn topological_order(&self) -> Option<Vec<usize>> {
		let mut degrees = self.in_degrees();
		let mut queue = std::collections::VecDeque::from(self.sorted_sources(&degrees));
		let successors = self.sorted_successors();
		let mut order = Vec::with_capacity(self.nodes.len());
		while let Some(node) = queue.pop_front() {
			order.push(node);
			for &next in &successors[node] {
				degrees[next] -= 1;
				if degrees[next] == 0 {
					let position = queue
						.iter()
						.position(|&queued| self.id(queued) >= self.id(next))
						.unwrap_or(queue.len());
					queue.insert(position, next);
				}
			}
		}
		(order.len() == self.nodes.len()).then_some(order)
	}

	/// `[...g.nodes].sort()`: code unit order.
	fn nodes_by_code_units(&self) -> Vec<usize> {
		let mut nodes = self.nodes.clone();
		npp::sort::sort_by(&mut nodes[..], |&a, &b| self.id(a).cmp(self.id(b)));
		nodes
	}

	/// `buildLayersFromRanks`.
	fn layers_from_ranks(
		&self,
		order: &[usize],
		ranks: &[Option<i64>],
		skip_groups: bool,
	) -> Vec<Vec<usize>> {
		let skipped = |node: usize| skip_groups && self.arena[node].is_group;
		let max_rank = self
			.nodes
			.iter()
			.filter(|&&node| !skipped(node))
			.map(|&node| ranks[node].unwrap_or(0))
			.fold(0, i64::max);
		let mut layers = vec![Vec::new(); max_rank as usize + 1];
		for &node in order {
			if !skipped(node) {
				layers[ranks[node].unwrap_or(0).max(0) as usize].push(node);
			}
		}
		layers
	}

	/// `removeCycles_DFS`: back edges of a depth-first search from the sources (in node order,
	/// then the other nodes) are reversed.
	fn remove_cycles(&self) -> Graph {
		let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); self.arena.len()];
		for (index, edge) in self.edges.iter().enumerate() {
			adjacency[edge.source].push(index);
		}
		let compare_ids = super::super::common::locale_compare;
		for list in &mut adjacency {
			npp::sort::sort_by(&mut list[..], |&a, &b| {
				let (a, b) = (&self.edges[a], &self.edges[b]);
				match a.target == b.target {
					true => compare_ids(&a.id, &b.id),
					false => self.compare_ids(a.target, b.target),
				}
			});
		}
		#[derive(Clone, Copy, PartialEq)]
		enum Color {
			White,
			Gray,
			Black,
		}
		let mut color = vec![Color::White; self.arena.len()];
		let mut reversed = vec![false; self.edges.len()];
		let degrees = self.in_degrees();
		let seeds = self
			.nodes
			.iter()
			.filter(|&&node| degrees[node] == 0)
			.chain(self.nodes.iter().filter(|&&node| degrees[node] != 0));
		for &seed in seeds {
			if color[seed] != Color::White {
				continue;
			}
			// Explicit stack of (node, next adjacency position).
			let mut stack = vec![(seed, 0)];
			color[seed] = Color::Gray;
			while let Some(top) = stack.last_mut() {
				let (node, position) = *top;
				let Some(&edge) = adjacency[node].get(position) else {
					color[node] = Color::Black;
					stack.pop();
					continue;
				};
				top.1 += 1;
				let target = self.edges[edge].target;
				match color[target] {
					Color::White => {
						color[target] = Color::Gray;
						stack.push((target, 0));
					}
					Color::Gray => reversed[edge] = true,
					Color::Black => {}
				}
			}
		}
		let mut acyclic = self.clone();
		for (edge, &is_reversed) in acyclic.edges.iter_mut().zip(&reversed) {
			if is_reversed {
				std::mem::swap(&mut edge.source, &mut edge.target);
			}
		}
		acyclic
	}
}

/// `sugiyamaLayout`: the ordered layers and the coordinates of every arena node.
fn sugiyama_layout(
	graph: &Graph,
	layout: &Layout,
	config: Config,
) -> (Vec<Vec<usize>>, Coordinates) {
	let lane_order =
		config.automatic_lane_ordering.then(|| ordering::optimize_top_lane_order(graph, layout));
	let acyclic = graph.remove_cycles();
	let ranks = match config.ignore_cross_lane_edges {
		true => layering::lane_aware_compact(&acyclic, layout.direction),
		false => layering::gravity(&acyclic, config.optimize_ranks_by_crossings),
	};
	let (layers, with_dummies) = layering::make_proper(&ranks, &acyclic);
	let ordered = ordering::order_layers(layers, &with_dummies, lane_order.as_deref());
	let coordinates =
		coordinates::assign(&ordered, &with_dummies, layout, config, lane_order.as_deref());
	(ordered, coordinates)
}

/// Ranks of the arena nodes and their layers.
struct Layering {
	layers: Vec<Vec<usize>>,
	ranks: Vec<Option<i64>>,
}

/// Coordinates of the arena nodes.
struct Coordinates {
	x: Vec<Option<f64>>,
	y: Vec<Option<f64>>,
}

/// Captured swimlane fixtures use 21px as the stable single-line label height.
const TOP_LANE_TITLE_BAND_HEIGHT: f64 = 21.0;
const MIN_TOP_LANE_HORIZONTAL_PADDING: f64 = 20.0;

fn top_lane_horizontal_padding(lane: &Node) -> f64 {
	lane.padding.unwrap_or(MIN_TOP_LANE_HORIZONTAL_PADDING).max(MIN_TOP_LANE_HORIZONTAL_PADDING)
}

/// `assignTopLaneTitleRect`.
fn assign_top_lane_title_rect(lane: &mut Node) {
	lane.title_rect = None;
	let (Some(x), Some(y), Some(width), Some(height), Some(content_top)) =
		(lane.x, lane.y, lane.width, lane.height, lane.content_top)
	else {
		return;
	};
	if ![x, y, width, height, content_top].iter().all(|value| value.is_finite())
		|| width <= 0.0
		|| height <= 0.0
	{
		return;
	}
	let top = y - height / 2.0;
	let header_bottom = content_top.min(y + height / 2.0);
	let title_height = TOP_LANE_TITLE_BAND_HEIGHT.min((header_bottom - top).max(0.0));
	let bottom = top + title_height;
	if bottom > top {
		lane.title_rect =
			Some(TitleRect { left: x - width / 2.0, right: x + width / 2.0, top, bottom });
	}
}

/// `writeBackToLayoutData`: node positions, group boxes, and lanes of equal height whose
/// widths make neighbouring lanes touch.
fn write_back(
	graph: &Graph,
	layout: &mut Layout,
	ordered: &[Vec<usize>],
	coordinates: &Coordinates,
) {
	let gaps = (layout.config.layer_gap, layout.config.node_gap);
	for (layer_index, layer) in ordered.iter().enumerate() {
		for (order, &node) in layer.iter().enumerate() {
			// Dummies are no layout nodes.
			let Some(layout_index) = layout_node_index(graph, layout, node) else {
				continue;
			};
			let target = &mut layout.nodes[layout_index];
			target.layer = Some(layer_index);
			target.order = Some(order);
			target.x = Some(coordinates.x[node].unwrap_or(order as f64 * gaps.1));
			target.y = Some(coordinates.y[node].unwrap_or(layer_index as f64 * gaps.0));
		}
	}
	let coordinate_of =
		|id: &str, values: &Vec<Option<f64>>| graph.by_id.get(id).and_then(|&index| values[index]);
	let mut bounds: HashMap<String, (f64, f64, f64, f64)> = HashMap::new();
	let mut top_level = Vec::new();
	for group in 0..layout.nodes.len() {
		if !layout.nodes[group].is_group {
			continue;
		}
		if layout.nodes[group].parent_id.is_none() {
			top_level.push(group);
		}
		let (mut min_x, mut max_x, mut min_y, mut max_y) =
			(f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
		let group_id = layout.nodes[group].id.as_str();
		for child in layout.nodes.iter().filter(|node| node.parent_id.as_deref() == Some(group_id))
		{
			let center_x = child.x.or_else(|| coordinate_of(&child.id, &coordinates.x));
			let center_y = child.y.or_else(|| coordinate_of(&child.id, &coordinates.y));
			let (width, height) = (child.width.unwrap_or(0.0), child.height.unwrap_or(0.0));
			if let (Some(center_x), Some(center_y)) = (center_x, center_y) {
				min_x = min_x.min(center_x - width / 2.0);
				max_x = max_x.max(center_x + width / 2.0);
				min_y = min_y.min(center_y - height / 2.0);
				max_y = max_y.max(center_y + height / 2.0);
			}
		}
		let group = &mut layout.nodes[group];
		if min_x == f64::INFINITY || min_y == f64::INFINITY {
			group.x = group.x.or(Some(0.0));
			group.y = group.y.or(Some(0.0));
			group.width = group.width.or(Some(0.0));
			group.height = group.height.or(Some(0.0));
			continue;
		}
		let padding = group.padding.unwrap_or(20.0);
		let horizontal_padding = match group.parent_id {
			Some(_) => padding,
			None => 2.0 * top_lane_horizontal_padding(group),
		};
		group.x = Some((min_x + max_x) / 2.0);
		group.y = Some((min_y + max_y) / 2.0);
		group.width = Some((max_x - min_x).max(0.0) + horizontal_padding);
		group.height = Some((max_y - min_y).max(0.0) + padding);
		bounds.insert(group.id.clone(), (min_x, max_x, min_y, max_y));
	}
	let bounds_of = |layout: &Layout, lane: usize| bounds.get(&layout.nodes[lane].id).copied();
	if top_level.is_empty() || bounds.is_empty() {
		return;
	}
	let (mut global_min_y, mut global_max_y, mut max_padding) =
		(f64::INFINITY, f64::NEG_INFINITY, 0.0f64);
	for &lane in &top_level {
		max_padding = max_padding.max(layout.nodes[lane].padding.unwrap_or(20.0));
		if let Some((_, _, min_y, max_y)) = bounds_of(layout, lane) {
			global_min_y = global_min_y.min(min_y);
			global_max_y = global_max_y.max(max_y);
		}
	}
	if global_min_y == f64::INFINITY || global_max_y == f64::NEG_INFINITY {
		return;
	}
	const MIN_HEADER_MARGIN: f64 = 36.0;
	let vertical_margin = max_padding.max(MIN_HEADER_MARGIN);
	let lane_height = (global_max_y - global_min_y).max(0.0) + 2.0 * vertical_margin;
	let center_y = (global_min_y + global_max_y) / 2.0;
	for &lane in &top_level {
		let lane = &mut layout.nodes[lane];
		lane.y = Some(center_y);
		lane.height = Some(lane_height);
		lane.content_top = Some(global_min_y);
	}
	let mut sorted = top_level.clone();
	// A stable sort by `x` like `Array.prototype.sort`.
	npp::sort::sort_by(&mut sorted[..], |&a, &b| {
		let (a, b) = (layout.nodes[a].x.unwrap_or(0.0), layout.nodes[b].x.unwrap_or(0.0));
		a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
	});
	let mut lanes = Vec::new();
	let mut centers = Vec::new();
	let mut base_widths = Vec::new();
	for &lane in &sorted {
		let Some((min_x, max_x, _, _)) = bounds_of(layout, lane) else {
			continue;
		};
		lanes.push(lane);
		centers.push((min_x + max_x) / 2.0);
		base_widths.push(
			(max_x - min_x).max(0.0) + 2.0 * top_lane_horizontal_padding(&layout.nodes[lane]),
		);
	}
	let count = lanes.len();
	if count == 0 {
		return;
	}
	let mut widths = base_widths.clone();
	if count > 1 {
		// Half widths `u` alternate around the centre distances so neighbours touch.
		let mut u = vec![0.0; count];
		for index in 0..count - 1 {
			u[index + 1] = 2.0 * (centers[index + 1] - centers[index]) - u[index];
		}
		let (mut lower, mut upper) = (0.0f64, f64::INFINITY);
		for index in 0..count {
			match index % 2 {
				0 => lower = lower.max(base_widths[index] - u[index]),
				_ => upper = upper.min(u[index] - base_widths[index]),
			}
		}
		let shift = match lower <= upper {
			true => (lower + upper) / 2.0,
			false => lower,
		};
		for index in 0..count {
			let width = u[index] + if index % 2 == 0 { shift } else { -shift };
			widths[index] = base_widths[index].max(width);
		}
	}
	let width_of: HashMap<String, f64> = lanes
		.iter()
		.zip(&widths)
		.map(|(&lane, &width)| (layout.nodes[lane].id.clone(), width))
		.collect();
	for &lane in &top_level {
		let lane = &mut layout.nodes[lane];
		if let Some(&width) = width_of.get(&lane.id) {
			lane.width = Some(width);
		}
		assign_top_lane_title_rect(lane);
	}
}

/// The layout node of an arena node: `nodeById` of the graph view holds the last node of an id.
fn layout_node_index(graph: &Graph, layout: &Layout, node: usize) -> Option<usize> {
	// Dummies are appended to the arena after the layout nodes.
	if graph.arena.get(node).is_none_or(|node| node.is_dummy) {
		return None;
	}
	let id = graph.id(node);
	layout.nodes.iter().rposition(|candidate| candidate.id == id)
}
