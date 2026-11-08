//! Coordinates of the layered nodes (`phase4.coordinates.ts`): every lane is a column as wide as
//! its widest layer, nodes are spread around the column centre, dummies of an edge line up
//! between its ends.

use std::collections::HashMap;

use super::{Config, Coordinates, Graph, Layout, ordering};
use crate::mermaid::color::js_round;

/// A column of the layout: a lane, or the dummies without one.
type Column = Option<usize>;

pub(super) fn assign(
	layers: &[Vec<usize>],
	graph: &Graph,
	layout: &Layout,
	config: Config,
	preferred: Option<&[usize]>,
) -> Coordinates {
	let layer_gap = config.layer_gap;
	let node_gap = config.node_gap;
	let lane_gap = node_gap * 2.0;
	let width = |node: usize| graph.arena[node].width;
	let height = |node: usize| graph.arena[node].height;
	let mut x: Vec<Option<f64>> = vec![None; graph.arena.len()];
	let mut y: Vec<Option<f64>> = vec![None; graph.arena.len()];
	let layer_heights: Vec<f64> = layers
		.iter()
		.map(|layer| layer.iter().map(|&node| height(node)).fold(0.0, f64::max))
		.collect();
	// LR and RL turn widths into horizontal spans, so the layer gaps widen up front.
	let mut extra_gaps = Vec::new();
	if layout.direction.is_horizontal() {
		for index in 0..layers.len().saturating_sub(1) {
			let widest =
				|layer: &[usize]| layer.iter().map(|&node| width(node)).fold(0.0, f64::max);
			let normal = layer_heights[index] / 2.0 + layer_heights[index + 1] / 2.0;
			let required = (widest(&layers[index]) + widest(&layers[index + 1])) / 2.0;
			extra_gaps.push((required - normal - layer_gap).max(0.0));
		}
	}
	let mut used: Vec<Column> = Vec::new();
	for &node in layers.iter().flatten() {
		if !used.contains(&graph.lane_of[node]) {
			used.push(graph.lane_of[node]);
		}
	}
	let mut columns: Vec<Column> = Vec::new();
	if used.contains(&None) {
		columns.push(None);
	}
	for lane in ordering::lane_order(graph, preferred) {
		if used.contains(&Some(lane)) {
			columns.push(Some(lane));
		}
	}
	let mut column_width: HashMap<Column, f64> =
		columns.iter().map(|&column| (column, 0.0)).collect();
	for layer in layers {
		let mut per_column: HashMap<Column, (f64, usize)> = HashMap::new();
		for &node in layer {
			let entry = per_column.entry(graph.lane_of[node]).or_insert((0.0, 0));
			entry.0 += width(node);
			entry.1 += 1;
		}
		for (column, (sum, count)) in per_column {
			let total = sum + node_gap * count.saturating_sub(1) as f64;
			let entry = column_width.entry(column).or_insert(0.0);
			*entry = entry.max(total);
		}
	}
	let widths: Vec<f64> =
		columns.iter().map(|column| column_width.get(column).copied().unwrap_or(0.0)).collect();
	let total_width =
		widths.iter().sum::<f64>() + lane_gap * columns.len().saturating_sub(1) as f64;
	let mut center_x: HashMap<Column, f64> = HashMap::new();
	let mut cursor = -total_width / 2.0;
	for (index, &column) in columns.iter().enumerate() {
		center_x.insert(column, cursor + widths[index] / 2.0);
		cursor += widths[index];
		if index + 1 < columns.len() {
			cursor += lane_gap;
		}
	}
	let mut y_offset = 0.0;
	for (index, layer) in layers.iter().enumerate() {
		let layer_height = layer_heights[index];
		for &column in &columns {
			let nodes: Vec<usize> =
				layer.iter().copied().filter(|&node| graph.lane_of[node] == column).collect();
			let Some(&center) = center_x.get(&column) else {
				continue;
			};
			let total = nodes.iter().map(|&node| width(node)).sum::<f64>()
				+ node_gap * nodes.len().saturating_sub(1) as f64;
			let mut start = center - total / 2.0;
			for &node in &nodes {
				x[node] = Some(match nodes.len() {
					1 => center,
					_ => start + width(node) / 2.0,
				});
				y[node] = Some(y_offset + layer_height / 2.0);
				start += width(node) + node_gap;
			}
		}
		y_offset += layer_height + layer_gap + extra_gaps.get(index).copied().unwrap_or(0.0);
	}
	// Dummy chains of an edge sit halfway between its ends.
	let mut chains: Vec<(&str, Vec<usize>)> = Vec::new();
	for (index, edge) in graph.edges.iter().enumerate() {
		let id = layout.edges[edge.reference].id.as_str();
		match chains.iter_mut().find(|(chain, _)| *chain == id) {
			Some((_, list)) => list.push(index),
			None => chains.push((id, vec![index])),
		}
	}
	for (_, chain) in chains {
		let reference = &layout.edges[graph.edges[chain[0]].reference];
		let coordinate = |id: &str| graph.by_id.get(id).and_then(|&node| x[node]).unwrap_or(0.0);
		let middle = js_round((coordinate(&reference.start) + coordinate(&reference.end)) / 2.0);
		for &edge in &chain {
			for node in [graph.edges[edge].source, graph.edges[edge].target] {
				let id = graph.id(node);
				if id != reference.start && id != reference.end && graph.arena[node].is_dummy {
					x[node] = Some(middle);
				}
			}
		}
	}
	Coordinates { x, y }
}
