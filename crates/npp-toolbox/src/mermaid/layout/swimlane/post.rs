//! Direction transforms and the clean-up pipeline after routing (`postProcessing.ts`,
//! `direction/lrTransform.ts`).

use std::collections::HashMap;

use super::{
	Direction, Layout, Node, NodeMap, TitleRect, detour, endpoints, geometry, labels, materialized,
	nudging,
};

/// Children bounds of nodes with coordinates (`boundsForChildren`).
fn children_bounds<'a>(children: impl Iterator<Item = &'a Node>) -> Option<(f64, f64, f64, f64)> {
	let (mut min_x, mut max_x, mut min_y, mut max_y) =
		(f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
	for child in children {
		let (Some(x), Some(y)) = (child.x, child.y) else { continue };
		let (width, height) = (child.width.unwrap_or(0.0), child.height.unwrap_or(0.0));
		min_x = min_x.min(x - width / 2.0);
		max_x = max_x.max(x + width / 2.0);
		min_y = min_y.min(y - height / 2.0);
		max_y = max_y.max(y + height / 2.0);
	}
	(min_x != f64::INFINITY && min_y != f64::INFINITY).then_some((min_x, max_x, min_y, max_y))
}

/// Ancestors that are groups, outermost last (`resolveTopLevelGroupId`, `groupDepth`).
fn group_ancestors(node: &Node, nodes: &[Node], by_id: &HashMap<String, usize>) -> Vec<usize> {
	let mut ancestors = Vec::new();
	let mut parent = node.parent_id.as_deref();
	while let Some(id) = parent {
		let Some(&index) = by_id.get(id).filter(|&&index| nodes[index].is_group) else {
			break;
		};
		if ancestors.contains(&index) {
			break;
		}
		ancestors.push(index);
		parent = nodes[index].parent_id.as_deref();
	}
	ancestors
}

/// `recomputeNestedGroupBounds`: nested groups (deepest first) wrap their children again.
fn recompute_nested_group_bounds(nodes: &mut [Node]) {
	let by_id = NodeMap::of(nodes).by_id;
	let mut groups: Vec<(usize, usize)> = (0..nodes.len())
		.filter(|&index| nodes[index].is_group && nodes[index].parent_id.is_some())
		.map(|index| (index, group_ancestors(&nodes[index], nodes, &by_id).len()))
		.collect();
	npp::sort::sort_by_key(&mut groups[..], |group| std::cmp::Reverse(group.1));
	for (group, _) in groups {
		let id = nodes[group].id.clone();
		let Some((min_x, max_x, min_y, max_y)) = children_bounds(
			nodes.iter().filter(|node| node.parent_id.as_deref() == Some(id.as_str())),
		) else {
			continue;
		};
		let node = &mut nodes[group];
		let padding = node.padding.unwrap_or(20.0);
		node.x = Some((min_x + max_x) / 2.0);
		node.y = Some((min_y + max_y) / 2.0);
		node.width = Some((max_x - min_x).max(0.0) + padding);
		node.height = Some((max_y - min_y).max(0.0) + padding);
	}
}

/// `mirrorAxis`: mirrors x (or y) around the extent of the content nodes.
fn mirror_axis(layout: &mut Layout, along_x: bool) -> bool {
	let value = |node: &Node| if along_x { node.x } else { node.y };
	let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
	for node in layout.nodes.iter().filter(|node| !node.is_group) {
		if let Some(value) = value(node) {
			min = min.min(value);
			max = max.max(value);
		}
	}
	if !min.is_finite() || !max.is_finite() {
		return false;
	}
	let mirror = |value: f64| min + max - value;
	for node in &mut layout.nodes {
		match along_x {
			true => node.x = node.x.map(mirror),
			false => node.y = node.y.map(mirror),
		}
		if let Some(rect) = node.title_rect {
			node.title_rect = Some(match along_x {
				true => TitleRect { left: mirror(rect.right), right: mirror(rect.left), ..rect },
				false => TitleRect { top: mirror(rect.bottom), bottom: mirror(rect.top), ..rect },
			});
		}
	}
	for point in layout.edges.iter_mut().flat_map(|edge| edge.points.iter_mut().flatten()) {
		match along_x {
			true => point.x = mirror(point.x),
			false => point.y = mirror(point.y),
		}
	}
	true
}

/// `applyLrDirectionTransform`: layers become columns, lanes become rows with a title band
/// on the left.
fn left_right_transform(layout: &mut Layout, right_to_left: bool) -> bool {
	const TITLE_BAND_SIZE: f64 = 36.0;
	let content: Vec<usize> =
		(0..layout.nodes.len()).filter(|&index| !layout.nodes[index].is_group).collect();
	let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
	for &index in &content {
		let node = &layout.nodes[index];
		min_x = min_x.min(node.x.unwrap_or(0.0));
		min_y = min_y.min(node.y.unwrap_or(0.0));
	}
	if !min_x.is_finite() || !min_y.is_finite() {
		return false;
	}
	let (mut total_width, mut total_height) = (0.0, 0.0);
	for &index in &content {
		total_width += layout.nodes[index].width.unwrap_or(0.0);
		total_height += layout.nodes[index].height.unwrap_or(0.0);
	}
	let count = content.len() as f64;
	let (average_width, average_height) = (total_width / count, total_height / count);
	let scale = if average_height > 0.0 { (average_width / average_height).max(1.0) } else { 1.0 };
	let transform = |x: f64, y: f64| ((y - min_y) * scale + TITLE_BAND_SIZE, x - min_x);
	for &index in &content {
		let node = &mut layout.nodes[index];
		let (x, y) = transform(node.x.unwrap_or(0.0), node.y.unwrap_or(0.0));
		node.x = Some(x);
		node.y = Some(y);
	}
	for point in layout.edges.iter_mut().flat_map(|edge| edge.points.iter_mut().flatten()) {
		(point.x, point.y) = transform(point.x, point.y);
	}
	recompute_nested_group_bounds(&mut layout.nodes);
	let lanes: Vec<usize> = (0..layout.nodes.len())
		.filter(|&index| layout.nodes[index].is_group && layout.nodes[index].parent_id.is_none())
		.collect();
	if lanes.is_empty() {
		if right_to_left {
			mirror_axis(layout, true);
		}
		return true;
	}
	let by_id = NodeMap::of(&layout.nodes).by_id;
	let mut children: HashMap<String, Vec<usize>> = HashMap::new();
	for index in 0..layout.nodes.len() {
		let node = &layout.nodes[index];
		if node.is_group {
			continue;
		}
		if let Some(&lane) = group_ancestors(node, &layout.nodes, &by_id).last() {
			children.entry(layout.nodes[lane].id.clone()).or_default().push(index);
		}
	}
	let max_padding =
		lanes.iter().map(|&lane| layout.nodes[lane].padding.unwrap_or(0.0)).fold(0.0, f64::max);
	let mut bounds: Vec<(usize, f64, f64, f64)> = Vec::new();
	let (mut global_min_x, mut global_max_x) = (f64::INFINITY, f64::NEG_INFINITY);
	for &lane in &lanes {
		let members = children.get(&layout.nodes[lane].id).map(Vec::as_slice).unwrap_or(&[]);
		let Some((min_x, max_x, min_y, max_y)) =
			children_bounds(members.iter().map(|&index| &layout.nodes[index]))
		else {
			continue;
		};
		global_min_x = global_min_x.min(min_x);
		global_max_x = global_max_x.max(max_x);
		bounds.push((lane, min_y, max_y, (min_y + max_y) / 2.0));
	}
	if global_min_x == f64::INFINITY || global_max_x == f64::NEG_INFINITY {
		return true;
	}
	let body_width = (global_max_x - global_min_x).max(0.0) + 2.0 * max_padding.max(10.0);
	let lane_width = TITLE_BAND_SIZE + body_width;
	let body_left = (global_min_x + global_max_x) / 2.0 - body_width / 2.0;
	let lane_left = body_left - TITLE_BAND_SIZE;
	let center_x = lane_left + lane_width / 2.0;
	let vertical_margin = max_padding.max(TITLE_BAND_SIZE);
	npp::sort::sort_by(&mut bounds[..], |a, b| {
		a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal)
	});
	for index in 0..bounds.len() {
		let (lane, content_top, content_bottom, _) = bounds[index];
		let top = match index {
			0 => content_top - vertical_margin,
			_ => (bounds[index - 1].2 + content_top) / 2.0,
		};
		let bottom = match bounds.get(index + 1) {
			None => content_bottom + vertical_margin,
			Some(next) => (content_bottom + next.1) / 2.0,
		};
		let node = &mut layout.nodes[lane];
		node.x = Some(center_x);
		node.y = Some((top + bottom) / 2.0);
		node.width = Some(lane_width);
		node.height = Some((bottom - top).max(0.0));
		node.content_top = Some(content_top);
		node.title_rect =
			Some(TitleRect { left: lane_left, right: lane_left + TITLE_BAND_SIZE, top, bottom });
	}
	if right_to_left {
		mirror_axis(layout, true);
	}
	true
}

/// `postProcessSwimlaneLayout`.
pub(super) fn post_process(layout: &mut Layout) {
	let direction = layout.direction;
	let has_content = layout.nodes.iter().any(|node| !node.is_group);
	match direction {
		Direction::LeftRight | Direction::RightLeft if has_content => {
			if !left_right_transform(layout, direction == Direction::RightLeft) {
				return;
			}
		}
		Direction::BottomTop if has_content && !mirror_axis(layout, false) => {
			return;
		}
		_ => {}
	}
	for edge in layout.edges.iter_mut().filter(|edge| !edge.is_layout_only) {
		if let Some(points) = edge.points.as_mut().filter(|points| points.len() >= 2) {
			*points = geometry::simplify(&geometry::orthogonalize(points));
		}
	}
	let Layout { nodes, edges, .. } = layout;
	detour::simplify_detoured_edges(edges, nodes);
	detour::straighten_collinear_sibling_detours(edges, nodes);
	detour::port_swap_to_l_shape(edges, nodes);
	let map = NodeMap::of(nodes);
	labels::anchor_labels_to_polyline(edges, nodes, &map);
	endpoints::clip_edge_endpoints_to_node_boundaries(edges, nodes, &map);
	nudging::collapse_short_terminal_stub(edges, nodes, &map);
	nudging::nudge_shared_interior_subpaths(edges, nodes, &map);
	materialized::separate_shared_rendered_terminal_lanes(edges, nodes, &map);
	materialized::collapse_redundant_rectangular_doglegs(edges, nodes, &map);
	materialized::lift_obstacle_hugging_same_side_rails(edges, nodes, &map);
	materialized::swap_destination_terminal_tails_to_reduce_crossings(edges, nodes, &map);
	let finalize = |edges: &mut [super::Edge], nodes: &mut [Node]| {
		materialized::resolve_rendered_orthogonal_crossings(edges, nodes, &map);
		materialized::separate_shared_rendered_terminal_lanes(edges, nodes, &map);
		materialized::reassign_crossing_external_rail_channels(edges, nodes, &map);
		materialized::shortcut_redundant_orthogonal_jogs(edges, nodes, &map);
		labels::anchor_labels_to_polyline(edges, nodes, &map);
		endpoints::prepare_edge_endpoints_for_renderer(edges, nodes, &map);
		materialized::lift_obstacle_hugging_same_side_rails(edges, nodes, &map);
		labels::anchor_labels_to_polyline(edges, nodes, &map);
		endpoints::prepare_edge_endpoints_for_renderer(edges, nodes, &map);
	};
	finalize(edges, nodes);
	nudging::nudge_shared_interior_subpaths(edges, nodes, &map);
	finalize(edges, nodes);
	for _ in 0..2 {
		materialized::lift_top_lane_title_bands_above_rails(edges, nodes, &map, direction);
		materialized::shift_left_lane_title_bands_left_of_rails(edges, nodes, &map, direction);
	}
}
