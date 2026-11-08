//! The node shapes of `rendering-elements/shapes/*`: their size around the label, their
//! elements and where edges meet them.

pub mod blink_path;
pub mod class;
pub mod er;
mod requirement;
mod state;
mod symbols;
pub mod usecase;

use std::cell::Cell;

use super::super::icons;
use super::super::label::LabelBox;
use super::super::svg::{Anchor, Baseline, Svg, TextRun, number};
use super::super::text::Font;
use super::super::{Look, color};
use super::intersect::{self, Bounds};
use super::sketch::SketchPart;
use super::{BBox, DiagramStyle, Node, NodeIcon, NodeImage, Point, Styles, Theme};

pub use class::class_box;
pub use er::er_box;
pub use requirement::requirement_box;
pub use state::rect_with_title;

/// The padding of the label paragraph of images and icons (`.image-shape p`).
pub const ASSET_LABEL_PADDING: f64 = 2.0;

/// Short names, aliases and internal aliases of Mermaid's shape registry.
const SHAPE_NAMES: &[&str] = &[
	"rect",
	"proc",
	"process",
	"rectangle",
	"squareRect",
	"rounded",
	"event",
	"roundedRect",
	"stadium",
	"terminal",
	"pill",
	"fr-rect",
	"subprocess",
	"subproc",
	"framed-rectangle",
	"subroutine",
	"cyl",
	"db",
	"database",
	"cylinder",
	"datastore",
	"data-store",
	"folder",
	"directory",
	"bucket",
	"console",
	"browser",
	"person",
	"circle",
	"circ",
	"bang",
	"cloud",
	"diam",
	"decision",
	"diamond",
	"question",
	"hex",
	"hexagon",
	"prepare",
	"lean-r",
	"lean-right",
	"in-out",
	"lean_right",
	"lean-l",
	"lean-left",
	"out-in",
	"lean_left",
	"trap-b",
	"priority",
	"trapezoid-bottom",
	"trapezoid",
	"trap-t",
	"manual",
	"trapezoid-top",
	"inv-trapezoid",
	"inv_trapezoid",
	"dbl-circ",
	"double-circle",
	"doublecircle",
	"text",
	"notch-rect",
	"card",
	"notched-rectangle",
	"lin-rect",
	"lined-rectangle",
	"lined-process",
	"lin-proc",
	"shaded-process",
	"sm-circ",
	"start",
	"small-circle",
	"stateStart",
	"fr-circ",
	"stop",
	"framed-circle",
	"stateEnd",
	"fork",
	"join",
	"forkJoin",
	"hourglass",
	"collate",
	"brace",
	"comment",
	"brace-l",
	"brace-r",
	"braces",
	"bolt",
	"com-link",
	"lightning-bolt",
	"doc",
	"document",
	"delay",
	"half-rounded-rectangle",
	"h-cyl",
	"das",
	"horizontal-cylinder",
	"lin-cyl",
	"disk",
	"lined-cylinder",
	"curv-trap",
	"curved-trapezoid",
	"display",
	"div-rect",
	"div-proc",
	"divided-rectangle",
	"divided-process",
	"tri",
	"extract",
	"triangle",
	"win-pane",
	"internal-storage",
	"window-pane",
	"f-circ",
	"junction",
	"filled-circle",
	"notch-pent",
	"loop-limit",
	"notched-pentagon",
	"flip-tri",
	"manual-file",
	"flipped-triangle",
	"sl-rect",
	"manual-input",
	"sloped-rectangle",
	"docs",
	"documents",
	"st-doc",
	"stacked-document",
	"st-rect",
	"procs",
	"processes",
	"stacked-rectangle",
	"bow-rect",
	"stored-data",
	"bow-tie-rectangle",
	"cross-circ",
	"summary",
	"crossed-circle",
	"tag-doc",
	"tagged-document",
	"tag-rect",
	"tag-proc",
	"tagged-rectangle",
	"tagged-process",
	"flag",
	"paper-tape",
	"odd",
	"rect_left_inv_arrow",
	"lin-doc",
	"lined-document",
	"icon",
	"iconSquare",
	"iconCircle",
	"iconRounded",
	"imageSquare",
	"labelRect",
];

/// `isValidShape`.
pub fn is_shape_name(name: &str) -> bool {
	SHAPE_NAMES.contains(&name)
}

/// A shape element. The first one of a shape gets the shape's style.
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
	Rect {
		x: f64,
		y: f64,
		width: f64,
		height: f64,
		rx: Option<f64>,
		class: &'static str,
	},
	Circle {
		center: Point,
		radius: f64,
		class: &'static str,
	},
	/// `insertPolygonShape`: the points moved by `offset`.
	Polygon {
		points: Vec<Point>,
		offset: Point,
		class: &'static str,
	},
	Path {
		d: String,
		offset: Point,
		class: &'static str,
	},
	/// The rim of a bucket: unfilled in the border color, unless the node has a style.
	Rim {
		center: Point,
		rx: f64,
		ry: f64,
	},
	Group {
		parts: Vec<Part>,
		class: &'static str,
	},
	/// A detail in the accent color (`nodeBorder`) with its own style instead of the node's.
	Accent(Accent),
	/// The frame of an image, painted like the hand-drawn shapes: no style sheet rules apply.
	ImageFrame {
		x: f64,
		y: f64,
		width: f64,
		height: f64,
	},
	Image {
		href: String,
		x: f64,
		y: f64,
		width: f64,
		height: f64,
	},
	/// A collapsed subgraph: its box, the dashed line below the title and three dots.
	CollapsedGroup {
		width: f64,
		height: f64,
		separator_y: f64,
		/// An agentflow `flow` container, drawn like its expanded frame.
		is_flow: bool,
	},
	/// The dashed line and the dots of a collapsed subgraph in the `handDrawn` look, where the
	/// rules of `.node` miss them.
	CollapsedMarks {
		width: f64,
		separator_y: f64,
		is_flow: bool,
	},
	/// The background of an icon, painted like the hand-drawn shapes, moved down by `offset_y`.
	IconBackground {
		d: String,
		offset_y: f64,
	},
	/// The area of a node with an icon, which takes the clicks, and the size and vertical
	/// offset of the icon's background.
	IconArea {
		width: f64,
		height: f64,
		background_size: f64,
		background_y: f64,
	},
	/// An icon with the position of its nested `<svg>`.
	Icon {
		icon: NodeIcon,
		x: f64,
		y: f64,
	},
	/// The background of the label of images (`.image-shape p`).
	LabelBackground {
		x: f64,
		y: f64,
		width: f64,
		height: f64,
	},
	Line {
		from: Point,
		to: Point,
		class: &'static str,
	},
	/// A roughjs shape without roughness: a group of its filled and its stroked outline, with
	/// the shapes nested in it.
	Rough {
		class: &'static str,
		d: String,
		paint: RoughPaint,
		inner: Vec<Part>,
	},
	/// The filled circle of a start state.
	StateStart {
		radius: f64,
	},
	/// The rectangle of a composite block.
	Composite {
		x: f64,
		y: f64,
		width: f64,
		height: f64,
	},
	/// The polygon of a block arrow: `insertPolygonShape` moved by `offset`.
	BlockArrow {
		points: Vec<Point>,
		offset: Point,
	},
	/// A part of the entity box of ER diagrams.
	Er(er::ErPart),
	Class(class::ClassPart),
	Usecase(usecase::UsecasePart),
	/// A roughjs call of the `handDrawn` look.
	Sketch(Box<SketchPart>),
	/// The parts of an entity drawn without roughness below its hand-drawn box, as a node of
	/// its own: they hide the edge markers under it.
	ErBackground(Vec<Part>),
}

/// The roughjs options of a shape: where its fill and stroke come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoughPaint {
	ForkJoin,
	Choice,
	Note,
	EndOuter,
	EndInner,
}

/// The details of the console and browser symbols.
#[derive(Clone, Debug, PartialEq)]
pub enum Accent {
	Line {
		from: Point,
		to: Point,
	},
	Dot {
		center: Point,
		radius: f64,
	},
	AddressBar {
		x: f64,
		y: f64,
		width: f64,
		height: f64,
	},
	/// The prompt of the console, a bold monospace text at its baseline start.
	Prompt {
		x: f64,
		y: f64,
	},
}

/// Where edges end on a shape.
#[derive(Clone, Debug, PartialEq)]
pub enum Intersect {
	Rect,
	Circle(f64),
	/// A circle as wide as the measured node: `stateStart`, `stateEnd`.
	NodeCircle,
	/// The diamond of `question`, as wide as the measured node.
	Diamond,
	Ellipse {
		rx: f64,
		ry: f64,
	},
	/// Points relative to the top left corner of the box.
	Polygon(Vec<Point>),
	/// The rectangle with its elliptic caps.
	Cylinder {
		rx: f64,
		ry: f64,
	},
	/// The same lying on its side.
	HorizontalCylinder {
		rx: f64,
		ry: f64,
	},
}

impl Intersect {
	pub fn point(&self, node: Bounds, point: Point) -> Point {
		match self {
			Intersect::Rect => intersect::rect(node, point),
			Intersect::Circle(radius) => intersect::circle(node, *radius, point),
			Intersect::NodeCircle => intersect::circle(node, node.width / 2.0, point),
			Intersect::Diamond => {
				let s = node.width;
				let corners = [(s / 2.0, 0.0), (s, -s / 2.0), (s / 2.0, -s), (0.0, -s / 2.0)];
				let corners = corners.map(|(x, y)| Point { x, y });
				intersect::polygon(node, &corners, point)
			}
			&Intersect::Ellipse { rx, ry } => intersect::ellipse(node, rx, ry, point),
			Intersect::Polygon(points) => intersect::polygon(node, points, point),
			&Intersect::Cylinder { rx, ry } => {
				let mut position = intersect::rect(node, point);
				let x = position.x - node.x;
				let is_on_cap = (x.abs() < node.width / 2.0)
					|| (x.abs() == node.width / 2.0
						&& (position.y - node.y).abs() > node.height / 2.0 - ry);
				if rx != 0.0 && is_on_cap {
					let mut y = ry * ry * (1.0 - x * x / (rx * rx));
					if y > 0.0 {
						y = y.sqrt();
					}
					y = ry - y;
					if point.y - node.y > 0.0 {
						y = -y;
					}
					position.y += y;
				}
				position
			}
			&Intersect::HorizontalCylinder { rx, ry } => {
				let mut position = intersect::rect(node, point);
				let y = position.y - node.y;
				let is_on_cap = (y.abs() < node.height / 2.0)
					|| (y.abs() == node.height / 2.0
						&& (position.x - node.x).abs() > node.width / 2.0 - rx);
				if ry != 0.0 && is_on_cap {
					let mut x = rx * rx * (1.0 - y * y / (ry * ry));
					if x != 0.0 {
						x = x.abs().sqrt();
					}
					x = rx - x;
					if point.x - node.x > 0.0 {
						x = -x;
					}
					position.x += x;
				}
				position
			}
		}
	}
}

/// A shape around its label, centered on the node.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
	pub parts: Vec<Part>,
	/// The box of the elements around the node center.
	pub bbox: BBox,
	pub intersect: Intersect,
	/// Center of the label relative to the node center.
	pub label_center: Point,
}

/// Shapes that drop their label.
pub fn has_no_label(shape: &str) -> bool {
	matches!(
		shape,
		"sm-circ"
			| "start" | "small-circle"
			| "stateStart"
			| "fr-circ"
			| "stop" | "framed-circle"
			| "stateEnd"
			| "fork" | "join"
			| "forkJoin"
			| "hourglass"
			| "collate"
			| "bolt" | "com-link"
			| "lightning-bolt"
			| "f-circ"
			| "junction"
			| "filled-circle"
			| "cross-circ"
			| "summary"
			| "crossed-circle"
			| "choice"
	)
}

/// Shapes that size from both axes and ignore `minNodeWidth`.
pub fn ignores_min_width(shape: &str) -> bool {
	matches!(shape, "circle" | "circ" | "dbl-circ" | "double-circle" | "doublecircle")
}

fn centered_box(width: f64, height: f64) -> BBox {
	let mut bbox = BBox::default();
	bbox.include_rect(-width / 2.0, -height / 2.0, width, height);
	bbox
}

fn points_box(points: &[Point], offset: Point) -> BBox {
	let mut bbox = BBox::default();
	for point in points {
		bbox.include_point(Point { x: point.x + offset.x, y: point.y + offset.y });
	}
	bbox
}

fn point(x: f64, y: f64) -> Point {
	Point { x, y }
}

/// `insertPolygonShape` of a `w` × `h` polygon.
fn polygon_shape(points: Vec<Point>, w: f64, h: f64, class: &'static str) -> Shape {
	let offset = point(-w / 2.0, h / 2.0);
	Shape {
		bbox: points_box(&points, offset),
		intersect: Intersect::Polygon(points.clone()),
		parts: vec![Part::Polygon { points, offset, class }],
		label_center: Point::default(),
	}
}

// The numbers of path data are written with 3 decimals, except where a sketch reads them: the
// numbers roughjs gets decide its picture and the size of the node.
thread_local! {
	// Clippy 1.99 reports the initializer although it is `const` already.
	#[allow(clippy::missing_const_for_thread_local)]
	static HAS_FULL_PRECISION: Cell<bool> = const { Cell::new(false) };
}

/// A number of path data.
fn path_number(value: f64) -> String {
	match HAS_FULL_PRECISION.get() {
		true => color::js_number(value),
		false => number(value),
	}
}

/// The shape of `node` around its laid out `label`, in a graph that flows left or right if
/// `is_horizontal`.
pub fn shape(
	node: &Node,
	label: &LabelBox,
	radius: f64,
	is_horizontal: bool,
	style: DiagramStyle,
) -> Shape {
	HAS_FULL_PRECISION.set(node.look == Look::HandDrawn);
	let shape = classic_shape(node, label, radius, is_horizontal, style);
	HAS_FULL_PRECISION.set(false);
	shape
}

fn classic_shape(
	node: &Node,
	label: &LabelBox,
	radius: f64,
	is_horizontal: bool,
	style: DiagramStyle,
) -> Shape {
	let (bw, bh) = (label.width, label.height);
	let padding = node.padding;
	let is_neo = node.look == Look::Neo;
	// `node.width || 0`: the size block diagrams draw a shape in.
	let min_width = node.width.unwrap_or(0.0);
	let min_height = node.height.unwrap_or(0.0);
	// Class diagrams share the note of state diagrams.
	let has_state_shapes = match style {
		DiagramStyle::State => true,
		DiagramStyle::Class | DiagramStyle::Usecase => node.shape == "note",
		_ => false,
	};
	if has_state_shapes
		&& let Some(shape) = state::shape(&node.shape, label, padding, is_horizontal)
	{
		return shape;
	}
	match node.shape.as_str() {
		"rounded" | "event" | "roundedRect" => {
			rect((bw, bh), (padding, padding), (min_width, min_height), Some(radius))
		}
		"composite" => composite(node, bw, bh),
		"block_arrow" => block_arrow(node, bw, bh),
		"stadium" | "terminal" | "pill" => stadium(
			bw,
			bh,
			if is_neo { 40.0 } else { padding },
			if is_neo { 24.0 } else { padding },
		),
		"fr-rect" | "subprocess" | "subproc" | "framed-rectangle" | "subroutine" => subroutine(
			(bw, bh),
			if is_neo { 28.0 } else { padding },
			if is_neo { 12.0 } else { padding },
			(min_width, min_height),
		),
		"cyl" | "db" | "database" | "cylinder" => {
			let label_padding = if is_neo { 24.0 } else { padding };
			let (width, height) = cylinder_label_size(node, label_padding);
			cylinder(bw.max(width), bh.max(height), label_padding, padding)
		}
		"circle" | "circ" => {
			let radius =
				(bw * bw + bh * bh).sqrt() / 2.0 + if is_neo { 16.0 } else { padding / 2.0 };
			Shape {
				parts: vec![Part::Circle {
					center: Point::default(),
					radius,
					class: "basic label-container",
				}],
				bbox: centered_box(radius * 2.0, radius * 2.0),
				intersect: Intersect::Circle(radius),
				label_center: Point::default(),
			}
		}
		"dbl-circ" | "double-circle" | "doublecircle" => {
			let gap = if is_neo { 12.0 } else { 5.0 };
			let label_radius = match node.width.filter(|width| is_truthy(*width)) {
				Some(width) => width / 2.0,
				None => (bw * bw + bh * bh).sqrt() / 2.0,
			};
			let inner = label_radius + if is_neo { 16.0 } else { padding };
			let outer = inner + gap;
			Shape {
				parts: vec![Part::Group {
					parts: vec![
						Part::Circle {
							center: Point::default(),
							radius: outer,
							class: "outer-circle",
						},
						Part::Circle {
							center: Point::default(),
							radius: inner,
							class: "inner-circle",
						},
					],
					class: "basic label-container",
				}],
				bbox: centered_box(outer * 2.0, outer * 2.0),
				intersect: Intersect::Circle(outer),
				label_center: Point::default(),
			}
		}
		"diam" | "decision" | "diamond" | "question" => {
			let s = bw + padding + bh + padding;
			let points = vec![
				point(s / 2.0, 0.0),
				point(s, -s / 2.0),
				point(s / 2.0, -s),
				point(0.0, -s / 2.0),
			];
			let mut shape = polygon_shape(points, s, s, "label-container");
			// `translate(-s / 2 + 0.5, s / 2)`.
			let offset = point(-s / 2.0 + 0.5, s / 2.0);
			if let Some(Part::Polygon { offset: part_offset, .. }) = shape.parts.first_mut() {
				*part_offset = offset;
			}
			if let Part::Polygon { points, .. } = &shape.parts[0] {
				shape.bbox = points_box(points, offset);
			}
			shape.intersect = Intersect::Diamond;
			shape
		}
		"hex" | "hexagon" | "prepare" => {
			let f = if is_neo { 3.5 } else { 4.0 };
			let (padding_x, padding_y) = if is_neo { (70.0, 32.0) } else { (padding, padding) };
			let (label_width, label_height) = hexagon_label_size(node, padding_x, padding_y);
			let h = if is_truthy(label_height) { label_height } else { bh } + padding_x;
			let m = h / f;
			let w = if is_truthy(label_width) { label_width } else { bw } + 2.0 * m + padding_y;
			let points = vec![
				point(m, 0.0),
				point(w - m, 0.0),
				point(w, -h / 2.0),
				point(w - m, -h),
				point(m, -h),
				point(0.0, -h / 2.0),
			];
			polygon_shape(points, w, h, "label-container")
		}
		"lean-r" | "lean-right" | "in-out" | "lean_right" => {
			let h = (bh + padding).max(min_height);
			let w = (bw + if is_neo { padding * 2.0 } else { padding }).max(min_width - h);
			let points = vec![
				point(-3.0 * h / 6.0, 0.0),
				point(w, 0.0),
				point(w + 3.0 * h / 6.0, -h),
				point(0.0, -h),
			];
			polygon_shape(points, w, h, "label-container")
		}
		"lean-l" | "lean-left" | "out-in" | "lean_left" => {
			let h = (bh + padding).max(min_height);
			let w = (bw + if is_neo { padding * 2.0 } else { padding }).max(min_width - h);
			let points = vec![
				point(0.0, 0.0),
				point(w + 3.0 * h / 6.0, 0.0),
				point(w, -h),
				point(-3.0 * h / 6.0, -h),
			];
			polygon_shape(points, w, h, "label-container")
		}
		"trap-b" | "priority" | "trapezoid-bottom" | "trapezoid" => {
			let h = (bh + padding).max(min_height);
			let w = (bw + if is_neo { padding * 2.0 } else { padding }).max(min_width - h);
			let points = vec![
				point(-3.0 * h / 6.0, 0.0),
				point(w + 3.0 * h / 6.0, 0.0),
				point(w, -h),
				point(0.0, -h),
			];
			polygon_shape(points, w, h, "label-container")
		}
		"trap-t" | "manual" | "trapezoid-top" | "inv-trapezoid" | "inv_trapezoid" => {
			// Unlike the other trapezoids, both paddings count twice.
			let h = (bh + padding * 2.0).max(min_height);
			let w = (bw + if is_neo { padding * 4.0 } else { padding * 2.0 }).max(min_width - h);
			let points = vec![
				point(0.0, 0.0),
				point(w, 0.0),
				point(w + 3.0 * h / 6.0, -h),
				point(-3.0 * h / 6.0, -h),
			];
			polygon_shape(points, w, h, "label-container")
		}
		"odd" | "rect_left_inv_arrow" => {
			let (padding_x, padding_y) =
				if is_neo { (21.0 * 2.0, 12.0 * 2.0) } else { (padding, padding) };
			let h = (bh + padding_y).max(min_height);
			let w = (bw + padding_x).max(min_width - h / 4.0);
			let (x, y) = (-w / 2.0, -h / 2.0);
			let notch = y / 2.0;
			let points = vec![
				point(x + notch, y),
				point(x, 0.0),
				point(x + notch, -y),
				point(-x, -y),
				point(-x, y),
			];
			let offset = point(-notch / 2.0, 0.0);
			Shape {
				bbox: points_box(&points, offset),
				intersect: Intersect::Polygon(points.clone()),
				parts: vec![Part::Path {
					d: closed_path(&points),
					offset,
					class: "basic label-container outer-path",
				}],
				label_center: point(-notch / 2.0, 0.0),
			}
		}
		"text" => {
			let (w, h) = (bw + padding, bh + padding);
			Shape {
				parts: vec![Part::Rect {
					x: -w / 2.0,
					y: -h / 2.0,
					width: w,
					height: h,
					rx: Some(0.0),
					class: "text",
				}],
				bbox: centered_box(w, h),
				intersect: Intersect::Rect,
				label_center: Point::default(),
			}
		}
		"labelRect" => Shape {
			parts: Vec::new(),
			bbox: centered_box(bw.max(10.0), bh.max(10.0)),
			intersect: Intersect::Rect,
			label_center: Point::default(),
		},
		"imageSquare" => image_square(node.image.as_ref(), label),
		"icon" | "iconSquare" | "iconCircle" | "iconRounded" => {
			icon_shape(&node.shape, node.icon.as_ref(), label, node.padding / 2.0)
		}
		"collapsedGroup" => {
			const INDICATOR_ROW_HEIGHT: f64 = 20.0;
			const SEPARATOR_GAP: f64 = 8.0;
			let width = (bw + padding * 2.0).max(80.0);
			let height = bh + SEPARATOR_GAP + INDICATOR_ROW_HEIGHT + padding * 2.0;
			Shape {
				parts: vec![Part::CollapsedGroup {
					width,
					height,
					separator_y: -height / 2.0 + padding + bh + SEPARATOR_GAP,
					is_flow: style == DiagramStyle::Agentflow,
				}],
				bbox: centered_box(width, height),
				intersect: Intersect::Rect,
				label_center: point(0.0, -(SEPARATOR_GAP + INDICATOR_ROW_HEIGHT) / 2.0),
			}
		}
		name if let Some(shape) = symbols::shape(
			name,
			(bw, bh),
			padding,
			is_neo,
			is_horizontal,
			(min_width, min_height),
		) =>
		{
			shape
		}
		// `squareRect`, and the shapes still to come.
		_ => {
			let padding_x = if is_neo { 16.0 } else { padding * 2.0 };
			let padding_y = if is_neo { 12.0 } else { padding };
			rect((bw, bh), (padding_x, padding_y), (min_width, min_height), None)
		}
	}
}

/// A number JavaScript takes as true.
fn is_truthy(value: f64) -> bool {
	value != 0.0 && !value.is_nan()
}

/// `hexagon` shrinks a given node size by its paddings before it places the label.
fn hexagon_label_size(node: &Node, padding_x: f64, padding_y: f64) -> (f64, f64) {
	let (width, height) = (node.width.unwrap_or(0.0), node.height.unwrap_or(0.0));
	if !is_truthy(width) && !is_truthy(height) {
		return (0.0, 0.0);
	}
	let f = if node.look == Look::Neo { 3.5 } else { 4.0 };
	let m = height / f;
	(width - 2.0 * m - padding_y, height - padding_x)
}

/// `cylinder` shrinks a given node size by its paddings and caps, to at least 8 pixels.
fn cylinder_label_size(node: &Node, label_padding: f64) -> (f64, f64) {
	const MINIMUM: f64 = 8.0;
	let original_width = node.width.unwrap_or(0.0);
	let mut width = original_width;
	let mut height = node.height.unwrap_or(0.0);
	if is_truthy(width) {
		width -= label_padding;
		if width < MINIMUM {
			width = MINIMUM;
		}
	}
	if is_truthy(height) {
		let rx = original_width / 2.0;
		let ry = rx / (2.5 + original_width / 50.0);
		height = height - label_padding - ry * 3.0;
		if height < MINIMUM {
			height = MINIMUM;
		}
	}
	(width, height)
}

/// The width the label of `node` wraps at where the node has a size; shapes that shrink the
/// size before they place the label wrap at the shrunk width.
pub fn label_width(node: &Node) -> Option<f64> {
	let is_neo = node.look == Look::Neo;
	let width = match node.shape.as_str() {
		"hex" | "hexagon" | "prepare" => {
			let (padding_x, padding_y) =
				if is_neo { (70.0, 32.0) } else { (node.padding, node.padding) };
			hexagon_label_size(node, padding_x, padding_y).0
		}
		"cyl" | "db" | "database" | "cylinder" => {
			cylinder_label_size(node, if is_neo { 24.0 } else { node.padding }).0
		}
		"h-cyl" | "das" | "horizontal-cylinder" => {
			let label_padding = if is_neo { 12.0 } else { node.padding / 2.0 };
			let size = (node.width.unwrap_or(0.0), node.height.unwrap_or(0.0));
			symbols::horizontal_cylinder_label_size(size, label_padding).0
		}
		_ => node.width.unwrap_or(0.0),
	};
	is_truthy(width).then_some(width)
}

/// `composite`: the rectangle of a composite block, around its label or in its laid out size.
fn composite(node: &Node, bw: f64, bh: f64) -> Shape {
	let padding = node.padding;
	let (width, height, x, y) = match node.is_positioned {
		true => {
			let (width, height) = (node.width.unwrap_or(0.0), node.height.unwrap_or(0.0));
			(width, height, -width / 2.0, -height / 2.0)
		}
		false => (bw + padding, bh + padding, -bw / 2.0 - padding / 2.0, -bh / 2.0 - padding / 2.0),
	};
	let mut bbox = BBox::default();
	// Blink's `getBBox` skips empty boxes.
	if width > 0.0 || height > 0.0 {
		bbox.include_rect(x, y, width, height);
	}
	Shape {
		parts: vec![Part::Composite { x, y, width, height }],
		bbox,
		intersect: Intersect::Rect,
		label_center: Point::default(),
	}
}

/// `getArrowPoints`: the outline of a block arrow pointing to `directions`.
fn arrow_points(directions: &[String], height: f64, padding: f64, width: f64) -> Vec<Point> {
	let has = |name: &str| {
		directions.iter().any(|direction| match direction.as_str() {
			"x" => name == "right" || name == "left",
			"y" => name == "up" || name == "down",
			other => other == name,
		})
	};
	let (h, mid, p, w) = (height, height / 2.0, padding, width);
	let points: &[(f64, f64)] = match (has("right"), has("left"), has("up"), has("down")) {
		(true, true, true, true) => &[
			(0.0, 0.0),
			(mid, 0.0),
			(w / 2.0, 2.0 * p),
			(w - mid, 0.0),
			(w, 0.0),
			(w, -h / 3.0),
			(w + 2.0 * p, -h / 2.0),
			(w, -2.0 * h / 3.0),
			(w, -h),
			(w - mid, -h),
			(w / 2.0, -h - 2.0 * p),
			(mid, -h),
			(0.0, -h),
			(0.0, -2.0 * h / 3.0),
			(-2.0 * p, -h / 2.0),
			(0.0, -h / 3.0),
		],
		(true, true, true, false) => {
			&[(mid, 0.0), (w - mid, 0.0), (w, -h / 2.0), (w - mid, -h), (mid, -h), (0.0, -h / 2.0)]
		}
		(true, true, false, true) => &[(0.0, 0.0), (mid, -h), (w - mid, -h), (w, 0.0)],
		(true, false, true, true) => &[(0.0, 0.0), (w, -mid), (w, -h + mid), (0.0, -h)],
		(false, true, true, true) => &[(w, 0.0), (0.0, -mid), (0.0, -h + mid), (w, -h)],
		(true, true, false, false) => &[
			(mid, 0.0),
			(mid, -p),
			(w - mid, -p),
			(w - mid, 0.0),
			(w, -h / 2.0),
			(w - mid, -h),
			(w - mid, -h + p),
			(mid, -h + p),
			(mid, -h),
			(0.0, -h / 2.0),
		],
		(false, false, true, true) => &[
			(w / 2.0, 0.0),
			(0.0, -p),
			(mid, -p),
			(mid, -h + p),
			(0.0, -h + p),
			(w / 2.0, -h),
			(w, -h + p),
			(w - mid, -h + p),
			(w - mid, -p),
			(w, -p),
		],
		(true, false, true, false) => &[(0.0, 0.0), (w, -mid), (0.0, -h)],
		(true, false, false, true) => &[(0.0, 0.0), (w, 0.0), (0.0, -h)],
		(false, true, true, false) => &[(w, 0.0), (0.0, -mid), (w, -h)],
		(false, true, false, true) => &[(w, 0.0), (0.0, 0.0), (w, -h)],
		(true, false, false, false) => &[
			(mid, -p),
			(mid, -p),
			(w - mid, -p),
			(w - mid, 0.0),
			(w, -h / 2.0),
			(w - mid, -h),
			(w - mid, -h + p),
			(mid, -h + p),
			(mid, -h + p),
		],
		(false, true, false, false) => &[
			(mid, 0.0),
			(mid, -p),
			(w - mid, -p),
			(w - mid, -h + p),
			(mid, -h + p),
			(mid, -h),
			(0.0, -h / 2.0),
		],
		(false, false, true, false) => &[
			(mid, -p),
			(mid, -h + p),
			(0.0, -h + p),
			(w / 2.0, -h),
			(w, -h + p),
			(w - mid, -h + p),
			(w - mid, -p),
		],
		(false, false, false, true) => &[
			(w / 2.0, 0.0),
			(0.0, -p),
			(mid, -p),
			(mid, -h + p),
			(w - mid, -h + p),
			(w - mid, -p),
			(w, -p),
		],
		(false, false, false, false) => &[(0.0, 0.0)],
	};
	points.iter().map(|&(x, y)| point(x, y)).collect()
}

/// `block_arrow`: an arrow around the label; spanning blocks stretch it to their width.
fn block_arrow(node: &Node, bw: f64, bh: f64) -> Shape {
	let padding = node.padding;
	let height = bh + 2.0 * padding;
	let midpoint = height / 2.0;
	let natural_width = bw + 2.0 * midpoint + padding;
	let node_width = node.width.unwrap_or(0.0);
	let is_spanning =
		node.is_positioned && node.width_in_columns > 1.0 && node_width > natural_width;
	let width = if is_spanning { node_width } else { natural_width };
	let points = arrow_points(&node.directions, height, padding / 2.0, width);
	let mut shape = polygon_shape(points, width, height, "label-container");
	if let Some(Part::Polygon { points, offset, .. }) = shape.parts.pop() {
		shape.parts.push(Part::BlockArrow { points, offset });
	}
	shape
}

/// `imageSquare`: the image in its frame, with the label on a background below or above it.
fn image_square(image: Option<&NodeImage>, label: &LabelBox) -> Shape {
	let image = image.cloned().unwrap_or_default();
	let (iw, ih) = (image.width, image.height);
	let has_label = !label.is_empty();
	let (lw, lh, gap) = match has_label {
		true => {
			(label.width + ASSET_LABEL_PADDING * 2.0, label.height + ASSET_LABEL_PADDING * 2.0, 8.0)
		}
		false => (0.0, 0.0, 0.0),
	};
	let outer_width = iw.max(lw);
	let outer_height = ih + lh + gap;
	let is_on_top = image.is_label_on_top;
	let frame_y = -ih / 2.0 + if is_on_top { lh / 2.0 + gap / 2.0 } else { -lh / 2.0 - gap / 2.0 };
	let image_y = if is_on_top { outer_height / 2.0 - ih } else { -outer_height / 2.0 };
	let label_y = if is_on_top { -ih / 2.0 - gap / 2.0 } else { ih / 2.0 + gap / 2.0 };
	let mut parts = vec![Part::ImageFrame { x: -iw / 2.0, y: frame_y, width: iw, height: ih }];
	if has_label {
		parts.push(Part::LabelBackground {
			x: -lw / 2.0,
			y: label_y - lh / 2.0,
			width: lw,
			height: lh,
		});
	}
	if !image.href.is_empty() {
		parts.push(Part::Image {
			href: image.href,
			x: -iw / 2.0,
			y: image_y,
			width: iw,
			height: ih,
		});
	}
	let (top, bottom) = (-outer_height / 2.0, outer_height / 2.0);
	let intersect = match (has_label, is_on_top) {
		(false, _) => Intersect::Rect,
		(true, true) => Intersect::Polygon(vec![
			point(-lw / 2.0, top),
			point(lw / 2.0, top),
			point(lw / 2.0, top + lh + gap),
			point(iw / 2.0, top + lh + gap),
			point(iw / 2.0, bottom),
			point(-iw / 2.0, bottom),
			point(-iw / 2.0, top + lh + gap),
			point(-lw / 2.0, top + lh + gap),
		]),
		// Mermaid halves the label width twice for the bottom right corner.
		(true, false) => Intersect::Polygon(vec![
			point(-iw / 2.0, top),
			point(iw / 2.0, top),
			point(iw / 2.0, top + ih),
			point(lw / 2.0, top + ih),
			point(lw / 4.0, bottom),
			point(-lw / 2.0, bottom),
			point(-lw / 2.0, top + ih),
			point(-iw / 2.0, top + ih),
		]),
	};
	Shape {
		parts,
		bbox: centered_box(outer_width, outer_height),
		intersect,
		label_center: point(0.0, label_y),
	}
}

/// The outline of `createRoundedRectPathD` around the center.
fn rounded_rect_path(width: f64, height: f64, radius: f64) -> String {
	let (x, y) = (-width / 2.0, -height / 2.0);
	let (r, n) = (number(radius), number);
	let arc = |to_x: f64, to_y: f64| format!("A{r} {r} 0 0 1 {} {}", n(to_x), n(to_y));
	format!(
		"M{} {} H{} {} V{} {} H{} {} V{} {} Z",
		n(x + radius),
		n(y),
		n(x + width - radius),
		arc(x + width, y + radius),
		n(y + height - radius),
		arc(x + width - radius, y + height),
		n(x + radius),
		arc(x, y + height - radius),
		n(y + radius),
		arc(x + radius, y),
	)
}

/// `icon`, `iconSquare`, `iconCircle` and `iconRounded`: the icon on its background, with the
/// label on a background below or above it.
fn icon_shape(name: &str, icon: Option<&NodeIcon>, label: &LabelBox, half_padding: f64) -> Shape {
	const CIRCLE_PADDING: f64 = 20.0;
	let icon = icon.cloned().unwrap_or_default();
	let has_label = !label.is_empty();
	let (lw, lh, gap) = match has_label {
		true => {
			(label.width + ASSET_LABEL_PADDING * 2.0, label.height + ASSET_LABEL_PADDING * 2.0, 8.0)
		}
		false => (0.0, 0.0, 0.0),
	};
	// `getBBox` of the icon in its nested `<svg>`; the unknown icon fills it.
	let icon_box = match &icon.glyph {
		Some(glyph) => glyph.box_in_square(icon.size),
		None => icons::IconBox { x: 0.0, y: 0.0, width: icon.size, height: icon.size },
	};
	let square = icon.size + half_padding * 2.0;
	let diameter =
		icon_box.width.max(icon_box.height) * std::f64::consts::SQRT_2 + CIRCLE_PADDING * 2.0;
	let (width, height, background) = match name {
		"iconSquare" => (square, square, Some(rounded_rect_path(square, square, 0.1))),
		"iconRounded" => (square, square, Some(rounded_rect_path(square, square, 5.0))),
		"iconCircle" => {
			let radius = number(diameter / 2.0);
			let minus = number(-diameter / 2.0);
			let d = format!(
				"M{radius} 0 A{radius} {radius} 0 1 1 {minus} 0 A{radius} {radius} 0 1 1 {radius} 0 Z"
			);
			(diameter, diameter, Some(d))
		}
		_ => (icon.size, icon.size, None),
	};
	let is_on_top = icon.is_label_on_top;
	let outer_width = width.max(lw);
	let outer_height = height + lh + gap;
	let center_y = if is_on_top { lh / 2.0 + gap / 2.0 } else { -lh / 2.0 - gap / 2.0 };
	let label_y = if is_on_top { -height / 2.0 - gap / 2.0 } else { height / 2.0 + gap / 2.0 };
	let mut parts = Vec::new();
	if let Some(d) = background {
		parts.push(Part::IconBackground { d, offset_y: center_y });
	}
	if has_label {
		parts.push(Part::LabelBackground {
			x: -lw / 2.0,
			y: label_y - lh / 2.0,
			width: lw,
			height: lh,
		});
	}
	parts.push(Part::IconArea {
		width: outer_width,
		height: outer_height,
		background_size: width,
		background_y: center_y,
	});
	parts.push(Part::Icon {
		x: -icon_box.width / 2.0 - icon_box.x,
		y: center_y - icon_box.height / 2.0 - icon_box.y,
		icon,
	});
	let (top, bottom) = (-outer_height / 2.0, outer_height / 2.0);
	let intersect = match (has_label && name != "iconCircle", is_on_top) {
		(false, _) => Intersect::Rect,
		(true, true) => Intersect::Polygon(vec![
			point(-lw / 2.0, top),
			point(lw / 2.0, top),
			point(lw / 2.0, top + lh + gap),
			point(width / 2.0, top + lh + gap),
			point(width / 2.0, bottom),
			point(-width / 2.0, bottom),
			point(-width / 2.0, top + lh + gap),
			point(-lw / 2.0, top + lh + gap),
		]),
		// Mermaid halves the label width twice for the bottom right corner.
		(true, false) => Intersect::Polygon(vec![
			point(-width / 2.0, top),
			point(width / 2.0, top),
			point(width / 2.0, top + height),
			point(lw / 2.0, top + height),
			point(lw / 4.0, bottom),
			point(-lw / 2.0, bottom),
			point(-lw / 2.0, top + height),
			point(-width / 2.0, top + height),
		]),
	};
	Shape {
		parts,
		bbox: centered_box(outer_width, outer_height),
		intersect,
		label_center: point(0.0, label_y),
	}
}

/// `drawRect`: at least `minimum` in size.
fn rect(
	(bw, bh): (f64, f64),
	(padding_x, padding_y): (f64, f64),
	(min_width, min_height): (f64, f64),
	radius: Option<f64>,
) -> Shape {
	let w = (bw + padding_x * 2.0).max(min_width);
	let h = (bh + padding_y * 2.0).max(min_height);
	Shape {
		parts: vec![Part::Rect {
			x: -w / 2.0,
			y: -h / 2.0,
			width: w,
			height: h,
			rx: radius,
			class: "basic label-container",
		}],
		bbox: centered_box(w, h),
		intersect: Intersect::Rect,
		label_center: Point::default(),
	}
}

/// `createPathFromPoints`.
fn closed_path(points: &[Point]) -> String {
	let mut d: Vec<String> = points
		.iter()
		.enumerate()
		.map(|(index, p)| {
			let (x, y) = (path_number(p.x), path_number(p.y));
			format!("{}{x},{y}", if index == 0 { 'M' } else { 'L' })
		})
		.collect();
	d.push("Z".to_owned());
	d.join(" ")
}

/// `generateCirclePoints`.
fn circle_points(
	center_x: f64,
	center_y: f64,
	radius: f64,
	count: usize,
	start: f64,
	end: f64,
) -> Vec<Point> {
	let (start, end) = (start.to_radians(), end.to_radians());
	let step = (end - start) / (count - 1) as f64;
	(0..count)
		.map(|index| {
			let angle = start + index as f64 * step;
			point(-(center_x + radius * angle.cos()), -(center_y + radius * angle.sin()))
		})
		.collect()
}

fn stadium(bw: f64, bh: f64, padding_x: f64, padding_y: f64) -> Shape {
	const ARC_POINTS: usize = 50;
	let h = bh + padding_y;
	let inscribed = h * (std::f64::consts::PI / (2.0 * (ARC_POINTS - 1) as f64)).cos();
	let cap_width = (inscribed * inscribed - bh * bh).max(0.0).sqrt();
	let w = (bw + h / 4.0 + padding_x)
		.max(1.5 * h + (h - inscribed))
		.max(bw + padding_x + h - cap_width);
	let radius = h / 2.0;
	let mut points = vec![point(-w / 2.0 + radius, -h / 2.0), point(w / 2.0 - radius, -h / 2.0)];
	points.extend(circle_points(-w / 2.0 + radius, 0.0, radius, ARC_POINTS, 90.0, 270.0));
	points.push(point(w / 2.0 - radius, h / 2.0));
	points.extend(circle_points(w / 2.0 - radius, 0.0, radius, ARC_POINTS, 270.0, 450.0));
	Shape {
		bbox: points_box(&points, Point::default()),
		intersect: Intersect::Polygon(points.clone()),
		parts: vec![Part::Path {
			d: closed_path(&points),
			offset: Point::default(),
			class: "basic label-container outer-path",
		}],
		label_center: Point::default(),
	}
}

fn subroutine(
	(bw, bh): (f64, f64),
	padding_x: f64,
	padding_y: f64,
	(min_width, min_height): (f64, f64),
) -> Shape {
	const FRAME_WIDTH: f64 = 8.0;
	let total_width = (bw + 2.0 * FRAME_WIDTH + padding_x).max(min_width);
	let h = (bh + padding_y).max(min_height);
	let w = total_width - 2.0 * FRAME_WIDTH;
	let points = vec![
		point(0.0, 0.0),
		point(w, 0.0),
		point(w, -h),
		point(0.0, -h),
		point(0.0, 0.0),
		point(-8.0, 0.0),
		point(w + 8.0, 0.0),
		point(w + 8.0, -h),
		point(-8.0, -h),
		point(-8.0, 0.0),
	];
	polygon_shape(points, w, h, "label-container")
}

fn cylinder(bw: f64, bh: f64, padding_y: f64, node_padding: f64) -> Shape {
	let w = bw + padding_y;
	let rx = w / 2.0;
	let ry = rx / (2.5 + w / 50.0);
	let h = bh + padding_y + ry;
	let d = format!(
		"M0,{ry} a{rx},{ry} 0,0,0 {w},0 a{rx},{ry} 0,0,0 {},0 l0,{h} a{rx},{ry} 0,0,0 {w},0 l0,{}",
		path_number(-w),
		path_number(-h),
		ry = path_number(ry),
		rx = path_number(rx),
		w = path_number(w),
		h = path_number(h)
	);
	let offset = point(-w / 2.0, -(h / 2.0 + ry));
	// `getBBox` of the path: the top of the upper cap and the bottom of the lower one.
	let (rx32, ry32, w32) = (rx as f32, ry as f32, w as f32);
	let cap = ry as f32;
	let (top, _) = blink_arc_extent((w32, cap), (0.0, cap), (rx32, ry32), false, false);
	let base = cap + (h as f32);
	let (_, bottom) = blink_arc_extent((0.0, base), (w32, base), (rx32, ry32), false, false);
	let mut bbox = BBox::default();
	bbox.include_point(point(offset.x, offset.y + top));
	bbox.include_point(point(offset.x + w, offset.y + bottom));
	Shape {
		parts: vec![Part::Path { d, offset, class: "basic label-container outer-path" }],
		bbox,
		intersect: Intersect::Cylinder { rx, ry },
		label_center: point(0.0, node_padding / 1.5),
	}
}

/// The top and bottom of the ellipse of an SVG arc from `from` to `to` as Blink computes it in
/// single precision (`SVGPathNormalizer::DecomposeArcToCubic`, without rotation): `getBBox`
/// takes the curves through these points. The center of a half ellipse between the ends of its
/// diameter moves by the rounding.
fn blink_arc_extent(
	from: (f32, f32),
	to: (f32, f32),
	(rx, ry): (f32, f32),
	is_large: bool,
	is_sweep: bool,
) -> (f64, f64) {
	let (mut rx, mut ry) = (rx.abs(), ry.abs());
	let middle = ((from.0 - to.0) * 0.5, (from.1 - to.1) * 0.5);
	let radii_scale = middle.0 * middle.0 / (rx * rx) + middle.1 * middle.1 / (ry * ry);
	if radii_scale > 1.0 {
		rx *= radii_scale.sqrt();
		ry *= radii_scale.sqrt();
	}
	// `AffineTransform` maps in double precision with factors of single precision.
	let (scale_x, scale_y) = (f64::from(1.0 / rx), f64::from(1.0 / ry));
	let map =
		|(x, y): (f32, f32)| ((scale_x * f64::from(x)) as f32, (scale_y * f64::from(y)) as f32);
	let (p1, p2) = (map(from), map(to));
	let mut delta = (p2.0 - p1.0, p2.1 - p1.1);
	let d = delta.0 * delta.0 + delta.1 * delta.1;
	let mut scale_factor = (1.0 / d - 0.25).max(0.0).sqrt();
	if is_sweep == is_large {
		scale_factor = -scale_factor;
	}
	delta = (delta.0 * scale_factor, delta.1 * scale_factor);
	let center_y = (p1.1 + p2.1) * 0.5 + delta.0;
	// The points of the unit circle, mapped back.
	let unmap = |y: f32| f64::from((f64::from(ry) * f64::from(y)) as f32);
	(unmap(center_y - 1.0), unmap(center_y + 1.0))
}

/// The stroke and fill of a node's kind in the color themes (`af-kind-*` of agentflow), which
/// outrank the node rules.
fn kind_paint<'t>(node: &Node, theme: &'t Theme) -> Option<(&'t str, Option<&'t str>)> {
	let slot = node.kind_slot.filter(|_| !theme.border_colors.is_empty())?;
	let stroke = theme.border_colors[slot % theme.border_colors.len()].as_str();
	let background = &theme.background_colors;
	let fill = (!background.is_empty()).then(|| background[slot % background.len()].as_str());
	Some((stroke, fill))
}

/// The node rules, with the paint of the node's kind for rectangles, paths and polygons.
fn write_node_paint(svg: &mut Svg, tag: &str, class: &str, node: &Node, theme: &Theme) {
	let Some((stroke, fill)) = kind_paint(node, theme).filter(|_| class != "text") else {
		write_paint(svg, tag, class, node.look, theme);
		return;
	};
	svg.attr_if("fill", fill.unwrap_or(&theme.main_bkg)).attr_if("stroke", stroke);
	svg.attr_if("stroke-width", &theme.node_stroke_width);
	if node.look == Look::Neo && (tag != "path" || class.contains("outer-path")) {
		svg.attr_if("filter", &theme.neo_filter);
	}
}

/// The paint of the node rules for an element of `tag` with `class`.
fn write_paint(svg: &mut Svg, tag: &str, class: &str, look: Look, theme: &Theme) {
	if class == "text" {
		// `rect.text` comes after `.node rect`, which misses the `rough-node` of handDrawn.
		svg.attr("fill", "none").attr("stroke-width", "0");
		if look != Look::HandDrawn {
			svg.attr_if("stroke", &theme.node_border);
		}
		return;
	}
	if class == "brace-area" {
		// The area of braces is invisible; it only gives the box and where edges end.
		svg.attr("fill", "none").attr("stroke-opacity", "0");
		return;
	}
	if look == Look::HandDrawn {
		// The node is a `rough-node`, which the `.node` rules miss.
		return;
	}
	let is_neo = look == Look::Neo;
	svg.attr_if("fill", &theme.main_bkg);
	match is_neo {
		true => svg.attr_if("stroke", &theme.neo_stroke),
		false => svg.attr_if("stroke", &theme.node_border),
	};
	svg.attr_if("stroke-width", &theme.node_stroke_width);
	let has_shadow = matches!(tag, "rect" | "polygon" | "circle") || class.contains("outer-path");
	if is_neo && has_shadow {
		svg.attr_if("filter", &theme.neo_filter);
	}
}

/// Writes a detail of the console or browser.
fn write_accent(svg: &mut Svg, accent: &Accent, look: Look, theme: &Theme) {
	let color = if theme.node_border.is_empty() { &theme.line_color } else { &theme.node_border };
	let color = if color.is_empty() { "currentColor" } else { color.as_str() };
	match *accent {
		Accent::Line { from, to } => {
			svg.open("line")
				.number_attr("x1", from.x)
				.number_attr("y1", from.y)
				.number_attr("x2", to.x)
				.number_attr("y2", to.y)
				.attr("style", &format!("stroke:{color};stroke-width:1px"))
				.close();
		}
		Accent::Dot { center, radius } => {
			svg.open("circle");
			write_paint(svg, "circle", "", look, theme);
			svg.number_attr("cx", center.x)
				.number_attr("cy", center.y)
				.number_attr("r", radius)
				.attr("style", &format!("fill:{color};stroke:none"))
				.close();
		}
		Accent::AddressBar { x, y, width, height } => {
			svg.open("rect").attr("class", "browser-address-bar");
			write_paint(svg, "rect", "", look, theme);
			svg.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr("rx", "3")
				.attr("ry", "3")
				.attr("style", &format!("fill:none;stroke:{color};stroke-width:1px;opacity:0.6"))
				.close();
		}
		Accent::Prompt { x, y } => {
			let mut font = Font::new("monospace", 14.0);
			font.weight = 700;
			let run = TextRun {
				x,
				y,
				anchor: Anchor::Start,
				baseline: Baseline::Alphabetic,
				font: &font,
				content: ">_",
			};
			svg.text(run, |svg| {
				svg.attr("class", "console-glyph").attr("fill", color);
			});
		}
	}
}

/// Writes a collapsed subgraph. The style sheet rules of nodes, the neo look and the color slot
/// outrank the cluster colors that Mermaid sets as presentation attributes.
fn write_collapsed_group(
	svg: &mut Svg,
	(width, height, separator_y): (f64, f64, f64),
	css: &str,
	node: &Node,
	theme: &Theme,
) {
	let is_neo = node.look == Look::Neo;
	let slot = theme.color_slot(node);
	let slot_border = slot.map(|slot| theme.border_colors[slot].as_str());
	let slot_background = slot
		.filter(|_| !theme.background_colors.is_empty())
		.map(|slot| theme.background_colors[slot % theme.background_colors.len()].as_str());
	let stroke = match is_neo {
		true => &theme.neo_stroke,
		false => &theme.node_border,
	};
	let filter = if is_neo { theme.neo_filter.as_str() } else { "" };
	let (x, y) = (-width / 2.0, -height / 2.0);
	svg.open("rect")
		.attr("class", "basic label-container collapsed-group")
		.attr_if("fill", slot_background.unwrap_or(&theme.main_bkg))
		.attr_if("stroke", slot_border.unwrap_or(stroke))
		.attr_if("stroke-width", &theme.node_stroke_width)
		.attr_if("filter", filter)
		.attr_if("style", css)
		.attr("rx", "8")
		.attr("ry", "8")
		.number_attr("x", x)
		.number_attr("y", y)
		.number_attr("width", width)
		.number_attr("height", height)
		.close();
	let marker_color = slot_border.unwrap_or(&theme.cluster_border);
	svg.open("line")
		.attr("class", "collapsed-separator")
		.number_attr("x1", x + 8.0)
		.number_attr("y1", separator_y)
		.number_attr("x2", x + width - 8.0)
		.number_attr("y2", separator_y)
		.attr_if("stroke", marker_color)
		.attr("stroke-width", "0.75px")
		.attr("stroke-dasharray", "3, 3")
		.close();
	for index in -1..=1 {
		svg.open("circle")
			.attr("class", "collapsed-indicator")
			.attr_if("fill", marker_color)
			.attr_if("stroke", if is_neo { stroke } else { "none" })
			.attr_if("stroke-width", &theme.node_stroke_width)
			.attr("opacity", "0.6")
			.attr_if("filter", filter)
			.number_attr("cx", f64::from(index) * 10.0)
			.number_attr("cy", separator_y + 10.0)
			.attr("r", "2.5")
			.close();
	}
}

/// Writes a collapsed agentflow container. Its presentation attributes give way to the node
/// rules, the neo look and the color slot, except for the dashed line.
fn write_collapsed_flow(
	svg: &mut Svg,
	(width, height, separator_y): (f64, f64, f64),
	css: &str,
	node: &Node,
	theme: &Theme,
) {
	let is_neo = node.look == Look::Neo;
	let slot = theme.color_slot(node);
	let slot_border = slot.map(|slot| theme.border_colors[slot].as_str());
	let slot_background = slot
		.filter(|_| !theme.background_colors.is_empty())
		.map(|slot| theme.background_colors[slot % theme.background_colors.len()].as_str());
	let stroke = match is_neo {
		true => &theme.neo_stroke,
		false => &theme.node_border,
	};
	let filter = if is_neo { theme.neo_filter.as_str() } else { "" };
	let (x, y) = (-width / 2.0, -height / 2.0);
	svg.open("rect")
		.attr("class", "basic label-container flow-collapsed")
		.attr_if("fill", slot_background.unwrap_or(&theme.main_bkg))
		.attr_if("stroke", slot_border.unwrap_or(stroke))
		.attr_if("stroke-width", &theme.node_stroke_width)
		.attr_if("filter", filter)
		.attr_if("style", css)
		.attr("rx", "10")
		.attr("ry", "10")
		.number_attr("x", x)
		.number_attr("y", y)
		.number_attr("width", width)
		.number_attr("height", height)
		.close();
	svg.open("line")
		.attr("class", "collapsed-separator")
		.number_attr("x1", x + 8.0)
		.number_attr("y1", separator_y)
		.number_attr("x2", x + width - 8.0)
		.number_attr("y2", separator_y)
		.attr_if("stroke", &theme.flow_container_stroke)
		.attr("stroke-width", "0.75px")
		.attr("stroke-dasharray", "3, 3")
		.close();
	for index in -1..=1 {
		svg.open("circle")
			.attr("class", "collapsed-indicator")
			.attr_if("fill", &theme.node_border)
			.attr_if("stroke", if is_neo { stroke } else { "none" })
			.attr("stroke-width", "0")
			.attr("opacity", "0.5")
			.attr_if("filter", filter)
			.number_attr("cx", f64::from(index) * 10.0)
			.number_attr("cy", separator_y + 10.0)
			.attr("r", "2.5")
			.close();
	}
}

fn write_part(svg: &mut Svg, part: &Part, node_styles: &Styles, node: &Node, theme: &Theme) {
	let look = node.look;
	let css = &node_styles.node_css();
	let css = css.as_str();
	match part {
		Part::Rect { x, y, width, height, rx, class } => {
			svg.open("rect").attr("class", class);
			write_node_paint(svg, "rect", class, node, theme);
			if *class == symbols::DATASTORE {
				// Dashes as long as the sides draw the top and bottom edges only.
				svg.attr("stroke-dasharray", &format!("{} {}", number(*width), number(*height)));
			}
			svg.attr_if("style", css);
			if let Some(rx) = rx {
				svg.number_attr("rx", *rx).number_attr("ry", *rx);
			}
			svg.number_attr("x", *x)
				.number_attr("y", *y)
				.number_attr("width", *width)
				.number_attr("height", *height)
				.close();
		}
		Part::Circle { center, radius, class } => {
			svg.open("circle").attr_if("class", class);
			write_paint(svg, "circle", class, look, theme);
			let filled;
			let css = match *class == symbols::FILLED && css.is_empty() {
				true => {
					filled = format!("fill:{}", theme.node_border);
					filled.as_str()
				}
				false => css,
			};
			svg.attr_if("style", css)
				.number_attr("r", *radius)
				.number_attr("cx", center.x)
				.number_attr("cy", center.y)
				.close();
		}
		Part::Accent(accent) => write_accent(svg, accent, look, theme),
		&Part::CollapsedMarks { width, separator_y, is_flow } => {
			// The slot rules of agentflow miss the marks of its collapsed containers.
			let slot = theme.color_slot(node).filter(|_| !is_flow);
			let slot = slot.map(|slot| theme.border_colors[slot].as_str());
			let stroke = if is_flow { &theme.flow_container_stroke } else { &theme.cluster_border };
			let color = slot.unwrap_or(stroke);
			svg.open("line")
				.attr("class", "collapsed-separator")
				.number_attr("x1", -width / 2.0 + 8.0)
				.number_attr("y1", separator_y)
				.number_attr("x2", width / 2.0 - 8.0)
				.number_attr("y2", separator_y)
				.attr_if("stroke", color)
				.attr("stroke-dasharray", "3, 3")
				.close();
			for index in -1..=1 {
				svg.open("circle")
					.attr("class", "collapsed-indicator")
					.attr_if("fill", color)
					.number_attr("cx", f64::from(index) * 10.0)
					.number_attr("cy", separator_y + 10.0)
					.attr("r", "2.5")
					.close();
			}
		}
		&Part::CollapsedGroup { width, height, separator_y, is_flow } => {
			let geometry = (width, height, separator_y);
			match is_flow {
				true => write_collapsed_flow(svg, geometry, css, node, theme),
				false => write_collapsed_group(svg, geometry, css, node, theme),
			}
		}
		&Part::ImageFrame { x, y, width, height } => {
			let styles = |key: &str, default: &str| -> String {
				node_styles.get(key).filter(|value| !value.is_empty()).unwrap_or(default).to_owned()
			};
			let stroke_width = styles("stroke-width", "1.3").replace("px", "");
			svg.open("rect")
				.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr_if("fill", &styles("fill", &theme.main_bkg))
				.attr_if("stroke", &styles("stroke", &theme.node_border))
				.attr("stroke-width", &stroke_width)
				.close();
		}
		Part::IconBackground { d, offset_y } => {
			write_icon_background(svg, d, *offset_y, node_styles, theme);
		}
		&Part::IconArea { width, height, .. } => {
			svg.open("rect")
				.number_attr("x", -width / 2.0)
				.number_attr("y", -height / 2.0)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr("fill", "transparent")
				.attr("stroke", "none")
				.close();
		}
		Part::Icon { icon, x, y } => {
			let color =
				node_styles.get("stroke").unwrap_or(theme.plain_node_border.as_str()).to_owned();
			svg.open("g").attr("transform", &super::translate(*x, *y)).end();
			match &icon.glyph {
				Some(glyph) => icons::write_glyph_svg(svg, glyph, icon.size, &|svg| {
					svg.attr_if("fill", &color);
				}),
				None => icons::write_unknown_svg(svg, icon.size),
			}
			svg.close_tag("g");
		}
		Part::Image { href, x, y, width, height } => {
			svg.open("image")
				.attr("href", href)
				.number_attr("width", *width)
				.number_attr("height", *height)
				.attr("preserveAspectRatio", "none")
				.attr("transform", &super::translate(*x, *y))
				.close();
		}
		&Part::LabelBackground { x, y, width, height } => {
			svg.open("rect")
				.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr_if("fill", &theme.edge_label_background)
				.close();
		}
		Part::Polygon { points, offset, class } => {
			let points: Vec<String> =
				points.iter().map(|p| format!("{},{}", number(p.x), number(p.y))).collect();
			svg.open("polygon").attr("points", &points.join(" ")).attr("class", class);
			write_node_paint(svg, "polygon", class, node, theme);
			svg.attr_if("style", css)
				.attr("transform", &format!("translate({},{})", number(offset.x), number(offset.y)))
				.close();
		}
		Part::Path { d, offset, class } => {
			svg.open("path").attr("d", d).attr("class", class);
			write_node_paint(svg, "path", class, node, theme);
			svg.attr_if("style", css);
			if *offset != Point::default() {
				svg.attr("transform", &super::translate(offset.x, offset.y));
			}
			svg.close();
		}
		Part::Rim { center, rx, ry } => {
			svg.open("ellipse");
			write_paint(svg, "ellipse", "", look, theme);
			let rim = format!("fill:none;stroke:{};stroke-width:1px", theme.node_border);
			svg.attr_if("style", if css.is_empty() { &rim } else { css })
				.number_attr("cx", center.x)
				.number_attr("cy", center.y)
				.number_attr("rx", *rx)
				.number_attr("ry", *ry)
				.close();
		}
		Part::Line { from, to, class } => {
			// `.statediagram-state .divider`.
			svg.open("line")
				.attr("class", class)
				.attr_if("stroke", &theme.node_border)
				.number_attr("x1", from.x)
				.number_attr("y1", from.y)
				.number_attr("x2", to.x)
				.number_attr("y2", to.y)
				.close();
		}
		Part::Rough { class, d, paint, inner } => {
			write_rough(svg, (class, d, *paint, inner), node_styles, node, theme);
		}
		&Part::StateStart { radius } => {
			// `.node circle.state-start`, the neo stroke and the small shadow of `nodeShadow`.
			let color = &theme.state.special_state_color;
			let is_neo = node.look == Look::Neo;
			let filter = match (theme.state.has_node_shadow, is_neo) {
				(true, _) => theme.state.small_shadow.as_str(),
				(false, true) => theme.neo_filter.as_str(),
				(false, false) => "",
			};
			svg.open("circle")
				.attr("class", "state-start")
				.attr_if("fill", color)
				.attr_if("stroke", if is_neo { &theme.neo_stroke } else { color })
				.attr_if("filter", filter)
				.number_attr("r", radius)
				.number_attr("width", radius * 2.0)
				.number_attr("height", radius * 2.0)
				.close();
		}
		&Part::Composite { x, y, width, height } => {
			write_composite(svg, (x, y, width, height), node, theme);
		}
		Part::BlockArrow { points, offset } => {
			let points: Vec<String> =
				points.iter().map(|p| format!("{},{}", number(p.x), number(p.y))).collect();
			svg.open("polygon").attr("points", &points.join(" ")).attr("class", "label-container");
			write_paint(svg, "polygon", "label-container", look, theme);
			svg.attr_if("style", &block_style(node))
				.attr("transform", &format!("translate({},{})", number(offset.x), number(offset.y)))
				.close();
		}
		Part::Er(part) => er::write(svg, part, node_styles, node, theme),
		Part::Class(part) => class::write(svg, part, node, theme),
		Part::Usecase(part) => usecase::write(svg, part, node, node_styles, theme),
		Part::Sketch(part) => super::sketch::write_part(svg, part),
		// Written by `write_node` before the node.
		Part::ErBackground(_) => {}
		Part::Group { parts, class } => {
			svg.open("g").attr_if("class", class).attr_if("style", css).end();
			for part in parts {
				write_part(svg, part, node_styles, node, theme);
			}
			svg.close_tag("g");
		}
	}
}

/// The `style` of shapes that take the node's own styles without `!important`, under the class
/// rules (`.class > *`), which are `!important`.
fn block_style(node: &Node) -> String {
	let own = node
		.css_styles
		.iter()
		.filter(|style| !style.starts_with("color:") && !style.starts_with("text-align:"));
	own.chain(node.css_compiled_styles.iter())
		.map(|style| style.trim())
		.filter(|style| !style.is_empty())
		.collect::<Vec<_>>()
		.join(";")
}

/// Writes the rectangle of a composite block: `.node .cluster`, the neo rules and the color
/// slot of its container.
fn write_composite(
	svg: &mut Svg,
	(x, y, width, height): (f64, f64, f64, f64),
	node: &Node,
	theme: &Theme,
) {
	let is_neo = node.look == Look::Neo;
	let slot = theme.color_slot(node);
	let fill = slot
		.filter(|_| !theme.background_colors.is_empty())
		.map(|slot| theme.background_colors[slot % theme.background_colors.len()].clone())
		.or_else(|| color::with_alpha(&theme.cluster_bkg, 0.5))
		.unwrap_or_default();
	let stroke = match slot {
		Some(slot) => theme.border_colors[slot].clone(),
		None if is_neo => theme.neo_stroke.clone(),
		None => color::with_alpha(&theme.cluster_border, 0.2).unwrap_or_default(),
	};
	svg.open("rect")
		.attr("class", "basic cluster composite label-container")
		.attr_if("fill", &fill)
		.attr_if("stroke", &stroke)
		.attr("stroke-width", "1px")
		.attr_if("filter", if is_neo { theme.neo_filter.as_str() } else { "" })
		.attr_if("style", &block_style(node))
		.number_attr("x", x)
		.number_attr("y", y)
		.number_attr("width", width)
		.number_attr("height", height)
		.close();
}

/// Writes the background of an icon: `userNodeOverrides` with the fill as the stroke. The neo rules
/// are for `.node` and do not reach `.icon-shape`.
fn write_icon_background(
	svg: &mut Svg,
	d: &str,
	offset_y: f64,
	node_styles: &Styles,
	theme: &Theme,
) {
	let style = |key: &str| node_styles.get(key).filter(|value| !value.is_empty());
	let fill = style("fill").unwrap_or(theme.plain_main_bkg.as_str());
	let width = style("stroke-width").map_or("1.3".to_owned(), |width| width.replace("px", ""));
	let transform = format!("translate(0,{})", number(offset_y));
	svg.open("path")
		.attr("d", d)
		.attr("transform", &transform)
		.attr("stroke", "none")
		.attr_if("fill", fill)
		.close();
	svg.open("path")
		.attr("d", d)
		.attr("transform", &transform)
		.attr_if("stroke", fill)
		.attr_if("stroke-width", &width)
		.attr("fill", "none")
		.close();
}

/// Writes a roughjs shape: `userNodeOverrides` and the options of the shape give the paint of
/// its paths, the neo rule `[data-look="neo"].node path` restrokes both.
fn write_rough(
	svg: &mut Svg,
	(class, d, paint, inner): (&str, &str, RoughPaint, &[Part]),
	node_styles: &Styles,
	node: &Node,
	theme: &Theme,
) {
	let style = |key: &str| node_styles.get(key).filter(|value| !value.is_empty());
	let user_width = style("stroke-width").map(|width| width.replace("px", ""));
	let user_width = user_width.as_deref().unwrap_or("1.3");
	let fill_or = |default: &str| style("fill").unwrap_or(default).to_owned();
	let (fill, stroke, width) = match paint {
		RoughPaint::ForkJoin => {
			(theme.line_color.clone(), theme.line_color.clone(), user_width.to_owned())
		}
		RoughPaint::Choice => (
			fill_or(&theme.plain_main_bkg),
			style("stroke").unwrap_or(&theme.plain_node_border).to_owned(),
			user_width.to_owned(),
		),
		RoughPaint::Note => {
			(theme.state.note_bkg.clone(), theme.state.note_border.clone(), user_width.to_owned())
		}
		RoughPaint::EndOuter => {
			(fill_or(&theme.plain_main_bkg), theme.line_color.clone(), "2".to_owned())
		}
		RoughPaint::EndInner => {
			(theme.state.end_inner.clone(), theme.state.end_inner.clone(), "2".to_owned())
		}
	};
	let dashes = match style("stroke-dasharray") {
		Some(dashes) => {
			let values: Vec<f64> =
				dashes.split_whitespace().map(|value| value.parse().unwrap_or(0.0)).collect();
			match values.as_slice() {
				[single] => format!("{} {}", number(*single), number(*single)),
				[first, second, ..] => format!("{} {}", number(*first), number(*second)),
				[] => "0 0".to_owned(),
			}
		}
		None => "0 0".to_owned(),
	};
	let is_neo = node.look == Look::Neo;
	let css = node_styles.node_css();
	let filter = match (paint, theme.state.has_node_shadow) {
		(RoughPaint::EndOuter, true) => theme.state.small_shadow.as_str(),
		_ if is_neo && class.contains("outer-path") => theme.neo_filter.as_str(),
		_ => "",
	};
	svg.open("g").attr_if("class", class).attr_if("filter", filter).end();
	let (stroke_of_fill, width_of_fill) = match is_neo {
		true => (theme.neo_stroke.as_str(), theme.node_stroke_width.as_str()),
		false => ("none", "0"),
	};
	svg.open("path")
		.attr("d", d)
		.attr_if("stroke", stroke_of_fill)
		.attr_if("stroke-width", width_of_fill)
		.attr_if("fill", &fill)
		.attr_if("style", &css)
		.close();
	let (stroke, width) = match is_neo {
		true => (theme.neo_stroke.clone(), theme.node_stroke_width.clone()),
		false => (stroke, width),
	};
	svg.open("path")
		.attr("d", d)
		.attr_if("stroke", &stroke)
		.attr_if("stroke-width", &width)
		.attr("fill", "none")
		// roughjs writes `0 0` without a dash style; EdgeHTML then draws no line at all.
		.attr_if("stroke-dasharray", if dashes == "0 0" { "" } else { &dashes })
		.attr_if("style", &css)
		.close();
	for part in inner {
		write_part(svg, part, node_styles, node, theme);
	}
	svg.close_tag("g");
}

/// Writes the elements of `shape` with the node's styles.
pub fn write(svg: &mut Svg, shape: &Shape, node: &Node, styles: &Styles, theme: &Theme) {
	for part in &shape.parts {
		write_part(svg, part, styles, node, theme);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn label(width: f64, height: f64) -> LabelBox {
		LabelBox {
			lines: Vec::new(),
			width,
			height,
			line_height: height,
			font: super::super::super::text::Font::new("sans-serif", 14.0),
			is_left_aligned: false,
			is_underlined: false,
		}
	}

	#[test]
	fn rounds_arcs_like_blink() {
		// `getBBox` of Mermaid's cylinder paths in Edge.
		let ry = 11.804_777_f32;
		let top = blink_arc_extent((111.828_125, ry), (0.0, ry), (55.914_063, ry), false, false);
		assert_eq!(top.0, 0.004_075_364_209_711_552);
		let bottom = blink_arc_extent((0.0, ry), (111.828_125, ry), (55.914_063, ry), false, false);
		assert_eq!(bottom.1 - f64::from(ry), 11.800_701_141_357_422);
		let cylinder = cylinder(103.828_125, 24.0, 8.0, 8.0);
		assert!((cylinder.bbox.height() - 67.406_181_335_449_22).abs() < 1e-5);
	}

	#[test]
	fn sizes_shapes_like_mermaid() {
		let node = |shape: &str, look| Node {
			shape: shape.to_owned(),
			padding: 15.0,
			look,
			..Node::default()
		};
		// The neo rectangle and diamond of the reference render.
		let flowchart = DiagramStyle::Flowchart;
		let shape =
			shape(&node("squareRect", Look::Neo), &label(120.0, 21.0), 5.0, false, flowchart);
		assert_eq!((shape.bbox.width(), shape.bbox.height()), (152.0, 45.0));
		let diamond =
			super::shape(&node("diamond", Look::Neo), &label(120.0, 21.0), 5.0, false, flowchart);
		assert_eq!((diamond.bbox.width(), diamond.bbox.height()), (171.0, 171.0));
		let classic =
			super::shape(&node("rect", Look::Classic), &label(40.0, 21.0), 5.0, false, flowchart);
		assert_eq!((classic.bbox.width(), classic.bbox.height()), (100.0, 51.0));
		for name in [
			"stadium",
			"subroutine",
			"cylinder",
			"circle",
			"doublecircle",
			"hexagon",
			"lean_right",
			"lean_left",
			"trapezoid",
			"inv_trapezoid",
			"odd",
			"text",
		] {
			let shape =
				super::shape(&node(name, Look::Classic), &label(40.0, 21.0), 5.0, false, flowchart);
			assert!(shape.bbox.width() >= 40.0 && shape.bbox.height() >= 21.0, "{name}");
		}
	}
}
