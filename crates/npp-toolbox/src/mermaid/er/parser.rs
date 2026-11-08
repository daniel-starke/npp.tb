//! The grammar of `erDiagram.jison`: jison's first matching rule wins (case-insensitive, rules
//! ending with a word character end at a word boundary), exclusive states select the rules, and
//! the parser calls the database like the grammar actions do.

use super::db::{Attribute, Cardinality, ErDb, Item, RelSpec};
use crate::mermaid::common::CommonFields;
use crate::mermaid::jison::{
	direction_length, is_word_byte, keyword, line_length, span, starts_with_ignoring_case,
};

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	Direction(&'static str),
	Newline,
	EntityName,
	Word,
	ErDiagram,
	BlockStart,
	BlockStop,
	Brkt,
	Comma,
	StyleSeparator,
	Colon,
	Sqs,
	Sqe,
	Style,
	ClassDef,
	Class,
	Subgraph,
	End,
	Cardinality(Cardinality),
	DecimalNum,
	EntityOne,
	Num,
	Identifying,
	NonIdentifying,
	UnicodeText,
	AttributeKey,
	AttributeWord,
	Comment,
	StyleText,
	Semi,
	/// A single character of jison's `.` rules.
	Character(char),
	/// Text no rule of the `style` state matches.
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
	Block,
	BlockBackquote,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
	Style,
}

/// Bounds the lexer state stack and the nesting of subgraphs.
const MAX_STATES: usize = 128;
const MAX_DEPTH: usize = 64;

#[derive(Debug)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type ParseResult<T> = Result<T, ParseError>;

/// `\w`
fn is_word(character: char) -> bool {
	character.is_ascii() && is_word_byte(character as u8)
}

/// `([^\x00-\x7F]|\w|\-|\*|\.)+`
fn unicode_text_length(text: &str) -> usize {
	span(text, |character| {
		!character.is_ascii() || is_word(character) || matches!(character, '-' | '*' | '.')
	})
}

/// `<style>([^\x00-\x7F]|\w|\-|\*)+`
fn style_text_length(text: &str) -> usize {
	span(text, |character| {
		!character.is_ascii() || is_word(character) || matches!(character, '-' | '*')
	})
}

/// `[À-￿]`: UTF-16 code units, so astral characters (surrogates) match too.
fn is_extended(character: char) -> bool {
	character >= '\u{C0}'
}

/// `[\*A-Za-z_À-￿][A-Za-z0-9\-\_\[\]\(\)\.,À-￿\*]*`
fn attribute_word_length(text: &str) -> usize {
	let mut characters = text.chars();
	match characters.next() {
		Some(first) if first == '*' || first == '_' || first.is_ascii_alphabetic() => {}
		Some(first) if is_extended(first) => {}
		_ => return 0,
	}
	let first_length = text.chars().next().map_or(0, char::len_utf8);
	first_length
		+ span(&text[first_length..], |character| {
			character.is_ascii_alphanumeric()
				|| is_extended(character)
				|| matches!(character, '-' | '_' | '[' | ']' | '(' | ')' | '.' | ',' | '*')
		})
}

/// JavaScript's `.`: any character but a line terminator.
fn is_line_terminator(character: char) -> bool {
	matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `([^\s]*)[~].*[~]([^\s]*)` with JavaScript's backtracking: the longest run of non-blanks
/// that has a `~` followed by another `~` on the line, up to the last `~` and the non-blanks
/// after it.
fn generic_word_length(text: &str) -> Option<usize> {
	let run = span(text, |character| !character.is_whitespace());
	let line = span(text, |character| !is_line_terminator(character));
	let last = text[..line].rfind('~')?;
	// The `~` of the first bracket is the last one in the run before `last`.
	text[..run.min(last)].rfind('~')?;
	let after = last + 1;
	Some(after + span(&text[after..], |character| !character.is_whitespace()))
}

/// `\"[^"%\r\n\v\b\\]+\"` (`ENTITY_NAME`) or `\"[^"]*\"` (`WORD`): the kind and length.
fn quoted_length(text: &str) -> Option<(TokenKind, usize)> {
	let inner = text.strip_prefix('"')?;
	let end = inner.find('"')?;
	let content = &inner[..end];
	let is_name =
		!content.is_empty() && !content.contains(['%', '\r', '\n', '\u{b}', '\u{8}', '\\']);
	let kind = if is_name { TokenKind::EntityName } else { TokenKind::Word };
	Some((kind, end + 2))
}

/// `name\s*<delimiter>\s*`
fn keyword_then(text: &str, name: &str, delimiter: char) -> Option<usize> {
	if !starts_with_ignoring_case(text, name) {
		return None;
	}
	let after = name.len() + span(&text[name.len()..], char::is_whitespace);
	if !text[after..].starts_with(delimiter) {
		return None;
	}
	let value = after + 1;
	Some(value + span(&text[value..], char::is_whitespace))
}

/// How a rule of literal text matches.
enum Rule {
	/// The text ignoring case, then a word boundary.
	Word(&'static str),
	/// The text ignoring case.
	Text(&'static str),
	/// `u(?=[\.\-\|])`
	MdParent,
}

impl Rule {
	fn length(&self, rest: &str) -> Option<usize> {
		match *self {
			Rule::Word(word) => keyword(rest, word).then_some(word.len()),
			Rule::Text(text) => starts_with_ignoring_case(rest, text).then_some(text.len()),
			Rule::MdParent => {
				let after = rest.strip_prefix(['u', 'U'])?;
				after.starts_with(['.', '-', '|']).then_some(1)
			}
		}
	}
}

/// The cardinality rules of `INITIAL` before the numbers. `\|o` and `\}o` end with a word
/// character, so jison adds a word boundary to them.
const CARDINALITY_RULES: &[(Rule, Cardinality)] = &[
	(Rule::Word("one or zero"), Cardinality::ZeroOrOne),
	(Rule::Word("one or more"), Cardinality::OneOrMore),
	(Rule::Word("one or many"), Cardinality::OneOrMore),
	(Rule::Text("1+"), Cardinality::OneOrMore),
	(Rule::Word("|o"), Cardinality::ZeroOrOne),
	(Rule::Word("zero or one"), Cardinality::ZeroOrOne),
	(Rule::Word("zero or more"), Cardinality::ZeroOrMore),
	(Rule::Word("zero or many"), Cardinality::ZeroOrMore),
	(Rule::Text("0+"), Cardinality::ZeroOrMore),
	(Rule::Word("}o"), Cardinality::ZeroOrMore),
	(Rule::Text("many(0)"), Cardinality::ZeroOrMore),
	(Rule::Text("many(1)"), Cardinality::OneOrMore),
	(Rule::Word("many"), Cardinality::ZeroOrMore),
	(Rule::Text("}|"), Cardinality::OneOrMore),
	(Rule::Word("one"), Cardinality::OnlyOne),
	(Rule::Word("only one"), Cardinality::OnlyOne),
];

/// The cardinality and identification rules of `INITIAL` after the numbers.
const SYMBOL_RULES: &[(Rule, TokenKind)] = &[
	(Rule::Text("||"), TokenKind::Cardinality(Cardinality::OnlyOne)),
	(Rule::Text("o|"), TokenKind::Cardinality(Cardinality::ZeroOrOne)),
	(Rule::Text("o{"), TokenKind::Cardinality(Cardinality::ZeroOrMore)),
	(Rule::Text("|{"), TokenKind::Cardinality(Cardinality::OneOrMore)),
	(Rule::MdParent, TokenKind::Cardinality(Cardinality::MdParent)),
	(Rule::Text(".."), TokenKind::NonIdentifying),
	(Rule::Text("--"), TokenKind::Identifying),
	(Rule::Word("to"), TokenKind::Identifying),
	(Rule::Word("optionally to"), TokenKind::NonIdentifying),
	(Rule::Text(".-"), TokenKind::NonIdentifying),
	(Rule::Text("-."), TokenKind::NonIdentifying),
];

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

	/// Consumes `length` bytes; returns them.
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

	/// jison's `.` rule: the next character as its own token.
	fn character(&mut self, rest: &str) -> Option<Token> {
		let character = rest.chars().next()?;
		Some(self.token(TokenKind::Character(character), character.len_utf8()))
	}

	fn next_token(&mut self) -> Token {
		loop {
			if self.done {
				return Token { kind: TokenKind::Eof, text: String::new(), line: self.line };
			}
			if self.position >= self.input.len() {
				self.done = true;
			}
			let rest = &self.input[self.position..];
			let result = match self.state() {
				Condition::Initial => self.initial(rest),
				Condition::Block => self.block(rest),
				Condition::BlockBackquote => self.block_backquote(rest),
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
		if rest.is_empty() {
			return None;
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
		let blanks = span(rest, |character| matches!(character, ' ' | '\t' | '\r'));
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		let newlines = span(rest, |character| character == '\n');
		if newlines > 0 {
			return Some(self.token(T::Newline, newlines));
		}
		if let Some((kind, length)) = quoted_length(rest) {
			return Some(self.token(kind, length));
		}
		if keyword(rest, "erDiagram") {
			return Some(self.token(T::ErDiagram, "erDiagram".len()));
		}
		let symbols: [(&str, T); 5] = [
			("{", T::BlockStart),
			("#", T::Brkt),
			(",", T::Comma),
			(":::", T::StyleSeparator),
			(":", T::Colon),
		];
		for (symbol, kind) in symbols {
			if rest.starts_with(symbol) {
				if kind == T::BlockStart {
					self.begin(Condition::Block);
				}
				return Some(self.token(kind, symbol.len()));
			}
		}
		if rest.starts_with('[') {
			return Some(self.token(T::Sqs, 1));
		}
		if rest.starts_with(']') {
			return Some(self.token(T::Sqe, 1));
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
			return Some(self.token(T::Class, "class".len()));
		}
		if keyword(rest, "subgraph") {
			return Some(self.token(T::Subgraph, "subgraph".len()));
		}
		if keyword(rest, "end") {
			let length = 3 + span(&rest[3..], char::is_whitespace);
			return Some(self.token(T::End, length));
		}
		if let Some(token) = self.relationship(rest) {
			return Some(token);
		}
		let text = unicode_text_length(rest);
		if text > 0 {
			return Some(self.token(T::UnicodeText, text));
		}
		self.character(rest)
	}

	/// The cardinality and identification rules of `INITIAL`, in the grammar's order.
	fn relationship(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		for (rule, kind) in CARDINALITY_RULES {
			if let Some(length) = rule.length(rest) {
				return Some(self.token(T::Cardinality(*kind), length));
			}
		}
		let digits = span(rest, |character| character.is_ascii_digit());
		if digits > 0 && rest[digits..].starts_with('.') {
			let decimals = span(&rest[digits + 1..], |character| character.is_ascii_digit());
			if decimals > 0 {
				return Some(self.token(T::DecimalNum, digits + 1 + decimals));
			}
		}
		if let Some(after) = rest.strip_prefix('1') {
			// `"1"(?=\s+[A-Za-z_"'])`, `"1"(?=\s+[0-9])` and `"1"(?=(\-\-|\.\.|\.\-|\-\.))`.
			let spaces = span(after, char::is_whitespace);
			let is_one = spaces > 0
				&& after[spaces..].chars().next().is_some_and(|next| {
					next.is_ascii_alphanumeric() || matches!(next, '_' | '"' | '\'')
				});
			let is_operator = ["--", "..", ".-", "-."].iter().any(|op| after.starts_with(op));
			if is_one || is_operator {
				return Some(self.token(T::Cardinality(Cardinality::OnlyOne), 1));
			}
			if keyword(rest, "1") {
				return Some(self.token(T::EntityOne, 1));
			}
		}
		if digits > 0 {
			return Some(self.token(T::Num, digits));
		}
		for (rule, kind) in SYMBOL_RULES {
			if let Some(length) = rule.length(rest) {
				return Some(self.token(kind.clone(), length));
			}
		}
		None
	}

	fn block(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		if rest.is_empty() {
			return None;
		}
		let blanks = span(rest, char::is_whitespace);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		// `\b` before the key: the character before belongs to an earlier token.
		let after_word = self.input[..self.position].chars().next_back().is_some_and(is_word);
		if !after_word && ["PK", "FK", "UK"].iter().any(|key| keyword(rest, key)) {
			return Some(self.token(T::AttributeKey, 2));
		}
		if let Some(length) = generic_word_length(rest) {
			return Some(self.token(T::AttributeWord, length));
		}
		let word = attribute_word_length(rest);
		if word > 0 {
			return Some(self.token(T::AttributeWord, word));
		}
		if rest.starts_with('`') {
			self.begin(Condition::BlockBackquote);
			self.take(1);
			return None;
		}
		if let Some(end) = rest.strip_prefix('"').and_then(|inner| inner.find('"')) {
			return Some(self.token(T::Comment, end + 2));
		}
		if rest.starts_with('}') {
			self.pop_state();
			return Some(self.token(T::BlockStop, 1));
		}
		self.character(rest)
	}

	fn block_backquote(&mut self, rest: &str) -> Option<Token> {
		if rest.is_empty() {
			return None;
		}
		if rest.starts_with('`') {
			self.pop_state();
			self.take(1);
			return None;
		}
		let length = rest.find('`').unwrap_or(rest.len());
		Some(self.token(TokenKind::AttributeWord, length))
	}

	fn style(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		if rest.is_empty() {
			return None;
		}
		let newlines = span(rest, |character| character == '\n');
		if newlines > 0 {
			self.pop_state();
			return Some(self.token(T::Newline, newlines));
		}
		let blanks = span(rest, char::is_whitespace);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		for (symbol, kind) in [(':', T::Colon), (',', T::Comma), ('#', T::Brkt)] {
			if rest.starts_with(symbol) {
				return Some(self.token(kind, 1));
			}
		}
		let text = style_text_length(rest);
		if text > 0 {
			return Some(self.token(T::StyleText, text));
		}
		if rest.starts_with(';') {
			return Some(self.token(T::Semi, 1));
		}
		// No rule matches: jison stops with a lexical error.
		let length = rest.len();
		Some(self.token(T::Invalid, length))
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

struct Parser<'a, 'c> {
	lexer: Lexer<'a>,
	current: Token,
	common: &'c mut CommonFields,
	db: &'c mut ErDb,
	depth: usize,
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

	fn is(&self, kind: &TokenKind) -> bool {
		self.current.kind == *kind
	}

	fn expect(&mut self, kind: TokenKind, expected: &str) -> ParseResult<Token> {
		if self.current.kind != kind {
			return self.error(expected);
		}
		Ok(self.advance())
	}

	/// `start: ER_DIAGRAM document EOF`; blank lines before it are what preprocessing left of
	/// front matter and directives.
	fn start(&mut self) -> ParseResult<()> {
		while self.is(&TokenKind::Newline) {
			self.advance();
		}
		self.expect(TokenKind::ErDiagram, "erDiagram")?;
		self.document()?;
		if !self.is(&TokenKind::Eof) {
			return self.error("a statement");
		}
		Ok(())
	}

	/// `document: line*`, up to the `end` of a subgraph or the end; the items of its statements.
	fn document(&mut self) -> ParseResult<Vec<Item>> {
		let mut items = Vec::new();
		loop {
			match self.current.kind {
				TokenKind::Eof | TokenKind::End => return Ok(items),
				TokenKind::Newline => {
					self.advance();
				}
				_ => items.extend(self.statement()?),
			}
		}
	}

	fn is_entity_name(&self) -> bool {
		matches!(
			self.current.kind,
			TokenKind::EntityName
				| TokenKind::UnicodeText
				| TokenKind::Num
				| TokenKind::DecimalNum
				| TokenKind::EntityOne
		)
	}

	/// `entityName`
	fn entity_name(&mut self) -> ParseResult<String> {
		if !self.is_entity_name() {
			return self.error("an entity name");
		}
		let token = self.advance();
		Ok(match token.kind {
			TokenKind::EntityName => token.text.replace('"', ""),
			_ => token.text,
		})
	}

	/// `idList: (UNICODE_TEXT | STYLE_TEXT) (COMMA (UNICODE_TEXT | STYLE_TEXT))*`
	fn id_list(&mut self) -> ParseResult<Vec<String>> {
		let mut ids = Vec::new();
		loop {
			if !matches!(self.current.kind, TokenKind::UnicodeText | TokenKind::StyleText) {
				return self.error("an id");
			}
			ids.push(self.advance().text);
			if !self.is(&TokenKind::Comma) {
				return Ok(ids);
			}
			self.advance();
		}
	}

	/// `stylesOpt: style (COMMA style)*` with `style: (STYLE_TEXT | NUM | COLON | BRKT)+`: the
	/// components of a style are joined without the blanks between them.
	fn styles(&mut self) -> ParseResult<Vec<String>> {
		let mut styles = Vec::new();
		loop {
			let mut style = String::new();
			while matches!(
				self.current.kind,
				TokenKind::StyleText | TokenKind::Num | TokenKind::Colon | TokenKind::Brkt
			) {
				style.push_str(&self.advance().text);
			}
			if style.is_empty() {
				return self.error("a style");
			}
			styles.push(style);
			if !self.is(&TokenKind::Comma) {
				return Ok(styles);
			}
			self.advance();
		}
	}

	/// `separator: SEMI | NEWLINE | EOF`
	fn separator(&mut self) -> ParseResult<()> {
		match self.current.kind {
			TokenKind::Semi | TokenKind::Newline => {
				self.advance();
				Ok(())
			}
			TokenKind::Eof => Ok(()),
			_ => self.error("the end of the statement"),
		}
	}

	fn cardinality(&mut self) -> ParseResult<Cardinality> {
		match self.current.kind {
			TokenKind::Cardinality(cardinality) => {
				self.advance();
				Ok(cardinality)
			}
			_ => self.error("a cardinality"),
		}
	}

	/// `relSpec: cardinality relType cardinality`
	fn rel_spec(&mut self) -> ParseResult<RelSpec> {
		let card_b = self.cardinality()?;
		let is_identifying = match self.current.kind {
			TokenKind::Identifying => true,
			TokenKind::NonIdentifying => false,
			_ => return self.error("a relationship type"),
		};
		self.advance();
		let card_a = self.cardinality()?;
		Ok(RelSpec { card_a, card_b, is_identifying })
	}

	/// `role: WORD | ENTITY_NAME | UNICODE_TEXT`
	fn role(&mut self) -> ParseResult<String> {
		match self.current.kind {
			TokenKind::Word | TokenKind::EntityName => Ok(self.advance().text.replace('"', "")),
			TokenKind::UnicodeText => Ok(self.advance().text),
			_ => self.error("a relationship label"),
		}
	}

	/// `attributes` between `{` and `}`.
	fn attributes(&mut self) -> ParseResult<Vec<Attribute>> {
		let mut attributes = Vec::new();
		while self.is(&TokenKind::AttributeWord) {
			let mut type_name = self.advance().text;
			if self.is(&TokenKind::Character('?')) {
				type_name.push('?');
				self.advance();
			}
			let name = self.expect(TokenKind::AttributeWord, "an attribute name")?.text;
			let mut keys = Vec::new();
			if self.is(&TokenKind::AttributeKey) {
				keys.push(self.advance().text);
				while self.is(&TokenKind::Character(',')) {
					self.advance();
					keys.push(self.expect(TokenKind::AttributeKey, "a key")?.text);
				}
			}
			let comment = match self.is(&TokenKind::Comment) {
				true => self.advance().text.replace('"', ""),
				false => String::new(),
			};
			attributes.push(Attribute { type_name, name, keys, comment });
		}
		Ok(attributes)
	}

	/// `BLOCK_START attributes? BLOCK_STOP`
	fn block(&mut self) -> ParseResult<Vec<Attribute>> {
		self.expect(TokenKind::BlockStart, "{")?;
		let attributes = self.attributes()?;
		self.expect(TokenKind::BlockStop, "an attribute or }")?;
		Ok(attributes)
	}

	/// `(STYLE_SEPARATOR idList)?`
	fn classes(&mut self) -> ParseResult<Vec<String>> {
		if !self.is(&TokenKind::StyleSeparator) {
			return Ok(Vec::new());
		}
		self.advance();
		self.id_list()
	}

	/// The statements that start with an entity name: relationships and entities.
	fn entity_statement(&mut self) -> ParseResult<Vec<Item>> {
		let name = self.entity_name()?;
		if self.is(&TokenKind::Sqs) {
			self.advance();
			let alias = self.entity_name()?;
			self.expect(TokenKind::Sqe, "]")?;
			let classes = self.classes()?;
			let attributes = match self.is(&TokenKind::BlockStart) {
				true => self.block()?,
				false => Vec::new(),
			};
			self.db.add_entity(&name, &alias);
			self.db.add_attributes(&name, attributes);
			if !classes.is_empty() {
				self.db.set_class(std::slice::from_ref(&name), &classes);
			}
			return Ok(vec![Item::Name(name)]);
		}
		let classes = self.classes()?;
		if matches!(self.current.kind, TokenKind::Cardinality(_)) {
			let spec = self.rel_spec()?;
			let other = self.entity_name()?;
			let other_classes = self.classes()?;
			self.expect(TokenKind::Colon, ":")?;
			let role = self.role()?;
			self.db.add_entity(&name, "");
			self.db.add_entity(&other, "");
			self.db.add_relationship(&name, &role, &other, spec);
			if !classes.is_empty() {
				self.db.set_class(std::slice::from_ref(&name), &classes);
			}
			if !other_classes.is_empty() {
				self.db.set_class(std::slice::from_ref(&other), &other_classes);
			}
			return Ok(vec![Item::Name(name), Item::Name(other)]);
		}
		let attributes = match self.is(&TokenKind::BlockStart) {
			true => self.block()?,
			false => Vec::new(),
		};
		self.db.add_entity(&name, "");
		self.db.add_attributes(&name, attributes);
		if !classes.is_empty() {
			self.db.set_class(std::slice::from_ref(&name), &classes);
		}
		Ok(vec![Item::Name(name)])
	}

	/// `subgraphHeader document END`
	fn subgraph(&mut self) -> ParseResult<Vec<Item>> {
		self.advance();
		let id = self.entity_name()?;
		let mut title = id.clone();
		if self.is(&TokenKind::Sqs) {
			self.advance();
			title = self.entity_name()?;
			while self.is_entity_name() {
				title.push(' ');
				title.push_str(&self.entity_name()?);
			}
			self.expect(TokenKind::Sqe, "]")?;
		}
		self.separator()?;
		if self.depth >= MAX_DEPTH {
			return self.error("fewer nested subgraphs");
		}
		self.depth += 1;
		let items = self.document()?;
		self.depth -= 1;
		self.expect(TokenKind::End, "end")?;
		Ok(vec![Item::Name(self.db.add_subgraph(&id, items, &title))])
	}

	fn statement(&mut self) -> ParseResult<Vec<Item>> {
		use TokenKind as T;
		match self.current.kind.clone() {
			T::AccTitle => {
				self.advance();
				let value = self.expect(T::AccTitleValue, "a title")?.text.trim().to_owned();
				self.common.accessibility.title = value.clone();
				Ok(vec![Item::Name(value)])
			}
			T::AccDescr => {
				self.advance();
				let value = self.expect(T::AccDescrValue, "a description")?.text.trim().to_owned();
				self.common.accessibility.description = value.clone();
				Ok(vec![Item::Name(value)])
			}
			T::AccDescrMultilineValue => {
				let value = self.advance().text.trim().to_owned();
				self.common.accessibility.description = value.clone();
				Ok(vec![Item::Name(value)])
			}
			T::Direction(direction) => {
				self.advance();
				if self.depth == 0 {
					self.db.direction = direction.to_owned();
					return Ok(Vec::new());
				}
				Ok(vec![Item::Direction(direction)])
			}
			T::ClassDef => {
				let keyword = self.advance().text;
				let ids = self.id_list()?;
				let styles = self.styles()?;
				self.separator()?;
				self.db.add_class(&ids, &styles);
				Ok(vec![Item::Name(keyword)])
			}
			T::Class => {
				let keyword = self.advance().text;
				let ids = self.id_list()?;
				let classes = self.id_list()?;
				self.db.set_class(&ids, &classes);
				Ok(vec![Item::Name(keyword)])
			}
			T::Style => {
				let keyword = self.advance().text;
				let ids = self.id_list()?;
				let styles = self.styles()?;
				self.separator()?;
				self.db.add_css_styles(&ids, &styles);
				Ok(vec![Item::Name(keyword)])
			}
			T::Subgraph => self.subgraph(),
			_ if self.is_entity_name() => self.entity_statement(),
			_ => self.error("a statement"),
		}
	}
}

/// Parses the code of an ER diagram into `db`.
pub fn parse(code: &str, common: &mut CommonFields, db: &mut ErDb) -> ParseResult<()> {
	let lexer = Lexer::new(code);
	let placeholder = Token { kind: TokenKind::Eof, text: String::new(), line: 1 };
	let mut parser = Parser { lexer, current: placeholder, common, db, depth: 0, accepted_line: 1 };
	parser.advance();
	parser.start()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn tokens(code: &str) -> Vec<(TokenKind, String)> {
		let mut lexer = Lexer::new(code);
		let mut tokens = Vec::new();
		for _ in 0..1000 {
			let token = lexer.next_token();
			if token.kind == TokenKind::Eof {
				break;
			}
			tokens.push((token.kind, token.text));
		}
		tokens
	}

	#[test]
	fn lexes_like_jison() {
		use Cardinality as C;
		use TokenKind as T;
		let kinds = |code: &str| tokens(code).into_iter().map(|(kind, _)| kind).collect::<Vec<_>>();
		assert_eq!(
			kinds("A ||--o{ B : places"),
			[
				T::UnicodeText,
				T::Cardinality(C::OnlyOne),
				T::Identifying,
				T::Cardinality(C::ZeroOrMore),
				T::UnicodeText,
				T::Colon,
				T::UnicodeText
			]
		);
		assert_eq!(
			kinds("1 one or more to many(0) 1.5"),
			[
				T::Cardinality(C::OnlyOne),
				T::Cardinality(C::OneOrMore),
				T::Identifying,
				T::Cardinality(C::ZeroOrMore),
				T::DecimalNum
			]
		);
		assert_eq!(kinds("10 1"), [T::Num, T::EntityOne]);
		assert_eq!(
			kinds("u--|{ oneThing"),
			[
				T::Cardinality(C::MdParent),
				T::Identifying,
				T::Cardinality(C::OneOrMore),
				T::UnicodeText
			]
		);
		assert_eq!(
			tokens("A {\n  Map~K,V~ items PK, FK \"note\"\n  string? `my name`\n}"),
			[
				(T::UnicodeText, "A".to_owned()),
				(T::BlockStart, "{".to_owned()),
				(T::AttributeWord, "Map~K,V~".to_owned()),
				(T::AttributeWord, "items".to_owned()),
				(T::AttributeKey, "PK".to_owned()),
				(T::Character(','), ",".to_owned()),
				(T::AttributeKey, "FK".to_owned()),
				(T::Comment, "\"note\"".to_owned()),
				(T::AttributeWord, "string".to_owned()),
				(T::Character('?'), "?".to_owned()),
				(T::AttributeWord, "my name".to_owned()),
				(T::BlockStop, "}".to_owned()),
			]
		);
		assert_eq!(
			tokens("classDef foo fill:#f9f, stroke-width: 4px\nA"),
			[
				(T::ClassDef, "classDef".to_owned()),
				(T::StyleText, "foo".to_owned()),
				(T::StyleText, "fill".to_owned()),
				(T::Colon, ":".to_owned()),
				(T::Brkt, "#".to_owned()),
				(T::StyleText, "f9f".to_owned()),
				(T::Comma, ",".to_owned()),
				(T::StyleText, "stroke-width".to_owned()),
				(T::Colon, ":".to_owned()),
				(T::StyleText, "4px".to_owned()),
				(T::Newline, "\n".to_owned()),
				(T::UnicodeText, "A".to_owned()),
			]
		);
		assert_eq!(kinds("A : direction LR\n"), [T::Direction("LR"), T::Newline]);
		assert_eq!(kinds("\"a b\" \"\" \"50%\""), [T::EntityName, T::Word, T::Word]);
	}

	#[test]
	fn finds_generic_words() {
		assert_eq!(generic_word_length("List~int~ name"), Some(9));
		assert_eq!(generic_word_length("Map~K, V~ x"), Some(9));
		assert_eq!(generic_word_length("a~b c"), None);
		assert_eq!(generic_word_length("plain ~x~"), None);
	}

	#[test]
	fn parses_statements() {
		let code = "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER {\n    string id PK\n    int total \"sum\"\n  }\n  P[Person]:::red\n  subgraph S [Sales area]\n    direction LR\n    ORDER\n  end\n  classDef red fill:#f00\n  style ORDER stroke:#000\n";
		let mut common = CommonFields::default();
		let mut db = ErDb::default();
		parse(code, &mut common, &mut db).unwrap();
		let names: Vec<_> = db.entities.iter().map(|entity| entity.id.as_str()).collect();
		assert_eq!(names, ["entity-CUSTOMER-0", "entity-ORDER-1", "entity-P-2"]);
		assert_eq!(db.entities[1].attributes.len(), 2);
		assert_eq!(db.entities[1].attributes[0].keys, ["PK"]);
		assert_eq!(db.entities[1].attributes[1].comment, "sum");
		assert_eq!(db.entities[1].css_styles, ["stroke:#000"]);
		assert_eq!(db.entities[2].alias, "Person");
		assert_eq!(db.entities[2].css_classes, "default red");
		assert_eq!(db.relationships.len(), 1);
		assert_eq!(db.subgraphs[0].title, "Sales area");
		assert_eq!(db.subgraphs[0].nodes, ["ORDER"]);
		assert_eq!(db.subgraphs[0].dir.as_deref(), Some("LR"));
		let error = parse("erDiagram\n  A {\n    string\n  }\n", &mut common, &mut db);
		// Like jison, the line of `string`, the last token before the error.
		assert_eq!(error.unwrap_err().line, 3);
	}
}
