//! The grammar of `stateDiagram.jison`: jison's first matching rule wins (case-insensitive,
//! literal keywords end at a word boundary), exclusive states select the rules, and the parser
//! produces the statement tree `StateDB.setRootDoc` gets.

use super::db::{Description, Note, StateStmt, StateType, Stmt};
use crate::mermaid::common::CommonFields;
use crate::mermaid::jison::{
	blank_length, direction_length, is_word_byte, keyword, keyword_and_space, line_length, span,
	starts_with_ignoring_case, word_length,
};

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
	Newline,
	Click,
	Href,
	String,
	Default,
	Direction(&'static str),
	Scale,
	Width,
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	ClassDef,
	DefaultClassDefId,
	ClassDefId,
	ClassDefStyleOpts,
	Class,
	ClassEntityIds,
	StyleClass,
	Style,
	StyleIds,
	StyleDefStyleOpts,
	Fork,
	Join,
	Choice,
	As,
	Id,
	StateDescr,
	CompositState,
	StructStart,
	StructStop,
	Note,
	LeftOf,
	RightOf,
	NoteText,
	Sd,
	HideEmpty,
	EdgeState,
	Descr,
	Arrow,
	Concurrent,
	StyleSeparator,
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
	State,
	StateString,
	StateId,
	Scale,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
	ClassDef,
	ClassDefId,
	Class,
	ClassStyle,
	Style,
	StyleDefStyles,
	Note,
	NoteId,
	NoteText,
	FloatingNote,
	FloatingNoteId,
	Struct,
}

/// Bounds the lexer state stack and the nesting of composite states.
const MAX_STATES: usize = 128;
const MAX_DEPTH: usize = 64;

#[derive(Debug)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type ParseResult<T> = Result<T, ParseError>;

/// The rules of `INITIAL` and `struct` that skip whitespace and comments: the length to skip.
fn comment_length(text: &str) -> usize {
	if text.starts_with('#') || (text.starts_with("%%") && !text[2..].starts_with('{')) {
		return line_length(text);
	}
	0
}

/// `\s*":"(?:[^:\n;]|":"[^:\n;])+`
fn description_length(text: &str) -> usize {
	let spaces = span(text, char::is_whitespace);
	let rest = &text[spaces..];
	if !rest.starts_with(':') {
		return 0;
	}
	let is_plain = |character: char| !matches!(character, ':' | '\n' | ';');
	let mut position = 1;
	loop {
		let tail = &rest[position..];
		let mut characters = tail.chars();
		match characters.next() {
			Some(character) if is_plain(character) => position += character.len_utf8(),
			Some(':') => match characters.next() {
				Some(next) if is_plain(next) => position += 1 + next.len_utf8(),
				_ => break,
			},
			_ => break,
		}
	}
	if position == 1 { 0 } else { spaces + position }
}

struct Lexer<'a> {
	input: &'a str,
	position: usize,
	line: usize,
	states: Vec<Condition>,
	done: bool,
	dividers: usize,
}

impl<'a> Lexer<'a> {
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

	fn token_with(&mut self, kind: TokenKind, length: usize, text: String) -> Token {
		let line = self.line;
		self.take(length);
		Token { kind, text, line }
	}

	/// `processId`: an id ends before a `%%` comment, which goes back to the input; an id that
	/// starts with one is skipped.
	fn id_token(&mut self, kind: TokenKind, length: usize) -> Option<Token> {
		let line = self.line;
		let text = self.take(length);
		match text.find("%%") {
			Some(0) => None,
			Some(index) => {
				// The comment is consumed again from the input.
				self.position -= length - index;
				self.line -= text[index..].matches('\n').count();
				Some(Token { kind, text: text[..index].to_owned(), line })
			}
			None => Some(Token { kind, text: text.to_owned(), line }),
		}
	}

	fn eof(&self) -> Token {
		Token { kind: TokenKind::Eof, text: String::new(), line: self.line }
	}

	fn next_token(&mut self) -> Token {
		loop {
			if self.done {
				return self.eof();
			}
			if self.position >= self.input.len() {
				self.done = true;
			}
			let rest = &self.input[self.position..];
			let result = match self.state() {
				Condition::Initial => self.initial(rest),
				Condition::Struct => self.structure(rest),
				Condition::State => self.state_rules(rest),
				state => self.small_state(state, rest),
			};
			match result {
				Some(token) => return token,
				None if self.done => return self.eof(),
				None => {}
			}
		}
	}

	/// The rules both `INITIAL` and `struct` have, in their order after the state specific ones
	/// they start with.
	fn shared(&mut self, rest: &str) -> Option<Option<Token>> {
		use TokenKind as T;
		if let Some(length) = keyword_and_space(rest, "classDef") {
			self.begin(Condition::ClassDef);
			return Some(Some(self.token(T::ClassDef, length)));
		}
		if let Some(length) = keyword_and_space(rest, "class") {
			self.begin(Condition::Class);
			return Some(Some(self.token(T::Class, length)));
		}
		if let Some(length) = keyword_and_space(rest, "style") {
			self.begin(Condition::Style);
			return Some(Some(self.token(T::Style, length)));
		}
		if let Some(length) = keyword_and_space(rest, "state") {
			self.begin(Condition::State);
			self.take(length);
			return Some(None);
		}
		None
	}

	/// The rules after `{`/`}` that both states have: notes, ids, descriptions and arrows.
	fn shared_tail(&mut self, rest: &str, in_struct: bool) -> Option<Option<Token>> {
		use TokenKind as T;
		if let Some(length) = keyword_and_space(rest, "note") {
			self.begin(Condition::Note);
			return Some(Some(self.token(T::Note, length)));
		}
		if !in_struct {
			for word in ["stateDiagram", "stateDiagram-v2"] {
				if let Some(length) = keyword_and_space(rest, word) {
					return Some(Some(self.token(T::Sd, length)));
				}
			}
			if keyword(rest, "hide empty description") {
				return Some(Some(self.token(T::HideEmpty, "hide empty description".len())));
			}
		}
		if rest.starts_with("[*]") {
			return Some(Some(self.token(T::EdgeState, 3)));
		}
		let id = span(rest, |character| {
			!(character.is_whitespace() || matches!(character, ':' | '-' | '{'))
		});
		if id > 0 {
			return Some(self.id_token(T::Id, id));
		}
		let description = description_length(rest);
		if description > 0 {
			let line = self.line;
			let text = self.take(description).trim().to_owned();
			return Some(Some(Token { kind: T::Descr, text, line }));
		}
		if rest.starts_with("-->") {
			return Some(Some(self.token(T::Arrow, 3)));
		}
		if in_struct && rest.starts_with("--") {
			return Some(Some(self.token(T::Concurrent, 2)));
		}
		if rest.starts_with(":::") {
			return Some(Some(self.token(T::StyleSeparator, 3)));
		}
		None
	}

	fn initial(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		if rest.is_empty() {
			return Some(Token { kind: T::Newline, text: String::new(), line: self.line });
		}
		if keyword(rest, "click") {
			return Some(self.token(T::Click, 5));
		}
		if keyword(rest, "href") {
			return Some(self.token(T::Href, 4));
		}
		if let Some(inner) = rest.strip_prefix('"')
			&& let Some(end) = inner.find('"')
		{
			return Some(self.token(T::String, end + 2));
		}
		if keyword(rest, "default") {
			return Some(self.token(T::Default, 7));
		}
		for dir in ["TB", "BT", "RL", "LR"] {
			if let Some(length) = direction_length(rest, dir) {
				return Some(self.token(T::Direction(dir), length));
			}
		}
		let newlines = span(rest, |character| character == '\n');
		if newlines > 0 {
			return Some(self.token(T::Newline, newlines));
		}
		let spaces = span(rest, char::is_whitespace);
		if spaces > 0 {
			self.take(spaces);
			return None;
		}
		let comment = comment_length(rest);
		if comment > 0 {
			self.take(comment);
			return None;
		}
		if let Some(length) = keyword_and_space(rest, "scale") {
			self.begin(Condition::Scale);
			return Some(self.token(T::Scale, length));
		}
		for (word, kind, state) in [
			("accTitle", T::AccTitle, Condition::AccTitle),
			("accDescr", T::AccDescr, Condition::AccDescr),
		] {
			if starts_with_ignoring_case(rest, word) {
				let after = &rest[word.len()..];
				let spaces = span(after, char::is_whitespace);
				if after[spaces..].starts_with(':') {
					let length = word.len() + spaces + 1;
					let trailing = span(&rest[length..], char::is_whitespace);
					self.begin(state);
					return Some(self.token(kind, length + trailing));
				}
			}
		}
		if starts_with_ignoring_case(rest, "accDescr") {
			let after = &rest["accDescr".len()..];
			let spaces = span(after, char::is_whitespace);
			if after[spaces..].starts_with('{') {
				let length = "accDescr".len() + spaces + 1;
				let trailing = span(&rest[length..], char::is_whitespace);
				self.begin(Condition::AccDescrMultiline);
				self.take(length + trailing);
				return None;
			}
		}
		if let Some(result) = self.shared(rest) {
			return result;
		}
		if rest.starts_with('{') {
			self.pop_state();
			self.begin(Condition::Struct);
			return Some(self.token(T::StructStart, 1));
		}
		if let Some(result) = self.shared_tail(rest, false) {
			return result;
		}
		let length = rest.chars().next().map_or(0, char::len_utf8);
		Some(self.token(T::Invalid, length))
	}

	fn structure(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		if rest.is_empty() {
			return Some(self.eof());
		}
		let blanks = blank_length(rest);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		let comment = comment_length(rest);
		if comment > 0 {
			self.take(comment);
			return None;
		}
		if let Some(result) = self.shared(rest) {
			return result;
		}
		for dir in ["TB", "BT", "RL", "LR"] {
			if let Some(length) = direction_length(rest, dir) {
				return Some(self.token(T::Direction(dir), length));
			}
		}
		if rest.starts_with('}') {
			self.pop_state();
			return Some(self.token(T::StructStop, 1));
		}
		if rest.starts_with('\n') {
			self.take(1);
			return None;
		}
		if let Some(result) = self.shared_tail(rest, true) {
			return result;
		}
		// No rule matches, a lexical error in jison.
		let length = rest.chars().next().map_or(0, char::len_utf8);
		Some(self.token(T::Invalid, length))
	}

	fn state_rules(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		let blanks = blank_length(rest);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		let comment = comment_length(rest);
		if comment > 0 {
			self.take(comment);
			return None;
		}
		let line = &rest[..line_length(rest)];
		for (suffix, kind) in [
			("<<fork>>", T::Fork),
			("<<join>>", T::Join),
			("<<choice>>", T::Choice),
			("[[fork]]", T::Fork),
			("[[join]]", T::Join),
			("[[choice]]", T::Choice),
		] {
			if let Some(start) = line.to_ascii_lowercase().rfind(suffix) {
				self.pop_state();
				let text = line[..start].trim().to_owned();
				return Some(self.token_with(kind, start + suffix.len(), text));
			}
		}
		if rest.starts_with('"') {
			self.begin(Condition::StateString);
			self.take(1);
			return None;
		}
		let spaces = span(rest, char::is_whitespace);
		if let Some(length) = keyword_and_space(&rest[spaces..], "as") {
			self.begin(Condition::StateId);
			return Some(self.token(T::As, spaces + length));
		}
		// `\w+\s+\w+.*?\{`
		let first = word_length(rest);
		if first > 0 {
			let gap = span(&rest[first..], char::is_whitespace);
			let second = word_length(&rest[first + gap..]);
			if gap > 0 && second > 0 {
				let after = first + gap + second;
				if let Some(brace) = rest[after..line_length(&rest[after..]) + after].find('{') {
					let found = rest[..after + brace + 1].trim();
					return Some(Token {
						kind: T::Invalid,
						text: format!("State name must be a single word. Found: \"{found}\""),
						line: self.line,
					});
				}
			}
		}
		let name = span(rest, |character| !(character.is_whitespace() || character == '{'));
		if name > 0 {
			return Some(self.token(T::CompositState, name));
		}
		if rest.starts_with('\n') {
			self.pop_state();
			self.take(1);
			return None;
		}
		if rest.starts_with('{') {
			self.pop_state();
			self.begin(Condition::Struct);
			return Some(self.token(T::StructStart, 1));
		}
		if rest.is_empty() {
			return Some(self.eof());
		}
		let length = rest.chars().next().map_or(0, char::len_utf8);
		Some(self.token(T::Invalid, length))
	}

	fn small_state(&mut self, state: Condition, rest: &str) -> Option<Token> {
		use TokenKind as T;
		let invalid = |lexer: &mut Self| {
			let length = rest.chars().next().map_or(0, char::len_utf8);
			if length == 0 {
				return Some(lexer.eof());
			}
			Some(lexer.token(T::Invalid, length))
		};
		match state {
			Condition::StateString => {
				if rest.starts_with('"') {
					self.pop_state();
					self.take(1);
					return None;
				}
				let length = span(rest, |character| character != '"');
				Some(self.token(T::StateDescr, length))
			}
			Condition::StateId => {
				let length = span(rest, |character| character != '\n' && character != '{');
				let token = self.id_token(T::Id, length);
				if token.is_some() {
					self.pop_state();
				}
				token
			}
			Condition::Scale => {
				let digits = span(rest, |character| character.is_ascii_digit());
				if digits > 0 {
					return Some(self.token(T::Width, digits));
				}
				let spaces = span(rest, char::is_whitespace);
				if spaces > 0 && keyword(&rest[spaces..], "width") {
					self.pop_state();
					self.take(spaces + 5);
					return None;
				}
				invalid(self)
			}
			Condition::AccTitle | Condition::AccDescr => {
				let kind =
					if state == Condition::AccTitle { T::AccTitleValue } else { T::AccDescrValue };
				self.pop_state();
				Some(self.token(kind, line_length(rest)))
			}
			Condition::AccDescrMultiline => {
				if rest.starts_with('}') {
					self.pop_state();
					self.take(1);
					return None;
				}
				let length = span(rest, |character| character != '}');
				Some(self.token(T::AccDescrMultilineValue, length))
			}
			Condition::ClassDef => {
				if let Some(length) = keyword_and_space(rest, "default") {
					self.pop_state();
					self.begin(Condition::ClassDefId);
					return Some(self.token(T::DefaultClassDefId, length));
				}
				let word = word_length(rest);
				let spaces = span(&rest[word..], char::is_whitespace);
				if word > 0 && spaces > 0 {
					self.pop_state();
					self.begin(Condition::ClassDefId);
					return Some(self.token(T::ClassDefId, word + spaces));
				}
				invalid(self)
			}
			Condition::ClassDefId | Condition::ClassStyle | Condition::StyleDefStyles => {
				let kind = match state {
					Condition::ClassDefId => T::ClassDefStyleOpts,
					Condition::ClassStyle => T::StyleClass,
					_ => T::StyleDefStyleOpts,
				};
				self.pop_state();
				Some(self.token(kind, line_length(rest)))
			}
			Condition::Class => {
				// `(\w+)+((","\s*\w+)*)`
				let mut length = word_length(rest);
				if length == 0 {
					return invalid(self);
				}
				while rest[length..].starts_with(',') {
					let spaces = span(&rest[length + 1..], char::is_whitespace);
					let word = word_length(&rest[length + 1 + spaces..]);
					if word == 0 {
						break;
					}
					length += 1 + spaces + word;
				}
				self.pop_state();
				self.begin(Condition::ClassStyle);
				Some(self.token(T::ClassEntityIds, length))
			}
			Condition::Style => {
				let ids =
					rest.bytes().take_while(|&byte| is_word_byte(byte) || byte == b',').count();
				let spaces = span(&rest[ids..], char::is_whitespace);
				if ids > 0 && spaces > 0 {
					self.pop_state();
					self.begin(Condition::StyleDefStyles);
					return Some(self.token(T::StyleIds, ids + spaces));
				}
				invalid(self)
			}
			Condition::Note => {
				for (words, kind) in [("left of", T::LeftOf), ("right of", T::RightOf)] {
					if keyword(rest, words) {
						self.pop_state();
						self.begin(Condition::NoteId);
						return Some(self.token(kind, words.len()));
					}
				}
				if rest.starts_with('"') {
					self.pop_state();
					self.begin(Condition::FloatingNote);
					self.take(1);
					return None;
				}
				invalid(self)
			}
			Condition::FloatingNote => {
				let spaces = span(rest, char::is_whitespace);
				if starts_with_ignoring_case(&rest[spaces..], "as") {
					let trailing = span(&rest[spaces + 2..], char::is_whitespace);
					self.pop_state();
					self.begin(Condition::FloatingNoteId);
					return Some(self.token(T::As, spaces + 2 + trailing));
				}
				if rest.starts_with('"') {
					self.take(1);
					return None;
				}
				let length = span(rest, |character| character != '"');
				if length == 0 {
					return Some(self.eof());
				}
				Some(self.token(T::NoteText, length))
			}
			Condition::FloatingNoteId => {
				let token = self.id_token(T::Id, line_length(rest));
				if token.is_some() {
					self.pop_state();
				}
				token
			}
			Condition::NoteId => {
				// `\s*[^:\n\s\-]+`
				let spaces = span(rest, char::is_whitespace);
				let name = span(&rest[spaces..], |character| {
					!(character.is_whitespace() || matches!(character, ':' | '-'))
				});
				if name == 0 {
					return invalid(self);
				}
				let token = self.id_token(T::Id, spaces + name);
				if token.is_some() {
					self.pop_state();
					self.begin(Condition::NoteText);
				}
				token
			}
			Condition::NoteText => {
				// `\s*":"[^:\n;]+`, of which `substr(2)` is the text.
				let spaces = span(rest, char::is_whitespace);
				if rest[spaces..].starts_with(':') {
					let text_length = span(&rest[spaces + 1..], |character| {
						!matches!(character, ':' | '\n' | ';')
					});
					if text_length > 0 {
						self.pop_state();
						let length = spaces + 1 + text_length;
						let text: String = rest[..length].chars().skip(2).collect();
						let text = text.trim().to_owned();
						return Some(self.token_with(T::NoteText, length, text));
					}
				}
				// `[\s\S]*?\n\s*"end note"\b`
				let lower = rest.to_ascii_lowercase();
				let mut search = 0;
				while let Some(found) = lower[search..].find('\n') {
					let newline = search + found;
					let spaces = span(&rest[newline + 1..], char::is_whitespace);
					let end = newline + 1 + spaces;
					if keyword(&rest[end..], "end note") {
						self.pop_state();
						let length = end + "end note".len();
						let text = rest[..length - "end note".len()].trim().to_owned();
						return Some(self.token_with(T::NoteText, length, text));
					}
					search = newline + 1;
				}
				invalid(self)
			}
			Condition::Initial | Condition::Struct | Condition::State => None,
		}
	}
}

struct Parser<'a, 'c> {
	lexer: Lexer<'a>,
	current: Token,
	common: &'c mut CommonFields,
	depth: usize,
}

impl Parser<'_, '_> {
	fn advance(&mut self) -> ParseResult<Token> {
		let next = self.lexer.next_token();
		Ok(std::mem::replace(&mut self.current, next))
	}

	fn error<T>(&self, expected: &str) -> ParseResult<T> {
		let message = match &self.current.kind {
			TokenKind::Invalid if self.current.text.starts_with("State name") => {
				self.current.text.clone()
			}
			TokenKind::Eof => format!("expected {expected}, found end of input"),
			TokenKind::Newline => format!("expected {expected}, found end of line"),
			_ => format!("expected {expected}, found {:?}", self.current.text),
		};
		Err(ParseError { line: self.current.line, message })
	}

	fn is(&self, kind: &TokenKind) -> bool {
		self.current.kind == *kind
	}

	fn expect(&mut self, kind: TokenKind, expected: &str) -> ParseResult<Token> {
		if self.current.kind != kind {
			return self.error(expected);
		}
		self.advance()
	}

	/// `start: NL* SD document`
	fn start(&mut self) -> ParseResult<Vec<Stmt>> {
		while self.is(&TokenKind::Newline) {
			self.advance()?;
		}
		self.expect(TokenKind::Sd, "stateDiagram")?;
		let document = self.document()?;
		if !self.is(&TokenKind::Eof) {
			return self.error("a statement");
		}
		Ok(document)
	}

	/// `document: line*`, up to the `}` of a composite state or the end.
	fn document(&mut self) -> ParseResult<Vec<Stmt>> {
		let mut statements = Vec::new();
		loop {
			match self.current.kind {
				TokenKind::Eof | TokenKind::StructStop => return Ok(statements),
				TokenKind::Newline => {
					self.advance()?;
				}
				_ => {
					if let Some(statement) = self.statement()? {
						statements.push(statement);
					}
				}
			}
		}
	}

	/// The `doc` of a composite state between `{` and `}`.
	fn nested_document(&mut self) -> ParseResult<Vec<Stmt>> {
		self.expect(TokenKind::StructStart, "{")?;
		if self.depth >= MAX_DEPTH {
			return self.error("fewer nested states");
		}
		self.depth += 1;
		let document = self.document()?;
		self.depth -= 1;
		self.expect(TokenKind::StructStop, "}")?;
		Ok(document)
	}

	/// `idStatement: (ID | EDGE_STATE) (':::' ID)?`
	fn id_statement(&mut self) -> ParseResult<StateStmt> {
		if !matches!(self.current.kind, TokenKind::Id | TokenKind::EdgeState) {
			return self.error("a state");
		}
		let id = self.advance()?.text.trim().to_owned();
		let mut state = StateStmt::new(id, Some(StateType::Default));
		state.description = Some(Description::Text(String::new()));
		if self.is(&TokenKind::StyleSeparator) {
			self.advance()?;
			let class = self.expect(TokenKind::Id, "a class name")?;
			state.classes.push(class.text.trim().to_owned());
		}
		Ok(state)
	}

	fn statement(&mut self) -> ParseResult<Option<Stmt>> {
		use TokenKind as T;
		let statement = match self.current.kind.clone() {
			T::ClassDef => {
				self.advance()?;
				let id = match self.current.kind {
					T::ClassDefId => self.advance()?.text,
					_ => return self.error("a class name"),
				};
				let classes = self.expect(T::ClassDefStyleOpts, "styles")?.text;
				Stmt::ClassDef { id: id.trim().to_owned(), classes: classes.trim().to_owned() }
			}
			T::Style => {
				self.advance()?;
				let id = self.expect(T::StyleIds, "state ids")?.text;
				let style_class = self.expect(T::StyleDefStyleOpts, "styles")?.text;
				Stmt::Style { id: id.trim().to_owned(), style_class: style_class.trim().to_owned() }
			}
			T::Class => {
				self.advance()?;
				let id = self.expect(T::ClassEntityIds, "state ids")?.text;
				let style_class = self.expect(T::StyleClass, "a class name")?.text;
				Stmt::ApplyClass {
					id: id.trim().to_owned(),
					style_class: style_class.trim().to_owned(),
				}
			}
			T::Id | T::EdgeState => {
				let mut state1 = self.id_statement()?;
				match self.current.kind {
					T::Descr => {
						let text = self.advance()?.text;
						state1.description = Some(Description::Text(trim_colon(&text)));
						Stmt::State(state1)
					}
					T::Arrow => {
						self.advance()?;
						let state2 = self.id_statement()?;
						let description = match self.current.kind {
							T::Descr => Some(trim_colon(&self.advance()?.text)),
							_ => None,
						};
						Stmt::Relation { state1, state2, description }
					}
					_ => Stmt::State(state1),
				}
			}
			T::HideEmpty => {
				self.advance()?;
				return Ok(Some(Stmt::Ignored));
			}
			T::Scale => {
				self.advance()?;
				self.expect(T::Width, "a width")?;
				return Ok(Some(Stmt::Ignored));
			}
			T::CompositState => {
				let id = self.advance()?.text;
				if !self.is(&T::StructStart) {
					return Ok(Some(Stmt::Ignored));
				}
				let doc = self.nested_document()?;
				let mut state = StateStmt::new(id, Some(StateType::Default));
				state.description = Some(Description::Text(String::new()));
				state.doc = Some(doc);
				Stmt::State(state)
			}
			T::StateDescr => {
				let description = self.advance()?.text;
				self.expect(T::As, "as")?;
				let id = self.expect(T::Id, "a state id")?.text;
				if self.is(&T::StructStart) {
					let doc = self.nested_document()?;
					let mut state = StateStmt::new(id, Some(StateType::Default));
					state.description = Some(Description::Text(description));
					state.doc = Some(doc);
					Stmt::State(state)
				} else {
					let mut state = StateStmt::new(id.clone(), Some(StateType::Default));
					state.description = Some(Description::Text(description.trim().to_owned()));
					if let Some((first, second)) = id.split_once(':') {
						let second = second.split(':').next().unwrap_or("");
						state.id = first.to_owned();
						state.description = Some(Description::Pair(
							description.trim().to_owned(),
							second.to_owned(),
						));
					}
					Stmt::State(state)
				}
			}
			T::Fork | T::Join | T::Choice => {
				let kind = match self.current.kind {
					T::Fork => StateType::Fork,
					T::Join => StateType::Join,
					_ => StateType::Choice,
				};
				let id = self.advance()?.text;
				Stmt::State(StateStmt::new(id, Some(kind)))
			}
			T::Concurrent => {
				self.advance()?;
				self.lexer.dividers += 1;
				let id = format!("divider-id-{}", self.lexer.dividers);
				Stmt::State(StateStmt::new(id, Some(StateType::Divider)))
			}
			T::Note => {
				self.advance()?;
				match self.current.kind {
					T::LeftOf | T::RightOf => {
						let position = self.advance()?.text.trim().to_owned();
						let id = self.expect(T::Id, "a state id")?.text.trim().to_owned();
						let text = self.expect(T::NoteText, "a note text")?.text.trim().to_owned();
						let mut state = StateStmt::new(id, None);
						state.note =
							Some(Note { is_left: position.eq_ignore_ascii_case("left of"), text });
						Stmt::State(state)
					}
					T::NoteText => {
						self.advance()?;
						self.expect(T::As, "as")?;
						self.expect(T::Id, "a note id")?;
						Stmt::Ignored
					}
					_ => return self.error("left of, right of or a note text"),
				}
			}
			T::Direction(dir) => {
				self.advance()?;
				Stmt::Direction(dir.to_owned())
			}
			T::AccTitle => {
				self.advance()?;
				let value = self.expect(T::AccTitleValue, "a title")?.text;
				self.common.accessibility.title = value.trim().to_owned();
				Stmt::Ignored
			}
			T::AccDescr => {
				self.advance()?;
				let value = self.expect(T::AccDescrValue, "a description")?.text;
				self.common.accessibility.description = value.trim().to_owned();
				Stmt::Ignored
			}
			T::AccDescrMultilineValue => {
				let value = self.advance()?.text;
				self.common.accessibility.description = value.trim().to_owned();
				Stmt::Ignored
			}
			T::Click => {
				self.advance()?;
				let state = self.id_statement()?;
				let (url, tooltip) = match self.current.kind {
					T::String => {
						let url = self.advance()?.text;
						let tooltip = self.expect(T::String, "a tooltip")?.text;
						(url, tooltip)
					}
					T::Href => {
						self.advance()?;
						(self.expect(T::String, "a link")?.text, String::new())
					}
					_ => return self.error("a link"),
				};
				self.expect(T::Newline, "end of line")?;
				Stmt::Click { id: state.id, url, tooltip }
			}
			_ => return self.error("a statement"),
		};
		Ok(Some(statement))
	}
}

/// `trimColon`.
fn trim_colon(text: &str) -> String {
	match text.strip_prefix(':') {
		Some(rest) => rest.trim().to_owned(),
		None => text.trim().to_owned(),
	}
}

/// Parses the code of a state diagram into its root document. Returns the statements and the
/// root direction statement's value.
pub fn parse(code: &str, common: &mut CommonFields) -> ParseResult<Vec<Stmt>> {
	let lexer = Lexer {
		input: code,
		position: 0,
		line: 1,
		states: vec![Condition::Initial],
		done: false,
		dividers: 0,
	};
	let placeholder = Token { kind: TokenKind::Eof, text: String::new(), line: 1 };
	let mut parser = Parser { lexer, current: placeholder, common, depth: 0 };
	parser.advance()?;
	parser.start()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn tokens(code: &str) -> Vec<(TokenKind, String)> {
		let mut lexer = Lexer {
			input: code,
			position: 0,
			line: 1,
			states: vec![Condition::Initial],
			done: false,
			dividers: 0,
		};
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
		use TokenKind as T;
		let token = |kind: T, text: &str| (kind, text.to_owned());
		assert_eq!(
			tokens("stateDiagram-v2\n  [*] --> A : go\n"),
			[
				token(T::Sd, "stateDiagram-v2\n  "),
				token(T::EdgeState, "[*]"),
				token(T::Arrow, "-->"),
				token(T::Id, "A"),
				token(T::Descr, ": go"),
				token(T::Newline, "\n"),
				token(T::Newline, ""),
			]
		);
		assert_eq!(
			tokens("state \"Long name\" as L {\n  X --\n}"),
			[
				token(T::StateDescr, "Long name"),
				token(T::As, "as "),
				token(T::Id, "L "),
				token(T::StructStart, "{"),
				token(T::Id, "X"),
				token(T::Concurrent, "--"),
				token(T::StructStop, "}"),
				token(T::Newline, ""),
			]
		);
		assert_eq!(
			tokens("state f <<fork>>\nnote right of A : a text\nA%%x\n"),
			[
				token(T::Fork, "f"),
				token(
					T::Newline,
					"
"
				),
				token(T::Note, "note "),
				token(T::RightOf, "right of"),
				token(T::Id, " A"),
				token(T::NoteText, "a text"),
				token(T::Newline, "\n"),
				token(T::Id, "A"),
				token(T::Newline, "\n"),
				token(T::Newline, ""),
			]
		);
		assert_eq!(
			tokens("note left of B\n  two\n  lines\nend note\n")[3],
			token(T::NoteText, "two\n  lines")
		);
		assert_eq!(tokens("  direction LR\n")[0], token(T::Direction("LR"), "  direction LR"));
	}

	#[test]
	fn parses_documents() {
		let mut common = CommonFields::default();
		let code = "stateDiagram-v2\n  [*] --> A\n  state C {\n    D --> E\n    --\n    F\n  }\n  \
		            classDef hot fill:#f00\n  class A,C hot\n  style A stroke:#0f0\n  \
		            note right of A : hi\n  direction LR\n";
		let doc = parse(code, &mut common).unwrap();
		assert_eq!(doc.len(), 7);
		assert!(matches!(&doc[0], Stmt::Relation { state1, .. } if state1.id == "[*]"));
		let Stmt::State(composite) = &doc[1] else { panic!() };
		assert_eq!(composite.doc.as_ref().map(Vec::len), Some(3));
		assert!(
			matches!(&doc[2], Stmt::ClassDef { id, classes } if id == "hot" && classes == "fill:#f00")
		);
		assert!(matches!(&doc[3], Stmt::ApplyClass { id, .. } if id == "A,C"));
		assert!(matches!(&doc[6], Stmt::Direction(dir) if dir == "LR"));
		for wrong in [
			"stateDiagram-v2\n  classDef default fill:#f00\n",
			"stateDiagram-v2\n  state Foo\n  state Bar {\n  }\n",
			"stateDiagram-v2\n  note left of B : n:x\n",
			"stateDiagram-v2\n  state A {\n",
			"flowchart\n",
		] {
			assert!(parse(wrong, &mut common).is_err(), "{wrong}");
		}
		assert_eq!(
			parse("stateDiagram-v2\n  state Foo\n  state Bar {\n  }\n", &mut common)
				.unwrap_err()
				.message,
			"State name must be a single word. Found: \"Foo\n  state Bar {\""
		);
	}
}
