//! `createText` without HTML labels (`createText.ts`, `createFormattedText`,
//! `splitText.ts`, `markdownToLines` of `handle-markdown-text.ts`): Markdown words in `<tspan>`
//! rows of 1.1 em, wrapped at a width.
//!
//! Every word after the first of a row is its own `<tspan>` with the space before it; Blink
//! shapes them one by one, so each is measured and written on its own.

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::Context;
use super::label::decode_entities;
use super::rendering::BBox;
use super::svg::{Anchor, Baseline, Svg, TextRun};
use super::text::Font;

/// The kind of a Markdown word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Normal,
	Strong,
	Emphasis,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
	pub content: String,
	pub kind: Kind,
}

/// A row of words.
pub type Line = Vec<Word>;

/// The line height in em.
const LINE_HEIGHT: f64 = 1.1;

/// `markdownToLines` of a label as `createText` prepares it: `<br>` tags and line ends break
/// lines, `**strong**` and `*emphasis*` mark words, raw HTML tags are words of their own and
/// other inline Markdown (code spans, links) is dropped like Mermaid's walk of marked's tokens.
pub fn markdown_lines(text: &str) -> Vec<Line> {
	let text = decode_entities(&replace_breaks(text));
	let mut preprocessed = text.replace("<br/>", "\n");
	while preprocessed.contains("\n\n") {
		preprocessed = preprocessed.replace("\n\n", "\n");
	}
	let preprocessed = dedent(&preprocessed);
	let mut lines: Vec<Line> = vec![Vec::new()];
	let mut kinds: Vec<Kind> = Vec::new();
	let mut depth = 0;
	let mut skipped = 0;
	// The text of consecutive text events, which marked keeps in one token.
	let mut pending = String::new();
	let flush = |lines: &mut Vec<Line>, pending: &mut String, kind: Kind| {
		for (index, text_line) in pending.split('\n').enumerate() {
			if index > 0 {
				lines.push(Vec::new());
			}
			if let Some(line) = lines.last_mut() {
				for word in text_line.split(' ').filter(|word| !word.is_empty()) {
					line.push(Word { content: word.to_owned(), kind });
				}
			}
		}
		pending.clear();
	};
	for (event, range) in Parser::new(&preprocessed).into_offset_iter() {
		let kind = kinds.last().copied().unwrap_or(Kind::Normal);
		if !matches!(event, Event::Text(_)) {
			flush(&mut lines, &mut pending, kind);
		}
		match event {
			Event::Start(Tag::Paragraph) if depth == 0 => depth += 1,
			Event::End(TagEnd::Paragraph) if depth == 1 => depth -= 1,
			Event::Start(_) if depth == 0 => {
				// Other blocks keep their source.
				if skipped == 0
					&& let Some(line) = lines.last_mut()
				{
					let raw = preprocessed[range].trim_end_matches('\n').to_owned();
					line.push(Word { content: raw, kind: Kind::Normal });
				}
				skipped += 1;
			}
			Event::End(_) if depth == 0 => skipped -= 1,
			_ if depth == 0 => {}
			Event::Start(Tag::Strong) => kinds.push(Kind::Strong),
			Event::Start(Tag::Emphasis) => kinds.push(Kind::Emphasis),
			Event::End(TagEnd::Strong | TagEnd::Emphasis) => _ = kinds.pop(),
			Event::Text(text) => {
				// Marked keeps entities as written, of which `decodeHTMLEntities` decodes only
				// `&amp;`, `&lt;` and `&gt;`, and its escape tokens are dropped.
				let raw = &preprocessed[range];
				match () {
					_ if raw.starts_with('&') => pending.push_str(&decode_basic_entities(raw)),
					_ if raw.starts_with('\\') && raw.len() > text.len() => {}
					_ => pending.push_str(&text),
				}
			}
			Event::SoftBreak | Event::HardBreak => lines.push(Vec::new()),
			Event::InlineHtml(html) | Event::Html(html) => {
				if let Some(line) = lines.last_mut() {
					line.push(Word { content: html.to_string(), kind: Kind::Normal });
				}
			}
			_ => {}
		}
	}
	flush(&mut lines, &mut pending, Kind::Normal);
	lines
}

/// `&amp;`, `&lt;`, `&gt;` and `&#39;` decoded, other entities as written.
fn decode_basic_entities(text: &str) -> String {
	text.replace("&lt;", "<").replace("&gt;", ">").replace("&#39;", "'").replace("&amp;", "&")
}

/// `text.replace(/<br\s*\/?>/g, '<br/>')`.
fn replace_breaks(text: &str) -> String {
	let mut output = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find("<br") {
		output.push_str(&rest[..start]);
		let after = &rest[start + 3..];
		let spaces = after.len() - after.trim_start().len();
		let after_spaces = &after[spaces..];
		let length = match () {
			_ if after_spaces.starts_with("/>") => Some(spaces + 2),
			_ if after_spaces.starts_with('>') => Some(spaces + 1),
			_ => None,
		};
		match length {
			Some(length) => {
				output.push_str("<br/>");
				rest = &after[length..];
			}
			None => {
				output.push_str("<br");
				rest = after;
			}
		}
	}
	output.push_str(rest);
	output
}

/// `dedent`: removes the indentation that all non-empty lines share.
fn dedent(text: &str) -> String {
	let indentation = text
		.lines()
		.filter(|line| !line.trim().is_empty())
		.map(|line| line.len() - line.trim_start_matches([' ', '\t']).len())
		.min()
		.unwrap_or(0);
	text.lines()
		.map(|line| line.get(indentation..).unwrap_or(line.trim_start_matches([' ', '\t'])))
		.collect::<Vec<_>>()
		.join("\n")
		.trim()
		.to_owned()
}

/// The font of a word in the label's font.
pub fn word_font(font: &Font, kind: Kind) -> Font {
	Font {
		weight: if kind == Kind::Strong { 700 } else { 400 },
		is_italic: kind == Kind::Emphasis,
		..font.clone()
	}
}

/// The runs of a row that Blink shapes as one: the words of the same kind, every word after
/// the first of the row with the space before it. The words' `<tspan>`s are in one text
/// chunk, so kerning goes across them where the font stays the same.
fn segments(line: &[Word]) -> Vec<(String, Kind)> {
	let mut runs: Vec<(String, Kind)> = Vec::new();
	for (index, word) in line.iter().enumerate() {
		let separator = if index == 0 { "" } else { " " };
		match runs.last_mut() {
			Some((text, kind)) if *kind == word.kind => {
				text.push_str(separator);
				text.push_str(&word.content);
			}
			_ => runs.push((format!("{separator}{}", word.content), word.kind)),
		}
	}
	runs
}

/// `getComputedTextLength` of a row.
pub fn line_width(context: &mut Context, line: &[Word], font: &Font) -> f64 {
	segments(line)
		.iter()
		.map(|(text, kind)| context.measure_exact(text, &word_font(font, *kind)))
		.sum()
}

/// `createFormattedText`'s rows: each line, split where it is wider than `width`
/// (`splitLineToFitWidth`, which splits words wider than a row by characters).
pub fn wrap(context: &mut Context, lines: &[Line], width: f64, font: &Font) -> Vec<Line> {
	let mut rows = Vec::new();
	for line in lines {
		let fits = |context: &mut Context, words: &[Word]| {
			// The joiner spaces of `splitLineToFitWidth` are words of their own there, which
			// measures the same.
			line_width(context, words, font) <= width
		};
		if fits(context, line) {
			rows.push(line.clone());
			continue;
		}
		let mut words: std::collections::VecDeque<Word> = line.iter().cloned().collect();
		let mut current: Vec<Word> = Vec::new();
		while let Some(word) = words.pop_front() {
			current.push(word);
			if fits(context, &current) {
				continue;
			}
			let Some(word) = current.pop() else { break };
			if !current.is_empty() {
				rows.push(std::mem::take(&mut current));
				words.push_front(word);
				continue;
			}
			if word.content.is_empty() {
				continue;
			}
			// `splitWordToFitWidth`: as many characters as fit, at least one.
			let characters: Vec<char> = word.content.chars().collect();
			let mut used = 0;
			while used < characters.len() {
				let candidate =
					Word { content: characters[..=used].iter().collect(), kind: word.kind };
				if !fits(context, std::slice::from_ref(&candidate)) {
					break;
				}
				used += 1;
			}
			let used = used.max(1).min(characters.len());
			rows.push(vec![Word { content: characters[..used].iter().collect(), kind: word.kind }]);
			let rest: String = characters[used..].iter().collect();
			if !rest.is_empty() {
				words.push_front(Word { content: rest, kind: word.kind });
			}
		}
		if !current.is_empty() {
			rows.push(current);
		}
	}
	rows
}

/// The baseline of row `index`: the `<tspan>`'s `y` of `index * 1.1 - 0.1` em and its `dy`.
pub fn row_baseline(index: usize, font: &Font) -> f64 {
	(index as f64 * LINE_HEIGHT - 0.1) * font.size + LINE_HEIGHT * font.size
}

/// The x of the start of a row.
fn row_start(context: &mut Context, row: &[Word], font: &Font, anchor: Anchor) -> f64 {
	let width = line_width(context, row, font);
	match anchor {
		Anchor::Start => 0.0,
		Anchor::Middle => -width / 2.0,
		Anchor::End => -width,
	}
}

/// Writes the rows like `createFormattedText`, relative to the label group's origin. Each word
/// is a `<text>` of its own; `attributes` adds the paint to each.
pub fn write(
	svg: &mut Svg,
	context: &mut Context,
	rows: &[Line],
	font: &Font,
	(anchor, baseline): (Anchor, Baseline),
	attributes: &dyn Fn(&mut Svg),
) {
	for (index, row) in rows.iter().enumerate() {
		let y = row_baseline(index, font);
		let mut x = row_start(context, row, font, anchor);
		for (text, kind) in segments(row) {
			let word_font = word_font(font, kind);
			let width = context.measure_exact(&text, &word_font);
			let trimmed = text.trim_start();
			let leading = width - context.measure_exact(trimmed, &word_font);
			let run = TextRun {
				x: x + leading,
				y,
				anchor: Anchor::Start,
				baseline,
				font: &word_font,
				content: trimmed,
			};
			svg.text(run, |svg| attributes(svg));
			x += width;
		}
	}
}

/// The bounding box of the rows relative to the label group's origin, like `getBBox` of the
/// text. The empty background rectangle of the group is not rendered and counts nowhere.
pub fn bounding_box(
	context: &mut Context,
	rows: &[Line],
	font: &Font,
	(anchor, baseline): (Anchor, Baseline),
) -> BBox {
	let mut bbox = BBox::default();
	let (ascent, descent) = context.vertical_metrics(font);
	let baseline_offset = match baseline {
		Baseline::Alphabetic => 0.0,
		Baseline::Middle => context.x_height(font) / 2.0,
		Baseline::Central => (ascent.round() - descent.round()) / 2.0,
		Baseline::Hanging => ascent.round() * 0.8,
		Baseline::TextBeforeEdge => ascent.round(),
	};
	for (index, row) in rows.iter().enumerate() {
		let y = row_baseline(index, font) + baseline_offset;
		let mut x = row_start(context, row, font, anchor);
		for (text, kind) in segments(row) {
			let word_font = word_font(font, kind);
			let width = context.measure_exact(&text, &word_font);
			let trimmed = text.trim_start();
			let start = x + width - context.measure_exact(trimmed, &word_font);
			let (ink_width, ink_height) = context.bounding_box(trimmed, &word_font);
			let left = context.bounding_box_left(trimmed, &word_font);
			let top = context.bounding_box_top(trimmed, &word_font);
			bbox.include_rect(start + left, y + top, ink_width, ink_height);
			x += width;
		}
	}
	bbox
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn words(lines: &[Line]) -> Vec<Vec<(&str, Kind)>> {
		lines
			.iter()
			.map(|line| line.iter().map(|word| (word.content.as_str(), word.kind)).collect())
			.collect()
	}

	#[test]
	fn splits_markdown_into_words() {
		let lines = markdown_lines("**Bold** and *it* `code` x<br>two  words");
		assert_eq!(
			words(&lines),
			[
				vec![
					("Bold", Kind::Strong),
					("and", Kind::Normal),
					("it", Kind::Emphasis),
					("x", Kind::Normal)
				],
				vec![("two", Kind::Normal), ("words", Kind::Normal)],
			]
		);
		assert_eq!(words(&markdown_lines("# Heading")), [vec![("# Heading", Kind::Normal)]]);
	}

	#[test]
	fn wraps_like_split_line_to_fit_width() {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let font = Font::new("sans-serif", 10.0);
		// 6 px per character.
		let rows = wrap(&mut context, &markdown_lines("aa bb cc dddddddddd"), 36.0, &font);
		let rows: Vec<String> = rows
			.iter()
			.map(|row| row.iter().map(|word| word.content.as_str()).collect::<Vec<_>>().join(" "))
			.collect();
		assert_eq!(rows, ["aa bb", "cc", "dddddd", "dddd"]);
		assert_eq!(row_baseline(0, &font), 10.0);
		assert!((row_baseline(1, &font) - 21.0).abs() < 1e-9);
	}
}
