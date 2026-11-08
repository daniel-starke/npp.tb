//! The flowchart symbols of Mermaid's shape registry (`@{ shape: … }`), one function each like
//! `rendering-elements/shapes/<name>.ts`. Mermaid draws most of them with roughjs; without the
//! hand-drawn look these are plain paths.

use super::blink_path;
use super::path_number as number;
use super::{
	Accent, BBox, Intersect, Part, Point, Shape, centered_box, circle_points, closed_path, point,
	points_box,
};

const OUTER: &str = "basic label-container outer-path";

/// A shape of one closed path through `points`, moved by `offset`.
fn path_shape(points: Vec<Point>, offset: Point, label_center: Point) -> Shape {
	Shape {
		bbox: points_box(&points, offset),
		parts: vec![Part::Path { d: closed_path(&points), offset, class: OUTER }],
		intersect: Intersect::Polygon(points),
		label_center,
	}
}

/// The label center for a label whose top left corner Mermaid translates to `(x, y)`.
fn label_at(x: f64, y: f64, bw: f64, bh: f64) -> Point {
	point(x + bw / 2.0, y + bh / 2.0)
}

/// The class of the junction circle, which Mermaid fills with the border color.
pub const FILLED: &str = "filled-circle";

/// The class of the data store rectangle, whose sides Mermaid leaves out with dashes.
pub const DATASTORE: &str = "basic label-container datastore";

/// `curlyBrace*.ts`'s own `generateCirclePoints`; `negate` like the left brace's copy.
fn brace_points(
	center_x: f64,
	center_y: f64,
	radius: f64,
	count: usize,
	start: f64,
	end: f64,
	negate: bool,
) -> Vec<Point> {
	let sign = if negate { -1.0 } else { 1.0 };
	let (start, end) = (start.to_radians(), end.to_radians());
	let step = (end - start) / (count - 1) as f64;
	(0..count)
		.map(|index| {
			let angle = start + index as f64 * step;
			point(
				sign * (center_x + radius * angle.cos()),
				sign * (center_y + radius * angle.sin()),
			)
		})
		.collect()
}

/// The open path through `points`: `createPathFromPoints` without its `Z`.
fn open_path(points: &[Point]) -> String {
	closed_path(points).replacen('Z', "", 1)
}

/// A brace shape: the brace paths over an invisibly stroked area, offset by `dx`.
fn brace_shape(braces: Vec<Vec<Point>>, area: Vec<Point>, dx: f64, label_center: Point) -> Shape {
	let offset = point(dx, 0.0);
	let mut parts: Vec<Part> = braces
		.iter()
		.rev()
		.map(|brace| Part::Path { d: open_path(brace), offset, class: "" })
		.collect();
	parts.push(Part::Path { d: closed_path(&area), offset, class: "brace-area" });
	let mut bbox = points_box(&area, offset);
	for brace in &braces {
		bbox.include(&points_box(brace, offset), 0.0, 0.0);
	}
	Shape {
		parts: vec![Part::Group { parts, class: "text" }],
		bbox,
		intersect: Intersect::Polygon(area),
		label_center,
	}
}

/// The shapes of this module by registry name; `None` for other names. `is_horizontal` tells
/// whether the graph flows left or right; the person and the horizontal cylinder grow to the
/// node size `(min_width, min_height)`, zero where it has none.
pub fn shape(
	name: &str,
	(bw, bh): (f64, f64),
	padding: f64,
	is_neo: bool,
	is_horizontal: bool,
	(min_width, min_height): (f64, f64),
) -> Option<Shape> {
	let neo_or = |neo: f64, classic: f64| if is_neo { neo } else { classic };
	let shape = match name {
		"notch-rect" | "card" | "notched-rectangle" => {
			const NOTCH: f64 = 12.0;
			let w = bw + neo_or(28.0 * 2.0, padding + NOTCH);
			let h = bh + neo_or(24.0 * 2.0, padding);
			let points = vec![
				point(NOTCH, -h),
				point(w, -h),
				point(w, 0.0),
				point(0.0, 0.0),
				point(0.0, -h + NOTCH),
				point(NOTCH, -h),
			];
			let offset = point(-w / 2.0, h / 2.0);
			Shape {
				bbox: points_box(&points, offset),
				intersect: Intersect::Polygon(points.clone()),
				parts: vec![Part::Polygon { points, offset, class: "label-container" }],
				label_center: Point::default(),
			}
		}
		"lin-rect" | "lined-rectangle" | "lined-process" | "lin-proc" | "shaded-process" => {
			const FRAME: f64 = 8.0;
			let (padding_x, padding_y) = (neo_or(16.0, padding), neo_or(12.0, padding));
			let total_width = bw + padding_x * 2.0 + neo_or(FRAME, FRAME * 2.0);
			let h = bh + padding_y * 2.0;
			let w = total_width - FRAME;
			let (x, y) = (FRAME - total_width / 2.0, -h / 2.0);
			let points = vec![
				point(x, y),
				point(x + w, y),
				point(x + w, y + h),
				point(x - FRAME, y + h),
				point(x - FRAME, y),
				point(x, y),
				point(x, y + h),
			];
			let mut shape = path_shape(points, Point::default(), point(FRAME / 2.0, 0.0));
			shape.intersect = Intersect::Rect;
			shape
		}
		"sm-circ" | "start" | "small-circle" | "stateStart" => Shape {
			parts: vec![Part::Circle {
				center: Point::default(),
				radius: 7.0,
				class: "state-start",
			}],
			bbox: centered_box(14.0, 14.0),
			intersect: Intersect::NodeCircle,
			label_center: Point::default(),
		},
		"fr-circ" | "stop" | "framed-circle" | "stateEnd" => Shape {
			parts: vec![Part::Group {
				parts: vec![
					Part::Circle { center: Point::default(), radius: 7.0, class: "" },
					Part::Circle { center: Point::default(), radius: 2.5, class: "" },
				],
				class: "outer-path",
			}],
			bbox: centered_box(14.0, 14.0),
			intersect: Intersect::NodeCircle,
			label_center: Point::default(),
		},
		"f-circ" | "junction" | "filled-circle" => Shape {
			parts: vec![Part::Circle { center: Point::default(), radius: 7.0, class: FILLED }],
			bbox: centered_box(14.0, 14.0),
			intersect: Intersect::Circle(7.0),
			label_center: Point::default(),
		},
		"fork" | "join" | "forkJoin" => {
			// The bar is across the direction of the flow.
			let (width, height) = if is_horizontal { (10.0, 70.0) } else { (70.0, 10.0) };
			Shape {
				parts: vec![Part::Rect {
					x: -width / 2.0,
					y: -height / 2.0,
					width,
					height,
					rx: None,
					class: "",
				}],
				bbox: centered_box(width, height),
				intersect: Intersect::Rect,
				label_center: Point::default(),
			}
		}
		"hourglass" | "collate" => {
			let points =
				vec![point(0.0, 0.0), point(30.0, 0.0), point(0.0, 30.0), point(30.0, 30.0)];
			path_shape(points, point(-15.0, -15.0), Point::default())
		}
		"bolt" | "com-link" | "lightning-bolt" => {
			let (width, height, gap) = (35.0, 35.0, 7.0);
			let points = vec![
				point(width, 0.0),
				point(0.0, height + gap / 2.0),
				point(width - 2.0 * gap, height + gap / 2.0),
				point(0.0, 2.0 * height),
				point(width, height - gap / 2.0),
				point(2.0 * gap, height - gap / 2.0),
			];
			let mut shape = path_shape(points, point(-width / 2.0, -height), Point::default());
			if let Some(Part::Path { class, .. }) = shape.parts.first_mut() {
				*class = "outer-path";
			}
			shape
		}
		"doc" | "document" => {
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(12.0, padding) * 2.0;
			let amplitude = if is_neo { h / 4.0 } else { h / 8.0 };
			let final_h = h + amplitude;
			let extra_w = ((14.0 - w) / 2.0).max(0.0);
			let mut points = vec![point(-w / 2.0 - extra_w, final_h / 2.0)];
			points.extend(sine_wave(
				-w / 2.0 - extra_w,
				final_h / 2.0,
				w / 2.0 + extra_w,
				final_h / 2.0,
				amplitude,
				0.8,
			));
			points.push(point(w / 2.0 + extra_w, -final_h / 2.0));
			points.push(point(-w / 2.0 - extra_w, -final_h / 2.0));
			let label = label_at(-w / 2.0 + padding, -h / 2.0 + padding - amplitude, bw, bh);
			path_shape(points, point(0.0, -amplitude / 2.0), label)
		}
		"delay" | "half-rounded-rectangle" => {
			let (padding_x, padding_y) = (neo_or(16.0, padding), neo_or(12.0, padding));
			let h = bh.max(10.0) + padding_y * 2.0;
			let radius = h / 2.0;
			let clearance = radius - (radius * radius - (bh / 2.0) * (bh / 2.0)).max(0.0).sqrt();
			let w = bw.max(15.0) + clearance * 2.0 + padding_x * 2.0;
			let mut points = vec![point(-w / 2.0, -h / 2.0), point(w / 2.0 - radius, -h / 2.0)];
			points.extend(circle_points(-w / 2.0 + radius, 0.0, radius, 50, 90.0, 270.0));
			points.push(point(w / 2.0 - radius, h / 2.0));
			points.push(point(-w / 2.0, h / 2.0));
			path_shape(points, Point::default(), Point::default())
		}
		"h-cyl" | "das" | "horizontal-cylinder" => {
			let label_padding = neo_or(12.0, padding / 2.0);
			let (width, height) =
				horizontal_cylinder_label_size((min_width, min_height), label_padding);
			let h = height.max(bh) + label_padding;
			let ry = h / 2.0;
			let rx = ry / (2.5 + h / 50.0);
			let w = width.max(bw) + rx + label_padding;
			let d = format!(
				"M0,0 a{rx},{ry} 0,0,1 0,{} l{w},0 a{rx},{ry} 0,0,1 0,{h} M{w},{} a{rx},{ry} 0,0,0 0,{h} l{},0",
				number(-h),
				number(-h),
				number(-w),
				rx = number(rx),
				ry = number(ry),
				w = number(w),
				h = number(h)
			);
			Shape {
				parts: vec![Part::Path { d, offset: point(-w / 2.0, h / 2.0), class: OUTER }],
				bbox: centered_box(w + 2.0 * rx, h),
				intersect: Intersect::HorizontalCylinder { rx, ry },
				label_center: point(-rx, 0.0),
			}
		}
		"lin-cyl" | "disk" | "lined-cylinder" => {
			let w = bw + neo_or(16.0, padding) * 2.0;
			let rx = w / 2.0;
			let ry = rx / (2.5 + w / 50.0);
			let h = bh + ry + neo_or(24.0, padding) * 2.0;
			let outer_offset = h * 0.1;
			let d = format!(
				"M0,{ry} a{rx},{ry} 0,0,0 {w},0 a{rx},{ry} 0,0,0 {},0 l0,{h} a{rx},{ry} 0,0,0 {w},0 l0,{} M0,{} a{rx},{ry} 0,0,0 {w},0",
				number(-w),
				number(-h),
				number(ry + outer_offset),
				ry = number(ry),
				rx = number(rx),
				w = number(w),
				h = number(h)
			);
			Shape {
				parts: vec![Part::Path {
					d,
					offset: point(-w / 2.0, -(h / 2.0 + ry)),
					class: OUTER,
				}],
				bbox: centered_box(w, h + 2.0 * ry),
				intersect: Intersect::Cylinder { rx, ry },
				label_center: point(0.0, ry),
			}
		}
		"curv-trap" | "curved-trapezoid" | "display" => {
			let (padding_x, padding_y) = (neo_or(16.0, padding), neo_or(12.0, padding));
			let h = (bh + padding_y * 2.0).max(5.0);
			let radius = h / 2.0;
			let clearance = radius - (radius * radius - (bh / 2.0) * (bh / 2.0)).max(0.0).sqrt();
			let side = (h / 4.0).max(clearance);
			let w =
				(bw + padding_x * 2.0 + side * 2.0).max((bw + padding_x * 2.0) * 1.25).max(20.0);
			let rw = w - radius;
			let tw = h / 4.0;
			let mut points = vec![
				point(rw, 0.0),
				point(tw, 0.0),
				point(0.0, h / 2.0),
				point(tw, h),
				point(rw, h),
			];
			points.extend(circle_points(-rw, -h / 2.0, radius, 50, 270.0, 90.0));
			path_shape(points, point(-w / 2.0, -h / 2.0), Point::default())
		}
		"div-rect" | "div-proc" | "divided-rectangle" | "divided-process" => {
			let (padding_x, padding_y) = (neo_or(16.0, padding), neo_or(16.0, padding));
			let w = bw + padding_x;
			let h = bh + padding_y;
			let offset = h * 0.2;
			let (x, y) = (-w / 2.0, -h / 2.0 - offset / 2.0);
			let points = vec![
				point(x, y + offset),
				point(-x, y + offset),
				point(-x, -y),
				point(x, -y),
				point(x, y),
				point(-x, y),
				point(-x, y + offset),
			];
			let label = label_at(x + padding / 2.0, y + offset + padding / 2.0, bw, bh);
			let mut shape = path_shape(points, Point::default(), label);
			shape.intersect = Intersect::Rect;
			shape
		}
		"tri" | "extract" | "triangle" => {
			let w = bw + neo_or(padding * 2.0, padding);
			let h = w + bh;
			let points = vec![point(0.0, 0.0), point(h, 0.0), point(h / 2.0, -h)];
			let label = label_at(-bw / 2.0, h / 2.0 - (bh + padding / 2.0), bw, bh);
			let mut shape = path_shape(points, point(-h / 2.0, h / 2.0), label);
			if let Some(Part::Path { class, .. }) = shape.parts.first_mut() {
				*class = "outer-path";
			}
			shape
		}
		"flip-tri" | "manual-file" | "flipped-triangle" => {
			let w = bw + neo_or(padding * 2.0, padding);
			let h = w + bh;
			let points = vec![point(0.0, -h), point(h, -h), point(h / 2.0, 0.0)];
			let label = label_at(-bw / 2.0, -h / 2.0 + padding / 2.0, bw, bh);
			let mut shape = path_shape(points, point(-h / 2.0, h / 2.0), label);
			if let Some(Part::Path { class, .. }) = shape.parts.first_mut() {
				*class = "outer-path";
			}
			shape
		}
		"win-pane" | "internal-storage" | "window-pane" => {
			const OFFSET: f64 = 10.0;
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(12.0, padding) * 2.0;
			let (x, y) = (-w / 2.0, -h / 2.0);
			let outer = vec![
				point(x - OFFSET, y - OFFSET),
				point(x - OFFSET, y + h),
				point(x + w, y + h),
				point(x + w, y - OFFSET),
			];
			let n = number;
			let d = format!(
				"M{},{} L{},{} L{},{} L{},{} L{},{} M{},{} L{},{} M{},{} L{},{}",
				n(x - OFFSET),
				n(y - OFFSET),
				n(x + w),
				n(y - OFFSET),
				n(x + w),
				n(y + h),
				n(x - OFFSET),
				n(y + h),
				n(x - OFFSET),
				n(y - OFFSET),
				n(x - OFFSET),
				n(y),
				n(x + w),
				n(y),
				n(x),
				n(y - OFFSET),
				n(x),
				n(y + h)
			);
			let offset = point(OFFSET / 2.0, OFFSET / 2.0);
			Shape {
				bbox: points_box(&outer, offset),
				parts: vec![Part::Path { d, offset, class: OUTER }],
				intersect: Intersect::Polygon(outer),
				label_center: point(OFFSET / 2.0, OFFSET / 2.0),
			}
		}
		"notch-pent" | "loop-limit" | "notched-pentagon" => {
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(12.0, padding) * 2.0;
			let points = vec![
				point(-w / 2.0 * 0.8, -h / 2.0),
				point(w / 2.0 * 0.8, -h / 2.0),
				point(w / 2.0, -h / 2.0 * 0.6),
				point(w / 2.0, h / 2.0),
				point(-w / 2.0, h / 2.0),
				point(-w / 2.0, -h / 2.0 * 0.6),
			];
			path_shape(points, Point::default(), Point::default())
		}
		"sl-rect" | "manual-input" | "sloped-rectangle" => {
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(12.0, padding) * 2.0;
			let (x, y) = (-w / 2.0, -h / 2.0);
			let points =
				vec![point(x, y), point(x, y + h), point(x + w, y + h), point(x + w, y - h / 2.0)];
			let label = label_at(-w / 2.0 + padding, -h / 4.0 + padding, bw, bh);
			path_shape(points, point(0.0, h / 4.0), label)
		}
		"st-rect" | "procs" | "processes" | "stacked-rectangle" => {
			let offset = neo_or(10.0, 5.0);
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(12.0, padding) * 2.0;
			let (x, y) = (-w / 2.0, -h / 2.0);
			let outer = vec![
				point(x - offset, y + offset),
				point(x - offset, y + h + offset),
				point(x + w - offset, y + h + offset),
				point(x + w - offset, y + h),
				point(x + w, y + h),
				point(x + w, y + h - offset),
				point(x + w + offset, y + h - offset),
				point(x + w + offset, y - offset),
				point(x + offset, y - offset),
				point(x + offset, y),
				point(x, y),
				point(x, y + offset),
			];
			let inner = vec![
				point(x, y + offset),
				point(x + w - offset, y + offset),
				point(x + w - offset, y + h),
				point(x + w, y + h),
				point(x + w, y),
				point(x, y),
			];
			Shape {
				bbox: points_box(&outer, Point::default()),
				parts: vec![Part::Group {
					parts: vec![
						Part::Path { d: closed_path(&outer), offset: Point::default(), class: "" },
						Part::Path { d: closed_path(&inner), offset: Point::default(), class: "" },
					],
					class: OUTER,
				}],
				intersect: Intersect::Polygon(outer),
				label_center: point(-offset, offset),
			}
		}
		"bow-rect" | "stored-data" | "bow-tie-rectangle" => {
			// Mermaid adds the arc's sagitta to the total width and takes it off again.
			let h = bh + neo_or(12.0, padding);
			let ry = h / 2.0;
			let rx = ry / (2.5 + h / 50.0);
			let w = bw + neo_or(16.0, padding) * 2.0;
			let mut points = vec![point(w / 2.0, -h / 2.0), point(-w / 2.0, -h / 2.0)];
			points.extend(arc_points(-w / 2.0, -h / 2.0, -w / 2.0, h / 2.0, rx, ry, false));
			points.push(point(w / 2.0, h / 2.0));
			points.extend(arc_points(w / 2.0, h / 2.0, w / 2.0, -h / 2.0, rx, ry, true));
			path_shape(points, point(rx / 2.0, 0.0), Point::default())
		}
		"cross-circ" | "summary" | "crossed-circle" => {
			let radius = 30.0;
			let c = radius * std::f64::consts::FRAC_1_SQRT_2;
			let d = format!(
				"M {},{} L {},{} M {},{} L {},{}",
				number(-c),
				number(c),
				number(c),
				number(-c),
				number(c),
				number(c),
				number(-c),
				number(-c)
			);
			Shape {
				parts: vec![Part::Group {
					parts: vec![
						Part::Circle { center: Point::default(), radius, class: "" },
						Part::Path { d, offset: Point::default(), class: "" },
					],
					class: "outer-path",
				}],
				bbox: centered_box(radius * 2.0, radius * 2.0),
				intersect: Intersect::Circle(radius),
				label_center: Point::default(),
			}
		}
		"docs" | "documents" | "st-doc" | "stacked-document" => {
			const OFFSET: f64 = 10.0;
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(12.0, padding) * 3.0;
			let amplitude = if is_neo { h / 4.0 } else { h / 8.0 };
			let final_h = h + amplitude / 2.0;
			let (x, y) = (-w / 2.0, -final_h / 2.0);
			let wave = sine_wave(
				x - OFFSET,
				y + final_h + OFFSET,
				x + w - OFFSET,
				y + final_h + OFFSET,
				amplitude,
				0.8,
			);
			let last_y = wave.last().map_or(y + final_h + OFFSET, |last| last.y);
			let mut outer =
				vec![point(x - OFFSET, y + OFFSET), point(x - OFFSET, y + final_h + OFFSET)];
			outer.extend(wave);
			outer.extend([
				point(x + w - OFFSET, last_y - OFFSET),
				point(x + w, last_y - OFFSET),
				point(x + w, last_y - 2.0 * OFFSET),
				point(x + w + OFFSET, last_y - 2.0 * OFFSET),
				point(x + w + OFFSET, y - OFFSET),
				point(x + OFFSET, y - OFFSET),
				point(x + OFFSET, y),
				point(x, y),
				point(x, y + OFFSET),
			]);
			let inner = vec![
				point(x, y + OFFSET),
				point(x + w - OFFSET, y + OFFSET),
				point(x + w - OFFSET, last_y - OFFSET),
				point(x + w, last_y - OFFSET),
				point(x + w, y),
				point(x, y),
			];
			let offset = point(0.0, -amplitude / 2.0);
			Shape {
				bbox: points_box(&outer, offset),
				parts: vec![Part::Group {
					parts: vec![
						Part::Path { d: closed_path(&outer), offset, class: "" },
						Part::Path { d: closed_path(&inner), offset, class: "" },
					],
					class: OUTER,
				}],
				intersect: Intersect::Polygon(outer),
				label_center: point(-OFFSET, OFFSET - amplitude),
			}
		}
		"tag-rect" | "tagged-rectangle" | "tag-proc" | "tagged-process" => {
			let h = bh + neo_or(12.0, padding) * 2.0;
			let tag = 0.2 * h;
			let w = bw + neo_or(16.0, padding) * 2.0;
			let (x, y) = (-w / 2.0, -h / 2.0);
			let rect = vec![
				point(x - tag / 2.0, y),
				point(x + w + tag / 2.0, y),
				point(x + w + tag / 2.0, y + h),
				point(x - tag / 2.0, y + h),
			];
			let corner = vec![
				point(x + w - tag / 2.0, y + h),
				point(x + w + tag / 2.0, y + h),
				point(x + w + tag / 2.0, y + h - tag),
			];
			Shape {
				bbox: points_box(&rect, Point::default()),
				parts: vec![Part::Group {
					parts: vec![
						Part::Path { d: closed_path(&rect), offset: Point::default(), class: "" },
						Part::Path { d: closed_path(&corner), offset: Point::default(), class: "" },
					],
					class: OUTER,
				}],
				intersect: Intersect::Polygon(rect),
				label_center: Point::default(),
			}
		}
		"tag-doc" | "tagged-document" => {
			let w = bw + padding * 2.0;
			let h = bh + padding * 2.0;
			let amplitude = h / 8.0;
			let (tag_width, tag_height) = (0.2 * w, 0.2 * h);
			let final_h = h + amplitude;
			let left = -w / 2.0 - w / 2.0 * 0.1;
			let right = w / 2.0 + w / 2.0 * 0.1;
			let mut points = vec![point(left, final_h / 2.0)];
			points.extend(sine_wave(left, final_h / 2.0, right, final_h / 2.0, amplitude, 0.8));
			points.extend([point(right, -final_h / 2.0), point(left, -final_h / 2.0)]);
			let x = -w / 2.0 + w / 2.0 * 0.1;
			let y = -final_h / 2.0 - tag_height * 0.4;
			let mut tag = vec![
				point(x + w - tag_width, (y + h) * 1.3),
				point(x + w, y + h - tag_height),
				point(x + w, (y + h) * 0.9),
			];
			tag.extend(sine_wave(
				x + w,
				(y + h) * 1.25,
				x + w - tag_width,
				(y + h) * 1.3,
				-h * 0.02,
				0.5,
			));
			let offset = point(0.0, -amplitude / 2.0);
			Shape {
				bbox: points_box(&points, offset),
				parts: vec![Part::Group {
					parts: vec![
						Part::Path { d: closed_path(&points), offset, class: "" },
						Part::Path { d: closed_path(&tag), offset, class: "" },
					],
					class: OUTER,
				}],
				intersect: Intersect::Polygon(points),
				label_center: label_at(
					-w / 2.0 + padding,
					-h / 2.0 + padding - amplitude / 2.0,
					bw,
					bh,
				),
			}
		}
		"flag" | "paper-tape" => {
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(20.0, padding);
			let amplitude = h / 8.0;
			let final_h = h + amplitude * 2.0;
			let mut points = vec![point(-w / 2.0, final_h / 2.0)];
			points.extend(sine_wave(
				-w / 2.0,
				final_h / 2.0,
				w / 2.0,
				final_h / 2.0,
				amplitude,
				1.0,
			));
			points.push(point(w / 2.0, -final_h / 2.0));
			points.extend(sine_wave(
				w / 2.0,
				-final_h / 2.0,
				-w / 2.0,
				-final_h / 2.0,
				amplitude,
				-1.0,
			));
			let mut shape = path_shape(points, Point::default(), Point::default());
			if let Some(Part::Path { class, .. }) = shape.parts.first_mut() {
				*class = "basic label-container";
			}
			shape
		}
		"lin-doc" | "lined-document" => {
			let w = bw + neo_or(16.0, padding) * 2.0;
			let h = bh + neo_or(12.0, padding) * 2.0;
			let amplitude = if is_neo { h / 4.0 } else { h / 8.0 };
			let final_h = h + amplitude;
			let (left, right) = (-w / 2.0 - w / 2.0 * 0.1, w / 2.0 + w / 2.0 * 0.1);
			let mut points = vec![point(left, -final_h / 2.0), point(left, final_h / 2.0)];
			points.extend(sine_wave(left, final_h / 2.0, right, final_h / 2.0, amplitude, 0.8));
			points.extend([
				point(right, -final_h / 2.0),
				point(left, -final_h / 2.0),
				point(-w / 2.0, -final_h / 2.0),
				point(-w / 2.0, final_h / 2.0 * 1.1),
				point(-w / 2.0, -final_h / 2.0),
			]);
			let label = label_at(
				-w / 2.0 + padding + w / 2.0 * 0.1 / 2.0,
				-h / 2.0 + padding - amplitude,
				bw,
				bh,
			);
			path_shape(points, point(0.0, -amplitude / 2.0), label)
		}
		"brace" | "comment" | "brace-l" | "brace-r" | "braces" => {
			// `curlyBraceLeft` adds the neo padding once, the right and double braces twice.
			let neo_factor = if matches!(name, "brace-r" | "braces") { 2.0 } else { 1.0 };
			let w = bw + neo_or(18.0 * neo_factor, padding);
			let h = bh + neo_or(12.0 * neo_factor, padding);
			let r = (h * 0.1).max(5.0);
			let arc = |cx, cy, count, start, end| brace_points(cx, cy, r, count, start, end, true);
			let left = || {
				let mut points = arc(w / 2.0, -h / 2.0, 30, -90.0, 0.0);
				points.push(point(-w / 2.0 - r, r));
				points.extend(arc(w / 2.0 + r * 2.0, -r, 20, -180.0, -270.0));
				points.extend(arc(w / 2.0 + r * 2.0, r, 20, -90.0, -180.0));
				points.push(point(-w / 2.0 - r, -h / 2.0));
				points.extend(arc(w / 2.0, h / 2.0, 20, 0.0, 90.0));
				points
			};
			// `curlyBraces` puts the middle of its area further left than `curlyBraceLeft`.
			let left_area = |middle_x: f64, closing: Vec<Point>| {
				let mut points = vec![point(w / 2.0, -h / 2.0 - r), point(-w / 2.0, -h / 2.0 - r)];
				points.extend(arc(w / 2.0, -h / 2.0, 20, -90.0, 0.0));
				points.push(point(-w / 2.0 - r, -r));
				points.extend(arc(middle_x, -r, 20, -180.0, -270.0));
				points.extend(arc(middle_x, r, 20, -90.0, -180.0));
				points.push(point(-w / 2.0 - r, h / 2.0));
				points.extend(arc(w / 2.0, h / 2.0, 20, 0.0, 90.0));
				points.push(point(-w / 2.0, h / 2.0 + r));
				points.extend(closing);
				points
			};
			let label = label_at(-w / 2.0 + padding / 2.0, -h / 2.0 + padding / 2.0, bw, bh);
			match name {
				"brace-r" => {
					let arc = |cx, cy, count, start, end| {
						brace_points(cx, cy, r, count, start, end, false)
					};
					let mut brace = arc(w / 2.0, -h / 2.0, 20, -90.0, 0.0);
					brace.push(point(w / 2.0 + r, -r));
					brace.extend(arc(w / 2.0 + r * 2.0, -r, 20, -180.0, -270.0));
					brace.extend(arc(w / 2.0 + r * 2.0, r, 20, -90.0, -180.0));
					brace.push(point(w / 2.0 + r, h / 2.0));
					brace.extend(arc(w / 2.0, h / 2.0, 20, 0.0, 90.0));
					let mut area =
						vec![point(-w / 2.0, -h / 2.0 - r), point(w / 2.0, -h / 2.0 - r)];
					area.extend(brace.iter().copied());
					area.extend([point(w / 2.0, h / 2.0 + r), point(-w / 2.0, h / 2.0 + r)]);
					brace_shape(vec![brace], area, -r, label)
				}
				"braces" => {
					let mut right = arc(-w / 2.0 + r + r / 2.0, -h / 2.0, 20, -90.0, -180.0);
					right.push(point(w / 2.0 - r / 2.0, r));
					right.extend(arc(-w / 2.0 - r / 2.0, -r, 20, 0.0, 90.0));
					right.extend(arc(-w / 2.0 - r / 2.0, r, 20, -90.0, 0.0));
					right.push(point(w / 2.0 - r / 2.0, -r));
					right.extend(arc(-w / 2.0 + r + r / 2.0, h / 2.0, 30, -180.0, -270.0));
					let mut closing = vec![point(w / 2.0 - r - r / 2.0, h / 2.0 + r)];
					closing.extend(right.iter().copied());
					let area = left_area(w / 2.0 + r * 2.0, closing);
					brace_shape(vec![left(), right], area, r - r / 4.0, label)
				}
				_ => {
					let area = left_area(w / 2.0 + w * 0.1, vec![point(w / 2.0, h / 2.0 + r)]);
					brace_shape(
						vec![left()],
						area,
						r,
						label_at(-w / 2.0 + r, -h / 2.0 + padding / 2.0, bw, bh),
					)
				}
			}
		}
		"datastore" | "data-store" => {
			// `labelPaddingX` is twice the padding.
			let w = bw + padding * 4.0;
			let h = bh + padding * 2.0;
			Shape {
				parts: vec![Part::Rect {
					x: -w / 2.0,
					y: -h / 2.0,
					width: w,
					height: h,
					rx: Some(0.0),
					class: DATASTORE,
				}],
				bbox: centered_box(w, h),
				intersect: Intersect::Rect,
				label_center: Point::default(),
			}
		}
		"folder" | "directory" => {
			let w = (bw + padding * 2.0).max(90.0);
			let content_height = bh + padding * 2.0;
			let tab_height = (content_height * 0.16).clamp(8.0, 14.0);
			let total_height = content_height + tab_height;
			let tab_width = (w * 0.38).max(28.0);
			let top = -total_height / 2.0;
			let points = vec![
				point(-w / 2.0, top),
				point(-w / 2.0 + tab_width, top),
				point(-w / 2.0 + tab_width, top + tab_height),
				point(w / 2.0, top + tab_height),
				point(w / 2.0, total_height / 2.0),
				point(-w / 2.0, total_height / 2.0),
			];
			let body_center = top + tab_height + (total_height - tab_height) / 2.0;
			let mut shape = path_shape(points, Point::default(), point(0.0, body_center));
			if let Some(Part::Path { class, .. }) = shape.parts.first_mut() {
				*class = "basic label-container";
			}
			shape
		}
		"bucket" => {
			let w = (bw + padding * 2.0).max(80.0);
			let rim = (w * 0.08).clamp(5.0, 12.0);
			let total_height = bh + padding * 2.0 + rim;
			let top = -total_height / 2.0 + rim;
			let bottom = total_height / 2.0;
			let bottom_w = w * 0.72;
			let n = number;
			let d = format!(
				"M{},{} L{},{} A{},{} 0 0 0 {},{} L{},{} A{},{} 0 0 0 {},{} Z",
				n(-w / 2.0),
				n(top),
				n(-bottom_w / 2.0),
				n(bottom),
				n(bottom_w / 2.0),
				n(rim),
				n(bottom_w / 2.0),
				n(bottom),
				n(w / 2.0),
				n(top),
				n(w / 2.0),
				n(rim),
				n(-w / 2.0),
				n(top)
			);
			const SEGMENTS: usize = 12;
			let arc = |rx: f64, cy: f64, sign: f64| -> Vec<Point> {
				(0..=SEGMENTS)
					.map(|i| {
						let theta = std::f64::consts::PI
							- i as f64 * std::f64::consts::PI / SEGMENTS as f64;
						point(rx * theta.cos(), cy + sign * rim * theta.sin())
					})
					.collect()
			};
			let mut outline = arc(w / 2.0, top, -1.0);
			outline.extend(arc(bottom_w / 2.0, bottom, 1.0).into_iter().rev());
			let mut bbox = points_box(&outline, Point::default());
			bbox.include_rect(-w / 2.0, top - rim, w, rim * 2.0);
			Shape {
				parts: vec![Part::Group {
					parts: vec![
						Part::Path { d, offset: Point::default(), class: "" },
						Part::Rim { center: point(0.0, top), rx: w / 2.0, ry: rim },
					],
					class: "basic label-container",
				}],
				bbox,
				intersect: Intersect::Polygon(outline),
				label_center: point(0.0, top + (bottom - top) / 2.0),
			}
		}
		"console" | "browser" => {
			const RADIUS: f64 = 12.0;
			let band = if name == "console" { 20.0 } else { 18.0 };
			let w = (bw + padding * 2.0).max(90.0);
			let h = bh + padding * 2.0 + band;
			let top = -h / 2.0;
			let mut parts = vec![Part::Rect {
				x: -w / 2.0,
				y: top,
				width: w,
				height: h,
				rx: Some(RADIUS),
				class: "",
			}];
			if name == "console" {
				parts.push(Part::Accent(Accent::Prompt { x: -w / 2.0 + 12.0, y: top + 16.0 }));
			} else {
				parts.push(Part::Accent(Accent::Line {
					from: point(-w / 2.0, top + band),
					to: point(w / 2.0, top + band),
				}));
				for index in 0..3 {
					parts.push(Part::Accent(Accent::Dot {
						center: point(-w / 2.0 + 12.0 + index as f64 * 9.0, top + band / 2.0),
						radius: 2.5,
					}));
				}
				parts.push(Part::Accent(Accent::AddressBar {
					x: -w / 2.0 + 44.0,
					y: top + 4.0,
					width: (w - 56.0).max(10.0),
					height: band - 8.0,
				}));
			}
			Shape {
				parts: vec![Part::Group { parts, class: "basic label-container" }],
				bbox: centered_box(w, h),
				intersect: Intersect::Rect,
				label_center: point(0.0, top + band + (h - band) / 2.0),
			}
		}
		"person" => {
			let w = (bw + padding * 2.0).max(min_width).max(100.0);
			let head_radius = (w * 0.23).clamp(16.0, 56.0);
			let overlap = head_radius * 0.27;
			let body_height = match min_height != 0.0 && !min_height.is_nan() {
				true => (bh + padding * 2.0).max(min_height - (2.0 * head_radius - overlap)),
				false => (bh + padding * 2.0).max(0.0),
			};
			let body_radius = (w * 0.177).min(body_height * 0.45);
			let total_height = body_height + 2.0 * head_radius - overlap;
			let top = -total_height / 2.0;
			let body_top = top + 2.0 * head_radius - overlap;
			let head_center_y = top + head_radius;
			let phi = ((body_top - head_center_y) / head_radius).min(1.0).asin().to_degrees();
			let left = -(-w / 2.0 + body_radius);
			let right = -(w / 2.0 - body_radius);
			let upper = -(body_top + body_radius);
			let lower = -(total_height / 2.0 - body_radius);
			let mut outline =
				circle_points(0.0, -head_center_y, head_radius, 24, 180.0 + phi, -phi);
			outline.extend(circle_points(left, upper, body_radius, 12, 90.0, 0.0));
			outline.extend(circle_points(left, lower, body_radius, 12, 360.0, 270.0));
			outline.extend(circle_points(right, lower, body_radius, 12, 270.0, 180.0));
			outline.extend(circle_points(right, upper, body_radius, 12, 180.0, 90.0));
			Shape {
				parts: vec![Part::Group {
					parts: vec![
						Part::Rect {
							x: -w / 2.0,
							y: body_top,
							width: w,
							height: body_height,
							rx: Some(body_radius),
							class: "",
						},
						Part::Circle {
							center: point(0.0, head_center_y),
							radius: head_radius,
							class: "",
						},
					],
					class: "basic label-container",
				}],
				bbox: centered_box(w, total_height),
				intersect: Intersect::Polygon(outline),
				label_center: point(0.0, body_top + body_height / 2.0),
			}
		}
		"bang" => {
			let half_padding = padding / 2.0;
			let w = (bw + 10.0 * half_padding).max(bw + 20.0);
			let h = (bh + 8.0 * half_padding).max(bh + 20.0);
			let r = 0.15 * (bw + 10.0 * half_padding);
			let arcs = [
				(r, r, 0.25 * w, -0.1 * h),
				(r, r, 0.25 * w, 0.0),
				(r, r, 0.25 * w, 0.0),
				(r, r, 0.25 * w, 0.1 * h),
				(r, r, 0.15 * w, 0.33 * h),
				(r * 0.8, r * 0.8, 0.0, 0.34 * h),
				(r, r, -0.15 * w, 0.33 * h),
				(r, r, -0.25 * w, 0.15 * h),
				(r, r, -0.25 * w, 0.0),
				(r, r, -0.25 * w, 0.0),
				(r, r, -0.25 * w, -0.15 * h),
				(r, r, -0.1 * w, -0.33 * h),
				(r * 0.8, r * 0.8, 0.0, -0.34 * h),
				(r, r, 0.1 * w, -0.33 * h),
			];
			arc_shape(&arcs, false, point(-w / 2.0, -h / 2.0), 1.0)
		}
		"cloud" => {
			let w = bw + padding;
			let h = bh + padding;
			let (r1, r2, r3, r4) = (0.15 * w, 0.25 * w, 0.35 * w, 0.2 * w);
			let arcs = [
				(r1, r1, 0.25 * w, -0.1 * w),
				(r3, r3, 0.4 * w, -0.1 * w),
				(r2, r2, 0.35 * w, 0.2 * w),
				(r1, r1, 0.15 * w, 0.35 * h),
				(r4, r4, -0.15 * w, 0.65 * h),
				(r2, r1, -0.25 * w, 0.15 * w),
				(r3, r3, -0.5 * w, 0.0),
				(r1, r1, -0.25 * w, -0.15 * w),
				(r1, r1, -0.1 * w, -0.35 * h),
				(r4, r4, 0.1 * w, -0.65 * h),
			];
			arc_shape(&arcs, true, point(-w / 2.0, -h / 2.0), 0.0)
		}
		_ => return None,
	};
	Some(shape)
}

/// `tiltedCylinder` shrinks a given node size by its padding and caps, to at least 10 × 5
/// pixels: the size it places the label in.
pub fn horizontal_cylinder_label_size(
	(width, height): (f64, f64),
	label_padding: f64,
) -> (f64, f64) {
	let is_truthy = |value: f64| value != 0.0 && !value.is_nan();
	let original_height = height;
	let height = match is_truthy(height) {
		true => (height - label_padding).max(5.0),
		false => height,
	};
	let width = match is_truthy(width) {
		true => {
			let ry = original_height / 2.0;
			let rx = ry / (2.5 + original_height / 50.0);
			(width - label_padding - rx * 3.0).max(10.0)
		}
		false => width,
	};
	(width, height)
}

/// The bang and cloud: a closed path of relative arcs `(rx, ry, dx, dy)` from the origin, moved
/// by `offset`, with Blink's box of it. Edges end at the node's box.
fn arc_shape(
	arcs: &[(f64, f64, f64, f64)],
	sweep: bool,
	offset: Point,
	first_rotation: f64,
) -> Shape {
	// Mermaid writes the large arc flag where the rotation belongs: `a r,r 1 0,sweep dx,dy`.
	let arcs: Vec<blink_path::Arc> = arcs
		.iter()
		.enumerate()
		.map(|(index, &(rx, ry, dx, dy))| blink_path::Arc {
			rx,
			ry,
			rotation: if index == 0 { first_rotation } else { 1.0 },
			is_large: false,
			is_sweep: sweep,
			dx,
			dy,
		})
		.collect();
	let mut d = "M0 0".to_owned();
	for arc in &arcs {
		d.push_str(&format!(
			" a{},{} {} 0,{} {},{}",
			number(arc.rx),
			number(arc.ry),
			number(arc.rotation),
			if sweep { 1 } else { 0 },
			number(arc.dx),
			number(arc.dy)
		));
	}
	d.push_str(" H0 V0 Z");
	// `getBBox` of the path in its group: moved by the translation in single precision.
	let [left, top, right, bottom] = blink_path::arc_path_bounds(&arcs);
	let (width, height) = (right - left, bottom - top);
	let (x, y) =
		(left + blink_path::parse_number(offset.x), top + blink_path::parse_number(offset.y));
	let mut bbox = BBox::default();
	bbox.include_point(point(f64::from(x), f64::from(y)));
	bbox.include_point(point(f64::from(x + width), f64::from(y + height)));
	Shape {
		parts: vec![Part::Path { d, offset, class: "basic label-container" }],
		bbox,
		intersect: Intersect::Rect,
		label_center: Point::default(),
	}
}

/// `generateFullSineWavePoints`.
fn sine_wave(x1: f64, y1: f64, x2: f64, y2: f64, amplitude: f64, cycles: f64) -> Vec<Point> {
	const STEPS: usize = 50;
	let (dx, dy) = (x2 - x1, y2 - y1);
	let frequency = 2.0 * std::f64::consts::PI / (dx / cycles);
	let mid_y = y1 + dy / 2.0;
	(0..=STEPS)
		.map(|step| {
			let x = x1 + step as f64 / STEPS as f64 * dx;
			point(x, mid_y + amplitude * (frequency * (x - x1)).sin())
		})
		.collect()
}

/// `generateArcPoints` of `bowTieRect.ts`: 20 points of the elliptic arc between two points.
fn arc_points(x1: f64, y1: f64, x2: f64, y2: f64, rx: f64, ry: f64, clockwise: bool) -> Vec<Point> {
	const COUNT: usize = 20;
	let (mid_x, mid_y) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
	let angle = (y2 - y1).atan2(x2 - x1);
	let (dx, dy) = ((x2 - x1) / 2.0, (y2 - y1) / 2.0);
	let distance = ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
	// Mermaid throws for radii too small for the chord; then the arc is a line.
	if distance.is_nan() || distance > 1.0 {
		return vec![point(x1, y1), point(x2, y2)];
	}
	let center_distance = (1.0 - distance * distance).sqrt();
	let sign = if clockwise { -1.0 } else { 1.0 };
	let center_x = mid_x + center_distance * ry * angle.sin() * sign;
	let center_y = mid_y - center_distance * rx * angle.cos() * sign;
	let start = ((y1 - center_y) / ry).atan2((x1 - center_x) / rx);
	let end = ((y2 - center_y) / ry).atan2((x2 - center_x) / rx);
	let mut range = end - start;
	if clockwise && range < 0.0 {
		range += 2.0 * std::f64::consts::PI;
	}
	if !clockwise && range > 0.0 {
		range -= 2.0 * std::f64::consts::PI;
	}
	(0..COUNT)
		.map(|index| {
			let angle = start + index as f64 / (COUNT - 1) as f64 * range;
			point(center_x + rx * angle.cos(), center_y + ry * angle.sin())
		})
		.collect()
}
