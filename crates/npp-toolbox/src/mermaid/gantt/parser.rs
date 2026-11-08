//! The gantt grammar (`gantt.jison`): a hand-written lexer that follows jison's first-match rules
//! and the statements, which call the database like the grammar's actions.

use super::super::jison::{
	keyword, keyword_and_delimiter, line_length, span, starts_with_ignoring_case,
};
use super::super::{Context, Result};
use super::db::Database;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	OpenDirective,
	AccTitle,
	AccTitleValue,
	AccDescription,
	AccDescriptionValue,
	AccDescriptionMultilineValue,
	NewLine,
	Href,
	CallbackName,
	CallbackArgs,
	Click,
	Gantt,
	DateFormat,
	InclusiveEndDates,
	TopAxis,
	AxisFormat,
	TickInterval,
	Includes,
	Excludes,
	TodayMarker,
	Weekday,
	Weekend,
	Date,
	Title,
	/// The `accDescription` keyword, which the grammar has no statement for.
	AccDescriptionKeyword,
	Section,
	TaskText,
	TaskData,
	Colon,
	/// The grammar's `EOF` terminal.
	EndOfFile,
	/// jison's end of input after `EOF`, or at the end in any other state.
	End,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
	kind: Kind,
	text: &'a str,
	/// The line the token ends on, where jison reports errors.
	line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
	Initial,
	AccTitle,
	AccDescription,
	AccDescriptionMultiline,
	Click,
	Href,
	CallbackName,
	CallbackArgs,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
	None,
	Push(Condition),
	Pop,
	/// Pops the condition and pushes another one.
	Replace(Condition),
}

/// `keyword\s<value>+`: the length of the match, at least one value character.
fn keyword_statement(text: &str, keyword: &str, is_value: fn(char) -> bool) -> Option<usize> {
	if !starts_with_ignoring_case(text, keyword) {
		return None;
	}
	let space =
		text[keyword.len()..].chars().next().filter(|character| character.is_whitespace())?;
	let prefix = keyword.len() + space.len_utf8();
	let value = span(&text[prefix..], is_value);
	(value > 0).then_some(prefix + value)
}

/// `[^#\n;]`
fn is_value_character(character: char) -> bool {
	!matches!(character, '#' | '\n' | ';')
}

/// `[^\n;]`
fn is_marker_character(character: char) -> bool {
	!matches!(character, '\n' | ';')
}

fn is_line_character(character: char) -> bool {
	character != '\n'
}

/// `keyword\s+<day>` with jison's word boundary after the day.
fn day_statement(text: &str, keyword: &str, day: &str) -> Option<usize> {
	if !starts_with_ignoring_case(text, keyword) {
		return None;
	}
	let spaces = span(&text[keyword.len()..], char::is_whitespace);
	let start = keyword.len() + spaces;
	(spaces > 0 && super::super::jison::keyword(&text[start..], day)).then_some(start + day.len())
}

/// `\d\d\d\d"-"\d\d"-"\d\d` with jison's word boundary.
fn is_date(text: &str) -> bool {
	let bytes = text.as_bytes();
	let pattern = b"dddd-dd-dd";
	bytes.len() >= pattern.len()
		&& pattern.iter().zip(bytes).all(|(&expected, &byte)| match expected {
			b'd' => byte.is_ascii_digit(),
			other => other == byte,
		}) && !bytes.get(pattern.len()).copied().is_some_and(super::super::jison::is_word_byte)
}

const WEEKDAYS: [&str; 7] =
	["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];

/// The rules of the initial state in grammar order: the token kind (`None` for skipped text), its
/// length and the action.
fn initial_rule(rest: &str) -> (Option<Kind>, usize, Action) {
	use Condition as C;
	let first = rest.chars().next().unwrap_or_default();
	if rest.starts_with("%%{") {
		return (Some(Kind::OpenDirective), 3, Action::None);
	}
	if let Some(length) = keyword_and_delimiter(rest, "accTitle", ':') {
		return (Some(Kind::AccTitle), length, Action::Push(C::AccTitle));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accDescr", ':') {
		return (Some(Kind::AccDescription), length, Action::Push(C::AccDescription));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accDescr", '{') {
		return (None, length, Action::Push(C::AccDescriptionMultiline));
	}
	if rest.starts_with("%%") {
		return (None, line_length(rest), Action::None);
	}
	// `[^\}]\%\%*[^\n]*`, whose first character may be a line end.
	let after_first = &rest[first.len_utf8()..];
	if first != '}' && after_first.starts_with('%') {
		return (None, first.len_utf8() + line_length(after_first), Action::None);
	}
	if first == '%' {
		let line = line_length(rest);
		return (None, line + span(&rest[line..], |character| character == '\n'), Action::None);
	}
	if first == '\n' {
		return (Some(Kind::NewLine), span(rest, |character| character == '\n'), Action::None);
	}
	if first.is_whitespace() {
		return (None, span(rest, char::is_whitespace), Action::None);
	}
	if starts_with_ignoring_case(rest, "href") {
		let spaces = span(&rest[4..], char::is_whitespace);
		if spaces > 0 && rest[4 + spaces..].starts_with('"') {
			return (None, 4 + spaces + 1, Action::Push(C::Href));
		}
	}
	for (word, condition) in [("call", C::CallbackName), ("click", C::Click)] {
		if starts_with_ignoring_case(rest, word) {
			let spaces = span(&rest[word.len()..], char::is_whitespace);
			if spaces > 0 {
				return (None, word.len() + spaces, Action::Push(condition));
			}
		}
	}
	if keyword(rest, "gantt") {
		return (Some(Kind::Gantt), 5, Action::None);
	}
	if let Some(length) = keyword_statement(rest, "dateFormat", is_value_character) {
		return (Some(Kind::DateFormat), length, Action::None);
	}
	if keyword(rest, "inclusiveEndDates") {
		return (Some(Kind::InclusiveEndDates), "inclusiveEndDates".len(), Action::None);
	}
	if keyword(rest, "topAxis") {
		return (Some(Kind::TopAxis), "topAxis".len(), Action::None);
	}
	for (word, kind) in [
		("axisFormat", Kind::AxisFormat),
		("tickInterval", Kind::TickInterval),
		("includes", Kind::Includes),
		("excludes", Kind::Excludes),
	] {
		if let Some(length) = keyword_statement(rest, word, is_value_character) {
			return (Some(kind), length, Action::None);
		}
	}
	if let Some(length) = keyword_statement(rest, "todayMarker", is_marker_character) {
		return (Some(Kind::TodayMarker), length, Action::None);
	}
	for day in WEEKDAYS {
		if let Some(length) = day_statement(rest, "weekday", day) {
			return (Some(Kind::Weekday), length, Action::None);
		}
	}
	for day in ["friday", "saturday"] {
		if let Some(length) = day_statement(rest, "weekend", day) {
			return (Some(Kind::Weekend), length, Action::None);
		}
	}
	if is_date(rest) {
		return (Some(Kind::Date), 10, Action::None);
	}
	if let Some(length) = keyword_statement(rest, "title", is_line_character) {
		return (Some(Kind::Title), length, Action::None);
	}
	if let Some(length) = keyword_statement(rest, "accDescription", is_value_character) {
		return (Some(Kind::AccDescriptionKeyword), length, Action::None);
	}
	if let Some(length) = keyword_statement(rest, "section", is_line_character) {
		return (Some(Kind::Section), length, Action::None);
	}
	if first != ':' {
		let length = span(rest, |character| !matches!(character, ':' | '\n'));
		return (Some(Kind::TaskText), length, Action::None);
	}
	let data = span(&rest[1..], is_value_character);
	match data {
		0 => (Some(Kind::Colon), 1, Action::None),
		_ => (Some(Kind::TaskData), 1 + data, Action::None),
	}
}

/// The tokens of `gantt.jison`. Like jison, the first rule that matches wins.
fn lex(code: &str, first_line: usize) -> Vec<Token<'_>> {
	use Condition as C;
	let mut tokens = Vec::new();
	let mut condition = vec![C::Initial];
	let mut position = 0;
	let mut line = first_line;
	loop {
		let rest = &code[position..];
		let current = condition.last().copied().unwrap_or(C::Initial);
		if rest.is_empty() {
			// The rules that match the empty text at the end, then jison's end of input.
			let kind = match current {
				C::Initial => Some(Kind::EndOfFile),
				C::AccTitle => Some(Kind::AccTitleValue),
				C::AccDescription => Some(Kind::AccDescriptionValue),
				C::AccDescriptionMultiline => Some(Kind::AccDescriptionMultilineValue),
				C::Click => Some(Kind::Click),
				C::Href => Some(Kind::Href),
				C::CallbackName => Some(Kind::CallbackName),
				C::CallbackArgs => Some(Kind::CallbackArgs),
			};
			if let Some(kind) = kind {
				tokens.push(Token { kind, text: "", line });
			}
			tokens.push(Token { kind: Kind::End, text: "", line });
			return tokens;
		}
		let (kind, length, action) = match current {
			C::Initial => initial_rule(rest),
			C::AccTitle => (Some(Kind::AccTitleValue), line_length(rest), Action::Pop),
			C::AccDescription => (Some(Kind::AccDescriptionValue), line_length(rest), Action::Pop),
			C::AccDescriptionMultiline => match rest.starts_with('}') {
				true => (None, 1, Action::Pop),
				false => (
					Some(Kind::AccDescriptionMultilineValue),
					span(rest, |character| character != '}'),
					Action::None,
				),
			},
			C::Click => match rest.chars().next() {
				Some(space) if space.is_whitespace() => (None, space.len_utf8(), Action::Pop),
				_ => (
					Some(Kind::Click),
					span(rest, |character| !character.is_whitespace()),
					Action::None,
				),
			},
			C::Href => match rest.starts_with('"') {
				true => (None, 1, Action::Pop),
				false => (Some(Kind::Href), span(rest, |character| character != '"'), Action::None),
			},
			C::CallbackName => {
				let spaces = span(&rest[usize::from(rest.starts_with('('))..], char::is_whitespace);
				if rest.starts_with('(') && rest[1 + spaces..].starts_with(')') {
					(None, 1 + spaces + 1, Action::Pop)
				} else if rest.starts_with('(') {
					(None, 1, Action::Replace(C::CallbackArgs))
				} else {
					(
						Some(Kind::CallbackName),
						span(rest, |character| character != '('),
						Action::None,
					)
				}
			}
			C::CallbackArgs => match rest.starts_with(')') {
				true => (None, 1, Action::Pop),
				false => (
					Some(Kind::CallbackArgs),
					span(rest, |character| character != ')'),
					Action::None,
				),
			},
		};
		line += rest[..length].matches('\n').count();
		if let Some(kind) = kind {
			tokens.push(Token { kind, text: &rest[..length], line });
		}
		position += length;
		match action {
			Action::None => {}
			Action::Push(next) => condition.push(next),
			Action::Pop => _ = condition.pop(),
			Action::Replace(next) => {
				condition.pop();
				condition.push(next);
			}
		}
	}
}

fn describe(token: &Token) -> String {
	match token.kind {
		Kind::End | Kind::EndOfFile => "the end of the diagram".to_owned(),
		Kind::NewLine => "a line end".to_owned(),
		_ => format!("{:?}", token.text.trim()),
	}
}

/// The text after a keyword and the one whitespace character `substr` removes with it.
fn after_keyword<'a>(text: &'a str, keyword: &str) -> &'a str {
	let rest = &text[keyword.len()..];
	let space = rest.chars().next().map_or(0, char::len_utf8);
	&rest[space..]
}

/// Parses the preprocessed code into `database`, like the grammar's actions.
pub(super) fn parse(code: &str, context: &Context, database: &mut Database) -> Result<()> {
	// Mermaid removes leading whitespace before parsing.
	let trimmed = code.trim_start();
	let first_line = 1 + code[..code.len() - trimmed.len()].matches('\n').count();
	let tokens = lex(trimmed, first_line);
	let mut position = 0;
	let error = |token: &Token, expected: &str| {
		context.error_at(token.line, format!("expected {expected}, found {}", describe(token)))
	};
	let first = tokens.first().copied().unwrap_or(Token { kind: Kind::End, text: "", line: 1 });
	if first.kind != Kind::Gantt {
		return Err(error(&first, "`gantt`"));
	}
	position += 1;
	loop {
		let Some(&token) = tokens.get(position) else { return Ok(()) };
		position += 1;
		let next = |offset: usize| tokens.get(position + offset).copied();
		match token.kind {
			Kind::NewLine => {}
			Kind::EndOfFile => {
				return match next(0) {
					Some(Token { kind: Kind::End, .. }) | None => Ok(()),
					Some(other) => Err(error(&other, "the end of the diagram")),
				};
			}
			Kind::DateFormat => {
				database.date_format = after_keyword(token.text, "dateFormat").to_owned()
			}
			Kind::InclusiveEndDates => database.inclusive_end_dates = true,
			// The grammar calls `yy.TopAxis()`, which the database does not have.
			Kind::TopAxis => {
				return Err(context.error_at(token.line, "yy.TopAxis is not a function"));
			}
			Kind::AxisFormat => {
				database.axis_format = after_keyword(token.text, "axisFormat").to_owned()
			}
			Kind::TickInterval => {
				database.tick_interval = Some(after_keyword(token.text, "tickInterval").to_owned())
			}
			Kind::Excludes => database.add_excludes(after_keyword(token.text, "excludes")),
			Kind::Includes => database.add_includes(after_keyword(token.text, "includes")),
			Kind::TodayMarker => {
				database.today_marker = after_keyword(token.text, "todayMarker").to_owned()
			}
			Kind::Weekday => {
				let day = token.text["weekday".len()..].trim_start().to_ascii_lowercase();
				database.weekday = day;
			}
			Kind::Weekend => {
				let day = token.text["weekend".len()..].trim_start().to_ascii_lowercase();
				database.weekend = day;
			}
			Kind::Title => database.set_title(after_keyword(token.text, "title")),
			Kind::AccTitle => {
				let value = next(0).filter(|value| value.kind == Kind::AccTitleValue);
				let Some(value) = value else {
					return Err(error(&next(0).unwrap_or(token), "a title"));
				};
				position += 1;
				database.set_acc_title(value.text);
			}
			Kind::AccDescription => {
				let value = next(0).filter(|value| value.kind == Kind::AccDescriptionValue);
				let Some(value) = value else {
					return Err(error(&next(0).unwrap_or(token), "a description"));
				};
				position += 1;
				database.set_acc_description(value.text);
			}
			Kind::AccDescriptionMultilineValue => database.set_acc_description(token.text),
			Kind::Section => database.add_section(after_keyword(token.text, "section")),
			Kind::Click => {
				let ids = token.text;
				let kinds = [next(0), next(1), next(2)].map(|token| token.map(|token| token.kind));
				let text = |offset: usize| next(offset).map_or("", |token| token.text);
				let (link, length, has_callback) = match kinds {
					[Some(Kind::CallbackName), Some(Kind::CallbackArgs), Some(Kind::Href)] => {
						(Some(text(2)), 3, true)
					}
					[Some(Kind::CallbackName), Some(Kind::CallbackArgs), _] => (None, 2, true),
					[Some(Kind::CallbackName), Some(Kind::Href), _] => (Some(text(1)), 2, true),
					[Some(Kind::CallbackName), ..] => (None, 1, true),
					[Some(Kind::Href), Some(Kind::CallbackName), Some(Kind::CallbackArgs)] => {
						(Some(text(0)), 3, true)
					}
					[Some(Kind::Href), Some(Kind::CallbackName), _] => (Some(text(0)), 2, true),
					[Some(Kind::Href), ..] => (Some(text(0)), 1, false),
					_ => {
						return Err(error(&next(0).unwrap_or(token), "`call` or `href`"));
					}
				};
				// Callbacks only run with `securityLevel: loose`, which a diagram cannot set,
				// but they still mark the tasks clickable.
				if has_callback {
					database.set_clickable(ids);
				}
				if let Some(link) = link {
					database.set_link(ids, link);
				}
				position += length;
			}
			Kind::TaskText => {
				let Some(data) = next(0).filter(|data| data.kind == Kind::TaskData) else {
					return Err(error(&next(0).unwrap_or(token), "task data after `:`"));
				};
				position += 1;
				database
					.add_task(token.text, data.text, data.line)
					.map_err(|message| context.error_at(data.line, message))?;
			}
			_ => return Err(error(&token, "a statement")),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn kinds(code: &str) -> Vec<(Kind, &str)> {
		lex(code, 1).into_iter().map(|token| (token.kind, token.text)).collect()
	}

	#[test]
	fn lexes_like_jison() {
		use Kind as K;
		assert_eq!(
			kinds("gantt\n dateFormat YYYY-MM-DD\nA task :a1, 2014-01-01, 3d\n"),
			[
				(K::Gantt, "gantt"),
				(K::NewLine, "\n"),
				(K::DateFormat, "dateFormat YYYY-MM-DD"),
				(K::NewLine, "\n"),
				(K::TaskText, "A task "),
				(K::TaskData, ":a1, 2014-01-01, 3d"),
				(K::NewLine, "\n"),
				(K::EndOfFile, ""),
				(K::End, ""),
			]
		);
		assert_eq!(
			kinds("click a,b href \"x\" call f(1)\n"),
			[
				(K::Click, "a,b"),
				(K::Href, "x"),
				(K::CallbackName, "f"),
				(K::CallbackArgs, "1"),
				(K::NewLine, "\n"),
				(K::EndOfFile, ""),
				(K::End, ""),
			]
		);
		// `\s+` takes the line end after trailing blanks, and `title\s` the one after `title`.
		assert_eq!(kinds("a :b\n  \nc")[3], (K::TaskText, "c"));
		assert_eq!(kinds("title\nx")[0], (K::Title, "title\nx"));
		assert_eq!(kinds("weekday  Monday x")[0], (K::Weekday, "weekday  Monday"));
		assert_eq!(kinds("weekday mondays")[0], (K::TaskText, "weekday mondays"));
		assert_eq!(kinds("2014-01-01 x")[0], (K::Date, "2014-01-01"));
		assert_eq!(kinds("x 50% done")[0], (K::TaskText, "x 50% done"));
		assert_eq!(kinds("\n% note\n\nx")[1], (K::TaskText, "x"));
		assert_eq!(kinds("call f")[0], (K::CallbackName, "f"));
	}
}
