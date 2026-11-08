//! Short terminal stubs retargeted to the node centre (`direction/terminalStub.ts`) and shared
//! interior rails moved onto parallel tracks (`direction/sharedTrackNudging.ts`).

use super::geometry::{
	Rect, RectEntry, Segment, dedupe, is_horizontal, is_vertical, node_rect, node_rect_entries,
	overlap_length, same_x, same_y, segment_hits_any_rect, segments, segments_strictly_cross,
};
use super::{Edge, Node, NodeMap, Point};
use crate::mermaid::text::to_fixed;

const EPS_LOCAL: f64 = 1e-3;
const BUFFER: f64 = 2.0;

/// The nodes in the order of Mermaid's `nodeByIdMap.values()`.
fn ordered_nodes(nodes: &[Node], map: &NodeMap) -> Vec<Node> {
	map.order.iter().map(|&index| nodes[index].clone()).collect()
}

/// `collapseShortTerminalStub`: a final stub shorter than 10 px after a bend is replaced by a
/// straight approach to the centre of the target's face.
pub(super) fn collapse_short_terminal_stub(edges: &mut [Edge], nodes: &mut [Node], map: &NodeMap) {
	const MIN_STUB: f64 = 10.0;
	let (real_rects, label_rects) = node_rect_entries(&ordered_nodes(nodes, map));
	for current in 0..edges.len() {
		let edge = &edges[current];
		if edge.is_layout_only {
			continue;
		}
		let Some(raw) = edge.points.as_ref().filter(|points| points.len() >= 4) else {
			continue;
		};
		let points = dedupe(raw, EPS_LOCAL);
		if points.len() < 4 {
			continue;
		}
		let last = points.len() - 1;
		let (end, penultimate, previous) = (points[last], points[last - 1], points[last - 2]);
		let last_length = (end.x - penultimate.x).hypot(end.y - penultimate.y);
		if last_length >= MIN_STUB || last_length < EPS_LOCAL {
			continue;
		}
		let (penultimate_dx, penultimate_dy) =
			(penultimate.x - previous.x, penultimate.y - previous.y);
		if penultimate_dx.hypot(penultimate_dy) < EPS_LOCAL {
			continue;
		}
		let (last_horizontal, last_vertical) =
			(is_horizontal(penultimate, end, EPS_LOCAL), is_vertical(penultimate, end, EPS_LOCAL));
		let (penultimate_horizontal, penultimate_vertical) = (
			is_horizontal(previous, penultimate, EPS_LOCAL),
			is_vertical(previous, penultimate, EPS_LOCAL),
		);
		if !((last_horizontal && penultimate_vertical) || (last_vertical && penultimate_horizontal))
		{
			continue;
		}
		let Some(&target) = map.by_id.get(&edge.end) else {
			continue;
		};
		let target_node = &nodes[target];
		let (center_x, center_y) = (target_node.x.unwrap_or(0.0), target_node.y.unwrap_or(0.0));
		let Some(rect) = node_rect(target_node) else {
			continue;
		};
		let (new_previous, new_end) = match penultimate_vertical {
			true => (
				Point { x: center_x, y: previous.y },
				Point { x: center_x, y: if penultimate_dy < 0.0 { rect.bottom } else { rect.top } },
			),
			false => (
				Point { x: previous.x, y: center_y },
				Point { x: if penultimate_dx > 0.0 { rect.right } else { rect.left }, y: center_y },
			),
		};
		let target_id = [edge.end.as_str()];
		let exclude: &[&str] = if edge.end.is_empty() { &[] } else { &target_id };
		if segment_hits_any_rect(new_previous, new_end, &real_rects, exclude, -BUFFER)
			|| segment_hits_any_rect(new_previous, new_end, &label_rects, &[], -BUFFER)
		{
			continue;
		}
		if let Some(source_rect) =
			map.by_id.get(&edge.start).and_then(|&index| node_rect(&nodes[index]))
			&& source_rect.contains_point(new_previous, BUFFER)
		{
			continue;
		}
		let key = |a: Point, b: Point| {
			format!(
				"{},{}|{},{}",
				to_fixed(a.x, 3),
				to_fixed(a.y, 3),
				to_fixed(b.x, 3),
				to_fixed(b.y, 3)
			)
		};
		let own: std::collections::HashSet<String> =
			points.windows(2).map(|pair| key(pair[0], pair[1])).collect();
		let crosses_other = |from: Point, to: Point| {
			edges.iter().enumerate().any(|(index, other)| {
				index != current
					&& !other.is_layout_only
					&& other.points.as_ref().is_some_and(|points| {
						points.len() >= 2
							&& points.windows(2).any(|pair| {
								!own.contains(&key(pair[0], pair[1]))
									&& segments_strictly_cross(
										from, to, pair[0], pair[1], EPS_LOCAL,
									)
							})
					})
			})
		};
		if crosses_other(new_previous, new_end) {
			continue;
		}
		if last >= 3 {
			let before = points[last - 3];
			let ends: Vec<&str> = [edge.start.as_str(), edge.end.as_str()]
				.into_iter()
				.filter(|id| !id.is_empty())
				.collect();
			if segment_hits_any_rect(before, new_previous, &real_rects, &ends, -BUFFER)
				|| crosses_other(before, new_previous)
			{
				continue;
			}
		}
		let mut new_points = points[..last - 2].to_vec();
		new_points.push(new_previous);
		new_points.push(new_end);
		let label = edge.label_node_id.clone();
		edges[current].points = Some(new_points.clone());
		// The label moves to the middle of the longest segment it fits on.
		let Some(&label_index) = label.as_ref().and_then(|label| map.by_id.get(label)) else {
			continue;
		};
		let label_node = &mut nodes[label_index];
		let (width, height) = (label_node.width.unwrap_or(0.0), label_node.height.unwrap_or(0.0));
		if width <= 0.0 || height <= 0.0 {
			continue;
		}
		let mut best: Option<Point> = None;
		let mut best_length = -1.0;
		for pair in new_points.windows(2) {
			let (a, b) = (pair[0], pair[1]);
			let length = (b.x - a.x).hypot(b.y - a.y);
			let fits = (same_y(a, b, EPS_LOCAL) && length >= width + 2.0)
				|| (same_x(a, b, EPS_LOCAL) && length >= height + 2.0);
			if fits && length > best_length {
				best_length = length;
				best = Some(Point { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 });
			}
		}
		if let Some(middle) = best {
			label_node.x = Some(middle.x);
			label_node.y = Some(middle.y);
		}
	}
}

/// A segment of an edge for the nudging (`SegmentLite`).
#[derive(Clone, Copy)]
struct EdgeSegment {
	segment: Segment,
	edge: usize,
	interior: bool,
}

fn edge_segments(edge: usize, points: &[Point]) -> Vec<EdgeSegment> {
	segments(points, EPS_LOCAL)
		.into_iter()
		.map(|segment| EdgeSegment {
			segment,
			edge,
			interior: segment.index >= 1 && segment.index + 3 <= points.len(),
		})
		.collect()
}

const MIN_SHARED: f64 = 8.0;
const TRACK_SHIFT: f64 = 7.0;

/// `hasCrowdedParallelTrack`: parallel segments closer than a track overlapping 8 px or more.
fn crowded(a: &Segment, b: &Segment) -> bool {
	if a.horizontal && b.horizontal {
		return overlap_length(a.a.x, a.b.x, b.a.x, b.b.x) >= MIN_SHARED
			&& (a.a.y - b.a.y).abs() < TRACK_SHIFT;
	}
	if a.vertical && b.vertical {
		return overlap_length(a.a.y, a.b.y, b.a.y, b.b.y) >= MIN_SHARED
			&& (a.a.x - b.a.x).abs() < TRACK_SHIFT;
	}
	false
}

/// `nudgeSharedInteriorSubpaths`: interior segments that share a rail with another edge move
/// to a parallel track (or, for a Z leaving the source, onto a detour from the face centre).
pub(super) fn nudge_shared_interior_subpaths(edges: &mut [Edge], nodes: &[Node], map: &NodeMap) {
	const SOURCE_DETOUR_STUB: f64 = 20.0;
	const MAX_ITERATIONS: usize = 12;
	const SHIFTS: [f64; 6] = [
		-TRACK_SHIFT,
		TRACK_SHIFT,
		-2.0 * TRACK_SHIFT,
		2.0 * TRACK_SHIFT,
		-3.0 * TRACK_SHIFT,
		3.0 * TRACK_SHIFT,
	];
	let (real_rects, label_rects) = node_rect_entries(&ordered_nodes(nodes, map));
	let candidate_is_safe = |edges: &[Edge],
	                         edge: usize,
	                         candidate: &[Point],
	                         real_rects: &[RectEntry],
	                         label_rects: &[RectEntry]| {
		let candidate_segments = edge_segments(edge, candidate);
		if candidate_segments.len() + 1 != candidate.len() {
			return false;
		}
		let this = &edges[edge];
		let ends: Vec<&str> = [this.start.as_str(), this.end.as_str()]
			.into_iter()
			.filter(|id| !id.is_empty())
			.collect();
		let own_label: Vec<&str> = this.label_node_id.as_deref().into_iter().collect();
		for segment in &candidate_segments {
			let (a, b) = (segment.segment.a, segment.segment.b);
			if segment_hits_any_rect(a, b, real_rects, &ends, -BUFFER)
				|| segment_hits_any_rect(a, b, label_rects, &own_label, -BUFFER)
			{
				return false;
			}
		}
		for (index, other) in edges.iter().enumerate() {
			if index == edge || other.is_layout_only {
				continue;
			}
			let Some(points) = other.points.as_ref().filter(|points| points.len() >= 2) else {
				continue;
			};
			let other_segments = edge_segments(index, &dedupe(points, super::geometry::EPS));
			for candidate_segment in &candidate_segments {
				for other_segment in &other_segments {
					let (c, o) = (&candidate_segment.segment, &other_segment.segment);
					if crowded(c, o) || segments_strictly_cross(c.a, c.b, o.a, o.b, EPS_LOCAL) {
						return false;
					}
				}
			}
		}
		true
	};
	let shifted = |edges: &[Edge], segment: &EdgeSegment, shift: f64| -> Option<Vec<Point>> {
		let mut candidate =
			dedupe(edges[segment.edge].points.as_deref().unwrap_or(&[]), super::geometry::EPS);
		let index = segment.segment.index;
		if candidate.len() < 4 || index + 1 >= candidate.len() {
			return None;
		}
		match (segment.segment.horizontal, segment.segment.vertical) {
			(true, _) => {
				candidate[index].y += shift;
				candidate[index + 1].y += shift;
			}
			(false, true) => {
				candidate[index].x += shift;
				candidate[index + 1].x += shift;
			}
			_ => return None,
		}
		(edge_segments(segment.edge, &candidate).len() + 1 == candidate.len()).then_some(candidate)
	};
	let center = |node: &Node, rect: &Rect| Point {
		x: node.x.unwrap_or((rect.left + rect.right) / 2.0),
		y: node.y.unwrap_or((rect.top + rect.bottom) / 2.0),
	};
	let source_detour = |edges: &[Edge], segment: &EdgeSegment, shift: f64| -> Option<Vec<Point>> {
		let edge = &edges[segment.edge];
		let points = dedupe(edge.points.as_deref().unwrap_or(&[]), super::geometry::EPS);
		if points.len() != 4 || segment.segment.index != 1 {
			return None;
		}
		let source_node = &nodes[*map.by_id.get(&edge.start)?];
		let target_node = &nodes[*map.by_id.get(&edge.end)?];
		let (source_rect, target_rect) = (node_rect(source_node)?, node_rect(target_node)?);
		let tail = &points[3..];
		let (source_center, target_center) =
			(center(source_node, &source_rect), center(target_node, &target_rect));
		let (a, b) = (segment.segment.a, segment.segment.b);
		let route = match (segment.segment.vertical, segment.segment.horizontal) {
			(true, _) => {
				let below = target_center.y >= source_center.y;
				let port_y = if below { source_rect.bottom } else { source_rect.top };
				let stub_y = port_y + if below { SOURCE_DETOUR_STUB } else { -SOURCE_DETOUR_STUB };
				if (below && b.y <= stub_y + EPS_LOCAL) || (!below && b.y >= stub_y - EPS_LOCAL) {
					return None;
				}
				let rail_x = a.x + shift;
				vec![
					Point { x: source_center.x, y: port_y },
					Point { x: source_center.x, y: stub_y },
					Point { x: rail_x, y: stub_y },
					Point { x: rail_x, y: b.y },
				]
			}
			(false, true) => {
				let right = target_center.x >= source_center.x;
				let port_x = if right { source_rect.right } else { source_rect.left };
				let stub_x = port_x + if right { SOURCE_DETOUR_STUB } else { -SOURCE_DETOUR_STUB };
				if (right && b.x <= stub_x + EPS_LOCAL) || (!right && b.x >= stub_x - EPS_LOCAL) {
					return None;
				}
				let rail_y = a.y + shift;
				vec![
					Point { x: port_x, y: source_center.y },
					Point { x: stub_x, y: source_center.y },
					Point { x: stub_x, y: rail_y },
					Point { x: b.x, y: rail_y },
				]
			}
			_ => return None,
		};
		let mut route = route;
		route.extend_from_slice(tail);
		Some(dedupe(&route, EPS_LOCAL))
	};
	for _ in 0..MAX_ITERATIONS {
		let mut all = Vec::new();
		for (index, edge) in edges.iter().enumerate() {
			if edge.is_layout_only {
				continue;
			}
			if let Some(points) = edge.points.as_ref().filter(|points| points.len() >= 2) {
				all.extend(edge_segments(index, &dedupe(points, super::geometry::EPS)));
			}
		}
		let mut fixed = false;
		'search: for i in 0..all.len() {
			for j in i + 1..all.len() {
				let (first, second) = (all[i], all[j]);
				if first.edge == second.edge || !crowded(&first.segment, &second.segment) {
					continue;
				}
				for segment in [first, second].into_iter().filter(|segment| segment.interior) {
					for shift in SHIFTS {
						if let Some(direct) = shifted(edges, &segment, shift)
							&& candidate_is_safe(
								edges,
								segment.edge,
								&direct,
								&real_rects,
								&label_rects,
							) {
							edges[segment.edge].points = Some(direct);
							fixed = true;
							break 'search;
						}
						if let Some(detoured) = source_detour(edges, &segment, shift)
							&& candidate_is_safe(
								edges,
								segment.edge,
								&detoured,
								&real_rects,
								&label_rects,
							) {
							edges[segment.edge].points = Some(detoured);
							fixed = true;
							break 'search;
						}
					}
				}
			}
		}
		if !fixed {
			return;
		}
	}
}
