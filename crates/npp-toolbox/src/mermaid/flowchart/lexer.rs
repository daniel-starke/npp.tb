//! The lexer of `flow.jison`: jison's first matching rule wins, and exclusive states select the
//! rules, so tokens depend on what came before (`-->` opens no text, `--` does).
//!
//! The agentflow dialect follows `agentflow.jison`, a copy of it with other keywords, only the
//! links `-->`, `--x`, `-- text -->` and `-.-`, and `%%` comments inside labels.

/// The grammar whose lexer rules apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
	#[default]
	Flowchart,
	Agentflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	ShapeData,
	CallbackName,
	CallbackArgs,
	MdStr,
	Str,
	Style,
	Default,
	LinkStyle,
	Interpolate,
	ClassDef,
	Class,
	Href,
	Click,
	Graph,
	Subgraph,
	Flow,
	Connector,
	Global,
	End,
	LinkTarget,
	NoDir,
	Dir,
	Direction,
	LinkId,
	Num,
	Brkt,
	StyleSeparator,
	Colon,
	Amp,
	Semi,
	Comma,
	Mult,
	Link,
	StartLink,
	EdgeText,
	EllipseEnd,
	Text,
	EllipseStart,
	StadiumEnd,
	StadiumStart,
	SubroutineEnd,
	SubroutineStart,
	VertexWithPropsStart,
	TagEnd,
	CylinderEnd,
	CylinderStart,
	DoubleCircleEnd,
	DoubleCircleStart,
	TrapEnd,
	InvTrapEnd,
	TrapStart,
	InvTrapStart,
	TagStart,
	Up,
	Sep,
	Down,
	Minus,
	NodeString,
	UnicodeText,
	Pipe,
	Pe,
	Ps,
	Sqe,
	Sqs,
	DiamondStop,
	DiamondStart,
	Newline,
	Space,
	Comment,
	Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
	pub kind: TokenKind,
	pub text: String,
	/// One-based line where the token starts.
	pub line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
	Initial,
	String,
	MdString,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
	Dir,
	Text,
	EllipseText,
	TrapText,
	EdgeText,
	ThickEdgeText,
	DottedEdgeText,
	Click,
	CallbackName,
	CallbackArgs,
	ShapeData,
	ShapeDataString,
}

pub struct Lexer<'a> {
	input: &'a str,
	position: usize,
	line: usize,
	states: Vec<State>,
	first_graph: bool,
	dialect: Dialect,
}

/// Bounds the lexer state stack: unbalanced brackets push states.
const MAX_STATES: usize = 256;

fn is_space(character: char) -> bool {
	character.is_whitespace()
}

/// `[^\S\n\r]`: whitespace but no line end.
fn is_blank(character: char) -> bool {
	character.is_whitespace() && character != '\n' && character != '\r'
}

/// Length of the leading characters of `text` that satisfy `predicate`.
fn span(text: &str, predicate: impl Fn(char) -> bool) -> usize {
	text.find(|character| !predicate(character)).unwrap_or(text.len())
}

/// The characters of `NODE_STRING`'s class.
fn is_node_string_character(character: char) -> bool {
	character.is_ascii_alphanumeric() || "!\"#$%&'*+.`?\\_/".contains(character)
}

/// `([A-Za-z0-9!"#$%&'*+.`?\\_/]|-(?=[^>\-.])|=(?!=))+`.
fn node_string_length(text: &str) -> usize {
	let bytes = text.as_bytes();
	let mut index = 0;
	while let Some(character) = text[index..].chars().next() {
		let next = bytes.get(index + 1).copied();
		let accepted = match character {
			'-' => next.is_some_and(|next| !matches!(next, b'>' | b'-' | b'.')),
			'=' => next != Some(b'='),
			other => is_node_string_character(other),
		};
		if !accepted {
			break;
		}
		index += character.len_utf8();
	}
	index
}

/// The letters of Mermaid's `UNICODE_TEXT` (letters of the Basic Multilingual Plane).
fn is_unicode_text(character: char) -> bool {
	!character.is_ascii() && character.is_alphabetic() && (character as u32) <= 0xffff
}

/// `\s*[xo<]?` followed by `body`, then `\s*`: the length of a link token or `None`.
fn link_length(text: &str, body: impl Fn(&str) -> Option<usize>) -> Option<usize> {
	let index = span(text, is_space);
	let rest = &text[index..];
	// `[xo<]?` is optional: try with it first, like the regular expression backtracks.
	let with_head =
		rest.starts_with(['x', 'o', '<']).then(|| body(&rest[1..]).map(|length| length + 1));
	let length = with_head.flatten().or_else(|| body(rest))?;
	Some(index + length + span(&text[index + length..], is_space))
}

/// `\s*` followed by `body`, then `\s*`: the length of an agentflow link token or `None`.
fn link_length_without_head(text: &str, body: impl Fn(&str) -> Option<usize>) -> Option<usize> {
	let index = span(text, is_space);
	let length = body(&text[index..])?;
	Some(index + length + span(&text[index + length..], is_space))
}

/// `"%%"(?!\{)[^\n]*`: a comment up to the line end.
fn comment_length(text: &str) -> Option<usize> {
	let comment = text.strip_prefix("%%").filter(|after| !after.starts_with('{'))?;
	Some(2 + comment.find('\n').unwrap_or(comment.len()))
}

/// `(?:[^…%]+|%(?!%))+` of agentflow's label text: no character of `excluded` and no `%%`.
fn text_length(text: &str, excluded: &str) -> usize {
	let mut length = 0;
	for (index, character) in text.char_indices() {
		let is_comment = character == '%' && text[index + 1..].starts_with('%');
		if excluded.contains(character) || is_comment {
			break;
		}
		length = index + character.len_utf8();
	}
	length
}

/// Whether `text` starts with `keyword` and no word character follows: jison appends `\b` to
/// rules that end with a word character.
fn starts_with_keyword(text: &str, keyword: &str) -> bool {
	text.strip_prefix(keyword).is_some_and(|after| !after.starts_with(is_word_character))
}

/// A character of a regular expression's `\w`, which `\b` looks at.
fn is_word_character(character: char) -> bool {
	character.is_ascii_alphanumeric() || character == '_'
}

/// `\s*\~\~[\~]+\s*`.
fn tilde_link_length(text: &str) -> Option<usize> {
	let mut index = span(text, is_space);
	let tildes = span(&text[index..], |c| c == '~');
	if tildes < 3 {
		return None;
	}
	index += tildes;
	Some(index + span(&text[index..], is_space))
}

/// `\-\-+[-xo>]` (or the same with `=`): two or more line characters and a final character. The
/// greedy run backtracks by one to let its last character be the final one.
fn arrow_body(text: &str, line: u8, ends: &[u8]) -> Option<usize> {
	let bytes = text.as_bytes();
	let run = bytes.iter().take_while(|&&byte| byte == line).count();
	if run < 2 {
		return None;
	}
	match bytes.get(run) {
		Some(next) if ends.contains(next) => Some(run + 1),
		_ if run >= 3 => Some(run),
		_ => None,
	}
}

/// `\-?\.+\-[xo>]?`.
fn dotted_body(text: &str) -> Option<usize> {
	let bytes = text.as_bytes();
	let mut index = usize::from(bytes.first() == Some(&b'-'));
	let dots = bytes[index..].iter().take_while(|&&byte| byte == b'.').count();
	if dots == 0 {
		// `\-?` backtracks: without the dash nothing matches either.
		return None;
	}
	index += dots;
	if bytes.get(index) != Some(&b'-') {
		return None;
	}
	index += 1;
	if matches!(bytes.get(index), Some(b'x' | b'o' | b'>')) {
		index += 1;
	}
	Some(index)
}

impl<'a> Lexer<'a> {
	#[cfg(test)]
	pub fn new(input: &'a str) -> Self {
		Self::with_dialect(input, Dialect::Flowchart)
	}

	pub fn with_dialect(input: &'a str, dialect: Dialect) -> Self {
		Self { input, position: 0, line: 1, states: Vec::new(), first_graph: true, dialect }
	}

	fn is_agentflow(&self) -> bool {
		self.dialect == Dialect::Agentflow
	}

	fn state(&self) -> State {
		self.states.last().copied().unwrap_or(State::Initial)
	}

	fn push(&mut self, state: State) {
		if self.states.len() < MAX_STATES {
			self.states.push(state);
		}
	}

	fn pop(&mut self) {
		self.states.pop();
	}

	/// The next token, or `Err` with the line of text no rule matches.
	pub fn next_token(&mut self) -> Result<Token, usize> {
		// Rules that match nothing change the state; more of them in a row would loop.
		let mut empty_matches = 0;
		loop {
			let rest = &self.input[self.position..];
			let line = self.line;
			if rest.is_empty() {
				return Ok(Token { kind: TokenKind::Eof, text: String::new(), line });
			}
			let Some((length, kind)) = self.match_rule(rest) else { return Err(line) };
			let length = length.min(rest.len());
			if length == 0 {
				empty_matches += 1;
				if empty_matches > 4 {
					return Err(line);
				}
			}
			let Some(text) = rest.get(..length) else { return Err(line) };
			self.position += length;
			self.line += text.matches('\n').count();
			if let Some(kind) = kind {
				let text = match kind {
					TokenKind::ShapeData if text == "@{" => String::new(),
					TokenKind::ShapeData if self.state() == State::ShapeDataString => {
						replace_line_breaks(text)
					}
					_ => text.to_owned(),
				};
				return Ok(Token { kind, text, line });
			}
		}
	}

	/// The first rule that matches `rest` in the current state: its length and token (`None` for
	/// rules that only change the state). Applies the rule's state change.
	fn match_rule(&mut self, rest: &str) -> Option<(usize, Option<TokenKind>)> {
		use TokenKind as T;
		let state = self.state();
		let starts = |prefix: &str| rest.starts_with(prefix);
		match state {
			State::AccTitle | State::AccDescr => {
				self.pop();
				let kind =
					if state == State::AccTitle { T::AccTitleValue } else { T::AccDescrValue };
				return Some((rest.find('\n').unwrap_or(rest.len()), Some(kind)));
			}
			State::AccDescrMultiline => {
				if starts("}") {
					self.pop();
					return Some((1, None));
				}
				return Some((
					rest.find('}').unwrap_or(rest.len()),
					Some(T::AccDescrMultilineValue),
				));
			}
			State::ShapeData => {
				if starts("\"") {
					self.push(State::ShapeDataString);
					return Some((1, Some(T::ShapeData)));
				}
				let length = span(rest, |c| c != '}' && c != '^' && c != '"');
				if length > 0 {
					return Some((length, Some(T::ShapeData)));
				}
				if starts("}") {
					self.pop();
					return Some((1, None));
				}
				return None;
			}
			State::ShapeDataString => {
				if starts("\"") {
					self.pop();
					return Some((1, Some(T::ShapeData)));
				}
				return Some((span(rest, |c| c != '"'), Some(T::ShapeData)));
			}
			State::CallbackName => {
				if starts("(") {
					let after = &rest[1..];
					let blanks = span(after, is_space);
					if after[blanks..].starts_with(')') {
						self.pop();
						return Some((1 + blanks + 1, None));
					}
					self.pop();
					self.push(State::CallbackArgs);
					return Some((1, None));
				}
				return Some((span(rest, |c| c != '('), Some(T::CallbackName)));
			}
			State::CallbackArgs => {
				if starts(")") {
					self.pop();
					return Some((1, None));
				}
				return Some((span(rest, |c| c != ')'), Some(T::CallbackArgs)));
			}
			State::MdString => {
				let length = span(rest, |c| c != '`' && c != '"');
				if length > 0 {
					return Some((length, Some(T::MdStr)));
				}
				if starts("`\"") {
					self.pop();
					return Some((2, None));
				}
			}
			State::String => {
				if starts("\"`") {
					self.push(State::MdString);
					return Some((2, None));
				}
				let length = span(rest, |c| c != '"');
				if length > 0 {
					return Some((length, Some(T::Str)));
				}
				if starts("\"") {
					self.pop();
					return Some((1, None));
				}
			}
			_ => {}
		}
		// `<*>["][`]` and `<*>["]`.
		if starts("\"`") {
			self.push(State::MdString);
			return Some((2, None));
		}
		if starts("\"") {
			self.push(State::String);
			return Some((1, None));
		}
		if state == State::Initial
			&& let Some(result) = self.match_initial_keywords(rest)
		{
			return Some(result);
		}
		match state {
			State::Click => {
				if rest.starts_with(is_space) {
					self.pop();
					let length = rest.chars().next().map_or(1, char::len_utf8);
					return Some((length, None));
				}
				return Some((span(rest, |c| !is_space(c)), Some(T::Click)));
			}
			State::Dir => {
				// `(\r?\n)*\s*\n`: up to the last line end of the leading whitespace.
				let blanks = span(rest, is_space);
				if let Some(end) = rest[..blanks].rfind('\n') {
					self.pop();
					return Some((end + 1, Some(T::NoDir)));
				}
				let after = &rest[blanks..];
				for direction in ["LR", "RL", "TB", "BT", "TD", "BR", "<", ">", "^", "v"] {
					if starts_with_keyword(after, direction) {
						self.pop();
						return Some((blanks + direction.len(), Some(T::Dir)));
					}
				}
			}
			_ => {}
		}
		if state == State::Initial
			&& let Some(result) = self.match_initial_symbols(rest)
		{
			return Some(result);
		}
		if self.is_agentflow() {
			if let Some(result) = self.match_agentflow_link(rest, state) {
				return Some(result);
			}
		} else if let Some(result) = self.match_link(rest, state) {
			return Some(result);
		}
		if self.is_agentflow()
			&& matches!(state, State::EllipseText | State::TrapText | State::Text)
			&& let Some(length) = comment_length(rest)
		{
			// `%%` comments in labels: no shape end and no text starts with `%%`.
			return Some((length, None));
		}
		if state == State::EllipseText {
			let bytes = rest.as_bytes();
			if matches!(bytes.first(), Some(b'-' | b'/' | b')')) && bytes.get(1) == Some(&b')') {
				self.pop();
				return Some((2, Some(T::EllipseEnd)));
			}
			let first = rest.chars().next()?;
			if !"()[]{}".contains(first) {
				return Some((first.len_utf8(), Some(T::Text)));
			}
		}
		if starts("(-") {
			self.push(State::EllipseText);
			return Some((2, Some(T::EllipseStart)));
		}
		if state == State::Text && starts("])") {
			self.pop();
			return Some((2, Some(T::StadiumEnd)));
		}
		if starts("([") {
			self.push(State::Text);
			return Some((2, Some(T::StadiumStart)));
		}
		if state == State::Text && starts("]]") {
			self.pop();
			return Some((2, Some(T::SubroutineEnd)));
		}
		if starts("[[") {
			self.push(State::Text);
			return Some((2, Some(T::SubroutineStart)));
		}
		if state == State::Initial && starts("[|") {
			return Some((2, Some(T::VertexWithPropsStart)));
		}
		if state == State::Initial && starts(">") {
			self.push(State::Text);
			return Some((1, Some(T::TagEnd)));
		}
		if state == State::Text && starts(")]") {
			self.pop();
			return Some((2, Some(T::CylinderEnd)));
		}
		if starts("[(") {
			self.push(State::Text);
			return Some((2, Some(T::CylinderStart)));
		}
		if state == State::Text && starts(")))") {
			self.pop();
			return Some((3, Some(T::DoubleCircleEnd)));
		}
		if starts("(((") {
			self.push(State::Text);
			return Some((3, Some(T::DoubleCircleStart)));
		}
		if state == State::TrapText {
			let bytes = rest.as_bytes();
			if matches!(bytes.first(), Some(b'\\' | b'(' | b'?' | b'=' | b']' | b')'))
				&& bytes.get(1) == Some(&b']')
			{
				self.pop();
				return Some((2, Some(T::TrapEnd)));
			}
			if starts("/]") {
				self.pop();
				return Some((2, Some(T::InvTrapEnd)));
			}
			if (starts("/") || starts("\\")) && !rest[1..].starts_with(']') {
				return Some((1, Some(T::Text)));
			}
			let length = match self.is_agentflow() {
				true => text_length(rest, "\\[](){}/"),
				false => span(rest, |c| !"\\[](){}/".contains(c)),
			};
			if length > 0 {
				return Some((length, Some(T::Text)));
			}
		}
		if starts("[/") {
			self.push(State::TrapText);
			return Some((2, Some(T::TrapStart)));
		}
		if starts("[\\") {
			self.push(State::TrapText);
			return Some((2, Some(T::InvTrapStart)));
		}
		if state == State::Initial {
			let token = match rest.as_bytes().first() {
				Some(b'<') => Some(T::TagStart),
				Some(b'^') => Some(T::Up),
				_ if starts("\\|") => return Some((2, Some(T::Sep))),
				_ if starts_with_keyword(rest, "v") => Some(T::Down),
				_ => None,
			};
			if let Some(token) = token {
				return Some((1, Some(token)));
			}
			let blanks = span(rest, |c| c == ' ' || c == '\t');
			if self.is_agentflow()
				&& let Some(length) = comment_length(&rest[blanks..])
			{
				return Some((blanks + length, Some(T::Comment)));
			}
			let length = node_string_length(rest);
			if length > 0 {
				return Some((length, Some(T::NodeString)));
			}
			if starts("-") {
				return Some((1, Some(T::Minus)));
			}
			if let Some(first) = rest.chars().next().filter(|&c| is_unicode_text(c)) {
				return Some((first.len_utf8(), Some(T::UnicodeText)));
			}
		}
		if state == State::Text && starts("|") {
			self.pop();
			return Some((1, Some(T::Pipe)));
		}
		if starts("|") {
			self.push(State::Text);
			return Some((1, Some(T::Pipe)));
		}
		if state == State::Text && starts(")") {
			self.pop();
			return Some((1, Some(T::Pe)));
		}
		if starts("(") {
			self.push(State::Text);
			return Some((1, Some(T::Ps)));
		}
		if state == State::Text && starts("]") {
			self.pop();
			return Some((1, Some(T::Sqe)));
		}
		if starts("[") {
			self.push(State::Text);
			return Some((1, Some(T::Sqs)));
		}
		if state == State::Text && starts("}") {
			self.pop();
			return Some((1, Some(T::DiamondStop)));
		}
		if starts("{") {
			self.push(State::Text);
			return Some((1, Some(T::DiamondStart)));
		}
		if state == State::Text {
			let length = match self.is_agentflow() {
				true => text_length(rest, "[](){}|\""),
				false => span(rest, |c| !"[](){}|\"".contains(c)),
			};
			if length > 0 {
				return Some((length, Some(T::Text)));
			}
		}
		if state == State::Initial {
			let newlines = span(rest, |c| c == '\n' || c == '\r');
			if newlines > 0 && rest[..newlines].contains('\n') {
				// `(\r?\n)+`
				let mut length = 0;
				let bytes = rest.as_bytes();
				loop {
					match (bytes.get(length), bytes.get(length + 1)) {
						(Some(b'\n'), _) => length += 1,
						(Some(b'\r'), Some(b'\n')) => length += 2,
						_ => break,
					}
				}
				if length > 0 {
					return Some((length, Some(T::Newline)));
				}
			}
			let blanks = span(rest, is_blank);
			if blanks > 0 {
				return Some((blanks, Some(T::Space)));
			}
		}
		None
	}

	/// The link rules of `flow.jison`.
	fn match_link(&mut self, rest: &str, state: State) -> Option<(usize, Option<TokenKind>)> {
		use TokenKind as T;
		// Links that end the edge text states.
		if matches!(state, State::Initial | State::EdgeText)
			&& let Some(length) = link_length(rest, |text| arrow_body(text, b'-', b"-xo>"))
		{
			self.pop_if(state != State::Initial);
			return Some((length, Some(T::Link)));
		}
		if state == State::Initial
			&& let Some(length) = link_length(rest, |text| text.starts_with("--").then_some(2))
		{
			self.push(State::EdgeText);
			return Some((length, Some(T::StartLink)));
		}
		if state == State::EdgeText {
			// `[^-]|\-(?!\-)+`
			let mut characters = rest.chars();
			let first = characters.next()?;
			if first != '-' || !rest[1..].starts_with('-') {
				return Some((first.len_utf8(), Some(T::EdgeText)));
			}
		}
		if matches!(state, State::Initial | State::ThickEdgeText)
			&& let Some(length) = link_length(rest, |text| arrow_body(text, b'=', b"=xo>"))
		{
			self.pop_if(state != State::Initial);
			return Some((length, Some(T::Link)));
		}
		if state == State::Initial
			&& let Some(length) = link_length(rest, |text| text.starts_with("==").then_some(2))
		{
			self.push(State::ThickEdgeText);
			return Some((length, Some(T::StartLink)));
		}
		if state == State::ThickEdgeText {
			let first = rest.chars().next()?;
			if first != '=' || !rest[1..].starts_with('=') {
				return Some((first.len_utf8(), Some(T::EdgeText)));
			}
		}
		if matches!(state, State::Initial | State::DottedEdgeText)
			&& let Some(length) = link_length(rest, dotted_body)
		{
			self.pop_if(state != State::Initial);
			return Some((length, Some(T::Link)));
		}
		if state == State::Initial
			&& let Some(length) = link_length(rest, |text| text.starts_with("-.").then_some(2))
		{
			self.push(State::DottedEdgeText);
			return Some((length, Some(T::StartLink)));
		}
		if state == State::DottedEdgeText {
			let first = rest.chars().next()?;
			if first != '.' || !rest[1..].starts_with('-') {
				return Some((first.len_utf8(), Some(T::EdgeText)));
			}
		}
		if let Some(length) = tilde_link_length(rest) {
			return Some((length, Some(T::Link)));
		}
		None
	}

	/// The link rules of `agentflow.jison`: `\s*\-\-+[x>]\s*` ends edge text, `\s*\-\-\s*`
	/// starts it, and `\s*\-\.+\-\s*` takes no text.
	fn match_agentflow_link(
		&mut self,
		rest: &str,
		state: State,
	) -> Option<(usize, Option<TokenKind>)> {
		use TokenKind as T;
		if matches!(state, State::Initial | State::EdgeText)
			&& let Some(length) = link_length_without_head(rest, |text| {
				let run = text.bytes().take_while(|&byte| byte == b'-').count();
				(run >= 2 && matches!(text.as_bytes().get(run), Some(b'x' | b'>')))
					.then_some(run + 1)
			}) {
			self.pop_if(state != State::Initial);
			return Some((length, Some(T::Link)));
		}
		if state == State::Initial
			&& let Some(length) =
				link_length_without_head(rest, |text| text.starts_with("--").then_some(2))
		{
			self.push(State::EdgeText);
			return Some((length, Some(T::StartLink)));
		}
		if state == State::EdgeText {
			let first = rest.chars().next()?;
			if first != '-' || !rest[1..].starts_with('-') {
				return Some((first.len_utf8(), Some(T::EdgeText)));
			}
		}
		if state == State::Initial
			&& let Some(length) = link_length_without_head(rest, |text| {
				let dots = text.bytes().skip(1).take_while(|&byte| byte == b'.').count();
				let is_dotted = text.starts_with('-') && dots > 0;
				(is_dotted && text.as_bytes().get(1 + dots) == Some(&b'-')).then_some(dots + 2)
			}) {
			return Some((length, Some(T::Link)));
		}
		None
	}

	fn pop_if(&mut self, condition: bool) {
		if condition {
			self.pop();
		}
	}

	/// The keyword rules of the initial state, which come before the symbols.
	fn match_initial_keywords(&mut self, rest: &str) -> Option<(usize, Option<TokenKind>)> {
		use TokenKind as T;
		let starts = |prefix: &str| rest.starts_with(prefix);
		for (keyword, title) in [("accTitle", true), ("accDescr", false)] {
			if !starts(keyword) {
				continue;
			}
			let after = &rest[keyword.len()..];
			let blanks = span(after, is_space);
			let after_blanks = &after[blanks..];
			if let Some(value) = after_blanks.strip_prefix(':') {
				let trailing = span(value, is_space);
				self.push(if title { State::AccTitle } else { State::AccDescr });
				let kind = if title { T::AccTitle } else { T::AccDescr };
				return Some((keyword.len() + blanks + 1 + trailing, Some(kind)));
			}
			if !title && after_blanks.starts_with('{') {
				let trailing = span(&after_blanks[1..], is_space);
				self.push(State::AccDescrMultiline);
				return Some((keyword.len() + blanks + 1 + trailing, None));
			}
		}
		if starts("@{") {
			self.push(State::ShapeData);
			return Some((2, Some(T::ShapeData)));
		}
		if starts("call") {
			let blanks = span(&rest[4..], is_space);
			if blanks > 0 {
				self.push(State::CallbackName);
				return Some((4 + blanks, None));
			}
		}
		for (keyword, kind) in [
			("style", T::Style),
			("default", T::Default),
			("linkStyle", T::LinkStyle),
			("interpolate", T::Interpolate),
			("classDef", T::ClassDef),
			("class", T::Class),
		] {
			if starts_with_keyword(rest, keyword) {
				return Some((keyword.len(), Some(kind)));
			}
		}
		if starts("href") && rest[4..].starts_with(is_space) {
			let length = 4 + rest[4..].chars().next().map_or(0, char::len_utf8);
			return Some((length, Some(T::Href)));
		}
		if starts("click") {
			let blanks = span(&rest[5..], is_space);
			if blanks > 0 {
				self.push(State::Click);
				return Some((5 + blanks, None));
			}
		}
		if self.is_agentflow() {
			return self.match_agentflow_keywords(rest);
		}
		for keyword in ["flowchart-elk", "swimlane-beta", "graph", "flowchart"] {
			if starts_with_keyword(rest, keyword) {
				if self.first_graph {
					self.first_graph = false;
					self.push(State::Dir);
				}
				return Some((keyword.len(), Some(T::Graph)));
			}
		}
		if starts_with_keyword(rest, "subgraph") {
			return Some((8, Some(T::Subgraph)));
		}
		// `"end"\b\s*`
		if starts("end")
			&& rest[3..].chars().next().is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
		{
			return Some((3 + span(&rest[3..], is_space), Some(T::End)));
		}
		for target in ["_self", "_blank", "_parent", "_top"] {
			if starts_with_keyword(rest, target) {
				return Some((target.len(), Some(T::LinkTarget)));
			}
		}
		None
	}

	/// The diagram and container keywords of `agentflow.jison` and the link targets.
	fn match_agentflow_keywords(&mut self, rest: &str) -> Option<(usize, Option<TokenKind>)> {
		use TokenKind as T;
		if starts_with_keyword(rest, "agentflow-beta") {
			if self.first_graph {
				self.first_graph = false;
				self.push(State::Dir);
			}
			return Some((14, Some(T::Graph)));
		}
		if starts_with_keyword(rest, "flow") {
			return Some((4, Some(T::Flow)));
		}
		if starts_with_keyword(rest, "connector") {
			return Some((9, Some(T::Connector)));
		}
		// `"global"\b[ \t]*` and `"end"\b[ \t]*`.
		for (keyword, kind) in [("global", T::Global), ("end", T::End)] {
			let after = rest.strip_prefix(keyword);
			if let Some(after) = after.filter(|after| !after.starts_with(is_word_character)) {
				let blanks = span(after, |c| c == ' ' || c == '\t');
				return Some((keyword.len() + blanks, Some(kind)));
			}
		}
		for target in ["_self", "_blank", "_parent", "_top"] {
			if starts_with_keyword(rest, target) {
				return Some((target.len(), Some(T::LinkTarget)));
			}
		}
		None
	}

	/// The rules of the initial state from `direction` to `*`.
	fn match_initial_symbols(&mut self, rest: &str) -> Option<(usize, Option<TokenKind>)> {
		use TokenKind as T;
		// `.*direction\s+(TB|BT|RL|LR|TD)[^\n]*`: anywhere in the rest of the line.
		let line_end = rest.find('\n').unwrap_or(rest.len());
		let line = &rest[..line_end];
		for (start, _) in line.match_indices("direction") {
			let after = &line[start + "direction".len()..];
			let blanks = span(after, is_space);
			if blanks > 0
				&& ["TB", "BT", "RL", "LR", "TD"].iter().any(|d| after[blanks..].starts_with(d))
			{
				return Some((line_end, Some(T::Direction)));
			}
		}
		// `[^\s\"]+\@(?=[^\{\"])`
		// The greedy run backtracks to the last `@` that is followed by an allowed character.
		let word = span(rest, |c| !is_space(c) && c != '"');
		let link_id =
			rest[..word].match_indices('@').map(|(index, _)| index).rev().find(|&index| {
				index > 0
					&& rest[index + 1..]
						.chars()
						.next()
						.is_some_and(|next| next != '{' && next != '"')
			});
		if let Some(at) = link_id {
			return Some((at + 1, Some(T::LinkId)));
		}
		let digits = span(rest, |c| c.is_ascii_digit());
		if digits > 0 {
			return Some((digits, Some(T::Num)));
		}
		let single = match rest.as_bytes().first()? {
			b'#' => Some(T::Brkt),
			b':' if rest.starts_with(":::") => return Some((3, Some(T::StyleSeparator))),
			b':' => Some(T::Colon),
			b'&' => Some(T::Amp),
			b';' => Some(T::Semi),
			b',' => Some(T::Comma),
			b'*' => Some(T::Mult),
			_ => None,
		};
		single.map(|kind| (1, Some(kind)))
	}
}

/// `yytext.replace(/\n\s*/g, "<br/>")` of quoted shape data.
fn replace_line_breaks(text: &str) -> String {
	let mut output = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(index) = rest.find('\n') {
		output.push_str(&rest[..index]);
		output.push_str("<br/>");
		let after = &rest[index + 1..];
		rest = &after[span(after, is_space)..];
	}
	output.push_str(rest);
	output
}

#[cfg(test)]
mod tests {
	use super::*;

	fn kinds(input: &str) -> Vec<(TokenKind, String)> {
		let mut lexer = Lexer::new(input);
		let mut tokens = Vec::new();
		loop {
			let Ok(token) = lexer.next_token() else {
				tokens.push((TokenKind::Eof, "<error>".to_owned()));
				break;
			};
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
		let tokens = kinds("graph TD\nA-->B");
		assert_eq!(
			tokens,
			[
				(T::Graph, "graph".into()),
				(T::Dir, " TD".into()),
				(T::Newline, "\n".into()),
				(T::NodeString, "A".into()),
				(T::Link, "-->".into()),
				(T::NodeString, "B".into()),
			]
		);
		let tokens = kinds("A-- text -->B");
		assert_eq!(tokens[1], (T::StartLink, "-- ".into()));
		assert_eq!(tokens.iter().filter(|(kind, _)| *kind == T::EdgeText).count(), 4);
		assert_eq!(tokens[tokens.len() - 2], (T::Link, " -->".into()));
		assert_eq!(kinds("A-.->B")[1], (T::Link, "-.->".into()));
		assert_eq!(kinds("A==>B")[1], (T::Link, "==>".into()));
		assert_eq!(kinds("A---B")[1], (T::Link, "---".into()));
		assert_eq!(kinds("A ~~~ B")[1], (T::Link, " ~~~ ".into()));
		let tokens = kinds("A[\"quoted\"]");
		assert_eq!(
			tokens[1..],
			[(T::Sqs, "[".into()), (T::Str, "quoted".into()), (T::Sqe, "]".into())]
		);
		assert_eq!(kinds("A([x])")[1].0, T::StadiumStart);
		assert_eq!(kinds("A[/x/]")[3].0, T::InvTrapEnd);
		assert_eq!(kinds("A[/x\\]")[3].0, T::TrapEnd);
		assert_eq!(kinds("e1@-->B")[0], (T::LinkId, "e1@".into()));
		assert_eq!(kinds("A@{ shape: rect }")[1], (T::ShapeData, String::new()));
		assert_eq!(kinds("classA")[0], (T::NodeString, "classA".into()));
		assert_eq!(kinds("class A")[0], (T::Class, "class".into()));
		assert_eq!(kinds("endpoint")[0], (T::NodeString, "endpoint".into()));
		assert_eq!(kinds("end\n")[0], (T::End, "end\n".into()));
		assert_eq!(kinds("  direction LR")[0], (T::Direction, "  direction LR".into()));
		assert_eq!(kinds("ñ")[0], (T::UnicodeText, "ñ".into()));
		assert_eq!(kinds("☺").last().unwrap().1, "<error>");
	}
}
