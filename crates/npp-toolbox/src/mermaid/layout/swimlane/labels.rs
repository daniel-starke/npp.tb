//! Edge labels placed on their routed polylines (`direction/labelAnchoring.ts`).

use super::geometry::{EPS, Rect, dedupe, node_rect, segment_overlaps_rect};
use super::{Edge, Node, NodeMap, Point};

const MARKER_CLEARANCE_LENGTH: f64 = 10.0;
const MARKER_CLEARANCE_HALF_WIDTH: f64 = 7.0;
/// A little more than the validator's border-hugging distance.
const LABEL_PLACEMENT_BUFFER: f64 = 3.0;
const LABEL_LANE_MARGIN: f64 = 1.0;
/// The point marker takes about 10 px at the end of an edge.
const LABEL_ENDPOINT_CLEARANCE: f64 = 12.0;
const ALONG_SEGMENT: [f64; 9] = [0.5, 0.25, 0.75, 0.05, 0.95, 0.15, 0.85, 0.1, 0.9];

fn js_sign(value: f64) -> f64 {
	if value > 0.0 {
		1.0
	} else if value < 0.0 {
		-1.0
	} else {
		0.0
	}
}

/// `markerClearanceRectFor`: the area of an arrowhead at an end.
fn marker_clearance(points: &[Point], at_start: bool) -> Option<Rect> {
	if points.len() < 2 {
		return None;
	}
	let (tip, inner) = match at_start {
		true => (points[0], points[1]),
		false => (points[points.len() - 1], points[points.len() - 2]),
	};
	let (dx, dy) = (inner.x - tip.x, inner.y - tip.y);
	if dx.abs() + dy.abs() < EPS {
		return None;
	}
	if dy.abs() <= EPS {
		let x2 = tip.x + js_sign(dx) * MARKER_CLEARANCE_LENGTH;
		return Some(Rect {
			left: tip.x.min(x2),
			right: tip.x.max(x2),
			top: tip.y - MARKER_CLEARANCE_HALF_WIDTH,
			bottom: tip.y + MARKER_CLEARANCE_HALF_WIDTH,
		});
	}
	if dx.abs() <= EPS {
		let y2 = tip.y + js_sign(dy) * MARKER_CLEARANCE_LENGTH;
		return Some(Rect {
			left: tip.x - MARKER_CLEARANCE_HALF_WIDTH,
			right: tip.x + MARKER_CLEARANCE_HALF_WIDTH,
			top: tip.y.min(y2),
			bottom: tip.y.max(y2),
		});
	}
	Some(Rect {
		left: tip.x.min(inner.x),
		right: tip.x.max(inner.x),
		top: tip.y.min(inner.y),
		bottom: tip.y.max(inner.y),
	})
}

fn overlaps_own_marker(rect: &Rect, points: &[Point]) -> bool {
	let visible = dedupe(points, EPS);
	[marker_clearance(&visible, true), marker_clearance(&visible, false)]
		.iter()
		.flatten()
		.any(|marker| rect.overlaps(marker))
}

#[derive(Clone, Copy, PartialEq)]
struct Candidate {
	index: usize,
	length: f64,
	horizontal: bool,
	middle: Point,
}

fn inclusive_contains(rect: &Rect, point: Point) -> bool {
	point.x >= rect.left - EPS
		&& point.x <= rect.right + EPS
		&& point.y >= rect.top - EPS
		&& point.y <= rect.bottom + EPS
}

/// `anchorLabelsToPolyline`: each label node moves onto a (preferably middle) segment of its
/// edge where it overlaps no foreign node, edge, other label or its own arrowheads, and joins
/// the lane that contains it.
pub(super) fn anchor_labels_to_polyline(edges: &[Edge], nodes: &mut [Node], map: &NodeMap) {
	let mut all_segments: Vec<(&str, Point, Point)> = Vec::new();
	for other in edges.iter().filter(|edge| !edge.is_layout_only) {
		if let Some(points) = other.points.as_ref().filter(|points| points.len() >= 2) {
			for pair in points.windows(2) {
				all_segments.push((other.id.as_str(), pair[0], pair[1]));
			}
		}
	}
	let mut foreign_nodes: Vec<(String, Rect)> = Vec::new();
	let mut lanes: Vec<(String, Rect)> = Vec::new();
	for &index in &map.order {
		let node = &nodes[index];
		if node.is_group {
			if node.parent_id.is_none()
				&& let Some(rect) = node_rect(node)
			{
				lanes.push((node.id.clone(), rect));
			}
			continue;
		}
		if node.is_edge_label {
			continue;
		}
		if let Some(rect) = node_rect(node) {
			foreign_nodes.push((node.id.clone(), rect));
		}
	}
	let overlaps_foreign_node = |label: &str, rect: &Rect| {
		let buffered = rect.inflate(LABEL_PLACEMENT_BUFFER);
		foreign_nodes.iter().any(|(id, other)| id != label && buffered.overlaps(other))
	};
	let overlaps_foreign_edge = |edge: &str, rect: &Rect| {
		let buffered = rect.inflate(LABEL_PLACEMENT_BUFFER);
		all_segments
			.iter()
			.any(|&(id, a, b)| id != edge && segment_overlaps_rect(a, b, &buffered, 0.0))
	};
	let containing_lane = |rect: &Rect| {
		lanes.iter().find(|(_, lane)| lane.contains_rect(rect)).map(|(id, _)| id.clone())
	};
	let mut placed: Vec<(String, Rect)> = Vec::new();
	for edge in edges.iter().filter(|edge| !edge.is_layout_only) {
		let Some(label_id) = edge.label_node_id.as_deref() else {
			continue;
		};
		let Some(&label_index) = map.by_id.get(label_id) else {
			continue;
		};
		let Some(points) = edge.points.as_ref().filter(|points| points.len() >= 2) else {
			continue;
		};
		let (width, height) =
			(nodes[label_index].width.unwrap_or(0.0), nodes[label_index].height.unwrap_or(0.0));
		if width <= 0.0 || height <= 0.0 {
			continue;
		}
		let mut segments: Vec<Candidate> = Vec::new();
		for (index, pair) in points.windows(2).enumerate() {
			let (a, b) = (pair[0], pair[1]);
			let (dx, dy) = ((a.x - b.x).abs(), (a.y - b.y).abs());
			if (dx < EPS && dy < EPS) || (dx >= EPS && dy >= EPS) {
				continue;
			}
			segments.push(Candidate {
				index,
				length: dx + dy,
				horizontal: dx >= EPS,
				middle: Point { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 },
			});
		}
		let (Some(&first), Some(&last)) = (segments.first(), segments.last()) else {
			continue;
		};
		// Mermaid compares the point index of a segment with the segment count.
		let middle: Vec<Candidate> = match segments.len() >= 3 {
			true => segments
				.iter()
				.copied()
				.filter(|s| s.index > 0 && s.index < segments.len() - 1)
				.collect(),
			false => segments.clone(),
		};
		let pool = if middle.is_empty() { segments.clone() } else { middle };
		let long_axis_horizontal = width >= height;
		let rank = |pool: &[Candidate]| {
			let mut ranked = pool.to_vec();
			let fits = |s: &Candidate| s.length >= if s.horizontal { width } else { height } + 2.0;
			npp::sort::sort_by(&mut ranked[..], |a, b| {
				let (a_long, b_long) =
					(a.horizontal == long_axis_horizontal, b.horizontal == long_axis_horizontal);
				b_long.cmp(&a_long).then_with(|| fits(b).cmp(&fits(a))).then_with(|| {
					b.length.partial_cmp(&a.length).unwrap_or(std::cmp::Ordering::Equal)
				})
			});
			ranked
		};
		let at = |segment: &Candidate, t: f64| {
			let (a, b) = (points[segment.index], points[segment.index + 1]);
			Point { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t }
		};
		let placement = |anchor: Point| -> Option<(String, Point, Rect)> {
			let centered = Rect::from_center(anchor.x, anchor.y, width, height);
			if let Some(lane) = containing_lane(&centered) {
				return Some((lane, anchor, centered));
			}
			// Near a lane border the box stays inside the lane and still covers the point.
			let (lane, lane_rect) =
				lanes.iter().find(|(_, rect)| inclusive_contains(rect, anchor))?;
			let (min_x, max_x) = (
				lane_rect.left + width / 2.0 + LABEL_LANE_MARGIN,
				lane_rect.right - width / 2.0 - LABEL_LANE_MARGIN,
			);
			let (min_y, max_y) = (
				lane_rect.top + height / 2.0 + LABEL_LANE_MARGIN,
				lane_rect.bottom - height / 2.0 - LABEL_LANE_MARGIN,
			);
			if min_x > max_x || min_y > max_y {
				return None;
			}
			let clamped =
				Point { x: anchor.x.max(min_x).min(max_x), y: anchor.y.max(min_y).min(max_y) };
			let rect = Rect::from_center(clamped.x, clamped.y, width, height);
			inclusive_contains(&rect, anchor).then(|| (lane.clone(), clamped, rect))
		};
		let clears_ends = |segment: &Candidate, anchor: Point| {
			let required = if segment.horizontal { width / 2.0 } else { height / 2.0 }
				+ LABEL_ENDPOINT_CLEARANCE;
			let distance = |end: Point| match segment.horizontal {
				true => (anchor.x - end.x).abs(),
				false => (anchor.y - end.y).abs(),
			};
			!(segment.index == first.index && distance(points[segment.index]) + EPS < required)
				&& !(segment.index == last.index
					&& distance(points[segment.index + 1]) + EPS < required)
		};
		let overlaps_placed = |rect: &Rect, placed: &[(String, Rect)]| {
			placed.iter().any(|(id, other)| id != label_id && rect.overlaps(other))
		};
		let try_pool = |pool: &[Candidate], placed: &[(String, Rect)]| -> Option<(String, Point)> {
			for segment in rank(pool) {
				for t in ALONG_SEGMENT {
					let anchor = at(&segment, t);
					if !clears_ends(&segment, anchor) {
						continue;
					}
					let Some((lane, anchor, rect)) = placement(anchor) else {
						continue;
					};
					if overlaps_own_marker(&rect, points) || overlaps_placed(&rect, placed) {
						continue;
					}
					if !overlaps_foreign_node(label_id, &rect)
						&& !overlaps_foreign_edge(&edge.id, &rect)
					{
						return Some((lane, anchor));
					}
				}
			}
			None
		};
		let fallback = |pool: &[Candidate],
		                placed: &[(String, Rect)],
		                require_clearance: bool,
		                allow_edge_overlap: bool|
		 -> Option<(String, Point)> {
			for segment in rank(pool) {
				let anchor = segment.middle;
				if require_clearance && !clears_ends(&segment, anchor) {
					continue;
				}
				if let Some((lane, anchor, rect)) = placement(anchor)
					&& !overlaps_own_marker(&rect, points)
					&& !overlaps_placed(&rect, placed)
					&& !overlaps_foreign_node(label_id, &rect)
					&& (allow_edge_overlap || !overlaps_foreign_edge(&edge.id, &rect))
				{
					return Some((lane, anchor));
				}
			}
			None
		};
		let chosen = try_pool(&pool, &placed)
			.or_else(|| {
				(pool.len() < segments.len()).then(|| try_pool(&segments, &placed)).flatten()
			})
			.or_else(|| fallback(&segments, &placed, true, false))
			.or_else(|| fallback(&segments, &placed, false, false))
			.or_else(|| fallback(&segments, &placed, false, true));
		if let Some((lane, anchor)) = chosen {
			let label = &mut nodes[label_index];
			label.x = Some(anchor.x);
			label.y = Some(anchor.y);
			label.parent_id = Some(lane);
			let rect = Rect::from_center(anchor.x, anchor.y, width, height);
			match placed.iter_mut().find(|(id, _)| id == label_id) {
				Some(entry) => entry.1 = rect,
				None => placed.push((label_id.to_string(), rect)),
			}
		}
	}
}
