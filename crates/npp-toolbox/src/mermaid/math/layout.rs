//! MathML Core layout as Blink does it: `core/layout/mathml/math_*_layout_algorithm.cc`,
//! `math_layout_utils.cc`, `core/paint/mathml_painter.cc`, `platform/fonts/shaping/
//! stretchy_operator_shaper.cc` and the user agent style sheet `mathml.css`.
//!
//! Sizes follow Blink's fixed point `LayoutUnit` of 1/64 pixel, so that the boxes match those of
//! the browsers; without a `MATH` table in the font the fallback values of MathML Core apply.

use std::rc::Rc;

use super::mathml::{Document, Element, Tag};
use super::open_type::{Axis, Constant, MathTable};
use super::operators::{OperatorProperties, is_vertical, math_italic, operator_properties};
use super::{FONT_FAMILY, Item, ItemKind};
use crate::mermaid::Context;
use crate::mermaid::text::Font;

/// The bound of the layouts of one formula: fractions of stretched operators are laid out more
/// than once, which would take exponential time on deep input.
const MAX_LAYOUTS: usize = 200_000;
const MATH_UNIT: f64 = 1.0 / 18.0;
/// Blink's `HarfBuzzRunGlyphData::kMaxGlyphs` limits assemblies; the parts of tall delimiters
/// stay far below it.
const MAX_ASSEMBLY_GLYPHS: usize = 1 << 12;
/// What lines of `\cancel` are drawn with: about the CSS gradient of the Markdown viewer.
const STRIKE_THICKNESS: f64 = 1.2;
/// The number of copies of `\pmb` is doubled at each level, so nesting is only drawn this far.
const MAX_SHADOW_ITEMS: usize = 1 << 12;

/// `LayoutUnit(float)`.
fn unit(value: f64) -> f64 {
	(value * 64.0).trunc() / 64.0
}

fn unit_floor(value: f64) -> f64 {
	(value * 64.0).floor() / 64.0
}

fn unit_ceil(value: f64) -> f64 {
	(value * 64.0).ceil() / 64.0
}

/// `LayoutUnit / 2`.
fn half(value: f64) -> f64 {
	unit(value / 2.0)
}

/// The inherited properties of the elements.
#[derive(Clone, Debug)]
struct Style {
	size: f64,
	/// `math-depth`
	depth: i32,
	/// `math-style: normal`
	is_display: bool,
	/// `math-shift: compact`
	is_cramped: bool,
	color: Option<Rc<str>>,
	is_hidden: bool,
}

/// What a parent asks an operator to stretch to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stretch {
	None,
	Block { ascent: f64, descent: f64 },
	Inline(f64),
}

/// A laid out box; item positions are relative to the left end of its baseline.
#[derive(Clone, Debug, Default)]
pub struct Laid {
	pub width: f64,
	pub ascent: f64,
	pub descent: f64,
	pub italic_correction: f64,
	pub items: Vec<Item>,
}

impl Laid {
	fn height(&self) -> f64 {
		self.ascent + self.descent
	}

	/// Moves the items of `self` by `(x, y)` into `items`.
	fn place_into(self, x: f64, y: f64, items: &mut Vec<Item>) {
		items.extend(self.items.into_iter().map(|item| Item {
			x: item.x + x,
			y: item.y + y,
			..item
		}));
	}
}

/// The properties of an embellished operator.
struct Embellished {
	properties: OperatorProperties,
	is_vertical: bool,
	lspace: f64,
	rspace: f64,
}

/// A stretched or large glyph.
#[derive(Default)]
struct Shaped {
	advance: f64,
	ascent: f64,
	descent: f64,
	italic_correction: f64,
	/// Glyphs with their origin relative to the paint origin.
	glyphs: Vec<(u16, f64, f64)>,
}

/// One bar in Blink's painter: `y` is its middle, and its thickness is snapped to pixels.
fn bar(x: f64, y_middle: f64, width: f64, thickness: f64, style: &Style) -> Option<Item> {
	if thickness <= 0.0 || width <= 0.0 || style.is_hidden {
		return None;
	}
	let height = thickness.round().max(1.0);
	Some(Item {
		x,
		y: y_middle - (height / 2.0).floor(),
		color: style.color.clone(),
		link: None,
		kind: ItemKind::Bar { width, height },
	})
}

/// A filled rectangle in a color of its own, without the pixel snapping of the bars.
fn filled(x: f64, y: f64, width: f64, height: f64, color: Option<Rc<str>>) -> Item {
	Item { x, y, color, link: None, kind: ItemKind::Bar { width, height } }
}

/// The value of `property` in a `style` attribute that `markdown_html::math` wrote.
fn style_property<'s>(style: &'s str, property: &str) -> Option<&'s str> {
	style.split(';').find_map(|declaration| {
		let (name, value) = declaration.split_once(':')?;
		(name.trim() == property).then(|| value.trim())
	})
}

pub struct Engine<'c, 'a> {
	pub context: &'c mut Context<'a>,
	pub document: &'c Document,
	pub table: Option<Rc<MathTable>>,
	/// A design unit of a font of 1 pixel in pixels.
	pub design_unit: f64,
	pub layouts: usize,
}

impl Engine<'_, '_> {
	pub fn is_exhausted(&self) -> bool {
		self.layouts > MAX_LAYOUTS
	}

	pub fn layout_root(&mut self, size: f64) -> Laid {
		let style = Style {
			size,
			depth: 0,
			is_display: true,
			is_cramped: false,
			color: None,
			is_hidden: false,
		};
		self.layout(self.document.root(), &style, Stretch::None)
	}

	fn font(size: f64) -> Font {
		Font::new(FONT_FAMILY, size)
	}

	fn constant(&self, constant: Constant, size: f64) -> Option<f64> {
		let value = self.table.as_ref()?.constant(constant)?;
		Some(value * size * self.design_unit)
	}

	fn rule_thickness_fallback(size: f64) -> f64 {
		size * MATH_UNIT
	}

	/// `x_height` of Blink's font metrics.
	fn x_height(&mut self, size: f64) -> f64 {
		self.context.x_height(&Self::font(size))
	}

	fn axis_height(&mut self, size: f64) -> f64 {
		let fallback = self.x_height(size) / 2.0;
		unit(self.constant(Constant::AxisHeight, size).unwrap_or(fallback))
	}

	/// Blink's `ComputedStyle` for font sizes of `font-size: math`.
	fn scale_for_depth(&self, from: i32, to: i32) -> f64 {
		if from == to {
			return 1.0;
		}
		let (low, high, is_inverted) = match from < to {
			true => (from, to, false),
			false => (to, from, true),
		};
		let mut exponent = high - low;
		let mut scale = 1.0;
		if let Some(table) = &self.table {
			let script = table.constant(Constant::ScriptPercentScaleDown).unwrap_or(71.0) / 100.0;
			let script_script =
				table.constant(Constant::ScriptScriptPercentScaleDown).unwrap_or(50.41) / 100.0;
			if low <= 0 && high >= 2 {
				scale *= script_script;
				exponent -= 2;
			} else if low == 1 {
				scale *= script_script / script;
				exponent -= 1;
			} else if high == 1 {
				scale *= script;
				exponent -= 1;
			}
		}
		scale *= 0.71f64.powi(exponent);
		if is_inverted { 1.0 / scale } else { scale }
	}

	/// The style of `element` given what its parent's rules of `mathml.css` set: a change of
	/// `math-depth`, and whether its font size is kept.
	fn own_style(
		&self,
		element: &Element,
		parent: &Style,
		depth_change: i32,
		keeps_font_size: bool,
	) -> Style {
		let mut style = parent.clone();
		style.depth = parent.depth + depth_change;
		if element.tag == Tag::Phantom {
			style.is_hidden = true;
		}
		if element.tag == Tag::Table {
			style.is_display = false;
		}
		if let Some(is_display) = element.is_true("displaystyle") {
			style.is_display = is_display;
		}
		if matches!(element.tag, Tag::Root | Tag::SquareRoot) {
			style.is_cramped = true;
		}
		if let Some(level) = element.attribute("scriptlevel") {
			let level = level.trim();
			match level.parse::<i32>() {
				Ok(value) if level.starts_with(['+', '-']) => style.depth = parent.depth + value,
				Ok(value) => style.depth = value,
				Err(_) => {}
			}
		}
		if let Some(color) = element.attribute("mathcolor") {
			style.color = Some(Rc::from(color));
		}
		if !keeps_font_size {
			style.size = parent.size * self.scale_for_depth(parent.depth, style.depth);
		}
		style
	}

	/// The style of child number `position` of `parent`, from the rules of `mathml.css`.
	fn child_style(&self, parent: &Element, parent_style: &Style, position: usize) -> Style {
		let child = self.document.element(parent.children[position]);
		let mut inherited = parent_style.clone();
		let mut depth_change = 0;
		let mut keeps_font_size = false;
		let accent = parent.is_true("accent") == Some(true);
		let accent_under = parent.is_true("accentunder") == Some(true);
		match parent.tag {
			Tag::Fraction => {
				// `math-depth: auto-add`
				if !inherited.is_display {
					depth_change = 1;
				}
				inherited.is_display = false;
				if position == 1 {
					inherited.is_cramped = true;
				}
			}
			Tag::Root if position > 0 => {
				depth_change = 2;
				inherited.is_display = false;
			}
			Tag::Subscript
			| Tag::Superscript
			| Tag::SubSuperscript
			| Tag::Under
			| Tag::Over
			| Tag::UnderOver => {
				if position > 0 {
					depth_change = 1;
					inherited.is_display = false;
				}
				keeps_font_size = match parent.tag {
					Tag::Under => position == 1 && accent_under,
					Tag::Over => position == 1 && accent,
					Tag::UnderOver => (position == 1 && accent_under) || (position == 2 && accent),
					_ => false,
				};
				let is_cramped_child = match parent.tag {
					Tag::Subscript | Tag::SubSuperscript => position == 1,
					Tag::Over | Tag::UnderOver => position == 0 && accent,
					_ => false,
				};
				if is_cramped_child {
					inherited.is_cramped = true;
				}
			}
			_ => {}
		}
		self.own_style(child, &inherited, depth_change, keeps_font_size)
	}

	/// `GetMathMLEmbellishedOperatorProperties`: `style` is the style of `index`.
	fn embellished(&self, index: usize, style: &Style) -> Option<Embellished> {
		let element = self.document.element(index);
		let core = element.core_operator?;
		if core != index {
			let child_style = self.child_style(element, style, element.core_child);
			return self.embellished(element.children[element.core_child], &child_style);
		}
		let content = element.text.trim();
		let mut properties =
			operator_properties(content, element.has_previous_sibling, element.has_next_sibling);
		let flag = |name: &str, default: bool| element.is_true(name).unwrap_or(default);
		properties.is_stretchy = flag("stretchy", properties.is_stretchy);
		properties.is_symmetric = flag("symmetric", properties.is_symmetric);
		properties.is_large_op = flag("largeop", properties.is_large_op);
		properties.has_movable_limits = flag("movablelimits", properties.has_movable_limits);
		let code_point = single_character(content);
		// `ComputedStyle::FontSize` is rounded to whole pixels.
		let pixel_size = style.size.round();
		let leading = unit(f64::from(properties.leading_space) * MATH_UNIT * pixel_size);
		let trailing = unit(f64::from(properties.trailing_space) * MATH_UNIT * pixel_size);
		let space = |name: &str, default: f64| {
			element
				.attribute(name)
				.and_then(|value| length(value, style.size, default))
				.unwrap_or(default)
				.max(0.0)
		};
		Some(Embellished {
			properties,
			is_vertical: code_point.is_none_or(is_vertical),
			lspace: space("lspace", leading),
			rspace: space("rspace", trailing),
		})
	}

	fn is_block_stretchy(&self, index: usize, style: &Style) -> bool {
		self.embellished(index, style)
			.is_some_and(|operator| operator.properties.is_stretchy && operator.is_vertical)
	}

	fn is_inline_stretchy(&self, index: usize, style: &Style) -> bool {
		self.embellished(index, style)
			.is_some_and(|operator| operator.properties.is_stretchy && !operator.is_vertical)
	}

	fn layout(&mut self, index: usize, style: &Style, stretch: Stretch) -> Laid {
		self.layouts += 1;
		if self.is_exhausted() {
			return Laid::default();
		}
		let element = self.document.element(index);
		let child_count = element.children.len();
		match element.tag {
			Tag::Padded | Tag::Enclose => self.layout_padded(element, style, stretch),
			Tag::Mrow if element.attribute("href").is_some() => {
				let mut laid = self.layout_row(element, style, stretch);
				if let Some(link) = element.attribute("href") {
					let link: Rc<str> = Rc::from(link);
					for item in laid.items.iter_mut().filter(|item| item.link.is_none()) {
						item.link = Some(link.clone());
					}
				}
				laid
			}
			Tag::Group
				if element.attribute("style").is_some_and(|css| css.contains("text-shadow")) =>
			{
				let mut laid = self.layout_row(element, style, stretch);
				if laid.items.len() <= MAX_SHADOW_ITEMS {
					// `\pmb`: the shadow is what makes it bold.
					let (x, y) = (0.02 * style.size, 0.01 * style.size);
					let copies: Vec<Item> = laid
						.items
						.iter()
						.map(|item| Item { x: item.x + x, y: item.y + y, ..item.clone() })
						.collect();
					laid.items.extend(copies);
				}
				laid
			}
			Tag::Identifier | Tag::Number | Tag::Text => self.layout_token(element, style),
			Tag::Operator => self.layout_operator(index, element, style, stretch),
			Tag::Space => self.layout_space(element, style),
			Tag::Fraction if child_count == 2 => self.layout_fraction(element, style),
			Tag::SquareRoot => self.layout_radical(element, style),
			Tag::Root if child_count == 2 => self.layout_radical(element, style),
			Tag::Subscript | Tag::Superscript if child_count == 2 => {
				self.layout_scripts(element, style)
			}
			Tag::SubSuperscript if child_count == 3 => self.layout_scripts(element, style),
			Tag::Under | Tag::Over | Tag::UnderOver
				if child_count == if element.tag == Tag::UnderOver { 3 } else { 2 } =>
			{
				let base_style = self.child_style(element, style, 0);
				let has_movable_limits = !style.is_display
					&& self
						.embellished(element.children[0], &base_style)
						.is_some_and(|operator| operator.properties.has_movable_limits);
				match has_movable_limits {
					true => self.layout_scripts(element, style),
					false => self.layout_under_over(element, style, stretch),
				}
			}
			Tag::Table => self.layout_table(element, style),
			_ => self.layout_row(element, style, stretch),
		}
	}

	/// `MathTokenLayoutAlgorithm`: the height of the ink, the width of the advances.
	fn layout_token(&mut self, element: &Element, style: &Style) -> Laid {
		let text = token_text(element);
		if text.is_empty() {
			return Laid::default();
		}
		let font = token_font(element, style.size);
		let width = unit_ceil(self.context.measure_exact(&text, &font));
		// Blink measures the ink of the characters before `text-transform`.
		let (ascent, descent) = match self.context.ink_bounds(element.text.trim(), &font) {
			Some([_, top, _, bottom]) => (unit(-top), unit(bottom)),
			None => (0.0, 0.0),
		};
		let items = match style.is_hidden {
			true => Vec::new(),
			false => vec![Item {
				x: 0.0,
				y: 0.0,
				color: style.color.clone(),
				link: None,
				kind: ItemKind::Text { text, font },
			}],
		};
		Laid { width, ascent, descent, italic_correction: 0.0, items }
	}

	fn layout_space(&mut self, element: &Element, style: &Style) -> Laid {
		let dimension = |name: &str| {
			element.attribute(name).and_then(|value| length(value, style.size, 0.0)).unwrap_or(0.0)
		};
		let (width, ascent, descent) = (
			unit(dimension("width").max(0.0)),
			unit(dimension("height").max(0.0)),
			unit(dimension("depth").max(0.0)),
		);
		let mut items = Vec::new();
		if let Some(background) = element.attribute("mathbackground")
			&& !style.is_hidden
			&& width > 0.0
			&& ascent + descent > 0.0
		{
			items.push(filled(0.0, -ascent, width, ascent + descent, Some(Rc::from(background))));
		}
		Laid { width, ascent, descent, items, ..Laid::default() }
	}

	/// `MathPaddedLayoutAlgorithm` with the CSS box that `\boxed` and `\colorbox` set, and the lines
	/// of `menclose`, which Blink does not draw and the math of the viewer sets as a background.
	fn layout_padded(&mut self, element: &Element, style: &Style, stretch: Stretch) -> Laid {
		let content = self.layout_row(element, style, stretch);
		let dimension = |name: &str| {
			element.attribute(name).and_then(|value| length(value, style.size, 0.0)).map(unit)
		};
		// `ParseMathLength` refuses what is negative, except for `voffset`.
		let positive = |name: &str| dimension(name).filter(|value| *value >= 0.0);
		let width = positive("width").unwrap_or(content.width);
		let ascent = positive("height").unwrap_or(content.ascent);
		let descent = positive("depth").unwrap_or(content.descent);
		let left_space = positive("lspace").unwrap_or(0.0);
		let vertical_offset = dimension("voffset").unwrap_or(0.0);
		let css = element.attribute("style").unwrap_or("");
		let padding = style_property(css, "padding")
			.and_then(|value| length(value, style.size, 0.0))
			.map_or(0.0, |padding| unit(padding.max(0.0)));
		let border = style_property(css, "border");
		let mut border_parts = border.unwrap_or("").split_whitespace();
		let border_width = border_parts
			.next()
			.and_then(|value| length(value, style.size, 0.0))
			// Blink rounds a border down to pixels, but not below one.
			.map_or(0.0, |width| if width > 0.0 { width.floor().max(1.0) } else { 0.0 });
		let border_color = border_parts.nth(1).map(Rc::from).or_else(|| style.color.clone());
		let inset = padding + border_width;
		let (outer_width, outer_ascent, outer_descent) =
			(width + 2.0 * inset, ascent + inset, descent + inset);
		let mut items = Vec::new();
		if !style.is_hidden {
			if let Some(background) = element.attribute("mathbackground") {
				let color = Some(Rc::from(background));
				let height = outer_ascent + outer_descent;
				items.push(filled(0.0, -outer_ascent, outer_width, height, color));
			}
			if border_width > 0.0 {
				let (top, height) = (-outer_ascent, outer_ascent + outer_descent);
				let color = || border_color.clone();
				let thickness = border_width;
				items.push(filled(0.0, top, outer_width, thickness, color()));
				items.push(filled(0.0, outer_descent - thickness, outer_width, thickness, color()));
				items.push(filled(0.0, top, thickness, height, color()));
				items.push(filled(outer_width - thickness, top, thickness, height, color()));
			}
		}
		let notation = element.attribute("notation").unwrap_or("");
		content.place_into(inset + left_space, -vertical_offset, &mut items);
		if !style.is_hidden {
			let (top, height) = (-outer_ascent, outer_ascent + outer_descent);
			let line = |x: f64, y: f64, end_x: f64, end_y: f64| Item {
				x,
				y,
				color: style.color.clone(),
				link: None,
				kind: ItemKind::Line { end_x, end_y, thickness: STRIKE_THICKNESS },
			};
			if notation.contains("updiagonalstrike") {
				items.push(line(0.0, top + height, outer_width, -height));
			}
			if notation.contains("downdiagonalstrike") {
				items.push(line(0.0, top, outer_width, height));
			}
			if notation.contains("horizontalstrike") {
				items.push(line(0.0, top + height / 2.0, outer_width, 0.0));
			}
		}
		Laid {
			width: outer_width,
			ascent: outer_ascent,
			descent: outer_descent,
			italic_correction: 0.0,
			items,
		}
	}

	/// The glyph of `character` if the font has it.
	fn glyph(&mut self, character: char, size: f64) -> Option<u16> {
		match self.context.glyph_index(&Self::font(size), character) {
			0 => None,
			glyph => Some(glyph),
		}
	}

	fn glyph_box(&mut self, glyph: u16, size: f64) -> [f64; 4] {
		self.context.glyph_bounds(&Self::font(size), glyph).unwrap_or([0.0; 4])
	}

	/// `StretchyOperatorShaper::Shape`.
	fn shape(&mut self, base: u16, axis: Axis, target: f64, size: f64) -> Shaped {
		let font = Self::font(size);
		let mut variants = vec![base];
		if let Some(table) = &self.table {
			variants.extend(table.variants(base, axis));
		}
		let mut shaped = Shaped::default();
		let scale = size * self.design_unit;
		for &variant in &variants {
			let [left, top, right, bottom] = self.glyph_box(variant, size);
			let italic_correction =
				self.table.as_ref().and_then(|table| table.italic_correction(variant));
			shaped = Shaped {
				advance: self.context.glyph_advance(&font, variant),
				ascent: -top,
				descent: bottom,
				italic_correction: italic_correction.unwrap_or(0.0) * scale,
				glyphs: vec![(variant, 0.0, 0.0)],
			};
			let stretch_size = match axis {
				Axis::Horizontal => right - left,
				Axis::Vertical => bottom - top,
			};
			if stretch_size >= target {
				return shaped;
			}
		}
		self.shape_assembly(base, axis, target, size).unwrap_or(shaped)
	}

	/// `GetAssemblyParameters` and `ShapeResult::CreateForStretchyMathOperator`.
	fn shape_assembly(&mut self, base: u16, axis: Axis, target: f64, size: f64) -> Option<Shaped> {
		let table = self.table.clone()?;
		let (parts, italic_correction) = table.assembly(base, axis)?;
		let scale = size * self.design_unit;
		let min_overlap = table.min_connector_overlap() * scale;
		let mut max_overlap = f64::MAX;
		let (mut non_extender_sum, mut extender_sum) = (0.0, 0.0);
		let (mut non_extender_count, mut extender_count) = (0usize, 0usize);
		for (position, part) in parts.iter().enumerate() {
			let advance = f64::from(part.full_advance) * scale;
			if part.is_extender {
				extender_count += 1;
				extender_sum += advance;
			} else {
				non_extender_count += 1;
				non_extender_sum += advance;
			}
			if part.is_extender || position != 0 {
				max_overlap = max_overlap.min(f64::from(part.start_connector) * scale);
			}
			if part.is_extender || position + 1 != parts.len() {
				max_overlap = max_overlap.min(f64::from(part.end_connector) * scale);
			}
		}
		let extender_step = extender_sum - min_overlap * extender_count as f64;
		if extender_count == 0 || max_overlap < min_overlap || extender_step <= 0.0 {
			return None;
		}
		let repetitions =
			((target - non_extender_sum + min_overlap * (non_extender_count as f64 - 1.0))
				/ extender_step)
				.ceil()
				.max(0.0)
				.min(((MAX_ASSEMBLY_GLYPHS - non_extender_count) / extender_count) as f64) as usize;
		let glyph_count = non_extender_count + repetitions * extender_count;
		let mut overlap = max_overlap;
		if glyph_count > 1 {
			let theoretical = (non_extender_sum + repetitions as f64 * extender_sum - target)
				/ (glyph_count as f64 - 1.0);
			overlap = min_overlap.max(overlap.min(theoretical));
		}
		let stretch_size = non_extender_sum + repetitions as f64 * extender_sum
			- overlap * (glyph_count as f64 - 1.0);
		let expanded = parts.iter().flat_map(|part| {
			std::iter::repeat_n(*part, if part.is_extender { repetitions } else { 1 })
		});
		let italic_correction = italic_correction * scale;
		let mut glyphs = Vec::with_capacity(glyph_count);
		match axis {
			Axis::Horizontal => {
				let mut x = 0.0;
				let (mut left, mut right, mut top, mut bottom) =
					(f64::MAX, f64::MIN, f64::MAX, f64::MIN);
				for part in expanded {
					let bounds = self.glyph_box(part.glyph, size);
					left = left.min(x + bounds[0]);
					right = right.max(x + bounds[2]);
					top = top.min(bounds[1]);
					bottom = bottom.max(bounds[3]);
					glyphs.push((part.glyph, x, 0.0));
					x += f64::from(part.full_advance) * scale - overlap;
				}
				Some(Shaped {
					advance: right - left,
					ascent: -top,
					descent: bottom,
					italic_correction,
					glyphs,
				})
			}
			Axis::Vertical => {
				// From the top down: the last part is the top one. The ink of each part starts
				// where the advances of the parts above it end.
				let mut y = 0.0;
				let (mut ink_left, mut ink_right, mut ink_bottom) = (f64::MAX, f64::MIN, 0.0f64);
				let reversed: Vec<_> = expanded.collect();
				for part in reversed.into_iter().rev() {
					let bounds = self.glyph_box(part.glyph, size);
					glyphs.push((part.glyph, 0.0, y - bounds[1]));
					ink_left = ink_left.min(bounds[0]);
					ink_right = ink_right.max(bounds[2]);
					ink_bottom = ink_bottom.max(y + bounds[3] - bounds[1]);
					y += f64::from(part.full_advance) * scale - overlap;
				}
				let ascent = ink_bottom.max(stretch_size);
				for glyph in &mut glyphs {
					glyph.2 -= ascent;
				}
				Some(Shaped {
					advance: ink_right - ink_left,
					ascent,
					descent: 0.0,
					italic_correction,
					glyphs,
				})
			}
		}
	}

	/// `GetMinMaxSizesForVerticalStretchyOperator`: the widest of all variants and parts.
	fn vertical_stretchy_width(&mut self, base: u16, size: f64) -> f64 {
		let font = Self::font(size);
		let mut glyphs = vec![base];
		if let Some(table) = &self.table {
			glyphs.extend(table.variants(base, Axis::Vertical));
			if let Some((parts, _)) = table.assembly(base, Axis::Vertical) {
				glyphs.extend(parts.iter().map(|part| part.glyph));
			}
		}
		glyphs.into_iter().fold(0.0, |width, glyph| {
			f64::max(width, unit(self.context.glyph_advance(&font, glyph)))
		})
	}

	/// `MathOperatorLayoutAlgorithm`, and the plain token for all others.
	fn layout_operator(
		&mut self,
		index: usize,
		element: &Element,
		style: &Style,
		stretch: Stretch,
	) -> Laid {
		let content = element.text.trim();
		let Some(operator) = self.embellished(index, style) else {
			return self.layout_token(element, style);
		};
		let properties = operator.properties;
		// `IsOperatorWithSpecialShaping`
		let glyph = single_character(content).and_then(|character| {
			let is_special = properties.is_stretchy || (properties.is_large_op && style.is_display);
			if is_special {
				self.glyph(character, style.size).map(|g| (g, character))
			} else {
				None
			}
		});
		let Some((base, _)) = glyph else {
			return self.layout_token(element, style);
		};
		let axis_height = self.axis_height(style.size);
		let axis = if operator.is_vertical { Axis::Vertical } else { Axis::Horizontal };
		let mut target = 0.0;
		let (mut target_ascent, mut target_descent) = (0.0, 0.0);
		if properties.is_stretchy {
			match (operator.is_vertical, stretch) {
				(false, Stretch::Inline(size)) => target = size,
				(true, Stretch::Block { ascent, descent }) => {
					target_ascent = ascent;
					target_descent = descent;
					if properties.is_symmetric {
						let half_size = f64::max(ascent - axis_height, descent + axis_height);
						target_ascent = half_size + axis_height;
						target_descent = half_size - axis_height;
					}
					target = target_ascent + target_descent;
					let bounds = self.glyph_box(base, style.size);
					let unstretched = unit(bounds[3] - bounds[1]);
					let size_attribute = |name: &str| {
						element
							.attribute(name)
							.and_then(|value| length(value, style.size, unstretched))
							.map(unit)
					};
					let min_size = size_attribute("minsize").unwrap_or(unstretched).max(0.0);
					let max_size = size_attribute("maxsize").unwrap_or(f64::MAX).max(min_size);
					if target <= 0.0 {
						target_ascent = half(min_size) + axis_height;
						target_descent = min_size - target_ascent;
					} else if target < min_size {
						target_ascent =
							unit((target_ascent - axis_height).max(0.0) * min_size / target)
								+ axis_height;
						target_descent = min_size - target_ascent;
					} else if max_size < target {
						target_ascent =
							unit((target_ascent - axis_height).max(0.0) * max_size / target)
								+ axis_height;
						target_descent = max_size - target_ascent;
					}
					target = target_ascent + target_descent;
				}
				_ => {}
			}
		} else {
			target = unit(
				self.constant(Constant::DelimitedSubFormulaMinHeight, style.size).unwrap_or(0.0),
			);
		}
		let shaped = self.shape(base, axis, target, style.size);
		let operator_ascent = unit_floor(shaped.ascent);
		let operator_descent = unit_floor(shaped.descent);
		let delta = if operator.is_vertical && properties.is_stretchy {
			half((operator_ascent - operator_descent) - (target_ascent - target_descent))
		} else if properties.is_large_op && properties.is_symmetric && !properties.is_stretchy {
			half((operator_ascent - operator_descent) - 2.0 * axis_height)
		} else {
			0.0
		};
		// The box is as wide as the operator's min-max size, the glyph centered in it.
		let width = match (properties.is_stretchy, operator.is_vertical) {
			(true, true) => self.vertical_stretchy_width(base, style.size),
			(true, false) => unit(self.shape(base, Axis::Horizontal, 0.0, style.size).advance),
			(false, _) => unit(shaped.advance),
		};
		let x = half(width - unit(shaped.advance));
		let mut items = Vec::new();
		if !style.is_hidden {
			for (glyph, glyph_x, glyph_y) in &shaped.glyphs {
				items.push(Item {
					x: x + glyph_x,
					y: delta + glyph_y,
					color: style.color.clone(),
					link: None,
					kind: ItemKind::Glyph { glyph: *glyph, size: style.size },
				});
			}
		}
		Laid {
			width,
			ascent: operator_ascent - delta,
			descent: operator_descent + delta,
			italic_correction: unit(shaped.italic_correction),
			items,
		}
	}

	/// `MathRowLayoutAlgorithm`: children on the baseline, stretchy ones as high as the others.
	fn layout_row(&mut self, element: &Element, style: &Style, stretch: Stretch) -> Laid {
		let should_add_space = element.tag == Tag::Math || !element.is_embellished_row;
		let count = element.children.len();
		let styles: Vec<Style> =
			(0..count).map(|position| self.child_style(element, style, position)).collect();
		let is_block_stretchy: Vec<bool> = (0..count)
			.map(|position| self.is_block_stretchy(element.children[position], &styles[position]))
			.collect();
		let mut laid: Vec<Option<Laid>> = Vec::with_capacity(count);
		laid.resize_with(count, || None);
		let inherits = stretch != Stretch::None;
		let mut sizes = (0.0f64, 0.0f64);
		// Non-stretchy children are laid out once: nothing changes for the second pass of Blink.
		for position in 0..count {
			if !is_block_stretchy[position] {
				let child = element.children[position];
				let child_stretch = match stretch {
					Stretch::Inline(_) if self.is_inline_stretchy(child, &styles[position]) => {
						stretch
					}
					_ => Stretch::None,
				};
				let result = self.layout(child, &styles[position], child_stretch);
				sizes.0 = sizes.0.max(result.ascent);
				sizes.1 = sizes.1.max(result.descent);
				laid[position] = Some(result);
			}
		}
		if !inherits && is_block_stretchy.iter().all(|&is_stretchy| is_stretchy) {
			for (&child, child_style) in element.children.iter().zip(&styles) {
				let zero = Stretch::Block { ascent: 0.0, descent: 0.0 };
				let result = self.layout(child, child_style, zero);
				sizes.0 = sizes.0.max(result.ascent);
				sizes.1 = sizes.1.max(result.descent);
			}
		}
		let target = match stretch {
			Stretch::Block { .. } => stretch,
			_ => Stretch::Block { ascent: sizes.0, descent: sizes.1 },
		};
		let (mut x, mut ascent, mut descent) = (0.0f64, 0.0f64, 0.0f64);
		let mut placed = Vec::with_capacity(count);
		for position in 0..count {
			let child = element.children[position];
			let result = match laid[position].take() {
				Some(result) => result,
				None => {
					let child_stretch =
						if matches!(stretch, Stretch::Inline(_)) { Stretch::None } else { target };
					self.layout(child, &styles[position], child_stretch)
				}
			};
			if should_add_space && let Some(operator) = self.embellished(child, &styles[position]) {
				x += operator.lspace;
				ascent = ascent.max(result.ascent);
				descent = descent.max(result.descent);
				let (width, right) = (result.width, operator.rspace);
				placed.push((result, x));
				x += width + right;
			} else {
				ascent = ascent.max(result.ascent);
				descent = descent.max(result.descent);
				let width = result.width;
				placed.push((result, x));
				x += width;
			}
		}
		let mut items = Vec::new();
		for (result, position) in placed {
			result.place_into(position, 0.0, &mut items);
		}
		Laid { width: x.max(0.0), ascent, descent, italic_correction: 0.0, items }
	}

	/// `MathFractionLayoutAlgorithm` with the bar of the painter.
	fn layout_fraction(&mut self, element: &Element, style: &Style) -> Laid {
		let numerator_style = self.child_style(element, style, 0);
		let denominator_style = self.child_style(element, style, 1);
		let numerator = self.layout(element.children[0], &numerator_style, Stretch::None);
		let denominator = self.layout(element.children[1], &denominator_style, Stretch::None);
		let size = style.size;
		let default_thickness = unit(
			self.constant(Constant::FractionRuleThickness, size)
				.unwrap_or_else(|| Self::rule_thickness_fallback(size)),
		);
		let thickness = element
			.attribute("linethickness")
			.and_then(|value| length(value, size, default_thickness))
			.map_or(default_thickness, |value| unit(value).max(0.0));
		let axis_height = self.axis_height(size);
		let fallback = Self::rule_thickness_fallback(size);
		let display = style.is_display;
		let (numerator_shift, denominator_shift);
		if thickness > 0.0 {
			let (gap, denominator_gap, shift_up, shift_down) = match display {
				true => (
					Constant::FractionNumDisplayStyleGapMin,
					Constant::FractionDenomDisplayStyleGapMin,
					Constant::FractionNumeratorDisplayStyleShiftUp,
					Constant::FractionDenominatorDisplayStyleShiftDown,
				),
				false => (
					Constant::FractionNumeratorGapMin,
					Constant::FractionDenominatorGapMin,
					Constant::FractionNumeratorShiftUp,
					Constant::FractionDenominatorShiftDown,
				),
			};
			let gap_fallback = if display { 3.0 } else { 1.0 } * fallback;
			let numerator_gap = unit(self.constant(gap, size).unwrap_or(gap_fallback));
			let denominator_gap =
				unit(self.constant(denominator_gap, size).unwrap_or(numerator_gap));
			let shift_up = unit(self.constant(shift_up, size).unwrap_or(0.0));
			let shift_down = unit(self.constant(shift_down, size).unwrap_or(0.0));
			numerator_shift = f64::max(
				shift_up,
				axis_height + half(thickness) + numerator_gap + numerator.descent,
			);
			denominator_shift = f64::max(
				shift_down,
				half(thickness) + denominator_gap + denominator.ascent - axis_height,
			);
		} else {
			let (gap, up, down) = match display {
				true => (
					Constant::StackDisplayStyleGapMin,
					Constant::StackTopDisplayStyleShiftUp,
					Constant::StackBottomDisplayStyleShiftDown,
				),
				false => (
					Constant::StackGapMin,
					Constant::StackTopShiftUp,
					Constant::StackBottomShiftDown,
				),
			};
			let gap_fallback = if display { 7.0 } else { 3.0 } * fallback;
			let gap_min = unit(self.constant(gap, size).unwrap_or(gap_fallback));
			let mut up = unit(self.constant(up, size).unwrap_or(0.0));
			let mut down = unit(self.constant(down, size).unwrap_or(0.0));
			let gap = down - denominator.ascent + up - numerator.descent;
			if gap < gap_min {
				let difference = gap_min - gap;
				let delta = half(difference);
				up += delta;
				down += difference - delta;
			}
			numerator_shift = up;
			denominator_shift = down;
		}
		let ascent =
			f64::max(numerator_shift + numerator.ascent, denominator.ascent - denominator_shift)
				.max(0.0);
		let descent =
			f64::max(numerator.descent - numerator_shift, denominator_shift + denominator.descent)
				.max(0.0);
		// `padding-inline: 1px` of `mathml.css`.
		let content_width = f64::max(numerator.width, denominator.width);
		let numerator_x = 1.0 + half(content_width - numerator.width);
		let denominator_x = 1.0 + half(content_width - denominator.width);
		let mut items = Vec::new();
		items.extend(bar(1.0, -axis_height, content_width, thickness, style));
		numerator.place_into(numerator_x, -numerator_shift, &mut items);
		denominator.place_into(denominator_x, denominator_shift, &mut items);
		Laid { width: content_width + 2.0, ascent, descent, italic_correction: 0.0, items }
	}

	/// `MathRadicalLayoutAlgorithm` with the symbol of the painter: `msqrt` has a row as base.
	fn layout_radical(&mut self, element: &Element, style: &Style) -> Laid {
		let size = style.size;
		let has_index = element.tag == Tag::Root;
		let (base, index_box) = if has_index {
			let base_style = self.child_style(element, style, 0);
			let index_style = self.child_style(element, style, 1);
			(
				self.layout(element.children[0], &base_style, Stretch::None),
				Some(self.layout(element.children[1], &index_style, Stretch::None)),
			)
		} else {
			(self.layout_row(element, style, Stretch::None), None)
		};
		let fallback = Self::rule_thickness_fallback(size);
		let x_height = self.x_height(size);
		let rule_thickness =
			unit(self.constant(Constant::RadicalRuleThickness, size).unwrap_or(fallback));
		let vertical_gap = unit(match style.is_display {
			true => self
				.constant(Constant::RadicalDisplayStyleVerticalGap, size)
				.unwrap_or(fallback + x_height / 4.0),
			false => {
				self.constant(Constant::RadicalVerticalGap, size).unwrap_or(5.0 * fallback / 4.0)
			}
		});
		let extra_ascender =
			unit(self.constant(Constant::RadicalExtraAscender, size).unwrap_or(rule_thickness));
		let raise_percent = self
			.table
			.as_ref()
			.and_then(|table| table.constant(Constant::RadicalDegreeBottomRaisePercent))
			.map_or(0.6, |percent| percent / 100.0);
		let (mut kern_before, mut kern_after, mut index_width) = (0.0, 0.0, 0.0);
		if let Some(index_box) = &index_box {
			index_width = index_box.width;
			kern_before = unit(
				self.constant(Constant::RadicalKernBeforeDegree, size)
					.unwrap_or(5.0 * size * MATH_UNIT),
			)
			.max(0.0);
			kern_after = unit(
				self.constant(Constant::RadicalKernAfterDegree, size)
					.unwrap_or(-10.0 * size * MATH_UNIT),
			)
			.max(-index_width);
		}
		let surd_glyph = self.glyph('\u{221A}', size);
		let mut surd = Shaped::default();
		let mut surd_width = 0.0;
		if let Some(glyph) = surd_glyph {
			let target = base.ascent + base.descent + vertical_gap + rule_thickness;
			surd = self.shape(glyph, Axis::Vertical, target, size);
			surd_width = self.vertical_stretchy_width(glyph, size);
		}
		let operator_x = index_width + kern_before + kern_after;
		let surd_advance = unit(surd.advance);
		let surd_block_size = unit(surd.ascent + surd.descent);
		let index_bottom_raise = unit(unit(raise_percent) * surd_block_size);
		let mut ascent = base.ascent + vertical_gap + rule_thickness + extra_ascender;
		let mut descent = f64::max(base.descent, surd_block_size + extra_ascender - ascent);
		let (index_ascent, index_descent) = index_box
			.as_ref()
			.map_or((0.0, 0.0), |index_box| (index_box.ascent, index_box.descent));
		if index_box.is_some() {
			let index_size = index_descent + index_ascent;
			ascent = ascent.max(index_bottom_raise + index_size - descent);
			descent = descent.max(descent - index_bottom_raise + index_size);
		}
		let width = index_width + kern_before + kern_after + surd_width + base.width;
		let base_x = surd_advance + operator_x;
		// The painter: the top of the symbol and the middle of the bar are at the same height.
		let top = -(vertical_gap + base.ascent);
		let mut items = Vec::new();
		if !style.is_hidden {
			for (glyph, glyph_x, glyph_y) in &surd.glyphs {
				items.push(Item {
					x: operator_x + glyph_x,
					y: top + unit(surd.ascent) + glyph_y,
					color: style.color.clone(),
					link: None,
					kind: ItemKind::Glyph { glyph: *glyph, size },
				});
			}
		}
		items.extend(bar(operator_x + surd_advance, top, base.width, rule_thickness, style));
		base.place_into(base_x, 0.0, &mut items);
		if let Some(index_box) = index_box {
			let y = descent - index_bottom_raise - index_descent;
			index_box.place_into(kern_before, y, &mut items);
		}
		Laid { width, ascent, descent, italic_correction: 0.0, items }
	}

	/// `MathScriptsLayoutAlgorithm`; `munder` and `mover` with movable limits use it too.
	fn layout_scripts(&mut self, element: &Element, style: &Style) -> Laid {
		let (subscript_position, superscript_position) = match element.tag {
			Tag::Subscript | Tag::Under => (Some(1), None),
			Tag::Superscript | Tag::Over => (None, Some(1)),
			_ => (Some(1), Some(2)),
		};
		let layout_child = |engine: &mut Self, position: usize| {
			let child_style = engine.child_style(element, style, position);
			engine.layout(element.children[position], &child_style, Stretch::None)
		};
		let subscript = subscript_position.map(|position| layout_child(self, position));
		let superscript = superscript_position.map(|position| layout_child(self, position));
		let base = layout_child(self, 0);
		let size = style.size;
		let x_height = self.x_height(size);
		let parameter = |engine: &Self, constant, fallback: f64| {
			unit(engine.constant(constant, size).unwrap_or(fallback))
		};
		let subscript_shift_down = parameter(self, Constant::SubscriptShiftDown, x_height / 3.0);
		let shift_up = match style.is_cramped {
			true => parameter(self, Constant::SuperscriptShiftUpCramped, x_height),
			false => parameter(self, Constant::SuperscriptShiftUp, x_height),
		};
		let baseline_drop_min = parameter(self, Constant::SubscriptBaselineDropMin, x_height / 2.0);
		let baseline_drop_max =
			parameter(self, Constant::SuperscriptBaselineDropMax, x_height / 2.0);
		let gap_min = parameter(self, Constant::SubSuperscriptGapMin, size / 5.0);
		let bottom_min = parameter(self, Constant::SuperscriptBottomMin, x_height / 4.0);
		let top_max = parameter(self, Constant::SubscriptTopMax, 4.0 * x_height / 5.0);
		let bottom_max_with_subscript =
			parameter(self, Constant::SuperscriptBottomMaxWithSubscript, 4.0 * x_height / 5.0);
		let space = parameter(self, Constant::SpaceAfterScript, size / 5.0);
		let mut sub_shift = 0.0;
		let mut sup_shift = 0.0;
		let (mut scripts_ascent, mut scripts_descent) = (0.0f64, 0.0f64);
		match (&subscript, &superscript) {
			(Some(subscript), None) => {
				scripts_descent = subscript.descent;
				sub_shift = f64::max(subscript_shift_down, base.descent + baseline_drop_min)
					.max(subscript.ascent - top_max);
			}
			(None, Some(superscript)) => {
				scripts_ascent = superscript.ascent;
				sup_shift = f64::max(shift_up, base.ascent - baseline_drop_max)
					.max(bottom_min + superscript.descent);
			}
			(Some(subscript), Some(superscript)) => {
				scripts_ascent = superscript.ascent;
				scripts_descent = subscript.descent;
				let mut sub = f64::max(subscript_shift_down, base.descent + baseline_drop_min)
					.max(subscript.ascent - top_max);
				let mut sup = f64::max(shift_up, base.ascent - baseline_drop_max)
					.max(bottom_min + superscript.descent);
				let mut gap = (sub - subscript.ascent) + (sup - superscript.descent);
				if gap < gap_min {
					let delta = bottom_max_with_subscript - (sup - superscript.descent);
					if delta > 0.0 {
						let delta = delta.min(gap_min - gap);
						sup += delta;
						gap += delta;
					}
					if gap < gap_min {
						sub += gap_min - gap;
					}
				}
				sub_shift = sub;
				sup_shift = sup;
			}
			(None, None) => {}
		}
		let ascent = f64::max(base.ascent, scripts_ascent + sup_shift).max(0.0);
		let descent = f64::max(base.descent, scripts_descent + sub_shift).max(0.0);
		let italic_correction = f64::min(base.width, base.italic_correction);
		let scripts_x = base.width;
		let scripts_width = f64::max(
			subscript.as_ref().map_or(0.0, |script| script.width),
			superscript.as_ref().map_or(0.0, |script| script.width),
		);
		let width = base.width + scripts_width + space;
		let mut items = Vec::new();
		base.place_into(0.0, 0.0, &mut items);
		if let Some(subscript) = subscript {
			let x = (scripts_x - italic_correction).max(0.0);
			subscript.place_into(x, sub_shift, &mut items);
		}
		if let Some(superscript) = superscript {
			superscript.place_into(scripts_x, -sup_shift, &mut items);
		}
		Laid { width, ascent, descent, italic_correction: 0.0, items }
	}

	/// `MathUnderOverLayoutAlgorithm`
	fn layout_under_over(&mut self, element: &Element, style: &Style, stretch: Stretch) -> Laid {
		let (under_position, over_position) = match element.tag {
			Tag::Under => (Some(1), None),
			Tag::Over => (None, Some(1)),
			_ => (Some(1), Some(2)),
		};
		let count = element.children.len();
		let styles: Vec<Style> =
			(0..count).map(|position| self.child_style(element, style, position)).collect();
		let is_inline_stretchy: Vec<bool> = (0..count)
			.map(|position| self.is_inline_stretchy(element.children[position], &styles[position]))
			.collect();
		let base_operator = self.embellished(element.children[0], &styles[0]);
		let is_large_op = base_operator.as_ref().is_some_and(|op| op.properties.is_large_op);
		let is_base_inline_stretchy = is_inline_stretchy[0];
		let is_base_block_stretchy = self.is_block_stretchy(element.children[0], &styles[0]);
		let inherits_block = matches!(stretch, Stretch::Block { .. });
		let inherits_inline = matches!(stretch, Stretch::Inline(_));
		let mut laid: Vec<Option<Laid>> = Vec::with_capacity(count);
		laid.resize_with(count, || None);
		let mut stretch_size = 0.0f64;
		let mut has_measured = false;
		for position in 0..count {
			if is_inline_stretchy[position] {
				continue;
			}
			let child = element.children[position];
			let result = self.layout(child, &styles[position], Stretch::None);
			stretch_size = stretch_size.max(result.width);
			has_measured = true;
			// The base is laid out again below if it has to stretch.
			if !(position == 0 && inherits_block && is_base_block_stretchy) {
				laid[position] = Some(result);
			}
		}
		if !has_measured {
			let skipped = usize::from(inherits_block || inherits_inline);
			for (&child, child_style) in element.children.iter().zip(&styles).skip(skipped) {
				let result = self.layout(child, child_style, Stretch::Inline(0.0));
				stretch_size = stretch_size.max(result.width);
			}
		}
		let mut take = |engine: &mut Self, position: usize| {
			if let Some(result) = laid[position].take() {
				return result;
			}
			let child = element.children[position];
			let is_inherited = (inherits_block && is_base_block_stretchy)
				|| (inherits_inline && is_base_inline_stretchy);
			let child_stretch = if position == 0 && is_inherited {
				stretch
			} else if is_inline_stretchy[position] {
				Stretch::Inline(stretch_size)
			} else {
				Stretch::None
			};
			engine.layout(child, &styles[position], child_stretch)
		};
		let base = take(self, 0);
		let over = over_position.map(|position| take(self, position));
		let under = under_position.map(|position| take(self, position));
		let width = [Some(&base), over.as_ref(), under.as_ref()]
			.into_iter()
			.flatten()
			.fold(0.0f64, |width, child| width.max(child.width));

		let size = style.size;
		let fallback = Self::rule_thickness_fallback(size);
		let constant = |engine: &Self, constant, default: f64| {
			unit(engine.constant(constant, size).unwrap_or(default))
		};
		let (under_gap, over_gap, under_shift, over_shift, under_extra, over_extra, accent_height);
		let uses_bar_fallback;
		if is_large_op {
			under_gap = constant(self, Constant::LowerLimitGapMin, 0.0);
			over_gap = constant(self, Constant::UpperLimitGapMin, 0.0);
			under_shift = constant(self, Constant::LowerLimitBaselineDropMin, 0.0);
			over_shift = constant(self, Constant::UpperLimitBaselineRiseMin, 0.0);
			(under_extra, over_extra, accent_height) = (0.0, 0.0, 0.0);
			uses_bar_fallback = false;
		} else if is_base_inline_stretchy {
			under_gap = constant(self, Constant::StretchStackGapBelowMin, 0.0);
			over_gap = constant(self, Constant::StretchStackGapAboveMin, 0.0);
			under_shift = constant(self, Constant::StretchStackBottomShiftDown, 0.0);
			over_shift = constant(self, Constant::StretchStackTopShiftUp, 0.0);
			(under_extra, over_extra, accent_height) = (0.0, 0.0, 0.0);
			uses_bar_fallback = false;
		} else {
			let x_height = self.x_height(size);
			under_gap = constant(self, Constant::UnderbarVerticalGap, 3.0 * fallback);
			over_gap = constant(self, Constant::OverbarVerticalGap, 3.0 * fallback);
			(under_shift, over_shift) = (0.0, 0.0);
			under_extra = constant(self, Constant::UnderbarExtraDescender, fallback);
			over_extra = constant(self, Constant::OverbarExtraAscender, fallback);
			accent_height = constant(self, Constant::AccentBaseHeight, x_height / 2.0);
			uses_bar_fallback = true;
		}
		let mut top = 0.0f64;
		let mut over_top = 0.0;
		if let Some(over) = &over {
			top += over_extra;
			over_top = top;
			if uses_bar_fallback {
				top += over.height();
				if element.is_true("accent") == Some(true) {
					if base.ascent < accent_height {
						top += accent_height - base.ascent;
					}
				} else {
					top += over_gap;
				}
			} else {
				top += f64::max(over.height() + over_gap, over.ascent + over_shift);
			}
		}
		let base_top = top;
		top += base.ascent;
		let ascent = top.max(0.0);
		let mut descent = base.descent;
		let mut under_top = 0.0;
		if let Some(under) = &under {
			if uses_bar_fallback {
				if element.is_true("accentunder") != Some(true) {
					descent += under_gap;
				}
			} else {
				descent += f64::max(under_gap, under_shift - under.ascent);
			}
			under_top = ascent + descent;
			descent += under.height() + under_extra;
		}
		let descent = descent.max(0.0);
		let mut items = Vec::new();
		let base_x = half(width - base.width);
		let base_y = base_top + base.ascent - ascent;
		base.place_into(base_x, base_y, &mut items);
		if let Some(over) = over {
			let x = half(width - over.width);
			let y = over_top + over.ascent - ascent;
			over.place_into(x, y, &mut items);
		}
		if let Some(under) = under {
			let x = half(width - under.width);
			let y = under_top + under.ascent - ascent;
			under.place_into(x, y, &mut items);
		}
		Laid { width, ascent, descent, italic_correction: 0.0, items }
	}

	/// A CSS `inline-table`: `mathml.css` pads the cells, which center their row of children and
	/// align their baselines in a row. Blink's table ignores `columnalign` and the spacings.
	fn layout_table(&mut self, element: &Element, style: &Style) -> Laid {
		struct Cell {
			laid: Laid,
			padding_x: f64,
			padding_y: f64,
		}
		let mut rows: Vec<Vec<Cell>> = Vec::new();
		let mut column_widths: Vec<f64> = Vec::new();
		for (row_position, &row_index) in element.children.iter().enumerate() {
			let row = self.document.element(row_index);
			if row.tag != Tag::TableRow {
				continue;
			}
			let row_style = self.child_style(element, style, row_position);
			let mut cells = Vec::with_capacity(row.children.len());
			for (cell_position, &cell_index) in row.children.iter().enumerate() {
				let cell = self.document.element(cell_index);
				if cell.tag != Tag::TableCell {
					continue;
				}
				let cell_style = self.child_style(row, &row_style, cell_position);
				let laid = self.layout_row(cell, &cell_style, Stretch::None);
				let padding_x = unit(0.4 * cell_style.size);
				let padding_y = unit(0.5 * self.x_height(cell_style.size));
				let width = laid.width + 2.0 * padding_x;
				match column_widths.get_mut(cells.len()) {
					Some(column_width) => *column_width = column_width.max(width),
					None => column_widths.push(width),
				}
				cells.push(Cell { laid, padding_x, padding_y });
			}
			rows.push(cells);
		}
		let width: f64 = column_widths.iter().sum();
		let mut row_tops = Vec::with_capacity(rows.len());
		let mut height = 0.0;
		let mut row_baselines = Vec::with_capacity(rows.len());
		for cells in &rows {
			let above = cells
				.iter()
				.fold(0.0f64, |above, cell| above.max(cell.padding_y + cell.laid.ascent));
			let below = cells
				.iter()
				.fold(0.0f64, |below, cell| below.max(cell.laid.descent + cell.padding_y));
			row_tops.push(height);
			row_baselines.push(above);
			height += above + below;
		}
		// The center of the table is on the math axis (`MathTableBaseline`).
		let axis_height = self.axis_height(style.size);
		let ascent = unit(height / 2.0 + axis_height);
		let descent = height - ascent;
		let mut items = Vec::new();
		for ((cells, top), baseline) in rows.into_iter().zip(row_tops).zip(row_baselines) {
			let mut x = 0.0;
			for (cell, column_width) in cells.into_iter().zip(&column_widths) {
				let content_width = column_width - 2.0 * cell.padding_x;
				let cell_x = x + cell.padding_x + half(content_width - cell.laid.width);
				cell.laid.place_into(cell_x, top + baseline - ascent, &mut items);
				x += column_width;
			}
		}
		Laid { width, ascent, descent, italic_correction: 0.0, items }
	}
}

fn single_character(text: &str) -> Option<char> {
	let mut characters = text.chars();
	let first = characters.next()?;
	characters.next().is_none().then_some(first)
}

/// The text of a token, trimmed like MathML whitespace; `mi` with one character is italic.
fn token_text(element: &Element) -> String {
	let text = element.text.trim_matches([' ', '\t', '\n', '\r']);
	if element.tag == Tag::Identifier
		&& element.attribute("mathvariant").is_none_or(|variant| variant != "normal")
		&& let Some(character) = single_character(text)
	{
		return math_italic(character).to_string();
	}
	text.to_owned()
}

/// The font of a token; `\textbf` and the like set a variant on text.
fn token_font(element: &Element, size: f64) -> Font {
	let mut font = Font::new(FONT_FAMILY, size);
	if element.tag == Tag::Text {
		match element.attribute("mathvariant") {
			Some("bold") => font.weight = 700,
			Some("italic") => font.is_italic = true,
			Some("monospace") => font.family = "monospace".to_owned(),
			Some("sans-serif") => font.family = "sans-serif".to_owned(),
			_ => {}
		}
	}
	font
}

/// A MathML length in pixels; a percentage is one of `base`. `None` if it is invalid.
fn length(value: &str, size: f64, base: f64) -> Option<f64> {
	let value = value.trim();
	let number_end = value
		.find(|character: char| !(character.is_ascii_digit() || "+-.".contains(character)))
		.unwrap_or(value.len());
	let number: f64 = value[..number_end].parse().ok()?;
	Some(match value[number_end..].trim() {
		"" => {
			if number != 0.0 {
				return None;
			}
			0.0
		}
		"em" => number * size,
		// `ex` of the math font, close to Blink's for the rare uses here.
		"ex" => number * size * 0.5,
		"px" => number,
		"pt" => number * 96.0 / 72.0,
		"pc" => number * 16.0,
		"in" => number * 96.0,
		"cm" => number * 96.0 / 2.54,
		"mm" => number * 96.0 / 25.4,
		"%" => number / 100.0 * base,
		_ => return None,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rounds_like_layout_units() {
		assert_eq!(unit(1.0 / 64.0 + 0.001), 1.0 / 64.0);
		assert_eq!(unit(-1.0 / 64.0 - 0.001), -1.0 / 64.0);
		assert_eq!(unit_ceil(1.0 / 64.0 + 0.001), 2.0 / 64.0);
		assert_eq!(unit_floor(-0.001), -1.0 / 64.0);
		assert_eq!(half(3.0 / 64.0), 1.0 / 64.0);
	}

	#[test]
	fn parses_lengths() {
		assert_eq!(length("1.5em", 10.0, 0.0), Some(15.0));
		assert_eq!(length("0", 10.0, 0.0), Some(0.0));
		assert_eq!(length("0.000em", 10.0, 0.0), Some(0.0));
		assert_eq!(length("3", 10.0, 0.0), None);
		assert_eq!(length("50%", 10.0, 8.0), Some(4.0));
		assert_eq!(length("72pt", 10.0, 0.0), Some(96.0));
		assert_eq!(length("x", 10.0, 0.0), None);
		assert_eq!(length("", 10.0, 0.0), None);
		assert_eq!(length("1furlong", 10.0, 0.0), None);
	}
}
