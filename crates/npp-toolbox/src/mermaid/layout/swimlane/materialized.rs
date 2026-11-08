//! Clean-up passes on the rendered edge geometry (`direction/materializedGeometry.ts`): terminal
//! lanes on shared faces, doglegs, rails hugging obstacles, title bands, crossing reduction.

use super::geometry::{
	Rect, RectEntry, RectSide, Segment, count_bends, dedupe, is_horizontal, is_vertical, node_rect,
	node_rect_entries, orthogonal_port_path, overlap_length, same_axis_overlap_length, same_point,
	same_side_track_path, same_x, same_y, segment_hits_any_rect, segments, segments_strictly_cross,
	simplify,
};
use super::{Edge, Node, NodeMap, Point};

const EPS_LOCAL: f64 = 1e-3;
const EPS: f64 = super::geometry::EPS;
const MIN_SHARED: f64 = 8.0;
const TRACK_SHIFT: f64 = 7.0;
const SHIFTS: [f64; 6] = [
	-TRACK_SHIFT,
	TRACK_SHIFT,
	-2.0 * TRACK_SHIFT,
	2.0 * TRACK_SHIFT,
	-3.0 * TRACK_SHIFT,
	3.0 * TRACK_SHIFT,
];

fn aligned(a: Point, b: Point) -> bool {
	same_x(a, b, EPS_LOCAL) || same_y(a, b, EPS_LOCAL)
}

fn points_of(edge: &Edge) -> &[Point] {
	edge.points.as_deref().unwrap_or(&[])
}

fn node_by_id<'a>(nodes: &'a [Node], map: &NodeMap, id: &str) -> Option<&'a Node> {
	map.by_id.get(id).map(|&index| &nodes[index])
}

/// The nodes in the order of Mermaid's `nodeByIdMap.values()`.
fn ordered_nodes(nodes: &[Node], map: &NodeMap) -> Vec<Node> {
	map.order.iter().map(|&index| nodes[index].clone()).collect()
}

/// `intersectRect` of the renderer: where the line from the centre to `point` leaves the box.
fn rect_intersect(node: &Node, point: Point) -> Point {
	let (x, y) = (node.x.unwrap_or(0.0), node.y.unwrap_or(0.0));
	let (dx, dy) = (point.x - x, point.y - y);
	let mut w = node.width.unwrap_or(0.0) / 2.0;
	let mut h = node.height.unwrap_or(0.0) / 2.0;
	if dy.abs() * w > dx.abs() * h {
		if dy < 0.0 {
			h = -h;
		}
		return Point { x: x + if dy == 0.0 { 0.0 } else { h * dx / dy }, y: y + h };
	}
	if dx < 0.0 {
		w = -w;
	}
	Point { x: x + w, y: y + if dx == 0.0 { 0.0 } else { w * dy / dx } }
}

/// The visible first or last rail of an edge at a node (`TerminalLane`).
#[derive(Clone, Copy)]
struct TerminalLane {
	edge: usize,
	node: usize,
	at_start: bool,
	vertical: bool,
	coord: f64,
	min: f64,
	max: f64,
	boundary: Point,
	rail_end: Point,
	rect: Rect,
}

fn terminal_lane(
	edge_index: usize,
	edge: &Edge,
	points: &[Point],
	at_start: bool,
	nodes: &[Node],
	map: &NodeMap,
) -> Option<TerminalLane> {
	let points = dedupe(points, EPS);
	if points.len() < 2 {
		return None;
	}
	let id = if at_start { &edge.start } else { &edge.end };
	let &node = map.by_id.get(id)?;
	let rect = node_rect(&nodes[node])?;
	let (endpoint, adjacent) = match at_start {
		true => (points[0], points[1]),
		false => (points[points.len() - 1], points[points.len() - 2]),
	};
	let boundary = rect_intersect(&nodes[node], endpoint);
	let rail_end = if aligned(adjacent, boundary) { adjacent } else { endpoint };
	let lane = |vertical: bool, coord: f64, a: f64, b: f64| TerminalLane {
		edge: edge_index,
		node,
		at_start,
		vertical,
		coord,
		min: a.min(b),
		max: a.max(b),
		boundary,
		rail_end,
		rect,
	};
	if same_x(boundary, rail_end, EPS_LOCAL) {
		return Some(lane(true, boundary.x, boundary.y, rail_end.y));
	}
	if same_y(boundary, rail_end, EPS_LOCAL) {
		return Some(lane(false, boundary.y, boundary.x, rail_end.x));
	}
	None
}

/// `separateSharedRenderedTerminalLanes`: terminal rails of two edges on the same face move
/// apart by whole tracks.
pub(super) fn separate_shared_rendered_terminal_lanes(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	const MIN_FACE_CLEARANCE: f64 = 16.0;
	let overlap =
		|a: &TerminalLane, b: &TerminalLane| (a.max.min(b.max) - a.min.max(b.min)).max(0.0);
	let same_face = |a: &TerminalLane, b: &TerminalLane| {
		if a.node != b.node || a.vertical != b.vertical {
			return false;
		}
		match a.vertical {
			false => {
				((a.boundary.x - a.rect.left).abs() < 1.0
					|| (a.boundary.x - a.rect.right).abs() < 1.0)
					&& same_x(a.boundary, b.boundary, 1.0)
			}
			true => {
				((a.boundary.y - a.rect.top).abs() < 1.0
					|| (a.boundary.y - a.rect.bottom).abs() < 1.0)
					&& same_y(a.boundary, b.boundary, 1.0)
			}
		}
	};
	let exact_conflict = |a: &TerminalLane, b: &TerminalLane| {
		a.node == b.node
			&& a.vertical == b.vertical
			&& overlap(a, b) >= MIN_SHARED
			&& (a.coord - b.coord).abs() < 0.5
	};
	let near_conflict = |a: &TerminalLane, b: &TerminalLane| {
		if a.node != b.node || a.vertical != b.vertical || a.vertical || a.at_start == b.at_start {
			return false;
		}
		let shared = overlap(a, b);
		if shared < MIN_SHARED {
			return false;
		}
		let face_span = a.rect.bottom - a.rect.top;
		if shared < face_span || shared > 2.0 * face_span {
			return false;
		}
		same_face(a, b) && (a.coord - b.coord).abs() < MIN_FACE_CLEARANCE
	};
	let shifted = |edges: &[Edge], lane: &TerminalLane, shift: f64| -> Option<Vec<Point>> {
		let points = dedupe(points_of(&edges[lane.edge]), EPS);
		if points.len() < 2 {
			return None;
		}
		let offset = |point: Point| match lane.vertical {
			true => Point { x: point.x + shift, y: point.y },
			false => Point { x: point.x, y: point.y + shift },
		};
		let (boundary, rail_end) = (offset(lane.boundary), offset(lane.rail_end));
		let rect = &lane.rect;
		let stays_on_face = if (lane.boundary.y - rect.top).abs() < 1.0
			|| (lane.boundary.y - rect.bottom).abs() < 1.0
		{
			same_y(boundary, lane.boundary, EPS_LOCAL)
				&& boundary.x >= rect.left + 1.0
				&& boundary.x <= rect.right - 1.0
		} else if (lane.boundary.x - rect.left).abs() < 1.0
			|| (lane.boundary.x - rect.right).abs() < 1.0
		{
			same_x(boundary, lane.boundary, EPS_LOCAL)
				&& boundary.y >= rect.top + 1.0
				&& boundary.y <= rect.bottom - 1.0
		} else {
			false
		};
		if !stays_on_face {
			return None;
		}
		match lane.at_start {
			true => {
				let skip = if same_point(points[1], lane.rail_end, EPS_LOCAL) { 2 } else { 1 };
				let rest = &points[skip..];
				if rest.first().is_some_and(|&next| !aligned(next, rail_end)) {
					return None;
				}
				let mut candidate = vec![boundary, rail_end];
				candidate.extend_from_slice(rest);
				Some(candidate)
			}
			false => {
				let keep = if same_point(points[points.len() - 2], lane.rail_end, EPS_LOCAL) {
					points.len() - 2
				} else {
					points.len() - 1
				};
				let before = &points[..keep];
				if before.last().is_some_and(|&previous| !aligned(previous, rail_end)) {
					return None;
				}
				let mut candidate = before.to_vec();
				candidate.push(rail_end);
				candidate.push(boundary);
				Some(candidate)
			}
		}
	};
	let straight_collinear = |edges: &[Edge], lane: &TerminalLane| {
		let edge = &edges[lane.edge];
		let points = dedupe(points_of(edge), EPS);
		let [a, b] = points[..] else {
			return false;
		};
		let (Some(start), Some(end)) =
			(node_by_id(nodes, map, &edge.start), node_by_id(nodes, map, &edge.end))
		else {
			return false;
		};
		let (start_x, start_y) = (start.x.unwrap_or(0.0), start.y.unwrap_or(0.0));
		let (end_x, end_y) = (end.x.unwrap_or(0.0), end.y.unwrap_or(0.0));
		(same_y(a, b, EPS_LOCAL) && (start_y - end_y).abs() < 1.0 && (start_x - end_x).abs() > 1.0)
			|| (same_x(a, b, EPS_LOCAL)
				&& (start_x - end_x).abs() < 1.0
				&& (start_y - end_y).abs() > 1.0)
	};
	for _ in 0..8 {
		let lanes: Vec<TerminalLane> = edges
			.iter()
			.enumerate()
			.filter(|(_, edge)| !edge.is_layout_only)
			.flat_map(|(index, edge)| {
				[true, false].into_iter().filter_map(move |at_start| {
					terminal_lane(index, edge, points_of(edge), at_start, nodes, map)
				})
			})
			.collect();
		let mut fixed = false;
		'search: for i in 0..lanes.len() {
			for j in i + 1..lanes.len() {
				let (first, second) = (lanes[i], lanes[j]);
				if first.edge == second.edge
					|| !(exact_conflict(&first, &second) || near_conflict(&first, &second))
				{
					continue;
				}
				let fixing_near = !exact_conflict(&first, &second);
				let mut candidates = [first, second];
				// Straight connectors move last, target ends before source ends.
				let key = |lane: &TerminalLane| (straight_collinear(edges, lane), lane.at_start);
				if key(&candidates[1]) < key(&candidates[0]) {
					candidates.swap(0, 1);
				}
				for lane in candidates {
					for shift in SHIFTS {
						let Some(candidate) = shifted(edges, &lane, shift) else {
							continue;
						};
						let Some(next) = terminal_lane(
							lane.edge,
							&edges[lane.edge],
							&candidate,
							lane.at_start,
							nodes,
							map,
						) else {
							continue;
						};
						let conflicts = lanes.iter().any(|other| {
							other.edge != lane.edge
								&& (exact_conflict(&next, other)
									|| (fixing_near && near_conflict(&next, other)))
						});
						if conflicts {
							continue;
						}
						edges[lane.edge].points = Some(candidate);
						fixed = true;
						break 'search;
					}
				}
			}
		}
		if !fixed {
			return;
		}
	}
}

/// The visible edges with replacement polylines for some of them (`EdgeReplacementMap`).
struct Replacements<'a> {
	edges: &'a [Edge],
	visible: Vec<usize>,
	replaced: Vec<(usize, Vec<Point>)>,
}

impl<'a> Replacements<'a> {
	fn new(edges: &'a [Edge]) -> Self {
		Self {
			edges,
			visible: (0..edges.len()).filter(|&index| !edges[index].is_layout_only).collect(),
			replaced: Vec::new(),
		}
	}

	fn with(edges: &'a [Edge], visible: &[usize], replaced: Vec<(usize, Vec<Point>)>) -> Self {
		Self { edges, visible: visible.to_vec(), replaced }
	}

	fn points(&self, edge: usize) -> Vec<Point> {
		let points = match self.replaced.iter().find(|(index, _)| *index == edge) {
			Some((_, points)) => points.as_slice(),
			None => points_of(&self.edges[edge]),
		};
		dedupe(points, EPS)
	}

	/// `strictCrossingCount`: interior crossings between all pairs of visible edges.
	fn strict_crossings(&self) -> usize {
		let all: Vec<Vec<Segment>> =
			self.visible.iter().map(|&edge| segments(&self.points(edge), EPS)).collect();
		let mut count = 0;
		for i in 0..all.len() {
			for j in i + 1..all.len() {
				for first in &all[i] {
					for second in &all[j] {
						if segments_strictly_cross(first.a, first.b, second.a, second.b, EPS_LOCAL)
						{
							count += 1;
						}
					}
				}
			}
		}
		count
	}

	fn total_bends(&self) -> usize {
		self.visible.iter().map(|&edge| count_bends(&self.points(edge), EPS)).sum()
	}

	/// A segment of the path shares at least 8 px of a rail with another visible edge.
	fn shares_track(&self, edge: usize, path: &[Point]) -> bool {
		let path_segments = segments(path, EPS);
		self.visible.iter().filter(|&&other| other != edge).any(|&other| {
			let other_segments = segments(&self.points(other), EPS);
			path_segments.iter().any(|a| {
				other_segments.iter().any(|b| same_axis_overlap_length(a, b, 0.5) >= MIN_SHARED)
			})
		})
	}
}

fn end_ids(edge: &Edge) -> Vec<&str> {
	[edge.start.as_str(), edge.end.as_str()].into_iter().filter(|id| !id.is_empty()).collect()
}

/// A path that leaves no segment through a foreign node or any label.
fn clear_of_rects(
	edge: &Edge,
	path: &[Point],
	real: &[RectEntry],
	labels: Option<&[RectEntry]>,
) -> bool {
	const BUFFER: f64 = 2.0;
	let ends = end_ids(edge);
	segments(path, EPS).iter().all(|segment| {
		!segment_hits_any_rect(segment.a, segment.b, real, &ends, -BUFFER)
			&& labels.is_none_or(|labels| {
				!segment_hits_any_rect(segment.a, segment.b, labels, &[], -BUFFER)
			})
	})
}

/// `collapseRedundantRectangularDoglegs`: a rectangular jog that returns to its own line is
/// removed when the shortcut is clear.
pub(super) fn collapse_redundant_rectangular_doglegs(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	let (real, labels) = node_rect_entries(&ordered_nodes(nodes, map));
	let is_safe = |edges: &[Edge], edge: usize, candidate: &[Point]| {
		let candidate_segments = segments(candidate, EPS);
		if candidate_segments.len() + 1 != candidate.len()
			|| !clear_of_rects(&edges[edge], candidate, &real, Some(&labels))
		{
			return false;
		}
		edges.iter().enumerate().all(|(index, other)| {
			if index == edge || other.is_layout_only || points_of(other).len() < 2 {
				return true;
			}
			let other_segments = segments(&dedupe(points_of(other), EPS), EPS);
			candidate_segments.iter().all(|c| {
				other_segments.iter().all(|o| {
					same_axis_overlap_length(c, o, 0.5) < MIN_SHARED
						&& !segments_strictly_cross(c.a, c.b, o.a, o.b, EPS_LOCAL)
				})
			})
		})
	};
	let horizontal = |a: Point, b: Point| is_horizontal(a, b, EPS);
	let vertical = |a: Point, b: Point| is_vertical(a, b, EPS);
	let without_dogleg = |points: &[Point], i: usize| -> Option<Vec<Point>> {
		if i + 4 >= points.len() {
			return None;
		}
		let (p0, p1, p2, p3, p4) =
			(points[i], points[i + 1], points[i + 2], points[i + 3], points[i + 4]);
		let terminal_vertical = horizontal(p0, p1)
			&& vertical(p1, p2)
			&& horizontal(p2, p3)
			&& vertical(p3, p4)
			&& same_x(p0, p3, EPS_LOCAL)
			&& same_x(p0, p4, EPS_LOCAL)
			&& same_x(p1, p2, EPS_LOCAL)
			&& (p1.x - p0.x) * (p3.x - p2.x) < 0.0;
		let terminal_horizontal = vertical(p0, p1)
			&& horizontal(p1, p2)
			&& vertical(p2, p3)
			&& horizontal(p3, p4)
			&& same_y(p0, p3, EPS_LOCAL)
			&& same_y(p0, p4, EPS_LOCAL)
			&& same_y(p1, p2, EPS_LOCAL)
			&& (p1.y - p0.y) * (p3.y - p2.y) < 0.0;
		let joined = |skip_to: usize| {
			let mut joined = points[..=i].to_vec();
			joined.extend_from_slice(&points[skip_to..]);
			dedupe(&joined, EPS)
		};
		if terminal_vertical || terminal_horizontal {
			return Some(joined(i + 4));
		}
		if i + 5 >= points.len() {
			return None;
		}
		let p5 = points[i + 5];
		let vertical_dogleg = vertical(p0, p1)
			&& horizontal(p1, p2)
			&& vertical(p2, p3)
			&& horizontal(p3, p4)
			&& vertical(p4, p5)
			&& same_x(p0, p4, EPS_LOCAL)
			&& same_x(p0, p5, EPS_LOCAL)
			&& same_x(p2, p3, EPS_LOCAL)
			&& (p2.x - p1.x) * (p4.x - p3.x) < 0.0;
		let horizontal_dogleg = horizontal(p0, p1)
			&& vertical(p1, p2)
			&& horizontal(p2, p3)
			&& vertical(p3, p4)
			&& horizontal(p4, p5)
			&& same_y(p0, p4, EPS_LOCAL)
			&& same_y(p0, p5, EPS_LOCAL)
			&& same_y(p2, p3, EPS_LOCAL)
			&& (p2.y - p1.y) * (p4.y - p3.y) < 0.0;
		(vertical_dogleg || horizontal_dogleg).then(|| joined(i + 5))
	};
	for _ in 0..8 {
		let mut fixed = false;
		'edges: for edge in 0..edges.len() {
			if edges[edge].is_layout_only {
				continue;
			}
			let points = dedupe(points_of(&edges[edge]), EPS);
			for i in 0..(points.len() + 1).saturating_sub(5) {
				let Some(candidate) = without_dogleg(&points, i) else { continue };
				if !is_safe(edges, edge, &candidate) {
					continue;
				}
				edges[edge].points = Some(candidate);
				fixed = true;
				break 'edges;
			}
		}
		if !fixed {
			return;
		}
	}
}

/// `liftObstacleHuggingSameSideRails`: the middle rail of a Z or U route that runs along a
/// foreign node moves 20 px beyond the blockers.
pub(super) fn lift_obstacle_hugging_same_side_rails(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	const BUFFER: f64 = 2.0;
	const CLEARANCE: f64 = 20.0;
	let (real, labels) = node_rect_entries(&ordered_nodes(nodes, map));
	for _ in 0..8 {
		let current_crossings = Replacements::new(edges).strict_crossings();
		let visible: Vec<usize> =
			(0..edges.len()).filter(|&index| !edges[index].is_layout_only).collect();
		let mut fixed = false;
		'edges: for &edge in &visible {
			let points = dedupe(points_of(&edges[edge]), EPS);
			let all = segments(&points, EPS);
			if all.len() != 3
				|| all[0].horizontal == all[1].horizontal
				|| all[2].horizontal == all[1].horizontal
			{
				continue;
			}
			let rail = all[1];
			let ends = end_ids(&edges[edge]);
			let blockers: Vec<&RectEntry> = real
				.iter()
				.filter(|entry| {
					if ends.contains(&entry.id.as_str()) {
						return false;
					}
					let rect = &entry.rect;
					match rail.horizontal {
						true => {
							overlap_length(rail.a.x, rail.b.x, rect.left, rect.right) >= MIN_SHARED
								&& rail.a.y >= rect.top - BUFFER
								&& rail.a.y <= rect.bottom + BUFFER
						}
						false => {
							overlap_length(rail.a.y, rail.b.y, rect.top, rect.bottom) >= MIN_SHARED
								&& rail.a.x >= rect.left - BUFFER
								&& rail.a.x <= rect.right + BUFFER
						}
					}
				})
				.collect();
			if blockers.is_empty() {
				continue;
			}
			let low = |pick: fn(&Rect) -> f64| {
				blockers.iter().map(|entry| pick(&entry.rect)).fold(f64::INFINITY, f64::min)
					- CLEARANCE
			};
			let high = |pick: fn(&Rect) -> f64| {
				blockers.iter().map(|entry| pick(&entry.rect)).fold(f64::NEG_INFINITY, f64::max)
					+ CLEARANCE
			};
			let coords = match rail.horizontal {
				true => [low(|rect| rect.top), high(|rect| rect.bottom)],
				false => [low(|rect| rect.left), high(|rect| rect.right)],
			};
			for coord in coords {
				let mut candidate = points.clone();
				match (rail.horizontal, rail.vertical) {
					(true, _) => {
						candidate[rail.index].y = coord;
						candidate[rail.index + 1].y = coord;
					}
					(false, true) => {
						candidate[rail.index].x = coord;
						candidate[rail.index + 1].x = coord;
					}
					_ => continue,
				}
				let candidate = simplify(&dedupe(&candidate, EPS));
				if segments(&candidate, EPS).len() + 1 != candidate.len()
					|| !clear_of_rects(&edges[edge], &candidate, &real, Some(&labels))
				{
					continue;
				}
				let replacements =
					Replacements::with(edges, &visible, vec![(edge, candidate.clone())]);
				if replacements.shares_track(edge, &candidate)
					|| replacements.strict_crossings() > current_crossings
				{
					continue;
				}
				edges[edge].points = Some(candidate);
				fixed = true;
				break 'edges;
			}
		}
		if !fixed {
			return;
		}
	}
}

/// A lane title band with a valid rect (`validTitleRect`).
fn valid_title_rect(node: &Node) -> Option<Rect> {
	let rect = node.title_rect?;
	let valid =
		[rect.left, rect.right, rect.top, rect.bottom].iter().all(|value| value.is_finite())
			&& rect.right > rect.left
			&& rect.bottom > rect.top;
	valid.then_some(Rect { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom })
}

fn intersects_title_horizontally(segment: &Segment, rect: &Rect) -> bool {
	segment.horizontal
		&& segment.a.y >= rect.top - EPS_LOCAL
		&& segment.a.y <= rect.bottom + EPS_LOCAL
		&& overlap_length(segment.a.x, segment.b.x, rect.left, rect.right) >= MIN_SHARED
}

fn intersects_title_vertically(segment: &Segment, rect: &Rect) -> bool {
	segment.vertical
		&& segment.a.x >= rect.left - EPS_LOCAL
		&& segment.a.x <= rect.right + EPS_LOCAL
		&& overlap_length(segment.a.y, segment.b.y, rect.top, rect.bottom) >= MIN_SHARED
}

/// `liftTopLaneTitleBandsAboveRails`: top title bands of TB lanes move up above any rail that
/// crosses them; the lanes grow by the same amount.
pub(super) fn lift_top_lane_title_bands_above_rails(
	edges: &[Edge],
	nodes: &mut [Node],
	map: &NodeMap,
	direction: super::Direction,
) {
	const CLEARANCE: f64 = 4.0;
	if matches!(
		direction,
		super::Direction::LeftRight | super::Direction::RightLeft | super::Direction::BottomTop
	) {
		return;
	}
	let lanes: Vec<(usize, Rect)> = map
		.order
		.iter()
		.filter_map(|&index| {
			let node = &nodes[index];
			if !node.is_group || node.parent_id.is_some() {
				return None;
			}
			let rect = valid_title_rect(node)?;
			let height = node.height?;
			if !node.y?.is_finite() || !height.is_finite() || height <= 0.0 {
				return None;
			}
			let (title_width, title_height) = (rect.right - rect.left, rect.bottom - rect.top);
			(title_height > 0.0 && title_width >= title_height).then_some((index, rect))
		})
		.collect();
	if lanes.is_empty() {
		return;
	}
	let mut delta = 0.0f64;
	for edge in edges.iter().filter(|edge| !edge.is_layout_only) {
		for segment in segments(&dedupe(points_of(edge), EPS), EPS) {
			for (_, rect) in &lanes {
				if intersects_title_horizontally(&segment, rect) {
					delta = delta.max(rect.bottom - segment.a.y + CLEARANCE);
				}
			}
		}
	}
	if delta <= EPS_LOCAL {
		return;
	}
	for (index, rect) in lanes {
		let node = &mut nodes[index];
		let (Some(y), Some(height)) = (node.y, node.height) else { continue };
		node.y = Some(y - delta / 2.0);
		node.height = Some(height + delta);
		node.title_rect = Some(super::TitleRect {
			left: rect.left,
			right: rect.right,
			top: rect.top - delta,
			bottom: rect.bottom - delta,
		});
	}
}

/// `shiftLeftLaneTitleBandsLeftOfRails`: the left title bands of LR lanes move left of any rail
/// that crosses them.
pub(super) fn shift_left_lane_title_bands_left_of_rails(
	edges: &[Edge],
	nodes: &mut [Node],
	map: &NodeMap,
	direction: super::Direction,
) {
	const CLEARANCE: f64 = 4.0;
	if direction != super::Direction::LeftRight {
		return;
	}
	let lanes: Vec<(usize, Rect)> = map
		.order
		.iter()
		.filter_map(|&index| {
			let node = &nodes[index];
			if !node.is_group || node.parent_id.is_some() {
				return None;
			}
			let rect = valid_title_rect(node)?;
			let width = node.width?;
			if !node.x?.is_finite() || !width.is_finite() || width <= 0.0 {
				return None;
			}
			let (title_width, title_height) = (rect.right - rect.left, rect.bottom - rect.top);
			(title_width > 0.0 && title_height >= title_width).then_some((index, rect))
		})
		.collect();
	if lanes.is_empty() {
		return;
	}
	let mut delta = 0.0f64;
	for edge in edges.iter().filter(|edge| !edge.is_layout_only) {
		for segment in segments(&dedupe(points_of(edge), EPS), EPS) {
			for (_, rect) in &lanes {
				if intersects_title_vertically(&segment, rect) {
					delta = delta.max(rect.right - segment.a.x + CLEARANCE);
				} else if intersects_title_horizontally(&segment, rect) {
					delta = delta.max(rect.right - segment.a.x.min(segment.b.x) + CLEARANCE);
				}
			}
		}
	}
	if delta <= EPS_LOCAL {
		return;
	}
	for (index, rect) in lanes {
		let node = &mut nodes[index];
		let (Some(x), Some(width)) = (node.x, node.width) else { continue };
		node.x = Some(x - delta / 2.0);
		node.width = Some(width + delta);
		node.title_rect = Some(super::TitleRect {
			left: rect.left - delta,
			right: rect.right - delta,
			top: rect.top,
			bottom: rect.bottom,
		});
	}
}

/// `swapDestinationTerminalTailsToReduceCrossings`: two edges into the same node exchange their
/// final segments when that removes crossings.
pub(super) fn swap_destination_terminal_tails_to_reduce_crossings(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	let (real, _) = node_rect_entries(&ordered_nodes(nodes, map));
	for _ in 0..4 {
		let current = Replacements::new(edges);
		let current_crossings = current.strict_crossings();
		if current_crossings == 0 {
			return;
		}
		let current_bends = current.total_bends();
		let tail = |edge: usize| -> Option<(Point, Point)> {
			let points = current.points(edge);
			if points.len() < 4 {
				return None;
			}
			let (start, terminal) = (points[points.len() - 2], points[points.len() - 1]);
			(is_horizontal(start, terminal, EPS_LOCAL) || is_vertical(start, terminal, EPS_LOCAL))
				.then_some((start, terminal))
		};
		let with_tail =
			|edge: usize, (tail_start, terminal): (Point, Point)| -> Option<Vec<Point>> {
				let points = current.points(edge);
				if points.len() < 3 {
					return None;
				}
				let (start, first_turn) = (points[0], points[1]);
				let connector = if is_horizontal(start, first_turn, EPS_LOCAL) {
					Point { x: first_turn.x, y: tail_start.y }
				} else if is_vertical(start, first_turn, EPS_LOCAL) {
					Point { x: tail_start.x, y: first_turn.y }
				} else {
					return None;
				};
				let candidate =
					simplify(&dedupe(&[start, first_turn, connector, tail_start, terminal], EPS));
				(segments(&candidate, EPS).len() + 1 == candidate.len()).then_some(candidate)
			};
		let mut by_destination: Vec<(&str, Vec<usize>)> = Vec::new();
		for &edge in &current.visible {
			let destination = edges[edge].end.as_str();
			if destination.is_empty()
				|| !map.by_id.contains_key(destination)
				|| current.points(edge).len() < 4
			{
				continue;
			}
			match by_destination.iter_mut().find(|(id, _)| *id == destination) {
				Some((_, list)) => list.push(edge),
				None => by_destination.push((destination, vec![edge])),
			}
		}
		let mut best: Option<Vec<(usize, Vec<Point>)>> = None;
		let (mut best_crossings, mut best_bends) = (current_crossings, current_bends);
		for (_, group) in &by_destination {
			for i in 0..group.len() {
				for j in i + 1..group.len() {
					let (first, second) = (group[i], group[j]);
					let (Some(first_tail), Some(second_tail)) = (tail(first), tail(second)) else {
						continue;
					};
					let (Some(first_candidate), Some(second_candidate)) =
						(with_tail(first, second_tail), with_tail(second, first_tail))
					else {
						continue;
					};
					let trial = Replacements::with(
						edges,
						&current.visible,
						vec![(first, first_candidate.clone()), (second, second_candidate.clone())],
					);
					if !clear_of_rects(&edges[first], &first_candidate, &real, None)
						|| trial.shares_track(first, &first_candidate)
						|| !clear_of_rects(&edges[second], &second_candidate, &real, None)
						|| trial.shares_track(second, &second_candidate)
					{
						continue;
					}
					let (crossings, bends) = (trial.strict_crossings(), trial.total_bends());
					if crossings >= current_crossings
						|| crossings > best_crossings
						|| (crossings == best_crossings && bends >= best_bends)
					{
						continue;
					}
					best = Some(trial.replaced);
					best_crossings = crossings;
					best_bends = bends;
				}
			}
		}
		let Some(replacements) = best else { return };
		for (edge, points) in replacements {
			edges[edge].points = Some(points);
		}
	}
}

/// The identity of a path for dedupes: its points at three decimals, like Mermaid's
/// `toFixed(3)` keys.
fn path_key(points: &[Point]) -> Vec<(i64, i64)> {
	let thousandths = |value: f64| (value * 1000.0).round() as i64;
	points.iter().map(|point| (thousandths(point.x), thousandths(point.y))).collect()
}

fn js_sign(value: f64) -> f64 {
	if value > 0.0 {
		1.0
	} else if value < 0.0 {
		-1.0
	} else {
		0.0
	}
}

/// An interior rail outside the span of both end nodes (`ExternalRail`).
#[derive(Clone)]
struct ExternalRail {
	edge: usize,
	points: Vec<Point>,
	segment: usize,
	vertical: bool,
	side: RectSide,
	coord: f64,
	min: f64,
	max: f64,
}

/// `reassignCrossingExternalRailChannels`: parallel outside rails of crossing edges are given
/// the channel order with the fewest crossings.
pub(super) fn reassign_crossing_external_rail_channels(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	const RAIL_CHANNEL_GAP: f64 = 12.0;
	const MAX_EXHAUSTIVE_COMPONENT: usize = 6;
	let (real, labels) = node_rect_entries(&ordered_nodes(nodes, map));
	let end_rects = |edge: &Edge| -> Option<(Rect, Rect)> {
		let rect = |id: &str| node_by_id(nodes, map, id).and_then(node_rect);
		Some((rect(&edge.start)?, rect(&edge.end)?))
	};
	for _ in 0..4 {
		let current = Replacements::new(edges);
		let current_crossings = current.strict_crossings();
		if current_crossings == 0 {
			return;
		}
		let mut rails: Vec<ExternalRail> = Vec::new();
		for &edge in &current.visible {
			let points = current.points(edge);
			for segment in segments(&points, EPS) {
				if segment.index == 0 || segment.index + 2 >= points.len() {
					continue;
				}
				let Some((source, target)) = end_rects(&edges[edge]) else {
					continue;
				};
				let (side, coord, min, max) = if segment.vertical {
					let coord = segment.a.x;
					let side = if coord < source.left.min(target.left) - EPS_LOCAL {
						RectSide::Left
					} else if coord > source.right.max(target.right) + EPS_LOCAL {
						RectSide::Right
					} else {
						continue;
					};
					(side, coord, segment.a.y.min(segment.b.y), segment.a.y.max(segment.b.y))
				} else if segment.horizontal {
					let coord = segment.a.y;
					let side = if coord < source.top.min(target.top) - EPS_LOCAL {
						RectSide::Top
					} else if coord > source.bottom.max(target.bottom) + EPS_LOCAL {
						RectSide::Bottom
					} else {
						continue;
					};
					(side, coord, segment.a.x.min(segment.b.x), segment.a.x.max(segment.b.x))
				} else {
					continue;
				};
				rails.push(ExternalRail {
					edge,
					points: points.clone(),
					segment: segment.index,
					vertical: segment.vertical,
					side,
					coord,
					min,
					max,
				});
			}
		}
		let interact = |a: &ExternalRail, b: &ExternalRail| {
			a.edge != b.edge
				&& a.vertical == b.vertical
				&& a.side == b.side
				&& overlap_length(a.min, a.max, b.min, b.max) >= MIN_SHARED
		};
		let mut components: Vec<Vec<usize>> = Vec::new();
		let mut seen = vec![false; rails.len()];
		for start in 0..rails.len() {
			if seen[start] {
				continue;
			}
			seen[start] = true;
			let (mut stack, mut component) = (vec![start], Vec::new());
			while let Some(rail) = stack.pop() {
				component.push(rail);
				for next in 0..rails.len() {
					if !seen[next] && interact(&rails[rail], &rails[next]) {
						seen[next] = true;
						stack.push(next);
					}
				}
			}
			if component.len() > 1 {
				components.push(component);
			}
		}
		let mut best: Option<Vec<(usize, Vec<Point>)>> = None;
		let mut best_crossings = current_crossings;
		let mut best_bends = current.total_bends();
		let mut best_displacement = f64::INFINITY;
		for component in &components {
			let component: Vec<&ExternalRail> =
				component.iter().map(|&rail| &rails[rail]).collect();
			let present: Vec<f64> = component.iter().map(|rail| rail.coord).collect();
			let mut coords: Vec<f64> = Vec::new();
			for rail in &component {
				if !coords.iter().any(|coord| (coord - rail.coord).abs() < EPS_LOCAL) {
					coords.push(rail.coord);
				}
			}
			while coords.len() < component.len() {
				let missing = (component.len() - coords.len()) as f64;
				coords.push(match component[0].side {
					RectSide::Left | RectSide::Top => {
						coords.iter().copied().fold(f64::INFINITY, f64::min)
							- RAIL_CHANNEL_GAP * missing
					}
					_ => {
						coords.iter().copied().fold(f64::NEG_INFINITY, f64::max)
							+ RAIL_CHANNEL_GAP * missing
					}
				});
			}
			let mut assignments: Vec<Vec<f64>> = Vec::new();
			if component.len() <= MAX_EXHAUSTIVE_COMPONENT {
				permutations(&coords, component.len(), &mut |next: &[f64]| {
					if next
						.iter()
						.zip(&present)
						.any(|(coord, now)| (coord - now).abs() >= EPS_LOCAL)
					{
						assignments.push(next.to_vec());
					}
				});
			} else {
				for i in 0..present.len() {
					for j in i + 1..present.len() {
						let mut assignment = present.clone();
						assignment.swap(i, j);
						assignments.push(assignment);
					}
				}
			}
			for assignment in assignments {
				let mut drafts: Vec<(usize, Vec<Point>)> = Vec::new();
				for (rail, &coord) in component.iter().zip(&assignment) {
					let index = match drafts.iter().position(|(edge, _)| *edge == rail.edge) {
						Some(index) => index,
						None => {
							drafts.push((rail.edge, rail.points.clone()));
							drafts.len() - 1
						}
					};
					let points = &mut drafts[index].1;
					match rail.vertical {
						true => {
							points[rail.segment].x = coord;
							points[rail.segment + 1].x = coord;
						}
						false => {
							points[rail.segment].y = coord;
							points[rail.segment + 1].y = coord;
						}
					}
				}
				let mut replaced = Vec::with_capacity(drafts.len());
				let mut valid = true;
				for (edge, points) in drafts {
					let simplified = simplify(&dedupe(&points, EPS));
					if segments(&simplified, EPS).len() + 1 != simplified.len() {
						valid = false;
						break;
					}
					replaced.push((edge, simplified));
				}
				if !valid
					|| !replaced.iter().all(|(edge, points)| {
						clear_of_rects(&edges[*edge], points, &real, Some(&labels))
					}) {
					continue;
				}
				let trial = Replacements::with(edges, &current.visible, replaced);
				let changed = |edge: usize| trial.replaced.iter().any(|(index, _)| *index == edge);
				let mut shares = false;
				'pairs: for i in 0..trial.visible.len() {
					let first = trial.visible[i];
					let first_segments = segments(&trial.points(first), EPS);
					for &second in &trial.visible[i + 1..] {
						if !changed(first) && !changed(second) {
							continue;
						}
						let second_segments = segments(&trial.points(second), EPS);
						if first_segments.iter().any(|a| {
							second_segments
								.iter()
								.any(|b| same_axis_overlap_length(a, b, 0.5) >= MIN_SHARED)
						}) {
							shares = true;
							break 'pairs;
						}
					}
				}
				if shares {
					continue;
				}
				let crossings = trial.strict_crossings();
				if crossings >= current_crossings {
					continue;
				}
				let bends = trial.total_bends();
				let displacement: f64 = component
					.iter()
					.zip(&assignment)
					.map(|(rail, coord)| (coord - rail.coord).abs())
					.sum();
				if crossings > best_crossings
					|| (crossings == best_crossings
						&& (bends > best_bends
							|| (bends == best_bends && displacement >= best_displacement)))
				{
					continue;
				}
				best = Some(trial.replaced);
				best_crossings = crossings;
				best_bends = bends;
				best_displacement = displacement;
			}
		}
		let Some(replacements) = best else { return };
		for (edge, points) in replacements {
			edges[edge].points = Some(points);
		}
	}
}

/// All arrangements of `count` distinct values of `values`, in Mermaid's recursion order.
fn permutations(values: &[f64], count: usize, visit: &mut dyn FnMut(&[f64])) {
	fn recurse(
		values: &[f64],
		count: usize,
		used: &mut [bool],
		next: &mut Vec<f64>,
		visit: &mut dyn FnMut(&[f64]),
	) {
		if next.len() == count {
			visit(next);
			return;
		}
		for (index, &value) in values.iter().enumerate() {
			if used[index] {
				continue;
			}
			used[index] = true;
			next.push(value);
			recurse(values, count, used, next, visit);
			next.pop();
			used[index] = false;
		}
	}
	recurse(values, count, &mut vec![false; values.len()], &mut Vec::new(), visit);
}

/// `shortcutRedundantOrthogonalJogs`: a U-turn of three segments is cut short through a
/// corner when that saves bends or length without new crossings.
pub(super) fn shortcut_redundant_orthogonal_jogs(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	let (real, labels) = node_rect_entries(&ordered_nodes(nodes, map));
	let path_length = |points: &[Point]| {
		segments(points, EPS).iter().map(|s| (s.a.x - s.b.x).hypot(s.a.y - s.b.y)).sum::<f64>()
	};
	let along_border = |segment: &Segment, rect: &Rect| {
		if segment.horizontal {
			let y = segment.a.y;
			return ((y - rect.top).abs() < 1.0 || (y - rect.bottom).abs() < 1.0)
				&& overlap_length(segment.a.x, segment.b.x, rect.left, rect.right) >= MIN_SHARED;
		}
		if segment.vertical {
			let x = segment.a.x;
			return ((x - rect.left).abs() < 1.0 || (x - rect.right).abs() < 1.0)
				&& overlap_length(segment.a.y, segment.b.y, rect.top, rect.bottom) >= MIN_SHARED;
		}
		false
	};
	let candidates_at = |points: &[Point], index: usize| -> Vec<Vec<Point>> {
		if index + 3 >= points.len() {
			return Vec::new();
		}
		let (p0, p1, p2, p3) =
			(points[index], points[index + 1], points[index + 2], points[index + 3]);
		let hvh = is_horizontal(p0, p1, EPS_LOCAL)
			&& is_vertical(p1, p2, EPS_LOCAL)
			&& is_horizontal(p2, p3, EPS_LOCAL);
		let vhv = is_vertical(p0, p1, EPS_LOCAL)
			&& is_horizontal(p1, p2, EPS_LOCAL)
			&& is_vertical(p2, p3, EPS_LOCAL);
		if !hvh && !vhv {
			return Vec::new();
		}
		let oppose = match hvh {
			true => js_sign(p1.x - p0.x) != js_sign(p3.x - p2.x),
			false => js_sign(p1.y - p0.y) != js_sign(p3.y - p2.y),
		};
		if !oppose {
			return Vec::new();
		}
		let corners = match same_x(p0, p3, EPS_LOCAL) || same_y(p0, p3, EPS_LOCAL) {
			true => vec![None],
			false => vec![Some(Point { x: p0.x, y: p3.y }), Some(Point { x: p3.x, y: p0.y })],
		};
		let mut seen = std::collections::HashSet::new();
		corners
			.into_iter()
			.map(|corner| {
				let mut candidate = points[..=index].to_vec();
				candidate.extend(corner);
				candidate.extend_from_slice(&points[index + 3..]);
				simplify(&dedupe(&candidate, EPS))
			})
			.filter(|candidate| {
				segments(candidate, EPS).len() + 1 == candidate.len()
					&& candidate.iter().any(|&point| same_point(point, p3, EPS_LOCAL))
					&& seen.insert(path_key(candidate))
			})
			.collect()
	};
	for _ in 0..8 {
		let current = Replacements::new(edges);
		let current_crossings = current.strict_crossings();
		let mut best: Option<(usize, Vec<Point>)> = None;
		let (mut best_crossings, mut best_bends, mut best_length) =
			(current_crossings, usize::MAX, f64::INFINITY);
		for &edge in &current.visible {
			let points = current.points(edge);
			let (bends, length) = (count_bends(&points, EPS_LOCAL), path_length(&points));
			let rects: Vec<Rect> = end_ids(&edges[edge])
				.iter()
				.filter_map(|id| node_by_id(nodes, map, id).and_then(node_rect))
				.collect();
			for index in 0..(points.len() + 1).saturating_sub(4) {
				for candidate in candidates_at(&points, index) {
					let (candidate_bends, candidate_length) =
						(count_bends(&candidate, EPS_LOCAL), path_length(&candidate));
					let improves = candidate_bends < bends
						|| (candidate_bends == bends && candidate_length < length - EPS_LOCAL);
					if !improves || !clear_of_rects(&edges[edge], &candidate, &real, Some(&labels))
					{
						continue;
					}
					if segments(&candidate, EPS)
						.iter()
						.any(|segment| rects.iter().any(|rect| along_border(segment, rect)))
					{
						continue;
					}
					let trial = Replacements::with(
						edges,
						&current.visible,
						vec![(edge, candidate.clone())],
					);
					if trial.shares_track(edge, &candidate) {
						continue;
					}
					let crossings = trial.strict_crossings();
					if crossings > current_crossings {
						continue;
					}
					if crossings > best_crossings
						|| (crossings == best_crossings
							&& (candidate_bends > best_bends
								|| (candidate_bends == best_bends
									&& candidate_length >= best_length)))
					{
						continue;
					}
					best = Some((edge, candidate));
					best_crossings = crossings;
					best_bends = candidate_bends;
					best_length = candidate_length;
				}
			}
		}
		let Some((edge, points)) = best else { return };
		edges[edge].points = Some(points);
	}
}

/// A pair of crossing edges (`CrossingPair`).
#[derive(Clone, Copy)]
struct CrossingPair {
	first: usize,
	second: usize,
	count: usize,
}

/// `CrossingSnapshot`.
struct Snapshot {
	count: usize,
	pairs: Vec<CrossingPair>,
	/// Crossing edges by their position among the visible edges.
	edges: Vec<usize>,
}

/// A rerouting candidate of one edge in a pair search (`PairCandidate`).
struct PairCandidate {
	path: Vec<Point>,
	segments: Vec<Segment>,
	/// Crossings with each base edge and their sum.
	crossings: Vec<usize>,
	total_crossings: usize,
	shared_track_conflicts: Vec<usize>,
	total_bends: usize,
	length: f64,
}

fn crossings_between(first: &[Segment], second: &[Segment]) -> usize {
	first
		.iter()
		.map(|a| {
			second.iter().filter(|b| segments_strictly_cross(a.a, a.b, b.a, b.b, EPS_LOCAL)).count()
		})
		.sum()
}

/// The extent of segments, to skip far apart edges in the crossing counts.
#[derive(Clone, Copy)]
struct Extent {
	min: Point,
	max: Point,
}

impl Extent {
	fn of(segments: &[Segment]) -> Extent {
		let mut extent = Extent {
			min: Point { x: f64::INFINITY, y: f64::INFINITY },
			max: Point { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY },
		};
		for point in segments.iter().flat_map(|segment| [segment.a, segment.b]) {
			extent.min = Point { x: extent.min.x.min(point.x), y: extent.min.y.min(point.y) };
			extent.max = Point { x: extent.max.x.max(point.x), y: extent.max.y.max(point.y) };
		}
		extent
	}

	/// Whether the extents come closer than `margin` (crossings need 0, shared tracks 0.5).
	fn near(&self, other: &Extent, margin: f64) -> bool {
		self.min.x <= other.max.x + margin
			&& other.min.x <= self.max.x + margin
			&& self.min.y <= other.max.y + margin
			&& other.min.y <= self.max.y + margin
	}
}

/// The base edges of the crossing resolution with their extents.
struct BaseEdges<'a> {
	segments: &'a [Vec<Segment>],
	extents: Vec<Extent>,
}

impl BaseEdges<'_> {
	/// Crossings of the candidate with every base edge except `own`, and their sum.
	fn crossings(&self, own: usize, candidate: &[Segment]) -> (Vec<usize>, usize) {
		let extent = Extent::of(candidate);
		let mut total = 0;
		let per_edge = (0..self.segments.len())
			.map(|other| {
				let count = match other != own && extent.near(&self.extents[other], 0.0) {
					true => crossings_between(candidate, &self.segments[other]),
					false => 0,
				};
				total += count;
				count
			})
			.collect();
		(per_edge, total)
	}

	/// The base edges (except `own`) the candidate shares 8 px of a track with.
	fn shared_tracks(&self, own: usize, candidate: &[Segment]) -> Vec<usize> {
		let extent = Extent::of(candidate);
		(0..self.segments.len())
			.filter(|&other| other != own && extent.near(&self.extents[other], 0.5))
			.filter(|&other| {
				candidate.iter().any(|a| {
					self.segments[other]
						.iter()
						.any(|b| same_axis_overlap_length(a, b, 0.5) >= MIN_SHARED)
				})
			})
			.collect()
	}
}

/// `pathLength` of the crossing resolution: Manhattan length over all points.
fn manhattan_length(points: &[Point]) -> f64 {
	points
		.windows(2)
		.map(|pair| (pair[1].x - pair[0].x).abs() + (pair[1].y - pair[0].y).abs())
		.sum()
}

/// `resolveRenderedOrthogonalCrossings`: crossing edges are rerouted through port pairs and
/// outer channels, one edge at a time or, failing that, as a pair.
pub(super) fn resolve_rendered_orthogonal_crossings(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	const ANCHOR: f64 = 20.0;
	const EXTRA_CHANNEL_COUNT: usize = 2;
	const MAX_PAIR_CANDIDATES_PER_EDGE: usize = 48;
	const SIDES: [RectSide; 4] = [RectSide::Top, RectSide::Bottom, RectSide::Left, RectSide::Right];
	let real_nodes: Vec<super::geometry::NodeInfo> = map
		.order
		.iter()
		.map(|&index| &nodes[index])
		.filter(|node| !node.is_group && !node.is_edge_label)
		.filter_map(|node| {
			Some(super::geometry::NodeInfo {
				id: node.id.clone(),
				cx: node.x.unwrap_or(0.0),
				cy: node.y.unwrap_or(0.0),
				rect: node_rect(node)?,
			})
		})
		.collect();
	if real_nodes.is_empty() {
		return;
	}
	let info_by_id: std::collections::HashMap<&str, &super::geometry::NodeInfo> =
		real_nodes.iter().map(|node| (node.id.as_str(), node)).collect();
	let real_rects: Vec<RectEntry> =
		real_nodes.iter().map(|node| RectEntry { id: node.id.clone(), rect: node.rect }).collect();
	let extreme = |pick: fn(&Rect) -> f64, minimum: bool| {
		let values = real_nodes.iter().map(|node| pick(&node.rect));
		match minimum {
			true => values.fold(f64::INFINITY, f64::min) - ANCHOR,
			false => values.fold(f64::NEG_INFINITY, f64::max) + ANCHOR,
		}
	};
	let outside = |side: RectSide| match side {
		RectSide::Top => extreme(|rect| rect.top, true),
		RectSide::Bottom => extreme(|rect| rect.bottom, false),
		RectSide::Left => extreme(|rect| rect.left, true),
		RectSide::Right => extreme(|rect| rect.right, false),
	};
	let outward =
		|side: RectSide| if matches!(side, RectSide::Left | RectSide::Top) { -1.0 } else { 1.0 };
	let tracks_for = |side: RectSide| -> Vec<f64> {
		(0..=EXTRA_CHANNEL_COUNT)
			.map(|channel| outside(side) + outward(side) * ANCHOR * channel as f64)
			.collect()
	};
	let both = |a: RectSide, b: RectSide| -> Vec<f64> {
		let mut tracks = tracks_for(a);
		tracks.extend(tracks_for(b));
		tracks
	};
	let push = |candidates: &mut Vec<Vec<Point>>, points: &[Point]| {
		let candidate = simplify(&dedupe(points, EPS));
		if segments(&candidate, EPS).len() + 1 == candidate.len() {
			candidates.push(candidate);
		}
	};
	let path_hits_node = |edge: &Edge, path: &[Point]| {
		let ends = end_ids(edge);
		segments(path, EPS)
			.iter()
			.any(|segment| segment_hits_any_rect(segment.a, segment.b, &real_rects, &ends, -2.0))
	};
	let candidates_for_sides = |src: Point,
	                            src_side: RectSide,
	                            dst: Point,
	                            dst_side: RectSide|
	 -> Vec<Vec<Point>> {
		let mut candidates = Vec::new();
		if let Some(base) = orthogonal_port_path(src, src_side, dst, dst_side, ANCHOR, EPS_LOCAL) {
			push(&mut candidates, &base);
		}
		if src_side == dst_side {
			let local = match src_side {
				RectSide::Left => src.x.min(dst.x) - ANCHOR,
				RectSide::Right => src.x.max(dst.x) + ANCHOR,
				RectSide::Top => src.y.min(dst.y) - ANCHOR,
				RectSide::Bottom => src.y.max(dst.y) + ANCHOR,
			};
			for seed in [local, outside(src_side)] {
				for channel in 0..=EXTRA_CHANNEL_COUNT {
					push(
						&mut candidates,
						&same_side_track_path(
							src,
							src_side,
							dst,
							seed + outward(src_side) * ANCHOR * channel as f64,
						),
					);
				}
			}
		}
		let (src_horizontal, dst_horizontal) = (src_side.is_horizontal(), dst_side.is_horizontal());
		match (src_horizontal, dst_horizontal) {
			(true, false) => {
				for x in tracks_for(src_side) {
					for y in tracks_for(dst_side) {
						push(
							&mut candidates,
							&[
								src,
								Point { x, y: src.y },
								Point { x, y },
								Point { x: dst.x, y },
								dst,
							],
						);
					}
				}
			}
			(false, true) => {
				for y in tracks_for(src_side) {
					for x in tracks_for(dst_side) {
						push(
							&mut candidates,
							&[
								src,
								Point { x: src.x, y },
								Point { x, y },
								Point { x, y: dst.y },
								dst,
							],
						);
					}
				}
			}
			(true, true) => {
				let y_tracks = both(RectSide::Top, RectSide::Bottom);
				for src_track in tracks_for(src_side) {
					for dst_track in tracks_for(dst_side) {
						for &y in &y_tracks {
							push(
								&mut candidates,
								&[
									src,
									Point { x: src_track, y: src.y },
									Point { x: src_track, y },
									Point { x: dst_track, y },
									Point { x: dst_track, y: dst.y },
									dst,
								],
							);
						}
					}
				}
			}
			(false, false) => {
				let x_tracks = both(RectSide::Left, RectSide::Right);
				for src_track in tracks_for(src_side) {
					for dst_track in tracks_for(dst_side) {
						for &x in &x_tracks {
							push(
								&mut candidates,
								&[
									src,
									Point { x: src.x, y: src_track },
									Point { x, y: src_track },
									Point { x, y: dst_track },
									Point { x: dst.x, y: dst_track },
									dst,
								],
							);
						}
					}
				}
			}
		}
		let mut seen = std::collections::HashSet::new();
		candidates
			.into_iter()
			.map(|candidate| dedupe(&candidate, EPS))
			.filter(|candidate| candidate.len() >= 2 && seen.insert(path_key(candidate)))
			.collect()
	};
	let terminal_preserving = |edge: &Edge| -> Vec<Vec<Point>> {
		let Some(destination) =
			info_by_id.get(edge.end.as_str()).filter(|_| !edge.start.is_empty())
		else {
			return Vec::new();
		};
		let points = dedupe(points_of(edge), EPS);
		if points.len() < 4 {
			return Vec::new();
		}
		let (first, departure) = (points[0], points[1]);
		let mut candidates = Vec::new();
		let external_x = both(RectSide::Left, RectSide::Right);
		let external_y = both(RectSide::Top, RectSide::Bottom);
		if is_vertical(first, departure, EPS_LOCAL) {
			for side in SIDES {
				let dst = super::geometry::port_for_side(destination, side);
				let target_y =
					if side.is_horizontal() { external_y.clone() } else { tracks_for(side) };
				for &track in &external_x {
					push(
						&mut candidates,
						&[
							first,
							departure,
							Point { x: track, y: departure.y },
							Point { x: track, y: dst.y },
							dst,
						],
					);
					for &target in &target_y {
						push(
							&mut candidates,
							&[
								first,
								departure,
								Point { x: track, y: departure.y },
								Point { x: track, y: target },
								Point { x: dst.x, y: target },
								dst,
							],
						);
					}
				}
			}
		} else if is_horizontal(first, departure, EPS_LOCAL) {
			for side in SIDES {
				let dst = super::geometry::port_for_side(destination, side);
				let target_x =
					if side.is_horizontal() { tracks_for(side) } else { external_x.clone() };
				for &track in &external_y {
					push(
						&mut candidates,
						&[
							first,
							departure,
							Point { x: departure.x, y: track },
							Point { x: dst.x, y: track },
							dst,
						],
					);
					for &target in &target_x {
						push(
							&mut candidates,
							&[
								first,
								departure,
								Point { x: departure.x, y: track },
								Point { x: target, y: track },
								Point { x: target, y: dst.y },
								dst,
							],
						);
					}
				}
			}
		}
		candidates
	};
	let candidate_paths = |edge: &Edge| -> Vec<Vec<Point>> {
		let (Some(source), Some(target)) =
			(info_by_id.get(edge.start.as_str()), info_by_id.get(edge.end.as_str()))
		else {
			return Vec::new();
		};
		let mut candidates = Vec::new();
		for src_side in SIDES {
			let src = super::geometry::port_for_side(source, src_side);
			for dst_side in SIDES {
				candidates.extend(candidates_for_sides(
					src,
					src_side,
					super::geometry::port_for_side(target, dst_side),
					dst_side,
				));
			}
		}
		candidates.extend(terminal_preserving(edge));
		candidates
	};
	for _ in 0..4 {
		let visible: Vec<usize> =
			(0..edges.len()).filter(|&index| !edges[index].is_layout_only).collect();
		let base_segments: Vec<Vec<Segment>> = visible
			.iter()
			.map(|&edge| segments(&dedupe(points_of(&edges[edge]), EPS), EPS))
			.collect();
		let snapshot = {
			let mut pairs = Vec::new();
			let mut crossing = vec![false; visible.len()];
			let mut count = 0;
			for i in 0..visible.len() {
				for j in i + 1..visible.len() {
					let pair_count = crossings_between(&base_segments[i], &base_segments[j]);
					if pair_count > 0 {
						count += pair_count;
						pairs.push(CrossingPair { first: i, second: j, count: pair_count });
						crossing[i] = true;
						crossing[j] = true;
					}
				}
			}
			Snapshot {
				count,
				pairs,
				edges: (0..visible.len()).filter(|&index| crossing[index]).collect(),
			}
		};
		if snapshot.count == 0 {
			return;
		}
		// Crossings with one edge replaced: the unchanged pairs plus the new ones.
		let crossings_of_edge = |position: usize| -> usize {
			snapshot
				.pairs
				.iter()
				.filter(|pair| pair.first == position || pair.second == position)
				.map(|pair| pair.count)
				.sum()
		};
		let base = BaseEdges {
			segments: &base_segments,
			extents: base_segments.iter().map(|segments| Extent::of(segments)).collect(),
		};
		let with_single = |position: usize, candidate_segments: &[Segment]| -> usize {
			snapshot.count - crossings_of_edge(position)
				+ base.crossings(position, candidate_segments).1
		};
		let shares_track_with_others = |position: usize, candidate_segments: &[Segment]| {
			!base.shared_tracks(position, candidate_segments).is_empty()
		};
		let mut best: Option<(usize, Vec<Point>)> = None;
		let (mut best_crossings, mut best_bends) = (snapshot.count, usize::MAX);
		for &position in &snapshot.edges {
			let edge = &edges[visible[position]];
			let current_bends = count_bends(&dedupe(points_of(edge), EPS), EPS_LOCAL);
			for candidate in candidate_paths(edge) {
				if path_hits_node(edge, &candidate) {
					continue;
				}
				let candidate_segments = segments(&dedupe(&candidate, EPS), EPS);
				if shares_track_with_others(position, &candidate_segments) {
					continue;
				}
				let crossings = with_single(position, &candidate_segments);
				let bends = count_bends(&candidate, EPS_LOCAL);
				let improves = crossings < snapshot.count
					|| (crossings == snapshot.count && bends < current_bends);
				if !improves
					|| crossings > best_crossings
					|| (crossings == best_crossings && bends >= best_bends)
				{
					continue;
				}
				best = Some((visible[position], candidate));
				best_crossings = crossings;
				best_bends = bends;
			}
		}
		if let Some((edge, points)) = best {
			edges[edge].points = Some(points);
			continue;
		}
		let Some(replacements) = best_paired_replacement(
			edges,
			&visible,
			&base,
			&snapshot,
			&candidate_paths,
			&path_hits_node,
			MAX_PAIR_CANDIDATES_PER_EDGE,
		) else {
			return;
		};
		for (edge, points) in replacements {
			edges[edge].points = Some(points);
		}
	}
}

/// A candidate path, its segments, crossings, bends (twice), length, crossings per base edge
/// and their sum.
type ScoredCandidate = (Vec<Point>, Vec<Segment>, usize, usize, usize, f64, Vec<usize>, usize);

/// `bestPairedReplacement`: two edges of a crossing component (or sharing its end nodes)
/// rerouted together.
fn best_paired_replacement(
	edges: &[Edge],
	visible: &[usize],
	base: &BaseEdges,
	snapshot: &Snapshot,
	candidate_paths: &dyn Fn(&Edge) -> Vec<Vec<Point>>,
	path_hits_node: &dyn Fn(&Edge, &[Point]) -> bool,
	max_candidates: usize,
) -> Option<Vec<(usize, Vec<Point>)>> {
	let points = |position: usize| dedupe(points_of(&edges[visible[position]]), EPS);
	let current_bends: usize =
		(0..visible.len()).map(|position| count_bends(&points(position), EPS)).sum();
	let current_length: f64 =
		(0..visible.len()).map(|position| manhattan_length(&points(position))).sum();
	let mut crossings_by_edge = vec![0usize; visible.len()];
	for pair in &snapshot.pairs {
		crossings_by_edge[pair.first] += pair.count;
		crossings_by_edge[pair.second] += pair.count;
	}
	// `crossingComponents`, then `pairSearchGroups` with the edges sharing their end nodes.
	let mut neighbours: Vec<Vec<usize>> = vec![Vec::new(); visible.len()];
	for pair in &snapshot.pairs {
		if !neighbours[pair.first].contains(&pair.second) {
			neighbours[pair.first].push(pair.second);
		}
		if !neighbours[pair.second].contains(&pair.first) {
			neighbours[pair.second].push(pair.first);
		}
	}
	let mut groups: Vec<Vec<usize>> = Vec::new();
	let mut seen = vec![false; visible.len()];
	for &start in &snapshot.edges {
		if seen[start] {
			continue;
		}
		seen[start] = true;
		let (mut stack, mut component) = (vec![start], Vec::new());
		while let Some(current) = stack.pop() {
			component.push(current);
			for &next in &neighbours[current] {
				if !seen[next] {
					seen[next] = true;
					stack.push(next);
				}
			}
		}
		component.sort_unstable();
		if component.len() <= 1 {
			continue;
		}
		let ends: Vec<&str> =
			component.iter().flat_map(|&position| end_ids(&edges[visible[position]])).collect();
		let mut group = component.clone();
		for position in 0..visible.len() {
			if !component.contains(&position)
				&& end_ids(&edges[visible[position]]).iter().any(|id| ends.contains(id))
			{
				group.push(position);
			}
		}
		group.sort_unstable();
		groups.push(group);
	}
	let mut options: Vec<Option<Vec<PairCandidate>>> = (0..visible.len()).map(|_| None).collect();
	let mut computed = vec![false; visible.len()];
	for group in &groups {
		for &position in group {
			if computed[position] {
				continue;
			}
			computed[position] = true;
			let edge = &edges[visible[position]];
			let mut seen_paths = std::collections::HashSet::new();
			let mut scored: Vec<ScoredCandidate> = candidate_paths(edge)
				.into_iter()
				.map(|candidate| simplify(&dedupe(&candidate, EPS)))
				.filter(|candidate| {
					!path_hits_node(edge, candidate)
						&& candidate.len() >= 2
						&& seen_paths.insert(path_key(candidate))
				})
				.map(|candidate| {
					let candidate_segments = segments(&candidate, EPS);
					let (per_edge, replaced) = base.crossings(position, &candidate_segments);
					let crossings = snapshot.count - crossings_by_edge[position] + replaced;
					let bends = count_bends(&candidate, EPS_LOCAL);
					let total_bends = count_bends(&candidate, EPS);
					let length = manhattan_length(&candidate);
					(
						candidate,
						candidate_segments,
						crossings,
						bends,
						total_bends,
						length,
						per_edge,
						replaced,
					)
				})
				.filter(|scored| scored.2 <= snapshot.count)
				.collect();
			npp::sort::sort_by(&mut scored[..], |a, b| {
				a.2.cmp(&b.2)
					.then(a.3.cmp(&b.3))
					.then(a.5.partial_cmp(&b.5).unwrap_or(std::cmp::Ordering::Equal))
			});
			scored.truncate(max_candidates);
			let candidates: Vec<PairCandidate> = scored
				.into_iter()
				.map(
					|(
						path,
						candidate_segments,
						_,
						_,
						total_bends,
						length,
						crossings,
						total_crossings,
					)| {
						let shared_track_conflicts =
							base.shared_tracks(position, &candidate_segments);
						PairCandidate {
							path,
							segments: candidate_segments,
							crossings,
							total_crossings,
							shared_track_conflicts,
							total_bends,
							length,
						}
					},
				)
				.collect();
			if !candidates.is_empty() {
				options[position] = Some(candidates);
			}
		}
	}
	let base_bends: Vec<usize> =
		(0..visible.len()).map(|position| count_bends(&points(position), EPS)).collect();
	let base_lengths: Vec<f64> =
		(0..visible.len()).map(|position| manhattan_length(&points(position))).collect();
	let crossing: Vec<bool> =
		(0..visible.len()).map(|position| snapshot.edges.contains(&position)).collect();
	let mut best: Option<Vec<(usize, Vec<Point>)>> = None;
	let (mut best_crossings, mut best_bends, mut best_length) =
		(snapshot.count, current_bends, current_length);
	for group in &groups {
		let with_options: Vec<usize> =
			group.iter().copied().filter(|&position| options[position].is_some()).collect();
		for i in 0..with_options.len() {
			for j in i + 1..with_options.len() {
				let (first, second) = (with_options[i], with_options[j]);
				if !crossing[first] && !crossing[second] {
					continue;
				}
				let affected: usize = snapshot
					.pairs
					.iter()
					.filter(|pair| {
						[pair.first, pair.second].iter().any(|&end| end == first || end == second)
					})
					.map(|pair| pair.count)
					.sum();
				for first_candidate in options[first].iter().flatten() {
					for second_candidate in options[second].iter().flatten() {
						let compatible = first_candidate
							.shared_track_conflicts
							.iter()
							.all(|&conflict| conflict == second)
							&& second_candidate
								.shared_track_conflicts
								.iter()
								.all(|&conflict| conflict == first)
							&& !first_candidate.segments.iter().any(|a| {
								second_candidate
									.segments
									.iter()
									.any(|b| same_axis_overlap_length(a, b, 0.5) >= MIN_SHARED)
							});
						if !compatible {
							continue;
						}
						// The crossings with all other edges, without the replaced partner.
						let replaced = crossings_between(
							&first_candidate.segments,
							&second_candidate.segments,
						) + (first_candidate.total_crossings
							- first_candidate.crossings[second])
							+ (second_candidate.total_crossings
								- second_candidate.crossings[first]);
						let crossings = snapshot.count - affected + replaced;
						if crossings >= snapshot.count {
							continue;
						}
						let bends = current_bends - base_bends[first] - base_bends[second]
							+ first_candidate.total_bends
							+ second_candidate.total_bends;
						let length = current_length - base_lengths[first] - base_lengths[second]
							+ first_candidate.length
							+ second_candidate.length;
						let better = crossings < best_crossings
							|| (crossings == best_crossings
								&& (bends < best_bends
									|| (bends == best_bends && length < best_length)));
						if better {
							best = Some(vec![
								(visible[first], first_candidate.path.clone()),
								(visible[second], second_candidate.path.clone()),
							]);
							best_crossings = crossings;
							best_bends = bends;
							best_length = length;
						}
					}
				}
			}
		}
	}
	best
}
