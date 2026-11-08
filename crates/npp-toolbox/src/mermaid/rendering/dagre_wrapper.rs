//! Mermaid's dagre layout renderer (`layout-algorithms/dagre/index.js`, `mermaid-graphlib.js`).
//!
//! Clusters without edges to the outside are cut out into graphs of their own, laid out first
//! (with their own direction) and placed as one node. Edges to other clusters are anchored to a
//! node inside and cut at the cluster's border. Self loops are laid out through two helper
//! nodes and drawn as a compact loop.

use std::collections::HashMap;

use super::super::label::LabelBox;
use super::super::layout::dagre::{self, EdgeAttributes, LabelPos, LayoutEdge, LayoutNode};
use super::super::layout::graph::{Graph, Key, Keys, RankDir};
use super::super::svg::Svg;
use super::super::{Context, Result};
use super::clusters::write_cluster;
use super::edges::{self, EdgeEnd, EdgeRoute};
use super::intersect::Bounds;
use super::markers::{MarkerColors, Markers};
use super::shapes::{self, Shape};
use super::{BBox, LayoutData, Point, Theme, title_margins};

/// Nesting depth of `adjustClustersAndEdges` and `extractor`.
const MAX_EXTRACT_DEPTH: usize = 10;

const SELF_LOOP_HELPER_SIZE: f64 = 0.1;

#[derive(Default)]
struct GraphNode {
	/// The node of the layout data, `None` for the helpers of self loops.
	index: Option<usize>,
	width: f64,
	height: f64,
	x: f64,
	y: f64,
	/// A cluster cut out into a graph of its own (`clusterNode`).
	cluster: Option<Box<Graph<GraphNode, GraphEdge>>>,
	/// The drawing of the cut out cluster, its area and `diff`.
	painted: Option<(Svg, BBox, f64)>,
}

impl LayoutNode for GraphNode {
	fn size(&self) -> (f64, f64) {
		(self.width, self.height)
	}

	fn set_position(&mut self, x: f64, y: f64) {
		self.x = x;
		self.y = y;
	}

	fn set_size(&mut self, width: f64, height: f64) {
		self.width = width;
		self.height = height;
	}
}

#[derive(Clone, Default)]
struct GraphEdge {
	index: usize,
	/// The segment of a self loop: 0, 1 or 2.
	self_loop: Option<u8>,
	from_cluster: Option<Key>,
	to_cluster: Option<Key>,
	minlen: f64,
	labelpos: LabelPos,
	/// The size of the label, `None` without one.
	label: Option<(f64, f64)>,
	points: Vec<Point>,
	label_position: Option<Point>,
}

impl LayoutEdge for GraphEdge {
	fn attributes(&self) -> EdgeAttributes {
		let (width, height) = self.label.unwrap_or((0.0, 0.0));
		EdgeAttributes {
			minlen: self.minlen,
			width,
			height,
			labelpos: self.labelpos,
			..EdgeAttributes::default()
		}
	}

	fn set_layout(&mut self, points: Vec<Point>, label: Option<Point>) {
		self.points = points;
		self.label_position = label;
	}
}

type LayoutGraph = Graph<GraphNode, GraphEdge>;

/// A segment of a self loop: its order, ends and edge.
type LoopSegment<'g> = (u8, Key, Key, &'g GraphEdge);

#[derive(Default)]
struct ClusterEntry {
	/// The node edges to the cluster are anchored to.
	anchor: Option<Key>,
	external_connections: bool,
	/// The drawn bounds, for cutting edges.
	bounds: Option<Bounds>,
}

struct Renderer<'r, 'c> {
	context: &'r mut Context<'c>,
	data: &'r LayoutData,
	theme: Theme,
	keys: Keys,
	clusters: HashMap<Key, ClusterEntry>,
	/// Cluster ids in the order `clusterDb` got them.
	cluster_order: Vec<Key>,
	descendants: HashMap<Key, Vec<Key>>,
	shapes: Vec<Option<(Shape, LabelBox)>>,
	/// The labels besides the main one: descriptions of `rectWithTitle` nodes and the attribute
	/// cells of `erBox` entities, with their centers.
	extra_labels: Vec<Vec<(LabelBox, Point)>>,
	edge_labels: Vec<Option<LabelBox>>,
	/// The segments of self loops drawn one by one, after the edges of the data in the index
	/// space of the edges.
	segment_edges: Vec<super::Edge>,
	markers: Markers,
	default_curve: String,
	merges_self_loops: bool,
}

pub fn render(context: &mut Context, data: &LayoutData) -> Result<(Svg, BBox)> {
	let theme = Theme::new(context, data.style);
	let mut svg = Svg::new();
	svg.open("g").end();
	let colors = MarkerColors {
		is_extension_filled: data.style == super::DiagramStyle::Usecase,
		line: theme.line_color.clone(),
		edge: theme.edge_stroke.clone(),
		main_bkg: theme.plain_main_bkg.clone(),
		stroke_width: theme.stroke_width.clone(),
	};
	let markers = Markers::write(&mut svg, &context.id, data.diagram_type, data.markers, colors);
	let default_curve =
		context.config_text(data.config_section, "curve").unwrap_or_else(|| "basis".to_owned());
	let mut renderer = Renderer {
		context,
		data,
		theme,
		keys: Keys::default(),
		clusters: HashMap::new(),
		cluster_order: Vec::new(),
		descendants: HashMap::new(),
		shapes: (0..data.nodes.len()).map(|_| None).collect(),
		extra_labels: (0..data.nodes.len()).map(|_| Vec::new()).collect(),
		edge_labels: (0..data.edges.len()).map(|_| None).collect(),
		markers,
		default_curve,
		// `shouldMergeSelfLoopSegments`.
		merges_self_loops: matches!(
			data.diagram_type,
			"flowchart" | "flowchart-v2" | "stateDiagram" | "er" | "classDiagram"
		),
		segment_edges: Vec::new(),
	};
	let mut graph = renderer.prepare();
	let (body, bbox, _) = renderer.render_graph(&mut graph, None, 0);
	svg.append(body);
	renderer.markers.write_colored(&mut svg, data.diagram_type);
	svg.close_tag("g");
	Ok((svg, bbox))
}

/// The edge of `index`: one of the data or a segment of a self loop.
fn edge_at<'e>(data: &'e LayoutData, segments: &'e [super::Edge], index: usize) -> &'e super::Edge {
	match index.checked_sub(data.edges.len()) {
		Some(segment) => &segments[segment],
		None => &data.edges[index],
	}
}

impl Renderer<'_, '_> {
	/// `prepareLayoutForDagre`.
	fn prepare(&mut self) -> LayoutGraph {
		let data = self.data;
		let mut graph = LayoutGraph::new(true, true);
		graph.label.rankdir = RankDir::parse(&data.direction);
		graph.label.nodesep = data.node_spacing;
		graph.label.ranksep = data.rank_spacing;
		graph.label.marginx = 8.0;
		graph.label.marginy = 8.0;
		for (index, node) in data.nodes.iter().enumerate() {
			let key = self.keys.intern(&node.id);
			graph.set_node(key, GraphNode { index: Some(index), ..GraphNode::default() });
			if let Some(parent) = &node.parent_id {
				let parent = self.keys.intern(parent);
				graph.set_parent(key, Some(parent));
			}
		}
		for (index, edge) in data.edges.iter().enumerate() {
			let minlen = edge.minlen.max(1) as f64;
			let labelpos = LabelPos::parse(edge.labelpos);
			let start = self.keys.intern(&edge.start);
			let end = self.keys.intern(&edge.end);
			if start != end {
				let name = self.keys.intern(&edge.id);
				graph.set_edge(
					start,
					end,
					Some(name),
					GraphEdge { index, minlen, labelpos, ..GraphEdge::default() },
				);
				continue;
			}
			let helper1 = self.keys.intern(&format!("{}---{}---1", edge.start, edge.start));
			let helper2 = self.keys.intern(&format!("{}---{}---2", edge.start, edge.start));
			let parent = graph.parent(start);
			for helper in [helper1, helper2] {
				graph.set_node(
					helper,
					GraphNode { width: 10.0, height: 10.0, ..GraphNode::default() },
				);
				graph.set_parent(helper, parent);
			}
			// Without merging, each segment is an edge of its own: the middle one has the label,
			// the first the start marker, the last the end marker.
			let mut indices = [index; 3];
			if !self.merges_self_loops {
				for (order, suffix) in ["1", "mid", "2"].into_iter().enumerate() {
					let mut segment = super::Edge {
						id: format!("{}-cyclic-special-{suffix}", edge.start),
						dom_id: None,
						attributes: Vec::new(),
						..edge.clone()
					};
					if order != 1 {
						segment.label.clear();
					}
					if order != 0 {
						segment.arrow_type_start = "none".to_owned();
					}
					if order != 2 {
						segment.arrow_type_end = "none".to_owned();
					}
					indices[order] = data.edges.len() + self.segment_edges.len();
					self.segment_edges.push(segment);
					self.edge_labels.push(None);
				}
			}
			let segment = |order: u8| GraphEdge {
				index: indices[usize::from(order)],
				minlen,
				labelpos,
				self_loop: Some(order),
				..GraphEdge::default()
			};
			let names = ["-cyclic-special-0", "-cyclic-special-1", "-cyclic-special-2"]
				.map(|suffix| self.keys.intern(&format!("{}{suffix}", edge.start)));
			let is_group = data.nodes.iter().any(|node| node.id == edge.start && node.is_group);
			let mut first = segment(0);
			let mut last = segment(2);
			if is_group {
				first.from_cluster = Some(start);
				last.to_cluster = Some(start);
			}
			graph.set_edge(start, helper1, Some(names[0]), first);
			graph.set_edge(helper1, helper2, Some(names[1]), segment(1));
			graph.set_edge(helper2, start, Some(names[2]), last);
		}
		self.adjust_clusters_and_edges(&mut graph);
		graph
	}

	fn extract_descendants(&self, id: Key, graph: &LayoutGraph, depth: usize) -> Vec<Key> {
		let children = graph.children(Some(id));
		let mut result = children.clone();
		if depth > 256 {
			return result;
		}
		for child in children {
			result.extend(self.extract_descendants(child, graph, depth + 1));
		}
		result
	}

	fn is_descendant(&self, id: Key, ancestor: Key) -> bool {
		self.descendants.get(&ancestor).is_some_and(|descendants| descendants.contains(&id))
	}

	/// `findNonClusterChild`.
	fn find_non_cluster_child(
		&self,
		id: Key,
		graph: &LayoutGraph,
		cluster: Option<Key>,
		depth: usize,
	) -> Option<Key> {
		let children = graph.children(Some(id));
		if children.is_empty() {
			return Some(id);
		}
		if depth > 256 {
			return None;
		}
		let mut reserve = None;
		for child in children {
			let Some(found) = self.find_non_cluster_child(child, graph, cluster, depth + 1) else {
				continue;
			};
			let has_common_edges =
				cluster.is_some_and(|cluster| has_common_edges(graph, cluster, found));
			if has_common_edges {
				reserve = Some(found);
			} else {
				return Some(found);
			}
		}
		reserve
	}

	fn anchor_of(&self, id: Key) -> Key {
		match self.clusters.get(&id) {
			Some(entry) if entry.external_connections => entry.anchor.unwrap_or(id),
			_ => id,
		}
	}

	fn is_in_extractable_cluster(&self, graph: &LayoutGraph, node: Key, root: Key) -> bool {
		let mut parent = graph.parent(node);
		while let Some(current) = parent.filter(|&current| current != root) {
			if self.clusters.get(&current).is_some_and(|entry| !entry.external_connections) {
				return true;
			}
			parent = graph.parent(current);
		}
		false
	}

	fn find_safe_anchor(
		&self,
		graph: &LayoutGraph,
		cluster: Key,
		excluded: Option<Key>,
	) -> Option<Key> {
		for child in graph.children(Some(cluster)) {
			if excluded
				.is_some_and(|excluded| child == excluded || self.is_descendant(child, excluded))
			{
				continue;
			}
			let Some(candidate) = self.find_non_cluster_child(child, graph, Some(cluster), 0)
			else {
				continue;
			};
			if !self.is_in_extractable_cluster(graph, candidate, cluster) {
				return Some(candidate);
			}
		}
		None
	}

	fn set_cluster(&mut self, id: Key, entry: ClusterEntry) {
		if self.clusters.insert(id, entry).is_none() {
			self.cluster_order.push(id);
		}
	}

	/// `adjustClustersAndEdges`.
	fn adjust_clusters_and_edges(&mut self, graph: &mut LayoutGraph) {
		for id in graph.nodes() {
			if graph.has_children(id) {
				let descendants = self.extract_descendants(id, graph, 0);
				self.descendants.insert(id, descendants);
				let anchor = self.find_non_cluster_child(id, graph, Some(id), 0);
				self.set_cluster(id, ClusterEntry { anchor, ..ClusterEntry::default() });
			}
		}
		let edges = graph.edges();
		for id in graph.nodes() {
			if !graph.has_children(id) {
				continue;
			}
			let leaves = edges
				.iter()
				.any(|edge| self.is_descendant(edge.v, id) != self.is_descendant(edge.w, id));
			if leaves && let Some(entry) = self.clusters.get_mut(&id) {
				entry.external_connections = true;
			}
		}
		for id in self.cluster_order.clone() {
			let Some(non_cluster_child) = self.clusters.get(&id).and_then(|entry| entry.anchor)
			else {
				continue;
			};
			let parent = graph.parent(non_cluster_child);
			if let Some(parent) = parent
				&& parent != id
				&& self.clusters.get(&parent).is_some_and(|entry| !entry.external_connections)
				&& let Some(entry) = self.clusters.get_mut(&id)
			{
				entry.anchor = Some(parent);
			}
			let has_outgoing_edge = graph.edges().iter().any(|edge| edge.v == id);
			let is_external =
				self.clusters.get(&id).is_some_and(|entry| entry.external_connections);
			if is_external
				&& has_outgoing_edge
				&& self.is_in_extractable_cluster(graph, non_cluster_child, id)
			{
				let safe = self.find_safe_anchor(graph, id, graph.parent(non_cluster_child));
				if let Some(safe) = safe
					&& let Some(entry) = self.clusters.get_mut(&id)
				{
					entry.anchor = Some(safe);
				}
			}
		}
		for edge in graph.edges() {
			if !self.clusters.contains_key(&edge.v) && !self.clusters.contains_key(&edge.w) {
				continue;
			}
			let v = self.anchor_of(edge.v);
			let w = self.anchor_of(edge.w);
			let Some(mut label) = graph.remove_edge(edge) else { continue };
			if v != edge.v {
				if let Some(entry) =
					graph.parent(v).and_then(|parent| self.clusters.get_mut(&parent))
				{
					entry.external_connections = true;
				}
				label.from_cluster = Some(edge.v);
			}
			if w != edge.w {
				if let Some(entry) =
					graph.parent(w).and_then(|parent| self.clusters.get_mut(&parent))
				{
					entry.external_connections = true;
				}
				label.to_cluster = Some(edge.w);
			}
			graph.set_edge(v, w, edge.name, label);
		}
		self.extract(graph, 0);
	}

	/// `edgeInCluster`.
	fn edge_in_cluster(&self, v: Key, w: Key, cluster: Key) -> bool {
		if v == cluster || w == cluster {
			return false;
		}
		self.is_descendant(v, cluster) || self.is_descendant(w, cluster)
	}

	/// `copy`: moves the children of `cluster` into `target`.
	fn copy(
		&self,
		cluster: Key,
		graph: &mut LayoutGraph,
		target: &mut LayoutGraph,
		root: Key,
		depth: usize,
	) {
		let mut nodes = graph.children(Some(cluster));
		if cluster != root {
			nodes.push(cluster);
		}
		for node in nodes {
			if graph.has_children(node) {
				if depth < 256 {
					self.copy(node, graph, target, root, depth + 1);
				}
			} else {
				let data = graph.node_mut(node).map(std::mem::take).unwrap_or_default();
				target.set_node(node, data);
				let parent = graph.parent(node);
				if parent != Some(root) {
					target.set_parent(node, parent);
				}
				if cluster != root && node != cluster {
					target.set_parent(node, Some(cluster));
				}
				// graphlib's `edges(node)` ignores its argument: every edge is checked.
				for edge in graph.edges() {
					if self.edge_in_cluster(edge.v, edge.w, root)
						&& let Some(label) = graph.edge(edge).cloned()
					{
						target.set_edge(edge.v, edge.w, edge.name, label);
					}
				}
			}
			graph.remove_node(node);
		}
	}

	/// `extractor`: cuts out the clusters without external connections.
	fn extract(&mut self, graph: &mut LayoutGraph, depth: usize) {
		if depth > MAX_EXTRACT_DEPTH {
			return;
		}
		let nodes = graph.nodes();
		if !nodes.iter().any(|&node| graph.has_children(node)) {
			return;
		}
		for node in nodes {
			let is_extractable =
				self.clusters.get(&node).is_some_and(|entry| !entry.external_connections);
			if !is_extractable || !graph.has_children(node) {
				continue;
			}
			let index = graph.node(node).and_then(|data| data.index);
			let mut dir = if graph.label.rankdir == RankDir::TopBottom {
				RankDir::LeftRight
			} else {
				RankDir::TopBottom
			};
			if let Some(own) = index.and_then(|index| self.data.nodes[index].dir.as_deref()) {
				dir = RankDir::parse(own);
			}
			let mut cluster_graph = LayoutGraph::new(true, true);
			cluster_graph.label.rankdir = dir;
			cluster_graph.label.nodesep = 50.0;
			cluster_graph.label.ranksep = 50.0;
			cluster_graph.label.marginx = 8.0;
			cluster_graph.label.marginy = 8.0;
			self.copy(node, graph, &mut cluster_graph, node, 0);
			graph.set_node(
				node,
				GraphNode { index, cluster: Some(Box::new(cluster_graph)), ..GraphNode::default() },
			);
		}
		for node in graph.nodes() {
			if let Some(cluster) = graph.node_mut(node).and_then(|data| data.cluster.as_mut()) {
				self.extract(cluster, depth + 1);
			}
		}
	}

	/// The shape and label of the layout data node `index`.
	fn measure_node(&mut self, index: usize, rankdir: RankDir) -> (f64, f64) {
		let measured = super::measure_node(
			self.context,
			&self.theme,
			self.data,
			index,
			rankdir.is_horizontal(),
		);
		self.extra_labels[index] = measured.extra_labels;
		self.shapes[index] = Some((measured.shape, measured.label));
		(measured.width, measured.height)
	}

	/// `measureDagreGraph`, `dagreLayout` and `paintDagreLayoutCore`: the drawing of `graph`, its
	/// area and `diff`.
	fn render_graph(
		&mut self,
		graph: &mut LayoutGraph,
		parent_cluster: Option<(Key, Option<usize>)>,
		depth: usize,
	) -> (Svg, BBox, f64) {
		let (title_top, title_bottom) = title_margins(self.context);
		let title_margin = title_top + title_bottom;
		let measured_nodes = graph.nodes();
		for &v in &measured_nodes {
			if let Some((parent, index)) = parent_cluster {
				graph.set_node(parent, GraphNode { index, ..GraphNode::default() });
				if graph.parent(v).is_none() {
					graph.set_parent(v, Some(parent));
				}
			}
			let (ranksep, nodesep) = (graph.label.ranksep, graph.label.nodesep);
			let has_children = graph.has_children(v);
			let Some(node) = graph.node_mut(v) else { continue };
			let index = node.index;
			if let Some(mut cluster) = node.cluster.take() {
				cluster.label.ranksep = ranksep + 25.0;
				cluster.label.nodesep = nodesep;
				let painted = if depth < MAX_EXTRACT_DEPTH + 2 {
					self.render_graph(&mut cluster, Some((v, index)), depth + 1)
				} else {
					(Svg::new(), BBox::default(), 0.0)
				};
				if let Some(node) = graph.node_mut(v) {
					node.width = painted.1.width();
					node.height = painted.1.height();
					node.painted = Some(painted);
					node.cluster = Some(cluster);
				}
			} else if has_children {
				let anchor = self.find_non_cluster_child(v, graph, None, 0);
				self.set_cluster(v, ClusterEntry { anchor, ..ClusterEntry::default() });
			} else if let Some(index) = index {
				let rankdir = graph.label.rankdir;
				let (width, height) = self.measure_node(index, rankdir);
				if let Some(node) = graph.node_mut(v) {
					node.width = width;
					node.height = height;
				}
			} else {
				// The helpers of self loops are `labelRect`s, a rectangle of 0.1 pixels.
				node.width = SELF_LOOP_HELPER_SIZE;
				node.height = SELF_LOOP_HELPER_SIZE;
			}
		}
		let mut label_order = Vec::new();
		for edge in graph.edges() {
			let Some(label) = graph.edge(edge) else { continue };
			let index = label.index;
			if self.merges_self_loops && label.self_loop.is_some_and(|order| order != 1) {
				continue;
			}
			let data_edge = edge_at(self.data, &self.segment_edges, index);
			let laid = edges::layout_edge_label(self.context, data_edge, &self.theme);
			let size = (laid.width, laid.height);
			self.edge_labels[index] = Some(laid);
			label_order.push(index);
			if let Some(label) = graph.edge_mut(edge) {
				label.label = Some(size);
			}
		}

		dagre::layout(graph, &mut self.keys);

		let mut clusters = Svg::new();
		let mut edge_paths = Svg::new();
		let mut nodes_svg: HashMap<Key, Svg> = HashMap::new();
		let mut delayed_nodes: Vec<(usize, usize, Svg)> = Vec::new();
		let mut bbox = BBox::default();
		let mut group_diff = 0.0;
		for v in sort_by_hierarchy(graph) {
			let has_children = graph.has_children(v);
			let Some(node) = graph.node_mut(v) else { continue };
			if node.painted.is_some() {
				node.y += title_margin;
				let bounds =
					Bounds { x: node.x, y: node.y, width: node.width, height: node.height };
				let (painted, painted_bbox, diff) = node.painted.take().unwrap_or_default();
				let entry = self.clusters.entry(v).or_default();
				entry.bounds = Some(bounds);
				let dx = bounds.x + diff - bounds.width / 2.0;
				let dy = bounds.y - bounds.height / 2.0 - 8.0;
				let mut group = Svg::new();
				group
					.open("g")
					.attr("class", "root")
					.attr("transform", &super::translate(dx, dy))
					.end();
				group.append(painted);
				group.close_tag("g");
				bbox.include(&painted_bbox, dx, dy);
				nodes_svg.insert(v, group);
			} else if has_children {
				node.height += title_margin;
				let bounds =
					Bounds { x: node.x, y: node.y, width: node.width, height: node.height };
				let Some(index) = node.index else { continue };
				let (drawn, diff) = write_cluster(
					&mut clusters,
					&mut bbox,
					self.context,
					&self.data.nodes[index],
					bounds,
					&self.theme,
				);
				if let Some(node) = graph.node_mut(v) {
					node.width = drawn.width;
				}
				let entry = self.clusters.entry(v).or_default();
				entry.bounds = Some(drawn);
				// The `diff` of the group, which places the graph of a cut out cluster.
				group_diff = diff;
			} else {
				node.y += title_margin / 2.0;
				let Some(index) = node.index else {
					// Invisible, but part of the diagram's area.
					bbox.include_rect(node.x, node.y, SELF_LOOP_HELPER_SIZE, SELF_LOOP_HELPER_SIZE);
					continue;
				};
				let mut group = Svg::new();
				let center = Point { x: node.x, y: node.y };
				let Some((shape, _)) = self.shapes[index].as_ref() else { continue };
				let data_node = &self.data.nodes[index];
				super::write_er_background(
					&mut group,
					self.context,
					&self.theme,
					data_node,
					shape,
					center,
				);
				if group.out.is_empty() {
					self.write_node(&mut group, &mut bbox, index, center);
					nodes_svg.insert(v, group);
				} else {
					// Mermaid inserts the nodes concurrently (`Promise.all`): the background
					// group is appended at once, the entity after the background's labels, one
					// awaited text for the name and four per attribute row.
					let awaited_texts = 1 + 4 * data_node.attributes.len();
					let mut entity = Svg::new();
					self.write_node(&mut entity, &mut bbox, index, center);
					nodes_svg.insert(v, group);
					delayed_nodes.push((awaited_texts, delayed_nodes.len(), entity));
				}
			}
		}
		// Stable: equal waits finish in the order they started.
		npp::sort::sort_by_key(&mut delayed_nodes[..], |&(awaited_texts, order, ..)| {
			(awaited_texts, order)
		});
		let routes = self.edges_to_render(graph, title_margin / 2.0);
		let mut label_positions: HashMap<usize, Point> = HashMap::new();
		let mut terminal_paths: HashMap<usize, Vec<Point>> = HashMap::new();
		for route in routes {
			let edge = edge_at(self.data, &self.segment_edges, route.index);
			let end_of = |key: Key| -> Option<(Bounds, Option<usize>)> {
				let node = graph.node(key)?;
				let bounds =
					Bounds { x: node.x, y: node.y, width: node.width, height: node.height };
				Some((
					bounds,
					node.index.filter(|_| !graph.has_children(key) && node.cluster.is_none()),
				))
			};
			// Unmerged segments of a self loop keep the ends of their edge: Mermaid cuts both ends
			// of each segment at the looped node.
			let (start, end) = match route.index >= self.data.edges.len() {
				true => (
					self.keys.get(&edge.start).unwrap_or(route.start),
					self.keys.get(&edge.end).unwrap_or(route.end),
				),
				false => (route.start, route.end),
			};
			let (Some(tail), Some(head)) = (end_of(start), end_of(end)) else {
				continue;
			};
			let intersect_of = |index: Option<usize>| {
				index
					.and_then(|index| self.shapes[index].as_ref())
					.map(|(shape, _)| &shape.intersect)
			};
			let cluster_bounds = |key: Option<Key>| {
				key.and_then(|key| self.clusters.get(&key)).and_then(|entry| entry.bounds)
			};
			let edge_route = EdgeRoute {
				jumps: None,
				points: route.points,
				label_position: route.label_position,
				from_cluster: cluster_bounds(route.from_cluster),
				to_cluster: cluster_bounds(route.to_cluster),
			};
			// Clusters drawn here end edges at their rectangle.
			let tail_intersect = intersect_of(tail.1);
			let head_intersect = intersect_of(head.1);
			let rect = shapes::Intersect::Rect;
			let tail_intersect =
				tail_intersect.or_else(|| graph.has_children(route.start).then_some(&rect));
			let head_intersect =
				head_intersect.or_else(|| graph.has_children(route.end).then_some(&rect));
			let placement = edges::write_edge(
				&mut edge_paths,
				&mut bbox,
				self.context,
				edge,
				&edge_route,
				&EdgeEnd { bounds: tail.0, intersect: tail_intersect },
				&EdgeEnd { bounds: head.0, intersect: head_intersect },
				&mut self.markers,
				&self.theme,
				&self.default_curve,
			);
			if let Some(position) = placement.label {
				label_positions.insert(
					route.index,
					Point { x: position.x, y: position.y + title_margin / 2.0 },
				);
			}
			terminal_paths.insert(route.index, placement.path);
		}

		let mut svg = Svg::new();
		let is_root = parent_cluster.is_none();
		if is_root {
			svg.open("g").attr("class", "root").end();
		}
		svg.open("g").attr("class", "clusters").end();
		svg.append(clusters);
		svg.close_tag("g");
		svg.open("g").attr("class", "edgePaths").end();
		svg.append(edge_paths);
		svg.close_tag("g");
		svg.open("g").attr("class", "edgeLabels").end();
		for &index in &label_order {
			let (Some(label), Some(&position)) =
				(self.edge_labels[index].as_ref(), label_positions.get(&index))
			else {
				continue;
			};
			if label.is_empty() {
				continue;
			}
			let label = label.clone();
			edges::write_edge_label(
				&mut svg,
				&mut bbox,
				self.context,
				edge_at(self.data, &self.segment_edges, index),
				&label,
				position,
				&self.theme,
			);
		}
		for &index in &label_order {
			let edge = edge_at(self.data, &self.segment_edges, index);
			let Some(path) = terminal_paths.get(&index) else { continue };
			let terminals = [
				(&edge.start_label_right, edges::Terminal::StartRight, &edge.arrow_type_start),
				(&edge.end_label_left, edges::Terminal::EndLeft, &edge.arrow_type_end),
			];
			for (text, terminal, arrow) in terminals {
				if text.is_empty() {
					continue;
				}
				// Any arrow type, even `none`, counts as a marker.
				let marker_size = if arrow.is_empty() { 0.0 } else { 10.0 };
				let Some(position) = edges::terminal_label_position(path, terminal, marker_size)
				else {
					continue;
				};
				let label = edges::layout_terminal_label(self.context, edge, text);
				edges::write_terminal_label(
					&mut svg,
					&mut bbox,
					self.context,
					edge,
					&label,
					position,
					&self.theme,
				);
			}
		}
		svg.close_tag("g");
		svg.open("g").attr("class", "nodes").end();
		for v in &measured_nodes {
			if let Some(group) = nodes_svg.remove(v) {
				svg.append(group);
			}
		}
		for (.., group) in delayed_nodes {
			svg.append(group);
		}
		svg.close_tag("g");
		if is_root {
			svg.close_tag("g");
		}
		(svg, bbox, group_diff)
	}

	/// `insertNode` and `positionNode`.
	fn write_node(&mut self, svg: &mut Svg, bbox: &mut BBox, index: usize, center: Point) {
		let node = &self.data.nodes[index];
		let Some((shape, label)) = self.shapes[index].clone() else { return };
		let extra_labels = self.extra_labels[index].clone();
		let drawing =
			super::NodeDrawing { shape: &shape, label: &label, extra_labels: &extra_labels };
		super::write_node(
			svg,
			bbox,
			self.context,
			&self.theme,
			self.data.style,
			node,
			&drawing,
			center,
		);
	}

	/// `getEdgesToRender`: the edges of `graph` with their routes, self loops merged.
	fn edges_to_render(&self, graph: &LayoutGraph, offset_y: f64) -> Vec<RenderedEdge> {
		let mut result = Vec::new();
		let mut loops: Vec<(usize, Vec<LoopSegment>)> = Vec::new();
		for edge in graph.edges() {
			let Some(label) = graph.edge(edge) else { continue };
			match label.self_loop.filter(|_| self.merges_self_loops) {
				Some(order) => match loops.iter_mut().find(|(index, _)| *index == label.index) {
					Some((_, segments)) => segments.push((order, edge.v, edge.w, label)),
					None => loops.push((label.index, vec![(order, edge.v, edge.w, label)])),
				},
				None => result.push(RenderedEdge::of(label, edge.v, edge.w, offset_y)),
			}
		}
		for (index, mut segments) in loops {
			let node_key = self.keys.get(&self.data.edges[index].start);
			let node = node_key.and_then(|key| graph.node(key).map(|node| (key, node)));
			let (true, Some((key, node))) = (segments.len() == 3, node) else {
				result.extend(
					segments
						.iter()
						.map(|&(_, v, w, label)| RenderedEdge::of(label, v, w, offset_y)),
				);
				continue;
			};
			npp::sort::sort_by_key(&mut segments[..], |segment| segment.0);
			let middle = segments[1].3;
			let (label_width, label_height) = middle.label.unwrap_or((0.0, 0.0));
			let side = self_loop_side(graph, node, &segments, key);
			let center = Point { x: node.x, y: node.y - offset_y };
			let points = self_loop_points(center, node.width, node.height, side, label_width);
			let label_position =
				self_loop_label_position(center, &points, side, label_width, label_height);
			let from_cluster = segments.iter().find_map(|segment| segment.3.from_cluster);
			let to_cluster = segments.iter().find_map(|segment| segment.3.to_cluster);
			result.push(RenderedEdge {
				index,
				start: key,
				end: key,
				points: points.into_iter().map(|p| Point { x: p.x, y: p.y + offset_y }).collect(),
				label_position: Some(Point { x: label_position.x, y: label_position.y }),
				from_cluster,
				to_cluster,
			});
		}
		result
	}
}

struct RenderedEdge {
	index: usize,
	start: Key,
	end: Key,
	points: Vec<Point>,
	label_position: Option<Point>,
	from_cluster: Option<Key>,
	to_cluster: Option<Key>,
}

impl RenderedEdge {
	fn of(label: &GraphEdge, v: Key, w: Key, offset_y: f64) -> RenderedEdge {
		RenderedEdge {
			index: label.index,
			start: v,
			end: w,
			points: label.points.iter().map(|p| Point { x: p.x, y: p.y + offset_y }).collect(),
			label_position: label.label_position,
			from_cluster: label.from_cluster,
			to_cluster: label.to_cluster,
		}
	}
}

/// `findCommonEdges(graph, id1, id2)` is not empty.
fn has_common_edges(graph: &LayoutGraph, id1: Key, id2: Key) -> bool {
	let edges = graph.edges();
	let second: Vec<(Key, Key)> = edges
		.iter()
		.filter(|edge| edge.v == id2 || edge.w == id2)
		.map(|edge| (edge.v, edge.w))
		.collect();
	edges.iter().filter(|edge| edge.v == id1 || edge.w == id1).any(|edge| {
		let v = if edge.v == id1 { id2 } else { edge.v };
		let w = if edge.w == id1 { id1 } else { edge.w };
		second.contains(&(v, w))
	})
}

/// `sortNodesByHierarchy`: each level of the cluster tree before the levels below.
fn sort_by_hierarchy(graph: &LayoutGraph) -> Vec<Key> {
	fn sorter(graph: &LayoutGraph, nodes: Vec<Key>, depth: usize) -> Vec<Key> {
		let mut result = nodes.clone();
		if depth > 256 {
			return result;
		}
		for node in nodes {
			result.extend(sorter(graph, graph.children(Some(node)), depth + 1));
		}
		result
	}
	sorter(graph, graph.children(None), 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
	Top,
	Bottom,
	Left,
	Right,
}

fn default_side(rankdir: RankDir) -> Side {
	match rankdir {
		RankDir::BottomTop => Side::Bottom,
		RankDir::LeftRight => Side::Right,
		RankDir::RightLeft => Side::Left,
		RankDir::TopBottom => Side::Top,
	}
}

/// `getSelfLoopSide`: where dagre put the helpers of the loop.
fn self_loop_side(
	graph: &LayoutGraph,
	node: &GraphNode,
	segments: &[LoopSegment],
	key: Key,
) -> Side {
	let mut helpers: Vec<Key> = Vec::new();
	for &(_, v, w, _) in segments {
		for end in [v, w] {
			if end != key && !helpers.contains(&end) {
				helpers.push(end);
			}
		}
	}
	let mut hints: Vec<Point> = helpers
		.iter()
		.filter_map(|&helper| graph.node(helper))
		.map(|helper| Point { x: helper.x, y: helper.y })
		.collect();
	if hints.is_empty() {
		hints = segments.iter().flat_map(|segment| segment.3.points.iter().copied()).collect();
	}
	if hints.is_empty() {
		return default_side(graph.label.rankdir);
	}
	let count = hints.len() as f64;
	let center = hints
		.iter()
		.fold(Point::default(), |sum, p| Point { x: sum.x + p.x / count, y: sum.y + p.y / count });
	let (dx, dy) = (center.x - node.x, center.y - node.y);
	if dx.abs() > dy.abs() {
		return if dx > 0.0 { Side::Right } else { Side::Left };
	}
	if dy.abs() > 0.0 {
		return if dy > 0.0 { Side::Bottom } else { Side::Top };
	}
	default_side(graph.label.rankdir)
}

/// `getSelfLoopPoints`: a compact loop beside the node.
fn self_loop_points(
	center: Point,
	width: f64,
	height: f64,
	side: Side,
	label_width: f64,
) -> Vec<Point> {
	let (x, y) = (center.x, center.y);
	let (half_width, half_height) = (width / 2.0, height / 2.0);
	// `max` keeps the bounds of the clamp below in order for NaN sizes.
	let max_span = (width * 0.8).clamp(36.0, 100.0).max(36.0);
	let span = label_width.max(width * 0.35).clamp(36.0, max_span);
	let depth = (width.min(height) * 0.45).clamp(24.0, 48.0);
	let point = |x, y| Point { x, y };
	match side {
		Side::Bottom => {
			let bottom = y + half_height;
			vec![
				point(x - span / 2.0, bottom),
				point(x - span / 2.0, bottom + depth),
				point(x + span / 2.0, bottom + depth),
				point(x + span / 2.0, bottom),
			]
		}
		Side::Right => {
			let right = x + half_width;
			vec![
				point(right, y - span / 2.0),
				point(right + depth, y - span / 2.0),
				point(right + depth, y + span / 2.0),
				point(right, y + span / 2.0),
			]
		}
		Side::Left => {
			let left = x - half_width;
			vec![
				point(left, y - span / 2.0),
				point(left - depth, y - span / 2.0),
				point(left - depth, y + span / 2.0),
				point(left, y + span / 2.0),
			]
		}
		Side::Top => {
			let top = y - half_height;
			vec![
				point(x - span / 2.0, top),
				point(x - span / 2.0, top - depth),
				point(x + span / 2.0, top - depth),
				point(x + span / 2.0, top),
			]
		}
	}
}

/// `getSelfLoopLabelPosition`.
fn self_loop_label_position(
	center: Point,
	points: &[Point],
	side: Side,
	label_width: f64,
	label_height: f64,
) -> Point {
	let gap = 4.0;
	let xs = points.iter().map(|p| p.x);
	let ys = points.iter().map(|p| p.y);
	match side {
		Side::Bottom => Point {
			x: center.x,
			y: ys.fold(f64::NEG_INFINITY, f64::max) + label_height / 2.0 + gap,
		},
		Side::Right => {
			Point { x: xs.fold(f64::NEG_INFINITY, f64::max) + label_width / 2.0 + gap, y: center.y }
		}
		Side::Left => {
			Point { x: xs.fold(f64::INFINITY, f64::min) - label_width / 2.0 - gap, y: center.y }
		}
		Side::Top => {
			Point { x: center.x, y: ys.fold(f64::INFINITY, f64::min) - label_height / 2.0 - gap }
		}
	}
}
