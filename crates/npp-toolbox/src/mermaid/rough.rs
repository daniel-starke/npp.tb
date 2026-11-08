//! The sketches of the `handDrawn` look: a port of roughjs 4.6.6 (`math.js`, `renderer.js`,
//! `generator.js`, `svg.js`, `fillers/{hachure,hatch}-filler.js`, `fillers/scan-line-hachure.js`)
//! with the same order of random numbers, so that a seed gives roughjs' picture.
//!
//! roughjs, hachure-fill, path-data-parser, points-on-curve and points-on-path are MIT licensed,
//! copyright (c) Preet Shihn.
//!
//! `curve`, `arc` and `linearPath` are left out because Mermaid does not call them, and with them
//! the options only they read.

mod flatten;
mod hachure;
mod path_data;
#[cfg(test)]
mod tests;

use path_data::Segment;

/// The numbers of `Math.imul(48271, seed)`. A seed of 0 is `Math.random()` in roughjs, which a
/// reproducible picture cannot use: callers choose another seed.
struct Random {
	seed: i32,
}

impl Random {
	fn new(seed: i32) -> Random {
		Random { seed: if seed == 0 { 1 } else { seed } }
	}

	fn next(&mut self) -> f64 {
		self.seed = self.seed.wrapping_mul(48271);
		f64::from(self.seed & i32::MAX) / 2_147_483_648.0
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillStyle {
	Hachure,
	Solid,
	CrossHatch,
}

/// The options of a roughjs call. Its random generator restarts at `seed` with every call,
/// because `RoughGenerator._o` copies the options and the generator lives on the copy.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
	pub seed: i32,
	pub roughness: f64,
	/// `None` is a falsy `fill`.
	pub fill: Option<String>,
	pub stroke: String,
	/// Written as it is: Mermaid passes on the text of the user's style.
	pub stroke_width: String,
	pub fill_style: FillStyle,
	pub fill_weight: f64,
	pub hachure_angle: f64,
	pub hachure_gap: f64,
	pub stroke_line_dash: Option<String>,
	pub fill_line_dash: Option<String>,
}

impl Default for Options {
	fn default() -> Options {
		Options {
			seed: 1,
			roughness: 1.0,
			fill: None,
			stroke: "#000".to_owned(),
			stroke_width: "1".to_owned(),
			fill_style: FillStyle::Hachure,
			fill_weight: -1.0,
			hachure_angle: -41.0,
			hachure_gap: -1.0,
			stroke_line_dash: None,
			fill_line_dash: None,
		}
	}
}

impl Options {
	/// `Number(strokeWidth)`.
	fn stroke_width_number(&self) -> f64 {
		self.stroke_width.trim().parse().unwrap_or(f64::NAN)
	}
}

const MAX_RANDOMNESS_OFFSET: f64 = 2.0;
const BOWING: f64 = 1.0;
const CURVE_FITTING: f64 = 0.95;
const CURVE_STEP_COUNT: f64 = 9.0;
const FILL_SHAPE_ROUGHNESS_GAIN: f64 = 0.8;

/// The largest size of an ellipse: its number of steps comes from the size.
const MAX_ELLIPSE_SIZE: f64 = 1e9;

/// A coordinate pair of the generators.
pub type Xy = [f64; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
	Move(Xy),
	Curve([f64; 6]),
	Line(Xy),
}

/// What the path of an operation set is: `path`, `fillPath` or `fillSketch` of roughjs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
	Outline,
	Fill,
	FillSketch,
}

/// `getBBox()` of a path or a group as Blink computes it.
pub use super::rendering::shapes::blink_path::RectF as Bounds;
use super::rendering::shapes::blink_path::{self, PathCommand};

/// A written path of a sketch: 3 decimals like all SVG numbers of the renderer. The bounds come
/// from the full precision of the operations as roughjs writes them, so the layout stays the
/// one of Mermaid.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchPath {
	pub d: String,
	pub kind: PathKind,
	/// `None` for an empty path, which `getBBox` of its group skips.
	pub bounds: Option<Bounds>,
}

/// The `<g>` of `RoughSVG.draw`: the fill first, the outline last.
#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
	pub options: Options,
	pub paths: Vec<SketchPath>,
	/// The `fill-rule="evenodd"` of a filled `polygon`.
	pub is_even_odd: bool,
}

impl Sketch {
	/// The box of the group.
	pub fn bounds(&self) -> Option<Bounds> {
		blink_path::union_all(self.paths.iter().filter_map(|path| path.bounds))
	}

	/// `fillSketch`: the width of a hachure line.
	pub fn fill_weight(&self) -> f64 {
		match self.options.fill_weight < 0.0 {
			true => self.options.stroke_width_number() / 2.0,
			false => self.options.fill_weight,
		}
	}
}

struct OpSet {
	kind: PathKind,
	ops: Vec<Op>,
}

fn finish(sets: Vec<OpSet>, options: &Options, is_even_odd: bool) -> Sketch {
	Sketch {
		options: options.clone(),
		paths: sets
			.into_iter()
			.map(|set| SketchPath {
				d: path_text(&set.ops),
				kind: set.kind,
				bounds: ops_bounds(&set.ops),
			})
			.collect(),
		is_even_odd,
	}
}

/// `svg::number`, without an allocation per number: sketches hold a lot of them.
fn push_number(out: &mut String, value: f64) {
	use std::fmt::Write as _;
	let thousandths = (value * 1000.0).round();
	if !thousandths.is_finite() || thousandths == 0.0 {
		out.push('0');
		return;
	}
	if thousandths.abs() >= 1e15 {
		out.push_str(&super::svg::number(value));
		return;
	}
	if thousandths < 0.0 {
		out.push('-');
	}
	let digits = thousandths.abs() as u64;
	let _ = write!(out, "{}", digits / 1000);
	let mut decimals = digits % 1000;
	if decimals != 0 {
		out.push('.');
		let mut divisor = 100;
		while decimals != 0 {
			out.push(char::from(b'0' + (decimals / divisor) as u8));
			decimals %= divisor;
			divisor /= 10;
		}
	}
}

/// `opsToPath`.
fn path_text(ops: &[Op]) -> String {
	let mut out = String::with_capacity(ops.len() * 28);
	let mut push_all = |command: char, numbers: &[f64]| {
		out.push(command);
		for (index, number) in numbers.iter().enumerate() {
			if index != 0 {
				out.push(' ');
			}
			push_number(&mut out, *number);
		}
	};
	for op in ops {
		match op {
			Op::Move(point) => push_all('M', point),
			Op::Curve(data) => push_all('C', data),
			Op::Line(point) => push_all('L', point),
		}
	}
	out
}

fn ops_bounds(ops: &[Op]) -> Option<Bounds> {
	blink_path::path_bounds(ops.iter().map(|op| match *op {
		Op::Move(point) => PathCommand::Move(point),
		Op::Line(point) => PathCommand::Line(point),
		Op::Curve(numbers) => PathCommand::Cubic(numbers),
	}))
}

/// The options that change between the outline and the solid fill of a path, which share a
/// random generator through `Object.assign({}, o, …)`.
#[derive(Clone, Copy)]
struct Stroke {
	roughness: f64,
	is_double: bool,
}

struct Renderer {
	random: Random,
}

impl Renderer {
	fn new(options: &Options) -> Renderer {
		Renderer { random: Random::new(options.seed) }
	}

	/// `_offset`.
	fn offset(&mut self, min: f64, max: f64, o: Stroke, roughness_gain: f64) -> f64 {
		o.roughness * roughness_gain * (self.random.next() * (max - min) + min)
	}

	/// `_offsetOpt`.
	fn offset_opt(&mut self, x: f64, o: Stroke, roughness_gain: f64) -> f64 {
		self.offset(-x, x, o, roughness_gain)
	}

	/// `_doubleLine`.
	fn double_line(&mut self, from: Xy, to: Xy, o: Stroke, ops: &mut Vec<Op>) {
		self.line(from, to, o, false, ops);
		if o.is_double {
			self.line(from, to, o, true, ops);
		}
	}

	/// `_line` with `move` set.
	fn line(&mut self, [x1, y1]: Xy, [x2, y2]: Xy, o: Stroke, is_overlay: bool, ops: &mut Vec<Op>) {
		let length_squared = (x1 - x2) * (x1 - x2) + (y1 - y2) * (y1 - y2);
		let length = length_squared.sqrt();
		let gain = if length < 200.0 {
			1.0
		} else if length > 500.0 {
			0.4
		} else {
			-0.0016668 * length + 1.233334
		};
		let mut offset = MAX_RANDOMNESS_OFFSET;
		if offset * offset * 100.0 > length_squared {
			offset = length / 10.0;
		}
		let diverge_point = 0.2 + self.random.next() * 0.2;
		let mid_x = BOWING * MAX_RANDOMNESS_OFFSET * (y2 - y1) / 200.0;
		let mid_y = BOWING * MAX_RANDOMNESS_OFFSET * (x1 - x2) / 200.0;
		let mid_x = self.offset_opt(mid_x, o, gain);
		let mid_y = self.offset_opt(mid_y, o, gain);
		let spread = if is_overlay { offset / 2.0 } else { offset };
		let move_x = x1 + self.offset_opt(spread, o, gain);
		let move_y = y1 + self.offset_opt(spread, o, gain);
		ops.push(Op::Move([move_x, move_y]));
		let c1 = mid_x + x1 + (x2 - x1) * diverge_point + self.offset_opt(spread, o, gain);
		let c2 = mid_y + y1 + (y2 - y1) * diverge_point + self.offset_opt(spread, o, gain);
		let c3 = mid_x + x1 + 2.0 * (x2 - x1) * diverge_point + self.offset_opt(spread, o, gain);
		let c4 = mid_y + y1 + 2.0 * (y2 - y1) * diverge_point + self.offset_opt(spread, o, gain);
		let c5 = x2 + self.offset_opt(spread, o, gain);
		let c6 = y2 + self.offset_opt(spread, o, gain);
		ops.push(Op::Curve([c1, c2, c3, c4, c5, c6]));
	}

	/// `linearPath`, closed.
	fn closed_path(&mut self, points: &[Xy], o: Stroke) -> Vec<Op> {
		let mut ops = Vec::with_capacity(points.len() * 4);
		match points {
			[] | [_] => {}
			[from, to] => self.double_line(*from, *to, o, &mut ops),
			_ => {
				for pair in points.windows(2) {
					self.double_line(pair[0], pair[1], o, &mut ops);
				}
				self.double_line(points[points.len() - 1], points[0], o, &mut ops);
			}
		}
		ops
	}

	/// `svgPath`.
	fn svg_path(&mut self, segments: &[Segment], o: Stroke) -> Vec<Op> {
		let mut ops = Vec::with_capacity(segments.len() * 4);
		let mut first = [0.0, 0.0];
		let mut current = [0.0, 0.0];
		for segment in segments {
			match *segment {
				Segment::Move(point) => {
					current = point;
					first = point;
				}
				Segment::Line(point) => {
					self.double_line(current, point, o, &mut ops);
					current = point;
				}
				Segment::Curve(data) => {
					self.bezier_to(data, current, o, &mut ops);
					current = [data[4], data[5]];
				}
				Segment::Close => {
					self.double_line(current, first, o, &mut ops);
					current = first;
				}
			}
		}
		ops
	}

	/// `_bezierTo`.
	fn bezier_to(&mut self, data: [f64; 6], current: Xy, o: Stroke, ops: &mut Vec<Op>) {
		let [x1, y1, x2, y2, x, y] = data;
		let offsets = [MAX_RANDOMNESS_OFFSET, MAX_RANDOMNESS_OFFSET + 0.3];
		let iterations = if o.is_double { 2 } else { 1 };
		for (index, offset) in offsets.into_iter().enumerate().take(iterations) {
			if index == 0 {
				ops.push(Op::Move(current));
			} else {
				let move_x = current[0] + self.offset_opt(offsets[0], o, 1.0);
				let move_y = current[1] + self.offset_opt(offsets[0], o, 1.0);
				ops.push(Op::Move([move_x, move_y]));
			}
			let end_x = x + self.offset_opt(offset, o, 1.0);
			let end_y = y + self.offset_opt(offset, o, 1.0);
			let c1 = x1 + self.offset_opt(offset, o, 1.0);
			let c2 = y1 + self.offset_opt(offset, o, 1.0);
			let c3 = x2 + self.offset_opt(offset, o, 1.0);
			let c4 = y2 + self.offset_opt(offset, o, 1.0);
			ops.push(Op::Curve([c1, c2, c3, c4, end_x, end_y]));
		}
	}

	/// `solidFillPolygon`.
	fn solid_fill_polygon(&mut self, polygons: &[Vec<Xy>], o: Stroke) -> Vec<Op> {
		let mut ops = Vec::new();
		for points in polygons {
			if points.len() > 2 {
				for (index, point) in points.iter().enumerate() {
					let x = point[0] + self.offset_opt(MAX_RANDOMNESS_OFFSET, o, 1.0);
					let y = point[1] + self.offset_opt(MAX_RANDOMNESS_OFFSET, o, 1.0);
					ops.push(if index == 0 { Op::Move([x, y]) } else { Op::Line([x, y]) });
				}
			}
		}
		ops
	}

	/// `patternFillPolygons` of the `hachure` and `cross-hatch` fillers.
	fn pattern_fill(&mut self, polygons: &mut [Vec<Xy>], options: &Options, o: Stroke) -> Vec<Op> {
		let mut ops = Vec::new();
		self.hachure_fill(polygons, options.hachure_angle, options, o, &mut ops);
		if options.fill_style == FillStyle::CrossHatch {
			self.hachure_fill(polygons, options.hachure_angle + 90.0, options, o, &mut ops);
		}
		ops
	}

	/// `polygonHachureLines` and `renderLines`.
	fn hachure_fill(
		&mut self,
		polygons: &mut [Vec<Xy>],
		hachure_angle: f64,
		options: &Options,
		o: Stroke,
		ops: &mut Vec<Op>,
	) {
		let mut gap = options.hachure_gap;
		if gap < 0.0 {
			gap = options.stroke_width_number() * 4.0;
		}
		let gap = super::color::js_round(gap.max(0.1));
		let skip_offset = match o.roughness >= 1.0 && self.random.next() > 0.7 && gap != 0.0 {
			true => gap,
			false => 1.0,
		};
		let lines = hachure::hachure_lines(polygons, gap, hachure_angle + 90.0, skip_offset);
		ops.reserve(lines.len() * 4);
		for [from, to] in lines {
			self.double_line(from, to, Stroke { is_double: true, ..o }, ops);
		}
	}

	/// `generateEllipseParams`.
	fn ellipse_params(&mut self, width: f64, height: f64, o: Stroke) -> EllipseParams {
		let half_width = width / 2.0;
		let half_height = height / 2.0;
		let psq = (std::f64::consts::TAU
			* ((half_width * half_width + half_height * half_height) / 2.0).sqrt())
		.sqrt();
		let step_count = (CURVE_STEP_COUNT.max(CURVE_STEP_COUNT / 200f64.sqrt() * psq)).ceil();
		let increment = std::f64::consts::TAU / step_count;
		let mut rx = half_width.abs();
		let mut ry = half_height.abs();
		let randomness = 1.0 - CURVE_FITTING;
		rx += self.offset_opt(rx * randomness, o, 1.0);
		ry += self.offset_opt(ry * randomness, o, 1.0);
		EllipseParams { increment, rx, ry }
	}

	/// `ellipseWithParams`: the operations and the estimated points of the fill.
	fn ellipse(&mut self, center: Xy, o: Stroke, params: &EllipseParams) -> (Vec<Op>, Vec<Xy>) {
		let inner = self.offset(0.4, 1.0, o, 1.0);
		let overlap = params.increment * self.offset(0.1, inner, o, 1.0);
		let (all_points, core_points) = self.ellipse_points(center, params, 1.0, overlap, o);
		let mut ops = curve(&all_points);
		if o.roughness != 0.0 {
			let (all_points, _) = self.ellipse_points(center, params, 1.5, 0.0, o);
			ops.extend(curve(&all_points));
		}
		(ops, core_points)
	}

	/// `_computeEllipsePoints`.
	fn ellipse_points(
		&mut self,
		[cx, cy]: Xy,
		params: &EllipseParams,
		offset: f64,
		overlap: f64,
		o: Stroke,
	) -> (Vec<Xy>, Vec<Xy>) {
		let &EllipseParams { increment, rx, ry } = params;
		let mut core_points = Vec::new();
		let mut all_points = Vec::new();
		if o.roughness == 0.0 {
			let increment = increment / 4.0;
			all_points.push([cx + rx * (-increment).cos(), cy + ry * (-increment).sin()]);
			let mut angle = 0.0f64;
			while angle <= std::f64::consts::TAU {
				let point = [cx + rx * angle.cos(), cy + ry * angle.sin()];
				core_points.push(point);
				all_points.push(point);
				angle += increment;
			}
			all_points.push([cx + rx, cy]);
			all_points.push([cx + rx * increment.cos(), cy + ry * increment.sin()]);
			return (all_points, core_points);
		}
		let rad_offset = self.offset_opt(0.5, o, 1.0) - std::f64::consts::FRAC_PI_2;
		let mut push = |renderer: &mut Renderer, scale: f64, angle: f64| -> Xy {
			let x = renderer.offset_opt(offset, o, 1.0) + cx + scale * rx * angle.cos();
			let y = renderer.offset_opt(offset, o, 1.0) + cy + scale * ry * angle.sin();
			all_points.push([x, y]);
			[x, y]
		};
		push(self, 0.9, rad_offset - increment);
		let end_angle = std::f64::consts::TAU + rad_offset - 0.01;
		let mut angle = rad_offset;
		while angle < end_angle {
			core_points.push(push(self, 1.0, angle));
			angle += increment;
		}
		push(self, 1.0, rad_offset + std::f64::consts::TAU + overlap * 0.5);
		push(self, 0.98, rad_offset + overlap);
		push(self, 0.9, rad_offset + overlap * 0.5);
		(all_points, core_points)
	}
}

struct EllipseParams {
	increment: f64,
	rx: f64,
	ry: f64,
}

/// `_curve` with more than 3 points and a tightness of 0.
fn curve(points: &[Xy]) -> Vec<Op> {
	let mut ops = Vec::with_capacity(points.len());
	if points.len() > 3 {
		ops.push(Op::Move(points[1]));
		for window in points.windows(4) {
			let [before, from, to, after] = [window[0], window[1], window[2], window[3]];
			ops.push(Op::Curve([
				from[0] + (to[0] - before[0]) / 6.0,
				from[1] + (to[1] - before[1]) / 6.0,
				to[0] + (from[0] - after[0]) / 6.0,
				to[1] + (from[1] - after[1]) / 6.0,
				to[0],
				to[1],
			]));
		}
	}
	ops
}

fn stroke_of(options: &Options) -> Stroke {
	Stroke { roughness: options.roughness, is_double: true }
}

fn is_all_finite(numbers: &[f64]) -> bool {
	numbers.iter().all(|number| number.is_finite())
}

fn has_stroke(options: &Options) -> bool {
	options.stroke != "none"
}

/// `rc.line`.
pub fn line(from: Xy, to: Xy, options: &Options) -> Sketch {
	finish(line_sets(from, to, options), options, false)
}

fn line_sets(from: Xy, to: Xy, options: &Options) -> Vec<OpSet> {
	if !is_all_finite(&[from[0], from[1], to[0], to[1]]) {
		return Vec::new();
	}
	let mut ops = Vec::with_capacity(4);
	Renderer::new(options).double_line(from, to, stroke_of(options), &mut ops);
	vec![OpSet { kind: PathKind::Outline, ops }]
}

/// `rc.rectangle`.
pub fn rectangle(x: f64, y: f64, width: f64, height: f64, options: &Options) -> Sketch {
	finish(rectangle_sets(x, y, width, height, options), options, false)
}

fn rectangle_sets(x: f64, y: f64, width: f64, height: f64, options: &Options) -> Vec<OpSet> {
	let points = vec![[x, y], [x + width, y], [x + width, y + height], [x, y + height]];
	polygon_sets(points, options)
}

/// `rc.polygon`.
pub fn polygon(points: Vec<Xy>, options: &Options) -> Sketch {
	finish(polygon_sets(points, options), options, true)
}

fn polygon_sets(points: Vec<Xy>, options: &Options) -> Vec<OpSet> {
	if !points.iter().all(|point| is_all_finite(point)) {
		return Vec::new();
	}
	let o = stroke_of(options);
	let mut renderer = Renderer::new(options);
	let outline = renderer.closed_path(&points, o);
	let mut sets = Vec::with_capacity(2);
	if options.fill.is_some() {
		let mut polygons = [points];
		sets.push(match options.fill_style {
			FillStyle::Solid => {
				OpSet { kind: PathKind::Fill, ops: renderer.solid_fill_polygon(&polygons, o) }
			}
			_ => OpSet {
				kind: PathKind::FillSketch,
				ops: renderer.pattern_fill(&mut polygons, options, o),
			},
		});
	}
	if has_stroke(options) {
		sets.push(OpSet { kind: PathKind::Outline, ops: outline });
	}
	sets
}

/// `rc.ellipse` around the center.
pub fn ellipse(x: f64, y: f64, width: f64, height: f64, options: &Options) -> Sketch {
	finish(ellipse_sets(x, y, width, height, options), options, false)
}

/// `rc.circle` around the center.
pub fn circle(x: f64, y: f64, diameter: f64, options: &Options) -> Sketch {
	ellipse(x, y, diameter, diameter, options)
}

fn ellipse_sets(x: f64, y: f64, width: f64, height: f64, options: &Options) -> Vec<OpSet> {
	if !is_all_finite(&[x, y, width, height])
		|| width.abs() > MAX_ELLIPSE_SIZE
		|| height.abs() > MAX_ELLIPSE_SIZE
	{
		return Vec::new();
	}
	let o = stroke_of(options);
	let mut renderer = Renderer::new(options);
	let params = renderer.ellipse_params(width, height, o);
	let (outline, estimated_points) = renderer.ellipse([x, y], o, &params);
	let mut sets = Vec::with_capacity(2);
	if options.fill.is_some() {
		sets.push(match options.fill_style {
			FillStyle::Solid => {
				OpSet { kind: PathKind::Fill, ops: renderer.ellipse([x, y], o, &params).0 }
			}
			_ => OpSet {
				kind: PathKind::FillSketch,
				ops: renderer.pattern_fill(&mut [estimated_points], options, o),
			},
		});
	}
	if has_stroke(options) {
		sets.push(OpSet { kind: PathKind::Outline, ops: outline });
	}
	sets
}

/// `rc.path`.
pub fn path(d: &str, options: &Options) -> Sketch {
	finish(path_sets(d, options), options, false)
}

fn path_sets(d: &str, options: &Options) -> Vec<OpSet> {
	if d.is_empty() {
		return Vec::new();
	}
	let segments = path_data::normalized_segments(d);
	let distance = (1.0 + options.roughness) / 2.0;
	let mut polygons = flatten::points_on_path(&segments, 1.0, distance);
	let o = stroke_of(options);
	let mut renderer = Renderer::new(options);
	let outline = renderer.svg_path(&segments, o);
	let fill = options.fill.as_deref();
	let mut sets = Vec::with_capacity(2);
	if fill.is_some_and(|fill| fill != "transparent" && fill != "none") {
		sets.push(match options.fill_style {
			FillStyle::Solid if polygons.len() == 1 => {
				let fill_stroke = Stroke {
					is_double: false,
					roughness: match options.roughness != 0.0 {
						true => options.roughness + FILL_SHAPE_ROUGHNESS_GAIN,
						false => 0.0,
					},
				};
				let mut ops = renderer.svg_path(&segments, fill_stroke);
				// `_mergedShape`.
				let mut index = 0;
				ops.retain(|op| {
					index += 1;
					index == 1 || !matches!(op, Op::Move(_))
				});
				OpSet { kind: PathKind::Fill, ops }
			}
			FillStyle::Solid => {
				OpSet { kind: PathKind::Fill, ops: renderer.solid_fill_polygon(&polygons, o) }
			}
			_ => OpSet {
				kind: PathKind::FillSketch,
				ops: renderer.pattern_fill(&mut polygons, options, o),
			},
		});
	}
	if has_stroke(options) {
		sets.push(OpSet { kind: PathKind::Outline, ops: outline });
	}
	sets
}
