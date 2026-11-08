//! C4 diagrams (`c4Diagram.jison`, `c4Db.ts`, `c4Renderer.ts`, `svgDraw.ts`,
//! `c4ShapeAdapter.ts`, `c4LabelHelper.ts`, `styles.js`): elements in rows inside nested
//! boundaries, drawn with the flowchart's shapes and stacked text labels, and straight or
//! curved relationships between them.

mod db;
mod parser;

use super::common::{self, BlinkBox};
use super::journey::css_font_size;
use super::label::{self, LabelBox, decode_entities_to_text, encode_entities};
use super::rendering::intersect::{self, Bounds};
use super::rendering::shapes::{self, Intersect, Part, Shape};
use super::rendering::{DiagramStyle, Node, Point, Styles, Theme, sketch, translate};
use super::svg::{self, Accessibility, Anchor, Baseline, Frame, Svg, TextRun, number};
use super::text::{Font, parse_font_size};
use super::value::Value;
use super::{Context, Diagram, Error, Look, Result};
use db::{C4Db, Element, Endpoint, GLOBAL, Rel};

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	// `init` hands the top-level `wrap` to the database before the parser runs.
	let wrap = matches!(context.config.get("wrap"), Some(Value::Bool(true)));
	let mut db = C4Db::new(wrap);
	parser::parse(&code, &mut db).map_err(|error| context.error_at(error.line, error.message))?;
	if db.rels.len() > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} edges found, but the limit is {}.",
			db.rels.len(),
			context.limits.max_edges
		)));
	}
	let conf = Conf::new(context);
	let mut renderer = Renderer {
		context,
		conf,
		db,
		body: Svg::new(),
		shape_places: Vec::new(),
		boundary_places: Vec::new(),
		global_max: (0.0, 0.0),
	};
	renderer.draw()
}

/// The `c4` configuration.
struct Conf {
	diagram_margin_x: f64,
	diagram_margin_y: f64,
	shape_margin: f64,
	shape_padding: f64,
	width: f64,
	next_line_padding_x: f64,
	use_max_width: bool,
	/// `c4.wrap`: element labels wrap, and boundaries and relationships where the top-level
	/// `wrap` is set too.
	wrap: bool,
	boundary_font: Font,
	message_font: Font,
	/// `flowchart.wrappingWidth`, where labels wrap without an element width.
	wrapping_width: f64,
}

/// A font of the configuration: `<prefix>FontFamily`, `<prefix>FontSize`, `<prefix>FontWeight`,
/// each of them `default`'s where it is not set.
fn config_font(context: &Context, prefix: &str, default: Font) -> Font {
	let text = |key: &str| {
		context.config_text("c4", &format!("{prefix}{key}")).filter(|value| !value.is_empty())
	};
	let mut font = default;
	if let Some(family) = text("FontFamily") {
		font.family = family;
	}
	let size = match context.config_value("c4", &format!("{prefix}FontSize")) {
		Some(Value::Number(size)) => Some(*size),
		_ => text("FontSize").and_then(|size| parse_font_size(&size, 16.0)),
	};
	if let Some(size) = size {
		font.size = size;
	}
	if let Some(weight) = text("FontWeight") {
		font.weight = font_weight(&weight);
	}
	font
}

/// `C4_ELEMENT_TYPES` without their `external_` variants.
const ELEMENT_TYPES: [&str; 10] = [
	"person",
	"system",
	"system_db",
	"system_queue",
	"container",
	"container_db",
	"container_queue",
	"component",
	"component_db",
	"component_queue",
];

/// A CSS `font-weight`.
fn font_weight(weight: &str) -> u16 {
	match weight.trim() {
		"bold" | "bolder" => 700,
		weight => weight.parse().unwrap_or(400),
	}
}

impl Conf {
	fn new(context: &Context) -> Self {
		let number = |key: &str, default: f64| context.config_number("c4", key, default);
		let default_font = || Font::new("\"Open Sans\", sans-serif", 14.0);
		let mut message_font =
			config_font(context, "message", Font { size: 12.0, ..default_font() });
		// `setConf` puts the section's own font settings on the message font.
		if let Some(family) = context.config_text("c4", "fontFamily").filter(|f| !f.is_empty()) {
			message_font.family = family;
		}
		if let Some(size) = context.config_value("c4", "fontSize").and_then(Value::as_number) {
			message_font.size = size;
		}
		if let Some(weight) = context.config_text("c4", "fontWeight") {
			message_font.weight = font_weight(&weight);
		}
		Self {
			diagram_margin_x: number("diagramMarginX", 50.0),
			diagram_margin_y: number("diagramMarginY", 10.0),
			shape_margin: number("c4ShapeMargin", 50.0),
			shape_padding: number("c4ShapePadding", 20.0),
			width: number("width", 216.0),
			next_line_padding_x: number("nextLinePaddingX", 0.0),
			use_max_width: context.config_bool("c4", "useMaxWidth", true),
			wrap: context.config_bool("c4", "wrap", true),
			boundary_font: config_font(context, "boundary", default_font()),
			message_font,
			wrapping_width: context.config_number("flowchart", "wrappingWidth", 200.0),
		}
	}
}

/// A measured text of a boundary or relationship (`calcC4ShapeTextWH`): its lines, as
/// `<br>` breaks them, and its size.
#[derive(Clone, Debug, Default)]
struct MeasuredText {
	text: String,
	width: f64,
	height: f64,
	/// `Y`: the offset below the top of the boundary.
	y: f64,
}

/// `Bounds.data` and `Bounds.nextData`: unset values are `None`, like JavaScript's undefined.
#[derive(Clone, Copy, Debug, Default)]
struct Extent {
	startx: Option<f64>,
	stopx: Option<f64>,
	starty: Option<f64>,
	stopy: Option<f64>,
}

impl Extent {
	fn set(&mut self, startx: f64, stopx: f64, starty: f64, stopy: f64) {
		*self = Extent {
			startx: Some(startx),
			stopx: Some(stopx),
			starty: Some(starty),
			stopy: Some(stopy),
		};
	}

	/// `updateVal` for every side.
	fn grow(&mut self, startx: f64, starty: f64, stopx: f64, stopy: f64) {
		let update = |slot: &mut Option<f64>, value: f64, function: fn(f64, f64) -> f64| {
			*slot = Some(match *slot {
				None => value,
				Some(old) => function(value, old),
			});
		};
		update(&mut self.startx, startx, js_min);
		update(&mut self.starty, starty, js_min);
		update(&mut self.stopx, stopx, js_max);
		update(&mut self.stopy, stopy, js_max);
	}
}

/// `Math.min`, which is `NaN` if either value is.
fn js_min(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

fn js_max(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// An undefined value in arithmetic.
fn defined(value: Option<f64>) -> f64 {
	value.unwrap_or(f64::NAN)
}

/// The layout state of a boundary's contents (`Bounds`).
#[derive(Clone, Debug, Default)]
struct Area {
	data: Extent,
	next: Extent,
	count: f64,
	width_limit: f64,
}

impl Area {
	fn set_data(&mut self, x: f64, y: f64) {
		self.data.set(x, x, y, y);
		self.next.set(x, x, y, y);
	}
}

/// Where an element was drawn.
#[derive(Clone, Debug)]
struct Place {
	x: f64,
	y: f64,
	width: f64,
	height: f64,
	/// `None` for boundaries, which meet relationships at their box.
	intersect: Option<Intersect>,
}

impl Place {
	fn center(&self) -> Point {
		Point { x: self.x + self.width / 2.0, y: self.y + self.height / 2.0 }
	}

	fn intersect(&self, point: Point) -> Point {
		let center = self.center();
		let bounds = Bounds { x: center.x, y: center.y, width: self.width, height: self.height };
		match &self.intersect {
			Some(shape) => shape.point(bounds, point),
			None => intersect::rect(bounds, point),
		}
	}
}

/// The label of an element: its sections (name, type, description), each with its lines.
struct ElementLabel {
	sections: Vec<Section>,
	width: f64,
	height: f64,
}

struct Section {
	/// The `<tspan>` texts of each line: the first word, then every further word with the space
	/// before it. Blink shapes them one by one, so there is no kerning between words.
	lines: Vec<Vec<String>>,
	font: Font,
	/// The section's `translate` inside the label.
	offset: Point,
}

struct Renderer<'c, 'm> {
	context: &'c mut Context<'m>,
	conf: Conf,
	db: C4Db,
	body: Svg,
	shape_places: Vec<Option<Place>>,
	boundary_places: Vec<Option<Place>>,
	global_max: (f64, f64),
}

impl Renderer<'_, '_> {
	fn draw(&mut self) -> Result<Diagram> {
		self.shape_places = vec![None; self.db.shapes.len()];
		self.boundary_places = vec![None; self.db.boundaries.len()];
		let (margin_x, margin_y) = (self.conf.diagram_margin_x, self.conf.diagram_margin_y);
		let mut screen = Area::default();
		screen.data.set(margin_x, margin_x, margin_y, margin_y);
		screen.next = screen.data;
		screen.width_limit = self.context.screen_width;
		self.global_max = (margin_x, margin_y);
		let top_level = self.children("");
		self.check_depth(&top_level)?;
		self.draw_inside_boundary(&mut screen, &top_level);
		self.write_markers();
		self.draw_rels()?;
		let (start_x, start_y) = (margin_x, margin_y);
		let box_width = self.global_max.0 - start_x;
		let box_height = self.global_max.1 - start_y;
		let width = box_width + 2.0 * margin_x;
		let height = box_height + 2.0 * margin_y;
		let title = self.db.title.clone();
		if !title.is_empty() {
			let font = Font::new(self.context.font_family(), self.context.font_size());
			let run = TextRun {
				x: box_width / 2.0 - 4.0 * margin_x,
				y: start_y + margin_y,
				anchor: Anchor::Start,
				baseline: Baseline::Alphabetic,
				font: &font,
				content: &decode_entities_to_text(&title),
			};
			self.body.text(run, |_| {});
		}
		let extra = if title.is_empty() { 0.0 } else { 60.0 };
		let frame = Frame {
			x: start_x - margin_x,
			y: -(margin_y + extra),
			width,
			height: height + extra,
			use_max_width: self.conf.use_max_width,
		};
		let accessibility = Accessibility {
			description: self.db.acc_description.clone(),
			..Accessibility::default()
		};
		let body = std::mem::take(&mut self.body);
		Ok(svg::document(self.context, "c4", &frame, &accessibility, body))
	}

	/// `getBoundaries(parent)`.
	fn children(&self, parent: &str) -> Vec<usize> {
		(0..self.db.boundaries.len())
			.filter(|&index| self.db.boundaries[index].parent_boundary == parent)
			.collect()
	}

	/// Refuses boundary trees deeper than the grammar allows: redefining a boundary inside
	/// another one moves it there, which can chain boundaries of separate blocks.
	fn check_depth(&self, top_level: &[usize]) -> Result<()> {
		let mut stack: Vec<(usize, usize)> = top_level.iter().map(|&index| (index, 1)).collect();
		while let Some((index, depth)) = stack.pop() {
			if depth > parser::MAX_NESTING + 1 {
				return Err(Error::new(format!(
					"Boundaries nested more than {} levels deep",
					parser::MAX_NESTING
				)));
			}
			let alias = &self.db.boundaries[index].alias;
			stack.extend(self.children(alias).into_iter().map(|child| (child, depth + 1)));
		}
		Ok(())
	}

	// Text measurement.

	/// `calculateTextWidth`.
	fn text_width(&mut self, text: &str, font: &Font) -> f64 {
		self.context.text_dimensions(text, font).0
	}

	/// `calcC4ShapeTextWH`.
	fn measure_text(&mut self, text: &str, wraps: bool, font: &Font, limit: f64) -> MeasuredText {
		if wraps {
			let context = &mut *self.context;
			let text =
				common::wrap_label(text, limit, &mut |line| context.text_dimensions(line, font).0);
			let height = self.context.text_dimensions(&text, font).1;
			return MeasuredText { text, width: limit, height, y: 0.0 };
		}
		let mut measured = MeasuredText { text: text.to_owned(), ..MeasuredText::default() };
		for line in common::split_breaks(text) {
			let (width, height) = self.context.text_dimensions(line, font);
			measured.width = js_max(width, measured.width);
			measured.height += height;
		}
		measured
	}

	// Boundaries.

	/// `drawInsideBoundary`.
	fn draw_inside_boundary(&mut self, parent: &mut Area, boundaries: &[usize]) {
		let mut area = Area {
			width_limit: parent.width_limit
				/ self.db.boundaries_in_row.min(boundaries.len() as f64),
			..Area::default()
		};
		let in_row = self.db.boundaries_in_row;
		for (position, &index) in boundaries.iter().enumerate() {
			let boundary = self.db.boundaries[index].clone();
			let wraps = boundary.wrap && self.conf.wrap;
			let mut y = if boundary.sprite.is_some() { 48.0 } else { 0.0 };
			let mut label_font = self.conf.boundary_font.clone();
			label_font.size += 2.0;
			label_font.weight = 700;
			let mut label =
				self.measure_text(&boundary.label, wraps, &label_font, area.width_limit);
			label.y = y + 8.0;
			y = label.y + label.height;
			let mut kind = None;
			if let Some(text) = boundary.kind.as_ref().filter(|text| !text.is_empty()) {
				let font = self.conf.boundary_font.clone();
				let mut measured =
					self.measure_text(&format!("[{text}]"), wraps, &font, area.width_limit);
				measured.y = y + 5.0;
				y = measured.y + measured.height;
				kind = Some(measured);
			}
			let mut descr = None;
			if let Some(text) = boundary.descr.as_ref().filter(|text| !text.is_empty()) {
				let mut font = self.conf.boundary_font.clone();
				font.size -= 2.0;
				let mut measured = self.measure_text(text, wraps, &font, area.width_limit);
				measured.y = y + 20.0;
				y = measured.y + measured.height;
				descr = Some(measured);
			}
			if position == 0 || position as f64 % in_row == 0.0 {
				let x = defined(parent.data.startx) + self.conf.diagram_margin_x;
				let y = defined(parent.data.stopy) + self.conf.diagram_margin_y + y;
				area.set_data(x, y);
			} else {
				let x = match area.data.stopx != area.data.startx {
					true => defined(area.data.stopx) + self.conf.diagram_margin_x,
					false => defined(area.data.startx),
				};
				let y = defined(area.data.starty);
				area.set_data(x, y);
			}
			let shapes: Vec<usize> = (0..self.db.shapes.len())
				.filter(|&shape| self.db.shapes[shape].parent_boundary == boundary.alias)
				.collect();
			if !shapes.is_empty() {
				self.draw_shapes(&mut area, &shapes);
			}
			let children = self.children(&boundary.alias);
			if !children.is_empty() {
				self.draw_inside_boundary(&mut area, &children);
			}
			if boundary.alias != GLOBAL {
				self.draw_boundary(index, &boundary, &area, [Some(label), kind, descr]);
			}
			let margin = self.conf.shape_margin;
			parent.data.stopy =
				Some(js_max(defined(area.data.stopy) + margin, defined(parent.data.stopy)));
			parent.data.stopx =
				Some(js_max(defined(area.data.stopx) + margin, defined(parent.data.stopx)));
			self.global_max.0 = js_max(self.global_max.0, defined(parent.data.stopx));
			self.global_max.1 = js_max(self.global_max.1, defined(parent.data.stopy));
		}
	}

	/// `drawBoundary`: the box of the boundary's area and its texts.
	fn draw_boundary(
		&mut self,
		index: usize,
		boundary: &Element,
		area: &Area,
		texts: [Option<MeasuredText>; 3],
	) {
		let x = defined(area.data.startx);
		let y = defined(area.data.starty);
		let width = defined(area.data.stopx) - x;
		let height = defined(area.data.stopy) - y;
		self.boundary_places[index] = Some(Place { x, y, width, height, intersect: None });
		self.body.open("g").end();
		self.body
			.open("rect")
			.number_attr("x", x)
			.number_attr("y", y)
			.attr("fill", boundary.bg_color.as_deref().unwrap_or("none"))
			.attr("stroke", boundary.border_color.as_deref().unwrap_or("#444444"))
			.number_attr("width", width)
			.number_attr("height", height)
			.attr("rx", "2.5")
			.attr("ry", "2.5")
			.attr("stroke-width", "1");
		if boundary.node_type.is_none() {
			self.body.attr("stroke-dasharray", "7.0,7.0");
		}
		self.body.close();
		let base = self.conf.boundary_font.clone();
		let fonts = [
			Font { size: base.size + 2.0, weight: 700, ..base.clone() },
			base.clone(),
			Font { size: base.size - 2.0, ..base },
		];
		for (text, font) in texts.iter().zip(&fonts) {
			if let Some(text) = text {
				let center = x + width / 2.0;
				self.draw_lines(&text.text, center, y + text.y, font, "#444444", false);
			}
		}
		self.body.close_tag("g");
	}

	/// `byTspan`: the lines of `text` centered on `x` and around `y`, each on its mathematical
	/// baseline (Blink: half the rounded ascent below the line's middle).
	fn draw_lines(&mut self, text: &str, x: f64, y: f64, font: &Font, fill: &str, is_italic: bool) {
		let lines = common::split_breaks(text);
		let count = lines.len() as f64;
		let ascent = self.context.vertical_metrics(font).0.round();
		let font = Font { is_italic, ..font.clone() };
		for (index, line) in lines.iter().enumerate() {
			let dy = index as f64 * font.size - font.size * (count - 1.0) / 2.0;
			let run = TextRun {
				x,
				y: y + dy + ascent / 2.0,
				anchor: Anchor::Middle,
				baseline: Baseline::Alphabetic,
				font: &font,
				content: &decode_entities_to_text(line),
			};
			self.body.text(run, |svg| _ = svg.attr("fill", fill));
		}
	}

	// Elements.

	/// The node of the shared shapes for `element` (`buildC4Node`).
	fn node(&self, element: &Element) -> Node {
		Node {
			id: element.alias.clone(),
			dom_id: element.alias.clone(),
			shape: resolve_shape(element).to_owned(),
			padding: self.conf.shape_padding,
			look: self.context.look,
			width: Some(self.conf.width).filter(|width| *width != 0.0 && !width.is_nan()),
			..Node::default()
		}
	}

	/// The fonts of an element's label: `.c4-shape.c4-<type> .label` from the per-type
	/// configuration, else the diagram's font.
	fn label_font(&mut self, element: &Element) -> Font {
		let mut font = Font::new(self.context.font_family(), self.context.font_size());
		let prefix = &element.type_c4_shape;
		let type_name = prefix.strip_prefix("external_").unwrap_or(prefix);
		if !ELEMENT_TYPES.contains(&type_name) {
			return font;
		}
		// The schema defaults of every element type, unless the configuration replaces them; a
		// falsy value adds no declaration, so the label inherits the diagram's font.
		let value = |key: &str| self.context.config_value("c4", &format!("{prefix}{key}")).cloned();
		match value("FontFamily") {
			None => font.family = "\"Open Sans\", sans-serif".to_owned(),
			Some(family) => {
				if let Some(family) = family.to_text().filter(|family| !family.is_empty()) {
					font.family = family;
				}
			}
		}
		let weight = value("FontWeight");
		match value("FontSize") {
			None => font.size = 14.0,
			Some(Value::Number(size)) if size != 0.0 => font.size = size,
			Some(size) => {
				// `CSSStyleDeclaration.setProperty` drops lengths without a unit.
				let size = size.to_text().filter(|size| size.trim().parse::<f64>().is_err());
				let parent = font.clone();
				if let Some(size) =
					size.and_then(|size| css_font_size(self.context, &size, &parent))
				{
					font.size = size;
				}
			}
		}
		if let Some(weight) = weight.as_ref().and_then(Value::to_text) {
			font.weight = font_weight(&weight);
		}
		font
	}

	/// `c4LabelHelper`: the sections of the label, each wrapped at `wrap_width`, stacked.
	fn element_label(&mut self, element: &Element, wrap_width: f64) -> ElementLabel {
		let font = self.label_font(element);
		let name_font = Font { weight: 700, ..font.clone() };
		let type_font = Font { size: font.size * 0.75, ..font.clone() };
		let descr_font = Font { size: font.size * 0.82, ..font.clone() };
		let mut texts = vec![(element.label.clone(), name_font), (stereotype(element), type_font)];
		if let Some(descr) = element.descr.as_ref().filter(|descr| !descr.is_empty()) {
			texts.push((descr.clone(), descr_font));
		}
		let width = if self.conf.wrap { wrap_width } else { f64::INFINITY };
		let mut sections = Vec::new();
		let mut boxes = Vec::new();
		for (text, font) in texts.into_iter().filter(|(text, _)| !text.is_empty()) {
			let text = common::sanitize_text(&label::decode_entities(&text));
			let lines = self.wrap_lines(&text, &font, width);
			let mut bbox = BlinkBox::default();
			for (index, line) in lines.iter().enumerate() {
				let baseline = (1.0 + 1.1 * index as f64) * font.size;
				let mut x = -self.segments_width(line, &font) / 2.0;
				for segment in line {
					let measurer = &mut *self.context.measurer;
					let advance = measurer.measure(segment, &font).0;
					let (box_width, box_height) = measurer.bounding_box(segment, &font);
					let left = x + measurer.bounding_box_left(segment, &font);
					let top = baseline + measurer.bounding_box_top(segment, &font);
					bbox.include(left, top, box_width, box_height);
					x += advance;
				}
			}
			boxes.push(bbox.extent.unwrap_or_default());
			sections.push(Section { lines, font, offset: Point::default() });
		}
		let total_width = boxes.iter().map(|(left, _, right, _)| right - left).fold(0.0, f64::max);
		let mut y = 0.0;
		for (section, (left, top, right, bottom)) in sections.iter_mut().zip(&boxes) {
			let width = right - left;
			section.offset = Point { x: total_width / 2.0 - left - width / 2.0, y: y - top };
			y += bottom - top + SECTION_GAP;
		}
		let height = if sections.is_empty() { 0.0 } else { y - SECTION_GAP };
		ElementLabel { sections, width: total_width, height }
	}

	/// `createText` without HTML: the lines of `text` (`nonMarkdownToLines`), each split into
	/// lines that fit `width` (`splitLineToFitWidth`). Words are measured in normal weight,
	/// which their `<tspan>`s have until the label helper removes it.
	fn wrap_lines(&mut self, text: &str, font: &Font, width: f64) -> Vec<Vec<String>> {
		let measure_font = Font { weight: 400, is_italic: false, ..font.clone() };
		let fits = |context: &mut Context, words: &[String]| {
			let segments = word_segments(words);
			segments
				.iter()
				.map(|segment| context.measure_exact(segment, &measure_font))
				.sum::<f64>()
				<= width
		};
		let mut lines = Vec::new();
		for line in non_markdown_lines(text) {
			if fits(self.context, &line) {
				lines.push(word_segments(&line));
				continue;
			}
			let mut words: std::collections::VecDeque<String> = line.into();
			let mut current: Vec<String> = Vec::new();
			while let Some(word) = words.pop_front() {
				current.push(word);
				if fits(self.context, &current) {
					continue;
				}
				let word = current.pop().unwrap_or_default();
				if !current.is_empty() {
					lines.push(word_segments(&current));
					current.clear();
					words.push_front(word);
					continue;
				}
				// `splitWordToFitWidth`: as many characters as fit, at least one.
				let characters: Vec<char> = word.chars().collect();
				let mut used = 0;
				while used < characters.len() {
					let candidate: String = characters[..=used].iter().collect();
					if !fits(self.context, &[candidate]) {
						break;
					}
					used += 1;
				}
				let used = used.max(1).min(characters.len());
				lines.push(vec![characters[..used].iter().collect()]);
				let rest: String = characters[used..].iter().collect();
				if !rest.is_empty() {
					words.push_front(rest);
				}
			}
			if !current.is_empty() {
				lines.push(word_segments(&current));
			}
		}
		lines
	}

	/// The advance of a line of `<tspan>` segments, the length of its text chunk.
	fn segments_width(&mut self, segments: &[String], font: &Font) -> f64 {
		segments.iter().map(|segment| self.context.measure_exact(segment, font)).sum()
	}

	/// The label and shape of `element`.
	fn element_shape(&mut self, element: &Element) -> (Node, ElementLabel, Shape) {
		let node = self.node(element);
		let wrap_width = match shapes::label_width(&node) {
			Some(width) => (width - 2.0 * node.padding).max(MIN_WRAP_WIDTH),
			None => self.conf.wrapping_width,
		};
		let label = self.element_label(element, wrap_width);
		let label_box = LabelBox {
			lines: Vec::new(),
			width: label.width,
			height: label.height,
			line_height: 0.0,
			font: Font::new(self.context.font_family(), self.context.font_size()),
			is_left_aligned: false,
			is_underlined: false,
		};
		let shape = shapes::shape(&node, &label_box, 5.0, false, DiagramStyle::Flowchart);
		let shape = match node.look {
			Look::HandDrawn => self.sketch(element, node.clone(), shape),
			_ => shape,
		};
		(node, label, shape)
	}

	/// The shape of `element` in the `handDrawn` look, painted by its `cssStyles`
	/// (`elementCssStyles`).
	fn sketch(&self, element: &Element, mut node: Node, shape: Shape) -> Shape {
		let type_c4_shape = &element.type_c4_shape;
		let fill = element.bg_color.clone().or_else(|| self.element_color(type_c4_shape, "bg"));
		let stroke =
			element.border_color.clone().or_else(|| self.element_color(type_c4_shape, "border"));
		node.css_styles.extend(fill.map(|fill| format!("fill:{fill}")));
		node.css_styles.extend(stroke.map(|stroke| format!("stroke:{stroke}")));
		let font_color = element.font_color.as_deref().unwrap_or("#FFFFFF");
		node.css_styles.push(format!("color:{font_color}"));
		if matches!(node.shape.as_str(), "rounded" | "fr-rect") {
			node.css_styles.extend(["rx:12px".to_owned(), "ry:12px".to_owned()]);
		}
		let styles = Styles::compile(&[&node.css_styles]);
		let theme = Theme::new(self.context, DiagramStyle::Flowchart);
		let mut shape =
			sketch::apply(shape, self.context, &node, &styles, &theme, DiagramStyle::Flowchart);
		widen_sketches(&mut shape.parts);
		shape
	}

	/// `drawC4ShapeArray`: measures the elements, places them in rows and draws them.
	fn draw_shapes(&mut self, area: &mut Area, indices: &[usize]) {
		let margin = self.conf.shape_margin;
		let mut drawings = Vec::new();
		for &index in indices {
			let element = self.db.shapes[index].clone();
			let (node, label, shape) = self.element_shape(&element);
			let (width, height) = (shape.bbox.width(), shape.bbox.height());
			// `Bounds.insert`.
			area.count += 1.0;
			let next_stopx = defined(area.next.stopx);
			let mut startx = match area.next.startx == area.next.stopx {
				true => next_stopx + margin,
				false => next_stopx + margin * 2.0,
			};
			let mut stopx = startx + width;
			let mut starty = defined(area.next.starty) + margin * 2.0;
			let mut stopy = starty + height;
			let limit = area.width_limit;
			if startx >= limit || stopx >= limit || area.count > self.db.shapes_in_row {
				startx = defined(area.next.startx) + margin + self.conf.next_line_padding_x;
				starty = defined(area.next.stopy) + margin * 2.0;
				stopx = startx + width;
				area.next.stopx = Some(stopx);
				area.next.starty = area.next.stopy;
				stopy = starty + height;
				area.next.stopy = Some(stopy);
				area.count = 1.0;
			}
			area.data.grow(startx, starty, stopx, stopy);
			area.next.grow(startx, starty, stopx, stopy);
			self.shape_places[index] = Some(Place {
				x: startx,
				y: starty,
				width,
				height,
				intersect: Some(shape.intersect.clone()),
			});
			drawings.push((index, element, node, label, shape));
		}
		for (index, element, node, label, shape) in drawings {
			if let Some(place) = self.shape_places[index].clone() {
				self.write_shape(&element, &node, &label, &shape, place.center());
			}
		}
		// `bumpLastMargin`.
		area.data.stopx = area.data.stopx.map(|stopx| stopx + margin);
		area.data.stopy = area.data.stopy.map(|stopy| stopy + margin);
	}

	fn write_shape(
		&mut self,
		element: &Element,
		node: &Node,
		label: &ElementLabel,
		shape: &Shape,
		center: Point,
	) {
		let type_c4_shape = &element.type_c4_shape;
		let node_class = match node.look {
			Look::HandDrawn => "rough-node",
			_ => "node",
		};
		let mut class = format!("{node_class} c4-shape c4-{type_c4_shape}");
		if type_c4_shape.starts_with("external_") {
			class.push_str(" c4-external");
		}
		let paint = Paint {
			fill: element.bg_color.clone().or_else(|| self.element_color(type_c4_shape, "bg")),
			stroke: element
				.border_color
				.clone()
				.or_else(|| self.element_color(type_c4_shape, "border")),
			// The rounded shapes' inline `rx: 12px` wins over the corner radius.
			radius: matches!(node.shape.as_str(), "rounded" | "fr-rect").then_some(12.0),
		};
		self.body.open("g").attr("transform", &translate(center.x, center.y)).end();
		self.body
			.open("g")
			.attr("class", &class)
			.attr("id", &format!("{}-{}", self.context.id, node.dom_id))
			.end();
		for part in &shape.parts {
			write_part(&mut self.body, part, &paint);
		}
		let font_color = element.font_color.clone().unwrap_or_else(|| "#FFFFFF".to_owned());
		let top_left = Point {
			x: shape.label_center.x - label.width / 2.0,
			y: shape.label_center.y - label.height / 2.0,
		};
		self.body
			.open("g")
			.attr("class", "label")
			.attr("transform", &translate(top_left.x, top_left.y))
			.end();
		for section in &label.sections {
			self.body
				.open("g")
				.attr("transform", &translate(section.offset.x, section.offset.y))
				.end();
			for (index, line) in section.lines.iter().enumerate() {
				let mut x = -self.segments_width(line, &section.font) / 2.0;
				for segment in line {
					let advance = self.context.measure_exact(segment, &section.font);
					// The space before the word is not written: the word starts after it.
					let word = segment.trim_start();
					let space = advance - self.context.measure_exact(word, &section.font);
					let run = TextRun {
						x: x + space,
						y: (1.0 + 1.1 * index as f64) * section.font.size,
						anchor: Anchor::Start,
						baseline: Baseline::Alphabetic,
						font: &section.font,
						content: word,
					};
					self.body.text(run, |svg| _ = svg.attr("fill", &font_color));
					x += advance;
				}
			}
			self.body.close_tag("g");
		}
		self.body.close_tag("g").close_tag("g").close_tag("g");
	}

	/// `<type>_bg_color` or `<type>_border_color` of the configuration.
	fn element_color(&self, type_c4_shape: &str, which: &str) -> Option<String> {
		let key = format!("{type_c4_shape}_{which}_color");
		match self.context.config_value("c4", &key) {
			Some(value) => value.to_text().filter(|color| !color.is_empty()),
			None => default_color(type_c4_shape, which == "bg").map(str::to_owned),
		}
	}

	// Relationships.

	fn write_markers(&mut self) {
		for (suffix, ref_x, d) in [
			("arrowhead", "9", "M 0 0 L 10 5 L 0 10 z"),
			("arrowend", "1", "M 10 0 L 0 5 L 10 10 z"),
		] {
			self.body.open("defs").end();
			self.body
				.open("marker")
				.attr("id", &format!("{}-{suffix}", self.context.id))
				.attr("refX", ref_x)
				.attr("refY", "5")
				.attr("markerUnits", "userSpaceOnUse")
				.attr("markerWidth", "12")
				.attr("markerHeight", "12")
				.attr("orient", "auto")
				.end();
			self.body.open("path").attr("d", d).close();
			self.body.close_tag("marker").close_tag("defs");
		}
	}

	/// Where `alias` was drawn, or Mermaid's error for an end without a shape.
	fn place(&self, endpoint: Endpoint, alias: &str) -> Result<&Place> {
		let place = match endpoint {
			Endpoint::Shape(index) => self.shape_places[index].as_ref(),
			Endpoint::Boundary(index) => self.boundary_places[index].as_ref(),
		};
		place.ok_or_else(|| {
			Error::new(format!(
				"C4 element \"{alias}\" has no intersect function. Please report this to \
				 https://github.com/mermaid-js/mermaid/issues"
			))
		})
	}

	/// `drawRels`: the ends of each relationship on its shapes, then the lines and labels.
	fn draw_rels(&mut self) -> Result<()> {
		let rels = std::mem::take(&mut self.db.rels);
		let is_dynamic = self.db.c4_type == "C4Dynamic";
		let font = self.conf.message_font.clone();
		let mut drawn = Vec::new();
		for (index, rel) in rels.iter().enumerate() {
			if rel.label_is_object && !is_dynamic {
				// Mermaid splits the label object into lines.
				return Err(Error::new("t.split is not a function"));
			}
			let wraps = rel.wrap && self.conf.wrap;
			let text = match is_dynamic {
				true => format!("{}: {}", index + 1, rel.label),
				false => rel.label.clone(),
			};
			let limit = self.text_width(&text, &font);
			let label = self.measure_text(&text, wraps, &font, limit);
			let techn = match rel.techn.is_empty() {
				true => None,
				false => {
					let limit = self.text_width(&rel.techn, &font);
					Some(self.measure_text(&rel.techn, wraps, &font, limit))
				}
			};
			let unknown = || {
				Error::new(format!(
					"C4 rel \"{}\" -> \"{}\" references an unknown shape or boundary",
					rel.from, rel.to
				))
			};
			let from = self.db.find(&rel.from).ok_or_else(unknown)?;
			let to = self.db.find(&rel.to).ok_or_else(unknown)?;
			let end_center = self.endpoint_center(to);
			let start = self.place(from, &rel.from)?.intersect(end_center);
			let start_center = self.endpoint_center(from);
			let end = self.place(to, &rel.to)?.intersect(start_center);
			drawn.push((rel, label, techn, start, end));
		}
		self.body.open("g").end();
		for (index, (rel, label, techn, start, end)) in drawn.into_iter().enumerate() {
			self.write_rel(index == 0, rel, &label, techn.as_ref(), start, end, &font);
		}
		self.body.close_tag("g");
		Ok(())
	}

	/// The center of an end, `NaN` where it was not drawn.
	fn endpoint_center(&self, endpoint: Endpoint) -> Point {
		let place = match endpoint {
			Endpoint::Shape(index) => self.shape_places[index].as_ref(),
			Endpoint::Boundary(index) => self.boundary_places[index].as_ref(),
		};
		place.map_or(Point { x: f64::NAN, y: f64::NAN }, Place::center)
	}

	#[allow(clippy::too_many_arguments)]
	fn write_rel(
		&mut self,
		is_first: bool,
		rel: &Rel,
		label: &MeasuredText,
		techn: Option<&MeasuredText>,
		start: Point,
		end: Point,
		font: &Font,
	) {
		let text_color = rel.text_color.clone().unwrap_or_else(|| "#444444".to_owned());
		let stroke = rel.line_color.clone().unwrap_or_else(|| "#444444".to_owned());
		let offset = |value: Option<f64>| value.filter(|value| *value != 0.0 && !value.is_nan());
		let offset_x = offset(rel.offset_x).unwrap_or(0.0);
		let offset_y = offset(rel.offset_y).unwrap_or(0.0);
		// The first relationship is a straight line, the others are curves.
		match is_first {
			true => self
				.body
				.open("line")
				.number_attr("x1", start.x)
				.number_attr("y1", start.y)
				.number_attr("x2", end.x)
				.number_attr("y2", end.y),
			false => {
				let control_x = start.x + (end.x - start.x) / 2.0 - (end.x - start.x) / 4.0;
				let control_y = start.y + (end.y - start.y) / 2.0;
				let d = format!(
					"M{},{} Q{},{} {},{}",
					number(start.x),
					number(start.y),
					number(control_x),
					number(control_y),
					number(end.x),
					number(end.y)
				);
				self.body.open("path").attr("d", &d)
			}
		};
		self.body.attr("stroke-width", "1").attr("stroke", &stroke).attr("fill", "none");
		if rel.kind != "rel_b" {
			self.body.attr("marker-end", &format!("url(#{}-arrowhead)", self.context.id));
		}
		if rel.kind == "birel" || rel.kind == "rel_b" {
			self.body.attr("marker-start", &format!("url(#{}-arrowend)", self.context.id));
		}
		self.body.close();
		let x = start.x.min(end.x) + (end.x - start.x).abs() / 2.0 + offset_x;
		let y = start.y.min(end.y) + (end.y - start.y).abs() / 2.0 + offset_y;
		// `byTspan` centers on `x + width / 2`.
		self.draw_lines(&label.text, x + label.width / 2.0, y, font, &text_color, false);
		if let Some(techn) = techn {
			let width = js_max(label.width, techn.width);
			let text = format!("[{}]", techn.text);
			let y = y + font.size + 5.0;
			self.draw_lines(&text, x + width / 2.0, y, font, &text_color, true);
		}
	}
}

const SECTION_GAP: f64 = 3.0;
const MIN_WRAP_WIDTH: f64 = 32.0;

/// The fill, stroke and corner radius of an element's shape.
struct Paint {
	fill: Option<String>,
	stroke: Option<String>,
	radius: Option<f64>,
}

fn write_paint(svg: &mut Svg, paint: &Paint) {
	if let Some(fill) = &paint.fill {
		svg.attr("fill", fill);
	}
	if let Some(stroke) = &paint.stroke {
		svg.attr("stroke", stroke);
	}
	// `.c4-shape rect, .c4-shape path, …`
	svg.attr("stroke-width", "2");
}

/// `.c4-shape path { stroke-width: 2px }` over the paths of roughjs.
fn widen_sketches(parts: &mut [Part]) {
	for part in parts {
		match part {
			Part::Sketch(part) => {
				part.node_paint = Some(sketch::NodePaint {
					fill: String::new(),
					stroke: String::new(),
					stroke_width: "2".to_owned(),
				});
			}
			Part::Group { parts, .. } => widen_sketches(parts),
			_ => {}
		}
	}
}

fn write_part(svg: &mut Svg, part: &Part, paint: &Paint) {
	match part {
		Part::Rect { x, y, width, height, rx, class } => {
			svg.open("rect").attr_if("class", class);
			write_paint(svg, paint);
			if let Some(rx) = paint.radius.or(*rx) {
				svg.number_attr("rx", rx).number_attr("ry", rx);
			}
			svg.number_attr("x", *x)
				.number_attr("y", *y)
				.number_attr("width", *width)
				.number_attr("height", *height)
				.close();
		}
		Part::Circle { center, radius, class } => {
			svg.open("circle").attr_if("class", class);
			write_paint(svg, paint);
			svg.number_attr("cx", center.x)
				.number_attr("cy", center.y)
				.number_attr("r", *radius)
				.close();
		}
		Part::Polygon { points, offset, class } => {
			let points: Vec<String> =
				points.iter().map(|p| format!("{},{}", number(p.x), number(p.y))).collect();
			svg.open("polygon").attr("points", &points.join(" ")).attr_if("class", class);
			write_paint(svg, paint);
			svg.attr("transform", &translate(offset.x, offset.y)).close();
		}
		Part::Path { d, offset, class } => {
			svg.open("path").attr("d", d).attr_if("class", class);
			write_paint(svg, paint);
			svg.attr("transform", &translate(offset.x, offset.y)).close();
		}
		Part::Group { parts, class } => {
			svg.open("g").attr_if("class", class).end();
			for part in parts {
				write_part(svg, part, paint);
			}
			svg.close_tag("g");
		}
		Part::Sketch(part) => sketch::write_part(svg, part),
		// The shapes of C4 elements consist of the parts above.
		_ => {}
	}
}

/// `nonMarkdownToLines`: lines at `\n`, line ends and `<br>`, each as its words (tags count
/// as words), with `&amp;`, `&lt;` and `&gt;` decoded like `decodeHTMLEntities`.
/// The `<tspan>` texts of a line of words: every word after the first keeps its space.
fn word_segments(words: &[String]) -> Vec<String> {
	let mut segments = Vec::with_capacity(words.len());
	for (index, word) in words.iter().enumerate() {
		segments.push(if index == 0 { word.clone() } else { format!(" {word}") });
	}
	segments
}

fn non_markdown_lines(text: &str) -> Vec<Vec<String>> {
	let mut lines = Vec::new();
	for chunk in text.split("\\n") {
		for chunk in chunk.split('\n') {
			for line in split_html_breaks(chunk) {
				let mut words = Vec::new();
				let mut rest = line.trim();
				while !rest.is_empty() {
					let length = match rest.strip_prefix('<').and_then(|tag| tag.find('>')) {
						Some(end) => end + 2,
						None if rest.starts_with(['<', '>']) => {
							rest = &rest[1..];
							continue;
						}
						None => rest
							.find(|c: char| c.is_whitespace() || c == '<' || c == '>')
							.unwrap_or(rest.len()),
					};
					if length == 0 {
						rest = rest.trim_start();
						continue;
					}
					let word = &rest[..length];
					words
						.push(word.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"));
					rest = &rest[length..];
				}
				lines.push(words);
			}
		}
	}
	lines
}

/// `/<br\s*\/?>/gi` splits.
fn split_html_breaks(text: &str) -> Vec<&str> {
	let mut parts = Vec::new();
	let mut start = 0;
	let lower = text.to_ascii_lowercase();
	let mut search = 0;
	while let Some(found) = lower[search..].find("<br") {
		let tag = search + found;
		let after = &lower[tag + 3..];
		let spaces = after.len() - after.trim_start().len();
		let after_spaces = &after[spaces..];
		let length = match after_spaces.as_bytes() {
			[b'/', b'>', ..] => Some(3 + spaces + 2),
			[b'>', ..] => Some(3 + spaces + 1),
			_ => None,
		};
		match length {
			Some(length) => {
				parts.push(&text[start..tag]);
				start = tag + length;
				search = start;
			}
			None => search = tag + 3,
		}
	}
	parts.push(&text[start..]);
	parts
}

/// The stereotype line of an element: `[Container: Node.js]` (`stereotypeText`).
fn stereotype(element: &Element) -> String {
	let base = element.type_c4_shape.strip_prefix("external_").unwrap_or(&element.type_c4_shape);
	let base = base.strip_suffix("_db").or_else(|| base.strip_suffix("_queue")).unwrap_or(base);
	let name = match base {
		"person" => "Person".to_owned(),
		"system" => "Software System".to_owned(),
		"container" => "Container".to_owned(),
		"component" => "Component".to_owned(),
		other => other.replace('_', " "),
	};
	match element.techn.as_deref().filter(|techn| !techn.is_empty()) {
		Some(techn) => format!("[{name}: {techn}]"),
		None => format!("[{name}]"),
	}
}

/// A shape keyword of `$shape`, `$sprite` or `$tags`.
fn keyword_shape(value: &str) -> Option<&'static str> {
	Some(match value.to_lowercase().as_str() {
		"person" => "person",
		"box" | "rounded" => "rounded",
		"cylinder" | "database" | "db" => "cylinder",
		"queue" | "pipe" => "h-cyl",
		"component" => "fr-rect",
		_ => return None,
	})
}

/// `resolveNodeShape`: `$shape`, `$sprite`, a `$tags` entry, then the element type.
fn resolve_shape(element: &Element) -> &'static str {
	let explicit = [&element.shape, &element.sprite]
		.into_iter()
		.find_map(|value| value.as_deref().and_then(keyword_shape));
	if let Some(shape) = explicit {
		return shape;
	}
	if let Some(shape) = element
		.tags
		.as_deref()
		.filter(|tags| !tags.is_empty())
		.and_then(|tags| tags.split(',').find_map(|tag| keyword_shape(tag.trim())))
	{
		return shape;
	}
	let kind = element.type_c4_shape.as_str();
	match kind {
		"person" | "external_person" => "person",
		_ if kind.ends_with("_db") => "cylinder",
		_ if kind.ends_with("_queue") => "h-cyl",
		_ => "rounded",
	}
}

/// The default colors of the element types.
fn default_color(type_c4_shape: &str, is_background: bool) -> Option<&'static str> {
	let base = type_c4_shape.strip_suffix("_db").or_else(|| type_c4_shape.strip_suffix("_queue"));
	let base = match base {
		// Persons have no database or queue variant.
		Some(base) if !base.ends_with("person") => base,
		Some(_) => return None,
		None => type_c4_shape,
	};
	let (background, border) = match base {
		"person" => ("#08427B", "#073B6F"),
		"external_person" => ("#686868", "#8A8A8A"),
		"system" => ("#1168BD", "#3C7FC0"),
		"external_system" => ("#999999", "#8A8A8A"),
		"container" => ("#438DD5", "#3C7FC0"),
		"external_container" => ("#B3B3B3", "#A6A6A6"),
		"component" => ("#85BBF0", "#78A8D8"),
		"external_component" => ("#CCCCCC", "#BFBFBF"),
		_ => return None,
	};
	Some(if is_background { background } else { border })
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::super::{Options, render as render_diagram};
	use super::*;

	const OPTIONS: Options = Options {
		is_dark: false,
		limits: super::super::Limits { max_text_size: 50_000, max_edges: 500 },
		screen_width: super::super::DEFAULT_SCREEN_WIDTH,
	};

	#[test]
	fn resolves_shapes_and_stereotypes() {
		let element = |kind: &str| Element { type_c4_shape: kind.to_owned(), ..Element::default() };
		assert_eq!(resolve_shape(&element("external_person")), "person");
		assert_eq!(resolve_shape(&element("container_db")), "cylinder");
		assert_eq!(resolve_shape(&element("component_queue")), "h-cyl");
		assert_eq!(resolve_shape(&element("system")), "rounded");
		let tagged = Element { tags: Some("v1, Pipe".to_owned()), ..element("system") };
		assert_eq!(resolve_shape(&tagged), "h-cyl");
		let shaped = Element { shape: Some("folder".to_owned()), ..tagged };
		assert_eq!(resolve_shape(&shaped), "h-cyl");
		let techn = Element { techn: Some("Rust".to_owned()), ..element("external_system_db") };
		assert_eq!(stereotype(&techn), "[Software System: Rust]");
		assert_eq!(default_color("external_component_queue", true), Some("#CCCCCC"));
		assert_eq!(default_color("person_db", true), None);
	}

	#[test]
	fn splits_label_lines_like_mermaid() {
		assert_eq!(
			non_markdown_lines("a  b<br/>c\\nd &amp; <b>e</b>"),
			vec![vec!["a", "b"], vec!["c"], vec!["d", "&", "<b>", "e", "</b>"]]
		);
	}

	fn render(source: &str) -> Diagram {
		render_diagram(source, &OPTIONS, &mut FixedMeasure)
			.unwrap_or_else(|error| panic!("{error:?}"))
	}

	#[test]
	fn lays_out_rows_and_boundaries_like_mermaid() {
		let diagram = render(
			"C4Context\ntitle T\nSystem(a, \"A\")\nSystem(b, \"B\")\nSystem(c, \"C\")\n\
			 Boundary(x, \"X\") {\n  Person(p, \"P\")\n}\nRel(a, p, \"uses\")\nRel(b, c, \"calls\")\n\
			 UpdateLayoutConfig($c4ShapeInRow=\"2\")\n",
		);
		let svg = &diagram.svg;
		let center = |alias: &str| {
			let end = svg.find(&format!("-{alias}\"")).expect(alias);
			let start = svg[..end].rfind("translate(").expect(alias) + "translate(".len();
			let (x, y) =
				svg[start..].split_once(')').expect(alias).0.split_once(", ").expect(alias);
			(x.parse::<f64>().expect(alias), y.parse::<f64>().expect(alias))
		};
		// Two elements per row: `c` starts the second row at the left.
		let (a, b, c) = (center("a"), center("b"), center("c"));
		assert!(a.1 == b.1 && a.0 < b.0, "{svg}");
		assert!(c.0 == a.0 && c.1 > a.1, "{svg}");
		assert!(svg.contains("<line"), "{svg}");
		assert!(svg.contains("<path d=\"M"), "{svg}");
		assert!(svg.contains("stroke-dasharray=\"7.0,7.0\""), "{svg}");
	}

	#[test]
	fn reports_relationships_to_missing_elements() {
		let error = render_diagram(
			"C4Context\nPerson(a, \"A\")\nRel(a, nowhere, \"x\")\n",
			&OPTIONS,
			&mut FixedMeasure,
		)
		.expect_err("unknown end");
		assert!(error.message.contains("references an unknown shape"), "{}", error.message);
		let error = render_diagram(
			"C4Context\nPerson(a, \"A\")\nRel(a, global, \"x\")\n",
			&OPTIONS,
			&mut FixedMeasure,
		)
		.expect_err("global boundary");
		assert!(error.message.contains("has no intersect function"), "{}", error.message);
	}
}
