//! The layout of @upsetjs/venn.js 2.0.0 (`circleintersection.js`, `layout.js`, `diagram.js`)
//! with the patched `fmin` it bundles (`nelderMead`, `bisect`, `conjugateGradient`).
//!
//! The optimizers are iterative, so the operations keep the order of the JavaScript code. The
//! C runtime's `atan2`, `acos`, `sin` and `cos` may round the last bit differently from Edge;
//! such float rounding differences are accepted. Rarely they flip a tie (the order of two arcs,
//! a cluster's orientation) and give a slightly different drawing.
//! venn.js keeps circles in objects keyed by set id, whose keys JavaScript orders with
//! array-index keys first: [`JsKeys`] reproduces that order.

use std::f64::consts::PI;

use super::super::layout::graph::array_index;

const SMALL: f64 = 1e-10;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Circle {
	pub x: f64,
	pub y: f64,
	pub radius: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
	pub x: f64,
	pub y: f64,
}

/// An area of the diagram: the sets (sorted) and its size.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
	pub sets: Vec<String>,
	pub size: f64,
}

/// The keys of a JavaScript object in iteration order: array indices ascending, then the
/// others in insertion order.
#[derive(Clone, Debug, Default)]
pub struct JsKeys {
	pub keys: Vec<String>,
}

impl JsKeys {
	/// The position of `key`, inserting it where JavaScript would.
	pub fn insert(&mut self, key: &str) -> usize {
		if let Some(position) = self.position(key) {
			return position;
		}
		let position = match array_index(key) {
			Some(index) => self
				.keys
				.iter()
				.position(|other| array_index(other).is_none_or(|other| other > index))
				.unwrap_or(self.keys.len()),
			None => self.keys.len(),
		};
		self.keys.insert(position, key.to_owned());
		position
	}

	pub fn position(&self, key: &str) -> Option<usize> {
		self.keys.iter().position(|other| other == key)
	}
}

/// JavaScript's `a - b` as a comparison for the stable `Array.prototype.sort`; `NaN` is equal.
fn compare(difference: f64) -> std::cmp::Ordering {
	difference.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
}

fn stable_sort_by<T>(items: &mut [T], compare_items: impl Fn(&T, &T) -> std::cmp::Ordering) {
	// Insertion sort: stable, and it never panics on inconsistent comparisons.
	for index in 1..items.len() {
		let mut position = index;
		while position > 0
			&& compare_items(&items[position - 1], &items[position]) == std::cmp::Ordering::Greater
		{
			items.swap(position - 1, position);
			position -= 1;
		}
	}
}

pub fn distance(a: Point, b: Point) -> f64 {
	((a.x - b.x) * (a.x - b.x) + (a.y - b.y) * (a.y - b.y)).sqrt()
}

fn center_of(circle: &Circle) -> Point {
	Point { x: circle.x, y: circle.y }
}

/// `circleArea`: the area of a circle segment of `width`.
fn circle_area(r: f64, width: f64) -> f64 {
	r * r * (1.0 - width / r).acos() - (r - width) * (width * (2.0 * r - width)).sqrt()
}

/// `circleOverlap`
pub fn circle_overlap(r1: f64, r2: f64, d: f64) -> f64 {
	if d >= r1 + r2 {
		return 0.0;
	}
	if d <= (r1 - r2).abs() {
		return PI * r1.min(r2) * r1.min(r2);
	}
	let w1 = r1 - (d * d - r2 * r2 + r1 * r1) / (2.0 * d);
	let w2 = r2 - (d * d - r1 * r1 + r2 * r2) / (2.0 * d);
	circle_area(r1, w1) + circle_area(r2, w2)
}

/// `circleCircleIntersection`
pub fn circle_circle_intersection(p1: &Circle, p2: &Circle) -> Vec<Point> {
	let d = distance(center_of(p1), center_of(p2));
	let (r1, r2) = (p1.radius, p2.radius);
	if d >= r1 + r2 || d <= (r1 - r2).abs() {
		return Vec::new();
	}
	let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
	let h = (r1 * r1 - a * a).sqrt();
	let x0 = p1.x + (a * (p2.x - p1.x)) / d;
	let y0 = p1.y + (a * (p2.y - p1.y)) / d;
	let rx = -(p2.y - p1.y) * (h / d);
	let ry = -(p2.x - p1.x) * (h / d);
	vec![Point { x: x0 + rx, y: y0 - ry }, Point { x: x0 - rx, y: y0 + ry }]
}

fn center(points: &[Point]) -> Point {
	let mut center = Point { x: 0.0, y: 0.0 };
	for point in points {
		center.x += point.x;
		center.y += point.y;
	}
	center.x /= points.len() as f64;
	center.y /= points.len() as f64;
	center
}

/// An arc of the outline of an intersection, with the index of its circle.
#[derive(Clone, Copy, Debug)]
pub struct Arc {
	pub circle: usize,
	pub width: f64,
	pub p1: Point,
	pub p2: Point,
	pub large: bool,
}

#[derive(Clone, Copy)]
struct InnerPoint {
	point: Point,
	parents: [usize; 2],
	angle: f64,
}

/// `intersectionArea`: the area where all `circles` overlap and the arcs of its outline.
pub fn intersection_area(circles: &[Circle]) -> (f64, Vec<Arc>) {
	let mut points = Vec::new();
	for i in 0..circles.len() {
		for j in i + 1..circles.len() {
			for point in circle_circle_intersection(&circles[i], &circles[j]) {
				points.push(InnerPoint { point, parents: [i, j], angle: 0.0 });
			}
		}
	}
	let mut inner: Vec<InnerPoint> = points
		.into_iter()
		.filter(|point| {
			circles
				.iter()
				.all(|circle| distance(point.point, center_of(circle)) < circle.radius + SMALL)
		})
		.collect();
	let (mut arc_area, mut polygon_area) = (0.0, 0.0);
	let mut arcs = Vec::new();
	if inner.len() > 1 {
		let middle = center(&inner.iter().map(|point| point.point).collect::<Vec<_>>());
		for point in &mut inner {
			point.angle = (point.point.x - middle.x).atan2(point.point.y - middle.y);
		}
		stable_sort_by(&mut inner, |a, b| compare(b.angle - a.angle));
		let mut p2 = inner[inner.len() - 1];
		for &p1 in &inner {
			polygon_area += (p2.point.x + p1.point.x) * (p1.point.y - p2.point.y);
			let mid =
				Point { x: (p1.point.x + p2.point.x) / 2.0, y: (p1.point.y + p2.point.y) / 2.0 };
			let mut arc: Option<Arc> = None;
			for &parent in &p1.parents {
				if !p2.parents.contains(&parent) {
					continue;
				}
				let circle = &circles[parent];
				let a1 = (p1.point.x - circle.x).atan2(p1.point.y - circle.y);
				let a2 = (p2.point.x - circle.x).atan2(p2.point.y - circle.y);
				let mut angle_difference = a2 - a1;
				if angle_difference < 0.0 {
					angle_difference += 2.0 * PI;
				}
				let a = a2 - angle_difference / 2.0;
				let mut width = distance(
					mid,
					Point {
						x: circle.x + circle.radius * a.sin(),
						y: circle.y + circle.radius * a.cos(),
					},
				);
				if width > circle.radius * 2.0 {
					width = circle.radius * 2.0;
				}
				if arc.is_none_or(|arc| arc.width > width) {
					arc = Some(Arc {
						circle: parent,
						width,
						p1: p1.point,
						p2: p2.point,
						large: width > circle.radius,
					});
				}
			}
			if let Some(arc) = arc {
				arc_area += circle_area(circles[arc.circle].radius, arc.width);
				arcs.push(arc);
				p2 = p1;
			}
		}
	} else if !circles.is_empty() {
		let mut smallest = 0;
		for index in 1..circles.len() {
			if circles[index].radius < circles[smallest].radius {
				smallest = index;
			}
		}
		let small = circles[smallest];
		let is_disjoint = circles.iter().any(|circle| {
			distance(center_of(circle), center_of(&small)) > (small.radius - circle.radius).abs()
		});
		if is_disjoint {
			arc_area = 0.0;
			polygon_area = 0.0;
		} else {
			arc_area = small.radius * small.radius * PI;
			arcs.push(Arc {
				circle: smallest,
				width: small.radius * 2.0,
				p1: Point { x: small.x, y: small.y + small.radius },
				p2: Point { x: small.x - SMALL, y: small.y + small.radius },
				large: true,
			});
		}
	}
	polygon_area /= 2.0;
	(arc_area + polygon_area, arcs)
}

/// `bisect`, `Err` where the ends have the same sign.
fn bisect(f: impl Fn(f64) -> f64, mut a: f64, b: f64) -> Result<f64, String> {
	let (f_a, f_b) = (f(a), f(b));
	let mut delta = b - a;
	if f_a * f_b > 0.0 {
		return Err("Initial bisect points must have opposite signs".to_owned());
	}
	if f_a == 0.0 {
		return Ok(a);
	}
	if f_b == 0.0 {
		return Ok(b);
	}
	for _ in 0..100 {
		delta /= 2.0;
		let mid = a + delta;
		let f_mid = f(mid);
		if f_mid * f_a >= 0.0 {
			a = mid;
		}
		if delta.abs() < 1e-10 || f_mid == 0.0 {
			return Ok(mid);
		}
	}
	Ok(a + delta)
}

/// `distanceFromIntersectArea`
fn distance_from_intersect_area(r1: f64, r2: f64, overlap: f64) -> Result<f64, String> {
	if r1.min(r2) * r1.min(r2) * PI <= overlap + SMALL {
		return Ok((r1 - r2).abs());
	}
	bisect(|distance| circle_overlap(r1, r2, distance) - overlap, 0.0, r1 + r2)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
	let mut sum = 0.0;
	for index in 0..a.len() {
		sum += a[index] * b[index];
	}
	sum
}

fn norm2(a: &[f64]) -> f64 {
	dot(a, a).sqrt()
}

fn weighted_sum(result: &mut [f64], w1: f64, v1: &[f64], w2: f64, v2: &[f64]) {
	for index in 0..result.len() {
		result[index] = w1 * v1[index] + w2 * v2[index];
	}
}

/// fmin's `nelderMead` as venn.js patches it: the tolerance of the simplex size is
/// `minErrorDelta` when given.
fn nelder_mead(
	mut f: impl FnMut(&[f64]) -> f64,
	x0: &[f64],
	max_iterations: usize,
	min_error_delta: Option<f64>,
) -> (Vec<f64>, f64) {
	let (rho, chi, psi, sigma) = (1.0, 2.0, -0.5, 0.5);
	let min_error = min_error_delta.unwrap_or(1e-6);
	let min_tolerance = min_error_delta.unwrap_or(1e-5);
	let n = x0.len();
	let mut simplex: Vec<(Vec<f64>, f64)> = Vec::with_capacity(n + 1);
	simplex.push((x0.to_vec(), f(x0)));
	for index in 0..n {
		let mut point = x0.to_vec();
		point[index] =
			if point[index] != 0.0 && !point[index].is_nan() { point[index] * 1.05 } else { 0.001 };
		let value = f(&point);
		simplex.push((point, value));
	}
	let mut centroid = x0.to_vec();
	let mut reflected = x0.to_vec();
	let mut contracted = x0.to_vec();
	let mut expanded = x0.to_vec();
	for _ in 0..max_iterations {
		stable_sort_by(&mut simplex, |a, b| compare(a.1 - b.1));
		let mut max_difference = 0f64;
		for index in 0..n {
			let difference = (simplex[0].0[index] - simplex[1].0[index]).abs();
			// `Math.max` is `NaN` when either is.
			max_difference = if difference.is_nan() || max_difference.is_nan() {
				f64::NAN
			} else {
				max_difference.max(difference)
			};
		}
		if (simplex[0].1 - simplex[n].1).abs() < min_error && max_difference < min_tolerance {
			break;
		}
		for (index, value) in centroid.iter_mut().enumerate() {
			*value = 0.0;
			for point in simplex.iter().take(n) {
				*value += point.0[index];
			}
			*value /= n as f64;
		}
		let worst = simplex[n].clone();
		weighted_sum(&mut reflected, 1.0 + rho, &centroid, -rho, &worst.0);
		let reflected_value = f(&reflected);
		let mut update: Option<(Vec<f64>, f64)> = None;
		if reflected_value < simplex[0].1 {
			weighted_sum(&mut expanded, 1.0 + chi, &centroid, -chi, &worst.0);
			let expanded_value = f(&expanded);
			update = Some(match expanded_value < reflected_value {
				true => (expanded.clone(), expanded_value),
				false => (reflected.clone(), reflected_value),
			});
		} else if reflected_value >= simplex[n - 1].1 {
			let mut should_reduce = false;
			if reflected_value > worst.1 {
				weighted_sum(&mut contracted, 1.0 + psi, &centroid, -psi, &worst.0);
				let contracted_value = f(&contracted);
				match contracted_value < worst.1 {
					true => update = Some((contracted.clone(), contracted_value)),
					false => should_reduce = true,
				}
			} else {
				weighted_sum(&mut contracted, 1.0 - psi * rho, &centroid, psi * rho, &worst.0);
				let contracted_value = f(&contracted);
				match contracted_value < reflected_value {
					true => update = Some((contracted.clone(), contracted_value)),
					false => should_reduce = true,
				}
			}
			if should_reduce {
				let best = simplex[0].0.clone();
				for point in simplex.iter_mut().skip(1) {
					let current = point.0.clone();
					weighted_sum(&mut point.0, 1.0 - sigma, &best, sigma, &current);
					point.1 = f(&point.0);
				}
			}
		} else {
			update = Some((reflected.clone(), reflected_value));
		}
		if let Some((values, value)) = update {
			simplex[n].0.copy_from_slice(&values);
			simplex[n].1 = value;
		}
	}
	stable_sort_by(&mut simplex, |a, b| compare(a.1 - b.1));
	let (x, fx) = simplex.swap_remove(0);
	(x, fx)
}

struct Point2 {
	x: Vec<f64>,
	fx: f64,
	fxprime: Vec<f64>,
}

/// The `zoom` of `wolfeLineSearch`.
#[allow(clippy::too_many_arguments)]
fn zoom(
	f: &mut impl FnMut(&[f64], &mut [f64]) -> f64,
	pk: &[f64],
	current: &Point2,
	next: &mut Point2,
	(phi0, phi_prime0): (f64, f64),
	mut a_lo: f64,
	mut a_high: f64,
	mut phi_lo: f64,
) -> f64 {
	let (c1, c2) = (1e-6, 0.1);
	for _ in 0..16 {
		let a = (a_lo + a_high) / 2.0;
		weighted_sum(&mut next.x, 1.0, &current.x, a, pk);
		let phi = f(&next.x, &mut next.fxprime);
		next.fx = phi;
		let phi_prime = dot(&next.fxprime, pk);
		if phi > phi0 + c1 * a * phi_prime0 || phi >= phi_lo {
			a_high = a;
		} else {
			if phi_prime.abs() <= -c2 * phi_prime0 {
				return a;
			}
			if phi_prime * (a_high - a_lo) >= 0.0 {
				a_high = a_lo;
			}
			a_lo = a;
			phi_lo = phi;
		}
	}
	0.0
}

/// fmin's `wolfeLineSearch`.
fn wolfe_line_search(
	f: &mut impl FnMut(&[f64], &mut [f64]) -> f64,
	pk: &[f64],
	current: &Point2,
	next: &mut Point2,
	a_start: f64,
) -> f64 {
	let (c1, c2) = (1e-6, 0.1);
	let phi0 = current.fx;
	let phi_prime0 = dot(&current.fxprime, pk);
	let mut phi_old = phi0;
	let mut a0 = 0.0;
	let mut a = if a_start != 0.0 && !a_start.is_nan() { a_start } else { 1.0 };
	for iteration in 0..10 {
		weighted_sum(&mut next.x, 1.0, &current.x, a, pk);
		let phi = f(&next.x, &mut next.fxprime);
		next.fx = phi;
		let phi_prime = dot(&next.fxprime, pk);
		if phi > phi0 + c1 * a * phi_prime0 || (iteration > 0 && phi >= phi_old) {
			return zoom(f, pk, current, next, (phi0, phi_prime0), a0, a, phi_old);
		}
		if phi_prime.abs() <= -c2 * phi_prime0 {
			return a;
		}
		if phi_prime >= 0.0 {
			return zoom(f, pk, current, next, (phi0, phi_prime0), a, a0, phi);
		}
		phi_old = phi;
		a0 = a;
		a *= 2.0;
	}
	a
}

/// fmin's `conjugateGradient`.
fn conjugate_gradient(
	mut f: impl FnMut(&[f64], &mut [f64]) -> f64,
	initial: &[f64],
	max_iterations: usize,
) -> (Vec<f64>, f64) {
	let mut current = Point2 { x: initial.to_vec(), fx: 0.0, fxprime: initial.to_vec() };
	let mut next = Point2 { x: initial.to_vec(), fx: 0.0, fxprime: initial.to_vec() };
	let mut yk = initial.to_vec();
	current.fx = f(&current.x, &mut current.fxprime);
	let mut pk: Vec<f64> = current.fxprime.iter().map(|value| value * -1.0).collect();
	let mut a = 1.0;
	for _ in 0..max_iterations {
		a = wolfe_line_search(&mut f, &pk, &current, &mut next, a);
		if a == 0.0 || a.is_nan() {
			for (direction, gradient) in pk.iter_mut().zip(&current.fxprime) {
				*direction = gradient * -1.0;
			}
		} else {
			weighted_sum(&mut yk, 1.0, &next.fxprime, -1.0, &current.fxprime);
			let delta_k = dot(&current.fxprime, &current.fxprime);
			let ratio = dot(&yk, &next.fxprime) / delta_k;
			let beta_k = if ratio.is_nan() { f64::NAN } else { 0f64.max(ratio) };
			let previous = pk.clone();
			weighted_sum(&mut pk, beta_k, &previous, -1.0, &next.fxprime);
			std::mem::swap(&mut current, &mut next);
		}
		if norm2(&current.fxprime) <= 1e-5 {
			break;
		}
	}
	(current.x, current.fx)
}

/// The circles of the sets with their ids in JavaScript key order.
#[derive(Clone, Debug, Default)]
pub struct Solution {
	pub ids: JsKeys,
	pub circles: Vec<Circle>,
}

impl Solution {
	pub fn get(&self, id: &str) -> Option<&Circle> {
		self.ids.position(id).map(|index| &self.circles[index])
	}

	fn set(&mut self, id: &str, circle: Circle) {
		match self.ids.position(id) {
			Some(index) => self.circles[index] = circle,
			None => {
				let index = self.ids.insert(id);
				self.circles.insert(index, circle);
			}
		}
	}
}

/// An area with the indices of its sets in a [`Solution`].
struct IndexedArea {
	sets: Vec<usize>,
	size: f64,
	weight: f64,
}

fn index_areas(areas: &[Area], solution: &Solution) -> Vec<IndexedArea> {
	areas
		.iter()
		.filter(|area| area.sets.len() > 1)
		.map(|area| IndexedArea {
			sets: area
				.sets
				.iter()
				.map(|set| solution.ids.position(set).unwrap_or(usize::MAX))
				.collect(),
			size: area.size,
			weight: 1.0,
		})
		.collect()
}

/// `lossFunction`, with the weights of `greedyLayout` where given.
fn loss(circles: &[Circle], areas: &[IndexedArea]) -> f64 {
	let mut output = 0.0;
	for area in areas {
		let Some(members) =
			area.sets.iter().map(|&set| circles.get(set).copied()).collect::<Option<Vec<_>>>()
		else {
			continue;
		};
		let overlap = match members.as_slice() {
			[left, right] => circle_overlap(
				left.radius,
				right.radius,
				distance(center_of(left), center_of(right)),
			),
			_ => intersection_area(&members).0,
		};
		output += area.weight * (overlap - area.size) * (overlap - area.size);
	}
	output
}

/// `addMissingAreas`: pairs of sets without an area get one of size 0.
fn add_missing_areas(areas: &[Area]) -> Vec<Area> {
	let mut result = areas.to_vec();
	let mut ids: Vec<String> = Vec::new();
	let mut pairs = std::collections::HashSet::new();
	for area in areas {
		match area.sets.as_slice() {
			[id] => ids.push(id.clone()),
			[a, b] => {
				pairs.insert(format!("{a};{b}"));
				pairs.insert(format!("{b};{a}"));
			}
			_ => {}
		}
	}
	npp::sort::sort_by(&mut ids[..], |a, b| a.encode_utf16().cmp(b.encode_utf16()));
	for i in 0..ids.len() {
		for j in i + 1..ids.len() {
			if !pairs.contains(&format!("{};{}", ids[i], ids[j])) {
				result.push(Area { sets: vec![ids[i].clone(), ids[j].clone()], size: 0.0 });
			}
		}
	}
	result
}

/// `greedyLayout`
fn greedy_layout(areas: &[Area]) -> Result<Solution, String> {
	let mut solution = Solution::default();
	let mut sizes: Vec<f64> = Vec::new();
	for area in areas.iter().filter(|area| area.sets.len() == 1) {
		let circle = Circle { x: 1e10, y: 1e10, radius: (area.size / PI).sqrt() };
		let position = solution.ids.position(&area.sets[0]);
		solution.set(&area.sets[0], circle);
		let index = solution.ids.position(&area.sets[0]).unwrap_or(0);
		match position {
			Some(_) => sizes[index] = area.size,
			None => sizes.insert(index, area.size),
		}
	}
	let count = solution.circles.len();
	// The overlaps of each set: the other set, the size and the weight.
	let mut overlaps: Vec<Vec<(usize, f64, f64)>> = vec![Vec::new(); count];
	let mut pair_areas = Vec::new();
	for area in areas.iter().filter(|area| area.sets.len() == 2) {
		let (Some(left), Some(right)) =
			(solution.ids.position(&area.sets[0]), solution.ids.position(&area.sets[1]))
		else {
			return Err("Cannot read properties of undefined (reading 'size')".to_owned());
		};
		let weight = if area.size + SMALL >= sizes[left].min(sizes[right]) { 0.0 } else { 1.0 };
		overlaps[left].push((right, area.size, weight));
		overlaps[right].push((left, area.size, weight));
		pair_areas.push(IndexedArea { sets: vec![left, right], size: area.size, weight: 1.0 });
	}
	let mut most_overlapped: Vec<(usize, f64)> = (0..count)
		.map(|set| {
			let mut size = 0.0;
			for &(_, overlap, weight) in &overlaps[set] {
				size += overlap * weight;
			}
			(set, size)
		})
		.collect();
	stable_sort_by(&mut most_overlapped, |a, b| compare(b.1 - a.1));
	let Some(&(first, _)) = most_overlapped.first() else {
		return Err("Cannot read properties of undefined (reading 'set')".to_owned());
	};
	let mut positioned = vec![false; count];
	solution.circles[first].x = 0.0;
	solution.circles[first].y = 0.0;
	positioned[first] = true;
	for &(set, _) in most_overlapped.iter().skip(1) {
		let mut overlap: Vec<(usize, f64, f64)> =
			overlaps[set].iter().copied().filter(|&(other, _, _)| positioned[other]).collect();
		stable_sort_by(&mut overlap, |a, b| compare(b.1 - a.1));
		if overlap.is_empty() {
			return Err("ERROR: missing pairwise overlap information".to_owned());
		}
		let radius = solution.circles[set].radius;
		let mut points = Vec::new();
		for j in 0..overlap.len() {
			let p1 = solution.circles[overlap[j].0];
			let d1 = distance_from_intersect_area(radius, p1.radius, overlap[j].1)?;
			points.push(Point { x: p1.x + d1, y: p1.y });
			points.push(Point { x: p1.x - d1, y: p1.y });
			points.push(Point { x: p1.x, y: p1.y + d1 });
			points.push(Point { x: p1.x, y: p1.y - d1 });
			for &(other, size, _) in &overlap[j + 1..] {
				let p2 = solution.circles[other];
				let d2 = distance_from_intersect_area(radius, p2.radius, size)?;
				points.extend(circle_circle_intersection(
					&Circle { x: p1.x, y: p1.y, radius: d1 },
					&Circle { x: p2.x, y: p2.y, radius: d2 },
				));
			}
		}
		let mut best_loss = 1e50;
		let mut best_point = points[0];
		for &point in &points {
			solution.circles[set].x = point.x;
			solution.circles[set].y = point.y;
			let local_loss = loss(&solution.circles, &pair_areas);
			if local_loss < best_loss {
				best_loss = local_loss;
				best_point = point;
			}
		}
		solution.circles[set].x = best_point.x;
		solution.circles[set].y = best_point.y;
		positioned[set] = true;
	}
	Ok(solution)
}

/// `constrainedMDSLayout`, with `random` for `Math.random`.
fn constrained_mds_layout(
	areas: &[Area],
	random: &mut dyn FnMut() -> f64,
	max_iterations: usize,
) -> Result<Solution, String> {
	let sets: Vec<&Area> = areas.iter().filter(|area| area.sets.len() == 1).collect();
	let mut set_ids = JsKeys::default();
	let mut set_index: Vec<(String, usize)> = Vec::new();
	for (index, set) in sets.iter().enumerate() {
		set_ids.insert(&set.sets[0]);
		match set_index.iter_mut().find(|(id, _)| *id == set.sets[0]) {
			Some(entry) => entry.1 = index,
			None => set_index.push((set.sets[0].clone(), index)),
		}
	}
	let index_of = |id: &str| set_index.iter().find(|(other, _)| other == id).map(|entry| entry.1);
	let n = sets.len();
	let mut distances = vec![vec![0.0; n]; n];
	let mut constraints = vec![vec![0.0; n]; n];
	for area in areas.iter().filter(|area| area.sets.len() == 2) {
		let (Some(left), Some(right)) = (index_of(&area.sets[0]), index_of(&area.sets[1])) else {
			continue;
		};
		let r1 = (sets[left].size / PI).sqrt();
		let r2 = (sets[right].size / PI).sqrt();
		let distance = distance_from_intersect_area(r1, r2, area.size)?;
		distances[left][right] = distance;
		distances[right][left] = distance;
		let constraint = if area.size + 1e-10 >= sets[left].size.min(sets[right].size) {
			1.0
		} else if area.size <= 1e-10 {
			-1.0
		} else {
			0.0
		};
		constraints[left][right] = constraint;
		constraints[right][left] = constraint;
	}
	let norms: Vec<f64> = distances.iter().map(|row| norm2(row)).collect();
	let norm = norm2(&norms) / n as f64;
	for row in &mut distances {
		for value in row.iter_mut() {
			*value /= norm;
		}
	}
	let objective = |x: &[f64], fxprime: &mut [f64]| -> f64 {
		for value in fxprime.iter_mut() {
			*value = 0.0;
		}
		let mut loss = 0.0;
		for i in 0..n {
			let (xi, yi) = (x[2 * i], x[2 * i + 1]);
			for j in i + 1..n {
				let (xj, yj) = (x[2 * j], x[2 * j + 1]);
				let dij = distances[i][j];
				let constraint = constraints[i][j];
				let squared = (xj - xi) * (xj - xi) + (yj - yi) * (yj - yi);
				let distance = squared.sqrt();
				let delta = squared - dij * dij;
				if (constraint > 0.0 && distance <= dij) || (constraint < 0.0 && distance >= dij) {
					continue;
				}
				loss += 2.0 * delta * delta;
				fxprime[2 * i] += 4.0 * delta * (xi - xj);
				fxprime[2 * i + 1] += 4.0 * delta * (yi - yj);
				fxprime[2 * j] += 4.0 * delta * (xj - xi);
				fxprime[2 * j + 1] += 4.0 * delta * (yj - yi);
			}
		}
		loss
	};
	let mut best: Option<(Vec<f64>, f64)> = None;
	for _ in 0..10 {
		let initial: Vec<f64> = (0..n * 2).map(|_| random()).collect();
		let current = conjugate_gradient(objective, &initial, max_iterations);
		if best.as_ref().is_none_or(|best| current.1 < best.1) {
			best = Some(current);
		}
	}
	let positions = best.map(|best| best.0).unwrap_or_default();
	let mut solution = Solution::default();
	for (index, set) in sets.iter().enumerate() {
		let circle = Circle {
			x: positions.get(2 * index).copied().unwrap_or(0.0) * norm,
			y: positions.get(2 * index + 1).copied().unwrap_or(0.0) * norm,
			radius: (set.size / PI).sqrt(),
		};
		solution.set(&set.sets[0], circle);
	}
	Ok(solution)
}

/// `venn` with `bestInitialLayout`: the circles of the sets.
pub fn venn(areas: &[Area], random: &mut dyn FnMut() -> f64) -> Result<Solution, String> {
	let max_iterations = 500;
	let areas = add_missing_areas(areas);
	let mut solution = greedy_layout(&areas)?;
	if areas.len() >= 8 {
		let constrained = constrained_mds_layout(&areas, random, max_iterations)?;
		let constrained_loss = loss(&constrained.circles, &index_areas(&areas, &constrained));
		let greedy_loss = loss(&solution.circles, &index_areas(&areas, &solution));
		if constrained_loss + 1e-8 < greedy_loss {
			solution = constrained;
		}
	}
	let indexed = index_areas(&areas, &solution);
	let mut initial = Vec::with_capacity(solution.circles.len() * 2);
	for circle in &solution.circles {
		initial.push(circle.x);
		initial.push(circle.y);
	}
	let mut current = solution.circles.clone();
	let (positions, _) = nelder_mead(
		|values| {
			for (index, circle) in current.iter_mut().enumerate() {
				circle.x = values[2 * index];
				circle.y = values[2 * index + 1];
			}
			loss(&current, &indexed)
		},
		&initial,
		max_iterations,
		None,
	);
	for (index, circle) in solution.circles.iter_mut().enumerate() {
		circle.x = positions[2 * index];
		circle.y = positions[2 * index + 1];
	}
	Ok(solution)
}

/// A circle with its set id, as `fromObjectNotation` gives them.
#[derive(Clone, Debug)]
struct Placed {
	id: String,
	circle: Circle,
}

fn bounds(circles: &[Placed]) -> [f64; 4] {
	let mut extent = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
	for placed in circles {
		let c = placed.circle;
		// `Math.max`/`Math.min` with `NaN` are `NaN`.
		let max = |a: f64, b: f64| if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) };
		let min = |a: f64, b: f64| if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) };
		extent[0] = min(extent[0], c.x - c.radius);
		extent[1] = max(extent[1], c.x + c.radius);
		extent[2] = min(extent[2], c.y - c.radius);
		extent[3] = max(extent[3], c.y + c.radius);
	}
	extent
}

fn to_placed(solution: &Solution) -> Vec<Placed> {
	solution
		.ids
		.keys
		.iter()
		.zip(&solution.circles)
		.map(|(id, circle)| Placed { id: id.clone(), circle: *circle })
		.collect()
}

fn to_solution(circles: Vec<Placed>) -> Solution {
	let mut solution = Solution::default();
	for placed in circles {
		solution.set(&placed.id, placed.circle);
	}
	solution
}

/// `orientateCircles` without an `orientationOrder`.
fn orientate(circles: &mut [Placed], orientation: f64) {
	stable_sort_by(circles, |a, b| compare(b.circle.radius - a.circle.radius));
	if let Some(first) = circles.first().map(|placed| placed.circle) {
		for placed in circles.iter_mut() {
			placed.circle.x -= first.x;
			placed.circle.y -= first.y;
		}
	}
	if circles.len() == 2 {
		let (first, second) = (circles[0].circle, circles[1].circle);
		if distance(center_of(&first), center_of(&second)) < (second.radius - first.radius).abs() {
			circles[1].circle.x = first.x + first.radius - second.radius - 1e-10;
			circles[1].circle.y = first.y;
		}
	}
	if circles.len() > 1 {
		let rotation = circles[1].circle.x.atan2(circles[1].circle.y) - orientation;
		let (c, s) = (rotation.cos(), rotation.sin());
		for placed in circles.iter_mut() {
			let (x, y) = (placed.circle.x, placed.circle.y);
			placed.circle.x = c * x - s * y;
			placed.circle.y = s * x + c * y;
		}
	}
	if circles.len() > 2 {
		let mut angle = circles[2].circle.x.atan2(circles[2].circle.y) - orientation;
		// Like JavaScript's loops, but bounded: a huge angle would loop for long.
		for _ in 0..64 {
			match angle {
				_ if angle < 0.0 => angle += 2.0 * PI,
				_ if angle > 2.0 * PI => angle -= 2.0 * PI,
				_ => break,
			}
		}
		if angle.is_finite() && !(0.0..=2.0 * PI).contains(&angle) {
			angle = angle.rem_euclid(2.0 * PI);
		}
		if angle > PI {
			let slope = circles[1].circle.y / (1e-10 + circles[1].circle.x);
			for placed in circles.iter_mut() {
				let d = (placed.circle.x + slope * placed.circle.y) / (1.0 + slope * slope);
				placed.circle.x = 2.0 * d - placed.circle.x;
				placed.circle.y = 2.0 * d * slope - placed.circle.y;
			}
		}
	}
}

/// `disjointCluster`: groups of circles that overlap, by the id of their root.
fn disjoint_clusters(circles: &[Placed]) -> Vec<Vec<Placed>> {
	let mut parents: Vec<usize> = (0..circles.len()).collect();
	fn find(parents: &mut [usize], mut index: usize) -> usize {
		let mut root = index;
		while parents[root] != root {
			root = parents[root];
		}
		while parents[index] != root {
			let next = parents[index];
			parents[index] = root;
			index = next;
		}
		root
	}
	for i in 0..circles.len() {
		for j in i + 1..circles.len() {
			let max_distance = circles[i].circle.radius + circles[j].circle.radius;
			if distance(center_of(&circles[i].circle), center_of(&circles[j].circle)) + 1e-10
				< max_distance
			{
				let (x_root, y_root) = (find(&mut parents, j), find(&mut parents, i));
				parents[x_root] = y_root;
			}
		}
	}
	let mut clusters: Vec<(String, Vec<Placed>)> = Vec::new();
	for index in 0..circles.len() {
		let root = find(&mut parents, index);
		let id = circles[root].id.clone();
		match clusters.iter_mut().find(|(other, _)| *other == id) {
			Some(cluster) => cluster.1.push(circles[index].clone()),
			None => clusters.push((id, vec![circles[index].clone()])),
		}
	}
	clusters.into_iter().map(|(_, cluster)| cluster).collect()
}

/// `normalizeSolution` with the default orientation.
pub fn normalize(solution: &Solution) -> Solution {
	let orientation = PI / 2.0;
	let circles = to_placed(solution);
	let mut clusters: Vec<(Vec<Placed>, f64, [f64; 4])> = disjoint_clusters(&circles)
		.into_iter()
		.map(|mut cluster| {
			orientate(&mut cluster, orientation);
			let bounds = bounds(&cluster);
			let size = (bounds[1] - bounds[0]) * (bounds[3] - bounds[2]);
			(cluster, size, bounds)
		})
		.collect();
	stable_sort_by(&mut clusters, |a, b| compare(b.1 - a.1));
	let mut iterator = clusters.into_iter();
	let Some((mut circles, _, mut return_bounds)) = iterator.next() else {
		return Solution::default();
	};
	let rest: Vec<(Vec<Placed>, f64, [f64; 4])> = iterator.collect();
	let spacing = (return_bounds[1] - return_bounds[0]) / 50.0;
	let add_cluster = |circles: &mut Vec<Placed>,
	                   return_bounds: &[f64; 4],
	                   cluster: Option<&(Vec<Placed>, f64, [f64; 4])>,
	                   right: bool,
	                   bottom: bool| {
		let Some((cluster, _, bounds)) = cluster else { return };
		let x_offset = match right {
			true => return_bounds[1] - bounds[0] + spacing,
			false => {
				let centering =
					(bounds[1] - bounds[0]) / 2.0 - (return_bounds[1] - return_bounds[0]) / 2.0;
				return_bounds[1] - bounds[1] + if centering < 0.0 { centering } else { 0.0 }
			}
		};
		let y_offset = match bottom {
			true => return_bounds[3] - bounds[2] + spacing,
			false => {
				let centering =
					(bounds[3] - bounds[2]) / 2.0 - (return_bounds[3] - return_bounds[2]) / 2.0;
				return_bounds[3] - bounds[3] + if centering < 0.0 { centering } else { 0.0 }
			}
		};
		for placed in cluster {
			let mut moved = placed.clone();
			moved.circle.x += x_offset;
			moved.circle.y += y_offset;
			circles.push(moved);
		}
	};
	let mut index = 0;
	while index < rest.len() {
		add_cluster(&mut circles, &return_bounds, rest.get(index), true, false);
		add_cluster(&mut circles, &return_bounds, rest.get(index + 1), false, true);
		add_cluster(&mut circles, &return_bounds, rest.get(index + 2), true, true);
		index += 3;
		return_bounds = bounds(&circles);
	}
	to_solution(circles)
}

/// `scaleSolution` without `scaleToFit`.
pub fn scale(solution: &Solution, width: f64, height: f64, padding: f64) -> Solution {
	let circles = to_placed(solution);
	let (width, height) = (width - 2.0 * padding, height - 2.0 * padding);
	let [x_min, x_max, y_min, y_max] = bounds(&circles);
	if x_max == x_min || y_max == y_min {
		return solution.clone();
	}
	let x_scaling = width / (x_max - x_min);
	let y_scaling = height / (y_max - y_min);
	let scaling =
		if x_scaling.is_nan() || y_scaling.is_nan() { f64::NAN } else { y_scaling.min(x_scaling) };
	let x_offset = (width - (x_max - x_min) * scaling) / 2.0;
	let y_offset = (height - (y_max - y_min) * scaling) / 2.0;
	to_solution(
		circles
			.into_iter()
			.map(|placed| Placed {
				circle: Circle {
					radius: scaling * placed.circle.radius,
					x: padding + x_offset + (placed.circle.x - x_min) * scaling,
					y: padding + y_offset + (placed.circle.y - y_min) * scaling,
				},
				id: placed.id,
			})
			.collect(),
	)
}

/// `circleMargin`
fn circle_margin(current: Point, interior: &[Circle], exterior: &[Circle]) -> f64 {
	let mut margin = interior[0].radius - distance(center_of(&interior[0]), current);
	for circle in &interior[1..] {
		let m = circle.radius - distance(center_of(circle), current);
		if m <= margin {
			margin = m;
		}
	}
	for circle in exterior {
		let m = distance(center_of(circle), current) - circle.radius;
		if m <= margin {
			margin = m;
		}
	}
	margin
}

/// `computeTextCentre` without `symmetricalTextCentre`: `(0, -1000)` for a disjoint area,
/// `None` without circles.
fn text_centre(interior: &[Circle], exterior: &[Circle], depth: usize) -> Option<Point> {
	if interior.is_empty() {
		return None;
	}
	let mut points = Vec::new();
	for c in interior {
		points.push(Point { x: c.x, y: c.y });
		points.push(Point { x: c.x + c.radius / 2.0, y: c.y });
		points.push(Point { x: c.x - c.radius / 2.0, y: c.y });
		points.push(Point { x: c.x, y: c.y + c.radius / 2.0 });
		points.push(Point { x: c.x, y: c.y - c.radius / 2.0 });
	}
	let mut initial = points[0];
	let mut margin = circle_margin(points[0], interior, exterior);
	for &point in &points[1..] {
		let m = circle_margin(point, interior, exterior);
		if m >= margin {
			initial = point;
			margin = m;
		}
	}
	let (solution, _) = nelder_mead(
		|p| -circle_margin(Point { x: p[0], y: p[1] }, interior, exterior),
		&[initial.x, initial.y],
		500,
		Some(1e-10),
	);
	let ret = Point { x: solution[0], y: solution[1] };
	let is_valid = interior.iter().all(|circle| distance(ret, center_of(circle)) <= circle.radius)
		&& exterior.iter().all(|circle| distance(ret, center_of(circle)) >= circle.radius);
	if is_valid {
		return Some(ret);
	}
	if interior.len() == 1 {
		return Some(center_of(&interior[0]));
	}
	let (_, arcs) = intersection_area(interior);
	match arcs.as_slice() {
		[] => Some(Point { x: 0.0, y: -1000.0 }),
		[arc] => Some(center_of(&interior[arc.circle])),
		_ if !exterior.is_empty() && depth == 0 => text_centre(interior, &[], depth + 1),
		_ => Some(center(&arcs.iter().map(|arc| arc.p1).collect::<Vec<_>>())),
	}
}

/// `computeTextCentres`: the label position of each area.
pub fn text_centres(solution: &Solution, areas: &[Area]) -> Vec<Option<Point>> {
	let ids = &solution.ids.keys;
	let circles = &solution.circles;
	// `getOverlappingCircles`: the circles each circle lies in.
	let mut overlapped: Vec<Vec<usize>> = vec![Vec::new(); circles.len()];
	for i in 0..circles.len() {
		for j in i + 1..circles.len() {
			let (a, b) = (circles[i], circles[j]);
			let d = distance(center_of(&a), center_of(&b));
			if d + b.radius <= a.radius + 1e-10 {
				overlapped[j].push(i);
			} else if d + a.radius <= b.radius + 1e-10 {
				overlapped[i].push(j);
			}
		}
	}
	areas
		.iter()
		.map(|area| {
			let members: Vec<usize> = ids
				.iter()
				.enumerate()
				.filter(|(_, id)| area.sets.contains(id))
				.map(|(index, _)| index)
				.collect();
			let mut excluded = vec![false; circles.len()];
			for set in &area.sets {
				if let Some(index) = ids.iter().position(|id| id == set) {
					for &other in &overlapped[index] {
						excluded[other] = true;
					}
				}
			}
			let interior: Vec<Circle> = members.iter().map(|&index| circles[index]).collect();
			let exterior: Vec<Circle> = (0..circles.len())
				.filter(|index| !members.contains(index) && !excluded[*index])
				.map(|index| circles[index])
				.collect();
			text_centre(&interior, &exterior, 0)
		})
		.collect()
}

/// `intersectionAreaPath` (`arcsToPath`), rounded to `round` decimals where given.
pub fn area_path(circles: &[Circle], round: Option<i32>) -> String {
	if circles.is_empty() {
		return "M 0 0".to_owned();
	}
	let (_, arcs) = intersection_area(circles);
	let r = |value: f64| match round {
		Some(digits) => {
			let factor = 10f64.powi(digits);
			super::super::color::js_round(value * factor) / factor
		}
		None => value,
	};
	let number = |value: f64| super::super::color::js_number(r(value));
	match arcs.as_slice() {
		[] => "M 0 0".to_owned(),
		[arc] => {
			let circle = circles[arc.circle];
			let (x, y, radius) = (r(circle.x), r(circle.y), r(circle.radius));
			let js = super::super::color::js_number;
			format!(
				"\nM {} {} \nm {} 0 \na {} {} 0 1 0 {} 0 \na {} {} 0 1 0 {} 0",
				js(x),
				js(y),
				js(-radius),
				js(radius),
				js(radius),
				js(radius * 2.0),
				js(radius),
				js(radius),
				js(-radius * 2.0)
			)
		}
		_ => {
			let mut path = format!("\nM {} {}", number(arcs[0].p2.x), number(arcs[0].p2.y));
			for arc in &arcs {
				let radius = number(circles[arc.circle].radius);
				path.push_str(&format!(
					" \nA {radius} {radius} 0 {} 1 {} {}",
					u8::from(arc.large),
					number(arc.p1.x),
					number(arc.p1.y)
				));
			}
			path
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mermaid::value::{Value, parse_json};

	fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
		let mut state = seed;
		move || {
			state = state.wrapping_add(0x6D2B_79F5);
			let mut t = (state ^ (state >> 15)).wrapping_mul(1 | state);
			t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
			f64::from(t ^ (t >> 14)) / 4_294_967_296.0
		}
	}

	/// The circles, text centre and path of an area.
	type LaidArea = (Vec<Circle>, Option<Point>, String);

	/// `venn.layout`: the circles, text centres and paths of each area.
	fn layout(
		areas: &[Area],
		width: f64,
		height: f64,
		padding: f64,
		seed: u32,
	) -> Result<Vec<LaidArea>, String> {
		let mut random = mulberry32(seed);
		let solution = venn(areas, &mut random)?;
		let solution = scale(&normalize(&solution), width, height, padding);
		let centres = text_centres(&solution, areas);
		Ok(areas
			.iter()
			.zip(centres)
			.map(|(area, centre)| {
				let circles: Vec<Circle> =
					area.sets.iter().filter_map(|set| solution.get(set).copied()).collect();
				let path = area_path(&circles, Some(2));
				(circles, centre, path)
			})
			.collect())
	}

	fn check_fixtures(text: &str) -> (usize, usize) {
		let (mut passed, mut failed) = (0, 0);
		for line in text.lines().filter(|line| !line.trim().is_empty()) {
			let fixture = parse_json(line).unwrap();
			let number = |value: &Value| value.as_number().unwrap_or(f64::NAN);
			let areas: Vec<Area> = match fixture.get("areas") {
				Some(Value::Array(areas)) => areas
					.iter()
					.map(|area| Area {
						sets: match area.get("sets") {
							Some(Value::Array(sets)) => sets
								.iter()
								.filter_map(|set| set.as_str().map(str::to_owned))
								.collect(),
							_ => Vec::new(),
						},
						size: area.get("size").map_or(f64::NAN, number),
					})
					.collect(),
				_ => Vec::new(),
			};
			let get = |key: &str| fixture.get(key).map_or(f64::NAN, number);
			let actual = layout(&areas, get("w"), get("h"), get("p"), get("seed") as u32);
			let ok = match (fixture.get("result"), &actual) {
				(Some(Value::Array(expected)), Ok(actual)) => {
					expected.iter().zip(actual).all(|(expected, (circles, centre, path))| {
						let close = |a: f64, b: f64| (a - b).abs() <= 1e-6 * (1.0 + b.abs());
						let circles_ok = match expected.get("circles") {
							Some(Value::Array(list)) => {
								list.iter().zip(circles).all(|(item, circle)| match item {
									Value::Array(values) => {
										close(circle.x, number(&values[1]))
											&& close(circle.y, number(&values[2]))
											&& close(circle.radius, number(&values[3]))
									}
									_ => false,
								})
							}
							_ => false,
						};
						let centre_ok = match (expected.get("text"), centre) {
							(Some(Value::Array(values)), Some(point)) => {
								close(point.x, number(&values[0]))
									&& close(point.y, number(&values[1]))
							}
							(Some(Value::Array(values)), None) => number(&values[1]) == -1000.0,
							(_, None) => true,
							_ => false,
						};
						let path_ok =
							expected.get("path").and_then(Value::as_str) == Some(path.as_str());
						if !(circles_ok && centre_ok && path_ok) {
							eprintln!(
								"  area mismatch: circles {circles_ok} centre {centre_ok} path {path_ok}: {path:?} vs {:?}",
								expected.get("path")
							);
						}
						circles_ok && centre_ok && path_ok
					})
				}
				(Some(Value::String(error)), Err(actual)) => error.ends_with(actual.as_str()),
				_ => false,
			};
			if ok {
				passed += 1;
			} else {
				failed += 1;
				eprintln!("FAILED {line}\n  got {actual:?}");
			}
		}
		(passed, failed)
	}

	#[test]
	fn lays_out_like_venn_js() {
		let (passed, failed) = check_fixtures(include_str!("fixtures.jsonl"));
		assert_eq!(failed, 0, "{passed} passed");
	}

	/// Float rounding differences of the math functions may flip rare ties.
	fn assert_mostly_equal(passed: usize, failed: usize) {
		assert!(failed * 100 <= passed + failed, "{failed} of {} differ", passed + failed);
	}

	/// `VENN_FIXTURES=<file> cargo test -p npp-toolbox --lib venn_fixtures -- --ignored`
	#[test]
	#[ignore]
	fn venn_fixtures() {
		let path = std::env::var("VENN_FIXTURES").expect("VENN_FIXTURES");
		let (passed, failed) = check_fixtures(&std::fs::read_to_string(path).unwrap());
		assert_mostly_equal(passed, failed);
	}

	#[test]
	fn orders_keys_like_javascript() {
		let mut keys = JsKeys::default();
		for key in ["b", "2", "a", "10", "1"] {
			keys.insert(key);
		}
		assert_eq!(keys.keys, ["1", "2", "10", "b", "a"]);
	}
}
