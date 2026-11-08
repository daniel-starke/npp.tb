//! graphlib's `Graph` (dagre-d3-es): a directed or undirected multigraph with compound nodes.
//!
//! graphlib keeps nodes, children, predecessors and successors in JavaScript objects, which list
//! array index keys (`"0"`, `"1"`, …) first in ascending order and all other keys in insertion
//! order. The layouts depend on these orders, so this graph keeps them too.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// An interned node id or edge name. The order is that of interning, not of the names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
	id: u32,
	/// The value of an array index key, `u32::MAX` for other keys.
	array_index: u32,
}

impl Key {
	/// Sorting keys of insertion order stably by this gives the order of a JavaScript object.
	pub fn object_order(self) -> u32 {
		self.array_index
	}
}

/// The names of the keys of one layout.
#[derive(Default)]
pub struct Keys {
	names: Vec<String>,
	by_name: HashMap<String, Key>,
	/// lodash's `uniqueId` counter.
	unique_counter: u64,
}

impl Keys {
	pub fn intern(&mut self, name: &str) -> Key {
		if let Some(&key) = self.by_name.get(name) {
			return key;
		}
		let key =
			Key { id: self.names.len() as u32, array_index: array_index(name).unwrap_or(u32::MAX) };
		self.names.push(name.to_owned());
		self.by_name.insert(name.to_owned(), key);
		key
	}

	pub fn get(&self, name: &str) -> Option<Key> {
		self.by_name.get(name).copied()
	}

	pub fn name(&self, key: Key) -> &str {
		self.names.get(key.id as usize).map_or("", String::as_str)
	}

	/// lodash's `uniqueId(prefix)`.
	pub fn unique(&mut self, prefix: &str) -> Key {
		self.unique_counter += 1;
		self.intern(&format!("{prefix}{}", self.unique_counter))
	}

	/// Continues `uniqueId` like a JavaScript page that already used it `counter` times.
	#[cfg(test)]
	pub fn set_unique_counter(&mut self, counter: u64) {
		self.unique_counter = counter;
	}

	/// JavaScript's `a < b` for the names: comparison of UTF-16 code units.
	pub fn less(&self, a: Key, b: Key) -> bool {
		self.name(a).encode_utf16().lt(self.name(b).encode_utf16())
	}
}

/// The value of `name` if JavaScript treats it as an array index: a canonical decimal number
/// below 2³² − 1.
pub(crate) fn array_index(name: &str) -> Option<u32> {
	let is_canonical = !name.is_empty()
		&& name.len() <= 10
		&& name.bytes().all(|byte| byte.is_ascii_digit())
		&& (name == "0" || !name.starts_with('0'));
	if !is_canonical {
		return None;
	}
	let value: u64 = name.parse().ok()?;
	(value < u64::from(u32::MAX)).then_some(value as u32)
}

/// Sorts keys of insertion order into the order of a JavaScript object's keys.
fn js_key_order(keys: &mut [Key]) {
	npp::sort::sort_by_key(&mut keys[..], |key| key.object_order());
}

#[derive(Default)]
pub struct KeyHasher(u64);

impl Hasher for KeyHasher {
	fn finish(&self) -> u64 {
		self.0
	}

	fn write(&mut self, bytes: &[u8]) {
		for &byte in bytes {
			self.0 = (self.0.rotate_left(8) ^ u64::from(byte)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
		}
	}

	fn write_u32(&mut self, value: u32) {
		self.0 = (self.0.rotate_left(32) ^ u64::from(value)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
	}
}

pub type KeyMap<V> = HashMap<Key, V, BuildHasherDefault<KeyHasher>>;

/// graphlib's edge object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
	pub v: Key,
	pub w: Key,
	pub name: Option<Key>,
}

struct NodeEntry<N> {
	label: Option<N>,
	/// Insertion number, to tell live entries of `Graph::order` from removed ones.
	sequence: u64,
	parent: Option<Key>,
	children: Vec<Key>,
	in_edges: Vec<usize>,
	out_edges: Vec<usize>,
	/// Neighbors with the number of edges to them, in insertion order.
	predecessors: Vec<(Key, u32)>,
	successors: Vec<(Key, u32)>,
}

struct EdgeEntry<E> {
	edge: Edge,
	label: E,
}

pub struct Graph<N, E> {
	directed: bool,
	compound: bool,
	pub label: GraphLabel,
	nodes: KeyMap<NodeEntry<N>>,
	/// Nodes in insertion order with their insertion numbers; removed nodes stay until compacted.
	order: Vec<(Key, u64)>,
	next_sequence: u64,
	/// Live nodes with array index keys; without them insertion order is the key order.
	array_index_nodes: usize,
	root_children: Vec<Key>,
	/// Edges in insertion order; removed edges leave `None`.
	edges: Vec<Option<EdgeEntry<E>>>,
	edge_slots: HashMap<Edge, usize, BuildHasherDefault<KeyHasher>>,
}

/// The graph attributes the layouts use. Mermaid sets neither `ranker`, `acyclicer` nor `align`, so
/// dagre ranks with network simplex, breaks cycles depth first and balances the alignments.
#[derive(Clone, Debug)]
pub struct GraphLabel {
	pub rankdir: RankDir,
	pub nodesep: f64,
	pub edgesep: f64,
	pub ranksep: f64,
	pub marginx: f64,
	pub marginy: f64,
	/// Results.
	pub width: f64,
	pub height: f64,
}

/// dagre's defaults.
impl Default for GraphLabel {
	fn default() -> Self {
		Self {
			rankdir: RankDir::TopBottom,
			nodesep: 50.0,
			edgesep: 20.0,
			ranksep: 50.0,
			marginx: 0.0,
			marginy: 0.0,
			width: 0.0,
			height: 0.0,
		}
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RankDir {
	#[default]
	TopBottom,
	BottomTop,
	LeftRight,
	RightLeft,
}

impl RankDir {
	/// Mermaid's direction keywords; unknown ones lay out top to bottom like dagre.
	pub fn parse(text: &str) -> RankDir {
		match text.to_ascii_uppercase().as_str() {
			"BT" => RankDir::BottomTop,
			"LR" => RankDir::LeftRight,
			"RL" => RankDir::RightLeft,
			_ => RankDir::TopBottom,
		}
	}

	pub fn is_horizontal(self) -> bool {
		matches!(self, RankDir::LeftRight | RankDir::RightLeft)
	}
}

impl<N, E> Graph<N, E> {
	pub fn new(directed: bool, compound: bool) -> Self {
		Self {
			directed,
			compound,
			label: GraphLabel::default(),
			nodes: KeyMap::default(),
			order: Vec::new(),
			next_sequence: 0,
			array_index_nodes: 0,
			root_children: Vec::new(),
			edges: Vec::new(),
			edge_slots: HashMap::default(),
		}
	}

	pub fn node_count(&self) -> usize {
		self.nodes.len()
	}

	pub fn nodes(&self) -> Vec<Key> {
		let mut keys: Vec<Key> = self
			.order
			.iter()
			.filter(|(key, sequence)| {
				self.nodes.get(key).is_some_and(|entry| entry.sequence == *sequence)
			})
			.map(|&(key, _)| key)
			.collect();
		if self.array_index_nodes > 0 {
			js_key_order(&mut keys);
		}
		keys
	}

	pub fn has_node(&self, v: Key) -> bool {
		self.nodes.contains_key(&v)
	}

	pub fn node(&self, v: Key) -> Option<&N> {
		self.nodes.get(&v)?.label.as_ref()
	}

	pub fn node_mut(&mut self, v: Key) -> Option<&mut N> {
		self.nodes.get_mut(&v)?.label.as_mut()
	}

	/// `setNode(v, label)`: adds the node or replaces its label.
	pub fn set_node(&mut self, v: Key, label: N) {
		match self.nodes.get_mut(&v) {
			Some(entry) => entry.label = Some(label),
			None => self.insert_node(v, Some(label)),
		}
	}

	/// `setNode(v)`: adds the node without a label if it is missing.
	pub fn ensure_node(&mut self, v: Key) {
		if !self.nodes.contains_key(&v) {
			self.insert_node(v, None);
		}
	}

	fn insert_node(&mut self, v: Key, label: Option<N>) {
		let sequence = self.next_sequence;
		self.next_sequence += 1;
		self.nodes.insert(
			v,
			NodeEntry {
				label,
				sequence,
				parent: None,
				children: Vec::new(),
				in_edges: Vec::new(),
				out_edges: Vec::new(),
				predecessors: Vec::new(),
				successors: Vec::new(),
			},
		);
		if self.order.len() > 2 * self.nodes.len() + 64 {
			let nodes = &self.nodes;
			self.order.retain(|(key, sequence)| {
				nodes.get(key).is_some_and(|entry| entry.sequence == *sequence)
			});
		}
		self.order.push((v, sequence));
		if v.array_index != u32::MAX {
			self.array_index_nodes += 1;
		}
		if self.compound {
			self.root_children.push(v);
		}
	}

	pub fn remove_node(&mut self, v: Key) {
		let Some(entry) = self.nodes.remove(&v) else { return };
		if v.array_index != u32::MAX {
			self.array_index_nodes -= 1;
		}
		if self.compound {
			self.remove_from_parent(v, entry.parent);
			for &child in &entry.children {
				if let Some(child_entry) = self.nodes.get_mut(&child) {
					child_entry.parent = None;
				}
				self.root_children.push(child);
			}
		}
		// The entry is gone, so removing the edges must not touch its lists.
		for slot in entry.in_edges.iter().chain(&entry.out_edges) {
			if let Some(edge) = self.edges.get(*slot).and_then(|edge| edge.as_ref()).map(|e| e.edge)
			{
				self.remove_edge(edge);
			}
		}
	}

	fn remove_from_parent(&mut self, v: Key, parent: Option<Key>) {
		let siblings = match parent {
			Some(parent) => match self.nodes.get_mut(&parent) {
				Some(entry) => &mut entry.children,
				None => return,
			},
			None => &mut self.root_children,
		};
		if let Some(index) = siblings.iter().position(|&sibling| sibling == v) {
			siblings.remove(index);
		}
	}

	/// `setParent(v, parent)`. Returns `false` and changes nothing if `parent` is `v` or one of
	/// its descendants, where graphlib throws.
	pub fn set_parent(&mut self, v: Key, parent: Option<Key>) -> bool {
		if !self.compound {
			return false;
		}
		if let Some(parent) = parent {
			let mut ancestor = Some(parent);
			while let Some(current) = ancestor {
				if current == v {
					return false;
				}
				ancestor = self.parent(current);
			}
			self.ensure_node(parent);
		}
		self.ensure_node(v);
		let previous = self.nodes.get(&v).and_then(|entry| entry.parent);
		self.remove_from_parent(v, previous);
		if let Some(entry) = self.nodes.get_mut(&v) {
			entry.parent = parent;
		}
		match parent {
			Some(parent) => {
				if let Some(entry) = self.nodes.get_mut(&parent) {
					entry.children.push(v);
				}
			}
			None => self.root_children.push(v),
		}
		true
	}

	pub fn parent(&self, v: Key) -> Option<Key> {
		self.nodes.get(&v)?.parent
	}

	/// Children of `v`, or the top-level nodes for `None`. Empty for a missing node.
	pub fn children(&self, v: Option<Key>) -> Vec<Key> {
		if !self.compound {
			return match v {
				None => self.nodes(),
				Some(_) => Vec::new(),
			};
		}
		let mut children = match v {
			None => self.root_children.clone(),
			Some(v) => self.nodes.get(&v).map(|entry| entry.children.clone()).unwrap_or_default(),
		};
		if self.array_index_nodes > 0 {
			js_key_order(&mut children);
		}
		children
	}

	pub fn has_children(&self, v: Key) -> bool {
		self.compound && self.nodes.get(&v).is_some_and(|entry| !entry.children.is_empty())
	}

	pub fn predecessors(&self, v: Key) -> Vec<Key> {
		self.neighbor_keys(v, |entry| &entry.predecessors)
	}

	pub fn successors(&self, v: Key) -> Vec<Key> {
		self.neighbor_keys(v, |entry| &entry.successors)
	}

	fn neighbor_keys(&self, v: Key, list: impl Fn(&NodeEntry<N>) -> &Vec<(Key, u32)>) -> Vec<Key> {
		let Some(entry) = self.nodes.get(&v) else { return Vec::new() };
		let mut keys: Vec<Key> = list(entry).iter().map(|&(key, _)| key).collect();
		if self.array_index_nodes > 0 {
			js_key_order(&mut keys);
		}
		keys
	}

	pub fn sources(&self) -> Vec<Key> {
		self.nodes()
			.into_iter()
			.filter(|v| self.nodes.get(v).is_some_and(|entry| entry.in_edges.is_empty()))
			.collect()
	}

	pub fn edges(&self) -> Vec<Edge> {
		self.edges.iter().flatten().map(|entry| entry.edge).collect()
	}

	fn slot(&self, edge: Edge) -> Option<usize> {
		let slot = self.edge_slots.get(&edge).copied();
		match slot {
			None if !self.directed => {
				self.edge_slots.get(&Edge { v: edge.w, w: edge.v, name: edge.name }).copied()
			}
			_ => slot,
		}
	}

	pub fn edge(&self, edge: Edge) -> Option<&E> {
		Some(&self.edges.get(self.slot(edge)?)?.as_ref()?.label)
	}

	pub fn edge_mut(&mut self, edge: Edge) -> Option<&mut E> {
		let slot = self.slot(edge)?;
		Some(&mut self.edges.get_mut(slot)?.as_mut()?.label)
	}

	/// `setEdge(v, w, label, name)`: adds the edge (and missing nodes) or replaces its label. An
	/// undirected graph keeps the ends in the given order, so callers order them like graphlib.
	pub fn set_edge(&mut self, v: Key, w: Key, name: Option<Key>, label: E) {
		let edge = Edge { v, w, name };
		if let Some(slot) = self.slot(edge) {
			if let Some(entry) = self.edges.get_mut(slot).and_then(Option::as_mut) {
				entry.label = label;
			}
			return;
		}
		self.ensure_node(v);
		self.ensure_node(w);
		let slot = self.edges.len();
		self.edges.push(Some(EdgeEntry { edge, label }));
		self.edge_slots.insert(edge, slot);
		if let Some(entry) = self.nodes.get_mut(&w) {
			entry.in_edges.push(slot);
			increment(&mut entry.predecessors, v);
		}
		if let Some(entry) = self.nodes.get_mut(&v) {
			entry.out_edges.push(slot);
			increment(&mut entry.successors, w);
		}
	}

	pub fn remove_edge(&mut self, edge: Edge) -> Option<E> {
		let slot = self.slot(edge)?;
		let entry = self.edges.get_mut(slot)?.take()?;
		let Edge { v, w, .. } = entry.edge;
		self.edge_slots.remove(&entry.edge);
		if let Some(node) = self.nodes.get_mut(&w) {
			node.in_edges.retain(|&other| other != slot);
			decrement(&mut node.predecessors, v);
		}
		if let Some(node) = self.nodes.get_mut(&v) {
			node.out_edges.retain(|&other| other != slot);
			decrement(&mut node.successors, w);
		}
		Some(entry.label)
	}

	fn edge_list(&self, slots: &[usize]) -> Vec<Edge> {
		slots
			.iter()
			.filter_map(|&slot| self.edges.get(slot)?.as_ref().map(|entry| entry.edge))
			.collect()
	}

	/// `inEdges(v)`: edges into `v`, in insertion order.
	pub fn in_edges(&self, v: Key) -> Vec<Edge> {
		self.nodes.get(&v).map(|entry| self.edge_list(&entry.in_edges)).unwrap_or_default()
	}

	pub fn out_edges(&self, v: Key) -> Vec<Edge> {
		self.nodes.get(&v).map(|entry| self.edge_list(&entry.out_edges)).unwrap_or_default()
	}

	/// `nodeEdges(v)`: in-edges, then out-edges.
	pub fn node_edges(&self, v: Key) -> Vec<Edge> {
		let mut edges = self.in_edges(v);
		edges.extend(self.out_edges(v));
		edges
	}
}

fn increment(counts: &mut Vec<(Key, u32)>, key: Key) {
	match counts.iter_mut().find(|(other, _)| *other == key) {
		Some((_, count)) => *count += 1,
		None => counts.push((key, 1)),
	}
}

fn decrement(counts: &mut Vec<(Key, u32)>, key: Key) {
	if let Some(index) = counts.iter().position(|(other, _)| *other == key) {
		counts[index].1 -= 1;
		if counts[index].1 == 0 {
			counts.remove(index);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn orders_keys_like_javascript_objects() {
		let mut keys = Keys::default();
		let names = ["b", "10", "a", "2", "01", "4294967295"];
		let interned: Vec<Key> = names.iter().map(|name| keys.intern(name)).collect();
		let mut graph: Graph<(), ()> = Graph::new(true, true);
		for &key in &interned {
			graph.ensure_node(key);
		}
		let order: Vec<&str> = graph.nodes().iter().map(|&key| keys.name(key)).collect();
		assert_eq!(order, ["2", "10", "b", "a", "01", "4294967295"]);
		graph.remove_node(interned[0]);
		graph.ensure_node(interned[0]);
		let order: Vec<&str> = graph.nodes().iter().map(|&key| keys.name(key)).collect();
		assert_eq!(order, ["2", "10", "a", "01", "4294967295", "b"]);
	}

	#[test]
	fn keeps_compound_structure_and_edges() {
		let mut keys = Keys::default();
		let [a, b, c, cluster] = ["a", "b", "c", "cluster"].map(|name| keys.intern(name));
		let mut graph: Graph<u32, u32> = Graph::new(true, true);
		graph.set_node(a, 1);
		assert!(graph.set_parent(a, Some(cluster)));
		assert!(graph.set_parent(b, Some(cluster)));
		assert!(!graph.set_parent(cluster, Some(a)));
		graph.set_edge(a, c, None, 5);
		graph.set_edge(a, c, Some(b), 6);
		graph.set_edge(b, c, None, 7);
		assert_eq!(graph.children(Some(cluster)), [a, b]);
		assert_eq!(graph.children(None), [cluster, c]);
		assert_eq!(graph.predecessors(c), [a, b]);
		assert_eq!(graph.edge(Edge { v: a, w: c, name: Some(b) }), Some(&6));
		graph.remove_node(cluster);
		assert_eq!(graph.children(None), [c, a, b]);
		graph.remove_node(a);
		assert_eq!(graph.edges(), [Edge { v: b, w: c, name: None }]);
		assert_eq!(graph.predecessors(c), [b]);
	}
}
