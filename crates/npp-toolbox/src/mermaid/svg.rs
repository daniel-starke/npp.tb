//! Writing SVG markup.

use std::fmt::Write as _;
use std::ops::Range;

use super::text::{Font, TextOutline, collapse_whitespace};
use super::{Context, Diagram};

/// A number for SVG: at most three decimals, no trailing zeros, no `-0`, `0` for NaN.
pub fn number(value: f64) -> String {
	if !value.is_finite() {
		return "0".to_owned();
	}
	let rounded = (value * 1000.0).round() / 1000.0;
	if rounded == 0.0 {
		return "0".to_owned();
	}
	let mut text = format!("{rounded:.3}");
	while text.ends_with('0') {
		text.pop();
	}
	if text.ends_with('.') {
		text.pop();
	}
	text
}

pub fn push_escaped(output: &mut String, text: &str) {
	for character in text.chars() {
		match character {
			'&' => output.push_str("&amp;"),
			'<' => output.push_str("&lt;"),
			'>' => output.push_str("&gt;"),
			'"' => output.push_str("&quot;"),
			// XML forbids most control characters.
			'\t' | '\n' | '\r' => output.push(' '),
			control if control < ' ' || control == '\u{FFFE}' || control == '\u{FFFF}' => {}
			other => output.push(other),
		}
	}
}

/// `text-anchor`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
	Start,
	Middle,
	End,
}

/// `dominant-baseline`: where `y` is in the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Baseline {
	Alphabetic,
	/// Halfway between the (rounded) ascent and descent.
	Central,
	/// Half the x-height above the alphabetic baseline.
	Middle,
	/// Four fifths of the (rounded) ascent below the top, in 1/64 px like Blink.
	Hanging,
	/// The (rounded) ascent below the top.
	TextBeforeEdge,
}

/// A line of text for [`Svg::text`].
pub struct TextRun<'a> {
	pub x: f64,
	pub y: f64,
	pub anchor: Anchor,
	pub baseline: Baseline,
	pub font: &'a Font,
	pub content: &'a str,
}

/// A `<text>` element written by [`Svg::text`], for replacing it by outlines.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedText {
	/// The whole element in the SVG.
	pub element: Range<usize>,
	/// The presentation attributes after the font attributes, with their leading space.
	pub attributes: Range<usize>,
	pub x: f64,
	pub y: f64,
	pub anchor: Anchor,
	pub baseline: Baseline,
	pub font: Font,
	pub content: String,
}

/// An SVG document under construction.
#[derive(Default)]
pub struct Svg {
	pub out: String,
	pub texts: Vec<RecordedText>,
}

impl Svg {
	pub fn new() -> Self {
		Self { out: String::with_capacity(4096), texts: Vec::new() }
	}

	/// A `<text>` element with its font, and the presentation attributes `attributes` writes
	/// (fill, class, transform, …). All text goes through here, so that it can be converted to
	/// outlines. Whitespace is collapsed like browsers do.
	pub fn text(&mut self, run: TextRun, attributes: impl FnOnce(&mut Svg)) {
		let content = collapse_whitespace(run.content);
		self.write_text(run, content, false, attributes);
	}

	/// [`Svg::text`] keeping every space, like `white-space: pre`.
	pub fn text_with_spaces(&mut self, run: TextRun, attributes: impl FnOnce(&mut Svg)) {
		let content = run.content.to_owned();
		self.write_text(run, content, true, attributes);
	}

	fn write_text(
		&mut self,
		run: TextRun,
		content: String,
		keeps_spaces: bool,
		attributes: impl FnOnce(&mut Svg),
	) {
		let start = self.out.len();
		self.open("text").number_attr("x", run.x).number_attr("y", run.y);
		if keeps_spaces {
			self.attr("xml:space", "preserve");
		}
		match run.anchor {
			Anchor::Start => {}
			Anchor::Middle => _ = self.attr("text-anchor", "middle"),
			Anchor::End => _ = self.attr("text-anchor", "end"),
		}
		match run.baseline {
			Baseline::Alphabetic => {}
			Baseline::Central => _ = self.attr("dominant-baseline", "central"),
			Baseline::Middle => _ = self.attr("dominant-baseline", "middle"),
			Baseline::Hanging => _ = self.attr("dominant-baseline", "hanging"),
			Baseline::TextBeforeEdge => _ = self.attr("dominant-baseline", "text-before-edge"),
		}
		self.attr("font-family", &run.font.family).number_attr("font-size", run.font.size);
		if run.font.weight != 400 {
			self.attr("font-weight", &run.font.weight.to_string());
		}
		if run.font.is_italic {
			self.attr("font-style", "italic");
		}
		let attributes_start = self.out.len();
		attributes(self);
		let attributes_end = self.out.len();
		self.text_element("text", &content);
		self.texts.push(RecordedText {
			element: start..self.out.len(),
			attributes: attributes_start..attributes_end,
			x: run.x,
			y: run.y,
			anchor: run.anchor,
			baseline: run.baseline,
			font: run.font.clone(),
			content,
		});
	}

	/// Appends `other`, keeping its recorded text.
	pub fn append(&mut self, other: Svg) {
		let offset = self.out.len();
		self.out.push_str(&other.out);
		self.texts.extend(other.texts.into_iter().map(|mut text| {
			text.element = text.element.start + offset..text.element.end + offset;
			text.attributes = text.attributes.start + offset..text.attributes.end + offset;
			text
		}));
	}

	/// `<tag` — attributes follow, then [`Self::end`], [`Self::close`] or [`Self::text_element`].
	pub fn open(&mut self, tag: &str) -> &mut Self {
		self.out.push('<');
		self.out.push_str(tag);
		self
	}

	pub fn attr(&mut self, name: &str, value: &str) -> &mut Self {
		self.out.push(' ');
		self.out.push_str(name);
		self.out.push_str("=\"");
		push_escaped(&mut self.out, value);
		self.out.push('"');
		self
	}

	/// An attribute only if `value` is not empty.
	pub fn attr_if(&mut self, name: &str, value: &str) -> &mut Self {
		if !value.is_empty() {
			self.attr(name, value);
		}
		self
	}

	pub fn number_attr(&mut self, name: &str, value: f64) -> &mut Self {
		let _ = write!(self.out, " {name}=\"{}\"", number(value));
		self
	}

	/// `>` of an element with content.
	pub fn end(&mut self) -> &mut Self {
		self.out.push('>');
		self
	}

	/// `/>` of an empty element.
	pub fn close(&mut self) -> &mut Self {
		self.out.push_str("/>");
		self
	}

	/// `>text</tag>`.
	pub fn text_element(&mut self, tag: &str, text: &str) -> &mut Self {
		self.out.push('>');
		push_escaped(&mut self.out, text);
		self.out.push_str("</");
		self.out.push_str(tag);
		self.out.push('>');
		self
	}

	pub fn close_tag(&mut self, tag: &str) -> &mut Self {
		self.out.push_str("</");
		self.out.push_str(tag);
		self.out.push('>');
		self
	}
}

/// The view box and size of a finished diagram.
pub struct Frame {
	pub x: f64,
	pub y: f64,
	pub width: f64,
	pub height: f64,
	/// Mermaid's `useMaxWidth`: shrink to the available width instead of a fixed size.
	pub use_max_width: bool,
}

/// Accessibility texts of a diagram.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Accessibility {
	pub title: String,
	pub description: String,
}

/// Wraps `body` in the `<svg>` root element with the diagram's font and text color, like
/// Mermaid's root style rule, and its accessibility title and description.
pub fn document(
	context: &Context,
	role: &str,
	frame: &Frame,
	accessibility: &Accessibility,
	body: Svg,
) -> Diagram {
	let mut svg = Svg::new();
	svg.open("svg")
		.attr("xmlns", "http://www.w3.org/2000/svg")
		.attr("xmlns:xlink", "http://www.w3.org/1999/xlink")
		.attr("id", &context.id)
		.number_attr("width", frame.width)
		.number_attr("height", frame.height)
		.attr(
			"viewBox",
			&format!(
				"{} {} {} {}",
				number(frame.x),
				number(frame.y),
				number(frame.width),
				number(frame.height)
			),
		)
		.attr("role", "graphics-document document")
		.attr("aria-roledescription", role);
	if frame.use_max_width {
		svg.attr("style", "max-width:100%;height:auto");
	}
	if !accessibility.title.is_empty() {
		svg.attr("aria-labelledby", &format!("{}-title", context.id));
	}
	if !accessibility.description.is_empty() {
		svg.attr("aria-describedby", &format!("{}-description", context.id));
	}
	svg.attr("font-family", &context.font_family())
		.attr("font-size", &context.theme.text("fontSize").unwrap_or_else(|| "16px".to_owned()))
		.attr_if("fill", &context.theme.css("textColor"))
		.end();
	if !accessibility.title.is_empty() {
		svg.open("title")
			.attr("id", &format!("{}-title", context.id))
			.text_element("title", &accessibility.title);
	}
	if !accessibility.description.is_empty() {
		svg.open("desc")
			.attr("id", &format!("{}-description", context.id))
			.text_element("desc", &accessibility.description);
	}
	svg.append(body);
	svg.close_tag("svg");
	Diagram { svg: svg.out, width: frame.width, height: frame.height, texts: svg.texts }
}

/// The SVG of `diagram` with its text as glyph outlines, for renderers without text support.
/// Text that cannot be outlined stays text.
pub fn outline_text(diagram: &Diagram, outliner: &mut dyn TextOutline) -> String {
	let svg = &diagram.svg;
	let mut output = String::with_capacity(svg.len() * 2);
	let mut written = 0;
	for text in &diagram.texts {
		let Some(outline) = outliner.outline(&text.content, &text.font) else { continue };
		let (Some(before), Some(attributes)) =
			(svg.get(written..text.element.start), svg.get(text.attributes.clone()))
		else {
			continue;
		};
		output.push_str(before);
		written = text.element.end;
		if outline.path.is_empty() {
			continue;
		}
		let x = match text.anchor {
			Anchor::Start => text.x,
			Anchor::Middle => text.x - outline.width / 2.0,
			Anchor::End => text.x - outline.width,
		};
		let y = match text.baseline {
			Baseline::Alphabetic => text.y,
			Baseline::Central => text.y + (outline.ascent.round() - outline.descent.round()) / 2.0,
			Baseline::Middle => text.y + outline.x_height / 2.0,
			Baseline::Hanging => text.y + (outline.ascent.round() * 0.8 * 64.0).round() / 64.0,
			Baseline::TextBeforeEdge => text.y + outline.ascent.round(),
		};
		let translate = format!("translate({},{})", number(x), number(y));
		// A transform of the text applies around the outline's own translation.
		match attributes.contains("transform=") {
			true => {
				let _ = write!(
					output,
					"<g{attributes}><path transform=\"{translate}\" d=\"{}\"/></g>",
					outline.path
				);
			}
			false => {
				let _ = write!(
					output,
					"<path{attributes} transform=\"{translate}\" d=\"{}\"/>",
					outline.path
				);
			}
		}
	}
	output.push_str(svg.get(written..).unwrap_or(""));
	output
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn formats_numbers_compactly() {
		assert_eq!(number(1.0), "1");
		assert_eq!(number(-0.0001), "0");
		assert_eq!(number(2.5), "2.5");
		assert_eq!(number(1.23456), "1.235");
		assert_eq!(number(f64::NAN), "0");
		assert_eq!(number(-12.3), "-12.3");
	}

	#[test]
	fn records_and_outlines_text() {
		use super::super::text::testing::FixedMeasure;
		let font = Font::new("sans-serif", 10.0);
		let mut svg = Svg::new();
		svg.open("g").end();
		let run = |content| TextRun {
			x: 10.0,
			y: 20.0,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &font,
			content,
		};
		svg.text(run("  a <b> "), |svg| _ = svg.attr("fill", "red"));
		svg.text(run("c"), |svg| _ = svg.attr("transform", "rotate(90)"));
		svg.close_tag("g");
		assert_eq!(svg.texts.len(), 2);
		assert_eq!(
			&svg.out[svg.texts[0].element.clone()],
			"<text x=\"10\" y=\"20\" text-anchor=\"middle\" font-family=\"sans-serif\" font-size=\"10\" fill=\"red\">a &lt;b&gt;</text>"
		);
		let mut wrapper = Svg::new();
		wrapper.out.push_str("<svg>");
		wrapper.append(svg);
		let diagram = Diagram { svg: wrapper.out, width: 1.0, height: 1.0, texts: wrapper.texts };
		let outlined = outline_text(&diagram, &mut FixedMeasure);
		// Width 30 for "a <b>", centered at x.
		assert_eq!(
			outlined,
			"<svg><g><path fill=\"red\" transform=\"translate(-5,20)\" d=\"M0,-9H30V0Z\"/><g transform=\"rotate(90)\"><path transform=\"translate(7,20)\" d=\"M0,-9H6V0Z\"/></g></g>"
		);
	}

	#[test]
	fn escapes_text() {
		let mut output = String::new();
		push_escaped(&mut output, "a<b>&\"c\u{1}\n");
		assert_eq!(output, "a&lt;b&gt;&amp;&quot;c ");
	}
}
