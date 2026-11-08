//! The grammar of `requirementDiagram.jison`: jison's first matching rule wins (case-insensitive,
//! rules ending with a word character end at a word boundary), exclusive states select the
//! rules, and the parser calls the database like the grammar actions do.

use super::db::{Fields, RequirementDb};
use crate::mermaid::common::CommonFields;
use crate::mermaid::jison::{direction_length, keyword, line_length, span, word_length};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
	Title,
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	Direction(&'static str),
	Newline,
	Rd,
	StructStart,
	StructStop,
	StyleSeparator,
	ColonSep,
	/// A keyword of the requirement body: `id`, `text`, `risk` or `verifyMethod`.
	RequirementField(usize),
	/// A keyword of the element body: `type` or `docref`.
	ElementField(usize),
	RequirementType(&'static str),
	Risk(&'static str),
	Verify(&'static str),
	Element,
	Relationship(&'static str),
	Style,
	ClassDef,
	Class,
	EndArrowL,
	EndArrowR,
	Line,
	QString,
	UnqString,
	/// `\w+`; `<*>[0-9]+` never matches after it.
	Alpha,
	Comma,
	Colon,
	Semicolon,
	Percent,
	Minus,
	Brkt,
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
	String,
	Style,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
}

/// Bounds the lexer state stack.
const MAX_STATES: usize = 128;

#[derive(Debug)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type ParseResult<T> = Result<T, ParseError>;

/// The keyword rules of `INITIAL` between `:` and `<-`, in the grammar's order.
const KEYWORDS: &[(&str, TokenKind)] = &[
	("id", TokenKind::RequirementField(0)),
	("text", TokenKind::RequirementField(1)),
	("risk", TokenKind::RequirementField(2)),
	("verifyMethod", TokenKind::RequirementField(3)),
	("requirement", TokenKind::RequirementType("Requirement")),
	("functionalRequirement", TokenKind::RequirementType("Functional Requirement")),
	("interfaceRequirement", TokenKind::RequirementType("Interface Requirement")),
	("performanceRequirement", TokenKind::RequirementType("Performance Requirement")),
	("physicalRequirement", TokenKind::RequirementType("Physical Requirement")),
	("designConstraint", TokenKind::RequirementType("Design Constraint")),
	("low", TokenKind::Risk("Low")),
	("medium", TokenKind::Risk("Medium")),
	("high", TokenKind::Risk("High")),
	("analysis", TokenKind::Verify("Analysis")),
	("demonstration", TokenKind::Verify("Demonstration")),
	("inspection", TokenKind::Verify("Inspection")),
	("test", TokenKind::Verify("Test")),
	("element", TokenKind::Element),
	("contains", TokenKind::Relationship("contains")),
	("copies", TokenKind::Relationship("copies")),
	("derives", TokenKind::Relationship("derives")),
	("satisfies", TokenKind::Relationship("satisfies")),
	("verifies", TokenKind::Relationship("verifies")),
	("refines", TokenKind::Relationship("refines")),
	("traces", TokenKind::Relationship("traces")),
	("type", TokenKind::ElementField(0)),
	("docref", TokenKind::ElementField(1)),
];

/// `name\s*<delimiter>\s*`
fn keyword_then(text: &str, name: &str, delimiter: char) -> Option<usize> {
	if !crate::mermaid::jison::starts_with_ignoring_case(text, name) {
		return None;
	}
	let after = name.len() + span(&text[name.len()..], char::is_whitespace);
	if !text[after..].starts_with(delimiter) {
		return None;
	}
	let value = after + 1;
	Some(value + span(&text[value..], char::is_whitespace))
}

/// `[\w][^:,\r\n\{\<\>\-\=]*`
fn unquoted_length(text: &str) -> usize {
	if word_length(text) == 0 {
		return 0;
	}
	span(text, |character| {
		!matches!(character, ':' | ',' | '\r' | '\n' | '{' | '<' | '>' | '-' | '=')
	})
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
			if self.done || self.position >= self.input.len() {
				self.done = true;
				return Token { kind: TokenKind::Eof, text: String::new(), line: self.line };
			}
			let rest = &self.input[self.position..];
			let result = match self.state() {
				Condition::Initial => self.initial(rest),
				Condition::String => self.string(rest),
				Condition::Style => self.style(rest),
				state => self.accessibility(state, rest),
			};
			if let Some(token) = result {
				return token;
			}
		}
	}

	fn initial(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		// `"title"\s[^#\n;]+`
		if crate::mermaid::jison::starts_with_ignoring_case(rest, "title")
			&& rest[5..].starts_with(char::is_whitespace)
		{
			let start = 5 + rest[5..].chars().next().map_or(0, char::len_utf8);
			let value = span(&rest[start..], |character| !matches!(character, '#' | '\n' | ';'));
			if value > 0 {
				return Some(self.token(T::Title, start + value));
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
			self.begin(Condition::AccDescrMultiline);
			self.take(length);
			return None;
		}
		for direction in ["TB", "BT", "RL", "LR"] {
			if let Some(length) = direction_length(rest, direction) {
				return Some(self.token(T::Direction(direction), length));
			}
		}
		// `(\r?\n)+`
		let newlines = newlines_length(rest);
		if newlines > 0 {
			return Some(self.token(T::Newline, newlines));
		}
		let blanks = span(rest, char::is_whitespace);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		if rest.starts_with('#') || rest.starts_with("%%") {
			let length = line_length(rest);
			self.take(length);
			return None;
		}
		if keyword(rest, "requirementDiagram") {
			return Some(self.token(T::Rd, "requirementDiagram".len()));
		}
		let symbols: [(&str, T); 4] = [
			("{", T::StructStart),
			("}", T::StructStop),
			(":::", T::StyleSeparator),
			(":", T::ColonSep),
		];
		for (symbol, kind) in symbols {
			if rest.starts_with(symbol) {
				return Some(self.token(kind, symbol.len()));
			}
		}
		for &(name, kind) in KEYWORDS {
			if keyword(rest, name) {
				return Some(self.token(kind, name.len()));
			}
		}
		if keyword(rest, "style") {
			self.begin(Condition::Style);
			return Some(self.token(T::Style, "style".len()));
		}
		if keyword(rest, "classDef") {
			self.begin(Condition::Style);
			return Some(self.token(T::ClassDef, "classDef".len()));
		}
		if keyword(rest, "class") {
			self.begin(Condition::Style);
			return Some(self.token(T::Class, "class".len()));
		}
		for (symbol, kind) in [("<-", T::EndArrowL), ("->", T::EndArrowR), ("-", T::Line)] {
			if rest.starts_with(symbol) {
				return Some(self.token(kind, symbol.len()));
			}
		}
		if rest.starts_with('"') {
			self.begin(Condition::String);
			self.take(1);
			return None;
		}
		let unquoted = unquoted_length(rest);
		if unquoted > 0 {
			let mut token = self.token(T::UnqString, unquoted);
			token.text = token.text.trim().to_owned();
			return Some(token);
		}
		self.everywhere(rest)
	}

	/// The `<*>` rules, which every state has after its own.
	fn everywhere(&mut self, rest: &str) -> Option<Token> {
		let word = word_length(rest);
		if word > 0 {
			return Some(self.token(TokenKind::Alpha, word));
		}
		if rest.starts_with(',') {
			return Some(self.token(TokenKind::Comma, 1));
		}
		// No rule matches: jison stops with a lexical error.
		let length = rest.len();
		Some(self.token(TokenKind::Invalid, length))
	}

	fn string(&mut self, rest: &str) -> Option<Token> {
		if rest.starts_with('"') {
			self.pop_state();
			self.take(1);
			return None;
		}
		let length = rest.find('"').unwrap_or(rest.len());
		Some(self.token(TokenKind::QString, length))
	}

	fn style(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		let word = word_length(rest);
		if word > 0 {
			return Some(self.token(T::Alpha, word));
		}
		let symbols = [
			(':', T::Colon),
			(';', T::Semicolon),
			('%', T::Percent),
			('-', T::Minus),
			('#', T::Brkt),
		];
		for (symbol, kind) in symbols {
			if rest.starts_with(symbol) {
				return Some(self.token(kind, 1));
			}
		}
		if rest.starts_with(' ') {
			self.take(1);
			return None;
		}
		if rest.starts_with('"') {
			self.begin(Condition::String);
			self.take(1);
			return None;
		}
		if rest.starts_with('\n') {
			self.pop_state();
			self.take(1);
			return None;
		}
		self.everywhere(rest)
	}

	/// `acc_title`, `acc_descr` and `acc_descr_multiline`.
	fn accessibility(&mut self, state: Condition, rest: &str) -> Option<Token> {
		match state {
			Condition::AccDescrMultiline if rest.starts_with('}') => {
				self.pop_state();
				self.take(1);
				None
			}
			Condition::AccDescrMultiline => {
				let length = rest.find('}').unwrap_or(rest.len());
				Some(self.token(TokenKind::AccDescrMultilineValue, length))
			}
			_ => {
				self.pop_state();
				let kind = match state {
					Condition::AccTitle => TokenKind::AccTitleValue,
					_ => TokenKind::AccDescrValue,
				};
				Some(self.token(kind, line_length(rest)))
			}
		}
	}
}

/// `(\r?\n)+`
fn newlines_length(text: &str) -> usize {
	let bytes = text.as_bytes();
	let mut length = 0;
	loop {
		match (bytes.get(length), bytes.get(length + 1)) {
			(Some(b'\n'), _) => length += 1,
			(Some(b'\r'), Some(b'\n')) => length += 2,
			_ => return length,
		}
	}
}

struct Parser<'a, 'c> {
	lexer: Lexer<'a>,
	current: Token,
	common: &'c mut CommonFields,
	db: &'c mut RequirementDb,
	/// The line after the last token the grammar took.
	accepted_line: usize,
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
			TokenKind::Eof => format!("expected {expected}, found end of input"),
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

	/// The directives of the accessibility, if the current token starts one.
	fn directive(&mut self) -> ParseResult<bool> {
		match self.current.kind {
			TokenKind::AccTitle => {
				self.advance();
				let value = self.expect(TokenKind::AccTitleValue, "a title")?.text;
				self.common.accessibility.title = value.trim().to_owned();
			}
			TokenKind::AccDescr => {
				self.advance();
				let value = self.expect(TokenKind::AccDescrValue, "a description")?.text;
				self.common.accessibility.description = value.trim().to_owned();
			}
			TokenKind::AccDescrMultilineValue => {
				let value = self.advance().text;
				self.common.accessibility.description = value.trim().to_owned();
			}
			_ => return Ok(false),
		}
		Ok(true)
	}

	/// `start: directive NEWLINE? start | RD NEWLINE diagram EOF`; blank lines before it are
	/// what preprocessing left of front matter and directives.
	fn start(&mut self) -> ParseResult<()> {
		loop {
			while self.is(TokenKind::Newline) {
				self.advance();
			}
			if !self.directive()? {
				break;
			}
		}
		self.expect(TokenKind::Rd, "requirementDiagram")?;
		self.expect(TokenKind::Newline, "a new line")?;
		while !self.is(TokenKind::Eof) {
			self.statement()?;
		}
		Ok(())
	}

	/// `unqString | qString`
	fn string(&mut self, expected: &str) -> ParseResult<String> {
		match self.current.kind {
			TokenKind::UnqString | TokenKind::QString => Ok(self.advance().text),
			_ => self.error(expected),
		}
	}

	/// `idList: (ALPHA | id) (COMMA (ALPHA | id))*`
	fn id_list(&mut self) -> ParseResult<Vec<String>> {
		let mut ids = Vec::new();
		loop {
			match self.current.kind {
				TokenKind::Alpha | TokenKind::UnqString | TokenKind::QString => {
					ids.push(self.advance().text);
				}
				_ => return self.error("an id"),
			}
			if !self.is(TokenKind::Comma) {
				return Ok(ids);
			}
			self.advance();
		}
	}

	/// `stylesOpt: style (COMMA style)*` with `style: styleComponent+`: the components of a
	/// style are joined without the spaces between them.
	fn styles(&mut self) -> ParseResult<Vec<String>> {
		let mut styles = Vec::new();
		loop {
			let mut style = String::new();
			while matches!(
				self.current.kind,
				TokenKind::Alpha
					| TokenKind::Colon
					| TokenKind::Brkt
					| TokenKind::Minus
					| TokenKind::Semicolon
			) {
				style.push_str(&self.advance().text);
			}
			if style.is_empty() {
				return self.error("a style");
			}
			styles.push(style);
			if !self.is(TokenKind::Comma) {
				return Ok(styles);
			}
			self.advance();
		}
	}

	/// `(STYLE_SEPARATOR idList)?`
	fn classes(&mut self) -> ParseResult<Vec<String>> {
		if !self.is(TokenKind::StyleSeparator) {
			return Ok(Vec::new());
		}
		self.advance();
		self.id_list()
	}

	/// The body of a requirement or element up to `}`: `KEYWORD COLONSEP value NEWLINE` lines
	/// and blank lines. The grammar is right recursive, so the first value of a field wins.
	fn body(&mut self, is_requirement: bool) -> ParseResult<Fields> {
		let mut fields = Fields::default();
		let mut is_set = [false; 4];
		loop {
			let field = match self.current.kind {
				TokenKind::StructStop => {
					self.advance();
					return Ok(fields);
				}
				TokenKind::Newline => {
					self.advance();
					continue;
				}
				TokenKind::RequirementField(field) if is_requirement => field,
				TokenKind::ElementField(field) if !is_requirement => field,
				_ => return self.error("a field or }"),
			};
			self.advance();
			self.expect(TokenKind::ColonSep, ":")?;
			let value = match (field, self.current.kind) {
				(2, TokenKind::Risk(risk)) if is_requirement => {
					self.advance();
					risk.to_owned()
				}
				(3, TokenKind::Verify(method)) if is_requirement => {
					self.advance();
					method.to_owned()
				}
				(2, _) if is_requirement => return self.error("a risk"),
				(3, _) if is_requirement => return self.error("a verification method"),
				_ => self.string("a value")?,
			};
			self.expect(TokenKind::Newline, "a new line")?;
			if !is_set[field] {
				is_set[field] = true;
				fields.values[field] = value;
			}
		}
	}

	/// `requirementDef` and `elementDef`: the name, classes and body.
	fn definition(&mut self) -> ParseResult<()> {
		let requirement_type = match self.advance().kind {
			TokenKind::RequirementType(requirement_type) => Some(requirement_type),
			_ => None,
		};
		let name = self.string("a name")?;
		let classes = self.classes()?;
		self.expect(TokenKind::StructStart, "{")?;
		self.expect(TokenKind::Newline, "a new line")?;
		let fields = self.body(requirement_type.is_some())?;
		match requirement_type {
			Some(requirement_type) => self.db.add_requirement(&name, requirement_type, fields),
			None => self.db.add_element(&name, fields),
		}
		if !classes.is_empty() {
			self.db.set_class(std::slice::from_ref(&name), &classes);
		}
		Ok(())
	}

	fn relationship(&mut self) -> ParseResult<&'static str> {
		match self.current.kind {
			TokenKind::Relationship(kind) => {
				self.advance();
				Ok(kind)
			}
			_ => self.error("a relationship"),
		}
	}

	/// `relationshipDef` and `id STYLE_SEPARATOR idList`.
	fn id_statement(&mut self) -> ParseResult<()> {
		let id = self.advance().text;
		match self.current.kind {
			TokenKind::EndArrowL => {
				self.advance();
				let kind = self.relationship()?;
				self.expect(TokenKind::Line, "-")?;
				let dst = self.string("an id")?;
				self.db.add_relationship(kind, &dst, &id);
			}
			TokenKind::Line => {
				self.advance();
				let kind = self.relationship()?;
				self.expect(TokenKind::EndArrowR, "->")?;
				let dst = self.string("an id")?;
				self.db.add_relationship(kind, &id, &dst);
			}
			TokenKind::StyleSeparator => {
				self.advance();
				let classes = self.id_list()?;
				self.db.set_class(std::slice::from_ref(&id), &classes);
			}
			_ => return self.error("a relationship or :::"),
		}
		Ok(())
	}

	fn statement(&mut self) -> ParseResult<()> {
		use TokenKind as T;
		if self.directive()? {
			return Ok(());
		}
		match self.current.kind {
			T::Newline => {
				self.advance();
			}
			T::Direction(direction) => {
				self.advance();
				self.db.direction = Some(direction);
			}
			T::RequirementType(_) | T::Element => self.definition()?,
			T::UnqString | T::QString => self.id_statement()?,
			T::Style => {
				self.advance();
				let ids = self.id_list()?;
				let styles = self.styles()?;
				self.db.set_css_style(&ids, &styles);
			}
			T::ClassDef => {
				self.advance();
				let ids = self.id_list()?;
				let styles = self.styles()?;
				self.db.define_class(&ids, &styles);
			}
			T::Class => {
				self.advance();
				let ids = self.id_list()?;
				let classes = self.id_list()?;
				self.db.set_class(&ids, &classes);
			}
			_ => return self.error("a statement"),
		}
		Ok(())
	}
}

/// Parses the code of a requirement diagram into `db`.
pub fn parse(code: &str, common: &mut CommonFields, db: &mut RequirementDb) -> ParseResult<()> {
	let lexer = Lexer::new(code);
	let placeholder = Token { kind: TokenKind::Eof, text: String::new(), line: 1 };
	let mut parser = Parser { lexer, current: placeholder, common, db, accepted_line: 1 };
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
			kinds("test_entity - satisfies -> test_req\n"),
			[
				T::UnqString,
				T::Line,
				T::Relationship("satisfies"),
				T::EndArrowR,
				T::UnqString,
				T::Newline
			]
		);
		assert_eq!(
			kinds("requirement a:::x {\nverifymethod: TEST\n"),
			[
				T::RequirementType("Requirement"),
				T::UnqString,
				T::StyleSeparator,
				T::UnqString,
				T::StructStart,
				T::Newline,
				T::RequirementField(3),
				T::ColonSep,
				T::Verify("Test"),
				T::Newline
			]
		);
		// Keywords end at word boundaries; values that start with one are keywords.
		assert_eq!(kinds("high_req test case"), [T::UnqString]);
		assert_eq!(kinds("test case"), [T::Verify("Test"), T::UnqString]);
		assert_eq!(
			kinds("classDef a,b fill:#f9f, stroke-width:4px\nx"),
			[
				T::ClassDef,
				T::Alpha,
				T::Comma,
				T::Alpha,
				T::Alpha,
				T::Colon,
				T::Brkt,
				T::Alpha,
				T::Comma,
				T::Alpha,
				T::Minus,
				T::Alpha,
				T::Colon,
				T::Alpha,
				T::UnqString
			]
		);
		assert_eq!(kinds("text: set direction LR\n"), [T::Direction("LR"), T::Newline]);
		assert_eq!(
			kinds("style a width:50%"),
			[T::Style, T::Alpha, T::Alpha, T::Colon, T::Alpha, T::Percent]
		);
		// `<string>["]` comes before `<string>[^"]*`: empty quotes are no token.
		assert_eq!(kinds("\"\" ="), [T::Invalid]);
	}

	#[test]
	fn parses_definitions_and_relationships() {
		let code = "requirementDiagram\n  requirement \"a b\":::red {\n    id: 1\n    text: first\n    text: second\n    risk: High\n  }\n  element e {\n    type: \"sim, ulation\"\n    docRef: reqs/e\n  }\n  e - satisfies -> a b\n  a b <- copies - e\n  classDef red fill:#f00\n  e:::red\n  direction LR\n";
		let mut common = CommonFields::default();
		let mut db = RequirementDb::default();
		parse(code, &mut common, &mut db).unwrap();
		let requirement = &db.requirements[0];
		assert_eq!(requirement.name, "a b");
		assert_eq!(requirement.fields.values, ["1", "first", "High", ""]);
		assert_eq!(requirement.classes, ["default", "red"]);
		assert_eq!(db.elements[0].fields.values[..2], ["sim, ulation", "reqs/e"]);
		assert_eq!(db.elements[0].css_styles, ["fill:#f00"]);
		assert_eq!(db.relations.len(), 2);
		assert_eq!((db.relations[1].src.as_str(), db.relations[1].dst.as_str()), ("e", "a b"));
		assert_eq!(db.direction, Some("LR"));
		let error = parse(
			"requirementDiagram\n  requirement a {\n    risk: big\n  }\n",
			&mut common,
			&mut db,
		);
		assert_eq!(error.unwrap_err().line, 3);
	}
}
