//! Layer assignment of the swimlane layout (`phase2.laneAwareCompact.ts`,
//! `phase2.longestPath.ts`, `phase2.gravity.ts`, `phase2.crossOptimization.ts`,
//! `phase2.multitree.order.ts`, `phase2.multitree.core.ts`, `phase2.crossCounts.ts`,
//! `driving-tree.ts`, `phase2.dummies.ts`).

use std::cmp::Ordering;
use std::collections::HashMap;

use super::{Direction, EdgeRef, Graph, GraphNode, Layering, js_sort};
use crate::mermaid::color::js_round;
use crate::mermaid::common::locale_compare;

/// `LAYERING.GRAVITY_ITERATIONS`.
const GRAVITY_ITERATIONS: usize = 8;
/// `LAYERING.MAX_CROSSING_OPTIMIZATION_PASSES`.
const MAX_CROSSING_OPTIMIZATION_PASSES: usize = 4;

/// `laneOf` of the compact layering: the node's lane, or its own id without one.
fn lane_key(graph: &Graph, node: usize) -> &str {
	match graph.lane_of[node] {
		Some(lane) => &graph.lane_names[lane],
		None => graph.id(node),
	}
}

/// `topoSortByGenerationIfAcyclic`: Kahn by generations, each generation sorted by id.
fn topological_order_by_generation(graph: &Graph) -> Option<Vec<usize>> {
	let mut degrees = graph.in_degrees();
	let successors = graph.sorted_successors();
	let mut frontier = graph.sorted_sources(&degrees);
	let mut order = Vec::with_capacity(graph.nodes.len());
	while !frontier.is_empty() {
		let mut next_frontier = Vec::new();
		for &node in &frontier {
			order.push(node);
			for &next in &successors[node] {
				degrees[next] -= 1;
				if degrees[next] == 0 {
					next_frontier.push(next);
				}
			}
		}
		js_sort(&mut next_frontier, |&a, &b| graph.compare_ids(a, b));
		frontier = next_frontier;
	}
	(order.len() == graph.nodes.len()).then_some(order)
}

/// `assignLayers_LaneAwareCompact` with cross-lane edges ignored: one node per layer and lane,
/// a node follows its same-lane predecessors.
pub(super) fn lane_aware_compact(graph: &Graph, direction: Direction) -> Layering {
	let order = match direction {
		Direction::LeftRight => topological_order_by_generation(graph),
		_ => graph.topological_order(),
	}
	.unwrap_or_else(|| graph.nodes_by_code_units());
	let mut ranks: Vec<Option<i64>> = vec![None; graph.arena.len()];
	let mut next_free: HashMap<&str, i64> = HashMap::new();
	for &node in &order {
		if graph.arena[node].is_group {
			continue;
		}
		let lane = lane_key(graph, node);
		let mut base = 0;
		for edge in graph.incoming(node) {
			let weight = i64::from(lane_key(graph, edge.source) == lane);
			base = base.max(ranks[edge.source].unwrap_or(0) + weight);
		}
		let rank = base.max(next_free.get(lane).copied().unwrap_or(0));
		ranks[node] = Some(rank);
		next_free.insert(lane, rank + 1);
	}
	let layers = graph.layers_from_ranks(&order, &ranks, true);
	Layering { layers, ranks }
}

/// `assignLayers_LongestPath` as `assignLayers_Gravity` calls it: all edges count, a node with
/// one predecessor in another lane shares its layer. Its layer order is not used.
fn longest_path(graph: &Graph, optimize_ranks_by_crossings: bool) -> Vec<Option<i64>> {
	let order = graph.topological_order().unwrap_or_else(|| graph.nodes_by_code_units());
	let mut ranks: Vec<Option<i64>> = vec![None; graph.arena.len()];
	for &node in &order {
		let incoming: Vec<&EdgeRef> = graph.incoming(node).collect();
		ranks[node] = Some(match incoming.as_slice() {
			[] => 0,
			[edge] => {
				let rank = ranks[edge.source].unwrap_or(0);
				match graph.lane_of[edge.source] != graph.lane_of[node] {
					true => rank,
					false => rank + 1,
				}
			}
			edges => {
				edges.iter().map(|edge| ranks[edge.source].unwrap_or(0) + 1).max().unwrap_or(0)
			}
		});
	}
	if optimize_ranks_by_crossings {
		optimize_ranks(graph, &mut ranks);
	}
	ranks
}

/// `assignLayers_Gravity` (cross-lane edges respected): longest-path ranks relaxed towards the
/// average of the neighbours.
pub(super) fn gravity(graph: &Graph, optimize_ranks_by_crossings: bool) -> Layering {
	let mut ranks = longest_path(graph, optimize_ranks_by_crossings);
	let mut predecessors = vec![Vec::new(); graph.arena.len()];
	let mut successors = vec![Vec::new(); graph.arena.len()];
	for edge in &graph.edges {
		successors[edge.source].push(edge.target);
		predecessors[edge.target].push(edge.source);
	}
	let order = graph.topological_order().unwrap_or_else(|| graph.nodes.clone());
	let reversed: Vec<usize> = order.iter().rev().copied().collect();
	let rank = |ranks: &[Option<i64>], node: usize| ranks[node].unwrap_or(0);
	let lower_bound = |ranks: &[Option<i64>], node: usize| {
		predecessors[node].iter().map(|&predecessor| rank(ranks, predecessor) + 1).fold(0, i64::max)
	};
	let upper_bound = |ranks: &[Option<i64>], node: usize| {
		successors[node].iter().map(|&successor| rank(ranks, successor) - 1).min()
	};
	let relax = |ranks: &mut Vec<Option<i64>>, nodes: &[usize]| {
		let mut changed = false;
		for &node in nodes {
			let (preds, succs) = (&predecessors[node], &successors[node]);
			if preds.is_empty() && succs.is_empty() {
				continue;
			}
			let own = rank(ranks, node) as f64;
			let predecessor_average = match preds.len() {
				0 => own,
				count => {
					preds.iter().map(|&p| (rank(ranks, p) + 1) as f64).sum::<f64>() / count as f64
				}
			};
			let successor_average = match succs.len() {
				0 => own,
				count => {
					succs.iter().map(|&s| (rank(ranks, s) - 1) as f64).sum::<f64>() / count as f64
				}
			};
			let desired = js_round((predecessor_average + successor_average) / 2.0) as i64;
			let lower = lower_bound(ranks, node);
			let upper = upper_bound(ranks, node).unwrap_or(lower.max(desired));
			let clamped = desired.max(lower).min(upper);
			if ranks[node] != Some(clamped) {
				ranks[node] = Some(clamped);
				changed = true;
			}
		}
		changed
	};
	for _ in 0..GRAVITY_ITERATIONS {
		let forward = relax(&mut ranks, &order);
		let backward = relax(&mut ranks, &reversed);
		if !forward && !backward {
			break;
		}
	}
	for &node in &order {
		let lower = lower_bound(&ranks, node);
		if rank(&ranks, node) < lower {
			ranks[node] = Some(lower);
		}
	}
	for &node in &reversed {
		if let Some(upper) = upper_bound(&ranks, node)
			&& rank(&ranks, node) > upper
		{
			ranks[node] = Some(upper);
		}
	}
	let layers = graph.layers_from_ranks(&order, &ranks, false);
	Layering { layers, ranks }
}

/// `countInversions` (merge sort).
pub(super) fn count_inversions(values: &mut [usize]) -> usize {
	if values.len() <= 1 {
		return 0;
	}
	let middle = values.len() / 2;
	let mut inversions =
		count_inversions(&mut values[..middle]) + count_inversions(&mut values[middle..]);
	let mut merged = Vec::with_capacity(values.len());
	let (mut left, mut right) = (0, middle);
	while left < middle || right < values.len() {
		if right >= values.len() || (left < middle && values[left] <= values[right]) {
			merged.push(values[left]);
			left += 1;
		} else {
			merged.push(values[right]);
			right += 1;
			inversions += middle - left;
		}
	}
	values.copy_from_slice(&merged);
	inversions
}

/// `totalCrossings` of `phase2.crossOptimization.ts`: edges between adjacent layers, one copy per
/// spanned layer, counted in edge order without sorting by the upper position.
fn total_crossings(graph: &Graph, layers: &[Vec<usize>], ranks: &[Option<i64>]) -> usize {
	let mut expanded = Vec::new();
	for edge in &graph.edges {
		let (Some(source_rank), Some(target_rank)) = (ranks[edge.source], ranks[edge.target])
		else {
			continue;
		};
		if source_rank == target_rank {
			continue;
		}
		let (upper, lower, upper_rank, lower_rank) = match source_rank > target_rank {
			true => (edge.target, edge.source, target_rank, source_rank),
			false => (edge.source, edge.target, source_rank, target_rank),
		};
		for _ in upper_rank..lower_rank {
			expanded.push((upper, lower));
		}
	}
	let mut position = vec![None; graph.arena.len()];
	let mut in_upper = vec![false; graph.arena.len()];
	let mut sum = 0;
	for pair in layers.windows(2) {
		for &node in &pair[0] {
			in_upper[node] = true;
		}
		for (index, &node) in pair[1].iter().enumerate() {
			// `buildLayerIndex` keeps the last position of a repeated id.
			position[node] = Some(index);
		}
		let mut values: Vec<usize> = expanded
			.iter()
			.filter(|&&(upper, _)| in_upper[upper])
			.filter_map(|&(_, lower)| position[lower])
			.collect();
		sum += count_inversions(&mut values);
		for &node in &pair[0] {
			in_upper[node] = false;
		}
		for &node in &pair[1] {
			position[node] = None;
		}
	}
	sum
}

/// `optimizeRanksByCrossings`: greedily lifts nodes to their lowest feasible rank when that
/// reduces the crossings of the multitree layer order.
fn optimize_ranks(graph: &Graph, ranks: &mut [Option<i64>]) {
	let mut predecessors = vec![Vec::new(); graph.arena.len()];
	for edge in &graph.edges {
		predecessors[edge.target].push(edge.source);
	}
	let layers = multitree_layer_order(graph, ranks);
	let mut best = total_crossings(graph, &layers, ranks);
	for _ in 0..MAX_CROSSING_OPTIMIZATION_PASSES {
		let mut changed = false;
		let mut by_rank = graph.nodes.clone();
		js_sort(&mut by_rank, |&a, &b| ranks[b].unwrap_or(0).cmp(&ranks[a].unwrap_or(0)));
		for node in by_rank {
			let rank = ranks[node].unwrap_or(0);
			if rank == 0 {
				continue;
			}
			let lower =
				predecessors[node].iter().map(|&p| ranks[p].unwrap_or(0) + 1).fold(0, i64::max);
			if lower >= rank {
				continue;
			}
			let old = ranks[node];
			ranks[node] = Some(lower);
			let trial = multitree_layer_order(graph, ranks);
			let score = total_crossings(graph, &trial, ranks);
			if score < best {
				best = score;
				changed = true;
			} else {
				ranks[node] = old;
			}
		}
		if !changed {
			break;
		}
	}
}

/// The spanning forest of `buildDrivingTree` as far as the multitree order uses it.
struct DrivingTree {
	parent: Vec<Option<usize>>,
	children: Vec<Vec<usize>>,
	roots: Vec<usize>,
}

/// `buildDrivingTree`: every node hangs below its preferred predecessor (same lane, then the
/// highest rank, then the topological position, then the id).
fn driving_tree(graph: &Graph, ranks: &[Option<i64>]) -> DrivingTree {
	let mut predecessors = vec![Vec::new(); graph.arena.len()];
	for edge in &graph.edges {
		predecessors[edge.target].push(edge.source);
	}
	for list in &mut predecessors {
		js_sort(list, |&a, &b| graph.compare_ids(a, b));
	}
	let order = graph.topological_order().unwrap_or_else(|| {
		let mut nodes = graph.nodes.clone();
		js_sort(&mut nodes, |&a, &b| graph.compare_ids(a, b));
		nodes
	});
	let mut topological_index = vec![0usize; graph.arena.len()];
	for (index, &node) in order.iter().enumerate() {
		topological_index[node] = index;
	}
	let mut has_parent_entry = vec![false; graph.arena.len()];
	let mut parent = vec![None; graph.arena.len()];
	let mut children = vec![Vec::new(); graph.arena.len()];
	for &node in &order {
		let mut candidates: Vec<usize> =
			predecessors[node].iter().copied().filter(|&p| has_parent_entry[p]).collect();
		if candidates.is_empty() {
			has_parent_entry[node] = true;
			continue;
		}
		let lane = graph.lane_of[node];
		js_sort(&mut candidates, |&a, &b| {
			let same_a = graph.lane_of[a].is_some() && graph.lane_of[a] == lane;
			let same_b = graph.lane_of[b].is_some() && graph.lane_of[b] == lane;
			if same_a != same_b {
				return match same_a {
					true => Ordering::Less,
					false => Ordering::Greater,
				};
			}
			if let (Some(rank_a), Some(rank_b)) = (ranks[a], ranks[b])
				&& rank_a != rank_b
			{
				return rank_b.cmp(&rank_a);
			}
			topological_index[a].cmp(&topological_index[b]).then_with(|| graph.compare_ids(a, b))
		});
		let chosen = candidates[0];
		has_parent_entry[node] = true;
		parent[node] = Some(chosen);
		children[chosen].push(node);
	}
	let mut roots: Vec<usize> =
		graph.nodes.iter().copied().filter(|&node| parent[node].is_none()).collect();
	js_sort(&mut roots, |&a, &b| {
		topological_index[a].cmp(&topological_index[b]).then_with(|| graph.compare_ids(a, b))
	});
	DrivingTree { parent, children, roots }
}

/// A preorder of the forest from the given roots, without recursion.
fn preorder(roots: &[usize], children: &[Vec<usize>]) -> Vec<usize> {
	let mut order = Vec::new();
	let mut stack: Vec<usize> = roots.iter().rev().copied().collect();
	while let Some(node) = stack.pop() {
		order.push(node);
		stack.extend(children[node].iter().rev());
	}
	order
}

/// `computeSubtreeCrossCounts`: for each tree edge, how many edges below the parent's subtree
/// cross the parent's layer (binary lifting LCA over the forest).
fn subtree_cross_counts(
	graph: &Graph,
	ranks: &[Option<i64>],
	tree: &DrivingTree,
) -> HashMap<(usize, usize), i64> {
	let count = graph.arena.len();
	let mut parent_of = vec![None; count];
	let mut depth = vec![0usize; count];
	let mut seen = vec![false; count];
	let mut queue = std::collections::VecDeque::new();
	for &node in &graph.nodes {
		if tree.parent[node].is_none() && !seen[node] {
			seen[node] = true;
			queue.push_back(node);
		}
	}
	while let Some(current) = queue.pop_front() {
		for &child in &tree.children[current] {
			if seen[child] {
				continue;
			}
			parent_of[child] = Some(current);
			depth[child] = depth[current] + 1;
			seen[child] = true;
			queue.push_back(child);
		}
	}
	let node_count = graph.nodes.len().max(1);
	let levels = ((node_count as f64).log2().ceil() as usize + 1).max(1);
	let mut up = vec![parent_of.clone()];
	for level in 1..levels {
		let previous = &up[level - 1];
		let next: Vec<Option<usize>> =
			previous.iter().map(|&p| p.and_then(|p| previous[p])).collect();
		up.push(next);
	}
	let lowest_common_ancestor = |mut a: usize, mut b: usize| -> Option<usize> {
		if depth[a] < depth[b] {
			std::mem::swap(&mut a, &mut b);
		}
		let difference = depth[a] - depth[b];
		for (level, row) in up.iter().enumerate() {
			if (difference >> level) & 1 == 1 {
				a = row[a]?;
			}
		}
		if a == b {
			return Some(a);
		}
		for row in up.iter().rev() {
			if let (Some(up_a), Some(up_b)) = (row[a], row[b])
				&& up_a != up_b
			{
				a = up_a;
				b = up_b;
			}
		}
		up[0][a]
	};
	let mut own_counts: Vec<HashMap<i64, i64>> = vec![HashMap::new(); count];
	for edge in &graph.edges {
		let (Some(mut upper_rank), Some(mut lower_rank)) = (ranks[edge.source], ranks[edge.target])
		else {
			continue;
		};
		let (mut upper, mut lower) = (edge.source, edge.target);
		if upper_rank > lower_rank {
			std::mem::swap(&mut upper, &mut lower);
			std::mem::swap(&mut upper_rank, &mut lower_rank);
		}
		if upper_rank == lower_rank {
			continue;
		}
		let Some(ancestor) = lowest_common_ancestor(upper, lower) else {
			continue;
		};
		for layer in upper_rank..lower_rank {
			*own_counts[ancestor].entry(layer).or_insert(0) += 1;
		}
	}
	// Post-order accumulation of the per-layer counts.
	let mut cross_counts = HashMap::new();
	let order = preorder(&tree.roots, &tree.children);
	let mut accumulated: Vec<Option<HashMap<i64, i64>>> = vec![None; count];
	for &node in order.iter().rev() {
		let mut accumulator = std::mem::take(&mut own_counts[node]);
		for &child in &tree.children[node] {
			let child_map = accumulated[child].take().unwrap_or_default();
			if let Some(parent_layer) = ranks[node] {
				let mut value = child_map.get(&parent_layer).copied().unwrap_or(0);
				if ranks[child].is_some_and(|child_layer| child_layer > parent_layer) {
					value += 1;
				}
				cross_counts.insert((node, child), value);
			}
			for (layer, value) in child_map {
				*accumulator.entry(layer).or_insert(0) += value;
			}
		}
		accumulated[node] = Some(accumulator);
	}
	cross_counts
}

/// `buildMultitreeLayerOrder`: layers filled in a preorder of the driving tree whose children
/// are ordered by their subtree crossings.
fn multitree_layer_order(graph: &Graph, ranks: &[Option<i64>]) -> Vec<Vec<usize>> {
	let tree = driving_tree(graph, ranks);
	let cross_counts = subtree_cross_counts(graph, ranks, &tree);
	let rank = |node: usize| ranks[node].unwrap_or(0);
	let compare_by_rank_then_id =
		|&a: &usize, &b: &usize| rank(a).cmp(&rank(b)).then_with(|| graph.compare_ids(a, b));
	let mut roots = tree.roots.clone();
	js_sort(&mut roots, compare_by_rank_then_id);
	// `annotateMinimumLayers`: the minimum rank of every subtree.
	let mut minimum_layer: Vec<Option<i64>> = vec![None; graph.arena.len()];
	for &node in preorder(&roots, &tree.children).iter().rev() {
		let minimum = tree.children[node]
			.iter()
			.filter_map(|&child| minimum_layer[child])
			.fold(rank(node), i64::min);
		minimum_layer[node] = Some(minimum);
	}
	let order_children = |node: usize| -> Vec<usize> {
		let layer = rank(node);
		let minimum = |child: usize| minimum_layer[child].unwrap_or(layer);
		let (mut future, mut present): (Vec<usize>, Vec<usize>) =
			tree.children[node].iter().partition(|&&child| minimum(child) > layer);
		js_sort(&mut future, |&a, &b| {
			minimum(a).cmp(&minimum(b)).then_with(|| graph.compare_ids(a, b))
		});
		let crossings = |child: usize| cross_counts.get(&(node, child)).copied().unwrap_or(0);
		js_sort(&mut present, |&a, &b| {
			crossings(a)
				.cmp(&crossings(b))
				.then_with(|| minimum(a).cmp(&minimum(b)))
				.then_with(|| graph.compare_ids(a, b))
		});
		future.extend(present);
		future
	};
	let max_rank = graph.nodes.iter().map(|&node| rank(node)).fold(0, i64::max);
	let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank as usize + 1];
	let mut emitted = vec![false; graph.arena.len()];
	let push = |layers: &mut Vec<Vec<usize>>, node: usize| {
		// A negative rank is an object property in JavaScript, not a layer.
		if let Ok(layer) = usize::try_from(rank(node)) {
			layers[layer].push(node);
		}
	};
	for &root in &roots {
		if emitted[root] {
			continue;
		}
		emitted[root] = true;
		push(&mut layers, root);
		let mut stack = vec![(order_children(root), 0usize)];
		while let Some((children, position)) = stack.last_mut() {
			let Some(&child) = children.get(*position) else {
				stack.pop();
				continue;
			};
			*position += 1;
			if emitted[child] {
				continue;
			}
			emitted[child] = true;
			push(&mut layers, child);
			stack.push((order_children(child), 0));
		}
	}
	for &node in &graph.nodes {
		if !emitted[node] {
			emitted[node] = true;
			push(&mut layers, node);
		}
	}
	// `deduplicateLayers` has nothing to do: every node is emitted once.
	layers
}

/// `makeProperLayering`: edges spanning several layers get a chain of dummies.
pub(super) fn make_proper(layering: &Layering, acyclic: &Graph) -> (Vec<Vec<usize>>, Graph) {
	let mut graph = acyclic.clone();
	let mut layers = layering.layers.clone();
	let mut ranks = layering.ranks.clone();
	let mut edges_sorted = acyclic.edges.clone();
	js_sort(&mut edges_sorted, |a, b| match a.id == b.id {
		true => match a.source == b.source {
			true => acyclic.compare_ids(a.target, b.target),
			false => acyclic.compare_ids(a.source, b.source),
		},
		false => locale_compare(&a.id, &b.id),
	});
	let mut new_edges = Vec::with_capacity(edges_sorted.len());
	let mut dummy_count = 0;
	for edge in edges_sorted {
		let source_rank = ranks[edge.source].unwrap_or(0);
		let target_rank = ranks[edge.target].unwrap_or(0);
		if target_rank - source_rank <= 1 {
			new_edges.push(edge);
			continue;
		}
		let mut previous = edge.source;
		for (link, layer) in (source_rank + 1..target_rank).enumerate() {
			let dummy = graph.arena.len();
			graph.arena.push(GraphNode {
				id: format!("placeholder-{dummy_count}"),
				parent_id: None,
				is_group: false,
				is_dummy: true,
				width: 0.0,
				height: 0.0,
			});
			dummy_count += 1;
			graph.lane_of.push(None);
			graph.nodes.push(dummy);
			ranks.push(Some(layer));
			// Mermaid would fail on a negative layer; such a dummy joins the first one.
			let layer = layer.max(0) as usize;
			if layers.len() <= layer {
				layers.resize(layer + 1, Vec::new());
			}
			layers[layer].push(dummy);
			new_edges.push(EdgeRef {
				id: format!("{}#{link}", edge.id),
				source: previous,
				target: dummy,
				reference: edge.reference,
			});
			previous = dummy;
		}
		new_edges.push(EdgeRef {
			id: format!("{}#{}", edge.id, (target_rank - source_rank - 1).max(0)),
			source: previous,
			target: edge.target,
			reference: edge.reference,
		});
	}
	graph.edges = new_edges;
	(layers, graph)
}
