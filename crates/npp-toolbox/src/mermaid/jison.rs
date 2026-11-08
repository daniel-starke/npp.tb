//! Helpers of the hand-written lexers that follow jison grammars: jison tries the rules in order
//! and takes the first match, ignoring ASCII case with `case-insensitive`; rules whose pattern
//! ends with a word character end at a word boundary.

pub fn is_word_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `keyword` at the start of `text`, ignoring ASCII case.
pub fn starts_with_ignoring_case(text: &str, keyword: &str) -> bool {
	text.get(..keyword.len()).is_some_and(|start| start.eq_ignore_ascii_case(keyword))
}

/// A literal keyword rule: the keyword, ignoring case, followed by a word boundary.
pub fn keyword(text: &str, keyword: &str) -> bool {
	starts_with_ignoring_case(text, keyword)
		&& !text.as_bytes().get(keyword.len()).copied().is_some_and(is_word_byte)
}

pub fn span(text: &str, predicate: impl Fn(char) -> bool) -> usize {
	text.find(|character| !predicate(character)).unwrap_or(text.len())
}

/// `keyword\s+`: the length with the whitespace, `None` without whitespace.
pub fn keyword_and_space(text: &str, keyword: &str) -> Option<usize> {
	if !starts_with_ignoring_case(text, keyword) {
		return None;
	}
	let spaces = span(&text[keyword.len()..], char::is_whitespace);
	(spaces > 0).then_some(keyword.len() + spaces)
}

/// `keyword\s*"<delimiter>"\s*`: the length of the match.
pub fn keyword_and_delimiter(text: &str, keyword: &str, delimiter: char) -> Option<usize> {
	if !starts_with_ignoring_case(text, keyword) {
		return None;
	}
	let mut length = keyword.len();
	length += span(&text[length..], char::is_whitespace);
	if !text[length..].starts_with(delimiter) {
		return None;
	}
	length += 1;
	Some(length + span(&text[length..], char::is_whitespace))
}

/// `((?!\n)\s)+`
pub fn blank_length(text: &str) -> usize {
	span(text, |character| character.is_whitespace() && character != '\n')
}

pub fn line_length(text: &str) -> usize {
	span(text, |character| character != '\n')
}

/// `\w+`
pub fn word_length(text: &str) -> usize {
	text.bytes().take_while(|&byte| is_word_byte(byte)).count()
}

/// `.*direction\s+<dir>[^\n]*`: the length of the match for `dir`.
pub fn direction_length(text: &str, dir: &str) -> Option<usize> {
	let line = line_length(text);
	let lower = text[..line].to_ascii_lowercase();
	for (start, _) in lower.match_indices("direction") {
		let after = start + "direction".len();
		let spaces = span(&text[after..], char::is_whitespace);
		let value = after + spaces;
		if spaces > 0 && starts_with_ignoring_case(&text[value..], dir) {
			let end = value + dir.len();
			return Some(end + line_length(&text[end..]));
		}
	}
	None
}
