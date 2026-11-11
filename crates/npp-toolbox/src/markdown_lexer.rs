//! The platform independent part of the Markdown lexer for the editor: the styles of Markdown
//! text from the events of `pulldown-cmark`, the code blocks to style with the lexer of their
//! language, and the Notepad++ styles file of the language.

use std::borrow::Cow;
use std::ffi::CStr;
use std::ops::Range;

use pulldown_cmark::{BrokenLink, CodeBlockKind, CowStr, Event, LinkType, Parser, Tag, TagEnd};

use crate::markdown_html::parser_options;

mod incremental;

pub use incremental::{LexerMemory, MarkdownDocument, lex_document};

/// The language name in Notepad++'s Language menu and Style Configurator.
pub const LANGUAGE_NAME: &CStr = c"Markdown (NppToolBox)";

/// The style numbers of the Markdown text. Code blocks use further styles from
/// [`FIRST_CODE_STYLE`] on.
pub mod style {
	pub const DEFAULT: u8 = 0;
	/// Heading level `n` is `HEADING_1 + n - 1`.
	pub const HEADING_1: u8 = 1;
	pub const EMPHASIS: u8 = 7;
	pub const STRONG: u8 = 8;
	pub const STRIKETHROUGH: u8 = 9;
	pub const INLINE_CODE: u8 = 10;
	pub const CODE_BLOCK: u8 = 11;
	pub const CODE_FENCE: u8 = 12;
	pub const BLOCKQUOTE: u8 = 13;
	pub const LIST_MARKER: u8 = 14;
	pub const TASK_MARKER: u8 = 15;
	pub const LINK: u8 = 16;
	pub const IMAGE: u8 = 17;
	pub const URL: u8 = 18;
	pub const HTML: u8 = 19;
	pub const HORIZONTAL_RULE: u8 = 20;
	pub const TABLE: u8 = 21;
	pub const TABLE_HEADER: u8 = 22;
	pub const FOOTNOTE: u8 = 23;
	pub const MATH: u8 = 24;
	pub const FRONT_MATTER: u8 = 25;
}

/// The first style for code blocks. The styles 32 to 39 are Scintilla's predefined ones.
pub const FIRST_CODE_STYLE: u8 = 40;

/// Notepad++'s `fontStyle` bits.
const BOLD: u8 = 1;
const ITALIC: u8 = 2;
const UNDERLINE: u8 = 4;

pub struct MarkdownStyle {
	pub name: &'static CStr,
	/// Lexilla's style tags (`SCI_TAGSOFSTYLE`).
	pub tags: &'static CStr,
	pub description: &'static CStr,
	/// The default text color (`0xRRGGBB`); `None` takes the color of the theme's default
	/// style. The colors are mid-tones that read on light and on dark themes.
	pub color: Option<u32>,
	/// Notepad++'s `fontStyle`: 1 bold, 2 italic, 4 underlined.
	pub font_style: u8,
}

const fn markdown_style(
	name: &'static CStr,
	tags: &'static CStr,
	description: &'static CStr,
	color: Option<u32>,
	font_style: u8,
) -> MarkdownStyle {
	MarkdownStyle { name, tags, description, color, font_style }
}

const HEADING_COLOR: Option<u32> = Some(0xA05DC8);
const LINK_COLOR: Option<u32> = Some(0x2E7DD1);
const CODE_COLOR: Option<u32> = Some(0x2A9D8F);
const MARKER_COLOR: Option<u32> = Some(0xD0503C);
const MUTED_COLOR: Option<u32> = Some(0x808080);

/// The styles by number, see [`style`].
pub const STYLES: [MarkdownStyle; 26] = [
	markdown_style(c"DEFAULT", c"default", c"Text", None, 0),
	markdown_style(c"HEADING 1", c"heading", c"Heading of level 1", HEADING_COLOR, BOLD),
	markdown_style(c"HEADING 2", c"heading", c"Heading of level 2", HEADING_COLOR, BOLD),
	markdown_style(c"HEADING 3", c"heading", c"Heading of level 3", HEADING_COLOR, BOLD),
	markdown_style(c"HEADING 4", c"heading", c"Heading of level 4", HEADING_COLOR, BOLD),
	markdown_style(c"HEADING 5", c"heading", c"Heading of level 5", HEADING_COLOR, BOLD),
	markdown_style(c"HEADING 6", c"heading", c"Heading of level 6", HEADING_COLOR, BOLD),
	markdown_style(c"EMPHASIS", c"emphasis", c"*Emphasis*", None, ITALIC),
	markdown_style(c"STRONG", c"strong", c"**Strong emphasis**", None, BOLD),
	markdown_style(c"STRIKETHROUGH", c"deleted", c"~~Strikethrough~~", MUTED_COLOR, 0),
	markdown_style(c"INLINE CODE", c"literal code", c"`Code span`", CODE_COLOR, 0),
	markdown_style(
		c"CODE BLOCK",
		c"literal code",
		c"Code block without a known language. Code blocks with one take the colors of the language, with the font and background of this style.",
		CODE_COLOR,
		0,
	),
	markdown_style(c"CODE FENCE", c"code fence", c"``` lines of a code block", MUTED_COLOR, 0),
	markdown_style(c"BLOCKQUOTE", c"quote", c"> Quote", MUTED_COLOR, ITALIC),
	markdown_style(c"LIST MARKER", c"list marker", c"- or 1. of a list item", MARKER_COLOR, BOLD),
	markdown_style(c"TASK MARKER", c"list marker", c"[ ] or [x] of a task", MARKER_COLOR, BOLD),
	markdown_style(c"LINK", c"link", c"[Link text]", LINK_COLOR, 0),
	markdown_style(c"IMAGE", c"link image", c"![Image description]", LINK_COLOR, 0),
	markdown_style(
		c"URL",
		c"link url",
		c"Destination of a link, image, <autolink> or reference definition. A double click opens it.",
		MUTED_COLOR,
		UNDERLINE,
	),
	markdown_style(c"HTML", c"html", c"Raw HTML", Some(0xB07000), 0),
	markdown_style(c"HORIZONTAL RULE", c"rule", c"--- or ***", MUTED_COLOR, BOLD),
	markdown_style(c"TABLE", c"table", c"| and delimiter row of a table", MUTED_COLOR, 0),
	markdown_style(c"TABLE HEADER", c"table heading", c"Header row of a table", None, BOLD),
	markdown_style(c"FOOTNOTE", c"footnote", c"[^1] and its definition", LINK_COLOR, 0),
	markdown_style(c"MATH", c"math", c"$Math$ and $$math$$", Some(0x3A9A3A), 0),
	markdown_style(c"FRONT MATTER", c"metadata", c"YAML front matter", MUTED_COLOR, 0),
];

/// The styles of a part of a document.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LexedMarkdown {
	/// The first styled byte. It may lie before the requested range, where a Markdown block
	/// starts whose styles depend on later text (e.g. a heading underlined with `===`).
	pub start: usize,
	/// The style of each byte from `start`. It may end after the requested range, at the end of
	/// the Markdown block there.
	pub styles: Vec<u8>,
	/// The code blocks in the styled range, styled as [`style::CODE_BLOCK`].
	pub code_blocks: Vec<CodeBlock>,
	/// The definitions of link references or footnotes changed, which changes the styles of
	/// references anywhere: the whole document needs to be styled again.
	pub changes_whole_document: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
	/// The first word of the info string, e.g. `rust`; empty for indented code.
	pub language: String,
	/// From the first to the last byte of code.
	pub content: Range<usize>,
	/// Parts of `content` that are no code but the markers of the enclosing blocks, such as
	/// `> ` in a quote, in [`Self::gap_style`].
	pub gaps: Vec<Range<usize>>,
	pub gap_style: u8,
}

/// The document as text for the Markdown parser, byte for byte, so that offsets stay the same.
/// Invalid UTF-8 and, in other encodings, each byte of non-ASCII characters becomes `x`.
pub fn parseable_text(
	bytes: &[u8],
	is_utf8: bool,
	is_lead_byte: impl Fn(u8) -> bool,
) -> Cow<'_, str> {
	const PLACEHOLDER: char = 'x';
	if is_utf8 {
		if let Ok(text) = std::str::from_utf8(bytes) {
			return Cow::Borrowed(text);
		}
		let mut text = String::with_capacity(bytes.len());
		for chunk in bytes.utf8_chunks() {
			text.push_str(chunk.valid());
			text.extend(std::iter::repeat_n(PLACEHOLDER, chunk.invalid().len()));
		}
		return Cow::Owned(text);
	}
	if bytes.is_ascii() {
		// ASCII is valid UTF-8.
		return Cow::Borrowed(std::str::from_utf8(bytes).unwrap_or_default());
	}
	let mut text = String::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		let byte = bytes[index];
		// The trail byte of a double byte character may be ASCII, such as `\` or `[`.
		let length = if is_lead_byte(byte) && index + 1 < bytes.len() { 2 } else { 1 };
		if byte.is_ascii() && length == 1 {
			text.push(char::from(byte));
		} else {
			text.extend(std::iter::repeat_n(PLACEHOLDER, length));
		}
		index += length;
	}
	Cow::Owned(text)
}

/// The styles of a part of a document, parsed on its own, see [`lex_slice`].
pub(crate) struct LexedSlice {
	/// The style of each byte of the slice.
	pub styles: Vec<u8>,
	/// Relative to the slice.
	pub code_blocks: Vec<CodeBlock>,
	/// A code or HTML block of the slice continues after it, such as an unclosed code fence: the
	/// text after the slice belongs to it.
	pub runs_past_end: bool,
	/// Code, HTML and metadata blocks, relative to the slice. Their lines depend on the line
	/// that opened them.
	pub verbatim_blocks: Vec<Range<usize>>,
	/// The start of the first `---` line that opens a metadata block if a closing `---` or `...`
	/// line follows anywhere later, as `pulldown-cmark` looks for one up to the end of the text.
	pub metadata_opener: Option<usize>,
}

/// Styles the slice `prefix_length..prefix_length + slice_length` of `text`. The prefix holds
/// definitions the slice refers to; after the slice may follow a sentinel line that shows
/// whether a block of the slice continues (see [`LexedSlice::runs_past_end`]).
/// `is_defined_link` tells whether a link reference without a definition in `text` has one in
/// the document.
pub(crate) fn lex_slice(
	text: &str,
	prefix_length: usize,
	slice_length: usize,
	is_defined_link: &mut dyn FnMut(&str) -> bool,
) -> LexedSlice {
	let slice = prefix_length..prefix_length + slice_length;
	let mut styler = Styler::new(text);
	let mut runs_past_end = false;
	let mut metadata_opener = None;
	let resolve = |link: BrokenLink| {
		is_defined_link(&link.reference).then_some((CowStr::Borrowed(""), CowStr::Borrowed("")))
	};
	let mut events = Parser::new_with_broken_link_callback(text, parser_options(), Some(resolve))
		.into_offset_iter();
	let mut depth = 0usize;
	let mut is_in_slice_block = false;
	for (event, range) in events.by_ref() {
		if depth == 0 {
			if range.start >= slice.end {
				break;
			}
			is_in_slice_block = range.start >= slice.start;
			if is_in_slice_block {
				let is_verbatim = matches!(
					event,
					Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::MetadataBlock(_))
				);
				runs_past_end |= is_verbatim && range.end > slice.end;
			}
		}
		// Also inside containers such as footnote definitions.
		if is_in_slice_block
			&& matches!(event, Event::Rule)
			&& metadata_opener.is_none()
			&& opens_metadata_block(text.as_bytes(), range.start)
		{
			metadata_opener = Some(range.start - slice.start);
		}
		match event {
			Event::Start(_) => depth += 1,
			Event::End(_) => depth = depth.saturating_sub(1),
			_ => {}
		}
		if is_in_slice_block {
			styler.handle(event, range);
		}
	}
	drop(events);
	for definition in reference_definitions(text.as_bytes(), slice.clone(), &styler.covered) {
		styler.paint(definition.clone(), style::LINK);
		if let Some(destination) = definition_destination(text.as_bytes(), definition) {
			styler.paint(destination, style::URL);
		}
	}
	let shift = |range: &Range<usize>| {
		range.start.clamp(slice.start, slice.end) - slice.start
			..range.end.clamp(slice.start, slice.end) - slice.start
	};
	let code_blocks = styler
		.code_blocks
		.iter()
		.map(|code_block| CodeBlock {
			language: code_block.language.clone(),
			content: shift(&code_block.content),
			gaps: code_block.gaps.iter().map(shift).filter(|gap| !gap.is_empty()).collect(),
			gap_style: code_block.gap_style,
		})
		.filter(|code_block| !code_block.content.is_empty())
		.collect();
	LexedSlice {
		styles: styler.rasterize(slice.clone()),
		code_blocks,
		runs_past_end,
		verbatim_blocks: styler.verbatim_blocks.iter().map(shift).collect(),
		metadata_opener,
	}
}

/// The link reference definitions in `slice`: `pulldown-cmark` reports no events for them, and
/// reports only the first definition of a label, so they are found as lines outside of all
/// `covered` ranges that start with `[` after the markers of quotes and list items. Only the
/// first line of a definition is found.
fn reference_definitions(
	text: &[u8],
	slice: Range<usize>,
	covered: &[Range<usize>],
) -> Vec<Range<usize>> {
	let mut definitions = Vec::new();
	let mut covered = RangeSweep::new(covered.to_vec());
	let mut position = slice.start;
	while position < slice.end {
		let line = line_at(text, position);
		let line_range = position..position + line.len();
		position = line_range.end;
		if line.is_empty() {
			break;
		}
		if covered.overlaps(line_range.clone()) {
			continue;
		}
		let content = trim_line_end(line);
		let index = content_start(content);
		if content.get(index) == Some(&b'[') {
			definitions.push(line_range.start + index..line_range.start + content.len());
		}
	}
	definitions
}

/// The destination in a link reference definition `[label]: destination "title"` starting at
/// `definition.start`, if the label ends on the first line.
fn definition_destination(text: &[u8], definition: Range<usize>) -> Option<Range<usize>> {
	let mut position = definition.start + 1;
	while position < definition.end {
		match text[position] {
			b'\\' => position += 2,
			b']' => break,
			_ => position += 1,
		}
	}
	let colon = position + 1;
	if colon >= definition.end || text[colon] != b':' {
		return None;
	}
	link_destination(text, colon + 1..definition.end)
}

/// The destination at the start of `range` after optional white space, as CommonMark reads it:
/// the text between `<` and `>`, or up to white space or an unbalanced `)`. After a line break,
/// the markers of an enclosing quote are skipped as well.
fn link_destination(text: &[u8], range: Range<usize>) -> Option<Range<usize>> {
	let end = range.end.min(text.len());
	let mut position = range.start;
	let mut is_after_line_break = false;
	while position < end {
		match text[position] {
			b' ' | b'\t' => {}
			b'\r' | b'\n' => is_after_line_break = true,
			b'>' if is_after_line_break => {}
			_ => break,
		}
		position += 1;
	}
	let start = position;
	if text.get(start) == Some(&b'<') {
		let mut position = start + 1;
		while position < end {
			match text[position] {
				b'\\' => position += 2,
				b'>' => return Some(start + 1..position),
				b'<' | b'\r' | b'\n' => return None,
				_ => position += 1,
			}
		}
		return None;
	}
	let mut open_parentheses = 0usize;
	while position < end {
		match text[position] {
			b'\\' if text.get(position + 1).is_some_and(u8::is_ascii_punctuation) => position += 1,
			b'(' => open_parentheses += 1,
			b')' if open_parentheses == 0 => break,
			b')' => open_parentheses -= 1,
			byte if byte.is_ascii_whitespace() || byte.is_ascii_control() => break,
			_ => {}
		}
		position += 1;
	}
	let destination = start..position.min(end);
	(!destination.is_empty()).then_some(destination)
}

/// Ranges asked about at increasing positions, in time linear in their number.
pub(crate) struct RangeSweep {
	/// Sorted, overlapping ones merged.
	ranges: Vec<Range<usize>>,
	next: usize,
}

impl RangeSweep {
	pub(crate) fn new(mut ranges: Vec<Range<usize>>) -> Self {
		ranges.sort_unstable_by_key(|range| range.start);
		let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
		for range in ranges {
			match merged.last_mut() {
				Some(last) if range.start < last.end => last.end = last.end.max(range.end),
				_ => merged.push(range),
			}
		}
		Self { ranges: merged, next: 0 }
	}

	fn skip_before(&mut self, position: usize) {
		while self.ranges.get(self.next).is_some_and(|range| range.end <= position) {
			self.next += 1;
		}
	}

	/// Whether `range` overlaps one of the ranges. `range.start` must not decrease.
	pub(crate) fn overlaps(&mut self, range: Range<usize>) -> bool {
		self.skip_before(range.start);
		self.ranges.get(self.next).is_some_and(|next| next.start < range.end)
	}

	/// Whether `position` lies inside one of the ranges, after its start. `position` must not
	/// decrease.
	pub(crate) fn contains_after_start(&mut self, position: usize) -> bool {
		self.skip_before(position);
		self.ranges.get(self.next).is_some_and(|next| next.start < position)
	}
}

/// The first byte of `line` after the indentation and the markers of quotes and list items.
pub(crate) fn content_start(line: &[u8]) -> usize {
	let mut index = 0;
	loop {
		let next = skip_list_marker(line, skip_container_markers(line, index));
		if next == index {
			return index;
		}
		index = next;
	}
}

fn skip_container_markers(line: &[u8], start: usize) -> usize {
	start + line[start..].iter().take_while(|&&byte| matches!(byte, b' ' | b'\t' | b'>')).count()
}

fn skip_list_marker(line: &[u8], start: usize) -> usize {
	let digits = line[start..].iter().take_while(|byte| byte.is_ascii_digit()).count();
	let marker_end = match line.get(start + digits) {
		Some(b'-' | b'*' | b'+') if digits == 0 => start + 1,
		Some(b'.' | b')') if digits > 0 => start + digits + 1,
		_ => return start,
	};
	match line.get(marker_end) {
		Some(b' ' | b'\t') => marker_end,
		_ => start,
	}
}

/// Whether the `---` line at `start` would open a YAML metadata block: exactly three dashes,
/// followed by a line that is neither blank nor a closing line.
fn opens_metadata_block(text: &[u8], start: usize) -> bool {
	let line = line_at(text, start);
	if trim_line_end(line) != b"---" {
		return false;
	}
	let next_line = line_at(text, start + line.len());
	!is_blank_line(next_line) && !closes_metadata_block(next_line)
}

/// Whether `line` closes a YAML metadata block.
pub(crate) fn closes_metadata_block(line: &[u8]) -> bool {
	matches!(trim_line_end(line), b"---" | b"...")
}

/// The line starting at `start`, with its line break.
pub(crate) fn line_at(text: &[u8], start: usize) -> &[u8] {
	let rest = text.get(start..).unwrap_or_default();
	let end = match rest.iter().position(|&byte| byte == b'\n' || byte == b'\r') {
		Some(index) if rest[index] == b'\r' && rest.get(index + 1) == Some(&b'\n') => index + 2,
		Some(index) => index + 1,
		None => rest.len(),
	};
	&rest[..end]
}

fn trim_line_end(line: &[u8]) -> &[u8] {
	let end =
		line.iter().rposition(|byte| !byte.is_ascii_whitespace()).map_or(0, |index| index + 1);
	&line[..end]
}

pub(crate) fn is_blank_line(line: &[u8]) -> bool {
	line.iter().all(|&byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

struct OpenLink {
	range: Range<usize>,
	/// The end of the link text or image description so far.
	text_end: usize,
	/// Whether the whole link is its destination, like `<https://example.com>`.
	is_autolink: bool,
}

struct OpenCodeBlock {
	language: String,
	code_ranges: Vec<Range<usize>>,
	gap_style: u8,
}

/// Paints the style of each element over its range, inner elements after outer ones.
struct Styler<'text> {
	text: &'text str,
	paintings: Vec<(Range<usize>, u8)>,
	/// The style of each open element.
	open_styles: Vec<u8>,
	links: Vec<OpenLink>,
	table_outer_style: u8,
	is_in_table_head: bool,
	code_block: Option<OpenCodeBlock>,
	code_blocks: Vec<CodeBlock>,
	/// Code, HTML and metadata blocks at any depth.
	verbatim_blocks: Vec<Range<usize>>,
	/// Everything but containers of blocks, which covers all text but link reference
	/// definitions.
	covered: Vec<Range<usize>>,
}

impl<'text> Styler<'text> {
	fn new(text: &'text str) -> Self {
		Self {
			text,
			paintings: Vec::new(),
			open_styles: Vec::new(),
			links: Vec::new(),
			table_outer_style: style::DEFAULT,
			is_in_table_head: false,
			code_block: None,
			code_blocks: Vec::new(),
			verbatim_blocks: Vec::new(),
			covered: Vec::new(),
		}
	}

	fn current_style(&self) -> u8 {
		self.open_styles.last().copied().unwrap_or(style::DEFAULT)
	}

	fn paint(&mut self, range: Range<usize>, style: u8) {
		if !range.is_empty() {
			self.paintings.push((range, style));
		}
	}

	fn handle(&mut self, event: Event, range: Range<usize>) {
		if !matches!(event, Event::Start(_) | Event::End(_)) {
			self.covered.push(range.clone());
		}
		if !matches!(event, Event::End(_))
			&& let Some(link) = self.links.last_mut()
		{
			link.text_end = link.text_end.max(range.end);
		}
		match event {
			Event::Start(tag) => self.start(tag, range),
			Event::End(tag) => self.end(tag, range),
			Event::Text(_) => {
				if let Some(code_block) = &mut self.code_block {
					code_block.code_ranges.push(range.clone());
					self.paint(range, style::CODE_BLOCK);
				}
			}
			Event::Code(_) => self.paint(range, style::INLINE_CODE),
			Event::InlineMath(_) | Event::DisplayMath(_) => self.paint(range, style::MATH),
			Event::InlineHtml(_) => self.paint(range, style::HTML),
			Event::FootnoteReference(_) => self.paint(range, style::FOOTNOTE),
			Event::Rule => self.paint(range, style::HORIZONTAL_RULE),
			Event::TaskListMarker(_) => self.paint(range, style::TASK_MARKER),
			Event::Html(_) | Event::SoftBreak | Event::HardBreak => {}
		}
	}

	fn start(&mut self, tag: Tag, range: Range<usize>) {
		let inherited = self.current_style();
		if matches!(tag, Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::MetadataBlock(_)) {
			self.verbatim_blocks.push(range.clone());
		}
		if !matches!(
			tag,
			Tag::BlockQuote(_) | Tag::List(_) | Tag::Item | Tag::FootnoteDefinition(_)
		) {
			self.covered.push(range.clone());
		}
		let painted = match tag {
			Tag::Heading { level, .. } => Some(style::HEADING_1 + (level as u8).saturating_sub(1)),
			Tag::BlockQuote(_) => Some(style::BLOCKQUOTE),
			Tag::CodeBlock(kind) => {
				let (language, painted) = match kind {
					CodeBlockKind::Fenced(info) => (
						info.split_whitespace().next().unwrap_or("").to_owned(),
						Some(style::CODE_FENCE),
					),
					CodeBlockKind::Indented => (String::new(), None),
				};
				self.code_block =
					Some(OpenCodeBlock { language, code_ranges: Vec::new(), gap_style: inherited });
				painted
			}
			Tag::HtmlBlock => Some(style::HTML),
			Tag::Item => {
				let marker = list_marker(self.text, range.start);
				self.paint(marker, style::LIST_MARKER);
				None
			}
			Tag::FootnoteDefinition(_) => {
				let label = footnote_label(self.text, range.clone());
				self.paint(label, style::FOOTNOTE);
				None
			}
			Tag::Table(_) => {
				self.table_outer_style = inherited;
				Some(style::TABLE)
			}
			Tag::TableHead => {
				self.is_in_table_head = true;
				None
			}
			Tag::TableCell => Some(match self.is_in_table_head {
				true => style::TABLE_HEADER,
				false => self.table_outer_style,
			}),
			Tag::Emphasis => Some(style::EMPHASIS),
			Tag::Strong => Some(style::STRONG),
			Tag::Strikethrough => Some(style::STRIKETHROUGH),
			Tag::Link { link_type, .. } | Tag::Image { link_type, .. } => {
				let is_image = matches!(tag, Tag::Image { .. });
				let is_autolink = matches!(link_type, LinkType::Autolink | LinkType::Email);
				let opener_length = if is_image { 2 } else { 1 };
				self.links.push(OpenLink {
					range: range.clone(),
					text_end: (range.start + opener_length).min(range.end),
					is_autolink,
				});
				Some(match (is_autolink, is_image) {
					(true, _) => style::URL,
					(false, true) => style::IMAGE,
					(false, false) => style::LINK,
				})
			}
			Tag::MetadataBlock(_) => Some(style::FRONT_MATTER),
			_ => None,
		};
		if let Some(painted) = painted {
			self.paint(range, painted);
		}
		self.open_styles.push(painted.unwrap_or(inherited));
	}

	fn end(&mut self, tag: TagEnd, range: Range<usize>) {
		self.open_styles.pop();
		match tag {
			TagEnd::TableHead => self.is_in_table_head = false,
			TagEnd::Link | TagEnd::Image => {
				let Some(link) = self.links.pop() else { return };
				let bytes = self.text.as_bytes();
				let range = link.range;
				if link.is_autolink {
					// `<` and `>` keep the style of the link text.
					if bytes.get(range.start) == Some(&b'<') && range.len() >= 2 {
						self.paint(range.start..range.start + 1, style::LINK);
						self.paint(range.end - 1..range.end, style::LINK);
					}
					return;
				}
				// The destination or reference follows the `]` closing the text. Only the
				// destination or the reference label is a URL, not the brackets and the title.
				let text_end = link.text_end.min(range.end);
				let Some(bracket) = self.text[text_end..range.end].find(']') else { return };
				let opener = text_end + bracket + 1;
				match bytes.get(opener) {
					Some(b'(') if opener < range.end => {
						if let Some(destination) = link_destination(bytes, opener + 1..range.end) {
							self.paint(destination, style::URL);
						}
					}
					Some(b'[') if opener + 1 < range.end && bytes[range.end - 1] == b']' => {
						self.paint(opener + 1..range.end - 1, style::URL);
					}
					_ => {}
				}
			}
			TagEnd::CodeBlock => {
				let Some(code_block) = self.code_block.take() else { return };
				let (Some(first), Some(last)) =
					(code_block.code_ranges.first(), code_block.code_ranges.last())
				else {
					return;
				};
				let content = first.start..last.end.max(first.start);
				let gaps: Vec<Range<usize>> = code_block
					.code_ranges
					.windows(2)
					.map(|pair| pair[0].end..pair[1].start)
					.filter(|gap| !gap.is_empty())
					.collect();
				for gap in &gaps {
					self.paint(gap.clone(), code_block.gap_style);
				}
				self.code_blocks.push(CodeBlock {
					language: code_block.language,
					content,
					gaps,
					gap_style: code_block.gap_style,
				});
			}
			_ => {}
		}
		let _ = range;
	}

	/// The style of every byte of `range`.
	fn rasterize(&self, range: Range<usize>) -> Vec<u8> {
		let mut styles = vec![style::DEFAULT; range.len()];
		for (painted, style) in &self.paintings {
			let start = painted.start.clamp(range.start, range.end) - range.start;
			let end = painted.end.clamp(range.start, range.end) - range.start;
			styles[start..end].fill(*style);
		}
		styles
	}
}

/// The `-`, `*`, `+`, `1.` or `1)` starting the list item at `start`.
fn list_marker(text: &str, start: usize) -> Range<usize> {
	let bytes = text.as_bytes();
	let mut position = start;
	while bytes.get(position).is_some_and(|&byte| byte == b' ' || byte == b'\t') {
		position += 1;
	}
	let marker_start = position;
	match bytes.get(position) {
		Some(b'-' | b'*' | b'+') => position += 1,
		Some(byte) if byte.is_ascii_digit() => {
			while bytes.get(position).is_some_and(u8::is_ascii_digit) {
				position += 1;
			}
			if bytes.get(position).is_some_and(|&byte| byte == b'.' || byte == b')') {
				position += 1;
			} else {
				position = marker_start;
			}
		}
		_ => {}
	}
	marker_start..position
}

/// The `[^label]:` starting a footnote definition.
fn footnote_label(text: &str, range: Range<usize>) -> Range<usize> {
	let definition = &text[range.clone()];
	let start = definition.find("[^").unwrap_or(definition.len());
	match definition[start..].find("]:") {
		Some(end) => range.start + start..range.start + start + end + 2,
		None => range.start..range.start,
	}
}

/// Notepad++'s styles file `plugins/Config/NppToolBox.xml` with the language and its default
/// styles. Notepad++ uses the language for files with `extensions` that no user defined
/// language claims, and saves changes of the Style Configurator into this file.
pub fn styles_file(extensions: &[&str], line_ending: &str) -> String {
	let name = LANGUAGE_NAME.to_str().unwrap_or_default();
	let mut lines = vec![
		"<?xml version=\"1.0\" encoding=\"UTF-8\" ?>".to_owned(),
		"<NotepadPlus>".to_owned(),
		"\t<Languages>".to_owned(),
		format!(
			"\t\t<Language name=\"{name}\" ext=\"{}\" commentStart=\"&lt;!--\" commentEnd=\"--&gt;\" />",
			extensions.join(" ")
		),
		"\t</Languages>".to_owned(),
		"\t<LexerStyles>".to_owned(),
		format!("\t\t<LexerType name=\"{name}\" desc=\"{name}\" ext=\"\">"),
	];
	for (id, style) in STYLES.iter().enumerate() {
		// `colorStyle` 1 sets only the text color, 0 neither color: the background and missing
		// text colors come from the default style of the theme, light or dark.
		let (color, color_style) = match style.color {
			Some(color) => (color, 1),
			None => (0x000000, 0),
		};
		lines.push(format!(
			"\t\t\t<WordsStyle name=\"{}\" styleID=\"{id}\" fgColor=\"{color:06X}\" bgColor=\"FFFFFF\" colorStyle=\"{color_style}\" fontName=\"\" fontStyle=\"{}\" fontSize=\"\" />",
			style.name.to_str().unwrap_or_default(),
			style.font_style,
		));
	}
	lines.extend([
		"\t\t</LexerType>".to_owned(),
		"\t</LexerStyles>".to_owned(),
		"</NotepadPlus>".to_owned(),
		String::new(),
	]);
	lines.join(line_ending)
}

#[cfg(test)]
mod tests {
	use super::style::*;
	use super::*;

	/// `text` with each styled byte replaced by the letter of its style (`a` is 0, `b` is 1, ...),
	/// for comparing styles at a glance.
	fn style_letters(text: &str) -> String {
		let lexed = lex_text(text);
		assert_eq!(lexed.styles.len(), text.len());
		lexed.styles.iter().map(|&style| char::from(b'a' + style)).collect()
	}

	fn lex_text(text: &str) -> LexedSlice {
		lex_slice(text, 0, text.len(), &mut |_| false)
	}

	fn styles_of(text: &str, part: &str) -> Vec<u8> {
		let lexed = lex_text(text);
		let start = text.find(part).expect("part of the text");
		lexed.styles[start..start + part.len()].to_vec()
	}

	fn uniform(style: u8, length: usize) -> Vec<u8> {
		vec![style; length]
	}

	#[test]
	fn styles_headings_and_emphasis() {
		let text = "# Title\n\nSome *em* and **strong** ~~gone~~.\n";
		assert_eq!(styles_of(text, "# Title"), uniform(HEADING_1, 7));
		assert_eq!(styles_of(text, "Some "), uniform(DEFAULT, 5));
		assert_eq!(styles_of(text, "*em*"), uniform(EMPHASIS, 4));
		assert_eq!(styles_of(text, "**strong**"), uniform(STRONG, 10));
		assert_eq!(styles_of(text, "~~gone~~"), uniform(STRIKETHROUGH, 8));
		assert_eq!(styles_of("Title\n===\n", "Title\n==="), uniform(HEADING_1, 9));
		assert_eq!(styles_of("### Three\n", "### Three"), uniform(HEADING_1 + 2, 9));
	}

	#[test]
	fn styles_links_and_their_destinations() {
		let text = "See [the *docs*](https://example.com \"Title\") and <https://a.b>.\n";
		assert_eq!(styles_of(text, "[the "), uniform(LINK, 5));
		assert_eq!(styles_of(text, "*docs*"), uniform(EMPHASIS, 6));
		assert_eq!(styles_of(text, "]("), uniform(LINK, 2));
		assert_eq!(styles_of(text, "https://example.com"), uniform(URL, 19));
		assert_eq!(styles_of(text, " \"Title\")"), uniform(LINK, 9));
		assert_eq!(
			styles_of(text, "<https://a.b>"),
			[vec![LINK], uniform(URL, 11), vec![LINK]].concat()
		);
		let image = "![alt `]` text](i.png)\n";
		assert_eq!(styles_of(image, "![alt "), uniform(IMAGE, 6));
		assert_eq!(styles_of(image, "`]`"), uniform(INLINE_CODE, 3));
		assert_eq!(styles_of(image, " text]"), uniform(IMAGE, 6));
		assert_eq!(
			styles_of(image, "(i.png)"),
			[vec![IMAGE], uniform(URL, 5), vec![IMAGE]].concat()
		);
		let reference = "[a][b] [b]\n\n[b]: https://example.com\n> - [b]: /again 'Title'\n";
		assert_eq!(styles_of(reference, "[a]"), uniform(LINK, 3));
		assert_eq!(styles_of(reference, "[b] "), [vec![LINK, URL, LINK], vec![DEFAULT]].concat());
		assert_eq!(
			styles_of(reference, "[b]: https://example.com"),
			[uniform(LINK, 5), uniform(URL, 19)].concat()
		);
		assert_eq!(
			styles_of(reference, "[b]: /again 'Title'"),
			[uniform(LINK, 5), uniform(URL, 6), uniform(LINK, 8)].concat()
		);
	}

	#[test]
	fn styles_only_the_destination_as_url() {
		let url = |text: &str| -> String {
			let lexed = lex_text(text);
			text.char_indices()
				.filter(|&(index, _)| lexed.styles[index] == URL)
				.map(|(_, character)| character)
				.collect()
		};
		assert_eq!(url("[a](<my file.md> \"t\")\n"), "my file.md");
		assert_eq!(url("[a](x_(1\\)).md)\n"), "x_(1\\)).md");
		assert_eq!(url("[a](\n  b.md\n)\n"), "b.md");
		assert_eq!(url("> [a](\n> b.md)\n"), "b.md");
		assert_eq!(url("[a]()\n"), "");
		assert_eq!(url("[a][]\n\n[a]: <x y>\n"), "x y");
		assert_eq!(url("[a]\n\n[a]:\n  next-line.md\n"), "");
		assert_eq!(url("<me@example.com>\n"), "me@example.com");
	}

	#[test]
	fn styles_lists_quotes_and_rules() {
		let text = "- [ ] task\n10. ten\n\n> quoted `code`\n\n---\n";
		assert_eq!(styles_of(text, "- [ ] task"), {
			let mut styles = uniform(DEFAULT, 10);
			styles[0] = LIST_MARKER;
			styles[2..5].fill(TASK_MARKER);
			styles
		});
		assert_eq!(
			styles_of(text, "10. ten"),
			[uniform(LIST_MARKER, 3), uniform(DEFAULT, 4)].concat()
		);
		assert_eq!(styles_of(text, "> quoted "), uniform(BLOCKQUOTE, 9));
		assert_eq!(styles_of(text, "`code`"), uniform(INLINE_CODE, 6));
		assert_eq!(styles_of(text, "---"), uniform(HORIZONTAL_RULE, 3));
	}

	#[test]
	fn styles_tables() {
		// `v` is TABLE, `w` TABLE HEADER and `a` DEFAULT.
		assert_eq!((char::from(b'a' + TABLE), char::from(b'a' + TABLE_HEADER)), ('v', 'w'));
		assert_eq!(
			style_letters("| A | B |\n|---|:-:|\n| a | b |\n"),
			concat!("vwwwvwwwvv", "vvvvvvvvvv", "vaaavaaavv")
		);
	}

	#[test]
	fn styles_code_html_math_and_footnotes() {
		let text = "Math $x^2$ and <b>bold</b>[^1].\n\n[^1]: Note\n\n<div>\nblock\n</div>\n";
		assert_eq!(styles_of(text, "$x^2$"), uniform(MATH, 5));
		assert_eq!(styles_of(text, "<b>"), uniform(HTML, 3));
		assert_eq!(styles_of(text, "bold"), uniform(DEFAULT, 4));
		assert_eq!(styles_of(text, "[^1]."), [uniform(FOOTNOTE, 4), vec![DEFAULT]].concat());
		assert_eq!(
			styles_of(text, "[^1]: Note"),
			[uniform(FOOTNOTE, 5), uniform(DEFAULT, 5)].concat()
		);
		assert_eq!(styles_of(text, "<div>\nblock\n</div>"), uniform(HTML, 18));
		let front_matter = "---\ntitle: x\n---\n\nText\n";
		assert_eq!(styles_of(front_matter, "---\ntitle: x\n---"), uniform(FRONT_MATTER, 16));
	}

	#[test]
	fn reports_code_blocks_with_their_language() {
		let text = "Text\n\n```rust ignore\nfn main() {}\nlet x = 1;\n```\n\n    indented\n";
		let lexed = lex_text(text);
		let code_start = text.find("fn main").unwrap_or_default();
		let code_end = text.find("```\n\n").unwrap_or_default();
		let indented = text.find("indented").unwrap_or_default();
		assert_eq!(
			lexed.code_blocks,
			[
				CodeBlock {
					language: "rust".to_owned(),
					content: code_start..code_end,
					gaps: Vec::new(),
					gap_style: DEFAULT,
				},
				CodeBlock {
					language: String::new(),
					content: indented..text.len(),
					gaps: Vec::new(),
					gap_style: DEFAULT,
				},
			]
		);
		assert_eq!(styles_of(text, "```rust ignore\n"), uniform(CODE_FENCE, 15));
		assert_eq!(styles_of(text, "fn main() {}"), uniform(CODE_BLOCK, 12));
		assert_eq!(
			styles_of(text, "```\n\n"),
			[uniform(CODE_FENCE, 3), uniform(DEFAULT, 2)].concat()
		);
	}

	#[test]
	fn reports_the_quote_markers_inside_code_blocks() {
		let text = "> ```c\n> int a;\n> int b;\n> ```\n";
		let lexed = lex_text(text);
		let [code_block] = lexed.code_blocks.as_slice() else { panic!("{:?}", lexed.code_blocks) };
		let second_line = text.find("> int b").unwrap_or_default();
		assert_eq!(code_block.language, "c");
		assert_eq!(code_block.gaps, vec![second_line..second_line + 2]);
		assert_eq!(code_block.gap_style, BLOCKQUOTE);
		assert_eq!(
			styles_of(text, "> int b"),
			[uniform(BLOCKQUOTE, 2), uniform(CODE_BLOCK, 5)].concat()
		);
	}

	#[test]
	fn styles_a_slice_with_definitions_in_front() {
		let text = "[^n]: x\n\nSee [^n] and [r].\n\nx y\n";
		let prefix = "[^n]: x\n\n".len();
		let slice = "See [^n] and [r].\n\n".len();
		let lexed = lex_slice(text, prefix, slice, &mut |label| label == "r");
		assert_eq!(lexed.styles.len(), slice);
		assert_eq!(lexed.styles[4..8], uniform(FOOTNOTE, 4));
		assert_eq!(lexed.styles[13..16], uniform(LINK, 3));
		assert!(!lexed.runs_past_end);
		let unclosed = "```\ncode\n\nx y\n";
		let lexed = lex_slice(unclosed, 0, "```\ncode\n\n".len(), &mut |_| false);
		assert!(lexed.runs_past_end);
		assert_eq!(lexed.verbatim_blocks, vec![0.."```\ncode\n\n".len()]);
	}

	#[test]
	fn sweeps_ranges() {
		let mut sweep = RangeSweep::new(vec![5..8, 0..2, 6..10, 10..12]);
		assert!(sweep.overlaps(1..3));
		assert!(!sweep.overlaps(2..5));
		assert!(!sweep.contains_after_start(5));
		assert!(sweep.contains_after_start(9));
		assert!(!sweep.contains_after_start(10));
		assert!(sweep.contains_after_start(11));
		assert!(!sweep.overlaps(12..20));
	}

	#[test]
	fn finds_metadata_openers() {
		let text = "Text\n\n---\ntitle: x\n\nMore\n";
		let lexed = lex_text(text);
		assert_eq!(lexed.metadata_opener, text.find("---"));
		assert_eq!(lex_text("Text\n\n---\n\nMore\n").metadata_opener, None);
		assert_eq!(lex_text("- - -\ntitle\n").metadata_opener, None);
		assert!(closes_metadata_block(b"...  \r\n"));
		assert!(!closes_metadata_block(b" ---\n"));
		assert_eq!(line_at(b"a\r\nb", 0), b"a\r\n");
		assert_eq!(line_at(b"a\rb", 2), b"b");
	}

	#[test]
	fn keeps_offsets_of_other_encodings() {
		let utf8 = "a\u{e4}b".as_bytes();
		assert!(matches!(parseable_text(utf8, true, |_| false), Cow::Borrowed("a\u{e4}b")));
		assert_eq!(parseable_text(b"a\xffb", true, |_| false), "axb");
		assert_eq!(parseable_text(b"a\xe4b", false, |_| false), "axb");
		// Shift-JIS: 0x95 0x5C is one character whose trail byte is `\`.
		assert_eq!(parseable_text(b"\x95\x5c*a*", false, |byte| byte == 0x95), "xx*a*");
		assert_eq!(parseable_text(b"*a*\x95", false, |byte| byte == 0x95), "*a*x");
	}

	#[test]
	fn writes_the_styles_file() {
		let file = styles_file(&["md", "markdown"], "\n");
		assert!(file.contains(
			"<Language name=\"Markdown (NppToolBox)\" ext=\"md markdown\" commentStart=\"&lt;!--\" commentEnd=\"--&gt;\" />"
		));
		assert!(file.contains(
			"<LexerType name=\"Markdown (NppToolBox)\" desc=\"Markdown (NppToolBox)\" ext=\"\">"
		));
		assert!(file.contains(
			"<WordsStyle name=\"HEADING 1\" styleID=\"1\" fgColor=\"A05DC8\" bgColor=\"FFFFFF\" colorStyle=\"1\" fontName=\"\" fontStyle=\"1\" fontSize=\"\" />"
		));
		assert!(file.contains(
			"name=\"DEFAULT\" styleID=\"0\" fgColor=\"000000\" bgColor=\"FFFFFF\" colorStyle=\"0\""
		));
		assert_eq!(file.matches("<WordsStyle ").count(), STYLES.len());
		assert!(STYLES.len() <= usize::from(FIRST_CODE_STYLE));
	}
}
