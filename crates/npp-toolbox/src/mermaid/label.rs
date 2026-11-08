//! Labels like Mermaid's HTML labels: a subset of HTML (or a Markdown string) in a `<div>` with
//! `line-height: 1.5`, `white-space: nowrap` and a maximum width at which the text wraps instead,
//! written as SVG `<text>` lines so that renderers without `<foreignObject>` show them too.

use std::rc::Rc;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::Context;
use super::icons::{self, IconGlyph};
use super::math::{self, MathBox};
use super::svg::{Anchor, Baseline, Svg, TextRun};
use super::text::Font;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
	pub bold: bool,
	pub italic: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
	pub text: String,
	pub style: Style,
}

/// The lines of a label as its markup breaks them, before wrapping. Whitespace is collapsed
/// like HTML does; runs of the same style are merged.
pub type Lines = Vec<Vec<Run>>;

/// Mermaid's `encodeEntities`, applied to the diagram code before parsing: `#name;` and `#123;`
/// become placeholders that survive the grammars, and `;` ends no `style`/`classDef` with a
/// color.
pub fn encode_entities(code: &str) -> String {
	let mut text = String::with_capacity(code.len());
	for line in code.split_inclusive('\n') {
		text.push_str(&drop_style_semicolon(line));
	}
	let mut output = String::with_capacity(text.len());
	let mut rest = text.as_str();
	while let Some(start) = rest.find('#') {
		output.push_str(&rest[..start]);
		let after = &rest[start + 1..];
		let name_length = after
			.char_indices()
			.find(|&(_, character)| !(character.is_ascii_alphanumeric() || character == '_'))
			.map_or(after.len(), |(index, _)| index);
		if name_length > 0 && after[name_length..].starts_with(';') {
			let name = &after[..name_length];
			let is_number = name.bytes().all(|byte| byte.is_ascii_digit());
			output.push_str(if is_number { "ﬂ°°" } else { "ﬂ°" });
			output.push_str(name);
			output.push_str("¶ß");
			rest = &after[name_length + 1..];
		} else {
			output.push('#');
			rest = after;
		}
	}
	output.push_str(rest);
	output
}

/// `txt.replace(/style.*:\S*#.*;/g, …)` and the same for `classDef`: the last `;` of the line
/// goes if a `:` before it is followed by a color like `#f9f`, so that it is not taken for an
/// entity.
fn drop_style_semicolon(line: &str) -> std::borrow::Cow<'_, str> {
	let mut line = std::borrow::Cow::Borrowed(line);
	for keyword in ["style", "classDef"] {
		let Some(start) = line.find(keyword) else { continue };
		let segment_start = start + keyword.len();
		let segment_end =
			line[segment_start..].find('\n').map_or(line.len(), |end| segment_start + end);
		let segment = &line[segment_start..segment_end];
		let Some(last_semicolon) = segment.rfind(';') else { continue };
		let has_color = segment.match_indices(':').any(|(colon, _)| {
			let token = &segment[colon + 1..];
			let token = &token[..token.find(char::is_whitespace).unwrap_or(token.len())];
			token.find('#').is_some_and(|hash| colon + 1 + hash < last_semicolon)
		});
		if has_color {
			let cut = segment_start + last_semicolon;
			let mut fixed = String::with_capacity(line.len());
			fixed.push_str(&line[..cut]);
			fixed.push_str(&line[cut + 1..]);
			line = std::borrow::Cow::Owned(fixed);
		}
	}
	line
}

/// Mermaid's `decodeEntities`: the placeholders of [`encode_entities`] back to HTML entities.
pub fn decode_entities(text: &str) -> String {
	text.replace("ﬂ°°", "&#").replace("ﬂ°", "&").replace("¶ß", ";")
}

/// The placeholders of [`encode_entities`] as the characters their entities stand for, for text
/// that is no HTML; unknown entities stay as written.
pub fn decode_entities_to_text(text: &str) -> String {
	let mut output = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find("ﬂ°") {
		output.push_str(&rest[..start]);
		let after = &rest[start + "ﬂ°".len()..];
		let (is_number, name_start) = match after.strip_prefix('°') {
			Some(number) => (true, number),
			None => (false, after),
		};
		let Some(end) = name_start.find("¶ß") else {
			output.push_str(&rest[start..]);
			return output;
		};
		let entity = format!("{}{};", if is_number { "#" } else { "" }, &name_start[..end]);
		match decode_entity(&entity) {
			Some((character, _)) => output.push(character),
			None => {
				output.push('&');
				output.push_str(&entity);
			}
		}
		rest = &name_start[end + "¶ß".len()..];
	}
	output.push_str(rest);
	output
}

/// `nonMarkdownToHTML`: a paragraph where newlines and `\n` sequences break lines.
pub fn non_markdown_to_html(text: &str) -> String {
	if text.is_empty() {
		return String::new();
	}
	format!("<p>{}</p>", text.replace("\\n", "<br />").replace('\n', "<br />"))
}

/// `markdownToHTML` with marked's subset Mermaid keeps: paragraphs, strong, emphasis, raw HTML;
/// line ends within a paragraph break the line.
pub fn markdown_to_html(text: &str) -> String {
	let mut html = String::with_capacity(text.len() + 16);
	for event in Parser::new(text) {
		match event {
			Event::Start(Tag::Paragraph) => html.push_str("<p>"),
			Event::End(TagEnd::Paragraph) => html.push_str("</p>"),
			Event::Start(Tag::Strong) => html.push_str("<strong>"),
			Event::End(TagEnd::Strong) => html.push_str("</strong>"),
			Event::Start(Tag::Emphasis) => html.push_str("<em>"),
			Event::End(TagEnd::Emphasis) => html.push_str("</em>"),
			Event::Text(text) | Event::Code(text) => push_html_escaped(&mut html, &text),
			Event::SoftBreak | Event::HardBreak => html.push_str("<br/>"),
			Event::Html(raw) | Event::InlineHtml(raw) => html.push_str(&raw),
			_ => {}
		}
	}
	html
}

fn push_html_escaped(output: &mut String, text: &str) {
	for character in text.chars() {
		match character {
			'&' => output.push_str("&amp;"),
			'<' => output.push_str("&lt;"),
			'>' => output.push_str("&gt;"),
			other => output.push(other),
		}
	}
}

/// `replaceIconSubstring`: Font Awesome references (`fa:fa-car`) become the characters of their
/// icons if `keeps_icons`. Others are the empty `<i>` of Mermaid, which shows nothing.
fn replace_icons(text: &str, keeps_icons: bool) -> String {
	let mut output = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find("fa") {
		let after = &rest[start + 2..];
		let has_prefix =
			after.starts_with(['b', 'k', 'l', 'r', 's']) && after[1..].starts_with(":fa-");
		let prefix_length = usize::from(has_prefix);
		if after[prefix_length..].starts_with(":fa-") {
			let name = &after[prefix_length + 4..];
			let name_length = name
				.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
				.unwrap_or(name.len());
			if name_length > 0 {
				output.push_str(&rest[..start]);
				let pack = &rest[start..start + 2 + prefix_length];
				if keeps_icons && let Some(icon) = icons::icon_character(pack, &name[..name_length])
				{
					output.push(icon);
				}
				rest = &name[name_length..];
				continue;
			}
		}
		output.push_str(&rest[..start + 2]);
		rest = after;
	}
	output.push_str(rest);
	output
}

/// The text of a label as Mermaid writes it into its HTML label, without icons.
pub fn label_lines(text: &str, is_markdown: bool) -> Lines {
	lines_of_label(text, is_markdown, false)
}

/// The label of a node, edge or cluster as Mermaid writes it into its HTML label. Characters of
/// the text stand for its icons.
pub fn label_lines_with_icons(text: &str, is_markdown: bool) -> Lines {
	lines_of_label(text, is_markdown, true)
}

fn lines_of_label(text: &str, is_markdown: bool, keeps_icons: bool) -> Lines {
	let decoded = decode_entities(text);
	let html = match is_markdown {
		true => markdown_to_html(&decoded),
		false => non_markdown_to_html(&decoded),
	};
	parse_html(&decode_entities(&replace_icons(&html, keeps_icons)))
}

/// A part of the text of a run: text, or the icon that a character stands for.
enum RunPart<'a> {
	Text(&'a str),
	Icon(IconGlyph),
}

/// The parts of the text of a run in their order.
fn run_parts(text: &str) -> Vec<RunPart<'_>> {
	let mut parts = Vec::new();
	let mut text_start = 0;
	for (index, character) in text.char_indices() {
		if let Some(icon) = icons::glyph_of_character(character) {
			if text_start < index {
				parts.push(RunPart::Text(&text[text_start..index]));
			}
			parts.push(RunPart::Icon(icon));
			text_start = index + character.len_utf8();
		}
	}
	if text_start < text.len() {
		parts.push(RunPart::Text(&text[text_start..]));
	}
	parts
}

fn has_icons(text: &str) -> bool {
	text.chars().any(|character| icons::glyph_of_character(character).is_some())
}

/// The width of the text of a run with its icons, which are no text to shape.
fn run_width(context: &mut Context, text: &str, font: &Font) -> f64 {
	if !has_icons(text) {
		return context.measure_exact(text, font);
	}
	run_parts(text)
		.into_iter()
		.map(|part| match part {
			RunPart::Text(text) => context.measure_exact(text, font),
			RunPart::Icon(icon) => icon_width(&icon, font),
		})
		.sum()
}

/// The width of the inline box of an icon: browsers cut lengths to units of 1/64 pixel.
fn icon_width(icon: &IconGlyph, font: &Font) -> f64 {
	(icon.width_in_em() * font.size * 64.0).floor() / 64.0
}

/// Named entities beyond the XML ones that labels commonly use.
const NAMED_ENTITIES: &[(&str, char)] = &[
	("amp", '&'),
	("lt", '<'),
	("gt", '>'),
	("quot", '"'),
	("apos", '\''),
	("nbsp", '\u{a0}'),
	("copy", '©'),
	("reg", '®'),
	("trade", '™'),
	("hellip", '…'),
	("mdash", '—'),
	("ndash", '–'),
	("laquo", '«'),
	("raquo", '»'),
	("lsquo", '‘'),
	("rsquo", '’'),
	("ldquo", '“'),
	("rdquo", '”'),
	("bull", '•'),
	("middot", '·'),
	("deg", '°'),
	("plusmn", '±'),
	("times", '×'),
	("divide", '÷'),
	("euro", '€'),
	("pound", '£'),
	("yen", '¥'),
	("cent", '¢'),
	("sect", '§'),
	("para", '¶'),
	("larr", '←'),
	("rarr", '→'),
	("uarr", '↑'),
	("darr", '↓'),
	("harr", '↔'),
	("lArr", '⇐'),
	("rArr", '⇒'),
	("hArr", '⇔'),
	("le", '≤'),
	("ge", '≥'),
	("ne", '≠'),
	("infin", '∞'),
	("hearts", '♥'),
	("check", '✓'),
	("micro", 'µ'),
	("frac12", '½'),
	("frac14", '¼'),
	("frac34", '¾'),
	("sup2", '²'),
	("sup3", '³'),
	("auml", 'ä'),
	("ouml", 'ö'),
	("uuml", 'ü'),
	("Auml", 'Ä'),
	("Ouml", 'Ö'),
	("Uuml", 'Ü'),
	("szlig", 'ß'),
];

/// The character of an entity at the start of `text` (after `&`) and its length with `;`.
fn decode_entity(text: &str) -> Option<(char, usize)> {
	let end = text.find(';').filter(|&end| end <= 32)?;
	let name = &text[..end];
	let character = if let Some(number) = name.strip_prefix('#') {
		let value = match number.strip_prefix(['x', 'X']) {
			Some(hex) => u32::from_str_radix(hex, 16).ok()?,
			None => number.parse().ok()?,
		};
		char::from_u32(value).filter(|&c| c != '\0').unwrap_or('\u{fffd}')
	} else {
		NAMED_ENTITIES.iter().find(|(known, _)| *known == name).map(|&(_, c)| c)?
	};
	Some((character, end + 1))
}

/// Builds lines of runs with HTML whitespace collapsing.
struct LineBuilder {
	lines: Lines,
	/// The style of the collapsible whitespace before the next text, which keeps the style of its
	/// first whitespace character.
	pending_space: Option<Style>,
}

impl LineBuilder {
	fn push_char(&mut self, character: char, style: Style) {
		if character.is_whitespace() && character != '\u{a0}' {
			self.pending_space.get_or_insert(style);
			return;
		}
		let Some(line) = self.lines.last_mut() else { return };
		// Whitespace at the start of a line collapses away.
		if let Some(space_style) = self.pending_space.take()
			&& !line.is_empty()
		{
			push_to_line(line, ' ', space_style);
		}
		push_to_line(line, character, style);
	}

	fn break_line(&mut self) {
		self.lines.push(Vec::new());
		self.pending_space = None;
	}

	/// A block boundary: a new line unless the current one is empty.
	fn block(&mut self) {
		if self.lines.last().is_some_and(|line| !line.is_empty()) {
			self.break_line();
		}
		self.pending_space = None;
	}
}

fn push_to_line(line: &mut Vec<Run>, character: char, style: Style) {
	match line.last_mut() {
		Some(run) if run.style == style => run.text.push(character),
		_ => line.push(Run { text: character.to_string(), style }),
	}
}

/// Parses the HTML of a label: text, entities, `<br>`, `<b>`/`<strong>`, `<i>`/`<em>` and blocks
/// (`<p>`, `<div>`); other tags are dropped with their markup, `<script>` and `<style>` with their
/// content, like the sanitizer.
pub fn parse_html(html: &str) -> Lines {
	let mut builder = LineBuilder { lines: vec![Vec::new()], pending_space: None };
	let mut bold = 0usize;
	let mut italic = 0usize;
	let mut rest = html;
	while let Some(character) = rest.chars().next() {
		let style = Style { bold: bold > 0, italic: italic > 0 };
		if character == '<'
			&& let Some(tag) = parse_tag(rest)
		{
			rest = &rest[tag.length..];
			match tag.name.as_str() {
				"br" => builder.break_line(),
				"b" | "strong" => match tag.is_closing {
					true => bold = bold.saturating_sub(1),
					false => bold += 1,
				},
				"i" | "em" => match tag.is_closing {
					true => italic = italic.saturating_sub(1),
					false => italic += 1,
				},
				"p" | "div" | "li" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
				| "blockquote" | "pre" | "table" | "tr" => builder.block(),
				"script" | "style" if !tag.is_closing => {
					let closing = format!("</{}", tag.name);
					let end = rest.to_ascii_lowercase().find(&closing).unwrap_or(rest.len());
					rest = &rest[end..];
				}
				_ => {}
			}
			continue;
		}
		if character == '&'
			&& let Some((decoded, length)) = decode_entity(&rest[1..])
		{
			builder.push_char(decoded, style);
			rest = &rest[1 + length..];
			continue;
		}
		builder.push_char(character, style);
		rest = &rest[character.len_utf8()..];
	}
	let mut lines = builder.lines;
	while lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
		lines.pop();
	}
	lines
}

struct HtmlTag {
	name: String,
	is_closing: bool,
	length: usize,
}

/// A tag at the start of `text`, which starts with `<`; `None` if `<` starts no tag.
fn parse_tag(text: &str) -> Option<HtmlTag> {
	let inner = &text[1..];
	let (is_closing, name_start) = match inner.strip_prefix('/') {
		Some(rest) => (true, rest),
		None => (false, inner),
	};
	if !name_start.starts_with(|c: char| c.is_ascii_alphabetic()) {
		// Comments and doctypes are markup too.
		if inner.starts_with('!') {
			let end = inner.find('>')?;
			return Some(HtmlTag { name: String::new(), is_closing: true, length: end + 2 });
		}
		return None;
	}
	let name_length = name_start
		.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
		.unwrap_or(name_start.len());
	let name = name_start[..name_length].to_ascii_lowercase();
	// The tag ends at the first `>` outside quotes.
	let mut quote = None;
	for (index, character) in inner.char_indices() {
		match quote {
			Some(open) if character == open => quote = None,
			Some(_) => {}
			None if character == '"' || character == '\'' => quote = Some(character),
			None if character == '>' => {
				return Some(HtmlTag { name, is_closing, length: index + 2 });
			}
			None => {}
		}
	}
	None
}

/// An item of the flex row of a label with math.
#[derive(Clone, Debug, PartialEq)]
pub enum FlexItem {
	Text(Run, f64),
	Formula(Rc<MathBox>),
}

/// A laid out line: runs with their widths, or the items of a label with math.
#[derive(Clone, Debug, PartialEq)]
pub struct LaidLine {
	pub runs: Vec<(Run, f64)>,
	pub width: f64,
	pub flex_items: Vec<FlexItem>,
	/// The height of a row of flex items.
	pub flex_height: f64,
}

/// A label laid out like its HTML `<div>`.
#[derive(Clone, Debug, PartialEq)]
pub struct LabelBox {
	pub lines: Vec<LaidLine>,
	/// Size of the `<div>`.
	pub width: f64,
	pub height: f64,
	pub line_height: f64,
	pub font: Font,
	/// `text-align: left` instead of centered lines.
	pub is_left_aligned: bool,
	/// `text-decoration: underline`.
	pub is_underlined: bool,
}

impl LabelBox {
	pub fn is_empty(&self) -> bool {
		self.lines.iter().all(|line| line.runs.is_empty() && line.flex_items.is_empty())
	}
}

/// The font of a run: `<strong>` and `<b>` are `bolder` than the label's font.
pub fn font_of(font: &Font, style: Style) -> Font {
	Font {
		weight: if style.bold { bolder(font.weight) } else { font.weight },
		is_italic: style.italic || font.is_italic,
		..font.clone()
	}
}

/// CSS `font-weight: bolder` of an inherited weight.
fn bolder(weight: u16) -> u16 {
	match weight {
		0..350 => 400,
		350..550 => 700,
		_ => weight.max(900),
	}
}

/// The extent of the line box of `line` above and below its baseline (CSS 2 §10.8): the inline
/// box of each font is `line_height` high around the font's rounded ascent and descent, with
/// the half-leading above floored like Blink. Fonts of other metrics, such as the Black face
/// of a bolder run, make the line taller.
fn line_box(context: &mut Context, font: &Font, line_height: f64, line: &LaidLine) -> (f64, f64) {
	let mut above = f64::NEG_INFINITY;
	let mut below = f64::NEG_INFINITY;
	let fonts = std::iter::once(font.clone())
		.chain(line.runs.iter().map(|(run, _)| font_of(font, run.style)));
	for font in fonts {
		let (ascent, descent) = context.vertical_metrics(&font);
		let (ascent, descent) = (ascent.round(), descent.round());
		let leading = ((line_height - ascent - descent) / 2.0).floor();
		above = above.max(ascent + leading);
		below = below.max(line_height - ascent - leading);
	}
	(above, below)
}

/// Whether `line` has runs in fonts other than `font`, whose line box may differ.
fn has_other_fonts(font: &Font, line: &LaidLine) -> bool {
	line.runs.iter().any(|(run, _)| font_of(font, run.style) != *font)
}

/// The height of the line box of `line` and how far its baseline is below the one of a line
/// in `font` alone.
fn line_metrics(
	context: &mut Context,
	font: &Font,
	line_height: f64,
	line: &LaidLine,
) -> (f64, f64) {
	if !has_other_fonts(font, line) {
		return (line_height, 0.0);
	}
	let (above, below) = line_box(context, font, line_height, line);
	let strut = LaidLine { runs: Vec::new(), width: 0.0, flex_items: Vec::new(), flex_height: 0.0 };
	let (strut_above, _) = line_box(context, font, line_height, &strut);
	(above + below, above - strut_above)
}

/// Browsers measure boxes in units of 1/64 pixel.
fn layout_unit(value: f64) -> f64 {
	(value * 64.0).ceil() / 64.0
}

fn measure_runs(context: &mut Context, font: &Font, runs: &[Run]) -> Vec<(Run, f64)> {
	runs.iter()
		.map(|run| {
			let width = run_width(context, &run.text, &font_of(font, run.style));
			(run.clone(), width)
		})
		.collect()
}

fn laid_line(context: &mut Context, font: &Font, runs: &[Run]) -> LaidLine {
	let runs = measure_runs(context, font, runs);
	let width = runs.iter().map(|(_, width)| width).sum();
	LaidLine { runs, width, flex_items: Vec::new(), flex_height: 0.0 }
}

/// Lays out a label with `$$…$$` like Mermaid's `renderKatexUnsanitized` in `addHtmlSpan`. The raw
/// text is the HTML, without Markdown and icons. Its line breaks are turned into line ends before
/// the split into lines looks for line breaks, so all of it is one centered flex row that does
/// not wrap.
pub fn layout_with_math(
	context: &mut Context,
	text: &str,
	font: &Font,
	min_width: f64,
) -> LabelBox {
	let text = decode_entities(text).replace("\\\\", "\\");
	let line_height = font.size * 1.5;
	let line = flex_line(context, &replace_line_breaks(&text), font, line_height);
	LabelBox {
		width: f64::max(line.width, min_width),
		height: line.flex_height,
		lines: vec![line],
		line_height,
		font: font.clone(),
		is_left_aligned: false,
		is_underlined: false,
	}
}

/// Lays out text with `$$…$$` where the split works, as the sequence diagram does: its lines are
/// blocks of `line-height: normal`, those with formulas flex rows. With `is_min_content`, lines
/// without formulas break at every space, as `drawKatex` measures its `fit-content` element in a
/// `foreignObject` without a width.
pub fn layout_math_lines(
	context: &mut Context,
	text: &str,
	font: &Font,
	is_min_content: bool,
) -> LabelBox {
	let line_height = context.normal_line_height(font);
	let text = decode_entities(text);
	let mut lines = Vec::new();
	let (mut width, mut height) = (0.0f64, 0.0);
	for line in super::common::split_breaks(&text) {
		if math::has_math(line) {
			let laid = flex_line(context, line, font, line_height);
			width = width.max(layout_unit(laid.width));
			height += laid.flex_height;
			lines.push(laid);
			continue;
		}
		let runs: Vec<Run> = parse_html(line).into_iter().flatten().collect();
		let parts = match is_min_content {
			true => words(&runs),
			false => vec![runs],
		};
		for runs in parts {
			let laid = laid_line(context, font, &runs);
			width = width.max(layout_unit(laid.width));
			height += line_height;
			lines.push(laid);
		}
	}
	LabelBox {
		lines,
		width,
		height,
		line_height,
		font: font.clone(),
		is_left_aligned: false,
		is_underlined: false,
	}
}

/// A flex row of the formulas and texts of `text`: items without spaces between them.
fn flex_line(context: &mut Context, text: &str, font: &Font, line_height: f64) -> LaidLine {
	let mut items = Vec::new();
	let (mut width, mut height) = (0.0, 0.0f64);
	for part in math::split(text) {
		match part {
			math::Part::Formula(latex) => {
				let formula = math::layout(context, latex, font.size);
				width += formula.width;
				height = height.max(formula.height());
				items.push(FlexItem::Formula(Rc::new(formula)));
			}
			math::Part::Text(html) => {
				for run in parse_html(html).into_iter().flatten() {
					let trimmed = run.text.trim();
					if trimmed.is_empty() {
						continue;
					}
					let run = Run { text: trimmed.to_owned(), style: run.style };
					let run_width =
						layout_unit(context.measure_exact(&run.text, &font_of(font, run.style)));
					width += run_width;
					height = height.max(line_height);
					items.push(FlexItem::Text(run, run_width));
				}
			}
		}
	}
	LaidLine { runs: Vec::new(), width, flex_items: items, flex_height: height }
}

/// `text.replace(lineBreakRegex, '\n')`.
fn replace_line_breaks(text: &str) -> String {
	let mut output = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find('<') {
		output.push_str(&rest[..start]);
		rest = &rest[start..];
		let after = rest[1..].strip_prefix(|c| c == 'b' || c == 'B');
		let after = after.and_then(|after| after.strip_prefix(|c| c == 'r' || c == 'R'));
		let end = after.and_then(|after| {
			let after = after.trim_start();
			let after = after.strip_prefix('/').unwrap_or(after);
			after.strip_prefix('>')
		});
		match end {
			Some(end) => {
				output.push('\n');
				rest = end;
			}
			None => {
				output.push('<');
				rest = &rest[1..];
			}
		}
	}
	output.push_str(rest);
	output
}

/// Writes the row of a label with math: its items are centered vertically in the row.
fn write_flex_row(
	svg: &mut Svg,
	context: &mut Context,
	label: &LabelBox,
	line: &LaidLine,
	(center_x, top): (f64, f64),
	attributes: &dyn Fn(&mut Svg),
) {
	let mut x = center_x - line.width / 2.0;
	for item in &line.flex_items {
		match item {
			FlexItem::Formula(formula) => {
				let formula_top = top + (line.flex_height - formula.height()) / 2.0;
				formula.draw(svg, context, (x, formula_top + formula.ascent), attributes);
				x += formula.width;
			}
			FlexItem::Text(run, width) => {
				let font = font_of(&label.font, run.style);
				let (ascent, descent) = context.vertical_metrics(&font);
				let (ascent, descent) = (ascent.round(), descent.round());
				let half_leading = (label.line_height - (ascent + descent)) / 2.0;
				let line_top = top + (line.flex_height - label.line_height) / 2.0;
				let text = TextRun {
					x,
					y: line_top + half_leading + ascent,
					anchor: Anchor::Start,
					baseline: Baseline::Alphabetic,
					font: &font,
					content: &run.text,
				};
				svg.text(text, |svg| attributes(svg));
				x += width;
			}
		}
	}
}

/// Lays out `lines` in `font` like Mermaid's label `<div>`: at least `min_width` wide. The div
/// is an unwrapped table cell limited by `max-width`, so text at least `wrap_width` wide measures
/// exactly `wrap_width`, and then wraps at spaces (`bbox.width === width` in `addHtmlSpan`).
pub fn layout(
	context: &mut Context,
	lines: &Lines,
	font: &Font,
	wrap_width: Option<f64>,
	min_width: f64,
) -> LabelBox {
	let line_height = font.size * 1.5;
	let is_empty = lines.iter().all(Vec::is_empty);
	if is_empty {
		return LabelBox {
			lines: Vec::new(),
			width: 0.0,
			height: 0.0,
			line_height,
			font: font.clone(),
			is_left_aligned: false,
			is_underlined: false,
		};
	}
	let natural: Vec<LaidLine> = lines.iter().map(|runs| laid_line(context, font, runs)).collect();
	let natural_width = natural.iter().map(|line| line.width).fold(0.0, f64::max);
	let (laid, width) = match wrap_width {
		Some(wrap_width) if layout_unit(natural_width) >= wrap_width => {
			// The table of the label is at least as wide as its widest word and the space
			// after it (`white-space: break-spaces`), and wraps the lines at its width.
			let mut widest_word: f64 = 0.0;
			let mut min_content: f64 = 0.0;
			for runs in lines {
				for line in wrap(context, font, runs, 0.0, false, &mut widest_word) {
					min_content = min_content.max(line.width);
				}
			}
			let width = wrap_width.max(layout_unit(min_content));
			let mut laid = Vec::new();
			for runs in lines {
				laid.extend(wrap(context, font, runs, width, false, &mut widest_word));
			}
			(laid, width)
		}
		_ => (natural, layout_unit(natural_width)),
	};
	let width = width.max(min_width);
	let height = laid.iter().map(|line| line_metrics(context, font, line_height, line).0).sum();
	LabelBox {
		lines: laid,
		width,
		height,
		line_height,
		font: font.clone(),
		is_left_aligned: false,
		is_underlined: false,
	}
}

/// Splits a line into words: runs up to the spaces between them.
fn words(runs: &[Run]) -> Vec<Vec<Run>> {
	let mut words: Vec<Vec<Run>> = vec![Vec::new()];
	for run in runs {
		for (index, part) in run.text.split(' ').enumerate() {
			if index > 0 {
				words.push(Vec::new());
			}
			if !part.is_empty()
				&& let Some(word) = words.last_mut()
			{
				word.push(Run { text: part.to_owned(), style: run.style });
			}
		}
	}
	words.retain(|word| !word.is_empty());
	words
}

/// Splits a word after its hyphens where browsers may break the line (UAX #14): not after a
/// hyphen that starts the word, nor before a digit or another hyphen.
fn pieces(word: Vec<Run>) -> Vec<Vec<Run>> {
	let text: String = word.iter().map(|run| run.text.as_str()).collect();
	let characters: Vec<char> = text.chars().collect();
	let mut breaks = Vec::new();
	let mut offset = 0;
	for (index, &character) in characters.iter().enumerate() {
		offset += character.len_utf8();
		let next = characters.get(index + 1);
		let is_at_icon = next.is_some()
			&& (icons::glyph_of_character(character).is_some()
				|| next.is_some_and(|next| icons::glyph_of_character(*next).is_some()));
		let is_after_hyphen = character == '-'
			&& index > 0
			&& next.is_some_and(|next| *next != '-' && !next.is_ascii_digit());
		if is_after_hyphen || is_at_icon {
			breaks.push(offset);
		}
	}
	if breaks.is_empty() {
		return vec![word];
	}
	let mut pieces = vec![Vec::new()];
	let mut position = 0;
	for run in word {
		let mut text = run.text.as_str();
		while !text.is_empty() {
			let end = position + text.len();
			let split = breaks.iter().find(|&&at| at > position && at < end).copied();
			let length = split.map_or(text.len(), |at| at - position);
			if let Some(piece) = pieces.last_mut() {
				piece.push(Run { text: text[..length].to_owned(), style: run.style });
			}
			if split.is_some() {
				pieces.push(Vec::new());
			}
			position += length;
			text = &text[length..];
		}
	}
	pieces
}

/// Wraps `runs` at `width` like `white-space: normal`: the space at the end of a line hangs and
/// is dropped. Returns the lines and the width of the widest piece that cannot be broken.
pub fn wrap_normal(
	context: &mut Context,
	font: &Font,
	runs: &[Run],
	width: f64,
) -> (Vec<LaidLine>, f64) {
	let mut widest_piece = 0.0;
	let lines = wrap(context, font, runs, width, true, &mut widest_piece);
	(lines, widest_piece)
}

/// Greedy wrapping like `white-space: break-spaces` at spaces, whose space after a word does
/// not hang, so it has to fit and stays at the end of the line, and after hyphens; with
/// `hanging_spaces` like `white-space: normal`. Pieces wider than the line overflow.
fn wrap(
	context: &mut Context,
	font: &Font,
	runs: &[Run],
	width: f64,
	hanging_spaces: bool,
	widest_word: &mut f64,
) -> Vec<LaidLine> {
	let mut lines = Vec::new();
	let mut current: Vec<Run> = Vec::new();
	// The pieces and whether a space comes before them.
	let mut all_pieces: Vec<(bool, Vec<Run>)> = Vec::new();
	for (index, word) in words(runs).into_iter().enumerate() {
		for (piece_index, piece) in pieces(word).into_iter().enumerate() {
			all_pieces.push((index > 0 && piece_index == 0, piece));
		}
	}
	let count = all_pieces.len();
	for index in 0..count {
		let (space_before, piece) = all_pieces[index].clone();
		let piece_width: f64 =
			measure_runs(context, font, &piece).iter().map(|(_, width)| width).sum();
		*widest_word = widest_word.max(piece_width);
		let mut candidate = current.clone();
		if !candidate.is_empty() && space_before {
			append_run(&mut candidate, Run { text: " ".to_owned(), style: piece[0].style });
		}
		for run in &piece {
			append_run(&mut candidate, run.clone());
		}
		let space = Run { text: " ".to_owned(), style: piece[piece.len() - 1].style };
		let ends_word = !hanging_spaces
			&& all_pieces.get(index + 1).is_some_and(|(space_before, _)| *space_before);
		let mut spaced = candidate.clone();
		if ends_word {
			append_run(&mut spaced, space.clone());
		}
		let candidate_width: f64 =
			measure_runs(context, font, &spaced).iter().map(|(_, width)| width).sum();
		if current.is_empty() || candidate_width <= width {
			current = candidate;
		} else {
			if space_before && !hanging_spaces {
				append_run(&mut current, Run { text: " ".to_owned(), style: piece[0].style });
			}
			lines.push(laid_line(context, font, &current));
			current = piece;
		}
	}
	lines.push(laid_line(context, font, &current));
	lines
}

fn append_run(runs: &mut Vec<Run>, run: Run) {
	match runs.last_mut() {
		Some(last) if last.style == run.style => last.text.push_str(&run.text),
		_ => runs.push(run),
	}
}

/// Writes the lines of `label` centered on `center_x` from `top`, each line placed in its line
/// box like a browser: half the leading above the font's (rounded) ascent.
pub fn write(
	svg: &mut Svg,
	context: &mut Context,
	label: &LabelBox,
	center_x: f64,
	top: f64,
	attributes: &dyn Fn(&mut Svg),
) {
	let (ascent, descent) = context.vertical_metrics(&label.font);
	let (ascent, descent) = (ascent.round(), descent.round());
	let half_leading = ((label.line_height - (ascent + descent)) / 2.0).floor();
	// `.label-icon` of the diagrams with icon styles; other icons stand on the baseline.
	let icon_drop = if context.has_icon_styles { label.font.size * 0.125 } else { 0.0 };
	let mut line_top = top;
	for line in &label.lines {
		if !line.flex_items.is_empty() {
			write_flex_row(svg, context, label, line, (center_x, line_top), attributes);
			line_top += line.flex_height;
			continue;
		}
		let (height, raise) = line_metrics(context, &label.font, label.line_height, line);
		let baseline = line_top + half_leading + ascent + raise;
		line_top += height;
		let left = match label.is_left_aligned {
			true => center_x - label.width / 2.0,
			false => center_x - line.width / 2.0,
		};
		if label.is_underlined {
			write_underline(svg, context, label, line, (left, baseline), attributes);
		}
		if let [(run, _)] = line.runs.as_slice()
			&& !run.text.ends_with(' ')
			&& !label.is_left_aligned
			&& !has_icons(&run.text)
		{
			let font = font_of(&label.font, run.style);
			let text = TextRun {
				x: center_x,
				y: baseline,
				anchor: Anchor::Middle,
				baseline: Baseline::Alphabetic,
				font: &font,
				content: &run.text,
			};
			svg.text(text, |svg| attributes(svg));
			continue;
		}
		let mut x = left;
		for (run, _) in &line.runs {
			let font = font_of(&label.font, run.style);
			for part in run_parts(&run.text) {
				match part {
					RunPart::Icon(icon) => {
						icon.write_inline(svg, (x, baseline + icon_drop), font.size, attributes);
						x += icon_width(&icon, &font);
					}
					RunPart::Text(content) => {
						// A leading space of a run is collapsed by `<text>`; place the text
						// after it.
						let leading = content.len() - content.trim_start().len();
						let offset =
							if leading > 0 { context.measure_exact(" ", &font) } else { 0.0 };
						let text = TextRun {
							x: x + offset,
							y: baseline,
							anchor: Anchor::Start,
							baseline: Baseline::Alphabetic,
							font: &font,
							content,
						};
						svg.text(text, |svg| attributes(svg));
						x += context.measure_exact(content, &font);
					}
				}
			}
		}
	}
}

/// The underline of `line` from its `left` end on its baseline: a rectangle, which renderers
/// without text draw too.
fn write_underline(
	svg: &mut Svg,
	context: &mut Context,
	label: &LabelBox,
	line: &LaidLine,
	(left, baseline): (f64, f64),
	attributes: &dyn Fn(&mut Svg),
) {
	// Spaces at the ends are not drawn.
	let text: String = line.runs.iter().map(|(run, _)| run.text.as_str()).collect();
	let leading = text.len() - text.trim_start().len();
	let start = match leading {
		0 => 0.0,
		_ => context.measure_exact(&text[..leading], &label.font),
	};
	let trailing = text.len() - text.trim_end().len();
	let end = match trailing {
		0 => line.width,
		_ => line.width - context.measure_exact(&text[text.len() - trailing..], &label.font),
	};
	if end <= start {
		return;
	}
	let (offset, thickness) = context.underline(&label.font);
	svg.open("rect")
		.number_attr("x", left + start)
		.number_attr("y", baseline + offset)
		.number_attr("width", end - start)
		.number_attr("height", thickness.round().max(1.0));
	attributes(svg);
	svg.close();
}

#[cfg(test)]
mod tests {
	use super::*;

	fn plain(lines: &Lines) -> Vec<String> {
		lines
			.iter()
			.map(|line| line.iter().map(|run| run.text.as_str()).collect::<String>())
			.collect()
	}

	#[cfg(feature = "font-awesome")]
	mod icons_in_labels {
		use super::super::super::text::testing::FixedMeasure;
		use super::*;

		fn icon_text(name: &str) -> char {
			icons::icon_character("fa", name).unwrap()
		}

		#[test]
		fn replaces_references_by_their_icons() {
			let car = icon_text("car");
			let lines = label_lines_with_icons("a fa:fa-car b", false);
			assert_eq!(plain(&lines), [format!("a {car} b")]);
			// Other text before the reference is kept, as the regular expression has no word start.
			assert_eq!(plain(&label_lines_with_icons("xfa:fa-car", false)), [format!("x{car}")]);
			// Unknown names and the Pro styles show nothing, `fad` is no reference.
			assert_eq!(plain(&label_lines_with_icons("a fa:fa-bogus b", false)), ["a b"]);
			assert_eq!(plain(&label_lines_with_icons("fal:fa-car fak:fa-car b", false)), ["b"]);
			assert_eq!(plain(&label_lines_with_icons("fad:fa-car", false)), ["fad:fa-car"]);
			assert_eq!(plain(&label_lines_with_icons("fa:fa- fa:", false)), ["fa:fa- fa:"]);
			// Without icons for those who want the words of a label.
			assert_eq!(plain(&label_lines("a fa:fa-car b", false)), ["a b"]);
		}

		#[test]
		fn measures_icons_as_inline_boxes() {
			let mut measure = FixedMeasure;
			let mut context = Context::for_tests(&mut measure);
			let font = Font::new("Arial", 14.0);
			let only_icon = label_lines_with_icons("fa:fa-user", false);
			let label = layout(&mut context, &only_icon, &font, None, 0.0);
			// 0.88 em in units of 1/64 pixel, and a whole line in spite of no text.
			assert_eq!(label.width, 12.3125);
			assert_eq!(label.height, 21.0);
			let text_width = context.measure_exact("x", &font);
			let with_text = label_lines_with_icons("fa:fa-car x fa:fa-car", false);
			let label = layout(&mut context, &with_text, &font, None, 0.0);
			let space = context.measure_exact(" ", &font);
			let expected = 14.0 * 2.0 + text_width + space * 2.0;
			assert!((label.width - expected).abs() < 1.0 / 32.0, "{} {expected}", label.width);
		}

		#[test]
		fn wraps_around_icons() {
			let mut measure = FixedMeasure;
			let mut context = Context::for_tests(&mut measure);
			let font = Font::new("Arial", 14.0);
			let long = "x".repeat(30);
			let source = format!("{long}fa:fa-car,{long}");
			let lines = label_lines_with_icons(&source, false);
			let label = layout(&mut context, &lines, &font, Some(120.0), 0.0);
			assert!(label.lines.len() >= 2, "{label:?}");
		}

		#[test]
		fn writes_icons_as_paths_next_to_the_text() {
			let mut measure = FixedMeasure;
			let mut context = Context::for_tests(&mut measure);
			let font = Font::new("Arial", 14.0);
			let lines = label_lines_with_icons("fa:fa-car x", false);
			let label = layout(&mut context, &lines, &font, None, 0.0);
			let mut svg = Svg::new();
			write(&mut svg, &mut context, &label, 0.0, 0.0, &|svg: &mut Svg| {
				svg.attr("fill", "red");
			});
			let out = &svg.out;
			assert_eq!(out.matches("class=\"label-icon\"").count(), 1, "{out}");
			assert_eq!(out.matches("fill=\"red\"").count(), 2, "{out}");
			assert!(out.contains(">x</text>"), "{out}");
			assert!(!out.contains('\u{100000}'), "{out}");
		}
	}

	#[test]
	fn encodes_and_decodes_entities() {
		assert_eq!(
			encode_entities("A[#quot;x#quot; #35;] #no"),
			"A[ﬂ°quot¶ßxﬂ°quot¶ß ﬂ°°35¶ß] #no"
		);
		assert_eq!(encode_entities("style A fill:#f9f;"), "style A fill:#f9f");
		assert_eq!(decode_entities("ﬂ°quot¶ß ﬂ°°35¶ß"), "&quot; &#35;");
	}

	#[test]
	fn parses_label_html() {
		let lines =
			parse_html("<p>a  <b>bold</b>\n text<br/>next &amp; &#35;<script>x</script></p>");
		assert_eq!(plain(&lines), ["a bold text", "next & #"]);
		assert_eq!(lines[0][0], Run { text: "a ".to_owned(), style: Style::default() });
		assert_eq!(
			lines[0][1],
			Run { text: "bold".to_owned(), style: Style { bold: true, italic: false } }
		);
		let lines = parse_html("a<b> bold</b>");
		assert_eq!(
			lines[0][1],
			Run { text: " bold".to_owned(), style: Style { bold: true, italic: false } }
		);
		assert_eq!(plain(&parse_html("a < b <x y='>'>c")), ["a < b c"]);
		assert_eq!(plain(&label_lines("one\\ntwo fa:fa-car", false)), ["one", "two"]);
		assert_eq!(plain(&label_lines("**big** and *it*\nnext", true)), ["big and it", "next"]);
	}

	#[test]
	fn lays_out_like_html_labels() {
		use super::super::text::testing::FixedMeasure;
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let font = Font::new("sans-serif", 10.0);
		// 6 pixels per character.
		let label = layout(&mut context, &label_lines("abc", false), &font, Some(120.0), 0.0);
		assert_eq!((label.width, label.height), (18.0, 15.0));
		let label = layout(&mut context, &label_lines("abc", false), &font, Some(120.0), 50.0);
		assert_eq!(label.width, 50.0);
		let text = "aaaa bbbb cccc dddd eeee";
		let label = layout(&mut context, &label_lines(text, false), &font, Some(60.0), 0.0);
		assert_eq!((label.width, label.lines.len()), (60.0, 3));
		// A text of exactly the width wraps, and fits on its line.
		let label = layout(&mut context, &label_lines(text, false), &font, Some(144.0), 0.0);
		assert_eq!((label.width, label.lines.len()), (144.0, 1));
		let label = layout(&mut context, &label_lines("", false), &font, Some(60.0), 50.0);
		assert_eq!((label.width, label.height), (0.0, 0.0));
	}

	#[test]
	fn lays_out_labels_with_math_as_one_flex_row() {
		use super::super::text::testing::FixedMeasure;
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let font = Font::new("sans-serif", 10.0);
		// A formula alone is as high as its ink, 7 + 2 pixels.
		let label = layout_with_math(&mut context, "$$x$$", &font, 0.0);
		assert_eq!((label.width, label.height), (6.0, 9.0));
		let label = layout_with_math(&mut context, "$$x$$", &font, 40.0);
		assert_eq!(label.width, 40.0);
		// Spaces around items and line breaks vanish, text has the line height.
		let label = layout_with_math(&mut context, "a <b>b</b> $$x$$<br/>c", &font, 0.0);
		assert_eq!((label.width, label.height, label.lines.len()), (24.0, 15.0, 1));
		assert_eq!(label.lines[0].flex_items.len(), 4);
		// Markdown stays text and two backslashes are one.
		let label = layout_with_math(&mut context, "**b** $$\\\\alpha$$", &font, 0.0);
		assert_eq!(label.lines[0].flex_items.len(), 2);
		assert_eq!(context.failure(), None);
		// An empty formula has no height.
		let label = layout_with_math(&mut context, "$$ $$", &font, 0.0);
		assert_eq!((label.width, label.height), (0.0, 0.0));
		assert!(!label.is_empty());
		assert_eq!(context.failure(), None);
		layout_with_math(&mut context, "$$\\foo$$", &font, 0.0);
		assert!(context.failure().is_some());
		assert_eq!(replace_line_breaks("a<br>b<BR />c<br/>d<brx>"), "a\nb\nc\nd<brx>");
	}

	#[test]
	fn lays_out_math_lines_for_sequence_diagrams() {
		use super::super::text::testing::FixedMeasure;
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let font = Font::new("sans-serif", 10.0);
		// Lines at the breaks: 9 pixels for the formula, the normal line of 12 for the text.
		let label = layout_math_lines(&mut context, "$$x$$<br/>abc", &font, false);
		assert_eq!(label.lines.len(), 2);
		assert_eq!((label.width, label.height), (18.0, 21.0));
		// Without room every word of a text line is a line; a row with a formula stays.
		let label = layout_math_lines(&mut context, "a $$x$$ b<br/>abc de", &font, true);
		assert_eq!(label.lines.len(), 3);
		assert_eq!(label.width, 18.0);
		let label = layout_math_lines(&mut context, "$$x$$<br/>abc", &font, false);
		let mut svg = Svg::new();
		write(&mut svg, &mut context, &label, 100.0, 0.0, &|_: &mut Svg| {});
		assert_eq!(svg.texts.len(), 2);
		assert!(svg.texts[1].y > svg.texts[0].y);
	}

	#[test]
	fn writes_labels_with_math() {
		use super::super::text::testing::FixedMeasure;
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let font = Font::new("sans-serif", 10.0);
		let label = layout_with_math(&mut context, "a $$x$$", &font, 0.0);
		let mut svg = Svg::new();
		write(&mut svg, &mut context, &label, 100.0, 50.0, &|svg: &mut Svg| {
			svg.attr("fill", "red");
		});
		assert_eq!(svg.texts.len(), 2);
		assert_eq!(svg.texts[0].content, "a");
		assert_eq!(svg.texts[0].x, 94.0);
		assert_eq!(svg.texts[1].x, 100.0);
		// Centered in the row of 15 pixels: 3 above the formula, whose ascent is 7.
		assert_eq!(svg.texts[1].y, 60.0);
	}
}
