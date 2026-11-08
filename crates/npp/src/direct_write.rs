//! Text measurement and glyph outlines with DirectWrite, the text engine WebView2 and EdgeHTML
//! use on Windows, so that laid out labels fit the text the browsers draw. Outlines replace text
//! where resvg renders an SVG, as resvg is built without text support.

mod interfaces;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use core::ffi::c_void;
use windows_core::{HRESULT, Interface, implement};

use crate::wide::to_wide_null_terminated;
use interfaces::*;

/// A CSS font.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font<'a> {
	/// A CSS font family list, e.g. `"trebuchet ms", verdana, arial, sans-serif`.
	pub family: &'a str,
	/// In pixels.
	pub size: f32,
	/// CSS weight, 400 normal, 700 bold.
	pub weight: u16,
	pub is_italic: bool,
}

/// Glyph outlines of a line of text, in pixels with the origin at the start of the baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
	/// SVG path data.
	pub path: String,
	/// Advance width, including trailing spaces.
	pub width: f32,
	/// Vertical metrics of the first font, positive.
	pub ascent: f32,
	pub descent: f32,
	pub x_height: f32,
}

/// Vertical metrics of a font in em.
#[derive(Clone, Copy, Debug, Default)]
struct VerticalMetrics {
	ascent: f32,
	descent: f32,
	x_height: f32,
	line_gap: f32,
	/// The top of the underline above the baseline, negative below it.
	underline_position: f32,
	underline_thickness: f32,
}

/// `getBBox` of a line of text relative to the start of its baseline.
#[derive(Clone, Copy, Debug)]
struct InkBox {
	left: f32,
	/// Relative to the baseline, negative above it.
	top: f32,
	width: f32,
	height: f32,
}

struct Format {
	format: IDWriteTextFormat,
	metrics: VerticalMetrics,
	/// The first font of the family list, for access to its glyphs and tables.
	face: Option<IDWriteFontFace>,
	units_per_em: f32,
	/// Skia takes GDI's metrics and rasterization where the font has a bitmap strike.
	has_bitmap_strike: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct FormatKey {
	family: String,
	size_bits: u32,
	weight: u16,
	is_italic: bool,
}

/// Measured lines kept by [`TextEngine`]; it starts over when full.
const MAX_CACHED_MEASUREMENTS: usize = 20_000;

pub struct TextEngine {
	factory: IDWriteFactory,
	collection: IDWriteFontCollection,
	formats: HashMap<FormatKey, Option<Format>>,
	measurements: HashMap<(FormatKey, String), (f32, f32)>,
	bounding_boxes: HashMap<(FormatKey, String), InkBox>,
}

impl TextEngine {
	/// `None` if DirectWrite is not available.
	pub fn new() -> Option<Self> {
		let mut factory = std::ptr::null_mut();
		// SAFETY: Valid IID and output pointer.
		let result =
			unsafe { DWriteCreateFactory(FACTORY_TYPE_SHARED, &IID_IDWRITE_FACTORY, &mut factory) };
		if result.is_err() || factory.is_null() {
			return None;
		}
		// SAFETY: DirectWrite returned an owned IDWriteFactory.
		let factory = unsafe { IDWriteFactory::from_raw(factory) };
		let mut collection = std::ptr::null_mut();
		// SAFETY: Valid output pointer.
		let result = unsafe { factory.GetSystemFontCollection(&mut collection, 0) };
		if result.is_err() || collection.is_null() {
			return None;
		}
		// SAFETY: DirectWrite returned an owned IDWriteFontCollection.
		let collection = unsafe { IDWriteFontCollection::from_raw(collection) };
		Some(Self {
			factory,
			collection,
			formats: HashMap::new(),
			measurements: HashMap::new(),
			bounding_boxes: HashMap::new(),
		})
	}

	/// Width and height of a line of `text` in pixels like a browser's `getBBox`: the advances
	/// including trailing spaces and the ascent and descent of the first font.
	pub fn measure(&mut self, text: &str, font: Font) -> (f32, f32) {
		let key = format_key(font);
		if let Some(&size) = self.measurements.get(&(key.clone(), text.to_owned())) {
			return size;
		}
		let size = self.measure_uncached(text, &key, font);
		if self.measurements.len() >= MAX_CACHED_MEASUREMENTS {
			self.measurements.clear();
		}
		self.measurements.insert((key, text.to_owned()), size);
		size
	}

	fn measure_uncached(&mut self, text: &str, key: &FormatKey, font: Font) -> (f32, f32) {
		let Some(format) = self.format(key) else {
			return (text.chars().count() as f32 * font.size * 0.55, font.size * 1.15);
		};
		let height = (format.metrics.ascent + format.metrics.descent) * font.size;
		if text.is_empty() {
			return (0.0, height);
		}
		let width = self
			.layout(text, key)
			.and_then(|layout| {
				let mut metrics = DWRITE_TEXT_METRICS::default();
				// SAFETY: Valid layout and output pointer.
				unsafe { layout.GetMetrics(&mut metrics) }
					.is_ok()
					.then_some(metrics.widthIncludingTrailingWhitespace)
			})
			.unwrap_or(0.0);
		(width, height)
	}

	/// Ascent and descent of the first font of the family list in pixels, positive.
	pub fn vertical_metrics(&mut self, font: Font) -> Option<(f32, f32)> {
		let metrics = self.format(&format_key(font))?.metrics;
		Some((metrics.ascent * font.size, metrics.descent * font.size))
	}

	/// The x-height and the line gap of the first font of the family list in pixels.
	pub fn x_height_and_line_gap(&mut self, font: Font) -> Option<(f32, f32)> {
		let metrics = self.format(&format_key(font))?.metrics;
		Some((metrics.x_height * font.size, metrics.line_gap * font.size))
	}

	/// The underline of the first font of the family list in pixels: the offset of its top
	/// below the baseline and its thickness.
	pub fn underline(&mut self, font: Font) -> Option<(f32, f32)> {
		let metrics = self.format(&format_key(font))?.metrics;
		Some((-metrics.underline_position * font.size, metrics.underline_thickness * font.size))
	}

	/// The bounds of the rasterized glyphs of a line of `text` relative to the start of its
	/// baseline: left, top, right, bottom. `None` if nothing is drawn.
	pub fn ink_bounds(&mut self, text: &str, font: Font) -> Option<[f32; 4]> {
		let key = format_key(font);
		let layout = self.layout(text, &key)?;
		let bounds = Rc::new(RefCell::new(None));
		let collect =
			Collect::InkBounds { factory: self.factory.clone(), bounds: Rc::clone(&bounds) };
		let renderer: IDWriteTextRenderer = GlyphRunRenderer { collect }.into();
		// SAFETY: Valid layout and renderer; DirectWrite calls the renderer before returning.
		unsafe { layout.Draw(std::ptr::null_mut(), renderer.as_raw(), 0.0, 0.0) }.ok().ok()?;
		bounds.take()
	}

	/// A table of the first font of the family list, `None` if it has no such table.
	pub fn font_table(&mut self, font: Font, tag: [u8; 4]) -> Option<Vec<u8>> {
		let face = self.format(&format_key(font))?.face.clone()?;
		let (mut data, mut size) = (std::ptr::null(), 0);
		let (mut table_context, mut exists) = (std::ptr::null_mut(), 0);
		// SAFETY: Valid face and output pointers. DirectWrite expects the first character of the
		// tag in the lowest byte.
		unsafe {
			face.TryGetFontTable(
				u32::from_le_bytes(tag),
				&mut data,
				&mut size,
				&mut table_context,
				&mut exists,
			)
		}
		.ok()
		.ok()?;
		if exists == 0 {
			return None;
		}
		let table = match data.is_null() {
			true => Vec::new(),
			false => {
				// SAFETY: DirectWrite returned `size` readable bytes until `ReleaseFontTable`.
				unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) }.to_vec()
			}
		};
		// SAFETY: The context of the successful call above.
		unsafe { face.ReleaseFontTable(table_context) };
		Some(table)
	}

	/// The size of a design unit of the first font of the family list in pixels.
	pub fn design_unit(&mut self, font: Font) -> Option<f32> {
		Some(font.size / self.format(&format_key(font))?.units_per_em)
	}

	/// The glyph of `character` in the first font of the family list; 0 if it has none.
	pub fn glyph_index(&mut self, font: Font, character: char) -> u16 {
		let Some(face) = self.format(&format_key(font)).and_then(|format| format.face.clone())
		else {
			return 0;
		};
		let (code_point, mut glyph) = (u32::from(character), 0);
		// SAFETY: One code point and one output glyph.
		match unsafe { face.GetGlyphIndices(&code_point, 1, &mut glyph) }.is_ok() {
			true => glyph,
			false => 0,
		}
	}

	/// The advance width of a glyph of the first font of the family list in pixels.
	pub fn glyph_advance(&mut self, font: Font, glyph: u16) -> f32 {
		let Some(format) = self.format(&format_key(font)) else { return 0.0 };
		let Some(face) = format.face.clone() else { return 0.0 };
		let (units_per_em, has_bitmap_strike) = (format.units_per_em, format.has_bitmap_strike);
		let mut metrics = DWRITE_GLYPH_METRICS::default();
		// SAFETY: One glyph and one output record; the transform may be null.
		let result = unsafe {
			match has_bitmap_strike {
				true => face.GetGdiCompatibleGlyphMetrics(
					font.size,
					1.0,
					std::ptr::null(),
					0,
					&glyph,
					1,
					&mut metrics,
					0,
				),
				false => face.GetDesignGlyphMetrics(&glyph, 1, &mut metrics, 0),
			}
		};
		match result.is_ok() {
			true => metrics.advanceWidth as f32 * font.size / units_per_em,
			false => 0.0,
		}
	}

	/// The bounds of a rasterized glyph of the first font of the family list relative to its
	/// origin: left, top, right, bottom. `None` if it draws nothing.
	pub fn glyph_bounds(&mut self, font: Font, glyph: u16) -> Option<[f32; 4]> {
		let face = self.format(&format_key(font))?.face.clone()?;
		let advance = self.glyph_advance(font, glyph);
		let run = DWRITE_GLYPH_RUN {
			fontFace: face.as_raw(),
			fontEmSize: font.size,
			glyphCount: 1,
			glyphIndices: &glyph,
			glyphAdvances: &advance,
			glyphOffsets: std::ptr::null(),
			isSideways: 0,
			bidiLevel: 0,
		};
		let measuring_mode =
			match self.format(&format_key(font)).is_some_and(|f| f.has_bitmap_strike) {
				true => MEASURING_MODE_GDI_CLASSIC,
				false => MEASURING_MODE_NATURAL,
			};
		let rect = glyph_ink_bounds(&self.factory, &run, measuring_mode)?;
		Some([rect.left as f32, rect.top as f32, rect.right as f32, rect.bottom as f32])
	}

	/// The outline of a glyph of the first font of the family list as SVG path data, in pixels
	/// relative to its origin.
	pub fn glyph_outline(&mut self, font: Font, glyph: u16) -> Option<String> {
		let face = self.format(&format_key(font))?.face.clone()?;
		let path = Rc::new(RefCell::new(String::new()));
		let sink: ID2D1SimplifiedGeometrySink =
			PathSink { path: Rc::clone(&path), x_offset: 0.0 }.into();
		// SAFETY: One glyph without advances and offsets, and a valid sink for this call.
		unsafe {
			face.GetGlyphRunOutline(
				font.size,
				&glyph,
				std::ptr::null(),
				std::ptr::null(),
				1,
				0,
				0,
				sink.as_raw(),
			)
		}
		.ok()
		.ok()?;
		Some(path.take())
	}

	/// The glyph outlines of a line of `text`, shaped with font fallback.
	pub fn outline(&mut self, text: &str, font: Font) -> Option<Outline> {
		let key = format_key(font);
		let metrics = self.format(&key)?.metrics;
		let layout = self.layout(text, &key)?;
		let mut text_metrics = DWRITE_TEXT_METRICS::default();
		// SAFETY: Valid layout and output pointer.
		unsafe { layout.GetMetrics(&mut text_metrics) }.ok().ok()?;
		let path = Rc::new(RefCell::new(String::new()));
		let renderer: IDWriteTextRenderer =
			GlyphRunRenderer { collect: Collect::Outline(Rc::clone(&path)) }.into();
		// SAFETY: Valid layout and renderer; DirectWrite calls the renderer before returning.
		unsafe { layout.Draw(std::ptr::null_mut(), renderer.as_raw(), 0.0, 0.0) }.ok().ok()?;
		let path = path.take();
		Some(Outline {
			path,
			width: text_metrics.widthIncludingTrailingWhitespace,
			ascent: metrics.ascent * font.size,
			descent: metrics.descent * font.size,
			x_height: metrics.x_height * font.size,
		})
	}

	/// Width and height of a line of `text` like Chromium's `getBBox` of an SVG `<text>` at an
	/// integer position: the advances and the rounded ascent and descent, united with the pixel
	/// bounds of the rasterized glyphs.
	pub fn bounding_box(&mut self, text: &str, font: Font) -> (f32, f32) {
		let ink_box = self.ink_box(text, font);
		(ink_box.width, ink_box.height)
	}

	/// The left edge of [`TextEngine::bounding_box`] relative to the start of the text: zero, or
	/// negative where glyph pixels stick out to the left.
	pub fn bounding_box_left(&mut self, text: &str, font: Font) -> f32 {
		self.ink_box(text, font).left
	}

	/// The top edge of [`TextEngine::bounding_box`] relative to the baseline: the rounded ascent
	/// above it, or more where glyph pixels stick out above that.
	pub fn bounding_box_top(&mut self, text: &str, font: Font) -> f32 {
		self.ink_box(text, font).top
	}

	fn ink_box(&mut self, text: &str, font: Font) -> InkBox {
		let key = (format_key(font), text.to_owned());
		if let Some(&ink_box) = self.bounding_boxes.get(&key) {
			return ink_box;
		}
		let ink_box = self.ink_box_uncached(text, font);
		if self.bounding_boxes.len() >= MAX_CACHED_MEASUREMENTS {
			self.bounding_boxes.clear();
		}
		self.bounding_boxes.insert(key, ink_box);
		ink_box
	}

	fn ink_box_uncached(&mut self, text: &str, font: Font) -> InkBox {
		// Chromium's layout units: 1/64 pixel, rounded up.
		let advance = (self.measure(text, font).0 * 64.0 - 0.001).ceil() / 64.0;
		let key = format_key(font);
		let Some(metrics) = self.format(&key).map(|format| format.metrics) else {
			let (width, height) = self.measure(text, font);
			return InkBox { left: 0.0, top: -0.8 * height, width, height };
		};
		let ascent = (metrics.ascent * font.size).round();
		let descent = (metrics.descent * font.size).round();
		let without_ink =
			InkBox { left: 0.0, top: -ascent, width: advance, height: ascent + descent };
		let Some(layout) = self.layout(text, &key) else { return without_ink };
		let bounds = Rc::new(RefCell::new(None));
		let collect =
			Collect::InkBounds { factory: self.factory.clone(), bounds: Rc::clone(&bounds) };
		let renderer: IDWriteTextRenderer = GlyphRunRenderer { collect }.into();
		// SAFETY: Valid layout and renderer; DirectWrite calls the renderer before returning.
		let drawn = unsafe { layout.Draw(std::ptr::null_mut(), renderer.as_raw(), 0.0, 0.0) };
		let [left, top, right, bottom] = match bounds.take() {
			Some(bounds) if drawn.is_ok() => bounds,
			_ => return without_ink,
		};
		InkBox {
			left: left.min(0.0),
			top: (-ascent).min(top),
			width: advance.max(right) - left.min(0.0),
			height: descent.max(bottom) - (-ascent).min(top),
		}
	}

	fn layout(&mut self, text: &str, key: &FormatKey) -> Option<IDWriteTextLayout> {
		let format = self.format(key)?;
		let has_bitmap_strike = format.has_bitmap_strike;
		let format = format.format.clone();
		let text: Vec<u16> = text.encode_utf16().collect();
		let mut layout = std::ptr::null_mut();
		// SAFETY: Valid text, format and output pointer; the transform may be null.
		let result = unsafe {
			match has_bitmap_strike {
				true => self.factory.CreateGdiCompatibleTextLayout(
					text.as_ptr(),
					text.len() as u32,
					format.as_raw(),
					1.0e7,
					1.0e7,
					1.0,
					std::ptr::null(),
					0,
					&mut layout,
				),
				false => self.factory.CreateTextLayout(
					text.as_ptr(),
					text.len() as u32,
					format.as_raw(),
					1.0e7,
					1.0e7,
					&mut layout,
				),
			}
		};
		if result.is_err() || layout.is_null() {
			return None;
		}
		// SAFETY: DirectWrite returned an owned IDWriteTextLayout.
		Some(unsafe { IDWriteTextLayout::from_raw(layout) })
	}

	fn format(&mut self, key: &FormatKey) -> Option<&Format> {
		if !self.formats.contains_key(key) {
			let format = self.create_format(key);
			self.formats.insert(key.clone(), format);
		}
		self.formats.get(key)?.as_ref()
	}

	fn create_format(&self, key: &FormatKey) -> Option<Format> {
		let (family_name, family) = self.find_family(&key.family)?;
		let style = if key.is_italic { FONT_STYLE_ITALIC } else { FONT_STYLE_NORMAL };
		let weight = i32::from(key.weight.clamp(1, 999));
		let mut font = std::ptr::null_mut();
		// SAFETY: Valid family and output pointer.
		unsafe { family.GetFirstMatchingFont(weight, FONT_STRETCH_NORMAL, style, &mut font) }
			.ok()
			.ok()?;
		if font.is_null() {
			return None;
		}
		// SAFETY: DirectWrite returned an owned IDWriteFont.
		let font = unsafe { IDWriteFont::from_raw(font) };
		let mut face = std::ptr::null_mut();
		// SAFETY: Valid font and output pointer.
		let face = match unsafe { font.CreateFontFace(&mut face) }.is_ok() && !face.is_null() {
			// SAFETY: DirectWrite returned an owned IDWriteFontFace.
			true => Some(unsafe { IDWriteFontFace::from_raw(face) }),
			false => None,
		};
		let mut metrics = DWRITE_FONT_METRICS::default();
		// SAFETY: Valid font and output pointer.
		unsafe { font.GetMetrics(&mut metrics) };
		let size = f32::from_bits(key.size_bits);
		let has_bitmap_strike = face.as_ref().is_some_and(|face| has_bitmap_strike(face, size));
		if has_bitmap_strike && let Some(face) = &face {
			let mut gdi_metrics = DWRITE_FONT_METRICS::default();
			// SAFETY: Valid face and output pointer; the transform may be null.
			let result = unsafe {
				face.GetGdiCompatibleMetrics(size, 1.0, std::ptr::null(), &mut gdi_metrics)
			};
			if result.is_ok() {
				metrics = gdi_metrics;
			}
		}
		let units_per_em = f32::from(metrics.designUnitsPerEm.max(1));
		let metrics = VerticalMetrics {
			ascent: f32::from(metrics.ascent) / units_per_em,
			descent: f32::from(metrics.descent) / units_per_em,
			x_height: f32::from(metrics.xHeight) / units_per_em,
			line_gap: f32::from(metrics.lineGap) / units_per_em,
			underline_position: f32::from(metrics.underlinePosition) / units_per_em,
			underline_thickness: f32::from(metrics.underlineThickness) / units_per_em,
		};
		let name = to_wide_null_terminated(&family_name);
		let locale = to_wide_null_terminated("en-us");
		let mut format = std::ptr::null_mut();
		// SAFETY: Null-terminated strings, valid collection and output pointer.
		unsafe {
			self.factory.CreateTextFormat(
				name.as_ptr(),
				self.collection.as_raw(),
				weight,
				style,
				FONT_STRETCH_NORMAL,
				f32::from_bits(key.size_bits),
				locale.as_ptr(),
				&mut format,
			)
		}
		.ok()
		.ok()?;
		if format.is_null() {
			return None;
		}
		// SAFETY: DirectWrite returned an owned IDWriteTextFormat.
		let format = unsafe { IDWriteTextFormat::from_raw(format) };
		// SAFETY: Valid format.
		let _ = unsafe { format.SetWordWrapping(WORD_WRAPPING_NO_WRAP) };
		Some(Format { format, metrics, face, units_per_em, has_bitmap_strike })
	}

	/// The first installed family of a CSS font family list, with browser defaults for the
	/// generic families.
	fn find_family(&self, list: &str) -> Option<(String, IDWriteFontFamily)> {
		let names = list
			.split(',')
			.map(|name| name.trim().trim_matches(|c| c == '"' || c == '\''))
			.filter(|name| !name.is_empty())
			.map(|name| match name.to_ascii_lowercase().as_str() {
				"sans-serif" | "system-ui" => "Arial",
				"serif" => "Times New Roman",
				"monospace" => "Consolas",
				"cursive" => "Comic Sans MS",
				"fantasy" => "Impact",
				// Windows' font substitutes, which browsers follow.
				"courier" => "Courier New",
				"helvetica" => "Arial",
				"times" => "Times New Roman",
				_ => name,
			})
			.chain(["Arial", "Segoe UI"]);
		for name in names {
			let wide_name = to_wide_null_terminated(name);
			let (mut index, mut exists) = (0, 0);
			// SAFETY: Null-terminated name and valid output pointers.
			let found = unsafe {
				self.collection.FindFamilyName(wide_name.as_ptr(), &mut index, &mut exists)
			};
			if found.is_err() || exists == 0 {
				continue;
			}
			let mut family = std::ptr::null_mut();
			// SAFETY: Valid index and output pointer.
			if unsafe { self.collection.GetFontFamily(index, &mut family) }.is_ok()
				&& !family.is_null()
			{
				// SAFETY: DirectWrite returned an owned IDWriteFontFamily.
				return Some((name.to_owned(), unsafe { IDWriteFontFamily::from_raw(family) }));
			}
		}
		None
	}
}

/// Skia's `has_bitmap_strike`: an `EBLC` entry of the truncated pixel size for at least four
/// glyphs.
fn has_bitmap_strike(face: &IDWriteFontFace, size: f32) -> bool {
	let (mut data, mut length) = (std::ptr::null(), 0);
	let (mut table_context, mut exists) = (std::ptr::null_mut(), 0);
	// SAFETY: Valid face and output pointers.
	let result = unsafe {
		face.TryGetFontTable(
			u32::from_le_bytes(*b"EBLC"),
			&mut data,
			&mut length,
			&mut table_context,
			&mut exists,
		)
	};
	if result.is_err() || exists == 0 {
		return false;
	}
	let table = match data.is_null() {
		true => &[][..],
		// SAFETY: DirectWrite returned `length` readable bytes until `ReleaseFontTable`.
		false => unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length as usize) },
	};
	let pixel_size = ((size * 64.0).round() / 64.0).trunc();
	let found = eblc_has_strike(table, pixel_size);
	// SAFETY: The context of the successful call above.
	unsafe { face.ReleaseFontTable(table_context) };
	found
}

/// Whether the `BitmapSizeTable`s of the `EBLC` table `table` cover `pixel_size`.
fn eblc_has_strike(table: &[u8], pixel_size: f32) -> bool {
	const HEADER_SIZE: usize = 8;
	const ENTRY_SIZE: usize = 48;
	let word = |offset: usize| -> Option<u32> {
		let bytes = table.get(offset..offset + 2)?;
		Some(u32::from(u16::from_be_bytes([bytes[0], bytes[1]])))
	};
	let count = table
		.get(4..8)
		.map_or(0, |bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize);
	(0..count.min(table.len() / ENTRY_SIZE)).any(|index| {
		let entry = HEADER_SIZE + index * ENTRY_SIZE;
		let (Some(start), Some(end)) = (word(entry + 40), word(entry + 42)) else { return false };
		let (Some(&ppem_x), Some(&ppem_y)) = (table.get(entry + 44), table.get(entry + 45)) else {
			return false;
		};
		f32::from(ppem_x) == pixel_size && f32::from(ppem_y) == pixel_size && end >= start + 3
	})
}

fn format_key(font: Font) -> FormatKey {
	FormatKey {
		family: font.family.to_owned(),
		size_bits: font.size.max(0.1).to_bits(),
		weight: font.weight,
		is_italic: font.is_italic,
	}
}

/// A coordinate for path data: two decimals, without trailing zeros.
fn push_coordinate(path: &mut String, value: f32) {
	let rounded = (value * 100.0).round() / 100.0;
	let _ = write!(path, "{}", if rounded == 0.0 { 0.0 } else { rounded });
}

fn push_point(path: &mut String, point: D2D1_POINT_2F, x_offset: f32) {
	push_coordinate(path, point.x + x_offset);
	path.push(',');
	push_coordinate(path, point.y);
}

/// What [`GlyphRunRenderer`] collects of the glyph runs of a text layout.
enum Collect {
	/// Their outlines as SVG path data.
	Outline(Rc<RefCell<String>>),
	/// The pixel bounds of their rasterized glyphs: left, top, right, bottom.
	InkBounds { factory: IDWriteFactory, bounds: Rc<RefCell<Option<[f32; 4]>>> },
}

/// Receives the glyph runs of a text layout.
#[implement(IDWriteTextRenderer)]
struct GlyphRunRenderer {
	collect: Collect,
}

impl IDWritePixelSnapping_Impl for GlyphRunRenderer_Impl {
	unsafe fn IsPixelSnappingDisabled(&self, _: *mut c_void, is_disabled: *mut BOOL) -> HRESULT {
		// SAFETY: DirectWrite passes a valid output pointer.
		unsafe { *is_disabled = 1 };
		HRESULT(0)
	}

	unsafe fn GetCurrentTransform(&self, _: *mut c_void, transform: *mut DWRITE_MATRIX) -> HRESULT {
		let identity = DWRITE_MATRIX { m11: 1.0, m22: 1.0, ..DWRITE_MATRIX::default() };
		// SAFETY: DirectWrite passes a valid output pointer.
		unsafe { *transform = identity };
		HRESULT(0)
	}

	unsafe fn GetPixelsPerDip(&self, _: *mut c_void, pixels_per_dip: *mut f32) -> HRESULT {
		// SAFETY: DirectWrite passes a valid output pointer.
		unsafe { *pixels_per_dip = 1.0 };
		HRESULT(0)
	}
}

impl IDWriteTextRenderer_Impl for GlyphRunRenderer_Impl {
	unsafe fn DrawGlyphRun(
		&self,
		_: *mut c_void,
		baseline_x: f32,
		baseline_y: f32,
		measuring_mode: i32,
		glyph_run: *const DWRITE_GLYPH_RUN,
		_: *const c_void,
		_: *mut c_void,
	) -> HRESULT {
		// SAFETY: DirectWrite passes a valid glyph run, borrowed for this call.
		let Some(run) = (unsafe { glyph_run.as_ref() }) else { return HRESULT(0) };
		let path = match &self.collect {
			Collect::Outline(path) => path,
			Collect::InkBounds { factory, bounds } => {
				// Like Skia: the bounds of each glyph drawn at the origin, moved to its position on
				// the baseline.
				let _ = baseline_y;
				let mut pen = baseline_x;
				for index in 0..run.glyphCount as usize {
					// SAFETY: The arrays hold `glyphCount` entries; offsets may be null.
					let (glyph, advance, offset) = unsafe {
						let offset = match run.glyphOffsets.is_null() {
							true => 0.0,
							false => (*run.glyphOffsets.add(index)).advanceOffset,
						};
						(*run.glyphIndices.add(index), *run.glyphAdvances.add(index), offset)
					};
					let single = DWRITE_GLYPH_RUN {
						fontFace: run.fontFace,
						fontEmSize: run.fontEmSize,
						glyphCount: 1,
						glyphIndices: &glyph,
						glyphAdvances: &advance,
						glyphOffsets: std::ptr::null(),
						isSideways: run.isSideways,
						bidiLevel: run.bidiLevel,
					};
					if let Some(rect) = glyph_ink_bounds(factory, &single, measuring_mode) {
						let x = pen + offset;
						let glyph = [
							x + rect.left as f32,
							rect.top as f32,
							x + rect.right as f32,
							rect.bottom as f32,
						];
						let mut bounds = bounds.borrow_mut();
						*bounds = Some(match *bounds {
							None => glyph,
							Some(known) => [
								known[0].min(glyph[0]),
								known[1].min(glyph[1]),
								known[2].max(glyph[2]),
								known[3].max(glyph[3]),
							],
						});
					}
					pen += advance;
				}
				return HRESULT(0);
			}
		};
		// SAFETY: The glyph run holds a borrowed font face.
		let Some(face) = (unsafe { IDWriteFontFace::from_raw_borrowed(&run.fontFace) }) else {
			return HRESULT(0);
		};
		// The outlines are relative to the run's baseline origin; a single line has one baseline.
		let sink: ID2D1SimplifiedGeometrySink =
			PathSink { path: Rc::clone(path), x_offset: baseline_x }.into();
		// SAFETY: The arrays of the glyph run and the sink are valid for this call.
		unsafe {
			face.GetGlyphRunOutline(
				run.fontEmSize,
				run.glyphIndices,
				run.glyphAdvances,
				run.glyphOffsets,
				run.glyphCount,
				run.isSideways,
				BOOL::from(run.bidiLevel % 2 == 1),
				sink.as_raw(),
			)
		}
	}

	unsafe fn DrawUnderline(
		&self,
		_: *mut c_void,
		_: f32,
		_: f32,
		_: *const c_void,
		_: *mut c_void,
	) -> HRESULT {
		HRESULT(0)
	}

	unsafe fn DrawStrikethrough(
		&self,
		_: *mut c_void,
		_: f32,
		_: f32,
		_: *const c_void,
		_: *mut c_void,
	) -> HRESULT {
		HRESULT(0)
	}

	unsafe fn DrawInlineObject(
		&self,
		_: *mut c_void,
		_: f32,
		_: f32,
		_: *mut c_void,
		_: BOOL,
		_: BOOL,
		_: *mut c_void,
	) -> HRESULT {
		HRESULT(0)
	}
}

/// `DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC` and `DWRITE_TEXTURE_CLEARTYPE_3x1`, what Skia
/// rasterizes glyphs with.
const RENDERING_MODE_NATURAL_SYMMETRIC: i32 = 5;
/// `DWRITE_MEASURING_MODE_NATURAL`, what text layouts measure with.
const MEASURING_MODE_NATURAL: i32 = 0;
/// `DWRITE_MEASURING_MODE_GDI_CLASSIC`, for fonts with a bitmap strike of the size.
const MEASURING_MODE_GDI_CLASSIC: i32 = 1;
/// `DWRITE_RENDERING_MODE_GDI_CLASSIC`, what Skia rasterizes them with.
const RENDERING_MODE_GDI_CLASSIC: i32 = 2;
const TEXTURE_CLEARTYPE_3X1: i32 = 1;

/// The pixel bounds of a glyph run drawn at the origin.
fn glyph_ink_bounds(
	factory: &IDWriteFactory,
	run: &DWRITE_GLYPH_RUN,
	measuring_mode: i32,
) -> Option<RECT> {
	let (rendering_mode, texture) = match measuring_mode {
		MEASURING_MODE_GDI_CLASSIC => (RENDERING_MODE_GDI_CLASSIC, TEXTURE_CLEARTYPE_3X1),
		_ => (RENDERING_MODE_NATURAL_SYMMETRIC, TEXTURE_CLEARTYPE_3X1),
	};
	let mut analysis = std::ptr::null_mut();
	// SAFETY: Valid glyph run and output pointer.
	let result = unsafe {
		factory.CreateGlyphRunAnalysis(
			run,
			1.0,
			std::ptr::null(),
			rendering_mode,
			measuring_mode,
			0.0,
			0.0,
			&mut analysis,
		)
	};
	if result.is_err() || analysis.is_null() {
		return None;
	}
	// SAFETY: DirectWrite returned an owned IDWriteGlyphRunAnalysis.
	let analysis = unsafe { IDWriteGlyphRunAnalysis::from_raw(analysis) };
	let mut rect = RECT::default();
	// SAFETY: Valid analysis and output pointer.
	unsafe { analysis.GetAlphaTextureBounds(texture, &mut rect) }.ok().ok()?;
	(rect.right > rect.left).then_some(rect)
}

/// Writes glyph outlines as SVG path data, shifted by `x_offset`.
#[implement(ID2D1SimplifiedGeometrySink)]
struct PathSink {
	path: Rc<RefCell<String>>,
	x_offset: f32,
}

impl ID2D1SimplifiedGeometrySink_Impl for PathSink_Impl {
	unsafe fn SetFillMode(&self, _: i32) {}

	unsafe fn SetSegmentFlags(&self, _: i32) {}

	unsafe fn BeginFigure(&self, start: D2D1_POINT_2F, _: i32) {
		let mut path = self.path.borrow_mut();
		path.push('M');
		push_point(&mut path, start, self.x_offset);
	}

	unsafe fn AddLines(&self, points: *const D2D1_POINT_2F, count: u32) {
		if points.is_null() {
			return;
		}
		// SAFETY: DirectWrite passes `count` points.
		let points = unsafe { std::slice::from_raw_parts(points, count as usize) };
		let mut path = self.path.borrow_mut();
		for &point in points {
			path.push('L');
			push_point(&mut path, point, self.x_offset);
		}
	}

	unsafe fn AddBeziers(&self, beziers: *const D2D1_BEZIER_SEGMENT, count: u32) {
		if beziers.is_null() {
			return;
		}
		// SAFETY: DirectWrite passes `count` segments.
		let beziers = unsafe { std::slice::from_raw_parts(beziers, count as usize) };
		let mut path = self.path.borrow_mut();
		for bezier in beziers {
			path.push('C');
			push_point(&mut path, bezier.point1, self.x_offset);
			path.push(' ');
			push_point(&mut path, bezier.point2, self.x_offset);
			path.push(' ');
			push_point(&mut path, bezier.point3, self.x_offset);
		}
	}

	unsafe fn EndFigure(&self, end: i32) {
		if end == FIGURE_END_CLOSED {
			self.path.borrow_mut().push('Z');
		}
	}

	unsafe fn Close(&self) -> HRESULT {
		HRESULT(0)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const FONT: Font = Font {
		family: "\"trebuchet ms\", verdana, arial, sans-serif",
		size: 16.0,
		weight: 400,
		is_italic: false,
	};

	#[test]
	fn measures_like_a_browser() {
		let mut engine = TextEngine::new().expect("DirectWrite");
		let (width, height) = engine.measure("Hello World", FONT);
		assert!(width > 70.0 && width < 100.0, "{width}");
		assert!(height > 16.0 && height < 20.0, "{height}");
		let (with_space, _) = engine.measure("Hello World ", FONT);
		assert!(with_space > width);
		assert_eq!(engine.measure("", FONT).0, 0.0);
		// Font fallback and shaping.
		assert!(engine.measure("漢字 مرحبا 😀", FONT).0 > 0.0);
		let bold = engine.measure("Hello World", Font { weight: 700, ..FONT }).0;
		assert!(bold > width);
		// Chromium's bounding box reaches the pixels of the `A` left of the origin.
		let arial = Font { family: "Arial", ..FONT };
		let (bounding, _) = engine.bounding_box("Alice", arial);
		assert!(bounding > engine.measure("Alice", arial).0 + 0.5, "{bounding}");
	}

	#[test]
	fn outlines_glyphs() {
		let mut engine = TextEngine::new().expect("DirectWrite");
		let outline = engine.outline("Hi", FONT).unwrap();
		assert!(outline.path.starts_with('M') && outline.path.ends_with('Z'), "{}", outline.path);
		assert!((outline.width - engine.measure("Hi", FONT).0).abs() < 0.01);
		assert!(outline.ascent > outline.x_height && outline.descent > 0.0);
		assert_eq!(engine.outline("", FONT).map(|outline| outline.path), Some(String::new()));
		let unknown = Font { family: "No Such Font", ..FONT };
		assert!(engine.outline("x", unknown).is_some());
	}

	#[test]
	fn reads_glyphs_and_tables_of_a_font() {
		let mut engine = TextEngine::new().expect("DirectWrite");
		let font = Font { family: "Cambria Math", ..FONT };
		let glyph = engine.glyph_index(font, 'x');
		assert_ne!(glyph, 0);
		let advance = engine.glyph_advance(font, glyph);
		assert!(advance > 4.0 && advance < 16.0, "{advance}");
		let [left, top, right, bottom] = engine.glyph_bounds(font, glyph).unwrap();
		assert!(left < right && top < bottom && bottom <= 1.0, "{top} {bottom}");
		assert!(engine.glyph_outline(font, glyph).unwrap().starts_with('M'));
		assert!(engine.ink_bounds("x", font).is_some());
		assert_eq!(engine.ink_bounds(" ", font), None);
		assert!(engine.font_table(font, *b"MATH").is_some_and(|table| table.len() > 100));
		assert!(engine.font_table(FONT, *b"NONE").is_none());
		assert!(engine.design_unit(font).is_some());
	}

	#[test]
	fn uses_gdi_metrics_for_bitmap_strikes() {
		let mut engine = TextEngine::new().expect("DirectWrite");
		// Cambria Math has bitmaps of 16 pixels and none of 14 (probed in Edge: 9 and 7.453).
		let font = Font { family: "Cambria Math", ..FONT };
		assert_eq!(engine.measure("\u{1D465}", font).0, 9.0);
		let at_14 = engine.measure("\u{1D465}", Font { size: 14.0, ..font }).0;
		assert!((at_14 - 7.453).abs() < 0.01, "{at_14}");
		let glyph = engine.glyph_index(font, 'x');
		assert_eq!(engine.glyph_advance(font, glyph).fract(), 0.0);
		assert!(!eblc_has_strike(&[], 16.0));
		// Two entries: 12 and 16 pixels, the first for 3 glyphs only.
		let mut table = vec![0, 2, 0, 0, 0, 0, 0, 2];
		for (start, end, ppem) in [(1u16, 3u16, 12u8), (1, 4, 16)] {
			let mut entry = [0u8; 48];
			entry[40..42].copy_from_slice(&start.to_be_bytes());
			entry[42..44].copy_from_slice(&end.to_be_bytes());
			entry[44] = ppem;
			entry[45] = ppem;
			table.extend_from_slice(&entry);
		}
		assert!(!eblc_has_strike(&table, 12.0));
		assert!(eblc_has_strike(&table, 16.0));
		assert!(!eblc_has_strike(&table, 17.0));
		assert!(!eblc_has_strike(&table[..30], 16.0));
	}
}
