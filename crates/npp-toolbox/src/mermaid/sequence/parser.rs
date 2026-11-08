//! The grammar of `sequenceDiagram.jison`: jison's first matching rule wins (literal keywords end
//! at a word boundary), exclusive states select the rules, and the parser produces the statements
//! in the order `SequenceDB.apply` sees them.

use super::db::{CentralConnection, LineType, NoteActors, Op, Placement, Text, extract_wrap};
use crate::mermaid::common::CommonFields;

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
	Newline,
	Num,
	ConfigStart,
	ConfigContent,
	ConfigEnd,
	Actor,
	Invalid,
	Box,
	Participant,
	ParticipantActor,
	Create,
	Destroy,
	As,
	Loop,
	Rect,
	Opt,
	Alt,
	Else,
	Par,
	ParOver,
	And,
	Critical,
	Option,
	Break,
	RestOfLine,
	End,
	LeftOf,
	RightOf,
	Links,
	Link,
	Properties,
	Details,
	Over,
	Note,
	Activate,
	Deactivate,
	Title,
	LegacyTitle,
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	SequenceDiagram,
	Autonumber,
	Off,
	Comma,
	Arrow(LineType),
	Text,
	Plus,
	Minus,
	Central,
	Eof,
}

#[derive(Clone, Debug)]
struct Token {
	kind: TokenKind,
	text: String,
	line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
	Initial,
	Id,
	Alias,
	Line,
	Config,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
}

/// Bounds the lexer state stack and the nesting of blocks.
const MAX_STATES: usize = 64;
const MAX_DEPTH: usize = 100;

/// JavaScript's `\b` after an ASCII keyword: the next character is no word character.
fn is_word_end(text: &str) -> bool {
	!text.bytes().next().is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// `keyword` at the start of `text`, ignoring ASCII case.
fn starts_with_ignoring_case(text: &str, keyword: &str) -> bool {
	text.get(..keyword.len()).is_some_and(|start| start.eq_ignore_ascii_case(keyword))
}

/// A literal keyword rule: the keyword, ignoring case, followed by a word boundary.
fn keyword(text: &str, keyword: &str) -> bool {
	starts_with_ignoring_case(text, keyword) && is_word_end(&text[keyword.len()..])
}

fn span(text: &str, predicate: impl Fn(char) -> bool) -> usize {
	text.find(|character| !predicate(character)).unwrap_or(text.len())
}

/// `((?!\n)\s)+`
fn blank_length(text: &str) -> usize {
	span(text, |character| character.is_whitespace() && character != '\n')
}

/// The arrows of the grammar in the order of its rules.
const ARROWS: &[(&str, LineType)] = &[
	("->>", LineType::Solid),
	("<<->>", LineType::BidirectionalSolid),
	("-->>", LineType::Dotted),
	("<<-->>", LineType::BidirectionalDotted),
	("->", LineType::SolidOpen),
	("-->", LineType::DottedOpen),
	("-x", LineType::SolidCross),
	("--x", LineType::DottedCross),
	("-)", LineType::SolidPoint),
	("--)", LineType::DottedPoint),
	("--|\\", LineType::SolidTopDotted),
	("--|/", LineType::SolidBottomDotted),
	("--\\\\", LineType::StickTopDotted),
	("--//", LineType::StickBottomDotted),
	("/|--", LineType::SolidArrowTopReverseDotted),
	("\\|--", LineType::SolidArrowBottomReverseDotted),
	("//--", LineType::StickArrowTopReverseDotted),
	("\\\\--", LineType::StickArrowBottomReverseDotted),
	("-|\\", LineType::SolidTop),
	("-|/", LineType::SolidBottom),
	("-\\\\", LineType::StickTop),
	("-//", LineType::StickBottom),
	("/|-", LineType::SolidArrowTopReverse),
	("\\|-", LineType::SolidArrowBottomReverse),
	("//-", LineType::StickArrowTopReverse),
	("\\\\-", LineType::StickArrowBottomReverse),
];

/// What ends the name of an actor: the arrows a name must not run into.
const ARROW_STARTS: &[&str] = &[
	"-x", "--x", "-)", "--)", "-|\\", "-\\", "-/", "-//", "-|/", "/|-", "\\|-", "//-", "\\\\-",
	"/|-", "--|\\", "--", "()",
];

/// Length of the `ACTOR` rule's match at the start of `text`.
fn actor_length(text: &str) -> usize {
	let first = span(text, |character| !"/\\+()<->:\n,;".contains(character));
	if first == 0 {
		return 0;
	}
	let mut position = first;
	loop {
		let rest = &text[position..];
		if ARROW_STARTS.iter().any(|arrow| starts_with_ignoring_case(rest, arrow)) {
			break;
		}
		let hyphens = span(rest, |character| character == '-');
		let more = span(&rest[hyphens..], |character| !"+<->:\n,;".contains(character));
		if more == 0 {
			break;
		}
		position += hyphens + more;
	}
	position
}

/// `([0-9]+(\.[0-9]{1,2})?|\.[0-9]{1,2})(?=[ \n]+)`
fn number_length(text: &str) -> usize {
	let followed_by_space = |end: usize| matches!(text.as_bytes().get(end), Some(b' ' | b'\n'));
	let digits = span(text, |character| character.is_ascii_digit());
	let fraction = |start: usize| -> Option<usize> {
		if text.as_bytes().get(start) != Some(&b'.') {
			return None;
		}
		let count = span(&text[start + 1..], |character| character.is_ascii_digit());
		(1..=count.min(2)).rev().map(|count| start + 1 + count).find(|&end| followed_by_space(end))
	};
	if digits > 0 {
		if let Some(end) = fraction(digits) {
			return end;
		}
		return if followed_by_space(digits) { digits } else { 0 };
	}
	fraction(0).unwrap_or(0)
}

struct Lexer<'a> {
	input: &'a str,
	position: usize,
	line: usize,
	states: Vec<State>,
	done: bool,
}

impl<'a> Lexer<'a> {
	fn state(&self) -> State {
		self.states.last().copied().unwrap_or(State::Initial)
	}

	fn begin(&mut self, state: State) {
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
				State::Initial => self.initial(rest),
				State::Id => self.id(rest),
				State::Alias => self.alias(rest),
				State::Line => self.line_state(rest),
				State::Config => self.config(rest),
				State::AccTitle | State::AccDescr => {
					let kind = match self.state() {
						State::AccTitle => TokenKind::AccTitleValue,
						_ => TokenKind::AccDescrValue,
					};
					self.pop_state();
					Some(self.token(kind, span(rest, |character| character != '\n')))
				}
				State::AccDescrMultiline => {
					if rest.starts_with('}') {
						self.pop_state();
						self.take(1);
						None
					} else {
						let length = span(rest, |character| character != '}');
						Some(self.token(TokenKind::AccDescrMultilineValue, length))
					}
				}
			};
			match result {
				Some(token) => return token,
				None if self.done => {
					return Token { kind: TokenKind::Eof, text: String::new(), line: self.line };
				}
				None => {}
			}
		}
	}

	/// `None` for skipped text, an error for text no rule matches.
	fn initial(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		if rest.is_empty() {
			return Some(Token { kind: T::Newline, text: String::new(), line: self.line });
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
		let rest_of_line = span(rest, |character| character != '\n');
		if rest.starts_with('#') || (rest.starts_with("%%") && !rest[2..].starts_with('{')) {
			self.take(rest_of_line);
			return None;
		}
		let first_length = rest.chars().next().map_or(0, char::len_utf8);
		if !rest.starts_with('}') && rest[first_length..].starts_with("%%") {
			self.take(rest_of_line);
			return None;
		}
		let number = number_length(rest);
		if number > 0 {
			return Some(self.token(T::Num, number));
		}
		const LINE_KEYWORDS: &[(&str, TokenKind)] = &[
			("box", TokenKind::Box),
			("participant", TokenKind::Participant),
			("actor", TokenKind::ParticipantActor),
			("create", TokenKind::Create),
			("destroy", TokenKind::Destroy),
			("loop", TokenKind::Loop),
			("rect", TokenKind::Rect),
			("opt", TokenKind::Opt),
			("alt", TokenKind::Alt),
			("else", TokenKind::Else),
			("par", TokenKind::Par),
			("par_over", TokenKind::ParOver),
			("and", TokenKind::And),
			("critical", TokenKind::Critical),
			("option", TokenKind::Option),
			("break", TokenKind::Break),
			("end", TokenKind::End),
			("left of", TokenKind::LeftOf),
			("right of", TokenKind::RightOf),
		];
		for (word, kind) in LINE_KEYWORDS {
			if keyword(rest, word) {
				match kind {
					T::Participant | T::ParticipantActor | T::Destroy => self.begin(State::Id),
					T::Create | T::End | T::LeftOf | T::RightOf => {}
					_ => self.begin(State::Line),
				}
				return Some(self.token(kind.clone(), word.len()));
			}
		}
		for (word, kind) in [
			("links", T::Links),
			("link", T::Link),
			("properties", T::Properties),
			("details", T::Details),
		] {
			if starts_with_ignoring_case(rest, word) {
				// `(?=[ \t]+[^\/\\\+\(\)<>:\n,;\s\-])`
				let after = &rest[word.len()..];
				let blanks = span(after, |character| character == ' ' || character == '\t');
				let next = after[blanks..].chars().next();
				if blanks > 0
					&& next.is_some_and(|next| {
						!next.is_whitespace() && !"/\\+()<>:\n,;-".contains(next)
					}) {
					return Some(self.token(kind, word.len()));
				}
			}
		}
		for (word, kind) in [("over", T::Over), ("note", T::Note)] {
			if keyword(rest, word) {
				return Some(self.token(kind, word.len()));
			}
		}
		for (word, kind) in [("activate", T::Activate), ("deactivate", T::Deactivate)] {
			if keyword(rest, word) {
				self.begin(State::Id);
				return Some(self.token(kind, word.len()));
			}
		}
		for (word, kind) in [("title", T::Title), ("title:", T::LegacyTitle)] {
			if starts_with_ignoring_case(rest, word) {
				let after = &rest[word.len()..];
				if let Some(space) = after.chars().next().filter(|next| next.is_whitespace()) {
					let value = span(&after[space.len_utf8()..], |character| {
						!matches!(character, '#' | '\n' | ';')
					});
					if value > 0 {
						let length = word.len() + space.len_utf8() + value;
						return Some(self.token(kind, length));
					}
				}
			}
		}
		for (word, kind, state) in
			[("accTitle", T::AccTitle, State::AccTitle), ("accDescr", T::AccDescr, State::AccDescr)]
		{
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
				self.begin(State::AccDescrMultiline);
				self.take(length + trailing);
				return None;
			}
		}
		for (word, kind) in [
			("sequenceDiagram", T::SequenceDiagram),
			("autonumber", T::Autonumber),
			("off", T::Off),
		] {
			if keyword(rest, word) {
				return Some(self.token(kind, word.len()));
			}
		}
		if rest.starts_with(',') {
			return Some(self.token(T::Comma, 1));
		}
		if rest.starts_with(';') {
			return Some(self.token(T::Newline, 1));
		}
		let actor = actor_length(rest);
		if actor > 0 {
			let mut token = self.token(T::Actor, actor);
			token.text = token.text.trim().to_owned();
			return Some(token);
		}
		for (arrow, line_type) in ARROWS {
			if starts_with_ignoring_case(rest, arrow) {
				return Some(self.token(T::Arrow(*line_type), arrow.len()));
			}
		}
		if let Some(text) = rest.strip_prefix(':') {
			let length = 1 + span(text, |character| !matches!(character, '#' | '\n' | ';'));
			return Some(self.token(T::Text, length));
		}
		for (text, kind) in [("+", T::Plus), ("-", T::Minus), ("()", T::Central)] {
			if rest.starts_with(text) {
				return Some(self.token(kind, text.len()));
			}
		}
		Some(self.token(T::Invalid, first_length))
	}

	fn id(&mut self, rest: &str) -> Option<Token> {
		use TokenKind as T;
		let blanks = blank_length(rest);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		let rest_of_line = span(rest, |character| character != '\n');
		if rest.starts_with('#') {
			self.take(rest_of_line);
			return None;
		}
		if rest.starts_with("@{") {
			self.begin(State::Config);
			return Some(self.token(T::ConfigStart, 2));
		}
		let is_name = |character: char| !"<>:\n,;@".contains(character);
		let word = span(rest, |character| is_name(character) && !character.is_whitespace());
		if word > 0 {
			let after = &rest[word..];
			let spaces = span(after, char::is_whitespace);
			if after[spaces..].starts_with("@{") {
				return Some(self.token(T::Actor, word));
			}
			// `(?=\s+as\s)`
			let alias = &after[spaces..];
			if spaces > 0
				&& starts_with_ignoring_case(alias, "as")
				&& alias[2..].chars().next().is_some_and(char::is_whitespace)
			{
				self.begin(State::Alias);
				return Some(self.token(T::Actor, word));
			}
		}
		// `[^<>:\n,;@]+(?=\s*[\n;#]|$)`, which backtracks to before a `#` when the name ends
		// otherwise.
		let name = span(rest, is_name);
		if name > 0 {
			let after = &rest[name..];
			let length = match after.chars().next() {
				None | Some('\n' | ';') => Some(name),
				_ => rest[..name].rfind('#').filter(|&hash| hash > 0),
			};
			if let Some(length) = length {
				self.pop_state();
				let mut token = self.token(T::Actor, length);
				token.text = token.text.trim().to_owned();
				return Some(token);
			}
		}
		if rest[name..].starts_with('<') {
			self.pop_state();
			return Some(self.token(T::Invalid, rest_of_line));
		}
		// No rule matches a line end, which is a lexical error in jison.
		self.pop_state();
		let mut token = self.token(T::Invalid, rest_of_line);
		token.text = token.text.trim().to_owned();
		Some(token)
	}

	fn alias(&mut self, rest: &str) -> Option<Token> {
		let blanks = blank_length(rest);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		if rest.starts_with('#') {
			self.take(span(rest, |character| character != '\n'));
			return None;
		}
		self.pop_state();
		self.pop_state();
		if keyword(rest, "as") {
			self.begin(State::Line);
			return Some(self.token(TokenKind::As, 2));
		}
		Some(Token { kind: TokenKind::Newline, text: String::new(), line: self.line })
	}

	fn line_state(&mut self, rest: &str) -> Option<Token> {
		let blanks = blank_length(rest);
		if blanks > 0 {
			self.take(blanks);
			return None;
		}
		if rest.starts_with('#') {
			self.take(span(rest, |character| character != '\n'));
			return None;
		}
		self.pop_state();
		let length = span(rest, |character| !matches!(character, '#' | '\n' | ';'));
		Some(self.token(TokenKind::RestOfLine, length))
	}

	fn config(&mut self, rest: &str) -> Option<Token> {
		if let Some(after) = rest.strip_prefix('}') {
			self.pop_state();
			// `\}(?=\s+as\s)`
			let spaces = span(after, char::is_whitespace);
			let alias = &after[spaces..];
			let is_alias = spaces > 0
				&& starts_with_ignoring_case(alias, "as")
				&& alias[2..].chars().next().is_some_and(char::is_whitespace);
			match is_alias {
				true => self.begin(State::Alias),
				false => self.pop_state(),
			}
			return Some(self.token(TokenKind::ConfigEnd, 1));
		}
		let length = span(rest, |character| character != '}');
		(length > 0).then(|| self.token(TokenKind::ConfigContent, length))
	}
}

/// A parse error and its line in the diagram code.
#[derive(Debug)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

struct Parser<'a> {
	lexer: Lexer<'a>,
	current: Token,
	ops: Vec<Op>,
	common: &'a mut CommonFields,
	depth: usize,
}

type ParseResult<T> = Result<T, ParseError>;

/// `parseMessage`: the text without `wrap:`/`nowrap:`, which set its wrapping.
pub fn parse_message(text: &str) -> Text {
	let (text, wrap) = extract_wrap(text);
	Text { text, wrap }
}

impl<'a> Parser<'a> {
	fn advance(&mut self) -> ParseResult<Token> {
		let next = self.lexer.next_token();
		Ok(std::mem::replace(&mut self.current, next))
	}

	fn error<T>(&self, expected: &str) -> ParseResult<T> {
		let found = match &self.current.kind {
			TokenKind::Eof => "end of input".to_owned(),
			TokenKind::Newline => "end of line".to_owned(),
			_ => format!("{:?}", self.current.text),
		};
		Err(ParseError {
			line: self.current.line,
			message: format!("expected {expected}, found {found}"),
		})
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

	/// `actor: ACTOR | 'link'`
	fn actor(&mut self) -> ParseResult<String> {
		match self.current.kind {
			TokenKind::Actor | TokenKind::Link => Ok(self.advance()?.text),
			_ => self.error("a participant"),
		}
	}

	fn text(&mut self) -> ParseResult<Text> {
		let token = self.expect(TokenKind::Text, "`:` and a text")?;
		Ok(parse_message(token.text.trim().get(1..).unwrap_or("")))
	}

	fn rest_of_line(&mut self) -> ParseResult<String> {
		match self.current.kind {
			TokenKind::RestOfLine => Ok(self.advance()?.text),
			_ => self.error("the rest of the line"),
		}
	}

	fn newline(&mut self) -> ParseResult<()> {
		self.expect(TokenKind::Newline, "the end of the line").map(|_| ())
	}

	fn start(&mut self) -> ParseResult<()> {
		while self.is(&TokenKind::Newline) {
			self.advance()?;
		}
		self.expect(TokenKind::SequenceDiagram, "`sequenceDiagram`")?;
		self.document()?;
		match self.current.kind {
			TokenKind::Eof => Ok(()),
			_ => self.error("a statement"),
		}
	}

	/// Lines up to a token that ends the enclosing block.
	fn document(&mut self) -> ParseResult<()> {
		loop {
			match self.current.kind {
				TokenKind::End
				| TokenKind::Else
				| TokenKind::And
				| TokenKind::Option
				| TokenKind::Eof => return Ok(()),
				TokenKind::Newline | TokenKind::Invalid => {
					self.advance()?;
				}
				_ => self.statement()?,
			}
		}
	}

	/// The statements of a block up to its `end`, with the sections that `separator` starts.
	fn block(&mut self, separator: Option<(TokenKind, LineType)>) -> ParseResult<()> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			let message = "blocks are nested too deeply".to_owned();
			return Err(ParseError { line: self.current.line, message });
		}
		self.document()?;
		while let Some((_, section)) = separator.as_ref().filter(|(token, _)| self.is(token)) {
			let section = *section;
			self.advance()?;
			let text = parse_message(&self.rest_of_line()?);
			self.ops.push(Op::Block(section, text));
			self.document()?;
		}
		self.expect(TokenKind::End, "`end`")?;
		self.depth -= 1;
		Ok(())
	}

	fn statement(&mut self) -> ParseResult<()> {
		use TokenKind as T;
		match self.current.kind.clone() {
			T::Participant | T::ParticipantActor | T::Destroy => {
				let op = self.participant()?;
				self.ops.push(op);
			}
			T::Create => {
				self.advance()?;
				match self.participant()? {
					Op::AddParticipant { actor, description, draw, config } => {
						self.ops.push(Op::CreateParticipant { actor, description, draw, config });
					}
					Op::DestroyParticipant { actor } => {
						self.ops.push(Op::CreateParticipant {
							actor,
							description: None,
							draw: None,
							config: None,
						});
					}
					_ => {}
				}
			}
			T::Box => {
				self.advance()?;
				let text = self.rest_of_line()?;
				self.ops.push(Op::BoxStart(text));
				while !self.is(&T::End) {
					match self.current.kind {
						T::Newline => {
							self.advance()?;
						}
						T::Participant | T::ParticipantActor | T::Destroy => {
							let op = self.participant()?;
							self.ops.push(op);
						}
						_ => return self.error("a participant or `end`"),
					}
				}
				self.advance()?;
				self.ops.push(Op::BoxEnd);
			}
			// The lexer returns `link` only before a blank and a name, so it starts no message.
			T::Actor => {
				self.signal()?;
				self.newline()?;
			}
			T::Autonumber => {
				self.advance()?;
				let op = match self.current.kind {
					T::Num => {
						let start = self.advance()?.text.parse::<f64>().ok();
						let step = match self.is(&T::Num) {
							true => self.advance()?.text.parse::<f64>().ok(),
							false => Some(1.0),
						};
						Op::Autonumber { start, step, visible: true }
					}
					T::Off => {
						self.advance()?;
						Op::Autonumber { start: None, step: None, visible: false }
					}
					_ => Op::Autonumber { start: None, step: None, visible: true },
				};
				self.newline()?;
				self.ops.push(op);
			}
			T::Activate | T::Deactivate => {
				let is_start = self.is(&T::Activate);
				self.advance()?;
				let actor = self.actor()?;
				self.newline()?;
				self.ops.push(match is_start {
					true => Op::ActiveStart(actor),
					false => Op::ActiveEnd(actor),
				});
			}
			T::Note => {
				self.advance()?;
				let placement = match self.current.kind {
					T::LeftOf => Placement::LeftOf,
					T::RightOf => Placement::RightOf,
					T::Over => Placement::Over,
					_ => return self.error("`left of`, `right of` or `over`"),
				};
				self.advance()?;
				let first = self.actor()?;
				let second = match placement == Placement::Over && self.is(&T::Comma) {
					true => {
						self.advance()?;
						Some(self.actor()?)
					}
					false => None,
				};
				let text = self.text()?;
				self.newline()?;
				self.ops.push(Op::actor(&first));
				if let Some(second) = &second {
					self.ops.push(Op::actor(second));
				}
				let actors = match (placement, second) {
					(Placement::Over, Some(second)) => NoteActors::Pair(first, second),
					(Placement::Over, None) => NoteActors::Pair(first.clone(), first),
					(_, _) => NoteActors::One(first),
				};
				self.ops.push(Op::AddNote { actors, placement, text });
			}
			T::Links | T::Link | T::Properties | T::Details => {
				let kind = self.advance()?.kind;
				let actor = self.actor()?;
				let text = self.text()?;
				self.newline()?;
				self.ops.push(Op::actor(&actor));
				self.ops.push(match kind {
					T::Links => Op::AddLinks(actor, text.text),
					T::Link => Op::AddLink(actor, text.text),
					T::Properties => Op::AddProperties(actor, text.text),
					_ => Op::AddDetails,
				});
			}
			T::Title => {
				let token = self.advance()?;
				self.common.title = Some(token.text.get(6..).unwrap_or("").trim().to_owned());
			}
			T::LegacyTitle => {
				let token = self.advance()?;
				self.common.title = Some(token.text.get(7..).unwrap_or("").trim().to_owned());
			}
			T::AccTitle => {
				self.advance()?;
				let value = self.expect(T::AccTitleValue, "a title")?;
				self.common.accessibility.title = value.text.trim().to_owned();
			}
			T::AccDescr => {
				self.advance()?;
				let value = self.expect(T::AccDescrValue, "a description")?;
				self.common.accessibility.description = value.text.trim().to_owned();
			}
			T::AccDescrMultilineValue => {
				let value = self.advance()?;
				self.common.accessibility.description = value.text.trim().to_owned();
			}
			T::Loop | T::Rect | T::Opt | T::Break => {
				let kind = self.advance()?.kind;
				let text = parse_message(&self.rest_of_line()?);
				let (start, end) = match kind {
					T::Loop => (LineType::LoopStart, LineType::LoopEnd),
					T::Rect => (LineType::RectStart, LineType::RectEnd),
					T::Opt => (LineType::OptStart, LineType::OptEnd),
					_ => (LineType::BreakStart, LineType::BreakEnd),
				};
				self.ops.push(Op::Block(start, text));
				self.block(None)?;
				self.ops.push(Op::Block(end, Text::default()));
			}
			T::Alt | T::Par | T::ParOver | T::Critical => {
				let kind = self.advance()?.kind;
				let text = parse_message(&self.rest_of_line()?);
				let (start, separator, section, end) = match kind {
					T::Alt => (LineType::AltStart, T::Else, LineType::AltElse, LineType::AltEnd),
					T::Par => (LineType::ParStart, T::And, LineType::ParAnd, LineType::ParEnd),
					T::ParOver => {
						(LineType::ParOverStart, T::And, LineType::ParAnd, LineType::ParEnd)
					}
					_ => (
						LineType::CriticalStart,
						T::Option,
						LineType::CriticalOption,
						LineType::CriticalEnd,
					),
				};
				self.ops.push(Op::Block(start, text));
				self.block(Some((separator, section)))?;
				self.ops.push(Op::Block(end, Text::default()));
			}
			_ => return self.error("a statement"),
		}
		Ok(())
	}

	fn participant(&mut self) -> ParseResult<Op> {
		use TokenKind as T;
		let kind = self.advance()?.kind;
		let is_config_actor = self.is(&T::Actor);
		let actor = self.actor()?;
		if kind == T::Destroy {
			self.newline()?;
			return Ok(Op::DestroyParticipant { actor });
		}
		let mut config = None;
		if is_config_actor && self.is(&T::ConfigStart) {
			self.advance()?;
			let content = self.expect(T::ConfigContent, "participant data")?;
			self.expect(T::ConfigEnd, "`}`")?;
			config = Some(content.text.trim().to_owned());
		}
		let mut description = None;
		if self.is(&T::As) {
			self.advance()?;
			description = Some(parse_message(&self.rest_of_line()?));
		}
		self.newline()?;
		let draw = Some(if kind == T::ParticipantActor { "actor" } else { "participant" });
		Ok(Op::AddParticipant { actor, description, draw, config })
	}

	fn signal(&mut self) -> ParseResult<()> {
		use TokenKind as T;
		let from = self.actor()?;
		let starts_central = self.is(&T::Central);
		if starts_central {
			self.advance()?;
		}
		let T::Arrow(line_type) = self.current.kind else { return self.error("an arrow") };
		self.advance()?;
		let modifier = match self.current.kind {
			T::Plus | T::Minus | T::Central => Some(self.advance()?.kind),
			_ => None,
		};
		if starts_central && matches!(modifier, Some(T::Plus | T::Minus)) {
			return self.error("a participant");
		}
		let to = self.actor()?;
		let text = self.text()?;
		self.ops.push(Op::actor(&from));
		self.ops.push(Op::actor(&to));
		let (activate, central) = match (starts_central, &modifier) {
			(true, Some(T::Central)) => (true, CentralConnection::Dual),
			(true, _) => (false, CentralConnection::Reverse),
			(false, Some(T::Central)) => (true, CentralConnection::Forward),
			(false, Some(T::Plus)) => (true, CentralConnection::None),
			_ => (false, CentralConnection::None),
		};
		self.ops.push(Op::AddMessage {
			from: from.clone(),
			to: to.clone(),
			line_type,
			text,
			activate,
			central,
		});
		match (starts_central, modifier) {
			(false, Some(T::Plus)) => self.ops.push(Op::ActiveStart(to)),
			(false, Some(T::Minus)) => self.ops.push(Op::ActiveEnd(from)),
			(false, Some(T::Central)) => self.ops.push(Op::CentralConnection(to)),
			(true, Some(T::Central)) => {
				self.ops.push(Op::CentralConnection(to));
				self.ops.push(Op::CentralConnectionReverse(from));
			}
			(true, _) => self.ops.push(Op::CentralConnectionReverse(from)),
			_ => {}
		}
		Ok(())
	}
}

/// Parses the diagram `code` (with a final line end) into the statements of the database; the
/// title and accessibility statements go to `common`.
pub fn parse(code: &str, common: &mut CommonFields) -> ParseResult<Vec<Op>> {
	let lexer =
		Lexer { input: code, position: 0, line: 1, states: vec![State::Initial], done: false };
	let placeholder = Token { kind: TokenKind::Eof, text: String::new(), line: 1 };
	let mut parser = Parser { lexer, current: placeholder, ops: Vec::new(), common, depth: 0 };
	parser.advance()?;
	parser.start()?;
	Ok(parser.ops)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn tokens(code: &str) -> Vec<(TokenKind, String)> {
		let mut lexer =
			Lexer { input: code, position: 0, line: 1, states: vec![State::Initial], done: false };
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
		let actor = |name: &str| (T::Actor, name.to_owned());
		assert_eq!(
			tokens("Alice-Bob->>+John: Hi\n"),
			[
				actor("Alice-Bob"),
				(T::Arrow(LineType::Solid), "->>".into()),
				(T::Plus, "+".into()),
				actor("John"),
				(T::Text, ": Hi".into()),
				(T::Newline, "\n".into()),
				(T::Newline, String::new()),
			]
		);
		assert_eq!(
			tokens("B//-A:x")[..2],
			[actor("B"), (T::Arrow(LineType::StickArrowTopReverse), "//-".into())]
		);
		assert_eq!(tokens("A()->>B:x")[..2], [actor("A"), (T::Central, "()".into())]);
		assert_eq!(tokens("A-XB:x")[1].0, T::Arrow(LineType::SolidCross));
		assert_eq!(
			tokens("participant A as Alice\n")[..4],
			[
				(T::Participant, "participant".into()),
				actor("A"),
				(T::As, "as".into()),
				(T::RestOfLine, "Alice".into()),
			]
		);
		assert_eq!(
			tokens("participant A@{ \"type\": \"queue\" }\n")[..5],
			[
				(T::Participant, "participant".into()),
				actor("A"),
				(T::ConfigStart, "@{".into()),
				(T::ConfigContent, " \"type\": \"queue\" ".into()),
				(T::ConfigEnd, "}".into()),
			]
		);
		assert_eq!(tokens("boxer->>B:x")[0], actor("boxer"));
		assert_eq!(tokens("autonumber 10 5\n")[1], (T::Num, "10".into()));
		assert_eq!(
			tokens("Loop Every minute\n")[..2],
			[(T::Loop, "Loop".into()), (T::RestOfLine, "Every minute".into())]
		);
		assert_eq!(tokens("title  My title\n")[0], (T::Title, "title  My title".into()));
	}

	#[test]
	fn parses_statements() {
		let mut common = CommonFields::default();
		let code = "sequenceDiagram\nparticipant A as Alice\nloop Every minute\nA->>B: wrap:Hi\nend\nalt x\nA-->>B: y\nelse z\nend\nNote over A,B: n\n";
		let ops = parse(code, &mut common).unwrap();
		assert!(
			matches!(&ops[0], Op::AddParticipant { actor, description: Some(Text { text, .. }), .. } if actor == "A" && text == "Alice")
		);
		assert!(
			matches!(&ops[1], Op::Block(LineType::LoopStart, Text { text, .. }) if text == "Every minute")
		);
		assert!(
			matches!(&ops[4], Op::AddMessage { text: Text { text, wrap: Some(true) }, .. } if text == "Hi")
		);
		assert!(matches!(&ops[5], Op::Block(LineType::LoopEnd, _)));
		assert!(matches!(&ops[10], Op::Block(LineType::AltElse, Text { text, .. }) if text == "z"));
		assert!(
			matches!(ops.last(), Some(Op::AddNote { actors: NoteActors::Pair(a, b), .. }) if a == "A" && b == "B")
		);
		let error = parse("sequenceDiagram\nloop x\nA->>B: y\n", &mut common).unwrap_err();
		assert!(error.message.contains("`end`"), "{}", error.message);
		assert!(parse("sequenceDiagram\nA->>B\n", &mut common).is_err());
	}

	#[test]
	fn accepts_any_input() {
		let mut common = CommonFields::default();
		for code in [
			"",
			"sequenceDiagram",
			"sequenceDiagram\nparticipant",
			"sequenceDiagram\n@{",
			"sequenceDiagram\naccDescr {",
			"x\u{0}\u{ffff}",
		] {
			let _ = parse(code, &mut common);
		}
		let nested = format!("sequenceDiagram\n{}", "loop\n".repeat(10_000));
		assert!(parse(&nested, &mut common).is_err());
	}
}
