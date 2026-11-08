//! Finds the comments of a document in the styles Notepad++'s lexer gave to its bytes.

use std::ops::Range;

use crate::code_highlight::{spans_of_styles, token_kind_of_style};
use crate::markdown_html::TokenKind;

/// The Lexilla name of the lexer of Notepad++'s user defined languages.
const USER_LEXER: &str = "user";
/// `SCE_USER_STYLE_COMMENT` and `SCE_USER_STYLE_COMMENTLINE` of Lexilla's `SciLexer.h`.
const USER_LEXER_COMMENT_STYLES: [u8; 2] = [1, 2];

/// Whether `style` of the lexer `lexer` is a comment: by its name in `stylers.xml`, else by
/// the tags the lexer reports for it. The user defined languages have fixed comment styles.
pub fn is_comment_style(
	lexer: &str,
	style: u8,
	styler_name: Option<&str>,
	tags: impl FnOnce() -> String,
) -> bool {
	if lexer == USER_LEXER {
		return USER_LEXER_COMMENT_STYLES.contains(&style);
	}
	let kind = styler_name.and_then(token_kind_of_style).or_else(|| token_kind_of_style(&tags()));
	kind == Some(TokenKind::Comment)
}

/// Byte ranges of the comments in `styles`. Adjacent comment styles, such as a documentation
/// comment and the keywords inside it, form one range.
pub fn comment_ranges(styles: &[u8], mut is_comment: impl FnMut(u8) -> bool) -> Vec<Range<usize>> {
	spans_of_styles(styles, |style| is_comment(style).then_some(TokenKind::Comment))
		.into_iter()
		.map(|span| span.range)
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn merges_adjacent_comment_styles() {
		// 0 code, 1 block comment, 2 line comment, 3 comment keyword
		let styles = [0, 1, 1, 3, 1, 0, 0, 2, 2, 0, 2];
		let ranges = comment_ranges(&styles, |style| style != 0);
		assert_eq!(ranges, [1..5, 7..9, 10..11]);
		assert_eq!(comment_ranges(&styles, |_| false), []);
		assert_eq!(comment_ranges(&[], |_| true), []);
	}

	#[test]
	fn recognizes_comments_by_styler_name_then_tags() {
		let no_tags = String::new;
		assert!(is_comment_style("cpp", 2, Some("COMMENT LINE"), no_tags));
		assert!(is_comment_style("cpp", 15, Some("COMMENT DOC KEYWORD"), no_tags));
		assert!(!is_comment_style("cpp", 6, Some("STRING"), || "comment".to_owned()));
		assert!(is_comment_style("hypertext", 42, None, || "comment".to_owned()));
		assert!(is_comment_style("sql", 1, Some("unknown"), || "comment line".to_owned()));
		assert!(!is_comment_style("props", 0, Some("DEFAULT"), no_tags));
		assert!(!is_comment_style("yaml", 6, Some("DOCUMENT"), no_tags));
	}

	#[test]
	fn uses_fixed_comment_styles_of_user_defined_languages() {
		let tags = || "comment".to_owned();
		assert!(is_comment_style(USER_LEXER, 1, None, tags));
		assert!(is_comment_style(USER_LEXER, 2, Some("DEFAULT"), tags));
		assert!(!is_comment_style(USER_LEXER, 3, Some("COMMENTS"), tags));
	}
}
