//! The configuration values of front matter (YAML) and `%%{init: …}%%` directives (JSON).
//!
//! Both parsers accept any input: they stop at the nesting limit and return `None` for what they
//! cannot read.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
	Null,
	Bool(bool),
	Number(f64),
	String(String),
	Array(Vec<Value>),
	/// In source order; later duplicate keys win in [`Value::get`].
	Object(Vec<(String, Value)>),
}

/// Deeper values are rejected, against unbounded recursion.
const MAX_DEPTH: usize = 32;

impl Value {
	pub fn get(&self, key: &str) -> Option<&Value> {
		match self {
			Value::Object(entries) => {
				entries.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value)
			}
			_ => None,
		}
	}

	pub fn path(&self, keys: &[&str]) -> Option<&Value> {
		keys.iter().try_fold(self, |value, key| value.get(key))
	}

	pub fn as_str(&self) -> Option<&str> {
		match self {
			Value::String(text) => Some(text),
			_ => None,
		}
	}

	/// Numbers, and strings like `"16px"` or `"0.5"` as Mermaid's `parseFloat` reads them.
	pub fn as_number(&self) -> Option<f64> {
		match self {
			Value::Number(number) => Some(*number),
			Value::String(text) => parse_leading_number(text),
			_ => None,
		}
	}

	pub fn as_bool(&self) -> Option<bool> {
		match self {
			Value::Bool(value) => Some(*value),
			Value::String(text) if text == "true" => Some(true),
			Value::String(text) if text == "false" => Some(false),
			_ => None,
		}
	}

	/// The text of a scalar, as JavaScript's `String(value)` writes it.
	pub fn to_text(&self) -> Option<String> {
		match self {
			Value::String(text) => Some(text.clone()),
			Value::Number(number) => Some(format_number(*number)),
			Value::Bool(value) => Some(value.to_string()),
			_ => None,
		}
	}

	/// Merges `other` into `self` like Mermaid's `assignWithDepth`: objects are merged
	/// recursively, other values replaced.
	pub fn merge(&mut self, other: &Value) {
		self.merge_at_depth(other, 0);
	}

	fn merge_at_depth(&mut self, other: &Value, depth: usize) {
		match (self, other) {
			(Value::Object(entries), Value::Object(other_entries)) if depth < MAX_DEPTH => {
				for (key, value) in other_entries {
					match entries.iter_mut().rev().find(|(name, _)| name == key) {
						Some((_, existing)) => existing.merge_at_depth(value, depth + 1),
						None => entries.push((key.clone(), value.clone())),
					}
				}
			}
			(this, other) => *this = other.clone(),
		}
	}

	pub fn remove(&mut self, key: &str) -> Option<Value> {
		let Value::Object(entries) = self else { return None };
		let index = entries.iter().rposition(|(name, _)| name == key)?;
		let (_, value) = entries.remove(index);
		entries.retain(|(name, _)| name != key);
		Some(value)
	}
}

/// JavaScript's `parseFloat`: the longest number at the start, ignoring leading whitespace.
pub fn parse_leading_number(text: &str) -> Option<f64> {
	let text = text.trim_start();
	let bytes = text.as_bytes();
	let mut end = 0;
	if matches!(bytes.first(), Some(b'+' | b'-')) {
		end = 1;
	}
	let digits_start = end;
	while bytes.get(end).is_some_and(u8::is_ascii_digit) {
		end += 1;
	}
	if bytes.get(end) == Some(&b'.') {
		end += 1;
		while bytes.get(end).is_some_and(u8::is_ascii_digit) {
			end += 1;
		}
	}
	if end == digits_start || &text[digits_start..end] == "." {
		return None;
	}
	if matches!(bytes.get(end), Some(b'e' | b'E')) {
		let mut exponent_end = end + 1;
		if matches!(bytes.get(exponent_end), Some(b'+' | b'-')) {
			exponent_end += 1;
		}
		let exponent_digits = exponent_end;
		while bytes.get(exponent_end).is_some_and(u8::is_ascii_digit) {
			exponent_end += 1;
		}
		if exponent_end > exponent_digits {
			end = exponent_end;
		}
	}
	text[..end].parse().ok().filter(|number: &f64| number.is_finite())
}

/// A number as JavaScript prints it for the common cases: integers without fraction.
pub fn format_number(number: f64) -> String {
	if number.fract() == 0.0 && number.abs() < 1e15 {
		format!("{}", number as i64)
	} else {
		format!("{number}")
	}
}

/// Strict JSON, as `JSON.parse` reads it.
pub fn parse_json(text: &str) -> Option<Value> {
	let mut parser = JsonParser { text: text.as_bytes(), position: 0 };
	let value = parser.value(0)?;
	parser.skip_whitespace();
	(parser.position == parser.text.len()).then_some(value)
}

struct JsonParser<'a> {
	text: &'a [u8],
	position: usize,
}

impl JsonParser<'_> {
	fn skip_whitespace(&mut self) {
		while self.text.get(self.position).is_some_and(|byte| b" \t\r\n".contains(byte)) {
			self.position += 1;
		}
	}

	fn eat(&mut self, byte: u8) -> bool {
		self.skip_whitespace();
		let is_match = self.text.get(self.position) == Some(&byte);
		if is_match {
			self.position += 1;
		}
		is_match
	}

	fn value(&mut self, depth: usize) -> Option<Value> {
		if depth > MAX_DEPTH {
			return None;
		}
		self.skip_whitespace();
		match *self.text.get(self.position)? {
			b'{' => {
				self.position += 1;
				let mut entries = Vec::new();
				if self.eat(b'}') {
					return Some(Value::Object(entries));
				}
				loop {
					self.skip_whitespace();
					let key = self.string()?;
					if !self.eat(b':') {
						return None;
					}
					entries.push((key, self.value(depth + 1)?));
					if self.eat(b'}') {
						return Some(Value::Object(entries));
					}
					if !self.eat(b',') {
						return None;
					}
				}
			}
			b'[' => {
				self.position += 1;
				let mut items = Vec::new();
				if self.eat(b']') {
					return Some(Value::Array(items));
				}
				loop {
					items.push(self.value(depth + 1)?);
					if self.eat(b']') {
						return Some(Value::Array(items));
					}
					if !self.eat(b',') {
						return None;
					}
				}
			}
			b'"' => self.string().map(Value::String),
			b't' => self.keyword("true", Value::Bool(true)),
			b'f' => self.keyword("false", Value::Bool(false)),
			b'n' => self.keyword("null", Value::Null),
			_ => self.number(),
		}
	}

	fn keyword(&mut self, word: &str, value: Value) -> Option<Value> {
		let is_match = self.text[self.position..].starts_with(word.as_bytes());
		is_match.then(|| {
			self.position += word.len();
			value
		})
	}

	fn number(&mut self) -> Option<Value> {
		let start = self.position;
		while self
			.text
			.get(self.position)
			.is_some_and(|byte| byte.is_ascii_digit() || b"+-.eE".contains(byte))
		{
			self.position += 1;
		}
		let text = std::str::from_utf8(&self.text[start..self.position]).ok()?;
		let number: f64 = text.parse().ok()?;
		number.is_finite().then_some(Value::Number(number))
	}

	fn string(&mut self) -> Option<String> {
		if self.text.get(self.position) != Some(&b'"') {
			return None;
		}
		self.position += 1;
		let mut bytes = Vec::new();
		loop {
			let byte = *self.text.get(self.position)?;
			self.position += 1;
			match byte {
				b'"' => return String::from_utf8(bytes).ok(),
				b'\\' => {
					let escape = *self.text.get(self.position)?;
					self.position += 1;
					let decoded = match escape {
						b'n' => '\n',
						b't' => '\t',
						b'r' => '\r',
						b'b' => '\u{8}',
						b'f' => '\u{c}',
						b'u' => self.unicode_escape()?,
						b'"' | b'\\' | b'/' => char::from(escape),
						_ => return None,
					};
					bytes.extend_from_slice(decoded.encode_utf8(&mut [0; 4]).as_bytes());
				}
				_ => bytes.push(byte),
			}
		}
	}

	fn unicode_escape(&mut self) -> Option<char> {
		let read_unit = |parser: &mut Self| {
			let hex = parser.text.get(parser.position..parser.position + 4)?;
			parser.position += 4;
			u16::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()
		};
		let unit = read_unit(self)?;
		if (0xD800..0xDC00).contains(&unit) && self.text[self.position..].starts_with(b"\\u") {
			self.position += 2;
			let low = read_unit(self)?;
			return char::decode_utf16([unit, low]).next()?.ok();
		}
		Some(char::from_u32(u32::from(unit)).unwrap_or('\u{FFFD}'))
	}
}

/// The YAML of front matter, with js-yaml's JSON schema: block mappings and sequences, flow
/// collections, plain and quoted scalars, block scalars and comments. Anchors, tags and
/// multi-document streams are not supported.
pub fn parse_yaml(text: &str) -> Option<Value> {
	// A flow collection may span lines, like the `{\n…\n}` Mermaid makes of shape data.
	let trimmed = text.trim();
	if trimmed.starts_with(['{', '[']) {
		let mut parser = FlowParser { text: trimmed.as_bytes(), position: 0 };
		if let Some(value) = parser.value(0) {
			parser.skip_whitespace();
			if parser.position == trimmed.len() {
				return Some(value);
			}
		}
	}
	let lines: Vec<YamlLine> = text
		.lines()
		.filter_map(|line| {
			let content = strip_yaml_comment(line).trim_end();
			let indent = line.len() - line.trim_start_matches(' ').len();
			// Comment lines stay for block scalars, which keep `#`.
			(!line.trim().is_empty()).then_some(YamlLine { indent, text: content, raw: line })
		})
		.collect();
	if lines.is_empty() {
		return Some(Value::Null);
	}
	let mut parser = YamlParser { lines, index: 0 };
	parser.skip_comments();
	let Some(first) = parser.lines.get(parser.index) else { return Some(Value::Null) };
	let indent = first.indent;
	let value = parser.block(indent, 0)?;
	Some(value)
}

struct YamlLine<'a> {
	indent: usize,
	/// Without comment and trailing whitespace.
	text: &'a str,
	/// For block scalars, which keep `#`.
	raw: &'a str,
}

struct YamlParser<'a> {
	lines: Vec<YamlLine<'a>>,
	index: usize,
}

impl<'a> YamlParser<'a> {
	fn skip_comments(&mut self) {
		while self.lines.get(self.index).is_some_and(|line| line.text.trim().is_empty()) {
			self.index += 1;
		}
	}

	/// Indentation and text of the current line after comment lines.
	fn current(&mut self) -> Option<(usize, &'a str)> {
		self.skip_comments();
		self.lines.get(self.index).map(|line| (line.indent, line.text))
	}

	/// The block starting at the current line with exactly `indent`.
	fn block(&mut self, indent: usize, depth: usize) -> Option<Value> {
		if depth > MAX_DEPTH {
			return None;
		}
		let (_, text) = self.current()?;
		let content = text.trim_start();
		if content == "-" || content.starts_with("- ") {
			return self.sequence(indent, depth);
		}
		if split_yaml_key(content).is_some() {
			return self.mapping(indent, depth);
		}
		self.index += 1;
		self.inline_value(content, None, depth)
	}

	fn mapping(&mut self, indent: usize, depth: usize) -> Option<Value> {
		let mut entries = Vec::new();
		while let Some((line_indent, text)) = self.current() {
			if line_indent != indent {
				break;
			}
			let Some((key, rest)) = split_yaml_key(text.trim_start()) else { break };
			self.index += 1;
			let value = self.value_after_indicator(rest, indent, depth)?;
			entries.push((key, value));
		}
		Some(Value::Object(entries))
	}

	fn sequence(&mut self, indent: usize, depth: usize) -> Option<Value> {
		let mut items = Vec::new();
		while let Some((line_indent, text)) = self.current() {
			let content = text.trim_start();
			if line_indent != indent || !(content == "-" || content.starts_with("- ")) {
				break;
			}
			let rest = content[1..].trim_start();
			if rest.is_empty() {
				self.index += 1;
				items.push(self.nested_block(indent, depth)?);
				continue;
			}
			if split_yaml_key(rest).is_some() {
				// `- key: value` starts a mapping at the column of `key`.
				let column = line_indent + (content.len() - rest.len());
				let original = &self.lines[self.index];
				let shifted: &str = &original.text[column.min(original.text.len())..];
				self.lines[self.index] =
					YamlLine { indent: column, text: shifted, raw: original.raw };
				items.push(self.mapping(column, depth + 1)?);
				continue;
			}
			self.index += 1;
			items.push(self.value_after_indicator(rest, indent, depth)?);
		}
		Some(Value::Array(items))
	}

	/// The value after `key:` or `- `, inline or as a nested block.
	fn value_after_indicator(
		&mut self,
		rest: &'a str,
		indent: usize,
		depth: usize,
	) -> Option<Value> {
		if rest.is_empty() {
			return self.nested_block(indent, depth);
		}
		if rest.starts_with('|') || rest.starts_with('>') {
			return Some(Value::String(self.block_scalar(rest, indent)));
		}
		self.inline_value(rest, Some(indent), depth)
	}

	/// A scalar or flow collection; quoted scalars and flow collections continue on the following
	/// lines indented deeper than `parent_indent`. `None` for text after a closing quote or a
	/// broken flow collection, which js-yaml rejects.
	fn inline_value(
		&mut self,
		first: &'a str,
		parent_indent: Option<usize>,
		depth: usize,
	) -> Option<Value> {
		let is_quoted = first.starts_with(['"', '\'']);
		let is_flow = first.starts_with(['{', '[']);
		if !is_quoted && !is_flow {
			return Some(yaml_scalar(first));
		}
		let mut text = first.to_owned();
		loop {
			let is_complete = match is_quoted {
				true => quoted_scalar_end(&text).is_some(),
				false => flow_depth(&text) == 0,
			};
			let next = self.lines.get(self.index).filter(|line| {
				parent_indent.is_none_or(|parent_indent| line.indent > parent_indent)
			});
			let Some(line) = next.filter(|_| !is_complete) else { break };
			// Quoted line breaks fold into spaces; `#` there is no comment.
			match is_quoted {
				true => text.extend([" ", line.raw.trim()]),
				false => text.extend(["\n", line.text]),
			}
			self.index += 1;
		}
		if is_quoted {
			let end = quoted_scalar_end(&text)?;
			let rest = text[end..].trim_start();
			return (rest.is_empty() || rest.starts_with('#')).then(|| yaml_scalar(&text[..end]));
		}
		let mut parser = FlowParser { text: text.as_bytes(), position: 0 };
		let value = parser.value(depth)?;
		parser.skip_whitespace();
		(parser.position == text.len()).then_some(value)
	}

	fn nested_block(&mut self, indent: usize, depth: usize) -> Option<Value> {
		match self.current() {
			Some((next_indent, _)) if next_indent > indent => self.block(next_indent, depth + 1),
			// A sequence may be at the indentation of its key.
			Some((next_indent, text))
				if next_indent == indent && text.trim_start().starts_with("- ") =>
			{
				self.sequence(indent, depth + 1)
			}
			_ => Some(Value::Null),
		}
	}

	fn block_scalar(&mut self, header: &str, indent: usize) -> String {
		let is_folded = header.starts_with('>');
		let keeps_final_newline = !header.contains('-');
		let mut lines = Vec::new();
		let mut content_indent = None;
		while let Some(line) = self.lines.get(self.index) {
			if line.indent <= indent {
				break;
			}
			let column = *content_indent.get_or_insert(line.indent);
			lines.push(line.raw.get(column.min(line.indent)..).unwrap_or("").trim_end());
			self.index += 1;
		}
		let mut text = match is_folded {
			true => lines.join(" "),
			false => lines.join("\n"),
		};
		if keeps_final_newline && !text.is_empty() {
			text.push('\n');
		}
		text
	}
}

/// `#` starts a comment at the line start or after whitespace, outside of quotes.
fn strip_yaml_comment(line: &str) -> &str {
	let mut quote = None;
	let mut previous = ' ';
	for (index, character) in line.char_indices() {
		match quote {
			Some(open) if character == open => quote = None,
			Some(_) => {}
			None if character == '#' && previous.is_whitespace() => return &line[..index],
			None if (character == '"' || character == '\'') && previous.is_whitespace() => {
				quote = Some(character);
			}
			None => {}
		}
		previous = character;
	}
	line
}

/// `key: rest` of a mapping entry, with the unquoted key.
fn split_yaml_key(content: &str) -> Option<(String, &str)> {
	if content.starts_with(['"', '\'']) {
		let quote = content.chars().next()?;
		let end = content[1..].find(quote)? + 1;
		let rest = content[end + 1..].trim_start().strip_prefix(':')?;
		if !(rest.is_empty() || rest.starts_with(' ')) {
			return None;
		}
		return Some((content[1..end].to_owned(), rest.trim()));
	}
	if content.starts_with(['{', '[']) || content == "-" || content.starts_with("- ") {
		return None;
	}
	let colon = content.match_indices(':').find(|&(index, _)| {
		let after = &content[index + 1..];
		after.is_empty() || after.starts_with([' ', '\t'])
	})?;
	let key = content[..colon.0].trim();
	(!key.is_empty()).then(|| (key.to_owned(), content[colon.0 + 1..].trim()))
}

/// The byte offset after the closing quote of the quoted scalar `text` starts with.
fn quoted_scalar_end(text: &str) -> Option<usize> {
	let bytes = text.as_bytes();
	let quote = *bytes.first()?;
	let mut index = 1;
	while let Some(&byte) = bytes.get(index) {
		index += 1;
		if byte == b'\\' && quote == b'"' {
			index += 1;
		} else if byte == quote {
			if quote == b'\'' && bytes.get(index) == Some(&b'\'') {
				index += 1;
				continue;
			}
			return Some(index);
		}
	}
	None
}

/// The open brackets at the end of the flow collection `text`, outside of quotes.
fn flow_depth(text: &str) -> usize {
	let bytes = text.as_bytes();
	let mut depth = 0usize;
	let mut index = 0;
	while let Some(&byte) = bytes.get(index) {
		match byte {
			b'"' | b'\'' => match quoted_scalar_end(&text[index..]) {
				Some(length) => index += length - 1,
				None => return depth.max(1),
			},
			b'{' | b'[' => depth += 1,
			b'}' | b']' => depth = depth.saturating_sub(1),
			_ => {}
		}
		index += 1;
	}
	depth
}

fn yaml_scalar(text: &str) -> Value {
	let text = text.trim();
	if let Some(inner) = text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
		return parse_json(text)
			.filter(|value| matches!(value, Value::String(_)))
			.unwrap_or_else(|| Value::String(inner.to_owned()));
	}
	if let Some(inner) = text.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
		return Value::String(inner.replace("''", "'"));
	}
	match text {
		"" | "~" | "null" | "Null" | "NULL" => return Value::Null,
		"true" | "True" | "TRUE" => return Value::Bool(true),
		"false" | "False" | "FALSE" => return Value::Bool(false),
		_ => {}
	}
	let is_number = text.bytes().all(|byte| byte.is_ascii_digit() || b"+-.eE".contains(&byte))
		&& text.bytes().any(|byte| byte.is_ascii_digit());
	if is_number
		&& let Ok(number) = text.parse::<f64>()
		&& number.is_finite()
	{
		return Value::Number(number);
	}
	Value::String(text.to_owned())
}

/// YAML flow collections: JSON with plain scalars and unquoted keys.
struct FlowParser<'a> {
	text: &'a [u8],
	position: usize,
}

impl FlowParser<'_> {
	fn skip_whitespace(&mut self) {
		while self.text.get(self.position).is_some_and(|byte| byte.is_ascii_whitespace()) {
			self.position += 1;
		}
	}

	fn eat(&mut self, byte: u8) -> bool {
		self.skip_whitespace();
		let is_match = self.text.get(self.position) == Some(&byte);
		if is_match {
			self.position += 1;
		}
		is_match
	}

	fn value(&mut self, depth: usize) -> Option<Value> {
		if depth > MAX_DEPTH {
			return None;
		}
		self.skip_whitespace();
		match *self.text.get(self.position)? {
			b'{' => {
				self.position += 1;
				let mut entries = Vec::new();
				loop {
					if self.eat(b'}') {
						return Some(Value::Object(entries));
					}
					let key = self.scalar(true)?.to_text().unwrap_or_default();
					let value = match self.eat(b':') {
						true => self.value(depth + 1)?,
						false => Value::Null,
					};
					entries.push((key, value));
					if !self.eat(b',') && !self.eat(b'}') {
						return None;
					} else if self.text.get(self.position - 1) == Some(&b'}') {
						return Some(Value::Object(entries));
					}
				}
			}
			b'[' => {
				self.position += 1;
				let mut items = Vec::new();
				loop {
					if self.eat(b']') {
						return Some(Value::Array(items));
					}
					items.push(self.value(depth + 1)?);
					if !self.eat(b',') && !self.eat(b']') {
						return None;
					} else if self.text.get(self.position - 1) == Some(&b']') {
						return Some(Value::Array(items));
					}
				}
			}
			_ => self.scalar(false),
		}
	}

	fn scalar(&mut self, is_key: bool) -> Option<Value> {
		self.skip_whitespace();
		let start = self.position;
		let quote = *self.text.get(start)?;
		if quote == b'"' || quote == b'\'' {
			self.position += 1;
			while let Some(&byte) = self.text.get(self.position) {
				self.position += 1;
				if byte == b'\\' && quote == b'"' {
					self.position += 1;
				} else if byte == quote {
					if quote == b'\'' && self.text.get(self.position) == Some(&b'\'') {
						self.position += 1;
						continue;
					}
					let text = std::str::from_utf8(&self.text[start..self.position]).ok()?;
					return Some(yaml_scalar(text));
				}
			}
			return None;
		}
		while let Some(&byte) = self.text.get(self.position) {
			let ends_key = is_key
				&& byte == b':'
				&& self
					.text
					.get(self.position + 1)
					.is_none_or(|next| next.is_ascii_whitespace() || b",}".contains(next));
			if b",]}".contains(&byte) || ends_key {
				break;
			}
			self.position += 1;
		}
		let text = std::str::from_utf8(&self.text[start..self.position]).ok()?;
		(!text.trim().is_empty() || is_key).then(|| yaml_scalar(text))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn object(entries: &[(&str, Value)]) -> Value {
		Value::Object(
			entries.iter().map(|(key, value)| ((*key).to_owned(), value.clone())).collect(),
		)
	}

	fn text(value: &str) -> Value {
		Value::String(value.to_owned())
	}

	#[test]
	fn parses_json() {
		let value = parse_json(r#" {"theme": "dark", "flowchart": {"curve": "basis", "x": [1, -2.5e1, true, null]}, "u": "\u00e4\ud83d\ude00"} "#).unwrap();
		assert_eq!(value.path(&["theme"]).and_then(Value::as_str), Some("dark"));
		assert_eq!(
			value.path(&["flowchart", "x"]),
			Some(&Value::Array(vec![
				Value::Number(1.0),
				Value::Number(-25.0),
				Value::Bool(true),
				Value::Null
			]))
		);
		assert_eq!(value.get("u").and_then(Value::as_str), Some("ä😀"));
		assert_eq!(parse_json("{\"a\": 1,}"), None);
		assert_eq!(parse_json("{a: 1}"), None);
		assert_eq!(parse_json(&"[".repeat(1000)), None);
	}

	#[test]
	fn parses_front_matter_yaml() {
		let yaml = "title: Hello: world\nconfig:\n  theme: forest # comment\n  themeVariables:\n    primaryColor: \"#00ff00\"\n  flowchart:\n    curve: linear\n    nodeSpacing: 30\n  list:\n    - a\n    - name: b\n      x: 1\n  flow: {a: 1, b: [x, 'y z']}\n";
		let value = parse_yaml(yaml).unwrap();
		assert_eq!(value.get("title"), Some(&text("Hello: world")));
		assert_eq!(value.path(&["config", "theme"]), Some(&text("forest")));
		assert_eq!(
			value.path(&["config", "themeVariables", "primaryColor"]),
			Some(&text("#00ff00"))
		);
		assert_eq!(value.path(&["config", "flowchart", "nodeSpacing"]), Some(&Value::Number(30.0)));
		assert_eq!(
			value.path(&["config", "list"]),
			Some(&Value::Array(vec![
				text("a"),
				object(&[("name", text("b")), ("x", Value::Number(1.0))])
			]))
		);
		assert_eq!(
			value.path(&["config", "flow"]),
			Some(&object(&[
				("a", Value::Number(1.0)),
				("b", Value::Array(vec![text("x"), text("y z")]))
			]))
		);
	}

	#[test]
	fn parses_block_scalars() {
		let value = parse_yaml("a: |\n  line 1\n  # not a comment\nb: x").unwrap();
		assert_eq!(value.get("a"), Some(&text("line 1\n# not a comment\n")));
		assert_eq!(value.get("b"), Some(&text("x")));
	}

	#[test]
	fn rejects_what_js_yaml_rejects() {
		assert_eq!(parse_yaml("shape: tool\nlabel: \"Multi\" x\n"), None);
		assert_eq!(parse_yaml("label: 'a''b' c\n"), None);
		assert_eq!(parse_yaml("a: [x, y] z\n"), None);
		assert_eq!(parse_yaml("a: {x: 1\n"), None);
		assert_eq!(parse_yaml("a: \"open\n"), None);
		let value = parse_yaml("label: \"Multi\" # comment\nb: 'it''s'\n").unwrap();
		assert_eq!(value.get("label"), Some(&text("Multi")));
		assert_eq!(value.get("b"), Some(&text("it's")));
	}

	#[test]
	fn continues_quoted_scalars_and_flow_collections() {
		let value = parse_yaml("a: \"one\n  two # three\"\nb: [x,\n  y]\nc: {d: 1,\n  e: 2}\n");
		let value = value.unwrap();
		assert_eq!(value.get("a"), Some(&text("one two # three")));
		assert_eq!(value.get("b"), Some(&Value::Array(vec![text("x"), text("y")])));
		assert_eq!(
			value.get("c"),
			Some(&object(&[("d", Value::Number(1.0)), ("e", Value::Number(2.0))]))
		);
	}

	#[test]
	fn merges_like_assign_with_depth() {
		let mut base = parse_json(r#"{"a": {"b": 1, "c": 2}, "d": 3}"#).unwrap();
		base.merge(&parse_json(r#"{"a": {"c": 4}, "d": {"e": 5}}"#).unwrap());
		assert_eq!(base, parse_json(r#"{"a": {"b": 1, "c": 4}, "d": {"e": 5}}"#).unwrap());
	}

	#[test]
	fn reads_numbers_like_parse_float() {
		assert_eq!(parse_leading_number(" 16px"), Some(16.0));
		assert_eq!(parse_leading_number("-.5e1x"), Some(-5.0));
		assert_eq!(parse_leading_number("1e"), Some(1.0));
		assert_eq!(parse_leading_number("px"), None);
		assert_eq!(parse_leading_number("."), None);
	}

	#[test]
	fn accepts_any_input() {
		for input in [
			"",
			"-",
			"- - -",
			": :",
			"a:\n  - b:\n    c",
			"{",
			"[a, {b: [",
			"'",
			"a: \"",
			"  - x\n- y",
			"a: |",
		] {
			let _ = parse_yaml(input);
			let _ = parse_json(input);
		}
		let deep = (0..200).map(|level| format!("{}a:\n", " ".repeat(level))).collect::<String>();
		let _ = parse_yaml(&deep);
	}
}
