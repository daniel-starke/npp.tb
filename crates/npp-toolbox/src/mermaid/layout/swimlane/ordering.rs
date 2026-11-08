//! Node order within the layers (`phase3.ordering.ts`: lane-aware median sweeps and adjacent
//! transpositions) and the automatic lane order (`laneOrdering.ts`).

use std::collections::HashMap;

use super::layering::count_inversions;
use super::{EdgeRef, Graph, Layout, js_sort};
use crate::mermaid::common::locale_compare;

#[derive(Clone, Copy, PartialEq)]
enum Sweep {
	Down,
	Up,
}

/// `buildLayerIndex`.
fn layer_index(layer: &[usize]) -> HashMap<usize, usize> {
	layer.iter().enumerate().map(|(index, &node)| (node, index)).collect()
}

fn median(values: &[usize]) -> f64 {
	if values.is_empty() {
		return f64::INFINITY;
	}
	let mut sorted = values.to_vec();
	sorted.sort_unstable();
	let count = sorted.len();
	match count % 2 {
		1 => sorted[(count - 1) / 2] as f64,
		_ => 0.5 * (sorted[count / 2 - 1] + sorted[count / 2]) as f64,
	}
}

fn barycenter(values: &[usize]) -> f64 {
	if values.is_empty() {
		return f64::INFINITY;
	}
	values.iter().map(|&value| value as f64).sum::<f64>() / values.len() as f64
}

/// `countCrossingsBetweenAdjacent` of `phase3.ordering.ts`.
fn count_crossings(upper: &[usize], lower: &[usize], edges: &[&EdgeRef]) -> usize {
	let upper_index = layer_index(upper);
	let lower_index = layer_index(lower);
	let mut pairs: Vec<(usize, usize)> = edges
		.iter()
		.filter_map(|edge| Some((*upper_index.get(&edge.source)?, *lower_index.get(&edge.target)?)))
		.collect();
	pairs.sort();
	let mut values: Vec<usize> = pairs.into_iter().map(|(_, lower)| lower).collect();
	count_inversions(&mut values)
}

/// `sortByHeuristic`: by the median neighbour position, nodes without neighbours last, ties in
/// the current order.
fn sort_by_median(
	graph: &Graph,
	nodes: &[usize],
	neighbours: &HashMap<usize, Vec<usize>>,
	current: &HashMap<usize, usize>,
) -> Vec<usize> {
	let score = |node: usize| median(neighbours.get(&node).map_or(&[][..], Vec::as_slice));
	let mut sorted = nodes.to_vec();
	crate::mermaid::common::v8_sort(&mut sorted, |&a, &b| {
		let (score_a, score_b) = (score(a), score(b));
		if score_a == score_b {
			let (index_a, index_b) =
				(current.get(&a).copied().unwrap_or(0), current.get(&b).copied().unwrap_or(0));
			return match index_a == index_b {
				true => ordering_value(graph.compare_ids(a, b)),
				false => index_a as f64 - index_b as f64,
			};
		}
		if !score_a.is_finite() {
			return 1.0;
		}
		if !score_b.is_finite() {
			return -1.0;
		}
		score_a - score_b
	});
	sorted
}

fn ordering_value(ordering: std::cmp::Ordering) -> f64 {
	ordering as i8 as f64
}

/// `reorderLayer`: nodes sorted by median within their lanes in lane order; nodes without a lane
/// (dummies) are inserted before the first node with a larger barycenter.
fn reorder_layer(
	graph: &Graph,
	fixed: &[usize],
	target: &[usize],
	sweep: Sweep,
	lane_order: &[usize],
) -> Vec<usize> {
	let fixed_index = layer_index(fixed);
	let current = layer_index(target);
	let mut neighbours: HashMap<usize, Vec<usize>> =
		target.iter().map(|&node| (node, Vec::new())).collect();
	for edge in &graph.edges {
		let (fixed_end, moving_end) = match sweep {
			Sweep::Down => (edge.source, edge.target),
			Sweep::Up => (edge.target, edge.source),
		};
		if let (Some(&position), Some(list)) =
			(fixed_index.get(&fixed_end), neighbours.get_mut(&moving_end))
		{
			list.push(position);
		}
	}
	if lane_order.is_empty() {
		return sort_by_median(graph, target, &neighbours, &current);
	}
	let mut result = Vec::with_capacity(target.len());
	for &lane in lane_order {
		let in_lane: Vec<usize> =
			target.iter().copied().filter(|&node| graph.lane_of[node] == Some(lane)).collect();
		if !in_lane.is_empty() {
			result.extend(sort_by_median(graph, &in_lane, &neighbours, &current));
		}
	}
	let without_lane: Vec<usize> =
		target.iter().copied().filter(|&node| graph.lane_of[node].is_none()).collect();
	if without_lane.is_empty() {
		return result;
	}
	let center = |node: usize| barycenter(neighbours.get(&node).map_or(&[][..], Vec::as_slice));
	for node in sort_by_median(graph, &without_lane, &neighbours, &current) {
		let own = center(node);
		let position = match own.is_finite() {
			true => result.iter().position(|&placed| own < center(placed)).unwrap_or(result.len()),
			false => result.len(),
		};
		result.insert(position, node);
	}
	result
}

/// `transposeImprove`: swaps neighbours of the same lane while that reduces the crossings with
/// the fixed layer and the layer beyond.
fn transpose_improve(
	graph: &Graph,
	upper: &[usize],
	current: &[usize],
	next: Option<&[usize]>,
) -> Vec<usize> {
	let mut best = current.to_vec();
	let in_upper: std::collections::HashSet<usize> = upper.iter().copied().collect();
	let in_layer: std::collections::HashSet<usize> = current.iter().copied().collect();
	let in_next: Option<std::collections::HashSet<usize>> =
		next.map(|next| next.iter().copied().collect());
	let edges_in: Vec<&EdgeRef> = graph
		.edges
		.iter()
		.filter(|edge| in_upper.contains(&edge.source) && in_layer.contains(&edge.target))
		.collect();
	let edges_out: Option<Vec<&EdgeRef>> = in_next.as_ref().map(|in_next| {
		graph
			.edges
			.iter()
			.filter(|edge| in_layer.contains(&edge.source) && in_next.contains(&edge.target))
			.collect()
	});
	let score = |order: &[usize]| {
		let mut score = count_crossings(upper, order, &edges_in);
		if let (Some(edges_out), Some(next)) = (&edges_out, next) {
			score += count_crossings(order, next, edges_out);
		}
		score
	};
	let mut best_score = score(&best);
	let mut improved = true;
	while improved {
		improved = false;
		for index in 0..best.len().saturating_sub(1) {
			if graph.lane_of[best[index]] != graph.lane_of[best[index + 1]] {
				continue;
			}
			best.swap(index, index + 1);
			let next_score = score(&best);
			if next_score < best_score {
				best_score = next_score;
				improved = true;
			} else {
				best.swap(index, index + 1);
			}
		}
	}
	best
}

/// `resolveTopLaneOrder`: the preferred lanes first, then the other lanes in source order.
fn resolve_top_lane_order(graph: &Graph, preferred: Option<&[usize]>) -> Vec<usize> {
	let source = &graph.source_lane_order;
	let Some(preferred) = preferred.filter(|preferred| !preferred.is_empty()) else {
		return source.clone();
	};
	let mut resolved: Vec<usize> = Vec::new();
	for &lane in preferred {
		if source.contains(&lane) && !resolved.contains(&lane) {
			resolved.push(lane);
		}
	}
	for &lane in source {
		if !resolved.contains(&lane) {
			resolved.push(lane);
		}
	}
	resolved
}

pub(super) fn lane_order(graph: &Graph, preferred: Option<&[usize]>) -> Vec<usize> {
	resolve_top_lane_order(graph, preferred)
}

/// `orderLayers`: three down and up sweeps of median ordering plus transpositions.
pub(super) fn order_layers(
	mut layers: Vec<Vec<usize>>,
	graph: &Graph,
	preferred: Option<&[usize]>,
) -> Vec<Vec<usize>> {
	let lane_order = resolve_top_lane_order(graph, preferred);
	for _ in 0..3 {
		for index in 1..layers.len() {
			layers[index] =
				reorder_layer(graph, &layers[index - 1], &layers[index], Sweep::Down, &lane_order);
			layers[index] = transpose_improve(
				graph,
				&layers[index - 1],
				&layers[index],
				layers.get(index + 1).map(Vec::as_slice),
			);
		}
		for index in (0..layers.len().saturating_sub(1)).rev() {
			layers[index] =
				reorder_layer(graph, &layers[index + 1], &layers[index], Sweep::Up, &lane_order);
			let previous = index.checked_sub(1).map(|previous| layers[previous].as_slice());
			layers[index] = transpose_improve(graph, &layers[index + 1], &layers[index], previous);
		}
	}
	layers
}

/// `AUTOMATIC_LANE_ORDERING_RESTARTS`.
const RESTARTS: u32 = 8;

/// FNV-1a over UTF-16 code units (`hashString` of `laneOrdering.ts`).
fn hash_string(input: &str) -> u32 {
	let mut hash: u32 = 2_166_136_261;
	for unit in input.encode_utf16() {
		hash ^= u32::from(unit);
		hash = hash.wrapping_mul(16_777_619);
	}
	hash
}

fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
	let mut state = seed;
	move || {
		state = state.wrapping_add(0x6d2b_79f5);
		let mut t = state;
		t = (t ^ (t >> 15)).wrapping_mul(t | 1);
		t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
		f64::from(t ^ (t >> 14)) / 4_294_967_296.0
	}
}

/// Edge counts between lane pairs, `a` before `b` in source order (`buildWeightedLaneEdges`).
fn weighted_lane_edges(graph: &Graph, layout: &Layout) -> Vec<(usize, usize, usize)> {
	let source = &graph.source_lane_order;
	let source_index = |lane: usize| source.iter().position(|&candidate| candidate == lane);
	let mut weights: Vec<(usize, usize, usize)> = Vec::new();
	for edge in &layout.edges {
		if edge.is_layout_only {
			continue;
		}
		let (Some(&start), Some(&end)) = (graph.by_id.get(&edge.start), graph.by_id.get(&edge.end))
		else {
			continue;
		};
		let (Some(lane_a), Some(lane_b)) = (graph.lane_of[start], graph.lane_of[end]) else {
			continue;
		};
		if lane_a == lane_b {
			continue;
		}
		let (Some(index_a), Some(index_b)) = (source_index(lane_a), source_index(lane_b)) else {
			continue;
		};
		let (a, b) = match index_a <= index_b {
			true => (lane_a, lane_b),
			false => (lane_b, lane_a),
		};
		match weights.iter_mut().find(|(x, y, _)| *x == a && *y == b) {
			Some(entry) => entry.2 += 1,
			None => weights.push((a, b, 1)),
		}
	}
	weights
}

fn arrangement_cost(order: &[usize], weights: &[(usize, usize, usize)]) -> f64 {
	let position = |lane: usize| order.iter().position(|&candidate| candidate == lane);
	weights
		.iter()
		.filter_map(|&(a, b, weight)| {
			Some(weight as f64 * (position(a)? as f64 - position(b)? as f64).abs())
		})
		.sum()
}

/// `greedySwitch`: adjacent swaps while they lower the cost; the cost and the distance from the
/// source order.
fn greedy_switch(
	start: &[usize],
	weights: &[(usize, usize, usize)],
	source: &[usize],
) -> (Vec<usize>, f64, f64) {
	let mut order = start.to_vec();
	let mut cost = arrangement_cost(&order, weights);
	let mut changed = true;
	let mut sweeps = 0;
	while changed && sweeps < order.len().max(1) {
		changed = false;
		sweeps += 1;
		for index in 0..order.len().saturating_sub(1) {
			order.swap(index, index + 1);
			let next_cost = arrangement_cost(&order, weights);
			if next_cost < cost {
				cost = next_cost;
				changed = true;
			} else {
				order.swap(index, index + 1);
			}
		}
	}
	let distance = order
		.iter()
		.enumerate()
		.map(|(index, &lane)| {
			let source_index =
				source.iter().position(|&candidate| candidate == lane).unwrap_or(index);
			(index as f64 - source_index as f64).abs()
		})
		.sum();
	(order, cost, distance)
}

/// `optimizeTopLaneOrder`: the lane order with the shortest weighted cross-lane distances from
/// greedy switching of the source order and of eight seeded shuffles.
pub(super) fn optimize_top_lane_order(graph: &Graph, layout: &Layout) -> Vec<usize> {
	let source = graph.source_lane_order.clone();
	if source.len() < 2 {
		return source;
	}
	let weights = weighted_lane_edges(graph, layout);
	if weights.is_empty() {
		return source;
	}
	let mut best = greedy_switch(&source, &weights, &source);
	let mut signature = weights.clone();
	let name = |lane: usize| graph.lane_names[lane].as_str();
	js_sort(&mut signature, |a, b| match a.0 == b.0 {
		true => locale_compare(name(a.1), name(b.1)),
		false => locale_compare(name(a.0), name(b.0)),
	});
	let signature = signature
		.iter()
		.map(|&(a, b, weight)| format!("{}:{}:{weight}", name(a), name(b)))
		.collect::<Vec<_>>()
		.join("|");
	let source_names = source.iter().map(|&lane| name(lane)).collect::<Vec<_>>().join("|");
	for restart in 0..RESTARTS {
		let mut random = mulberry32(hash_string(&format!("{source_names}#{signature}#{restart}")));
		let mut start = source.clone();
		for index in (1..start.len()).rev() {
			let other = (random() * (index + 1) as f64).floor() as usize;
			start.swap(index, other.min(index));
		}
		let candidate = greedy_switch(&start, &weights, &source);
		let is_better = match candidate.1 != best.1 {
			true => candidate.1 < best.1,
			false => candidate.2 < best.2,
		};
		if is_better {
			best = candidate;
		}
	}
	best.0
}
