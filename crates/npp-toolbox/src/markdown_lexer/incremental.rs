//! Incremental lexing: only the part of the document around the requested range is parsed.
//!
//! Markdown blocks depend on the text before them, but at some lines nothing that came before
//! matters: a line at column 0 after a blank line, or a heading or bullet item, which end any
//! paragraph, list item, quote or table before them. Only a code fence, an HTML block or a
//! metadata block can continue over such a line. These restart lines are remembered in the line
//! states of the document. Parsing starts at the last restart line before the requested range
//! and ends at the first one after it.
//!
//! Scintilla only asks to style from the first line whose text changed, so the line states
//! before it are still valid. The parts outside the parsed text are handled like this:
//!
//! - Link references resolve against the definitions found by a quick scan of the document,
//!   footnote references against footnote definitions put in front of the parsed text. When
//!   the definitions change, the whole document is styled again.
//! - A sentinel line after the parsed text shows whether a code or HTML block continues past the
//!   end; then more text is parsed.
//! - `pulldown-cmark` turns a `---` line into the opener of a metadata block if a closing line
//!   follows anywhere later. The parsed text is extended up to that line, and the lines after an
//!   opener without one are no restart lines, as typing a closing line later would change them.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Range;

use super::{
	CodeBlock, LexedMarkdown, LexedSlice, RangeSweep, closes_metadata_block, content_start,
	lex_slice, line_at, parseable_text,
};

/// The document a lexer styles, as Scintilla's `IDocument` provides it.
pub trait MarkdownDocument {
	fn length(&self) -> usize;
	/// Appends the bytes of `range` to `buffer`.
	fn read(&self, range: Range<usize>, buffer: &mut Vec<u8>);
	fn line_of(&self, position: usize) -> usize;
	/// The start of `line`, or the length of the document after the last line.
	fn line_start(&self, line: usize) -> usize;
	/// Whether the last lexing of `line` found that parsing can start there.
	fn is_restart_line(&self, line: usize) -> bool;
	fn set_restart_line(&mut self, line: usize, is_restart: bool);
	fn is_utf8(&self) -> bool;
	fn is_lead_byte(&self, byte: u8) -> bool;
}

/// Bytes read from the document at least at once.
const READ_CHUNK: usize = 16 * 1024;
/// A line that an unclosed code fence or HTML block takes in, but that cannot complete anything
/// before it (`x y` is no link destination with a title).
const SENTINEL: &str = "x y\n";
/// A paragraph after the definitions in front of the parsed text, after which the text starts
/// like at the start of a document.
const PREFIX_END: &str = "x y\n\n";

/// What a lexer remembers of a document between calls.
#[derive(Clone, Default)]
pub struct LexerMemory {
	/// The definitions of the last scan, `None` before the first one.
	labels: Option<Labels>,
}

/// Styles at least `requested` of `document`, see [`LexedMarkdown`], and updates the restart
/// lines of the parsed part.
pub fn lex_document<D: MarkdownDocument>(
	document: &mut D,
	memory: &mut LexerMemory,
	requested: Range<usize>,
) -> LexedMarkdown {
	let length = document.length();
	let requested = requested.start.min(length)..requested.end.min(length);
	if requested.is_empty() {
		return LexedMarkdown { start: requested.start, ..LexedMarkdown::default() };
	}
	let start = restart_position(document, requested.start);
	let (lexed, restart_lines) = lex_from(document, memory, start, requested.end);
	let first_line = document.line_of(start);
	for (index, is_restart) in restart_lines.into_iter().enumerate() {
		document.set_restart_line(first_line + 1 + index, is_restart);
	}
	lexed
}

/// Styles from the restart line at `start` to the first restart line at or after `end`. Also
/// returns whether each line after the first one is a restart line.
fn lex_from<D: MarkdownDocument>(
	document: &D,
	memory: &mut LexerMemory,
	start: usize,
	end: usize,
) -> (LexedMarkdown, Vec<bool>) {
	let length = document.length();
	let mut reader = Reader { document, start, length, bytes: Vec::new() };
	// Scanned when needed, at most once.
	let labels: RefCell<Option<Labels>> = RefCell::new(None);
	let scan_labels = || {
		let mut labels = labels.borrow_mut();
		if labels.is_none() {
			*labels = Some(Labels::scan(document));
		}
	};
	// Definitions removed or added anywhere change the references, so they are checked while
	// there are any, and when the parsed text may define one.
	let had_definitions = memory.labels.as_ref().is_some_and(|labels| !labels.is_empty());
	let mut min_cut = end;
	loop {
		let cut = reader.restart_at_or_after(min_cut);
		let (text, prefix_length) = {
			let slice = parseable_text(reader.read(start..cut), document.is_utf8(), |byte| {
				document.is_lead_byte(byte)
			});
			if had_definitions || slice.contains("]:") || slice.contains("[^") {
				scan_labels();
			}
			let mut text = String::new();
			if slice.contains("[^")
				&& let Some(labels) = labels.borrow().as_ref()
			{
				for label in &labels.footnotes {
					text.push_str("[^");
					text.push_str(label);
					text.push_str("]: x\n\n");
				}
				// Ends the last definition, which would continue with an indented first line.
				if !text.is_empty() {
					text.push_str(PREFIX_END);
				}
			}
			let prefix_length = text.len();
			text.push_str(&slice);
			if cut < length {
				text.push_str(SENTINEL);
			}
			(text, prefix_length)
		};
		let mut is_defined_link = |label: &str| {
			scan_labels();
			labels
				.borrow()
				.as_ref()
				.is_some_and(|labels| labels.links.contains(&normalized_label(label)))
		};
		let lexed = lex_slice(&text, prefix_length, cut - start, &mut is_defined_link);
		if cut < length {
			if lexed.runs_past_end {
				min_cut = cut + (cut - start).max(READ_CHUNK);
				continue;
			}
			if let Some(opener) = lexed.metadata_opener
				&& let Some(closer_end) = reader.metadata_closer_end(start + opener)
				&& closer_end > cut
			{
				min_cut = closer_end;
				continue;
			}
		}
		let cut_line = (cut < length).then(|| reader.line(cut).to_vec());
		let restart_lines = restart_lines(reader.read(start..cut), cut_line.as_deref(), &lexed);
		let shift = |range: &Range<usize>| range.start + start..range.end + start;
		let code_blocks = lexed
			.code_blocks
			.iter()
			.map(|code_block| CodeBlock {
				language: code_block.language.clone(),
				content: shift(&code_block.content),
				gaps: code_block.gaps.iter().map(shift).collect(),
				gap_style: code_block.gap_style,
			})
			.collect();
		let mut changes_whole_document = false;
		if let Some(labels) = labels.into_inner() {
			changes_whole_document = memory.labels.as_ref().is_some_and(|known| *known != labels);
			memory.labels = Some(labels);
		}
		let lexed =
			LexedMarkdown { start, styles: lexed.styles, code_blocks, changes_whole_document };
		return (lexed, restart_lines);
	}
}

/// The start of the last restart line at or before the line of `position`, else 0.
fn restart_position(document: &impl MarkdownDocument, position: usize) -> usize {
	let line = document.line_of(position);
	for candidate in (1..=line).rev() {
		if !document.is_restart_line(candidate) {
			continue;
		}
		let line_start = document.line_start(candidate);
		// The text of the line of `position` may have changed since it was marked, the text
		// before it has not.
		if candidate < line {
			return line_start;
		}
		let mut text = Vec::new();
		let previous_start = document.line_start(candidate - 1);
		document.read(previous_start..document.line_start(candidate + 1), &mut text);
		let previous = line_at(&text, 0);
		if is_restart_candidate(previous, line_at(&text, previous.len())) {
			return line_start;
		}
	}
	0
}

/// Whether parsing can start at `line` after `previous_line`, unless a code fence, an HTML
/// block or a metadata block runs over it.
pub(crate) fn is_restart_candidate(previous_line: &[u8], line: &[u8]) -> bool {
	let Some(&first) = line.first() else { return false };
	if matches!(first, b' ' | b'\t' | b'\r' | b'\n') {
		return false;
	}
	// After a link reference definition, a line of only spaces or tabs starts a paragraph in
	// `pulldown-cmark` 0.13, which takes in the lines after it. So only empty lines count.
	let is_previous_empty = previous_line.iter().all(|&byte| matches!(byte, b'\r' | b'\n'));
	is_previous_empty || starts_heading(line) || starts_bullet_item(line)
}

/// An ATX heading, which no paragraph continues.
fn starts_heading(line: &[u8]) -> bool {
	let hashes = line.iter().take_while(|&&byte| byte == b'#').count();
	(1..=6).contains(&hashes)
		&& line.get(hashes).is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
}

/// A bullet item with content, which ends a paragraph before it. An empty one could be the
/// underline of a heading, and one of only `-`, `|` and `:` the delimiter row of a table.
fn starts_bullet_item(line: &[u8]) -> bool {
	matches!(line.first(), Some(b'-' | b'*' | b'+'))
		&& matches!(line.get(1), Some(b' ' | b'\t'))
		&& line[2..]
			.iter()
			.any(|byte| !matches!(byte, b'-' | b'|' | b':' | b' ' | b'\t' | b'\r' | b'\n'))
}

/// For each line of `slice` after the first one, and the line at its end (`cut_line`), whether
/// it is a restart line.
fn restart_lines(slice: &[u8], cut_line: Option<&[u8]>, lexed: &LexedSlice) -> Vec<bool> {
	let mut verbatim_blocks = RangeSweep::new(lexed.verbatim_blocks.clone());
	let mut is_restart = |previous: &[u8], line: &[u8], position: usize| {
		// Asked for every line, as the sweep must see increasing positions.
		let is_in_verbatim_block = verbatim_blocks.contains_after_start(position);
		is_restart_candidate(previous, line)
			&& !is_in_verbatim_block
			&& lexed.metadata_opener.is_none_or(|opener| position <= opener)
	};
	let mut restart_lines = Vec::new();
	let mut previous = line_at(slice, 0);
	let mut position = previous.len();
	while position < slice.len() {
		let line = line_at(slice, position);
		restart_lines.push(is_restart(previous, line, position));
		previous = line;
		position += line.len();
	}
	match cut_line {
		Some(cut_line) => restart_lines.push(is_restart(previous, cut_line, position)),
		// The empty last line of the document, where typing may start.
		None if previous.ends_with(b"\n") || previous.ends_with(b"\r") => {
			restart_lines.push(false);
		}
		None => {}
	}
	restart_lines
}

/// Reads the document from `start` on, in chunks, as far as needed.
struct Reader<'document, D> {
	document: &'document D,
	start: usize,
	length: usize,
	/// The text from `start`.
	bytes: Vec<u8>,
}

impl<D: MarkdownDocument> Reader<'_, D> {
	fn end(&self) -> usize {
		self.start + self.bytes.len()
	}

	fn fill_to(&mut self, end: usize) {
		let end = end.min(self.length);
		let from = self.end();
		if end <= from {
			return;
		}
		let chunk_end = end.max(from + self.bytes.len().max(READ_CHUNK)).min(self.length);
		self.document.read(from..chunk_end, &mut self.bytes);
	}

	fn read(&mut self, range: Range<usize>) -> &[u8] {
		self.fill_to(range.end);
		&self.bytes[range.start - self.start..range.end - self.start]
	}

	/// The line at `position` with its line break; empty at the end of the document.
	fn line(&mut self, position: usize) -> &[u8] {
		self.fill_to(position + 1);
		loop {
			let relative = (position - self.start).min(self.bytes.len());
			let rest = &self.bytes[relative..];
			let is_complete = match rest.iter().position(|&byte| byte == b'\n' || byte == b'\r') {
				// A `\r` may be followed by `\n`.
				Some(index) => rest[index] == b'\n' || relative + index + 1 < self.bytes.len(),
				None => false,
			};
			if is_complete || self.end() >= self.length {
				return line_at(&self.bytes, relative);
			}
			self.fill_to(self.end() + 1);
		}
	}

	/// The start of the line containing `position`, not before `start`.
	fn line_start_of(&mut self, position: usize) -> usize {
		self.fill_to(position + 1);
		let mut end = (position - self.start).min(self.bytes.len());
		if self.bytes.get(end) == Some(&b'\n') && end > 0 && self.bytes[end - 1] == b'\r' {
			end -= 1;
		}
		self.bytes[..end]
			.iter()
			.rposition(|&byte| byte == b'\n' || byte == b'\r')
			.map_or(self.start, |index| self.start + index + 1)
	}

	/// The first restart candidate at or after `position` (> `start`), else the end of the
	/// document.
	fn restart_at_or_after(&mut self, position: usize) -> usize {
		let mut previous_start = self.line_start_of(position.saturating_sub(1).max(self.start));
		loop {
			let previous_length = self.line(previous_start).len();
			let candidate = previous_start + previous_length;
			if previous_length == 0 || candidate >= self.length {
				return self.length;
			}
			if candidate >= position {
				let line_length = self.line(candidate).len();
				let relative = previous_start - self.start;
				let previous = &self.bytes[relative..relative + previous_length];
				let line =
					&self.bytes[candidate - self.start..candidate - self.start + line_length];
				if is_restart_candidate(previous, line) {
					return candidate;
				}
			}
			previous_start = candidate;
		}
	}

	/// The end of the first line closing a metadata block after the opener at `opener`.
	fn metadata_closer_end(&mut self, opener: usize) -> Option<usize> {
		let mut position = opener + self.line(opener).len();
		loop {
			let line = self.line(position);
			if line.is_empty() {
				return None;
			}
			let line_length = line.len();
			if closes_metadata_block(line) {
				return Some(position + line_length);
			}
			position += line_length;
		}
	}
}

/// The labels of the link reference and footnote definitions of a document, found by their
/// syntax at the start of a line. Lines that look like a definition count, even where the
/// parser would not take them as one (e.g. inside a code block).
#[derive(Clone, Default, PartialEq, Eq)]
struct Labels {
	/// Normalized, see [`normalized_label`].
	links: HashSet<String>,
	footnotes: Vec<String>,
}

impl Labels {
	fn scan(document: &impl MarkdownDocument) -> Self {
		let mut bytes = Vec::new();
		document.read(0..document.length(), &mut bytes);
		let text = parseable_text(&bytes, document.is_utf8(), |byte| document.is_lead_byte(byte));
		let mut labels = Self::default();
		// `split` finds the line feeds with `memchr`. Definitions after a lone `\r` are missed.
		let mut line_start = 0;
		for line in text.split('\n') {
			let mut rest = &text[line_start..];
			line_start += line.len() + 1;
			// A footnote definition may start with further definitions.
			while let Some((definition, end)) = definition_label(rest) {
				match definition {
					Definition::Link(label) => {
						labels.links.insert(normalized_label(label));
						break;
					}
					Definition::Footnote(label) => {
						if !labels.footnotes.iter().any(|known| known == label) {
							labels.footnotes.push(label.to_owned());
						}
					}
				}
				rest = rest[end..].trim_start_matches([' ', '\t']);
			}
		}
		labels
	}

	fn is_empty(&self) -> bool {
		self.links.is_empty() && self.footnotes.is_empty()
	}
}

enum Definition<'line> {
	Link(&'line str),
	Footnote(&'line str),
}

/// The label of `[label]: …` or `[^label]: …` at the start of `text`, after the indentation
/// and the markers of quotes and list items, and the end of its `]:`. Link labels may continue
/// on the next lines, up to a blank line.
fn definition_label(text: &str) -> Option<(Definition<'_>, usize)> {
	/// CommonMark's limit of a link label.
	const MAX_LABEL_LENGTH: usize = 999;
	let first_line = line_at(text.as_bytes(), 0);
	let label_start = content_start(first_line) + 1;
	let rest = text[label_start - 1..].strip_prefix('[')?;
	let end = rest.bytes().take(MAX_LABEL_LENGTH + 1).position(|byte| byte == b']')?;
	let label = &rest[..end];
	// The lines between two line breaks; the label may end with one.
	let lines: Vec<&str> = label.split('\n').collect();
	let inner_lines = lines.get(1..lines.len().saturating_sub(1)).unwrap_or_default();
	let has_blank_line = inner_lines.iter().any(|line| line.trim().is_empty());
	if !rest[end + 1..].starts_with(':')
		|| label.trim().is_empty()
		|| label.contains('[')
		|| has_blank_line
	{
		return None;
	}
	let definition_end = label_start + end + 2;
	// A footnote label cannot span lines, then it is a link label starting with `^`.
	match label.strip_prefix('^') {
		Some(footnote) if !footnote.contains(['\r', '\n']) => {
			Some((Definition::Footnote(footnote), definition_end))
		}
		_ => Some((Definition::Link(label), definition_end)),
	}
}

/// Labels match case-insensitively, with whitespace collapsed.
fn normalized_label(label: &str) -> String {
	label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[cfg(test)]
mod tests {
	use super::super::style::*;
	use super::*;

	/// A document that keeps its line states like Scintilla when it is edited.
	#[derive(Clone)]
	struct TextDocument {
		text: Vec<u8>,
		restart_lines: Vec<bool>,
		styles: Vec<u8>,
		end_styled: usize,
		/// The parsed range of each call.
		lexed: Vec<Range<usize>>,
		memory: LexerMemory,
		line_starts: Vec<usize>,
	}

	impl TextDocument {
		fn new(text: &str) -> Self {
			Self {
				text: text.as_bytes().to_vec(),
				restart_lines: Vec::new(),
				styles: vec![0; text.len()],
				end_styled: 0,
				lexed: Vec::new(),
				memory: LexerMemory::default(),
				line_starts: line_starts(text.as_bytes()),
			}
		}

		/// Like Scintilla: the lines after the line of `position` are removed or inserted, the
		/// inserted ones with the state of the line after.
		fn edit(&mut self, position: usize, deleted: usize, inserted: &str) {
			let line = self.line_of(position);
			let old_line_count = self.line_starts.len();
			self.text.splice(position..position + deleted, inserted.bytes());
			self.styles.splice(position..position + deleted, inserted.bytes().map(|_| 0));
			self.line_starts = line_starts(&self.text);
			let new_line_count = self.line_starts.len();
			if new_line_count < old_line_count {
				let removed = old_line_count - new_line_count;
				let first = (line + 1).min(self.restart_lines.len());
				let last = (line + 1 + removed).min(self.restart_lines.len());
				self.restart_lines.drain(first..last);
			} else if line < self.restart_lines.len() {
				let state = self.restart_lines.get(line + 1).copied().unwrap_or(false);
				for _ in old_line_count..new_line_count {
					self.restart_lines.insert(line + 1, state);
				}
			}
			// Scintilla's `Document::DeleteChars` restyles the last character when the deletion
			// reaches the end.
			let modified = match inserted.is_empty() && position == self.text.len() {
				true => position.saturating_sub(1),
				false => position,
			};
			self.end_styled = self.end_styled.min(modified);
		}

		/// Like Scintilla's `EnsureStyledTo`, repeated like painting does after
		/// `IDocument::ChangeLexerState`.
		fn ensure_styled(&mut self, position: usize) {
			while position > self.end_styled {
				self.style_once(position);
			}
		}

		fn style_once(&mut self, position: usize) {
			let start = self.line_start(self.line_of(self.end_styled));
			let mut memory = std::mem::take(&mut self.memory);
			let lexed = lex_document(self, &mut memory, start..position);
			self.memory = memory;
			let end = lexed.start + lexed.styles.len();
			self.styles[lexed.start..end].copy_from_slice(&lexed.styles);
			self.lexed.push(lexed.start..end);
			// Like `IDocument::ChangeLexerState`.
			self.end_styled = if lexed.changes_whole_document { 0 } else { end };
		}
	}

	impl MarkdownDocument for TextDocument {
		fn length(&self) -> usize {
			self.text.len()
		}

		fn read(&self, range: Range<usize>, buffer: &mut Vec<u8>) {
			buffer.extend_from_slice(&self.text[range]);
		}

		fn line_of(&self, position: usize) -> usize {
			self.line_starts.partition_point(|&start| start <= position) - 1
		}

		fn line_start(&self, line: usize) -> usize {
			self.line_starts.get(line).copied().unwrap_or(self.text.len())
		}

		fn is_restart_line(&self, line: usize) -> bool {
			self.restart_lines.get(line).copied().unwrap_or(false)
		}

		fn set_restart_line(&mut self, line: usize, is_restart: bool) {
			if self.restart_lines.len() <= line {
				self.restart_lines.resize(line + 1, false);
			}
			self.restart_lines[line] = is_restart;
		}

		fn is_utf8(&self) -> bool {
			true
		}

		fn is_lead_byte(&self, _byte: u8) -> bool {
			false
		}
	}

	fn line_starts(text: &[u8]) -> Vec<usize> {
		let mut starts = vec![0];
		let mut position = 0;
		while position < text.len() {
			position += line_at(text, position).len();
			if matches!(text[position - 1], b'\n' | b'\r') {
				starts.push(position);
			}
		}
		starts
	}

	/// `cargo test --release -p npp-toolbox --lib lexes_a_keystroke -- --ignored --nocapture`
	#[test]
	#[ignore = "benchmark"]
	fn lexes_a_keystroke_faster_than_the_whole_document() {
		let section = "## Section\n\nSome *text* with a [link](https://example.com) and `code`.\n\n\
		               - item one\n- item two\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n";
		let text = section.repeat(1024 * 1024 / section.len());
		let mut document = TextDocument::new(&text);
		let started = std::time::Instant::now();
		document.ensure_styled(text.len());
		let whole = started.elapsed();
		let middle = text.len() / 2;
		let position = middle + text[middle..].find("Some").unwrap_or_default();
		let started = std::time::Instant::now();
		const KEYSTROKES: u32 = 100;
		for _ in 0..KEYSTROKES {
			document.edit(position, 0, "x");
			// About one screen of text.
			document.ensure_styled(position + 4000);
		}
		let keystroke = started.elapsed() / KEYSTROKES;
		println!("{} bytes: whole document {whole:?}, keystroke {keystroke:?}", text.len());
		assert!(keystroke * 20 < whole);
	}

	fn fully_lexed(text: &[u8]) -> Vec<u8> {
		if text.is_empty() {
			return Vec::new();
		}
		let mut document = TextDocument::new(std::str::from_utf8(text).unwrap_or_default());
		document.ensure_styled(text.len());
		assert_eq!(document.lexed, vec![0..text.len()]);
		document.styles
	}

	#[test]
	fn parses_only_the_blocks_around_a_change() {
		let paragraphs: String = (0..200).map(|index| format!("Paragraph {index}\n\n")).collect();
		let mut document = TextDocument::new(&paragraphs);
		document.ensure_styled(paragraphs.len());
		let position = paragraphs.find("Paragraph 100").unwrap_or_default();
		document.edit(position + 2, 0, "*x*");
		document.lexed.clear();
		document.ensure_styled(position + 40);
		let [lexed] = document.lexed.as_slice() else { panic!("{:?}", document.lexed) };
		assert!(lexed.start >= position - 20 && lexed.end <= position + 80, "{lexed:?}");
		assert_eq!(document.styles[position + 2..position + 5], [EMPHASIS; 3]);
		assert_eq!(document.styles, fully_lexed(&document.text));
	}

	#[test]
	fn restarts_at_headings_and_bullet_items() {
		let items: String = (0..100).map(|index| format!("- item {index}\n")).collect();
		let text = format!("# Title\nText\n{items}## End\n");
		let mut document = TextDocument::new(&text);
		document.ensure_styled(text.len());
		let position = text.find("- item 50").unwrap_or_default();
		document.edit(position + 2, 0, "`");
		document.lexed.clear();
		document.ensure_styled(position + 10);
		let [lexed] = document.lexed.as_slice() else { panic!("{:?}", document.lexed) };
		assert_eq!(lexed.start, position);
		assert!(lexed.end < position + 40, "{lexed:?}");
		assert!(is_restart_candidate(b"text\n", b"# x\n"));
		assert!(is_restart_candidate(b"text\n", b"* x\n"));
		assert!(!is_restart_candidate(b"text\n", b"-\n"));
		assert!(!is_restart_candidate(b"text\n", b"#x\n"));
		assert!(!is_restart_candidate(b"text\n", b"2. x\n"));
		assert!(!is_restart_candidate(b"\n", b"  x\n"));
		assert!(is_restart_candidate(b"\r\n", b"x\r\n"));
		assert!(!is_restart_candidate(b"  \n", b"x\n"));
	}

	#[test]
	fn continues_code_blocks_past_restart_lines() {
		let text = "Intro\n\n```\ncode\n\n# not a heading\n```\n\nText\n";
		let mut document = TextDocument::new(text);
		document.ensure_styled(text.find("code").unwrap_or_default());
		assert_eq!(document.styles, fully_lexed(text.as_bytes()));
		let heading = text.find("# not").unwrap_or_default();
		assert!(!document.is_restart_line(document.line_of(heading)));
		// Removing the closing fence makes the code block run to the end.
		let fence = text.rfind("```").unwrap_or_default();
		document.edit(fence, 3, "");
		document.ensure_styled(document.text.len());
		assert_eq!(document.styles, fully_lexed(&document.text));
	}

	#[test]
	fn resolves_references_outside_the_parsed_part() {
		let filler: String = (0..50).map(|index| format!("Filler {index}\n\n")).collect();
		let text = format!("See [r] and [^f].\n\n{filler}[r]: /url\n\n[^f]: Note\n");
		let mut document = TextDocument::new(&text);
		document.ensure_styled(text.len());
		document.edit(0, 0, "x");
		document.lexed.clear();
		document.ensure_styled(10);
		assert!(document.lexed.iter().all(|lexed| lexed.end < 100), "{:?}", document.lexed);
		assert_eq!(document.styles[5..8], [LINK; 3]);
		assert_eq!(document.styles[13..17], [FOOTNOTE; 4]);
		assert_eq!(normalized_label(" Foo \t Bar "), "foo bar");
		assert!(matches!(definition_label("   [^a]: x"), Some((Definition::Footnote("a"), 8))));
		assert!(matches!(definition_label("> -   [a]: x"), Some((Definition::Link("a"), 10))));
		assert!(definition_label("[a] x").is_none());
		assert!(matches!(definition_label("[a\nb]: x"), Some((Definition::Link("a\nb"), 6))));
		assert!(definition_label("[a\n\nb]: x").is_none());
		assert!(matches!(definition_label("[^a\nb]: x"), Some((Definition::Link("^a\nb"), 7))));
		let nested = TextDocument::new("[^f]: [r]: /u\n");
		let labels = Labels::scan(&nested);
		assert_eq!((labels.footnotes, labels.links.len()), (vec!["f".to_owned()], 1));
	}

	#[test]
	fn includes_metadata_blocks_closed_later() {
		let filler: String = (0..20).map(|index| format!("Filler {index}\n\n")).collect();
		let text = format!("Intro\n\n---\ntitle: x\n\n{filler}---\n\nEnd\n");
		let mut document = TextDocument::new(&text);
		document.ensure_styled(20);
		assert_eq!(document.styles[7..10], [FRONT_MATTER; 3]);
		document.ensure_styled(text.len());
		assert_eq!(document.styles, fully_lexed(text.as_bytes()));
		// Without the closing line, the `---` is a rule, and the lines after it are no restart
		// lines, as typing a closing line would change them.
		let closer = text.rfind("---").unwrap_or_default();
		document.edit(closer, 3, "abc");
		document.ensure_styled(document.text.len());
		assert_eq!(document.styles, fully_lexed(&document.text));
		assert_eq!(document.styles[7..10], [HORIZONTAL_RULE; 3]);
		let filler_line = document.line_of(text.find("Filler 5").unwrap_or_default());
		assert!(!document.is_restart_line(filler_line));
		document.edit(closer, 3, "...");
		document.ensure_styled(document.text.len());
		assert_eq!(document.styles, fully_lexed(&document.text));
	}

	#[test]
	fn handles_crlf_line_ends() {
		let text = "# A\r\n\r\nText *a*\r\n\r\n- b\r\n- c\r\n";
		let mut document = TextDocument::new(text);
		document.ensure_styled(text.len());
		assert_eq!(document.styles, fully_lexed(text.as_bytes()));
		document.edit(text.find("- c").unwrap_or_default(), 0, "`x` ");
		document.ensure_styled(document.text.len());
		assert_eq!(document.styles, fully_lexed(&document.text));
	}

	/// Random edits styled incrementally, like Scintilla does while typing, give the same
	/// styles as lexing the whole text at once.
	#[test]
	fn styles_like_a_full_lexing_after_random_edits() {
		let pieces = [
			"# H",
			"## H",
			"- item",
			"* b",
			"+ c",
			"1. n",
			"2) m",
			"> q",
			"```",
			"```rust",
			"~~~",
			"<div>",
			"</div>",
			"<!--",
			"-->",
			"---",
			"...",
			"***",
			"| a | b |",
			"|---|---|",
			"*e*",
			"_u_",
			"**s**",
			"`c`",
			"[l](u)",
			"[r]",
			"[^f]",
			"\n[r]: /u\n",
			"\n[^f]: note\n",
			"$m$",
			"$$",
			"text",
			"word",
			" ",
			"    ",
			"\t",
			"\n",
			"\n",
			"\n",
			"\n\n",
			"===",
			"-",
			"x",
			"<b>",
			"\\",
			"![i](p)",
			"<https://a.b>",
		];
		let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
		let mut random = |limit: usize| {
			seed ^= seed << 13;
			seed ^= seed >> 7;
			seed ^= seed << 17;
			(seed % limit.max(1) as u64) as usize
		};
		let iterations: usize = std::env::var("RANDOM_LEXER_ITERATIONS")
			.ok()
			.and_then(|n| n.parse().ok())
			.unwrap_or(300);
		for _ in 0..iterations {
			let mut text = String::new();
			for _ in 0..random(60) {
				text.push_str(pieces[random(pieces.len())]);
			}
			let mut document = TextDocument::new(&text);
			let styled = random(text.len() + 1);
			document.ensure_styled(styled);
			let mut steps = vec![format!("{text:?} styled to {styled}")];
			for _ in 0..12 {
				let position = random(document.text.len() + 1);
				let (deleted, inserted) = match random(3) {
					0 => (random(12).min(document.text.len() - position), ""),
					_ => (0, pieces[random(pieces.len())]),
				};
				document.edit(position, deleted, inserted);
				let visible = (position + random(200)).min(document.text.len());
				document.ensure_styled(visible);
				steps.push(format!(
					"at {position} deleted {deleted}, inserted {inserted:?}, styled to {visible}"
				));
				let mut complete = document.clone();
				complete.ensure_styled(complete.text.len());
				let expected = fully_lexed(&complete.text);
				assert_eq!(complete.styles, expected, "{steps:#?}");
			}
		}
	}
}
