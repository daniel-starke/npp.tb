//! Realignment of GitHub Flavored Markdown tables.
//!
//! Cells are split at unescaped `|` only. Like GFM, a pipe inside a code span still separates
//! cells unless it is escaped as `\|`. Rows may be prefixed by indentation and blockquote markers,
//! which are kept.

use std::ops::Range;

use crate::text_width::display_width;

#[derive(Debug, PartialEq, Eq)]
pub struct RealignedTable {
	/// Rows of the input block that belong to the table and are replaced by `lines`.
	pub rows: Range<usize>,
	pub lines: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TableError {
	MissingDelimiterRow,
	CaretOutsideTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColumnAlignment {
	Unspecified,
	Left,
	Right,
	Center,
}

impl ColumnAlignment {
	fn colon_count(self) -> usize {
		match self {
			ColumnAlignment::Unspecified => 0,
			ColumnAlignment::Left | ColumnAlignment::Right => 1,
			ColumnAlignment::Center => 2,
		}
	}
}

#[derive(Debug)]
struct ParsedRow {
	/// Byte ranges of the trimmed cell contents within the whole line.
	cells: Vec<Range<usize>>,
	/// Byte positions of the pipes separating cells (excluding leading and trailing pipes).
	separators: Vec<usize>,
	has_leading_pipe: bool,
	has_trailing_pipe: bool,
}

/// Indentation and blockquote markers in front of a table row.
pub fn row_prefix(line: &str) -> &str {
	let content_start = line.find(|character| !matches!(character, ' ' | '\t' | '>'));
	&line[..content_start.unwrap_or(line.len())]
}

pub fn is_row_with_prefix(line: &str, prefix: &str) -> bool {
	line.strip_prefix(prefix).is_some_and(|content| {
		row_prefix(line) == prefix && !unescaped_pipe_positions(content).is_empty()
	})
}

/// A GFM delimiter row such as `|:----|----:|`, without any prefix.
pub fn is_delimiter_row(content: &str) -> bool {
	let row = parse_row(content, 0);
	let has_pipe = !row.separators.is_empty() || row.has_leading_pipe || row.has_trailing_pipe;
	has_pipe
		&& !row.cells.is_empty()
		&& row.cells.iter().all(|cell| is_delimiter_cell(&content[cell.clone()]))
}

/// Realigns the table containing `caret_row` within `block`, a run of consecutive table-like
/// lines sharing the same prefix. Lines before the header row are not part of the table.
pub fn realign_table(block: &[&str], caret_row: usize) -> Result<RealignedTable, TableError> {
	let prefix = row_prefix(block[caret_row]);
	let parsed_rows: Vec<ParsedRow> =
		block.iter().map(|line| parse_row(line, prefix.len())).collect();
	let delimiter_index = (1..block.len())
		.find(|&index| is_delimiter_row(&block[index][prefix.len()..]))
		.ok_or(TableError::MissingDelimiterRow)?;
	let header_index = delimiter_index - 1;
	if caret_row < header_index {
		return Err(TableError::CaretOutsideTable);
	}

	let table_lines = &block[header_index..];
	let table_rows = &parsed_rows[header_index..];
	let cell_text = |row: usize, column: usize| -> &str {
		table_rows[row].cells.get(column).map_or("", |range| &table_lines[row][range.clone()])
	};
	let column_count = table_rows.iter().map(|row| row.cells.len()).max().unwrap_or(1).max(1);
	let alignments: Vec<ColumnAlignment> =
		(0..column_count).map(|column| alignment_of(cell_text(1, column))).collect();
	// A row without outer pipes cannot represent an empty first or last cell, and a single
	// column table needs pipes to be recognized as a table at all.
	let needs_leading_pipe = table_rows[0].has_leading_pipe
		|| column_count == 1
		|| (0..table_rows.len()).any(|row| cell_text(row, 0).is_empty());
	let needs_trailing_pipe = table_rows[0].has_trailing_pipe
		|| column_count == 1
		|| (0..table_rows.len()).any(|row| cell_text(row, column_count - 1).is_empty());

	// Cells are padded by one space next to each pipe. Delimiter cells fill that padding with
	// dashes too (`|:---|`), so they must be long enough for their colons plus one dash.
	let padding_of = |column: usize| {
		usize::from(column > 0 || needs_leading_pipe)
			+ usize::from(column + 1 < column_count || needs_trailing_pipe)
	};
	let widths: Vec<usize> = (0..column_count)
		.map(|column| {
			let delimiter_minimum = alignments[column].colon_count() + 1;
			(0..table_rows.len())
				.filter(|&row| row != 1)
				.map(|row| display_width(cell_text(row, column), 1))
				.fold(delimiter_minimum.saturating_sub(padding_of(column)).max(1), usize::max)
		})
		.collect();

	let lines = (0..table_rows.len())
		.map(|row| {
			let cells: Vec<String> = (0..column_count)
				.map(|column| {
					let has_left_space = column > 0 || needs_leading_pipe;
					let has_right_space = column + 1 < column_count || needs_trailing_pipe;
					if row == 1 {
						delimiter_cell(alignments[column], widths[column] + padding_of(column))
					} else {
						let text =
							pad_cell(cell_text(row, column), widths[column], alignments[column]);
						format!(
							"{}{text}{}",
							if has_left_space { " " } else { "" },
							if has_right_space { " " } else { "" }
						)
					}
				})
				.collect();
			join_cells(prefix, &cells, needs_leading_pipe, needs_trailing_pipe)
		})
		.collect();
	Ok(RealignedTable { rows: header_index..block.len(), lines })
}

/// Maps a caret (character offset) in an original row to the same cell and offset within the
/// cell content of the realigned row.
pub fn caret_column_after_realign(old_line: &str, old_column: usize, new_line: &str) -> usize {
	let prefix_length = row_prefix(old_line).len();
	let old_row = parse_row(old_line, prefix_length);
	let new_row = parse_row(new_line, row_prefix(new_line).len());
	let caret_byte =
		old_line.char_indices().nth(old_column).map_or(old_line.len(), |(byte, _)| byte);
	let cell_index = old_row.separators.iter().filter(|&&separator| separator < caret_byte).count();
	let (Some(old_cell), Some(new_cell)) =
		(old_row.cells.get(cell_index), new_row.cells.get(cell_index))
	else {
		return old_column.min(new_line.chars().count());
	};
	let offset_in_cell = old_line[old_cell.clone()]
		.char_indices()
		.take_while(|&(byte, _)| old_cell.start + byte < caret_byte)
		.count();
	let new_cell_start = new_line[..new_cell.start].chars().count();
	new_cell_start + offset_in_cell.min(new_line[new_cell.clone()].chars().count())
}

fn parse_row(line: &str, prefix_length: usize) -> ParsedRow {
	let content = &line[prefix_length..];
	let trimmed_start = content.len() - content.trim_start().len();
	let trimmed_end = content.trim_end().len();
	let mut pipes = unescaped_pipe_positions(content);
	let has_leading_pipe = pipes.first() == Some(&trimmed_start);
	let has_trailing_pipe = trimmed_end > 0
		&& pipes.last() == Some(&(trimmed_end - 1))
		&& (pipes.len() > 1 || !has_leading_pipe);
	if has_trailing_pipe {
		pipes.pop();
	}
	if has_leading_pipe {
		pipes.remove(0);
	}
	let cells_start = trimmed_start + usize::from(has_leading_pipe);
	let cells_end = (trimmed_end - usize::from(has_trailing_pipe)).max(cells_start);
	let boundaries: Vec<usize> =
		std::iter::once(cells_start).chain(pipes.iter().map(|&pipe| pipe + 1)).collect();
	let ends = pipes.iter().copied().chain(std::iter::once(cells_end));
	let cells = boundaries
		.iter()
		.zip(ends)
		.map(|(&start, end)| {
			let segment = &content[start..end];
			let content_start = start + (segment.len() - segment.trim_start().len());
			let content_end = (start + segment.trim_end().len()).max(content_start);
			prefix_length + content_start..prefix_length + content_end
		})
		.collect();
	ParsedRow {
		cells,
		separators: pipes.iter().map(|&pipe| prefix_length + pipe).collect(),
		has_leading_pipe,
		has_trailing_pipe,
	}
}

fn unescaped_pipe_positions(content: &str) -> Vec<usize> {
	let mut positions = Vec::new();
	let mut preceding_backslashes = 0;
	for (byte, character) in content.char_indices() {
		if character == '|' && preceding_backslashes % 2 == 0 {
			positions.push(byte);
		}
		preceding_backslashes = if character == '\\' { preceding_backslashes + 1 } else { 0 };
	}
	positions
}

fn is_delimiter_cell(cell: &str) -> bool {
	let dashes = cell.strip_prefix(':').unwrap_or(cell);
	let dashes = dashes.strip_suffix(':').unwrap_or(dashes);
	!dashes.is_empty() && dashes.bytes().all(|byte| byte == b'-')
}

fn alignment_of(delimiter_cell: &str) -> ColumnAlignment {
	match (
		delimiter_cell.starts_with(':'),
		delimiter_cell.len() > 1 && delimiter_cell.ends_with(':'),
	) {
		(true, true) => ColumnAlignment::Center,
		(true, false) => ColumnAlignment::Left,
		(false, true) => ColumnAlignment::Right,
		(false, false) => ColumnAlignment::Unspecified,
	}
}

/// `length` includes the padding spaces of data cells, which delimiter cells fill with dashes.
fn delimiter_cell(alignment: ColumnAlignment, length: usize) -> String {
	let (left, right) = match alignment {
		ColumnAlignment::Unspecified => ("", ""),
		ColumnAlignment::Left => (":", ""),
		ColumnAlignment::Right => ("", ":"),
		ColumnAlignment::Center => (":", ":"),
	};
	format!("{left}{}{right}", "-".repeat(length - left.len() - right.len()))
}

fn pad_cell(text: &str, width: usize, alignment: ColumnAlignment) -> String {
	let padding = width.saturating_sub(display_width(text, 1));
	let left_padding = match alignment {
		ColumnAlignment::Unspecified | ColumnAlignment::Left => 0,
		ColumnAlignment::Right => padding,
		ColumnAlignment::Center => padding / 2,
	};
	format!("{}{text}{}", " ".repeat(left_padding), " ".repeat(padding - left_padding))
}

/// `cells` already contain their padding spaces.
fn join_cells(prefix: &str, cells: &[String], leading_pipe: bool, trailing_pipe: bool) -> String {
	let mut line = String::from(prefix);
	if leading_pipe {
		line.push('|');
	}
	line.push_str(&cells.join("|"));
	if trailing_pipe {
		line.push('|');
	}
	line.truncate(line.trim_end().len());
	line
}

#[cfg(test)]
mod tests {
	use super::*;

	fn realign(block: &[&str]) -> Vec<String> {
		realign_table(block, block.len() - 1).unwrap().lines
	}

	#[test]
	fn pads_columns_and_honors_alignment_markers() {
		let lines = realign(&[
			"| Name | Qty | Note |",
			"|:-|-:|:-:|",
			"| apple | 3 | ok |",
			"| kiwi | 12 | fine |",
		]);
		assert_eq!(
			lines,
			[
				"| Name  | Qty | Note |",
				"|:------|----:|:----:|",
				"| apple |   3 |  ok  |",
				"| kiwi  |  12 | fine |",
			]
		);
	}

	#[test]
	fn keeps_tables_without_outer_pipes_and_trims_last_cell() {
		let lines = realign(&["a | bb", "--|--", "ccc | d"]);
		assert_eq!(lines, ["a   | bb", "----|---", "ccc | d"]);
	}

	#[test]
	fn adds_missing_cells_and_trailing_pipe_when_needed() {
		let lines = realign(&["a | b | c", "---|---|---", "x | y"]);
		assert_eq!(lines, ["a | b | c |", "--|---|---|", "x | y |   |"]);
	}

	#[test]
	fn keeps_escaped_pipes_and_measures_wide_characters() {
		let lines = realign(&["| 表 | a \\| b |", "|---|---|", "| x | y |"]);
		assert_eq!(lines, ["| 表 | a \\| b |", "|----|--------|", "| x  | y      |"]);
	}

	#[test]
	fn keeps_blockquote_and_indentation_prefix() {
		let lines = realign(&["> | a | b |", "> |-|-|", "> | long | x |"]);
		assert_eq!(lines, ["> | a    | b |", "> |------|---|", "> | long | x |"]);
	}

	#[test]
	fn skips_lines_above_the_header_row() {
		let block = ["intro | text", "| a | b |", "|---|---|", "| c | d |"];
		let table = realign_table(&block, 3).unwrap();
		assert_eq!(table.rows, 1..4);
		assert_eq!(realign_table(&block, 0), Err(TableError::CaretOutsideTable));
	}

	#[test]
	fn adds_leading_pipe_for_single_column() {
		assert_eq!(realign(&["a |", "-|", "bcde |"]), ["| a    |", "|------|", "| bcde |"]);
	}

	#[test]
	fn rejects_blocks_without_delimiter_row() {
		assert_eq!(
			realign_table(&["| a | b |", "| c | d |"], 0),
			Err(TableError::MissingDelimiterRow)
		);
	}

	#[test]
	fn detects_table_rows_and_delimiter_rows() {
		assert!(is_row_with_prefix("  | a |", "  "));
		assert!(!is_row_with_prefix("  | a |", ""));
		assert!(!is_row_with_prefix("no pipe", ""));
		assert!(is_delimiter_row("| :--- | ---: |"));
		assert!(is_delimiter_row("--|--"));
		assert!(!is_delimiter_row("| a | --- |"));
		assert!(!is_delimiter_row("---"));
	}

	#[test]
	fn keeps_single_character_columns_narrow() {
		assert_eq!(realign(&["| a |", "| - |", "| b |"]), ["| a |", "|---|", "| b |"]);
		assert_eq!(realign(&["a|b", ":-:|:-:", "c|d"]), ["a  | b", ":-:|:-:", "c  | d"]);
	}

	#[test]
	fn maps_caret_into_same_cell() {
		let old_line = "|a|bc|";
		let new_line = "| a   | bc  |";
		// Caret between "b" and "c".
		assert_eq!(caret_column_after_realign(old_line, 4, new_line), 9);
		// Caret right after the leading pipe.
		assert_eq!(caret_column_after_realign(old_line, 1, new_line), 2);
	}
}
