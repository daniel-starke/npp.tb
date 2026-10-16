//! Extracts integer constants, plain enums and struct declarations from C/C++ headers.
//!
//! This is not a C preprocessor. It understands exactly what the Notepad++ and Scintilla
//! interface headers use: object-like `#define NAME <integer expression>`, `const`/`constexpr`
//! integer variables, `enum Name { ... }` and simple `struct Name { ... }` declarations.
//! Anything it cannot evaluate is skipped.

use std::collections::HashMap;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedHeader {
	/// In declaration order. Names already defined by an earlier header or line are skipped.
	pub constants: Vec<(String, i64)>,
	pub enums: Vec<EnumDeclaration>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct EnumDeclaration {
	pub name: String,
	pub members: Vec<(String, i64)>,
}

/// `known` holds the values of previously parsed headers and system macros (e.g. `WM_USER`);
/// new constants are added to it.
pub fn parse_header(source: &str, known: &mut HashMap<String, i64>) -> ParsedHeader {
	let source = strip_comments(&source.replace("\\\r\n", " ").replace("\\\n", " "));
	let mut parsed = ParsedHeader::default();
	for line in source.lines() {
		let line = line.trim();
		let definition = if let Some(rest) = line.strip_prefix("#define") {
			parse_define(rest)
		} else {
			parse_constant_variable(line)
		};
		let Some((name, expression)) = definition else { continue };
		if known.contains_key(&name) {
			continue;
		}
		if let Some(value) = evaluate(&expression, known) {
			known.insert(name.clone(), value);
			parsed.constants.push((name, value));
		}
	}
	parsed.enums = parse_enums(&source, known);
	parsed
}

/// Field declarations of `struct name { ... }`, whitespace normalized and default member
/// initializers removed, e.g. `["HWND _nppHandle", ...]`.
pub fn struct_fields(source: &str, name: &str) -> Option<Vec<String>> {
	let source = strip_comments(source);
	let mut search_from = 0;
	while let Some(found) = source[search_from..].find("struct") {
		let after_keyword = search_from + found + "struct".len();
		search_from = after_keyword;
		let rest = source[after_keyword..].trim_start();
		let Some(after_name) = rest.strip_prefix(name) else { continue };
		let Some(body) = after_name.trim_start().strip_prefix('{') else { continue };
		let body = &body[..matching_brace_end(body)?];
		return Some(
			body.split(';')
				.map(|field| {
					let field = field.split('=').next().unwrap_or_default();
					field.split_whitespace().collect::<Vec<_>>().join(" ")
				})
				.filter(|field| !field.is_empty())
				.collect(),
		);
	}
	None
}

/// Byte offset of the `}` closing a block whose opening brace precedes `text`.
fn matching_brace_end(text: &str) -> Option<usize> {
	let mut depth = 0usize;
	for (offset, character) in text.char_indices() {
		match character {
			'{' => depth += 1,
			'}' if depth == 0 => return Some(offset),
			'}' => depth -= 1,
			_ => {}
		}
	}
	None
}

fn parse_define(rest: &str) -> Option<(String, String)> {
	if !rest.starts_with([' ', '\t']) {
		return None;
	}
	let rest = rest.trim_start();
	let name_length = identifier_length(rest);
	// `#define NAME(` is a function-like macro.
	if name_length == 0 || rest[name_length..].starts_with('(') {
		return None;
	}
	let expression = rest[name_length..].trim();
	(!expression.is_empty()).then(|| (rest[..name_length].to_owned(), expression.to_owned()))
}

/// `const int menuItemSize = 64;` or `constexpr ULONG dmfInit = 0x0000000BUL;`
fn parse_constant_variable(line: &str) -> Option<(String, String)> {
	let declaration = line.strip_prefix("constexpr ").or_else(|| line.strip_prefix("const "))?;
	let (left, right) = declaration.strip_suffix(';')?.split_once('=')?;
	let name = left.split_whitespace().last()?;
	(identifier_length(name) == name.len()).then(|| (name.to_owned(), right.trim().to_owned()))
}

fn parse_enums(source: &str, known: &mut HashMap<String, i64>) -> Vec<EnumDeclaration> {
	let mut enums = Vec::new();
	let mut search_from = 0;
	while let Some(found) = source[search_from..].find("enum") {
		let keyword_start = search_from + found;
		search_from = keyword_start + "enum".len();
		let is_word_start = keyword_start == 0
			|| !source[..keyword_start]
				.ends_with(|character: char| is_identifier_character(character));
		let rest = &source[search_from..];
		if !is_word_start || !rest.starts_with(char::is_whitespace) {
			continue;
		}
		let rest = rest.trim_start();
		let name_length = identifier_length(rest);
		let name = &rest[..name_length];
		// Scoped enums have no global constants.
		if name_length == 0 || name == "class" || name == "struct" {
			continue;
		}
		let Some(body) = rest[name_length..].trim_start().strip_prefix('{') else { continue };
		let Some(body_end) = body.find('}') else { continue };
		let mut members = Vec::new();
		let mut next_value = 0;
		for member in body[..body_end].split(',').map(str::trim).filter(|member| !member.is_empty())
		{
			let (member_name, value) = match member.split_once('=') {
				Some((member_name, expression)) => {
					let Some(value) = evaluate(expression.trim(), known) else { break };
					(member_name.trim(), value)
				}
				None => (member, next_value),
			};
			known.insert(member_name.to_owned(), value);
			members.push((member_name.to_owned(), value));
			next_value = value + 1;
		}
		enums.push(EnumDeclaration { name: name.to_owned(), members });
	}
	enums
}

/// Replaces comments by a space (block comments keep their line breaks), leaving string and
/// character literals intact.
fn strip_comments(source: &str) -> String {
	let mut output = String::with_capacity(source.len());
	let mut characters = source.chars().peekable();
	while let Some(character) = characters.next() {
		match character {
			'/' if characters.peek() == Some(&'/') => {
				while characters.next_if(|&next| next != '\n').is_some() {}
				output.push(' ');
			}
			'/' if characters.peek() == Some(&'*') => {
				characters.next();
				let mut previous = '\0';
				for next in characters.by_ref() {
					if next == '\n' {
						output.push('\n');
					}
					if previous == '*' && next == '/' {
						break;
					}
					previous = next;
				}
				output.push(' ');
			}
			'"' | '\'' => {
				output.push(character);
				let mut is_escaped = false;
				for next in characters.by_ref() {
					output.push(next);
					if next == character && !is_escaped {
						break;
					}
					is_escaped = next == '\\' && !is_escaped;
				}
			}
			_ => output.push(character),
		}
	}
	output
}

fn is_identifier_character(character: char) -> bool {
	character.is_ascii_alphanumeric() || character == '_'
}

fn identifier_length(text: &str) -> usize {
	if text.starts_with(|character: char| character.is_ascii_digit()) {
		return 0;
	}
	text.find(|character| !is_identifier_character(character)).unwrap_or(text.len())
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
	Number(i64),
	Identifier(String),
	Operator(&'static str),
}

/// Evaluates integer constant expressions with `+ - * / % | & ^ ~ << >>` and parentheses.
pub fn evaluate(expression: &str, known: &HashMap<String, i64>) -> Option<i64> {
	let tokens = tokenize(expression)?;
	let mut parser = ExpressionParser { tokens: &tokens, position: 0, known };
	let value = parser.binary(0)?;
	(parser.position == tokens.len()).then_some(value)
}

fn tokenize(expression: &str) -> Option<Vec<Token>> {
	const OPERATORS: [&str; 15] =
		["<<", ">>", "+", "-", "*", "/", "%", "|", "&", "^", "~", "(", ")", "!", ","];
	let mut tokens = Vec::new();
	let mut rest = expression.trim_start();
	while !rest.is_empty() {
		if let Some(operator) = OPERATORS.into_iter().find(|operator| rest.starts_with(operator)) {
			tokens.push(Token::Operator(operator));
			rest = &rest[operator.len()..];
		} else if rest.starts_with(|character: char| character.is_ascii_digit()) {
			let length =
				rest.find(|character| !is_identifier_character(character)).unwrap_or(rest.len());
			tokens.push(Token::Number(parse_integer_literal(&rest[..length])?));
			rest = &rest[length..];
		} else {
			let length = identifier_length(rest);
			if length == 0 {
				return None;
			}
			tokens.push(Token::Identifier(rest[..length].to_owned()));
			rest = &rest[length..];
		}
		rest = rest.trim_start();
	}
	Some(tokens)
}

fn parse_integer_literal(literal: &str) -> Option<i64> {
	let digits = literal.trim_end_matches(['u', 'U', 'l', 'L']);
	if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
		i64::from_str_radix(hex, 16).ok()
	} else if digits.len() > 1 && digits.starts_with('0') {
		i64::from_str_radix(&digits[1..], 8).ok()
	} else {
		digits.parse().ok()
	}
}

struct ExpressionParser<'a> {
	tokens: &'a [Token],
	position: usize,
	known: &'a HashMap<String, i64>,
}

impl ExpressionParser<'_> {
	/// Precedence climbing. Higher numbers bind tighter (C precedence).
	fn binary(&mut self, minimum_precedence: u8) -> Option<i64> {
		let mut left = self.unary()?;
		while let Some(Token::Operator(operator)) = self.tokens.get(self.position) {
			let precedence = match *operator {
				"|" => 1,
				"^" => 2,
				"&" => 3,
				"<<" | ">>" => 4,
				"+" | "-" => 5,
				"*" | "/" | "%" => 6,
				_ => break,
			};
			if precedence < minimum_precedence {
				break;
			}
			self.position += 1;
			let right = self.binary(precedence + 1)?;
			left = match *operator {
				"|" => left | right,
				"^" => left ^ right,
				"&" => left & right,
				"<<" => left.checked_shl(u32::try_from(right).ok()?)?,
				">>" => left.checked_shr(u32::try_from(right).ok()?)?,
				"+" => left.checked_add(right)?,
				"-" => left.checked_sub(right)?,
				"*" => left.checked_mul(right)?,
				"/" => left.checked_div(right)?,
				_ => left.checked_rem(right)?,
			};
		}
		Some(left)
	}

	fn unary(&mut self) -> Option<i64> {
		let token = self.tokens.get(self.position)?.clone();
		self.position += 1;
		match token {
			Token::Number(value) => Some(value),
			Token::Identifier(name) => self.known.get(&name).copied(),
			Token::Operator("-") => self.unary()?.checked_neg(),
			Token::Operator("+") => self.unary(),
			Token::Operator("~") => Some(!self.unary()?),
			Token::Operator("(") => {
				let value = self.binary(0)?;
				(self.tokens.get(self.position) == Some(&Token::Operator(")"))).then(|| {
					self.position += 1;
					value
				})
			}
			_ => None,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn evaluates_expressions_referring_to_known_constants() {
		let known = HashMap::from([("WM_USER".to_owned(), 0x400)]);
		assert_eq!(evaluate("(WM_USER + 1000)", &known), Some(2024));
		assert_eq!(evaluate("0x0000000BUL", &known), Some(11));
		assert_eq!(evaluate("-1", &known), Some(-1));
		assert_eq!(evaluate("1 << 4 | 1", &known), Some(17));
		assert_eq!(evaluate("2 + 3 * 4", &known), Some(14));
		assert_eq!(evaluate("UNKNOWN + 1", &known), None);
		assert_eq!(evaluate("Sci_CharacterRange", &known), None);
	}

	#[test]
	fn parses_defines_constants_and_enums() {
		let header = r#"
			#define NPPMSG  (WM_USER + 1000)
				#define NPPM_GETCURRENTSCINTILLA (NPPMSG + 4) // trailing comment
			#define MACRO(x) (x)
			#define CharacterRange Sci_CharacterRange
			/* #define COMMENTED_OUT 1 */
			const int menuItemSize = 64;
			namespace NppDarkMode
			{
				constexpr ULONG dmfInit = 0x0000000BUL;
			}
			enum LangType {L_TEXT, L_PHP , \
				L_C, L_EXTERNAL = 90};
			enum class MacroStatus { Idle, Recording };
		"#;
		let mut known = HashMap::from([("WM_USER".to_owned(), 0x400)]);
		let parsed = parse_header(header, &mut known);
		assert_eq!(
			parsed.constants,
			[
				("NPPMSG".to_owned(), 2024),
				("NPPM_GETCURRENTSCINTILLA".to_owned(), 2028),
				("menuItemSize".to_owned(), 64),
				("dmfInit".to_owned(), 11),
			]
		);
		assert_eq!(
			parsed.enums,
			[EnumDeclaration {
				name: "LangType".to_owned(),
				members: vec![
					("L_TEXT".to_owned(), 0),
					("L_PHP".to_owned(), 1),
					("L_C".to_owned(), 2),
					("L_EXTERNAL".to_owned(), 90),
				],
			}]
		);
	}

	#[test]
	fn extracts_normalized_struct_fields() {
		let header = "struct FuncItem\n{\n\twchar_t _itemName[menuItemSize] = { '\\0' };\n\tPFUNCPLUGINCMD _pFunc = nullptr; // c\n};";
		assert_eq!(
			struct_fields(header, "FuncItem"),
			Some(vec![
				"wchar_t _itemName[menuItemSize]".to_owned(),
				"PFUNCPLUGINCMD _pFunc".to_owned()
			])
		);
		assert_eq!(struct_fields(header, "Func"), None);
	}
}
