//! A scanner for the Langium grammars of Mermaid (`packages/parser/src/language`): the terminals
//! of `common.langium` and Langium's value conversion.
//!
//! Langium lexes with chevrotain before it parses. The diagram parsers ask the scanner for the
//! token they expect next instead, which accepts the same documents.

use super::common::CommonFields;
use super::{Context, Error};

pub struct Scanner<'a> {
	text: &'a str,
	position: usize,
	line: usize,
}

impl<'a> Scanner<'a> {
	pub fn new(text: &'a str) -> Self {
		Self { text, position: 0, line: 1 }
	}

	pub fn rest(&self) -> &'a str {
		&self.text[self.position..]
	}

	/// The one-based line of the scanner position.
	pub fn line(&self) -> usize {
		self.line
	}

	fn advance(&mut self, length: usize) -> &'a str {
		let taken = &self.text[self.position..self.position + length];
		self.line += taken.matches('\n').count();
		self.position += length;
		taken
	}

	/// Skips the hidden terminals `WHITESPACE` and `SINGLE_LINE_COMMENT`.
	pub fn skip_hidden(&mut self) {
		loop {
			let rest = self.rest();
			let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
			self.advance(blanks);
			if self.rest().starts_with("%%") {
				let length = self.rest().find(['\n', '\r']).unwrap_or(self.rest().len());
				self.advance(length);
				continue;
			}
			break;
		}
	}

	pub fn is_at_end(&mut self) -> bool {
		self.skip_hidden();
		self.position == self.text.len()
	}

	/// `NEWLINE`: `\r?\n`.
	pub fn newline(&mut self) -> bool {
		self.skip_hidden();
		let length = match self.rest() {
			rest if rest.starts_with("\r\n") => 2,
			rest if rest.starts_with('\n') => 1,
			_ => return false,
		};
		self.advance(length);
		true
	}

	pub fn skip_newlines(&mut self) {
		while self.newline() {}
	}

	/// `EOL`: `NEWLINE+ | EOF`.
	pub fn end_of_line(&mut self) -> bool {
		if self.is_at_end() {
			return true;
		}
		let found = self.newline();
		self.skip_newlines();
		found
	}

	pub fn punctuation(&mut self, token: &str) -> bool {
		self.skip_hidden();
		if self.rest().starts_with(token) {
			self.advance(token.len());
			return true;
		}
		false
	}

	/// A keyword, which Langium does not take when the `ID` terminal matches more, or, for the
	/// keywords the token builder restricts, when anything but whitespace or a comment follows.
	pub fn keyword(&mut self, keyword: &str, is_restricted: bool) -> bool {
		self.skip_hidden();
		let Some(after) = self.rest().strip_prefix(keyword) else { return false };
		let continues_id = after.starts_with(is_word_character)
			|| (after.starts_with('-') && after[1..].starts_with(is_word_character));
		let is_followed_by_text = !(after.is_empty()
			|| after.starts_with("%%")
			|| after.starts_with(char::is_whitespace));
		if (continues_id && keyword.ends_with(is_word_character))
			|| (is_restricted && is_followed_by_text)
		{
			return false;
		}
		self.advance(keyword.len());
		true
	}

	/// `INT`: `0|[1-9][0-9]*(?!\.)`.
	pub fn int(&mut self) -> Option<f64> {
		self.skip_hidden();
		let length = int_length(self.rest())?;
		self.advance(length).parse().ok()
	}

	/// `NUMBER`: `FLOAT | INT`, with `FLOAT` `[0-9]+\.[0-9]+(?!\.)`.
	pub fn number(&mut self) -> Option<f64> {
		self.skip_hidden();
		match float_length(self.rest()) {
			Some(length) => self.advance(length).parse().ok(),
			None => self.int(),
		}
	}

	/// `STRING`: `"([^"\\]|\\.)*"|'([^'\\]|\\.)*'`, with Langium's escapes.
	pub fn string(&mut self) -> Option<String> {
		self.skip_hidden();
		let rest = self.rest();
		let quote = rest.chars().next().filter(|&quote| quote == '"' || quote == '\'')?;
		let mut value = String::new();
		let mut characters = rest.char_indices().skip(1);
		while let Some((index, character)) = characters.next() {
			match character {
				'\\' => {
					// `.` does not match line ends.
					let (_, escaped) =
						characters.next().filter(|(_, c)| *c != '\n' && *c != '\r')?;
					value.push(match escaped {
						'b' => '\u{8}',
						'f' => '\u{c}',
						'n' => '\n',
						'r' => '\r',
						't' => '\t',
						'v' => '\u{b}',
						'0' => '\0',
						other => other,
					});
				}
				_ if character == quote => {
					self.advance(index + 1);
					return Some(value);
				}
				_ => value.push(character),
			}
		}
		None
	}

	/// `ID`: `[\w]([-\w]*\w)?`, unless chevrotain takes another token: a keyword of the same
	/// length, or a terminal that comes first and matches a prefix, like `BOOLEAN` in `trueX`
	/// or the grammar's own literal `terminals`.
	pub fn id(&mut self, keywords: &[&str], terminals: &[&str]) -> Option<&'a str> {
		self.id_among(keywords, terminals, true)
	}

	/// `ID` of a grammar that uses neither `BOOLEAN` nor the number terminals, which Langium then
	/// leaves out of the lexer: `1a` and `trueX` are ids there.
	pub fn id_without_numbers(&mut self, keywords: &[&str], terminals: &[&str]) -> Option<&'a str> {
		self.id_among(keywords, terminals, false)
	}

	fn id_among(
		&mut self,
		keywords: &[&str],
		terminals: &[&str],
		has_numbers: bool,
	) -> Option<&'a str> {
		self.skip_hidden();
		let rest = self.rest();
		let mut length = 0;
		for (index, character) in rest.char_indices() {
			if is_word_character(character) {
				length = index + character.len_utf8();
			} else if character != '-' || index == 0 {
				break;
			}
		}
		let literals: &[&str] = if has_numbers { &["true", "false", "title"] } else { &["title"] };
		let is_other_token = keywords.contains(&&rest[..length])
			|| terminals.iter().chain(literals).any(|t| rest.starts_with(t))
			|| (has_numbers && (int_length(rest).is_some() || float_length(rest).is_some()))
			|| starts_accessibility(rest);
		(length > 0 && !is_other_token).then(|| self.advance(length))
	}

	/// The `TITLE`, `ACC_TITLE` and `ACC_DESCR` terminals of `TitleAndAccessibilities`.
	pub fn common_field(&mut self, fields: &mut CommonFields) -> bool {
		self.skip_hidden();
		let rest = self.rest();
		let until_comment_or_line_end = |text: &str| {
			let line_end = text.find(['\n', '\r']).unwrap_or(text.len());
			line_end.min(text.find("%%").unwrap_or(text.len()))
		};
		if let Some(after) = rest.strip_prefix("accTitle") {
			let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
			if let Some(value) = after[blanks..].strip_prefix(':') {
				let length = until_comment_or_line_end(value);
				fields.accessibility.title = single_line_value(&value[..length]);
				self.advance("accTitle".len() + blanks + 1 + length);
				return true;
			}
			return false;
		}
		if let Some(after) = rest.strip_prefix("accDescr") {
			let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
			if let Some(value) = after[blanks..].strip_prefix(':') {
				let length = until_comment_or_line_end(value);
				fields.accessibility.description = single_line_value(&value[..length]);
				self.advance("accDescr".len() + blanks + 1 + length);
				return true;
			}
			let spaces = after.len() - after.trim_start().len();
			if let Some(block) = after[spaces..].strip_prefix('{')
				&& let Some(end) = block.find('}')
			{
				fields.accessibility.description = multi_line_value(&block[..end]);
				self.advance("accDescr".len() + spaces + 1 + end + 1);
				return true;
			}
			return false;
		}
		if let Some(after) = rest.strip_prefix("title") {
			let length = match after.starts_with([' ', '\t']) {
				true => until_comment_or_line_end(after),
				false => 0,
			};
			fields.title = Some(single_line_value(&after[..length]));
			self.advance("title".len() + length);
			return true;
		}
		false
	}

	/// The next token for error messages.
	pub fn found(&mut self) -> String {
		self.skip_hidden();
		let rest = self.rest();
		if rest.is_empty() {
			return "the end of the diagram".to_owned();
		}
		if rest.starts_with(['\n', '\r']) {
			return "the end of the line".to_owned();
		}
		let token: String = rest.chars().take_while(|c| !c.is_whitespace()).take(20).collect();
		format!("{token:?}")
	}

	/// An error at the scanner's line: `expected` and the token found.
	pub fn error(&mut self, context: &Context, expected: &str) -> Error {
		let found = self.found();
		context.error_at(self.line, format!("expected {expected}, found {found}"))
	}
}

fn is_word_character(character: char) -> bool {
	character.is_ascii_alphanumeric() || character == '_'
}

/// Length of `0|[1-9][0-9]*(?!\.)` at the start of `text`.
fn int_length(text: &str) -> Option<usize> {
	let digits = text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
	let length = match text.as_bytes().first()? {
		b'0' => 1,
		b'1'..=b'9' => digits,
		_ => return None,
	};
	// The lookahead fails for the whole match: the regex cannot backtrack into a shorter one
	// that is followed by a digit instead.
	(!text[length..].starts_with('.')).then_some(length)
}

/// Length of `[0-9]+\.[0-9]+(?!\.)` at the start of `text`, with the regex's backtracking.
fn float_length(text: &str) -> Option<usize> {
	let digits = text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len();
	let after = text.get(digits..)?.strip_prefix('.')?;
	let fraction = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
	if digits == 0 || fraction == 0 {
		return None;
	}
	// A shorter fraction is followed by a digit, so only a dot after the whole one fails.
	let fraction = match after[fraction..].starts_with('.') {
		true if fraction > 1 => fraction - 1,
		true => return None,
		false => fraction,
	};
	Some(digits + 1 + fraction)
}

/// Whether `ACC_TITLE` or `ACC_DESCR` matches at the start of `text`.
fn starts_accessibility(text: &str) -> bool {
	let after_blanks = |text: &str| text.trim_start_matches([' ', '\t']).to_owned();
	if let Some(after) = text.strip_prefix("accTitle") {
		return after_blanks(after).starts_with(':');
	}
	if let Some(after) = text.strip_prefix("accDescr") {
		let after = after_blanks(after);
		return after.starts_with(':')
			|| after.trim_start().strip_prefix('{').is_some_and(|block| block.contains('}'));
	}
	false
}

/// A single line `title`, `accTitle` or `accDescr`: trimmed, runs of blanks as one space.
fn single_line_value(text: &str) -> String {
	let mut value = String::with_capacity(text.len());
	let mut blanks = 0;
	for character in text.trim().chars() {
		if character == ' ' || character == '\t' {
			blanks += 1;
			continue;
		}
		if blanks > 0 {
			value.push(' ');
			blanks = 0;
		}
		value.push(character);
	}
	value
}

/// A multi-line `accDescr { … }`: lines without surrounding whitespace, no empty lines.
fn multi_line_value(text: &str) -> String {
	let lines: Vec<String> = text.lines().map(single_line_value).collect();
	let mut value = lines.join("\n");
	while value.contains("\n\n") {
		value = value.replace("\n\n", "\n");
	}
	value.trim_matches('\n').to_owned()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn scans_langium_terminals() {
		let mut scanner =
			Scanner::new("  12 0 01 3.5 7. \"a\\nb\" 'c\"' x-y- %% note\n\ttitle  A   b %% c\n");
		assert_eq!(scanner.int(), Some(12.0));
		assert_eq!(scanner.int(), Some(0.0));
		// `0` then `1`, like the regex.
		assert_eq!(scanner.int(), Some(0.0));
		assert_eq!(scanner.int(), Some(1.0));
		assert_eq!(scanner.int(), None);
		assert_eq!(scanner.number(), Some(3.5));
		assert_eq!(scanner.number(), None);
		assert_eq!(scanner.id(&[], &[]), Some("7"));
		assert!(scanner.punctuation("."));
		assert_eq!(scanner.string().as_deref(), Some("a\nb"));
		assert_eq!(scanner.string().as_deref(), Some("c\""));
		assert_eq!(scanner.id(&[], &[]), Some("x-y"));
		assert!(scanner.punctuation("-"));
		assert!(scanner.end_of_line());
		let mut fields = CommonFields::default();
		assert!(scanner.common_field(&mut fields));
		assert_eq!(fields.title.as_deref(), Some("A b"));
		assert_eq!(scanner.line(), 2);
		assert!(scanner.end_of_line());
		assert!(scanner.is_at_end());
	}

	#[test]
	fn scans_ids_like_chevrotain() {
		for (text, expected) in [
			("axisX", Some("axisX")),
			("axis", None),
			("circleA", None),
			("trueX", None),
			("titles", None),
			("2D", None),
			("1.x", Some("1")),
			("a-b_c", Some("a-b_c")),
			("-a", None),
		] {
			let id = Scanner::new(text).id(&["axis"], &["circle"]).map(str::to_owned);
			assert_eq!(id.as_deref(), expected, "{text}");
		}
		assert_eq!(Scanner::new("1.55.").number(), Some(1.5));
	}

	#[test]
	fn scans_keywords_and_accessibility() {
		let mut scanner = Scanner::new("packet-beta x");
		assert!(!scanner.keyword("packet", true));
		assert!(scanner.keyword("packet-beta", false));
		let mut scanner = Scanner::new("packetx");
		assert!(!scanner.keyword("packet", false));
		let mut scanner = Scanner::new("accDescr {\n  one  two\n\n three }\naccTitle : t");
		let mut fields = CommonFields::default();
		assert!(scanner.common_field(&mut fields));
		assert_eq!(fields.accessibility.description, "one two\nthree");
		assert_eq!(scanner.line(), 4);
		assert!(scanner.newline());
		assert!(scanner.common_field(&mut fields));
		assert_eq!(fields.accessibility.title, "t");
	}
}
