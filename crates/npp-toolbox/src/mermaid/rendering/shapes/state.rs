//! The shapes of state diagrams (`stateStart.ts`, `stateEnd.ts`, `forkJoin.ts`, `choice.ts`,
//! `note.ts`, `rectWithTitle.ts`). Mermaid draws most of them with roughjs, which writes a
//! filled and a stroked path; without roughness they are the plain outlines here.

use super::super::super::label::LabelBox;
use super::path_number as number;
use super::{Intersect, Part, Point, RoughPaint, Shape, centered_box, point, points_box};

/// The outline of a centered `width` × `height` rectangle.
fn rectangle_path(width: f64, height: f64) -> String {
	let (x, y) = (-width / 2.0, -height / 2.0);
	format!(
		"M{} {} L{} {} L{} {} L{} {} Z",
		number(x),
		number(y),
		number(x + width),
		number(y),
		number(x + width),
		number(y + height),
		number(x),
		number(y + height)
	)
}

/// The outline of a circle of `radius` around the origin.
fn circle_path(radius: f64) -> String {
	let r = number(radius);
	let minus = number(-radius);
	format!("M{r},0 A{r},{r} 0 1,1 {minus},0 A{r},{r} 0 1,1 {r},0 Z")
}

/// The box `getBBox` gives the end state: roughjs closes its circle with a curve whose control
/// points lie outside, at x = 7.0177; the curve reaches x = 7 + 0.75 · 0.0177.
fn end_box() -> super::BBox {
	let mut bbox = centered_box(14.0, 14.0);
	bbox.include_point(point(7.0 + 0.75 * 0.017_724_288_152_425, 0.0));
	bbox
}

/// The state shapes by registry name; `None` for other names.
pub fn shape(name: &str, label: &LabelBox, padding: f64, is_horizontal: bool) -> Option<Shape> {
	const SPECIAL_SIZE: f64 = 14.0;
	let shape = match name {
		"stateStart" | "sm-circ" | "start" | "small-circle" => Shape {
			parts: vec![Part::StateStart { radius: SPECIAL_SIZE / 2.0 }],
			bbox: centered_box(SPECIAL_SIZE, SPECIAL_SIZE),
			intersect: Intersect::NodeCircle,
			label_center: Point::default(),
		},
		"stateEnd" | "fr-circ" | "stop" | "framed-circle" => {
			let inner = Part::Rough {
				class: "",
				d: circle_path(SPECIAL_SIZE * 5.0 / 14.0 / 2.0),
				paint: RoughPaint::EndInner,
				inner: Vec::new(),
			};
			Shape {
				parts: vec![Part::Rough {
					class: "outer-path",
					d: circle_path(SPECIAL_SIZE / 2.0),
					paint: RoughPaint::EndOuter,
					inner: vec![inner],
				}],
				bbox: end_box(),
				intersect: Intersect::NodeCircle,
				label_center: Point::default(),
			}
		}
		"fork" | "join" | "forkJoin" => {
			let (width, height) = if is_horizontal { (10.0, 70.0) } else { (70.0, 10.0) };
			Shape {
				parts: vec![Part::Rough {
					class: "",
					d: rectangle_path(width, height),
					paint: RoughPaint::ForkJoin,
					inner: Vec::new(),
				}],
				bbox: centered_box(width, height),
				intersect: Intersect::Rect,
				label_center: Point::default(),
			}
		}
		"choice" => {
			const SIZE: f64 = 28.0;
			let points = vec![
				point(0.0, SIZE / 2.0),
				point(SIZE / 2.0, 0.0),
				point(0.0, -SIZE / 2.0),
				point(-SIZE / 2.0, 0.0),
			];
			let d: Vec<String> = points
				.iter()
				.enumerate()
				.map(|(index, p)| {
					format!("{}{},{}", if index == 0 { 'M' } else { 'L' }, number(p.x), number(p.y))
				})
				.collect();
			Shape {
				bbox: points_box(&points, Point::default()),
				parts: vec![Part::Rough {
					class: "",
					d: d.join(" ") + " Z",
					paint: RoughPaint::Choice,
					inner: Vec::new(),
				}],
				intersect: Intersect::Polygon(points),
				label_center: Point::default(),
			}
		}
		"note" => {
			let width = label.width + padding * 2.0;
			let height = label.height + padding * 2.0;
			Shape {
				parts: vec![Part::Rough {
					class: "basic label-container outer-path",
					d: rectangle_path(width, height),
					paint: RoughPaint::Note,
					inner: Vec::new(),
				}],
				bbox: centered_box(width, height),
				intersect: Intersect::Rect,
				label_center: Point::default(),
			}
		}
		_ => return None,
	};
	Some(shape)
}

/// `rectWithTitle`: the title above a line and the descriptions below it. Returns the shape,
/// whose label is the title, and the center of the descriptions.
pub fn rect_with_title(title: &LabelBox, description: &LabelBox, padding: f64) -> (Shape, Point) {
	const RADIUS: f64 = 5.0;
	let half_padding = padding / 2.0;
	let (title_width, title_height) = (title.width, title.height);
	let (description_width, description_height) = (description.width, description.height);
	let width = title_width.max(description_width);
	let height = title_height + half_padding + 5.0 + description_height;
	// The label group's top left corner; the narrower label is centered in it.
	let left = -width / 2.0;
	let top = -height / 2.0 - half_padding + 3.0;
	let title_x =
		if description_width < title_width { 0.0 } else { (description_width - title_width) / 2.0 };
	let description_x =
		if description_width > title_width { 0.0 } else { (title_width - description_width) / 2.0 };
	let title_center = point(left + title_x + title_width / 2.0, top + title_height / 2.0);
	let description_center = point(
		left + description_x + description_width / 2.0,
		top + title_height + half_padding + 5.0 + description_height / 2.0,
	);
	let (x, y) = (-width / 2.0 - half_padding, -height / 2.0 - half_padding);
	let line_y = -height / 2.0 - half_padding + title_height + half_padding;
	let shape = Shape {
		parts: vec![Part::Group {
			parts: vec![
				Part::Rect {
					x,
					y,
					width: width + padding,
					height: height + padding,
					rx: Some(RADIUS),
					class: "outer title-state",
				},
				Part::Line {
					from: point(x, line_y),
					to: point(width / 2.0 + half_padding, line_y),
					class: "divider",
				},
			],
			class: "",
		}],
		bbox: centered_box(width + padding, height + padding),
		intersect: Intersect::Rect,
		label_center: title_center,
	};
	(shape, description_center)
}
