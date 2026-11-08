//! The grammar of `classDiagram.jison`: jison's first matching rule wins (case-sensitive, rules
//! ending with a word character end at a word boundary), exclusive states select the rules, and
//! the parser calls the database like the grammar actions do.

use super::db::{ClassDb, InvalidMember, RelationStatement, RelationType, cleanup_label};
use crate::mermaid::common::CommonFields;
use crate::mermaid::jison::{is_word_byte, line_length, span, word_length};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
	Direction(&'static str),
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	Newline,
	ClassDiagram,
	EdgeState,
	CallbackName,
	CallbackArgs,
	Str,
	Style,
	ClassDef,
	Namespace,
	StructStart,
	StructStop,
	EofInStruct,
	Class,
	OpenInStruct,
	Member,
	CssClass,
	Callback,
	Link,
	Click,
	NoteFor,
	Note,
	AnnotationStart,
	AnnotationEnd,
	Href,
	GenericType,
	BquoteStr,
	LinkTarget,
	Relation(RelationType),
	Line,
	DottedLine,
	Label,
	StyleSeparator,
	Minus,
	Dot,
	Plus,
	Colon,
	Comma,
	Brkt,
	Pct,
	Equals,
	Alpha,
	Sqs,
	Sqe,
	Punctuation,
	UnicodeText,
	Space,
	/// Text no rule matches.
	Invalid,
	Eof,
}

#[derive(Clone, Debug)]
struct Token {
	kind: TokenKind,
	text: String,
	line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
	Initial,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
	CallbackName,
	CallbackArgs,
	String,
	Namespace,
	NamespaceBody,
	Class,
	ClassBody,
	Generic,
	BqString,
}

/// Bounds the lexer state stack.
const MAX_STATES: usize = 128;

/// Bounds the nesting of namespaces.
const MAX_NAMESPACE_DEPTH: usize = 64;

#[derive(Debug)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type ParseResult<T> = Result<T, ParseError>;

/// A literal rule: `keyword`, followed by a word boundary if it ends with a word character.
fn literal(text: &str, keyword: &str) -> bool {
	text.starts_with(keyword)
		&& !(keyword.as_bytes().last().copied().is_some_and(is_word_byte)
			&& text.as_bytes().get(keyword.len()).copied().is_some_and(is_word_byte))
}

/// `name\s*<delimiter>\s*`
fn keyword_then(text: &str, name: &str, delimiter: char) -> Option<usize> {
	let rest = text.strip_prefix(name)?;
	let after = name.len() + span(rest, char::is_whitespace);
	if !text[after..].starts_with(delimiter) {
		return None;
	}
	let value = after + 1;
	Some(value + span(&text[value..], char::is_whitespace))
}

/// `.*direction\s+<dir>[^\n]*`: the length of the match for `dir`, case-sensitive.
fn direction_length(text: &str, dir: &str) -> Option<usize> {
	// `.` stops at `\r` too.
	let line = span(text, |character| character != '\n' && character != '\r');
	for (start, _) in text[..line].match_indices("direction") {
		let after = start + "direction".len();
		let spaces = span(&text[after..], char::is_whitespace);
		let value = after + spaces;
		if spaces > 0 && text[value..].starts_with(dir) {
			let end = value + dir.len();
			return Some(end + line_length(&text[end..]));
		}
	}
	None
}

/// `\s*(\r?\n)+`: the whitespace up to its last line end.
fn newline_length(text: &str) -> Option<usize> {
	let mut end = None;
	for (index, character) in text.char_indices() {
		if !character.is_whitespace() {
			break;
		}
		if character == '\n' {
			end = Some(index + 1);
		}
	}
	end
}

/// `(\r?\n?)+` after a comment: the line ends that follow it.
fn line_ends_length(text: &str) -> usize {
	let bytes = text.as_bytes();
	let mut length = 0;
	loop {
		match (bytes.get(length), bytes.get(length + 1)) {
			(Some(b'\r'), Some(b'\n')) => length += 2,
			(Some(b'\r' | b'\n'), _) => length += 1,
			_ => return length,
		}
	}
}

/// The characters of `UNICODE_TEXT`: the letters beyond ASCII of the Basic Multilingual Plane.
fn is_unicode_text(character: char) -> bool {
	!character.is_ascii() && (character as u32) < 0x10000 && character.is_alphabetic()
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
		let text = self.take(length).to_owned();
		Token { kind, text, line }
	}

	fn next_token(&mut self) -> Token {
		loop {
			if self.done {
				return Token { kind: TokenKind::Eof, text: String::new(), line: self.line };
			}
			if self.position >= self.input.len() {
				// `<<EOF>>` of the bodies comes before the one of all states.
				let kind = match self.state() {
					Condition::NamespaceBody | Condition::ClassBody => TokenKind::EofInStruct,
					_ => TokenKind::Eof,
				};
				self.done = true;
				return Token { kind, text: String::new(), line: self.line };
			}
			let rest = &self.input[self.position..];
			if let Some(token) = self.rules(rest) {
				return token;
			}
		}
	}

	/// The rules in the order of the grammar, each in the states it is active in.
	fn rules(&mut self, rest: &str) -> Option<Token> {
		use Condition as C;
		use TokenKind as T;
		let state = self.state();
		let is_initial = state == C::Initial;
		if is_initial {
			for direction in ["TB", "BT", "RL", "LR"] {
				if let Some(length) = direction_length(rest, direction) {
					return Some(self.token(T::Direction(direction), length));
				}
			}
			if rest.starts_with("%%") {
				let line = line_length(rest);
				let length = line + line_ends_length(&rest[line..]);
				self.take(length);
				return None;
			}
			if let Some(length) = keyword_then(rest, "accTitle", ':') {
				self.begin(C::AccTitle);
				return Some(self.token(T::AccTitle, length));
			}
		}
		match state {
			C::AccTitle | C::AccDescr => {
				self.pop_state();
				let kind = if state == C::AccTitle { T::AccTitleValue } else { T::AccDescrValue };
				return Some(self.token(kind, line_length(rest)));
			}
			C::AccDescrMultiline if rest.starts_with('}') => {
				self.pop_state();
				self.take(1);
				return None;
			}
			C::AccDescrMultiline => {
				let length = rest.find('}').unwrap_or(rest.len());
				return Some(self.token(T::AccDescrMultilineValue, length));
			}
			_ => {}
		}
		if is_initial {
			if let Some(length) = keyword_then(rest, "accDescr", ':') {
				self.begin(C::AccDescr);
				return Some(self.token(T::AccDescr, length));
			}
			if let Some(length) = keyword_then(rest, "accDescr", '{') {
				self.begin(C::AccDescrMultiline);
				self.take(length);
				return None;
			}
			if let Some(length) = newline_length(rest) {
				return Some(self.token(T::Newline, length));
			}
			let blanks = span(rest, char::is_whitespace);
			if blanks > 0 {
				self.take(blanks);
				return None;
			}
			for keyword in ["classDiagram-v2", "classDiagram"] {
				if literal(rest, keyword) {
					return Some(self.token(T::ClassDiagram, keyword.len()));
				}
			}
			if rest.starts_with("[*]") {
				return Some(self.token(T::EdgeState, 3));
			}
			// `"call"[\s]+`
			if let Some(after) = rest.strip_prefix("call") {
				let spaces = span(after, char::is_whitespace);
				if spaces > 0 {
					self.begin(C::CallbackName);
					self.take(4 + spaces);
					return None;
				}
			}
		}
		match state {
			C::CallbackName => {
				if let Some(after) = rest.strip_prefix('(') {
					let spaces = span(after, char::is_whitespace);
					if after[spaces..].starts_with(')') {
						self.pop_state();
						self.take(spaces + 2);
						return None;
					}
					self.pop_state();
					self.begin(C::CallbackArgs);
					self.take(1);
					return None;
				}
				let length = rest.find('(').unwrap_or(rest.len());
				return Some(self.token(T::CallbackName, length));
			}
			C::CallbackArgs => {
				if rest.starts_with(')') {
					self.pop_state();
					self.take(1);
					return None;
				}
				let length = rest.find(')').unwrap_or(rest.len());
				return Some(self.token(T::CallbackArgs, length));
			}
			C::String => {
				if rest.starts_with('"') {
					self.pop_state();
					self.take(1);
					return None;
				}
				let length = rest.find('"').unwrap_or(rest.len());
				return Some(self.token(T::Str, length));
			}
			_ => {}
		}
		if rest.starts_with('"') {
			self.begin(C::String);
			self.take(1);
			return None;
		}
		if is_initial {
			if literal(rest, "style") {
				return Some(self.token(T::Style, 5));
			}
			if literal(rest, "classDef") {
				return Some(self.token(T::ClassDef, 8));
			}
		}
		if matches!(state, C::Initial | C::Namespace | C::NamespaceBody)
			&& literal(rest, "namespace")
		{
			self.begin(C::Namespace);
			return Some(self.token(T::Namespace, 9));
		}
		match state {
			C::Namespace => {
				if let Some(length) = newline_length(rest) {
					self.pop_state();
					return Some(self.token(T::Newline, length));
				}
				let blanks = span(rest, char::is_whitespace);
				if blanks > 0 {
					self.take(blanks);
					return None;
				}
				if rest.starts_with('{') {
					self.begin(C::NamespaceBody);
					return Some(self.token(T::StructStart, 1));
				}
				if rest.starts_with('}') {
					// `this.less(0)`: the brace is read again in the state before.
					self.pop_state();
					return None;
				}
			}
			C::NamespaceBody => {
				if rest.starts_with('}') {
					self.pop_state();
					return Some(self.token(T::StructStop, 1));
				}
				if let Some(length) = newline_length(rest) {
					return Some(self.token(T::Newline, length));
				}
				let blanks = span(rest, char::is_whitespace);
				if blanks > 0 {
					self.take(blanks);
					return None;
				}
				if rest.starts_with("[*]") {
					return Some(self.token(T::EdgeState, 3));
				}
			}
			_ => {}
		}
		if matches!(state, C::Initial | C::NamespaceBody) && literal(rest, "class") {
			self.begin(C::Class);
			return Some(self.token(T::Class, 5));
		}
		match state {
			C::Class => {
				if let Some(length) = newline_length(rest) {
					self.pop_state();
					return Some(self.token(T::Newline, length));
				}
				let blanks = span(rest, char::is_whitespace);
				if blanks > 0 {
					self.take(blanks);
					return None;
				}
				if rest.starts_with('}') {
					self.pop_state();
					self.pop_state();
					return Some(self.token(T::StructStop, 1));
				}
				if rest.starts_with('{') {
					self.begin(C::ClassBody);
					return Some(self.token(T::StructStart, 1));
				}
			}
			C::ClassBody => {
				if rest.starts_with('}') {
					self.pop_state();
					return Some(self.token(T::StructStop, 1));
				}
				if rest.starts_with("[*]") {
					return Some(self.token(T::EdgeState, 3));
				}
				if rest.starts_with('{') {
					return Some(self.token(T::OpenInStruct, 1));
				}
				if rest.starts_with('\n') {
					self.take(1);
					return None;
				}
				let length = rest.find(['{', '}', '\n']).unwrap_or(rest.len());
				return Some(self.token(T::Member, length));
			}
			_ => {}
		}
		self.everywhere(state, rest)
	}

	/// The `<*>` rules after the ones of the states, and the remaining `INITIAL` ones.
	fn everywhere(&mut self, state: Condition, rest: &str) -> Option<Token> {
		use Condition as C;
		use TokenKind as T;
		let keywords: [(&str, T); 9] = [
			("cssClass", T::CssClass),
			("callback", T::Callback),
			("link", T::Link),
			("click", T::Click),
			("note for", T::NoteFor),
			("note", T::Note),
			("<<", T::AnnotationStart),
			(">>", T::AnnotationEnd),
			("href", T::Href),
		];
		for (keyword, kind) in keywords {
			if literal(rest, keyword) {
				return Some(self.token(kind, keyword.len()));
			}
		}
		match state {
			C::Generic if rest.starts_with('~') => {
				self.pop_state();
				self.take(1);
				return None;
			}
			C::Generic => {
				let length = rest.find('~').unwrap_or(rest.len());
				return Some(self.token(T::GenericType, length));
			}
			_ => {}
		}
		if rest.starts_with('~') {
			self.begin(C::Generic);
			self.take(1);
			return None;
		}
		match state {
			C::BqString if rest.starts_with('`') => {
				self.pop_state();
				self.take(1);
				return None;
			}
			C::BqString => {
				let length = rest.find('`').unwrap_or(rest.len());
				return Some(self.token(T::BquoteStr, length));
			}
			_ => {}
		}
		if rest.starts_with('`') {
			self.begin(C::BqString);
			self.take(1);
			return None;
		}
		for target in ["_self", "_blank", "_parent", "_top"] {
			if literal(rest, target) {
				return Some(self.token(T::LinkTarget, target.len()));
			}
		}
		// `\s*` before the relation types.
		let blanks = span(rest, char::is_whitespace);
		let after = &rest[blanks..];
		let relations: [(&str, T); 6] = [
			("<|", T::Relation(RelationType::Extension)),
			("|>", T::Relation(RelationType::Extension)),
			(">", T::Relation(RelationType::Dependency)),
			("<", T::Relation(RelationType::Dependency)),
			("*", T::Relation(RelationType::Composition)),
			("o", T::Relation(RelationType::Aggregation)),
		];
		for (symbol, kind) in relations {
			if literal(after, symbol) {
				return Some(self.token(kind, blanks + symbol.len()));
			}
		}
		if after.starts_with("()") {
			return Some(self.token(T::Relation(RelationType::Lollipop), blanks + 2));
		}
		if rest.starts_with("--") {
			return Some(self.token(T::Line, 2));
		}
		if rest.starts_with("..") {
			return Some(self.token(T::DottedLine, 2));
		}
		if let Some(label) = rest.strip_prefix(':') {
			let length = span(label, |character| !matches!(character, ':' | '\n' | ';'));
			if length > 0 {
				return Some(self.token(T::Label, 1 + length));
			}
		}
		if rest.starts_with(":::") {
			return Some(self.token(T::StyleSeparator, 3));
		}
		let symbols: [(char, T); 3] = [('-', T::Minus), ('.', T::Dot), ('+', T::Plus)];
		for (symbol, kind) in symbols {
			if rest.starts_with(symbol) {
				return Some(self.token(kind, 1));
			}
		}
		if state == C::Initial {
			let symbols: [(char, T); 3] = [(':', T::Colon), (',', T::Comma), ('#', T::Brkt)];
			for (symbol, kind) in symbols {
				if rest.starts_with(symbol) {
					return Some(self.token(kind, 1));
				}
			}
		}
		if rest.starts_with('%') {
			return Some(self.token(T::Pct, 1));
		}
		if rest.starts_with('=') {
			return Some(self.token(T::Equals, 1));
		}
		let word = word_length(rest);
		if word > 0 {
			return Some(self.token(T::Alpha, word));
		}
		let symbols: [(char, T); 2] = [('[', T::Sqs), (']', T::Sqe)];
		for (symbol, kind) in symbols {
			if rest.starts_with(symbol) {
				return Some(self.token(kind, 1));
			}
		}
		let first = rest.chars().next().unwrap_or(' ');
		if "!\"#$%&'*+,-.`?\\/".contains(first) {
			return Some(self.token(T::Punctuation, 1));
		}
		if is_unicode_text(first) {
			return Some(self.token(T::UnicodeText, first.len_utf8()));
		}
		if first.is_whitespace() {
			return Some(self.token(T::Space, first.len_utf8()));
		}
		// No rule matches: jison stops with a lexical error.
		Some(self.token(T::Invalid, rest.len()))
	}
}

struct Parser<'a, 'c> {
	lexer: Lexer<'a>,
	current: Token,
	common: &'c mut CommonFields,
	db: &'c mut ClassDb,
	/// The line after the last token the grammar took.
	accepted_line: usize,
	namespace_depth: usize,
}

/// `alphaNumToken`
fn is_alpha_num(kind: TokenKind) -> bool {
	matches!(kind, TokenKind::UnicodeText | TokenKind::Alpha | TokenKind::Minus)
}

fn is_relation_start(kind: TokenKind) -> bool {
	matches!(kind, TokenKind::Relation(_) | TokenKind::Line | TokenKind::DottedLine)
}

/// What a statement of a namespace contributes to it.
enum Contribution {
	Class(String),
	Note(String),
	Nothing,
}

impl Parser<'_, '_> {
	fn advance(&mut self) -> Token {
		let next = self.lexer.next_token();
		let token = std::mem::replace(&mut self.current, next);
		self.accepted_line = token.line + token.text.matches('\n').count();
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
			TokenKind::Eof | TokenKind::EofInStruct => {
				format!("expected {expected}, found end of input")
			}
			TokenKind::Newline => format!("expected {expected}, found end of line"),
			_ => format!("expected {expected}, found {:?}", self.current.text),
		};
		Err(ParseError { line, message })
	}

	fn is(&self, kind: TokenKind) -> bool {
		self.current.kind == kind
	}

	fn expect(&mut self, kind: TokenKind, expected: &str) -> ParseResult<Token> {
		if self.current.kind != kind {
			return self.error(expected);
		}
		Ok(self.advance())
	}

	fn invalid_member(&self, _: InvalidMember) -> ParseError {
		ParseError {
			line: self.accepted_line,
			message: "A method needs a name before its parentheses.".to_owned(),
		}
	}

	/// `start: NEWLINE* CLASS_DIAGRAM NEWLINE statements EOF`; blank lines before it are what
	/// preprocessing left of front matter and directives.
	fn start(&mut self) -> ParseResult<()> {
		while self.is(TokenKind::Newline) {
			self.advance();
		}
		self.expect(TokenKind::ClassDiagram, "classDiagram")?;
		self.expect(TokenKind::Newline, "a new line")?;
		loop {
			self.statement()?;
			match self.current.kind {
				TokenKind::Eof => return Ok(()),
				TokenKind::Newline => {
					self.advance();
					if self.is(TokenKind::Eof) {
						return Ok(());
					}
				}
				_ => return self.error("a new line"),
			}
		}
	}

	/// `className` and `namespaceName`: names joined without the blanks between them, with dots,
	/// a backquoted name at the end, and for classes a generic type.
	fn name(&mut self, allows_generic: bool) -> ParseResult<String> {
		let mut name = String::new();
		loop {
			if self.is(TokenKind::BquoteStr) {
				name.push_str(&self.advance().text);
				break;
			}
			if !is_alpha_num(self.current.kind) {
				return self.error("a name");
			}
			name.push_str(&self.advance().text);
			if self.is(TokenKind::Dot) {
				self.advance();
				name.push('.');
				continue;
			}
			if !is_alpha_num(self.current.kind) && !self.is(TokenKind::BquoteStr) {
				break;
			}
		}
		if allows_generic && self.is(TokenKind::GenericType) {
			let generic = self.advance().text;
			name = format!("{name}~{generic}~");
		}
		Ok(name)
	}

	fn class_name(&mut self) -> ParseResult<String> {
		self.name(true)
	}

	fn alpha_num(&mut self) -> ParseResult<String> {
		if !is_alpha_num(self.current.kind) {
			return self.error("a name");
		}
		Ok(self.advance().text)
	}

	fn string(&mut self) -> ParseResult<String> {
		Ok(self.expect(TokenKind::Str, "a string")?.text)
	}

	/// `classLabel: SQS STR SQE`, if one follows.
	fn class_label(&mut self) -> ParseResult<Option<String>> {
		if !self.is(TokenKind::Sqs) {
			return Ok(None);
		}
		self.advance();
		let label = self.string()?;
		self.expect(TokenKind::Sqe, "]")?;
		Ok(Some(label))
	}

	/// `members: MEMBER+` up to the `}`, which it takes.
	fn members(&mut self) -> ParseResult<Vec<String>> {
		let mut members = Vec::new();
		while self.is(TokenKind::Member) {
			members.push(self.advance().text);
		}
		self.expect(TokenKind::StructStop, "a member or }")?;
		Ok(members)
	}

	fn add_members(&mut self, class: &str, members: &[String]) -> ParseResult<()> {
		for member in members {
			self.db.add_member(class, member).map_err(|error| self.invalid_member(error))?;
		}
		Ok(())
	}

	fn add_annotation(&mut self, class: &str, annotation: &str) -> ParseResult<()> {
		if self.db.add_annotation(class, annotation) {
			return Ok(());
		}
		Err(ParseError {
			line: self.accepted_line,
			message: format!("The annotation refers to the unknown class \"{class}\"."),
		})
	}

	/// `classStatement`: returns the class name.
	fn class_statement(&mut self) -> ParseResult<String> {
		self.expect(TokenKind::Class, "class")?;
		let name = self.class_name()?;
		self.db.add_class(&name);
		if let Some(label) = self.class_label()? {
			self.db.set_class_label(&name, &label);
		}
		match self.current.kind {
			TokenKind::StyleSeparator => {
				self.advance();
				let class = self.alpha_num()?;
				// Only a body with members may follow.
				let members = match self.is(TokenKind::StructStart) {
					true => {
						self.advance();
						if !self.is(TokenKind::Member) {
							return self.error("a member");
						}
						Some(self.members()?)
					}
					false => None,
				};
				self.db.set_css_class(&name, &class);
				if let Some(members) = members {
					self.add_members(&name, &members)?;
				}
			}
			TokenKind::StructStart => {
				self.advance();
				let members = self.members()?;
				self.add_members(&name, &members)?;
			}
			TokenKind::AnnotationStart => {
				self.advance();
				let annotation = self.alpha_num()?;
				self.expect(TokenKind::AnnotationEnd, ">>")?;
				let members = match self.is(TokenKind::StructStart) {
					true => {
						self.advance();
						self.members()?
					}
					false => Vec::new(),
				};
				self.add_annotation(&name, &annotation)?;
				self.add_members(&name, &members)?;
			}
			_ => {}
		}
		Ok(name)
	}

	/// `noteStatement`: returns the note id.
	fn note_statement(&mut self) -> ParseResult<String> {
		let class = match self.advance().kind {
			TokenKind::NoteFor => Some(self.class_name()?),
			_ => None,
		};
		let text = self.string()?;
		Ok(self.db.add_note(&text, class.as_deref()))
	}

	/// `namespaceStatement`: its classes, notes and nested namespaces.
	fn namespace_statement(&mut self) -> ParseResult<()> {
		self.expect(TokenKind::Namespace, "namespace")?;
		let name = self.name(false)?;
		let label = self.class_label()?;
		if !self.is(TokenKind::StructStart) {
			return self.error("{");
		}
		if self.namespace_depth >= MAX_NAMESPACE_DEPTH {
			return Err(ParseError {
				line: self.current.line,
				message: "Namespaces are nested too deeply.".to_owned(),
			});
		}
		let id = self.db.add_namespace(&name, label.as_deref());
		self.advance();
		if self.is(TokenKind::Newline) {
			self.advance();
		}
		self.namespace_depth += 1;
		let mut classes = Vec::new();
		let mut notes = Vec::new();
		loop {
			let contribution = match self.current.kind {
				TokenKind::Class => Contribution::Class(self.class_statement()?),
				TokenKind::Note | TokenKind::NoteFor => Contribution::Note(self.note_statement()?),
				TokenKind::Namespace => {
					self.namespace_statement()?;
					Contribution::Nothing
				}
				_ => return self.error("a class, note or namespace"),
			};
			match contribution {
				Contribution::Class(class) => classes.push(class),
				Contribution::Note(note) => notes.push(note),
				Contribution::Nothing => {}
			}
			if self.is(TokenKind::Newline) {
				self.advance();
			}
			if self.is(TokenKind::StructStop) {
				self.advance();
				break;
			}
		}
		self.namespace_depth -= 1;
		self.db.add_classes_to_namespace(&id, &classes, &notes);
		self.db.pop_namespace();
		Ok(())
	}

	/// `stylesOpt: style (COMMA style)*` with `style: styleComponent+`: the components of a
	/// style are joined without the blanks between them.
	fn styles(&mut self) -> ParseResult<Vec<String>> {
		use TokenKind as T;
		let mut styles = Vec::new();
		loop {
			let mut style = String::new();
			while matches!(
				self.current.kind,
				T::Alpha | T::Colon | T::Space | T::Brkt | T::Style | T::Pct | T::Label
			) {
				style.push_str(&self.advance().text);
			}
			if style.is_empty() {
				return self.error("a style");
			}
			styles.push(style);
			if !self.is(T::Comma) {
				return Ok(styles);
			}
			self.advance();
		}
	}

	/// `relationStatement` after its first class, with the label after it.
	fn relation_statement(&mut self, id1: String) -> ParseResult<()> {
		use TokenKind as T;
		let title1 = match self.is(T::Str) {
			true => Some(self.advance().text),
			false => None,
		};
		let relation_type = |kind: T| match kind {
			T::Relation(relation_type) => Some(relation_type),
			_ => None,
		};
		let type1 = relation_type(self.current.kind);
		if type1.is_some() {
			self.advance();
		}
		let is_dotted = match self.current.kind {
			T::Line => false,
			T::DottedLine => true,
			_ => return self.error("-- or .."),
		};
		self.advance();
		let type2 = relation_type(self.current.kind);
		if type2.is_some() {
			self.advance();
		}
		let title2 = match self.is(T::Str) {
			true => Some(self.advance().text),
			false => None,
		};
		let id2 = self.class_name()?;
		let title = match self.is(T::Label) {
			true => Some(cleanup_label(&self.advance().text)),
			false => None,
		};
		self.db.add_relation(RelationStatement {
			id1,
			id2,
			title1,
			title2,
			type1,
			type2,
			is_dotted,
			title,
		});
		Ok(())
	}

	/// `clickStatement` after `callback`, `link` or `click`.
	fn click_statement(&mut self, keyword: TokenKind) -> ParseResult<()> {
		use TokenKind as T;
		let name = self.class_name()?;
		let is_link = match keyword {
			T::Callback => false,
			T::Link => true,
			_ => match self.current.kind {
				T::CallbackName => {
					self.advance();
					if self.is(T::CallbackArgs) {
						self.advance();
					}
					false
				}
				T::Href => {
					self.advance();
					true
				}
				_ => return self.error("call or href"),
			},
		};
		if !is_link {
			if keyword == T::Callback {
				self.string()?;
			}
			self.db.set_click_event(&name);
			if self.is(T::Str) {
				let tooltip = self.advance().text;
				self.db.set_tooltip(&name, &tooltip);
			}
			return Ok(());
		}
		let link = self.string()?;
		let tooltip = match self.is(T::Str) {
			true => Some(self.advance().text),
			false => None,
		};
		let target = match self.is(T::LinkTarget) {
			true => Some(self.advance().text),
			false => None,
		};
		self.db.set_link(&name, &link, target.as_deref());
		if let Some(tooltip) = tooltip {
			self.db.set_tooltip(&name, &tooltip);
		}
		Ok(())
	}

	fn statement(&mut self) -> ParseResult<()> {
		use TokenKind as T;
		match self.current.kind {
			T::AccTitle => {
				self.advance();
				let value = self.expect(T::AccTitleValue, "a title")?.text;
				self.common.accessibility.title = value.trim().to_owned();
			}
			T::AccDescr => {
				self.advance();
				let value = self.expect(T::AccDescrValue, "a description")?.text;
				self.common.accessibility.description = value.trim().to_owned();
			}
			T::AccDescrMultilineValue => {
				let value = self.advance().text;
				self.common.accessibility.description = value.trim().to_owned();
			}
			T::Direction(direction) => {
				self.advance();
				self.db.direction = Some(direction);
			}
			T::Namespace => self.namespace_statement()?,
			T::Class => {
				self.class_statement()?;
			}
			T::AnnotationStart => {
				self.advance();
				let annotation = self.alpha_num()?;
				self.expect(T::AnnotationEnd, ">>")?;
				let name = self.class_name()?;
				self.add_annotation(&name, &annotation)?;
			}
			T::Note | T::NoteFor => {
				self.note_statement()?;
			}
			T::ClassDef => {
				self.advance();
				let mut ids = vec![self.expect(T::Alpha, "a class name")?.text];
				while self.is(T::Comma) {
					self.advance();
					ids.push(self.expect(T::Alpha, "a class name")?.text);
				}
				let styles = self.styles()?;
				self.db.define_class(&ids, &styles);
			}
			T::Style => {
				self.advance();
				let id = self.expect(T::Alpha, "a class name")?.text;
				let styles = self.styles()?;
				self.db.set_css_style(&id, &styles);
			}
			T::CssClass => {
				self.advance();
				let ids = self.string()?;
				let class = self.expect(T::Alpha, "a class name")?.text;
				self.db.set_css_class(&ids, &class);
			}
			keyword @ (T::Callback | T::Link | T::Click) => {
				self.advance();
				self.click_statement(keyword)?;
			}
			T::Member => {
				self.advance();
			}
			kind if is_alpha_num(kind) || kind == T::BquoteStr => {
				let name = self.class_name()?;
				if self.is(T::Str) || is_relation_start(self.current.kind) {
					self.relation_statement(name)?;
				} else if self.is(T::Label) {
					let label = cleanup_label(&self.advance().text);
					self.db
						.add_member(&name, &label)
						.map_err(|error| self.invalid_member(error))?;
				}
			}
			_ => return self.error("a statement"),
		}
		Ok(())
	}
}

/// Parses the code of a class diagram into `db`.
pub fn parse(code: &str, common: &mut CommonFields, db: &mut ClassDb) -> ParseResult<()> {
	let lexer = Lexer::new(code);
	let placeholder = Token { kind: TokenKind::Eof, text: String::new(), line: 1 };
	let mut parser =
		Parser { lexer, current: placeholder, common, db, accepted_line: 1, namespace_depth: 0 };
	parser.advance();
	parser.start()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn kinds(code: &str) -> Vec<TokenKind> {
		let mut lexer = Lexer::new(code);
		let mut kinds = Vec::new();
		for _ in 0..1000 {
			let token = lexer.next_token();
			if token.kind == TokenKind::Eof {
				break;
			}
			kinds.push(token.kind);
		}
		kinds
	}

	#[test]
	fn lexes_like_jison() {
		use TokenKind as T;
		assert_eq!(
			kinds("Animal <|-- Duck : label\n"),
			[
				T::Alpha,
				T::Relation(RelationType::Extension),
				T::Line,
				T::Alpha,
				T::Label,
				T::Newline
			]
		);
		assert_eq!(
			kinds("class A~T~ {\n  +int x\n}\n"),
			[
				T::Class,
				T::Alpha,
				T::GenericType,
				T::StructStart,
				T::Member,
				T::StructStop,
				T::Newline
			]
		);
		// `o` is a relation only at a word boundary.
		assert_eq!(
			kinds("obj o-- o"),
			[
				T::Alpha,
				T::Relation(RelationType::Aggregation),
				T::Line,
				T::Relation(RelationType::Aggregation)
			]
		);
		assert_eq!(kinds("A : direction LR"), [T::Direction("LR")]);
		assert_eq!(kinds("Direction LR"), [T::Alpha, T::Alpha]);
		assert_eq!(
			kinds("click A call f(1)"),
			[T::Click, T::Alpha, T::CallbackName, T::CallbackArgs]
		);
		assert_eq!(
			kinds("classDef a fill:#f9f,stroke:#333"),
			[T::ClassDef, T::Alpha, T::Alpha, T::Label, T::Label]
		);
		assert_eq!(kinds("A ; B"), [T::Alpha, T::Invalid]);
	}

	#[test]
	fn parses_classes_relations_and_namespaces() {
		let code = "classDiagram\n  class Animal[\"An animal\"] {\n    <<interface>>\n    +int age\n    +isMammal() bool\n  }\n  Animal \"1\" <|-- \"*\" Duck : has\n  Duck : +swim()\n  namespace Zoo.Area {\n    class Cage\n    note for Cage \"Locked\"\n  }\n  class Duck:::bird\n  classDef bird fill:#f00\n  style Cage stroke:#00f\n  link Cage \"https://example.com\" \"Tip\" _self\n  direction LR\n";
		let mut common = CommonFields::default();
		let mut db = ClassDb::default();
		parse(code, &mut common, &mut db).unwrap();
		let animal = db.class("Animal").unwrap();
		assert_eq!(animal.text, "An animal");
		assert_eq!(animal.annotations, ["interface"]);
		assert_eq!(animal.members.len(), 1);
		assert_eq!(animal.methods[0].text, "\\+isMammal() : bool");
		let relation = &db.relations[0];
		assert_eq!((relation.title1.as_str(), relation.title2.as_str()), ("1", "*"));
		assert_eq!(relation.title.as_deref(), Some("has"));
		assert_eq!(relation.type1, Some(RelationType::Extension));
		assert_eq!(db.class("Duck").unwrap().methods.len(), 1);
		// `classDef` styles only the classes that have the class already.
		assert_eq!(db.class("Duck").unwrap().styles, ["fill:#f00"]);
		let cage = db.class("Cage").unwrap();
		assert_eq!(cage.parent.as_deref(), Some("Zoo.Area"));
		assert_eq!(cage.styles, ["stroke:#00f"]);
		assert_eq!(cage.link_target.as_deref(), Some("_self"));
		assert_eq!(cage.tooltip.as_deref(), Some("Tip"));
		assert_eq!(cage.css_classes, "default clickable");
		assert_eq!(db.notes[0].parent.as_deref(), Some("Zoo.Area"));
		assert_eq!(db.namespaces.len(), 2);
		assert_eq!(db.direction, Some("LR"));
		let error =
			parse("classDiagram\n  A <|-- B\n  A ; B\n", &mut common, &mut ClassDb::default());
		assert_eq!(error.unwrap_err().line, 3);
	}
}
