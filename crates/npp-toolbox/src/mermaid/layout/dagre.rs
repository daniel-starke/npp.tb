//! dagre (dagre-d3-es 7.0.14), the layered layout of flowcharts, class, state and ER diagrams.
//!
//! The port keeps dagre's algorithms, its tie breaking and the iteration orders of its JavaScript
//! objects, so that diagrams come out like Mermaid's. Deep recursions over node chains use
//! explicit stacks; recursions over the cluster hierarchy are bounded by the nesting depth.

mod order;
mod position;
mod rank;

use super::graph::{Edge, Graph, GraphLabel, Key, KeyMap, Keys, RankDir};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
	pub x: f64,
	pub y: f64,
}

/// `labelpos` of an edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelPos {
	Left,
	Center,
	#[default]
	Right,
}

impl LabelPos {
	/// dagre's `labelpos`: `l`, `c` or `r` (the default).
	pub fn parse(text: &str) -> LabelPos {
		match text {
			"l" => LabelPos::Left,
			"c" => LabelPos::Center,
			_ => LabelPos::Right,
		}
	}
}

/// The edge attributes dagre reads.
#[derive(Clone, Copy, Debug)]
pub struct EdgeAttributes {
	pub minlen: f64,
	pub weight: f64,
	pub width: f64,
	pub height: f64,
	pub labeloffset: f64,
	pub labelpos: LabelPos,
}

impl Default for EdgeAttributes {
	fn default() -> Self {
		Self {
			minlen: 1.0,
			weight: 1.0,
			width: 0.0,
			height: 0.0,
			labeloffset: 10.0,
			labelpos: LabelPos::Right,
		}
	}
}

/// A node of the graph to lay out.
pub trait LayoutNode {
	fn size(&self) -> (f64, f64);
	fn set_position(&mut self, x: f64, y: f64);
	/// The size of a cluster, around its children.
	fn set_size(&mut self, width: f64, height: f64);
}

/// An edge of the graph to lay out.
pub trait LayoutEdge {
	fn attributes(&self) -> EdgeAttributes;
	/// The route and the center of the label, if the edge has one.
	fn set_layout(&mut self, points: Vec<Point>, label: Option<Point>);
}

/// Lays out `graph`: positions of its nodes, sizes of its clusters, routes of its edges and the
/// size in `graph.label`. `keys` must hold the names of the graph's keys.
pub fn layout<N: LayoutNode, E: LayoutEdge>(graph: &mut Graph<N, E>, keys: &mut Keys) {
	let mut layout = Layout::build(graph, keys);
	layout.run();
	layout.update_input(graph);
}

/// The ranks of a laid out graph, for routers that work layer by layer. Every edge spans at
/// least two ranks (dagre doubles `minlen` to make room for labels); the points between the
/// ends of an edge lie on the ranks between the ranks of its ends, one point per rank.
#[derive(Default)]
pub struct Ranks {
	/// The rank of each node without children.
	pub nodes: KeyMap<i64>,
	/// The ranks of the top and bottom border of each cluster.
	pub clusters: KeyMap<(i64, i64)>,
}

/// [`layout`], which also returns the ranks of the nodes and clusters.
pub fn layout_ranked<N: LayoutNode, E: LayoutEdge>(
	graph: &mut Graph<N, E>,
	keys: &mut Keys,
) -> Ranks {
	let mut layout = Layout::build(graph, keys);
	layout.run();
	layout.update_input(graph);
	let mut ranks = Ranks::default();
	let as_rank = |rank: Option<f64>| rank.filter(|rank| rank.is_finite()).map(|rank| rank as i64);
	for v in graph.nodes() {
		let node = layout.node(v);
		if graph.has_children(v) {
			if let (Some(top), Some(bottom)) = (as_rank(node.min_rank), as_rank(node.max_rank)) {
				ranks.clusters.insert(v, (top, bottom));
			}
		} else if let Some(rank) = as_rank(node.rank) {
			ranks.nodes.insert(v, rank);
		}
	}
	ranks
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dummy {
	EdgeProxy,
	Edge,
	EdgeLabel,
	Border,
	Root,
	SelfEdge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BorderType {
	Left,
	Right,
}

#[derive(Clone, Debug, Default)]
struct NodeLabel {
	width: f64,
	height: f64,
	x: f64,
	y: f64,
	rank: Option<f64>,
	order: Option<usize>,
	dummy: Option<Dummy>,
	border_top: Option<Key>,
	border_bottom: Option<Key>,
	/// Border nodes by rank.
	border_left: Vec<Option<Key>>,
	border_right: Vec<Option<Key>>,
	min_rank: Option<f64>,
	max_rank: Option<f64>,
	/// The edge a dummy belongs to, and its label.
	edge_label: Option<usize>,
	edge_obj: Option<Edge>,
	labelpos: Option<LabelPos>,
	border_type: Option<BorderType>,
	self_edges: Vec<(Edge, usize)>,
	/// `e` of edge proxies and self edge dummies.
	edge: Option<Edge>,
	/// `label` of self edge dummies.
	label: Option<usize>,
}

#[derive(Clone, Debug, Default)]
struct EdgeLabel {
	minlen: f64,
	weight: f64,
	width: f64,
	height: f64,
	labeloffset: f64,
	labelpos: LabelPos,
	points: Option<Vec<Point>>,
	x: Option<f64>,
	y: Option<f64>,
	reversed: bool,
	forward_name: Option<Key>,
	label_rank: Option<f64>,
	nesting_edge: bool,
}

impl EdgeLabel {
	fn weighted(weight: f64, minlen: f64) -> Self {
		Self { weight, minlen, ..Self::default() }
	}
}

/// JavaScript's `Math.max`: NaN wins.
fn js_max(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

fn js_min(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// lodash's `_.min`: ignores NaN, `None` without numbers.
fn lodash_min(values: impl IntoIterator<Item = f64>) -> Option<f64> {
	values.into_iter().filter(|value| !value.is_nan()).fold(None, |min, value| match min {
		Some(min) if min <= value => Some(min),
		_ => Some(value),
	})
}

fn lodash_max(values: impl IntoIterator<Item = f64>) -> Option<f64> {
	values.into_iter().filter(|value| !value.is_nan()).fold(None, |max, value| match max {
		Some(max) if max >= value => Some(max),
		_ => Some(value),
	})
}

/// A stable merge sort that, unlike `slice::sort_by`, accepts comparators that are not total
/// orders, like JavaScript's `Array.prototype.sort`.
fn stable_sort_by<T: Clone>(items: &mut [T], mut less: impl FnMut(&T, &T) -> bool) {
	fn merge_sort<T: Clone>(
		items: &mut [T],
		buffer: &mut Vec<T>,
		less: &mut impl FnMut(&T, &T) -> bool,
	) {
		let length = items.len();
		if length <= 1 {
			return;
		}
		let middle = length / 2;
		merge_sort(&mut items[..middle], buffer, less);
		merge_sort(&mut items[middle..], buffer, less);
		buffer.clear();
		let (mut left, mut right) = (0, middle);
		while left < middle && right < length {
			// Take from the right only if it is strictly less, which keeps equal items in order.
			if less(&items[right], &items[left]) {
				buffer.push(items[right].clone());
				right += 1;
			} else {
				buffer.push(items[left].clone());
				left += 1;
			}
		}
		buffer.extend_from_slice(&items[left..middle]);
		buffer.extend_from_slice(&items[right..length]);
		items.clone_from_slice(buffer);
	}
	let mut buffer = Vec::with_capacity(items.len());
	merge_sort(items, &mut buffer, &mut less);
}

/// The layout graph of dagre with its node and edge labels. Graphs derived from it (the
/// non-compound views, layer graphs) share the labels by index like JavaScript shares objects.
struct Layout<'k> {
	keys: &'k mut Keys,
	g: Graph<usize, usize>,
	nodes: Vec<NodeLabel>,
	edges: Vec<EdgeLabel>,
	dummy_chains: Vec<Key>,
	nesting_root: Option<Key>,
	node_rank_factor: f64,
	/// Stand-ins for missing labels, so that broken states do not panic.
	missing_node: NodeLabel,
	missing_edge: EdgeLabel,
}

impl<'k> Layout<'k> {
	/// `buildLayoutGraph`.
	fn build<N: LayoutNode, E: LayoutEdge>(input: &Graph<N, E>, keys: &'k mut Keys) -> Self {
		let mut g = Graph::new(true, true);
		g.label = GraphLabel { width: 0.0, height: 0.0, ..input.label.clone() };
		let mut nodes = Vec::new();
		for v in input.nodes() {
			let (width, height) = input.node(v).map_or((0.0, 0.0), LayoutNode::size);
			g.set_node(v, nodes.len());
			nodes.push(NodeLabel { width, height, ..NodeLabel::default() });
			g.set_parent(v, input.parent(v));
		}
		let mut edges = Vec::new();
		for edge in input.edges() {
			let attributes = input.edge(edge).map(LayoutEdge::attributes).unwrap_or_default();
			g.set_edge(edge.v, edge.w, edge.name, edges.len());
			edges.push(EdgeLabel {
				minlen: attributes.minlen,
				weight: attributes.weight,
				width: attributes.width,
				height: attributes.height,
				labeloffset: attributes.labeloffset,
				labelpos: attributes.labelpos,
				..EdgeLabel::default()
			});
		}
		Self {
			keys,
			g,
			nodes,
			edges,
			dummy_chains: Vec::new(),
			nesting_root: None,
			node_rank_factor: 1.0,
			missing_node: NodeLabel::default(),
			missing_edge: EdgeLabel::default(),
		}
	}

	fn node(&self, v: Key) -> &NodeLabel {
		self.g.node(v).and_then(|&index| self.nodes.get(index)).unwrap_or(&self.missing_node)
	}

	fn node_mut(&mut self, v: Key) -> &mut NodeLabel {
		match self.g.node(v).copied() {
			Some(index) if index < self.nodes.len() => &mut self.nodes[index],
			_ => &mut self.missing_node,
		}
	}

	fn label_index(&self, edge: Edge) -> Option<usize> {
		self.g.edge(edge).copied().filter(|&index| index < self.edges.len())
	}

	fn edge(&self, edge: Edge) -> &EdgeLabel {
		match self.label_index(edge) {
			Some(index) => &self.edges[index],
			None => &self.missing_edge,
		}
	}

	fn edge_mut(&mut self, edge: Edge) -> &mut EdgeLabel {
		match self.label_index(edge) {
			Some(index) => &mut self.edges[index],
			None => &mut self.missing_edge,
		}
	}

	fn add_edge_label(&mut self, label: EdgeLabel) -> usize {
		self.edges.push(label);
		self.edges.len() - 1
	}

	/// `util.addDummyNode`.
	fn add_dummy_node(&mut self, dummy: Dummy, mut label: NodeLabel, prefix: &str) -> Key {
		let v = loop {
			let v = self.keys.unique(prefix);
			if !self.g.has_node(v) {
				break v;
			}
		};
		label.dummy = Some(dummy);
		self.g.set_node(v, self.nodes.len());
		self.nodes.push(label);
		v
	}

	fn run(&mut self) {
		self.make_space_for_edge_labels();
		self.remove_self_edges();
		self.acyclic();
		self.nesting_graph_run();
		rank::rank(self);
		self.inject_edge_label_proxies();
		self.remove_empty_ranks();
		self.nesting_graph_cleanup();
		self.normalize_ranks();
		self.assign_rank_min_max();
		self.remove_edge_label_proxies();
		self.normalize();
		self.parent_dummy_chains();
		self.add_border_segments();
		order::order(self);
		self.insert_self_edges();
		self.adjust_coordinate_system();
		position::position(self);
		self.position_self_edges();
		self.remove_border_nodes();
		self.undo_normalize();
		self.fixup_edge_label_coords();
		self.undo_coordinate_system();
		self.translate_graph();
		self.assign_node_intersects();
		self.reverse_points_for_reversed_edges();
		self.undo_acyclic();
	}

	/// `updateInputGraph`.
	fn update_input<N: LayoutNode, E: LayoutEdge>(&self, input: &mut Graph<N, E>) {
		for v in input.nodes() {
			let is_cluster = self.g.has_children(v);
			let layout = self.node(v);
			if let Some(node) = input.node_mut(v) {
				node.set_position(layout.x, layout.y);
				if is_cluster {
					node.set_size(layout.width, layout.height);
				}
			}
		}
		for edge in input.edges() {
			let layout = self.edge(edge);
			let label = layout.x.map(|x| Point { x, y: layout.y.unwrap_or(f64::NAN) });
			if let Some(input_edge) = input.edge_mut(edge) {
				input_edge.set_layout(layout.points.clone().unwrap_or_default(), label);
			}
		}
		input.label.width = self.g.label.width;
		input.label.height = self.g.label.height;
	}

	fn make_space_for_edge_labels(&mut self) {
		self.g.label.ranksep /= 2.0;
		let is_vertical = matches!(self.g.label.rankdir, RankDir::TopBottom | RankDir::BottomTop);
		for edge in self.g.edges() {
			let label = self.edge_mut(edge);
			label.minlen *= 2.0;
			if label.labelpos != LabelPos::Center {
				match is_vertical {
					true => label.width += label.labeloffset,
					false => label.height += label.labeloffset,
				}
			}
		}
	}

	fn remove_self_edges(&mut self) {
		for edge in self.g.edges() {
			if edge.v == edge.w {
				if let Some(label) = self.label_index(edge) {
					self.node_mut(edge.v).self_edges.push((edge, label));
				}
				self.g.remove_edge(edge);
			}
		}
	}

	/// `acyclic.run` with the depth-first feedback arc set.
	fn acyclic(&mut self) {
		let mut feedback = Vec::new();
		let mut visited = KeyMap::default();
		let mut on_stack = KeyMap::default();
		for root in self.g.nodes() {
			if visited.insert(root, ()).is_some() {
				continue;
			}
			on_stack.insert(root, ());
			let mut frames = vec![(root, self.g.out_edges(root), 0)];
			while let Some((v, edges, index)) = frames.last_mut() {
				let Some(&edge) = edges.get(*index) else {
					on_stack.remove(v);
					frames.pop();
					continue;
				};
				*index += 1;
				match on_stack.entry(edge.w) {
					std::collections::hash_map::Entry::Occupied(_) => feedback.push(edge),
					std::collections::hash_map::Entry::Vacant(slot) => {
						if visited.insert(edge.w, ()).is_none() {
							slot.insert(());
							frames.push((edge.w, self.g.out_edges(edge.w), 0));
						}
					}
				}
			}
		}
		for edge in feedback {
			let Some(label) = self.g.remove_edge(edge) else { continue };
			if let Some(stored) = self.edges.get_mut(label) {
				stored.forward_name = edge.name;
				stored.reversed = true;
			}
			let name = self.keys.unique("rev");
			self.g.set_edge(edge.w, edge.v, Some(name), label);
		}
	}

	fn undo_acyclic(&mut self) {
		for edge in self.g.edges() {
			let Some(index) = self.label_index(edge) else { continue };
			if !self.edges[index].reversed {
				continue;
			}
			self.g.remove_edge(edge);
			let label = &mut self.edges[index];
			let forward_name = label.forward_name.take();
			label.reversed = false;
			self.g.set_edge(edge.w, edge.v, forward_name, index);
		}
	}

	/// `nestingGraph.run`.
	fn nesting_graph_run(&mut self) {
		let root = self.add_dummy_node(Dummy::Root, NodeLabel::default(), "_root");
		let mut depths = KeyMap::default();
		let mut stack: Vec<(Key, usize)> =
			self.g.children(None).into_iter().rev().map(|v| (v, 1)).collect();
		while let Some((v, depth)) = stack.pop() {
			depths.insert(v, depth as f64);
			stack
				.extend(self.g.children(Some(v)).into_iter().rev().map(|child| (child, depth + 1)));
		}
		let height = depths.values().copied().fold(f64::NEG_INFINITY, f64::max) - 1.0;
		let node_sep = 2.0 * height + 1.0;
		self.nesting_root = Some(root);
		for edge in self.g.edges() {
			self.edge_mut(edge).minlen *= node_sep;
		}
		let weight = self.g.edges().iter().map(|&edge| self.edge(edge).weight).sum::<f64>() + 1.0;
		for child in self.g.children(None) {
			self.nesting_dfs(root, node_sep, weight, height, &depths, child);
		}
		self.node_rank_factor = node_sep;
	}

	fn nesting_dfs(
		&mut self,
		root: Key,
		node_sep: f64,
		weight: f64,
		height: f64,
		depths: &KeyMap<f64>,
		v: Key,
	) {
		let children = self.g.children(Some(v));
		if children.is_empty() {
			if v != root {
				let label = self.add_edge_label(EdgeLabel::weighted(0.0, node_sep));
				self.g.set_edge(root, v, None, label);
			}
			return;
		}
		let top = self.add_dummy_node(Dummy::Border, NodeLabel::default(), "_bt");
		let bottom = self.add_dummy_node(Dummy::Border, NodeLabel::default(), "_bb");
		self.g.set_parent(top, Some(v));
		self.node_mut(v).border_top = Some(top);
		self.g.set_parent(bottom, Some(v));
		self.node_mut(v).border_bottom = Some(bottom);
		let depth = depths.get(&v).copied().unwrap_or(f64::NAN);
		for child in children {
			self.nesting_dfs(root, node_sep, weight, height, depths, child);
			let child_node = self.node(child);
			let child_top = child_node.border_top.unwrap_or(child);
			let child_bottom = child_node.border_bottom.unwrap_or(child);
			let this_weight = if child_node.border_top.is_some() { weight } else { 2.0 * weight };
			let minlen = if child_top != child_bottom { 1.0 } else { height - depth + 1.0 };
			for (from, to) in [(top, child_top), (child_bottom, bottom)] {
				let label = self.add_edge_label(EdgeLabel {
					nesting_edge: true,
					..EdgeLabel::weighted(this_weight, minlen)
				});
				self.g.set_edge(from, to, None, label);
			}
		}
		if self.g.parent(v).is_none() {
			let label = self.add_edge_label(EdgeLabel::weighted(0.0, height + depth));
			self.g.set_edge(root, top, None, label);
		}
	}

	fn nesting_graph_cleanup(&mut self) {
		if let Some(root) = self.nesting_root.take() {
			self.g.remove_node(root);
		}
		for edge in self.g.edges() {
			if self.edge(edge).nesting_edge {
				self.g.remove_edge(edge);
			}
		}
	}

	fn inject_edge_label_proxies(&mut self) {
		for edge in self.g.edges() {
			let label = self.edge(edge);
			let has_size = label.width != 0.0
				&& !label.width.is_nan()
				&& label.height != 0.0
				&& !label.height.is_nan();
			if has_size {
				let v_rank = self.node(edge.v).rank.unwrap_or(f64::NAN);
				let w_rank = self.node(edge.w).rank.unwrap_or(f64::NAN);
				let proxy = NodeLabel {
					rank: Some((w_rank - v_rank) / 2.0 + v_rank),
					edge: Some(edge),
					..NodeLabel::default()
				};
				self.add_dummy_node(Dummy::EdgeProxy, proxy, "_ep");
			}
		}
	}

	/// Integer index of a rank like a JavaScript array index, `None` for other numbers.
	fn rank_index(rank: f64) -> Option<usize> {
		(rank >= 0.0 && rank.fract() == 0.0 && rank < 4e9).then_some(rank as usize)
	}

	fn remove_empty_ranks(&mut self) {
		let nodes = self.g.nodes();
		let Some(offset) = lodash_min(nodes.iter().filter_map(|&v| self.node(v).rank)) else {
			return;
		};
		let mut layers: Vec<Option<Vec<Key>>> = Vec::new();
		for &v in &nodes {
			let Some(index) = self.node(v).rank.and_then(|rank| Self::rank_index(rank - offset))
			else {
				continue;
			};
			if layers.len() <= index {
				layers.resize(index + 1, None);
			}
			layers[index].get_or_insert_with(Vec::new).push(v);
		}
		let mut delta = 0.0;
		let factor = self.node_rank_factor;
		for (index, layer) in layers.iter().enumerate() {
			match layer {
				None if (index as f64) % factor != 0.0 => delta -= 1.0,
				Some(layer) if delta != 0.0 => {
					for &v in layer {
						if let Some(rank) = &mut self.node_mut(v).rank {
							*rank += delta;
						}
					}
				}
				_ => {}
			}
		}
	}

	fn normalize_ranks(&mut self) {
		let nodes = self.g.nodes();
		let Some(min) = lodash_min(nodes.iter().filter_map(|&v| self.node(v).rank)) else {
			return;
		};
		for v in nodes {
			if let Some(rank) = &mut self.node_mut(v).rank {
				*rank -= min;
			}
		}
	}

	fn assign_rank_min_max(&mut self) {
		for v in self.g.nodes() {
			let node = self.node(v);
			if let (Some(top), Some(bottom)) = (node.border_top, node.border_bottom) {
				let min_rank = self.node(top).rank;
				let max_rank = self.node(bottom).rank;
				let node = self.node_mut(v);
				node.min_rank = Some(min_rank.unwrap_or(f64::NAN));
				node.max_rank = Some(max_rank.unwrap_or(f64::NAN));
			}
		}
	}

	fn remove_edge_label_proxies(&mut self) {
		for v in self.g.nodes() {
			let node = self.node(v);
			if node.dummy == Some(Dummy::EdgeProxy) {
				let (edge, rank) = (node.edge, node.rank);
				if let Some(edge) = edge {
					self.edge_mut(edge).label_rank = rank;
				}
				self.g.remove_node(v);
			}
		}
	}

	/// `normalize.run`: splits long edges into chains of dummies, one per rank.
	fn normalize(&mut self) {
		self.dummy_chains.clear();
		for edge in self.g.edges() {
			self.normalize_edge(edge);
		}
	}

	fn normalize_edge(&mut self, edge: Edge) {
		let mut v = edge.v;
		let mut v_rank = self.node(v).rank.unwrap_or(f64::NAN);
		let w = edge.w;
		let w_rank = self.node(w).rank.unwrap_or(f64::NAN);
		let Some(label_index) = self.label_index(edge) else { return };
		let label = &self.edges[label_index];
		let (label_rank, weight) = (label.label_rank, label.weight);
		if w_rank == v_rank + 1.0 {
			return;
		}
		self.g.remove_edge(edge);
		let mut first = true;
		v_rank += 1.0;
		while v_rank < w_rank {
			self.edges[label_index].points = Some(Vec::new());
			let mut attributes = NodeLabel {
				edge_label: Some(label_index),
				edge_obj: Some(edge),
				rank: Some(v_rank),
				..NodeLabel::default()
			};
			let mut dummy = Dummy::Edge;
			if Some(v_rank) == label_rank {
				let label = &self.edges[label_index];
				attributes.width = label.width;
				attributes.height = label.height;
				attributes.labelpos = Some(label.labelpos);
				dummy = Dummy::EdgeLabel;
			}
			let dummy = self.add_dummy_node(dummy, attributes, "_d");
			let segment = self.add_edge_label(EdgeLabel::weighted(weight, f64::NAN));
			self.g.set_edge(v, dummy, edge.name, segment);
			if first {
				self.dummy_chains.push(dummy);
				first = false;
			}
			v = dummy;
			v_rank += 1.0;
		}
		let segment = self.add_edge_label(EdgeLabel::weighted(weight, f64::NAN));
		self.g.set_edge(v, w, edge.name, segment);
	}

	fn undo_normalize(&mut self) {
		for &start in &self.dummy_chains.clone() {
			let mut v = start;
			let Some(mut node_index) = self.g.node(v).copied() else { continue };
			let (Some(label_index), Some(edge)) =
				(self.nodes[node_index].edge_label, self.nodes[node_index].edge_obj)
			else {
				continue;
			};
			self.g.set_edge(edge.v, edge.w, edge.name, label_index);
			while self.nodes[node_index].dummy.is_some() {
				let next = self.g.successors(v).first().copied();
				self.g.remove_node(v);
				let node = &self.nodes[node_index];
				let point = Point { x: node.x, y: node.y };
				let is_label = node.dummy == Some(Dummy::EdgeLabel);
				let (width, height) = (node.width, node.height);
				let label = &mut self.edges[label_index];
				label.points.get_or_insert_with(Vec::new).push(point);
				if is_label {
					label.x = Some(point.x);
					label.y = Some(point.y);
					label.width = width;
					label.height = height;
				}
				let Some(next) = next else { break };
				v = next;
				let Some(&next_index) = self.g.node(v) else { break };
				node_index = next_index;
			}
		}
	}

	/// `parentDummyChains`: moves the dummies of each edge into the clusters it passes.
	fn parent_dummy_chains(&mut self) {
		let mut postorder = KeyMap::default();
		let mut lim = 0usize;
		for child in self.g.children(None) {
			self.postorder_dfs(child, &mut lim, &mut postorder);
		}
		for start in self.dummy_chains.clone() {
			let Some(edge) = self.node(start).edge_obj else { continue };
			let (path, lca) = self.find_path(&postorder, edge.v, edge.w);
			let mut v = start;
			let mut path_index = 0;
			let mut ascending = true;
			let mut guard = self.g.node_count() + 1;
			while v != edge.w && guard > 0 {
				guard -= 1;
				let rank = self.node(v).rank.unwrap_or(f64::NAN);
				let mut path_v = path.get(path_index).copied().flatten();
				if ascending {
					loop {
						path_v = path.get(path_index).copied().flatten();
						let at_lca = path_index >= path.len() || path_v == lca;
						let below = path_v
							.and_then(|p| self.node(p).max_rank)
							.is_some_and(|max| max < rank);
						if at_lca || !below {
							break;
						}
						path_index += 1;
					}
					if path_index >= path.len() || path_v == lca {
						ascending = false;
					}
				}
				if !ascending {
					while path_index + 1 < path.len() {
						let next = path[path_index + 1];
						let contains =
							next.and_then(|p| self.node(p).min_rank).is_some_and(|min| min <= rank);
						if !contains {
							break;
						}
						path_index += 1;
					}
					path_v = path.get(path_index).copied().flatten();
				}
				self.g.set_parent(v, path_v);
				match self.g.successors(v).first() {
					Some(&next) => v = next,
					None => break,
				}
			}
		}
	}

	fn postorder_dfs(&self, v: Key, lim: &mut usize, result: &mut KeyMap<(usize, usize)>) {
		let low = *lim;
		for child in self.g.children(Some(v)) {
			self.postorder_dfs(child, lim, result);
		}
		result.insert(v, (low, *lim));
		*lim += 1;
	}

	/// The path from `v` to `w` through their lowest common ancestor, and the ancestor (`None`
	/// for the root).
	fn find_path(
		&self,
		postorder: &KeyMap<(usize, usize)>,
		v: Key,
		w: Key,
	) -> (Vec<Option<Key>>, Option<Key>) {
		let numbers = |key: Key| postorder.get(&key).copied().unwrap_or((0, 0));
		let low = numbers(v).0.min(numbers(w).0);
		let lim = numbers(v).1.max(numbers(w).1);
		let mut v_path = Vec::new();
		let mut parent = Some(v);
		loop {
			parent = parent.and_then(|key| self.g.parent(key));
			v_path.push(parent);
			match parent {
				Some(key) if numbers(key).0 > low || lim > numbers(key).1 => {}
				_ => break,
			}
		}
		let lca = parent;
		let mut w_path = Vec::new();
		let mut parent = Some(w);
		loop {
			parent = parent.and_then(|key| self.g.parent(key));
			if parent == lca {
				break;
			}
			w_path.push(parent);
			if parent.is_none() {
				break;
			}
		}
		w_path.reverse();
		v_path.extend(w_path);
		(v_path, lca)
	}

	fn add_border_segments(&mut self) {
		for child in self.g.children(None) {
			self.border_segments_dfs(child);
		}
	}

	fn border_segments_dfs(&mut self, v: Key) {
		for child in self.g.children(Some(v)) {
			self.border_segments_dfs(child);
		}
		let node = self.node(v);
		let (Some(min_rank), Some(max_rank)) = (node.min_rank, node.max_rank) else { return };
		{
			let node = self.node_mut(v);
			node.border_left.clear();
			node.border_right.clear();
		}
		let mut rank = min_rank;
		while rank < max_rank + 1.0 {
			self.add_border_node(BorderType::Left, "_bl", v, rank);
			self.add_border_node(BorderType::Right, "_br", v, rank);
			rank += 1.0;
		}
	}

	fn add_border_node(&mut self, border_type: BorderType, prefix: &str, cluster: Key, rank: f64) {
		let Some(index) = Self::rank_index(rank) else { return };
		let label =
			NodeLabel { rank: Some(rank), border_type: Some(border_type), ..NodeLabel::default() };
		let node = self.node(cluster);
		let borders = match border_type {
			BorderType::Left => &node.border_left,
			BorderType::Right => &node.border_right,
		};
		let previous =
			index.checked_sub(1).and_then(|previous| borders.get(previous).copied().flatten());
		let current = self.add_dummy_node(Dummy::Border, label, prefix);
		let node = self.node_mut(cluster);
		let list = match border_type {
			BorderType::Left => &mut node.border_left,
			BorderType::Right => &mut node.border_right,
		};
		if list.len() <= index {
			list.resize(index + 1, None);
		}
		list[index] = Some(current);
		self.g.set_parent(current, Some(cluster));
		if let Some(previous) = previous {
			let label = self.add_edge_label(EdgeLabel::weighted(1.0, f64::NAN));
			self.g.set_edge(previous, current, None, label);
		}
	}

	/// `util.buildLayerMatrix` over the nodes of `graph`.
	fn layer_matrix<E>(&self, graph: &Graph<usize, E>) -> Vec<Vec<Key>> {
		let nodes = graph.nodes();
		let label = |v: Key| graph.node(v).and_then(|&index| self.nodes.get(index));
		let max_rank = lodash_max(nodes.iter().filter_map(|&v| label(v)?.rank));
		let mut layers: Vec<Vec<Option<Key>>> = match max_rank.and_then(Self::rank_index) {
			Some(max) => vec![Vec::new(); max + 1],
			None => return Vec::new(),
		};
		for v in nodes {
			let Some(node) = label(v) else { continue };
			let (Some(rank), Some(order)) = (node.rank.and_then(Self::rank_index), node.order)
			else {
				continue;
			};
			let Some(layer) = layers.get_mut(rank) else { continue };
			if layer.len() <= order {
				layer.resize(order + 1, None);
			}
			layer[order] = Some(v);
		}
		layers.into_iter().map(|layer| layer.into_iter().flatten().collect()).collect()
	}

	fn insert_self_edges(&mut self) {
		for layer in self.layer_matrix(&self.g) {
			let mut order_shift = 0;
			for (index, v) in layer.into_iter().enumerate() {
				let node = self.node_mut(v);
				node.order = Some(index + order_shift);
				let self_edges = std::mem::take(&mut node.self_edges);
				let rank = node.rank;
				for (edge, label_index) in self_edges {
					let Some(label) = self.edges.get(label_index) else { continue };
					order_shift += 1;
					let dummy = NodeLabel {
						width: label.width,
						height: label.height,
						rank,
						order: Some(index + order_shift),
						edge: Some(edge),
						label: Some(label_index),
						..NodeLabel::default()
					};
					self.add_dummy_node(Dummy::SelfEdge, dummy, "_se");
				}
			}
		}
	}

	fn position_self_edges(&mut self) {
		for v in self.g.nodes() {
			let node = self.node(v).clone();
			if node.dummy != Some(Dummy::SelfEdge) {
				continue;
			}
			let (Some(edge), Some(label_index)) = (node.edge, node.label) else { continue };
			let self_node = self.node(edge.v);
			let x = self_node.x + self_node.width / 2.0;
			let y = self_node.y;
			let dx = node.x - x;
			let dy = self_node.height / 2.0;
			self.g.set_edge(edge.v, edge.w, edge.name, label_index);
			self.g.remove_node(v);
			if let Some(label) = self.edges.get_mut(label_index) {
				label.points = Some(vec![
					Point { x: x + 2.0 * dx / 3.0, y: y - dy },
					Point { x: x + 5.0 * dx / 6.0, y: y - dy },
					Point { x: x + dx, y },
					Point { x: x + 5.0 * dx / 6.0, y: y + dy },
					Point { x: x + 2.0 * dx / 3.0, y: y + dy },
				]);
				label.x = Some(node.x);
				label.y = Some(node.y);
			}
		}
	}

	fn remove_border_nodes(&mut self) {
		for v in self.g.nodes() {
			if !self.g.has_children(v) {
				continue;
			}
			let node = self.node(v);
			let top = node.border_top.map(|key| self.node(key).y).unwrap_or(f64::NAN);
			let bottom = node.border_bottom.map(|key| self.node(key).y).unwrap_or(f64::NAN);
			let last = |list: &Vec<Option<Key>>| list.last().copied().flatten();
			let left = last(&node.border_left).map(|key| self.node(key).x).unwrap_or(f64::NAN);
			let right = last(&node.border_right).map(|key| self.node(key).x).unwrap_or(f64::NAN);
			let node = self.node_mut(v);
			node.width = (right - left).abs();
			node.height = (bottom - top).abs();
			node.x = left + node.width / 2.0;
			node.y = top + node.height / 2.0;
		}
		for v in self.g.nodes() {
			if self.node(v).dummy == Some(Dummy::Border) {
				self.g.remove_node(v);
			}
		}
	}

	fn fixup_edge_label_coords(&mut self) {
		for edge in self.g.edges() {
			let label = self.edge_mut(edge);
			let Some(x) = &mut label.x else { continue };
			if matches!(label.labelpos, LabelPos::Left | LabelPos::Right) {
				label.width -= label.labeloffset;
			}
			match label.labelpos {
				LabelPos::Left => *x -= label.width / 2.0 + label.labeloffset,
				LabelPos::Right => *x += label.width / 2.0 + label.labeloffset,
				LabelPos::Center => {}
			}
		}
	}

	fn swap_width_height(&mut self) {
		for v in self.g.nodes() {
			let node = self.node_mut(v);
			std::mem::swap(&mut node.width, &mut node.height);
		}
		for edge in self.g.edges() {
			let label = self.edge_mut(edge);
			std::mem::swap(&mut label.width, &mut label.height);
		}
	}

	fn adjust_coordinate_system(&mut self) {
		if self.g.label.rankdir.is_horizontal() {
			self.swap_width_height();
		}
	}

	fn undo_coordinate_system(&mut self) {
		let rankdir = self.g.label.rankdir;
		if matches!(rankdir, RankDir::BottomTop | RankDir::RightLeft) {
			for v in self.g.nodes() {
				let node = self.node_mut(v);
				node.y = -node.y;
			}
			for edge in self.g.edges() {
				let label = self.edge_mut(edge);
				for point in label.points.iter_mut().flatten() {
					point.y = -point.y;
				}
				if let Some(y) = &mut label.y {
					*y = -*y;
				}
			}
		}
		if rankdir.is_horizontal() {
			for v in self.g.nodes() {
				let node = self.node_mut(v);
				std::mem::swap(&mut node.x, &mut node.y);
			}
			for edge in self.g.edges() {
				let label = self.edge_mut(edge);
				for point in label.points.iter_mut().flatten() {
					std::mem::swap(&mut point.x, &mut point.y);
				}
				if label.x.is_some() {
					std::mem::swap(&mut label.x, &mut label.y);
				}
			}
			self.swap_width_height();
		}
	}

	fn translate_graph(&mut self) {
		let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::INFINITY, 0.0, f64::INFINITY, 0.0);
		let margin_x = self.g.label.marginx;
		let margin_y = self.g.label.marginy;
		let mut extremes = |x: f64, y: f64, width: f64, height: f64| {
			min_x = js_min(min_x, x - width / 2.0);
			max_x = js_max(max_x, x + width / 2.0);
			min_y = js_min(min_y, y - height / 2.0);
			max_y = js_max(max_y, y + height / 2.0);
		};
		for v in self.g.nodes() {
			let node = self.node(v);
			extremes(node.x, node.y, node.width, node.height);
		}
		for edge in self.g.edges() {
			let label = self.edge(edge);
			if let Some(x) = label.x {
				extremes(x, label.y.unwrap_or(f64::NAN), label.width, label.height);
			}
		}
		min_x -= margin_x;
		min_y -= margin_y;
		for v in self.g.nodes() {
			let node = self.node_mut(v);
			node.x -= min_x;
			node.y -= min_y;
		}
		for edge in self.g.edges() {
			let label = self.edge_mut(edge);
			for point in label.points.iter_mut().flatten() {
				point.x -= min_x;
				point.y -= min_y;
			}
			if let Some(x) = &mut label.x {
				*x -= min_x;
			}
			if let Some(y) = &mut label.y {
				*y -= min_y;
			}
		}
		self.g.label.width = max_x - min_x + margin_x;
		self.g.label.height = max_y - min_y + margin_y;
	}

	fn assign_node_intersects(&mut self) {
		for edge in self.g.edges() {
			let node_v = self.node(edge.v).clone();
			let node_w = self.node(edge.w).clone();
			let label = self.edge_mut(edge);
			let (p1, p2) = match &label.points {
				Some(points) if !points.is_empty() => (points[0], points[points.len() - 1]),
				// A JavaScript array is truthy even when empty, so `[]` takes its ends from
				// `undefined` here.
				Some(_) => (Point { x: f64::NAN, y: f64::NAN }, Point { x: f64::NAN, y: f64::NAN }),
				None => (Point { x: node_w.x, y: node_w.y }, Point { x: node_v.x, y: node_v.y }),
			};
			let points = label.points.get_or_insert_with(Vec::new);
			points.insert(0, intersect_rect(&node_v, p1));
			points.push(intersect_rect(&node_w, p2));
		}
	}

	fn reverse_points_for_reversed_edges(&mut self) {
		for edge in self.g.edges() {
			let label = self.edge_mut(edge);
			if label.reversed
				&& let Some(points) = &mut label.points
			{
				points.reverse();
			}
		}
	}
}

/// `util.intersectRect`: where the line from `point` to the center of `node` crosses its
/// border. dagre throws for a point at the center; this returns the center.
fn intersect_rect(node: &NodeLabel, point: Point) -> Point {
	let dx = point.x - node.x;
	let dy = point.y - node.y;
	let mut w = node.width / 2.0;
	let mut h = node.height / 2.0;
	if dx == 0.0 && dy == 0.0 {
		return Point { x: node.x, y: node.y };
	}
	let (sx, sy) = if dy.abs() * w > dx.abs() * h {
		if dy < 0.0 {
			h = -h;
		}
		(h * dx / dy, h)
	} else {
		if dx < 0.0 {
			w = -w;
		}
		(w, w * dy / dx)
	};
	Point { x: node.x + sx, y: node.y + sy }
}

#[cfg(test)]
mod tests;
