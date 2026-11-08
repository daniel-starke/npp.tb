//! Ranking (`rank/`): longest path, a feasible tight tree and network simplex on the simplified
//! non-compound view of the layout graph.

use std::collections::HashMap;

use super::super::graph::{Edge, Graph, Key, KeyMap};
use super::{Layout, lodash_min};

/// An edge of the simplified graph: multi-edges merged.
struct Simple {
	weight: f64,
	minlen: f64,
}

/// The feasible tree, an undirected graph in graphlib's node and edge order.
type Tree = Graph<(), ()>;

struct Ranking<'l, 'k> {
	layout: &'l mut Layout<'k>,
	g: Graph<usize, Simple>,
}

/// `rank(asNonCompoundGraph(g))` with network simplex. Ranks go to the shared node labels.
pub(super) fn rank(layout: &mut Layout) {
	// asNonCompoundGraph, then simplify.
	let mut g: Graph<usize, Simple> = Graph::new(true, false);
	for v in layout.g.nodes() {
		if !layout.g.has_children(v)
			&& let Some(&label) = layout.g.node(v)
		{
			g.set_node(v, label);
		}
	}
	for edge in layout.g.edges() {
		g.ensure_node(edge.v);
		g.ensure_node(edge.w);
	}
	for edge in layout.g.edges() {
		let label = layout.edge(edge);
		let simple = Edge { name: None, ..edge };
		let (weight, minlen) = g.edge(simple).map_or((0.0, 1.0), |old| (old.weight, old.minlen));
		let minlen = super::js_max(minlen, label.minlen);
		g.set_edge(edge.v, edge.w, None, Simple { weight: weight + label.weight, minlen });
	}
	let mut ranking = Ranking { layout, g };
	ranking.network_simplex();
}

impl Ranking<'_, '_> {
	fn rank(&self, v: Key) -> f64 {
		self.g
			.node(v)
			.and_then(|&index| self.layout.nodes.get(index))
			.and_then(|node| node.rank)
			.unwrap_or(f64::NAN)
	}

	fn set_rank(&mut self, v: Key, rank: f64) {
		if let Some(node) = self.g.node(v).and_then(|&index| self.layout.nodes.get_mut(index)) {
			node.rank = Some(rank);
		}
	}

	fn minlen(&self, edge: Edge) -> f64 {
		self.g.edge(edge).map_or(f64::NAN, |label| label.minlen)
	}

	fn weight(&self, edge: Edge) -> f64 {
		self.g.edge(edge).map_or(f64::NAN, |label| label.weight)
	}

	fn slack(&self, edge: Edge) -> f64 {
		self.rank(edge.w) - self.rank(edge.v) - self.minlen(edge)
	}

	/// `t.setEdge(v, w)` of the undirected tree, which orders the ends by name.
	fn set_tree_edge(&self, tree: &mut Tree, v: Key, w: Key) {
		let (v, w) = if self.layout.keys.less(w, v) { (w, v) } else { (v, w) };
		tree.set_edge(v, w, None, ());
	}

	fn network_simplex(&mut self) {
		if self.g.node_count() == 0 {
			return;
		}
		self.longest_path();
		let tree = self.feasible_tree();
		let mut simplex = Simplex::new(self, &tree);
		simplex.run();
		for (index, &v) in simplex.keys.iter().enumerate() {
			if simplex.is_ranked[index] {
				self.set_rank(v, simplex.rank[index]);
			}
		}
	}

	/// `longestPath`: every node as low as its successors allow.
	fn longest_path(&mut self) {
		let mut visited = KeyMap::default();
		for source in self.g.sources() {
			if visited.insert(source, ()).is_some() {
				continue;
			}
			let mut frames = vec![(source, self.g.out_edges(source), 0, None::<f64>)];
			while let Some((v, edges, index, minimum)) = frames.last_mut() {
				if let Some(&edge) = edges.get(*index) {
					if visited.insert(edge.w, ()).is_some() {
						let value = self.rank(edge.w) - self.minlen(edge);
						*minimum = lodash_min(minimum.iter().copied().chain([value]));
						*index += 1;
					} else {
						let out_edges = self.g.out_edges(edge.w);
						frames.push((edge.w, out_edges, 0, None));
					}
					continue;
				}
				let (v, rank) = (*v, minimum.unwrap_or(0.0));
				self.set_rank(v, rank);
				frames.pop();
				if let Some((_, edges, index, minimum)) = frames.last_mut() {
					let edge = edges[*index];
					let value = rank - self.minlen(edge);
					*minimum = lodash_min(minimum.iter().copied().chain([value]));
					*index += 1;
				}
			}
		}
	}

	/// `feasibleTree`: a spanning tree of tight edges, shifting ranks until it spans the graph.
	fn feasible_tree(&mut self) -> Tree {
		let mut tree = Tree::new(false, false);
		let Some(start) = self.g.nodes().first().copied() else { return tree };
		let size = self.g.node_count();
		tree.set_node(start, ());
		while self.tight_tree(&mut tree) < size {
			let Some(edge) = self.find_min_slack_edge(&tree) else { break };
			let delta = match tree.has_node(edge.v) {
				true => self.slack(edge),
				false => -self.slack(edge),
			};
			for v in tree.nodes() {
				let rank = self.rank(v);
				self.set_rank(v, rank + delta);
			}
		}
		tree
	}

	fn tight_tree(&self, tree: &mut Tree) -> usize {
		for root in tree.nodes() {
			let mut frames = vec![(root, self.g.node_edges(root), 0)];
			while let Some((v, edges, index)) = frames.last_mut() {
				let Some(&edge) = edges.get(*index) else {
					frames.pop();
					continue;
				};
				*index += 1;
				let v = *v;
				let w = if v == edge.v { edge.w } else { edge.v };
				let slack = self.slack(edge);
				if !tree.has_node(w) && (slack == 0.0 || slack.is_nan()) {
					tree.set_node(w, ());
					self.set_tree_edge(tree, v, w);
					frames.push((w, self.g.node_edges(w), 0));
				}
			}
		}
		tree.node_count()
	}

	fn find_min_slack_edge(&self, tree: &Tree) -> Option<Edge> {
		let mut best: Option<(Edge, f64)> = None;
		for edge in self.g.edges() {
			if tree.has_node(edge.v) == tree.has_node(edge.w) {
				continue;
			}
			let slack = self.slack(edge);
			if slack.is_nan() {
				continue;
			}
			if best.is_none_or(|(_, minimum)| slack < minimum) {
				best = Some((edge, slack));
			}
		}
		best.map(|(edge, _)| edge)
	}
}

#[derive(Clone, Copy)]
struct SimplexEdge {
	v: usize,
	w: usize,
	weight: f64,
	minlen: f64,
}

/// Network simplex on dense indices, making dagre's choices: the first tree edge in insertion
/// order with a negative cut value leaves, and the first edge of least slack in the order of
/// `g.edges()` enters. Low and lim values may come from any depth-first numbering, as only
/// ancestry and parent/child comparisons use them.
struct Simplex {
	keys: Vec<Key>,
	rank: Vec<f64>,
	/// Whether a node got a rank, to leave the others unranked.
	is_ranked: Vec<bool>,
	edges: Vec<SimplexEdge>,
	/// The edges of every node: in-edges, then out-edges, like `nodeEdges`.
	node_edges: Vec<Vec<usize>>,
	/// The ends of the tree edges in insertion order.
	tree_edges: Vec<(usize, usize)>,
	tree_neighbors: Vec<Vec<usize>>,
	root: usize,
	/// Nodes outside the tree keep 0 like graphlib's missing labels.
	low: Vec<usize>,
	lim: Vec<usize>,
	parent: Vec<Option<usize>>,
	/// The cut value of the tree edge from every node to its parent.
	cut_value: Vec<f64>,
	stack: Vec<(usize, usize, usize)>,
}

impl Simplex {
	fn new(ranking: &Ranking, tree: &Tree) -> Simplex {
		let keys = ranking.g.nodes();
		let count = keys.len();
		let mut index_of: KeyMap<usize> = KeyMap::default();
		for (index, &key) in keys.iter().enumerate() {
			index_of.insert(key, index);
		}
		let mut edge_index: HashMap<Edge, usize> = HashMap::new();
		let mut edges = Vec::new();
		for edge in ranking.g.edges() {
			let (Some(&v), Some(&w)) = (index_of.get(&edge.v), index_of.get(&edge.w)) else {
				continue;
			};
			edge_index.insert(edge, edges.len());
			edges.push(SimplexEdge {
				v,
				w,
				weight: ranking.weight(edge),
				minlen: ranking.minlen(edge),
			});
		}
		let node_edges = keys
			.iter()
			.map(|&v| {
				let edges = ranking.g.node_edges(v);
				edges.iter().filter_map(|edge| edge_index.get(edge).copied()).collect()
			})
			.collect();
		let mut tree_edges = Vec::new();
		let mut tree_neighbors = vec![Vec::new(); count];
		for edge in tree.edges() {
			if let (Some(&v), Some(&w)) = (index_of.get(&edge.v), index_of.get(&edge.w)) {
				tree_edges.push((v, w));
				tree_neighbors[v].push(w);
				tree_neighbors[w].push(v);
			}
		}
		let root = tree.nodes().first().and_then(|root| index_of.get(root)).copied().unwrap_or(0);
		Simplex {
			rank: keys.iter().map(|&v| ranking.rank(v)).collect(),
			is_ranked: vec![false; count],
			keys,
			edges,
			node_edges,
			tree_edges,
			tree_neighbors,
			root,
			low: vec![0; count],
			lim: vec![0; count],
			parent: vec![None; count],
			cut_value: vec![f64::NAN; count],
			stack: Vec::new(),
		}
	}

	fn run(&mut self) {
		self.assign_low_lim();
		// Pairs of `lim` and node: `lim` is unique, so sorting the pairs orders by it alone.
		let mut order: Vec<(usize, usize)> = (0..self.keys.len())
			.filter(|&v| self.parent[v].is_some())
			.map(|v| (self.lim[v], v))
			.collect();
		order.sort_unstable();
		for (_, child) in order {
			self.update_cut_value(child);
		}
		// Network simplex can cycle in degenerate cases; dagre would hang there.
		let mut remaining = 50 * (self.keys.len() + self.edges.len()) + 1000;
		while remaining > 0 {
			remaining -= 1;
			let Some(position) = self
				.tree_edges
				.iter()
				.position(|&(v, w)| self.cut_value[self.child_of(v, w)] < 0.0)
			else {
				break;
			};
			let (v, w) = self.tree_edges[position];
			let Some(enter) = self.enter_edge(v, w) else { break };
			self.tree_edges.remove(position);
			self.tree_neighbors[v].retain(|&other| other != w);
			self.tree_neighbors[w].retain(|&other| other != v);
			let (x, y) = (self.edges[enter].v, self.edges[enter].w);
			self.tree_edges.push((x, y));
			self.tree_neighbors[x].push(y);
			self.tree_neighbors[y].push(x);
			self.assign_low_lim();
			self.update_cut_values(v, w);
			self.update_ranks(x, y);
		}
	}

	/// The end of the tree edge `v`–`w` that is the child.
	fn child_of(&self, v: usize, w: usize) -> usize {
		if self.parent[v] == Some(w) { v } else { w }
	}

	/// The graph edge from `v` to `w`.
	fn edge_between(&self, v: usize, w: usize) -> Option<SimplexEdge> {
		self.node_edges[v]
			.iter()
			.map(|&edge| self.edges[edge])
			.find(|edge| edge.v == v && edge.w == w)
	}

	/// `dfsAssignLowLim` from the root.
	fn assign_low_lim(&mut self) {
		let mut stack = std::mem::take(&mut self.stack);
		let mut next_lim = 1;
		self.parent[self.root] = None;
		stack.push((self.root, 0, next_lim));
		while let Some(top) = stack.last_mut() {
			let (v, low) = (top.0, top.2);
			if let Some(&w) = self.tree_neighbors[v].get(top.1) {
				top.1 += 1;
				// In a tree, every neighbor but the parent is unvisited.
				if Some(w) != self.parent[v] {
					self.parent[w] = Some(v);
					stack.push((w, 0, next_lim));
				}
				continue;
			}
			self.low[v] = low;
			self.lim[v] = next_lim;
			next_lim += 1;
			stack.pop();
		}
		self.stack = stack;
	}

	/// `calcCutValue` of the tree edge from `child` to its parent.
	fn update_cut_value(&mut self, child: usize) {
		let Some(parent) = self.parent[child] else { return };
		let (child_is_tail, graph_edge) = match self.edge_between(child, parent) {
			Some(edge) => (true, Some(edge)),
			None => (false, self.edge_between(parent, child)),
		};
		let mut cut_value = graph_edge.map_or(f64::NAN, |edge| edge.weight);
		for &index in &self.node_edges[child] {
			let edge = self.edges[index];
			let is_out_edge = edge.v == child;
			let other = if is_out_edge { edge.w } else { edge.v };
			if other == parent {
				continue;
			}
			let points_to_head = is_out_edge == child_is_tail;
			cut_value += if points_to_head { edge.weight } else { -edge.weight };
			// The other tree edges of the child lead to its children.
			if self.parent[other] == Some(child) {
				let other_cut_value = self.cut_value[other];
				cut_value += if points_to_head { -other_cut_value } else { other_cut_value };
			}
		}
		self.cut_value[child] = cut_value;
	}

	/// `initCutValues` after the tree edge `v`–`w` left: only the tree edges on the cycle that
	/// the entering edge closed change, which is the path between `v` and `w` in the new tree.
	/// All other edges keep their subtrees, so their cut values stay bit for bit the same; the
	/// path is recomputed descendants first.
	fn update_cut_values(&mut self, v: usize, w: usize) {
		let mut path = Vec::new();
		for (start, other) in [(v, w), (w, v)] {
			let other_lim = self.lim[other];
			let mut node = start;
			// Up to the lowest common ancestor, whose subtree did not change.
			while !(self.low[node] <= other_lim && other_lim <= self.lim[node]) {
				let Some(parent) = self.parent[node] else { break };
				path.push((self.lim[node], node));
				node = parent;
			}
		}
		path.sort_unstable();
		for (_, child) in path {
			self.update_cut_value(child);
		}
	}

	/// `enterEdge` for the leaving tree edge `v`–`w`.
	fn enter_edge(&self, v: usize, w: usize) -> Option<usize> {
		let (v, w) = if self.edge_between(v, w).is_some() { (v, w) } else { (w, v) };
		let (tail, flip) = if self.lim[v] > self.lim[w] { (w, true) } else { (v, false) };
		let (low, lim) = (self.low[tail], self.lim[tail]);
		let is_descendant = |node: usize| low <= self.lim[node] && self.lim[node] <= lim;
		let mut best: Option<(usize, f64)> = None;
		for (index, edge) in self.edges.iter().enumerate() {
			if flip != is_descendant(edge.v) || flip == is_descendant(edge.w) {
				continue;
			}
			let slack = self.rank[edge.w] - self.rank[edge.v] - edge.minlen;
			if slack.is_nan() {
				continue;
			}
			if best.is_none_or(|(_, minimum)| slack < minimum) {
				best = Some((index, slack));
			}
		}
		best.map(|(index, _)| index)
	}

	/// `updateRanks` after the entering edge `v`–`w` joined the subtree without the root
	/// again: the other nodes keep their parents and therefore their ranks.
	fn update_ranks(&mut self, v: usize, w: usize) {
		let subtree = self.child_of(v, w);
		let mut stack = vec![subtree];
		while let Some(node) = stack.pop() {
			let Some(parent) = self.parent[node] else { continue };
			let rank = match self.edge_between(node, parent) {
				Some(edge) => self.rank[parent] - edge.minlen,
				None => {
					self.rank[parent]
						+ self.edge_between(parent, node).map_or(f64::NAN, |edge| edge.minlen)
				}
			};
			self.rank[node] = rank;
			self.is_ranked[node] = true;
			for &child in &self.tree_neighbors[node] {
				if self.parent[child] == Some(node) {
					stack.push(child);
				}
			}
		}
	}
}
