//! `\def`, `\gdef`, `\newcommand`, `\renewcommand` and `\providecommand`: a pass over the LaTeX of one
//! formula that writes the definitions' expansions into a new source, before it is converted.
//!
//! Follows KaTeX (`MacroExpander.ts`, `functions/def.ts`, `macros.ts`) with its error messages:
//! definitions end with their group and with their formula, and an expansion limit stops macros that
//! never end. After an error the rest of the source stays as written, so that the command shows up
//! as unknown.

use std::borrow::Cow;

/// KaTeX's `maxExpand`.
const MAX_EXPANSIONS: usize = 1000;
/// The bound of what the expansions write: an argument used twice doubles the text every time.
const MAX_SOURCE_BYTES: usize = 1 << 18;
const TOO_MANY_EXPANSIONS: &str =
	"Too many expansions: infinite loop or need to increase maxExpand setting";

pub struct Expansion<'a> {
	pub source: Cow<'a, str>,
	/// The first error, which callers that tolerate malformed input ignore.
	pub error: Option<String>,
}

/// `latex` with its macros expanded; `is_known_command` tells which commands `\newcommand` must not
/// redefine.
pub fn expand_macros<'a>(latex: &'a str, is_known_command: &dyn Fn(&str) -> bool) -> Expansion<'a> {
	if !latex.contains("def") && !latex.contains("command") {
		return Expansion { source: Cow::Borrowed(latex), error: None };
	}
	let mut expander = Expander {
		text: latex.to_owned(),
		position: 0,
		output: String::with_capacity(latex.len()),
		global: Vec::new(),
		local: Vec::new(),
		group_starts: Vec::new(),
		expansions: 0,
		has_definition: false,
		error: None,
		is_known_command,
	};
	expander.run();
	if !expander.has_definition {
		return Expansion { source: Cow::Borrowed(latex), error: expander.error };
	}
	Expansion { source: Cow::Owned(expander.output), error: expander.error }
}

struct Macro {
	name: String,
	/// The text before each parameter and after the last one: `\def\a#1.#2;{}` has `["", ".", ";"]`.
	delimiters: Vec<String>,
	body: String,
}

struct Expander<'k> {
	/// What is still to read, from `position`; expansions are written back into it.
	text: String,
	position: usize,
	output: String,
	global: Vec<Macro>,
	local: Vec<Macro>,
	/// For each open group the length of `local` at its start.
	group_starts: Vec<usize>,
	expansions: usize,
	has_definition: bool,
	error: Option<String>,
	is_known_command: &'k dyn Fn(&str) -> bool,
}

impl Expander<'_> {
	fn run(&mut self) {
		while let Some(character) = self.rest().chars().next() {
			match character {
				'\\' => {
					if !self.control_sequence() {
						break;
					}
				}
				'%' => {
					let length = self.rest().find('\n').map_or(self.rest().len(), |end| end + 1);
					self.copy(length);
				}
				'{' => {
					self.group_starts.push(self.local.len());
					self.copy(1);
				}
				'}' => {
					if let Some(start) = self.group_starts.pop() {
						self.local.truncate(start);
					}
					self.copy(1);
				}
				_ => {
					let found = self.find_macro(&character_macro_name(character));
					match found {
						Some(found) => {
							if !self.expand(found, character.len_utf8()) {
								break;
							}
						}
						None => self.copy(character.len_utf8()),
					}
				}
			}
		}
		let rest = self.text[self.position..].to_owned();
		self.output.push_str(&rest);
	}

	fn rest(&self) -> &str {
		&self.text[self.position..]
	}

	fn copy(&mut self, length: usize) {
		let end = self.position + length;
		self.output.push_str(&self.text[self.position..end]);
		self.position = end;
	}

	fn fail(&mut self, message: impl Into<String>) {
		if self.error.is_none() {
			self.error = Some(message.into());
		}
	}

	/// Handles the control sequence at the position; `false` stops the expansion.
	fn control_sequence(&mut self) -> bool {
		let length = control_sequence_length(self.rest());
		let name = self.rest()[1..length].to_owned();
		let is_definition = matches!(
			name.as_str(),
			"def" | "gdef" | "global" | "newcommand" | "renewcommand" | "providecommand"
		);
		if is_definition {
			let start = self.position;
			self.position += length;
			self.has_definition = true;
			return match self.definition(&name) {
				Some(()) => true,
				None => {
					// The malformed definition stays and shows up as an unknown command.
					self.position = start;
					let length = control_sequence_length(self.rest());
					self.copy(length);
					true
				}
			};
		}
		let Some(found) = self.find_macro(&name) else {
			self.copy(length);
			return true;
		};
		self.expand(found, length)
	}

	fn find_macro(&self, name: &str) -> Option<(bool, usize)> {
		let position = |macros: &[Macro]| macros.iter().rposition(|known| known.name == name);
		position(&self.local)
			.map(|index| (true, index))
			.or_else(|| position(&self.global).map(|index| (false, index)))
	}

	fn get_macro(&self, (is_local, index): (bool, usize)) -> &Macro {
		if is_local { &self.local[index] } else { &self.global[index] }
	}

	/// Replaces the use of a macro at the position by its body with the arguments.
	fn expand(&mut self, found: (bool, usize), name_length: usize) -> bool {
		self.expansions += 1;
		if self.expansions > MAX_EXPANSIONS {
			self.fail(TOO_MANY_EXPANSIONS);
			return false;
		}
		let start = self.position;
		let found_macro = self.get_macro(found);
		let (arguments, end) =
			match macro_arguments(&self.text, start + name_length, &found_macro.delimiters) {
				Ok(result) => result,
				Err(message) => {
					self.fail(message);
					return false;
				}
			};
		let mut expansion = substitute(&found_macro.body, &arguments);
		// A body or an argument may end in a command that the next letters would extend.
		let next = self.text[end..].chars().next();
		if next.is_some_and(|next| next.is_ascii_alphabetic()) && ends_in_command_word(&expansion) {
			expansion.push(' ');
		}
		let new_size = self.output.len() + self.text.len() - (end - start) + expansion.len();
		if new_size > MAX_SOURCE_BYTES {
			self.fail(TOO_MANY_EXPANSIONS);
			return false;
		}
		self.text.replace_range(start..end, &expansion);
		true
	}

	fn skip_whitespace(&mut self) {
		let rest = self.rest();
		self.position += rest.len() - rest.trim_start().len();
	}

	/// Reads a definition after its command; `None` if it is malformed.
	fn definition(&mut self, command: &str) -> Option<()> {
		match command {
			"global" => {
				self.skip_whitespace();
				let length =
					self.rest().starts_with('\\').then(|| control_sequence_length(self.rest()));
				let is_definition = length.is_some_and(|length| {
					matches!(&self.rest()[1..length], "def" | "gdef" | "global")
				});
				if !is_definition {
					self.fail("Invalid token after macro prefix");
					return None;
				}
				self.position += length?;
				self.def(true)
			}
			"def" => self.def(false),
			"gdef" => self.def(true),
			_ => self.new_command(command),
		}
	}

	/// `\def\name<parameter text>{body}` after `\def`.
	fn def(&mut self, is_global: bool) -> Option<()> {
		self.skip_whitespace();
		// KaTeX lets any other token be a macro, also a single letter.
		if self.rest().is_empty() || self.rest().starts_with(['{', '}', '$', '&', '#', '^', '_']) {
			self.fail("Expected a control sequence");
			return None;
		}
		let (name, _) = self.macro_name();
		let mut delimiters = vec![String::new()];
		loop {
			let Some(character) = self.rest().chars().next() else {
				self.fail("Expected a macro definition");
				return None;
			};
			if character == '{' {
				break;
			}
			self.position += character.len_utf8();
			if character == '#' {
				let number = self.rest().chars().next().and_then(|digit| digit.to_digit(10));
				let Some(number) = number.filter(|number| (1..=9).contains(number)) else {
					self.fail("Invalid argument number");
					return None;
				};
				if number as usize != delimiters.len() {
					self.fail(format!("Argument number \"{number}\" out of order"));
					return None;
				}
				self.position += 1;
				delimiters.push(String::new());
			} else if !character.is_whitespace()
				&& let Some(delimiter) = delimiters.last_mut()
			{
				delimiter.push(character);
			}
		}
		let Some((body, end)) = braced_group(&self.text, self.position) else {
			self.fail("Expected a macro definition");
			return None;
		};
		let body = body.to_owned();
		self.position = end;
		self.define(Macro { name, delimiters, body }, is_global);
		Some(())
	}

	/// `\newcommand{\name}[count]{body}` after the command.
	fn new_command(&mut self, command: &str) -> Option<()> {
		self.skip_whitespace();
		let is_braced = self.rest().starts_with('{');
		if is_braced {
			self.position += 1;
			self.skip_whitespace();
		}
		if self.rest().is_empty() || self.rest().starts_with('}') {
			self.fail(format!("\\{command}'s first argument must be a macro"));
			return None;
		}
		let (name, shown_name) = self.macro_name();
		self.skip_whitespace();
		if is_braced {
			if !self.rest().starts_with('}') {
				self.fail(format!("\\{command}'s first argument must be a macro"));
				return None;
			}
			self.position += 1;
			self.skip_whitespace();
		}
		// A character always is a symbol for KaTeX.
		let exists = self.find_macro(&name).is_some()
			|| !shown_name.starts_with('\\')
			|| (self.is_known_command)(&name);
		if exists && command == "newcommand" {
			self.fail(format!(
				"\\newcommand{{{shown_name}}} attempting to redefine {shown_name}; use \\renewcommand"
			));
		}
		if !exists && command == "renewcommand" {
			self.fail(format!(
				"\\renewcommand{{{shown_name}}} when command {shown_name} does not yet exist; use \\newcommand"
			));
		}
		let mut parameter_count = 0;
		if self.rest().starts_with('[') {
			let end = self.rest().find(']')?;
			let count = self.rest()[1..end].trim();
			let Some(count) = count.parse::<usize>().ok().filter(|count| *count <= 9) else {
				self.fail(format!("Invalid number of arguments: {count}"));
				return None;
			};
			parameter_count = count;
			self.position += end + 1;
			self.skip_whitespace();
		}
		let (body, end) = match braced_group(&self.text, self.position) {
			Some((body, end)) => (body.to_owned(), end),
			None if self.rest().is_empty() => {
				self.fail("Unexpected end of input in a macro argument, expected '}'");
				return None;
			}
			None => {
				let length = token_length(self.rest());
				(self.rest()[..length].to_owned(), self.position + length)
			}
		};
		self.position = end;
		if !(exists && command == "providecommand") {
			let delimiters = vec![String::new(); parameter_count + 1];
			self.define(Macro { name, delimiters, body }, false);
		}
		Some(())
	}

	/// Reads the token at the position, which is not empty: its key in the macros and how it is
	/// written.
	fn macro_name(&mut self) -> (String, String) {
		let rest = self.rest();
		let length = token_length(rest);
		let shown = rest[..length].to_owned();
		self.position += length;
		let name = match shown.strip_prefix('\\') {
			Some(name) => name.to_owned(),
			None => shown.chars().next().map_or_else(String::new, character_macro_name),
		};
		(name, shown)
	}

	fn define(&mut self, new_macro: Macro, is_global: bool) {
		if is_global {
			self.global.push(new_macro);
		} else {
			self.local.push(new_macro);
		}
	}
}

/// The key of a macro that is a single character, which no control sequence has.
fn character_macro_name(character: char) -> String {
	format!("{character} ")
}

/// A message for the end of the input in KaTeX's format.
fn at_end_of_input(message: &str, text: &str) -> String {
	let count = text.chars().count();
	let shown: String = text.chars().skip(count.saturating_sub(15)).collect();
	let mark = if count > 15 { "…" } else { "" };
	format!("{message} at end of input: {mark}{shown}")
}

/// The length of the control sequence at the start of `text`, which starts with `\`.
fn control_sequence_length(text: &str) -> usize {
	let after = &text[1..];
	let word = after.bytes().take_while(u8::is_ascii_alphabetic).count();
	if word > 0 {
		return 1 + word;
	}
	1 + after.chars().next().map_or(0, char::len_utf8)
}

/// The length of the token at the start of `text`, which is not empty.
fn token_length(text: &str) -> usize {
	match text.starts_with('\\') {
		true => control_sequence_length(text),
		false => text.chars().next().map_or(0, char::len_utf8),
	}
}

/// The offset after the `}` closing the `{` at `from` or, without an opening brace, `from` itself.
fn balanced_end(text: &str, from: usize) -> Option<usize> {
	let mut depth = 0usize;
	let mut characters = text[from..].char_indices();
	while let Some((index, character)) = characters.next() {
		match character {
			'\\' => {
				characters.next();
			}
			'{' => depth += 1,
			'}' => {
				depth = depth.checked_sub(1)?;
				if depth == 0 {
					return Some(from + index + 1);
				}
			}
			_ => {}
		}
	}
	None
}

/// The inside of the group that starts at `from` and the offset after it.
fn braced_group(text: &str, from: usize) -> Option<(&str, usize)> {
	if !text[from..].starts_with('{') {
		return None;
	}
	let end = balanced_end(text, from)?;
	Some((&text[from + 1..end - 1], end))
}

/// The arguments of a macro whose name ends at `from` and the offset after them.
fn macro_arguments(
	text: &str,
	from: usize,
	delimiters: &[String],
) -> Result<(Vec<String>, usize), String> {
	let mismatch = || "Use of the macro doesn't match its definition".to_owned();
	let mut position = from;
	let first = delimiters.first().map_or("", String::as_str);
	if !first.is_empty() {
		let trimmed = text[position..].trim_start();
		position = text.len() - trimmed.len();
		position += first.len();
		if !trimmed.starts_with(first) {
			return Err(mismatch());
		}
	}
	let mut arguments = Vec::with_capacity(delimiters.len().saturating_sub(1));
	for delimiter in delimiters.iter().skip(1) {
		if delimiter.is_empty() {
			let trimmed = text[position..].trim_start();
			position = text.len() - trimmed.len();
			if trimmed.is_empty() {
				let message = "Unexpected end of input in a macro argument, expected '}'";
				return Err(at_end_of_input(message, text));
			}
			if let Some((group, end)) = braced_group(text, position) {
				arguments.push(group.to_owned());
				position = end;
			} else if trimmed.starts_with('}') {
				return Err("Extra }".to_owned());
			} else if trimmed.starts_with('{') {
				let message = "Unexpected end of input in a macro argument, expected '}'";
				return Err(at_end_of_input(message, text));
			} else {
				let length = token_length(trimmed);
				arguments.push(trimmed[..length].to_owned());
				position += length;
			}
		} else {
			let end = delimited_end(text, position, delimiter).ok_or_else(|| {
				let message =
					format!("Unexpected end of input in a macro argument, expected '{delimiter}'");
				at_end_of_input(&message, text)
			})?;
			let argument = text[position..end].trim();
			let is_one_group =
				braced_group(argument, 0).is_some_and(|(_, after)| after == argument.len());
			arguments.push(match is_one_group {
				true => argument[1..argument.len() - 1].to_owned(),
				false => argument.to_owned(),
			});
			position = end + delimiter.len();
		}
	}
	Ok((arguments, position))
}

/// Where `delimiter` next occurs outside of groups.
fn delimited_end(text: &str, from: usize, delimiter: &str) -> Option<usize> {
	let mut depth = 0usize;
	let mut characters = text[from..].char_indices();
	while let Some((index, character)) = characters.next() {
		if depth == 0 && text[from + index..].starts_with(delimiter) {
			return Some(from + index);
		}
		match character {
			'\\' => {
				characters.next();
			}
			'{' => depth += 1,
			'}' => depth = depth.checked_sub(1)?,
			_ => {}
		}
	}
	None
}

/// `body` with `#1` … `#9` replaced by the arguments and `##` by `#`.
fn substitute(body: &str, arguments: &[String]) -> String {
	let mut output = String::with_capacity(body.len());
	let mut characters = body.chars().peekable();
	while let Some(character) = characters.next() {
		if character != '#' {
			output.push(character);
			continue;
		}
		match characters.peek().copied() {
			Some('#') => {
				characters.next();
				output.push('#');
			}
			Some(digit) if digit.is_ascii_digit() => {
				let index = digit.to_digit(10).map_or(0, |number| number as usize);
				match index.checked_sub(1).and_then(|index| arguments.get(index)) {
					Some(argument) => {
						characters.next();
						let starts_with_letter = argument
							.chars()
							.next()
							.is_some_and(|first| first.is_ascii_alphabetic());
						if starts_with_letter && ends_in_command_word(&output) {
							output.push(' ');
						}
						output.push_str(argument);
					}
					None => output.push('#'),
				}
			}
			_ => output.push('#'),
		}
	}
	output
}

/// Whether `text` ends in a control word such as `\alpha`, which is not an escaped backslash.
fn ends_in_command_word(text: &str) -> bool {
	let word_start = text.trim_end_matches(|character: char| character.is_ascii_alphabetic());
	if word_start.len() == text.len() {
		return false;
	}
	let backslashes = word_start.bytes().rev().take_while(|byte| *byte == b'\\').count();
	backslashes % 2 == 1
}

#[cfg(test)]
mod tests {
	use super::*;

	fn expand(latex: &str) -> String {
		expand_macros(latex, &|name| name == "sin").source.into_owned()
	}

	fn error(latex: &str) -> Option<String> {
		expand_macros(latex, &|name| name == "sin").error
	}

	#[test]
	fn leaves_formulas_without_definitions_alone() {
		let formula = "\\defined + \\frac{a}{b}";
		assert!(matches!(expand_macros(formula, &|_| false).source, Cow::Borrowed(_)));
		assert_eq!(expand("\\undefined"), "\\undefined");
	}

	#[test]
	fn expands_definitions() {
		assert_eq!(expand("\\newcommand{\\a}{x}\\a"), "x");
		assert_eq!(expand("\\newcommand\\a{x}\\a+\\a"), "x+x");
		assert_eq!(expand("\\newcommand{\\a}[1]{#1^2}\\a{y}"), "y^2");
		assert_eq!(expand("\\newcommand{\\a}[2]{#2#1}\\a12"), "21");
		assert_eq!(expand("\\renewcommand{\\sin}{x}\\sin"), "x");
		assert_eq!(expand("\\def\\a{x}\\a"), "x");
		assert_eq!(expand("\\def\\a#1{#1^2}\\a{y}"), "y^2");
		assert_eq!(expand("\\def\\a#1.#2;{#2#1}\\a b.c;"), "cb");
		assert_eq!(expand("\\def\\a{\\b}\\def\\b{y}\\a"), "y");
		assert_eq!(expand("\\providecommand{\\sin}{x}\\sin"), "\\sin");
		assert_eq!(expand("\\providecommand{\\a}{x}\\a"), "x");
		assert_eq!(expand("\\def\\a{##}\\a"), "#");
		assert_eq!(expand("\\def a{x}a+a"), "x+x");
		assert_eq!(expand("\\renewcommand{a}{x}a"), "x");
	}

	#[test]
	fn keeps_commands_apart_from_following_letters() {
		assert_eq!(expand("\\def\\a{\\alpha}\\a b"), "\\alpha b");
		assert_eq!(expand("\\def\\a#1{\\b#1}\\a c"), "\\b c");
		assert_eq!(expand("\\def\\a#1{\\\\#1}\\a c"), "\\\\c");
	}

	#[test]
	fn scopes_definitions_to_their_group() {
		assert_eq!(expand("{\\def\\a{x}\\a}\\a"), "{x}\\a");
		assert_eq!(expand("{\\gdef\\a{x}\\a}\\a"), "{x}x");
		assert_eq!(expand("{\\global\\def\\a{x}}\\a"), "{}x");
	}

	#[test]
	fn reports_what_katex_rejects() {
		assert_eq!(error("\\def\\a{\\a}\\a").as_deref(), Some(TOO_MANY_EXPANSIONS));
		assert_eq!(
			error("\\newcommand{\\sin}{x}").as_deref(),
			Some("\\newcommand{\\sin} attempting to redefine \\sin; use \\renewcommand")
		);
		assert_eq!(
			error("\\renewcommand{\\a}{x}").as_deref(),
			Some("\\renewcommand{\\a} when command \\a does not yet exist; use \\newcommand")
		);
		assert_eq!(error("\\def {x}").as_deref(), Some("Expected a control sequence"));
		assert_eq!(
			error("\\newcommand{x}{y}").as_deref(),
			Some("\\newcommand{x} attempting to redefine x; use \\renewcommand")
		);
		assert_eq!(
			error("\\def\\a#1.{x}\\a y").as_deref(),
			Some(
				"Unexpected end of input in a macro argument, expected '.' at end of input: …def\\a#1.{x}\\a y"
			)
		);
		assert_eq!(error("\\def\\a#1{x}\\a}").as_deref(), Some("Extra }"));
		assert_eq!(error("\\def\\a#2{x}").as_deref(), Some("Argument number \"2\" out of order"));
		assert!(error("\\newcommand{xy}{z}").is_some());
		assert!(error("\\newcommand{\\a}[x]{y}").is_some());
		assert!(error("\\def\\a#1{x}\\a").is_some());
		assert!(error("\\def\\a").is_some());
	}

	#[test]
	fn stops_expansions_that_grow() {
		let doubling = format!("\\def\\a#1{{#1#1}}{}x{}", "\\a{".repeat(40), "}".repeat(40));
		let expansion = expand_macros(&doubling, &|_| false);
		assert_eq!(expansion.error.as_deref(), Some(TOO_MANY_EXPANSIONS));
		assert!(expansion.source.len() <= 2 * MAX_SOURCE_BYTES);
		let endless = "\\def\\a{\\a\\a}\\a";
		assert_eq!(expand_macros(endless, &|_| false).error.as_deref(), Some(TOO_MANY_EXPANSIONS));
	}

	#[test]
	fn survives_malformed_definitions() {
		for latex in [
			"\\def",
			"\\def\\",
			"\\global",
			"\\global x",
			"\\newcommand",
			"\\newcommand{",
			"\\newcommand{\\a",
			"\\newcommand{\\a}[",
			"\\newcommand{\\a}[1",
			"\\renewcommand\\",
			"\\def\\a#",
			"\\def\\a#1",
			"\\def\\a{",
			"\\def\\a#1;{x}\\a",
			"\\def\\a;{x}\\a",
			"\\def\\a{x}}\\a",
			"\\def\\ä{x}\\ä",
			"\\def\\a#1{#9}\\a1",
		] {
			let _ = expand(latex);
		}
	}
}
