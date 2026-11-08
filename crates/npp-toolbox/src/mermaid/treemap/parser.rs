//! The treemap grammar (`treemap.langium`, `valueConverter.ts`) and the database it fills
//! (`parser.ts`, `db.ts`, `utils.ts`).
//!
//! Chevrotain takes the first terminal that matches, in the order of the grammar. `INDENTATION`
//! (`[ \t]{1,}`) comes before the hidden whitespace, so every run of blanks is a token: blanks
//! at the end of a line or before `:::` are parse errors, as in Mermaid. `TITLE`, `ACC_TITLE` and
//! `ACC_DESCR` come first and take the blanks before them.

use std::collections::HashMap;

use super::super::common::CommonFields;
use super::super::langium::Scanner;
use super::super::value::parse_leading_number;
use super::super::{Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
	Boolean,
	/// `TITLE`, `ACC_TITLE` or `ACC_DESCR`.
	CommonField,
	Keyword,
	ClassDef,
	StyleSeparator,
	Separator,
	Comma,
	Indentation,
	Id,
	Number,
	String,
	/// A character no terminal matches, which Mermaid reports as a lexer error.
	Invalid,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Token<'a> {
	pub kind: Kind,
	pub text: &'a str,
	pub line: usize,
}

/// `classDef\s+([a-zA-Z_][a-zA-Z0-9_]+)(?:\s+([^;\r\n]*))?(?:;)?`: the length, the class name
/// and the style text. `\s+` also takes line ends, and with them the next line as style text.
fn class_def(text: &str) -> Option<(usize, &str, Option<&str>)> {
	let after = text.strip_prefix("classDef")?;
	let spaces = after.len() - after.trim_start().len();
	if spaces == 0 {
		return None;
	}
	let name_start = "classDef".len() + spaces;
	let name_length = text[name_start..]
		.bytes()
		.enumerate()
		.take_while(|&(index, byte)| match index {
			0 => byte.is_ascii_alphabetic() || byte == b'_',
			_ => byte.is_ascii_alphanumeric() || byte == b'_',
		})
		.count();
	if name_length < 2 {
		return None;
	}
	let name_end = name_start + name_length;
	let mut end = name_end;
	let after_name = &text[name_end..];
	let blanks = after_name.len() - after_name.trim_start().len();
	let mut style = None;
	if blanks > 0 {
		let style_start = name_end + blanks;
		let length =
			text[style_start..].find([';', '\r', '\n']).unwrap_or(text.len() - style_start);
		style = Some(&text[style_start..style_start + length]);
		end = style_start + length;
	}
	if text[end..].starts_with(';') {
		end += 1;
	}
	Some((end, &text[name_start..name_end], style))
}

/// The length of the terminal of `kind` that matches first at the start of `rest`, `None` for
/// the hidden comments and line ends.
fn token_at(rest: &str) -> (Option<Kind>, usize) {
	for word in ["true", "false"] {
		if rest.starts_with(word) {
			return (Some(Kind::Boolean), word.len());
		}
	}
	let mut scanner = Scanner::new(rest);
	let mut fields = CommonFields::default();
	if scanner.common_field(&mut fields) {
		return (Some(Kind::CommonField), rest.len() - scanner.rest().len());
	}
	for keyword in ["treemap-beta", "treemap"] {
		if rest.starts_with(keyword) {
			return (Some(Kind::Keyword), keyword.len());
		}
	}
	if let Some((length, _, _)) = class_def(rest) {
		return (Some(Kind::ClassDef), length);
	}
	if rest.starts_with(":::") {
		return (Some(Kind::StyleSeparator), 3);
	}
	let span = |predicate: fn(u8) -> bool| rest.bytes().take_while(|&byte| predicate(byte)).count();
	let first = rest.as_bytes()[0];
	match first {
		b':' => (Some(Kind::Separator), 1),
		b',' => (Some(Kind::Comma), 1),
		b' ' | b'\t' => (Some(Kind::Indentation), span(|byte| byte == b' ' || byte == b'\t')),
		b'%' if rest.starts_with("%%") => (None, rest.find('\n').unwrap_or(rest.len())),
		b'\n' => (None, 1),
		b'\r' if rest.starts_with("\r\n") => (None, 2),
		byte if byte.is_ascii_alphabetic() || byte == b'_' => {
			(Some(Kind::Id), span(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
		}
		byte if byte.is_ascii_digit() || byte == b'.' || byte == b',' => (
			Some(Kind::Number),
			span(|byte| byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b',')),
		),
		b'"' | b'\'' => match rest[1..].find(char::from(first)) {
			Some(end) => (Some(Kind::String), end + 2),
			None => (Some(Kind::Invalid), 1),
		},
		_ => (Some(Kind::Invalid), rest.chars().next().map_or(1, char::len_utf8)),
	}
}

pub(super) fn lex(code: &str) -> Vec<Token<'_>> {
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	while position < code.len() {
		let rest = &code[position..];
		let (kind, length) = token_at(rest);
		if let Some(kind) = kind {
			tokens.push(Token { kind, text: &rest[..length], line });
		}
		line += rest[..length].matches('\n').count();
		position += length;
	}
	tokens
}

/// A row of the treemap: a section or a leaf, with its indentation.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Item {
	pub level: usize,
	pub name: String,
	/// `Some` for leaves: the value as `parseFloat` reads it, `NaN` where it reads none.
	pub value: Option<f64>,
	pub class_selector: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct Treemap {
	pub fields: CommonFields,
	pub items: Vec<Item>,
	/// The styles of each class, in the order of their `classDef`s.
	pub classes: HashMap<String, Vec<String>>,
}

fn describe(token: Option<&Token>) -> String {
	match token {
		None => "the end of the diagram".to_owned(),
		Some(token) if token.kind == Kind::Indentation => "blanks".to_owned(),
		Some(token) => format!("{:?}", token.text),
	}
}

struct Parser<'a, 'c, 'm> {
	tokens: Vec<Token<'a>>,
	index: usize,
	context: &'c Context<'m>,
}

impl<'a> Parser<'a, '_, '_> {
	fn peek(&self, offset: usize) -> Option<Kind> {
		self.tokens.get(self.index + offset).map(|token| token.kind)
	}

	fn is(&self, kind: Kind) -> bool {
		self.peek(0) == Some(kind)
	}

	fn advance(&mut self) -> Option<Token<'a>> {
		let token = self.tokens.get(self.index).copied();
		self.index += 1;
		token
	}

	fn error<T>(&self, expected: &str) -> Result<T> {
		let token = self.tokens.get(self.index);
		let line = token.or(self.tokens.last()).map_or(1, |token| token.line);
		let message = match token {
			Some(token) if token.kind == Kind::Invalid => {
				format!("unexpected character {:?}", token.text)
			}
			_ => format!("expected {expected}, found {}", describe(token)),
		};
		Err(self.context.error_at(line, message))
	}

	fn expect(&mut self, kind: Kind, expected: &str) -> Result<Token<'a>> {
		match self.is(kind) {
			true => Ok(self.advance().unwrap_or(Token { kind, text: "", line: 1 })),
			false => self.error(expected),
		}
	}

	/// `(STYLE_SEPARATOR classSelector=ID2)?`
	fn class_selector(&mut self) -> Result<Option<String>> {
		if !self.is(Kind::StyleSeparator) {
			return Ok(None);
		}
		self.advance();
		Ok(Some(self.expect(Kind::Id, "a class name")?.text.to_owned()))
	}

	/// `Item`: a leaf when a separator follows the name, else a section.
	fn item(&mut self, level: usize) -> Result<Item> {
		let name = self.expect(Kind::String, "a quoted name or `classDef`")?.text;
		let name = name[1..name.len() - 1].to_owned();
		let separator_offset = usize::from(self.is(Kind::Indentation));
		let is_leaf = matches!(self.peek(separator_offset), Some(Kind::Separator | Kind::Comma));
		if !is_leaf {
			return Ok(Item { level, name, value: None, class_selector: self.class_selector()? });
		}
		self.index += separator_offset + 1;
		if self.is(Kind::Indentation) {
			self.advance();
		}
		let number = self.expect(Kind::Number, "a number")?.text.replace(',', "");
		let value = parse_leading_number(&number).unwrap_or(f64::NAN);
		Ok(Item { level, name, value: Some(value), class_selector: self.class_selector()? })
	}
}

/// `addClass`: escaped commas stay, other commas separate styles like semicolons.
fn add_class(classes: &mut HashMap<String, Vec<String>>, name: &str, style: &str) {
	let styles = classes.entry(name.to_owned()).or_default();
	let protected = style.replace("\\,", "\u{0}");
	for part in protected.split([',', ';']) {
		styles.push(part.replace('\u{0}', ","));
	}
}

pub(super) fn parse(code: &str, context: &Context) -> Result<Treemap> {
	let mut parser = Parser { tokens: lex(code), index: 0, context };
	let mut treemap = Treemap { fields: CommonFields::new(context), ..Treemap::default() };
	parser.expect(Kind::Keyword, "`treemap`")?;
	let mut class_defs = Vec::new();
	while let Some(token) = parser.tokens.get(parser.index).copied() {
		match token.kind {
			Kind::CommonField => {
				parser.advance();
				Scanner::new(token.text).common_field(&mut treemap.fields);
			}
			Kind::Indentation | Kind::String | Kind::ClassDef => {
				let level = match token.kind {
					Kind::Indentation => parser.advance().map_or(0, |token| token.text.len()),
					_ => 0,
				};
				match parser.is(Kind::ClassDef) {
					true => {
						let text = parser.advance().map_or("", |token| token.text);
						if let Some((_, name, style)) = class_def(text) {
							class_defs
								.push((name.to_owned(), style.unwrap_or_default().to_owned()));
						}
					}
					false => {
						let item = parser.item(level)?;
						treemap.items.push(item);
					}
				}
			}
			_ => return parser.error("a quoted name or `classDef`"),
		}
	}
	for (name, style) in class_defs {
		add_class(&mut treemap.classes, &name, &style);
	}
	Ok(treemap)
}

/// A node of the hierarchy that `buildHierarchy` makes of the rows.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct TreeNode {
	pub name: String,
	/// `None` for leaves, which cannot have children.
	pub children: Option<Vec<usize>>,
	pub value: Option<f64>,
	pub class_selector: Option<String>,
	pub styles: Option<Vec<String>>,
}

/// `buildHierarchy`: the nodes and the top-level ones. An item belongs to the last section
/// before it that is less indented.
pub(super) fn build_hierarchy(treemap: &Treemap) -> (Vec<TreeNode>, Vec<usize>) {
	let mut nodes: Vec<TreeNode> = Vec::with_capacity(treemap.items.len());
	let mut roots = Vec::new();
	let mut stack: Vec<(usize, usize)> = Vec::new();
	for item in &treemap.items {
		let styles = item
			.class_selector
			.as_ref()
			.and_then(|class| treemap.classes.get(class))
			.filter(|styles| !styles.is_empty())
			.cloned();
		let index = nodes.len();
		nodes.push(TreeNode {
			name: item.name.clone(),
			children: if item.value.is_some() { None } else { Some(Vec::new()) },
			value: item.value,
			class_selector: item.class_selector.clone(),
			styles,
		});
		while stack.last().is_some_and(|&(_, level)| level >= item.level) {
			stack.pop();
		}
		match stack.last() {
			Some(&(parent, _)) => nodes[parent].children.get_or_insert_with(Vec::new).push(index),
			None => roots.push(index),
		}
		if item.value.is_none() {
			stack.push((index, item.level));
		}
	}
	(nodes, roots)
}

#[cfg(test)]
mod tests {
	use super::super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<Treemap> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	fn kinds(code: &str) -> Vec<Kind> {
		lex(code).iter().map(|token| token.kind).collect()
	}

	#[test]
	fn lexes_like_chevrotain() {
		use Kind as K;
		assert_eq!(
			kinds("treemap\n  \"a\": 1,000:::c1 %% x\n  title A"),
			[
				K::Keyword,
				K::Indentation,
				K::String,
				K::Separator,
				K::Indentation,
				K::Number,
				K::StyleSeparator,
				K::Id,
				K::Indentation,
				K::CommonField
			]
		);
		assert_eq!(kinds("titleX"), [K::CommonField, K::Id]);
		assert_eq!(kinds("classDef c x"), [K::Id, K::Indentation, K::Id, K::Indentation, K::Id]);
		assert_eq!(class_def("classDef c1\n  \"a\": 1"), Some((20, "c1", Some("\"a\": 1"))));
		assert_eq!(class_def("classDef c1 fill:#f00;x"), Some((22, "c1", Some("fill:#f00"))));
		assert_eq!(kinds("#"), [K::Invalid]);
	}

	#[test]
	fn parses_rows_and_classes() {
		let treemap = parsed(
			"treemap-beta\ntitle My Tree\n\"Root\":::c1\n  \"a\" : 1,000.5\n  'b', 1_000:::c2\n    \"c\"\nclassDef c1 fill:#f00\\,x,color:blue\nclassDef c1 stroke:red",
		)
		.unwrap();
		assert_eq!(treemap.fields.title.as_deref(), Some("My Tree"));
		let names: Vec<_> =
			treemap.items.iter().map(|item| (item.name.as_str(), item.level, item.value)).collect();
		assert_eq!(
			names,
			[("Root", 0, None), ("a", 2, Some(1000.5)), ("b", 2, Some(1.0)), ("c", 4, None)]
		);
		assert_eq!(treemap.classes["c1"], ["fill:#f00,x", "color:blue", "stroke:red"]);
		let (nodes, roots) = build_hierarchy(&treemap);
		assert_eq!(roots, [0]);
		assert_eq!(nodes[0].children, Some(vec![1, 2, 3]));
		assert_eq!(nodes[0].styles.as_ref().map(Vec::len), Some(3));
	}

	#[test]
	fn rejects_what_mermaid_rejects() {
		for code in [
			"treemap\n\"Root\"\n  \"a\": 5 :::c1",
			"treemap\n\"Root\"   \n  \"a\": 5",
			"treemap\n\"Root\"\n  %% comment\n  \"a\": 5",
			"treemap\n\"Root\"\n  \"a\" 5",
			"treemap\ntitleX",
			"treemap\n\"Root\"\nclassDef c fill:#f00",
			"\"Root\"",
		] {
			assert!(parsed(code).is_err(), "{code}");
		}
		assert_eq!(parsed("treemap\n\"Root\"\n  \"a\" 5").err().unwrap().line, Some(3));
		assert!(parsed("treemap\n\"Root\"\n  \"a\": 1 \"b\": 2").is_ok());
	}
}
