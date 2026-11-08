//! Parsing helpers shared by the diagram grammars: a line cursor and the `title`, `accTitle`
//! and `accDescr` statements of Mermaid's common grammar.

use std::cmp::Ordering;

use super::svg::Accessibility;
use super::{Context, Error, Result};

/// The lines of diagram code with their one-based numbers.
pub struct Lines<'a> {
	lines: Vec<&'a str>,
	index: usize,
}

impl<'a> Lines<'a> {
	pub fn new(code: &'a str) -> Self {
		Self { lines: code.split('\n').collect(), index: 0 }
	}

	/// The next line and its number.
	pub fn next_line(&mut self) -> Option<(usize, &'a str)> {
		let line = *self.lines.get(self.index)?;
		self.index += 1;
		Some((self.index, line))
	}

	/// Number of the line last returned.
	pub fn line_number(&self) -> usize {
		self.index
	}
}

/// `text` without a `%%` comment, which Mermaid's grammars hide; `%%` in quotes is kept.
pub fn strip_comment(text: &str) -> &str {
	let mut quote = None;
	let bytes = text.as_bytes();
	for (index, &byte) in bytes.iter().enumerate() {
		match quote {
			Some(open) if byte == open => quote = None,
			Some(_) => {}
			None if byte == b'"' || byte == b'\'' => quote = Some(byte),
			None if byte == b'%' && bytes.get(index + 1) == Some(&b'%') => return &text[..index],
			None => {}
		}
	}
	text
}

/// Whether `text` starts with `keyword` followed by a non-word character or the end.
pub fn starts_with_word(text: &str, keyword: &str) -> bool {
	text.starts_with(keyword)
		&& text[keyword.len()..]
			.chars()
			.next()
			.is_none_or(|next| !(next.is_alphanumeric() || next == '_'))
}

/// `@braintree/sanitize-url`: scripts and data become `about:blank`.
pub fn sanitize_url(url: &str) -> String {
	let compact: String = url
		.chars()
		.filter(|c| !c.is_whitespace() && !c.is_control())
		.collect::<String>()
		.to_ascii_lowercase();
	let is_dangerous =
		["javascript:", "data:", "vbscript:"].iter().any(|scheme| compact.starts_with(scheme));
	if is_dangerous { "about:blank".to_owned() } else { url.to_owned() }
}

/// `parseGenericTypes`: `~` pairs of a comma separated part become angle brackets.
pub fn parse_generic_types(input: &str) -> String {
	fn process_set(input: &str) -> String {
		let count = input.matches('~').count();
		if count <= 1 {
			return input.to_owned();
		}
		let (input, has_starting_tilde) = match !count.is_multiple_of(2) && input.starts_with('~') {
			true => (&input[1..], true),
			false => (input, false),
		};
		let mut characters: Vec<char> = input.chars().collect();
		loop {
			let first = characters.iter().position(|&c| c == '~');
			let last = characters.iter().rposition(|&c| c == '~');
			match (first, last) {
				(Some(first), Some(last)) if first != last => {
					characters[first] = '<';
					characters[last] = '>';
				}
				_ => break,
			}
		}
		let text: String = characters.into_iter().collect();
		if has_starting_tilde { format!("~{text}") } else { text }
	}
	// `input.split(/(,)/)`: the parts with the commas between them.
	let mut sets: Vec<&str> = Vec::new();
	for (index, part) in input.split(',').enumerate() {
		if index > 0 {
			sets.push(",");
		}
		sets.push(part);
	}
	let mut output: Vec<String> = Vec::new();
	let mut index = 0;
	while index < sets.len() {
		let mut set = sets[index].to_owned();
		if set == "," && index > 0 && index + 1 < sets.len() {
			let (previous, next) = (sets[index - 1], sets[index + 1]);
			if previous.matches('~').count() == 1 && next.matches('~').count() == 1 {
				set = format!("{previous},{next}");
				index += 1;
				output.pop();
			}
		}
		output.push(process_set(&set));
		index += 1;
	}
	output.concat()
}

/// The value of a single line `title`, `accTitle` or `accDescr`: trimmed, runs of blanks as one.
fn single_line_value(text: &str) -> String {
	let mut value = String::with_capacity(text.len());
	let mut previous_blank = false;
	for character in text.trim().chars() {
		let is_blank = character == ' ' || character == '\t';
		if is_blank && previous_blank {
			continue;
		}
		value.push(if is_blank { ' ' } else { character });
		previous_blank = is_blank;
	}
	value
}

/// Collects the common statements of a diagram.
#[derive(Debug, Default)]
pub struct CommonFields {
	pub title: Option<String>,
	pub accessibility: Accessibility,
}

impl CommonFields {
	/// Front matter `title` sets the diagram title before the statements.
	pub fn new(context: &Context) -> Self {
		Self { title: context.title.clone(), accessibility: Accessibility::default() }
	}

	/// Handles `statement` (trimmed, without comment) if it is `title`, `accTitle` or `accDescr`,
	/// reading further lines from `lines` for a multi-line `accDescr { … }`.
	pub fn parse(&mut self, statement: &str, lines: &mut Lines) -> Option<Result<()>> {
		if let Some(rest) = statement.strip_prefix("accTitle") {
			let value = rest.trim_start().strip_prefix(':')?;
			self.accessibility.title = single_line_value(value);
			return Some(Ok(()));
		}
		if let Some(rest) = statement.strip_prefix("accDescr") {
			let rest = rest.trim_start();
			if let Some(value) = rest.strip_prefix(':') {
				self.accessibility.description = single_line_value(value);
				return Some(Ok(()));
			}
			let block = rest.strip_prefix('{')?;
			let start_line = lines.line_number();
			let mut text = String::from(block);
			while !text.contains('}') {
				let Some((_, line)) = lines.next_line() else {
					return Some(Err(Error::at(start_line, "accDescr without closing }")));
				};
				text.push('\n');
				text.push_str(line);
			}
			let (inside, after) = text.split_once('}').unwrap_or((&text, ""));
			if !after.trim().is_empty() {
				return Some(Err(Error::at(
					lines.line_number(),
					format!("unexpected {:?} after accDescr", after.trim()),
				)));
			}
			let description = inside
				.lines()
				.map(single_line_value)
				.filter(|line| !line.is_empty())
				.collect::<Vec<_>>()
				.join("\n");
			self.accessibility.description = description;
			return Some(Ok(()));
		}
		if starts_with_word(statement, "title") {
			let rest = &statement["title".len()..];
			if rest.is_empty() || rest.starts_with([' ', '\t']) {
				self.title = Some(single_line_value(rest));
				return Some(Ok(()));
			}
		}
		None
	}
}

/// `setAccDescription`: trimmed, sanitized, indentation after line ends removed.
pub fn acc_description(text: &str) -> String {
	let sanitized = sanitize_text(text.trim());
	let mut description = String::with_capacity(sanitized.len());
	let mut lines = sanitized.split('\n');
	description.push_str(lines.next().unwrap_or_default());
	for line in lines {
		description.push('\n');
		description.push_str(line.trim_start());
	}
	description
}

/// `sanitizeText`: DOMPurify serializes text that contains `<` as HTML, which `.text()` then
/// shows literally. Tags are kept as they are; a tag cut off by the end is dropped.
pub fn sanitize_text(text: &str) -> String {
	if !text.contains('<') {
		return text.to_owned();
	}
	let mut sanitized = String::with_capacity(text.len() + 8);
	let mut rest = text;
	while let Some(character) = rest.chars().next() {
		let is_tag = character == '<'
			&& rest[1..]
				.starts_with(|next: char| next.is_ascii_alphabetic() || "/!?".contains(next));
		if is_tag && !rest.contains('>') {
			let after = &rest[1..];
			let is_cut_off_tag = !after.starts_with('/')
				|| after[1..].starts_with(|next: char| next.is_ascii_alphabetic());
			if is_cut_off_tag {
				break;
			}
		}
		if is_tag && rest.contains('>') {
			let end = rest.find('>').map_or(rest.len(), |end| end + 1);
			sanitized.push_str(&rest[..end]);
			rest = &rest[end..];
			continue;
		}
		let entity_length = (character == '&')
			.then(|| {
				let end = rest.find(';')?;
				let name = &rest[1..end];
				let is_entity = !name.is_empty()
					&& (name.chars().all(|c| c.is_ascii_alphanumeric())
						|| name.strip_prefix('#').is_some_and(|number| {
							!number.is_empty() && number.chars().all(|c| c.is_ascii_alphanumeric())
						}));
				is_entity.then_some(end + 1)
			})
			.flatten();
		match (character, entity_length) {
			(_, Some(length)) => {
				sanitized.push_str(&rest[..length]);
				rest = &rest[length..];
				continue;
			}
			('&', None) => sanitized.push_str("&amp;"),
			('<', None) => sanitized.push_str("&lt;"),
			('>', None) => sanitized.push_str("&gt;"),
			_ => sanitized.push(character),
		}
		rest = &rest[character.len_utf8()..];
	}
	sanitized
}

/// Mermaid's `STRING` terminal: `"…"` or `'…'` with backslash escapes. Returns the value and the
/// rest of `text`.
pub fn parse_quoted(text: &str) -> Option<(String, &str)> {
	let quote = text.chars().next().filter(|&quote| quote == '"' || quote == '\'')?;
	let mut value = String::new();
	let mut characters = text[1..].char_indices();
	while let Some((index, character)) = characters.next() {
		match character {
			'\\' => {
				let (_, escaped) = characters.next()?;
				value.push(escaped);
			}
			_ if character == quote => return Some((value, &text[1 + index + 1..])),
			_ => value.push(character),
		}
	}
	None
}

/// `wrapLabel`: lines of words up to `max_width` as `width_of` measures them, words broken with
/// hyphens where they are wider, joined with `<br/>`.
pub fn wrap_label(label: &str, max_width: f64, width_of: &mut dyn FnMut(&str) -> f64) -> String {
	if label.is_empty() || split_breaks(label).len() > 1 {
		return label.to_owned();
	}
	let words: Vec<&str> = label.split(' ').filter(|word| !word.is_empty()).collect();
	let mut completed: Vec<String> = Vec::new();
	let mut next_line = String::new();
	for (index, word) in words.iter().enumerate() {
		let word_length = width_of(&format!("{word} "));
		let next_line_length = width_of(&next_line);
		if word_length > max_width {
			let (hyphenated, remaining) = break_string(word, max_width, width_of);
			completed.push(std::mem::take(&mut next_line));
			completed.extend(hyphenated);
			next_line = remaining;
		} else if next_line_length + word_length >= max_width {
			completed.push(std::mem::replace(&mut next_line, (*word).to_owned()));
		} else if next_line.is_empty() {
			next_line = (*word).to_owned();
		} else {
			next_line = format!("{next_line} {word}");
		}
		if index + 1 == words.len() {
			completed.push(next_line.clone());
		}
	}
	completed.retain(|line| !line.is_empty());
	completed.join("<br/>")
}

/// `breakString`
fn break_string(
	word: &str,
	max_width: f64,
	width_of: &mut dyn FnMut(&str) -> f64,
) -> (Vec<String>, String) {
	let characters: Vec<char> = word.chars().collect();
	let mut lines = Vec::new();
	let mut current = String::new();
	for (index, character) in characters.iter().enumerate() {
		let next_line = format!("{current}{character}");
		if width_of(&next_line) >= max_width {
			let is_last = index + 1 == characters.len();
			lines.push(if is_last { next_line } else { format!("{next_line}-") });
			current.clear();
		} else {
			current = next_line;
		}
	}
	(lines, current)
}

/// Mermaid's `lineBreakRegex`: `<br>`, `<br/>`, `</br>` in any case and spacing.
pub fn split_breaks(text: &str) -> Vec<&str> {
	let mut lines = Vec::new();
	let mut start = 0;
	let bytes = text.as_bytes();
	let mut index = 0;
	while index < bytes.len() {
		if bytes[index] == b'<'
			&& let Some(length) = break_length(&text[index..])
		{
			lines.push(&text[start..index]);
			index += length;
			start = index;
			continue;
		}
		index += 1;
	}
	lines.push(&text[start..]);
	lines
}

/// Length of `<\/?br\s*\/?>` at the start of `text`.
fn break_length(text: &str) -> Option<usize> {
	let mut rest = text.strip_prefix('<')?;
	rest = rest.strip_prefix('/').unwrap_or(rest);
	if !rest.get(..2).is_some_and(|br| br.eq_ignore_ascii_case("br")) {
		return None;
	}
	rest = rest[2..].trim_start();
	rest = rest.strip_prefix('/').unwrap_or(rest);
	rest = rest.strip_prefix('>')?;
	Some(text.len() - rest.len())
}

/// Sorts stably without trusting `compare` to be a total order (Rust's sort may panic then).
pub fn stable_sort<T: Copy>(items: &mut [T], compare: impl Fn(T, T) -> Ordering) {
	for index in 1..items.len() {
		let item = items[index];
		let mut position = index;
		while position > 0 && compare(item, items[position - 1]) == Ordering::Less {
			items[position] = items[position - 1];
			position -= 1;
		}
		items[position] = item;
	}
}

/// `Array.prototype.sort` of V8 (`ArrayTimSort`): the first run, reversed when strictly
/// descending, then binary insertion. Exact for any comparator below 64 items, where V8 sorts a
/// single run; longer arrays give the same result whenever `compare` is consistent.
/// `compare(a, b)` returns the JavaScript comparator value (`< 0` sorts `a` first).
pub fn v8_sort<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> f64) {
	let length = items.len();
	if length < 2 {
		return;
	}
	let mut run = 2;
	let is_descending = compare(&items[1], &items[0]) < 0.0;
	while run < length {
		let order = compare(&items[run], &items[run - 1]);
		if (is_descending && order >= 0.0) || (!is_descending && order < 0.0) {
			break;
		}
		run += 1;
	}
	if is_descending {
		items[..run].reverse();
	}
	for start in run..length {
		let (mut left, mut right) = (0, start);
		while left < right {
			let middle = left + ((right - left) >> 1);
			if compare(&items[start], &items[middle]) < 0.0 {
				right = middle;
			} else {
				left = middle + 1;
			}
		}
		items[left..=start].rotate_right(1);
	}
}

/// `a.localeCompare(b)` in Edge (ICU root collation): ASCII blanks, punctuation, symbols,
/// digits and letters in ICU's primary order, case only as a tertiary difference with
/// lowercase first (probed in Edge). Other characters follow the ASCII ones by code point,
/// where ICU would interleave accented letters with their base letters.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
	const ORDER: &[u8] = b" _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";
	fn primary(character: char) -> u32 {
		let lower = character.to_ascii_lowercase();
		if lower.is_ascii() {
			if let Some(position) = ORDER.iter().position(|&byte| byte as char == lower) {
				return position as u32 + 1;
			}
			// Control characters come first.
			return 0;
		}
		ORDER.len() as u32 + 1 + character as u32
	}
	let primaries = a.chars().map(primary).cmp(b.chars().map(primary));
	if primaries != Ordering::Equal {
		return primaries;
	}
	let tertiary = |character: char| character.is_ascii_uppercase();
	a.chars().map(tertiary).cmp(b.chars().map(tertiary)).then_with(|| a.cmp(b))
}

/// `getBBox` of a group: Blink unites only non-empty boxes (`gfx::RectF::Union`).
#[derive(Default)]
pub struct BlinkBox {
	pub extent: Option<(f64, f64, f64, f64)>,
}

impl BlinkBox {
	pub fn include(&mut self, x: f64, y: f64, width: f64, height: f64) {
		if !(width > 0.0 && height > 0.0 && x.is_finite() && y.is_finite()) {
			return;
		}
		let (right, bottom) = (x + width, y + height);
		self.extent = Some(match self.extent {
			Some((left, top, old_right, old_bottom)) => {
				(left.min(x), top.min(y), old_right.max(right), old_bottom.max(bottom))
			}
			None => (x, y, right, bottom),
		});
	}
}

/// JavaScript's `Number()` of a text: `NaN` unless it is a decimal number, `Infinity` or empty.
pub fn js_number_of(text: &str) -> f64 {
	let text = text.trim();
	if text.is_empty() {
		return 0.0;
	}
	let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
	if unsigned == "Infinity" {
		return if text.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY };
	}
	let is_decimal = unsigned
		.starts_with(|character: char| character.is_ascii_digit() || character == '.')
		&& unsigned
			.bytes()
			.all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-'));
	match is_decimal {
		true => text.parse().unwrap_or(f64::NAN),
		false => f64::NAN,
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn compares_like_locale_compare_in_edge() {
		let mut words = vec![
			"a",
			"A",
			"ab",
			"aB",
			"Ab",
			"AB",
			"a-b",
			"a_b",
			"a1",
			"a10",
			"a2",
			"A1",
			"b",
			"_a",
			"-a",
			"a b",
			"placeholder-10",
			"placeholder-2",
		];
		npp::sort::sort_by(&mut words[..], |a, b| super::locale_compare(a, b));
		assert_eq!(
			words,
			[
				"_a",
				"-a",
				"a",
				"A",
				"a b",
				"a_b",
				"a-b",
				"a1",
				"A1",
				"a10",
				"a2",
				"ab",
				"aB",
				"Ab",
				"AB",
				"b",
				"placeholder-10",
				"placeholder-2"
			]
		);
	}

	#[test]
	fn sorts_like_v8() {
		let mut values = vec![3, 1, 2, 5, 4];
		super::v8_sort(&mut values, |a, b| f64::from(a - b));
		assert_eq!(values, [1, 2, 3, 4, 5]);
		// A comparator that sees no order keeps the input; a strictly descending start is reversed.
		let mut values = vec![(2, 'a'), (1, 'b'), (1, 'c')];
		super::v8_sort(&mut values, |a, b| f64::from(a.0 - b.0));
		assert_eq!(values, [(1, 'b'), (1, 'c'), (2, 'a')]);
	}

	use super::*;

	#[test]
	fn parses_generic_types() {
		assert_eq!(parse_generic_types("List~int~"), "List<int>");
		assert_eq!(parse_generic_types("Map~K, V~"), "Map<K, V>");
		assert_eq!(parse_generic_types("Array~Array~string~~"), "Array<Array<string>>");
		assert_eq!(parse_generic_types("plain"), "plain");
		assert_eq!(parse_generic_types("~a~b"), "<a>b");
	}

	#[test]
	fn parses_common_statements() {
		let code = "accDescr {\n  first   line\n\n  second\n}";
		let mut lines = Lines::new(code);
		lines.next_line();
		let mut fields = CommonFields::default();
		assert!(fields.parse("title   My  title", &mut lines).is_some());
		assert!(fields.parse("accTitle: A   title ", &mut lines).is_some());
		assert!(matches!(fields.parse("accDescr {", &mut lines), Some(Ok(()))));
		assert_eq!(fields.title.as_deref(), Some("My title"));
		assert_eq!(fields.accessibility.title, "A title");
		assert_eq!(fields.accessibility.description, "first line\nsecond");
		assert!(fields.parse("titled", &mut lines).is_none());
		assert!(matches!(fields.parse("accDescr {", &mut Lines::new("")), Some(Err(_))));
	}

	#[test]
	fn parses_strings_and_comments() {
		assert_eq!(parse_quoted(r#""a \"b\"" : 1"#), Some(("a \"b\"".to_owned(), " : 1")));
		assert_eq!(parse_quoted("'x'"), Some(("x".to_owned(), "")));
		assert_eq!(parse_quoted("\"open"), None);
		assert_eq!(strip_comment("\"50%%\" : 1 %% note"), "\"50%%\" : 1 ");
	}

	#[test]
	fn sanitizes_text_like_dompurify() {
		assert_eq!(sanitize_text("a & b"), "a & b");
		assert_eq!(sanitize_text("x < y & z"), "x &lt; y &amp; z");
		assert_eq!(sanitize_text("<b>bold</b> &amp;"), "<b>bold</b> &amp;");
		assert_eq!(sanitize_text("A & B <b>bold</b> x<y"), "A &amp; B <b>bold</b> x");
	}
}
