//! Rewrapping of paragraphs to a maximum line width, keeping their common prefix.
//!
//! A line is decomposed into a base prefix (indentation, comment marker, blockquote markers),
//! an optional Markdown list marker and its text. A paragraph is a run of lines whose base prefix
//! is identical (after tab expansion). A list item continues with lines indented to its text.
//! Structures whose line breaks are significant (code fences, indented code, tables, headings,
//! thematic breaks, HTML, link reference definitions, block comment delimiters) are never touched.

use std::ops::Range;

use crate::markdown_table::is_delimiter_row;
use crate::text_width::{column_after, display_width, expand_tabs};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentSyntax {
	/// Markdown documents: no comment markers, two trailing spaces are a hard line break.
	Markdown,
	/// Source code and plain text: comment markers are part of the prefix and the comment text
	/// is treated as Markdown.
	Code,
}

#[derive(Clone, Copy, Debug)]
pub struct RewrapSettings {
	pub max_line_width: usize,
	pub tab_width: usize,
	pub syntax: DocumentSyntax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LineParts<'a> {
	/// Indentation, comment marker and blockquote markers, each with the whitespace after it.
	base_prefix: &'a str,
	/// List item marker including an optional task checkbox and the whitespace after it.
	list_marker: &'a str,
	text: &'a str,
}

#[derive(Clone, Copy, Debug)]
enum LineRole<'a> {
	Wrappable(LineParts<'a>),
	/// Blank lines and lines whose line breaks are significant.
	Verbatim,
}

#[derive(Clone, Copy)]
struct Fence {
	character: char,
	length: usize,
}

enum Piece<'a> {
	Word(&'a str),
	/// Explicit Markdown line break. The suffix (two spaces) must be kept to preserve it.
	HardBreak(&'static str),
}

/// Lines of the paragraph containing `line_index`, or `None` if that line is not wrappable.
pub fn paragraph_at(
	lines: &[&str],
	line_index: usize,
	settings: &RewrapSettings,
) -> Option<Range<usize>> {
	let roles = classify_lines(lines, settings.syntax);
	paragraph_ranges(&roles, settings.tab_width)
		.into_iter()
		.find(|range| range.contains(&line_index))
}

/// Replacement lines for `range`. Paragraphs only partially inside `range` are rewrapped only
/// within it. Everything else is copied unchanged.
pub fn rewrap_range(lines: &[&str], range: Range<usize>, settings: &RewrapSettings) -> Vec<String> {
	let roles = classify_lines(lines, settings.syntax);
	let mut paragraph_end_by_line: Vec<Option<usize>> = vec![None; lines.len()];
	for paragraph in paragraph_ranges(&roles, settings.tab_width) {
		for line in paragraph.clone() {
			paragraph_end_by_line[line] = Some(paragraph.end);
		}
	}
	let mut output = Vec::with_capacity(range.len());
	let mut index = range.start;
	while index < range.end {
		match paragraph_end_by_line[index] {
			Some(paragraph_end) => {
				let end = paragraph_end.min(range.end);
				let parts: Vec<LineParts> = roles[index..end]
					.iter()
					.map(|role| match role {
						LineRole::Wrappable(parts) => *parts,
						LineRole::Verbatim => {
							unreachable!("paragraphs contain wrappable lines only")
						}
					})
					.collect();
				output.extend(wrap_paragraph(&parts, settings));
				index = end;
			}
			None => {
				output.push(lines[index].to_owned());
				index += 1;
			}
		}
	}
	output
}

fn classify_lines<'a>(lines: &[&'a str], syntax: DocumentSyntax) -> Vec<LineRole<'a>> {
	let parsed: Vec<LineParts> = lines.iter().map(|line| parse_line(line, syntax)).collect();
	let after_base = |index: usize| -> &str {
		let parts = &parsed[index];
		&lines[index][parts.base_prefix.len()..]
	};
	let mut roles = Vec::with_capacity(lines.len());
	let mut open_fence: Option<Fence> = None;
	let mut list_text_column: Option<usize> = None;
	let mut previous_was_wrappable = false;

	for (index, parts) in parsed.iter().enumerate() {
		let rest = after_base(index);
		let is_neighbor_delimiter_row = |neighbor: Option<usize>| {
			neighbor
				.filter(|&line| line < lines.len())
				.is_some_and(|line| is_delimiter_row(after_base(line)))
		};
		let is_verbatim = if let Some(fence) = open_fence {
			if closes_fence(rest, fence) {
				open_fence = None;
			}
			true
		} else if let Some(fence) = opens_fence(rest).or_else(|| opens_fence(parts.text)) {
			open_fence = Some(fence);
			true
		} else {
			let is_table_line = rest.starts_with('|')
				|| is_delimiter_row(rest)
				|| (rest.contains('|')
					&& (is_neighbor_delimiter_row(index.checked_sub(1))
						|| is_neighbor_delimiter_row(Some(index + 1))));
			parts.text.trim().is_empty()
				|| is_table_line
				|| is_thematic_break_or_setext_underline(rest)
				|| is_heading(rest)
				|| rest.starts_with('<')
				|| is_link_reference_definition(rest)
				|| (syntax == DocumentSyntax::Markdown
					&& parts.base_prefix.contains('>')
					&& is_alert_marker(rest))
				|| (syntax == DocumentSyntax::Code && is_block_comment_delimiter_line(lines[index]))
				|| (syntax == DocumentSyntax::Markdown
					&& parts.list_marker.is_empty()
					&& !previous_was_wrappable
					&& is_indented_code(parts.base_prefix, list_text_column))
		};

		if is_verbatim {
			previous_was_wrappable = false;
			roles.push(LineRole::Verbatim);
			continue;
		}
		let base_width = display_width(parts.base_prefix, 1);
		if !parts.list_marker.is_empty() {
			list_text_column = Some(column_after(parts.list_marker, base_width, 1));
		} else if list_text_column.is_some_and(|column| base_width < column) {
			list_text_column = None;
		}
		previous_was_wrappable = true;
		roles.push(LineRole::Wrappable(*parts));
	}
	roles
}

fn paragraph_ranges(roles: &[LineRole], tab_width: usize) -> Vec<Range<usize>> {
	let mut ranges = Vec::new();
	let mut index = 0;
	while index < roles.len() {
		let LineRole::Wrappable(first) = roles[index] else {
			index += 1;
			continue;
		};
		let continuation_key = continuation_key(&first, tab_width);
		let start = index;
		index += 1;
		while let Some(LineRole::Wrappable(next)) = roles.get(index) {
			if !next.list_marker.is_empty()
				|| expand_tabs(next.base_prefix, tab_width) != continuation_key
			{
				break;
			}
			index += 1;
		}
		ranges.push(start..index);
	}
	ranges
}

/// The base prefix continuation lines of a paragraph starting with `first` must have.
fn continuation_key(first: &LineParts, tab_width: usize) -> String {
	let base_width = display_width(first.base_prefix, tab_width);
	let text_column = column_after(first.list_marker, base_width, tab_width);
	expand_tabs(first.base_prefix, tab_width) + &" ".repeat(text_column - base_width)
}

fn wrap_paragraph(parts: &[LineParts], settings: &RewrapSettings) -> Vec<String> {
	let first = &parts[0];
	let first_prefix = format!("{}{}", first.base_prefix, first.list_marker);
	// An existing continuation line shows the preferred indentation style (tabs or spaces).
	let continuation_prefix = match parts.get(1) {
		Some(second) => second.base_prefix.to_owned(),
		None => {
			let hanging_indent = display_width(&first_prefix, settings.tab_width)
				- display_width(first.base_prefix, settings.tab_width);
			format!("{}{}", first.base_prefix, " ".repeat(hanging_indent))
		}
	};
	let continuation_width = display_width(&continuation_prefix, settings.tab_width);

	let mut output = Vec::new();
	let mut line = first_prefix.clone();
	let mut line_width = display_width(&first_prefix, settings.tab_width);
	let mut line_has_words = false;
	for piece in pieces(parts, settings.syntax) {
		match piece {
			Piece::Word(word) => {
				let word_width = display_width(word, settings.tab_width);
				if line_has_words && line_width + 1 + word_width > settings.max_line_width {
					output.push(trimmed_end(line));
					line = continuation_prefix.clone();
					line_width = continuation_width;
					line_has_words = false;
				}
				if line_has_words {
					line.push(' ');
					line_width += 1;
				}
				line.push_str(word);
				line_width += word_width;
				line_has_words = true;
			}
			Piece::HardBreak(suffix) => {
				output.push(trimmed_end(line) + suffix);
				line = continuation_prefix.clone();
				line_width = continuation_width;
				line_has_words = false;
			}
		}
	}
	if line_has_words || output.is_empty() {
		output.push(trimmed_end(line));
	}
	output
}

fn pieces<'a>(parts: &[LineParts<'a>], syntax: DocumentSyntax) -> Vec<Piece<'a>> {
	let mut pieces = Vec::new();
	for (index, part) in parts.iter().enumerate() {
		pieces.extend(part.text.split_whitespace().map(Piece::Word));
		if index + 1 == parts.len() {
			break;
		}
		// Trailing double spaces in code comments are too often accidental to honor them.
		if syntax == DocumentSyntax::Markdown && part.text.ends_with("  ") {
			pieces.push(Piece::HardBreak("  "));
		} else if part.text.trim_end().ends_with('\\') {
			pieces.push(Piece::HardBreak(""));
		}
	}
	pieces
}

fn trimmed_end(mut line: String) -> String {
	line.truncate(line.trim_end().len());
	line
}

fn parse_line(line: &str, syntax: DocumentSyntax) -> LineParts<'_> {
	let indentation_length = whitespace_length(line);
	let mut position = indentation_length;
	if syntax == DocumentSyntax::Code
		&& let Some(marker_length) =
			comment_marker_length(&line[position..], indentation_length > 0)
	{
		position += marker_length;
		position += whitespace_length(&line[position..]);
	}
	while line[position..].starts_with('>') {
		position += 1;
		position += whitespace_length(&line[position..]);
	}
	let list_marker_length = list_marker_length(&line[position..]);
	LineParts {
		base_prefix: &line[..position],
		list_marker: &line[position..position + list_marker_length],
		text: &line[position + list_marker_length..],
	}
}

fn whitespace_length(text: &str) -> usize {
	text.len() - text.trim_start_matches([' ', '\t']).len()
}

fn comment_marker_length(text: &str, is_indented: bool) -> Option<usize> {
	if let Some(marker) =
		["///", "//!", "//", "--"].into_iter().find(|marker| text.starts_with(marker))
	{
		return Some(marker.len());
	}
	for repeated in ['#', ';'] {
		let count = text.chars().take_while(|&character| character == repeated).count();
		if count > 0 {
			return Some(count);
		}
	}
	// These are common in prose, so they need a following blank. A lone `*` is only a block
	// comment continuation when indented (` * text`). At line start it is a Markdown bullet.
	["%", "'", "REM", "rem", "::", "*"].into_iter().find_map(|marker| {
		let rest = text.strip_prefix(marker)?;
		let is_followed_by_blank = rest.is_empty() || rest.starts_with([' ', '\t']);
		(is_followed_by_blank && (marker != "*" || is_indented)).then_some(marker.len())
	})
}

fn list_marker_length(text: &str) -> usize {
	let bullet_length = if text.starts_with(['-', '*', '+']) {
		1
	} else {
		let digits = text.bytes().take_while(u8::is_ascii_digit).count();
		match text.as_bytes().get(digits) {
			Some(b'.' | b')') if (1..=9).contains(&digits) => digits + 1,
			_ => return 0,
		}
	};
	let after_bullet = &text[bullet_length..];
	let blank_length = whitespace_length(after_bullet);
	if blank_length == 0 || after_bullet.trim().is_empty() {
		return 0;
	}
	let marker_length = bullet_length + blank_length;
	let after_marker = &text[marker_length..];
	match ["[ ]", "[x]", "[X]"].into_iter().find(|checkbox| after_marker.starts_with(checkbox)) {
		Some(checkbox) => {
			let blank_after_checkbox = whitespace_length(&after_marker[checkbox.len()..]);
			if blank_after_checkbox > 0 {
				marker_length + checkbox.len() + blank_after_checkbox
			} else {
				marker_length
			}
		}
		None => marker_length,
	}
}

fn opens_fence(text: &str) -> Option<Fence> {
	let text = text.trim_start();
	let character =
		text.chars().next().filter(|&character| character == '`' || character == '~')?;
	let length = text.chars().take_while(|&next| next == character).count();
	(length >= 3).then_some(Fence { character, length })
}

fn closes_fence(text: &str, fence: Fence) -> bool {
	let text = text.trim();
	text.chars().count() >= fence.length
		&& text.chars().all(|character| character == fence.character)
}

fn is_thematic_break_or_setext_underline(text: &str) -> bool {
	let marks: Vec<char> = text.chars().filter(|character| !character.is_whitespace()).collect();
	let Some(&first) = marks.first() else { return false };
	marks.iter().all(|&character| character == first)
		&& match first {
			'-' | '*' | '_' => marks.len() >= 3 || (first == '-' && !text.trim().contains(' ')),
			'=' => true,
			_ => false,
		}
}

fn is_heading(text: &str) -> bool {
	let level = text.chars().take_while(|&character| character == '#').count();
	(1..=6).contains(&level) && text[level..].chars().next().is_none_or(char::is_whitespace)
}

fn is_link_reference_definition(text: &str) -> bool {
	text.starts_with('[') && text.contains("]:")
}

/// A GitHub alert marker like `[!NOTE]`. It only starts an alert alone on its line.
fn is_alert_marker(text: &str) -> bool {
	let Some(kind) = text.trim().strip_prefix("[!").and_then(|rest| rest.strip_suffix(']')) else {
		return false;
	};
	["NOTE", "TIP", "IMPORTANT", "WARNING", "CAUTION"]
		.into_iter()
		.any(|alert_kind| alert_kind.eq_ignore_ascii_case(kind))
}

fn is_block_comment_delimiter_line(line: &str) -> bool {
	line.trim_start().starts_with("/*") || line.trim_end().ends_with("*/")
}

/// Markdown indented code: four columns beyond the enclosing blockquote or list item text.
fn is_indented_code(base_prefix: &str, list_text_column: Option<usize>) -> bool {
	let quote_text_column = base_prefix
		.rfind('>')
		.map_or(0, |position| display_width(&base_prefix[..=position], 1) + 1);
	let required_column = list_text_column.unwrap_or(0).max(quote_text_column) + 4;
	display_width(base_prefix, 4) >= required_column
}

#[cfg(test)]
mod tests {
	use super::*;

	fn settings(max_line_width: usize, syntax: DocumentSyntax) -> RewrapSettings {
		RewrapSettings { max_line_width, tab_width: 4, syntax }
	}

	fn rewrap_all(text: &str, max_line_width: usize, syntax: DocumentSyntax) -> String {
		let lines: Vec<&str> = text.lines().collect();
		rewrap_range(&lines, 0..lines.len(), &settings(max_line_width, syntax)).join("\n")
	}

	#[test]
	fn wraps_plain_paragraph() {
		assert_eq!(
			rewrap_all("one two three four five six", 13, DocumentSyntax::Markdown),
			"one two three\nfour five six"
		);
	}

	#[test]
	fn joins_short_lines() {
		assert_eq!(rewrap_all("one\ntwo\nthree", 80, DocumentSyntax::Markdown), "one two three");
	}

	#[test]
	fn keeps_line_comment_prefix() {
		assert_eq!(
			rewrap_all("\t// alpha beta gamma delta epsilon", 23, DocumentSyntax::Code),
			"\t// alpha beta gamma\n\t// delta epsilon"
		);
		assert_eq!(
			rewrap_all("# alpha beta\n# gamma delta", 30, DocumentSyntax::Code),
			"# alpha beta gamma delta"
		);
		assert_eq!(
			rewrap_all("/// alpha beta gamma", 14, DocumentSyntax::Code),
			"/// alpha beta\n/// gamma"
		);
	}

	#[test]
	fn keeps_block_comment_body_and_delimiters() {
		let text = "/**\n * alpha beta\n * gamma delta epsilon\n */";
		assert_eq!(
			rewrap_all(text, 17, DocumentSyntax::Code),
			"/**\n * alpha beta\n * gamma delta\n * epsilon\n */"
		);
	}

	#[test]
	fn separates_paragraphs_at_blank_comment_lines() {
		let text = "// a b\n//\n// c d";
		assert_eq!(rewrap_all(text, 80, DocumentSyntax::Code), text);
	}

	#[test]
	fn uses_hanging_indent_for_list_items() {
		assert_eq!(
			rewrap_all(
				"- alpha beta gamma delta\n- [ ] one two three four",
				14,
				DocumentSyntax::Markdown
			),
			"- alpha beta\n  gamma delta\n- [ ] one two\n      three\n      four"
		);
		assert_eq!(
			rewrap_all("12. alpha beta\n    gamma", 80, DocumentSyntax::Markdown),
			"12. alpha beta gamma"
		);
		assert_eq!(
			rewrap_all("// * alpha beta gamma", 14, DocumentSyntax::Code),
			"// * alpha\n//   beta\n//   gamma"
		);
	}

	#[test]
	fn keeps_nested_blockquotes() {
		assert_eq!(
			rewrap_all("> > alpha beta gamma", 14, DocumentSyntax::Markdown),
			"> > alpha beta\n> > gamma"
		);
	}

	#[test]
	fn keeps_alert_markers_on_their_own_line() {
		assert_eq!(
			rewrap_all("> [!NOTE]\n> alpha beta\n> gamma", 80, DocumentSyntax::Markdown),
			"> [!NOTE]\n> alpha beta gamma"
		);
		assert_eq!(
			rewrap_all("> > [!tip] \n> > alpha beta gamma", 14, DocumentSyntax::Markdown),
			"> > [!tip] \n> > alpha beta\n> > gamma"
		);
		// Outside of a quote, and unknown kinds, it is ordinary text.
		assert_eq!(
			rewrap_all("[!NOTE]\nalpha\n\n> [!OTHER]\n> beta", 80, DocumentSyntax::Markdown),
			"[!NOTE] alpha\n\n> [!OTHER] beta"
		);
		// Code comments are no Markdown documents.
		assert_eq!(
			rewrap_all("// > [!NOTE]\n// > alpha", 80, DocumentSyntax::Code),
			"// > [!NOTE] alpha"
		);
	}

	#[test]
	fn leaves_structures_with_significant_line_breaks_alone() {
		let text = "# Title\n\n```\nlet a = 1;\nlet b = 2;\n```\n\n| a | b |\n|---|---|\n| c | d |\n\n---\n\n    code\n    block";
		assert_eq!(rewrap_all(text, 5, DocumentSyntax::Markdown), text);
		let doc_comment = "/// ```\n/// let a = 1;\n/// ```";
		assert_eq!(rewrap_all(doc_comment, 5, DocumentSyntax::Code), doc_comment);
	}

	#[test]
	fn keeps_hard_line_breaks() {
		assert_eq!(
			rewrap_all("alpha  \nbeta gamma\\\ndelta", 80, DocumentSyntax::Markdown),
			"alpha  \nbeta gamma\\\ndelta"
		);
	}

	#[test]
	fn keeps_overlong_words_on_their_own_line() {
		assert_eq!(
			rewrap_all(
				"see https://example.com/a/very/long/path now",
				10,
				DocumentSyntax::Markdown
			),
			"see\nhttps://example.com/a/very/long/path\nnow"
		);
	}

	#[test]
	fn measures_display_width() {
		assert_eq!(rewrap_all("表表 表表 表表", 11, DocumentSyntax::Markdown), "表表 表表\n表表");
	}

	#[test]
	fn finds_paragraph_around_line() {
		let lines = ["intro", "", "// a", "// b", "code();", "```", "x", "```"];
		let settings = settings(80, DocumentSyntax::Code);
		assert_eq!(paragraph_at(&lines, 3, &settings), Some(2..4));
		assert_eq!(paragraph_at(&lines, 1, &settings), None);
		assert_eq!(paragraph_at(&lines, 6, &settings), None);
	}

	#[test]
	fn treats_lines_with_different_prefix_as_separate_paragraphs() {
		let text = "// a\n# b\nplain";
		assert_eq!(rewrap_all(text, 80, DocumentSyntax::Code), text);
	}

	#[test]
	fn treats_tab_and_space_indentation_alike() {
		assert_eq!(rewrap_all("- a\n\tb", 80, DocumentSyntax::Markdown), "- a\n\tb");
		assert_eq!(rewrap_all("1.  a\n\tb", 80, DocumentSyntax::Markdown), "1.  a b");
	}
}
