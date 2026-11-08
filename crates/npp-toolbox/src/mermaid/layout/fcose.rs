//! The CoSE layout of `cytoscape-fcose` 2.2.0 (`fcose/index.js`, `cose.js`, `auxiliary.js` on
//! `cose-base` 2.2.0 and `layout-base` 2.0.1) with the options of Mermaid's architecture
//! diagrams: `quality: 'proof'`, `randomize: false` (an incremental layout from the given
//! positions, so no spectral layout), no animation, compound nodes, and alignment and relative
//! placement constraints, which turn off tiling, component packing and tree reduction.
//!
//! The constraint handling of `ConstraintHandler.js` without its transformation step, which only
//! runs for randomized layouts, is in `fcose/constraints.rs`. `Math.random` is reached only by
//! the shuffling of relative placement constraints; the caller supplies it. The repulsion grid
//! is replaced by an equivalent search that visits the neighbors in the grid's order, so that
//! huge graphs cannot allocate huge grids.

mod constraints;

use super::cose_bilkent::{Rect, clip_points, separation_amount, sign};
use super::js_math;

pub use constraints::RelativePlacement;

const LEAF_REPULSION: f64 = 4500.0;
const SIMPLE_NODE_SIZE: f64 = 40.0;
const EMPTY_COMPOUND_NODE_SIZE: f64 = 40.0;
const GRAPH_MARGIN: f64 = 15.0;
const PER_LEVEL_IDEAL_EDGE_LENGTH_FACTOR: f64 = 0.1;
const GRAVITY_STRENGTH: f64 = 0.25;
const COMPOUND_GRAVITY_STRENGTH: f64 = 1.0;
const GRAVITY_RANGE_FACTOR: f64 = 3.8;
const COMPOUND_GRAVITY_RANGE_FACTOR: f64 = 1.5;
const COOLING_FACTOR_INCREMENTAL: f64 = 0.3;
const COOLING_ADAPTATION_FACTOR: f64 = 0.33;
const ADAPTATION_LOWER_NODE_LIMIT: f64 = 1000.0;
const ADAPTATION_UPPER_NODE_LIMIT: f64 = 5000.0;
const MAX_NODE_DISPLACEMENT_INCREMENTAL: f64 = 100.0;
const CONVERGENCE_CHECK_PERIOD: usize = 100;
const GRID_CALCULATION_CHECK_PERIOD: usize = 10;
const INTEGER_MAX: f64 = 2_147_483_647.0;
/// Iterations after which the layout stops even if JavaScript would go on: a fractional or
/// huge `numIter` is only ended by convergence there.
const ITERATION_LIMIT: usize = 100_000;

/// A node of the cytoscape graph, in the order of the cytoscape elements.
#[derive(Clone, Debug)]
pub struct Node {
	pub parent: Option<usize>,
	/// The cytoscape position: the center.
	pub x: f64,
	pub y: f64,
	/// `layoutDimensions`.
	pub width: f64,
	pub height: f64,
	/// `parseInt` of the `padding` style, the margin of a compound's child graph.
	pub padding: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Edge {
	pub source: usize,
	pub target: usize,
	pub ideal_length: f64,
	pub elasticity: f64,
}

/// The input of [`layout`].
pub struct Input<'a> {
	pub nodes: &'a [Node],
	pub edges: &'a [Edge],
	/// `alignmentConstraint.vertical`: lists of nodes that share a center x.
	pub vertical: Option<&'a [Vec<usize>]>,
	/// `alignmentConstraint.horizontal`: lists of nodes that share a center y.
	pub horizontal: Option<&'a [Vec<usize>]>,
	pub relative: Option<&'a [RelativePlacement]>,
	/// `numIter`.
	pub iterations: f64,
	/// The center of the cytoscape bounding box of all elements before the layout, which the
	/// result is moved to (`aux.relocateComponent`).
	pub center: (f64, f64),
}

/// `FDLayout.calcGrid` failed with `RangeError: Invalid array length`, which happens for
/// constraints that cannot be satisfied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidGrid;

/// `LNode`; index 0 is the parent node of the root graph, which no graph lists.
#[derive(Clone, Debug, Default)]
struct LNode {
	owner: usize,
	child: Option<usize>,
	rect: Rect,
	edges: Vec<usize>,
	spring: (f64, f64),
	repulsion: (f64, f64),
	gravitation: (f64, f64),
	displacement: (f64, f64),
	no_of_children: f64,
	estimated_size: f64,
	depth: usize,
	/// The `paddingLeft` of the node, which is undefined for the parent of the root graph.
	padding: Option<f64>,
	cells: [i64; 4],
	surrounding: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
struct LGraph {
	parent: usize,
	nodes: Vec<usize>,
	edges: Vec<usize>,
	left: f64,
	right: f64,
	top: f64,
	bottom: f64,
	estimated_size: f64,
	is_connected: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct LEdge {
	source: usize,
	target: usize,
	ideal_length: f64,
	elasticity: f64,
	is_inter_graph: bool,
	lca: usize,
	source_in_lca: usize,
	target_in_lca: usize,
	length: f64,
	length_x: f64,
	length_y: f64,
}

/// The constants that `cose.js` sets from the options before the layout.
#[derive(Clone, Copy, Debug)]
struct Constants {
	/// `DEFAULT_EDGE_LENGTH`: the average ideal edge length.
	edge_length: f64,
	/// `MIN_REPULSION_DIST`.
	min_repulsion_distance: f64,
}

struct Layout<'r> {
	nodes: Vec<LNode>,
	graphs: Vec<LGraph>,
	edges: Vec<LEdge>,
	/// `getAllNodes`: the nodes of the graphs in the order of the graphs.
	all_nodes: Vec<usize>,
	/// The position of each node in `all_nodes`.
	order: Vec<usize>,
	gravity_nodes: Vec<usize>,
	constants: Constants,
	ideal_edge_length: f64,
	max_iterations: f64,
	total_iterations: usize,
	cooling_cycle: f64,
	max_cooling_cycle: f64,
	final_temperature: f64,
	cooling_factor: f64,
	initial_cooling_factor: f64,
	total_displacement: f64,
	old_total_displacement: f64,
	total_displacement_threshold: f64,
	repulsion_range: f64,
	/// The size of the repulsion grid.
	grid_size: (i64, i64),
	constraints: Option<constraints::Movement>,
	random: &'r mut dyn FnMut() -> f64,
}

/// Lays out the nodes from their positions and returns the new centers of the nodes that are
/// not compounds (cytoscape places compounds around their children).
pub fn layout(
	input: &Input,
	random: &mut dyn FnMut() -> f64,
) -> Result<Vec<Option<(f64, f64)>>, InvalidGrid> {
	let count = input.nodes.len();
	let mut children: Vec<Vec<usize>> = vec![Vec::new(); count];
	let mut top_most = Vec::new();
	for (index, node) in input.nodes.iter().enumerate() {
		match node.parent.filter(|&parent| parent < count && parent != index) {
			Some(parent) => children[parent].push(index),
			None => top_most.push(index),
		}
	}
	let mut layout = Layout {
		nodes: vec![LNode::default()],
		graphs: vec![LGraph { parent: 0, ..LGraph::default() }],
		edges: Vec::new(),
		all_nodes: Vec::new(),
		order: Vec::new(),
		gravity_nodes: Vec::new(),
		constants: Constants { edge_length: 50.0, min_repulsion_distance: 5.0 },
		ideal_edge_length: 50.0,
		max_iterations: 0.0,
		total_iterations: 0,
		cooling_cycle: 0.0,
		max_cooling_cycle: 0.0,
		final_temperature: 0.0,
		cooling_factor: COOLING_FACTOR_INCREMENTAL,
		initial_cooling_factor: COOLING_FACTOR_INCREMENTAL,
		total_displacement: 0.0,
		old_total_displacement: 0.0,
		total_displacement_threshold: 0.0,
		repulsion_range: 0.0,
		grid_size: (0, 0),
		constraints: None,
		random,
	};
	// `processChildrenList`, depth first like its recursion: a compound's child graph is
	// created right after the compound.
	let mut layout_node_of = vec![0; count];
	let mut stack: Vec<(usize, std::vec::IntoIter<usize>)> = vec![(0, top_most.into_iter())];
	while let Some((graph, members)) = stack.last_mut() {
		let graph = *graph;
		let Some(member) = members.next() else {
			stack.pop();
			continue;
		};
		let node = &input.nodes[member];
		let index = layout.nodes.len();
		layout_node_of[member] = index;
		let x = node.x - node.width / 2.0;
		let y = node.y - node.height / 2.0;
		layout.nodes.push(LNode {
			owner: graph,
			rect: Rect {
				x: if x.is_nan() { 0.0 } else { x },
				y: if y.is_nan() { 0.0 } else { y },
				width: node.width,
				height: node.height,
			},
			padding: Some(node.padding),
			..LNode::default()
		});
		layout.graphs[graph].nodes.push(index);
		if !children[member].is_empty() {
			let child_graph = layout.graphs.len();
			layout.graphs.push(LGraph { parent: index, ..LGraph::default() });
			layout.nodes[index].child = Some(child_graph);
			stack.push((child_graph, std::mem::take(&mut children[member]).into_iter()));
		}
	}
	// `processEdges`: one edge between two nodes at most.
	let mut ideal_length_total = 0.0;
	let mut edge_count = 0.0;
	let mut inter_graph_edges = Vec::new();
	for edge in input.edges {
		if edge.source >= count || edge.target >= count {
			continue;
		}
		let (source, target) = (layout_node_of[edge.source], layout_node_of[edge.target]);
		if source == target || layout.edge_between(source, target) {
			continue;
		}
		let index = layout.edges.len();
		let source_graph = layout.nodes[source].owner;
		let is_inter_graph = source_graph != layout.nodes[target].owner;
		layout.edges.push(LEdge {
			source,
			target,
			ideal_length: edge.ideal_length,
			elasticity: edge.elasticity,
			is_inter_graph,
			..LEdge::default()
		});
		match is_inter_graph {
			true => inter_graph_edges.push(index),
			false => layout.graphs[source_graph].edges.push(index),
		}
		layout.nodes[source].edges.push(index);
		layout.nodes[target].edges.push(index);
		ideal_length_total += edge.ideal_length;
		edge_count += 1.0;
	}
	// The ideal length option is a function, so without edges the default stays.
	let edge_length = if edge_count > 0.0 { ideal_length_total / edge_count } else { 50.0 };
	layout.constants = Constants { edge_length, min_repulsion_distance: edge_length / 10.0 };
	layout.ideal_edge_length = if edge_length < 10.0 { 10.0 } else { edge_length };
	layout.max_iterations = input.iterations;
	layout.all_nodes = layout.graphs.iter().flat_map(|graph| graph.nodes.iter().copied()).collect();
	layout.order = vec![0; layout.nodes.len()];
	for (position, &node) in layout.all_nodes.iter().enumerate() {
		layout.order[node] = position;
	}
	// `getAllEdges`: the edges of the graphs, then the inter-graph edges.
	let mut all_edges: Vec<usize> =
		layout.graphs.iter().flat_map(|graph| graph.edges.iter().copied()).collect();
	all_edges.extend(inter_graph_edges);
	let edges: Vec<LEdge> = all_edges.iter().map(|&edge| layout.edges[edge]).collect();
	let mut renumbered = vec![0; edges.len()];
	for (new, &old) in all_edges.iter().enumerate() {
		renumbered[old] = new;
	}
	layout.edges = edges;
	for node in &mut layout.nodes {
		for edge in &mut node.edges {
			*edge = renumbered[*edge];
		}
	}
	for graph in &mut layout.graphs {
		for edge in &mut graph.edges {
			*edge = renumbered[*edge];
		}
	}
	if layout.graphs[0].nodes.is_empty() {
		return Ok(vec![None; count]);
	}
	let map_lists = |lists: Option<&[Vec<usize>]>| {
		lists.map(|lists| {
			lists
				.iter()
				.map(|list| {
					list.iter().filter_map(|&node| layout_node_of.get(node).copied()).collect()
				})
				.collect()
		})
	};
	let mut constraints = constraints::Constraints {
		vertical: map_lists(input.vertical),
		horizontal: map_lists(input.horizontal),
		relative: input.relative.map(|relative| {
			relative
				.iter()
				.filter(|constraint| constraint.first < count && constraint.second < count)
				.map(|constraint| RelativePlacement {
					first: layout_node_of[constraint.first],
					second: layout_node_of[constraint.second],
					..*constraint
				})
				.collect()
		}),
	};
	layout.classic_layout(&mut constraints)?;
	layout.update_bounds();
	layout.relocate(input.center);
	Ok((0..count)
		.map(|member| {
			let node = &layout.nodes[layout_node_of[member]];
			node.child.is_none().then(|| (node.rect.center_x(), node.rect.center_y()))
		})
		.collect())
}

impl Layout<'_> {
	/// `getEdgesBetween(other).length > 0`.
	fn edge_between(&self, node: usize, other: usize) -> bool {
		self.nodes[node].edges.iter().any(|&edge| {
			let edge = &self.edges[edge];
			edge.source == other || edge.target == other
		})
	}

	/// `CoSELayout.classicLayout` of an incremental layout with constraints.
	fn classic_layout(
		&mut self,
		constraints: &mut constraints::Constraints,
	) -> Result<(), InvalidGrid> {
		self.calculate_gravity_nodes();
		for index in 1..self.nodes.len() {
			self.nodes[index].no_of_children = self.no_of_children(index);
		}
		self.calc_lowest_common_ancestors();
		self.calc_inclusion_tree_depths();
		self.calc_estimated_sizes();
		self.calc_ideal_edge_lengths();
		// The alignment constraint is always given, if only with empty lists.
		self.constraints = Some(constraints::handle(self, constraints));
		self.init_spring_embedder();
		while !self.tick()? {}
		Ok(())
	}

	/// `calculateNodesToApplyGravitationTo`: the nodes of the graphs that are not connected.
	fn calculate_gravity_nodes(&mut self) {
		let mut nodes = Vec::new();
		for graph in 0..self.graphs.len() {
			self.update_connected(graph);
			if !self.graphs[graph].is_connected {
				nodes.extend(self.graphs[graph].nodes.iter().copied());
			}
		}
		self.gravity_nodes = nodes;
	}

	/// The node and its descendants (`withChildren`).
	fn with_children(&self, node: usize) -> Vec<usize> {
		let mut result = Vec::new();
		let mut stack = vec![node];
		while let Some(node) = stack.pop() {
			result.push(node);
			if let Some(child) = self.nodes[node].child {
				stack.extend(self.graphs[child].nodes.iter().rev());
			}
		}
		result
	}

	/// `LEdge.getOtherEndInGraph`.
	fn other_end_in_graph(&self, edge: usize, node: usize, graph: usize) -> Option<usize> {
		let edge = &self.edges[edge];
		let mut other = if edge.source == node { edge.target } else { edge.source };
		loop {
			let owner = self.nodes[other].owner;
			if owner == graph {
				return Some(other);
			}
			if owner == 0 {
				return None;
			}
			other = self.graphs[owner].parent;
		}
	}

	/// `LGraph.updateConnected`.
	fn update_connected(&mut self, graph: usize) {
		let nodes = &self.graphs[graph].nodes;
		if nodes.is_empty() {
			self.graphs[graph].is_connected = true;
			return;
		}
		let mut visited = vec![false; self.nodes.len()];
		let mut queue = std::collections::VecDeque::new();
		for node in self.with_children(nodes[0]) {
			visited[node] = true;
			queue.push_back(node);
		}
		while let Some(current) = queue.pop_front() {
			for &edge in &self.nodes[current].edges {
				if let Some(neighbor) = self.other_end_in_graph(edge, current, graph)
					&& !visited[neighbor]
				{
					for node in self.with_children(neighbor) {
						visited[node] = true;
						queue.push_back(node);
					}
				}
			}
		}
		let nodes = &self.graphs[graph].nodes;
		let visited_count = visited.iter().filter(|&&visited| visited).count();
		let in_graph = nodes.iter().filter(|&&node| visited[node]).count();
		self.graphs[graph].is_connected = visited_count >= nodes.len() && in_graph == nodes.len();
	}

	/// `LNode.getNoOfChildren`: the number of leaves below a compound, 1 for a leaf.
	fn no_of_children(&self, node: usize) -> f64 {
		let leaves = self
			.with_children(node)
			.into_iter()
			.filter(|&descendant| descendant != node && self.nodes[descendant].child.is_none())
			.count();
		match self.nodes[node].child {
			None => 1.0,
			Some(_) => leaves.max(1) as f64,
		}
	}

	/// `LGraphManager.calcLowestCommonAncestors`.
	fn calc_lowest_common_ancestors(&mut self) {
		for index in 0..self.edges.len() {
			let LEdge { source, target, .. } = self.edges[index];
			let mut lca = None;
			let mut source_in_lca = source;
			let mut target_in_lca = target;
			let mut source_graph = self.nodes[source].owner;
			while lca.is_none() {
				target_in_lca = target;
				let mut target_graph = self.nodes[target].owner;
				loop {
					if target_graph == source_graph {
						lca = Some(target_graph);
						break;
					}
					if target_graph == 0 {
						break;
					}
					target_in_lca = self.graphs[target_graph].parent;
					target_graph = self.nodes[target_in_lca].owner;
				}
				if source_graph == 0 {
					break;
				}
				if lca.is_none() {
					source_in_lca = self.graphs[source_graph].parent;
					source_graph = self.nodes[source_in_lca].owner;
				}
			}
			let edge = &mut self.edges[index];
			edge.lca = lca.unwrap_or(0);
			edge.source_in_lca = source_in_lca;
			edge.target_in_lca = target_in_lca;
		}
	}

	/// `LGraphManager.calcInclusionTreeDepths`: the graphs come before their child graphs.
	fn calc_inclusion_tree_depths(&mut self) {
		for graph in 0..self.graphs.len() {
			let depth = match graph {
				0 => 1,
				_ => self.nodes[self.graphs[graph].parent].depth + 1,
			};
			for position in 0..self.graphs[graph].nodes.len() {
				let node = self.graphs[graph].nodes[position];
				self.nodes[node].depth = depth;
			}
		}
	}

	/// `LGraph.getInclusionTreeDepth`.
	fn graph_depth(&self, graph: usize) -> usize {
		match graph {
			0 => 1,
			_ => self.nodes[self.graphs[graph].parent].depth,
		}
	}

	/// `calcEstimatedSize` of the root graph: child graphs come after their parents, so the
	/// reverse order computes them first. Compounds take their estimated size as their size.
	fn calc_estimated_sizes(&mut self) {
		for graph in (0..self.graphs.len()).rev() {
			let mut size = 0.0;
			for position in 0..self.graphs[graph].nodes.len() {
				let node = self.graphs[graph].nodes[position];
				let estimated = match self.nodes[node].child {
					None => (self.nodes[node].rect.width + self.nodes[node].rect.height) / 2.0,
					Some(child) => {
						let estimated = self.graphs[child].estimated_size;
						self.nodes[node].rect.width = estimated;
						self.nodes[node].rect.height = estimated;
						estimated
					}
				};
				self.nodes[node].estimated_size = estimated;
				size += estimated;
			}
			let count = self.graphs[graph].nodes.len() as f64;
			self.graphs[graph].estimated_size =
				if size == 0.0 { EMPTY_COMPOUND_NODE_SIZE } else { size / count.sqrt() };
		}
	}

	/// `FDLayout.calcIdealEdgeLengths` with the smart calculation.
	fn calc_ideal_edge_lengths(&mut self) {
		for index in 0..self.edges.len() {
			let edge = self.edges[index];
			if !edge.is_inter_graph {
				continue;
			}
			let original = edge.ideal_length;
			let mut ideal = original;
			ideal += self.nodes[edge.source_in_lca].estimated_size
				+ self.nodes[edge.target_in_lca].estimated_size
				- 2.0 * SIMPLE_NODE_SIZE;
			let lca_depth = self.graph_depth(edge.lca) as f64;
			ideal += original
				* PER_LEVEL_IDEAL_EDGE_LENGTH_FACTOR
				* (self.nodes[edge.source].depth as f64 + self.nodes[edge.target].depth as f64
					- 2.0 * lca_depth);
			self.edges[index].ideal_length = ideal;
		}
	}

	/// `FDLayout.initSpringEmbedder` and `CoSELayout.initSpringEmbedder` of an incremental
	/// layout.
	fn init_spring_embedder(&mut self) {
		let count = self.all_nodes.len() as f64;
		if count > ADAPTATION_LOWER_NODE_LIMIT {
			self.cooling_factor = f64::max(
				self.cooling_factor * COOLING_ADAPTATION_FACTOR,
				self.cooling_factor
					- (count - ADAPTATION_LOWER_NODE_LIMIT)
						/ (ADAPTATION_UPPER_NODE_LIMIT - ADAPTATION_LOWER_NODE_LIMIT)
						* self.cooling_factor
						* (1.0 - COOLING_ADAPTATION_FACTOR),
			);
		}
		self.max_iterations = js_max(count * 5.0, self.max_iterations);
		self.total_displacement_threshold = (3.0 * self.constants.edge_length) / 100.0 * count;
		// `2 * (level + 1) * idealEdgeLength`.
		self.repulsion_range = 2.0 * (0.0 + 1.0) * self.ideal_edge_length;
		self.cooling_cycle = 0.0;
		self.max_cooling_cycle = self.max_iterations / CONVERGENCE_CHECK_PERIOD as f64;
		self.final_temperature = 0.04;
	}

	/// `CoSELayout.tick` without tree growing: true when the layout is done.
	fn tick(&mut self) -> Result<bool, InvalidGrid> {
		self.total_iterations += 1;
		if self.total_iterations as f64 == self.max_iterations
			|| self.total_iterations > ITERATION_LIMIT
		{
			return Ok(true);
		}
		if self.total_iterations.is_multiple_of(CONVERGENCE_CHECK_PERIOD) {
			if self.is_converged() {
				return Ok(true);
			}
			self.cooling_cycle += 1.0;
			// Quality `proof`: the cooling adjuster stays 1.
			let exponent =
				js_math::log(100.0 * (self.initial_cooling_factor - self.final_temperature))
					/ js_math::log(self.max_cooling_cycle);
			let cooled = self.initial_cooling_factor
				- js_math::pow(self.cooling_cycle, exponent) / 100.0 * 1.0;
			self.cooling_factor = js_max(cooled, self.final_temperature);
		}
		self.total_displacement = 0.0;
		self.update_bounds();
		self.calc_spring_forces();
		self.calc_repulsion_forces()?;
		self.calc_gravitational_forces();
		self.move_nodes();
		Ok(false)
	}

	/// `FDLayout.isConverged`.
	fn is_converged(&mut self) -> bool {
		let mut oscillating = false;
		if self.total_iterations as f64 > self.max_iterations / 3.0 {
			oscillating = (self.total_displacement - self.old_total_displacement).abs() < 2.0;
		}
		let converged = self.total_displacement < self.total_displacement_threshold;
		self.old_total_displacement = self.total_displacement;
		converged || oscillating
	}

	/// `LGraphManager.updateBounds`: child graphs come after their parents, so the reverse
	/// order updates them first; a compound takes the bounds of its child graph.
	fn update_bounds(&mut self) {
		for graph in (0..self.graphs.len()).rev() {
			let (mut left, mut right, mut top, mut bottom) =
				(INTEGER_MAX, -INTEGER_MAX, INTEGER_MAX, -INTEGER_MAX);
			for &node in &self.graphs[graph].nodes {
				let rect = &self.nodes[node].rect;
				if left > rect.x {
					left = rect.x;
				}
				if right < rect.right() {
					right = rect.right();
				}
				if top > rect.y {
					top = rect.y;
				}
				if bottom < rect.bottom() {
					bottom = rect.bottom();
				}
			}
			let parent = self.graphs[graph].parent;
			let margin = self.nodes[parent].padding.unwrap_or(GRAPH_MARGIN);
			let bounds = Rect { x: left, y: top, width: right - left, height: bottom - top };
			let graph_data = &mut self.graphs[graph];
			graph_data.left = bounds.x - margin;
			graph_data.right = bounds.x + bounds.width + margin;
			graph_data.top = bounds.y - margin;
			graph_data.bottom = bounds.y + bounds.height + margin;
			if graph != 0 {
				let (left, top) = (graph_data.left, graph_data.top);
				let (width, height) =
					(graph_data.right - graph_data.left, graph_data.bottom - graph_data.top);
				self.nodes[parent].rect = Rect { x: left, y: top, width, height };
			}
		}
	}

	fn calc_spring_forces(&mut self) {
		for index in 0..self.edges.len() {
			let LEdge { source, target, ideal_length, elasticity, .. } = self.edges[index];
			// `LEdge.updateLength`.
			let (overlapping, clip) =
				clip_points(&self.nodes[target].rect, &self.nodes[source].rect);
			if overlapping {
				continue;
			}
			let edge = &mut self.edges[index];
			edge.length_x = clip[0] - clip[2];
			edge.length_y = clip[1] - clip[3];
			if edge.length_x.abs() < 1.0 {
				edge.length_x = sign(edge.length_x);
			}
			if edge.length_y.abs() < 1.0 {
				edge.length_y = sign(edge.length_y);
			}
			edge.length = (edge.length_x * edge.length_x + edge.length_y * edge.length_y).sqrt();
			let length = edge.length;
			if length == 0.0 {
				continue;
			}
			let spring_force = elasticity * (length - ideal_length);
			let force_x = spring_force * (edge.length_x / length);
			let force_y = spring_force * (edge.length_y / length);
			let source = &mut self.nodes[source].spring;
			source.0 += force_x;
			source.1 += force_y;
			let target = &mut self.nodes[target].spring;
			target.0 -= force_x;
			target.1 -= force_y;
		}
	}

	/// `FDLayout.calcRepulsionForces` with the FR grid variant.
	fn calc_repulsion_forces(&mut self) -> Result<(), InvalidGrid> {
		if self.total_iterations % GRID_CALCULATION_CHECK_PERIOD == 1 {
			self.update_grid()?;
			let columns = self.columns();
			let mut seen = vec![usize::MAX; self.nodes.len()];
			for position in 0..self.all_nodes.len() {
				let node = self.all_nodes[position];
				self.nodes[node].surrounding = self.surrounding(node, &columns, &mut seen);
			}
		}
		for position in 0..self.all_nodes.len() {
			let node = self.all_nodes[position];
			for index in 0..self.nodes[node].surrounding.len() {
				let other = self.nodes[node].surrounding[index];
				self.calc_repulsion_force(node, other);
			}
		}
		Ok(())
	}

	/// `FDLayout.updateGrid`: the grid size and the cells of every node. `calcGrid` throws for
	/// sizes that are no valid array length.
	fn update_grid(&mut self) -> Result<(), InvalidGrid> {
		let root = &self.graphs[0];
		let (left, right, top, bottom) = (root.left, root.right, root.top, root.bottom);
		let range = self.repulsion_range;
		let columns = js_parse_int(((right - left) / range).ceil());
		let rows = js_parse_int(((bottom - top) / range).ceil());
		let is_array_length = |length: f64| (0.0..4_294_967_296.0).contains(&length);
		if !is_array_length(columns) || (columns > 0.0 && !is_array_length(rows)) {
			return Err(InvalidGrid);
		}
		self.grid_size = (columns as i64, rows as i64);
		for position in 0..self.all_nodes.len() {
			let node = &mut self.nodes[self.all_nodes[position]];
			let rect = &node.rect;
			node.cells = [
				grid_cell(((rect.x - left) / range).floor()),
				grid_cell(((rect.width + rect.x - left) / range).floor()),
				grid_cell(((rect.y - top) / range).floor()),
				grid_cell(((rect.height + rect.y - top) / range).floor()),
			];
		}
		Ok(())
	}

	/// The nodes by the grid columns they fall into, in the order of `all_nodes`; nodes across
	/// more than `WIDE` columns are listed under `None` instead, so that huge nodes cost no
	/// memory.
	fn columns(&self) -> std::collections::HashMap<Option<i64>, Vec<usize>> {
		const WIDE: i64 = 32;
		let mut columns: std::collections::HashMap<Option<i64>, Vec<usize>> =
			std::collections::HashMap::new();
		let last_column = self.grid_size.0 - 1;
		for &node in &self.all_nodes {
			let cells = self.nodes[node].cells;
			let (start, finish) = (cells[0].max(0), cells[1].min(last_column));
			if finish - start > WIDE {
				columns.entry(None).or_default().push(node);
			} else {
				for column in start..=finish {
					columns.entry(Some(column)).or_default().push(node);
				}
			}
		}
		columns
	}

	/// The surrounding of `node` that `calculateRepulsionForceOfANode` finds in the grid: the
	/// later nodes of the same graph in a neighboring cell within the repulsion range, in the
	/// order of the cells (column, then row) and the nodes in them.
	fn surrounding(
		&self,
		node: usize,
		columns: &std::collections::HashMap<Option<i64>, Vec<usize>>,
		seen: &mut [usize],
	) -> Vec<usize> {
		let data = &self.nodes[node];
		let [start_x, finish_x, start_y, finish_y] = data.cells;
		let (column_count, rows) = self.grid_size;
		let scan_x = ((start_x - 1).max(0), (finish_x + 1).min(column_count - 1));
		let scan_y = ((start_y - 1).max(0), (finish_y + 1).min(rows - 1));
		let mut found: Vec<(i64, i64, usize, usize)> = Vec::new();
		let candidates = (scan_x.0..=scan_x.1)
			.filter_map(|column| columns.get(&Some(column)))
			.chain(columns.get(&None))
			.flatten();
		let position = self.order[node];
		for &other in candidates {
			let other_position = self.order[other];
			if other_position <= position || seen[other] == node {
				continue;
			}
			seen[other] = node;
			let other_data = &self.nodes[other];
			if other_data.owner != data.owner {
				continue;
			}
			let [other_start_x, other_finish_x, other_start_y, other_finish_y] = other_data.cells;
			// The other node's cells within the grid.
			let cells_x = (other_start_x.max(0), other_finish_x.min(column_count - 1));
			let cells_y = (other_start_y.max(0), other_finish_y.min(rows - 1));
			let first_x = scan_x.0.max(cells_x.0);
			let first_y = scan_y.0.max(cells_y.0);
			if first_x > scan_x.1.min(cells_x.1) || first_y > scan_y.1.min(cells_y.1) {
				continue;
			}
			let (a, b) = (&data.rect, &other_data.rect);
			let distance_x =
				(a.center_x() - b.center_x()).abs() - ((a.width / 2.0) + (b.width / 2.0));
			let distance_y =
				(a.center_y() - b.center_y()).abs() - ((a.height / 2.0) + (b.height / 2.0));
			if distance_x <= self.repulsion_range && distance_y <= self.repulsion_range {
				found.push((first_x, first_y, other_position, other));
			}
		}
		found.sort_unstable();
		found.into_iter().map(|(_, _, _, other)| other).collect()
	}

	/// `FDLayout.calcRepulsionForce` of nodes without uniform sizes.
	fn calc_repulsion_force(&mut self, a: usize, b: usize) {
		let (rect_a, rect_b) = (self.nodes[a].rect, self.nodes[b].rect);
		let (children_a, children_b) = (self.nodes[a].no_of_children, self.nodes[b].no_of_children);
		let (force_a, force_b);
		if rect_a.intersects(&rect_b) {
			let (overlap_x, overlap_y) =
				separation_amount(&rect_a, &rect_b, self.constants.edge_length / 2.0);
			let (force_x, force_y) = (2.0 * overlap_x, 2.0 * overlap_y);
			let children = children_a * children_b / (children_a + children_b);
			force_a = (-(children * force_x), -(children * force_y));
			force_b = (children * force_x, children * force_y);
		} else {
			let (_, clip) = clip_points(&rect_a, &rect_b);
			let mut distance_x = clip[2] - clip[0];
			let mut distance_y = clip[3] - clip[1];
			let minimum = self.constants.min_repulsion_distance;
			if distance_x.abs() < minimum {
				distance_x = sign(distance_x) * minimum;
			}
			if distance_y.abs() < minimum {
				distance_y = sign(distance_y) * minimum;
			}
			let distance_squared = distance_x * distance_x + distance_y * distance_y;
			let distance = distance_squared.sqrt();
			let force = (LEAF_REPULSION / 2.0 + LEAF_REPULSION / 2.0) * children_a * children_b
				/ distance_squared;
			let (force_x, force_y) = (force * distance_x / distance, force * distance_y / distance);
			force_a = (-force_x, -force_y);
			force_b = (force_x, force_y);
		}
		let node = &mut self.nodes[a].repulsion;
		node.0 += force_a.0;
		node.1 += force_a.1;
		let node = &mut self.nodes[b].repulsion;
		node.0 += force_b.0;
		node.1 += force_b.1;
	}

	/// `FDLayout.calcGravitationalForces`.
	fn calc_gravitational_forces(&mut self) {
		for position in 0..self.gravity_nodes.len() {
			let node = self.gravity_nodes[position];
			let owner = &self.graphs[self.nodes[node].owner];
			let center_x = (owner.right + owner.left) / 2.0;
			let center_y = (owner.top + owner.bottom) / 2.0;
			let is_root = self.nodes[node].owner == 0;
			let range_factor =
				if is_root { GRAVITY_RANGE_FACTOR } else { COMPOUND_GRAVITY_RANGE_FACTOR };
			let estimated_size = owner.estimated_size * range_factor;
			let node = &mut self.nodes[node];
			let distance_x = node.rect.center_x() - center_x;
			let distance_y = node.rect.center_y() - center_y;
			let abs_distance_x = distance_x.abs() + node.rect.width / 2.0;
			let abs_distance_y = distance_y.abs() + node.rect.height / 2.0;
			if abs_distance_x > estimated_size || abs_distance_y > estimated_size {
				node.gravitation = match is_root {
					true => (-GRAVITY_STRENGTH * distance_x, -GRAVITY_STRENGTH * distance_y),
					false => (
						-GRAVITY_STRENGTH * distance_x * COMPOUND_GRAVITY_STRENGTH,
						-GRAVITY_STRENGTH * distance_y * COMPOUND_GRAVITY_STRENGTH,
					),
				};
			}
		}
	}

	/// `CoSELayout.moveNodes`: displacements, constraints, then moves.
	fn move_nodes(&mut self) {
		for position in 0..self.all_nodes.len() {
			let node = self.all_nodes[position];
			self.calculate_displacement(node);
		}
		if let Some(mut movement) = self.constraints.take() {
			movement.update_displacements(self);
			self.constraints = Some(movement);
		}
		for position in 0..self.all_nodes.len() {
			let node = &mut self.nodes[self.all_nodes[position]];
			if node.child.is_none() {
				node.rect.x += node.displacement.0;
				node.rect.y += node.displacement.1;
				self.total_displacement += node.displacement.0.abs() + node.displacement.1.abs();
			}
			node.spring = (0.0, 0.0);
			node.repulsion = (0.0, 0.0);
			node.gravitation = (0.0, 0.0);
			node.displacement = (0.0, 0.0);
		}
	}

	/// `CoSENode.calculateDisplacement`: a compound passes its displacement on to its leaves.
	fn calculate_displacement(&mut self, index: usize) {
		let limit = self.cooling_factor * MAX_NODE_DISPLACEMENT_INCREMENTAL;
		let cooling_factor = self.cooling_factor;
		let node = &mut self.nodes[index];
		node.displacement.0 += cooling_factor
			* (node.spring.0 + node.repulsion.0 + node.gravitation.0)
			/ node.no_of_children;
		node.displacement.1 += cooling_factor
			* (node.spring.1 + node.repulsion.1 + node.gravitation.1)
			/ node.no_of_children;
		if node.displacement.0.abs() > limit {
			node.displacement.0 = limit * sign(node.displacement.0);
		}
		if node.displacement.1.abs() > limit {
			node.displacement.1 = limit * sign(node.displacement.1);
		}
		let displacement = node.displacement;
		if node.child.is_some() {
			for descendant in self.with_children(index) {
				let descendant = &mut self.nodes[descendant];
				if descendant.child.is_none() {
					descendant.displacement.0 += displacement.0;
					descendant.displacement.1 += displacement.1;
				}
			}
		}
	}

	/// `aux.relocateComponent`: moves the center of the bounding box of all nodes to `center`.
	fn relocate(&mut self, center: (f64, f64)) {
		let (mut min_x, mut max_x) = (f64::INFINITY, f64::NEG_INFINITY);
		let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
		for &node in &self.all_nodes {
			let rect = &self.nodes[node].rect;
			if rect.x < min_x {
				min_x = rect.x;
			}
			if rect.x + rect.width > max_x {
				max_x = rect.x + rect.width;
			}
			if rect.y < min_y {
				min_y = rect.y;
			}
			if rect.y + rect.height > max_y {
				max_y = rect.y + rect.height;
			}
		}
		let diff_x = center.0 - (max_x + min_x) / 2.0;
		let diff_y = center.1 - (max_y + min_y) / 2.0;
		for position in 0..self.all_nodes.len() {
			let rect = &mut self.nodes[self.all_nodes[position]].rect;
			let (x, y) = (rect.center_x() + diff_x, rect.center_y() + diff_y);
			rect.x = x - rect.width / 2.0;
			rect.y = y - rect.height / 2.0;
		}
	}

	/// `getCenterX`/`getCenterY` of a node.
	fn center(&self, node: usize) -> (f64, f64) {
		(self.nodes[node].rect.center_x(), self.nodes[node].rect.center_y())
	}

	/// `setCenter`.
	fn set_center(&mut self, node: usize, x: f64, y: f64) {
		let rect = &mut self.nodes[node].rect;
		rect.x = x - rect.width / 2.0;
		rect.y = y - rect.height / 2.0;
	}
}

/// `Math.max` of JavaScript, which propagates `NaN`.
fn js_max(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// `parseInt` of a JavaScript number with an integral value: the number as text, up to the
/// first character that is no digit, so `1e+21` gives 1. `NaN` stays.
fn js_parse_int(value: f64) -> f64 {
	if !value.is_finite() {
		return f64::NAN;
	}
	if value.abs() < 1e21 {
		return value.trunc();
	}
	let text = format!("{:e}", value.abs());
	let digit = text.chars().next().and_then(|digit| digit.to_digit(10)).unwrap_or(0);
	f64::from(digit).copysign(value)
}

/// A grid cell index of `parseInt(Math.floor(…))`, bounded so that neighbor cells cannot
/// overflow; `NaN` cells match no cell.
fn grid_cell(value: f64) -> i64 {
	const LIMIT: f64 = 1e15;
	let value = js_parse_int(value);
	if value.is_nan() { i64::MIN / 4 } else { value.clamp(-LIMIT, LIMIT) as i64 }
}
