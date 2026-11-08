//! `getBBox` as Blink computes it in single precision: of SVG paths of relative arcs, which
//! become conics (Skia's `SkPath::arcTo` for SVG arcs), and of absolute lines and cubics (the
//! sketches of roughjs), whose box is `SkPath::computeTightBounds`, and of groups, which move
//! and unite the boxes of their children as `gfx::RectF`. Layouts that take the box as the node
//! size (cose-bilkent) amplify a difference in its last bit.

/// An arc `a rx,ry rotation large,sweep dx,dy` of the path data.
#[derive(Clone, Copy, Debug)]
pub struct Arc {
	pub rx: f64,
	pub ry: f64,
	pub rotation: f64,
	pub is_large: bool,
	pub is_sweep: bool,
	pub dx: f64,
	pub dy: f64,
}

/// An affine `SkMatrix`: `x' = x * sx + y * kx`, `y' = x * ky + y * sy` (no translation is
/// needed here).
#[derive(Clone, Copy)]
struct Matrix {
	sx: f32,
	kx: f32,
	ky: f32,
	sy: f32,
}

impl Matrix {
	/// `setRotate`.
	fn rotation(degrees: f32) -> Matrix {
		let radians = degrees * (std::f32::consts::PI / 180.0);
		let (sin, cos) = (snap_to_zero(sinf(radians)), snap_to_zero(cosf(radians)));
		Matrix { sx: cos, kx: -sin, ky: sin, sy: cos }
	}

	fn map(self, (x, y): (f32, f32)) -> (f32, f32) {
		(x * self.sx + y * self.kx, x * self.ky + y * self.sy)
	}
}

/// A number of the path data as Blink's `GenericParseNumber` reads it: in single precision,
/// the decimals summed one by one, from the text JavaScript writes for `value`.
pub fn parse_number(value: f64) -> f32 {
	let text = super::super::super::color::js_number(value);
	let (mantissa, exponent) = match text.split_once('e') {
		Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().unwrap_or(0)),
		None => (text.as_str(), 0),
	};
	let (sign, digits) = match mantissa.strip_prefix('-') {
		Some(digits) => (-1.0f32, digits),
		None => (1.0, mantissa),
	};
	let (integer_digits, decimal_digits) = digits.split_once('.').unwrap_or((digits, ""));
	let mut integer = 0.0f32;
	let mut multiplier = 1.0f32;
	for digit in integer_digits.bytes().rev() {
		integer += multiplier * f32::from(digit - b'0');
		multiplier *= 10.0;
	}
	let mut decimal = 0.0f32;
	let mut fraction = 1.0f32;
	for digit in decimal_digits.bytes() {
		fraction *= 0.1;
		decimal += f32::from(digit - b'0') * fraction;
	}
	let mut number = (integer + decimal) * sign;
	if exponent != 0 {
		number *= 10f64.powi(exponent) as f32;
	}
	number
}

/// A number of the path data like [`parse_number`], `None` where Blink's parser stops: a value
/// that is not finite in JavaScript's text or in single precision.
pub fn parse_finite_number(value: f64) -> Option<f32> {
	match value.is_finite() {
		true => Some(parse_number(value)).filter(|number| number.is_finite()),
		false => None,
	}
}

/// A `gfx::RectF`: the box Blink keeps for an SVG element, in single precision. A size of at
/// most `kTrivial` is 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectF {
	pub x: f32,
	pub y: f32,
	pub width: f32,
	pub height: f32,
}

/// `SizeF::kTrivial`.
const TRIVIAL_SIZE: f32 = 8.0 * f32::EPSILON;

/// `SizeF::clamp`.
fn clamp_size(size: f32) -> f32 {
	if size > TRIVIAL_SIZE { size } else { 0.0 }
}

impl RectF {
	/// `gfx::BoundingRect` of the corners, as `SkRectToRectF` converts Skia's box: a size whose
	/// end misses the far corner grows to the next number.
	pub fn bounding(left: f32, top: f32, right: f32, bottom: f32) -> RectF {
		let size = |start: f32, end: f32| {
			let mut size = end - start;
			if start + size != end {
				size = size.next_up();
				if size.is_infinite() {
					size = f32::MAX;
				}
			}
			clamp_size(size)
		};
		RectF { x: left, y: top, width: size(left, right), height: size(top, bottom) }
	}

	pub fn right(self) -> f32 {
		self.x + self.width
	}

	pub fn bottom(self) -> f32 {
		self.y + self.height
	}

	/// `RectF::UnionEvenIfEmpty`, which grows the size where rounding misses an end.
	pub fn union(self, other: RectF) -> RectF {
		let (x, y) = (self.x.min(other.x), self.y.min(other.y));
		let (right, bottom) = (self.right().max(other.right()), self.bottom().max(other.bottom()));
		let mut union =
			RectF { x, y, width: clamp_size(right - x), height: clamp_size(bottom - y) };
		// `SizeF::SetToNextWidth`.
		let next = |size: f32| size.max(TRIVIAL_SIZE).next_up();
		if union.right() < right && union.width < f32::MAX {
			union.width = next(union.width);
		}
		if union.bottom() < bottom && union.height < f32::MAX {
			union.height = next(union.height);
		}
		union
	}

	/// `AffineTransform::MapRect` of a translation: the corners are moved and the size is the
	/// distance of the moved corners (`QuadF::BoundingBox`), which can lose its last bit.
	pub fn translated(self, dx: f32, dy: f32) -> RectF {
		if dx == 0.0 && dy == 0.0 {
			return self;
		}
		let (x, y) = (self.x + dx, self.y + dy);
		let (right, bottom) = (self.right() + dx, self.bottom() + dy);
		RectF { x, y, width: clamp_size(right - x), height: clamp_size(bottom - y) }
	}
}

/// The union of a container's boxes in document order (`UpdateObjectBoundingBox`): the first
/// box is taken as it is.
pub fn union_all(boxes: impl IntoIterator<Item = RectF>) -> Option<RectF> {
	boxes.into_iter().reduce(RectF::union)
}

/// A command of absolute path data.
#[derive(Clone, Copy, Debug)]
pub enum PathCommand {
	Move([f64; 2]),
	Line([f64; 2]),
	Cubic([f64; 6]),
}

/// `getBBox` of absolute path data in Blink: the numbers parsed in single precision, the box
/// of `SkPath::computeTightBounds` and its conversion to a `RectF`. Parsing stops at a number
/// Blink rejects; `None` if no point is left.
pub fn path_bounds(commands: impl IntoIterator<Item = PathCommand>) -> Option<RectF> {
	let mut bounds: Option<[f32; 4]> = None;
	let mut include = |(x, y): (f32, f32)| {
		let [left, top, right, bottom] = bounds.get_or_insert([x, y, x, y]);
		(*left, *top, *right, *bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
	};
	let mut current = (0.0f32, 0.0f32);
	let point = |x: f64, y: f64| Some((parse_finite_number(x)?, parse_finite_number(y)?));
	for command in commands {
		match command {
			PathCommand::Move([x, y]) | PathCommand::Line([x, y]) => {
				let Some(end) = point(x, y) else { break };
				include(end);
				current = end;
			}
			PathCommand::Cubic([x1, y1, x2, y2, x, y]) => {
				let (Some(control1), Some(control2), Some(end)) =
					(point(x1, y1), point(x2, y2), point(x, y))
				else {
					break;
				};
				let xs = [current.0, control1.0, control2.0, end.0];
				let ys = [current.1, control1.1, control2.1, end.1];
				// `compute_cubic_extremas`: each parameter gives a whole point.
				let ts: Vec<f32> = cubic_extrema(xs).into_iter().chain(cubic_extrema(ys)).collect();
				for t in ts {
					include((eval_cubic(xs, t), eval_cubic(ys, t)));
				}
				include(end);
				current = end;
			}
		}
	}
	bounds.map(|[left, top, right, bottom]| RectF::bounding(left, top, right, bottom))
}

/// `SkFindCubicExtrema`.
fn cubic_extrema([a, b, c, d]: [f32; 4]) -> Vec<f32> {
	let big_a = d - a + 3.0 * (b - c);
	let big_b = 2.0 * (a - b - b + c);
	let big_c = b - a;
	unit_quad_roots(big_a, big_b, big_c)
}

/// `SkCubicCoeff::eval` of one coordinate.
fn eval_cubic([p0, p1, p2, p3]: [f32; 4], t: f32) -> f32 {
	let a = p3 + 3.0 * (p1 - p2) - p0;
	let b = 3.0 * (p2 - (p1 + p1) + p0);
	let c = 3.0 * (p1 - p0);
	((a * t + b) * t + c) * t + p0
}

/// `SkScalarSinSnapToZero` and its cosine.
fn snap_to_zero(value: f32) -> f32 {
	if value.abs() <= 1.0 / 4096.0 { 0.0 } else { value }
}

/// The C runtime's single precision functions, correctly rounded.
fn sinf(x: f32) -> f32 {
	f64::from(x).sin() as f32
}

fn cosf(x: f32) -> f32 {
	f64::from(x).cos() as f32
}

fn tanf(x: f32) -> f32 {
	f64::from(x).tan() as f32
}

fn atan2f(y: f32, x: f32) -> f32 {
	f64::from(y).atan2(f64::from(x)) as f32
}

/// A path segment after the current point.
enum Segment {
	Line((f32, f32)),
	Conic((f32, f32), (f32, f32), f32),
}

/// `SkPath::arcTo` of an SVG arc from `from` to `to`.
fn arc_to(from: (f32, f32), to: (f32, f32), arc: &Arc) -> Vec<Segment> {
	let (mut rx, mut ry) = (parse_number(arc.rx), parse_number(arc.ry));
	if rx == 0.0 || ry == 0.0 || from == to {
		return vec![Segment::Line(to)];
	}
	(rx, ry) = (rx.abs(), ry.abs());
	let angle = parse_number(arc.rotation);
	let middle = ((from.0 - to.0) * 0.5, (from.1 - to.1) * 0.5);
	let middle = Matrix::rotation(-angle).map(middle);
	let radii_scale = middle.0 * middle.0 / (rx * rx) + middle.1 * middle.1 / (ry * ry);
	if radii_scale > 1.0 {
		let scale = radii_scale.sqrt();
		rx *= scale;
		ry *= scale;
	}
	// `setScale(1 / rx, 1 / ry)` and `preRotate(-angle)`; Skia concatenates in double
	// precision, which equals the single precision products here.
	let rotation = Matrix::rotation(-angle);
	let (scale_x, scale_y) = (1.0 / rx, 1.0 / ry);
	let inverse = Matrix {
		sx: scale_x * rotation.sx,
		kx: scale_x * rotation.kx,
		ky: scale_y * rotation.ky,
		sy: scale_y * rotation.sy,
	};
	let (mut unit0, mut unit1) = (inverse.map(from), inverse.map(to));
	let mut delta = (unit1.0 - unit0.0, unit1.1 - unit0.1);
	let d = delta.0 * delta.0 + delta.1 * delta.1;
	let mut scale_factor = (1.0 / d - 0.25).max(0.0).sqrt();
	// `(arcSweep == kCCW) != arcLarge` with SVG's sweep flag 1 being clockwise.
	if !arc.is_sweep != arc.is_large {
		scale_factor = -scale_factor;
	}
	delta = (delta.0 * scale_factor, delta.1 * scale_factor);
	let mut center = (unit0.0 + unit1.0, unit0.1 + unit1.1);
	center = (center.0 * 0.5 - delta.1, center.1 * 0.5 + delta.0);
	unit0 = (unit0.0 - center.0, unit0.1 - center.1);
	unit1 = (unit1.0 - center.0, unit1.1 - center.1);
	let theta1 = atan2f(unit0.1, unit0.0);
	let theta2 = atan2f(unit1.1, unit1.0);
	let mut theta_arc = theta2 - theta1;
	const PI: f32 = std::f32::consts::PI;
	if theta_arc < 0.0 && arc.is_sweep {
		theta_arc += PI * 2.0;
	} else if theta_arc > 0.0 && !arc.is_sweep {
		theta_arc -= PI * 2.0;
	}
	if theta_arc.abs() < PI / (1000.0 * 1000.0) {
		return vec![Segment::Line(to)];
	}
	// `setRotate(angle)` and `preScale(rx, ry)`.
	let rotation = Matrix::rotation(angle);
	let forward = Matrix {
		sx: rotation.sx * rx,
		kx: rotation.kx * ry,
		ky: rotation.ky * rx,
		sy: rotation.sy * ry,
	};
	let segments = (theta_arc / (2.0 * PI / 3.0)).abs().ceil();
	if !(segments.is_finite() && segments <= 4.0) {
		return vec![Segment::Line(to)];
	}
	let theta_width = theta_arc / segments;
	let t = tanf(0.5 * theta_width);
	if !t.is_finite() {
		return Vec::new();
	}
	let w = (0.5 + cosf(theta_width) * 0.5).sqrt();
	let is_integer = |value: f32| value == value.floor();
	let expects_integers = (PI / 2.0 - theta_width.abs()).abs() <= 1.0 / 4096.0
		&& is_integer(rx)
		&& is_integer(ry)
		&& is_integer(to.0)
		&& is_integer(to.1);
	let mut start_theta = theta1;
	let mut result = Vec::new();
	for _ in 0..segments as i32 {
		let end_theta = start_theta + theta_width;
		let (sin_end, cos_end) = (snap_to_zero(sinf(end_theta)), snap_to_zero(cosf(end_theta)));
		let end = (cos_end + center.0, sin_end + center.1);
		let control = (end.0 + t * sin_end, end.1 + -t * cos_end);
		let (mut control, mut end) = (forward.map(control), forward.map(end));
		if expects_integers {
			let round = |(x, y): (f32, f32)| ((x + 0.5).floor(), (y + 0.5).floor());
			(control, end) = (round(control), round(end));
		}
		result.push(Segment::Conic(control, end, w));
		start_theta = end_theta;
	}
	// `setLastPt`: the arc ends exactly at its target.
	if let Some(Segment::Conic(_, end, _)) = result.last_mut() {
		*end = to;
	}
	result
}

/// `valid_unit_divide`.
fn unit_divide(numerator: f32, denominator: f32) -> Option<f32> {
	let (mut numerator, mut denominator) = (numerator, denominator);
	if numerator < 0.0 {
		numerator = -numerator;
		denominator = -denominator;
	}
	if denominator == 0.0 || numerator == 0.0 || numerator >= denominator {
		return None;
	}
	let ratio = numerator / denominator;
	(!ratio.is_nan() && ratio != 0.0).then_some(ratio)
}

/// `SkFindUnitQuadRoots`.
fn unit_quad_roots(a: f32, b: f32, c: f32) -> Vec<f32> {
	if a == 0.0 {
		return unit_divide(-c, b).into_iter().collect();
	}
	let discriminant = f64::from(b) * f64::from(b) - 4.0 * f64::from(a) * f64::from(c);
	if discriminant < 0.0 {
		return Vec::new();
	}
	let r = discriminant.sqrt() as f32;
	if !r.is_finite() {
		return Vec::new();
	}
	let q = if b < 0.0 { -(b - r) / 2.0 } else { -(b + r) / 2.0 };
	let mut roots: Vec<f32> = unit_divide(q, a).into_iter().chain(unit_divide(c, q)).collect();
	if roots.len() == 2 {
		if roots[0] > roots[1] {
			roots.swap(0, 1);
		} else if roots[0] == roots[1] {
			roots.pop();
		}
	}
	roots
}

/// `conic_find_extrema` of one coordinate: the parameter of its single extremum.
fn conic_extremum(p0: f32, p1: f32, p2: f32, w: f32) -> Option<f32> {
	let p20 = p2 - p0;
	let p10 = p1 - p0;
	let wp10 = w * p10;
	match unit_quad_roots(w * p20 - p20, p20 - 2.0 * wp10, wp10).as_slice() {
		[t] => Some(*t),
		_ => None,
	}
}

/// `SkConicCoeff::eval` of one coordinate.
fn eval_conic(p0: f32, p1: f32, p2: f32, w: f32, t: f32) -> f32 {
	let p1w = p1 * w;
	let a = p2 - (p1w + p1w) + p0;
	let b = (p1w - p0) + (p1w - p0);
	let denominator_b = (w - 1.0) + (w - 1.0);
	let denominator_a = 0.0 - denominator_b;
	let numerator = (a * t + b) * t + p0;
	let denominator = (denominator_a * t + denominator_b) * t + 1.0;
	numerator / denominator
}

/// The box of `M0 0` followed by the relative `arcs` and `H0 V0 Z`: left, top, right and
/// bottom.
pub fn arc_path_bounds(arcs: &[Arc]) -> [f32; 4] {
	let mut min = (0.0f32, 0.0f32);
	let mut max = (0.0f32, 0.0f32);
	let mut include = |(x, y): (f32, f32)| {
		min = (min.0.min(x), min.1.min(y));
		max = (max.0.max(x), max.1.max(y));
	};
	let mut current = (0.0f32, 0.0f32);
	for arc in arcs {
		let target = (current.0 + parse_number(arc.dx), current.1 + parse_number(arc.dy));
		let mut last = current;
		for segment in arc_to(current, target, arc) {
			match segment {
				Segment::Line(end) => {
					include(end);
					last = end;
				}
				Segment::Conic(control, end, w) => {
					let ts = [
						conic_extremum(last.0, control.0, end.0, w),
						conic_extremum(last.1, control.1, end.1, w),
					];
					for t in ts.into_iter().flatten() {
						include((
							eval_conic(last.0, control.0, end.0, w, t),
							eval_conic(last.1, control.1, end.1, w, t),
						));
					}
					include(end);
					last = end;
				}
			}
		}
		current = target;
	}
	include((0.0, current.1));
	[min.0, min.1, max.0, max.1]
}

#[cfg(test)]
mod tests {
	use super::*;

	fn arc(rx: f64, ry: f64, rotation: f64, sweep: bool, dx: f64, dy: f64) -> Arc {
		Arc { rx, ry, rotation, is_large: false, is_sweep: sweep, dx, dy }
	}

	#[test]
	fn bounds_curves_like_blink() {
		// `getBBox` in Edge: the parsed numbers and the extremes in single precision, the size
		// grown where the end is missed.
		let curve = |numbers: [f64; 8]| {
			let [x0, y0, rest @ ..] = numbers;
			let rect = path_bounds([PathCommand::Move([x0, y0]), PathCommand::Cubic(rest)]);
			rect.map(|rect| [rect.x, rect.y, rect.width, rect.height].map(f64::from))
		};
		assert_eq!(
			curve([
				-1.4984904806381678,
				31.116447694574028,
				4.148807623313764,
				23.66757191809302,
				11.420953414584403,
				16.571483179624565,
				27.95816909062682,
				-3.573796918120046
			]),
			Some([-1.4984904527664185, -3.5737969875335693, 29.456661224365234, 34.69024658203125])
		);
		assert_eq!(
			curve([
				-27.6107822173013,
				-2.695064633129345,
				-22.641051219864046,
				-9.285239750022214,
				-17.794854048970784,
				-14.661735506534022,
				-5.833498844051765,
				-28.372685858764775
			]),
			Some([-27.610782623291016, -28.3726863861084, 21.777284622192383, 25.677623748779297])
		);
		assert_eq!(path_bounds([PathCommand::Move([f64::NAN, 0.0])]), None);
	}

	#[test]
	fn moves_boxes_like_blink() {
		// `getBBox` in Edge of a group around a moved path: the width loses its last bit.
		let [x, y, width, height] =
			[-0.9706512689590454, -42.59003448486328, 135.3415985107422, 43.88985824584961]
				.map(|value: f64| value as f32);
		let path = RectF { x, y, width, height };
		let moved = path.translated(-66.953125, 20.5);
		assert_eq!(
			[moved.x, moved.y, moved.width, moved.height].map(f64::from),
			[-67.92377471923828, -22.09003448486328, 135.34158325195312, 43.88985824584961]
		);
		assert_eq!(path.translated(0.0, 0.0), path);
	}

	#[test]
	fn unites_boxes_like_blink() {
		let a = RectF { x: -1.0, y: 0.0, width: 1.0, height: 1.0 };
		let b = RectF { x: 0.0, y: 2.0, width: 0.0, height: 1.0 };
		assert_eq!(union_all([a, b]), Some(RectF { x: -1.0, y: 0.0, width: 1.0, height: 3.0 }));
		// The rounded size misses the right end and grows to the next number.
		let c = RectF { x: 0.1, y: 0.0, width: 1.0, height: 1.0 };
		let d = RectF { x: -3.0e-8, y: 0.0, width: 1.0, height: 1.0 };
		let union = c.union(d);
		assert!(union.right() >= c.right());
		assert_eq!(union_all([]), None);
	}

	#[test]
	fn bounds_arcs_like_blink() {
		// `getBBox` in Edge of `M0 0 a10,10 0 0,1 20,0 H0 V0 Z` and of a cloud.
		assert_eq!(
			arc_path_bounds(&[arc(10.0, 10.0, 0.0, true, 20.0, 0.0)]),
			[0.0, -10.0, 20.0, 0.0]
		);
		let (r1, r2, r3, r4) =
			(17.71601562499999, 29.52669270833332, 41.33736979166665, 23.62135416666666);
		let cloud = [
			arc(r1, r1, 0.0, true, 29.52669270833332, -11.810677083333328),
			arc(r3, r3, 1.0, true, 47.24270833333331, -11.810677083333328),
			arc(r2, r2, 1.0, true, 41.33736979166665, 23.621354166666656),
			arc(r1, r1, 1.0, true, 17.71601562499999, 10.85),
			arc(r4, r4, 1.0, true, -17.71601562499999, 20.15),
			arc(r2, r1, 1.0, true, -29.52669270833332, 17.71601562499999),
			arc(r3, r3, 1.0, true, -59.05338541666664, 0.0),
			arc(r1, r1, 1.0, true, -29.52669270833332, -17.71601562499999),
			arc(r1, r1, 1.0, true, -11.810677083333328, -10.85),
			arc(r4, r4, 1.0, true, 11.810677083333328, -20.15),
		];
		let [left, top, right, bottom] = arc_path_bounds(&cloud);
		let rect = [left, top, right - left, bottom - top].map(f64::from);
		assert_eq!(
			rect,
			[-11.810686111450195, -26.645158767700195, 147.6334686279297, 87.76841735839844]
		);
	}
}
