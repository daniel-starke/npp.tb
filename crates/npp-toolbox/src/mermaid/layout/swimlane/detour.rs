//! Rewrites of heavily bent routes (`direction/detourSimplification.ts`) and of U-detours
//! between aligned siblings (`direction/siblingSharedFaceRouting.ts`).

use std::collections::HashMap;

use super::geometry::{
	self, EPS, NodeInfo, RectSide, classify_three_segments, count_bends, orthogonal_port_path,
	port_for_side, real_node_bounds, same_axis_overlap_length, same_side_track_path,
	segment_hits_any_rect, segments, segments_cross_with,
};
use super::{Edge, Node, Point};

const MIN_SHARED: f64 = 8.0;
/// Raykov's `ANCHOR_OFFSET`: ports leave their side this far before turning.
const ANCHOR: f64 = 20.0;
const BEND_THRESHOLD: usize = 4;
const SIDES: [RectSide; 4] = [RectSide::Top, RectSide::Bottom, RectSide::Left, RectSide::Right];

/// `nearestSideOfRect`.
fn nearest_side(point: Point, info: &NodeInfo) -> RectSide {
	let distances = [
		(RectSide::Top, (point.y - info.rect.top).abs()),
		(RectSide::Bottom, (point.y - info.rect.bottom).abs()),
		(RectSide::Left, (point.x - info.rect.left).abs()),
		(RectSide::Right, (point.x - info.rect.right).abs()),
	];
	let mut best = distances[0];
	for &candidate in &distances[1..] {
		if candidate.1 < best.1 {
			best = candidate;
		}
	}
	best.0
}

/// Crossings and long overlaps of a path with the other edges (`pathConflictCount`).
fn conflict_count(path: &[Point], edges: &[Edge], current: usize, include_incident: bool) -> usize {
	let mut conflicts = 0;
	let path_segments = segments(path, EPS);
	let (start, end) = (edges[current].start.as_str(), edges[current].end.as_str());
	for (index, other) in edges.iter().enumerate() {
		if index == current || other.is_layout_only {
			continue;
		}
		let incident =
			[other.start.as_str(), other.end.as_str()].iter().any(|id| *id == start || *id == end);
		if !include_incident && !start.is_empty() && !end.is_empty() && incident {
			continue;
		}
		let Some(points) = other.points.as_ref().filter(|points| points.len() >= 2) else {
			continue;
		};
		let other_segments = segments(points, EPS);
		for path_segment in &path_segments {
			for other_segment in &other_segments {
				if segments_cross_with(
					path_segment.a,
					path_segment.b,
					other_segment.a,
					other_segment.b,
					EPS,
					EPS,
				) || same_axis_overlap_length(path_segment, other_segment, EPS) >= MIN_SHARED
				{
					conflicts += 1;
				}
			}
		}
	}
	conflicts
}

/// `simplifyDetouredEdges`: an edge with four or more bends takes the port pair with the
/// fewest bends (or fewer crossings) whose faces no other edge claims.
pub(super) fn simplify_detoured_edges(edges: &mut [Edge], nodes: &[Node]) {
	let (info_by_id, rects) = real_node_bounds(nodes);
	let extreme = |pick: fn(&geometry::Rect) -> f64, minimum: bool| {
		let values = rects.iter().map(|entry| pick(&entry.rect));
		match minimum {
			true => values.fold(f64::INFINITY, f64::min) - ANCHOR,
			false => values.fold(f64::NEG_INFINITY, f64::max) + ANCHOR,
		}
	};
	let outside_track = |side: RectSide| match side {
		RectSide::Top => extreme(|rect| rect.top, true),
		RectSide::Bottom => extreme(|rect| rect.bottom, false),
		RectSide::Left => extreme(|rect| rect.left, true),
		RectSide::Right => extreme(|rect| rect.right, false),
	};
	let candidates = |src: Point, src_side: RectSide, dst: Point, dst_side: RectSide| {
		let mut paths = Vec::new();
		if let Some(base) = orthogonal_port_path(src, src_side, dst, dst_side, ANCHOR, EPS) {
			paths.push(base);
		}
		if src_side == dst_side {
			paths.push(same_side_track_path(src, src_side, dst, outside_track(src_side)));
		}
		paths
	};
	let hits_node = |path: &[Point], exclude: &[&str]| {
		path.windows(2).any(|pair| segment_hits_any_rect(pair[0], pair[1], &rects, exclude, 1.0))
	};
	let mut claims: HashMap<String, Vec<(RectSide, String)>> = HashMap::new();
	for edge in edges.iter().filter(|edge| !edge.is_layout_only) {
		let Some(points) = edge.points.as_ref().filter(|points| !points.is_empty()) else {
			continue;
		};
		if let Some(info) = info_by_id.get(&edge.start) {
			claims
				.entry(edge.start.clone())
				.or_default()
				.push((nearest_side(points[0], info), edge.id.clone()));
		}
		if let Some(info) = info_by_id.get(&edge.end) {
			claims
				.entry(edge.end.clone())
				.or_default()
				.push((nearest_side(points[points.len() - 1], info), edge.id.clone()));
		}
	}
	let is_claimed = |claims: &HashMap<String, Vec<(RectSide, String)>>,
	                  node: &str,
	                  side: RectSide,
	                  ignore: &str| {
		claims
			.get(node)
			.is_some_and(|list| list.iter().any(|(claimed, id)| id != ignore && *claimed == side))
	};
	for current in 0..edges.len() {
		let edge = &edges[current];
		if edge.is_layout_only {
			continue;
		}
		let Some(points) = edge.points.clone().filter(|points| points.len() >= 2) else {
			continue;
		};
		let current_bends = count_bends(&points, EPS);
		if current_bends < BEND_THRESHOLD || edge.start.is_empty() || edge.end.is_empty() {
			continue;
		}
		let (Some(src_info), Some(dst_info)) =
			(info_by_id.get(&edge.start), info_by_id.get(&edge.end))
		else {
			continue;
		};
		let (src_id, dst_id, edge_id) = (edge.start.clone(), edge.end.clone(), edge.id.clone());
		let current_crossings = conflict_count(&points, edges, current, true);
		let current_non_incident = conflict_count(&points, edges, current, false);
		let mut best_path: Option<Vec<Point>> = None;
		let mut best_crossings = current_crossings;
		let mut best_bends = current_bends;
		for src_side in SIDES {
			if is_claimed(&claims, &src_id, src_side, &edge_id) {
				continue;
			}
			let src_port = port_for_side(src_info, src_side);
			for dst_side in SIDES {
				if is_claimed(&claims, &dst_id, dst_side, &edge_id) {
					continue;
				}
				let dst_port = port_for_side(dst_info, dst_side);
				for path in candidates(src_port, src_side, dst_port, dst_side) {
					if hits_node(&path, &[&src_id, &dst_id]) {
						continue;
					}
					let path_bends = count_bends(&path, EPS);
					if current_crossings > 0 {
						let path_crossings = conflict_count(&path, edges, current, true);
						if path_crossings > best_crossings
							|| (path_crossings == best_crossings && path_bends >= best_bends)
						{
							continue;
						}
						best_crossings = path_crossings;
						best_bends = path_bends;
						best_path = Some(path);
						continue;
					}
					if conflict_count(&path, edges, current, false) > current_non_incident {
						continue;
					}
					if path_bends < best_bends {
						best_bends = path_bends;
						best_path = Some(path);
					}
				}
			}
		}
		if let Some(path) = best_path {
			for node in [&src_id, &dst_id] {
				if let Some(list) = claims.get_mut(node) {
					list.retain(|(_, id)| *id != edge_id);
				}
			}
			claims
				.entry(src_id.clone())
				.or_default()
				.push((nearest_side(path[0], src_info), edge_id.clone()));
			claims
				.entry(dst_id.clone())
				.or_default()
				.push((nearest_side(path[path.len() - 1], dst_info), edge_id.clone()));
			edges[current].points = Some(path);
		}
	}
}

/// `straightenCollinearSiblingDetours`: a three-segment detour between nodes in one row or
/// column becomes a straight line, shifted along the faces when that keeps it clear.
pub(super) fn straighten_collinear_sibling_detours(edges: &mut [Edge], nodes: &[Node]) {
	const EPSILON: f64 = 1e-6;
	const PORT_SHIFT: f64 = 4.0;
	const LABEL_CLEARANCE_BUFFER: f64 = 3.0;
	let (info_by_id, rects) = real_node_bounds(nodes);
	let mut label_size: HashMap<&str, (f64, f64)> = HashMap::new();
	for node in nodes.iter().filter(|node| !node.is_group && node.is_edge_label) {
		label_size
			.insert(node.id.as_str(), (node.width.unwrap_or(0.0), node.height.unwrap_or(0.0)));
	}
	let same_pair = |a: &Edge, b: &Edge| {
		(a.start == b.start && a.end == b.end) || (a.start == b.end && a.end == b.start)
	};
	for current in 0..edges.len() {
		let edge = &edges[current];
		if edge.is_layout_only
			|| classify_three_segments(edge.points.as_deref().unwrap_or(&[]), EPSILON).is_none()
		{
			continue;
		}
		let Some(pair) = geometry::node_pair(edge, &info_by_id, EPSILON) else {
			continue;
		};
		if pair.collinear_x == pair.collinear_y {
			continue;
		}
		let (src, dst) = (pair.src, pair.dst);
		let (target_src, target_dst) = match pair.collinear_x {
			true => {
				let below = dst.cy > src.cy;
				(
					Point { x: src.cx, y: if below { src.rect.bottom } else { src.rect.top } },
					Point { x: dst.cx, y: if below { dst.rect.top } else { dst.rect.bottom } },
				)
			}
			false => {
				let east = dst.cx > src.cx;
				(
					Point { x: if east { src.rect.right } else { src.rect.left }, y: src.cy },
					Point { x: if east { dst.rect.left } else { dst.rect.right }, y: dst.cy },
				)
			}
		};
		let exclude = [edge.start.as_str(), edge.end.as_str()];
		if segment_hits_any_rect(target_src, target_dst, &rects, &exclude, 1.0) {
			continue;
		}
		// A label of this edge or of an anti-parallel sibling widens the shift.
		let mut largest_half = 0.0f64;
		let mut consider = |label: Option<&String>| {
			if let Some(&(width, height)) = label.and_then(|label| label_size.get(label.as_str())) {
				largest_half =
					largest_half.max(if pair.collinear_x { width / 2.0 } else { height / 2.0 });
			}
		};
		consider(edge.label_node_id.as_ref());
		for (index, other) in edges.iter().enumerate() {
			if index != current
				&& !other.is_layout_only
				&& !other.start.is_empty()
				&& !other.end.is_empty()
				&& same_pair(edge, other)
			{
				consider(other.label_node_id.as_ref());
			}
		}
		let label_shift =
			if largest_half > 0.0 { largest_half + LABEL_CLEARANCE_BUFFER } else { 0.0 };
		let shift = if label_shift > PORT_SHIFT { label_shift } else { PORT_SHIFT };
		let mut replacement = None;
		for delta in [0.0, shift, -shift] {
			let (mut shifted_src, mut shifted_dst) = (target_src, target_dst);
			match pair.collinear_x {
				true => {
					shifted_src.x += delta;
					shifted_dst.x += delta;
					if shifted_src.x <= src.rect.left
						|| shifted_src.x >= src.rect.right
						|| shifted_dst.x <= dst.rect.left
						|| shifted_dst.x >= dst.rect.right
					{
						continue;
					}
				}
				false => {
					shifted_src.y += delta;
					shifted_dst.y += delta;
					if shifted_src.y <= src.rect.top
						|| shifted_src.y >= src.rect.bottom
						|| shifted_dst.y <= dst.rect.top
						|| shifted_dst.y >= dst.rect.bottom
					{
						continue;
					}
				}
			}
			if segment_hits_any_rect(shifted_src, shifted_dst, &rects, &exclude, 1.0)
				|| geometry::segment_conflicts_with_any_edge(
					shifted_src,
					shifted_dst,
					edges,
					Some(current),
					EPSILON,
					false,
				) {
				continue;
			}
			replacement = Some(vec![shifted_src, shifted_dst]);
			break;
		}
		if let Some(points) = replacement {
			edges[current].points = Some(points);
		}
	}
}

/// `portSwapToLShape`: a Z-shaped route between nodes in different rows and columns leaves the
/// source from the perpendicular face instead when the L that results is clear.
pub(super) fn port_swap_to_l_shape(edges: &mut [Edge], nodes: &[Node]) {
	const EPSILON: f64 = 1e-6;
	const PORT_SHIFT: f64 = 8.0;
	const DELTAS: [f64; 5] = [0.0, PORT_SHIFT, -PORT_SHIFT, 2.0 * PORT_SHIFT, -2.0 * PORT_SHIFT];
	let (info_by_id, rects) = real_node_bounds(nodes);
	for current in 0..edges.len() {
		let edge = &edges[current];
		if edge.is_layout_only {
			continue;
		}
		let Some(points) = edge.points.as_ref().filter(|points| points.len() >= 4) else {
			continue;
		};
		let route = geometry::dedupe(points, EPSILON);
		let Some(kind) = classify_three_segments(&route, EPSILON) else {
			continue;
		};
		let p3 = route[3];
		let Some(pair) = geometry::node_pair(edge, &info_by_id, EPSILON) else {
			continue;
		};
		if pair.collinear_x || pair.collinear_y {
			continue;
		}
		let (src, dst) = (pair.src, pair.dst);
		let rect = src.rect;
		let mut replacement = None;
		for delta in DELTAS {
			let (np0, np1) = match kind {
				geometry::ThreeSegmentKind::HorizontalVerticalHorizontal => {
					let y = if dst.cy > src.cy { rect.bottom } else { rect.top };
					let x = src.cx + delta;
					if x <= rect.left + EPSILON || x >= rect.right - EPSILON {
						continue;
					}
					(Point { x, y }, Point { x, y: p3.y })
				}
				geometry::ThreeSegmentKind::VerticalHorizontalVertical => {
					let x = if dst.cx > src.cx { rect.right } else { rect.left };
					let y = src.cy + delta;
					if y <= rect.top + EPSILON || y >= rect.bottom - EPSILON {
						continue;
					}
					(Point { x, y }, Point { x: p3.x, y })
				}
			};
			let np2 = p3;
			let first_degenerate = geometry::same_point(np0, np1, EPSILON);
			let second_degenerate = geometry::same_point(np1, np2, EPSILON);
			if first_degenerate && second_degenerate {
				continue;
			}
			if !first_degenerate
				&& segment_hits_any_rect(np0, np1, &rects, &[edge.start.as_str()], 1.0)
			{
				continue;
			}
			if !second_degenerate
				&& segment_hits_any_rect(np1, np2, &rects, &[edge.end.as_str()], 1.0)
			{
				continue;
			}
			let conflicts = |a: Point, b: Point| {
				geometry::segment_conflicts_with_any_edge(a, b, edges, Some(current), EPSILON, true)
			};
			if (!first_degenerate && conflicts(np0, np1))
				|| (!second_degenerate && conflicts(np1, np2))
			{
				continue;
			}
			replacement = Some(match (first_degenerate, second_degenerate) {
				(true, _) => vec![np1, np2],
				(_, true) => vec![np0, np1],
				_ => vec![np0, np1, np2],
			});
			break;
		}
		if let Some(points) = replacement {
			edges[current].points = Some(points);
		}
	}
}
