//! Polyline and rectangle helpers of the swimlane post-processing (`direction/geometry.ts`).

use super::{Edge, Node, Point};

/// `EPS` of the direction passes.
pub(super) const EPS: f64 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RectSide {
	Top,
	Bottom,
	Left,
	Right,
}

impl RectSide {
	pub(super) fn is_horizontal(self) -> bool {
		matches!(self, Self::Left | Self::Right)
	}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
	pub left: f64,
	pub right: f64,
	pub top: f64,
	pub bottom: f64,
}

impl Rect {
	pub(super) fn from_center(x: f64, y: f64, width: f64, height: f64) -> Self {
		Self {
			left: x - width / 2.0,
			right: x + width / 2.0,
			top: y - height / 2.0,
			bottom: y + height / 2.0,
		}
	}

	pub(super) fn inflate(self, margin: f64) -> Self {
		Self {
			left: self.left - margin,
			right: self.right + margin,
			top: self.top - margin,
			bottom: self.bottom + margin,
		}
	}

	/// `pointInsideRect`.
	pub(super) fn contains_point(&self, point: Point, buffer: f64) -> bool {
		point.x > self.left + buffer
			&& point.x < self.right - buffer
			&& point.y > self.top + buffer
			&& point.y < self.bottom - buffer
	}

	/// `rectContainsRect`.
	pub(super) fn contains_rect(&self, inner: &Rect) -> bool {
		self.left <= inner.left
			&& self.right >= inner.right
			&& self.top <= inner.top
			&& self.bottom >= inner.bottom
	}

	/// `rectsOverlap`.
	pub(super) fn overlaps(&self, other: &Rect) -> bool {
		self.left < other.right
			&& self.right > other.left
			&& self.top < other.bottom
			&& self.bottom > other.top
	}
}

/// A node with a measured box (`NodeBoundsInfo`).
#[derive(Clone, Debug)]
pub(super) struct NodeInfo {
	pub id: String,
	pub cx: f64,
	pub cy: f64,
	pub rect: Rect,
}

/// `measuredNodeRect`: only nodes with a positive size.
fn measured(node: &Node) -> Option<(f64, f64, Rect)> {
	let (x, y) = (node.x.unwrap_or(0.0), node.y.unwrap_or(0.0));
	let (width, height) = (node.width.unwrap_or(0.0), node.height.unwrap_or(0.0));
	(width > 0.0 && height > 0.0).then(|| (x, y, Rect::from_center(x, y, width, height)))
}

/// `nodeBoundsInfoFor`: non-group nodes with a positive size.
pub(super) fn node_info(node: &Node) -> Option<NodeInfo> {
	if node.is_group {
		return None;
	}
	let (cx, cy, rect) = measured(node)?;
	Some(NodeInfo { id: node.id.clone(), cx, cy, rect })
}

/// `rectOfNodeBounds`.
pub(super) fn node_rect(node: &Node) -> Option<Rect> {
	measured(node).map(|(_, _, rect)| rect)
}

pub(super) fn same_point(a: Point, b: Point, epsilon: f64) -> bool {
	(a.x - b.x).abs() < epsilon && (a.y - b.y).abs() < epsilon
}

pub(super) fn same_x(a: Point, b: Point, epsilon: f64) -> bool {
	(a.x - b.x).abs() < epsilon
}

pub(super) fn same_y(a: Point, b: Point, epsilon: f64) -> bool {
	(a.y - b.y).abs() < epsilon
}

pub(super) fn is_horizontal(a: Point, b: Point, epsilon: f64) -> bool {
	same_y(a, b, epsilon) && (a.x - b.x).abs() > epsilon
}

pub(super) fn is_vertical(a: Point, b: Point, epsilon: f64) -> bool {
	same_x(a, b, epsilon) && (a.y - b.y).abs() > epsilon
}

/// `overlapLength`.
pub(super) fn overlap_length(a1: f64, a2: f64, b1: f64, b2: f64) -> f64 {
	(a1.max(a2).min(b1.max(b2)) - a1.min(a2).max(b1.min(b2))).max(0.0)
}

/// `OrthogonalSegment`.
#[derive(Clone, Copy, Debug)]
pub(super) struct Segment {
	pub index: usize,
	pub a: Point,
	pub b: Point,
	pub horizontal: bool,
	pub vertical: bool,
}

/// `sameAxisSegmentOverlapLength`.
pub(super) fn same_axis_overlap_length(a: &Segment, b: &Segment, epsilon: f64) -> f64 {
	if a.horizontal && b.horizontal && same_y(a.a, b.a, epsilon) {
		return overlap_length(a.a.x, a.b.x, b.a.x, b.b.x);
	}
	if a.vertical && b.vertical && same_x(a.a, b.a, epsilon) {
		return overlap_length(a.a.y, a.b.y, b.a.y, b.b.y);
	}
	0.0
}

/// `orthogonalSegmentsForPoints`: the horizontal and vertical segments of a polyline.
pub(super) fn segments(points: &[Point], epsilon: f64) -> Vec<Segment> {
	points
		.windows(2)
		.enumerate()
		.filter_map(|(index, pair)| {
			let (a, b) = (pair[0], pair[1]);
			let (horizontal, vertical) = (is_horizontal(a, b, epsilon), is_vertical(a, b, epsilon));
			(horizontal || vertical).then_some(Segment { index, a, b, horizontal, vertical })
		})
		.collect()
}

/// `countOrthogonalBends`.
pub(super) fn count_bends(points: &[Point], epsilon: f64) -> usize {
	segments(points, epsilon)
		.windows(2)
		.filter(|pair| pair[0].horizontal != pair[1].horizontal)
		.count()
}

/// `dedupeConsecutivePoints`.
pub(super) fn dedupe(points: &[Point], epsilon: f64) -> Vec<Point> {
	let mut result: Vec<Point> = Vec::with_capacity(points.len());
	for &point in points {
		if result.last().is_none_or(|&last| !same_point(last, point, epsilon)) {
			result.push(point);
		}
	}
	result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThreeSegmentKind {
	HorizontalVerticalHorizontal,
	VerticalHorizontalVertical,
}

/// `classifyThreeSegmentRoute`.
pub(super) fn classify_three_segments(points: &[Point], epsilon: f64) -> Option<ThreeSegmentKind> {
	let [p0, p1, p2, p3] = *points else {
		return None;
	};
	if is_horizontal(p0, p1, epsilon)
		&& is_vertical(p1, p2, epsilon)
		&& is_horizontal(p2, p3, epsilon)
	{
		return Some(ThreeSegmentKind::HorizontalVerticalHorizontal);
	}
	(is_vertical(p0, p1, epsilon) && is_horizontal(p1, p2, epsilon) && is_vertical(p2, p3, epsilon))
		.then_some(ThreeSegmentKind::VerticalHorizontalVertical)
}

/// `segmentBoundsOverlapRect`.
pub(super) fn segment_overlaps_rect(a: Point, b: Point, rect: &Rect, buffer: f64) -> bool {
	a.x.max(b.x) > rect.left - buffer
		&& a.x.min(b.x) < rect.right + buffer
		&& a.y.max(b.y) > rect.top - buffer
		&& a.y.min(b.y) < rect.bottom + buffer
}

/// `portForRectSide`.
pub(super) fn port_for_side(node: &NodeInfo, side: RectSide) -> Point {
	match side {
		RectSide::Top => Point { x: node.cx, y: node.rect.top },
		RectSide::Bottom => Point { x: node.cx, y: node.rect.bottom },
		RectSide::Left => Point { x: node.rect.left, y: node.cy },
		RectSide::Right => Point { x: node.rect.right, y: node.cy },
	}
}

/// `buildOrthogonalPortPath`: a straight, L, Z or U path between two ports facing the given
/// sides, if one leaves and enters them outward.
pub(super) fn orthogonal_port_path(
	src: Point,
	src_side: RectSide,
	dst: Point,
	dst_side: RectSide,
	anchor: f64,
	epsilon: f64,
) -> Option<Vec<Point>> {
	use RectSide::{Bottom, Left, Right, Top};
	let (src_horizontal, dst_horizontal) = (src_side.is_horizontal(), dst_side.is_horizontal());
	if src_horizontal && dst_horizontal {
		let opposing = (src_side == Right && dst_side == Left && src.x < dst.x)
			|| (src_side == Left && dst_side == Right && src.x > dst.x);
		if opposing {
			if same_y(src, dst, epsilon) {
				return Some(vec![src, dst]);
			}
			let middle = (src.x + dst.x) / 2.0;
			return Some(vec![
				src,
				Point { x: middle, y: src.y },
				Point { x: middle, y: dst.y },
				dst,
			]);
		}
		if src_side == dst_side {
			if same_y(src, dst, epsilon) {
				return None;
			}
			let x = match src_side {
				Left => src.x.min(dst.x) - anchor,
				_ => src.x.max(dst.x) + anchor,
			};
			return Some(vec![src, Point { x, y: src.y }, Point { x, y: dst.y }, dst]);
		}
		return None;
	}
	if !src_horizontal && !dst_horizontal {
		if src_side == dst_side {
			if same_x(src, dst, epsilon) {
				return None;
			}
			let y = match src_side {
				Top => src.y.min(dst.y) - anchor,
				_ => src.y.max(dst.y) + anchor,
			};
			return Some(vec![src, Point { x: src.x, y }, Point { x: dst.x, y }, dst]);
		}
		let same_direction = (src_side == Bottom && dst_side == Top && src.y < dst.y)
			|| (src_side == Top && dst_side == Bottom && src.y > dst.y);
		if !same_direction {
			return None;
		}
		if same_x(src, dst, epsilon) {
			return Some(vec![src, dst]);
		}
		let middle = (src.y + dst.y) / 2.0;
		return Some(vec![src, Point { x: src.x, y: middle }, Point { x: dst.x, y: middle }, dst]);
	}
	if src_horizontal {
		let from_source =
			(src_side == Right && dst.x > src.x) || (src_side == Left && dst.x < src.x);
		let into_target =
			(dst_side == Top && src.y < dst.y) || (dst_side == Bottom && src.y > dst.y);
		return (from_source && into_target).then(|| vec![src, Point { x: dst.x, y: src.y }, dst]);
	}
	let from_source = (src_side == Bottom && dst.y > src.y) || (src_side == Top && dst.y < src.y);
	let into_target = (dst_side == Left && src.x < dst.x) || (dst_side == Right && src.x > dst.x);
	(from_source && into_target).then(|| vec![src, Point { x: src.x, y: dst.y }, dst])
}

/// `buildSameSideTrackPath`.
pub(super) fn same_side_track_path(
	src: Point,
	side: RectSide,
	dst: Point,
	track: f64,
) -> Vec<Point> {
	match side.is_horizontal() {
		true => vec![src, Point { x: track, y: src.y }, Point { x: track, y: dst.y }, dst],
		false => vec![src, Point { x: src.x, y: track }, Point { x: dst.x, y: track }, dst],
	}
}

/// A node box with its id (`RectEntry`).
#[derive(Clone, Debug)]
pub(super) struct RectEntry {
	pub id: String,
	pub rect: Rect,
}

/// `collectRealNodeBounds`: measured non-label nodes (the last node of an id wins the map).
pub(super) fn real_node_bounds(
	nodes: &[Node],
) -> (std::collections::HashMap<String, NodeInfo>, Vec<RectEntry>) {
	let mut by_id = std::collections::HashMap::new();
	let mut rects = Vec::new();
	for node in nodes.iter().filter(|node| !node.is_edge_label) {
		let Some(info) = node_info(node) else { continue };
		rects.push(RectEntry { id: info.id.clone(), rect: info.rect });
		by_id.insert(info.id.clone(), info);
	}
	(by_id, rects)
}

/// `collectNodeRectEntries`: measured nodes, split into real nodes and label nodes.
pub(super) fn node_rect_entries(nodes: &[Node]) -> (Vec<RectEntry>, Vec<RectEntry>) {
	let (mut real, mut labels) = (Vec::new(), Vec::new());
	for node in nodes {
		let Some(info) = node_info(node) else { continue };
		let entry = RectEntry { id: info.id, rect: info.rect };
		match node.is_edge_label {
			true => labels.push(entry),
			false => real.push(entry),
		}
	}
	(real, labels)
}

/// `getNodePairGeometry`.
pub(super) struct NodePair<'a> {
	pub src: &'a NodeInfo,
	pub dst: &'a NodeInfo,
	pub collinear_x: bool,
	pub collinear_y: bool,
}

pub(super) fn node_pair<'a>(
	edge: &Edge,
	by_id: &'a std::collections::HashMap<String, NodeInfo>,
	epsilon: f64,
) -> Option<NodePair<'a>> {
	if edge.start.is_empty() || edge.end.is_empty() {
		return None;
	}
	let (src, dst) = (by_id.get(&edge.start)?, by_id.get(&edge.end)?);
	Some(NodePair {
		src,
		dst,
		collinear_x: (src.cx - dst.cx).abs() < epsilon,
		collinear_y: (src.cy - dst.cy).abs() < epsilon,
	})
}

/// `segmentHitsAnyRect`.
pub(super) fn segment_hits_any_rect(
	a: Point,
	b: Point,
	rects: &[RectEntry],
	exclude: &[&str],
	shrink: f64,
) -> bool {
	rects.iter().any(|entry| {
		!exclude.contains(&entry.id.as_str()) && segment_overlaps_rect(a, b, &entry.rect, -shrink)
	})
}

/// `orthogonalSegmentsCross`: a horizontal and a vertical segment touch or cross, except where
/// they share an endpoint (within `endpoint_tolerance`, Mermaid's default is `1e-6`).
pub(super) fn segments_cross_with(
	a1: Point,
	b1: Point,
	a2: Point,
	b2: Point,
	epsilon: f64,
	endpoint_tolerance: f64,
) -> bool {
	let (s1h, s1v, s2h, s2v) = (
		same_y(a1, b1, epsilon),
		same_x(a1, b1, epsilon),
		same_y(a2, b2, epsilon),
		same_x(a2, b2, epsilon),
	);
	if (s1h && s2h) || (s1v && s2v) || !(s1h || s1v) || !(s2h || s2v) {
		return false;
	}
	let horizontal = if s1h { (a1, b1) } else { (a2, b2) };
	let vertical = if s1v { (a1, b1) } else { (a2, b2) };
	let h_y = horizontal.0.y;
	let (h_x1, h_x2) = (horizontal.0.x.min(horizontal.1.x), horizontal.0.x.max(horizontal.1.x));
	let v_x = vertical.0.x;
	let (v_y1, v_y2) = (vertical.0.y.min(vertical.1.y), vertical.0.y.max(vertical.1.y));
	if v_x < h_x1 || v_x > h_x2 || h_y < v_y1 || h_y > v_y2 {
		return false;
	}
	let at = |point: Point| {
		(v_x - point.x).abs() < endpoint_tolerance && (h_y - point.y).abs() < endpoint_tolerance
	};
	!((at(horizontal.0) || at(horizontal.1)) && (at(vertical.0) || at(vertical.1)))
}

pub(super) fn segments_cross(a1: Point, b1: Point, a2: Point, b2: Point, epsilon: f64) -> bool {
	segments_cross_with(a1, b1, a2, b2, epsilon, 1e-6)
}

/// `sameAxisSegmentsOverlap`.
pub(super) fn same_axis_segments_overlap(
	a1: Point,
	b1: Point,
	a2: Point,
	b2: Point,
	epsilon: f64,
) -> bool {
	let (s1h, s1v, s2h, s2v) = (
		same_y(a1, b1, epsilon),
		same_x(a1, b1, epsilon),
		same_y(a2, b2, epsilon),
		same_x(a2, b2, epsilon),
	);
	if s1v && s2v && same_x(a1, a2, epsilon) {
		return overlap_length(a1.y, b1.y, a2.y, b2.y) > epsilon;
	}
	if s1h && s2h && same_y(a1, a2, epsilon) {
		return overlap_length(a1.x, b1.x, a2.x, b2.x) > epsilon;
	}
	false
}

/// `segmentConflictsWithAnyEdge`: crossings or overlaps with the other edges' polylines.
pub(super) fn segment_conflicts_with_any_edge(
	a: Point,
	b: Point,
	edges: &[Edge],
	exclude: Option<usize>,
	epsilon: f64,
	skip_degenerate_other: bool,
) -> bool {
	for (index, other) in edges.iter().enumerate() {
		if Some(index) == exclude || other.is_layout_only {
			continue;
		}
		let Some(points) = other.points.as_ref().filter(|points| points.len() >= 2) else {
			continue;
		};
		for pair in points.windows(2) {
			let (oa, ob) = (pair[0], pair[1]);
			if skip_degenerate_other && same_point(oa, ob, epsilon) {
				continue;
			}
			if segments_cross(a, b, oa, ob, epsilon)
				|| same_axis_segments_overlap(a, b, oa, ob, epsilon)
			{
				return true;
			}
		}
	}
	false
}

/// `orthogonalSegmentsStrictlyCross`: interior crossings only.
pub(super) fn segments_strictly_cross(
	a1: Point,
	b1: Point,
	a2: Point,
	b2: Point,
	epsilon: f64,
) -> bool {
	let (a_h, a_v, b_h, b_v) = (
		same_y(a1, b1, epsilon),
		same_x(a1, b1, epsilon),
		same_y(a2, b2, epsilon),
		same_x(a2, b2, epsilon),
	);
	if !((a_h && b_v) || (a_v && b_h)) {
		return false;
	}
	let (horizontal, vertical) = match a_h {
		true => ((a1, b1), (a2, b2)),
		false => ((a2, b2), (a1, b1)),
	};
	let h_y = horizontal.0.y;
	let (h_min, h_max) = (horizontal.0.x.min(horizontal.1.x), horizontal.0.x.max(horizontal.1.x));
	let v_x = vertical.0.x;
	let (v_min, v_max) = (vertical.0.y.min(vertical.1.y), vertical.0.y.max(vertical.1.y));
	v_x > h_min + epsilon && v_x < h_max - epsilon && h_y > v_min + epsilon && h_y < v_max - epsilon
}

fn strictly_between(value: f64, a: f64, b: f64) -> bool {
	value > a.min(b) + EPS && value < a.max(b) - EPS
}

fn is_collinear_intermediate(previous: Point, current: Point, next: Point) -> bool {
	if same_x(previous, current, EPS) && same_x(current, next, EPS) {
		return strictly_between(current.y, previous.y, next.y);
	}
	if same_y(previous, current, EPS) && same_y(current, next, EPS) {
		return strictly_between(current.x, previous.x, next.x);
	}
	false
}

/// `simplifyPolylineOnce`: drops a point whose neighbours coincide (with its successor) and
/// collinear intermediate points.
fn simplify_once(points: &[Point]) -> (Vec<Point>, bool) {
	let mut changed = false;
	let mut out: Vec<Point> = Vec::with_capacity(points.len());
	let mut index = 0;
	while index < points.len() {
		let current = points[index];
		if let (Some(&previous), Some(&next)) = (out.last(), points.get(index + 1)) {
			if same_point(previous, next, EPS) {
				index += 2;
				changed = true;
				continue;
			}
			if is_collinear_intermediate(previous, current, next) {
				index += 1;
				changed = true;
				continue;
			}
		}
		out.push(current);
		index += 1;
	}
	(out, changed)
}

/// `orthogonalizePolyline`: L-bends for diagonal steps, consecutive duplicates removed.
pub(super) fn orthogonalize(points: &[Point]) -> Vec<Point> {
	let Some(&first) = points.first() else {
		return Vec::new();
	};
	let mut cleaned = vec![first];
	for &current in &points[1..] {
		let previous = cleaned[cleaned.len() - 1];
		if !same_x(previous, current, EPS) && !same_y(previous, current, EPS) {
			let incoming_vertical =
				cleaned.len() >= 2 && same_x(cleaned[cleaned.len() - 2], previous, EPS);
			cleaned.push(match incoming_vertical {
				true => Point { x: previous.x, y: current.y },
				false => Point { x: current.x, y: previous.y },
			});
		}
		cleaned.push(current);
	}
	dedupe(&cleaned, EPS)
}

/// `simplifyPolyline`: at most 32 passes of `simplify_once`.
pub(super) fn simplify(points: &[Point]) -> Vec<Point> {
	if points.len() < 3 {
		return points.to_vec();
	}
	let mut work = points.to_vec();
	for _ in 0..32 {
		let (next, changed) = simplify_once(&work);
		work = next;
		if !changed {
			break;
		}
	}
	work
}
