//! The CoSE layout of `cytoscape-cose-bilkent` 4.1.0 (`cose-base` 1.0.3 on `layout-base` 1.0.2)
//! for flat graphs, with the options of Mermaid's mindmaps: `quality: 'proof'`, `randomize`
//! (non-incremental), no animation, no compound nodes.
//!
//! A forest starts from `positionNodesRadially`; other graphs are scattered by `RandomSeed`
//! starting at its initial seed, like the first layout in a page. Tree reduction, which only
//! runs for graphs that are no forest and draws on `Math.random`, is left out: mindmaps are
//! trees. The repulsion grid is replaced by an equivalent search that visits the neighbors in
//! the grid's order, so that huge nodes cannot allocate huge grids.

use super::dagre::Point;

const DEFAULT_EDGE_LENGTH: f64 = 50.0;
const SPRING_STRENGTH: f64 = 0.45;
const REPULSION_STRENGTH: f64 = 4500.0;
const GRAVITY_STRENGTH: f64 = 0.25;
const GRAVITY_RANGE_FACTOR: f64 = 3.8;
const MAX_ITERATIONS: usize = 2500;
const CONVERGENCE_CHECK_PERIOD: usize = 100;
const GRID_CALCULATION_CHECK_PERIOD: usize = 10;
const MAX_NODE_DISPLACEMENT: f64 = 300.0;
const MIN_REPULSION_DIST: f64 = DEFAULT_EDGE_LENGTH / 10.0;
const GRAPH_MARGIN: f64 = 15.0;
const RADIAL_SEPARATION: f64 = DEFAULT_EDGE_LENGTH;
const COMPONENT_SEPARATION: f64 = 60.0;
const WORLD_BOUNDARY: f64 = 1_000_000.0;
const INITIAL_WORLD_BOUNDARY: f64 = WORLD_BOUNDARY / 1000.0;
const WORLD_CENTER_X: f64 = 1200.0;
const WORLD_CENTER_Y: f64 = 900.0;
const INTEGER_MAX: f64 = 2_147_483_647.0;
const INTEGER_MIN: f64 = -2_147_483_648.0;
const ADAPTATION_LOWER_NODE_LIMIT: f64 = 1000.0;
const ADAPTATION_UPPER_NODE_LIMIT: f64 = 5000.0;
const COOLING_ADAPTATION_FACTOR: f64 = 0.33;

/// `RectangleD`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Rect {
	pub(super) x: f64,
	pub(super) y: f64,
	pub(super) width: f64,
	pub(super) height: f64,
}

impl Rect {
	pub(super) fn right(&self) -> f64 {
		self.x + self.width
	}

	pub(super) fn bottom(&self) -> f64 {
		self.y + self.height
	}

	pub(super) fn center_x(&self) -> f64 {
		self.x + self.width / 2.0
	}

	pub(super) fn center_y(&self) -> f64 {
		self.y + self.height / 2.0
	}

	pub(super) fn intersects(&self, other: &Rect) -> bool {
		!(self.right() < other.x
			|| self.bottom() < other.y
			|| other.right() < self.x
			|| other.bottom() < self.y)
	}
}

#[derive(Clone, Debug, Default)]
struct Node {
	rect: Rect,
	edges: Vec<usize>,
	spring: (f64, f64),
	repulsion: (f64, f64),
	gravitation: (f64, f64),
	/// The grid cells the node falls into: start and finish column and row.
	cells: [i64; 4],
	surrounding: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Edge {
	source: usize,
	target: usize,
	length: f64,
	length_x: f64,
	length_y: f64,
}

impl Edge {
	fn other_end(&self, node: usize) -> usize {
		if self.source == node { self.target } else { self.source }
	}
}

/// The bounds of the root graph with its margin.
#[derive(Clone, Copy, Debug, Default)]
struct Bounds {
	left: f64,
	right: f64,
	top: f64,
	bottom: f64,
}

/// `IMath.sign`.
pub(super) fn sign(value: f64) -> f64 {
	if value > 0.0 {
		1.0
	} else if value < 0.0 {
		-1.0
	} else {
		0.0
	}
}

/// `parseInt(n)` of a JavaScript number in the range of the grid, bounded so that neighbor
/// cells cannot overflow.
fn parse_int(value: f64) -> i64 {
	const LIMIT: f64 = 1e15;
	if value.is_finite() { value.trunc().clamp(-LIMIT, LIMIT) as i64 } else { 0 }
}

/// `IGeometry.getIntersection2`: the clipping points of the line between the centers of `a`
/// and `b` with their rectangles, and whether they overlap.
pub(super) fn clip_points(a: &Rect, b: &Rect) -> (bool, [f64; 4]) {
	let (p1x, p1y, p2x, p2y) = (a.center_x(), a.center_y(), b.center_x(), b.center_y());
	if a.intersects(b) {
		return (true, [p1x, p1y, p2x, p2y]);
	}
	let mut result = [f64::NAN; 4];
	let (top_left_ax, top_left_ay, top_right_ax) = (a.x, a.y, a.right());
	let (bottom_left_ax, bottom_left_ay, bottom_right_ax) = (a.x, a.bottom(), a.right());
	let (half_width_a, half_height_a) = (a.width / 2.0, a.height / 2.0);
	let (top_left_bx, top_left_by, top_right_bx) = (b.x, b.y, b.right());
	let (bottom_left_bx, bottom_left_by, bottom_right_bx) = (b.x, b.bottom(), b.right());
	let (half_width_b, half_height_b) = (b.width / 2.0, b.height / 2.0);
	if p1x == p2x {
		if p1y > p2y {
			result = [p1x, top_left_ay, p2x, bottom_left_by];
		} else if p1y < p2y {
			result = [p1x, bottom_left_ay, p2x, top_left_by];
		}
		return (false, result);
	}
	if p1y == p2y {
		if p1x > p2x {
			result = [top_left_ax, p1y, top_right_bx, p2y];
		} else if p1x < p2x {
			result = [top_right_ax, p1y, top_left_bx, p2y];
		}
		return (false, result);
	}
	let slope_a = a.height / a.width;
	let slope_b = b.height / b.width;
	let slope_prime = (p2y - p1y) / (p2x - p1x);
	let mut found_a = false;
	let mut found_b = false;
	if -slope_a == slope_prime {
		if p1x > p2x {
			result[0] = bottom_left_ax;
			result[1] = bottom_left_ay;
		} else {
			result[0] = top_right_ax;
			result[1] = top_left_ay;
		}
		found_a = true;
	} else if slope_a == slope_prime {
		if p1x > p2x {
			result[0] = top_left_ax;
			result[1] = top_left_ay;
		} else {
			result[0] = bottom_right_ax;
			result[1] = bottom_left_ay;
		}
		found_a = true;
	}
	if -slope_b == slope_prime {
		if p2x > p1x {
			result[2] = bottom_left_bx;
			result[3] = bottom_left_by;
		} else {
			result[2] = top_right_bx;
			result[3] = top_left_by;
		}
		found_b = true;
	} else if slope_b == slope_prime {
		if p2x > p1x {
			result[2] = top_left_bx;
			result[3] = top_left_by;
		} else {
			result[2] = bottom_right_bx;
			result[3] = bottom_left_by;
		}
		found_b = true;
	}
	if found_a && found_b {
		return (false, result);
	}
	let cardinal = |slope: f64, line: u8| if slope > slope_prime { line } else { 1 + line % 4 };
	let (direction_a, direction_b) = match (p1x > p2x, p1y > p2y) {
		(true, true) => (cardinal(slope_a, 4), cardinal(slope_b, 2)),
		(true, false) => (cardinal(-slope_a, 3), cardinal(-slope_b, 1)),
		(false, true) => (cardinal(-slope_a, 1), cardinal(-slope_b, 3)),
		(false, false) => (cardinal(slope_a, 2), cardinal(slope_b, 4)),
	};
	if !found_a {
		let (x, y) = match direction_a {
			1 => (p1x + (-half_height_a) / slope_prime, top_left_ay),
			2 => (bottom_right_ax, p1y + half_width_a * slope_prime),
			3 => (p1x + half_height_a / slope_prime, bottom_left_ay),
			_ => (bottom_left_ax, p1y + (-half_width_a) * slope_prime),
		};
		result[0] = x;
		result[1] = y;
	}
	if !found_b {
		let (x, y) = match direction_b {
			1 => (p2x + (-half_height_b) / slope_prime, top_left_by),
			2 => (bottom_right_bx, p2y + half_width_b * slope_prime),
			3 => (p2x + half_height_b / slope_prime, bottom_left_by),
			_ => (bottom_left_bx, p2y + (-half_width_b) * slope_prime),
		};
		result[2] = x;
		result[3] = y;
	}
	(false, result)
}

/// `IGeometry.calcSeparationAmount` of two overlapping rectangles.
pub(super) fn separation_amount(a: &Rect, b: &Rect, buffer: f64) -> (f64, f64) {
	let direction_x = if a.center_x() < b.center_x() { -1.0 } else { 1.0 };
	let direction_y = if a.center_y() < b.center_y() { -1.0 } else { 1.0 };
	let mut overlap_x = a.right().min(b.right()) - a.x.max(b.x);
	let mut overlap_y = a.bottom().min(b.bottom()) - a.y.max(b.y);
	if a.x <= b.x && a.right() >= b.right() {
		overlap_x += (b.x - a.x).min(a.right() - b.right());
	} else if b.x <= a.x && b.right() >= a.right() {
		overlap_x += (a.x - b.x).min(b.right() - a.right());
	}
	if a.y <= b.y && a.bottom() >= b.bottom() {
		overlap_y += (b.y - a.y).min(a.bottom() - b.bottom());
	} else if b.y <= a.y && b.bottom() >= a.bottom() {
		overlap_y += (a.y - b.y).min(b.bottom() - a.bottom());
	}
	let mut slope = ((b.center_y() - a.center_y()) / (b.center_x() - a.center_x())).abs();
	if b.center_y() == a.center_y() && b.center_x() == a.center_x() {
		slope = 1.0;
	}
	let mut move_by_y = slope * overlap_x;
	let mut move_by_x = overlap_y / slope;
	if overlap_x < move_by_x {
		move_by_x = overlap_x;
	} else {
		move_by_y = overlap_y;
	}
	(-direction_x * ((move_by_x / 2.0) + buffer), -direction_y * ((move_by_y / 2.0) + buffer))
}

/// `Math.min`/`Math.max` of JavaScript, which propagate `NaN`.
fn js_max(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

struct Layout {
	nodes: Vec<Node>,
	edges: Vec<Edge>,
	bounds: Bounds,
	gravity_nodes: Vec<usize>,
	estimated_size: f64,
	max_iterations: usize,
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
	/// `RandomSeed.seed`.
	random_seed: f64,
}

/// Lays out the nodes of the given `(width, height)` with the given edges and returns their
/// centers.
pub fn layout(sizes: &[(f64, f64)], edges: &[(usize, usize)]) -> Vec<Point> {
	let mut layout = Layout {
		nodes: sizes
			.iter()
			.map(|&(width, height)| Node {
				// Nodes start at the origin: Mermaid's layout data has no position.
				rect: Rect { x: 0.0 - width / 2.0, y: 0.0 - height / 2.0, width, height },
				..Node::default()
			})
			.collect(),
		edges: Vec::new(),
		bounds: Bounds::default(),
		gravity_nodes: Vec::new(),
		estimated_size: 0.0,
		max_iterations: MAX_ITERATIONS,
		total_iterations: 0,
		cooling_cycle: 0.0,
		max_cooling_cycle: 0.0,
		final_temperature: 0.0,
		cooling_factor: 0.3,
		initial_cooling_factor: 0.3,
		total_displacement: 0.0,
		old_total_displacement: 0.0,
		total_displacement_threshold: 0.0,
		repulsion_range: 0.0,
		grid_size: (0, 0),
		random_seed: 1.0,
	};
	for &(source, target) in edges {
		if source == target || source >= sizes.len() || target >= sizes.len() {
			continue;
		}
		let exists = layout.nodes[source].edges.iter().any(|&edge| {
			let edge = &layout.edges[edge];
			edge.source == target || edge.target == target
		});
		if exists {
			continue;
		}
		let index = layout.edges.len();
		layout.edges.push(Edge { source, target, length: 0.0, length_x: 0.0, length_y: 0.0 });
		layout.nodes[source].edges.push(index);
		layout.nodes[target].edges.push(index);
	}
	if !layout.nodes.is_empty() {
		layout.run();
	}
	layout
		.nodes
		.iter()
		.map(|node| Point { x: node.rect.center_x(), y: node.rect.center_y() })
		.collect()
}

impl Layout {
	/// `Layout.runLayout` without tiling: all nodes have edges or are alone.
	fn run(&mut self) {
		// `initParameters`.
		self.max_cooling_cycle = (self.max_iterations / CONVERGENCE_CHECK_PERIOD) as f64;
		self.final_temperature = CONVERGENCE_CHECK_PERIOD as f64 / self.max_iterations as f64;
		// `classicLayout`.
		if !self.is_connected() {
			self.gravity_nodes = (0..self.nodes.len()).collect();
		}
		let size: f64 = self
			.nodes
			.iter()
			.fold(0.0, |sum, node| sum + (node.rect.width + node.rect.height) / 2.0);
		self.estimated_size = match size == 0.0 {
			true => 40.0,
			false => size / (self.nodes.len() as f64).sqrt(),
		};
		let forest = self.flat_forest();
		if forest.is_empty() {
			self.scatter();
		} else {
			self.position_radially(&forest);
		}
		self.init_spring_embedder();
		while !self.tick() {}
		self.update_bounds();
		// `doPostLayout`: the left top corner of the drawing goes to the origin.
		self.transform_to(0.0, 0.0);
	}

	/// `LGraph.updateConnected` of the root graph.
	fn is_connected(&self) -> bool {
		let mut visited = vec![false; self.nodes.len()];
		let mut queue = std::collections::VecDeque::from([0]);
		visited[0] = true;
		let mut count = 1;
		while let Some(node) = queue.pop_front() {
			for &edge in &self.nodes[node].edges {
				let other = self.edges[edge].other_end(node);
				if !visited[other] {
					visited[other] = true;
					count += 1;
					queue.push_back(other);
				}
			}
		}
		count >= self.nodes.len()
	}

	/// `getFlatForest`: the trees of the graph in BFS order, empty if one component has a cycle.
	fn flat_forest(&self) -> Vec<Vec<usize>> {
		let count = self.nodes.len();
		let mut forest = Vec::new();
		let mut unprocessed: Vec<usize> = (0..count).collect();
		let mut is_processed = vec![false; count];
		let mut parents: Vec<Option<usize>> = vec![None; count];
		let mut is_visited = vec![false; count];
		while let Some(&first) = unprocessed.first() {
			let mut visited = Vec::new();
			let mut to_be_visited = std::collections::VecDeque::from([first]);
			while let Some(current) = to_be_visited.pop_front() {
				if !is_visited[current] {
					is_visited[current] = true;
					visited.push(current);
				}
				for &edge in &self.nodes[current].edges {
					let neighbor = self.edges[edge].other_end(current);
					if parents[current] == Some(neighbor) {
						continue;
					}
					if is_visited[neighbor] {
						return Vec::new();
					}
					to_be_visited.push_back(neighbor);
					parents[neighbor] = Some(current);
				}
			}
			for &node in &visited {
				is_processed[node] = true;
				is_visited[node] = false;
				parents[node] = None;
			}
			unprocessed.retain(|&node| !is_processed[node]);
			forest.push(visited);
		}
		forest
	}

	/// `positionNodesRandomly` with `LNode.scatter`.
	fn scatter(&mut self) {
		for index in 0..self.nodes.len() {
			let (min, max) = (-INITIAL_WORLD_BOUNDARY, INITIAL_WORLD_BOUNDARY);
			let x = WORLD_CENTER_X + (self.next_double() * (max - min)) + min;
			let y = WORLD_CENTER_Y + (self.next_double() * (max - min)) + min;
			self.nodes[index].rect.x = x;
			self.nodes[index].rect.y = y;
		}
	}

	/// `RandomSeed.nextDouble`.
	fn next_double(&mut self) -> f64 {
		let x = js_math::sin(self.random_seed) * 10000.0;
		self.random_seed += 1.0;
		x - x.floor()
	}

	/// `positionNodesRadially`: each tree around its center, the trees in rows.
	fn position_radially(&mut self, forest: &[Vec<usize>]) {
		let columns = (forest.len() as f64).sqrt().ceil() as usize;
		let mut height = 0.0;
		let mut current_y = 0.0;
		let mut current_x = 0.0;
		let mut point = Point { x: 0.0, y: 0.0 };
		for (index, tree) in forest.iter().enumerate() {
			if index % columns == 0 {
				current_x = 0.0;
				current_y = height;
				if index != 0 {
					current_y += COMPONENT_SEPARATION;
				}
				height = 0.0;
			}
			let center = center_of_tree(tree);
			point = self.radial_layout(tree, center, Point { x: current_x, y: current_y });
			if point.y > height {
				height = point.y.floor();
			}
			current_x = (point.x + COMPONENT_SEPARATION).floor();
		}
		self.transform_to(WORLD_CENTER_X - point.x / 2.0, WORLD_CENTER_Y - point.y / 2.0);
	}

	/// `CoSELayout.radialLayout`: returns the bottom right corner of the placed tree.
	fn radial_layout(&mut self, tree: &[usize], center: usize, start: Point) -> Point {
		let max_diagonal = tree.iter().fold(INTEGER_MIN, |max, &node| {
			let rect = &self.nodes[node].rect;
			let diagonal = (rect.width * rect.width + rect.height * rect.height).sqrt();
			if diagonal > max { diagonal } else { max }
		});
		let separation =
			if max_diagonal > RADIAL_SEPARATION { max_diagonal } else { RADIAL_SEPARATION };
		self.branch_radial_layout(center, separation);
		let (mut left, mut right, mut top, mut bottom) =
			(INTEGER_MAX, -INTEGER_MAX, INTEGER_MAX, -INTEGER_MAX);
		for &node in tree {
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
		let bounds = Rect { x: left, y: top, width: right - left, height: bottom - top };
		for &node in tree {
			self.transform_node(node, (bounds.x, bounds.y), (start.x, start.y));
		}
		Point {
			x: start.x + (bounds.right() - bounds.x),
			y: start.y + (bounds.bottom() - bounds.y),
		}
	}

	/// `CoSELayout.branchRadialLayout` with an explicit stack: each node in the middle of its
	/// angle interval, its children splitting the interval, a level per `separation`.
	fn branch_radial_layout(&mut self, root: usize, separation: f64) {
		struct Frame {
			node: usize,
			parent: Option<usize>,
			start_angle: f64,
			step_angle: f64,
			distance: f64,
			child_count: usize,
			branch_count: usize,
			index: usize,
			steps: usize,
		}
		let mut stack: Vec<Frame> = Vec::new();
		let enter = |layout: &mut Layout,
		             stack: &mut Vec<Frame>,
		             node: usize,
		             parent: Option<usize>,
		             (start_angle, end_angle): (f64, f64),
		             distance: f64| {
			let mut half_interval = ((end_angle - start_angle) + 1.0) / 2.0;
			if half_interval < 0.0 {
				half_interval += 180.0;
			}
			let node_angle = (half_interval + start_angle) % 360.0;
			let teta = (node_angle * std::f64::consts::TAU) / 360.0;
			let x = distance * js_math::cos(teta);
			let y = distance * js_math::sin(teta);
			let rect = &mut layout.nodes[node].rect;
			rect.x = x - rect.width / 2.0;
			rect.y = y - rect.height / 2.0;
			let edges = &layout.nodes[node].edges;
			let child_count = edges.len() - usize::from(parent.is_some());
			let index = match parent {
				Some(parent) => {
					let position = edges
						.iter()
						.position(|&edge| layout.edges[edge].other_end(node) == parent)
						.unwrap_or(0);
					(position + 1) % edges.len()
				}
				None => 0,
			};
			stack.push(Frame {
				node,
				parent,
				start_angle,
				step_angle: (end_angle - start_angle).abs() / child_count as f64,
				distance,
				child_count,
				branch_count: 0,
				index,
				steps: 0,
			});
		};
		enter(self, &mut stack, root, None, (0.0, 359.0), 0.0);
		while let Some(frame) = stack.last_mut() {
			let edges = &self.nodes[frame.node].edges;
			if frame.branch_count == frame.child_count || frame.steps > edges.len() * 2 {
				stack.pop();
				continue;
			}
			let neighbor = self.edges[edges[frame.index]].other_end(frame.node);
			frame.index = (frame.index + 1) % edges.len();
			frame.steps += 1;
			if Some(neighbor) == frame.parent {
				continue;
			}
			let child_start =
				(frame.start_angle + frame.branch_count as f64 * frame.step_angle) % 360.0;
			let child_end = (child_start + frame.step_angle) % 360.0;
			frame.branch_count += 1;
			let (node, distance) = (frame.node, frame.distance + separation);
			enter(self, &mut stack, neighbor, Some(node), (child_start, child_end), distance);
		}
	}

	/// `LNode.transform` with a translation from `device` to `world`.
	fn transform_node(&mut self, node: usize, device: (f64, f64), world: (f64, f64)) {
		let rect = &mut self.nodes[node].rect;
		let left = rect.x.clamp(-WORLD_BOUNDARY, WORLD_BOUNDARY);
		let top = rect.y.clamp(-WORLD_BOUNDARY, WORLD_BOUNDARY);
		rect.x = world.0 + (left - device.0);
		rect.y = world.1 + (top - device.1);
	}

	/// `Layout.transform`: moves the left top corner of the root graph, margin included, to the
	/// given point.
	fn transform_to(&mut self, x: f64, y: f64) {
		let (mut top, mut left) = (INTEGER_MAX, INTEGER_MAX);
		for node in &self.nodes {
			if top > node.rect.y {
				top = node.rect.y;
			}
			if left > node.rect.x {
				left = node.rect.x;
			}
		}
		if top == INTEGER_MAX {
			return;
		}
		let device = (left - GRAPH_MARGIN, top - GRAPH_MARGIN);
		for node in 0..self.nodes.len() {
			self.transform_node(node, device, (x, y));
		}
	}

	/// `FDLayout.initSpringEmbedder` of a non-incremental layout.
	fn init_spring_embedder(&mut self) {
		let count = self.nodes.len() as f64;
		self.cooling_factor = match count > ADAPTATION_LOWER_NODE_LIMIT {
			true => COOLING_ADAPTATION_FACTOR.max(
				1.0 - (count - ADAPTATION_LOWER_NODE_LIMIT)
					/ (ADAPTATION_UPPER_NODE_LIMIT - ADAPTATION_LOWER_NODE_LIMIT)
					* (1.0 - COOLING_ADAPTATION_FACTOR),
			),
			false => 1.0,
		};
		self.initial_cooling_factor = self.cooling_factor;
		self.max_iterations = (self.nodes.len() * 5).max(self.max_iterations);
		self.total_displacement_threshold = (3.0 * DEFAULT_EDGE_LENGTH) / 100.0 * count;
		// `2 * (level + 1) * idealEdgeLength`.
		self.repulsion_range = 2.0 * (0.0 + 1.0) * DEFAULT_EDGE_LENGTH;
	}

	/// `CoSELayout.tick`: one iteration; true when the layout is done.
	fn tick(&mut self) -> bool {
		self.total_iterations += 1;
		if self.total_iterations == self.max_iterations {
			return true;
		}
		if self.total_iterations.is_multiple_of(CONVERGENCE_CHECK_PERIOD) {
			if self.is_converged() {
				return true;
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
		self.calc_repulsion_forces();
		self.calc_gravitational_forces();
		self.move_nodes();
		false
	}

	/// `FDLayout.isConverged`.
	fn is_converged(&mut self) -> bool {
		let mut oscillating = false;
		if self.total_iterations as f64 > self.max_iterations as f64 / 3.0 {
			oscillating = (self.total_displacement - self.old_total_displacement).abs() < 2.0;
		}
		let converged = self.total_displacement < self.total_displacement_threshold;
		self.old_total_displacement = self.total_displacement;
		converged || oscillating
	}

	/// `LGraph.updateBounds` of the root graph.
	fn update_bounds(&mut self) {
		let (mut left, mut right, mut top, mut bottom) =
			(INTEGER_MAX, -INTEGER_MAX, INTEGER_MAX, -INTEGER_MAX);
		for node in &self.nodes {
			let rect = &node.rect;
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
		let bounds = Rect { x: left, y: top, width: right - left, height: bottom - top };
		self.bounds = Bounds {
			left: bounds.x - GRAPH_MARGIN,
			right: bounds.x + bounds.width + GRAPH_MARGIN,
			top: bounds.y - GRAPH_MARGIN,
			bottom: bounds.y + bounds.height + GRAPH_MARGIN,
		};
	}

	fn calc_spring_forces(&mut self) {
		for index in 0..self.edges.len() {
			let Edge { source, target, .. } = self.edges[index];
			// `LEdge.updateLength`.
			let (overlapping, clip) =
				clip_points(&self.nodes[target].rect, &self.nodes[source].rect);
			let edge = &mut self.edges[index];
			if overlapping {
				continue;
			}
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
			let spring_force = SPRING_STRENGTH * (length - DEFAULT_EDGE_LENGTH);
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
	fn calc_repulsion_forces(&mut self) {
		let updates_surroundings = self.total_iterations % GRID_CALCULATION_CHECK_PERIOD == 1;
		if updates_surroundings {
			self.update_grid();
			let columns = self.columns();
			let mut seen = vec![usize::MAX; self.nodes.len()];
			for index in 0..self.nodes.len() {
				self.nodes[index].surrounding = self.surrounding(index, &columns, &mut seen);
			}
		}
		for index in 0..self.nodes.len() {
			for position in 0..self.nodes[index].surrounding.len() {
				let other = self.nodes[index].surrounding[position];
				self.calc_repulsion_force(index, other);
			}
		}
	}

	/// `FDLayout.updateGrid`: the grid size and the cells of every node.
	fn update_grid(&mut self) {
		let Bounds { left, right, top, bottom } = self.bounds;
		let range = self.repulsion_range;
		self.grid_size = (
			parse_int(((right - left) / range).ceil()),
			parse_int(((bottom - top) / range).ceil()),
		);
		for node in &mut self.nodes {
			let rect = &node.rect;
			node.cells = [
				parse_int(((rect.x - left) / range).floor()),
				parse_int(((rect.width + rect.x - left) / range).floor()),
				parse_int(((rect.y - top) / range).floor()),
				parse_int(((rect.height + rect.y - top) / range).floor()),
			];
		}
	}

	/// The nodes by the grid columns they fall into; nodes across more than `WIDE` columns are
	/// listed under `None` instead, so that huge nodes cost no memory.
	fn columns(&self) -> std::collections::HashMap<Option<i64>, Vec<usize>> {
		const WIDE: i64 = 32;
		let mut columns: std::collections::HashMap<Option<i64>, Vec<usize>> =
			std::collections::HashMap::new();
		let last_column = self.grid_size.0 - 1;
		for (index, node) in self.nodes.iter().enumerate() {
			let (start, finish) = (node.cells[0].max(0), node.cells[1].min(last_column));
			if finish - start > WIDE {
				columns.entry(None).or_default().push(index);
			} else {
				for column in start..=finish {
					columns.entry(Some(column)).or_default().push(index);
				}
			}
		}
		columns
	}

	/// The surrounding of `node` that `calculateRepulsionForceOfANode` finds in the grid: the
	/// later nodes in a neighboring cell within the repulsion range, in the order of the cells
	/// (column, then row) and the nodes in them.
	fn surrounding(
		&self,
		index: usize,
		columns: &std::collections::HashMap<Option<i64>, Vec<usize>>,
		seen: &mut [usize],
	) -> Vec<usize> {
		let node = &self.nodes[index];
		let [start_x, finish_x, start_y, finish_y] = node.cells;
		let (column_count, rows) = self.grid_size;
		let scan_x = ((start_x - 1).max(0), (finish_x + 1).min(column_count - 1));
		let scan_y = ((start_y - 1).max(0), (finish_y + 1).min(rows - 1));
		let mut found: Vec<(i64, i64, usize)> = Vec::new();
		let candidates = (scan_x.0..=scan_x.1)
			.filter_map(|column| columns.get(&Some(column)))
			.chain(columns.get(&None))
			.flatten();
		for &other_index in candidates {
			if other_index <= index || seen[other_index] == index {
				continue;
			}
			seen[other_index] = index;
			let other = &self.nodes[other_index];
			let [other_start_x, other_finish_x, other_start_y, other_finish_y] = other.cells;
			// The other node's cells within the grid.
			let cells_x = (other_start_x.max(0), other_finish_x.min(column_count - 1));
			let cells_y = (other_start_y.max(0), other_finish_y.min(rows - 1));
			let first_x = scan_x.0.max(cells_x.0);
			let first_y = scan_y.0.max(cells_y.0);
			if first_x > scan_x.1.min(cells_x.1) || first_y > scan_y.1.min(cells_y.1) {
				continue;
			}
			let (a, b) = (&node.rect, &other.rect);
			let distance_x =
				(a.center_x() - b.center_x()).abs() - ((a.width / 2.0) + (b.width / 2.0));
			let distance_y =
				(a.center_y() - b.center_y()).abs() - ((a.height / 2.0) + (b.height / 2.0));
			if distance_x <= self.repulsion_range && distance_y <= self.repulsion_range {
				found.push((first_x, first_y, other_index));
			}
		}
		found.sort_unstable();
		found.into_iter().map(|(_, _, other)| other).collect()
	}

	/// `FDLayout.calcRepulsionForce` of nodes without uniform sizes.
	fn calc_repulsion_force(&mut self, a: usize, b: usize) {
		let (rect_a, rect_b) = (self.nodes[a].rect, self.nodes[b].rect);
		let (force_a, force_b);
		if rect_a.intersects(&rect_b) {
			let (overlap_x, overlap_y) =
				separation_amount(&rect_a, &rect_b, DEFAULT_EDGE_LENGTH / 2.0);
			let (force_x, force_y) = (2.0 * overlap_x, 2.0 * overlap_y);
			// Simple nodes have a single child each: `1 * 1 / (1 + 1)`.
			let children = 1.0 * 1.0 / (1.0 + 1.0);
			force_a = (-(children * force_x), -(children * force_y));
			force_b = (children * force_x, children * force_y);
		} else {
			let (_, clip) = clip_points(&rect_a, &rect_b);
			let mut distance_x = clip[2] - clip[0];
			let mut distance_y = clip[3] - clip[1];
			if distance_x.abs() < MIN_REPULSION_DIST {
				distance_x = sign(distance_x) * MIN_REPULSION_DIST;
			}
			if distance_y.abs() < MIN_REPULSION_DIST {
				distance_y = sign(distance_y) * MIN_REPULSION_DIST;
			}
			let distance_squared = distance_x * distance_x + distance_y * distance_y;
			let distance = distance_squared.sqrt();
			let force = REPULSION_STRENGTH * 1.0 * 1.0 / distance_squared;
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

	/// `FDLayout.calcGravitationalForces` of the nodes in the root graph.
	fn calc_gravitational_forces(&mut self) {
		let Bounds { left, right, top, bottom } = self.bounds;
		let center_x = (right + left) / 2.0;
		let center_y = (top + bottom) / 2.0;
		let estimated_size = self.estimated_size * GRAVITY_RANGE_FACTOR;
		for position in 0..self.gravity_nodes.len() {
			let node = &mut self.nodes[self.gravity_nodes[position]];
			let distance_x = node.rect.center_x() - center_x;
			let distance_y = node.rect.center_y() - center_y;
			let abs_distance_x = distance_x.abs() + node.rect.width / 2.0;
			let abs_distance_y = distance_y.abs() + node.rect.height / 2.0;
			if abs_distance_x > estimated_size || abs_distance_y > estimated_size {
				node.gravitation = (-GRAVITY_STRENGTH * distance_x, -GRAVITY_STRENGTH * distance_y);
			}
		}
	}

	/// `CoSENode.move` of every node.
	fn move_nodes(&mut self) {
		let limit = self.cooling_factor * MAX_NODE_DISPLACEMENT;
		for node in &mut self.nodes {
			let mut dx =
				self.cooling_factor * (node.spring.0 + node.repulsion.0 + node.gravitation.0) / 1.0;
			let mut dy =
				self.cooling_factor * (node.spring.1 + node.repulsion.1 + node.gravitation.1) / 1.0;
			if dx.abs() > limit {
				dx = limit * sign(dx);
			}
			if dy.abs() > limit {
				dy = limit * sign(dy);
			}
			node.rect.x += dx;
			node.rect.y += dy;
			self.total_displacement += dx.abs() + dy.abs();
			node.spring = (0.0, 0.0);
			node.repulsion = (0.0, 0.0);
			node.gravitation = (0.0, 0.0);
		}
	}
}

/// `Layout.findCenterOfTree`. Its loop removes every other node of the list per round, so
/// the center is the node that survives the halving.
fn center_of_tree(tree: &[usize]) -> usize {
	let mut list: Vec<usize> = tree.to_vec();
	while list.len() > 2 {
		list = list.iter().skip(1).step_by(2).copied().collect();
	}
	list.first().copied().unwrap_or(0)
}

use super::js_math;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_the_center_like_layout_base() {
		assert_eq!(center_of_tree(&[7]), 7);
		assert_eq!(center_of_tree(&[1, 2]), 1);
		assert_eq!(center_of_tree(&[0, 1, 2]), 1);
		assert_eq!(center_of_tree(&[0, 1, 2, 3, 4, 5, 6]), 3);
		assert_eq!(center_of_tree(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]), 7);
	}

	#[test]
	fn lays_out_without_panicking() {
		assert!(layout(&[], &[]).is_empty());
		let single = layout(&[(40.0, 20.0)], &[]);
		assert_eq!(single, [Point { x: 35.0, y: 25.0 }]);
		let odd =
			layout(&[(0.0, 0.0), (f64::NAN, 1e300), (1e9, 1e9)], &[(0, 1), (1, 2), (2, 0), (0, 0)]);
		assert_eq!(odd.len(), 3);
	}

	/// Compares one line of fixtures, returns the difference, if any. A fixture is a JSON object
	/// with the node `sizes`, the `edges` and the `positions` that cytoscape 3.34 with
	/// cytoscape-cose-bilkent 4.1.0 gives in Edge (random trees, laid out like `mindmap`).
	fn check_fixture(line: &str) -> Option<String> {
		use super::super::super::value::{Value, parse_json};
		let value = parse_json(line)?;
		let pairs = |key: &str| -> Vec<(f64, f64)> {
			match value.get(key) {
				Some(Value::Array(items)) => items
					.iter()
					.filter_map(|item| match item {
						Value::Array(pair) if pair.len() == 2 => {
							Some((pair[0].as_number()?, pair[1].as_number()?))
						}
						_ => None,
					})
					.collect(),
				_ => Vec::new(),
			}
		};
		let sizes = pairs("sizes");
		let edges: Vec<(usize, usize)> =
			pairs("edges").into_iter().map(|(s, t)| (s as usize, t as usize)).collect();
		let expected = pairs("positions");
		let actual = layout(&sizes, &edges);
		for (index, (point, (x, y))) in actual.iter().zip(&expected).enumerate() {
			if (point.x - x).abs() > 1e-6 || (point.y - y).abs() > 1e-6 {
				return Some(format!(
					"{} nodes, node {index}: ({}, {}) instead of ({x}, {y})",
					sizes.len(),
					point.x,
					point.y
				));
			}
		}
		None
	}

	#[test]
	fn matches_embedded_fixtures() {
		for (index, line) in include_str!("cose_bilkent/fixtures.jsonl").lines().enumerate() {
			assert_eq!(check_fixture(line), None, "fixture {index}");
		}
	}

	/// `COSE_FIXTURES=<file> cargo test -p npp-toolbox cose_fixtures -- --ignored` compares with
	/// a file of fixtures, one per line (see `cose_bilkent/fixtures.jsonl`).
	#[test]
	#[ignore]
	fn cose_fixtures() {
		let path = std::env::var("COSE_FIXTURES").expect("COSE_FIXTURES");
		let text = std::fs::read_to_string(path).expect("fixture file");
		let mut failures = 0;
		for (index, line) in text.lines().enumerate() {
			if let Some(difference) = check_fixture(line) {
				failures += 1;
				if failures <= 10 {
					eprintln!("fixture {index}: {difference}");
				}
			}
		}
		assert_eq!(failures, 0, "{failures} of {} fixtures differ", text.lines().count());
	}
}
