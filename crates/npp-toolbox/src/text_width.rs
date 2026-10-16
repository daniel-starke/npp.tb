//! Display width of text as it appears in a monospaced editor.

use unicode_width::UnicodeWidthChar;

/// Column reached after writing `text` starting at `start_column`. East Asian wide characters
/// take two columns, combining marks none, tabs advance to the next multiple of `tab_width`.
pub fn column_after(text: &str, start_column: usize, tab_width: usize) -> usize {
	let tab_width = tab_width.max(1);
	text.chars().fold(start_column, |column, character| match character {
		'\t' => (column / tab_width + 1) * tab_width,
		_ => column + character.width().unwrap_or(0),
	})
}

pub fn display_width(text: &str, tab_width: usize) -> usize {
	column_after(text, 0, tab_width)
}

/// Replaces tabs by spaces so that strings with mixed indentation styles become comparable.
pub fn expand_tabs(text: &str, tab_width: usize) -> String {
	let mut expanded = String::with_capacity(text.len());
	let mut column = 0;
	for character in text.chars() {
		let next_column = column_after(character.encode_utf8(&mut [0; 4]), column, tab_width);
		if character == '\t' {
			expanded.extend(std::iter::repeat_n(' ', next_column - column));
		} else {
			expanded.push(character);
		}
		column = next_column;
	}
	expanded
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn counts_wide_and_combining_characters() {
		assert_eq!(display_width("abc", 4), 3);
		assert_eq!(display_width("表格", 4), 4);
		assert_eq!(display_width("e\u{301}", 4), 1);
	}

	#[test]
	fn advances_tabs_to_next_tab_stop() {
		assert_eq!(display_width("\t", 4), 4);
		assert_eq!(display_width("ab\tc", 4), 5);
		assert_eq!(column_after("\t", 3, 4), 4);
	}

	#[test]
	fn expands_tabs_relative_to_column() {
		assert_eq!(expand_tabs("a\tb", 4), "a   b");
		assert_eq!(expand_tabs("//\t", 4), "//  ");
	}
}
