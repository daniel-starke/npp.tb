//! The four railroad grammars (`railroad.langium`, `railroad-ebnf.langium`,
//! `railroad-abnf.langium`, `railroad-peg.langium` with their `valueConverter.ts`) and their
//! conversion into railroad expressions (`railroadParser.ts`, `ebnfParser.ts`, `abnfParser.ts`,
//! `pegParser.ts`, `railroadDb.ts`).
//!
//! Chevrotain lexes the whole document before Langium parses it: at each position the
//! whitespace terminal, then the keywords, longest first, then the other terminals in the order
//! of the grammar; the first that matches wins, except that a keyword gives way to a terminal
//! that matches more (`LONGER_ALT`, e.g. the identifier `choices`, or the EBNF comment `(*`).

use super::super::common::{CommonFields, sanitize_text};
use super::super::langium::Scanner;
use super::super::{Context, Error, Result};
use super::{Expression, Rule};

/// Expressions nested deeper are refused, which bounds the recursion of the parsers and of the
/// layout.
pub const MAX_NESTING: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
	/// `railroad-beta`: Mermaid's own function syntax.
	Functions,
	Ebnf,
	Abnf,
	Peg,
}

impl Syntax {
	fn header(self) -> &'static str {
		match self {
			Syntax::Functions => "railroad-beta",
			Syntax::Ebnf => "railroad-ebnf-beta",
			Syntax::Abnf => "railroad-abnf-beta",
			Syntax::Peg => "railroad-peg-beta",
		}
	}

	/// The keywords of the grammar, longest first like Langium sorts them.
	fn keywords(self) -> &'static [&'static str] {
		match self {
			Syntax::Functions => &[
				"railroad-beta",
				"nonterminal",
				"zeroOrMore",
				"oneOrMore",
				"sequence",
				"optional",
				"terminal",
				"special",
				"choice",
				"=",
				";",
				"(",
				")",
				",",
			],
			Syntax::Ebnf => &[
				"railroad-ebnf-beta",
				"::=",
				"=",
				";",
				"|",
				",",
				"(",
				")",
				"[",
				"]",
				"{",
				"}",
				"?",
				"*",
				"+",
				"-",
			],
			Syntax::Abnf => &["railroad-abnf-beta", "=", ";", "/", "(", ")", "[", "]"],
			Syntax::Peg => {
				&["railroad-peg-beta", "<-", ";", "/", "&", "!", "?", "*", "+", "(", ")", "."]
			}
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	Keyword,
	Title,
	AccTitle,
	AccDescr,
	Id,
	String,
	/// EBNF's `? … ?`.
	Special,
	/// ABNF's `%x41`.
	NumVal,
	/// ABNF's `2*3`.
	Repeat,
	/// ABNF's `3`.
	ExactRepeat,
	Hidden,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
	kind: Kind,
	text: &'a str,
	line: usize,
}

/// The tokens without the hidden ones, or the line and character of the first lexer error.
fn lex(code: &str, syntax: Syntax) -> std::result::Result<Vec<Token<'_>>, (usize, char)> {
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	while let Some(rest) = code.get(position..).filter(|rest| !rest.is_empty()) {
		let Some((kind, length)) = next_token(rest, syntax).filter(|(_, length)| *length > 0)
		else {
			return Err((line, rest.chars().next().unwrap_or(' ')));
		};
		let text = &rest[..length];
		if kind != Kind::Hidden {
			tokens.push(Token { kind, text, line });
		}
		line += text.matches('\n').count();
		position += length;
	}
	Ok(tokens)
}

/// A terminal's matcher: its kind and length at the start of the text.
type Terminal = fn(&str) -> Option<(Kind, usize)>;

fn next_token(rest: &str, syntax: Syntax) -> Option<(Kind, usize)> {
	let whitespace = rest.len() - rest.trim_start_matches(['\t', ' ', '\r', '\n']).len();
	if whitespace > 0 {
		return Some((Kind::Hidden, whitespace));
	}
	for &keyword in syntax.keywords() {
		if !rest.starts_with(keyword) {
			continue;
		}
		if keyword == syntax.header() {
			// `AbstractMermaidTokenBuilder` allows only whitespace or a comment after it.
			let after = &rest[keyword.len()..];
			if !(after.starts_with("%%") || after.chars().next().is_none_or(char::is_whitespace)) {
				continue;
			}
		}
		if let Some(longer) = longer_alternative(rest, keyword, syntax)
			&& longer.1 > keyword.len()
		{
			return Some(longer);
		}
		return Some((Kind::Keyword, keyword.len()));
	}
	let terminals: &[Terminal] = match syntax {
		Syntax::Functions => &[
			title,
			acc_title,
			acc_descr,
			id,
			string,
			yaml,
			directive,
			single_line_comment,
			block_comment,
		],
		Syntax::Ebnf => &[
			title,
			acc_title,
			acc_descr,
			id,
			string,
			special,
			yaml,
			directive,
			single_line_comment,
			block_comment,
			iso_comment,
		],
		Syntax::Abnf => &[
			title,
			acc_title,
			acc_descr,
			rule_name,
			abnf_string,
			numeric_value,
			repeat,
			exact_repeat,
			yaml,
			directive,
			single_line_comment,
			abnf_comment,
		],
		Syntax::Peg => &[
			title,
			acc_title,
			acc_descr,
			id,
			string,
			yaml,
			directive,
			single_line_comment,
			peg_comment,
		],
	};
	terminals.iter().find_map(|terminal| terminal(rest))
}

/// The terminal that Langium makes the `LONGER_ALT` of a keyword: those the keyword is a
/// prefix of a match of.
fn longer_alternative(rest: &str, keyword: &str, syntax: Syntax) -> Option<(Kind, usize)> {
	match (syntax, keyword) {
		(Syntax::Abnf, _) if keyword.starts_with(|c: char| c.is_ascii_alphabetic()) => {
			rule_name(rest)
		}
		(_, _) if keyword.starts_with(|c: char| c.is_ascii_alphabetic()) => id(rest),
		(Syntax::Ebnf, "(") => iso_comment(rest),
		(Syntax::Ebnf, "?") => special(rest),
		(Syntax::Ebnf, "-") => yaml(rest),
		(Syntax::Abnf, ";") => abnf_comment(rest),
		_ => None,
	}
}

fn line_end(text: &str) -> usize {
	text.find(['\n', '\r']).unwrap_or(text.len())
}

/// The end of a single line value: the line end or a `%%` before it.
fn comment_or_line_end(text: &str) -> usize {
	let end = line_end(text);
	text[..end].find("%%").unwrap_or(end)
}

fn blank_length(text: &str) -> usize {
	text.len() - text.trim_start_matches([' ', '\t']).len()
}

/// `TITLE`: `title(?:[\t ][^\n\r]*?(?=%%)|[\t ][^\n\r]*|)`.
fn title(rest: &str) -> Option<(Kind, usize)> {
	let after = rest.strip_prefix("title")?;
	let value = match after.starts_with([' ', '\t']) {
		true => comment_or_line_end(after),
		false => 0,
	};
	Some((Kind::Title, 5 + value))
}

/// `ACC_TITLE`: `accTitle[\t ]*:(?:[^\n\r]*?(?=%%)|[^\n\r]*)`.
fn acc_title(rest: &str) -> Option<(Kind, usize)> {
	let after = rest.strip_prefix("accTitle")?;
	let blanks = blank_length(after);
	let value = after[blanks..].strip_prefix(':')?;
	Some((Kind::AccTitle, 8 + blanks + 1 + comment_or_line_end(value)))
}

/// `ACC_DESCR`: `accDescr(?:[\t ]*:([^\n\r]*?(?=%%)|[^\n\r]*)|\s*{([^}]*)})`.
fn acc_descr(rest: &str) -> Option<(Kind, usize)> {
	let after = rest.strip_prefix("accDescr")?;
	let blanks = blank_length(after);
	if let Some(value) = after[blanks..].strip_prefix(':') {
		return Some((Kind::AccDescr, 8 + blanks + 1 + comment_or_line_end(value)));
	}
	let spaces = after.len() - after.trim_start().len();
	let block = after[spaces..].strip_prefix('{')?;
	let end = block.find('}')?;
	Some((Kind::AccDescr, 8 + spaces + 1 + end + 1))
}

/// `RR_ID`, `EBNF_ID`, `PEG_ID`: `[A-Z_a-z][\w-]*`.
fn id(rest: &str) -> Option<(Kind, usize)> {
	if !rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
		return None;
	}
	let length = rest.len()
		- rest
			.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-')
			.len();
	Some((Kind::Id, length))
}

/// `ABNF_RULENAME`: `[A-Za-z][A-Za-z0-9-]*`.
fn rule_name(rest: &str) -> Option<(Kind, usize)> {
	if !rest.starts_with(|c: char| c.is_ascii_alphabetic()) {
		return None;
	}
	let length =
		rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_alphanumeric() || c == '-').len();
	Some((Kind::Id, length))
}

/// `RR_STRING`, `EBNF_STRING`, `PEG_STRING`: `"([^"\\]|\\.)*"|'([^'\\]|\\.)*'`.
fn string(rest: &str) -> Option<(Kind, usize)> {
	let quote = rest.chars().next().filter(|&quote| quote == '"' || quote == '\'')?;
	let mut characters = rest.char_indices().skip(1);
	while let Some((index, character)) = characters.next() {
		if character == '\\' {
			// `.` matches no line end.
			characters
				.next()
				.filter(|(_, c)| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))?;
		} else if character == quote {
			return Some((Kind::String, index + 1));
		}
	}
	None
}

/// `ABNF_STRING`: `"[^"]*"`.
fn abnf_string(rest: &str) -> Option<(Kind, usize)> {
	let content = rest.strip_prefix('"')?;
	Some((Kind::String, content.find('"')? + 2))
}

/// `EBNF_SPECIAL_SEQUENCE`: `\?(?=[^?;]*[^?\s;][^?;]*\?)[^?;]*\?`.
fn special(rest: &str) -> Option<(Kind, usize)> {
	let content = rest.strip_prefix('?')?;
	let end = content.find(['?', ';'])?;
	let is_closed = content[end..].starts_with('?');
	let has_text = content[..end].chars().any(|c| !is_js_whitespace(c));
	(is_closed && has_text).then_some((Kind::Special, end + 2))
}

/// `ABNF_NUMVAL`: `%[xXdDbB][0-9A-Fa-f]+(?:-[0-9A-Fa-f]+|\.[0-9A-Fa-f]+)*`.
fn numeric_value(rest: &str) -> Option<(Kind, usize)> {
	let digits =
		|text: &str| text.len() - text.trim_start_matches(|c: char| c.is_ascii_hexdigit()).len();
	let after = rest.strip_prefix('%')?.strip_prefix(['x', 'X', 'd', 'D', 'b', 'B'])?;
	let mut length = digits(after);
	if length == 0 {
		return None;
	}
	while let Some(next) = after[length..].strip_prefix(['-', '.']) {
		let more = digits(next);
		if more == 0 {
			break;
		}
		length += 1 + more;
	}
	Some((Kind::NumVal, 2 + length))
}

fn decimal_digits(text: &str) -> usize {
	text.len() - text.trim_start_matches(|c: char| c.is_ascii_digit()).len()
}

/// `ABNF_REPEAT`: `[0-9]*\*[0-9]*`.
fn repeat(rest: &str) -> Option<(Kind, usize)> {
	let leading = decimal_digits(rest);
	let after = rest[leading..].strip_prefix('*')?;
	Some((Kind::Repeat, leading + 1 + decimal_digits(after)))
}

/// `ABNF_EXACT_REPEAT`: `[0-9]+`.
fn exact_repeat(rest: &str) -> Option<(Kind, usize)> {
	let length = decimal_digits(rest);
	(length > 0).then_some((Kind::ExactRepeat, length))
}

/// `(?:\r?\n|(?!\S))` after a front matter or directive: its length, if it matches.
fn block_end(text: &str) -> Option<usize> {
	if text.starts_with("\r\n") {
		return Some(2);
	}
	if text.starts_with('\n') {
		return Some(1);
	}
	text.chars().next().is_none_or(is_js_whitespace).then_some(0)
}

/// The hidden `YAML`: `---[\t ]*\r?\n(?:[\S\s]*?\r?\n)?---(?:\r?\n|(?!\S))`.
fn yaml(rest: &str) -> Option<(Kind, usize)> {
	let after = rest.strip_prefix("---")?;
	let blanks = blank_length(after);
	let after = &after[blanks..];
	let newline = if after.starts_with("\r\n") { 2 } else { usize::from(after.starts_with('\n')) };
	if newline == 0 {
		return None;
	}
	let body_start = 3 + blanks + newline;
	let body = &rest[body_start..];
	// The optional group is tried first, with its lazy content as short as possible.
	for (index, _) in body.match_indices('\n') {
		if let Some(tail) = body[index + 1..].strip_prefix("---").and_then(block_end) {
			return Some((Kind::Hidden, body_start + index + 1 + 3 + tail));
		}
	}
	let tail = body.strip_prefix("---").and_then(block_end)?;
	Some((Kind::Hidden, body_start + 3 + tail))
}

/// The hidden `DIRECTIVE`: `[\t ]*%%{[\S\s]*?}%%(?:\r?\n|(?!\S))`.
fn directive(rest: &str) -> Option<(Kind, usize)> {
	let body = rest.strip_prefix("%%{")?;
	body.match_indices("}%%").find_map(|(index, _)| {
		let tail = block_end(&body[index + 3..])?;
		Some((Kind::Hidden, 3 + index + 3 + tail))
	})
}

/// The hidden `SINGLE_LINE_COMMENT`: `[\t ]*%%[^\n\r]*`.
fn single_line_comment(rest: &str) -> Option<(Kind, usize)> {
	rest.starts_with("%%").then(|| (Kind::Hidden, line_end(rest)))
}

/// The hidden `BLOCK_COMMENT`: `\/\*[\s\S]*?\*\/`.
fn block_comment(rest: &str) -> Option<(Kind, usize)> {
	let body = rest.strip_prefix("/*")?;
	Some((Kind::Hidden, 2 + body.find("*/")? + 2))
}

/// EBNF's hidden `EBNF_ISO_COMMENT`: `\(\*[\s\S]*?\*\)`.
fn iso_comment(rest: &str) -> Option<(Kind, usize)> {
	let body = rest.strip_prefix("(*")?;
	Some((Kind::Hidden, 2 + body.find("*)")? + 2))
}

/// ABNF's hidden `ABNF_COMMENT`: `;[^\n\r]*`.
fn abnf_comment(rest: &str) -> Option<(Kind, usize)> {
	rest.starts_with(';').then(|| (Kind::Hidden, line_end(rest)))
}

/// PEG's hidden `PEG_LINE_COMMENT`: `#[^\n\r]*`.
fn peg_comment(rest: &str) -> Option<(Kind, usize)> {
	rest.starts_with('#').then(|| (Kind::Hidden, line_end(rest)))
}

/// JavaScript's `\s`.
fn is_js_whitespace(character: char) -> bool {
	character.is_whitespace() || character == '\u{feff}'
}

/// `decodeEscapedString` of the value converters: the text between the quotes with `\n`, `\r`
/// and `\t` decoded and other escaped characters taken as they are.
fn decode_string(text: &str) -> String {
	let content = text.get(1..text.len().saturating_sub(1)).unwrap_or_default();
	let mut value = String::with_capacity(content.len());
	let mut characters = content.chars();
	while let Some(character) = characters.next() {
		if character != '\\' {
			value.push(character);
			continue;
		}
		match characters.next() {
			Some('n') => value.push('\n'),
			Some('r') => value.push('\r'),
			Some('t') => value.push('\t'),
			Some(other) => value.push(other),
			None => value.push('\\'),
		}
	}
	value
}

/// A parsed expression with its height, the number of expressions on its longest path.
type Parsed = (Expression, usize);

struct Parser<'a, 'c, 'm> {
	tokens: Vec<Token<'a>>,
	index: usize,
	syntax: Syntax,
	context: &'c Context<'m>,
	last_line: usize,
	nesting: usize,
}

impl<'a> Parser<'a, '_, '_> {
	fn peek(&self) -> Option<Token<'a>> {
		self.tokens.get(self.index).copied()
	}

	fn is_keyword(&self, keyword: &str) -> bool {
		self.peek().is_some_and(|token| token.kind == Kind::Keyword && token.text == keyword)
	}

	fn is_kind(&self, kind: Kind) -> bool {
		self.peek().is_some_and(|token| token.kind == kind)
	}

	fn take_keyword(&mut self, keyword: &str) -> bool {
		let found = self.is_keyword(keyword);
		self.index += usize::from(found);
		found
	}

	fn expect_keyword(&mut self, keyword: &str) -> Result<()> {
		match self.take_keyword(keyword) {
			true => Ok(()),
			false => Err(self.error(&format!("'{keyword}'"))),
		}
	}

	fn take(&mut self, kind: Kind) -> Option<&'a str> {
		let token = self.peek().filter(|token| token.kind == kind)?;
		self.index += 1;
		Some(token.text)
	}

	fn expect(&mut self, kind: Kind, expected: &str) -> Result<&'a str> {
		self.take(kind).ok_or_else(|| self.error(expected))
	}

	fn error(&self, expected: &str) -> Error {
		let (line, found) = match self.peek() {
			Some(token) => {
				let text: String = token.text.chars().take(20).collect();
				(token.line, format!("{text:?}"))
			}
			None => (self.last_line, "the end of the diagram".to_owned()),
		};
		self.context.error_at(line, format!("expected {expected}, found {found}"))
	}

	fn enter(&mut self) -> Result<()> {
		self.nesting += 1;
		match self.nesting > MAX_NESTING {
			true => Err(self.nesting_error()),
			false => Ok(()),
		}
	}

	fn leave(&mut self) {
		self.nesting -= 1;
	}

	fn nesting_error(&self) -> Error {
		let line = self.peek().map_or(self.last_line, |token| token.line);
		self.context
			.error_at(line, format!("Expressions are nested more than {MAX_NESTING} levels deep"))
	}

	/// An expression that wraps `inner`, refused when that makes the tree too high.
	fn wrap(
		&self,
		inner: Parsed,
		make: impl FnOnce(Box<Expression>) -> Expression,
	) -> Result<Parsed> {
		let height = inner.1 + 1;
		match height > MAX_NESTING {
			true => Err(self.nesting_error()),
			false => Ok((make(Box::new(inner.0)), height)),
		}
	}

	fn document(&mut self) -> Result<(CommonFields, Vec<Rule>)> {
		self.expect_keyword(self.syntax.header())?;
		let mut fields = CommonFields::default();
		while let Some(token) = self.peek()
			&& matches!(token.kind, Kind::Title | Kind::AccTitle | Kind::AccDescr)
		{
			self.index += 1;
			if token.kind != Kind::Title {
				Scanner::new(token.text).common_field(&mut fields);
			}
		}
		let mut rules = Vec::new();
		while self.is_kind(Kind::Id) {
			let mut rule = self.rule()?;
			sanitize(&mut rule.definition);
			rule.name = sanitize_text(&rule.name);
			rules.push(rule);
		}
		if self.peek().is_some() {
			return Err(self.error("a rule"));
		}
		Ok((fields, rules))
	}

	fn rule(&mut self) -> Result<Rule> {
		let name = self.expect(Kind::Id, "a rule name")?.to_owned();
		let definition = match self.syntax {
			Syntax::Functions => {
				self.expect_keyword("=")?;
				self.function()?
			}
			Syntax::Ebnf => {
				if !self.take_keyword("=") && !self.take_keyword("::=") {
					return Err(self.error("'=' or '::='"));
				}
				self.ebnf_choice()?
			}
			Syntax::Abnf => {
				self.expect_keyword("=")?;
				self.abnf_alternation()?
			}
			Syntax::Peg => {
				self.expect_keyword("<-")?;
				self.peg_choice()?
			}
		};
		self.expect_keyword(";")?;
		Ok(Rule { name, definition: definition.0 })
	}

	/// `RailroadExpression` of `railroad-beta`.
	fn function(&mut self) -> Result<Parsed> {
		const FUNCTIONS: [&str; 8] = [
			"sequence",
			"choice",
			"optional",
			"oneOrMore",
			"zeroOrMore",
			"terminal",
			"nonterminal",
			"special",
		];
		let Some(&function) = FUNCTIONS.iter().find(|function| self.is_keyword(function)) else {
			return Err(self.error("an expression"));
		};
		self.index += 1;
		self.expect_keyword("(")?;
		let expression = match function {
			"terminal" | "nonterminal" | "special" => {
				let value = decode_string(self.expect(Kind::String, "a string")?);
				let expression = match function {
					"terminal" => Expression::Terminal(value),
					"nonterminal" => Expression::NonTerminal(value),
					_ => Expression::Special(value),
				};
				(expression, 1)
			}
			"sequence" | "choice" => {
				self.enter()?;
				let mut list = vec![self.function()?];
				while self.take_keyword(",") {
					list.push(self.function()?);
				}
				self.leave();
				match function {
					"sequence" => self.list(list, Expression::Sequence)?,
					_ => self.list(list, Expression::Choice)?,
				}
			}
			_ => {
				self.enter()?;
				let inner = self.function()?;
				self.leave();
				match function {
					"optional" => self.wrap(inner, Expression::Optional)?,
					"oneOrMore" => self.wrap(inner, |element| Expression::Repetition {
						element,
						has_bypass: false,
					})?,
					_ => self.wrap(inner, |element| Expression::Repetition {
						element,
						has_bypass: true,
					})?,
				}
			}
		};
		self.expect_keyword(")")?;
		Ok(expression)
	}

	/// A sequence or choice of `list`, or its only element.
	fn list(
		&self,
		mut list: Vec<Parsed>,
		make: impl FnOnce(Vec<Expression>) -> Expression,
	) -> Result<Parsed> {
		if list.len() == 1 {
			return Ok(list.remove(0));
		}
		let height = list.iter().map(|(_, height)| *height).max().unwrap_or(0) + 1;
		if height > MAX_NESTING {
			return Err(self.nesting_error());
		}
		Ok((make(list.into_iter().map(|(expression, _)| expression).collect()), height))
	}

	fn ebnf_choice(&mut self) -> Result<Parsed> {
		self.enter()?;
		let mut alternatives = vec![self.ebnf_sequence()?];
		while self.take_keyword("|") {
			alternatives.push(self.ebnf_sequence()?);
		}
		self.leave();
		self.list(alternatives, Expression::Choice)
	}

	fn starts_ebnf_primary(&self) -> bool {
		["(", "[", "{"].iter().any(|keyword| self.is_keyword(keyword))
			|| self
				.peek()
				.is_some_and(|token| matches!(token.kind, Kind::String | Kind::Special | Kind::Id))
	}

	fn ebnf_sequence(&mut self) -> Result<Parsed> {
		let mut elements = vec![self.ebnf_term()?];
		loop {
			if self.take_keyword(",") || self.starts_ebnf_primary() {
				elements.push(self.ebnf_term()?);
			} else {
				break;
			}
		}
		self.list(elements, Expression::Sequence)
	}

	fn ebnf_term(&mut self) -> Result<Parsed> {
		let mut term = self.ebnf_primary()?;
		loop {
			term = if self.take_keyword("?") {
				self.wrap(term, Expression::Optional)?
			} else if self.take_keyword("*") {
				self.wrap(term, |element| Expression::Repetition { element, has_bypass: true })?
			} else if self.take_keyword("+") {
				self.wrap(term, |element| Expression::Repetition { element, has_bypass: false })?
			} else if self.take_keyword("-") {
				let except = self.ebnf_primary()?;
				let height = term.1.max(except.1) + 1;
				if height > MAX_NESTING {
					return Err(self.nesting_error());
				}
				let minus = Expression::Terminal("-".to_owned());
				(Expression::Sequence(vec![term.0, minus, except.0]), height)
			} else {
				return Ok(term);
			};
		}
	}

	fn ebnf_primary(&mut self) -> Result<Parsed> {
		if let Some(text) = self.take(Kind::String) {
			return Ok((Expression::Terminal(decode_string(text)), 1));
		}
		if let Some(text) = self.take(Kind::Special) {
			let content = text.get(1..text.len() - 1).unwrap_or_default();
			return Ok((Expression::Special(content.trim().to_owned()), 1));
		}
		if let Some(name) = self.take(Kind::Id) {
			return Ok((Expression::NonTerminal(name.to_owned()), 1));
		}
		for (open, close) in [("(", ")"), ("[", "]"), ("{", "}")] {
			if !self.take_keyword(open) {
				continue;
			}
			let inner = self.ebnf_choice()?;
			self.expect_keyword(close)?;
			return match open {
				"(" => Ok(inner),
				"[" => self.wrap(inner, Expression::Optional),
				_ => {
					self.wrap(inner, |element| Expression::Repetition { element, has_bypass: true })
				}
			};
		}
		Err(self.error("an expression"))
	}

	fn abnf_alternation(&mut self) -> Result<Parsed> {
		self.enter()?;
		let mut alternatives = vec![self.abnf_concatenation()?];
		while self.take_keyword("/") {
			alternatives.push(self.abnf_concatenation()?);
		}
		self.leave();
		self.list(alternatives, Expression::Choice)
	}

	fn abnf_concatenation(&mut self) -> Result<Parsed> {
		let mut elements = vec![self.abnf_element()?];
		while self.is_keyword("(")
			|| self.is_keyword("[")
			|| self.peek().is_some_and(|token| {
				matches!(
					token.kind,
					Kind::Repeat | Kind::ExactRepeat | Kind::String | Kind::NumVal | Kind::Id
				)
			}) {
			elements.push(self.abnf_element()?);
		}
		self.list(elements, Expression::Sequence)
	}

	fn abnf_element(&mut self) -> Result<Parsed> {
		let repeat = self.take(Kind::Repeat).or_else(|| self.take(Kind::ExactRepeat));
		let primary = self.abnf_primary()?;
		let Some(repeat) = repeat else { return Ok(primary) };
		// `parseRepeat`: only a minimum of 0, and the range `0*1`, change the drawing.
		let (minimum, maximum) = repeat.split_once('*').unwrap_or((repeat, repeat));
		let is_zero = |digits: &str| digits.bytes().all(|digit| digit == b'0');
		let is_one = |digits: &str| digits.trim_start_matches('0') == "1";
		if is_zero(minimum) && repeat.contains('*') && is_one(maximum) {
			return self.wrap(primary, Expression::Optional);
		}
		let has_bypass = is_zero(minimum);
		self.wrap(primary, |element| Expression::Repetition { element, has_bypass })
	}

	fn abnf_primary(&mut self) -> Result<Parsed> {
		if let Some(text) = self.take(Kind::String) {
			let value = text.get(1..text.len() - 1).unwrap_or_default();
			return Ok((Expression::Terminal(value.to_owned()), 1));
		}
		if let Some(text) = self.take(Kind::NumVal) {
			return Ok((Expression::Terminal(text.to_owned()), 1));
		}
		if let Some(name) = self.take(Kind::Id) {
			return Ok((Expression::NonTerminal(name.to_owned()), 1));
		}
		for (open, close) in [("(", ")"), ("[", "]")] {
			if !self.take_keyword(open) {
				continue;
			}
			let inner = self.abnf_alternation()?;
			self.expect_keyword(close)?;
			return match open {
				"(" => Ok(inner),
				_ => self.wrap(inner, Expression::Optional),
			};
		}
		Err(self.error("an element"))
	}

	fn peg_choice(&mut self) -> Result<Parsed> {
		self.enter()?;
		let mut alternatives = vec![self.peg_sequence()?];
		while self.take_keyword("/") {
			alternatives.push(self.peg_sequence()?);
		}
		self.leave();
		self.list(alternatives, Expression::Choice)
	}

	fn peg_sequence(&mut self) -> Result<Parsed> {
		let mut elements = vec![self.peg_prefix()?];
		while ["&", "!", "(", "."].iter().any(|keyword| self.is_keyword(keyword))
			|| self.peek().is_some_and(|token| matches!(token.kind, Kind::String | Kind::Id))
		{
			elements.push(self.peg_prefix()?);
		}
		self.list(elements, Expression::Sequence)
	}

	fn peg_prefix(&mut self) -> Result<Parsed> {
		let operator = ["&", "!"].into_iter().find(|operator| self.take_keyword(operator));
		let inner = self.peg_suffix()?;
		let Some(operator) = operator else { return Ok(inner) };
		// Lookaheads are drawn as special sequences.
		let label = match &inner.0 {
			Expression::Terminal(value) => format!("\"{value}\""),
			Expression::NonTerminal(name) => name.clone(),
			Expression::Special(text) => text.clone(),
			_ => "(...)".to_owned(),
		};
		Ok((Expression::Special(format!("{operator}{label}")), 1))
	}

	fn peg_suffix(&mut self) -> Result<Parsed> {
		let primary = self.peg_primary()?;
		if self.take_keyword("?") {
			return self.wrap(primary, Expression::Optional);
		}
		if self.take_keyword("*") {
			return self
				.wrap(primary, |element| Expression::Repetition { element, has_bypass: true });
		}
		if self.take_keyword("+") {
			return self
				.wrap(primary, |element| Expression::Repetition { element, has_bypass: false });
		}
		Ok(primary)
	}

	fn peg_primary(&mut self) -> Result<Parsed> {
		if let Some(text) = self.take(Kind::String) {
			return Ok((Expression::Terminal(decode_string(text)), 1));
		}
		if let Some(name) = self.take(Kind::Id) {
			return Ok((Expression::NonTerminal(name.to_owned()), 1));
		}
		if self.take_keyword(".") {
			return Ok((Expression::Special(".".to_owned()), 1));
		}
		if self.take_keyword("(") {
			let inner = self.peg_choice()?;
			self.expect_keyword(")")?;
			return Ok(inner);
		}
		Err(self.error("an expression"))
	}
}

/// `sanitizeAstNode` of `railroadDb.ts`.
fn sanitize(expression: &mut Expression) {
	match expression {
		Expression::Terminal(text) | Expression::NonTerminal(text) | Expression::Special(text) => {
			*text = sanitize_text(text);
		}
		Expression::Sequence(list) | Expression::Choice(list) => list.iter_mut().for_each(sanitize),
		Expression::Optional(element) | Expression::Repetition { element, .. } => sanitize(element),
	}
}

pub fn parse(code: &str, syntax: Syntax, context: &Context) -> Result<(CommonFields, Vec<Rule>)> {
	let tokens = match lex(code, syntax) {
		Ok(tokens) => tokens,
		Err((line, character)) => {
			return Err(context.error_at(line, format!("unexpected character {character:?}")));
		}
	};
	let last_line = 1 + code.trim_end().matches('\n').count();
	let mut parser = Parser { tokens, index: 0, syntax, context, last_line, nesting: 0 };
	parser.document()
}

#[cfg(test)]
mod tests {
	use super::super::super::text::testing::FixedMeasure;
	use super::*;

	fn parse_rules(code: &str, syntax: Syntax) -> Result<Vec<Rule>> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, syntax, &context).map(|(_, rules)| rules)
	}

	fn describe(expression: &Expression) -> String {
		match expression {
			Expression::Terminal(value) => format!("t({value})"),
			Expression::NonTerminal(name) => format!("n({name})"),
			Expression::Special(text) => format!("s({text})"),
			Expression::Sequence(list) => {
				format!("seq[{}]", list.iter().map(describe).collect::<Vec<_>>().join(" "))
			}
			Expression::Choice(list) => {
				format!("or[{}]", list.iter().map(describe).collect::<Vec<_>>().join(" "))
			}
			Expression::Optional(element) => format!("opt({})", describe(element)),
			Expression::Repetition { element, has_bypass: true } => {
				format!("many({})", describe(element))
			}
			Expression::Repetition { element, has_bypass: false } => {
				format!("some({})", describe(element))
			}
		}
	}

	fn rules(code: &str, syntax: Syntax) -> Vec<String> {
		match parse_rules(code, syntax) {
			Ok(rules) => rules
				.iter()
				.map(|rule| format!("{} = {}", rule.name, describe(&rule.definition)))
				.collect(),
			Err(error) => vec![format!("error {:?}: {}", error.line, error.message)],
		}
	}

	#[test]
	fn parses_function_syntax() {
		let code = "railroad-beta\ntitle Numbers\nlist = sequence(terminal(\"[\"), optional(sequence(\
			nonterminal(\"n\"), zeroOrMore(sequence(terminal(','), nonterminal(\"n\"))))), \
			terminal(\"]\")) ;\nsign = choice(terminal(\"+\"), special(\"a \\\"b\\\"\")) ;\n\
			one = sequence(oneOrMore(terminal(\"x\"))) ; /* c */ choices = terminal(\"y\");";
		assert_eq!(
			rules(code, Syntax::Functions),
			[
				"list = seq[t([) opt(seq[n(n) many(seq[t(,) n(n)])]) t(])]",
				"sign = or[t(+) s(a \"b\")]",
				"one = some(t(x))",
				"choices = t(y)",
			]
		);
	}

	#[test]
	fn parses_ebnf() {
		let code = "railroad-ebnf-beta\nexpr ::= term ( \"+\" term | \"-\" term )* ;\n\
			id = letter , { letter | digit | \"_\" } ; (* comment *)\n\
			opt = [ a ] c+ - d ? special ? b? ;
			quirk = b? c ? ;";
		assert_eq!(
			rules(code, Syntax::Ebnf),
			[
				"expr = seq[n(term) many(or[seq[t(+) n(term)] seq[t(-) n(term)]])]",
				"id = seq[n(letter) many(or[n(letter) n(digit) t(_)])]",
				"opt = seq[opt(n(a)) seq[some(n(c)) t(-) n(d)] s(special) opt(n(b))]",
				// Like in Mermaid, the postfix `?` opens a special sequence when another follows.
				"quirk = seq[n(b) s(c)]",
			]
		);
		// `? ;` cannot start a special sequence, so `?` is the postfix.
		assert_eq!(rules("railroad-ebnf-beta\na = b ? ;", Syntax::Ebnf), ["a = opt(n(b))"]);
	}

	#[test]
	fn parses_abnf() {
		let code = "railroad-abnf-beta\nscheme = ALPHA *( ALPHA / DIGIT / \"+\" ) ;\n\
			n = 1*3DIGIT 0*1x 2y %x41-5A [z] ;\n";
		assert_eq!(
			rules(code, Syntax::Abnf),
			[
				"scheme = seq[n(ALPHA) many(or[n(ALPHA) n(DIGIT) t(+)])]",
				"n = seq[some(n(DIGIT)) opt(n(x)) some(n(y)) t(%x41-5A) opt(n(z))]",
			]
		);
		// `;` followed by text is a comment, so the rule goes on.
		assert!(
			rules("railroad-abnf-beta\na = b ;c\nd = e ;", Syntax::Abnf)[0].starts_with("error")
		);
	}

	#[test]
	fn parses_peg() {
		let code = "railroad-peg-beta\nE <- T ((\"+\" / \"-\") T)* ; # comment\n\
			A <- &B !\"c\" !(d) !e* . e? ;";
		assert_eq!(
			rules(code, Syntax::Peg),
			[
				"E = seq[n(T) many(seq[or[t(+) t(-)] n(T)])]",
				"A = seq[s(&B) s(!\"c\") s(!d) s(!(...)) s(.) opt(n(e))]",
			]
		);
	}

	#[test]
	fn reports_errors_on_their_lines() {
		for (code, syntax, line) in [
			("railroad-beta\nx = terminal(\"a\")\ny = nonterminal(\"b\") ;", Syntax::Functions, 3),
			("railroad-beta\nx = terminal(\"a\" ;", Syntax::Functions, 2),
			("railroad-beta\n\nx = é ;", Syntax::Functions, 3),
			("railroad-betax\n", Syntax::Functions, 1),
			("Railroad-beta\n", Syntax::Functions, 1),
			("railroad-ebnf-beta\na = ;", Syntax::Ebnf, 2),
			("railroad-peg-beta\na = b ;", Syntax::Peg, 2),
			("railroad-abnf-beta\na = \"b ;", Syntax::Abnf, 2),
		] {
			let Err(error) = parse_rules(code, syntax) else { panic!("{code}") };
			assert_eq!(error.line, Some(line), "{code}: {}", error.message);
		}
		assert!(
			parse_rules("railroad-beta", Syntax::Functions).is_ok_and(|rules| rules.is_empty())
		);
	}

	#[test]
	fn refuses_deep_nesting_without_overflowing() {
		let deep = format!(
			"railroad-beta\nx = {}terminal(\"a\"){} ;",
			"optional(".repeat(MAX_NESTING + 5),
			")".repeat(MAX_NESTING + 5)
		);
		assert!(parse_rules(&deep, Syntax::Functions).is_err());
		let postfixes = format!("railroad-ebnf-beta\nx = a{} ;", "*".repeat(10 * MAX_NESTING));
		assert!(parse_rules(&postfixes, Syntax::Ebnf).is_err());
		let groups =
			format!("railroad-peg-beta\nx <- {}a{} ;", "(".repeat(10000), ")".repeat(10000));
		assert!(parse_rules(&groups, Syntax::Peg).is_err());
		let fine = format!("railroad-ebnf-beta\nx = a{} ;", "?".repeat(MAX_NESTING - 1));
		assert!(parse_rules(&fine, Syntax::Ebnf).is_ok());
	}
}
