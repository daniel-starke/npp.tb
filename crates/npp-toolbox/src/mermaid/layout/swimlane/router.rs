//! Orthogonal edge router of the swimlane layout (`orthogonalRouter/router.ts`): ports on the
//! node sides, A* over a grid of pipes around the obstacles, crossing reduction by moving
//! segments between the tracks of a pipe, and the polylines rebuilt from the track coordinates.

use std::collections::HashMap;

use super::{EPSILON, Layout, Node, Point};
use crate::mermaid::common::v8_sort;

const NODE_PADDING: f64 = 8.0;
const HORIZONTAL_PIPE_MARGIN: f64 = 15.0;
const VERTICAL_PIPE_MARGIN: f64 = 15.0;
const ROUTING_MARGIN: f64 = 25.0;
const ANCHOR_OFFSET: f64 = 20.0;
const TRACK_SPACING: f64 = 10.0;
const CROSSING_PENALTY: f64 = 1000.0;
const MIN_PORT_SPACING: f64 = 8.0;
const MAX_PORT_SPACING: f64 = 20.0;
/// Pops of one A* search; Mermaid has no limit, its grids stay far below it.
const MAX_SEARCH_STEPS: usize = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Side {
	Top,
	Bottom,
	Left,
	Right,
}

impl Side {
	fn is_vertical(self) -> bool {
		matches!(self, Side::Top | Side::Bottom)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Role {
	Source,
	Target,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Orientation {
	Horizontal,
	Vertical,
}

#[derive(Clone, Copy, Debug)]
struct SegmentRef {
	edge: usize,
	segment: usize,
	from: f64,
	to: f64,
}

impl SegmentRef {
	fn is(&self, edge: usize, segment: usize) -> bool {
		self.edge == edge && self.segment == segment
	}
}

#[derive(Clone, Debug)]
struct Track {
	index: usize,
	segments: Vec<SegmentRef>,
}

#[derive(Clone, Debug)]
struct Pipe {
	orientation: Orientation,
	coord: f64,
	tracks: Vec<Track>,
}

#[derive(Clone, Copy, Debug)]
struct RoutedSegment {
	edge: usize,
	segment: usize,
	orientation: Orientation,
	pipe: usize,
	track: usize,
	from: f64,
	to: f64,
}

impl RoutedSegment {
	fn reference(&self) -> SegmentRef {
		SegmentRef { edge: self.edge, segment: self.segment, from: self.from, to: self.to }
	}
}

#[derive(Clone, Copy, Debug)]
struct Obstacle {
	node: usize,
	min_x: f64,
	min_y: f64,
	max_x: f64,
	max_y: f64,
	/// Half extent in x after the LR transform (the height there).
	visual_x_half_extent: f64,
}

#[derive(Clone, Copy, Debug)]
struct SideInfo {
	source: usize,
	target: usize,
	source_side: Side,
	target_side: Side,
	abs_dx: f64,
	abs_dy: f64,
	dx_sign: f64,
	dy_sign: f64,
}

fn center(node: &Node) -> Point {
	Point { x: node.x.unwrap_or(0.0), y: node.y.unwrap_or(0.0) }
}

/// `chooseOrthogonalSide`: vertical sides win unless the target is three times further sideways.
fn choose_side(node: &Node, target: Point, fallback: Side) -> Side {
	let Point { x, y } = center(node);
	let (dx, dy) = (target.x - x, target.y - y);
	let (abs_dx, abs_dy) = (dx.abs(), dy.abs());
	if abs_dx < EPSILON && abs_dy < EPSILON {
		return fallback;
	}
	if abs_dy > EPSILON && abs_dy * 3.0 >= abs_dx {
		return if dy > 0.0 { Side::Bottom } else { Side::Top };
	}
	if abs_dx > EPSILON {
		return if dx > 0.0 { Side::Right } else { Side::Left };
	}
	fallback
}

/// `portForSide`: the centre of a side.
fn port_for_side(node: &Node, side: Side) -> Point {
	let (width, height) = (node.width.unwrap_or(10.0), node.height.unwrap_or(10.0));
	let Point { x, y } = center(node);
	match side {
		Side::Top => Point { x, y: y - height / 2.0 },
		Side::Bottom => Point { x, y: y + height / 2.0 },
		Side::Left => Point { x: x - width / 2.0, y },
		Side::Right => Point { x: x + width / 2.0, y },
	}
}

fn is_diamond(node: &Node) -> bool {
	matches!(node.shape.as_str(), "question" | "diamond")
}

/// An insertion-ordered map like JavaScript's `Map`.
struct OrderedMap<K, V> {
	keys: HashMap<K, usize>,
	entries: Vec<(K, V)>,
}

impl<K: std::hash::Hash + Eq + Clone, V> OrderedMap<K, V> {
	fn new() -> Self {
		Self { keys: HashMap::new(), entries: Vec::new() }
	}

	fn get(&self, key: &K) -> Option<&V> {
		self.keys.get(key).map(|&index| &self.entries[index].1)
	}

	fn entry(&mut self, key: K, default: impl FnOnce() -> V) -> &mut V {
		let index = match self.keys.get(&key) {
			Some(&index) => index,
			None => {
				self.keys.insert(key.clone(), self.entries.len());
				self.entries.push((key, default()));
				self.entries.len() - 1
			}
		};
		&mut self.entries[index].1
	}
}

struct Router<'a> {
	nodes: &'a [Node],
	node_by_id: HashMap<&'a str, usize>,
	obstacles: Vec<Obstacle>,
	pipes: Vec<Pipe>,
	/// Pipes in creation order; fast-path pipes are not registered.
	registered: Vec<usize>,
	segments: Vec<RoutedSegment>,
	edge_segments: Vec<Vec<usize>>,
}

impl Router<'_> {
	fn node(&self, id: &str) -> Option<&Node> {
		self.node_by_id.get(id).map(|&index| &self.nodes[index])
	}

	/// `getOrAddPipe`: the first registered pipe of the orientation within 1 px, else a new one.
	fn pipe(&mut self, orientation: Orientation, coord: f64) -> usize {
		let found = self.registered.iter().copied().find(|&pipe| {
			let pipe = &self.pipes[pipe];
			pipe.orientation == orientation && (pipe.coord - coord).abs() < 1.0
		});
		found.unwrap_or_else(|| {
			self.pipes.push(Pipe { orientation, coord, tracks: Vec::new() });
			self.registered.push(self.pipes.len() - 1);
			self.pipes.len() - 1
		})
	}

	/// `isSegmentBlocked`: an obstacle other than the edge's ends lies across the segment.
	fn is_blocked(&self, p1: Point, p2: Point, start: usize, end: usize) -> bool {
		let (min_x, max_x) = (p1.x.min(p2.x), p1.x.max(p2.x));
		let (min_y, max_y) = (p1.y.min(p2.y), p1.y.max(p2.y));
		self.obstacles.iter().any(|obstacle| {
			if obstacle.node == start || obstacle.node == end {
				return false;
			}
			match (p1.x - p2.x).abs() > EPSILON {
				true => {
					obstacle.min_y < p1.y
						&& obstacle.max_y > p1.y
						&& obstacle.max_x > min_x
						&& obstacle.min_x < max_x
				}
				false => {
					obstacle.min_x < p1.x
						&& obstacle.max_x > p1.x
						&& obstacle.max_y > min_y
						&& obstacle.min_y < max_y
				}
			}
		})
	}

	/// `crossingPenalty`: crossings of a horizontal or vertical step with routed segments.
	fn crossing_penalty(&self, edge: usize, from: Point, to: Point) -> f64 {
		if self.segments.is_empty() {
			return 0.0;
		}
		let is_horizontal = (from.y - to.y).abs() < EPSILON;
		let is_vertical = (from.x - to.x).abs() < EPSILON;
		let (along, minimum, maximum, wanted) = match (is_horizontal, is_vertical) {
			(true, _) => (
				from.y,
				from.x.min(to.x) - EPSILON,
				from.x.max(to.x) + EPSILON,
				Orientation::Vertical,
			),
			(false, true) => (
				from.x,
				from.y.min(to.y) - EPSILON,
				from.y.max(to.y) + EPSILON,
				Orientation::Horizontal,
			),
			_ => return 0.0,
		};
		if maximum <= minimum {
			return 0.0;
		}
		let mut penalties = 0.0;
		for segment in &self.segments {
			if segment.edge == edge || segment.orientation != wanted {
				continue;
			}
			let coord = self.pipes[segment.pipe].coord;
			if coord < minimum || coord > maximum {
				continue;
			}
			if segment.from - EPSILON <= along && segment.to + EPSILON >= along {
				penalties += CROSSING_PENALTY;
			}
		}
		penalties
	}
}

/// The ports of an edge after side assignment and port distribution.
struct Ports {
	source: Point,
	target: Point,
	source_side: Side,
	target_side: Side,
}

/// `routeEdgesOrthogonal`.
pub(super) fn route_edges_orthogonal(layout: &mut Layout) {
	let is_left_right = layout.direction.is_left_right();
	// The routing view: every edge except the layering-only ones.
	let edges: Vec<usize> =
		(0..layout.edges.len()).filter(|&index| !layout.edges[index].is_layout_only).collect();
	let nodes = &layout.nodes;
	let mut node_by_id = HashMap::new();
	for (index, node) in nodes.iter().enumerate() {
		node_by_id.insert(node.id.as_str(), index);
	}
	let lane_of = assign_lanes(nodes);
	let obstacles = nodes
		.iter()
		.enumerate()
		.filter(|(_, node)| !node.is_group && !node.is_edge_label)
		.map(|(index, node)| {
			let (width, height) = (node.width.unwrap_or(10.0), node.height.unwrap_or(10.0));
			let Point { x, y } = center(node);
			Obstacle {
				node: index,
				min_x: x - width / 2.0 - NODE_PADDING,
				max_x: x + width / 2.0 + NODE_PADDING,
				min_y: y - height / 2.0 - NODE_PADDING,
				max_y: y + height / 2.0 + NODE_PADDING,
				visual_x_half_extent: match is_left_right {
					true => height / 2.0 + NODE_PADDING,
					false => width / 2.0 + NODE_PADDING,
				},
			}
		})
		.collect();
	let mut router = Router {
		nodes,
		node_by_id,
		obstacles,
		pipes: Vec::new(),
		registered: Vec::new(),
		segments: Vec::new(),
		edge_segments: vec![Vec::new(); edges.len()],
	};
	let edge = |index: usize| &layout.edges[edges[index]];
	let endpoints = |router: &Router, index: usize| -> Option<(usize, usize)> {
		let edge = edge(index);
		Some((
			*router.node_by_id.get(edge.start.as_str())?,
			*router.node_by_id.get(edge.end.as_str())?,
		))
	};
	let mut points: Vec<Option<Vec<Point>>> =
		edges.iter().map(|&index| layout.edges[index].points.clone()).collect();
	let mut straight_edges = vec![false; edges.len()];

	// Phase 1: cross-lane edges first, then the shorter ones.
	let mut routing_order: Vec<(usize, u8, f64)> = (0..edges.len())
		.map(|index| {
			let edge = edge(index);
			let source_lane = lane_of.get(edge.start.as_str());
			let target_lane = lane_of.get(edge.end.as_str());
			let cross_lane = matches!((source_lane, target_lane), (Some(a), Some(b)) if a != b);
			let distance = match endpoints(&router, index) {
				Some((source, target)) => {
					let (a, b) = (center(&nodes[source]), center(&nodes[target]));
					(b.x - a.x).abs() + (b.y - a.y).abs()
				}
				None => 0.0,
			};
			(index, u8::from(cross_lane), distance)
		})
		.collect();
	v8_sort(&mut routing_order, |a, b| {
		if a.1 != b.1 {
			return f64::from(b.1) - f64::from(a.1);
		}
		if (a.2 - b.2).abs() > 1.0 {
			return a.2 - b.2;
		}
		a.0 as f64 - b.0 as f64
	});

	// Step 6.1: the natural sides of every edge.
	let mut incident_totals: HashMap<&str, usize> = HashMap::new();
	for index in 0..edges.len() {
		let edge = edge(index);
		if edge.start != edge.end {
			*incident_totals.entry(edge.start.as_str()).or_insert(0) += 1;
			*incident_totals.entry(edge.end.as_str()).or_insert(0) += 1;
		}
	}
	let mut side_info: Vec<Option<SideInfo>> = vec![None; edges.len()];
	for (index, info) in side_info.iter_mut().enumerate() {
		if edge(index).start == edge(index).end
			|| points[index].as_ref().is_some_and(|points| !points.is_empty())
		{
			continue;
		}
		let Some((source, target)) = endpoints(&router, index) else {
			continue;
		};
		let (a, b) = (center(&nodes[source]), center(&nodes[target]));
		let (dx, dy) = (b.x - a.x, b.y - a.y);
		*info = Some(SideInfo {
			source,
			target,
			source_side: choose_side(&nodes[source], b, Side::Bottom),
			target_side: choose_side(&nodes[target], a, Side::Bottom),
			abs_dx: dx.abs(),
			abs_dy: dy.abs(),
			dx_sign: js_sign(dx),
			dy_sign: js_sign(dy),
		});
	}

	// Step 6.2: siblings leaving the same side move to their secondary side when it is less loaded.
	let strength = |info: &SideInfo| match info.source_side.is_vertical() {
		true if info.abs_dx == 0.0 => f64::INFINITY,
		true => info.abs_dy / info.abs_dx,
		false if info.abs_dy == 0.0 => f64::INFINITY,
		false => info.abs_dx / info.abs_dy,
	};
	let secondary = |info: &SideInfo| match info.source_side.is_vertical() {
		true if info.dx_sign >= 0.0 => Side::Right,
		true => Side::Left,
		false if info.dy_sign >= 0.0 => Side::Bottom,
		false => Side::Top,
	};
	let mut source_groups: OrderedMap<(usize, Side), Vec<usize>> = OrderedMap::new();
	let mut side_load: HashMap<(usize, Side), usize> = HashMap::new();
	for (index, info) in side_info.iter().enumerate() {
		let Some(info) = info else { continue };
		source_groups.entry((info.source, info.source_side), Vec::new).push(index);
	}
	for info in side_info.iter().flatten() {
		*side_load.entry((info.source, info.source_side)).or_insert(0) += 1;
		*side_load.entry((info.target, info.target_side)).or_insert(0) += 1;
	}
	for (_, group) in &mut source_groups.entries {
		if group.len() < 2 {
			continue;
		}
		v8_sort(group, |&a, &b| {
			let (strength_a, strength_b) =
				(strength(&side_info[a].unwrap()), strength(&side_info[b].unwrap()));
			if (strength_a - strength_b).abs() > 1e-9 {
				return strength_b - strength_a;
			}
			a as f64 - b as f64
		});
		for &index in group.iter().skip(1) {
			let Some(info) = side_info[index].as_mut() else { continue };
			let other = secondary(info);
			let primary_load =
				side_load.get(&(info.source, info.source_side)).copied().unwrap_or(0);
			let secondary_load = side_load.get(&(info.source, other)).copied().unwrap_or(0);
			if secondary_load >= primary_load {
				continue;
			}
			side_load.insert((info.source, info.source_side), primary_load - 1);
			side_load.insert((info.source, other), secondary_load + 1);
			info.source_side = other;
		}
	}

	// Step 6.2b: an out-edge of a diamond leaves a side no in-edge uses.
	let mut in_sides: HashMap<usize, Vec<Side>> = HashMap::new();
	for info in side_info.iter().flatten() {
		let sides = in_sides.entry(info.target).or_default();
		if !sides.contains(&info.target_side) {
			sides.push(info.target_side);
		}
	}
	for info in side_info.iter_mut().flatten() {
		if !is_diamond(&nodes[info.source]) {
			continue;
		}
		let Some(sides) = in_sides.get(&info.source) else { continue };
		if !sides.contains(&info.source_side) {
			continue;
		}
		let other = secondary(info);
		if sides.contains(&other) || side_load.get(&(info.source, other)).copied().unwrap_or(0) > 0
		{
			continue;
		}
		let primary_load = side_load.get(&(info.source, info.source_side)).copied().unwrap_or(0);
		side_load.insert((info.source, info.source_side), primary_load.saturating_sub(1));
		side_load.insert((info.source, other), 1);
		info.source_side = other;
	}

	// Step 6.3: ports of a side are spread in the order of their opposite ends.
	let mut port_groups: OrderedMap<(usize, Side, Role), Vec<(usize, f64)>> = OrderedMap::new();
	for (index, info) in side_info.iter().enumerate() {
		let Some(info) = info else { continue };
		let (source, target) = (center(&nodes[info.source]), center(&nodes[info.target]));
		let opposite =
			|side: Side, point: Point| if side.is_vertical() { point.x } else { point.y };
		port_groups
			.entry((info.source, info.source_side, Role::Source), Vec::new)
			.push((index, opposite(info.source_side, target)));
		port_groups
			.entry((info.target, info.target_side, Role::Target), Vec::new)
			.push((index, opposite(info.target_side, source)));
	}
	let mut port_offsets: HashMap<(usize, Role), f64> = HashMap::new();
	for ((node, side, role), group) in &mut port_groups.entries {
		if group.len() < 2 {
			continue;
		}
		v8_sort(group, |a, b| a.1 - b.1);
		let node = &nodes[*node];
		let length = match side.is_vertical() {
			true => node.width.unwrap_or(10.0),
			false => node.height.unwrap_or(10.0),
		};
		let effective = if is_diamond(node) { length * 0.3 } else { length };
		let spacing =
			MAX_PORT_SPACING.min(MIN_PORT_SPACING.max(effective / (group.len() + 1) as f64));
		let start = -(spacing * (group.len() - 1) as f64) / 2.0;
		for (position, &(index, _)) in group.iter().enumerate() {
			port_offsets.insert((index, *role), start + position as f64 * spacing);
		}
	}
	let face_count = |node: usize, side: Side| {
		port_groups.get(&(node, side, Role::Source)).map_or(0, Vec::len)
			+ port_groups.get(&(node, side, Role::Target)).map_or(0, Vec::len)
	};
	let face_has_label_node = |node: usize, side: Side| {
		[Role::Source, Role::Target].iter().any(|&role| {
			port_groups.get(&(node, side, role)).is_some_and(|group| {
				group.iter().any(|&(index, _)| edge(index).label_node_id.is_some())
			})
		})
	};
	let ports_for_edge = |index: usize, source: usize, target: usize| -> Ports {
		let (source_node, target_node) = (&nodes[source], &nodes[target]);
		let info = side_info[index];
		let source_side = info.map_or_else(
			|| choose_side(source_node, center(target_node), Side::Bottom),
			|info| info.source_side,
		);
		let target_side = info.map_or_else(
			|| choose_side(target_node, center(source_node), Side::Bottom),
			|info| info.target_side,
		);
		let mut source_port = match info {
			Some(info) => port_for_side(source_node, info.source_side),
			None => port_for_side(
				source_node,
				choose_side(source_node, center(target_node), Side::Bottom),
			),
		};
		let mut target_port = match info {
			Some(info) => port_for_side(target_node, info.target_side),
			None => {
				port_for_side(target_node, choose_side(target_node, center(source_node), Side::Top))
			}
		};
		let apply = |port: &mut Point, side: Side, offset: Option<&f64>| {
			if let Some(&offset) = offset {
				match side.is_vertical() {
					true => port.x += offset,
					false => port.y += offset,
				}
			}
		};
		apply(&mut source_port, source_side, port_offsets.get(&(index, Role::Source)));
		apply(&mut target_port, target_side, port_offsets.get(&(index, Role::Target)));
		Ports { source: source_port, target: target_port, source_side, target_side }
	};

	for &(index, _, _) in &routing_order {
		router.edge_segments[index].clear();
		if points[index].as_ref().is_some_and(|points| !points.is_empty())
			|| edge(index).start == edge(index).end
		{
			continue;
		}
		let Some((source, target)) = endpoints(&router, index) else {
			continue;
		};
		let (source_node, target_node) = (&nodes[source], &nodes[target]);
		let ports = ports_for_edge(index, source, target);
		let (source_port, target_port) = (ports.source, ports.target);
		let anchor = |port: Point, side: Side, node: &Node| {
			let Point { x, y } = center(node);
			match side.is_vertical() {
				true => Point {
					x: port.x,
					y: if port.y > y { port.y + ANCHOR_OFFSET } else { port.y - ANCHOR_OFFSET },
				},
				false => Point {
					x: if port.x > x { port.x + ANCHOR_OFFSET } else { port.x - ANCHOR_OFFSET },
					y: port.y,
				},
			}
		};
		let mut source_anchor = anchor(source_port, ports.source_side, source_node);
		let mut target_anchor = anchor(target_port, ports.target_side, target_node);
		let inside_obstacle = |router: &Router, point: Point| -> Option<Obstacle> {
			router.obstacles.iter().copied().find(|obstacle| {
				obstacle.node != source
					&& obstacle.node != target
					&& point.x > obstacle.min_x
					&& point.x < obstacle.max_x
					&& point.y > obstacle.min_y
					&& point.y < obstacle.max_y
			})
		};
		// `obstacleDetour`: the corner beyond an obstacle in front of a port.
		let detour = |port: Point,
		              node: &Node,
		              opposite: &Node,
		              obstacle: &Obstacle,
		              vertical: bool|
		 -> (Point, bool) {
			let (own, other) = (center(node), center(opposite));
			match vertical {
				true => {
					let positive = port.y > own.y;
					let x = if other.x >= port.x {
						obstacle.max_x + HORIZONTAL_PIPE_MARGIN
					} else {
						obstacle.min_x - HORIZONTAL_PIPE_MARGIN
					};
					let y = if positive {
						obstacle.max_y + VERTICAL_PIPE_MARGIN
					} else {
						obstacle.min_y - VERTICAL_PIPE_MARGIN
					};
					(Point { x, y }, positive)
				}
				false => {
					let positive = port.x > own.x;
					let x = if positive {
						obstacle.max_x + HORIZONTAL_PIPE_MARGIN
					} else {
						obstacle.min_x - HORIZONTAL_PIPE_MARGIN
					};
					let y = if other.y >= port.y {
						obstacle.max_y + VERTICAL_PIPE_MARGIN
					} else {
						obstacle.min_y - VERTICAL_PIPE_MARGIN
					};
					(Point { x, y }, positive)
				}
			}
		};
		// Anchors inside an obstacle move beyond it; the first point after a port stays
		// orthogonal to it.
		let mut source_waypoints = Vec::new();
		if let Some(obstacle) = inside_obstacle(&router, source_anchor) {
			let vertical = ports.source_side.is_vertical();
			let (corner, positive) =
				detour(source_port, source_node, target_node, &obstacle, vertical);
			source_anchor = corner;
			source_waypoints = match vertical {
				true => {
					let gap_y = match positive {
						true => (obstacle.min_y - 2.0).min(source_port.y + ANCHOR_OFFSET),
						false => (obstacle.max_y + 2.0).max(source_port.y - ANCHOR_OFFSET),
					};
					vec![
						Point { x: source_port.x, y: gap_y },
						Point { x: corner.x, y: gap_y },
						corner,
					]
				}
				false => {
					let gap_x = match positive {
						true => (obstacle.min_x - 2.0).min(source_port.x + ANCHOR_OFFSET),
						false => (obstacle.max_x + 2.0).max(source_port.x - ANCHOR_OFFSET),
					};
					vec![
						Point { x: gap_x, y: source_port.y },
						Point { x: gap_x, y: corner.y },
						corner,
					]
				}
			};
		}
		let mut target_waypoints = Vec::new();
		if let Some(obstacle) = inside_obstacle(&router, target_anchor) {
			let vertical = ports.target_side.is_vertical();
			let (corner, _) = detour(target_port, target_node, source_node, &obstacle, vertical);
			target_anchor = corner;
			target_waypoints = match vertical {
				true => vec![corner, Point { x: target_port.x, y: corner.y }],
				false => vec![corner, Point { x: corner.x, y: target_port.y }],
			};
		}

		// Centred straight-line fast path: facing aligned ports without obstacles in between.
		if source_waypoints.is_empty() && target_waypoints.is_empty() {
			let same_x = (source_anchor.x - target_anchor.x).abs() < HORIZONTAL_PIPE_MARGIN;
			let same_y = (source_anchor.y - target_anchor.y).abs() < HORIZONTAL_PIPE_MARGIN;
			let has_offset = port_offsets.contains_key(&(index, Role::Source))
				|| port_offsets.contains_key(&(index, Role::Target));
			let source_face = face_count(source, ports.source_side);
			let target_face = face_count(target, ports.target_side);
			let contested = source_face > 1 || target_face > 1;
			let incident =
				|node: &Node| incident_totals.get(node.id.as_str()).copied().unwrap_or(0);
			let contested_has_label = (source_face > 1
				&& face_has_label_node(source, ports.source_side))
				|| (target_face > 1 && face_has_label_node(target, ports.target_side));
			let simple_contested = contested
				&& !contested_has_label
				&& (source_face <= 1 || incident(source_node) <= 2)
				&& (target_face <= 1 || incident(target_node) <= 2);
			if (same_x || same_y)
				&& !has_offset
				&& (!contested || simple_contested)
				&& !router.is_blocked(source_port, target_port, source, target)
			{
				points[index] = Some(vec![source_port, source_anchor, target_anchor, target_port]);
				straight_edges[index] = true;
				// Visible to later crossing penalties, but not part of the track assignment.
				let (orientation, coord, from, to) = match same_y {
					true => (
						Orientation::Horizontal,
						source_port.y,
						source_port.x.min(target_port.x),
						source_port.x.max(target_port.x),
					),
					false => (
						Orientation::Vertical,
						source_port.x,
						source_port.y.min(target_port.y),
						source_port.y.max(target_port.y),
					),
				};
				router.pipes.push(Pipe { orientation, coord, tracks: Vec::new() });
				let pipe = router.pipes.len() - 1;
				router.segments.push(RoutedSegment {
					edge: index,
					segment: 0,
					orientation,
					pipe,
					track: 0,
					from,
					to,
				});
				continue;
			}
		}

		let source_pipe = router.pipe(Orientation::Vertical, source_anchor.x);
		source_anchor.x = router.pipes[source_pipe].coord;
		let target_pipe = router.pipe(Orientation::Vertical, target_anchor.x);
		target_anchor.x = router.pipes[target_pipe].coord;

		// The search box: both anchors plus room around obstacles in the way.
		let mut box_min_x = source_anchor.x.min(target_anchor.x) - 50.0;
		let mut box_max_x = source_anchor.x.max(target_anchor.x) + 50.0;
		let mut box_min_y = source_anchor.y.min(target_anchor.y) - 50.0;
		let mut box_max_y = source_anchor.y.max(target_anchor.y) + 50.0;
		let (path_min_x, path_max_x) =
			(source_anchor.x.min(target_anchor.x), source_anchor.x.max(target_anchor.x));
		let (path_min_y, path_max_y) =
			(source_anchor.y.min(target_anchor.y), source_anchor.y.max(target_anchor.y));
		for obstacle in &router.obstacles {
			if obstacle.min_x < path_max_x
				&& obstacle.max_x > path_min_x
				&& obstacle.min_y < path_max_y
				&& obstacle.max_y > path_min_y
			{
				box_min_x = box_min_x.min(obstacle.min_x - ROUTING_MARGIN);
				box_max_x = box_max_x.max(obstacle.max_x + ROUTING_MARGIN);
				box_min_y = box_min_y.min(obstacle.min_y - ROUTING_MARGIN);
				box_max_y = box_max_y.max(obstacle.max_y + ROUTING_MARGIN);
			}
		}
		for obstacle in router.obstacles.clone() {
			if obstacle.max_x < box_min_x
				|| obstacle.min_x > box_max_x
				|| obstacle.max_y < box_min_y
				|| obstacle.min_y > box_max_y
			{
				continue;
			}
			router.pipe(Orientation::Horizontal, obstacle.min_y - HORIZONTAL_PIPE_MARGIN);
			router.pipe(Orientation::Horizontal, obstacle.max_y + HORIZONTAL_PIPE_MARGIN);
			router.pipe(Orientation::Vertical, obstacle.min_x - VERTICAL_PIPE_MARGIN);
			router.pipe(Orientation::Vertical, obstacle.max_x + VERTICAL_PIPE_MARGIN);
		}
		router.pipe(Orientation::Horizontal, source_anchor.y);
		router.pipe(Orientation::Horizontal, target_anchor.y);

		let blocked =
			|router: &Router, p1: Point, p2: Point| router.is_blocked(p1, p2, source, target);
		let corner_hv = Point { x: target_anchor.x, y: source_anchor.y };
		let corner_vh = Point { x: source_anchor.x, y: target_anchor.y };
		let mut found: Vec<Point> = Vec::new();
		if !blocked(&router, source_anchor, corner_hv)
			&& !blocked(&router, corner_hv, target_anchor)
		{
			found = match (source_anchor.y - target_anchor.y).abs() < EPSILON
				|| (source_anchor.x - target_anchor.x).abs() < EPSILON
			{
				true => vec![source_anchor, target_anchor],
				false => vec![source_anchor, corner_hv, target_anchor],
			};
		} else if !blocked(&router, source_anchor, corner_vh)
			&& !blocked(&router, corner_vh, target_anchor)
		{
			found = match (source_anchor.x - target_anchor.x).abs() < EPSILON {
				true => vec![source_anchor, target_anchor],
				false => vec![source_anchor, corner_vh, target_anchor],
			};
		}
		if found.is_empty() {
			found = search(
				&router,
				index,
				source_anchor,
				target_anchor,
				source,
				target,
				(box_min_x, box_max_x, box_min_y, box_max_y),
			);
		}
		if found.is_empty() {
			found = vec![
				source_anchor,
				Point { x: source_anchor.x, y: target_anchor.y },
				target_anchor,
			];
		}
		if found.len() > 4
			&& let Some(simplified) =
				simplify_detour(&router, &found, is_left_right, source, target)
		{
			found = simplified;
		}

		// Ports, handle waypoints and the path; an overshoot before the target port is dropped.
		let mut full = vec![source_port];
		full.extend(source_waypoints);
		full.extend(found);
		full.extend(target_waypoints.into_iter().rev());
		full.push(target_port);
		if full.len() >= 3 {
			let count = full.len();
			let (a, b, c) = (full[count - 3], full[count - 2], full[count - 1]);
			let horizontal = (a.y - b.y).abs() < EPSILON && (b.y - c.y).abs() < EPSILON;
			let vertical = (a.x - b.x).abs() < EPSILON && (b.x - c.x).abs() < EPSILON;
			let overshoots = |ab: f64, ac: f64| {
				let sign = js_sign(ab);
				sign != 0.0 && sign == js_sign(ac) && ab.abs() > ac.abs()
			};
			if (horizontal && overshoots(b.x - a.x, c.x - a.x))
				|| (!horizontal && vertical && overshoots(b.y - a.y, c.y - a.y))
			{
				full.remove(count - 2);
			}
		}
		let mut simplified = vec![full[0]];
		for position in 1..full.len() - 1 {
			let current = full[position];
			// The anchor stays, Mermaid's tests expect four points for Z and U shapes.
			if position == 1 {
				simplified.push(current);
				continue;
			}
			let previous = simplified[simplified.len() - 1];
			let next = full[position + 1];
			if (previous.y - current.y).abs() < EPSILON && (current.y - next.y).abs() < EPSILON {
				if (current.x > previous.x) != (next.x > current.x) {
					simplified.push(current);
				}
				continue;
			}
			if (previous.x - current.x).abs() < EPSILON && (current.x - next.x).abs() < EPSILON {
				if (current.y > previous.y) != (next.y > current.y) {
					simplified.push(current);
				}
				continue;
			}
			simplified.push(current);
		}
		simplified.push(full[full.len() - 1]);
		for (segment, pair) in simplified.windows(2).enumerate() {
			let (p1, p2) = (pair[0], pair[1]);
			let orientation = match (p1.x - p2.x).abs() < EPSILON {
				true => Orientation::Vertical,
				false => Orientation::Horizontal,
			};
			let (coord, from, to) = match orientation {
				Orientation::Vertical => (p1.x, p1.y.min(p2.y), p1.y.max(p2.y)),
				Orientation::Horizontal => (p1.y, p1.x.min(p2.x), p1.x.max(p2.x)),
			};
			let pipe = router.pipe(orientation, coord);
			let routed =
				RoutedSegment { edge: index, segment, orientation, pipe, track: 0, from, to };
			router.segments.push(routed);
			router.edge_segments[index].push(router.segments.len() - 1);
			let tracks = &mut router.pipes[pipe].tracks;
			if tracks.is_empty() {
				tracks.push(Track { index: 0, segments: Vec::new() });
			}
			tracks[0].segments.push(routed.reference());
		}
	}

	// Phase 2: crossing reduction.
	let mut destinations: HashMap<usize, Destination> = HashMap::new();
	for _ in 0..10 {
		let mut changed = 0;
		changed += fix_handle_crossings(&mut router, &edges, layout, &mut destinations, true);
		changed += fix_handle_crossings(&mut router, &edges, layout, &mut destinations, false);
		changed += fix_pipe_crossings(&mut router);
		if changed == 0 {
			break;
		}
	}

	// Phase 3: coordinates of the tracks, clusters of overlapping segments per pipe.
	let mut segment_coords: HashMap<(usize, usize), f64> = HashMap::new();
	for &pipe in &router.registered {
		let pipe_coord = router.pipes[pipe].coord;
		let mut segments: Vec<(SegmentRef, usize)> = Vec::new();
		for track in &router.pipes[pipe].tracks {
			for reference in &track.segments {
				segments.push((*reference, track.index));
			}
		}
		v8_sort(&mut segments, |a, b| a.0.from - b.0.from);
		let mut clusters: Vec<Vec<(SegmentRef, usize)>> = Vec::new();
		if let Some(&first) = segments.first() {
			let mut current = vec![first];
			let mut cluster_end = first.0.to;
			for &segment in &segments[1..] {
				if segment.0.from < cluster_end {
					current.push(segment);
					cluster_end = cluster_end.max(segment.0.to);
				} else {
					clusters.push(std::mem::replace(&mut current, vec![segment]));
					cluster_end = segment.0.to;
				}
			}
			clusters.push(current);
		}
		for cluster in clusters {
			let mut used: Vec<usize> = Vec::new();
			let mut scores: HashMap<usize, f64> = HashMap::new();
			for &(reference, track) in &cluster {
				if !used.contains(&track) {
					used.push(track);
				}
				let delta = destination(&router, &mut destinations, reference.edge).delta;
				*scores.entry(track).or_insert(0.0) += delta;
			}
			let score = |track: usize| scores.get(&track).copied().unwrap_or(0.0);
			let mut left: Vec<usize> =
				used.iter().copied().filter(|&track| score(track) < -1.0).collect();
			let mut right: Vec<usize> =
				used.iter().copied().filter(|&track| score(track) > 1.0).collect();
			let mut neutral: Vec<usize> =
				used.iter().copied().filter(|&track| score(track).abs() <= 1.0).collect();
			v8_sort(&mut left, |&a, &b| score(b) - score(a));
			v8_sort(&mut right, |&a, &b| score(a) - score(b));
			let mut assign = |track: usize, coord: f64| {
				for &(reference, _) in cluster.iter().filter(|(_, candidate)| *candidate == track) {
					let effective = if straight_edges[reference.edge] { pipe_coord } else { coord };
					segment_coords.insert((reference.edge, reference.segment), effective);
				}
			};
			for (count, &track) in left.iter().enumerate() {
				assign(track, pipe_coord - (count + 1) as f64 * TRACK_SPACING);
			}
			if neutral.is_empty() && !used.is_empty() {
				let mut by_closeness = used.clone();
				v8_sort(&mut by_closeness, |&a, &b| score(a).abs() - score(b).abs());
				let best = by_closeness[0];
				left.retain(|&track| track != best);
				right.retain(|&track| track != best);
				neutral.push(best);
			}
			for (assigned, &track) in neutral.iter().enumerate() {
				let coord = match assigned {
					0 => pipe_coord,
					_ => {
						let direction = if assigned % 2 == 1 { 1.0 } else { -1.0 };
						pipe_coord + direction * assigned.div_ceil(2) as f64 * TRACK_SPACING * 0.5
					}
				};
				assign(track, coord);
			}
			for (count, &track) in right.iter().enumerate() {
				assign(track, pipe_coord + (count + 1) as f64 * TRACK_SPACING);
			}
		}
	}

	for (index, routed) in points.iter_mut().enumerate() {
		let indices = &router.edge_segments[index];
		if indices.is_empty() {
			continue;
		}
		let Some((source, target)) = endpoints(&router, index) else {
			continue;
		};
		let ports = ports_for_edge(index, source, target);
		let lines: Vec<Line> = indices
			.iter()
			.map(|&segment| {
				let segment = router.segments[segment];
				Line {
					orientation: segment.orientation,
					coord: segment_coords
						.get(&(segment.edge, segment.segment))
						.copied()
						.unwrap_or(router.pipes[segment.pipe].coord),
					from: segment.from,
					to: segment.to,
				}
			})
			.collect();
		let mut new_points = vec![ports.source];
		for (position, line) in lines.iter().enumerate() {
			let previous = new_points[new_points.len() - 1];
			let (previous_along, previous_track) = match line.orientation {
				Orientation::Vertical => (previous.y, previous.x),
				Orientation::Horizontal => (previous.x, previous.y),
			};
			if (previous_track - line.coord).abs() > EPSILON {
				new_points.push(line.point(previous_along));
			}
			match lines.get(position + 1) {
				Some(next) if next.orientation == line.orientation => {
					if (line.coord - next.coord).abs() > EPSILON {
						let junction = match line.orientation {
							Orientation::Vertical => (previous_along + next.from) / 2.0,
							Orientation::Horizontal => line.shared_end(next),
						};
						new_points.push(line.point(junction));
						new_points.push(next.point(junction));
					} else if position == 0 || position + 2 == lines.len() {
						new_points.push(line.point(line.shared_end(next)));
					}
				}
				Some(next) => new_points.push(line.point(next.coord)),
				None => {
					let end = match (line.from - previous_along).abs()
						< (line.to - previous_along).abs()
					{
						true => line.to,
						false => line.from,
					};
					new_points.push(line.point(end));
				}
			}
		}
		let last = new_points[new_points.len() - 1];
		if (last.x - ports.target.x).abs() > EPSILON || (last.y - ports.target.y).abs() > EPSILON {
			new_points.push(ports.target);
		}
		let mut filtered: Vec<Point> = Vec::with_capacity(new_points.len());
		for point in new_points {
			if filtered.last().is_none_or(|previous| {
				(point.x - previous.x).abs() > EPSILON || (point.y - previous.y).abs() > EPSILON
			}) {
				filtered.push(point);
			}
		}
		*routed = Some(filtered);
	}

	let node_by_id: HashMap<String, usize> =
		router.node_by_id.iter().map(|(&id, &index)| (id.to_string(), index)).collect();
	let clamped: Vec<(usize, Vec<Point>)> =
		edges.iter().zip(points).filter_map(|(&edge, points)| Some((edge, points?))).collect();
	for (edge, points) in clamped {
		layout.edges[edge].points = Some(points);
	}
	layout.edges.retain(|edge| !edge.is_layout_only);
	// Endpoints inside their node snap to its nearest side.
	for edge in &mut layout.edges {
		let Some(points) = edge.points.as_mut().filter(|points| points.len() >= 2) else {
			continue;
		};
		if let Some(&source) = node_by_id.get(&edge.start) {
			points[0] = clamp_to_boundary(points[0], &layout.nodes[source]);
		}
		if let Some(&target) = node_by_id.get(&edge.end) {
			let last = points.len() - 1;
			points[last] = clamp_to_boundary(points[last], &layout.nodes[target]);
		}
	}
}

/// `Math.sign` without its `NaN` and `-0`, which the router never compares.
fn js_sign(value: f64) -> f64 {
	if value > 0.0 {
		1.0
	} else if value < 0.0 {
		-1.0
	} else {
		0.0
	}
}

/// The lane (top-level group) of every node, later groups overwriting earlier ones.
fn assign_lanes(nodes: &[Node]) -> HashMap<&str, &str> {
	let mut lane_of = HashMap::new();
	for group in nodes.iter().filter(|node| node.is_group && node.parent_id.is_none()) {
		let mut stack = vec![group];
		let mut visited = std::collections::HashSet::new();
		while let Some(node) = stack.pop() {
			if !visited.insert(node.id.as_str()) {
				continue;
			}
			lane_of.insert(node.id.as_str(), group.id.as_str());
			// Children in reverse so they are visited in source order, like the recursion.
			for child in nodes
				.iter()
				.rev()
				.filter(|child| child.parent_id.as_deref() == Some(node.id.as_str()))
			{
				stack.push(child);
			}
		}
	}
	lane_of
}

/// `nodeBoundaryClamp`: a point inside a node moves to its nearest side.
fn clamp_to_boundary(point: Point, node: &Node) -> Point {
	let Point { x, y } = center(node);
	let (width, height) = (node.width.unwrap_or(0.0), node.height.unwrap_or(0.0));
	if width <= 0.0 || height <= 0.0 {
		return point;
	}
	let (left, right, top, bottom) =
		(x - width / 2.0, x + width / 2.0, y - height / 2.0, y + height / 2.0);
	if point.x < left || point.x > right || point.y < top || point.y > bottom {
		return point;
	}
	let distances = [point.x - left, right - point.x, point.y - top, bottom - point.y];
	let minimum = distances.iter().copied().fold(f64::INFINITY, f64::min);
	if minimum == distances[0] {
		Point { x: left, y: point.y }
	} else if minimum == distances[1] {
		Point { x: right, y: point.y }
	} else if minimum == distances[2] {
		Point { x: point.x, y: top }
	} else {
		Point { x: point.x, y: bottom }
	}
}

/// A rebuilt segment (`RoutedLine`).
struct Line {
	orientation: Orientation,
	coord: f64,
	from: f64,
	to: f64,
}

impl Line {
	fn point(&self, along: f64) -> Point {
		match self.orientation {
			Orientation::Vertical => Point { x: self.coord, y: along },
			Orientation::Horizontal => Point { x: along, y: self.coord },
		}
	}

	/// `sharedLineEndpointCoord`.
	fn shared_end(&self, next: &Line) -> f64 {
		match (self.to - next.from).abs() < EPSILON || (self.to - next.to).abs() < EPSILON {
			true => self.to,
			false => self.from,
		}
	}
}

/// The A* over the pipe grid: steps along pipes to the next
/// crossing pipe, with penalties for crossings, wrong directions and bends.
fn search(
	router: &Router,
	edge: usize,
	start: Point,
	end: Point,
	source: usize,
	target: usize,
	bounds: (f64, f64, f64, f64),
) -> Vec<Point> {
	let (min_x, max_x, min_y, max_y) = bounds;
	let mut horizontal: Vec<f64> = Vec::new();
	let mut vertical: Vec<f64> = Vec::new();
	for &pipe in &router.registered {
		let pipe = &router.pipes[pipe];
		match pipe.orientation {
			Orientation::Horizontal if pipe.coord >= min_y && pipe.coord <= max_y => {
				horizontal.push(pipe.coord)
			}
			Orientation::Vertical if pipe.coord >= min_x && pipe.coord <= max_x => {
				vertical.push(pipe.coord)
			}
			_ => {}
		}
	}
	v8_sort(&mut horizontal, |a, b| a - b);
	v8_sort(&mut vertical, |a, b| a - b);
	// `toFixed(1)` keys as tenths.
	let key = |point: Point| ((point.x * 10.0).round() as i64, (point.y * 10.0).round() as i64);
	#[derive(Clone, Copy, PartialEq)]
	enum Arrival {
		None,
		Horizontal,
		Vertical,
	}
	struct Open {
		key: (i64, i64),
		f: f64,
		point: Point,
	}
	let end_key = key(end);
	let mut g_score: HashMap<(i64, i64), f64> = HashMap::new();
	let mut came_from: HashMap<(i64, i64), Point> = HashMap::new();
	let mut arrival: HashMap<(i64, i64), Arrival> = HashMap::new();
	let mut open_set: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
	let start_key = key(start);
	g_score.insert(start_key, 0.0);
	arrival.insert(start_key, Arrival::None);
	open_set.insert(start_key);
	let mut open =
		vec![Open { key: start_key, f: (end.x - start.x).hypot(end.y - start.y), point: start }];
	let destination = (end.x - start.x, end.y - start.y);
	for _ in 0..MAX_SEARCH_STEPS {
		if open.is_empty() {
			break;
		}
		v8_sort(&mut open, |a, b| a.f - b.f);
		let current = open.remove(0);
		open_set.remove(&current.key);
		if current.key == end_key {
			let mut path = vec![end];
			let mut current_key = end_key;
			let mut steps = 0;
			while let Some(&previous) = came_from.get(&current_key) {
				path.push(previous);
				current_key = key(previous);
				steps += 1;
				if steps > came_from.len() {
					break;
				}
			}
			path.reverse();
			return path;
		}
		let Point { x, y } = current.point;
		let v_index = vertical.iter().position(|&coord| (coord - x).abs() < 1.0);
		let h_index = horizontal.iter().position(|&coord| (coord - y).abs() < 1.0);
		let mut neighbours = Vec::with_capacity(4);
		if let Some(v_index) = v_index {
			if v_index > 0 {
				neighbours.push(Point { x: vertical[v_index - 1], y });
			}
			if v_index + 1 < vertical.len() {
				neighbours.push(Point { x: vertical[v_index + 1], y });
			}
		}
		if let Some(h_index) = h_index {
			if h_index > 0 {
				neighbours.push(Point { x, y: horizontal[h_index - 1] });
			}
			if h_index + 1 < horizontal.len() {
				neighbours.push(Point { x, y: horizontal[h_index + 1] });
			}
		}
		for neighbour in neighbours {
			let (low_x, high_x) = (x.min(neighbour.x), x.max(neighbour.x));
			let (low_y, high_y) = (y.min(neighbour.y), y.max(neighbour.y));
			let blocked = router.obstacles.iter().any(|obstacle| {
				if obstacle.node == source || obstacle.node == target {
					return false;
				}
				match low_x != high_x {
					true => {
						obstacle.min_y < y
							&& obstacle.max_y > y && obstacle.max_x > low_x
							&& obstacle.min_x < high_x
					}
					false => {
						obstacle.min_x < x
							&& obstacle.max_x > x && obstacle.max_y > low_y
							&& obstacle.min_y < high_y
					}
				}
			});
			if blocked {
				continue;
			}
			let neighbour_key = key(neighbour);
			let distance = (neighbour.x - x).abs() + (neighbour.y - y).abs();
			let penalty = router.crossing_penalty(edge, current.point, neighbour);
			let (move_x, move_y) = (neighbour.x - x, neighbour.y - y);
			let mut direction_penalty = 0.0;
			if (destination.1 > 10.0 && move_y < -5.0) || (destination.1 < -10.0 && move_y > 5.0) {
				direction_penalty = move_y.abs() * 100.0;
			}
			if (destination.0 > 10.0 && move_x < -5.0) || (destination.0 < -10.0 && move_x > 5.0) {
				direction_penalty += move_x.abs() * 50.0;
			}
			let current_arrival = arrival.get(&current.key).copied().unwrap_or(Arrival::None);
			let move_arrival =
				if move_x.abs() > EPSILON { Arrival::Horizontal } else { Arrival::Vertical };
			let bend_penalty =
				if current_arrival != Arrival::None && current_arrival != move_arrival {
					50.0
				} else {
					0.0
				};
			let tentative = g_score.get(&current.key).copied().unwrap_or(f64::INFINITY)
				+ distance + penalty
				+ direction_penalty
				+ bend_penalty;
			let heuristic = (end.x - neighbour.x).abs() + (end.y - neighbour.y).abs();
			if tentative < g_score.get(&neighbour_key).copied().unwrap_or(f64::INFINITY) {
				came_from.insert(neighbour_key, current.point);
				g_score.insert(neighbour_key, tentative);
				arrival.insert(neighbour_key, move_arrival);
				if open_set.insert(neighbour_key) {
					open.push(Open {
						key: neighbour_key,
						f: tentative + heuristic,
						point: neighbour,
					});
				} else if let Some(entry) = open.iter_mut().find(|entry| entry.key == neighbour_key)
				{
					entry.f = tentative + heuristic;
				}
			}
		}
	}
	Vec::new()
}

/// The path simplification after A*: a U-shaped detour through the extreme x of the path,
/// returning right behind the obstacles.
fn simplify_detour(
	router: &Router,
	path: &[Point],
	is_left_right: bool,
	source: usize,
	target: usize,
) -> Option<Vec<Point>> {
	let start = path[0];
	let end = path[path.len() - 1];
	let mut min_x = start.x.min(end.x);
	let mut max_x = start.x.max(end.x);
	for point in path {
		min_x = min_x.min(point.x);
		max_x = max_x.max(point.x);
	}
	let went_right = max_x > start.x.max(end.x);
	let went_left = min_x < start.x.min(end.x);
	let obstacles = &router.obstacles;
	if is_left_right {
		let margin = VERTICAL_PIPE_MARGIN;
		if went_right {
			let path_x = start.x.max(end.x);
			let (path_min_y, path_max_y) = (start.y.min(end.y), start.y.max(end.y));
			let detours: Vec<&Obstacle> = obstacles
				.iter()
				.filter(|o| {
					o.min_x < path_x
						&& o.max_x > path_x
						&& o.min_y < path_max_y
						&& o.max_y > path_min_y
				})
				.collect();
			if !detours.is_empty() {
				let mut visual_max_x = start.x.max(end.x);
				for obstacle in detours {
					if obstacle.visual_x_half_extent.is_nan() {
						continue;
					}
					visual_max_x = visual_max_x.max(
						(obstacle.min_x + obstacle.max_x) / 2.0
							+ obstacle.visual_x_half_extent
							+ margin,
					);
				}
				if !visual_max_x.is_nan() {
					max_x = visual_max_x;
				}
			}
		}
		if went_left {
			let detours: Vec<&Obstacle> = obstacles
				.iter()
				.filter(|o| {
					o.min_x < start.x.min(end.x) + margin
						&& o.min_y < start.y.max(end.y)
						&& o.max_y > start.y.min(end.y)
				})
				.collect();
			if !detours.is_empty() {
				let mut visual_min_x = start.x.min(end.x);
				for obstacle in detours {
					visual_min_x = visual_min_x.min(
						(obstacle.min_x + obstacle.max_x) / 2.0
							- obstacle.visual_x_half_extent
							- margin,
					);
				}
				min_x = visual_min_x;
			}
		}
	}
	let best_return_y = |detour_x: f64| -> f64 {
		let going_down = end.y > start.y;
		let relevant: Vec<&Obstacle> = obstacles
			.iter()
			.filter(|o| {
				start.x.min(end.x) < o.max_x
					&& start.x.max(end.x) > o.min_x
					&& start.y.min(end.y) < o.max_y
					&& start.y.max(end.y) > o.min_y
			})
			.collect();
		let mut filtered = relevant.clone();
		if is_left_right && !relevant.is_empty() {
			let at_detour: Vec<&Obstacle> = relevant
				.iter()
				.copied()
				.filter(|o| o.min_x < detour_x && o.max_x > detour_x)
				.collect();
			if !at_detour.is_empty() {
				filtered = at_detour;
			}
		}
		if filtered.is_empty() {
			return end.y;
		}
		match going_down {
			true => {
				let best = filtered.iter().map(|o| o.max_y).fold(f64::NEG_INFINITY, f64::max)
					+ HORIZONTAL_PIPE_MARGIN;
				if best < end.y - EPSILON {
					return best;
				}
			}
			false => {
				let best = filtered.iter().map(|o| o.min_y).fold(f64::INFINITY, f64::min)
					- HORIZONTAL_PIPE_MARGIN;
				if best > end.y + EPSILON {
					return best;
				}
			}
		}
		end.y
	};
	let try_detour = |detour_x: f64| -> Option<Vec<Point>> {
		let best_y = best_return_y(detour_x);
		let corner1 = Point { x: detour_x, y: start.y };
		let corner2 = Point { x: detour_x, y: best_y };
		let corner3 = Point { x: end.x, y: best_y };
		let blocked = |a: Point, b: Point| router.is_blocked(a, b, source, target);
		if blocked(start, corner1)
			|| blocked(corner1, corner2)
			|| blocked(corner2, corner3)
			|| (best_y != end.y && blocked(corner3, end))
		{
			return None;
		}
		Some(match (best_y - end.y).abs() < EPSILON {
			true => vec![start, corner1, corner2, end],
			false => vec![start, corner1, corner2, corner3, end],
		})
	};
	match (went_right, went_left) {
		(true, false) => try_detour(max_x),
		(false, true) => try_detour(min_x),
		_ => None,
	}
}

/// `getDestInfo`: where the first horizontal segment of an edge leads, relative to its first
/// segment's pipe. Cached at the first request like Mermaid's cache.
#[derive(Clone, Copy)]
struct Destination {
	destination: f64,
	deviation: f64,
	delta: f64,
}

fn destination(
	router: &Router,
	cache: &mut HashMap<usize, Destination>,
	edge: usize,
) -> Destination {
	if let Some(&info) = cache.get(&edge) {
		return info;
	}
	let indices = &router.edge_segments[edge];
	let info = match indices.first() {
		None => Destination { destination: 0.0, deviation: 0.0, delta: 0.0 },
		Some(&first) => {
			let base = router.pipes[router.segments[first].pipe].coord;
			let mut destination = base;
			if let Some(segment) = indices[1..]
				.iter()
				.map(|&index| router.segments[index])
				.find(|segment| segment.orientation == Orientation::Horizontal)
			{
				destination = match (segment.from - base).abs() > (segment.to - base).abs() {
					true => segment.from,
					false => segment.to,
				};
			}
			Destination {
				destination,
				deviation: (destination - base).abs(),
				delta: destination - base,
			}
		}
	};
	cache.insert(edge, info);
	info
}

fn segments_overlap(a: (f64, f64), b: (f64, f64)) -> bool {
	a.0 < b.1 && b.0 < a.1
}

impl Router<'_> {
	fn span(&self, segment: usize) -> (f64, f64) {
		(self.segments[segment].from, self.segments[segment].to)
	}

	/// `trySwapSegmentsAcrossTracks`.
	fn try_swap(&mut self, first: usize, second: usize) -> bool {
		let (s1, s2) = (self.segments[first], self.segments[second]);
		let (t1, t2) = (s1.track, s2.track);
		let can_first_move = !self.pipes[s2.pipe].tracks[t2].segments.iter().any(|r| {
			!r.is(s2.edge, s2.segment) && segments_overlap((r.from, r.to), self.span(first))
		});
		let can_second_move = !self.pipes[s1.pipe].tracks[t1].segments.iter().any(|r| {
			!r.is(s1.edge, s1.segment) && segments_overlap((r.from, r.to), self.span(second))
		});
		if !(can_first_move && can_second_move) {
			return false;
		}
		let index_1 = self.pipes[s2.pipe].tracks[t2].index;
		let index_2 = self.pipes[s1.pipe].tracks[t1].index;
		self.segments[first].track = index_1;
		self.segments[second].track = index_2;
		let track = &mut self.pipes[s1.pipe].tracks[t1].segments;
		track.retain(|r| !r.is(s1.edge, s1.segment));
		track.push(s2.reference());
		let track = &mut self.pipes[s2.pipe].tracks[t2].segments;
		track.retain(|r| !r.is(s2.edge, s2.segment));
		track.push(s1.reference());
		true
	}

	fn create_track(&mut self, pipe: usize) -> usize {
		let tracks = &mut self.pipes[pipe].tracks;
		let index = tracks.len();
		tracks.push(Track { index, segments: Vec::new() });
		index
	}

	fn move_to_track(&mut self, segment: usize, track: usize) {
		let routed = self.segments[segment];
		self.pipes[routed.pipe].tracks[routed.track]
			.segments
			.retain(|r| !r.is(routed.edge, routed.segment));
		self.segments[segment].track = track;
		self.pipes[routed.pipe].tracks[track].segments.push(routed.reference());
	}

	/// `moveSegmentChainToTrack`: all segments of the edge in the same pipe.
	fn move_chain_to_track(&mut self, segment: usize, track: usize) {
		let (edge, pipe) = (self.segments[segment].edge, self.segments[segment].pipe);
		for index in self.edge_segments[edge].clone() {
			if self.segments[index].pipe == pipe {
				self.move_to_track(index, track);
			}
		}
	}

	fn adjacent_along_edge(&self, segment: usize) -> Vec<usize> {
		let indices = &self.edge_segments[self.segments[segment].edge];
		let mut adjacent = Vec::new();
		// `indexOf` is -1 for fast-path segments, which have no list.
		match indices.iter().position(|&index| index == segment) {
			Some(position) => {
				if position > 0 {
					adjacent.push(indices[position - 1]);
				}
				if position + 1 < indices.len() {
					adjacent.push(indices[position + 1]);
				}
			}
			None => {
				if let Some(&first) = indices.first() {
					adjacent.push(first);
				}
			}
		}
		adjacent
	}

	fn have_any_crossing(&self, a: usize, b: usize) -> bool {
		let (a, b) = (self.segments[a], self.segments[b]);
		if a.orientation == b.orientation {
			return false;
		}
		let (h, v) = match a.orientation {
			Orientation::Horizontal => (a, b),
			Orientation::Vertical => (b, a),
		};
		let (v_coord, h_coord) = (self.pipes[v.pipe].coord, self.pipes[h.pipe].coord);
		v_coord > h.from && v_coord < h.to && h_coord > v.from && h_coord < v.to
	}

	fn find_available_track(&self, pipe: usize, segment: usize) -> Option<usize> {
		let routed = self.segments[segment];
		self.pipes[pipe]
			.tracks
			.iter()
			.find(|track| {
				!track.segments.iter().any(|r| {
					!r.is(routed.edge, routed.segment)
						&& segments_overlap((r.from, r.to), (routed.from, routed.to))
				})
			})
			.map(|track| track.index)
	}

	fn segments_conflict(&self, first: usize, second: usize) -> bool {
		if self.segments[first].track == self.segments[second].track {
			return segments_overlap(self.span(first), self.span(second));
		}
		let (adjacent_1, adjacent_2) =
			(self.adjacent_along_edge(first), self.adjacent_along_edge(second));
		adjacent_1.iter().any(|&a| adjacent_2.iter().any(|&b| self.have_any_crossing(a, b)))
	}

	fn resolve_conflict(&mut self, first: usize, second: usize, chain: bool) {
		if self.try_swap(first, second) {
			return;
		}
		let pipe = self.segments[first].pipe;
		let track = match self.find_available_track(pipe, second) {
			Some(track) => track,
			None => self.create_track(pipe),
		};
		match chain {
			true => self.move_chain_to_track(second, track),
			false => self.move_to_track(second, track),
		}
	}
}

/// `fixSourceHandleCrossings` / `fixTargetHandleCrossings`: the first (last) segments of the
/// edges of a node, ordered so that edges going straight keep the centre track.
fn fix_handle_crossings(
	router: &mut Router,
	edges: &[usize],
	layout: &Layout,
	cache: &mut HashMap<usize, Destination>,
	at_source: bool,
) -> usize {
	let mut groups: OrderedMap<&str, Vec<usize>> = OrderedMap::new();
	for (index, &edge) in edges.iter().enumerate() {
		if router.edge_segments[index].is_empty() {
			continue;
		}
		let edge = &layout.edges[edge];
		let node = if at_source { edge.start.as_str() } else { edge.end.as_str() };
		groups.entry(node, Vec::new).push(index);
	}
	let distance = |router: &Router, index: usize| {
		let edge = &layout.edges[edges[index]];
		match (router.node(&edge.start), router.node(&edge.end)) {
			(Some(source), Some(target)) => {
				let (a, b) = (center(source), center(target));
				(b.x - a.x).abs() + (b.y - a.y).abs()
			}
			_ => 0.0,
		}
	};
	let mut crossings = 0;
	for (_, mut group) in groups.entries {
		if at_source {
			for &index in &group {
				destination(router, cache, index);
			}
			let router_ref = &*router;
			v8_sort(&mut group, |&a, &b| {
				let (info_a, info_b) = (cache[&a], cache[&b]);
				if (info_a.deviation - info_b.deviation).abs() > 1.0 {
					return info_a.deviation - info_b.deviation;
				}
				if (info_a.destination - info_b.destination).abs() > 1.0 {
					return info_a.destination - info_b.destination;
				}
				let (distance_a, distance_b) = (distance(router_ref, a), distance(router_ref, b));
				if (distance_a - distance_b).abs() > 1.0 {
					return distance_b - distance_a;
				}
				let (length_a, length_b) =
					(router_ref.edge_segments[a].len(), router_ref.edge_segments[b].len());
				if length_a != length_b {
					return length_a as f64 - length_b as f64;
				}
				if length_a == 1 {
					let span = |index: usize| {
						let segment = router_ref.segments[router_ref.edge_segments[index][0]];
						(segment.to - segment.from).abs()
					};
					let (span_a, span_b) = (span(a), span(b));
					if (span_a - span_b).abs() > 1.0 {
						return span_a - span_b;
					}
				}
				0.0
			});
		} else {
			let router_ref = &*router;
			let approach = |index: usize| {
				let indices = &router_ref.edge_segments[index];
				match indices.len() {
					0 | 1 => 0.0,
					count => {
						let previous = router_ref.segments[indices[count - 2]];
						(previous.to - previous.from).abs()
					}
				}
			};
			v8_sort(&mut group, |&a, &b| {
				let (score_a, score_b) = (approach(a), approach(b));
				if (score_a - score_b).abs() > 0.1 {
					return score_a - score_b;
				}
				a as f64 - b as f64
			});
		}
		let handles: Vec<usize> = group
			.iter()
			.map(|&index| {
				let indices = &router.edge_segments[index];
				if at_source { indices[0] } else { indices[indices.len() - 1] }
			})
			.collect();
		for i in 0..handles.len() {
			for j in i + 1..handles.len() {
				let (h1, h2) = (handles[i], handles[j]);
				if router.segments[h1].pipe != router.segments[h2].pipe {
					continue;
				}
				if router.segments_conflict(h1, h2) {
					crossings += 1;
					router.resolve_conflict(h1, h2, true);
				}
			}
		}
	}
	crossings
}

/// `fixPipeCrossings`: conflicting segments of a pipe move to free or new tracks.
fn fix_pipe_crossings(router: &mut Router) -> usize {
	let mut crossings = 0;
	for pipe in router.registered.clone() {
		let mut pipe_segments: Vec<usize> = Vec::new();
		for track in &router.pipes[pipe].tracks {
			for reference in &track.segments {
				if let Some(&index) = router.edge_segments[reference.edge]
					.iter()
					.find(|&&index| router.segments[index].segment == reference.segment)
				{
					pipe_segments.push(index);
				}
			}
		}
		let segments = &router.segments;
		v8_sort(&mut pipe_segments, |&a, &b| {
			let (a, b) = (segments[a], segments[b]);
			match a.edge != b.edge {
				true => a.edge as f64 - b.edge as f64,
				false => a.segment as f64 - b.segment as f64,
			}
		});
		for i in 0..pipe_segments.len() {
			for j in i + 1..pipe_segments.len() {
				let (s1, s2) = (pipe_segments[i], pipe_segments[j]);
				if router.segments_conflict(s1, s2) {
					crossings += 1;
					router.resolve_conflict(s1, s2, false);
				}
			}
		}
	}
	crossings
}
