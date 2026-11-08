//! Where an edge toward a point leaves a node (`rendering-elements/intersect/*`).

use super::Point;

/// The center and size of a laid out node.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
	pub x: f64,
	pub y: f64,
	pub width: f64,
	pub height: f64,
}

/// `intersect-rect.js`.
pub fn rect(node: Bounds, point: Point) -> Point {
	let dx = point.x - node.x;
	let dy = point.y - node.y;
	let mut w = node.width / 2.0;
	let mut h = node.height / 2.0;
	let (sx, sy);
	if dy.abs() * w > dx.abs() * h {
		if dy < 0.0 {
			h = -h;
		}
		sx = if dy == 0.0 { 0.0 } else { h * dx / dy };
		sy = h;
	} else {
		if dx < 0.0 {
			w = -w;
		}
		sx = w;
		sy = if dx == 0.0 { 0.0 } else { w * dy / dx };
	}
	Point { x: node.x + sx, y: node.y + sy }
}

/// `intersect-ellipse.js`.
pub fn ellipse(node: Bounds, rx: f64, ry: f64, point: Point) -> Point {
	let px = node.x - point.x;
	let py = node.y - point.y;
	let det = (rx * rx * py * py + ry * ry * px * px).sqrt();
	let mut dx = (rx * ry * px / det).abs();
	if point.x < node.x {
		dx = -dx;
	}
	let mut dy = (rx * ry * py / det).abs();
	if point.y < node.y {
		dy = -dy;
	}
	Point { x: node.x + dx, y: node.y + dy }
}

pub fn circle(node: Bounds, radius: f64, point: Point) -> Point {
	ellipse(node, radius, radius, point)
}

/// `intersect-line.js`: where the segment `p1`–`p2` crosses `q1`–`q2`.
fn line(p1: Point, p2: Point, q1: Point, q2: Point) -> Option<Point> {
	let a1 = p2.y - p1.y;
	let b1 = p1.x - p2.x;
	let c1 = p2.x * p1.y - p1.x * p2.y;
	let r3 = a1 * q1.x + b1 * q1.y + c1;
	let r4 = a1 * q2.x + b1 * q2.y + c1;
	let epsilon = 1e-6;
	if r3 != 0.0 && r4 != 0.0 && r3 * r4 > 0.0 {
		return None;
	}
	let a2 = q2.y - q1.y;
	let b2 = q1.x - q2.x;
	let c2 = q2.x * q1.y - q1.x * q2.y;
	let r1 = a2 * p1.x + b2 * p1.y + c2;
	let r2 = a2 * p2.x + b2 * p2.y + c2;
	if r1.abs() < epsilon && r2.abs() < epsilon && r1 * r2 > 0.0 {
		return None;
	}
	let denominator = a1 * b2 - a2 * b1;
	if denominator == 0.0 {
		return None;
	}
	Some(Point { x: (b1 * c2 - b2 * c1) / denominator, y: (a2 * c1 - a1 * c2) / denominator })
}

/// `intersect-polygon.js`: `points` relative to the top left corner of the node's box.
pub fn polygon(node: Bounds, points: &[Point], point: Point) -> Point {
	let min_x = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
	let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
	let left = node.x - node.width / 2.0 - min_x;
	let top = node.y - node.height / 2.0 - min_y;
	let center = Point { x: node.x, y: node.y };
	let mut intersections: Vec<Point> = Vec::new();
	for (index, &p1) in points.iter().enumerate() {
		let p2 = points[if index + 1 < points.len() { index + 1 } else { 0 }];
		let q1 = Point { x: left + p1.x, y: top + p1.y };
		let q2 = Point { x: left + p2.x, y: top + p2.y };
		if let Some(intersection) = line(center, point, q1, q2) {
			intersections.push(intersection);
		}
	}
	let distance = |p: &Point| (p.x - point.x).hypot(p.y - point.y);
	// JavaScript's sort is stable, so equally distant points keep their order.
	npp::sort::sort_by(&mut intersections[..], |p, q| {
		distance(p).partial_cmp(&distance(q)).unwrap_or(std::cmp::Ordering::Equal)
	});
	intersections.first().copied().unwrap_or(center)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn intersects_like_mermaid() {
		let node = Bounds { x: 0.0, y: 0.0, width: 20.0, height: 10.0 };
		assert_eq!(rect(node, Point { x: 0.0, y: 50.0 }), Point { x: 0.0, y: 5.0 });
		assert_eq!(rect(node, Point { x: -40.0, y: 0.0 }), Point { x: -10.0, y: 0.0 });
		assert_eq!(circle(node, 5.0, Point { x: 10.0, y: 0.0 }), Point { x: 5.0, y: 0.0 });
		let diamond = [
			Point { x: 10.0, y: 0.0 },
			Point { x: 20.0, y: -5.0 },
			Point { x: 10.0, y: -10.0 },
			Point { x: 0.0, y: -5.0 },
		];
		let hit = polygon(node, &diamond, Point { x: 0.0, y: 30.0 });
		assert_eq!(hit, Point { x: 0.0, y: 5.0 });
	}
}
