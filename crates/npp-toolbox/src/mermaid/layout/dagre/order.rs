//! Ordering (`order/`): the order of the nodes in each rank, by barycenter sweeps over layer
//! graphs that keep clusters contiguous, keeping the order with the fewest crossings.

use super::super::graph::{Edge, Graph, Key, KeyMap};
use super::{Layout, lodash_max, stable_sort_by};

/// A node of a layer graph: the shared label of a layout node, or the borders of a cluster in
/// this rank.
#[derive(Clone, Copy)]
enum LayerNode {
	Shared(usize),
	Borders { left: Option<Key>, right: Option<Key> },
}

struct LayerGraph {
	graph: Graph<LayerNode, f64>,
	root: Key,
}

pub(super) fn order(layout: &mut Layout) {
	let nodes = layout.g.nodes();
	let Some(max_rank) =
		lodash_max(nodes.iter().filter_map(|&v| layout.node(v).rank)).and_then(Layout::rank_index)
	else {
		return;
	};
	let down: Vec<LayerGraph> =
		(1..=max_rank).map(|rank| build_layer_graph(layout, rank as f64, true)).collect();
	let up: Vec<LayerGraph> =
		(0..max_rank).rev().map(|rank| build_layer_graph(layout, rank as f64, false)).collect();
	let layering = init_order(layout);
	assign_order(layout, &layering);
	let mut best_crossings = f64::INFINITY;
	let mut best = None;
	let mut iteration = 0;
	let mut last_best = 0;
	while last_best < 4 {
		let layer_graphs = if iteration % 2 == 1 { &down } else { &up };
		sweep_layer_graphs(layout, layer_graphs, iteration % 4 >= 2);
		let layering = layout.layer_matrix(&layout.g);
		let crossings = cross_count(layout, &layering);
		if crossings < best_crossings {
			last_best = 0;
			best = Some(layering);
			best_crossings = crossings;
		}
		iteration += 1;
		last_best += 1;
	}
	if let Some(best) = best {
		assign_order(layout, &best);
	}
}

fn assign_order(layout: &mut Layout, layering: &[Vec<Key>]) {
	for layer in layering {
		for (index, &v) in layer.iter().enumerate() {
			layout.node_mut(v).order = Some(index);
		}
	}
}

/// `buildLayerGraph`: the nodes of `rank` under their clusters, with the weights of their edges
/// to the previous rank (`in_edges`) or the next one.
fn build_layer_graph(layout: &mut Layout, rank: f64, in_edges: bool) -> LayerGraph {
	let root = loop {
		let root = layout.keys.unique("_root");
		if !layout.g.has_node(root) {
			break root;
		}
	};
	let mut graph: Graph<LayerNode, f64> = Graph::new(true, true);
	let shared = |layout: &Layout, v: Key| layout.g.node(v).map(|&index| LayerNode::Shared(index));
	let add_default = |graph: &mut Graph<LayerNode, f64>, layout: &Layout, v: Key| {
		if !graph.has_node(v) {
			match shared(layout, v) {
				Some(label) => graph.set_node(v, label),
				None => graph.ensure_node(v),
			}
		}
	};
	for v in layout.g.nodes() {
		let node = layout.node(v);
		let in_rank = node.rank == Some(rank)
			|| (node.min_rank.is_some_and(|min| min <= rank)
				&& node.max_rank.is_some_and(|max| rank <= max));
		if !in_rank {
			continue;
		}
		add_default(&mut graph, layout, v);
		let parent = layout.g.parent(v).unwrap_or(root);
		add_default(&mut graph, layout, parent);
		graph.set_parent(v, Some(parent));
		let edges = if in_edges { layout.g.in_edges(v) } else { layout.g.out_edges(v) };
		for edge in edges {
			let u = if edge.v == v { edge.w } else { edge.v };
			let layer_edge = Edge { v: u, w: v, name: None };
			let weight = graph.edge(layer_edge).copied().unwrap_or(0.0);
			add_default(&mut graph, layout, u);
			graph.set_edge(u, v, None, layout.edge(edge).weight + weight);
		}
		let node = layout.node(v);
		if node.min_rank.is_some() {
			let index = Layout::rank_index(rank).unwrap_or(usize::MAX);
			let left = node.border_left.get(index).copied().flatten();
			let right = node.border_right.get(index).copied().flatten();
			graph.set_node(v, LayerNode::Borders { left, right });
		}
	}
	LayerGraph { graph, root }
}

/// `initOrder`: a depth-first order from the nodes of the lowest ranks.
fn init_order(layout: &Layout) -> Vec<Vec<Key>> {
	let simple_nodes: Vec<Key> =
		layout.g.nodes().into_iter().filter(|&v| !layout.g.has_children(v)).collect();
	let rank_of = |v: Key| layout.node(v).rank.and_then(Layout::rank_index);
	let Some(max_rank) = simple_nodes.iter().filter_map(|&v| rank_of(v)).max() else {
		return Vec::new();
	};
	let mut layers = vec![Vec::new(); max_rank + 1];
	let mut ordered = simple_nodes.clone();
	// lodash's `sortBy` puts nodes without a rank last.
	stable_sort_by(&mut ordered, |&a, &b| match (rank_of(a), rank_of(b)) {
		(Some(a), Some(b)) => a < b,
		(Some(_), None) => true,
		_ => false,
	});
	let mut visited = KeyMap::default();
	for start in ordered {
		if visited.insert(start, ()).is_some() {
			continue;
		}
		let visit = |v: Key, layers: &mut Vec<Vec<Key>>| {
			if let Some(layer) = rank_of(v).and_then(|rank| layers.get_mut(rank)) {
				layer.push(v);
			}
		};
		visit(start, &mut layers);
		let mut frames = vec![(layout.g.successors(start), 0)];
		while let Some((successors, index)) = frames.last_mut() {
			let Some(&w) = successors.get(*index) else {
				frames.pop();
				continue;
			};
			*index += 1;
			if visited.insert(w, ()).is_none() {
				visit(w, &mut layers);
				frames.push((layout.g.successors(w), 0));
			}
		}
	}
	layers
}

fn sweep_layer_graphs(layout: &mut Layout, layer_graphs: &[LayerGraph], bias_right: bool) {
	let mut constraints: Graph<(), ()> = Graph::new(true, false);
	for layer_graph in layer_graphs {
		let sorted = sort_subgraph(layout, layer_graph, layer_graph.root, &constraints, bias_right);
		for (index, &v) in sorted.vs.iter().enumerate() {
			if let Some(&LayerNode::Shared(label)) = layer_graph.graph.node(v)
				&& let Some(node) = layout.nodes.get_mut(label)
			{
				node.order = Some(index);
			}
		}
		add_subgraph_constraints(&layer_graph.graph, &mut constraints, &sorted.vs);
	}
}

fn add_subgraph_constraints(
	graph: &Graph<LayerNode, f64>,
	constraints: &mut Graph<(), ()>,
	vs: &[Key],
) {
	let mut previous = KeyMap::default();
	let mut root_previous: Option<Key> = None;
	for &v in vs {
		let mut child = graph.parent(v);
		while let Some(current) = child {
			let parent = graph.parent(current);
			let previous_child = match parent {
				Some(parent) => previous.insert(parent, current),
				None => root_previous.replace(current),
			};
			if let Some(previous_child) = previous_child
				&& previous_child != current
			{
				constraints.set_edge(previous_child, current, None, ());
				break;
			}
			child = parent;
		}
	}
}

/// An entry of `sort`/`resolveConflicts`: nodes to place together and their barycenter.
#[derive(Clone)]
struct SortResult {
	vs: Vec<Key>,
	/// `None` if the property is missing; NaN is a value like in JavaScript.
	barycenter: Option<f64>,
	weight: Option<f64>,
}

struct BarycenterEntry {
	v: Key,
	barycenter: Option<f64>,
	weight: Option<f64>,
}

fn order_of(layout: &Layout, graph: &Graph<LayerNode, f64>, v: Key) -> f64 {
	match graph.node(v) {
		Some(&LayerNode::Shared(label)) => layout
			.nodes
			.get(label)
			.and_then(|node| node.order)
			.map_or(f64::NAN, |order| order as f64),
		_ => f64::NAN,
	}
}

/// `sortSubgraph`: orders the children of `v` by barycenter, keeping clusters together.
fn sort_subgraph(
	layout: &Layout,
	layer_graph: &LayerGraph,
	v: Key,
	constraints: &Graph<(), ()>,
	bias_right: bool,
) -> SortResult {
	let graph = &layer_graph.graph;
	let mut movable = graph.children(Some(v));
	let (border_left, border_right) = match graph.node(v) {
		Some(&LayerNode::Borders { left, right }) => (left, right),
		_ => (None, None),
	};
	if border_left.is_some() {
		movable.retain(|&w| Some(w) != border_left && Some(w) != border_right);
	}
	let mut barycenters: Vec<BarycenterEntry> = movable
		.iter()
		.map(|&w| {
			let in_edges = graph.in_edges(w);
			if in_edges.is_empty() {
				return BarycenterEntry { v: w, barycenter: None, weight: None };
			}
			let (mut sum, mut weight) = (0.0, 0.0);
			for edge in in_edges {
				let edge_weight = graph.edge(edge).copied().unwrap_or(f64::NAN);
				sum += edge_weight * order_of(layout, graph, edge.v);
				weight += edge_weight;
			}
			BarycenterEntry { v: w, barycenter: Some(sum / weight), weight: Some(weight) }
		})
		.collect();
	let mut subgraphs: KeyMap<SortResult> = KeyMap::default();
	for entry in &mut barycenters {
		if graph.has_children(entry.v) {
			let result = sort_subgraph(layout, layer_graph, entry.v, constraints, bias_right);
			if let Some(other) = result.barycenter {
				let other_weight = result.weight.unwrap_or(f64::NAN);
				match (entry.barycenter, entry.weight) {
					(Some(barycenter), Some(weight)) => {
						entry.barycenter = Some(
							(barycenter * weight + other * other_weight) / (weight + other_weight),
						);
						entry.weight = Some(weight + other_weight);
					}
					_ => {
						entry.barycenter = Some(other);
						entry.weight = Some(other_weight);
					}
				}
			}
			subgraphs.insert(entry.v, result);
		}
	}
	let mut entries = resolve_conflicts(&barycenters, constraints);
	for (entry, _) in &mut entries {
		entry.vs = entry
			.vs
			.iter()
			.flat_map(|v| match subgraphs.get(v) {
				Some(subgraph) => subgraph.vs.clone(),
				None => vec![*v],
			})
			.collect();
	}
	let mut result = sort(entries, bias_right);
	if let (Some(left), Some(right)) = (border_left, border_right) {
		let mut vs = Vec::with_capacity(result.vs.len() + 2);
		vs.push(left);
		vs.append(&mut result.vs);
		vs.push(right);
		result.vs = vs;
		let left_predecessor = graph.predecessors(left).first().copied();
		let right_predecessor = graph.predecessors(right).first().copied();
		if let Some(left_predecessor) = left_predecessor {
			let left_order = order_of(layout, graph, left_predecessor);
			let right_order =
				right_predecessor.map_or(f64::NAN, |right| order_of(layout, graph, right));
			let barycenter = result.barycenter.unwrap_or(0.0);
			let weight = result.weight.unwrap_or(0.0);
			result.barycenter =
				Some((barycenter * weight + left_order + right_order) / (weight + 2.0));
			result.weight = Some(weight + 2.0);
		}
	}
	result
}

/// An entry of `resolveConflicts`.
struct ConflictEntry {
	indegree: usize,
	incoming: Vec<usize>,
	outgoing: Vec<usize>,
	vs: Vec<Key>,
	i: usize,
	barycenter: Option<f64>,
	weight: Option<f64>,
	merged: bool,
}

/// `resolveConflicts` with `SortResult`s carrying the original index `i`.
fn resolve_conflicts(
	barycenters: &[BarycenterEntry],
	constraints: &Graph<(), ()>,
) -> Vec<(SortResult, usize)> {
	let mut entries: Vec<ConflictEntry> = Vec::with_capacity(barycenters.len());
	let mut by_key: KeyMap<usize> = KeyMap::default();
	// `mappedEntries[entry.v] = …` replaces an earlier entry of the same key in place.
	let mut keys_in_order: Vec<Key> = Vec::new();
	for (i, entry) in barycenters.iter().enumerate() {
		let conflict = ConflictEntry {
			indegree: 0,
			incoming: Vec::new(),
			outgoing: Vec::new(),
			vs: vec![entry.v],
			i,
			barycenter: entry.barycenter,
			weight: entry.weight,
			merged: false,
		};
		match by_key.get(&entry.v) {
			Some(&index) => entries[index] = conflict,
			None => {
				by_key.insert(entry.v, entries.len());
				keys_in_order.push(entry.v);
				entries.push(conflict);
			}
		}
	}
	for edge in constraints.edges() {
		if let (Some(&v), Some(&w)) = (by_key.get(&edge.v), by_key.get(&edge.w)) {
			entries[w].indegree += 1;
			entries[v].outgoing.push(w);
		}
	}
	// `_.filter(mappedEntries, …)` lists the object's keys in JavaScript order.
	npp::sort::sort_by_key(&mut keys_in_order[..], |key| key.object_order());
	let mut source_set: Vec<usize> = keys_in_order
		.iter()
		.filter_map(|key| by_key.get(key).copied())
		.filter(|&index| entries[index].indegree == 0)
		.collect();
	let mut processed = Vec::new();
	while let Some(v) = source_set.pop() {
		processed.push(v);
		let mut incoming = std::mem::take(&mut entries[v].incoming);
		incoming.reverse();
		for &u in &incoming {
			if entries[u].merged {
				continue;
			}
			let should_merge = match (entries[u].barycenter, entries[v].barycenter) {
				(Some(u_barycenter), Some(v_barycenter)) => u_barycenter >= v_barycenter,
				_ => true,
			};
			if should_merge {
				merge_entries(&mut entries, v, u);
			}
		}
		entries[v].incoming = incoming;
		for w in entries[v].outgoing.clone() {
			entries[w].incoming.push(v);
			entries[w].indegree -= 1;
			if entries[w].indegree == 0 {
				source_set.push(w);
			}
		}
	}
	processed
		.into_iter()
		.filter(|&index| !entries[index].merged)
		.map(|index| {
			let entry = &entries[index];
			(
				SortResult {
					vs: entry.vs.clone(),
					barycenter: entry.barycenter,
					weight: entry.weight,
				},
				entry.i,
			)
		})
		.collect()
}

fn merge_entries(entries: &mut [ConflictEntry], target: usize, source: usize) {
	let mut sum = 0.0;
	let mut weight = 0.0;
	let truthy = |value: Option<f64>| value.is_some_and(|value| value != 0.0 && !value.is_nan());
	for index in [target, source] {
		let entry = &entries[index];
		if truthy(entry.weight) {
			sum += entry.barycenter.unwrap_or(f64::NAN) * entry.weight.unwrap_or(f64::NAN);
			weight += entry.weight.unwrap_or(f64::NAN);
		}
	}
	let mut vs = entries[source].vs.clone();
	vs.extend_from_slice(&entries[target].vs);
	let source_i = entries[source].i;
	let target_entry = &mut entries[target];
	target_entry.vs = vs;
	target_entry.barycenter = Some(sum / weight);
	target_entry.weight = Some(weight);
	target_entry.i = target_entry.i.min(source_i);
	entries[source].merged = true;
}

/// `sort`: entries with barycenters in their order, the others at their original places.
fn sort(entries: Vec<(SortResult, usize)>, bias_right: bool) -> SortResult {
	let (mut sortable, mut unsortable): (Vec<_>, Vec<_>) =
		entries.into_iter().partition(|(entry, _)| entry.barycenter.is_some());
	// `_.sortBy(…, entry => -entry.i)`: descending, so that the smallest `i` is last.
	stable_sort_by(&mut unsortable, |a, b| a.1 > b.1);
	stable_sort_by(&mut sortable, |a, b| {
		let (a_barycenter, b_barycenter) =
			(a.0.barycenter.unwrap_or(f64::NAN), b.0.barycenter.unwrap_or(f64::NAN));
		if a_barycenter < b_barycenter {
			return true;
		}
		if a_barycenter > b_barycenter {
			return false;
		}
		match bias_right {
			false => a.1 < b.1,
			true => b.1 < a.1,
		}
	});
	let mut vs = Vec::new();
	let mut sum = 0.0;
	let mut weight = 0.0;
	let mut vs_index = 0;
	let consume =
		|vs: &mut Vec<Key>, unsortable: &mut Vec<(SortResult, usize)>, mut index: usize| {
			while let Some((entry, i)) = unsortable.last() {
				if *i > index {
					break;
				}
				vs.extend_from_slice(&entry.vs);
				unsortable.pop();
				index += 1;
			}
			index
		};
	vs_index = consume(&mut vs, &mut unsortable, vs_index);
	for (entry, _) in sortable {
		vs_index += entry.vs.len();
		vs.extend_from_slice(&entry.vs);
		let entry_weight = entry.weight.unwrap_or(f64::NAN);
		sum += entry.barycenter.unwrap_or(f64::NAN) * entry_weight;
		weight += entry_weight;
		vs_index = consume(&mut vs, &mut unsortable, vs_index);
	}
	let has_weight = weight != 0.0 && !weight.is_nan();
	SortResult {
		vs,
		barycenter: has_weight.then_some(sum / weight),
		weight: has_weight.then_some(weight),
	}
}

/// `crossCount`: weighted crossings between adjacent layers, with an accumulator tree.
fn cross_count(layout: &Layout, layering: &[Vec<Key>]) -> f64 {
	let mut crossings = 0.0;
	for pair in layering.windows(2) {
		let (north, south) = (&pair[0], &pair[1]);
		let south_position: KeyMap<usize> =
			south.iter().enumerate().map(|(index, &v)| (v, index)).collect();
		let mut entries: Vec<(usize, f64)> = Vec::new();
		for &v in north {
			let mut edges: Vec<(usize, f64)> = layout
				.g
				.out_edges(v)
				.into_iter()
				.filter_map(|edge| Some((*south_position.get(&edge.w)?, layout.edge(edge).weight)))
				.collect();
			stable_sort_by(&mut edges, |a, b| a.0 < b.0);
			entries.extend(edges);
		}
		let mut first_index = 1;
		while first_index < south.len() {
			first_index <<= 1;
		}
		let tree_size = 2 * first_index - 1;
		first_index -= 1;
		let mut tree = vec![0.0; tree_size];
		for (position, weight) in entries {
			let mut index = position + first_index;
			if index >= tree.len() {
				continue;
			}
			tree[index] += weight;
			let mut weight_sum = 0.0;
			while index > 0 {
				if index % 2 == 1 {
					weight_sum += tree.get(index + 1).copied().unwrap_or(f64::NAN);
				}
				index = (index - 1) >> 1;
				tree[index] += weight;
			}
			crossings += weight * weight_sum;
		}
	}
	crossings
}
