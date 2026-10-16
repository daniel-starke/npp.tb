//! Minimal JSON reader for the data files the development tasks download.

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
	Null,
	Bool(bool),
	Number(f64),
	String(String),
	Array(Vec<JsonValue>),
	Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
	pub fn field(&self, name: &str) -> Option<&JsonValue> {
		match self {
			JsonValue::Object(fields) => {
				fields.iter().find(|(field_name, _)| field_name == name).map(|(_, value)| value)
			}
			_ => None,
		}
	}
}

pub fn parse_json(text: &str) -> Result<JsonValue, String> {
	let mut reader = Reader { text, position: 0 };
	let value = reader.value()?;
	reader.skip_whitespace();
	if reader.position != text.len() {
		return Err(reader.error("trailing characters"));
	}
	Ok(value)
}

struct Reader<'a> {
	text: &'a str,
	position: usize,
}

impl Reader<'_> {
	fn error(&self, message: &str) -> String {
		format!("invalid JSON at byte {}: {message}", self.position)
	}

	fn rest(&self) -> &str {
		&self.text[self.position..]
	}

	fn skip_whitespace(&mut self) {
		let rest = self.rest();
		self.position += rest.len() - rest.trim_start().len();
	}

	fn expect(&mut self, token: &str) -> Result<(), String> {
		self.skip_whitespace();
		if self.rest().starts_with(token) {
			self.position += token.len();
			Ok(())
		} else {
			Err(self.error(&format!("expected {token:?}")))
		}
	}

	fn value(&mut self) -> Result<JsonValue, String> {
		self.skip_whitespace();
		let Some(first) = self.rest().chars().next() else {
			return Err(self.error("unexpected end"));
		};
		match first {
			'{' => self.object(),
			'[' => self.array(),
			'"' => self.string().map(JsonValue::String),
			't' => self.expect("true").map(|()| JsonValue::Bool(true)),
			'f' => self.expect("false").map(|()| JsonValue::Bool(false)),
			'n' => self.expect("null").map(|()| JsonValue::Null),
			_ => self.number(),
		}
	}

	fn object(&mut self) -> Result<JsonValue, String> {
		self.expect("{")?;
		let mut fields = Vec::new();
		self.skip_whitespace();
		if self.rest().starts_with('}') {
			self.position += 1;
			return Ok(JsonValue::Object(fields));
		}
		loop {
			self.skip_whitespace();
			let name = self.string()?;
			self.expect(":")?;
			fields.push((name, self.value()?));
			self.skip_whitespace();
			match self.rest().chars().next() {
				Some(',') => self.position += 1,
				Some('}') => {
					self.position += 1;
					return Ok(JsonValue::Object(fields));
				}
				_ => return Err(self.error("expected ',' or '}'")),
			}
		}
	}

	fn array(&mut self) -> Result<JsonValue, String> {
		self.expect("[")?;
		let mut items = Vec::new();
		self.skip_whitespace();
		if self.rest().starts_with(']') {
			self.position += 1;
			return Ok(JsonValue::Array(items));
		}
		loop {
			items.push(self.value()?);
			self.skip_whitespace();
			match self.rest().chars().next() {
				Some(',') => self.position += 1,
				Some(']') => {
					self.position += 1;
					return Ok(JsonValue::Array(items));
				}
				_ => return Err(self.error("expected ',' or ']'")),
			}
		}
	}

	fn string(&mut self) -> Result<String, String> {
		self.expect("\"")?;
		let mut value = String::new();
		loop {
			let mut characters = self.rest().chars();
			let Some(character) = characters.next() else {
				return Err(self.error("unterminated string"));
			};
			self.position += character.len_utf8();
			match character {
				'"' => return Ok(value),
				'\\' => value.push(self.escape()?),
				_ => value.push(character),
			}
		}
	}

	fn escape(&mut self) -> Result<char, String> {
		let Some(escaped) = self.rest().chars().next() else {
			return Err(self.error("unterminated escape"));
		};
		self.position += 1;
		Ok(match escaped {
			'"' => '"',
			'\\' => '\\',
			'/' => '/',
			'b' => '\u{8}',
			'f' => '\u{c}',
			'n' => '\n',
			'r' => '\r',
			't' => '\t',
			'u' => {
				let high = self.hex_unit()?;
				if (0xD800..0xDC00).contains(&high) {
					self.expect("\\u")?;
					let low = self.hex_unit()?;
					let combined = 0x10000 + ((high - 0xD800) << 10) + (low.wrapping_sub(0xDC00));
					char::from_u32(combined).ok_or_else(|| self.error("invalid surrogate pair"))?
				} else {
					char::from_u32(high).ok_or_else(|| self.error("invalid \\u escape"))?
				}
			}
			_ => return Err(self.error("invalid escape")),
		})
	}

	fn hex_unit(&mut self) -> Result<u32, String> {
		let digits = self.rest().get(..4).ok_or_else(|| self.error("short \\u escape"))?;
		let unit = u32::from_str_radix(digits, 16).map_err(|_| self.error("invalid \\u escape"))?;
		self.position += 4;
		Ok(unit)
	}

	fn number(&mut self) -> Result<JsonValue, String> {
		let length = self
			.rest()
			.find(|character: char| !(character.is_ascii_digit() || "+-.eE".contains(character)))
			.unwrap_or(self.rest().len());
		let number = self.rest()[..length].parse().map_err(|_| self.error("invalid value"))?;
		self.position += length;
		Ok(JsonValue::Number(number))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_nested_values() {
		let value = parse_json(r#" {"a": [1, -2.5e1, true, null], "b": "x\"\u00e4\ud83d\ude00"} "#)
			.expect("valid JSON");
		assert_eq!(
			value.field("a"),
			Some(&JsonValue::Array(vec![
				JsonValue::Number(1.0),
				JsonValue::Number(-25.0),
				JsonValue::Bool(true),
				JsonValue::Null
			]))
		);
		assert_eq!(value.field("b"), Some(&JsonValue::String("x\"ä😀".to_owned())));
	}

	#[test]
	fn rejects_malformed_input() {
		for input in ["", "[1,", "{\"a\" 1}", "\"open", "[1] x", "\"\\q\""] {
			assert!(parse_json(input).is_err(), "{input}");
		}
	}
}
