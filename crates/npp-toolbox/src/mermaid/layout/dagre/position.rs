//! Positioning (`position/`): y by rank, x by Brandes and Köpf's four alignments, balanced.

use std::collections::HashSet;

use super::super::graph::{Edge, Graph, Key, KeyMap};
use super::{
	BorderType, Dummy, LabelPos, Layout, js_max, js_min, lodash_max, lodash_min, stable_sort_by,
};

/// The non-compound view of the layout graph, sharing its labels.
type Flat = Graph<usize, usize>;

type Conflicts = HashSet<(Key, Key)>;

pub(super) fn position(layout: &mut Layout) {
	let mut g: Flat = Graph::new(true, false);
	for v in layout.g.nodes() {
		if !layout.g.has_children(v)
			&& let Some(&label) = layout.g.node(v)
		{
			g.set_node(v, label);
		}
	}
	for edge in layout.g.edges() {
		if let Some(&label) = layout.g.edge(edge) {
			g.set_edge(edge.v, edge.w, edge.name, label);
		}
	}
	let layering = layout.layer_matrix(&g);
	position_y(layout, &layering);
	let xs = position_x(layout, &g, &layering);
	for (v, x) in xs {
		if let Some(node) = g.node(v).and_then(|&index| layout.nodes.get_mut(index)) {
			node.x = x;
		}
	}
}

fn label<'a>(layout: &'a Layout, g: &Flat, v: Key) -> &'a super::NodeLabel {
	g.node(v).and_then(|&index| layout.nodes.get(index)).unwrap_or(&layout.missing_node)
}

fn position_y(layout: &mut Layout, layering: &[Vec<Key>]) {
	let rank_sep = layout.g.label.ranksep;
	let mut previous_y = 0.0;
	for layer in layering {
		// dagre's maximum of an empty layer is `undefined`, which would make all later y NaN.
		let max_height = lodash_max(layer.iter().map(|&v| layout.node(v).height)).unwrap_or(0.0);
		for &v in layer {
			layout.node_mut(v).y = previous_y + max_height / 2.0;
		}
		previous_y += max_height + rank_sep;
	}
}

fn is_dummy(layout: &Layout, g: &Flat, v: Key) -> bool {
	label(layout, g, v).dummy.is_some()
}

fn order(layout: &Layout, g: &Flat, v: Key) -> f64 {
	label(layout, g, v).order.map_or(f64::NAN, |order| order as f64)
}

/// Conflicts are unordered pairs; dagre orders them by name, any order serves as well.
fn conflict_key(v: Key, w: Key) -> (Key, Key) {
	if v <= w { (v, w) } else { (w, v) }
}

fn find_type1_conflicts(
	layout: &Layout,
	g: &Flat,
	layering: &[Vec<Key>],
	conflicts: &mut Conflicts,
) {
	for pair in layering.windows(2) {
		let (previous_layer, layer) = (&pair[0], &pair[1]);
		let mut k0 = 0.0;
		let mut scan_position = 0;
		let previous_layer_length = previous_layer.len() as f64;
		let last_node = layer.last().copied();
		for (index, &v) in layer.iter().enumerate() {
			let w = match is_dummy(layout, g, v) {
				true => g.predecessors(v).into_iter().find(|&u| is_dummy(layout, g, u)),
				false => None,
			};
			let k1 = w.map_or(previous_layer_length, |w| order(layout, g, w));
			if w.is_some() || Some(v) == last_node {
				for &scan_node in &layer[scan_position..=index] {
					for u in g.predecessors(scan_node) {
						let u_position = order(layout, g, u);
						if (u_position < k0 || k1 < u_position)
							&& !(is_dummy(layout, g, u) && is_dummy(layout, g, scan_node))
						{
							conflicts.insert(conflict_key(u, scan_node));
						}
					}
				}
				scan_position = index + 1;
				k0 = k1;
			}
		}
	}
}

fn find_type2_conflicts(
	layout: &Layout,
	g: &Flat,
	layering: &[Vec<Key>],
	conflicts: &mut Conflicts,
) {
	let scan = |conflicts: &mut Conflicts,
	            south: &[Key],
	            range: std::ops::Range<usize>,
	            previous_north_border: Option<f64>,
	            next_north_border: Option<f64>| {
		for &v in south.get(range).unwrap_or(&[]) {
			if !is_dummy(layout, g, v) {
				continue;
			}
			for u in g.predecessors(v) {
				let u_order = order(layout, g, u);
				// A comparison with `undefined` is false.
				let outside = previous_north_border.is_some_and(|border| u_order < border)
					|| next_north_border.is_some_and(|border| u_order > border);
				if is_dummy(layout, g, u) && outside {
					conflicts.insert(conflict_key(u, v));
				}
			}
		}
	};
	for pair in layering.windows(2) {
		let (north, south) = (&pair[0], &pair[1]);
		let mut previous_north_position = Some(-1.0);
		let mut next_north_position: Option<f64> = None;
		let mut south_position = 0;
		for (south_lookahead, &v) in south.iter().enumerate() {
			if label(layout, g, v).dummy == Some(Dummy::Border)
				&& let Some(&predecessor) = g.predecessors(v).first()
			{
				next_north_position = Some(order(layout, g, predecessor));
				scan(
					conflicts,
					south,
					south_position..south_lookahead,
					previous_north_position,
					next_north_position,
				);
				south_position = south_lookahead;
				previous_north_position = next_north_position;
			}
			// dagre scans the rest after every node, not once after the loop.
			scan(
				conflicts,
				south,
				south_position..south.len(),
				next_north_position,
				Some(north.len() as f64),
			);
		}
	}
}

struct Alignment {
	root: KeyMap<Key>,
	/// The nodes in the order of dagre's `align` object.
	order: Vec<Key>,
}

fn vertical_alignment(
	g: &Flat,
	layering: &[Vec<Key>],
	conflicts: &Conflicts,
	upward: bool,
) -> Alignment {
	let mut root = KeyMap::default();
	let mut align = KeyMap::default();
	let mut position = KeyMap::default();
	let mut order = Vec::new();
	for layer in layering {
		for (index, &v) in layer.iter().enumerate() {
			root.insert(v, v);
			if align.insert(v, v).is_none() {
				order.push(v);
			}
			position.insert(v, index);
		}
	}
	for layer in layering {
		let mut previous_index: isize = -1;
		for &v in layer {
			let mut ws = if upward { g.predecessors(v) } else { g.successors(v) };
			if ws.is_empty() {
				continue;
			}
			let position_of = |w: &Key| position.get(w).copied();
			stable_sort_by(&mut ws, |a, b| match (position_of(a), position_of(b)) {
				(Some(a), Some(b)) => a < b,
				(Some(_), None) => true,
				_ => false,
			});
			let middle = (ws.len() as f64 - 1.0) / 2.0;
			for index in (middle.floor() as usize)..=(middle.ceil() as usize) {
				let Some(&w) = ws.get(index) else { continue };
				let Some(w_position) = position_of(&w) else { continue };
				if align.get(&v) == Some(&v)
					&& previous_index < w_position as isize
					&& !conflicts.contains(&conflict_key(v, w))
				{
					align.insert(w, v);
					let w_root = root.get(&w).copied().unwrap_or(w);
					align.insert(v, w_root);
					root.insert(v, w_root);
					previous_index = w_position as isize;
				}
			}
		}
	}
	Alignment { root, order }
}

/// `sep`: the minimum distance between the centers of neighbors `v` and `w`.
fn separation(layout: &Layout, g: &Flat, v: Key, w: Key, reverse_sep: bool) -> f64 {
	let (node_sep, edge_sep) = (layout.g.label.nodesep, layout.g.label.edgesep);
	let v_label = label(layout, g, v);
	let w_label = label(layout, g, w);
	let mut sum = v_label.width / 2.0;
	let delta = match v_label.labelpos {
		Some(LabelPos::Left) => -v_label.width / 2.0,
		Some(LabelPos::Right) => v_label.width / 2.0,
		_ => 0.0,
	};
	if delta != 0.0 && !delta.is_nan() {
		sum += if reverse_sep { delta } else { -delta };
	}
	sum += if v_label.dummy.is_some() { edge_sep } else { node_sep } / 2.0;
	sum += if w_label.dummy.is_some() { edge_sep } else { node_sep } / 2.0;
	sum += w_label.width / 2.0;
	let delta = match w_label.labelpos {
		Some(LabelPos::Left) => w_label.width / 2.0,
		Some(LabelPos::Right) => -w_label.width / 2.0,
		_ => 0.0,
	};
	if delta != 0.0 && !delta.is_nan() {
		sum += if reverse_sep { delta } else { -delta };
	}
	sum
}

fn horizontal_compaction(
	layout: &Layout,
	g: &Flat,
	layering: &[Vec<Key>],
	alignment: &Alignment,
	reverse_sep: bool,
) -> KeyMap<f64> {
	let root_of = |v: Key| alignment.root.get(&v).copied().unwrap_or(v);
	// buildBlockGraph
	let mut block_graph: Graph<(), f64> = Graph::new(true, false);
	for layer in layering {
		let mut previous: Option<Key> = None;
		for &v in layer {
			let v_root = root_of(v);
			block_graph.ensure_node(v_root);
			if let Some(u) = previous {
				let u_root = root_of(u);
				let edge = Edge { v: u_root, w: v_root, name: None };
				let previous_max = block_graph.edge(edge).copied().unwrap_or(0.0);
				let previous_max = if previous_max.is_nan() { 0.0 } else { previous_max };
				let separation = separation(layout, g, v, u, reverse_sep);
				block_graph.set_edge(u_root, v_root, None, js_max(separation, previous_max));
			}
			previous = Some(v);
		}
	}
	let border_type = if reverse_sep { BorderType::Left } else { BorderType::Right };
	let mut xs: KeyMap<f64> = KeyMap::default();
	let x_of = |xs: &KeyMap<f64>, v: Key| xs.get(&v).copied().unwrap_or(f64::NAN);
	// iterate: a depth-first walk with an explicit stack, where nodes seen a second time are
	// placed. The stack may hold a node several times; dagre places it each time.
	let iterate = |xs: &mut KeyMap<f64>, first_pass: bool| {
		let mut stack = block_graph.nodes();
		let mut visited = KeyMap::default();
		while let Some(element) = stack.pop() {
			if visited.insert(element, ()).is_some() {
				if first_pass {
					let x = block_graph.in_edges(element).iter().fold(0.0, |accumulator, &edge| {
						let separation = block_graph.edge(edge).copied().unwrap_or(f64::NAN);
						js_max(accumulator, x_of(xs, edge.v) + separation)
					});
					xs.insert(element, x);
				} else {
					let minimum = block_graph.out_edges(element).iter().fold(
						f64::INFINITY,
						|accumulator, &edge| {
							let separation = block_graph.edge(edge).copied().unwrap_or(f64::NAN);
							js_min(accumulator, x_of(xs, edge.w) - separation)
						},
					);
					let node = label(layout, g, element);
					if minimum != f64::INFINITY && node.border_type != Some(border_type) {
						let x = js_max(x_of(xs, element), minimum);
						xs.insert(element, x);
					}
				}
			} else {
				stack.push(element);
				match first_pass {
					true => stack.extend(block_graph.predecessors(element)),
					false => stack.extend(block_graph.successors(element)),
				}
			}
		}
	};
	iterate(&mut xs, true);
	iterate(&mut xs, false);
	for &v in &alignment.order {
		let x = x_of(&xs, root_of(v));
		xs.insert(v, x);
	}
	xs
}

/// The x of every node of the four alignments up-left, up-right, down-left and down-right.
fn position_x(layout: &Layout, g: &Flat, layering: &[Vec<Key>]) -> Vec<(Key, f64)> {
	let mut conflicts = Conflicts::new();
	find_type1_conflicts(layout, g, layering, &mut conflicts);
	find_type2_conflicts(layout, g, layering, &mut conflicts);
	let mut alignments: Vec<KeyMap<f64>> = Vec::with_capacity(4);
	let mut orders: Vec<Vec<Key>> = Vec::with_capacity(4);
	for upward in [true, false] {
		let mut adjusted: Vec<Vec<Key>> = match upward {
			true => layering.to_vec(),
			false => layering.iter().rev().cloned().collect(),
		};
		for right in [false, true] {
			if right {
				adjusted =
					adjusted.iter().map(|layer| layer.iter().rev().copied().collect()).collect();
			}
			let alignment = vertical_alignment(g, &adjusted, &conflicts, upward);
			let mut xs = horizontal_compaction(layout, g, &adjusted, &alignment, right);
			if right {
				for x in xs.values_mut() {
					*x = -*x;
				}
			}
			alignments.push(xs);
			orders.push(alignment.order);
		}
	}
	// findSmallestWidthAlignment
	let width = |xs: &KeyMap<f64>| {
		let mut max = f64::NEG_INFINITY;
		let mut min = f64::INFINITY;
		for (&v, &x) in xs {
			let half_width = label(layout, g, v).width / 2.0;
			max = js_max(x + half_width, max);
			min = js_min(x - half_width, min);
		}
		max - min
	};
	let mut smallest = None;
	for (index, xs) in alignments.iter().enumerate() {
		let value = width(xs);
		if value.is_nan() {
			continue;
		}
		if smallest.is_none_or(|(_, minimum)| value < minimum) {
			smallest = Some((index, value));
		}
	}
	// alignCoordinates
	if let Some((align_to, _)) = smallest {
		let align_to_min = lodash_min(alignments[align_to].values().copied());
		let align_to_max = lodash_max(alignments[align_to].values().copied());
		for (index, is_left) in [(0, true), (1, false), (2, true), (3, false)] {
			if index == align_to {
				continue;
			}
			let delta = match is_left {
				true => align_to_min.zip(lodash_min(alignments[index].values().copied())),
				false => align_to_max.zip(lodash_max(alignments[index].values().copied())),
			}
			.map_or(f64::NAN, |(target, current)| target - current);
			if delta != 0.0 && !delta.is_nan() {
				for x in alignments[index].values_mut() {
					*x += delta;
				}
			}
		}
	}
	// balance: Mermaid sets no `align`, so the median of the four.
	orders[0]
		.iter()
		.map(|&v| {
			let x_in = |index: usize| alignments[index].get(&v).copied().unwrap_or(f64::NAN);
			let mut values = [x_in(0), x_in(1), x_in(2), x_in(3)];
			// lodash's ascending sort puts NaN last.
			stable_sort_by(&mut values, |a, b| !a.is_nan() && (b.is_nan() || a < b));
			(v, (values[1] + values[2]) / 2.0)
		})
		.collect()
}
