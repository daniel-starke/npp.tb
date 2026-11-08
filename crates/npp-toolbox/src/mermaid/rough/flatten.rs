//! `pointsOnPath` of points-on-path 0.2.1 and the flattening of points-on-curve 0.2.0 (both MIT,
//! copyright (c) Preet Shihn), with explicit bounds where the originals recurse without one.

use super::Xy;
use super::path_data::Segment;

/// Flat for all coordinates up to about 4e9. Curves beyond it stop at this depth.
const MAX_SPLIT_DEPTH: u32 = 16;

fn distance_squared(a: Xy, b: Xy) -> f64 {
	(a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])
}

fn lerp(a: Xy, b: Xy, t: f64) -> Xy {
	[a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn distance_to_segment_squared(p: Xy, v: Xy, w: Xy) -> f64 {
	let length_squared = distance_squared(v, w);
	if length_squared == 0.0 {
		return distance_squared(p, v);
	}
	let t = ((p[0] - v[0]) * (w[0] - v[0]) + (p[1] - v[1]) * (w[1] - v[1])) / length_squared;
	distance_squared(p, lerp(v, w, t.clamp(0.0, 1.0)))
}

fn flatness([p1, p2, p3, p4]: [Xy; 4]) -> f64 {
	let square = |value: f64| value * value;
	let ux = square(3.0 * p2[0] - 2.0 * p1[0] - p4[0]);
	let uy = square(3.0 * p2[1] - 2.0 * p1[1] - p4[1]);
	let vx = square(3.0 * p3[0] - 2.0 * p4[0] - p1[0]);
	let vy = square(3.0 * p3[1] - 2.0 * p4[1] - p1[1]);
	ux.max(vx) + uy.max(vy)
}

/// `getPointsOnBezierCurveWithSplitting`.
fn split_curve(curve: [Xy; 4], tolerance: f64, depth: u32, out: &mut Vec<Xy>) {
	let [p1, p2, p3, p4] = curve;
	if flatness(curve) < tolerance || depth >= MAX_SPLIT_DEPTH {
		match out.last() {
			Some(&last) if distance_squared(last, p1).sqrt() > 1.0 => out.push(p1),
			Some(_) => {}
			None => out.push(p1),
		}
		out.push(p4);
	} else {
		let (q1, q2, q3) = (lerp(p1, p2, 0.5), lerp(p2, p3, 0.5), lerp(p3, p4, 0.5));
		let (r1, r2) = (lerp(q1, q2, 0.5), lerp(q2, q3, 0.5));
		let red = lerp(r1, r2, 0.5);
		split_curve([p1, q1, r1, red], tolerance, depth + 1, out);
		split_curve([red, r2, q3, p4], tolerance, depth + 1, out);
	}
}

/// `simplifyPoints`, whose recursion goes to the left part first.
fn simplify(points: &[Xy], epsilon: f64) -> Vec<Xy> {
	let mut out = Vec::new();
	let mut ranges = vec![(0, points.len())];
	while let Some((start, end)) = ranges.pop() {
		let (Some(&first), Some(&last)) = (points.get(start), points.get(end.wrapping_sub(1)))
		else {
			continue;
		};
		let mut max_distance_squared = 0.0;
		let mut farthest = 1;
		for (index, point) in points.iter().enumerate().take(end - 1).skip(start + 1) {
			let distance = distance_to_segment_squared(*point, first, last);
			if distance > max_distance_squared {
				max_distance_squared = distance;
				farthest = index;
			}
		}
		if f64::sqrt(max_distance_squared) > epsilon {
			ranges.push((farthest, end));
			ranges.push((start, farthest + 1));
		} else {
			if out.is_empty() {
				out.push(first);
			}
			out.push(last);
		}
	}
	out
}

/// `appendPendingCurve`: a `pointsOnBezierCurves` of its own, whose points are then added.
fn append_pending_curve(pending: &mut Vec<Xy>, points: &mut Vec<Xy>, tolerance: f64) {
	if pending.len() >= 4 {
		let mut flattened = Vec::new();
		for curve in pending.windows(4).step_by(3) {
			split_curve([curve[0], curve[1], curve[2], curve[3]], tolerance, 0, &mut flattened);
		}
		points.append(&mut flattened);
	}
	pending.clear();
}

/// `pointsOnPath`, with the path already normalized.
pub fn points_on_path(segments: &[Segment], tolerance: f64, distance: f64) -> Vec<Vec<Xy>> {
	let mut sets: Vec<Vec<Xy>> = Vec::new();
	let mut points: Vec<Xy> = Vec::new();
	let mut start = [0.0, 0.0];
	let mut pending: Vec<Xy> = Vec::new();
	for segment in segments {
		match *segment {
			Segment::Move(point) => {
				append_pending_curve(&mut pending, &mut points, tolerance);
				if !points.is_empty() {
					sets.push(std::mem::take(&mut points));
				}
				start = point;
				points.push(start);
			}
			Segment::Line(point) => {
				append_pending_curve(&mut pending, &mut points, tolerance);
				points.push(point);
			}
			Segment::Curve([x1, y1, x2, y2, x, y]) => {
				if pending.is_empty() {
					pending.push(points.last().copied().unwrap_or(start));
				}
				pending.extend([[x1, y1], [x2, y2], [x, y]]);
			}
			Segment::Close => {
				append_pending_curve(&mut pending, &mut points, tolerance);
				points.push(start);
			}
		}
	}
	append_pending_curve(&mut pending, &mut points, tolerance);
	if !points.is_empty() {
		sets.push(points);
	}
	sets.iter().map(|set| simplify(set, distance)).filter(|set| !set.is_empty()).collect()
}
