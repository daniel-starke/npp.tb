//! The grammar of `block.jison`: jison's first matching rule wins (case-sensitive; rules ending
//! with a word character end at a word boundary), exclusive states select the rules, and the
//! actions build the statements like the grammar's actions do.

use crate::mermaid::jison::{is_word_byte, span, word_length};

/// `keyword`
fn keyword(text: &str, keyword: &str) -> bool {
	text.starts_with(keyword)
		&& !text.as_bytes().get(keyword.len()).copied().is_some_and(is_word_byte)
}

/// The block types of `typeStr2Type`, `na` for unknown shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType {
	Na,
	Round,
	BlockArrow,
	Space,
	Square,
	Diamond,
	Hexagon,
	LeanRight,
	LeanLeft,
	Trapezoid,
	InvTrapezoid,
	RectLeftInvArrow,
	Circle,
	Stadium,
	Subroutine,
	Cylinder,
	DoubleCircle,
	Composite,
}

/// `typeStr2Type`: the type of the delimiters around a label.
fn type_of(delimiters: &str) -> BlockType {
	match delimiters {
		"[]" => BlockType::Square,
		"()" => BlockType::Round,
		"(())" => BlockType::Circle,
		">]" => BlockType::RectLeftInvArrow,
		"{}" => BlockType::Diamond,
		"{{}}" => BlockType::Hexagon,
		"([])" => BlockType::Stadium,
		"[[]]" => BlockType::Subroutine,
		"[()]" => BlockType::Cylinder,
		"((()))" => BlockType::DoubleCircle,
		"[//]" => BlockType::LeanRight,
		"[\\\\]" => BlockType::LeanLeft,
		"[/\\]" => BlockType::Trapezoid,
		"[\\/]" => BlockType::InvTrapezoid,
		"<[]>" => BlockType::BlockArrow,
		_ => BlockType::Na,
	}
}

/// A block as a statement gives it. `None` stands for JavaScript's `undefined`, which a chain
/// of edges leaves: `a --> b --> c` reads the fields of the array of `a --> b`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockStatement {
	pub id: Option<String>,
	pub label: Option<String>,
	pub kind: Option<BlockType>,
	pub directions: Option<Vec<String>>,
	pub width_in_columns: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgeStatement {
	/// `start-end`, with `undefined` for a missing start.
	pub id: String,
	pub start: Option<String>,
	pub end: String,
	pub label: String,
	pub is_thick: bool,
	pub is_dotted: bool,
	pub arrow_type_start: &'static str,
	pub arrow_type_end: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
	Block(BlockStatement),
	Edge(EdgeStatement),
	/// `columns N`, -1 for `auto`.
	Columns(f64),
	Space {
		id: String,
		width: f64,
	},
	Composite {
		block: BlockStatement,
		children: Vec<Statement>,
	},
	ClassDef {
		id: String,
		css: String,
	},
	ApplyClass {
		ids: String,
		class: String,
	},
	ApplyStyles {
		ids: String,
		styles: String,
	},
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TokenKind {
	DiagramKey,
	IdBlock,
	Columns,
	SpaceBlock,
	Default,
	LinkStyle,
	Interpolate,
	ClassDef,
	ClassDefId,
	DefaultClassDefId,
	ClassDefStyleOpts,
	Class,
	ClassEntityIds,
	StyleClass,
	Style,
	StyleEntityIds,
	StyleDefinitionData,
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	End,
	NodeDStart,
	NodeDEnd,
	BlockArrowStart,
	BlockArrowEnd,
	NodeId,
	MdStr,
	Str,
	Dir,
	Link,
	StartLink,
	LinkLabel,
	Size,
	/// Text no rule matches.
	Invalid,
	Eof,
}

#[derive(Clone, Debug)]
struct Token {
	kind: TokenKind,
	/// `yytext` as the rule's action leaves it.
	text: String,
	line: usize,
	/// The newlines of the matched text.
	newlines: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
	Initial,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
	String,
	MdString,
	Node,
	BlockArrow,
	ArrowDir,
	LinkLabel,
	Class,
	ClassStyle,
	ClassDef,
	ClassDefId,
	StyleStatement,
	StyleDefinition,
}

/// Bounds the lexer state stack.
const MAX_STATES: usize = 128;
/// Bounds the nesting of composite blocks. The layout (`setBlockSizes`) sizes each child twice,
/// so its time doubles with every level.
pub const MAX_NESTING: usize = 12;

#[derive(Debug)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type ParseResult<T> = Result<T, ParseError>;

/// The shape starts of `INITIAL`, in the grammar's order (`")))"` never wins over `")"`).
const SHAPE_STARTS: &[&str] = &[
	"-)", "(-", "))", ")", "(((", "((", "{{", "{", ">", "([", "(", "[[", "[|", "[(", "[\\", "[/",
	"[",
];
/// The shape ends of `NODE`, in the grammar's order.
const SHAPE_ENDS: &[&str] = &[
	"(((", ")))", "))", "}}", "}", "(-", "-)", "((", "]]", "(", "])", "\\]", "/]", ")]", ")", "]>",
	"]",
];
const DIRECTIONS: &[&str] = &["right", "left", "x", "y", "up", "down"];

fn is_space(character: char) -> bool {
	character.is_whitespace()
}

/// `\s*[xo<]?<c><c>+[<c>xo>]\s*` for `c` = `-` or `=`: the length of the match.
fn long_link(text: &str, line: u8) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut position = span(text, is_space);
	if matches!(bytes.get(position), Some(b'x' | b'o' | b'<')) {
		position += 1;
	}
	let run = bytes[position..].iter().take_while(|&&byte| byte == line).count();
	if run < 2 {
		return None;
	}
	position += run;
	match bytes.get(position) {
		Some(b'x' | b'o' | b'>') => position += 1,
		// The last character of the run is the end.
		_ if run >= 3 => {}
		_ => return None,
	}
	Some(position + span(&text[position..], is_space))
}

/// `\s*[xo<]?\-?\.+\-[xo>]?\s*`
fn dotted_link(text: &str) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut position = span(text, is_space);
	if matches!(bytes.get(position), Some(b'x' | b'o' | b'<')) {
		position += 1;
	}
	if bytes.get(position) == Some(&b'-') {
		position += 1;
	}
	let dots = bytes[position..].iter().take_while(|&&byte| byte == b'.').count();
	if dots == 0 || bytes.get(position + dots) != Some(&b'-') {
		return None;
	}
	position += dots + 1;
	if matches!(bytes.get(position), Some(b'x' | b'o' | b'>')) {
		position += 1;
	}
	Some(position + span(&text[position..], is_space))
}

/// `\s*\~\~[\~]+\s*`
fn tilde_link(text: &str) -> Option<usize> {
	let position = span(text, is_space);
	let tildes = span(&text[position..], |character| character == '~');
	(tildes >= 3).then(|| position + tildes + span(&text[position + tildes..], is_space))
}

/// `\s*[xo<]?<start>\s*`: the start of a link with a label.
fn link_start(text: &str, start: &str) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut position = span(text, is_space);
	if matches!(bytes.get(position), Some(b'x' | b'o' | b'<'))
		&& !text[position..].starts_with(start)
	{
		position += 1;
	}
	if !text[position..].starts_with(start) {
		return None;
	}
	position += start.len();
	Some(position + span(&text[position..], is_space))
}

/// `(\w+)+((","\s*\w+)*)`
fn id_list_length(text: &str) -> usize {
	let mut length = word_length(text);
	if length == 0 {
		return 0;
	}
	loop {
		let rest = &text[length..];
		if !rest.starts_with(',') {
			return length;
		}
		let spaces = span(&rest[1..], is_space);
		let word = word_length(&rest[1 + spaces..]);
		if word == 0 {
			return length;
		}
		length += 1 + spaces + word;
	}
}

/// `keyword\s+`
fn keyword_and_space(text: &str, keyword: &str) -> Option<usize> {
	if !text.starts_with(keyword) {
		return None;
	}
	let spaces = span(&text[keyword.len()..], is_space);
	(spaces > 0).then_some(keyword.len() + spaces)
}

/// `keyword\s*"<delimiter>"\s*`
fn keyword_then(text: &str, keyword: &str, delimiter: char) -> Option<usize> {
	if !text.starts_with(keyword) {
		return None;
	}
	let after = keyword.len() + span(&text[keyword.len()..], is_space);
	if !text[after..].starts_with(delimiter) {
		return None;
	}
	let value = after + 1;
	Some(value + span(&text[value..], is_space))
}

fn digits_length(text: &str) -> usize {
	text.bytes().take_while(u8::is_ascii_digit).count()
}

struct Lexer<'a> {
	input: &'a str,
	position: usize,
	line: usize,
	states: Vec<Condition>,
	done: bool,
}

impl<'a> Lexer<'a> {
	fn new(input: &'a str) -> Self {
		Lexer { input, position: 0, line: 1, states: vec![Condition::Initial], done: false }
	}

	fn state(&self) -> Condition {
		self.states.last().copied().unwrap_or(Condition::Initial)
	}

	fn begin(&mut self, state: Condition) {
		if self.states.len() < MAX_STATES {
			self.states.push(state);
		}
	}

	fn pop_state(&mut self) {
		if self.states.len() > 1 {
			self.states.pop();
		}
	}

	fn take(&mut self, length: usize) -> &'a str {
		let text = &self.input[self.position..self.position + length];
		self.position += length;
		self.line += text.matches('\n').count();
		text
	}

	fn token(&mut self, kind: TokenKind, length: usize) -> Token {
		let line = self.line;
		let text = self.take(length);
		Token { kind, text: text.to_owned(), line, newlines: text.matches('\n').count() }
	}

	fn next_token(&mut self) -> Token {
		loop {
			if self.done {
				return self.eof();
			}
			// jison tries the rules once more on the empty rest; some of them match it.
			if self.position >= self.input.len() {
				self.done = true;
			}
			let rest = &self.input[self.position..];
			let result = match self.state() {
				Condition::Initial => self.initial(rest),
				Condition::String => self.string(rest),
				Condition::MdString => self.md_string(rest),
				Condition::Node => self.node(rest),
				Condition::BlockArrow => self.block_arrow(rest),
				Condition::ArrowDir => self.arrow_dir(rest),
				Condition::LinkLabel => self.link_label(rest),
				Condition::ClassDef => self.class_def(rest),
				state => self.rest_of_line(state, rest),
			};
			match result {
				Some(token) => return token,
				None if self.done => return self.eof(),
				None => {}
			}
		}
	}

	fn eof(&self) -> Token {
		Token { kind: TokenKind::Eof, text: String::new(), line: self.line, newlines: 0 }
	}

	fn invalid(&mut self, rest: &str) -> Option<Token> {
		if rest.is_empty() {
			return None;
		}
		Some(self.token(TokenKind::Invalid, rest.len()))
	}

	fn initial(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		if keyword(rest, "block-beta") {
			return Some(self.token(T::DiagramKey, "block-beta".len()));
		}
		if rest.starts_with("block:") {
			return Some(self.token(T::IdBlock, "block:".len()));
		}
		if keyword(rest, "block") {
			return Some(self.token(T::DiagramKey, "block".len()));
		}
		let blanks = span(rest, is_space);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		if let Some(length) = keyword_and_space(rest, "columns") {
			if keyword(&rest[length..], "auto") {
				let mut token = self.token(T::Columns, length + "auto".len());
				token.text = "-1".to_owned();
				return Some(token);
			}
			let digits = digits_length(&rest[length..]);
			if digits > 0 {
				let mut token = self.token(T::Columns, length + digits);
				token.text = token.text[length..].to_owned();
				return Some(token);
			}
		}
		if rest.starts_with("\"`") {
			self.take(2);
			self.begin(Condition::MdString);
			return None;
		}
		if rest.starts_with('"') {
			self.take(1);
			self.begin(Condition::String);
			return None;
		}
		if let Some(after) = rest.strip_prefix("space") {
			let digits = after.strip_prefix(':').map_or(0, digits_length);
			if digits > 0 {
				let mut token = self.token(T::SpaceBlock, "space:".len() + digits);
				token.text = token.text["space:".len()..].to_owned();
				return Some(token);
			}
			if keyword(rest, "space") {
				let mut token = self.token(T::SpaceBlock, "space".len());
				token.text = "1".to_owned();
				return Some(token);
			}
		}
		for (keyword, kind) in
			[("default", T::Default), ("linkStyle", T::LinkStyle), ("interpolate", T::Interpolate)]
		{
			if self::keyword(rest, keyword) {
				return Some(self.token(kind, keyword.len()));
			}
		}
		let statements = [
			("classDef", T::ClassDef, Condition::ClassDef),
			("class", T::Class, Condition::Class),
			("style", T::Style, Condition::StyleStatement),
		];
		for (keyword, kind, state) in statements {
			if let Some(length) = keyword_and_space(rest, keyword) {
				self.begin(state);
				return Some(self.token(kind, length));
			}
		}
		if let Some(length) = keyword_then(rest, "accTitle", ':') {
			self.begin(Condition::AccTitle);
			return Some(self.token(T::AccTitle, length));
		}
		if let Some(length) = keyword_then(rest, "accDescr", ':') {
			self.begin(Condition::AccDescr);
			return Some(self.token(T::AccDescr, length));
		}
		if let Some(length) = keyword_then(rest, "accDescr", '{') {
			self.take(length);
			self.begin(Condition::AccDescrMultiline);
			return None;
		}
		if keyword(rest, "end") {
			let length = 3 + span(&rest[3..], is_space);
			return Some(self.token(T::End, length));
		}
		if let Some(start) = SHAPE_STARTS.iter().find(|start| rest.starts_with(**start)) {
			self.begin(Condition::Node);
			return Some(self.token(T::NodeDStart, start.len()));
		}
		if rest.starts_with("<[") {
			self.begin(Condition::BlockArrow);
			return Some(self.token(T::BlockArrowStart, 2));
		}
		let id = span(rest, |character| {
			!matches!(character, '(' | '[' | '-' | ')' | '{' | '}' | '<' | '>' | ':' | '=')
				&& !is_space(character)
		});
		if id > 0 {
			return Some(self.token(T::NodeId, id));
		}
		if rest.is_empty() {
			return Some(self.eof());
		}
		let link = long_link(rest, b'-')
			.or_else(|| long_link(rest, b'='))
			.or_else(|| dotted_link(rest))
			.or_else(|| tilde_link(rest));
		if let Some(length) = link {
			return Some(self.token(T::Link, length));
		}
		let start = link_start(rest, "--")
			.or_else(|| link_start(rest, "=="))
			.or_else(|| link_start(rest, "-."));
		if let Some(length) = start {
			self.begin(Condition::LinkLabel);
			return Some(self.token(T::StartLink, length));
		}
		if let Some(after) = rest.strip_prefix(':') {
			let digits = digits_length(after);
			if digits > 0 {
				let mut token = self.token(T::Size, 1 + digits);
				token.text.remove(0);
				return Some(token);
			}
		}
		self.invalid(rest)
	}

	fn string(&mut self, rest: &str) -> Option<Token> {
		if rest.starts_with('"') {
			self.take(1);
			self.pop_state();
			return None;
		}
		let length = rest.find('"').unwrap_or(rest.len());
		Some(self.token(TokenKind::Str, length))
	}

	fn md_string(&mut self, rest: &str) -> Option<Token> {
		let length = rest.find(['`', '"']).unwrap_or(rest.len());
		if length > 0 {
			return Some(self.token(TokenKind::MdStr, length));
		}
		if rest.starts_with("`\"") {
			self.take(2);
			self.pop_state();
			return None;
		}
		self.invalid(rest)
	}

	/// The strings that `NODE`, `BLOCK_ARROW` and `LLABEL` start.
	fn quoted(&mut self, rest: &str, label_token: bool) -> Option<Option<Token>> {
		if rest.starts_with("\"`") {
			self.take(2);
			self.begin(Condition::MdString);
			return Some(None);
		}
		if rest.starts_with('"') {
			self.begin(Condition::String);
			if label_token {
				return Some(Some(self.token(TokenKind::LinkLabel, 1)));
			}
			self.take(1);
			return Some(None);
		}
		None
	}

	fn node(&mut self, rest: &str) -> Option<Token> {
		if let Some(end) = SHAPE_ENDS.iter().find(|end| rest.starts_with(**end)) {
			self.pop_state();
			return Some(self.token(TokenKind::NodeDEnd, end.len()));
		}
		match self.quoted(rest, false) {
			Some(token) => token,
			None => self.invalid(rest),
		}
	}

	fn block_arrow(&mut self, rest: &str) -> Option<Token> {
		if let Some(token) = self.quoted(rest, false) {
			return token;
		}
		// `"]>"\s*"("`
		if let Some(after) = rest.strip_prefix("]>") {
			let spaces = span(after, is_space);
			if after[spaces..].starts_with('(') {
				self.take(2 + spaces + 1);
				self.begin(Condition::ArrowDir);
				return None;
			}
		}
		self.invalid(rest)
	}

	fn arrow_dir(&mut self, rest: &str) -> Option<Token> {
		// `","?\s*<direction>\s*`, the comma and its blanks removed.
		let comma = usize::from(rest.starts_with(','));
		let spaces = span(&rest[comma..], is_space);
		let name_start = comma + spaces;
		if let Some(name) = DIRECTIONS.iter().find(|name| rest[name_start..].starts_with(**name)) {
			let end = name_start + name.len();
			let length = end + span(&rest[end..], is_space);
			let mut token = self.token(TokenKind::Dir, length);
			if comma == 1 {
				token.text = token.text[name_start..].to_owned();
			}
			return Some(token);
		}
		if let Some(after) = rest.strip_prefix(')') {
			let length = 1 + span(after, is_space);
			self.pop_state();
			self.pop_state();
			let mut token = self.token(TokenKind::BlockArrowEnd, length);
			token.text = "]>".to_owned();
			return Some(token);
		}
		self.invalid(rest)
	}

	fn link_label(&mut self, rest: &str) -> Option<Token> {
		if let Some(token) = self.quoted(rest, true) {
			return token;
		}
		let link =
			long_link(rest, b'-').or_else(|| long_link(rest, b'=')).or_else(|| dotted_link(rest));
		if let Some(length) = link {
			self.pop_state();
			return Some(self.token(TokenKind::Link, length));
		}
		self.invalid(rest)
	}

	fn class_def(&mut self, rest: &str) -> Option<Token> {
		// `DEFAULT\s+`, else `\w+\s+`.
		let default_spaces = span(rest.get("DEFAULT".len()..).unwrap_or(""), is_space);
		let (kind, word) = match rest.starts_with("DEFAULT") && default_spaces > 0 {
			true => (TokenKind::DefaultClassDefId, "DEFAULT".len()),
			false => (TokenKind::ClassDefId, word_length(rest)),
		};
		let spaces = span(&rest[word..], is_space);
		if word == 0 || spaces == 0 {
			return self.invalid(rest);
		}
		self.pop_state();
		self.begin(Condition::ClassDefId);
		Some(self.token(kind, word + spaces))
	}

	/// The states that take the rest of the line, or the ids before it.
	fn rest_of_line(&mut self, state: Condition, rest: &str) -> Option<Token> {
		let line = span(rest, |character| character != '\n');
		let kind = match state {
			Condition::Class | Condition::StyleStatement => {
				let ids = id_list_length(rest);
				if ids == 0 {
					return self.invalid(rest);
				}
				let (kind, next) = match state {
					Condition::Class => (TokenKind::ClassEntityIds, Condition::ClassStyle),
					_ => (TokenKind::StyleEntityIds, Condition::StyleDefinition),
				};
				self.pop_state();
				self.begin(next);
				return Some(self.token(kind, ids));
			}
			Condition::ClassDefId => TokenKind::ClassDefStyleOpts,
			Condition::ClassStyle => TokenKind::StyleClass,
			Condition::StyleDefinition => TokenKind::StyleDefinitionData,
			Condition::AccTitle => TokenKind::AccTitleValue,
			Condition::AccDescr => TokenKind::AccDescrValue,
			_ => {
				// `acc_descr_multiline`
				if rest.starts_with('}') {
					self.take(1);
					self.pop_state();
					return None;
				}
				let length = rest.find('}').unwrap_or(rest.len());
				return Some(self.token(TokenKind::AccDescrMultilineValue, length));
			}
		};
		self.pop_state();
		Some(self.token(kind, line))
	}
}

/// `edgeStrToEdgeData`: the marker at the end of a link.
fn arrow_end(link: &str) -> &'static str {
	match link.trim().chars().last() {
		Some('x') => "arrow_cross",
		Some('o') => "arrow_circle",
		Some('>') => "arrow_point",
		_ => "",
	}
}

/// `edgeStrToEdgeStartData`: the marker at the start of a link.
fn arrow_start(link: &str) -> &'static str {
	match link.trim().chars().next() {
		Some('x') => "arrow_cross",
		Some('o') => "arrow_circle",
		Some('<') => "arrow_point",
		_ => "arrow_open",
	}
}

/// The result of a `nodeStatement`: a block or the array of an edge with its ends.
enum NodeStatement {
	Single(BlockStatement),
	Edge(Box<[Statement; 3]>),
}

impl NodeStatement {
	/// The block the next edge of a chain starts at: an array has no fields.
	fn as_start(&self) -> BlockStatement {
		match self {
			NodeStatement::Single(block) => {
				BlockStatement { width_in_columns: None, ..block.clone() }
			}
			NodeStatement::Edge(_) => BlockStatement::default(),
		}
	}

	fn into_statements(self, statements: &mut Vec<Statement>) {
		match self {
			NodeStatement::Single(block) => statements.push(Statement::Block(block)),
			NodeStatement::Edge(triple) => statements.extend(*triple),
		}
	}
}

/// A `node`: its id and, with a shape, the delimiters, label and directions.
struct ParsedNode {
	id: String,
	shape: Option<(String, String, Option<Vec<String>>)>,
}

struct Parser<'a> {
	lexer: Lexer<'a>,
	current: Token,
	/// The line after the last token the grammar took.
	accepted_line: usize,
	/// `generateId`'s counter.
	ids: usize,
	depth: usize,
}

impl Parser<'_> {
	fn advance(&mut self) -> Token {
		let next = self.lexer.next_token();
		let token = std::mem::replace(&mut self.current, next);
		self.accepted_line = token.line + token.newlines;
		token
	}

	/// A parse error. jison takes the lexer's line only when the grammar takes a token, so it
	/// names the line after the last token taken; a lexical error names its own line.
	fn error<T>(&self, expected: &str) -> ParseResult<T> {
		let line = match self.current.kind {
			TokenKind::Invalid => self.current.line,
			_ => self.accepted_line,
		};
		let message = match &self.current.kind {
			TokenKind::Invalid => format!("Lexical error on line {line}. Unrecognized text."),
			TokenKind::Eof => format!("expected {expected}, found end of input"),
			_ => format!("expected {expected}, found {:?}", self.current.text),
		};
		Err(ParseError { line, message })
	}

	fn expect(&mut self, kind: TokenKind, expected: &str) -> ParseResult<Token> {
		if self.current.kind != kind {
			return self.error(expected);
		}
		Ok(self.advance())
	}

	/// `generateId`, without its random part.
	fn generate_id(&mut self) -> String {
		self.ids += 1;
		format!("id:{}", self.ids)
	}

	fn document(&mut self) -> ParseResult<Vec<Statement>> {
		let mut statements = Vec::new();
		loop {
			self.statement(&mut statements)?;
			if matches!(self.current.kind, TokenKind::End | TokenKind::Eof) {
				return Ok(statements);
			}
		}
	}

	fn statement(&mut self, statements: &mut Vec<Statement>) -> ParseResult<()> {
		use TokenKind as T;
		match self.current.kind {
			T::NodeId => self.node_statement()?.into_statements(statements),
			T::Columns => {
				let text = self.advance().text;
				statements.push(Statement::Columns(parse_int(&text)));
			}
			T::SpaceBlock => {
				let width = parse_int(&self.advance().text);
				let id = self.generate_id();
				statements.push(Statement::Space { id, width });
			}
			T::IdBlock | T::DiagramKey => {
				if self.depth >= MAX_NESTING {
					return Err(ParseError {
						line: self.current.line,
						message: "Blocks are nested too deeply.".to_owned(),
					});
				}
				let is_named = self.advance().kind == T::IdBlock;
				let block = match is_named {
					true => match self.node_statement()? {
						NodeStatement::Single(block) => block,
						NodeStatement::Edge(_) => BlockStatement::default(),
					},
					false => {
						BlockStatement { label: Some(String::new()), ..BlockStatement::default() }
					}
				};
				self.depth += 1;
				let children = self.document()?;
				self.depth -= 1;
				self.expect(T::End, "end")?;
				let mut block = block;
				if !is_named {
					block.id = Some(self.generate_id());
				}
				block.kind = Some(BlockType::Composite);
				statements.push(Statement::Composite { block, children });
			}
			T::ClassDef => {
				self.advance();
				let id = self.expect(T::ClassDefId, "a class name")?.text;
				let css = self.expect(T::ClassDefStyleOpts, "styles")?.text;
				let id = id.trim().to_owned();
				statements.push(Statement::ClassDef { id, css: css.trim().to_owned() });
			}
			T::Class => {
				self.advance();
				let ids = self.expect(T::ClassEntityIds, "block ids")?.text;
				let class = self.expect(T::StyleClass, "a class name")?.text;
				let (ids, class) = (ids.trim().to_owned(), class.trim().to_owned());
				statements.push(Statement::ApplyClass { ids, class });
			}
			T::Style => {
				self.advance();
				let ids = self.expect(T::StyleEntityIds, "block ids")?.text;
				let styles = self.expect(T::StyleDefinitionData, "styles")?.text;
				let (ids, styles) = (ids.trim().to_owned(), styles.trim().to_owned());
				statements.push(Statement::ApplyStyles { ids, styles });
			}
			_ => return self.error("a statement"),
		}
		Ok(())
	}

	fn node_statement(&mut self) -> ParseResult<NodeStatement> {
		let first = self.node()?;
		let mut block = block_of(first);
		block.width_in_columns = Some(1.0);
		if self.current.kind == TokenKind::Size {
			block.width_in_columns = Some(parse_int(&self.advance().text));
		}
		let mut statement = NodeStatement::Single(block);
		while matches!(self.current.kind, TokenKind::Link | TokenKind::StartLink) {
			let (link, label) = self.link()?;
			let end = self.node()?;
			let start = statement.as_start();
			let end_block = block_of(end);
			let edge = EdgeStatement {
				id: format!(
					"{}-{}",
					start.id.as_deref().unwrap_or("undefined"),
					end_block.id.as_deref().unwrap_or("undefined")
				),
				start: start.id.clone(),
				end: end_block.id.clone().unwrap_or_default(),
				label,
				is_thick: link.contains("=="),
				is_dotted: link.contains(".-"),
				arrow_type_start: arrow_start(&link),
				arrow_type_end: arrow_end(&link),
			};
			let end_block = BlockStatement { width_in_columns: None, ..end_block };
			statement = NodeStatement::Edge(Box::new([
				Statement::Block(start),
				Statement::Edge(edge),
				Statement::Block(end_block),
			]));
		}
		Ok(statement)
	}

	fn link(&mut self) -> ParseResult<(String, String)> {
		if self.current.kind == TokenKind::Link {
			return Ok((self.advance().text, String::new()));
		}
		self.expect(TokenKind::StartLink, "a link")?;
		self.expect(TokenKind::LinkLabel, "a label")?;
		let label = self.expect(TokenKind::Str, "a label")?.text;
		let link = self.expect(TokenKind::Link, "a link")?.text;
		Ok((link, label))
	}

	fn node(&mut self) -> ParseResult<ParsedNode> {
		let id = self.expect(TokenKind::NodeId, "a block id")?.text;
		let shape = match self.current.kind {
			TokenKind::NodeDStart => {
				let start = self.advance().text;
				let label = self.expect(TokenKind::Str, "a label")?.text;
				let end = self.expect(TokenKind::NodeDEnd, "the end of the shape")?.text;
				Some((start + &end, label, None))
			}
			TokenKind::BlockArrowStart => {
				let start = self.advance().text;
				let label = self.expect(TokenKind::Str, "a label")?.text;
				let mut directions = vec![self.expect(TokenKind::Dir, "a direction")?.text];
				while self.current.kind == TokenKind::Dir {
					directions.push(self.advance().text);
				}
				let end = self.expect(TokenKind::BlockArrowEnd, "the end of the arrow")?.text;
				Some((start + &end, label, Some(directions)))
			}
			_ => None,
		};
		Ok(ParsedNode { id, shape })
	}
}

/// The block of a `node` statement: `typeStr2Type` of its delimiters.
fn block_of(node: ParsedNode) -> BlockStatement {
	let (kind, label, directions) = match node.shape {
		Some((delimiters, label, directions)) => (type_of(&delimiters), Some(label), directions),
		None => (BlockType::Na, None, None),
	};
	BlockStatement {
		id: Some(node.id),
		label,
		kind: Some(kind),
		directions,
		width_in_columns: None,
	}
}

/// `parseInt` of a run of digits.
fn parse_int(digits: &str) -> f64 {
	digits.parse().unwrap_or(f64::NAN)
}

/// Parses the code of a block diagram (`start: BLOCK_DIAGRAM_KEY document EOF`).
pub fn parse(code: &str) -> ParseResult<Vec<Statement>> {
	let mut lexer = Lexer::new(code);
	let current = lexer.next_token();
	let mut parser = Parser { lexer, current, accepted_line: 1, ids: 0, depth: 0 };
	parser.expect(TokenKind::DiagramKey, "block")?;
	let statements = parser.document()?;
	parser.expect(TokenKind::Eof, "the end of the diagram")?;
	Ok(statements)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn lex(code: &str) -> Vec<(TokenKind, String)> {
		let mut lexer = Lexer::new(code);
		let mut tokens = Vec::new();
		loop {
			let token = lexer.next_token();
			if token.kind == TokenKind::Eof {
				return tokens;
			}
			tokens.push((token.kind, token.text));
		}
	}

	#[test]
	fn lexes_like_jison() {
		use TokenKind as T;
		let tokens = lex("block-beta\ncolumns 3\nA[\"a\"]:2 --> B block Arrow<[\"x\"]>(x, down)\n");
		let kinds: Vec<_> = tokens.iter().map(|(kind, _)| kind.clone()).collect();
		assert_eq!(
			kinds,
			[
				T::DiagramKey,
				T::Columns,
				T::NodeId,
				T::NodeDStart,
				T::Str,
				T::NodeDEnd,
				T::Size,
				T::Link,
				T::NodeId,
				T::DiagramKey,
				T::NodeId,
				T::BlockArrowStart,
				T::Str,
				T::Dir,
				T::Dir,
				T::BlockArrowEnd,
			]
		);
		assert_eq!(tokens[1].1, "3");
		assert_eq!(tokens[6].1, "2");
		assert_eq!(tokens[10].1, "Arrow");
		assert_eq!(tokens[13].1, "x");
		assert_eq!(tokens[14].1, "down");
		// Keywords end at a word boundary; `~` is part of ids.
		assert_eq!(lex("spaces")[0], (T::NodeId, "spaces".to_owned()));
		assert_eq!(lex("blockArrow")[0], (T::NodeId, "blockArrow".to_owned()));
		assert_eq!(lex("space:2")[0], (T::SpaceBlock, "2".to_owned()));
		assert_eq!(lex("endx")[0], (T::NodeId, "endx".to_owned()));
		assert_eq!(lex("A ~~~ B")[1], (T::NodeId, "~~~".to_owned()));
		assert_eq!(lex("A -- \"t\" --> B")[1].0, T::StartLink);
		assert_eq!(lex("a---b")[1], (T::Link, "---".to_owned()));
		assert_eq!(lex("a-.->b")[1], (T::Link, "-.->".to_owned()));
		assert_eq!(lex("A[ \"x\"]").last().map(|token| token.0.clone()), Some(T::Invalid));
	}

	#[test]
	fn parses_statements_like_the_grammar() {
		let statements = parse("block\n  columns 2\n  a[\"A\"]:2 --> b(\"B\")\n  space:3\n  block:c\n    d\n  end\n  classDef blue fill:#66f;\n  class a,b blue\n  style d fill:#f9F,stroke:#333\n").unwrap();
		assert_eq!(statements[0], Statement::Columns(2.0));
		// The start of an edge loses its width.
		let Statement::Block(a) = &statements[1] else { panic!() };
		assert_eq!(
			(a.id.as_deref(), a.kind, a.width_in_columns),
			(Some("a"), Some(BlockType::Square), None)
		);
		let Statement::Edge(edge) = &statements[2] else { panic!() };
		assert_eq!(
			(edge.id.as_str(), edge.arrow_type_start, edge.arrow_type_end),
			("a-b", "arrow_open", "arrow_point")
		);
		assert!(matches!(&statements[4], Statement::Space { width, .. } if *width == 3.0));
		let Statement::Composite { block, children } = &statements[5] else { panic!() };
		assert_eq!((block.id.as_deref(), children.len()), (Some("c"), 1));
		assert_eq!(
			statements[6],
			Statement::ClassDef { id: "blue".to_owned(), css: "fill:#66f;".to_owned() }
		);
		assert_eq!(
			statements[8],
			Statement::ApplyStyles {
				ids: "d".to_owned(),
				styles: "fill:#f9F,stroke:#333".to_owned()
			}
		);
		// A chain reads the fields of the array of its first edge.
		let chain = parse("block\na --> b --> c\n").unwrap();
		let Statement::Block(start) = &chain[0] else { panic!() };
		assert_eq!(start.id, None);
		let Statement::Edge(edge) = &chain[1] else { panic!() };
		assert_eq!(edge.id, "undefined-c");
		// Errors.
		for code in [
			"block\n",
			"block\na[\"\"]\n",
			"block\naccTitle: x\n",
			"block\nA[\"`md`\"]\n",
			"block\nblock\nA\n",
		] {
			assert!(parse(code).is_err(), "{code}");
		}
		assert_eq!(parse("block\nA\nB(\nC\n").unwrap_err().line, 3);
		let nested =
			|depth| format!("block\n{}A\n{}", "block\n".repeat(depth), "end\n".repeat(depth));
		assert!(parse(&nested(MAX_NESTING)).is_ok());
		assert!(parse(&nested(MAX_NESTING + 1)).is_err());
	}
}
