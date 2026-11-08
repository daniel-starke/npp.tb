//! Text measurement and outlines for Mermaid diagrams with DirectWrite.

use npp::direct_write::{self, TextEngine};

use crate::mermaid::{Font, Outline, TextMeasure, TextOutline};

fn engine_font(font: &Font) -> direct_write::Font<'_> {
	direct_write::Font {
		family: &font.family,
		size: font.size as f32,
		weight: font.weight,
		is_italic: font.is_italic,
	}
}

impl TextMeasure for TextEngine {
	fn measure(&mut self, text: &str, font: &Font) -> (f64, f64) {
		let (width, height) = TextEngine::measure(self, text, engine_font(font));
		(f64::from(width), f64::from(height))
	}

	fn bounding_box(&mut self, text: &str, font: &Font) -> (f64, f64) {
		let (width, height) = TextEngine::bounding_box(self, text, engine_font(font));
		(f64::from(width), f64::from(height))
	}

	fn bounding_box_left(&mut self, text: &str, font: &Font) -> f64 {
		f64::from(TextEngine::bounding_box_left(self, text, engine_font(font)))
	}

	fn bounding_box_top(&mut self, text: &str, font: &Font) -> f64 {
		f64::from(TextEngine::bounding_box_top(self, text, engine_font(font)))
	}

	fn vertical_metrics(&mut self, font: &Font) -> (f64, f64) {
		match TextEngine::vertical_metrics(self, engine_font(font)) {
			Some((ascent, descent)) => (f64::from(ascent), f64::from(descent)),
			None => (font.size * 0.9, font.size * 0.25),
		}
	}

	fn x_height(&mut self, font: &Font) -> f64 {
		match TextEngine::x_height_and_line_gap(self, engine_font(font)) {
			Some((x_height, _)) => f64::from(x_height),
			None => font.size * 0.5,
		}
	}

	fn line_gap(&mut self, font: &Font) -> f64 {
		match TextEngine::x_height_and_line_gap(self, engine_font(font)) {
			Some((_, line_gap)) => f64::from(line_gap),
			None => 0.0,
		}
	}

	fn underline(&mut self, font: &Font) -> (f64, f64) {
		match TextEngine::underline(self, engine_font(font)) {
			Some((offset, thickness)) => (f64::from(offset), f64::from(thickness)),
			None => (font.size * 0.1, font.size / 16.0),
		}
	}

	fn ink_bounds(&mut self, text: &str, font: &Font) -> Option<[f64; 4]> {
		TextEngine::ink_bounds(self, text, engine_font(font)).map(|bounds| bounds.map(f64::from))
	}

	fn font_table(&mut self, font: &Font, tag: [u8; 4]) -> Option<Vec<u8>> {
		TextEngine::font_table(self, engine_font(font), tag)
	}

	fn design_unit(&mut self, font: &Font) -> f64 {
		TextEngine::design_unit(self, engine_font(font)).map_or(font.size / 1000.0, f64::from)
	}

	fn glyph_index(&mut self, font: &Font, character: char) -> u16 {
		TextEngine::glyph_index(self, engine_font(font), character)
	}

	fn glyph_advance(&mut self, font: &Font, glyph: u16) -> f64 {
		f64::from(TextEngine::glyph_advance(self, engine_font(font), glyph))
	}

	fn glyph_bounds(&mut self, font: &Font, glyph: u16) -> Option<[f64; 4]> {
		TextEngine::glyph_bounds(self, engine_font(font), glyph).map(|bounds| bounds.map(f64::from))
	}

	fn glyph_outline(&mut self, font: &Font, glyph: u16) -> Option<String> {
		TextEngine::glyph_outline(self, engine_font(font), glyph)
	}
}

impl TextOutline for TextEngine {
	fn outline(&mut self, text: &str, font: &Font) -> Option<Outline> {
		let outline = TextEngine::outline(self, text, engine_font(font))?;
		Some(Outline {
			path: outline.path,
			width: f64::from(outline.width),
			ascent: f64::from(outline.ascent),
			descent: f64::from(outline.descent),
			x_height: f64::from(outline.x_height),
		})
	}
}

/// Rough metrics where DirectWrite is missing, which does not happen on supported Windows.
struct EstimatedText;

impl TextMeasure for EstimatedText {
	fn measure(&mut self, text: &str, font: &Font) -> (f64, f64) {
		(text.chars().count() as f64 * font.size * 0.55, font.size * 1.15)
	}
}

impl TextOutline for EstimatedText {
	fn outline(&mut self, _: &str, _: &Font) -> Option<Outline> {
		None
	}
}

/// The text system for [`crate::mermaid::DiagramRenderer`].
pub fn text_system() -> Box<dyn crate::mermaid::TextSystem> {
	match TextEngine::new() {
		Some(engine) => Box::new(engine),
		None => Box::new(EstimatedText),
	}
}
