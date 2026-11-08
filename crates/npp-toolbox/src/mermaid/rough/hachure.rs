//! `hachureLines` of hachure-fill 0.5.2 (MIT, copyright (c) Preet Shihn).

use super::super::color::js_round;
use super::super::common::v8_sort;
use super::Xy;

/// The scan lines per call. They are a pixel apart, so only absurdly large shapes end early.
const MAX_SCAN_LINES: u32 = 100_000;

/// The largest coordinate that gets a hachure. Beyond it a scan may not end.
const MAX_COORDINATE: f64 = 1e9;

#[derive(Clone, Copy)]
struct Edge {
	y_min: f64,
	y_max: f64,
	x: f64,
	inverse_slope: f64,
}

fn rotate_points(points: &mut [Xy], degrees: f64) {
	let angle = std::f64::consts::PI / 180.0 * degrees;
	let (sin, cos) = (angle.sin(), angle.cos());
	for point in points {
		let [x, y] = *point;
		*point = [x * cos - y * sin, x * sin + y * cos];
	}
}

/// The polygons stay turned back in place, with their rounding errors, which `cross-hatch` sees.
pub fn hachure_lines(polygons: &mut [Vec<Xy>], gap: f64, angle: f64, step: f64) -> Vec<[Xy; 2]> {
	let gap = gap.max(0.1);
	let is_turned = angle != 0.0;
	if is_turned {
		for polygon in polygons.iter_mut() {
			rotate_points(polygon, angle);
		}
	}
	let mut lines = straight_hachure_lines(polygons, gap, step);
	if is_turned {
		for polygon in polygons.iter_mut() {
			rotate_points(polygon, -angle);
		}
		for line in &mut lines {
			rotate_points(line, -angle);
		}
	}
	lines
}

fn sorted_edges(polygons: &[Vec<Xy>]) -> Vec<Edge> {
	let mut edges = Vec::new();
	for polygon in polygons {
		let (Some(first), Some(last)) = (polygon.first(), polygon.last()) else {
			continue;
		};
		let closing = (first != last).then_some(*first);
		if polygon.len() + usize::from(closing.is_some()) <= 2 {
			continue;
		}
		let vertices = polygon.iter().chain(closing.iter());
		for (from, to) in vertices.clone().zip(vertices.skip(1)) {
			if from[1] != to[1] {
				let y_min = from[1].min(to[1]);
				edges.push(Edge {
					y_min,
					y_max: from[1].max(to[1]),
					x: if y_min == from[1] { from[0] } else { to[0] },
					inverse_slope: (to[0] - from[0]) / (to[1] - from[1]),
				});
			}
		}
	}
	v8_sort(&mut edges, |a, b| {
		if a.y_min != b.y_min {
			return a.y_min - b.y_min;
		}
		if a.x != b.x {
			return a.x - b.x;
		}
		a.y_max - b.y_max
	});
	edges
}

fn straight_hachure_lines(polygons: &[Vec<Xy>], gap: f64, step: f64) -> Vec<[Xy; 2]> {
	let mut lines = Vec::new();
	let is_usable = polygons.iter().flatten().flatten().all(|c| c.abs() <= MAX_COORDINATE);
	if !is_usable {
		return lines;
	}
	let mut edges = sorted_edges(polygons);
	let Some(first) = edges.first() else {
		return lines;
	};
	let mut y = first.y_min;
	let mut next_edge = 0;
	let mut active: Vec<usize> = Vec::new();
	let mut iteration = 0u32;
	while (!active.is_empty() || next_edge < edges.len()) && iteration < MAX_SCAN_LINES {
		while next_edge < edges.len() && edges[next_edge].y_min <= y {
			active.push(next_edge);
			next_edge += 1;
		}
		active.retain(|&edge| edges[edge].y_max > y);
		v8_sort(&mut active, |&a, &b| {
			let difference = edges[a].x - edges[b].x;
			// The slope of a nearly horizontal edge can overflow: `NaN` keeps the order.
			if difference.is_nan() { 0.0 } else { difference }
		});
		if step != 1.0 || f64::from(iteration) % gap == 0.0 {
			for pair in active.as_chunks::<2>().0 {
				let from = js_round(edges[pair[0]].x);
				let to = js_round(edges[pair[1]].x);
				lines.push([[from, y], [to, y]]);
			}
		}
		y += step;
		for &edge in &active {
			edges[edge].x += step * edges[edge].inverse_slope;
		}
		iteration += 1;
	}
	lines
}
