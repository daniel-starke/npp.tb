//! Text measurement and the text helpers shared by the diagrams.

/// A CSS font.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
	/// A CSS font family list.
	pub family: String,
	/// In pixels.
	pub size: f64,
	/// CSS weight: 400 normal, 700 bold.
	pub weight: u16,
	pub is_italic: bool,
}

impl Font {
	pub fn new(family: impl Into<String>, size: f64) -> Self {
		Self { family: family.into(), size, weight: 400, is_italic: false }
	}
}

/// Measures a line of text like a browser's `getBBox`: width of the advances and height of the
/// line box.
pub trait TextMeasure {
	fn measure(&mut self, text: &str, font: &Font) -> (f64, f64);

	/// Size of the bounding box of an SVG `<text>`, which includes glyph pixels outside the
	/// advances and the line.
	fn bounding_box(&mut self, text: &str, font: &Font) -> (f64, f64) {
		self.measure(text, font)
	}

	/// The left edge of [`TextMeasure::bounding_box`] relative to the start of the text, negative
	/// where glyphs stick out to the left.
	fn bounding_box_left(&mut self, _text: &str, _font: &Font) -> f64 {
		0.0
	}

	/// The top edge of [`TextMeasure::bounding_box`] relative to the baseline, negative above it:
	/// the rounded ascent, or more where glyphs stick out above it.
	fn bounding_box_top(&mut self, _text: &str, font: &Font) -> f64 {
		-self.vertical_metrics(font).0.round()
	}

	/// Ascent and descent of the font in pixels, positive, which place baselines in line boxes.
	fn vertical_metrics(&mut self, font: &Font) -> (f64, f64) {
		(font.size * 0.9, font.size * 0.25)
	}

	/// The x-height of the font in pixels, CSS's `ex`.
	fn x_height(&mut self, font: &Font) -> f64 {
		font.size * 0.5
	}

	/// The line gap of the font in pixels, part of CSS's `line-height: normal`.
	fn line_gap(&mut self, _font: &Font) -> f64 {
		0.0
	}

	/// The offset of the top of the underline below the baseline and its thickness.
	fn underline(&mut self, font: &Font) -> (f64, f64) {
		(font.size * 0.1, font.size / 16.0)
	}

	/// The bounds of the drawn pixels of a line of text relative to the start of its baseline,
	/// `[left, top, right, bottom]`; `None` if it draws nothing. Math tokens have this height.
	fn ink_bounds(&mut self, text: &str, font: &Font) -> Option<[f64; 4]> {
		let (width, _) = self.measure(text.trim(), font);
		(width > 0.0).then(|| [0.0, -font.size * 0.7, width, font.size * 0.2])
	}

	/// A table of the first font of `font`'s family list, such as the OpenType `MATH` table.
	fn font_table(&mut self, _font: &Font, _tag: [u8; 4]) -> Option<Vec<u8>> {
		None
	}

	/// The size of a design unit of the first font of `font`'s family list in pixels.
	fn design_unit(&mut self, font: &Font) -> f64 {
		font.size / 1000.0
	}

	/// The glyph of `character` in the first font of `font`'s family list; 0 if it has none.
	fn glyph_index(&mut self, _font: &Font, _character: char) -> u16 {
		0
	}

	/// The advance width of a glyph of the first font of `font`'s family list.
	fn glyph_advance(&mut self, _font: &Font, _glyph: u16) -> f64 {
		0.0
	}

	/// The bounds of the drawn pixels of a glyph relative to its origin, `[left, top, right,
	/// bottom]`; `None` if it draws nothing.
	fn glyph_bounds(&mut self, _font: &Font, _glyph: u16) -> Option<[f64; 4]> {
		None
	}

	/// The outline of a glyph as SVG path data relative to its origin.
	fn glyph_outline(&mut self, _font: &Font, _glyph: u16) -> Option<String> {
		None
	}
}

/// Glyph outlines of a line of text, in pixels with the origin at the start of the baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
	/// SVG path data.
	pub path: String,
	pub width: f64,
	/// Vertical metrics of the first font, positive.
	pub ascent: f64,
	pub descent: f64,
	pub x_height: f64,
}

/// Converts text to glyph outlines, for renderers without text support.
pub trait TextOutline {
	fn outline(&mut self, text: &str, font: &Font) -> Option<Outline>;
}

/// Text as an SVG `<text>` element shows it by default: runs of whitespace as one space, without
/// leading and trailing whitespace. No-break spaces and other Unicode spaces stay.
pub fn collapse_whitespace(text: &str) -> String {
	text.split([' ', '\t', '\n', '\r', '\x0c'])
		.filter(|part| !part.is_empty())
		.collect::<Vec<_>>()
		.join(" ")
}

/// CSS `font-size` text like `16px` or `1.2em` in pixels, Mermaid's `parseFontSize`.
pub fn parse_font_size(text: &str, base: f64) -> Option<f64> {
	let number = super::value::parse_leading_number(text)?;
	match text.trim().ends_with("em") && !text.trim().ends_with("rem") {
		true => Some(number * base),
		false => Some(number),
	}
}

/// JavaScript's `number.toFixed(digits)` for the small numbers of labels.
pub fn to_fixed(number: f64, digits: u32) -> String {
	if !number.is_finite() {
		return super::color::js_number(number);
	}
	let scale = 10f64.powi(digits as i32);
	let scaled = number.abs() * scale;
	// Halves round up, like the exact decimal rounding of `toFixed` for these magnitudes.
	let floor = scaled.floor();
	let rounded = if scaled - floor >= 0.5 { floor + 1.0 } else { floor };
	let sign = if number < 0.0 && rounded != 0.0 { "-" } else { "" };
	match digits {
		0 => format!("{sign}{rounded}"),
		_ => {
			let integer = (rounded / scale).floor();
			let fraction = rounded - integer * scale;
			format!("{sign}{integer}.{fraction:0width$}", width = digits as usize)
		}
	}
}

#[cfg(test)]
pub mod testing {
	use super::{Font, Outline, TextMeasure, TextOutline};

	/// Deterministic metrics for tests: every character is 0.6 em wide, lines 1.2 em high.
	pub struct FixedMeasure;

	impl TextMeasure for FixedMeasure {
		fn measure(&mut self, text: &str, font: &Font) -> (f64, f64) {
			(text.chars().count() as f64 * font.size * 0.6, font.size * 1.2)
		}

		fn vertical_metrics(&mut self, font: &Font) -> (f64, f64) {
			(font.size * 0.9, font.size * 0.3)
		}
	}

	/// A box per line of text, with the metrics of [`FixedMeasure`].
	impl TextOutline for FixedMeasure {
		fn outline(&mut self, text: &str, font: &Font) -> Option<Outline> {
			let width = self.measure(text, font).0;
			let ascent = font.size * 0.9;
			Some(Outline {
				path: format!("M0,{}H{width}V0Z", -ascent),
				width,
				ascent,
				descent: font.size * 0.3,
				x_height: font.size * 0.5,
			})
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_like_to_fixed() {
		assert_eq!(to_fixed(33.333, 0), "33");
		assert_eq!(to_fixed(2.5, 0), "3");
		assert_eq!(to_fixed(0.49999999999999994, 0), "0");
		assert_eq!(to_fixed(-0.2, 0), "0");
		assert_eq!(to_fixed(1.25, 1), "1.3");
		assert_eq!(to_fixed(3.0, 2), "3.00");
	}

	#[test]
	fn parses_font_sizes() {
		assert_eq!(parse_font_size("25px", 16.0), Some(25.0));
		assert_eq!(parse_font_size("1.5em", 16.0), Some(24.0));
		assert_eq!(parse_font_size("x", 16.0), None);
	}
}
