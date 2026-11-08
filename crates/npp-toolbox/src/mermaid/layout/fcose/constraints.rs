//! The constraints of fcose: their enforcement on the start positions (`ConstraintHandler.js`
//! without the transformation of randomized layouts) and their effect on every move
//! (`CoSELayout.initConstraintVariables` and `updateDisplacements` with
//! `RELAX_MOVEMENT_ON_CONSTRAINTS`). No node is fixed.
//!
//! JavaScript keeps these in `Map`s and `Set`s, whose iteration order is the insertion order;
//! [`OrderedMap`] does the same. Nodes of an alignment are represented by a dummy per alignment
//! list, named `"dummy" + i` in JavaScript.

use std::collections::{HashMap, VecDeque};

use super::Layout;

/// `{left, right, gap}` (horizontal) or `{top, bottom, gap}`: `second` is at least `gap` right
/// of or below `first`. A `NaN` gap stands for a missing one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RelativePlacement {
	pub first: usize,
	pub second: usize,
	pub is_horizontal: bool,
	pub gap: f64,
}

/// The constraints with layout node indices.
pub(super) struct Constraints {
	pub vertical: Option<Vec<Vec<usize>>>,
	pub horizontal: Option<Vec<Vec<usize>>>,
	pub relative: Option<Vec<RelativePlacement>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
	Node(usize),
	Dummy(usize),
}

/// A `Map` with JavaScript's iteration order.
#[derive(Clone, Debug)]
struct OrderedMap<V> {
	entries: Vec<(Key, V)>,
	index: HashMap<Key, usize>,
}

impl<V> OrderedMap<V> {
	fn new() -> Self {
		Self { entries: Vec::new(), index: HashMap::new() }
	}

	fn get(&self, key: Key) -> Option<&V> {
		self.index.get(&key).map(|&index| &self.entries[index].1)
	}

	fn get_mut(&mut self, key: Key) -> Option<&mut V> {
		self.index.get(&key).map(|&index| &mut self.entries[index].1)
	}

	fn contains(&self, key: Key) -> bool {
		self.index.contains_key(&key)
	}

	fn set(&mut self, key: Key, value: V) {
		match self.index.get(&key) {
			Some(&index) => self.entries[index].1 = value,
			None => {
				self.index.insert(key, self.entries.len());
				self.entries.push((key, value));
			}
		}
	}

	fn entry(&mut self, key: Key, default: impl FnOnce() -> V) -> &mut V {
		if !self.contains(key) {
			self.set(key, default());
		}
		let index = self.index[&key];
		&mut self.entries[index].1
	}

	fn keys(&self) -> impl Iterator<Item = Key> + '_ {
		self.entries.iter().map(|(key, _)| *key)
	}
}

/// An edge of a constraint graph: `{id, gap, direction}`.
#[derive(Clone, Copy, Debug)]
struct Adjacent {
	id: Key,
	gap: f64,
	is_horizontal: bool,
}

/// The alignment dummies of one direction: the nodes of each and the dummy of each node.
struct Dummies {
	members: Vec<Vec<usize>>,
	of_node: HashMap<usize, usize>,
}

impl Dummies {
	fn new(alignment: Option<&Vec<Vec<usize>>>) -> Self {
		let mut dummies = Dummies { members: Vec::new(), of_node: HashMap::new() };
		for (index, list) in alignment.into_iter().flatten().enumerate() {
			dummies.members.push(list.clone());
			for &node in list {
				dummies.of_node.insert(node, index);
			}
		}
		dummies
	}

	/// The dummy of a node, or the node.
	fn key_of(&self, node: Key) -> Key {
		match node {
			Key::Node(node) => self.of_node.get(&node).map_or(node_key(node), |&d| Key::Dummy(d)),
			dummy => dummy,
		}
	}
}

fn node_key(node: usize) -> Key {
	Key::Node(node)
}

/// What moves need of the constraints.
pub(super) struct Movement {
	vertical: Vec<Vec<usize>>,
	horizontal: Vec<Vec<usize>>,
	relative: Option<[Axis; 2]>,
}

/// The relative placement constraints of one axis while moving.
struct Axis {
	is_horizontal: bool,
	/// `nodesInRelativeHorizontal`.
	order: Vec<Key>,
	/// Per key: `(other is after it, other, gap)`.
	constraints: HashMap<Key, Vec<(bool, Key, f64)>>,
	/// `nodeToTempPositionMap…`.
	positions: HashMap<Key, f64>,
	dummies: Dummies,
}

/// `ConstraintHandler.handleConstraints` with enforcement only, then
/// `initConstraintVariables`.
pub(super) fn handle(layout: &mut Layout, constraints: &mut Constraints) -> Movement {
	let leaves: Vec<usize> = layout
		.all_nodes
		.iter()
		.copied()
		.filter(|&node| layout.nodes[node].child.is_none())
		.collect();
	let mut index_of: HashMap<usize, usize> = HashMap::new();
	let mut x_coords = Vec::with_capacity(leaves.len());
	let mut y_coords = Vec::with_capacity(leaves.len());
	for (index, &node) in leaves.iter().enumerate() {
		index_of.insert(node, index);
		let (x, y) = layout.center(node);
		x_coords.push(x);
		y_coords.push(y);
	}
	if let Some(relative) = &mut constraints.relative {
		for constraint in relative.iter_mut() {
			if constraint.gap.is_nan() {
				let size = |node: usize| match constraint.is_horizontal {
					true => layout.nodes[node].rect.width,
					false => layout.nodes[node].rect.height,
				};
				constraint.gap = layout.constants.edge_length
					+ size(constraint.first) / 2.0
					+ size(constraint.second) / 2.0;
			}
		}
	}
	let coordinate =
		|coords: &[f64], node: usize| index_of.get(&node).map_or(f64::NAN, |&index| coords[index]);
	// The dag of the relative placement constraints.
	let mut dag: OrderedMap<Vec<Adjacent>> = OrderedMap::new();
	for constraint in constraints.relative.iter().flatten() {
		let (first, second) = (node_key(constraint.first), node_key(constraint.second));
		let adjacent =
			Adjacent { id: second, gap: constraint.gap, is_horizontal: constraint.is_horizontal };
		dag.entry(first, Vec::new).push(adjacent);
		dag.entry(second, Vec::new);
	}
	// Alignment.
	for (alignment, coords) in
		[(&constraints.vertical, &mut x_coords), (&constraints.horizontal, &mut y_coords)]
	{
		for list in alignment.iter().flatten() {
			let mut set: Vec<usize> = Vec::new();
			for &node in list {
				if !set.contains(&node) {
					set.push(node);
				}
			}
			let sum = set.iter().fold(0.0, |sum, &node| sum + coordinate(coords, node));
			let position = sum / set.len() as f64;
			for &node in &set {
				if let Some(&index) = index_of.get(&node) {
					coords[index] = position;
				}
			}
		}
	}
	// Relative placement.
	if constraints.relative.is_some() {
		let vertical_dummies = Dummies::new(constraints.vertical.as_ref());
		let horizontal_dummies = Dummies::new(constraints.horizontal.as_ref());
		let dummy_positions = |dummies: &Dummies, coords: &[f64]| -> Vec<f64> {
			dummies
				.members
				.iter()
				.map(|members| members.first().map_or(f64::NAN, |&node| coordinate(coords, node)))
				.collect()
		};
		let vertical_positions = dummy_positions(&vertical_dummies, &x_coords);
		let horizontal_positions = dummy_positions(&horizontal_dummies, &y_coords);
		let mut dag_horizontal: OrderedMap<Vec<Adjacent>> = OrderedMap::new();
		let mut dag_vertical: OrderedMap<Vec<Adjacent>> = OrderedMap::new();
		for (node, adjacents) in &dag.entries {
			for adjacent in adjacents {
				let (dummies, axis_dag) = match adjacent.is_horizontal {
					true => (&vertical_dummies, &mut dag_horizontal),
					false => (&horizontal_dummies, &mut dag_vertical),
				};
				let source = dummies.key_of(*node);
				let target = Adjacent { id: dummies.key_of(adjacent.id), ..*adjacent };
				axis_dag.entry(source, Vec::new).push(target);
				axis_dag.entry(target.id, Vec::new);
			}
		}
		let horizontal_map = appropriate_positions(&dag_horizontal, &|key| match key {
			Key::Node(node) => coordinate(&x_coords, node),
			Key::Dummy(dummy) => vertical_positions.get(dummy).copied().unwrap_or(f64::NAN),
		});
		let vertical_map = appropriate_positions(&dag_vertical, &|key| match key {
			Key::Node(node) => coordinate(&y_coords, node),
			Key::Dummy(dummy) => horizontal_positions.get(dummy).copied().unwrap_or(f64::NAN),
		});
		for (map, dummies, coords) in [
			(&horizontal_map, &vertical_dummies, &mut x_coords),
			(&vertical_map, &horizontal_dummies, &mut y_coords),
		] {
			for (key, position) in &map.entries {
				match key {
					Key::Dummy(dummy) => {
						for node in dummies.members.get(*dummy).into_iter().flatten() {
							if let Some(&index) = index_of.get(node) {
								coords[index] = *position;
							}
						}
					}
					Key::Node(node) => {
						if let Some(&index) = index_of.get(node) {
							coords[index] = *position;
						}
					}
				}
			}
		}
	}
	for (index, &node) in leaves.iter().enumerate() {
		layout.set_center(node, x_coords[index], y_coords[index]);
	}
	movement(layout, constraints)
}

/// `findAppropriatePositionForRelativePlacement` in the enforcement phase, where the set of
/// fixed nodes is given but empty: the sources of each weakly connected component start at
/// their average position.
fn appropriate_positions(
	graph: &OrderedMap<Vec<Adjacent>>,
	position_before: &dyn Fn(Key) -> f64,
) -> OrderedMap<f64> {
	// The component sources (`dagToUndirected`, `findComponents`, `dagToReversed`).
	let mut undirected: OrderedMap<Vec<Key>> = OrderedMap::new();
	let mut has_predecessor: HashMap<Key, bool> = HashMap::new();
	for key in graph.keys() {
		undirected.set(key, Vec::new());
	}
	for (key, adjacents) in &graph.entries {
		for adjacent in adjacents {
			if let Some(list) = undirected.get_mut(*key) {
				list.push(adjacent.id);
			}
			if let Some(list) = undirected.get_mut(adjacent.id) {
				list.push(*key);
			}
			has_predecessor.insert(adjacent.id, true);
		}
	}
	let mut component_sources: Vec<Vec<Key>> = Vec::new();
	let mut visited: HashMap<Key, bool> = HashMap::new();
	for start in undirected.keys() {
		if visited.contains_key(&start) {
			continue;
		}
		let mut component = vec![start];
		visited.insert(start, true);
		let mut queue = VecDeque::from([start]);
		while let Some(current) = queue.pop_front() {
			for &neighbor in undirected.get(current).into_iter().flatten() {
				if let std::collections::hash_map::Entry::Vacant(entry) = visited.entry(neighbor) {
					entry.insert(true);
					component.push(neighbor);
					queue.push_back(neighbor);
				}
			}
		}
		component.retain(|key| !has_predecessor.contains_key(key));
		component_sources.push(component);
	}

	let mut in_degrees: OrderedMap<i64> = OrderedMap::new();
	for key in graph.keys() {
		in_degrees.set(key, 0);
	}
	for (_, adjacents) in &graph.entries {
		for adjacent in adjacents {
			if let Some(degree) = in_degrees.get_mut(adjacent.id) {
				*degree += 1;
			}
		}
	}
	let mut positions: OrderedMap<f64> = OrderedMap::new();
	let mut past: HashMap<Key, Vec<Key>> = HashMap::new();
	let mut queue = VecDeque::new();
	for (key, degree) in &in_degrees.entries {
		if *degree == 0 {
			queue.push_back(*key);
		} else {
			positions.set(*key, f64::NEG_INFINITY);
		}
		past.insert(*key, vec![*key]);
	}
	for component in &component_sources {
		let sum = component.iter().fold(0.0, |sum, &key| sum + position_before(key));
		let position = sum / component.len() as f64;
		for &key in component {
			positions.set(key, position);
		}
	}
	let position_of =
		|positions: &OrderedMap<f64>, key: Key| positions.get(key).copied().unwrap_or(f64::NAN);
	while let Some(current) = queue.pop_front() {
		for adjacent in graph.get(current).into_iter().flatten() {
			let reached = position_of(&positions, current) + adjacent.gap;
			if position_of(&positions, adjacent.id) < reached {
				positions.set(adjacent.id, reached);
			}
			if let Some(degree) = in_degrees.get_mut(adjacent.id) {
				*degree -= 1;
				if *degree == 0 {
					queue.push_back(adjacent.id);
				}
			}
			let mut union = past.get(&current).cloned().unwrap_or_default();
			for &key in past.get(&adjacent.id).into_iter().flatten() {
				if !union.contains(&key) {
					union.push(key);
				}
			}
			past.insert(adjacent.id, union);
		}
	}
	// Each group of sinks that share an ancestor keeps the center of its range.
	let mut components: Vec<Vec<Key>> = Vec::new();
	for key in in_degrees.keys() {
		let is_sink = graph.get(key).is_some_and(Vec::is_empty);
		let Some(value) = past.get(&key).filter(|_| is_sink) else { continue };
		let first = value.first().copied();
		let existing = components
			.iter()
			.rposition(|component| first.is_some_and(|first| component.contains(&first)));
		match existing {
			None => components.push(value.clone()),
			Some(index) => {
				for &key in value {
					if !components[index].contains(&key) {
						components[index].push(key);
					}
				}
			}
		}
	}
	for component in &components {
		let (mut min_before, mut max_before) = (f64::INFINITY, f64::NEG_INFINITY);
		let (mut min_after, mut max_after) = (f64::INFINITY, f64::NEG_INFINITY);
		for &key in component {
			let before = position_before(key);
			let after = position_of(&positions, key);
			if before < min_before {
				min_before = before;
			}
			if before > max_before {
				max_before = before;
			}
			if after < min_after {
				min_after = after;
			}
			if after > max_after {
				max_after = after;
			}
		}
		let diff = (min_before + max_before) / 2.0 - (min_after + max_after) / 2.0;
		for &key in component {
			let position = position_of(&positions, key) + diff;
			positions.set(key, position);
		}
	}
	positions
}

/// `initConstraintVariables`.
fn movement(layout: &Layout, constraints: &Constraints) -> Movement {
	let relative = constraints.relative.as_ref().map(|relative| {
		let mut axes = [true, false].map(|is_horizontal| Axis {
			is_horizontal,
			order: Vec::new(),
			constraints: HashMap::new(),
			positions: HashMap::new(),
			dummies: Dummies::new(match is_horizontal {
				true => constraints.vertical.as_ref(),
				false => constraints.horizontal.as_ref(),
			}),
		});
		for constraint in relative {
			let axis = &mut axes[usize::from(!constraint.is_horizontal)];
			let first = axis.dummies.key_of(node_key(constraint.first));
			let second = axis.dummies.key_of(node_key(constraint.second));
			for key in [first, second] {
				if !axis.order.contains(&key) {
					axis.order.push(key);
					axis.constraints.insert(key, Vec::new());
					let node = match key {
						Key::Node(node) => Some(node),
						Key::Dummy(dummy) => axis.dummies.members[dummy].first().copied(),
					};
					let position = node.map_or(f64::NAN, |node| {
						let (x, y) = layout.center(node);
						if axis.is_horizontal { x } else { y }
					});
					axis.positions.insert(key, position);
				}
			}
			if let Some(list) = axis.constraints.get_mut(&first) {
				list.push((true, second, constraint.gap));
			}
			if let Some(list) = axis.constraints.get_mut(&second) {
				list.push((false, first, constraint.gap));
			}
		}
		axes
	});
	Movement {
		vertical: constraints.vertical.clone().unwrap_or_default(),
		horizontal: constraints.horizontal.clone().unwrap_or_default(),
		relative,
	}
}

impl Movement {
	/// `updateDisplacements`.
	pub(super) fn update_displacements(&mut self, layout: &mut Layout) {
		for (lists, is_x) in [(&self.vertical, true), (&self.horizontal, false)] {
			for list in lists {
				let displacement = |layout: &Layout, node: usize| match is_x {
					true => layout.nodes[node].displacement.0,
					false => layout.nodes[node].displacement.1,
				};
				let total = list.iter().fold(0.0, |sum, &node| sum + displacement(layout, node));
				let average = total / list.len() as f64;
				for &node in list {
					match is_x {
						true => layout.nodes[node].displacement.0 = average,
						false => layout.nodes[node].displacement.1 = average,
					}
				}
			}
		}
		let Some(axes) = &mut self.relative else { return };
		if layout.total_iterations.is_multiple_of(10) {
			for axis in axes.iter_mut() {
				shuffle(&mut axis.order, &mut *layout.random);
			}
		}
		for axis in axes.iter_mut() {
			let get = |layout: &Layout, node: usize| match axis.is_horizontal {
				true => layout.nodes[node].displacement.0,
				false => layout.nodes[node].displacement.1,
			};
			for position in 0..axis.order.len() {
				let key = axis.order[position];
				let mut displacement = match key {
					Key::Node(node) => get(layout, node),
					Key::Dummy(dummy) => axis.dummies.members[dummy]
						.first()
						.map_or(f64::NAN, |&node| get(layout, node)),
				};
				let temporary = |key: Key| axis.positions.get(&key).copied().unwrap_or(f64::NAN);
				for &(is_after, other, gap) in axis.constraints.get(&key).into_iter().flatten() {
					if is_after {
						let diff = (temporary(other) - temporary(key)) - displacement;
						if diff < gap {
							displacement -= gap - diff;
						}
					} else {
						let diff = (temporary(key) - temporary(other)) + displacement;
						if diff < gap {
							displacement += gap - diff;
						}
					}
				}
				let moved = temporary(key) + displacement;
				axis.positions.insert(key, moved);
				let nodes = match key {
					Key::Node(node) => vec![node],
					Key::Dummy(dummy) => axis.dummies.members[dummy].clone(),
				};
				for node in nodes {
					match axis.is_horizontal {
						true => layout.nodes[node].displacement.0 = displacement,
						false => layout.nodes[node].displacement.1 = displacement,
					}
				}
			}
		}
	}
}

/// `this.shuffle`: swaps the last third of the array with random elements.
fn shuffle(array: &mut [Key], random: &mut dyn FnMut() -> f64) {
	let length = array.len() as f64;
	let mut index = array.len() as f64 - 1.0;
	while index >= 0.0 && index >= 2.0 * length / 3.0 {
		let other = (random() * (index + 1.0)).floor();
		let (i, j) = (index as usize, other as usize);
		if j < array.len() {
			array.swap(i, j);
		}
		index -= 1.0;
	}
}
