//! The `elk` layout: Mermaid's ELK renderer (`layout-algorithms/elk/render.ts`) drawing a layout
//! of its own. Like ELK's hierarchical layout, every subgraph is laid out on its own, innermost
//! first, and is a block in the layout around it: dagre's layering, ordering and placement after
//! breaking cycles in the order the edges are declared, then the orthogonal router
//! (`layout::orthogonal`). Edges crossing subgraph borders are laid out in pieces that meet at
//! fixed ports on the frames. The edges are drawn with rounded corners and line hops like
//! Mermaid draws ELK's routes. This is not ELK: the pictures look like Mermaid's but do not
//! match them.

use super::super::label::LabelBox;
use super::super::layout::dagre::{self, EdgeAttributes, LabelPos, LayoutEdge, LayoutNode};
use std::collections::{HashMap, HashSet};

use super::super::layout::graph::{Graph, Keys, RankDir};
use super::super::layout::orthogonal::{self, Rect};
use super::super::svg::Svg;
use super::super::value::Value;
use super::super::{Context, Result};
use super::clusters::{layout_cluster_label, write_cluster};
use super::edges::{self, EdgeEnd, EdgeRoute};
use super::intersect::Bounds;
use super::line_jumps::{self, EdgeGeometry};
use super::markers::{MarkerColors, Markers};
use super::shapes::Intersect;
use super::{BBox, LayoutData, MeasuredNode, Point, Styles, Theme};

/// `spacing.baseValue` of the root graph: the room between nodes and between layers.
const NODE_SPACING: f64 = 40.0;
/// The room between edges running side by side through a layer.
const EDGE_SPACING: f64 = 20.0;
/// `elk.padding` of subgraphs.
const SUBGRAPH_PADDING: f64 = 24.0;
/// The margin around the graph.
const MARGIN: f64 = 8.0;
/// The bisection steps of [`outline_attach_point`].
const OUTLINE_RAY_STEPS: usize = 20;

#[derive(Default)]
struct GraphNode {
	width: f64,
	height: f64,
	x: f64,
	y: f64,
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

#[derive(Default)]
struct GraphEdge {
	index: usize,
	minlen: f64,
	label: (f64, f64),
	points: Vec<Point>,
	label_position: Option<Point>,
}

impl LayoutEdge for GraphEdge {
	fn attributes(&self) -> EdgeAttributes {
		EdgeAttributes {
			minlen: self.minlen,
			width: self.label.0,
			height: self.label.1,
			labelpos: LabelPos::Center,
			..EdgeAttributes::default()
		}
	}

	fn set_layout(&mut self, points: Vec<Point>, label: Option<Point>) {
		self.points = points;
		self.label_position = label;
	}
}

/// How an edge is laid out: in pieces in the layouts of the containers it passes.
#[derive(Default)]
struct EdgePlan {
	/// The container where the edge runs between its ends or the outermost clusters around
	/// them, and these items; `None` when one end is a cluster around the other.
	main: Option<(Option<usize>, usize, usize)>,
	/// The clusters the edge leaves, innermost first, with the item it leaves from in each.
	leaves: Vec<(usize, usize)>,
	/// The clusters the edge enters, innermost first, with the item it goes to in each.
	enters: Vec<(usize, usize)>,
}

/// A piece of an edge in the layout of a container.
#[derive(Clone, Copy)]
enum PartKind {
	/// Between two items of the container.
	Main { source: usize, target: usize },
	/// From an item to the border of the container.
	Leave { item: usize },
	/// From the border of the container to an item.
	Enter { item: usize },
}

/// The extent of points and rectangles.
#[derive(Clone, Copy)]
struct Extent {
	left: f64,
	top: f64,
	right: f64,
	bottom: f64,
}

impl Default for Extent {
	fn default() -> Self {
		Extent {
			left: f64::INFINITY,
			top: f64::INFINITY,
			right: f64::NEG_INFINITY,
			bottom: f64::NEG_INFINITY,
		}
	}
}

impl Extent {
	fn is_empty(&self) -> bool {
		self.left > self.right || self.top > self.bottom
	}

	fn include(&mut self, point: Point) {
		if point.x.is_finite() && point.y.is_finite() {
			self.left = self.left.min(point.x);
			self.right = self.right.max(point.x);
			self.top = self.top.min(point.y);
			self.bottom = self.bottom.max(point.y);
		}
	}

	fn include_rect(&mut self, rect: Rect) {
		let (half_width, half_height) = (rect.width / 2.0, rect.height / 2.0);
		self.include(Point { x: rect.x - half_width, y: rect.y - half_height });
		self.include(Point { x: rect.x + half_width, y: rect.y + half_height });
	}
}

/// The edges, given by the indices of their ends, that close a cycle with the edges before them
/// in their direction; those are reversed in the layering.
fn edges_closing_cycles(ends: &[(usize, usize)], node_count: usize) -> Vec<bool> {
	let mut successors: Vec<Vec<usize>> = vec![Vec::new(); node_count];
	let mut visited = vec![usize::MAX; node_count];
	let mut stack = Vec::new();
	let mut reversed = vec![false; ends.len()];
	for (position, &(source, target)) in ends.iter().enumerate() {
		if source == target || source >= node_count || target >= node_count {
			continue;
		}
		// Whether `target` reaches `source` along the edges so far.
		stack.clear();
		stack.push(target);
		visited[target] = position;
		let mut reaches = false;
		while let Some(node) = stack.pop() {
			if node == source {
				reaches = true;
				break;
			}
			for &next in &successors[node] {
				if visited[next] != position {
					visited[next] = position;
					stack.push(next);
				}
			}
		}
		reversed[position] = reaches;
		match reaches {
			true => successors[target].push(source),
			false => successors[source].push(target),
		}
	}
	reversed
}

fn finite_or_zero(value: f64) -> f64 {
	if value.is_finite() { value } else { 0.0 }
}

/// Lays out and draws `data` with the `elk` layout.
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
	let mut markers =
		Markers::write(&mut svg, &context.id, data.diagram_type, data.markers, colors);
	let root_direction = RankDir::parse(&data.direction);
	let count = data.nodes.len();

	// The hierarchy: a group with children is a cluster, an empty one a node.
	let mut indices: HashMap<&str, usize> = HashMap::new();
	for (index, node) in data.nodes.iter().enumerate() {
		indices.entry(node.id.as_str()).or_insert(index);
	}
	let index_of = |id: &str| indices.get(id).copied();
	let parents: Vec<Option<usize>> = data
		.nodes
		.iter()
		.map(|node| {
			node.parent_id
				.as_deref()
				.and_then(index_of)
				.filter(|&parent| data.nodes[parent].is_group)
		})
		.collect();
	let mut children: Vec<Vec<usize>> = vec![Vec::new(); count];
	let mut root_items = Vec::new();
	for (index, parent) in parents.iter().enumerate() {
		match parent {
			Some(parent) => children[*parent].push(index),
			None => root_items.push(index),
		}
	}
	let is_cluster = |index: usize| !children[index].is_empty();
	// A node and the clusters around it, innermost first; `None` in a broken hierarchy.
	let chain = |node: usize| -> Option<Vec<usize>> {
		let mut chain = vec![node];
		let mut current = node;
		while let Some(parent) = parents[current] {
			if chain.len() > count {
				return None;
			}
			chain.push(parent);
			current = parent;
		}
		Some(chain)
	};
	let depths: Vec<Option<usize>> =
		(0..count).map(|index| chain(index).map(|chain| chain.len() - 1)).collect();

	// Every subgraph is laid out on its own and is a block in the layout around it, like ELK's
	// hierarchical layout. An edge between nodes of different subgraphs is laid out in pieces:
	// out of the subgraphs around its start, between the outermost blocks of both ends in their
	// common container, and into the subgraphs around its end.
	let plans: Vec<EdgePlan> = data
		.edges
		.iter()
		.map(|edge| {
			let (Some(start), Some(end)) = (index_of(&edge.start), index_of(&edge.end)) else {
				return EdgePlan::default();
			};
			let (Some(start_chain), Some(end_chain)) = (chain(start), chain(end)) else {
				return EdgePlan::default();
			};
			let around_end: HashSet<usize> = end_chain[1..].iter().copied().collect();
			let leaves = start_chain[1..]
				.iter()
				.position(|cluster| around_end.contains(cluster))
				.unwrap_or(start_chain.len() - 1);
			let common = start_chain.get(leaves + 1).copied();
			let enters = match common {
				Some(common) => end_chain[1..]
					.iter()
					.position(|&cluster| cluster == common)
					.unwrap_or(end_chain.len() - 1),
				None => end_chain.len() - 1,
			};
			let (source, target) = (start_chain[leaves], end_chain[enters]);
			EdgePlan {
				main: (start == end || source != target).then_some((common, source, target)),
				leaves: (0..leaves)
					.map(|level| (start_chain[level + 1], start_chain[level]))
					.collect(),
				enters: (0..enters).map(|level| (end_chain[level + 1], end_chain[level])).collect(),
			}
		})
		.collect();
	let mut parts_at: HashMap<Option<usize>, Vec<(usize, PartKind)>> = HashMap::new();
	let mut is_crossed = vec![false; count];
	for (edge, plan) in plans.iter().enumerate() {
		if let Some((common, source, target)) = plan.main {
			parts_at.entry(common).or_default().push((edge, PartKind::Main { source, target }));
		}
		for &(cluster, item) in &plan.leaves {
			is_crossed[cluster] = true;
			parts_at.entry(Some(cluster)).or_default().push((edge, PartKind::Leave { item }));
		}
		for &(cluster, item) in &plan.enters {
			is_crossed[cluster] = true;
			parts_at.entry(Some(cluster)).or_default().push((edge, PartKind::Enter { item }));
		}
	}

	// Cycles are broken in the order the edges are declared: an edge runs against the layers when
	// the earlier edges lead from its target to its source. Its pieces inside clusters run against
	// the layers too: they leave and enter on the other side of the frames.
	let mut runs_upward = vec![false; data.edges.len()];
	for (container, parts) in &parts_at {
		let items = match container {
			Some(cluster) => &children[*cluster],
			None => &root_items,
		};
		let local: HashMap<usize, usize> =
			items.iter().enumerate().map(|(position, &item)| (item, position)).collect();
		let local_of = |item: usize| local.get(&item).copied().unwrap_or(usize::MAX);
		let ends: Vec<(usize, usize)> = parts
			.iter()
			.map(|&(_, kind)| match kind {
				PartKind::Main { source, target } => (local_of(source), local_of(target)),
				_ => (usize::MAX, usize::MAX),
			})
			.collect();
		for (&(edge, _), reversed) in parts.iter().zip(edges_closing_cycles(&ends, items.len())) {
			runs_upward[edge] |= reversed;
		}
	}

	// Clusters outside in. A subgraph keeps its own direction when no edge crosses its border.
	let mut clusters_outside_in: Vec<usize> =
		(0..count).filter(|&index| is_cluster(index) && depths[index].is_some()).collect();
	npp::sort::sort_by_key(&mut clusters_outside_in[..], |&index| depths[index]);
	let mut directions = vec![root_direction; count];
	for &cluster in &clusters_outside_in {
		let outer = parents[cluster].map_or(root_direction, |parent| directions[parent]);
		directions[cluster] = match data.nodes[cluster].dir.as_deref() {
			Some(dir) if !is_crossed[cluster] => RankDir::parse(dir),
			_ => outer,
		};
	}
	let direction_in =
		|container: Option<usize>| container.map_or(root_direction, |c| directions[c]);

	// Measuring (`createGraphWithElements`).
	let mut measured: Vec<Option<MeasuredNode>> = Vec::with_capacity(count);
	for (index, &parent) in parents.iter().enumerate() {
		measured.push(match is_cluster(index) {
			true => None,
			false => {
				let is_horizontal = direction_in(parent).is_horizontal();
				Some(super::measure_node(context, &theme, data, index, is_horizontal))
			}
		});
	}
	let titles: Vec<Option<LabelBox>> = (0..count)
		.map(|index| {
			is_cluster(index).then(|| {
				let node = &data.nodes[index];
				let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
				layout_cluster_label(context, node, &styles, None)
			})
		})
		.collect();
	let edge_labels: Vec<LabelBox> =
		data.edges.iter().map(|edge| edges::layout_edge_label(context, edge, &theme)).collect();

	// Self loops go out of the side of their node, which needs room.
	let mut loops: Vec<Vec<usize>> = vec![Vec::new(); count];
	for (edge, plan) in plans.iter().enumerate() {
		if let Some((_, source, target)) = plan.main
			&& source == target
		{
			loops[source].push(edge);
		}
	}
	let loop_room: Vec<f64> = loops
		.iter()
		.enumerate()
		.map(|(index, loops)| {
			let is_horizontal = direction_in(parents[index]).is_horizontal();
			let across = |label: &LabelBox| if is_horizontal { label.height } else { label.width };
			let widths = loops.iter().map(|&edge| across(&edge_labels[edge]));
			orthogonal::self_loop_room(loops.len(), widths)
		})
		.collect();

	// The layouts of the containers, inside out. Positions are in the coordinates of the
	// container until the blocks are placed.
	let mut item_bounds: Vec<Option<Rect>> = vec![None; count];
	let mut frames: Vec<Option<Rect>> = vec![None; count];
	let mut main_routes: Vec<Option<orthogonal::Route>> = vec![None; data.edges.len()];
	// The pieces of edges inside clusters and their ports on the frames (offsets from the
	// center), keyed by edge, cluster and whether the edge enters.
	let mut piece_routes: HashMap<(usize, usize, bool), Vec<Point>> = HashMap::new();
	let mut ports: HashMap<(usize, usize, bool), Point> = HashMap::new();
	let containers: Vec<Option<usize>> =
		clusters_outside_in.iter().rev().map(|&cluster| Some(cluster)).chain([None]).collect();
	for container in containers {
		let items: &[usize] = match container {
			Some(cluster) => &children[cluster],
			None => &root_items,
		};
		let direction = direction_in(container);
		let is_horizontal = direction.is_horizontal();
		let parts: &[(usize, PartKind)] = parts_at.get(&container).map_or(&[], Vec::as_slice);

		// Layout nodes: the items, then one dummy per piece at the border.
		let mut router_index: HashMap<usize, usize> = HashMap::new();
		let mut sizes: Vec<(String, f64, f64, f64)> = Vec::with_capacity(items.len() + parts.len());
		for &item in items {
			let size = match is_cluster(item) {
				true => frames[item].map(|frame| (frame.width, frame.height)),
				false => measured[item].as_ref().map(|node| (node.width, node.height)),
			};
			let (width, height) = size.unwrap_or((0.0, 0.0));
			router_index.insert(item, sizes.len());
			sizes.push((data.nodes[item].id.clone(), width, height, loop_room[item]));
		}
		let mut ends: Vec<(usize, usize)> = Vec::with_capacity(parts.len());
		for (position, &(_, kind)) in parts.iter().enumerate() {
			let item = |item: usize| router_index.get(&item).copied().unwrap_or(usize::MAX);
			let mut dummy = || {
				sizes.push((format!("\u{2}{position}"), 0.0, 0.0, 0.0));
				sizes.len() - 1
			};
			ends.push(match kind {
				PartKind::Main { source, target } => (item(source), item(target)),
				PartKind::Leave { item: source } => (item(source), dummy()),
				PartKind::Enter { item: target } => (dummy(), item(target)),
			});
		}
		let label_size = |edge: usize| {
			let label = &edge_labels[edge];
			if label.is_empty() { (0.0, 0.0) } else { (label.width, label.height) }
		};
		let has_label = |&(edge, kind): &(usize, PartKind)| {
			matches!(kind, PartKind::Main { .. }) && !edge_labels[edge].is_empty()
		};

		let reversed: Vec<bool> = parts.iter().map(|&(edge, _)| runs_upward[edge]).collect();

		// dagre's layering, ordering and placement. Dummies at the border belong before the
		// first or after the last layer; a second run lengthens their edges where needed.
		let run_dagre = |minlens: &[f64]| {
			let mut keys = Keys::default();
			let mut graph: Graph<GraphNode, GraphEdge> = Graph::new(true, false);
			graph.label.rankdir = direction;
			graph.label.nodesep = NODE_SPACING;
			graph.label.ranksep = NODE_SPACING;
			graph.label.edgesep = EDGE_SPACING;
			graph.label.marginx = MARGIN;
			graph.label.marginy = MARGIN;
			let node_keys: Vec<_> = sizes.iter().map(|size| keys.intern(&size.0)).collect();
			for (&key, &(_, mut width, mut height, room)) in node_keys.iter().zip(&sizes) {
				match is_horizontal {
					true => height += room,
					false => width += room,
				}
				graph.set_node(key, GraphNode { width, height, ..GraphNode::default() });
			}
			for (position, (&(source, target), part)) in ends.iter().zip(parts).enumerate() {
				let (Some(&source_key), Some(&target_key)) =
					(node_keys.get(source), node_keys.get(target))
				else {
					continue;
				};
				if source == target {
					continue;
				}
				let (source_key, target_key) = match reversed[position] {
					true => (target_key, source_key),
					false => (source_key, target_key),
				};
				let name = keys.intern(&format!("{position}"));
				let label = if has_label(part) { label_size(part.0) } else { (0.0, 0.0) };
				graph.set_edge(
					source_key,
					target_key,
					Some(name),
					GraphEdge {
						index: position,
						// ELK has no minimum length of edges.
						minlen: minlens.get(position).copied().unwrap_or(1.0),
						label,
						..GraphEdge::default()
					},
				);
			}
			let ranks = dagre::layout_ranked(&mut graph, &mut keys);
			let node_ranks: Vec<Option<i64>> =
				node_keys.iter().map(|key| ranks.nodes.get(key).copied()).collect();
			(graph, node_keys, node_ranks)
		};
		let (mut graph, mut node_keys, mut node_ranks) = run_dagre(&[]);
		let item_ranks = || node_ranks[..items.len()].iter().flatten().copied();
		let (lowest, highest) = (item_ranks().min(), item_ranks().max());
		let mut minlens = vec![1.0; parts.len()];
		let mut lengthen = false;
		for (position, &(source, target)) in ends.iter().enumerate() {
			let rank = |node: usize| node_ranks.get(node).copied().flatten();
			let (Some(source_rank), Some(target_rank)) = (rank(source), rank(target)) else {
				continue;
			};
			// The rank of the item of a piece, and whether its dummy belongs below the layers.
			let (item_rank, dummy_rank, is_below) = match parts[position].1 {
				PartKind::Main { .. } => continue,
				PartKind::Leave { .. } => (source_rank, target_rank, !reversed[position]),
				PartKind::Enter { .. } => (target_rank, source_rank, reversed[position]),
			};
			let needed = match is_below {
				true => highest.map(|highest| (highest - item_rank) / 2 + 1),
				false => lowest.map(|lowest| (item_rank - lowest) / 2 + 1),
			};
			if let Some(needed) = needed.filter(|&needed| needed > 1) {
				minlens[position] = needed as f64;
				lengthen |= (dummy_rank - item_rank).abs() < needed * 2;
			}
		}
		if lengthen {
			(graph, node_keys, node_ranks) = run_dagre(&minlens);
		}
		let mut laid_edges: Vec<Option<&GraphEdge>> = vec![None; parts.len()];
		for laid in graph.edges() {
			if let Some(laid) = graph.edge(laid)
				&& let Some(slot) = laid_edges.get_mut(laid.index)
			{
				*slot = Some(laid);
			}
		}

		// The orthogonal routes.
		let router_nodes: Vec<orthogonal::Node> = node_keys
			.iter()
			.zip(&sizes)
			.zip(&node_ranks)
			.map(|((&key, &(_, width, height, room)), rank)| {
				let (x, y) = graph.node(key).map_or((0.0, 0.0), |node| (node.x, node.y));
				// The room of self loops is on the left (top in horizontal layouts) of the shape.
				let (x, y) = match is_horizontal {
					true => (x, y + room / 2.0),
					false => (x + room / 2.0, y),
				};
				orthogonal::Node { bounds: Rect { x, y, width, height }, rank: rank.unwrap_or(0) }
			})
			.collect();
		let mut router_edges = Vec::with_capacity(parts.len());
		for (position, (&(source, target), part)) in ends.iter().zip(parts).enumerate() {
			let (edge, kind) = *part;
			if source >= sizes.len() || target >= sizes.len() {
				router_edges.push(orthogonal::Edge { source: usize::MAX, ..Default::default() });
				continue;
			}
			let laid = laid_edges[position];
			let mut waypoints = laid
				.filter(|laid| laid.points.len() >= 2)
				.map(|laid| laid.points[1..laid.points.len() - 1].to_vec())
				.unwrap_or_default();
			if reversed[position] {
				waypoints.reverse();
			}
			let (label_width, label_height) = label_size(edge);
			let label = match has_label(part) {
				false => None,
				true if source == target => {
					Some(Rect { x: 0.0, y: 0.0, width: label_width, height: label_height })
				}
				true => laid.and_then(|laid| laid.label_position).map(|center| Rect {
					x: center.x,
					y: center.y,
					width: label_width,
					height: label_height,
				}),
			};
			// Ends at blocks the edge continues inside of have the ports of the pieces inside.
			let (source_item, target_item) = match kind {
				PartKind::Main { source, target } => (Some(source), Some(target)),
				PartKind::Leave { item } => (Some(item), None),
				PartKind::Enter { item } => (None, Some(item)),
			};
			router_edges.push(orthogonal::Edge {
				source,
				target,
				waypoints,
				label,
				source_port: source_item.and_then(|item| ports.get(&(edge, item, false)).copied()),
				target_port: target_item.and_then(|item| ports.get(&(edge, item, true)).copied()),
			});
		}
		let mut layout = orthogonal::Layout {
			direction,
			layer_spacing: NODE_SPACING,
			nodes: router_nodes,
			edges: router_edges,
		};
		let routes = orthogonal::route(&mut layout);
		for (&item, node) in items.iter().zip(&layout.nodes) {
			item_bounds[item] = Some(node.bounds);
		}

		// The frame of a cluster: around its children with padding and the title on top, and
		// out to the dummies, where the pieces of crossing edges end at the frame.
		let Some(cluster) = container else {
			for (route, &(edge, _)) in routes.into_iter().zip(parts) {
				main_routes[edge] = Some(route);
			}
			continue;
		};
		let mut extent = Extent::default();
		for node in &layout.nodes[..items.len()] {
			extent.include_rect(node.bounds);
		}
		for (route, &(_, kind)) in routes.iter().zip(parts) {
			let points = match kind {
				PartKind::Main { .. } => &route.points[..],
				PartKind::Leave { .. } => &route.points[..route.points.len().saturating_sub(1)],
				PartKind::Enter { .. } => route.points.get(1..).unwrap_or_default(),
			};
			for point in points {
				extent.include(*point);
			}
		}
		for label in layout.edges.iter().filter_map(|edge| edge.label) {
			extent.include_rect(label);
		}
		let (title_width, title_height) = titles[cluster].as_ref().map_or((0.0, 0.0), |title| {
			(finite_or_zero(title.width), finite_or_zero(title.height))
		});
		let mut frame = match extent.is_empty() {
			true => Extent { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 },
			false => Extent {
				left: extent.left - SUBGRAPH_PADDING,
				top: extent.top - SUBGRAPH_PADDING - title_height,
				right: extent.right + SUBGRAPH_PADDING,
				bottom: extent.bottom + SUBGRAPH_PADDING,
			},
		};
		let least_width = title_width + SUBGRAPH_PADDING;
		if frame.right - frame.left < least_width {
			let grow = (least_width - (frame.right - frame.left)) / 2.0;
			frame.left -= grow;
			frame.right += grow;
		}
		let center =
			Point { x: (frame.left + frame.right) / 2.0, y: (frame.top + frame.bottom) / 2.0 };
		for (route, &(edge, kind)) in routes.into_iter().zip(parts) {
			let mut points = route.points;
			let enters = match kind {
				PartKind::Main { .. } => {
					main_routes[edge] = Some(orthogonal::Route { points, label: route.label });
					continue;
				}
				PartKind::Leave { .. } => false,
				PartKind::Enter { .. } => true,
			};
			if points.len() >= 2 {
				let (border, inner) = match enters {
					true => (0, 1),
					false => (points.len() - 1, points.len() - 2),
				};
				let (point, next) = (points[border], points[inner]);
				let moved = match (point.x - next.x).abs() <= 1e-6 {
					true if point.y <= next.y => Point { x: point.x, y: frame.top },
					true => Point { x: point.x, y: frame.bottom },
					false if point.x <= next.x => Point { x: frame.left, y: point.y },
					false => Point { x: frame.right, y: point.y },
				};
				points[border] = moved;
				ports.insert(
					(edge, cluster, enters),
					Point { x: moved.x - center.x, y: moved.y - center.y },
				);
			}
			piece_routes.insert((edge, cluster, enters), points);
		}
		frames[cluster] = Some(Rect {
			x: center.x,
			y: center.y,
			width: frame.right - frame.left,
			height: frame.bottom - frame.top,
		});
	}

	// Placing the blocks: the offsets from the coordinates of each cluster to the final ones.
	let mut offsets: Vec<Point> = vec![Point { x: 0.0, y: 0.0 }; count];
	for &cluster in &clusters_outside_in {
		let outer = parents[cluster].map_or(Point { x: 0.0, y: 0.0 }, |parent| offsets[parent]);
		if let (Some(item), Some(frame)) = (item_bounds[cluster], frames[cluster]) {
			offsets[cluster] =
				Point { x: item.x + outer.x - frame.x, y: item.y + outer.y - frame.y };
		}
	}
	let offset_of =
		|container: Option<usize>| container.map_or(Point { x: 0.0, y: 0.0 }, |c| offsets[c]);
	let moved =
		|point: Point, offset: Point| Point { x: point.x + offset.x, y: point.y + offset.y };
	let final_bounds: Vec<Option<Rect>> = (0..count)
		.map(|index| {
			let offset = offset_of(parents[index]);
			item_bounds[index].map(|rect| Rect {
				x: rect.x + offset.x,
				y: rect.y + offset.y,
				..rect
			})
		})
		.collect();

	// The edges from their pieces.
	let mut routes: Vec<orthogonal::Route> = Vec::with_capacity(data.edges.len());
	for (edge, plan) in plans.iter().enumerate() {
		let mut route = orthogonal::Route::default();
		let mut append = |points: &[Point], offset: Point| {
			for &point in points {
				let point = moved(point, offset);
				let is_repeated = route.points.last().is_some_and(|last: &Point| {
					(last.x - point.x).abs() <= 1e-6 && (last.y - point.y).abs() <= 1e-6
				});
				if !is_repeated {
					route.points.push(point);
				}
			}
		};
		for &(cluster, _) in &plan.leaves {
			if let Some(points) = piece_routes.get(&(edge, cluster, false)) {
				append(points, offsets[cluster]);
			}
		}
		let mut label = None;
		if let Some((common, _, _)) = plan.main
			&& let Some(main) = &main_routes[edge]
		{
			append(&main.points, offset_of(common));
			label = main.label.map(|center| moved(center, offset_of(common)));
		}
		for &(cluster, _) in plan.enters.iter().rev() {
			if let Some(points) = piece_routes.get(&(edge, cluster, true)) {
				append(points, offsets[cluster]);
			}
		}
		route.label = label;
		routes.push(route);
	}

	// Painting: clusters outside in, then edges, labels and nodes.
	let mut bbox = BBox::default();
	let mut clusters_svg = Svg::new();
	let mut cluster_bounds: Vec<Option<Bounds>> = vec![None; count];
	for &index in &clusters_outside_in {
		let Some(frame) = final_bounds[index] else { continue };
		let (drawn, _) = write_cluster(
			&mut clusters_svg,
			&mut bbox,
			context,
			&data.nodes[index],
			to_bounds(frame),
			&theme,
		);
		cluster_bounds[index] = Some(drawn);
	}
	let node_bounds = |index: usize| {
		final_bounds.get(index).copied().flatten().filter(|_| !is_cluster(index)).map(to_bounds)
	};

	let mut drawn: Vec<(usize, Vec<Point>, Option<Point>)> = Vec::new();
	for (index, route) in routes.into_iter().enumerate() {
		let edge = &data.edges[index];
		let mut points = route.points;
		if points.len() < 2 {
			// Edges the router could not route run straight between their ends.
			let (Some(tail), Some(head)) = (
				end_bounds(index_of(&edge.start), &cluster_bounds, &node_bounds),
				end_bounds(index_of(&edge.end), &cluster_bounds, &node_bounds),
			) else {
				continue;
			};
			points = vec![
				Intersect::Rect.point(tail, Point { x: head.x, y: head.y }),
				Intersect::Rect.point(head, Point { x: tail.x, y: tail.y }),
			];
		} else {
			// Ports lie on the bounding box; move them onto the outline of the shape.
			for (end, id) in [(0, &edge.start), (1, &edge.end)] {
				let Some(node) = index_of(id).filter(|&node| !is_cluster(node)) else {
					continue;
				};
				let (Some(bounds), Some(shape)) = (node_bounds(node), measured[node].as_ref())
				else {
					continue;
				};
				if matches!(shape.shape.intersect, Intersect::Rect) {
					continue;
				}
				let last = points.len() - 1;
				let (port, next) = match end {
					0 => (points[0], points[1]),
					_ => (points[last], points[last - 1]),
				};
				if let Some(attach) =
					outline_attach_point(&shape.shape.intersect, bounds, port, next)
				{
					let position = if end == 0 { 0 } else { last };
					points[position] = attach;
				}
			}
		}
		drawn.push((index, points, route.label));
	}

	let hops = context.config.path(&["elk", "lineHops"]).cloned();
	let hops_enabled = !matches!(hops, Some(Value::Bool(false)));
	let use_gaps = hops.as_ref().and_then(Value::as_str) == Some("gap");
	let geometries: Vec<EdgeGeometry> = drawn
		.iter()
		.map(|(index, points, _)| {
			let edge = &data.edges[*index];
			let offset = |arrow: &str| edges::marker_offset(arrow).filter(|offset| *offset != 0.0);
			EdgeGeometry {
				points,
				is_rounded: true,
				start_offset: offset(&edge.arrow_type_start),
				end_offset: offset(&edge.arrow_type_end),
			}
		})
		.collect();
	let mut jumps: Vec<Option<line_jumps::JumpPath>> = match hops_enabled {
		true => line_jumps::jump_paths(&geometries, use_gaps),
		false => geometries.iter().map(|_| None).collect(),
	};
	let mut edge_paths = Svg::new();
	let mut labels_svg = Svg::new();
	let mut terminals: Vec<(usize, Vec<Point>)> = Vec::new();
	for (position, (index, points, label)) in drawn.iter().enumerate() {
		let mut edge = data.edges[*index].clone();
		edge.curve = Some("rounded".to_owned());
		let route = EdgeRoute {
			points: points.clone(),
			label_position: *label,
			from_cluster: None,
			to_cluster: None,
			jumps: jumps[position].take(),
		};
		let free = |point: Point| Bounds { x: point.x, y: point.y, width: 0.0, height: 0.0 };
		let tail = EdgeEnd { bounds: free(points[0]), intersect: None };
		let head = EdgeEnd { bounds: free(points[points.len() - 1]), intersect: None };
		let placement = edges::write_edge(
			&mut edge_paths,
			&mut bbox,
			context,
			&edge,
			&route,
			&tail,
			&head,
			&mut markers,
			&theme,
			"rounded",
		);
		let laid_label = &edge_labels[*index];
		if let Some(position) = label.or(placement.label)
			&& !laid_label.is_empty()
		{
			edges::write_edge_label(
				&mut labels_svg,
				&mut bbox,
				context,
				&data.edges[*index],
				laid_label,
				position,
				&theme,
			);
		}
		terminals.push((*index, placement.path));
	}
	for (index, path) in &terminals {
		let edge = &data.edges[*index];
		let terminals = [
			(&edge.start_label_right, edges::Terminal::StartRight, &edge.arrow_type_start),
			(&edge.end_label_left, edges::Terminal::EndLeft, &edge.arrow_type_end),
		];
		for (text, terminal, arrow) in terminals {
			if text.is_empty() {
				continue;
			}
			let marker_size = if arrow.is_empty() { 0.0 } else { 10.0 };
			let Some(position) = edges::terminal_label_position(path, terminal, marker_size) else {
				continue;
			};
			let label = edges::layout_terminal_label(context, edge, text);
			edges::write_terminal_label(
				&mut labels_svg,
				&mut bbox,
				context,
				edge,
				&label,
				position,
				&theme,
			);
		}
	}

	let mut nodes_svg = Svg::new();
	for index in (0..count).filter(|&index| !is_cluster(index)) {
		let (Some(bounds), Some(node)) = (node_bounds(index), measured[index].as_ref()) else {
			continue;
		};
		let drawing = super::NodeDrawing {
			shape: &node.shape,
			label: &node.label,
			extra_labels: &node.extra_labels,
		};
		let center = Point { x: bounds.x, y: bounds.y };
		super::write_er_background(
			&mut nodes_svg,
			context,
			&theme,
			&data.nodes[index],
			&node.shape,
			center,
		);
		super::write_node(
			&mut nodes_svg,
			&mut bbox,
			context,
			&theme,
			data.style,
			&data.nodes[index],
			&drawing,
			center,
		);
	}

	svg.open("g").attr("class", "root").end();
	svg.open("g").attr("class", "clusters").end();
	svg.append(clusters_svg);
	svg.close_tag("g");
	svg.open("g").attr("class", "edges edgePaths").end();
	svg.append(edge_paths);
	svg.close_tag("g");
	svg.open("g").attr("class", "edgeLabels").end();
	svg.append(labels_svg);
	svg.close_tag("g");
	svg.open("g").attr("class", "nodes").end();
	svg.append(nodes_svg);
	svg.close_tag("g");
	svg.close_tag("g");
	markers.write_colored(&mut svg, data.diagram_type);
	svg.close_tag("g");
	Ok((svg, bbox))
}

fn to_bounds(rect: Rect) -> Bounds {
	Bounds { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

/// The bounds of the end `node` of an edge: its cluster frame or its node.
fn end_bounds(
	node: Option<usize>,
	cluster_bounds: &[Option<Bounds>],
	node_bounds: &impl Fn(usize) -> Option<Bounds>,
) -> Option<Bounds> {
	let node = node?;
	cluster_bounds.get(node).copied().flatten().or_else(|| node_bounds(node))
}

/// `outlineAttachPoint` (`elk/geometry.ts`): where the ray from `port` into the node, against
/// the direction the edge leaves in, meets the outline of the shape, so that the edge stays
/// orthogonal. `None` for diagonal departures or when the ray cannot be resolved.
fn outline_attach_point(
	intersect: &Intersect,
	bounds: Bounds,
	port: Point,
	next: Point,
) -> Option<Point> {
	const AXIS_EPSILON: f64 = 1e-6;
	let (dx, dy) = (next.x - port.x, next.y - port.y);
	if (dx == 0.0 && dy == 0.0) || (dx.abs() > AXIS_EPSILON && dy.abs() > AXIS_EPSILON) {
		return None;
	}
	let center = Point { x: bounds.x, y: bounds.y };
	let is_inside = |probe: Point| {
		let crossing = intersect.point(bounds, probe);
		let probe_distance = (probe.x - center.x).hypot(probe.y - center.y);
		let outline_distance = (crossing.x - center.x).hypot(crossing.y - center.y);
		probe_distance <= outline_distance + 1e-9
	};
	let horizontal = dx.abs() > dy.abs();
	let along = |t: f64| match horizontal {
		true => Point { x: t, y: port.y },
		false => Point { x: port.x, y: t },
	};
	let (mut inner, mut outer) = match horizontal {
		true => (center.x, port.x),
		false => (center.y, port.y),
	};
	if !is_inside(along(inner)) {
		return None;
	}
	if is_inside(along(outer)) {
		return Some(port);
	}
	for _ in 0..OUTLINE_RAY_STEPS {
		let middle = (inner + outer) / 2.0;
		if is_inside(along(middle)) {
			inner = middle;
		} else {
			outer = middle;
		}
	}
	Some(along(inner))
}

#[cfg(test)]
mod tests;
