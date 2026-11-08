//! The `handDrawn` look of the shared rendering: the branches `node.look === 'handDrawn'` of
//! `rendering-elements/shapes/*`, `handDrawnShapeStyles.ts` and `roundedRectPath.ts`. A shape is
//! built by its classic code and its parts then become the roughjs calls of Mermaid.

use super::super::color::js_number;
use super::super::common::js_number_of;
use super::super::rough::{self, FillStyle, Options, PathKind, Sketch};
use super::super::svg::{Svg, number};
use super::super::{Context, Look};
use super::shapes::blink_path::{self, PathCommand};
use super::shapes::class::ClassPart;
use super::shapes::er::{ErKind, ErPart};
use super::shapes::usecase::{
	AWESOME_SILHOUETTE_PATH, ActorVariant, HOLLOW_BODY_PATH, ICON_CENTER_Y, ICON_FRAME_SIZE,
	STICK_FIGURE_PATH, UsecasePart,
};
use super::shapes::{Part, RoughPaint, Shape};
use super::{BBox, DiagramStyle, Node, Point, Styles, Theme};

/// A roughjs call of a shape: its group with the paths.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchPart {
	pub sketch: Sketch,
	pub class: &'static str,
	pub offset: Point,
	pub style: String,
	/// `.attr('stroke-opacity', 0)` of the areas of brace shapes.
	pub is_invisible: bool,
	/// Drawn by Mermaid with an invisible paint (`fill: transparent; stroke: none`): only its
	/// box counts, for the size of the node.
	pub is_measured_only: bool,
	/// The `style` of every path.
	pub path_style: String,
	/// A call whose group is inserted into this one.
	pub inner: Option<Box<SketchPart>>,
	/// The `.node path` rule over all paths: shapes that keep the class `node`.
	pub node_paint: Option<NodePaint>,
	/// The `style` of the outline.
	pub outline_style: String,
}

/// The paint of the `.node path` rule.
#[derive(Clone, Debug, PartialEq)]
pub struct NodePaint {
	pub fill: String,
	pub stroke: String,
	pub stroke_width: String,
}

impl SketchPart {
	fn new(sketch: Sketch, class: &'static str) -> SketchPart {
		SketchPart {
			sketch,
			class,
			offset: Point::default(),
			style: String::new(),
			is_invisible: false,
			is_measured_only: false,
			path_style: String::new(),
			inner: None,
			node_paint: None,
			outline_style: String::new(),
		}
	}

	fn include_in(&self, bbox: &mut BBox) {
		if let Some(bounds) = self.bounds() {
			let drawn = bounds_box(bounds);
			bbox.include_point(drawn.min);
			bbox.include_point(drawn.max);
		}
	}

	/// `getBBox` of the inserted group and the paths, moved by the offset.
	fn bounds(&self) -> Option<rough::Bounds> {
		let inner = self.inner.as_ref().and_then(|inner| inner.bounds());
		let own = self.sketch.bounds().map(|bounds| {
			bounds.translated(
				blink_path::parse_number(self.offset.x),
				blink_path::parse_number(self.offset.y),
			)
		});
		blink_path::union_all(inner.into_iter().chain(own))
	}
}

/// A box of Blink in the numbers of the renderer.
pub fn bounds_box(bounds: rough::Bounds) -> BBox {
	let (x, y) = (f64::from(bounds.x), f64::from(bounds.y));
	BBox {
		min: Point { x, y },
		max: Point { x: x + f64::from(bounds.width), y: y + f64::from(bounds.height) },
	}
}

impl Look {
	/// The value of `data-look`.
	pub fn data_name(self) -> &'static str {
		match self {
			Look::Classic => "classic",
			Look::Neo => "neo",
			Look::HandDrawn => "handDrawn",
		}
	}
}

impl Context<'_> {
	/// The seed of the sketches of an element. `handDrawnSeed` is used like roughjs does, with
	/// the 32 bits of `Math.imul`. Without one roughjs takes `Math.random()`, but a picture must
	/// stay the same from one render to the next, so the seed comes from the diagram and the
	/// element.
	pub fn sketch_seed(&self, element: &str) -> i32 {
		let configured = self.config.get("handDrawnSeed").and_then(|seed| seed.as_number());
		if let Some(seed) = configured.filter(|seed| seed.is_finite())
			&& seed.trunc() != 0.0
		{
			let wrapped = seed.trunc().rem_euclid(4_294_967_296.0) as u64 as u32 as i32;
			if wrapped != 0 {
				return wrapped;
			}
		}
		let mut hash = 0xcbf2_9ce4_8422_2325u64;
		for byte in self.id.bytes().chain([0]).chain(element.bytes()) {
			hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
		}
		((hash ^ (hash >> 32)) as u32 & 0x7fff_ffff | 1) as i32
	}
}

/// `getStrokeDashArray`.
fn stroke_dash_array(style: Option<&str>) -> String {
	let Some(style) = style.filter(|style| !style.is_empty()) else {
		return "0 0".to_owned();
	};
	let number_or_zero = |text: &str| {
		let value = js_number_of(text);
		if value.is_nan() { 0.0 } else { value }
	};
	let mut values = style.split_whitespace().map(number_or_zero);
	let first = values.next().unwrap_or(0.0);
	let second = values.next().unwrap_or(first);
	format!("{} {}", js_number(first), js_number(second))
}

/// `userNodeOverrides(node, {})`.
pub fn user_node_overrides(styles: &Styles, theme: &Theme, seed: i32) -> Options {
	let style = |key: &str| styles.get(key).filter(|value| !value.is_empty());
	Options {
		seed,
		roughness: 0.7,
		fill: Some(style("fill").unwrap_or(&theme.plain_main_bkg).to_owned()),
		fill_style: FillStyle::Hachure,
		fill_weight: 1.5,
		hachure_gap: 1.5,
		stroke: style("stroke").unwrap_or(&theme.plain_node_border).to_owned(),
		stroke_width: style("stroke-width")
			.map(|width| width.replacen("px", "", 1))
			.filter(|width| !width.is_empty())
			.unwrap_or_else(|| "1.3".to_owned()),
		fill_line_dash: Some("0 0".to_owned()),
		stroke_line_dash: Some(stroke_dash_array(styles.get("stroke-dasharray"))),
		..Options::default()
	}
}

/// The options of an agentflow `flow` container (`flowGroup`, `getCollapsedStyle`).
pub fn flow_container_options(options: &mut Options, theme: &Theme) {
	options.fill = Some("none".to_owned());
	options.stroke = theme.flow_container_stroke.clone();
	options.stroke_width = "0.75".to_owned();
	options.fill_weight = 0.0;
}

/// `styles2String`: the stroke styles and the fill style of an element, `!important`.
pub fn border_and_background_styles(styles: &Styles) -> (String, String) {
	let (mut border, mut background) = (Vec::new(), Vec::new());
	for (key, value) in &styles.entries {
		if key.is_empty() || super::is_label_style(key) {
			continue;
		}
		if key.contains("stroke") {
			border.push(format!("{key}:{value} !important"));
		}
		if key == "fill" {
			background.push(format!("{key}:{value} !important"));
		}
	}
	(border.join(";"), background.join(";"))
}

/// Writes the group of a roughjs call with a class and the paint of rules over its paths.
pub fn write_group(svg: &mut Svg, sketch: &Sketch, class: &str, paint: &PathPaint) {
	svg.open("g").attr_if("class", class).end();
	write_paths(svg, sketch, paint);
	svg.close_tag("g");
}

/// The box `getBBox` gives for a sketch, `None` if it draws nothing.
pub fn sketch_box(sketch: &Sketch) -> Option<BBox> {
	sketch.bounds().map(bounds_box)
}

/// `solidStateFill`.
fn solid_state_fill(color: &str, seed: i32) -> Options {
	Options {
		seed,
		fill: Some(color.to_owned()),
		stroke: color.to_owned(),
		hachure_angle: 120.0,
		hachure_gap: 1.5,
		fill_weight: 1.5,
		roughness: 0.7,
		..Options::default()
	}
}

/// `createRoundedRectPathD`.
pub fn rounded_rect_path(x: f64, y: f64, width: f64, height: f64, radius: f64) -> String {
	let n = js_number;
	let arc =
		|to_x: f64, to_y: f64| format!("A {r} {r} 0 0 1 {} {}", n(to_x), n(to_y), r = n(radius));
	format!(
		"M {} {} H {} {} V {} {} H {} {} V {} {} Z",
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

/// `createPathFromPoints`.
fn path_from_points(points: &[Point]) -> String {
	let mut d = String::new();
	for (index, point) in points.iter().enumerate() {
		d.push_str(if index == 0 { "M" } else { " L" });
		d.push_str(&js_number(point.x));
		d.push(',');
		d.push_str(&js_number(point.y));
	}
	d.push_str(" Z");
	d
}

/// The points of a path of `createPathFromPoints`, for the shapes that hand them to `rc.polygon`.
fn points_of_path(d: &str) -> Vec<[f64; 2]> {
	d.split(['M', 'L', 'Z'])
		.filter_map(|point| {
			let (x, y) = point.trim().split_once(',')?;
			Some([x.parse().ok()?, y.parse().ok()?])
		})
		.collect()
}

fn is_named(name: &str, names: &[&str]) -> bool {
	names.contains(&name)
}

/// The paint a style sheet rule or a `style` attribute puts over the attributes of the sketch.
#[derive(Default)]
pub struct PathPaint<'a> {
	pub stroke: Option<&'a str>,
	pub fill: Option<&'a str>,
	pub fill_stroke_width: Option<&'a str>,
	/// The width of every path.
	pub stroke_width: Option<&'a str>,
	/// A `path[stroke='none'] { fill }` rule.
	pub fill_where_no_stroke: Option<&'a str>,
	/// A `path[fill='none'] { stroke }` rule.
	pub stroke_where_no_fill: Option<&'a str>,
	pub outline_style: &'a str,
	pub fill_style: &'a str,
	/// `path:first-of-type` and `path:nth-of-type(2)` rules: the stroke of the first two paths.
	pub stroke_by_position: [Option<&'a str>; 2],
}

/// A dash attribute of roughjs. EdgeHTML draws no line at all with `0 0`.
fn visible_dashes(dashes: Option<&str>) -> &str {
	dashes.filter(|dashes| !matches!(*dashes, "0 0" | "0" | "")).unwrap_or("")
}

/// The `<path>`s of `RoughSVG.draw`.
pub fn write_paths(svg: &mut Svg, sketch: &Sketch, paint: &PathPaint) {
	let options = &sketch.options;
	let fill_text = options.fill.as_deref().unwrap_or("");
	let weight = number(sketch.fill_weight());
	for (position, path) in sketch.paths.iter().enumerate() {
		let (mut stroke, stroke_width, mut fill, dashes, style) = match path.kind {
			PathKind::Outline => (
				paint.stroke.unwrap_or(&options.stroke),
				paint.stroke_width.unwrap_or(&options.stroke_width),
				paint.fill.unwrap_or("none"),
				visible_dashes(options.stroke_line_dash.as_deref()),
				paint.outline_style,
			),
			PathKind::Fill => (
				paint.stroke.unwrap_or("none"),
				paint.stroke_width.unwrap_or("0"),
				paint.fill.unwrap_or(fill_text),
				"",
				paint.fill_style,
			),
			PathKind::FillSketch => (
				paint.stroke.unwrap_or(fill_text),
				paint.stroke_width.or(paint.fill_stroke_width).unwrap_or(&weight),
				paint.fill.unwrap_or("none"),
				visible_dashes(options.fill_line_dash.as_deref()),
				paint.fill_style,
			),
		};
		// `path[stroke='none']` and `path[fill='none']` select by the attributes of roughjs.
		let (stroke_is_none, fill_is_none) = (stroke == "none", fill == "none");
		if let Some(rule_fill) = paint.fill_where_no_stroke.filter(|_| stroke_is_none) {
			fill = rule_fill;
		}
		if let Some(rule_stroke) = paint.stroke_where_no_fill.filter(|_| fill_is_none) {
			stroke = rule_stroke;
		}
		if let Some(&Some(rule_stroke)) = paint.stroke_by_position.get(position) {
			stroke = rule_stroke;
		}
		svg.open("path")
			.attr("d", &path.d)
			.attr_if("stroke", stroke)
			.attr_if("stroke-width", stroke_width)
			.attr_if("fill", fill);
		if path.kind == PathKind::Fill && sketch.is_even_odd {
			svg.attr("fill-rule", "evenodd");
		}
		svg.attr_if("stroke-dasharray", dashes).attr_if("style", style).close();
	}
}

/// Writes the group of a roughjs call.
pub fn write_part(svg: &mut Svg, part: &SketchPart) {
	if part.is_measured_only {
		return;
	}
	write_part_painted(svg, part, part.node_paint.as_ref());
}

fn set(value: &str) -> Option<&str> {
	Some(value).filter(|value| !value.is_empty())
}

fn write_part_painted(svg: &mut Svg, part: &SketchPart, node_paint: Option<&NodePaint>) {
	svg.open("g").attr_if("class", part.class).attr_if("style", &part.style);
	if part.is_invisible {
		svg.attr("stroke-opacity", "0");
	}
	if part.offset != Point::default() {
		svg.attr("transform", &super::translate(part.offset.x, part.offset.y));
	}
	svg.end();
	let outline_style = match part.outline_style.is_empty() {
		true => &part.path_style,
		false => &part.outline_style,
	};
	let paint = PathPaint {
		outline_style,
		fill_style: &part.path_style,
		stroke: node_paint.and_then(|paint| set(&paint.stroke)),
		fill: node_paint.and_then(|paint| set(&paint.fill)),
		stroke_width: node_paint.and_then(|paint| set(&paint.stroke_width)),
		..PathPaint::default()
	};
	write_paths(svg, &part.sketch, &paint);
	if let Some(inner) = &part.inner {
		write_part_painted(svg, inner, node_paint);
	}
	svg.close_tag("g");
}

fn visit_sketches<'p>(parts: &'p [Part], visit: &mut impl FnMut(&'p SketchPart)) {
	for part in parts {
		match part {
			Part::Sketch(sketch) => visit(sketch),
			Part::Group { parts, .. } => visit_sketches(parts, visit),
			_ => {}
		}
	}
}

/// A rule of the diagram's style sheet that fills every path of the sketches of `shape`.
pub fn fill_all_paths(shape: &mut Shape, fill: &str) {
	fn visit(parts: &mut [Part], fill: &str) {
		for part in parts {
			match part {
				Part::Sketch(sketch) => {
					let mut paint = sketch.node_paint.take().unwrap_or(NodePaint {
						fill: String::new(),
						stroke: String::new(),
						stroke_width: String::new(),
					});
					paint.fill = fill.to_owned();
					sketch.node_paint = Some(paint);
				}
				Part::Group { parts, .. } => visit(parts, fill),
				_ => {}
			}
		}
	}
	visit(&mut shape.parts, fill);
}

/// `getBBox` of all the elements of a shape, which go into the viewport of the diagram.
pub fn include_ink(shape: &Shape, bbox: &mut BBox, center: Point) {
	visit_sketches(&shape.parts, &mut |part| {
		let mut ink = BBox::default();
		part.include_in(&mut ink);
		bbox.include(&ink, center.x, center.y);
	});
}

struct Sketcher<'a> {
	options: Options,
	name: &'a str,
	theme: &'a Theme,
}

impl Sketcher<'_> {
	fn with(&self, change: impl FnOnce(&mut Options)) -> Options {
		let mut options = self.options.clone();
		change(&mut options);
		options
	}

	fn is_text_shape(&self) -> bool {
		is_named(self.name, &["brace", "comment", "brace-l", "brace-r", "braces"])
	}

	/// The use case shapes (`usecaseEllipse`, `usecaseBusiness`, `usecaseJsonTable`, the `[…]`
	/// form) and the glyphs of actors (`appendActorPath`, `appendActorCircle`,
	/// `drawIconActor`).
	fn usecase(&self, parts: &[Part], node: &Node) -> Vec<Part> {
		let hollow = self.with(|options| options.fill = Some("none".to_owned()));
		// `attr('style', node.cssStyles)` writes the array with commas.
		let styled = |sketch, class| {
			Part::Sketch(Box::new(SketchPart {
				style: node.css_styles.join(","),
				..SketchPart::new(sketch, class)
			}))
		};
		let plain = |sketch, class| Part::Sketch(Box::new(SketchPart::new(sketch, class)));
		let mut sketched = Vec::with_capacity(parts.len());
		for part in parts {
			let Part::Usecase(part) = part else {
				sketched.push(part.clone());
				continue;
			};
			sketched.push(match part {
				&UsecasePart::Ellipse { rx, ry, class } => {
					let sketch = rough::ellipse(0.0, 0.0, rx * 2.0, ry * 2.0, &self.options);
					match class == "basic label-container" {
						true => styled(sketch, class),
						false => plain(sketch, class),
					}
				}
				&UsecasePart::Body { x, y, width, height } => styled(
					rough::rectangle(x, y, width, height, &self.options),
					"basic label-container",
				),
				&UsecasePart::JsonCell { x, y, width, height, class } => {
					plain(rough::rectangle(x, y, width, height, &self.options), class)
				}
				&UsecasePart::Glyph { variant, offset_y, business, .. } => {
					let path = |d, options, class| SketchPart::new(rough::path(d, options), class);
					let sketches = match variant {
						ActorVariant::Normal => {
							vec![path(STICK_FIGURE_PATH, &self.options, "usecase-actor-stick")]
						}
						ActorVariant::Hollow => vec![
							path(HOLLOW_BODY_PATH, &hollow, "usecase-actor-hollow-body"),
							SketchPart::new(
								rough::circle(0.0, -23.0, 18.0, &hollow),
								"usecase-actor-hollow-head",
							),
						],
						ActorVariant::Awesome => vec![path(
							AWESOME_SILHOUETTE_PATH,
							&self.options,
							"usecase-actor-awesome-silhouette",
						)],
						ActorVariant::Icon => {
							let frame = -ICON_FRAME_SIZE / 2.0;
							let rectangle = rough::rectangle(
								frame,
								ICON_CENTER_Y + frame,
								ICON_FRAME_SIZE,
								ICON_FRAME_SIZE,
								&self.options,
							);
							vec![SketchPart::new(rectangle, "usecase-actor-icon-frame")]
						}
					};
					Part::Usecase(UsecasePart::Glyph { variant, offset_y, business, sketches })
				}
				other => Part::Usecase(other.clone()),
			});
		}
		sketched
	}

	fn part(&self, part: Part, index: usize) -> Part {
		match part {
			Part::Rect { x, y, width, height, rx, class } => {
				let sketch = match rx.filter(|radius| *radius != 0.0) {
					Some(radius) => {
						let d = rounded_rect_path(x, y, width, height, radius);
						rough::path(&d, &self.options)
					}
					None => rough::rectangle(x, y, width, height, &self.options),
				};
				Part::Sketch(Box::new(SketchPart::new(sketch, class)))
			}
			Part::Circle { center, radius, class } => {
				let sketch = rough::circle(center.x, center.y, radius * 2.0, &self.options);
				Part::Sketch(Box::new(SketchPart::new(sketch, class)))
			}
			Part::Polygon { points, offset, class } => {
				let d = path_from_points(&points);
				let sketch = rough::path(&d, &self.options);
				Part::Sketch(Box::new(SketchPart { offset, ..SketchPart::new(sketch, class) }))
			}
			Part::Path { d, offset, class } => {
				let is_polygon = is_named(
					self.name,
					&[
						"lin-rect",
						"lined-rectangle",
						"lined-process",
						"lin-proc",
						"shaded-process",
						"div-rect",
						"div-proc",
						"divided-rectangle",
						"divided-process",
						"lin-doc",
						"lined-document",
					],
				);
				let is_tag = index == 1
					&& is_named(
						self.name,
						&[
							"tag-rect",
							"tagged-rectangle",
							"tag-proc",
							"tagged-process",
							"tag-doc",
							"tagged-document",
						],
					);
				let options = match () {
					() if is_tag => self.with(|options| options.fill_style = FillStyle::Solid),
					() if self.is_text_shape() => {
						self.with(|options| options.fill = Some("none".to_owned()))
					}
					() => self.options.clone(),
				};
				let sketch = match is_polygon {
					true => rough::polygon(points_of_path(&d), &options),
					false => rough::path(&d, &options),
				};
				Part::Sketch(Box::new(SketchPart {
					offset,
					is_invisible: class == "brace-area",
					..SketchPart::new(sketch, class)
				}))
			}
			Part::Line { from, to, .. } => {
				let sketch = rough::line([from.x, from.y], [to.x, to.y], &self.options);
				Part::Sketch(Box::new(SketchPart::new(sketch, "")))
			}
			Part::Group { parts, class } => Part::Group {
				parts: parts
					.into_iter()
					.enumerate()
					.map(|(index, part)| self.part(part, index))
					.collect(),
				class,
			},
			other => other,
		}
	}

	fn rough_part(&self, part: &Part, bbox: BBox) -> Option<Vec<Part>> {
		let Part::Rough { paint, d, class, .. } = part else {
			return None;
		};
		let (x, y) = (bbox.min.x, bbox.min.y);
		let (width, height) = (bbox.width(), bbox.height());
		let theme = self.theme;
		let sketch = |sketch: Sketch, class: &'static str| {
			vec![Part::Sketch(Box::new(SketchPart::new(sketch, class)))]
		};
		Some(match paint {
			RoughPaint::ForkJoin => {
				let options = self.with(|options| {
					options.stroke = theme.line_color.clone();
					options.fill = Some(theme.line_color.clone());
				});
				sketch(rough::rectangle(x, y, width, height, &options), class)
			}
			RoughPaint::Note => {
				let options = self.with(|options| {
					options.fill = Some(theme.state.note_bkg.clone());
					options.stroke = theme.state.note_border.clone();
				});
				sketch(rough::rectangle(x, y, width, height, &options), class)
			}
			RoughPaint::Choice => sketch(rough::path(d, &self.options), class),
			RoughPaint::EndOuter | RoughPaint::EndInner => self.state_end(),
		})
	}

	/// `stateEnd`.
	fn state_end(&self) -> Vec<Part> {
		const WIDTH: f64 = 14.0;
		let theme = self.theme;
		let outer = self.with(|options| {
			options.stroke = theme.line_color.clone();
			options.stroke_width = "2".to_owned();
		});
		let inner = self.with(|options| {
			options.fill = Some(theme.state.end_inner.clone());
			options.stroke = theme.state.end_inner.clone();
			options.stroke_width = "2".to_owned();
			options.fill_style = FillStyle::Solid;
		});
		let inner = SketchPart::new(rough::circle(0.0, 0.0, WIDTH * 5.0 / 14.0, &inner), "");
		vec![Part::Sketch(Box::new(SketchPart {
			inner: Some(Box::new(inner)),
			..SketchPart::new(rough::circle(0.0, 0.0, WIDTH, &outer), "")
		}))]
	}

	/// `subroutine`: the rectangle measures the node, the lines come after it.
	fn subroutine(&self, points: &[Point]) -> Vec<Part> {
		const FRAME_WIDTH: f64 = 8.0;
		let (w, h) = (points[1].x, -points[2].y);
		let (x, y) = (-w / 2.0 - FRAME_WIDTH, -h / 2.0);
		let line_at = |line_x: f64| {
			let sketch = rough::line([line_x, y], [line_x, y + h], &self.options);
			Part::Sketch(Box::new(SketchPart::new(sketch, "")))
		};
		let rectangle = rough::rectangle(x, y, w + 16.0, h, &self.options);
		vec![
			Part::Sketch(Box::new(SketchPart::new(rectangle, "basic label-container"))),
			line_at(x + FRAME_WIDTH + w),
			line_at(x + FRAME_WIDTH),
		]
	}

	/// `cylinder` and `linedCylinder`: the outline with its bottom curve measures the node, the
	/// top curve is not moved with it.
	fn cylinder(&self, rx: f64, ry: f64, offset: Point, outer_offset: Option<f64>) -> Vec<Part> {
		let n = js_number;
		let w = -offset.x * 2.0;
		let h = (-offset.y - ry) * 2.0;
		let mut outer = format!(
			"M0,{ry} M{w},{ry} a{rx},{ry_} 0,0,0 {minus_w},0 l0,{h_} a{rx},{ry_} 0,0,0 {w},0 l0,{minus_h}",
			ry = n(ry),
			w = n(w),
			rx = n(rx),
			ry_ = n(ry),
			minus_w = n(-w),
			h_ = n(h),
			minus_h = n(-h),
		);
		if let Some(outer_offset) = outer_offset {
			outer.push_str(&format!(
				" M0,{} a{},{} 0,0,0 {},0",
				n(ry + outer_offset),
				n(rx),
				n(ry),
				n(w)
			));
		}
		let inner =
			format!("M{},{} a{},{} 0,0,0 {},0", n(-w / 2.0), n(-h / 2.0), n(rx), n(ry), n(w));
		let inner_options = match outer_offset {
			Some(_) => self.options.clone(),
			None => self.with(|options| options.fill = Some("none".to_owned())),
		};
		vec![
			Part::Sketch(Box::new(SketchPart {
				offset,
				..SketchPart::new(rough::path(&outer, &self.options), "basic label-container")
			})),
			Part::Sketch(Box::new(SketchPart::new(
				rough::path(&inner, &inner_options),
				if outer_offset.is_some() { "line" } else { "" },
			))),
		]
	}

	/// `tiltedCylinder`.
	fn horizontal_cylinder(&self, rx: f64, ry: f64, offset: Point) -> Vec<Part> {
		let n = js_number;
		let w = -offset.x * 2.0;
		let h = offset.y * 2.0;
		let outer = format!(
			"M0,0 M{w},0 a{rx},{ry} 0,0,0 0,{minus_h} l{minus_w},0 a{rx},{ry} 0,0,0 0,{h} l{w},0",
			w = n(w),
			rx = n(rx),
			ry = n(ry),
			minus_h = n(-h),
			minus_w = n(-w),
			h = n(h),
		);
		let inner =
			format!("M{},{} a{},{} 0,0,0 0,{}", n(w / 2.0), n(-h / 2.0), n(rx), n(ry), n(h));
		let inner_options = self.with(|options| options.fill = Some("none".to_owned()));
		vec![
			Part::Sketch(Box::new(SketchPart {
				offset,
				..SketchPart::new(rough::path(&outer, &self.options), "basic label-container")
			})),
			Part::Sketch(Box::new(SketchPart::new(rough::path(&inner, &inner_options), ""))),
		]
	}

	/// `doublecircle`.
	fn double_circle(&self, outer: f64, inner: f64) -> Vec<Part> {
		let of = |stroke_width: &str| {
			self.with(|options| {
				options.roughness = 0.2;
				options.stroke_width = stroke_width.to_owned();
			})
		};
		vec![Part::Group {
			parts: vec![
				Part::Sketch(Box::new(SketchPart::new(
					rough::circle(0.0, 0.0, outer * 2.0, &of("2.5")),
					"",
				))),
				Part::Sketch(Box::new(SketchPart::new(
					rough::circle(0.0, 0.0, inner * 2.0, &of("1.5")),
					"",
				))),
			],
			class: "",
		}]
	}

	/// `classBox`: the rectangle, and the dividers across its drawn box.
	fn class_box(&self, parts: &[Part], css: &str) -> Vec<Part> {
		let mut drawn_box: Option<BBox> = None;
		let mut sketched = Vec::with_capacity(parts.len());
		for part in parts {
			sketched.push(match part {
				Part::Class(ClassPart::Outer(corners)) => {
					let [top_left, top_right, bottom_right, _] = *corners;
					let (width, height) = (top_right.x - top_left.x, bottom_right.y - top_left.y);
					let rectangle =
						rough::rectangle(top_left.x, top_left.y, width, height, &self.options);
					drawn_box = sketch_box(&rectangle);
					Part::Sketch(Box::new(SketchPart {
						outline_style: css.to_owned(),
						..SketchPart::new(rectangle, "basic label-container outer-path")
					}))
				}
				Part::Class(ClassPart::Divider { from, to }) => {
					let (left, right) =
						drawn_box.map_or((from.x, to.x), |bbox| (bbox.min.x, bbox.max.x));
					let line = rough::line([left, from.y], [right, to.y], &self.options);
					Part::Sketch(Box::new(SketchPart {
						path_style: css.to_owned(),
						..SketchPart::new(line, "divider")
					}))
				}
				other => other.clone(),
			});
		}
		sketched
	}

	/// `erBox`: the entity over a copy of it without roughness, rows and dividers.
	fn er_box(&self, parts: &[Part], node: &Node) -> Vec<Part> {
		let theme = self.theme;
		let css = node.css_styles.join("");
		let mut sketched = vec![Part::ErBackground(parts.to_vec())];
		for part in parts {
			let Part::Er(part) = part else {
				sketched.push(part.clone());
				continue;
			};
			sketched.push(Part::Sketch(Box::new(match *part {
				ErPart::Rect { x, y, width, height } => SketchPart {
					style: node.css_styles.join(","),
					..SketchPart::new(
						rough::rectangle(x, y, width, height, &self.options),
						"basic label-container",
					)
				},
				ErPart::Rough { kind, corners } => {
					let [top_left, top_right, bottom_right, _] = corners;
					let (width, height) = (top_right.x - top_left.x, bottom_right.y - top_left.y);
					match kind {
						ErKind::Outer => SketchPart {
							style: css.clone(),
							..SketchPart::new(
								rough::rectangle(
									top_left.x,
									top_left.y,
									width,
									height,
									&self.options,
								),
								"outer-path",
							)
						},
						ErKind::RowOdd | ErKind::RowEven => {
							let is_even = kind == ErKind::RowEven;
							let options = self.with(|options| {
								options.fill = match is_even {
									true => theme.er.row_even.clone(),
									false => theme.er.row_odd.clone(),
								};
								options.stroke = theme.plain_node_border.clone();
							});
							let rectangle =
								rough::rectangle(top_left.x, top_left.y, width, height, &options);
							SketchPart {
								style: css.clone(),
								..SketchPart::new(
									rectangle,
									if is_even { "row-rect-even" } else { "row-rect-odd" },
								)
							}
						}
						ErKind::Divider => SketchPart::new(
							rough::polygon(corners.map(|p| [p.x, p.y]).to_vec(), &self.options),
							"divider",
						),
					}
				}
				ErPart::Line { from, to } => SketchPart::new(
					rough::line([from.x, from.y], [to.x, to.y], &self.options),
					"divider",
				),
			})));
		}
		sketched
	}

	/// `requirementBox`: the rectangle and the divider below the name.
	fn requirement_box(&self, parts: &[Part], css: &str) -> Vec<Part> {
		parts
			.iter()
			.map(|part| match part {
				Part::Er(ErPart::Rough { kind: ErKind::Outer, corners }) => {
					let [top_left, top_right, bottom_right, _] = *corners;
					let (width, height) = (top_right.x - top_left.x, bottom_right.y - top_left.y);
					Part::Sketch(Box::new(SketchPart {
						style: css.to_owned(),
						..SketchPart::new(
							rough::rectangle(top_left.x, top_left.y, width, height, &self.options),
							"basic label-container outer-path",
						)
					}))
				}
				Part::Er(ErPart::Line { from, to }) => Part::Sketch(Box::new(SketchPart::new(
					rough::line([from.x, from.y], [to.x, to.y], &self.options),
					"divider",
				))),
				other => other.clone(),
			})
			.collect()
	}

	/// `dataStore`: the outline gives way to two lines at the top and bottom of its box. The
	/// node takes the size of what is drawn in the end.
	fn datastore(&self, x: f64, y: f64, width: f64, height: f64) -> Vec<Part> {
		let rectangle = rough::rectangle(x, y, width, height, &self.options);
		let outline = rectangle.paths.iter().find(|path| path.kind == PathKind::Outline);
		let bounds = outline.and_then(|path| path.bounds);
		let mut parts = Vec::with_capacity(3);
		let mut fill = rectangle;
		fill.paths.retain(|path| path.kind != PathKind::Outline);
		parts.push(Part::Sketch(Box::new(SketchPart::new(fill, "basic label-container"))));
		if let Some(bounds) = bounds.map(bounds_box) {
			for line_y in [bounds.min.y, bounds.max.y] {
				let line =
					rough::line([bounds.min.x, line_y], [bounds.max.x, line_y], &self.options);
				parts.push(Part::Sketch(Box::new(SketchPart::new(line, ""))));
			}
		}
		parts
	}

	/// `icon`, `iconSquare`, `iconCircle` and `iconRounded`: the background of the icon and the
	/// transparent rectangle around icon and label. The plain icon's background has no paint;
	/// both invisible sketches only give the node its size.
	fn icon(&self, parts: &[Part], styles: &Styles) -> Vec<Part> {
		let mut sketched = Vec::with_capacity(parts.len() + 2);
		for part in parts {
			match *part {
				Part::IconBackground { .. } => {}
				Part::IconArea { width, height, background_size: size, background_y } => {
					let mut options = self.options.clone();
					let corner = -size / 2.0;
					let (background, class) = match self.name {
						"icon" => {
							options.stroke = "none".to_owned();
							options.fill = Some("none".to_owned());
							(rough::rectangle(corner, corner, size, size, &options), "")
						}
						name => {
							let fill = styles.get("fill").unwrap_or(&self.theme.plain_main_bkg);
							options.stroke = fill.to_owned();
							match name {
								"iconCircle" => (rough::circle(0.0, 0.0, size, &options), ""),
								"iconRounded" => {
									let d = rounded_rect_path(corner, corner, size, size, 5.0);
									(rough::path(&d, &options), "icon-shape2")
								}
								_ => {
									let d = rounded_rect_path(corner, corner, size, size, 0.1);
									(rough::path(&d, &options), "")
								}
							}
						}
					};
					sketched.insert(
						0,
						Part::Sketch(Box::new(SketchPart {
							offset: Point { x: 0.0, y: background_y },
							is_measured_only: self.name == "icon",
							..SketchPart::new(background, class)
						})),
					);
					options.fill = Some("transparent".to_owned());
					options.stroke = "none".to_owned();
					sketched.push(self.measured_rectangle(width, height, &options));
					sketched.push(part.clone());
				}
				_ => sketched.push(part.clone()),
			}
		}
		sketched
	}

	/// `imageSquare`: the frame of the image and the rectangle without paint around image and
	/// label, which only gives the node its size.
	fn image(&self, parts: &[Part], bbox: BBox) -> Vec<Part> {
		let mut sketched = Vec::with_capacity(parts.len() + 1);
		for part in parts {
			match *part {
				Part::ImageFrame { x, y, width, height } => {
					let frame = rough::rectangle(x, -height / 2.0, width, height, &self.options);
					sketched.push(Part::Sketch(Box::new(SketchPart {
						offset: Point { x: 0.0, y: y + height / 2.0 },
						..SketchPart::new(frame, "")
					})));
					let mut options = self.options.clone();
					options.fill = Some("none".to_owned());
					options.stroke = "none".to_owned();
					sketched.push(self.measured_rectangle(bbox.width(), bbox.height(), &options));
				}
				_ => sketched.push(part.clone()),
			}
		}
		sketched
	}

	/// A centered rectangle that Mermaid draws without paint.
	fn measured_rectangle(&self, width: f64, height: f64, options: &Options) -> Part {
		let rectangle = rough::rectangle(-width / 2.0, -height / 2.0, width, height, options);
		Part::Sketch(Box::new(SketchPart {
			is_measured_only: true,
			..SketchPart::new(rectangle, "")
		}))
	}
}

/// The box `updateNodeBounds` takes from the sketches, with the other elements of the group.
fn measured_box(parts: &[Part]) -> Option<rough::Bounds> {
	let mut boxes = Vec::new();
	visit_sketches(parts, &mut |part| {
		if part.sketch.bounds().is_some() {
			boxes.extend(part.bounds());
		}
	});
	blink_path::union_all(boxes)
}

/// `.node path`, which hits the shapes of start and end states: they keep the class `node`.
fn node_paint(theme: &Theme) -> NodePaint {
	NodePaint {
		fill: theme.main_bkg.clone(),
		stroke: theme.node_border.clone(),
		stroke_width: theme.node_stroke_width.clone(),
	}
}

/// The parts of a shape as Mermaid draws them in the `handDrawn` look.
pub fn sketch_shape(
	mut shape: Shape,
	node: &Node,
	styles: &Styles,
	theme: &Theme,
	style: DiagramStyle,
	seed: i32,
) -> Shape {
	// Only the flowchart's style sheet has a `.node path` rule.
	let node_paint = |theme: &Theme| (style == DiagramStyle::Flowchart).then(|| node_paint(theme));
	let name = node.shape.as_str();
	let sketcher = Sketcher { options: user_node_overrides(styles, theme, seed), name, theme };
	let parts = std::mem::take(&mut shape.parts);
	let special = match (name, parts.as_slice()) {
		(_, [Part::Usecase(_), ..]) => Some(sketcher.usecase(&parts, node)),
		(
			"fr-rect" | "subprocess" | "subproc" | "framed-rectangle" | "subroutine",
			[Part::Polygon { points, .. }],
		) => Some(sketcher.subroutine(points)),
		("cyl" | "db" | "database" | "cylinder", [Part::Path { offset, .. }]) => {
			match shape.intersect {
				super::shapes::Intersect::Cylinder { rx, ry } => {
					Some(sketcher.cylinder(rx, ry, *offset, None))
				}
				_ => None,
			}
		}
		("lin-cyl" | "disk" | "lined-cylinder", [Part::Path { offset, .. }]) => {
			match shape.intersect {
				super::shapes::Intersect::Cylinder { rx, ry } => {
					let h = (-offset.y - ry) * 2.0;
					Some(sketcher.cylinder(rx, ry, *offset, Some(h * 0.1)))
				}
				_ => None,
			}
		}
		("h-cyl" | "das" | "horizontal-cylinder", [Part::Path { offset, .. }]) => {
			match shape.intersect {
				super::shapes::Intersect::HorizontalCylinder { rx, ry } => {
					Some(sketcher.horizontal_cylinder(rx, ry, *offset))
				}
				_ => None,
			}
		}
		("dbl-circ" | "double-circle" | "doublecircle", [Part::Group { parts, .. }]) => {
			match parts.as_slice() {
				[Part::Circle { radius: outer, .. }, Part::Circle { radius: inner, .. }] => {
					Some(sketcher.double_circle(*outer, *inner))
				}
				_ => None,
			}
		}
		("sm-circ" | "start" | "small-circle" | "stateStart", [_]) => {
			let options = solid_state_fill(&theme.line_color, seed);
			let circle = rough::circle(0.0, 0.0, 14.0, &options);
			let part = SketchPart {
				node_paint: node_paint(theme),
				..SketchPart::new(circle, "state-start")
			};
			Some(vec![Part::Sketch(Box::new(part))])
		}
		("fr-circ" | "stop" | "framed-circle" | "stateEnd", [_]) => {
			let mut parts = sketcher.state_end();
			if let Some(Part::Sketch(part)) = parts.first_mut() {
				part.node_paint = node_paint(theme);
			}
			Some(parts)
		}
		("f-circ" | "junction" | "filled-circle", [Part::Circle { radius, .. }]) => {
			let options = sketcher.with(|options| options.fill_style = FillStyle::Solid);
			let circle = rough::circle(0.0, 0.0, radius * 2.0, &options);
			Some(vec![Part::Sketch(Box::new(SketchPart {
				path_style: format!("fill: {} !important;", theme.plain_node_border),
				..SketchPart::new(circle, "")
			}))])
		}
		("fork" | "join" | "forkJoin", [Part::Rect { x, y, width, height, .. }]) => {
			let options = sketcher.with(|options| {
				options.stroke = theme.line_color.clone();
				options.fill = Some(theme.line_color.clone());
			});
			let rectangle = rough::rectangle(*x, *y, *width, *height, &options);
			Some(vec![Part::Sketch(Box::new(SketchPart::new(rectangle, "")))])
		}
		("datastore" | "data-store", [Part::Rect { x, y, width, height, .. }]) => {
			Some(sketcher.datastore(*x, *y, *width, *height))
		}
		(
			"collapsedGroup",
			&[Part::CollapsedGroup { width, height, separator_y, is_flow: false }],
		) => {
			let mut options = sketcher.options.clone();
			options.fill = Some(theme.cluster_bkg.clone());
			options.stroke = theme.cluster_border.clone();
			options.fill_style = FillStyle::Solid;
			let d = rounded_rect_path(-width / 2.0, -height / 2.0, width, height, 8.0);
			// `[data-color-id].rough-node .collapsed-group path` paints both paths.
			let slot = theme.color_slot(node);
			let node_paint = slot.map(|slot| NodePaint {
				stroke: theme.border_colors[slot].clone(),
				fill: theme
					.background_colors
					.get(slot % theme.background_colors.len().max(1))
					.cloned()
					.unwrap_or_default(),
				stroke_width: String::new(),
			});
			let part = SketchPart {
				// `attr('style', node.cssStyles)` writes the array with commas.
				style: node.css_styles.join(","),
				node_paint,
				..SketchPart::new(
					rough::path(&d, &options),
					"basic label-container collapsed-group",
				)
			};
			let marks = Part::CollapsedMarks { width, separator_y, is_flow: false };
			Some(vec![Part::Sketch(Box::new(part)), marks])
		}
		(
			"collapsedGroup",
			&[Part::CollapsedGroup { width, height, separator_y, is_flow: true }],
		) => {
			let mut options = sketcher.options.clone();
			flow_container_options(&mut options, theme);
			let d = rounded_rect_path(-width / 2.0, -height / 2.0, width, height, 10.0);
			let part = SketchPart {
				style: node.css_styles.join(","),
				..SketchPart::new(rough::path(&d, &options), "basic label-container flow-collapsed")
			};
			let marks = Part::CollapsedMarks { width, separator_y, is_flow: true };
			Some(vec![Part::Sketch(Box::new(part)), marks])
		}
		("text" | "labelRect" | "composite", _) => Some(parts.clone()),
		("icon" | "iconSquare" | "iconCircle" | "iconRounded", _) => {
			Some(sketcher.icon(&parts, styles))
		}
		("imageSquare", _) => Some(sketcher.image(&parts, shape.bbox)),
		("classBox", _) => Some(sketcher.class_box(&parts, &node.css_styles.join(";"))),
		("erBox", _) => Some(sketcher.er_box(&parts, node)),
		("requirementBox", _) => {
			Some(sketcher.requirement_box(&parts, &styles.node_css_important()))
		}
		(_, [part @ Part::Rough { .. }]) => sketcher.rough_part(part, shape.bbox),
		_ => None,
	};
	shape.parts = match special {
		Some(parts) => parts,
		None => {
			let is_converted = !parts
				.iter()
				.any(|part| matches!(part, Part::Rough { .. } | Part::Er(_) | Part::Class(_)));
			match is_converted {
				true => parts
					.into_iter()
					.enumerate()
					.map(|(index, part)| sketcher.part(part, index))
					.collect(),
				false => parts,
			}
		}
	};
	let nominal_height = shape.bbox.height();
	// The cells of a JSON table do not count, only its border (`updateNodeBounds`).
	let measured = match name {
		"usecaseJsonTable" => shape.parts.get(..1).and_then(measured_box),
		_ => measured_box(&shape.parts),
	};
	if let Some(bounds) = measured {
		let mut boxes = vec![bounds];
		include_other_elements(&shape.parts, &mut boxes);
		shape.bbox = blink_path::union_all(boxes).map_or(shape.bbox, bounds_box);
	}
	// The outlines of images and icons with a label reach from the top to the bottom of the
	// measured node, which the sketch makes taller.
	let is_asset =
		matches!(name, "icon" | "iconSquare" | "iconCircle" | "iconRounded" | "imageSquare");
	if is_asset && let super::shapes::Intersect::Polygon(points) = &mut shape.intersect {
		let growth = (shape.bbox.height() - nominal_height) / 2.0;
		for point in points {
			point.y += if point.y == nominal_height / 2.0 { growth } else { -growth };
		}
	}
	shape
}

/// The elements of a group that are not sketched, like the rim of a bucket.
fn include_other_elements(parts: &[Part], boxes: &mut Vec<rough::Bounds>) {
	for part in parts {
		match part {
			Part::Rim { center, rx, ry } => {
				// `LayoutSVGEllipse`: the center less the radii, the size twice the radii.
				let [cx, cy, rx, ry] = [center.x, center.y, *rx, *ry].map(blink_path::parse_number);
				boxes.push(rough::Bounds {
					x: cx - rx,
					y: cy - ry,
					width: rx * 2.0,
					height: ry * 2.0,
				});
			}
			Part::Group { parts, .. } => include_other_elements(parts, boxes),
			// `M x y L x y`, which can reach past the sketched ellipse.
			Part::Usecase(UsecasePart::BusinessMarker { d }) => {
				let numbers: Vec<f64> = d
					.split(|c: char| c.is_ascii_whitespace() || c == 'M' || c == 'L')
					.filter_map(|number| number.parse().ok())
					.collect();
				let commands = numbers.as_chunks::<2>().0.iter().enumerate().map(
					|(index, &point)| match index {
						0 => PathCommand::Move(point),
						_ => PathCommand::Line(point),
					},
				);
				boxes.extend(blink_path::path_bounds(commands));
			}
			_ => {}
		}
	}
}

/// Sketches a shape of a node in the `handDrawn` look, and leaves all others as they are.
pub fn apply(
	shape: Shape,
	context: &Context,
	node: &Node,
	styles: &Styles,
	theme: &Theme,
	style: DiagramStyle,
) -> Shape {
	match node.look {
		Look::HandDrawn => {
			let seed = context.sketch_seed(&node.id);
			sketch_shape(shape, node, styles, theme, style, seed)
		}
		_ => shape,
	}
}
