//! `layoutArchitecture` of `architectureRenderer.ts`: the cytoscape graph Mermaid builds and
//! the parts of cytoscape 3.34 it depends on, around two runs of fcose.
//!
//! Cytoscape sizes compounds from their children's bounding boxes, labels included
//! (`compound-sizing-wrt-labels: include`), and fcose moves the result to the center of the
//! cytoscape bounding box of all elements, edges and their labels included. The edges end at
//! the manual endpoints Mermaid sets (`0 50%` for `L` and so on, offsets from the node center
//! in percent of the node size); segment edges bend at the point the segment weights of the
//! first run give.

use super::super::layout::fcose;
use super::super::layout::js_math;
use super::parser::Direction;

/// What a cytoscape node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Group,
	Service,
	Junction,
}

#[derive(Clone, Debug)]
pub struct Node {
	pub kind: Kind,
	pub parent: Option<usize>,
	/// The canvas size of the cytoscape label: the width rounded up and the font size.
	pub label: Option<(f64, f64)>,
}

#[derive(Clone, Debug)]
pub struct Edge {
	pub source: usize,
	pub target: usize,
	pub source_dir: Direction,
	pub target_dir: Direction,
	/// `segments` edges join an x and a y direction.
	pub is_segments: bool,
	pub label: Option<(f64, f64)>,
}

pub struct Config {
	pub icon_size: f64,
	/// The `padding` style of groups in pixels.
	pub padding: f64,
	pub iterations: f64,
	pub seed: u32,
	pub same_group_length: f64,
	pub cross_group_length: f64,
	pub same_group_elasticity: f64,
}

/// A cytoscape bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Box {
	pub x1: f64,
	pub y1: f64,
	pub x2: f64,
	pub y2: f64,
}

impl Box {
	const EMPTY: Box =
		Box { x1: f64::INFINITY, y1: f64::INFINITY, x2: f64::NEG_INFINITY, y2: f64::NEG_INFINITY };

	pub fn width(&self) -> f64 {
		self.x2 - self.x1
	}

	pub fn height(&self) -> f64 {
		self.y2 - self.y1
	}

	/// `updateBounds`: boxes without area are left out.
	fn update(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
		if x2 - x1 == 0.0 || y2 - y1 == 0.0 {
			return;
		}
		self.x1 = if x1 < self.x1 { x1 } else { self.x1 };
		self.x2 = if x2 > self.x2 { x2 } else { self.x2 };
		self.y1 = if y1 < self.y1 { y1 } else { self.y1 };
		self.y2 = if y2 > self.y2 { y2 } else { self.y2 };
	}

	fn update_from(&mut self, other: &Box) {
		self.update(other.x1, other.y1, other.x2, other.y2);
	}

	fn expand(&mut self, padding: f64) {
		self.x1 -= padding;
		self.x2 += padding;
		self.y1 -= padding;
		self.y2 += padding;
	}

	/// `noninf` of every side.
	fn finite(mut self) -> Box {
		for value in [&mut self.x1, &mut self.y1, &mut self.x2, &mut self.y2] {
			if value.is_infinite() {
				*value = 0.0;
			}
		}
		self
	}
}

/// The points of a drawn edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgePoints {
	pub start: (f64, f64),
	pub mid: (f64, f64),
	pub end: (f64, f64),
}

pub struct Layout {
	/// The node centers.
	pub positions: Vec<(f64, f64)>,
	pub edges: Vec<EdgePoints>,
	/// `boundingBox()` of each node.
	pub boxes: Vec<Box>,
}

/// The edge width of the default style sheet.
const EDGE_HALF_WIDTH: f64 = 1.5;
/// The border of compounds in the default style sheet, half on each side.
const COMPOUND_BORDER: f64 = 1.0;
/// The size of nodes without a size style.
const DEFAULT_NODE_SIZE: f64 = 30.0;
/// `bezierProjPcts` of the renderer.
const BEZIER_PROJECTIONS: [f64; 7] = [0.05, 0.225, 0.4, 0.5, 0.6, 0.775, 0.95];
/// Labels are padded by this `marginOfError`.
const LABEL_MARGIN: f64 = 2.0;
/// `control-point-step-size` of loops.
const LOOP_STEP: f64 = 40.0;

struct Cytoscape<'a> {
	nodes: &'a [Node],
	edges: &'a [Edge],
	config: &'a Config,
	children: Vec<Vec<usize>>,
	positions: Vec<(f64, f64)>,
	/// `autoWidth` and `autoHeight` of compounds.
	auto_sizes: Vec<(f64, f64)>,
	/// `segment-weights` and `segment-distances`.
	segments: Vec<(f64, f64)>,
}

/// Lays out the graph like `layoutArchitecture`; fails like fcose's `calcGrid` for constraints
/// that cannot be satisfied.
pub fn layout(
	nodes: &[Node],
	edges: &[Edge],
	constraints: &Constraints,
	config: &Config,
) -> Result<Layout, fcose::InvalidGrid> {
	let mut children = vec![Vec::new(); nodes.len()];
	for (index, node) in nodes.iter().enumerate() {
		if let Some(parent) = node.parent.filter(|&parent| parent < nodes.len()) {
			children[parent].push(index);
		}
	}
	let mut cytoscape = Cytoscape {
		nodes,
		edges,
		config,
		children,
		positions: vec![(0.0, 0.0); nodes.len()],
		auto_sizes: vec![(0.0, 0.0); nodes.len()],
		segments: vec![(0.0, 0.5); edges.len()],
	};
	cytoscape.update_compounds();
	cytoscape.run_fcose(constraints)?;
	// `layoutstop`: segment edges bend where the first run's end points meet.
	for (index, edge) in edges.iter().enumerate() {
		let (source, target) = (cytoscape.positions[edge.source], cytoscape.positions[edge.target]);
		if source.0 == target.0 || source.1 == target.1 {
			continue;
		}
		let points = cytoscape.edge_points(index);
		let (start, end) = (points.start, points.end);
		let point = match edge.source_dir.is_y() {
			true => (start.0, end.1),
			false => (end.0, start.1),
		};
		let (weight, distance) = segment_weights(start, end, point);
		if weight.is_finite() {
			cytoscape.segments[index].0 = weight;
		}
		if distance.is_finite() {
			cytoscape.segments[index].1 = distance;
		}
	}
	cytoscape.run_fcose(constraints)?;
	let edge_points = (0..edges.len()).map(|edge| cytoscape.edge_points(edge)).collect();
	let boxes = (0..nodes.len()).map(|node| cytoscape.node_box(node)).collect();
	Ok(Layout { positions: cytoscape.positions, edges: edge_points, boxes })
}

/// The constraints in node indices.
#[derive(Clone, Debug, Default)]
pub struct Constraints {
	pub horizontal: Vec<Vec<usize>>,
	pub vertical: Vec<Vec<usize>>,
	pub relative: Vec<fcose::RelativePlacement>,
}

/// `getSegmentWeights` of the renderer.
fn segment_weights(source: (f64, f64), target: (f64, f64), point: (f64, f64)) -> (f64, f64) {
	let (s_x, s_y) = source;
	let (t_x, t_y) = target;
	let (point_x, point_y) = point;
	let slope = (s_y - t_y) / (s_x - t_x);
	let mut distance = (point_y - s_y + ((s_x - point_x) * (s_y - t_y)) / (s_x - t_x))
		/ (1.0 + slope * slope).sqrt();
	let mut weight = ((point_y - s_y) * (point_y - s_y) + (point_x - s_x) * (point_x - s_x)
		- distance * distance)
		.sqrt();
	let length = ((t_x - s_x) * (t_x - s_x) + (t_y - s_y) * (t_y - s_y)).sqrt();
	weight /= length;
	let sign = |value: f64| match () {
		_ if value >= 0.0 => 1.0,
		_ if value < 0.0 => -1.0,
		_ => f64::NAN,
	};
	let side = sign((t_x - s_x) * (point_y - s_y) - (t_y - s_y) * (point_x - s_x));
	let ahead = sign((t_x - s_x) * (point_x - s_x) + (t_y - s_y) * (point_y - s_y));
	distance = distance.abs() * side;
	weight *= ahead;
	(weight, distance)
}

/// `Math.min` and `Math.max` of three values, which propagate `NaN`.
fn js_min3(a: f64, b: f64, c: f64) -> f64 {
	if a.is_nan() || b.is_nan() || c.is_nan() { f64::NAN } else { a.min(b).min(c) }
}

fn js_max3(a: f64, b: f64, c: f64) -> f64 {
	if a.is_nan() || b.is_nan() || c.is_nan() { f64::NAN } else { a.max(b).max(c) }
}

/// `math.shortenIntersection` by 0: `offset + 1 * (intersection - offset)`.
fn shorten(intersection: (f64, f64), offset: (f64, f64)) -> (f64, f64) {
	let displacement = (intersection.0 - offset.0, intersection.1 - offset.1);
	let length = (displacement.0 * displacement.0 + displacement.1 * displacement.1).sqrt();
	let mut ratio = (length - 0.0) / length;
	if ratio < 0.0 {
		ratio = 0.00001;
	}
	(offset.0 + ratio * displacement.0, offset.1 + ratio * displacement.1)
}

/// `qbezierAt`.
fn qbezier_at(p0: f64, p1: f64, p2: f64, t: f64) -> f64 {
	(1.0 - t) * (1.0 - t) * p0 + 2.0 * (1.0 - t) * t * p1 + t * t * p2
}

/// The manual endpoint of a direction as fractions of the node size.
fn endpoint_fractions(direction: Direction) -> (f64, f64) {
	match direction {
		Direction::Left => (0.0, 0.5),
		Direction::Right => (1.0, 0.5),
		Direction::Top => (0.5, 0.0),
		Direction::Bottom => (0.5, 1.0),
	}
}

impl Cytoscape<'_> {
	fn is_parent(&self, node: usize) -> bool {
		!self.children[node].is_empty()
	}

	fn padding(&self, node: usize) -> f64 {
		match self.nodes[node].kind {
			Kind::Group => self.config.padding,
			_ => 0.0,
		}
	}

	/// `width()` and `height()`: the size style, or the children's extent of a compound.
	fn size(&self, node: usize) -> (f64, f64) {
		if self.is_parent(node) {
			return self.auto_sizes[node];
		}
		match self.nodes[node].kind {
			Kind::Group => (DEFAULT_NODE_SIZE, DEFAULT_NODE_SIZE),
			_ => (self.config.icon_size, self.config.icon_size),
		}
	}

	/// `outerWidth()` and `outerHeight()`.
	fn outer_size(&self, node: usize) -> (f64, f64) {
		let (width, height) = self.size(node);
		let border = if self.is_parent(node) { COMPOUND_BORDER } else { 0.0 };
		let padding = 2.0 * self.padding(node);
		(width + border + padding, height + border + padding)
	}

	fn body(&self, node: usize) -> Box {
		let (x, y) = self.positions[node];
		let (width, height) = self.outer_size(node);
		Box { x1: x - width / 2.0, y1: y - height / 2.0, x2: x + width / 2.0, y2: y + height / 2.0 }
	}

	/// The label below a service, with the margin of error.
	fn label_box(&self, node: usize) -> Option<Box> {
		let (label_width, label_height) = self.nodes[node].label?;
		let (x, y) = self.positions[node];
		let label_y = y + self.size(node).1 / 2.0 + 0.0;
		Some(Box {
			x1: x - label_width / 2.0 - LABEL_MARGIN,
			y1: label_y - LABEL_MARGIN,
			x2: x + label_width / 2.0 + LABEL_MARGIN,
			y2: label_y + label_height + LABEL_MARGIN,
		})
	}

	/// `updateCompoundBounds` of all compounds, children first: a group comes after the group
	/// it is in.
	fn update_compounds(&mut self) {
		for node in (0..self.nodes.len()).rev() {
			if !self.is_parent(node) {
				continue;
			}
			// `children.boundingBox({includeLabels: true, includeOverlays: false})`.
			let mut bounds = Box::EMPTY;
			for &child in &self.children[node] {
				let mut body = Box::EMPTY;
				body.update_from(&self.body(child));
				body.expand(1.0);
				let mut child_box = Box::EMPTY;
				child_box.update_from(&body);
				if let Some(label) = self.label_box(child) {
					child_box.update_from(&label);
				}
				bounds.update_from(&child_box);
			}
			let bounds = bounds.finite();
			if bounds.width() == 0.0 || bounds.height() == 0.0 {
				self.auto_sizes[node] = (DEFAULT_NODE_SIZE, DEFAULT_NODE_SIZE);
				continue;
			}
			self.auto_sizes[node] = (bounds.width(), bounds.height());
			self.positions[node] = ((bounds.x1 + bounds.x2) / 2.0, (bounds.y1 + bounds.y2) / 2.0);
		}
	}

	/// `boundingBox()` of a node.
	fn node_box(&self, node: usize) -> Box {
		let mut bounds = Box::EMPTY;
		bounds.update_from(&self.body(node));
		if let Some(label) = self.label_box(node) {
			bounds.update_from(&label);
		}
		expanded(bounds.finite())
	}

	fn manual_endpoint(&self, node: usize, direction: Direction) -> (f64, f64) {
		let (x, y) = self.positions[node];
		let (width, height) = self.outer_size(node);
		// `0` is a length, the other values are percentages of the size.
		let scale = |fraction: f64, size: f64| if fraction == 0.0 { 0.0 } else { fraction * size };
		let (fraction_x, fraction_y) = endpoint_fractions(direction);
		(scale(fraction_x, width) + x, scale(fraction_y, height) + y)
	}

	/// The control points of loop `edge`: the loops of a node fan out.
	fn loop_points(&self, edge: usize) -> [(f64, f64); 2] {
		let source = self.edges[edge].source;
		let count = (0..edge)
			.filter(|&other| {
				self.edges[other].source == source && self.edges[other].target == source
			})
			.count() as f64;
		let (x, y) = self.positions[source];
		// `loop-direction` -45deg and `loop-sweep` -90deg.
		let direction = -45.0 * std::f64::consts::PI / 180.0;
		let sweep = -90.0 * std::f64::consts::PI / 180.0;
		let loop_angle = direction - std::f64::consts::PI / 2.0;
		let out_angle = loop_angle - sweep / 2.0;
		let in_angle = loop_angle + sweep / 2.0;
		let distance = 1.4 * LOOP_STEP * (count / 3.0 + 1.0);
		[
			(x + js_math::cos(out_angle) * distance, y + js_math::sin(out_angle) * distance),
			(x + js_math::cos(in_angle) * distance, y + js_math::sin(in_angle) * distance),
		]
	}

	/// `sourceEndpoint()`, `midpoint()` and `targetEndpoint()`.
	fn edge_points(&self, index: usize) -> EdgePoints {
		let edge = &self.edges[index];
		let source_point = self.manual_endpoint(edge.source, edge.source_dir);
		let target_point = self.manual_endpoint(edge.target, edge.target_dir);
		if edge.source == edge.target {
			let [first, second] = self.loop_points(index);
			let mid = ((first.0 + second.0) / 2.0, (first.1 + second.1) / 2.0);
			let start = shorten(source_point, first);
			let end = shorten(target_point, second);
			return EdgePoints { start, mid, end };
		}
		if edge.is_segments {
			let segment = self.segment_point(index, source_point, target_point);
			let start = shorten(source_point, segment);
			let end = shorten(target_point, segment);
			return EdgePoints { start, mid: segment, end };
		}
		let (source, target) = (self.positions[edge.source], self.positions[edge.target]);
		// The raw values of the endpoint styles, added to the other end's position, are the
		// points the edge comes from.
		let source_raw = endpoint_fractions(edge.source_dir);
		let target_raw = endpoint_fractions(edge.target_dir);
		let toward_source = (source.0 + source_raw.0, source.1 + source_raw.1);
		let toward_target = (target.0 + target_raw.0, target.1 + target_raw.1);
		let end = shorten(target_point, toward_source);
		let start = shorten(source_point, toward_target);
		let mid =
			((start.0 + end.0 + start.0 + end.0) / 4.0, (start.1 + end.1 + start.1 + end.1) / 4.0);
		EdgePoints { start, mid, end }
	}

	/// `findSegmentsPoints` with `edge-distances: endpoints`.
	fn segment_point(&self, index: usize, source: (f64, f64), target: (f64, f64)) -> (f64, f64) {
		let (weight, distance) = self.segments[index];
		let (dx, dy) = (target.0 - source.0, target.1 - source.1);
		let length = (dx * dx + dy * dy).sqrt();
		let normal = (-dy / length, dx / length);
		let adjusted = (
			source.0 * (1.0 - weight) + target.0 * weight,
			source.1 * (1.0 - weight) + target.1 * weight,
		);
		(adjusted.0 + normal.0 * distance, adjusted.1 + normal.1 * distance)
	}

	/// `boundingBox()` of an edge: its points with the edge width, and its label.
	fn edge_box(&self, index: usize) -> Box {
		let edge = &self.edges[index];
		let points = self.edge_points(index);
		let mut bounds = Box::EMPTY;
		let (start, mid, end) = (points.start, points.mid, points.end);
		bounds.update(
			js_min3(start.0, mid.0, end.0) - EDGE_HALF_WIDTH,
			js_min3(start.1, mid.1, end.1) - EDGE_HALF_WIDTH,
			js_max3(start.0, mid.0, end.0) + EDGE_HALF_WIDTH,
			js_max3(start.1, mid.1, end.1) + EDGE_HALF_WIDTH,
		);
		let mut include = |(x, y): (f64, f64)| {
			bounds.update(
				x - EDGE_HALF_WIDTH,
				y - EDGE_HALF_WIDTH,
				x + EDGE_HALF_WIDTH,
				y + EDGE_HALF_WIDTH,
			);
		};
		if edge.source == edge.target {
			let [first, second] = self.loop_points(index);
			let all = [start, first, mid, second, end];
			for slice in [&all[0..3], &all[2..5]] {
				for t in BEZIER_PROJECTIONS {
					include((
						qbezier_at(slice[0].0, slice[1].0, slice[2].0, t),
						qbezier_at(slice[0].1, slice[1].1, slice[2].1, t),
					));
				}
			}
		} else if edge.is_segments {
			for point in [start, mid, end] {
				include(point);
			}
		}
		if let Some((width, height)) = edge.label {
			bounds.update(
				mid.0 - width / 2.0 - LABEL_MARGIN,
				mid.1 - height / 2.0 - LABEL_MARGIN,
				mid.0 + width / 2.0 + LABEL_MARGIN,
				mid.1 + height / 2.0 + LABEL_MARGIN,
			);
		}
		expanded(bounds.finite())
	}

	/// The center of `eles.boundingBox()` of all elements.
	fn center(&self) -> (f64, f64) {
		let mut bounds = Box::EMPTY;
		for edge in 0..self.edges.len() {
			bounds.update_from(&self.edge_box(edge));
		}
		for node in 0..self.nodes.len() {
			bounds.update_from(&self.node_box(node));
		}
		let bounds = bounds.finite();
		(bounds.x1 + bounds.width() / 2.0, bounds.y1 + bounds.height() / 2.0)
	}

	/// One `layout.run()` with `Math.random` seeded.
	fn run_fcose(&mut self, constraints: &Constraints) -> Result<(), fcose::InvalidGrid> {
		let center = self.center();
		let nodes: Vec<fcose::Node> = (0..self.nodes.len())
			.map(|node| {
				let (width, height) = self.outer_size(node);
				// `layoutDimensions` avoids zero sizes.
				let (width, height) =
					if width == 0.0 || height == 0.0 { (1.0, 1.0) } else { (width, height) };
				let (x, y) = self.positions[node];
				fcose::Node {
					parent: self.nodes[node].parent,
					x,
					y,
					width,
					height,
					padding: self.padding(node).trunc(),
				}
			})
			.collect();
		let edges: Vec<fcose::Edge> = self
			.edges
			.iter()
			.map(|edge| {
				let is_same_group =
					self.nodes[edge.source].parent == self.nodes[edge.target].parent;
				fcose::Edge {
					source: edge.source,
					target: edge.target,
					ideal_length: match is_same_group {
						true => self.config.same_group_length,
						false => self.config.cross_group_length,
					},
					elasticity: match is_same_group {
						true => self.config.same_group_elasticity,
						false => 0.001,
					},
				}
			})
			.collect();
		let non_empty = |lists: &[Vec<usize>]| (!lists.is_empty()).then_some(lists.to_vec());
		let vertical = non_empty(&constraints.vertical);
		let horizontal = non_empty(&constraints.horizontal);
		let input = fcose::Input {
			nodes: &nodes,
			edges: &edges,
			vertical: vertical.as_deref(),
			horizontal: horizontal.as_deref(),
			relative: (!constraints.relative.is_empty()).then_some(&constraints.relative[..]),
			iterations: self.config.iterations,
			center,
		};
		let mut random = mulberry32(self.config.seed);
		let positions = fcose::layout(&input, &mut random)?;
		for (node, position) in positions.into_iter().enumerate() {
			if let Some(position) = position {
				self.positions[node] = position;
			}
		}
		self.update_compounds();
		Ok(())
	}
}

/// The final `expandBoundingBox(bounds, 1)` of boxes with an area.
fn expanded(mut bounds: Box) -> Box {
	if bounds.width() > 0.0 && bounds.height() > 0.0 {
		bounds.expand(1.0);
	}
	bounds
}

/// `withSeededRandom`'s mulberry32 (`architectureSeed.ts`).
pub fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
	let mut state = seed;
	move || {
		state = state.wrapping_add(0x6d2b_79f5);
		let mut t = state;
		t = (t ^ (t >> 15)).wrapping_mul(t | 1);
		t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
		f64::from(t ^ (t >> 14)) / 4_294_967_296.0
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn generates_mulberry32() {
		let mut random = mulberry32(1);
		// `withSeededRandom(1, () => [Math.random(), Math.random()])`.
		assert_eq!(random(), 0.6270739405881613);
		assert_eq!(random(), 0.002735721180215478);
	}
}
