//! The d3 curves of edges (`d3-shape`) as path segments, with the length and bounding box a
//! browser computes for the path.

use std::fmt::Write as _;

use super::Point;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
	Move(Point),
	Line(Point),
	Quad(Point, Point),
	Cubic(Point, Point, Point),
	Close,
}

/// Path data built like `d3-path` with three digits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathData {
	pub segments: Vec<Segment>,
}

/// `Math.round(value * 1000) / 1000` as d3-path writes it.
fn js_round3(value: f64) -> String {
	let rounded = (value * 1000.0 + 0.5).floor() / 1000.0;
	super::super::svg::number(rounded)
}

impl PathData {
	pub fn move_to(&mut self, x: f64, y: f64) {
		self.segments.push(Segment::Move(Point { x, y }));
	}

	pub fn line_to(&mut self, x: f64, y: f64) {
		self.segments.push(Segment::Line(Point { x, y }));
	}

	pub fn quad_to(&mut self, x1: f64, y1: f64, x: f64, y: f64) {
		self.segments.push(Segment::Quad(Point { x: x1, y: y1 }, Point { x, y }));
	}

	pub fn cubic_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64) {
		self.segments.push(Segment::Cubic(
			Point { x: x1, y: y1 },
			Point { x: x2, y: y2 },
			Point { x, y },
		));
	}

	fn close(&mut self) {
		self.segments.push(Segment::Close);
	}

	pub fn to_svg(&self) -> String {
		self.write(js_round3)
	}

	/// The path with its numbers in full, as Mermaid's own path builders write them.
	pub fn to_full_svg(&self) -> String {
		self.write(super::super::color::js_number)
	}

	fn write(&self, format: fn(f64) -> String) -> String {
		let mut d = String::new();
		let point = |d: &mut String, p: Point| {
			let _ = write!(d, "{},{}", format(p.x), format(p.y));
		};
		for segment in &self.segments {
			match *segment {
				Segment::Move(p) => {
					d.push('M');
					point(&mut d, p);
				}
				Segment::Line(p) => {
					d.push('L');
					point(&mut d, p);
				}
				Segment::Quad(c, p) => {
					d.push('Q');
					point(&mut d, c);
					d.push(',');
					point(&mut d, p);
				}
				Segment::Cubic(c1, c2, p) => {
					d.push('C');
					point(&mut d, c1);
					d.push(',');
					point(&mut d, c2);
					d.push(',');
					point(&mut d, p);
				}
				Segment::Close => d.push('Z'),
			}
		}
		d
	}

	/// Visits the segments as cubic curves from their start points.
	fn for_each_curve(&self, mut visit: impl FnMut([Point; 4])) {
		let mut start = Point::default();
		let mut current = Point::default();
		for segment in &self.segments {
			match *segment {
				Segment::Move(p) => {
					start = p;
					current = p;
				}
				Segment::Line(p) => {
					visit([current, current, p, p]);
					current = p;
				}
				Segment::Quad(c, p) => {
					let c1 = Point {
						x: current.x + 2.0 / 3.0 * (c.x - current.x),
						y: current.y + 2.0 / 3.0 * (c.y - current.y),
					};
					let c2 = Point {
						x: p.x + 2.0 / 3.0 * (c.x - p.x),
						y: p.y + 2.0 / 3.0 * (c.y - p.y),
					};
					visit([current, c1, c2, p]);
					current = p;
				}
				Segment::Cubic(c1, c2, p) => {
					visit([current, c1, c2, p]);
					current = p;
				}
				Segment::Close => {
					visit([current, current, start, start]);
					current = start;
				}
			}
		}
	}

	/// `getTotalLength()`.
	pub fn length(&self) -> f64 {
		let mut length = 0.0;
		self.for_each_curve(|curve| length += cubic_length(curve, 0));
		length
	}

	/// `getBBox()`: the extremes of the curves, not of their control points. `None` without
	/// segments.
	pub fn bounds(&self) -> Option<(Point, Point)> {
		let mut min = Point { x: f64::INFINITY, y: f64::INFINITY };
		let mut max = Point { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY };
		let mut include = |p: Point| {
			min = Point { x: min.x.min(p.x), y: min.y.min(p.y) };
			max = Point { x: max.x.max(p.x), y: max.y.max(p.y) };
		};
		for segment in &self.segments {
			if let Segment::Move(p) = segment {
				include(*p);
			}
		}
		self.for_each_curve(|[p0, p1, p2, p3]| {
			include(p0);
			include(p3);
			for t in cubic_extrema(p0.x, p1.x, p2.x, p3.x)
				.into_iter()
				.chain(cubic_extrema(p0.y, p1.y, p2.y, p3.y))
			{
				include(cubic_point([p0, p1, p2, p3], t));
			}
		});
		min.x.is_finite().then_some((min, max))
	}
}

fn cubic_point([p0, p1, p2, p3]: [Point; 4], t: f64) -> Point {
	let u = 1.0 - t;
	let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
	Point {
		x: a * p0.x + b * p1.x + c * p2.x + d * p3.x,
		y: a * p0.y + b * p1.y + c * p2.y + d * p3.y,
	}
}

/// Parameters in (0, 1) where the derivative of a cubic coordinate vanishes.
fn cubic_extrema(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
	let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
	let b = 2.0 * (p0 - 2.0 * p1 + p2);
	let c = p1 - p0;
	let mut roots = Vec::new();
	if a.abs() < 1e-12 {
		if b.abs() > 1e-12 {
			roots.push(-c / b);
		}
	} else {
		let discriminant = b * b - 4.0 * a * c;
		if discriminant >= 0.0 {
			let root = discriminant.sqrt();
			roots.push((-b + root) / (2.0 * a));
			roots.push((-b - root) / (2.0 * a));
		}
	}
	roots.retain(|&t| t > 0.0 && t < 1.0);
	roots
}

/// Arc length by subdivision until the control polygon and the chord agree.
fn cubic_length(curve: [Point; 4], depth: u32) -> f64 {
	let [p0, p1, p2, p3] = curve;
	let distance = |a: Point, b: Point| (b.x - a.x).hypot(b.y - a.y);
	let chord = distance(p0, p3);
	let polygon = distance(p0, p1) + distance(p1, p2) + distance(p2, p3);
	if polygon - chord < 1e-3 || depth >= 16 {
		return (chord + polygon) / 2.0;
	}
	let mid = |a: Point, b: Point| Point { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
	let (p01, p12, p23) = (mid(p0, p1), mid(p1, p2), mid(p2, p3));
	let (p012, p123) = (mid(p01, p12), mid(p12, p23));
	let center = mid(p012, p123);
	cubic_length([p0, p01, p012, center], depth + 1)
		+ cubic_length([center, p123, p23, p3], depth + 1)
}

/// The curve types of Mermaid's `curve` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
	Linear,
	Basis,
	Cardinal,
	BumpX,
	BumpY,
	CatmullRom,
	MonotoneX,
	MonotoneY,
	Natural,
	Step,
	StepAfter,
	StepBefore,
	/// Straight lines with rounded corners, drawn by Mermaid itself.
	Rounded,
}

impl Curve {
	/// `insertEdge`'s mapping; unknown names are `basis`.
	pub fn parse(name: &str) -> Curve {
		match name {
			"linear" => Curve::Linear,
			"cardinal" => Curve::Cardinal,
			"bumpX" => Curve::BumpX,
			"bumpY" => Curve::BumpY,
			"catmullRom" => Curve::CatmullRom,
			"monotoneX" => Curve::MonotoneX,
			"monotoneY" => Curve::MonotoneY,
			"natural" => Curve::Natural,
			"step" => Curve::Step,
			"stepAfter" => Curve::StepAfter,
			"stepBefore" => Curve::StepBefore,
			"rounded" => Curve::Rounded,
			_ => Curve::Basis,
		}
	}
}

/// d3's `line().curve(curve)` through `points`.
pub fn line(points: &[Point], curve: Curve) -> PathData {
	let mut path = PathData::default();
	match curve {
		Curve::Linear | Curve::Rounded => linear(&mut path, points),
		Curve::Basis => basis(&mut path, points),
		Curve::Cardinal => cardinal(&mut path, points),
		Curve::CatmullRom => catmull_rom(&mut path, points),
		Curve::BumpX => bump(&mut path, points, true),
		Curve::BumpY => bump(&mut path, points, false),
		Curve::MonotoneX => monotone(&mut path, points, false),
		Curve::MonotoneY => monotone(&mut path, points, true),
		Curve::Natural => natural(&mut path, points),
		Curve::Step => step(&mut path, points, 0.5),
		Curve::StepAfter => step(&mut path, points, 1.0),
		Curve::StepBefore => step(&mut path, points, 0.0),
	}
	path
}

/// A line of a single point is closed, like d3 does.
fn close_single(path: &mut PathData, count: usize) {
	if count == 1 {
		path.close();
	}
}

fn linear(path: &mut PathData, points: &[Point]) {
	for (index, p) in points.iter().enumerate() {
		match index {
			0 => path.move_to(p.x, p.y),
			_ => path.line_to(p.x, p.y),
		}
	}
	close_single(path, points.len());
}

fn basis(path: &mut PathData, points: &[Point]) {
	let (mut x0, mut y0, mut x1, mut y1) = (f64::NAN, f64::NAN, f64::NAN, f64::NAN);
	let bezier = |path: &mut PathData, x0: f64, y0: f64, x1: f64, y1: f64, x: f64, y: f64| {
		path.cubic_to(
			(2.0 * x0 + x1) / 3.0,
			(2.0 * y0 + y1) / 3.0,
			(x0 + 2.0 * x1) / 3.0,
			(y0 + 2.0 * y1) / 3.0,
			(x0 + 4.0 * x1 + x) / 6.0,
			(y0 + 4.0 * y1 + y) / 6.0,
		);
	};
	for (index, p) in points.iter().enumerate() {
		match index {
			0 => path.move_to(p.x, p.y),
			1 => {}
			2 => {
				path.line_to((5.0 * x0 + x1) / 6.0, (5.0 * y0 + y1) / 6.0);
				bezier(path, x0, y0, x1, y1, p.x, p.y);
			}
			_ => bezier(path, x0, y0, x1, y1, p.x, p.y),
		}
		(x0, x1, y0, y1) = (x1, p.x, y1, p.y);
	}
	match points.len() {
		0 => {}
		1 => path.close(),
		2 => path.line_to(x1, y1),
		_ => {
			bezier(path, x0, y0, x1, y1, x1, y1);
			path.line_to(x1, y1);
		}
	}
}

/// d3's curve state of the last three points.
#[derive(Clone, Copy)]
struct Window {
	x0: f64,
	y0: f64,
	x1: f64,
	y1: f64,
	x2: f64,
	y2: f64,
}

impl Window {
	fn new() -> Self {
		Self { x0: f64::NAN, y0: f64::NAN, x1: f64::NAN, y1: f64::NAN, x2: f64::NAN, y2: f64::NAN }
	}

	fn shift(&mut self, x: f64, y: f64) {
		(self.x0, self.x1, self.x2) = (self.x1, self.x2, x);
		(self.y0, self.y1, self.y2) = (self.y1, self.y2, y);
	}
}

fn cardinal(path: &mut PathData, points: &[Point]) {
	let k = 1.0 / 6.0;
	let segment = |path: &mut PathData, w: &Window, x: f64, y: f64| {
		path.cubic_to(
			w.x1 + k * (w.x2 - w.x0),
			w.y1 + k * (w.y2 - w.y0),
			w.x2 + k * (w.x1 - x),
			w.y2 + k * (w.y1 - y),
			w.x2,
			w.y2,
		);
	};
	let mut w = Window::new();
	for (index, p) in points.iter().enumerate() {
		match index {
			0 => path.move_to(p.x, p.y),
			1 => (w.x1, w.y1) = (p.x, p.y),
			_ => segment(path, &w, p.x, p.y),
		}
		w.shift(p.x, p.y);
	}
	match points.len() {
		1 => path.close(),
		2 => path.line_to(w.x2, w.y2),
		n if n > 2 => segment(path, &w, w.x1, w.y1),
		_ => {}
	}
}

fn catmull_rom(path: &mut PathData, points: &[Point]) {
	const ALPHA: f64 = 0.5;
	const EPSILON: f64 = 1e-6;
	#[derive(Default)]
	struct Lengths {
		l01_a: f64,
		l12_a: f64,
		l23_a: f64,
		l01_2a: f64,
		l12_2a: f64,
		l23_2a: f64,
	}
	let segment = |path: &mut PathData, w: &Window, l: &Lengths, x: f64, y: f64| {
		let (mut x1, mut y1, mut x2, mut y2) = (w.x1, w.y1, w.x2, w.y2);
		if l.l01_a > EPSILON {
			let a = 2.0 * l.l01_2a + 3.0 * l.l01_a * l.l12_a + l.l12_2a;
			let n = 3.0 * l.l01_a * (l.l01_a + l.l12_a);
			x1 = (x1 * a - w.x0 * l.l12_2a + w.x2 * l.l01_2a) / n;
			y1 = (y1 * a - w.y0 * l.l12_2a + w.y2 * l.l01_2a) / n;
		}
		if l.l23_a > EPSILON {
			let b = 2.0 * l.l23_2a + 3.0 * l.l23_a * l.l12_a + l.l12_2a;
			let m = 3.0 * l.l23_a * (l.l23_a + l.l12_a);
			x2 = (x2 * b + w.x1 * l.l23_2a - x * l.l12_2a) / m;
			y2 = (y2 * b + w.y1 * l.l23_2a - y * l.l12_2a) / m;
		}
		path.cubic_to(x1, y1, x2, y2, w.x2, w.y2);
	};
	let mut w = Window::new();
	let mut l = Lengths::default();
	let point = |path: &mut PathData,
	             w: &mut Window,
	             l: &mut Lengths,
	             state: &mut usize,
	             x: f64,
	             y: f64| {
		if *state > 0 {
			let (x23, y23) = (w.x2 - x, w.y2 - y);
			l.l23_2a = (x23 * x23 + y23 * y23).powf(ALPHA);
			l.l23_a = l.l23_2a.sqrt();
		}
		match *state {
			0 => {
				*state = 1;
				path.move_to(x, y);
			}
			1 => *state = 2,
			_ => {
				*state = 3;
				segment(path, w, l, x, y);
			}
		}
		(l.l01_a, l.l12_a) = (l.l12_a, l.l23_a);
		(l.l01_2a, l.l12_2a) = (l.l12_2a, l.l23_2a);
		w.shift(x, y);
	};
	let mut state = 0;
	for p in points {
		point(path, &mut w, &mut l, &mut state, p.x, p.y);
	}
	match state {
		1 => path.close(),
		2 => path.line_to(w.x2, w.y2),
		3 => {
			let (x, y) = (w.x2, w.y2);
			point(path, &mut w, &mut l, &mut state, x, y);
		}
		_ => {}
	}
}

fn bump(path: &mut PathData, points: &[Point], horizontal: bool) {
	let (mut x0, mut y0) = (0.0, 0.0);
	for (index, p) in points.iter().enumerate() {
		if index == 0 {
			path.move_to(p.x, p.y);
		} else if horizontal {
			x0 = (x0 + p.x) / 2.0;
			path.cubic_to(x0, y0, x0, p.y, p.x, p.y);
		} else {
			y0 = (y0 + p.y) / 2.0;
			path.cubic_to(x0, y0, p.x, y0, p.x, p.y);
		}
		(x0, y0) = (p.x, p.y);
	}
	close_single(path, points.len());
}

/// `curveMonotoneX`; `swap` reflects x and y for `curveMonotoneY`.
fn monotone(path: &mut PathData, points: &[Point], swap: bool) {
	let sign = |x: f64| if x < 0.0 { -1.0 } else { 1.0 };
	let emit_move = |path: &mut PathData, x: f64, y: f64| match swap {
		true => path.move_to(y, x),
		false => path.move_to(x, y),
	};
	let emit_line = |path: &mut PathData, x: f64, y: f64| match swap {
		true => path.line_to(y, x),
		false => path.line_to(x, y),
	};
	let (mut x0, mut y0, mut x1, mut y1, mut t0) =
		(f64::NAN, f64::NAN, f64::NAN, f64::NAN, f64::NAN);
	let emit_curve = |path: &mut PathData, x0: f64, y0: f64, x1: f64, y1: f64, t0: f64, t1: f64| {
		let dx = (x1 - x0) / 3.0;
		let (a, b, c, d, e, f) = (x0 + dx, y0 + dx * t0, x1 - dx, y1 - dx * t1, x1, y1);
		match swap {
			true => path.cubic_to(b, a, d, c, f, e),
			false => path.cubic_to(a, b, c, d, e, f),
		}
	};
	// `h0 || h1 < 0 && -0`: a zero width takes the sign of the other.
	let divisor = |h: f64, other: f64| {
		if h != 0.0 {
			h
		} else if other < 0.0 {
			-0.0
		} else {
			0.0
		}
	};
	let slope3 = |x0: f64, y0: f64, x1: f64, y1: f64, x2: f64, y2: f64| {
		let h0 = x1 - x0;
		let h1 = x2 - x1;
		let s0 = (y1 - y0) / divisor(h0, h1);
		let s1 = (y2 - y1) / divisor(h1, h0);
		let p = (s0 * h1 + s1 * h0) / (h0 + h1);
		let value = (sign(s0) + sign(s1)) * s0.abs().min(s1.abs()).min(0.5 * p.abs());
		if value.is_nan() { 0.0 } else { value }
	};
	let slope2 = |x0: f64, y0: f64, x1: f64, y1: f64, t: f64| {
		let h = x1 - x0;
		if h != 0.0 && !h.is_nan() { (3.0 * (y1 - y0) / h - t) / 2.0 } else { t }
	};
	let mut state = 0;
	for p in points {
		let (x, y) = if swap { (p.y, p.x) } else { (p.x, p.y) };
		if x == x1 && y == y1 {
			continue;
		}
		let mut t1 = f64::NAN;
		match state {
			0 => {
				state = 1;
				emit_move(path, x, y);
			}
			1 => state = 2,
			2 => {
				state = 3;
				t1 = slope3(x0, y0, x1, y1, x, y);
				emit_curve(path, x0, y0, x1, y1, slope2(x0, y0, x1, y1, t1), t1);
			}
			_ => {
				t1 = slope3(x0, y0, x1, y1, x, y);
				emit_curve(path, x0, y0, x1, y1, t0, t1);
			}
		}
		(x0, x1, y0, y1, t0) = (x1, x, y1, y, t1);
	}
	match state {
		1 => path.close(),
		2 => emit_line(path, x1, y1),
		3 => emit_curve(path, x0, y0, x1, y1, t0, slope2(x0, y0, x1, y1, t0)),
		_ => {}
	}
}

fn natural(path: &mut PathData, points: &[Point]) {
	let n = points.len();
	if n == 0 {
		return;
	}
	path.move_to(points[0].x, points[0].y);
	if n == 1 {
		path.close();
		return;
	}
	if n == 2 {
		path.line_to(points[1].x, points[1].y);
		return;
	}
	let xs: Vec<f64> = points.iter().map(|p| p.x).collect();
	let ys: Vec<f64> = points.iter().map(|p| p.y).collect();
	let (ax, bx) = natural_control_points(&xs);
	let (ay, by) = natural_control_points(&ys);
	for i in 0..n - 1 {
		path.cubic_to(ax[i], ay[i], bx[i], by[i], xs[i + 1], ys[i + 1]);
	}
}

fn natural_control_points(x: &[f64]) -> (Vec<f64>, Vec<f64>) {
	let n = x.len() - 1;
	let mut a = vec![0.0; n];
	let mut b = vec![0.0; n];
	let mut r = vec![0.0; n];
	(a[0], b[0], r[0]) = (0.0, 2.0, x[0] + 2.0 * x[1]);
	for i in 1..n.saturating_sub(1) {
		(a[i], b[i], r[i]) = (1.0, 4.0, 4.0 * x[i] + 2.0 * x[i + 1]);
	}
	(a[n - 1], b[n - 1], r[n - 1]) = (2.0, 7.0, 8.0 * x[n - 1] + x[n]);
	for i in 1..n {
		let m = a[i] / b[i - 1];
		b[i] -= m;
		r[i] -= m * r[i - 1];
	}
	a[n - 1] = r[n - 1] / b[n - 1];
	for i in (0..n - 1).rev() {
		a[i] = (r[i] - a[i + 1]) / b[i];
	}
	b[n - 1] = (x[n] + a[n - 1]) / 2.0;
	for i in 0..n - 1 {
		b[i] = 2.0 * x[i + 1] - a[i + 1];
	}
	(a, b)
}

fn step(path: &mut PathData, points: &[Point], t: f64) {
	let (mut x0, mut y0) = (f64::NAN, f64::NAN);
	for (index, p) in points.iter().enumerate() {
		if index == 0 {
			path.move_to(p.x, p.y);
		} else if t <= 0.0 {
			path.line_to(x0, p.y);
			path.line_to(p.x, p.y);
		} else {
			let x1 = x0 * (1.0 - t) + p.x * t;
			path.line_to(x1, y0);
			path.line_to(x1, p.y);
		}
		(x0, y0) = (p.x, p.y);
	}
	if 0.0 < t && t < 1.0 && points.len() >= 2 {
		path.line_to(x0, y0);
	}
	close_single(path, points.len());
}

/// Mermaid's `generateRoundedPath`: straight segments with quadratic corners of `radius`.
pub fn rounded(points: &[Point], radius: f64) -> PathData {
	let mut path = PathData::default();
	if points.len() < 2 {
		return path;
	}
	let epsilon = 1e-5;
	for (index, current) in points.iter().enumerate() {
		if index == 0 {
			path.move_to(current.x, current.y);
			continue;
		}
		if index == points.len() - 1 {
			path.line_to(current.x, current.y);
			continue;
		}
		let previous = points[index - 1];
		let next = points[index + 1];
		let (dx1, dy1) = (current.x - previous.x, current.y - previous.y);
		let (dx2, dy2) = (next.x - current.x, next.y - current.y);
		let (length1, length2) = (dx1.hypot(dy1), dx2.hypot(dy2));
		if length1 < epsilon || length2 < epsilon {
			path.line_to(current.x, current.y);
			continue;
		}
		let (nx1, ny1, nx2, ny2) = (dx1 / length1, dy1 / length1, dx2 / length2, dy2 / length2);
		let angle = (nx1 * nx2 + ny1 * ny2).clamp(-1.0, 1.0).acos();
		if angle < epsilon || (std::f64::consts::PI - angle).abs() < epsilon {
			path.line_to(current.x, current.y);
			continue;
		}
		let cut = (radius / (angle / 2.0).sin()).min(length1 / 2.0).min(length2 / 2.0);
		path.line_to(current.x - nx1 * cut, current.y - ny1 * cut);
		path.quad_to(current.x, current.y, current.x + nx2 * cut, current.y + ny2 * cut);
	}
	path
}

#[cfg(test)]
mod tests {
	use super::*;

	fn points(coordinates: &[(f64, f64)]) -> Vec<Point> {
		coordinates.iter().map(|&(x, y)| Point { x, y }).collect()
	}

	#[test]
	fn draws_basis_like_d3() {
		let path = line(&points(&[(195.0, 53.0), (195.0, 78.0), (195.0, 99.0)]), Curve::Basis);
		assert_eq!(
			path.to_svg(),
			"M195,53L195,57.167C195,61.333,195,69.667,195,77.333C195,85,195,92,195,95.5L195,99"
		);
		assert!((path.length() - 46.0).abs() < 1e-6);
		let (min, max) = path.bounds().unwrap();
		assert_eq!((min.y, max.y), (53.0, 99.0));
	}

	#[test]
	fn draws_other_curves() {
		let input = points(&[(0.0, 0.0), (10.0, 10.0), (20.0, 0.0)]);
		assert_eq!(line(&input, Curve::Linear).to_svg(), "M0,0L10,10L20,0");
		assert_eq!(line(&input, Curve::Step).to_svg(), "M0,0L5,0L5,10L15,10L15,0L20,0");
		assert_eq!(line(&input, Curve::StepBefore).to_svg(), "M0,0L0,10L10,10L10,0L20,0");
		assert_eq!(line(&input[..1], Curve::Basis).to_svg(), "M0,0Z");
		for curve in [
			Curve::Cardinal,
			Curve::CatmullRom,
			Curve::MonotoneX,
			Curve::MonotoneY,
			Curve::Natural,
			Curve::BumpX,
		] {
			let d = line(&input, curve).to_svg();
			assert!(d.starts_with("M0,0") && d.ends_with("20,0"), "{curve:?}: {d}");
		}
		assert_eq!(rounded(&input, 5.0).to_svg().chars().filter(|&c| c == 'Q').count(), 1);
	}
}
