//! Edge ends on the node borders (`direction/endpointClip.ts`): clipping at the boundary,
//! clearance from the corners, and the duplicated end points the renderer expects.

use super::geometry::{
	EPS, Rect, dedupe, node_rect, orthogonalize, same_point, same_x, same_y, simplify,
};
use super::{Edge, Node, NodeMap, Point};

const INSIDE_EPS: f64 = 0.5;
const CORNER_CLEARANCE: f64 = 4.0;

#[derive(Clone, Copy, PartialEq)]
enum BorderSide {
	Top,
	Bottom,
	Left,
	Right,
}

impl BorderSide {
	fn is_horizontal(self) -> bool {
		matches!(self, Self::Left | Self::Right)
	}
}

/// The boxes of an edge's ends (`endpointContextFor`).
pub(super) fn end_rects(
	edge: &Edge,
	nodes: &[Node],
	map: &NodeMap,
) -> (Option<Rect>, Option<Rect>) {
	let rect = |id: &str| map.by_id.get(id).and_then(|&index| node_rect(&nodes[index]));
	(rect(&edge.start), rect(&edge.end))
}

/// `segmentEnterPoint`.
fn enter_point(outside: Point, inside: Point, rect: &Rect) -> Point {
	if same_y(outside, inside, EPS) {
		return Point {
			x: if outside.x < rect.left { rect.left } else { rect.right },
			y: outside.y,
		};
	}
	if same_x(outside, inside, EPS) {
		return Point {
			x: outside.x,
			y: if outside.y < rect.top { rect.top } else { rect.bottom },
		};
	}
	Point {
		x: outside.x.max(rect.left).min(rect.right),
		y: outside.y.max(rect.top).min(rect.bottom),
	}
}

/// `clipEndpoint`: points inside the node are replaced by the point where the route enters it.
fn clip_endpoint(points: Vec<Point>, rect: &Rect, at_start: bool) -> Vec<Point> {
	let count = points.len() as isize;
	let step: isize = if at_start { 1 } else { -1 };
	let mut outside = if at_start { 0 } else { count - 1 };
	while outside >= 0
		&& outside < count
		&& rect.contains_point(points[outside as usize], INSIDE_EPS)
	{
		outside += step;
	}
	if outside < 0 || outside >= count {
		return points;
	}
	let inside = outside - step;
	if inside < 0 || inside >= count {
		return points;
	}
	let (outside, inside) = (outside as usize, inside as usize);
	let entry = enter_point(points[outside], points[inside], rect);
	match at_start {
		true => std::iter::once(entry).chain(points[outside..].iter().copied()).collect(),
		false => points[..=outside].iter().copied().chain(std::iter::once(entry)).collect(),
	}
}

/// `clipEdgeEndpointsToNodeBoundaries`.
pub(super) fn clip_edge_endpoints_to_node_boundaries(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	for edge in edges.iter_mut() {
		if edge.is_layout_only {
			continue;
		}
		let Some(points) = edge.points.clone().filter(|points| points.len() >= 2) else {
			continue;
		};
		let (source, target) = end_rects(edge, nodes, map);
		let mut next = points;
		if let Some(rect) = &source {
			next = clip_endpoint(next, rect, true);
		}
		if let Some(rect) = &target {
			next = clip_endpoint(next, rect, false);
		}
		next = simplify(&orthogonalize(&next));
		if let Some(cleared) =
			clear_straight_corner_connections(&next, source.as_ref(), target.as_ref())
		{
			next = cleared;
		}
		edge.points = Some(simplify(&orthogonalize(&next)));
	}
}

/// `snapEndpointToBoundary`.
fn snap_to_boundary(inner: Point, endpoint: Point, rect: &Rect, use_approach_side: bool) -> Point {
	if same_y(inner, endpoint, EPS) {
		if endpoint.y < rect.top - EPS || endpoint.y > rect.bottom + EPS {
			return endpoint;
		}
		if use_approach_side {
			if inner.x < rect.left - EPS {
				return Point { x: rect.left, y: inner.y };
			}
			if inner.x > rect.right + EPS {
				return Point { x: rect.right, y: inner.y };
			}
		}
		let to_left = (endpoint.x - rect.left).abs() <= (endpoint.x - rect.right).abs();
		return Point { x: if to_left { rect.left } else { rect.right }, y: inner.y };
	}
	if same_x(inner, endpoint, EPS) {
		if endpoint.x < rect.left - EPS || endpoint.x > rect.right + EPS {
			return endpoint;
		}
		if use_approach_side {
			if inner.y < rect.top - EPS {
				return Point { x: inner.x, y: rect.top };
			}
			if inner.y > rect.bottom + EPS {
				return Point { x: inner.x, y: rect.bottom };
			}
		}
		let to_top = (endpoint.y - rect.top).abs() <= (endpoint.y - rect.bottom).abs();
		return Point { x: inner.x, y: if to_top { rect.top } else { rect.bottom } };
	}
	endpoint
}

/// `firstDistinctAdjacent`.
fn first_distinct_adjacent(points: &[Point], endpoint: usize, at_start: bool) -> Option<Point> {
	let reference = points[endpoint];
	let found = match at_start {
		true => points[endpoint + 1..].iter().copied().find(|&p| !same_point(p, reference, EPS)),
		false => points[..endpoint].iter().rev().copied().find(|&p| !same_point(p, reference, EPS)),
	};
	found.or_else(|| match at_start {
		true => points.get(endpoint + 1).copied(),
		false => endpoint.checked_sub(1).map(|index| points[index]),
	})
}

/// `cornerClearanceRange`.
fn clearance_range(min: f64, max: f64) -> (f64, f64) {
	let (low, high) = (min + CORNER_CLEARANCE, max - CORNER_CLEARANCE);
	match low <= high {
		true => (low, high),
		false => ((min + max) / 2.0, (min + max) / 2.0),
	}
}

fn clearance_range_for_side(rect: &Rect, side: BorderSide) -> (f64, f64) {
	match side.is_horizontal() {
		true => clearance_range(rect.top, rect.bottom),
		false => clearance_range(rect.left, rect.right),
	}
}

/// `terminalSideForSegment`: the side a terminal segment ends on.
fn terminal_side(endpoint: Point, adjacent: Point, rect: &Rect) -> Option<BorderSide> {
	let y_within = endpoint.y >= rect.top - EPS && endpoint.y <= rect.bottom + EPS;
	let x_within = endpoint.x >= rect.left - EPS && endpoint.x <= rect.right + EPS;
	if same_y(endpoint, adjacent, EPS) && y_within {
		if (endpoint.x - rect.left).abs() < EPS {
			return Some(BorderSide::Left);
		}
		if (endpoint.x - rect.right).abs() < EPS {
			return Some(BorderSide::Right);
		}
	}
	if same_x(endpoint, adjacent, EPS) && x_within {
		if (endpoint.y - rect.top).abs() < EPS {
			return Some(BorderSide::Top);
		}
		if (endpoint.y - rect.bottom).abs() < EPS {
			return Some(BorderSide::Bottom);
		}
	}
	None
}

/// `clearStraightEndpointCornerConnections`: a straight edge moves away from the corners of
/// both end faces when their clearance ranges overlap. `None` when nothing changes.
fn clear_straight_corner_connections(
	points: &[Point],
	source: Option<&Rect>,
	target: Option<&Rect>,
) -> Option<Vec<Point>> {
	let [start, end] = *points else {
		return None;
	};
	let horizontal = match (same_y(start, end, EPS), same_x(start, end, EPS)) {
		(true, _) => true,
		(false, true) => false,
		_ => return None,
	};
	let mut ranges = Vec::new();
	if let Some(rect) = source
		&& let Some(side) = terminal_side(start, end, rect)
		&& side.is_horizontal() == horizontal
	{
		ranges.push(clearance_range_for_side(rect, side));
	}
	if let Some(rect) = target
		&& let Some(side) = terminal_side(end, start, rect)
		&& side.is_horizontal() == horizontal
	{
		ranges.push(clearance_range_for_side(rect, side));
	}
	if ranges.is_empty() {
		return None;
	}
	let low = ranges.iter().map(|range| range.0).fold(f64::NEG_INFINITY, f64::max);
	let high = ranges.iter().map(|range| range.1).fold(f64::INFINITY, f64::min);
	if low > high {
		return None;
	}
	let current = if horizontal { start.y } else { start.x };
	let next = current.max(low).min(high);
	if (next - current).abs() < EPS {
		return None;
	}
	Some(match horizontal {
		true => vec![Point { x: start.x, y: next }, Point { x: end.x, y: next }],
		false => vec![Point { x: next, y: start.y }, Point { x: next, y: end.y }],
	})
}

/// `clearEndpointCornerConnection`: an end too close to a corner moves along its face, with the
/// collinear run of points behind it.
fn clear_corner_connection(points: Vec<Point>, rect: &Rect, at_start: bool) -> Vec<Point> {
	if points.len() < 2 {
		return points;
	}
	let endpoint_index = if at_start { 0 } else { points.len() - 1 };
	let endpoint = points[endpoint_index];
	let Some(adjacent) = first_distinct_adjacent(&points, endpoint_index, at_start) else {
		return points;
	};
	let Some(side) = terminal_side(endpoint, adjacent, rect) else {
		return points;
	};
	let horizontal = side.is_horizontal();
	let adjusted = match horizontal {
		true => {
			let (low, high) = clearance_range(rect.top, rect.bottom);
			Point { x: endpoint.x, y: endpoint.y.max(low).min(high) }
		}
		false => {
			let (low, high) = clearance_range(rect.left, rect.right);
			Point { x: endpoint.x.max(low).min(high), y: endpoint.y }
		}
	};
	if same_point(endpoint, adjusted, EPS) {
		return points;
	}
	let mut next = points.clone();
	let indices: Box<dyn Iterator<Item = usize>> = match at_start {
		true => Box::new(0..points.len()),
		false => Box::new((0..points.len()).rev()),
	};
	for index in indices {
		let point = points[index];
		match horizontal {
			true if !same_y(point, endpoint, EPS) => break,
			false if !same_x(point, endpoint, EPS) => break,
			true => next[index].y = adjusted.y,
			false => next[index].x = adjusted.x,
		}
	}
	next
}

/// `borderSideForSegment`: the side a segment runs along.
fn border_side_of_segment(a: Point, b: Point, rect: &Rect) -> Option<BorderSide> {
	let x_within = a.x.min(b.x) >= rect.left - EPS && a.x.max(b.x) <= rect.right + EPS;
	let y_within = a.y.min(b.y) >= rect.top - EPS && a.y.max(b.y) <= rect.bottom + EPS;
	let on = |value: f64, edge: f64| (value - edge).abs() < EPS;
	if on(a.y, rect.top) && on(b.y, rect.top) && x_within {
		return Some(BorderSide::Top);
	}
	if on(a.y, rect.bottom) && on(b.y, rect.bottom) && x_within {
		return Some(BorderSide::Bottom);
	}
	if on(a.x, rect.left) && on(b.x, rect.left) && y_within {
		return Some(BorderSide::Left);
	}
	if on(a.x, rect.right) && on(b.x, rect.right) && y_within {
		return Some(BorderSide::Right);
	}
	None
}

fn leaves_outward(side: BorderSide, from: Point, to: Point, rect: &Rect) -> bool {
	match side {
		BorderSide::Top => same_x(from, to, EPS) && to.y < rect.top - EPS,
		BorderSide::Bottom => same_x(from, to, EPS) && to.y > rect.bottom + EPS,
		BorderSide::Left => same_y(from, to, EPS) && to.x < rect.left - EPS,
		BorderSide::Right => same_y(from, to, EPS) && to.x > rect.right + EPS,
	}
}

/// `collapseOwnBorderStub`: a first (last) segment along the node's own border is dropped.
fn collapse_border_stub(points: Vec<Point>, rect: &Rect, at_start: bool) -> Vec<Point> {
	if points.len() < 3 {
		return points;
	}
	let last = points.len() - 1;
	match at_start {
		true => match border_side_of_segment(points[0], points[1], rect) {
			Some(side) if leaves_outward(side, points[1], points[2], rect) => points[1..].to_vec(),
			_ => points,
		},
		false => match border_side_of_segment(points[last - 1], points[last], rect) {
			Some(side) if leaves_outward(side, points[last - 1], points[last - 2], rect) => {
				points[..last].to_vec()
			}
			_ => points,
		},
	}
}

/// `snapAndCollapseEndpoints`.
fn snap_and_collapse(
	points: Vec<Point>,
	source: Option<&Rect>,
	target: Option<&Rect>,
) -> Vec<Point> {
	let mut next = points;
	if let Some(rect) = source {
		if let Some(adjacent) = first_distinct_adjacent(&next, 0, true) {
			next[0] = snap_to_boundary(adjacent, next[0], rect, false);
		}
		next = collapse_border_stub(next, rect, true);
	}
	if let Some(rect) = target {
		let last = next.len() - 1;
		if let Some(adjacent) = first_distinct_adjacent(&next, last, false) {
			next[last] = snap_to_boundary(adjacent, next[last], rect, true);
		}
		next = collapse_border_stub(next, rect, false);
	}
	if let Some(cleared) = clear_straight_corner_connections(&next, source, target) {
		return cleared;
	}
	if next.len() == 2 {
		return next;
	}
	if let Some(rect) = source {
		next = clear_corner_connection(next, rect, true);
	}
	if let Some(rect) = target {
		next = clear_corner_connection(next, rect, false);
	}
	next
}

/// `prepareEdgeEndpointsForRenderer`: ends snapped to the borders and, for bent routes,
/// duplicated so the renderer's intersection keeps the terminal segments.
pub(super) fn prepare_edge_endpoints_for_renderer(
	edges: &mut [Edge],
	nodes: &[Node],
	map: &NodeMap,
) {
	for edge in edges.iter_mut() {
		if edge.is_layout_only {
			continue;
		}
		let Some(points) = edge.points.as_ref().filter(|points| points.len() >= 2) else {
			continue;
		};
		let input = dedupe(points, EPS);
		let (source, target) = end_rects(edge, nodes, map);
		let next = match input.len() {
			// A single point remains when both ends coincide; Mermaid would index past it.
			0 | 1 => input,
			_ => snap_and_collapse(input, source.as_ref(), target.as_ref()),
		};
		if next.len() < 3 {
			edge.points = Some(next);
			continue;
		}
		let (first, last) = (next[0], next[next.len() - 1]);
		let mut duplicated = Vec::with_capacity(next.len() + 2);
		duplicated.push(first);
		duplicated.extend_from_slice(&next[..next.len() - 1]);
		duplicated.push(last);
		duplicated.push(last);
		edge.points = Some(duplicated);
	}
}
