//! The JSON tables of use case diagrams (`usecaseJson.ts`): strict JSON whose objects keep the
//! source order of their properties, a repeated property keeping its first place and its last
//! value.

/// Nesting deeper than this is rejected instead of risking the stack.
const MAX_DEPTH: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
	Null,
	Bool(bool),
	Number(f64),
	String(String),
	Array(Vec<JsonValue>),
	Object(Vec<(String, JsonValue)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
	/// The byte offset of the error in the JSON text.
	pub offset: usize,
	pub message: String,
}

struct Reader<'a> {
	text: &'a str,
	position: usize,
}

impl Reader<'_> {
	fn bytes(&self) -> &[u8] {
		self.text.as_bytes()
	}

	fn error(&self, what: &str) -> JsonError {
		let found = match self.text[self.position..].chars().next() {
			Some(character) => format!("'{character}'"),
			None => "the end".to_owned(),
		};
		JsonError {
			offset: self.position,
			message: format!("Invalid JSON: {what}, found {found} at position {}", self.position),
		}
	}

	fn skip_whitespace(&mut self) {
		while self.bytes().get(self.position).is_some_and(|byte| b" \t\r\n".contains(byte)) {
			self.position += 1;
		}
	}

	fn value(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
		if depth > MAX_DEPTH {
			return Err(self.error("nesting too deep"));
		}
		self.skip_whitespace();
		match self.bytes().get(self.position) {
			Some(b'{') => self.object(depth),
			Some(b'[') => self.array(depth),
			Some(b'"') => self.string().map(JsonValue::String),
			Some(b't') => self.literal("true", JsonValue::Bool(true)),
			Some(b'f') => self.literal("false", JsonValue::Bool(false)),
			Some(b'n') => self.literal("null", JsonValue::Null),
			Some(b'-' | b'0'..=b'9') => self.number(),
			_ => Err(self.error("expected a value")),
		}
	}

	fn object(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
		self.position += 1;
		let mut properties: Vec<(String, JsonValue)> = Vec::new();
		self.skip_whitespace();
		if self.bytes().get(self.position) == Some(&b'}') {
			self.position += 1;
			return Ok(JsonValue::Object(properties));
		}
		loop {
			self.skip_whitespace();
			if self.bytes().get(self.position) != Some(&b'"') {
				return Err(self.error("expected a property name"));
			}
			let key = self.string()?;
			self.skip_whitespace();
			if self.bytes().get(self.position) != Some(&b':') {
				return Err(self.error("expected ':'"));
			}
			self.position += 1;
			let value = self.value(depth + 1)?;
			match properties.iter_mut().find(|(known, _)| *known == key) {
				Some(entry) => entry.1 = value,
				None => properties.push((key, value)),
			}
			self.skip_whitespace();
			match self.bytes().get(self.position) {
				Some(b'}') => {
					self.position += 1;
					return Ok(JsonValue::Object(properties));
				}
				Some(b',') => self.position += 1,
				_ => return Err(self.error("expected ',' or '}'")),
			}
		}
	}

	fn array(&mut self, depth: usize) -> Result<JsonValue, JsonError> {
		self.position += 1;
		let mut items = Vec::new();
		self.skip_whitespace();
		if self.bytes().get(self.position) == Some(&b']') {
			self.position += 1;
			return Ok(JsonValue::Array(items));
		}
		loop {
			items.push(self.value(depth + 1)?);
			self.skip_whitespace();
			match self.bytes().get(self.position) {
				Some(b']') => {
					self.position += 1;
					return Ok(JsonValue::Array(items));
				}
				Some(b',') => self.position += 1,
				_ => return Err(self.error("expected ',' or ']'")),
			}
		}
	}

	fn literal(&mut self, word: &str, value: JsonValue) -> Result<JsonValue, JsonError> {
		match self.text[self.position..].starts_with(word) {
			true => {
				self.position += word.len();
				Ok(value)
			}
			false => Err(self.error("expected a value")),
		}
	}

	fn number(&mut self) -> Result<JsonValue, JsonError> {
		let start = self.position;
		let digits = |reader: &mut Self| {
			let count = reader.bytes()[reader.position..]
				.iter()
				.take_while(|byte| byte.is_ascii_digit())
				.count();
			reader.position += count;
			count
		};
		if self.bytes().get(self.position) == Some(&b'-') {
			self.position += 1;
		}
		match self.bytes().get(self.position) {
			Some(b'0') => self.position += 1,
			Some(b'1'..=b'9') => {
				digits(self);
			}
			_ => return Err(self.error("expected a digit")),
		}
		if self.bytes().get(self.position) == Some(&b'.') {
			self.position += 1;
			if digits(self) == 0 {
				return Err(self.error("expected a digit"));
			}
		}
		if matches!(self.bytes().get(self.position), Some(b'e' | b'E')) {
			self.position += 1;
			if matches!(self.bytes().get(self.position), Some(b'+' | b'-')) {
				self.position += 1;
			}
			if digits(self) == 0 {
				return Err(self.error("expected a digit"));
			}
		}
		let number = self.text[start..self.position].parse().unwrap_or(f64::NAN);
		Ok(JsonValue::Number(number))
	}

	fn string(&mut self) -> Result<String, JsonError> {
		self.position += 1;
		let mut units: Vec<u16> = Vec::new();
		loop {
			let Some(character) = self.text[self.position..].chars().next() else {
				return Err(self.error("unterminated string"));
			};
			if character == '"' {
				self.position += 1;
				return Ok(String::from_utf16_lossy(&units));
			}
			if (character as u32) < 0x20 {
				return Err(self.error("bad control character in string"));
			}
			self.position += character.len_utf8();
			if character != '\\' {
				let mut buffer = [0u16; 2];
				units.extend_from_slice(character.encode_utf16(&mut buffer));
				continue;
			}
			let escape = self.bytes().get(self.position).copied();
			let unit = match escape {
				Some(b'"') => u16::from(b'"'),
				Some(b'\\') => u16::from(b'\\'),
				Some(b'/') => u16::from(b'/'),
				Some(b'b') => 8,
				Some(b'f') => 12,
				Some(b'n') => u16::from(b'\n'),
				Some(b'r') => u16::from(b'\r'),
				Some(b't') => u16::from(b'\t'),
				Some(b'u') => {
					let hex = self.text.get(self.position + 1..self.position + 5).unwrap_or("");
					match u16::from_str_radix(hex, 16) {
						Ok(unit)
							if hex.len() == 4 && hex.bytes().all(|b| b.is_ascii_hexdigit()) =>
						{
							self.position += 4;
							unit
						}
						_ => {
							self.position += 1;
							return Err(self.error("bad Unicode escape"));
						}
					}
				}
				_ => return Err(self.error("bad escaped character")),
			};
			self.position += 1;
			units.push(unit);
		}
	}
}

/// Parses the JSON object `text` (`parseOrderedJsonObject`).
pub fn parse_object(text: &str) -> Result<JsonValue, JsonError> {
	let mut reader = Reader { text, position: 0 };
	let value = reader.value(0)?;
	reader.skip_whitespace();
	if reader.position != text.len() {
		return Err(reader.error("unexpected text after the JSON value"));
	}
	match value {
		JsonValue::Object(_) => Ok(value),
		_ => {
			Err(JsonError { offset: 0, message: "JSON value must have an object root".to_owned() })
		}
	}
}

/// `String(number)` of JavaScript.
pub fn js_number_string(number: f64) -> String {
	if number.is_nan() {
		return "NaN".to_owned();
	}
	if number.is_infinite() {
		return if number > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
	}
	if number == 0.0 {
		return "0".to_owned();
	}
	let magnitude = number.abs();
	if (1e-6..1e21).contains(&magnitude) {
		return format!("{number}");
	}
	let text = format!("{number:e}");
	match text.split_once('e') {
		Some((mantissa, exponent)) if !exponent.starts_with('-') => {
			format!("{mantissa}e+{exponent}")
		}
		_ => text,
	}
}

/// One row of a JSON table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonRow {
	pub key: String,
	pub accessible_key: String,
	pub value: String,
}

fn scalar(value: &JsonValue) -> Option<String> {
	match value {
		JsonValue::Null => Some("null".to_owned()),
		JsonValue::Bool(value) => Some(value.to_string()),
		JsonValue::Number(number) => Some(js_number_string(*number)),
		JsonValue::String(text) => Some(text.clone()),
		_ => None,
	}
}

/// `flattenJsonRows`: the leaves with their paths, `sanitize`d. Nesting is bounded by the parser.
pub fn flatten_rows(value: &JsonValue, sanitize: &dyn Fn(&str) -> String) -> Vec<JsonRow> {
	let mut rows = Vec::new();
	let mut append = |key: &str, accessible_key: &str, value: &str| {
		rows.push(JsonRow {
			key: sanitize(key),
			accessible_key: sanitize(accessible_key),
			value: sanitize(value),
		});
	};
	// An explicit stack of values and paths, visited in source order.
	let mut stack: Vec<(&JsonValue, String)> = vec![(value, String::new())];
	while let Some((current, path)) = stack.pop() {
		match current {
			JsonValue::Array(items) if items.is_empty() => append(&path, &path, "[]"),
			JsonValue::Array(items) => {
				let scalars: Option<Vec<String>> = items.iter().map(scalar).collect();
				match scalars {
					Some(scalars) => {
						for (index, element) in scalars.iter().enumerate() {
							append(if index == 0 { &path } else { "" }, &path, element);
						}
					}
					None => {
						for (index, item) in items.iter().enumerate().rev() {
							stack.push((item, format!("{path}[{index}]")));
						}
					}
				}
			}
			JsonValue::Object(properties) if properties.is_empty() => append(&path, &path, "{}"),
			JsonValue::Object(properties) => {
				for (key, item) in properties.iter().rev() {
					let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
					stack.push((item, child));
				}
			}
			scalar_value => {
				let text = scalar(scalar_value).unwrap_or_default();
				append(&path, &path, &text);
			}
		}
	}
	rows
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn keeps_the_source_order() {
		let value = parse_object(
			r#"{"2": "second", "1": "first", "a": {"b": [1, 2.5e21, null]}, "a": {"c": true},
			"items": [{"name": "Book"}], "e": {}, "f": []}"#,
		)
		.unwrap();
		let rows = flatten_rows(&value, &|text| text.to_owned());
		let cells: Vec<(&str, &str)> =
			rows.iter().map(|row| (row.key.as_str(), row.value.as_str())).collect();
		assert_eq!(
			cells,
			[
				("2", "second"),
				("1", "first"),
				("a.c", "true"),
				("items[0].name", "Book"),
				("e", "{}"),
				("f", "[]")
			]
		);
		let value = parse_object(r#"{"n": [1, 2.5e21, 1e-7, 0.1]}"#).unwrap();
		let rows = flatten_rows(&value, &|text| text.to_owned());
		let values: Vec<&str> = rows.iter().map(|row| row.value.as_str()).collect();
		assert_eq!(values, ["1", "2.5e+21", "1e-7", "0.1"]);
		assert_eq!(rows[1].key, "");
		assert!(parse_object(r#"{"a": 01}"#).is_err());
		assert!(parse_object(r#"{"a": "\x"}"#).is_err());
		assert_eq!(parse_object(r#"{"a": 1,}"#).unwrap_err().offset, 8);
	}
}
