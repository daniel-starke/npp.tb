//! Line hops where swimlane edges cross (`rendering-elements/lineJump.ts`).

use super::super::color::{js_number, js_round};
use super::Point;

const ROUNDED_CORNER_RADIUS: f64 = 5.0;
const CORNER_EPSILON: f64 = 1e-5;
const CORNER_JUMP_CLEARANCE: f64 = 2.0;
const MIN_USEFUL_RADIUS_RATIO: f64 = 0.6;
const ENDPOINT_EPSILON: f64 = 1e-6;
pub const JUMP_RADIUS: f64 = 6.0;

/// An edge as the hops see it (`EdgeGeom`): the clipped points of the rendered path.
pub struct EdgeGeometry<'a> {
	pub points: &'a [Point],
	pub is_rounded: bool,
	pub start_offset: Option<f64>,
	pub end_offset: Option<f64>,
}

/// A hop on an edge: its segment, the position along it and the crossing point.
#[derive(Clone, Copy)]
struct Crossing {
	edge: usize,
	segment: usize,
	t: f64,
	point: Point,
}

/// The rewritten path of an edge with hops.
pub struct JumpPath {
	pub d: String,
	pub length: f64,
	pub bounds: (Point, Point),
}

fn segment_intersection(a1: Point, a2: Point, b1: Point, b2: Point) -> Option<(Point, f64, f64)> {
	let (dx_a, dy_a) = (a2.x - a1.x, a2.y - a1.y);
	let (dx_b, dy_b) = (b2.x - b1.x, b2.y - b1.y);
	let denominator = dx_a * dy_b - dy_a * dx_b;
	if denominator == 0.0 {
		return None;
	}
	let (dx, dy) = (b1.x - a1.x, b1.y - a1.y);
	let t_a = (dx * dy_b - dy * dx_b) / denominator;
	let t_b = (dx * dy_a - dy * dx_a) / denominator;
	let inside = |t: f64| t > ENDPOINT_EPSILON && t < 1.0 - ENDPOINT_EPSILON;
	(inside(t_a) && inside(t_b)).then_some((
		Point { x: a1.x + t_a * dx_a, y: a1.y + t_a * dy_a },
		t_a,
		t_b,
	))
}

fn is_horizontal(a: Point, b: Point) -> bool {
	(b.x - a.x).abs() >= (b.y - a.y).abs()
}

/// `computeRoundedCorner`: where the rounding of the corner at `current` starts and ends.
fn rounded_corner(
	previous: Point,
	current: Point,
	next: Point,
	radius: f64,
) -> Option<(Point, Point, f64)> {
	let (dx1, dy1, dx2, dy2) =
		(current.x - previous.x, current.y - previous.y, next.x - current.x, next.y - current.y);
	let (length1, length2) = (dx1.hypot(dy1), dx2.hypot(dy2));
	if length1 < CORNER_EPSILON || length2 < CORNER_EPSILON {
		return None;
	}
	let (nx1, ny1, nx2, ny2) = (dx1 / length1, dy1 / length1, dx2 / length2, dy2 / length2);
	let angle = (nx1 * nx2 + ny1 * ny2).clamp(-1.0, 1.0).acos();
	if angle < CORNER_EPSILON || (std::f64::consts::PI - angle).abs() < CORNER_EPSILON {
		return None;
	}
	let cut = (radius / (angle / 2.0).sin()).min(length1 / 2.0).min(length2 / 2.0);
	Some((
		Point { x: current.x - nx1 * cut, y: current.y - ny1 * cut },
		Point { x: current.x + nx2 * cut, y: current.y + ny2 * cut },
		cut,
	))
}

/// `crossingSitsInRoundedCorner`.
fn in_rounded_corner(edge: &EdgeGeometry, segment: usize, t: f64) -> bool {
	if !edge.is_rounded {
		return false;
	}
	let points = edge.points;
	let (Some(&a), Some(&b)) = (points.get(segment), points.get(segment + 1)) else {
		return false;
	};
	let distance = t * (b.x - a.x).hypot(b.y - a.y);
	let length = (b.x - a.x).hypot(b.y - a.y);
	if segment > 0
		&& let Some((_, _, cut)) = rounded_corner(points[segment - 1], a, b, ROUNDED_CORNER_RADIUS)
		&& distance < cut
	{
		return true;
	}
	segment + 2 < points.len()
		&& rounded_corner(a, b, points[segment + 2], ROUNDED_CORNER_RADIUS)
			.is_some_and(|(_, _, cut)| length - distance < cut)
}

/// `findEdgeIntersections`: the hop goes on the horizontal one of two crossing segments, else
/// on the later edge.
fn find_intersections(edges: &[EdgeGeometry]) -> Vec<Crossing> {
	let mut crossings = Vec::new();
	for i in 0..edges.len() {
		for j in i + 1..edges.len() {
			for (si, a) in edges[i].points.windows(2).enumerate() {
				for (sj, b) in edges[j].points.windows(2).enumerate() {
					let Some((point, t_a, t_b)) = segment_intersection(a[0], a[1], b[0], b[1])
					else {
						continue;
					};
					if in_rounded_corner(&edges[i], si, t_a)
						|| in_rounded_corner(&edges[j], sj, t_b)
					{
						continue;
					}
					let (a_horizontal, b_horizontal) =
						(is_horizontal(a[0], a[1]), is_horizontal(b[0], b[1]));
					crossings.push(match a_horizontal != b_horizontal && a_horizontal {
						true => Crossing { edge: i, segment: si, t: t_a, point },
						false => Crossing { edge: j, segment: sj, t: t_b, point },
					});
				}
			}
		}
	}
	crossings
}

/// `fmt`: three decimals in JavaScript's number format.
fn format(value: f64) -> String {
	js_number(js_round(value * 1000.0) / 1000.0)
}

fn point_text(point: Point) -> String {
	format!("{},{}", format(point.x), format(point.y))
}

/// The rewritten paths of the edges that get hops, with `gap` instead of arcs if asked.
pub fn jump_paths(edges: &[EdgeGeometry], use_gaps: bool) -> Vec<Option<JumpPath>> {
	let crossings = find_intersections(edges);
	let mut result: Vec<Option<JumpPath>> = edges.iter().map(|_| None).collect();
	for (index, edge) in edges.iter().enumerate() {
		let jumps: Vec<Crossing> =
			crossings.iter().copied().filter(|crossing| crossing.edge == index).collect();
		if !jumps.is_empty() && edge.points.len() >= 2 {
			result[index] = Some(rewrite(edge, &jumps, use_gaps));
		}
	}
	result
}

/// `rewriteEdgePath`.
fn rewrite(edge: &EdgeGeometry, jumps: &[Crossing], use_gaps: bool) -> JumpPath {
	let mut points = edge.points.to_vec();
	let count = points.len();
	if let Some(offset) = edge.start_offset {
		let angle = (points[1].y - points[0].y).atan2(points[1].x - points[0].x);
		points[0] =
			Point { x: points[0].x + offset * angle.cos(), y: points[0].y + offset * angle.sin() };
	}
	if let Some(offset) = edge.end_offset {
		let (a, b) = (edge.points[count - 2], edge.points[count - 1]);
		let angle = (b.y - a.y).atan2(b.x - a.x);
		points[count - 1] = Point { x: b.x - offset * angle.cos(), y: b.y - offset * angle.sin() };
	}
	let mut parts = vec![format!("M{}", point_text(points[0]))];
	let mut path = super::curves::PathData::default();
	path.move_to(points[0].x, points[0].y);
	let mut arcs_length = 0.0;
	let mut bounds = (points[0], points[0]);
	let mut include = |point: Point| {
		bounds.0 = Point { x: bounds.0.x.min(point.x), y: bounds.0.y.min(point.y) };
		bounds.1 = Point { x: bounds.1.x.max(point.x), y: bounds.1.y.max(point.y) };
	};
	let minimum_radius = JUMP_RADIUS * MIN_USEFUL_RADIUS_RATIO;
	for segment in 0..count - 1 {
		let (a, b) = (points[segment], points[segment + 1]);
		let length = (b.x - a.x).hypot(b.y - a.y);
		let (ux, uy) =
			if length == 0.0 { (0.0, 0.0) } else { ((b.x - a.x) / length, (b.y - a.y) / length) };
		let sweep = match (b.x - a.x).abs() >= (b.y - a.y).abs() {
			true => u8::from(b.x - a.x >= 0.0),
			false => u8::from(b.y - a.y >= 0.0),
		};
		let mut start_consumed = 0.0;
		if edge.is_rounded
			&& segment > 0
			&& let Some((_, _, cut)) = rounded_corner(
				points[segment - 1],
				a,
				*points.get(segment + 1).unwrap_or(&a),
				ROUNDED_CORNER_RADIUS,
			) {
			start_consumed = cut;
		}
		let mut end_stop = length;
		let mut upcoming = None;
		if edge.is_rounded && segment + 2 < count {
			upcoming =
				rounded_corner(a, b, *points.get(segment + 2).unwrap_or(&b), ROUNDED_CORNER_RADIUS);
			if let Some((_, _, cut)) = upcoming {
				end_stop = length - cut;
			}
		}
		let mut on_segment: Vec<(f64, Point, f64, f64)> = jumps
			.iter()
			.filter(|jump| jump.segment == segment)
			.map(|jump| (jump.t, jump.point, jump.t * length, JUMP_RADIUS))
			.collect();
		super::super::common::v8_sort(&mut on_segment, |a, b| a.0 - b.0);
		let mut on_segment: Vec<(f64, Point, f64, f64)> = on_segment
			.into_iter()
			.filter_map(|(t, point, distance, radius)| {
				let room =
					(distance - start_consumed).min(end_stop - distance) - CORNER_JUMP_CLEARANCE;
				let radius = radius.min(room);
				(radius >= minimum_radius).then_some((t, point, distance, radius))
			})
			.collect();
		for k in 0..on_segment.len().saturating_sub(1) {
			let gap = on_segment[k + 1].2 - on_segment[k].2;
			if on_segment[k].3 + on_segment[k + 1].3 > gap {
				let half = gap / 2.0;
				on_segment[k].3 = on_segment[k].3.min(half);
				on_segment[k + 1].3 = on_segment[k + 1].3.min(half);
			}
		}
		for &(_, center, _, radius) in &on_segment {
			if radius < minimum_radius {
				continue;
			}
			let before = Point { x: center.x - ux * radius, y: center.y - uy * radius };
			let after = Point { x: center.x + ux * radius, y: center.y + uy * radius };
			parts.push(format!("L{}", point_text(before)));
			path.line_to(before.x, before.y);
			include(before);
			match use_gaps {
				false => {
					parts.push(format!(
						"A{},{} 0 0 {sweep} {}",
						format(radius),
						format(radius),
						point_text(after)
					));
					arcs_length += std::f64::consts::PI * radius;
					// Horizontal hops bulge upwards, vertical ones to the right.
					include(match (b.x - a.x).abs() >= (b.y - a.y).abs() {
						true => Point { x: center.x, y: center.y - radius },
						false => Point { x: center.x + radius, y: center.y },
					});
				}
				true => parts.push(format!("M{}", point_text(after))),
			}
			path.move_to(after.x, after.y);
			include(after);
		}
		match upcoming {
			Some((start, end, _)) => {
				parts.push(format!("L{},{}", format(start.x), format(start.y)));
				parts.push(format!(
					"Q{},{} {},{}",
					format(b.x),
					format(b.y),
					format(end.x),
					format(end.y)
				));
				path.line_to(start.x, start.y);
				path.quad_to(b.x, b.y, end.x, end.y);
				include(start);
				include(end);
			}
			None => {
				parts.push(format!("L{}", point_text(b)));
				path.line_to(b.x, b.y);
				include(b);
			}
		}
	}
	JumpPath { d: parts.join(" "), length: path.length() + arcs_length, bounds }
}
