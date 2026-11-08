//! LaTeX math to MathML, rendered by current browsers and converted to equations by Word.
//!
//! Covers the LaTeX commonly written in Markdown: fractions, roots, scripts and limits, Greek
//! letters and symbols, accents, fonts, `\text`, `\left`/`\right`, matrices, cases and aligned
//! environments, macro definitions and equation numbers. Unknown commands are shown in red.
//! Malformed input never fails: missing arguments stay empty and stray tokens are skipped. The
//! nesting depth is limited because a stack overflow would abort Notepad++.

use super::escape::{is_safe_url, push_escaped_attribute, push_escaped_text};
use super::math_equations::Equations;
use super::math_macros::expand_macros;
use super::math_symbols::{AccentPosition, Symbol, accent, delimiter_command, negated, symbol};

const MAX_DEPTH: usize = 48;
const ERROR_COLOR: &str = "#d1242f";
const FUNCTION_APPLICATION: &str = "<mo>\u{2061}</mo>";
/// `\displaystyle`, `\textstyle`, `\scriptstyle` and `\scriptscriptstyle`, the styles of `\genfrac`.
const STYLE_ATTRIBUTES: [&str; 4] = [
	"displaystyle=\"true\" scriptlevel=\"0\"",
	"displaystyle=\"false\" scriptlevel=\"0\"",
	"displaystyle=\"false\" scriptlevel=\"1\"",
	"displaystyle=\"false\" scriptlevel=\"2\"",
];

/// Writes a formula; `equations` holds the labels and numbers of the document so far.
pub fn push_math(output: &mut String, latex: &str, is_display: bool, equations: &mut Equations) {
	let expansion = expand_macros(latex, &is_known_command);
	let mut converter = Converter::new(&expansion.source, is_display);
	converter.equations = Some(equations);
	let content = converter.top_level();
	output.push_str(match (is_display, converter.has_equation_number) {
		// The number sits at the right end of the line.
		(true, true) => {
			"<math display=\"block\" style=\"width:100%\" xmlns=\"http://www.w3.org/1998/Math/MathML\">"
		}
		(true, false) => "<math display=\"block\" xmlns=\"http://www.w3.org/1998/Math/MathML\">",
		(false, _) => "<math xmlns=\"http://www.w3.org/1998/Math/MathML\">",
	});
	output.push_str(&content);
	output.push_str("</math>");
}

/// `latex` as display math, for callers that treat malformed input like KaTeX's `throwOnError`.
/// The error is the first thing KaTeX would reject, in its words.
pub fn latex_to_mathml(latex: &str) -> Result<String, String> {
	let expansion = expand_macros(latex, &is_known_command);
	let mut converter = Converter::new(&expansion.source, true);
	converter.follows_katex = true;
	let content = converter.top_level();
	match expansion.error.or(converter.first_error) {
		Some(error) => Err(error),
		None => Ok(format!("<math display=\"block\">{content}</math>")),
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
	/// `\name`
	Command(&'a str),
	/// `\` followed by a non-letter, e.g. `\{` or `\\`.
	ControlSymbol(char),
	Character(char),
	BeginGroup,
	EndGroup,
	Superscript,
	Subscript,
	Alignment,
}

const LINE_BREAK: Token = Token::ControlSymbol('\\');

/// Where a sequence of atoms ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
	/// `{...}`
	Group,
	/// `[...]` of `\sqrt[n]`.
	Bracket,
	/// `\left ... \right`
	LeftRight,
	/// A cell of an environment or of the top level, ended by `&` or `\\`.
	Cell,
	/// A cell of `CD`, which an arrow ends too.
	CommutativeCell,
	/// A label of a `CD` arrow up to the given character.
	CommutativeLabel(char),
}

/// Where the rows of [`Converter::rows`] end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowsEnd {
	TopLevel,
	Environment,
	/// The `}` of `\substack`.
	Group,
}

/// What the rows of [`Converter::rows`] do with `\tag` and `\label`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowNumbers {
	/// They stay for the enclosing rows.
	None,
	/// A row shows its `\tag`.
	Tags,
	/// A row shows its `\tag` or, with a `\label`, the next number.
	TagsAndLabels,
}

struct Rows {
	cells: Vec<Vec<String>>,
	/// The number of each row, empty for the empty cell of KaTeX.
	numbers: Vec<Option<String>>,
}

/// `\tag`
struct EquationTag {
	shown: String,
	number: String,
}

impl Context {
	fn ends_at(self, token: Token) -> bool {
		let is_cell_end = matches!(
			token,
			Token::Alignment | LINE_BREAK | Token::Command("end" | "cr" | "newline")
		);
		match self {
			Context::Group => token == Token::EndGroup,
			Context::Bracket => matches!(token, Token::EndGroup | Token::Character(']')),
			Context::LeftRight => {
				token == Token::EndGroup || token == Token::Command("right") || is_cell_end
			}
			Context::Cell => token == Token::EndGroup || is_cell_end,
			Context::CommutativeCell => {
				token == Token::EndGroup || is_cell_end || token == Token::Character('@')
			}
			Context::CommutativeLabel(end) => {
				token == Token::EndGroup
					|| is_cell_end || token == Token::Character('@')
					|| token == Token::Character(end)
			}
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Font {
	Roman,
	Bold,
	Italic,
	BoldItalic,
	SansSerif,
	Monospace,
	Script,
	Fraktur,
	DoubleStruck,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Limits {
	/// Scripts are always written as sub- and superscripts.
	Never,
	/// Above and below in display math, like `\sum`.
	InDisplay,
	/// Always above and below, like `\overbrace`.
	Always,
}

struct Atom {
	mathml: String,
	limits: Limits,
}

impl Atom {
	fn new(mathml: String) -> Self {
		Self { mathml, limits: Limits::Never }
	}
}

#[derive(Clone, Copy)]
enum ColumnAlignment {
	Center,
	Left,
	/// Alternating right and left, like `aligned`.
	RightLeft,
}

struct Converter<'a> {
	source: &'a str,
	/// Byte offset of the next unread character.
	position: usize,
	depth: usize,
	is_display: bool,
	font: Option<Font>,
	/// The first malformed input, which [`push_math`] tolerates.
	first_error: Option<String>,
	/// Writes the characters and spaces of KaTeX's MathML, as a browser's layout depends on them.
	follows_katex: bool,
	/// The labels and numbers of a document; KaTeX has neither `\label` nor `\ref`.
	equations: Option<&'a mut Equations>,
	/// Whether the whole formula is display math, unlike `is_display`, which changes inside of it.
	is_display_formula: bool,
	/// Inside of an environment that numbers its rows, which then gets `\tag` and `\label`.
	is_in_numbered_rows: bool,
	has_equation_number: bool,
	pending_tag: Option<EquationTag>,
	pending_labels: Vec<&'a str>,
	/// `\nonumber` in the current row.
	is_row_unnumbered: bool,
}

impl<'a> Converter<'a> {
	fn new(source: &'a str, is_display: bool) -> Self {
		Self {
			source,
			position: 0,
			depth: 0,
			is_display,
			font: None,
			first_error: None,
			follows_katex: false,
			equations: None,
			is_display_formula: is_display,
			is_in_numbered_rows: false,
			has_equation_number: false,
			pending_tag: None,
			pending_labels: Vec::new(),
			is_row_unnumbered: false,
		}
	}

	/// Notes an error at the source range `start..end`, formatted like KaTeX's `ParseError`.
	fn fail(&mut self, message: &str, start: usize, end: usize) {
		if self.first_error.is_some() {
			return;
		}
		let source = self.source;
		let (start, end) = (start.min(source.len()), end.min(source.len()));
		let (before, after) = (source.get(..start).unwrap_or(""), source.get(end..).unwrap_or(""));
		let underlined: String = source
			.get(start..end)
			.unwrap_or("")
			.chars()
			.flat_map(|character| [character, '\u{332}'])
			.collect();
		let before_count = before.chars().count();
		let left: String = before.chars().skip(before_count.saturating_sub(15)).collect();
		let left_mark = if before_count > 15 { "…" } else { "" };
		let right: String = after.chars().take(15).collect();
		let right_mark = if after.chars().count() > 15 { "…" } else { "" };
		let place = match start == source.len() {
			true => "at end of input".to_owned(),
			false => format!("at position {}", before_count + 1),
		};
		self.first_error =
			Some(format!("{message} {place}: {left_mark}{left}{underlined}{right}{right_mark}"));
	}

	/// Notes an unexpected next token, or the end of the input.
	fn fail_expected(&mut self, expected: &str) {
		self.skip_whitespace_and_comments();
		let start = self.position;
		let found = self.peek_token().map(token_text);
		let position = self.position;
		self.next_token();
		let end = self.position;
		self.position = position;
		let message = format!("Expected '{expected}', got '{}'", found.as_deref().unwrap_or("EOF"));
		self.fail(&message, start, end);
	}

	fn fail_too_deep(&mut self) {
		let end = self.source.len();
		self.fail("Too many nested groups", end, end);
	}

	fn rest(&self) -> &'a str {
		self.source.get(self.position..).unwrap_or("")
	}

	fn skip_whitespace_and_comments(&mut self) {
		loop {
			let rest = self.rest();
			let trimmed = rest.trim_start();
			self.position += rest.len() - trimmed.len();
			if !trimmed.starts_with('%') {
				return;
			}
			self.position += trimmed.find('\n').unwrap_or(trimmed.len());
		}
	}

	fn next_token(&mut self) -> Option<Token<'a>> {
		self.skip_whitespace_and_comments();
		let rest = self.rest();
		let mut characters = rest.chars();
		let first = characters.next()?;
		self.position += first.len_utf8();
		Some(match first {
			'\\' => {
				let after_backslash = &rest[1..];
				let name_length =
					after_backslash.bytes().take_while(u8::is_ascii_alphabetic).count();
				if name_length > 0 {
					self.position += name_length;
					Token::Command(&after_backslash[..name_length])
				} else if let Some(symbol) = characters.next() {
					self.position += symbol.len_utf8();
					Token::ControlSymbol(symbol)
				} else {
					Token::Character('\\')
				}
			}
			'{' => Token::BeginGroup,
			'}' => Token::EndGroup,
			'^' => Token::Superscript,
			'_' => Token::Subscript,
			'&' => Token::Alignment,
			character => Token::Character(character),
		})
	}

	fn peek_token(&mut self) -> Option<Token<'a>> {
		let position = self.position;
		let token = self.next_token();
		self.position = position;
		token
	}

	/// Enters one nesting level. At the limit the rest of the input is dropped, so that every
	/// caller returns without going deeper.
	fn enter(&mut self) -> bool {
		if self.depth >= MAX_DEPTH {
			self.position = self.source.len();
			self.fail_too_deep();
			return false;
		}
		self.depth += 1;
		true
	}

	fn leave(&mut self) {
		self.depth = self.depth.saturating_sub(1);
	}

	fn top_level(&mut self) -> String {
		let row_numbers = match (self.equations.is_some(), self.is_display_formula) {
			(true, true) => RowNumbers::TagsAndLabels,
			_ => RowNumbers::None,
		};
		let mut rows = self.rows(RowsEnd::TopLevel, row_numbers);
		let has_numbers = rows.numbers.iter().any(Option::is_some);
		let content = if rows.cells.len() == 1 && rows.cells[0].len() == 1 && !has_numbers {
			rows.cells.pop().and_then(|mut row| row.pop()).unwrap_or_default()
		} else if self.follows_katex {
			// KaTeX's `\\` is a space of no width outside of environments, so there is one line.
			let cells: Vec<String> = rows.cells.into_iter().flatten().collect();
			format!("<mrow>{}</mrow>", cells.join("<mspace/>"))
		} else {
			let has_columns = rows.cells.iter().any(|row| row.len() > 1);
			let alignment =
				if has_columns { ColumnAlignment::RightLeft } else { ColumnAlignment::Center };
			table(&rows, &[], alignment, true)
		};
		match self.pending_tag.take() {
			// KaTeX tags the whole formula.
			Some(tag) if self.follows_katex => format!(
				"<mtable style=\"width:100%\"><mtr><mtd style=\"width:50%\"></mtd><mtd>{content}</mtd><mtd style=\"width:50%\"></mtd><mtd>{}</mtd></mtr></mtable>",
				text_element(&tag.shown, "")
			),
			_ => content,
		}
	}

	/// Cells separated by `&` and rows by `\\`, up to the given end.
	fn rows(&mut self, end: RowsEnd, row_numbers: RowNumbers) -> Rows {
		let is_environment = end == RowsEnd::Environment;
		let mut rows = vec![Vec::new()];
		let mut numbers = Vec::new();
		loop {
			let cell = self.sequence(Context::Cell);
			if let Some(row) = rows.last_mut() {
				row.push(cell);
			}
			match self.peek_token() {
				None => break,
				Some(Token::Alignment) => {
					if end == RowsEnd::TopLevel {
						self.fail_expected("EOF");
					}
					self.next_token();
				}
				Some(LINE_BREAK | Token::Command("cr" | "newline")) => {
					self.next_token();
					self.skip_optional_bracket();
					numbers.push(self.row_number(row_numbers));
					rows.push(Vec::new());
				}
				Some(Token::Command("end")) if is_environment => break,
				Some(Token::EndGroup) if end == RowsEnd::Group => break,
				Some(Token::Command("end")) => {
					self.fail_expected("EOF");
					self.next_token();
					let name = self.raw_argument().unwrap_or("");
					if let Some(row) = rows.last_mut() {
						row.push(error_text(&format!("\\end{{{name}}}")));
					}
				}
				// A stray `}` or anything else the cell stopped at.
				Some(_) => {
					self.fail_expected(if is_environment { "\\end" } else { "EOF" });
					self.next_token();
				}
			}
		}
		// `\\` after the last row does not start a new one.
		if rows.len() > 1 && rows.last().is_some_and(|row| row.iter().all(String::is_empty)) {
			rows.pop();
		} else {
			numbers.push(self.row_number(row_numbers));
		}
		Rows { cells: rows, numbers }
	}

	/// The number of the row that just ended, as the content of its cell.
	fn row_number(&mut self, row_numbers: RowNumbers) -> Option<String> {
		if row_numbers == RowNumbers::None {
			return None;
		}
		let is_unnumbered = std::mem::take(&mut self.is_row_unnumbered);
		let tag = self.pending_tag.take();
		if self.follows_katex {
			// `\tag` draws nothing here: KaTeX's MathML has an empty cell for the number.
			let is_numbered = row_numbers == RowNumbers::TagsAndLabels && !is_unnumbered;
			return (is_numbered || tag.is_some()).then(String::new);
		}
		let labels = std::mem::take(&mut self.pending_labels);
		let tag = match tag {
			Some(tag) => tag,
			None if !labels.is_empty()
				&& !is_unnumbered
				&& row_numbers == RowNumbers::TagsAndLabels =>
			{
				let number = self.equations.as_mut()?.next_number();
				EquationTag { shown: format!("({number})"), number }
			}
			None => return None,
		};
		let equations = self.equations.as_mut()?;
		for label in labels {
			equations.set_number(label, &tag.number);
		}
		self.has_equation_number = true;
		Some(text_element(&tag.shown, ""))
	}

	/// Atoms up to the end of `context`, which is not consumed.
	fn sequence(&mut self, context: Context) -> String {
		let mut items = String::new();
		while let Some(token) = self.peek_token() {
			if context.ends_at(token) {
				break;
			}
			match token {
				Token::Command(command @ ("over" | "choose" | "atop" | "brace" | "brack")) => {
					self.next_token();
					let denominator = self.nested_sequence(context);
					return infix_fraction(command, &items, &denominator);
				}
				Token::Command("above") => {
					self.next_token();
					let thickness = self.dimension().map_or_else(String::new, |thickness| {
						format!(" linethickness=\"{}\"", line_thickness(&thickness))
					});
					let denominator = self.nested_sequence(context);
					return format!(
						"<mfrac{thickness}><mrow>{items}</mrow><mrow>{denominator}</mrow></mfrac>"
					);
				}
				Token::Command(
					command @ ("displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle"),
				) => {
					self.next_token();
					let rest = self.with_display(command == "displaystyle", |converter| {
						converter.nested_sequence(context)
					});
					let attributes = match command {
						"displaystyle" => STYLE_ATTRIBUTES[0],
						"textstyle" => STYLE_ATTRIBUTES[1],
						"scriptstyle" => STYLE_ATTRIBUTES[2],
						_ => STYLE_ATTRIBUTES[3],
					};
					items.push_str(&format!("<mstyle {attributes}>{rest}</mstyle>"));
					break;
				}
				Token::Command("color") => {
					self.next_token();
					let color = self.raw_argument().unwrap_or("");
					let rest = self.nested_sequence(context);
					items.push_str(&with_color(color, &rest));
					break;
				}
				Token::Command(command @ ("rm" | "bf" | "it" | "sf" | "tt" | "cal")) => {
					self.next_token();
					let font = old_style_font(command);
					let rest = self.with_font(font, |converter| converter.nested_sequence(context));
					items.push_str(&rest);
					break;
				}
				_ => {
					let atom = match token {
						// A script without a base.
						Token::Superscript | Token::Subscript => Some(Atom::new(String::new())),
						_ => {
							self.next_token();
							self.atom(token, false)
						}
					};
					if let Some(atom) = atom {
						let with_scripts = self.with_scripts(atom);
						items.push_str(&with_scripts);
					}
				}
			}
		}
		items
	}

	fn nested_sequence(&mut self, context: Context) -> String {
		if !self.enter() {
			return error_text("…");
		}
		let sequence = self.sequence(context);
		self.leave();
		sequence
	}

	/// Converts in display style or not, which decides where limits go.
	fn with_display<T>(&mut self, is_display: bool, convert: impl FnOnce(&mut Self) -> T) -> T {
		let outer = std::mem::replace(&mut self.is_display, is_display);
		let converted = convert(self);
		self.is_display = outer;
		converted
	}

	fn with_font(&mut self, font: Font, convert: impl FnOnce(&mut Self) -> String) -> String {
		let outer_font = self.font.replace(font);
		let converted = convert(self);
		self.font = outer_font;
		converted
	}

	/// One argument: a group or a single token. Numbers are single digits, as in `\frac12`.
	fn argument(&mut self) -> String {
		if !self.enter() {
			return error_text("…");
		}
		let argument = match self.peek_token() {
			Some(Token::BeginGroup) => {
				self.next_token();
				self.group_content()
			}
			Some(
				Token::EndGroup
				| Token::Alignment
				| LINE_BREAK
				| Token::Superscript
				| Token::Subscript
				| Token::Command("end" | "right"),
			) => {
				self.fail_expected("group");
				"<mrow></mrow>".to_owned()
			}
			None => {
				let end = self.source.len();
				self.fail("Unexpected end of input in a macro argument, expected '}'", end, end);
				"<mrow></mrow>".to_owned()
			}
			Some(token) => {
				self.next_token();
				self.atom(token, true).map(|atom| atom.mathml).unwrap_or_default()
			}
		};
		self.leave();
		argument
	}

	/// The rest of a group whose `{` was read, as one `mrow`.
	fn group_content(&mut self) -> String {
		let content = self.sequence(Context::Group);
		if self.peek_token() == Some(Token::EndGroup) {
			self.next_token();
		} else {
			self.fail_expected("}");
		}
		format!("<mrow>{content}</mrow>")
	}

	/// `None` for tokens that produce nothing, such as a stray `&`.
	fn atom(&mut self, token: Token<'a>, is_argument: bool) -> Option<Atom> {
		Some(Atom::new(match token {
			Token::Character(character) => self.character(character, is_argument),
			Token::BeginGroup => {
				if !self.enter() {
					return Some(Atom::new(error_text("…")));
				}
				let group = self.group_content();
				self.leave();
				group
			}
			Token::ControlSymbol(control_symbol) => match control_symbol {
				'{' => fence("{", false),
				'}' => fence("}", false),
				'|' if self.follows_katex => upright_identifier("∥"),
				'|' => fence("‖", false),
				'%' | '&' | '#' | '$' | '_' if self.follows_katex => {
					upright_identifier(&control_symbol.to_string())
				}
				'%' | '&' | '#' | '$' | '_' => operator_element(&control_symbol.to_string()),
				'\\' => return None,
				_ => match symbol(&control_symbol.to_string()) {
					Some(Symbol::Space(width)) => self.space(width),
					_ => {
						let end = self.position;
						let start = end.saturating_sub(1 + control_symbol.len_utf8());
						let text = format!("\\{control_symbol}");
						let message = format!("Undefined control sequence: {text}");
						self.fail(&message, start, end);
						error_text(&text)
					}
				},
			},
			Token::Command(name) => return self.command(name),
			Token::EndGroup | Token::Alignment | Token::Superscript | Token::Subscript => {
				return None;
			}
		}))
	}

	fn character(&mut self, character: char, is_argument: bool) -> String {
		match character {
			'0'..='9' => {
				let mut number = character.to_string();
				if !is_argument {
					number.push_str(self.take_number_rest());
				}
				let number: String = number.chars().map(|digit| self.styled(digit)).collect();
				let mut mathml = String::from("<mn>");
				push_escaped_text(&mut mathml, &number);
				mathml.push_str("</mn>");
				mathml
			}
			'#' if self.follows_katex => {
				let end = self.position;
				self.fail("Expected 'EOF', got '#'", end.saturating_sub(1), end);
				operator_element("#")
			}
			'~' => "<mtext>\u{a0}</mtext>".to_owned(),
			'\'' => operator_element("′"),
			'-' => operator_element("−"),
			'*' => operator_element("∗"),
			'|' if self.follows_katex => upright_identifier("∣"),
			'(' | ')' | '[' | ']' | '|' | '/' => fence(&character.to_string(), false),
			_ if character.is_alphabetic() => self.identifier(character),
			_ => operator_element(&character.to_string()),
		}
	}

	/// The digits and inner decimal points after the first digit of a number.
	fn take_number_rest(&mut self) -> &'a str {
		let rest = self.rest();
		let bytes = rest.as_bytes();
		let mut length = 0;
		while let Some(&byte) = bytes.get(length) {
			let is_decimal_point =
				byte == b'.' && bytes.get(length + 1).is_some_and(u8::is_ascii_digit);
			if !byte.is_ascii_digit() && !is_decimal_point {
				break;
			}
			length += 1;
		}
		self.position += length;
		&rest[..length]
	}

	fn identifier(&self, letter: char) -> String {
		let mut mathml = String::from(match self.font {
			Some(Font::Roman) => "<mi mathvariant=\"normal\">",
			_ => "<mi>",
		});
		mathml.push(self.styled(letter));
		mathml.push_str("</mi>");
		mathml
	}

	/// `character` in the current font, using the Mathematical Alphanumeric Symbols.
	fn styled(&self, character: char) -> char {
		self.font.map_or(character, |font| styled_character(character, font))
	}

	fn command(&mut self, name: &'a str) -> Option<Atom> {
		let mathml = match name {
			"frac" | "dfrac" | "tfrac" | "cfrac" => {
				let is_display = matches!(name, "dfrac" | "cfrac");
				let numerator = self.with_display(is_display, Self::argument);
				let denominator = self.with_display(is_display, Self::argument);
				let fraction = format!("<mfrac>{numerator}{denominator}</mfrac>");
				match name {
					"dfrac" | "cfrac" => {
						format!("<mstyle displaystyle=\"true\">{fraction}</mstyle>")
					}
					"tfrac" => format!("<mstyle displaystyle=\"false\">{fraction}</mstyle>"),
					_ => fraction,
				}
			}
			"binom" | "dbinom" | "tbinom" => {
				let is_display = name == "dbinom";
				let top = self.with_display(is_display, Self::argument);
				let bottom = self.with_display(is_display, Self::argument);
				let binomial = format!(
					"<mrow>{}<mfrac linethickness=\"0\">{top}{bottom}</mfrac>{}</mrow>",
					fence("(", true),
					fence(")", true)
				);
				match name {
					"dbinom" => format!("<mstyle displaystyle=\"true\">{binomial}</mstyle>"),
					"tbinom" => format!("<mstyle displaystyle=\"false\">{binomial}</mstyle>"),
					_ => binomial,
				}
			}
			"sqrt" => {
				let index = self.optional_bracket();
				let radicand = self.argument();
				match index {
					Some(index) => format!("<mroot>{radicand}<mrow>{index}</mrow></mroot>"),
					None => format!("<msqrt>{radicand}</msqrt>"),
				}
			}
			"left" => self.left_right(),
			"right" | "middle" => {
				let end = self.position;
				let message = format!("Expected 'EOF', got '\\{name}'");
				self.fail(&message, end.saturating_sub(name.len() + 1), end);
				let delimiter = self.delimiter().unwrap_or("");
				fence(delimiter, true)
			}
			"big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl" | "biggl" | "Biggl" | "bigr"
			| "Bigr" | "biggr" | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => {
				let size = match name.trim_end_matches(['l', 'r', 'm']) {
					"big" => "1.2em",
					"Big" => "1.8em",
					"bigg" => "2.4em",
					_ => "3em",
				};
				let delimiter = self.delimiter().unwrap_or("");
				let mut mathml = format!(
					"<mo fence=\"true\" stretchy=\"true\" minsize=\"{size}\" maxsize=\"{size}\">"
				);
				push_escaped_text(&mut mathml, delimiter);
				mathml.push_str("</mo>");
				mathml
			}
			"text" | "textrm" | "textnormal" | "textup" | "textmd" | "mbox" | "hbox" => {
				text_element(self.raw_argument().unwrap_or(""), "")
			}
			"textbf" => text_element(
				self.raw_argument().unwrap_or(""),
				" mathvariant=\"bold\" style=\"font-weight:bold\"",
			),
			"textit" | "emph" => text_element(
				self.raw_argument().unwrap_or(""),
				" mathvariant=\"italic\" style=\"font-style:italic\"",
			),
			"texttt" => text_element(
				self.raw_argument().unwrap_or(""),
				" mathvariant=\"monospace\" style=\"font-family:monospace\"",
			),
			"textsf" => text_element(
				self.raw_argument().unwrap_or(""),
				" mathvariant=\"sans-serif\" style=\"font-family:sans-serif\"",
			),
			"mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "mathcal" | "mathscr"
			| "mathfrak" | "mathbb" | "boldsymbol" | "bm" | "mathbfit" | "mathnormal" | "Bbb"
			| "bold" | "frak" => {
				let font = match name {
					"mathrm" => Font::Roman,
					"mathbf" | "bold" => Font::Bold,
					"mathit" | "mathnormal" => Font::Italic,
					"mathsf" => Font::SansSerif,
					"mathtt" => Font::Monospace,
					"mathcal" | "mathscr" => Font::Script,
					"mathfrak" | "frak" => Font::Fraktur,
					"mathbb" | "Bbb" => Font::DoubleStruck,
					_ => Font::BoldItalic,
				};
				self.with_font(font, Self::argument)
			}
			"pmb" => {
				format!(
					"<mstyle style=\"text-shadow: 0.02em 0.01em 0.04px\">{}</mstyle>",
					self.argument()
				)
			}
			"operatorname" | "operatornamewithlimits" => {
				let is_starred = self.rest().starts_with('*');
				if is_starred {
					self.position += 1;
				}
				let operator_name = self.raw_argument().unwrap_or("").trim().trim_end_matches('*');
				let mut mathml = String::from("<mi>");
				push_escaped_text(&mut mathml, operator_name);
				mathml.push_str("</mi>");
				let limits = match (name, is_starred) {
					("operatornamewithlimits", _) => Limits::InDisplay,
					(_, true) => {
						mathml.push_str(FUNCTION_APPLICATION);
						Limits::InDisplay
					}
					_ => {
						mathml.push_str(FUNCTION_APPLICATION);
						Limits::Never
					}
				};
				return Some(Atom { mathml, limits });
			}
			"begin" => self.environment(),
			"end" => {
				let environment = self.raw_argument().unwrap_or("");
				error_text(&format!("\\end{{{environment}}}"))
			}
			"overset" | "stackrel" | "underset" => {
				let script = self.argument();
				let base = self.argument();
				match name {
					"underset" => format!("<munder>{base}{script}</munder>"),
					_ => format!("<mover>{base}{script}</mover>"),
				}
			}
			"genfrac" => self.general_fraction(),
			"substack" => self.substack(),
			"smash" => {
				let edges = self.raw_optional_bracket().unwrap_or("tb");
				let height = if edges.contains('t') { " height=\"0px\"" } else { "" };
				let depth = if edges.contains('b') { " depth=\"0px\"" } else { "" };
				format!("<mpadded{height}{depth}>{}</mpadded>", self.argument())
			}
			"raisebox" => {
				let offset = self.dimension().unwrap_or_else(|| "0em".to_owned());
				let text = text_element(self.raw_argument().unwrap_or(""), "");
				format!(
					"<mpadded voffset=\"{offset}\"><mstyle {}>{text}</mstyle></mpadded>",
					STYLE_ATTRIBUTES[1]
				)
			}
			"rule" => self.rule(),
			"llap" | "rlap" | "clap" => {
				let content = text_element(self.raw_argument().unwrap_or(""), "");
				self.overlap(name, &content)
			}
			"mathllap" | "mathrlap" | "mathclap" => {
				let content = self.argument();
				self.overlap(name, &content)
			}
			"colorbox" | "fcolorbox" => {
				let frame = match name {
					"fcolorbox" => valid_color(self.raw_argument().unwrap_or("")),
					_ => None,
				};
				let background = valid_color(self.raw_argument().unwrap_or(""));
				let text = text_element(self.raw_argument().unwrap_or(""), "");
				color_box(frame, background, &text)
			}
			"smallint" => {
				let mathml = "<mo largeop=\"false\">∫</mo>".to_owned();
				let limits = if self.follows_katex { Limits::InDisplay } else { Limits::Never };
				return Some(Atom { mathml, limits });
			}
			"varlimsup" => {
				let mathml =
					"<mover accent=\"true\"><mi>lim</mi><mo stretchy=\"true\">‾</mo></mover>";
				return Some(Atom { mathml: mathml.to_owned(), limits: Limits::InDisplay });
			}
			"varliminf" => {
				let mathml = "<munder accentunder=\"true\"><mi>lim</mi><mo stretchy=\"true\">_</mo></munder>";
				return Some(Atom { mathml: mathml.to_owned(), limits: Limits::InDisplay });
			}
			"xrightarrow" | "xleftarrow" | "xleftrightarrow" | "xRightarrow" | "xLeftarrow"
			| "xLeftrightarrow" | "xmapsto" | "xhookrightarrow" | "xhookleftarrow"
			| "xrightleftharpoons" | "xlongequal" => {
				let below = self.optional_bracket();
				let above = self.argument();
				let arrow = format!(
					"<mo stretchy=\"true\" minsize=\"2em\">{}</mo>",
					extensible_arrow(name)
				);
				match below {
					Some(below) => {
						format!("<munderover>{arrow}<mrow>{below}</mrow>{above}</munderover>")
					}
					None => format!("<mover>{arrow}{above}</mover>"),
				}
			}
			"boxed" | "fbox" => {
				let content = match name {
					"fbox" => text_element(self.raw_argument().unwrap_or(""), ""),
					_ => self.argument(),
				};
				format!(
					"<menclose notation=\"box\" style=\"border:1px solid;padding:0.2em\">{content}</menclose>"
				)
			}
			"cancel" | "bcancel" | "xcancel" | "sout" => {
				// Browsers ignore `notation`: the style draws the lines.
				let (notation, directions): (_, &[_]) = match name {
					"cancel" => ("updiagonalstrike", &["to bottom right"]),
					"bcancel" => ("downdiagonalstrike", &["to top right"]),
					"xcancel" => (
						"updiagonalstrike downdiagonalstrike",
						&["to bottom right", "to top right"],
					),
					_ => ("horizontalstrike", &["to bottom"]),
				};
				let lines: Vec<String> =
					directions.iter().map(|direction| strike_line(direction)).collect();
				let content = self.argument();
				format!(
					"<menclose notation=\"{notation}\" style=\"background:{}\">{content}</menclose>",
					lines.join(",")
				)
			}
			"phantom" => format!("<mphantom>{}</mphantom>", self.argument()),
			"hphantom" => format!(
				"<mpadded height=\"0px\" depth=\"0px\"><mphantom>{}</mphantom></mpadded>",
				self.argument()
			),
			"vphantom" => {
				format!("<mpadded width=\"0px\"><mphantom>{}</mphantom></mpadded>", self.argument())
			}
			"textcolor" => {
				let color = self.raw_argument().unwrap_or("");
				let content = self.argument();
				with_color(color, &content)
			}
			"color" => {
				// Only reached as an argument, where it has nothing to color.
				self.raw_argument();
				return None;
			}
			"hspace" | "mspace" | "kern" | "mkern" | "hskip" | "mskip" => {
				self.dimension().map(|width| space(&width)).unwrap_or_default()
			}
			"not" => self.negation(),
			"pmod" | "pod" => {
				let argument = self.argument();
				let word = match name {
					"pmod" => "<mi>mod</mi><mspace width=\"0.3333em\"/>",
					_ => "",
				};
				format!(
					"<mspace width=\"1em\"/>{}{word}{argument}{}",
					fence("(", false),
					fence(")", false)
				)
			}
			"bmod" => "<mo lspace=\"0.2222em\" rspace=\"0.2222em\">mod</mo>".to_owned(),
			"mod" => "<mspace width=\"1em\"/><mi>mod</mi><mspace width=\"0.3333em\"/>".to_owned(),
			"mathop" | "mathbin" | "mathrel" | "mathord" | "mathopen" | "mathclose"
			| "mathpunct" | "mathinner" => self.argument(),
			"href" => {
				let url = self.raw_argument().unwrap_or("").trim();
				let content = self.argument();
				self.link(url, content)
			}
			"tag" => {
				self.tag();
				return None;
			}
			"label" if self.equations.is_some() => return self.label(),
			"ref" | "eqref" if self.equations.is_some() => {
				let label = self.raw_argument().unwrap_or("").trim();
				self.equations.as_mut()?.reference(label, name == "eqref")
			}
			"nonumber" | "notag" => {
				self.is_row_unnumbered = true;
				return None;
			}
			"cline" => {
				self.raw_argument();
				return None;
			}
			"limits" | "nolimits" | "displaylimits" | "hline" | "hdashline" | "relax" | "strut"
			| "mathstrut" | "nobreak" | "allowbreak" => return None,
			_ => return Some(self.accent_or_symbol(name)),
		};
		Some(Atom::new(mathml))
	}

	/// `\href`: browsers link only HTML elements, which are allowed in `mtext`. The layout of labels
	/// in SVG takes the link from `mrow`.
	fn link(&self, url: &str, content: String) -> String {
		if !is_safe_url(url, false) {
			return content;
		}
		let mut href = String::new();
		push_escaped_attribute(&mut href, url);
		if self.follows_katex {
			return format!("<mrow href=\"{href}\">{content}</mrow>");
		}
		if !self.equations.as_ref().is_some_and(|equations| equations.writes_links()) {
			return content;
		}
		format!(
			"<mtext><a href=\"{href}\"><math displaystyle=\"{}\" xmlns=\"http://www.w3.org/1998/Math/MathML\">{content}</math></a></mtext>",
			self.is_display
		)
	}

	/// `\tag{…}` and `\tag*{…}` for the row in progress.
	fn tag(&mut self) {
		let is_starred = self.rest().starts_with('*');
		if is_starred {
			self.position += 1;
		}
		let start = self.position;
		let text = self.raw_argument().unwrap_or("");
		if self.pending_tag.is_some() {
			if self.first_error.is_none() {
				self.first_error = Some("Multiple \\tag".to_owned());
			}
			return;
		}
		if !self.is_display_formula {
			self.fail("\\tag works only in display equations", start, self.position);
			return;
		}
		let number = plain_text(text);
		let shown = if is_starred { number.clone() } else { format!("({number})") };
		self.pending_tag = Some(EquationTag { shown, number });
	}

	/// `\label{name}`: an anchor, whose number comes with the end of its row.
	fn label(&mut self) -> Option<Atom> {
		let name = self.raw_argument().unwrap_or("").trim();
		if name.is_empty() {
			return None;
		}
		let equations = self.equations.as_mut()?;
		if !equations.add_label(name) {
			return Some(Atom::new(error_text(&format!("\\label{{{name}}}"))));
		}
		let writes_links = equations.writes_links();
		self.pending_labels.push(name);
		if !writes_links {
			return None;
		}
		let mut anchor = String::from("<mspace id=\"");
		push_escaped_attribute(&mut anchor, name);
		// The anchor is on the baseline, which would leave the formula above a jump's position.
		anchor.push_str("\" style=\"scroll-margin-top:5em\"/>");
		Some(Atom::new(anchor))
	}

	/// `\genfrac` after its name: delimiters, line thickness, style, numerator and denominator.
	fn general_fraction(&mut self) -> String {
		let open = self.delimiter_argument();
		let close = self.delimiter_argument();
		let thickness = css_length(self.raw_argument().unwrap_or("").trim());
		let style = self.raw_argument().unwrap_or("").trim();
		let style_index =
			style.parse::<usize>().ok().filter(|index| *index < STYLE_ATTRIBUTES.len());
		let thickness = thickness.map_or_else(String::new, |thickness| {
			let thickness = scaled_line_thickness(&thickness, style_index.unwrap_or(0));
			format!(" linethickness=\"{thickness}\"")
		});
		let is_display = style_index.map_or(self.is_display, |index| index == 0);
		let numerator = self.with_display(is_display, Self::argument);
		let denominator = self.with_display(is_display, Self::argument);
		let fraction = format!("<mfrac{thickness}>{numerator}{denominator}</mfrac>");
		let with_fences = match open.is_empty() && close.is_empty() {
			true => fraction,
			false => {
				format!(
					"<mrow>{}{fraction}{}</mrow>",
					self.tall_fence(open),
					self.tall_fence(close)
				)
			}
		};
		match style_index {
			Some(index) => format!("<mstyle {}>{with_fences}</mstyle>", STYLE_ATTRIBUTES[index]),
			None => with_fences,
		}
	}

	/// A delimiter alone or in a group, where `{}` is none.
	fn delimiter_argument(&mut self) -> &'static str {
		if self.peek_token() != Some(Token::BeginGroup) {
			return self.delimiter().unwrap_or("");
		}
		self.next_token();
		let delimiter = self.delimiter().unwrap_or("");
		if self.peek_token() == Some(Token::EndGroup) {
			self.next_token();
		} else {
			self.fail_expected("}");
		}
		delimiter
	}

	/// `\substack{…}`: rows like those of an environment, in the size of scripts.
	fn substack(&mut self) -> String {
		if self.peek_token() != Some(Token::BeginGroup) {
			self.fail_expected("group");
			return "<mrow></mrow>".to_owned();
		}
		self.next_token();
		if !self.enter() {
			return error_text("…");
		}
		let rows =
			self.with_display(false, |converter| converter.rows(RowsEnd::Group, RowNumbers::None));
		self.leave();
		if self.peek_token() == Some(Token::EndGroup) {
			self.next_token();
		} else {
			self.fail_expected("}");
		}
		if rows.cells.iter().any(|row| row.len() > 1) && self.first_error.is_none() {
			self.first_error = Some("{subarray} can contain only one column".to_owned());
		}
		let mut mathml = String::from(
			"<mstyle scriptlevel=\"1\"><mtable rowspacing=\"0.1em\" columnalign=\"center\" columnspacing=\"1em\">",
		);
		for row in &rows.cells {
			mathml.push_str("<mtr>");
			for cell in row {
				mathml.push_str(&format!(
					"<mtd><mstyle scriptlevel=\"1\" displaystyle=\"false\">{cell}</mstyle></mtd>"
				));
			}
			mathml.push_str("</mtr>");
		}
		mathml.push_str("</mtable></mstyle>");
		mathml
	}

	/// `\rule[raise]{width}{height}`
	fn rule(&mut self) -> String {
		let raise = self.raw_optional_bracket().and_then(|raise| css_length(raise.trim()));
		let width = self.dimension().unwrap_or_else(|| "0em".to_owned());
		let height = self.dimension().unwrap_or_else(|| "0em".to_owned());
		let raise_or_zero = raise.as_deref().unwrap_or("0em");
		match self.follows_katex {
			// KaTeX's box has the height of the raise only, so the rule overflows it. Browsers
			// refuse a negative height, which then is that of the rule.
			true => {
				let depth = raise_or_zero
					.strip_prefix('-')
					.map_or_else(String::new, |depth| format!(" depth=\"{depth}\""));
				format!(
					"<mpadded height=\"{raise_or_zero}\"{depth} voffset=\"{raise_or_zero}\"><mspace mathbackground=\"black\" width=\"{width}\" height=\"{height}\"/></mpadded>"
				)
			}
			false => format!(
				"<mpadded voffset=\"{raise_or_zero}\"><mspace mathbackground=\"currentColor\" width=\"{width}\" height=\"{height}\"/></mpadded>"
			),
		}
	}

	/// `\llap`, `\rlap`, `\clap` and their math forms: no width, the content to the left of the
	/// position, to its right or around it.
	fn overlap(&self, name: &str, content: &str) -> String {
		let shift = match name.trim_start_matches("math") {
			"llap" => Some(("-1width", "-100%")),
			"clap" => Some(("-0.5width", "-50%")),
			_ => None,
		};
		match (shift, self.follows_katex) {
			(None, _) => format!("<mpadded width=\"0px\">{content}</mpadded>"),
			// Browsers refuse the unit `width`, so the content stays to the right.
			(Some((lspace, _)), true) => {
				format!("<mpadded lspace=\"{lspace}\" width=\"0px\">{content}</mpadded>")
			}
			(Some((_, shift)), false) => format!(
				"<mpadded width=\"0px\"><mrow style=\"transform:translateX({shift})\">{content}</mrow></mpadded>"
			),
		}
	}

	/// The rows of `CD`, whose name was read, with arrows in cells of their own.
	fn commutative_diagram(&mut self) -> String {
		if !self.enter() {
			return error_text("…");
		}
		let outer_display = std::mem::replace(&mut self.is_display, true);
		let mut cells = Vec::new();
		loop {
			let mut row = Vec::new();
			let mut cell = self.sequence(Context::CommutativeCell);
			while self.peek_token() == Some(Token::Character('@')) {
				self.next_token();
				row.push(std::mem::take(&mut cell));
				row.push(self.commutative_arrow());
				cell = self.sequence(Context::CommutativeCell);
			}
			// A row of vertical arrows has its first arrow in the first column.
			if cells.len() % 2 == 0 {
				row.push(cell);
			} else if !row.is_empty() {
				row.remove(0);
			}
			cells.push(row);
			match self.peek_token() {
				Some(Token::Alignment | LINE_BREAK | Token::Command("cr" | "newline")) => {
					self.next_token();
				}
				Some(Token::Command("end")) | None => break,
				Some(_) => {
					self.fail_expected("\\\\ or \\cr or \\end");
					self.next_token();
				}
			}
		}
		self.is_display = outer_display;
		self.leave();
		if cells.len() > 1 && cells.last().is_some_and(|row| row.iter().all(String::is_empty)) {
			cells.pop();
		}
		if self.peek_token() == Some(Token::Command("end")) {
			self.next_token();
			self.raw_argument();
		}
		let numbers = cells.iter().map(|_| None).collect();
		table(&Rows { cells, numbers }, &[], ColumnAlignment::Center, true)
	}

	/// An arrow of `CD` whose `@` was read.
	fn commutative_arrow(&mut self) -> String {
		let start = self.position;
		let kind = match self.peek_token() {
			Some(Token::Character(kind)) if "<>AV=|.".contains(kind) => kind,
			_ => {
				self.next_token();
				let end = self.position;
				self.fail("Expected one of \"<>AV=|.\" after @", start, end);
				return String::new();
			}
		};
		self.next_token();
		match kind {
			'=' => "<mo stretchy=\"true\">=</mo>".to_owned(),
			'|' => big_arrow(if self.follows_katex { "∥" } else { "‖" }),
			'.' => String::new(),
			_ => {
				let first = self.commutative_label(kind, start);
				let second = self.commutative_label(kind, start);
				match kind {
					'>' | '<' => {
						let arrow = if kind == '>' { "→" } else { "←" };
						format!(
							"<munderover><mo stretchy=\"true\">{arrow}</mo>{}{}</munderover>",
							wide_label(&second),
							wide_label(&first)
						)
					}
					_ => format!(
						"<mrow><mstyle {style}>{first}</mstyle>{}<mstyle {style}>{second}</mstyle></mrow>",
						big_arrow(if kind == 'V' { "↓" } else { "↑" }),
						style = "scriptlevel=\"1\" displaystyle=\"false\""
					),
				}
			}
		}
	}

	/// A label of a `CD` arrow up to its closing character.
	fn commutative_label(&mut self, end: char, arrow_start: usize) -> String {
		let label = self.with_display(false, |converter| {
			converter.nested_sequence(Context::CommutativeLabel(end))
		});
		if self.peek_token() == Some(Token::Character(end)) {
			self.next_token();
		} else {
			let message = format!("Missing a {end} character to complete a CD arrow.");
			self.fail(&message, arrow_start, arrow_start + end.len_utf8());
		}
		label
	}

	fn accent_or_symbol(&mut self, name: &str) -> Atom {
		let Some((character, position, is_stretchy)) = accent(name) else {
			if symbol(name).is_none() {
				let end = self.position;
				let message = format!("Undefined control sequence: \\{name}");
				self.fail(&message, end.saturating_sub(name.len() + 1), end);
			}
			if self.follows_katex {
				match (name, symbol(name)) {
					("vert" | "lvert" | "rvert", _) => {
						return Atom::new(upright_identifier("∣"));
					}
					("Vert" | "lVert" | "rVert", _) => {
						return Atom::new(upright_identifier("∥"));
					}
					("vdots", _) => return Atom::new(upright_identifier("⋮")),
					(_, Some(Symbol::Space(width))) => return Atom::new(self.space(width)),
					_ => {}
				}
			}
			return symbol_atom(name);
		};
		let base = self.argument();
		let is_brace = matches!(name, "overbrace" | "underbrace");
		let accent = match (self.follows_katex, is_stretchy) {
			(true, false) => format!("<mo>{}</mo>", katex_accent(name, character)),
			_ => format!("<mo stretchy=\"{is_stretchy}\">{character}</mo>"),
		};
		// KaTeX's braces are scripts in the size of scripts, no accents.
		let is_accent = !(self.follows_katex && is_brace);
		let mathml = match (position, is_accent) {
			(AccentPosition::Over, true) => {
				format!("<mover accent=\"true\">{base}{accent}</mover>")
			}
			(AccentPosition::Over, false) => format!("<mover>{base}{accent}</mover>"),
			(AccentPosition::Under, true) => {
				format!("<munder accentunder=\"true\">{base}{accent}</munder>")
			}
			(AccentPosition::Under, false) => format!("<munder>{base}{accent}</munder>"),
		};
		let limits = if is_brace { Limits::Always } else { Limits::Never };
		Atom { mathml, limits }
	}

	/// A space: KaTeX's `SpaceNode` writes some widths as Unicode spaces in text.
	fn space(&self, width: &str) -> String {
		if self.follows_katex {
			let text = match width {
				"0.0556em" => Some("\u{200a}"),
				"0.1667em" => Some("\u{2009}"),
				"0.2222em" => Some("\u{2005}"),
				"0.2778em" => Some("\u{2005}\u{200a}"),
				"-0.0556em" => Some("\u{200a}\u{2063}"),
				"-0.1667em" => Some("\u{2009}\u{2063}"),
				"-0.2222em" => Some("\u{205f}\u{2063}"),
				"-0.2778em" => Some("\u{2005}\u{2063}"),
				"0.25em" => Some("\u{a0}"),
				_ => None,
			};
			if let Some(text) = text {
				return format!("<mtext>{text}</mtext>");
			}
		}
		space(width)
	}

	/// Sub- and superscripts and primes following `atom`.
	fn with_scripts(&mut self, atom: Atom) -> String {
		let mut subscript = None;
		let mut superscript = None;
		let mut primes = String::new();
		let mut limits = atom.limits;
		loop {
			match self.peek_token() {
				Some(Token::Character('\'')) => {
					self.next_token();
					primes.push('′');
				}
				Some(Token::Superscript) if superscript.is_none() => {
					self.next_token();
					self.check_script_argument("^");
					superscript = Some(self.with_display(false, Self::argument));
				}
				Some(Token::Subscript) if subscript.is_none() => {
					self.next_token();
					self.check_script_argument("_");
					subscript = Some(self.with_display(false, Self::argument));
				}
				Some(Token::Superscript) => self.fail_double_script("superscript"),
				Some(Token::Subscript) => self.fail_double_script("subscript"),
				Some(Token::Command("limits")) => {
					self.next_token();
					limits = Limits::Always;
				}
				Some(Token::Command("nolimits")) => {
					self.next_token();
					limits = Limits::Never;
				}
				_ => break,
			}
		}
		if !primes.is_empty() {
			let primes = operator_element(&primes);
			superscript = Some(match superscript {
				Some(superscript) => format!("<mrow>{primes}{superscript}</mrow>"),
				None => primes,
			});
		}
		let base = match atom.mathml.is_empty() {
			true => "<mrow></mrow>".to_owned(),
			// A function name is two elements, of which a script has only one base.
			false
				if atom.mathml.ends_with(FUNCTION_APPLICATION)
					&& (subscript.is_some() || superscript.is_some()) =>
			{
				format!("<mrow>{}</mrow>", atom.mathml)
			}
			false => atom.mathml,
		};
		let as_limits = match limits {
			Limits::Never => false,
			Limits::InDisplay => self.is_display,
			Limits::Always => true,
		};
		let (under, over, under_over) = match as_limits {
			true => ("munder", "mover", "munderover"),
			false => ("msub", "msup", "msubsup"),
		};
		match (subscript, superscript) {
			(None, None) => base,
			(Some(subscript), None) => format!("<{under}>{base}{subscript}</{under}>"),
			(None, Some(superscript)) => format!("<{over}>{base}{superscript}</{over}>"),
			(Some(subscript), Some(superscript)) => {
				format!("<{under_over}>{base}{subscript}{superscript}</{under_over}>")
			}
		}
	}

	/// Notes a script without an argument, whose sign was just read.
	fn check_script_argument(&mut self, sign: &str) {
		if matches!(
			self.peek_token(),
			Some(
				Token::EndGroup
					| Token::Alignment
					| LINE_BREAK | Token::Superscript
					| Token::Subscript
					| Token::Command("end" | "right"),
			) | None
		) {
			let end = self.position;
			let message = format!("Expected group after '{sign}'");
			self.fail(&message, end - sign.len(), end);
		}
	}

	/// Notes a second script of the same kind, which is the next token and is dropped.
	fn fail_double_script(&mut self, kind: &str) {
		self.skip_whitespace_and_comments();
		let start = self.position;
		self.fail(&format!("Double {kind}"), start, start + 1);
		self.next_token();
		self.argument();
	}

	/// A delimiter after `\\left` or `\\right`.
	fn stretchy_delimiter(&mut self) -> &'a str {
		let delimiter = self.delimiter().unwrap_or("");
		match self.follows_katex {
			true => katex_delimiter(delimiter),
			false => delimiter,
		}
	}

	fn left_right(&mut self) -> String {
		let open = self.stretchy_delimiter();
		if !self.enter() {
			return error_text("…");
		}
		let content = self.sequence(Context::LeftRight);
		self.leave();
		let close = match self.peek_token() {
			Some(Token::Command("right")) => {
				self.next_token();
				self.stretchy_delimiter()
			}
			_ => {
				self.fail_expected("\\right");
				""
			}
		};
		format!("<mrow>{}{content}{}</mrow>", self.tall_fence(open), self.tall_fence(close))
	}

	/// A delimiter that should grow. KaTeX leaves it to the operator dictionary, which does not
	/// let `∣` and `∥` grow.
	fn tall_fence(&self, delimiter: &str) -> String {
		if !self.follows_katex || delimiter.is_empty() {
			return fence(delimiter, true);
		}
		let mut mathml = String::from("<mo fence=\"true\">");
		push_escaped_text(&mut mathml, delimiter);
		mathml.push_str("</mo>");
		mathml
	}

	/// The delimiter after `\left`, `\right`, `\big` and so on and `""` for `.`.
	fn delimiter(&mut self) -> Option<&'static str> {
		let delimiter = match self.peek_token()? {
			Token::Character(character) => match character {
				'(' => "(",
				')' => ")",
				'[' => "[",
				']' => "]",
				'|' => "|",
				'/' => "/",
				'<' => "⟨",
				'>' => "⟩",
				'.' => "",
				_ => return None,
			},
			Token::ControlSymbol(control_symbol) => match control_symbol {
				'{' => "{",
				'}' => "}",
				'|' => "‖",
				_ => return None,
			},
			Token::Command(name) => {
				let is_corner = matches!(name, "ulcorner" | "urcorner" | "llcorner" | "lrcorner");
				if is_corner && self.follows_katex && self.first_error.is_none() {
					self.first_error = Some("Invalid delimiter type 'htmlmathml'".to_owned());
				}
				delimiter_command(name)?
			}
			_ => return None,
		};
		self.next_token();
		Some(delimiter)
	}

	fn negation(&mut self) -> String {
		let relation = match self.peek_token() {
			Some(Token::Character(character)) if !character.is_alphanumeric() => {
				character.to_string()
			}
			Some(Token::Command(name)) => match symbol(name) {
				Some(Symbol::Operator(operator)) => operator.to_owned(),
				_ => return operator_element("\u{338}"),
			},
			_ => return operator_element("\u{338}"),
		};
		self.next_token();
		operator_element(&negated(&relation))
	}

	fn environment(&mut self) -> String {
		self.skip_whitespace_and_comments();
		let name_start = self.position;
		let name = self.raw_argument().unwrap_or("").trim();
		if !is_known_environment(name) {
			let message = format!("No such environment: {name}");
			self.fail(&message, name_start, self.position);
		}
		if name == "CD" {
			return self.commutative_diagram();
		}
		let (open, close, alignment, is_display_style) = match name {
			"pmatrix" | "pmatrix*" => ("(", ")", ColumnAlignment::Center, false),
			"bmatrix" | "bmatrix*" => ("[", "]", ColumnAlignment::Center, false),
			"Bmatrix" | "Bmatrix*" => ("{", "}", ColumnAlignment::Center, false),
			"vmatrix" | "vmatrix*" => ("|", "|", ColumnAlignment::Center, false),
			"Vmatrix" | "Vmatrix*" => ("‖", "‖", ColumnAlignment::Center, false),
			"cases" | "dcases" => ("{", "", ColumnAlignment::Left, name == "dcases"),
			"rcases" => ("", "}", ColumnAlignment::Left, false),
			"drcases" => ("", "}", ColumnAlignment::Left, true),
			"aligned" | "align" | "align*" | "split" | "eqnarray" | "eqnarray*" | "alignat"
			| "alignat*" | "alignedat" | "flalign" | "flalign*" => {
				("", "", ColumnAlignment::RightLeft, true)
			}
			"gathered" | "gather" | "gather*" | "multline" | "multline*" | "equation"
			| "equation*" => ("", "", ColumnAlignment::Center, true),
			_ => ("", "", ColumnAlignment::Center, false),
		};
		if name.starts_with("alignat") || name == "alignedat" {
			// The number of column pairs.
			self.raw_argument();
		}
		let column_alignments = match name {
			"array" | "subarray" => array_column_alignments(self.raw_argument().unwrap_or("")),
			_ => Vec::new(),
		};
		if !self.enter() {
			return error_text("…");
		}
		let numbers_rows = is_numbered_environment(name)
			&& self.is_display_formula
			&& !self.is_in_numbered_rows
			&& (self.equations.is_some() || self.follows_katex);
		let row_numbers = match (numbers_rows, name.ends_with('*')) {
			(false, _) => RowNumbers::None,
			(true, true) => RowNumbers::Tags,
			(true, false) => RowNumbers::TagsAndLabels,
		};
		let outer_is_numbered = self.is_in_numbered_rows;
		self.is_in_numbered_rows |= numbers_rows;
		let rows = self.with_display(is_display_style, |converter| {
			converter.rows(RowsEnd::Environment, row_numbers)
		});
		self.is_in_numbered_rows = outer_is_numbered;
		self.leave();
		if self.peek_token() == Some(Token::Command("end")) {
			self.next_token();
			self.raw_argument();
		}
		let table = table(&rows, &column_alignments, alignment, is_display_style);
		match (open, close) {
			("", "") => table,
			_ => {
				let (open, close) = match self.follows_katex {
					true => (katex_delimiter(open), katex_delimiter(close)),
					false => (open, close),
				};
				format!("<mrow>{}{table}{}</mrow>", self.tall_fence(open), self.tall_fence(close))
			}
		}
	}

	/// The source text of a `{...}` group or of a single token, for text and names.
	fn raw_argument(&mut self) -> Option<&'a str> {
		self.skip_whitespace_and_comments();
		let rest = self.rest();
		let Some(group) = rest.strip_prefix('{') else {
			let start = self.position;
			self.next_token()?;
			return self.source.get(start..self.position);
		};
		let mut depth = 0usize;
		let mut is_escaped = false;
		for (index, character) in group.char_indices() {
			match character {
				'\\' => {
					is_escaped = !is_escaped;
					continue;
				}
				'{' if !is_escaped => depth += 1,
				'}' if !is_escaped && depth == 0 => {
					self.position += "{".len() + index + "}".len();
					return Some(&group[..index]);
				}
				'}' if !is_escaped => depth -= 1,
				_ => {}
			}
			is_escaped = false;
		}
		self.position = self.source.len();
		Some(group)
	}

	/// The text in `[...]` if it follows.
	fn raw_optional_bracket(&mut self) -> Option<&'a str> {
		self.skip_whitespace_and_comments();
		let rest = self.rest();
		let inner = rest.strip_prefix('[')?;
		let end = inner.find(']')?;
		self.position += end + 2;
		Some(&inner[..end])
	}

	/// The math in `[...]` if it follows, e.g. the index of `\sqrt[3]{x}`.
	fn optional_bracket(&mut self) -> Option<String> {
		if self.peek_token() != Some(Token::Character('[')) {
			return None;
		}
		self.next_token();
		if !self.enter() {
			return Some(error_text("…"));
		}
		let content = self.sequence(Context::Bracket);
		self.leave();
		if self.peek_token() == Some(Token::Character(']')) {
			self.next_token();
		}
		Some(content)
	}

	/// Skips the spacing of `\\[2pt]`.
	fn skip_optional_bracket(&mut self) {
		let rest = self.rest();
		if rest.starts_with('[') {
			self.position += rest.find(']').map_or(rest.len(), |end| end + 1);
		}
	}

	/// A TeX dimension such as `1em`, `-2.5pt` or `3mu`, as a CSS length.
	fn dimension(&mut self) -> Option<String> {
		self.skip_whitespace_and_comments();
		let rest = self.rest();
		let text = if rest.starts_with('{') {
			self.raw_argument()?.trim()
		} else {
			let number_length = rest
				.find(|character: char| !(character.is_ascii_digit() || "+-.".contains(character)))
				.unwrap_or(rest.len());
			let unit_length =
				rest[number_length..].bytes().take_while(u8::is_ascii_alphabetic).take(2).count();
			self.position += number_length + unit_length;
			&rest[..number_length + unit_length]
		};
		css_length(text)
	}
}

/// Whether `name` is written by a command: what `\newcommand` must not redefine, unlike `\renewcommand`.
fn is_known_command(name: &str) -> bool {
	const STRUCTURAL_COMMANDS: [&str; 96] = [
		"above",
		"allowbreak",
		"atop",
		"begin",
		"bf",
		"big",
		"Big",
		"bigg",
		"Bigg",
		"biggl",
		"Biggl",
		"biggm",
		"Biggm",
		"biggr",
		"Biggr",
		"bigl",
		"Bigl",
		"bigm",
		"Bigm",
		"bigr",
		"Bigr",
		"binom",
		"bm",
		"boxed",
		"boldsymbol",
		"brace",
		"brack",
		"cal",
		"cancel",
		"bcancel",
		"cfrac",
		"choose",
		"clap",
		"color",
		"colorbox",
		"dbinom",
		"dfrac",
		"displaylimits",
		"displaystyle",
		"emph",
		"end",
		"fbox",
		"fcolorbox",
		"frac",
		"genfrac",
		"hbox",
		"href",
		"hphantom",
		"hskip",
		"hspace",
		"it",
		"kern",
		"left",
		"limits",
		"llap",
		"mathbb",
		"mathbf",
		"mathcal",
		"mathclap",
		"mathfrak",
		"mathit",
		"mathllap",
		"mathrlap",
		"mathrm",
		"mathsf",
		"mathtt",
		"mbox",
		"mkern",
		"mod",
		"mskip",
		"mspace",
		"nolimits",
		"nonumber",
		"not",
		"notag",
		"operatorname",
		"over",
		"overset",
		"phantom",
		"pmod",
		"raisebox",
		"relax",
		"right",
		"rlap",
		"rm",
		"rule",
		"sf",
		"smash",
		"sqrt",
		"stackrel",
		"substack",
		"tag",
		"text",
		"textcolor",
		"underset",
		"vphantom",
	];
	STRUCTURAL_COMMANDS.contains(&name)
		|| symbol(name).is_some()
		|| accent(name).is_some()
		|| delimiter_command(name).is_some()
}

/// The environments that number their rows in LaTeX, starred or not.
fn is_numbered_environment(name: &str) -> bool {
	matches!(
		name.trim_end_matches('*'),
		"align" | "alignat" | "gather" | "equation" | "multline" | "flalign" | "eqnarray"
	)
}

/// The environments of the match in [`Converter::environment`], with the plain ones.
fn is_known_environment(name: &str) -> bool {
	matches!(
		name,
		"CD" | "drcases"
			| "pmatrix"
			| "pmatrix*"
			| "bmatrix"
			| "bmatrix*"
			| "Bmatrix"
			| "Bmatrix*"
			| "vmatrix"
			| "vmatrix*"
			| "Vmatrix"
			| "Vmatrix*"
			| "matrix"
			| "matrix*"
			| "smallmatrix"
			| "cases" | "dcases"
			| "rcases"
			| "aligned"
			| "align" | "align*"
			| "split" | "eqnarray"
			| "eqnarray*"
			| "alignat"
			| "alignat*"
			| "alignedat"
			| "flalign"
			| "flalign*"
			| "gathered"
			| "gather"
			| "gather*"
			| "multline"
			| "multline*"
			| "equation"
			| "equation*"
			| "array" | "darray"
			| "subarray"
	)
}

fn upright_identifier(identifier: &str) -> String {
	let mut mathml = String::from("<mi mathvariant=\"normal\">");
	push_escaped_text(&mut mathml, identifier);
	mathml.push_str("</mi>");
	mathml
}

/// The character KaTeX writes for a delimiter.
fn katex_delimiter(delimiter: &str) -> &str {
	match delimiter {
		"|" => "∣",
		"‖" => "∥",
		other => other,
	}
}

/// The character KaTeX writes for an accent that does not stretch.
fn katex_accent(name: &str, character: &'static str) -> &'static str {
	match name {
		"vec" => "\u{20d7}",
		"bar" => "\u{2c9}",
		"acute" => "\u{2ca}",
		"grave" => "\u{2cb}",
		_ => character,
	}
}

/// A token as written in the source, for error messages.
fn token_text(token: Token) -> String {
	match token {
		Token::Command(name) => format!("\\{name}"),
		Token::ControlSymbol(symbol) => format!("\\{symbol}"),
		Token::Character(character) => character.to_string(),
		Token::BeginGroup => "{".to_owned(),
		Token::EndGroup => "}".to_owned(),
		Token::Superscript => "^".to_owned(),
		Token::Subscript => "_".to_owned(),
		Token::Alignment => "&".to_owned(),
	}
}

fn symbol_atom(name: &str) -> Atom {
	let Some(symbol) = symbol(name) else {
		return Atom::new(error_text(&format!("\\{name}")));
	};
	match symbol {
		Symbol::Identifier(identifier) => Atom::new(format!("<mi>{identifier}</mi>")),
		Symbol::UprightIdentifier(identifier) => {
			Atom::new(format!("<mi mathvariant=\"normal\">{identifier}</mi>"))
		}
		Symbol::Operator(operator) => Atom::new(operator_element(operator)),
		Symbol::LargeOperator(operator, has_limits) => Atom {
			mathml: format!("<mo largeop=\"true\" movablelimits=\"false\">{operator}</mo>"),
			limits: if has_limits { Limits::InDisplay } else { Limits::Never },
		},
		Symbol::Function(function) => {
			Atom::new(format!("<mi>{function}</mi>{FUNCTION_APPLICATION}"))
		}
		Symbol::LimitFunction(function) => {
			Atom { mathml: format!("<mi>{function}</mi>"), limits: Limits::InDisplay }
		}
		Symbol::Space(width) => Atom::new(space(width)),
		Symbol::Delimiter(delimiter) => Atom::new(fence(delimiter, false)),
	}
}

/// Converts with integers: float parsing and formatting would add large tables to the DLL.
fn css_length(dimension: &str) -> Option<String> {
	let unit_start = dimension.find(|character: char| character.is_ascii_alphabetic())?;
	let (number, unit) = dimension.split_at(unit_start);
	let thousandths = thousandths_of(number.trim())?;
	let (thousandths, unit) = match unit.trim() {
		"mu" => (thousandths / 18, "em"),
		unit @ ("em" | "ex" | "pt" | "px" | "cm" | "mm" | "in" | "pc") => (thousandths, unit),
		_ => return None,
	};
	let sign = if thousandths < 0 { "-" } else { "" };
	let magnitude = thousandths.unsigned_abs();
	Some(format!("{sign}{}.{:03}{unit}", magnitude / 1000, magnitude % 1000))
}

/// A decimal number such as `-2.5` in thousandths, e.g. `-2500`.
fn thousandths_of(number: &str) -> Option<i64> {
	let (sign, digits) = match number.strip_prefix('-') {
		Some(digits) => (-1, digits),
		None => (1, number.strip_prefix('+').unwrap_or(number)),
	};
	let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
	let is_valid = !(whole.is_empty() && fraction.is_empty())
		&& whole.len() <= 9
		&& (whole.bytes().chain(fraction.bytes())).all(|byte| byte.is_ascii_digit());
	if !is_valid {
		return None;
	}
	let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
	let fraction = fraction.bytes().chain(std::iter::repeat(b'0')).take(3);
	let fraction = fraction.fold(0, |value, digit| value * 10 + i64::from(digit - b'0'));
	Some(sign * (whole * 1000 + fraction))
}

fn old_style_font(command: &str) -> Font {
	match command {
		"rm" => Font::Roman,
		"bf" => Font::Bold,
		"it" => Font::Italic,
		"sf" => Font::SansSerif,
		"tt" => Font::Monospace,
		_ => Font::Script,
	}
}

/// Column alignments from an `array` specification such as `{l|cr}`.
fn array_column_alignments(specification: &str) -> Vec<&'static str> {
	let mut alignments = Vec::new();
	let mut depth = 0usize;
	for character in specification.chars() {
		match character {
			'{' => depth += 1,
			'}' => depth = depth.saturating_sub(1),
			'l' if depth == 0 => alignments.push("left"),
			'c' if depth == 0 => alignments.push("center"),
			'r' if depth == 0 => alignments.push("right"),
			_ => {}
		}
	}
	alignments
}

fn table(
	rows: &Rows,
	column_alignments: &[&str],
	alignment: ColumnAlignment,
	is_display_style: bool,
) -> String {
	const SPACE_CELL: &str = "<mtd style=\"width:50%\"></mtd>";
	// A number that is drawn makes the table as wide as the line, with spaces on both sides of the
	// cells. A hidden copy on the left keeps the cells in the middle of the line, as browsers lay
	// out the content of cells as math, which `text-align` does not move to the end of a space.
	// KaTeX's MathML has the spaces and an empty cell for the number.
	let is_wide = rows.numbers.iter().flatten().any(|number| !number.is_empty());
	let mut mathml = String::from(match (is_display_style, is_wide) {
		(true, true) => "<mtable displaystyle=\"true\" style=\"width:100%\">",
		(true, false) => "<mtable displaystyle=\"true\">",
		(false, _) => "<mtable>",
	});
	for (row_index, row) in rows.cells.iter().enumerate() {
		let number = rows.numbers.get(row_index).and_then(Option::as_deref);
		mathml.push_str("<mtr>");
		if is_wide {
			// EdgeHTML has no `mphantom` and takes the style only on the text itself.
			let hidden =
				number.unwrap_or("").replacen("<mtext>", "<mtext style=\"visibility:hidden\">", 1);
			mathml.push_str(&format!("<mtd><mphantom>{hidden}</mphantom></mtd>"));
		}
		if number.is_some() || is_wide {
			mathml.push_str(SPACE_CELL);
		}
		for (column, cell) in row.iter().enumerate() {
			let (align, padding) = match (column_alignments.get(column), alignment) {
				(Some(align), _) => (*align, ""),
				(None, ColumnAlignment::Center) => ("center", ""),
				(None, ColumnAlignment::Left) => ("left", ""),
				(None, ColumnAlignment::RightLeft) if column % 2 == 0 => {
					("right", ";padding-right:0")
				}
				(None, ColumnAlignment::RightLeft) => ("left", ";padding-left:0"),
			};
			mathml.push_str(&format!(
				"<mtd columnalign=\"{align}\" style=\"text-align:{align}{padding}\">{cell}</mtd>"
			));
		}
		if number.is_some() || is_wide {
			mathml.push_str(SPACE_CELL);
			mathml.push_str(&format!("<mtd>{}</mtd>", number.unwrap_or("")));
		}
		mathml.push_str("</mtr>");
	}
	mathml.push_str("</mtable>");
	mathml
}

fn infix_fraction(command: &str, numerator: &str, denominator: &str) -> String {
	let fraction = |thickness: &str| {
		format!("<mfrac{thickness}><mrow>{numerator}</mrow><mrow>{denominator}</mrow></mfrac>")
	};
	let (open, close) = match command {
		"over" => return fraction(""),
		"atop" => return fraction(" linethickness=\"0\""),
		"choose" => ("(", ")"),
		"brace" => ("{", "}"),
		_ => ("[", "]"),
	};
	format!(
		"<mrow>{}{}{}</mrow>",
		fence(open, true),
		fraction(" linethickness=\"0\""),
		fence(close, true)
	)
}

fn operator_element(operator: &str) -> String {
	let mut mathml = String::from("<mo>");
	push_escaped_text(&mut mathml, operator);
	mathml.push_str("</mo>");
	mathml
}

/// A delimiter. Only stretchy after `\left`, `\right` and around environments, as in TeX.
fn fence(delimiter: &str, is_stretchy: bool) -> String {
	if delimiter.is_empty() {
		return String::new();
	}
	let mut mathml = format!("<mo fence=\"true\" stretchy=\"{is_stretchy}\">");
	push_escaped_text(&mut mathml, delimiter);
	mathml.push_str("</mo>");
	mathml
}

fn space(width: &str) -> String {
	format!("<mspace width=\"{width}\"/>")
}

/// The text of `\text{...}` without its markup.
fn plain_text(text: &str) -> String {
	let mut plain = String::with_capacity(text.len());
	let mut characters = text.chars();
	while let Some(character) = characters.next() {
		match character {
			'\\' => match characters.next() {
				Some(escaped @ ('{' | '}' | '$' | '%' | '&' | '_' | '#')) => plain.push(escaped),
				Some(_) | None => plain.push(' '),
			},
			'~' => plain.push('\u{a0}'),
			'{' | '}' => {}
			_ => plain.push(character),
		}
	}
	plain
}

/// `\text{...}` content. Leading and trailing spaces are kept as no-break spaces.
fn text_element(text: &str, attributes: &str) -> String {
	let plain = plain_text(text);
	let trimmed = plain.trim();
	let mut mathml = format!("<mtext{attributes}>");
	if plain.starts_with(char::is_whitespace) {
		mathml.push('\u{a0}');
	}
	push_escaped_text(&mut mathml, trimmed);
	if plain.ends_with(char::is_whitespace) && !trimmed.is_empty() {
		mathml.push('\u{a0}');
	}
	mathml.push_str("</mtext>");
	mathml
}

fn error_text(text: &str) -> String {
	let mut mathml = format!("<mtext mathcolor=\"{ERROR_COLOR}\" style=\"color:{ERROR_COLOR}\">");
	push_escaped_text(&mut mathml, text);
	mathml.push_str("</mtext>");
	mathml
}

/// `color` if it can be written into attributes.
fn valid_color(color: &str) -> Option<&str> {
	let color = color.trim();
	let is_valid = !color.is_empty()
		&& color.len() <= 32
		&& color.chars().all(|character| character.is_ascii_alphanumeric() || character == '#');
	is_valid.then_some(color)
}

fn with_color(color: &str, content: &str) -> String {
	match valid_color(color) {
		Some(color) => {
			format!("<mstyle mathcolor=\"{color}\" style=\"color:{color}\">{content}</mstyle>")
		}
		None => content.to_owned(),
	}
}

/// `\colorbox` and `\fcolorbox`. KaTeX's `width="+6pt"` is 6 points in browsers, which leaves the
/// text outside of its background, so the padding is written as such.
fn color_box(frame: Option<&str>, background: Option<&str>, text: &str) -> String {
	let border = frame.map_or_else(String::new, |frame| format!(";border:0.04em solid {frame}"));
	let background = background
		.map_or_else(String::new, |background| format!(" mathbackground=\"{background}\""));
	format!(
		"<mpadded{background} style=\"padding:3pt{border}\"><mstyle {}>{text}</mstyle></mpadded>",
		STYLE_ATTRIBUTES[1]
	)
}

/// A line across a box as a CSS background, `direction` across the line.
fn strike_line(direction: &str) -> String {
	format!(
		"linear-gradient({direction},transparent calc(50% - 0.6px),currentColor calc(50% - 0.6px),currentColor calc(50% + 0.6px),transparent calc(50% + 0.6px))"
	)
}

/// The line thickness of a fraction: KaTeX writes a point as 0.1 em.
fn line_thickness(css_length: &str) -> String {
	scaled_line_thickness(css_length, 0)
}

/// [`line_thickness`] for a style of `\genfrac`, in whose font size KaTeX expresses the length.
fn scaled_line_thickness(css_length: &str, style_index: usize) -> String {
	let ten_thousandths = match (css_length.strip_suffix("em"), css_length.strip_suffix("pt")) {
		(Some(em), _) => thousandths_of(em).map(|thousandths| thousandths * 10),
		(_, Some(points)) => thousandths_of(points),
		_ => None,
	};
	let Some(ten_thousandths) = ten_thousandths else {
		return css_length.to_owned();
	};
	let scaled = match style_index {
		2 => ten_thousandths * 10 / 7,
		3 => ten_thousandths * 2,
		_ => ten_thousandths,
	};
	let sign = if scaled < 0 { "-" } else { "" };
	let magnitude = scaled.unsigned_abs();
	format!("{sign}{}.{:04}em", magnitude / 10_000, magnitude % 10_000)
}

/// The character of an `\xrightarrow` and the like.
fn extensible_arrow(name: &str) -> &'static str {
	match name {
		"xrightarrow" => "→",
		"xleftarrow" => "←",
		"xleftrightarrow" => "↔",
		"xRightarrow" => "⇒",
		"xLeftarrow" => "⇐",
		"xLeftrightarrow" => "⇔",
		"xmapsto" => "↦",
		"xhookrightarrow" => "↪",
		"xhookleftarrow" => "↩",
		"xrightleftharpoons" => "⇌",
		_ => "=",
	}
}

/// A vertical arrow of `CD`, as tall as `\Big`.
fn big_arrow(arrow: &str) -> String {
	format!("<mo fence=\"true\" stretchy=\"true\" minsize=\"1.8em\" maxsize=\"1.8em\">{arrow}</mo>")
}

/// A label of a horizontal arrow of `CD`, with some room, as browsers ignore `minsize` there.
fn wide_label(label: &str) -> String {
	format!("<mrow><mspace width=\"1em\"/>{label}<mspace width=\"1em\"/></mrow>")
}

/// Latin letters and digits from the Mathematical Alphanumeric Symbols block, which shows the
/// font without `mathvariant` support. Holes in the block are filled from Letterlike Symbols.
fn styled_character(character: char, font: Font) -> char {
	let letterlike = match (font, character) {
		(Font::Italic, 'h') => Some('ℎ'),
		(Font::Script, 'B') => Some('ℬ'),
		(Font::Script, 'E') => Some('ℰ'),
		(Font::Script, 'F') => Some('ℱ'),
		(Font::Script, 'H') => Some('ℋ'),
		(Font::Script, 'I') => Some('ℐ'),
		(Font::Script, 'L') => Some('ℒ'),
		(Font::Script, 'M') => Some('ℳ'),
		(Font::Script, 'R') => Some('ℛ'),
		(Font::Script, 'e') => Some('ℯ'),
		(Font::Script, 'g') => Some('ℊ'),
		(Font::Script, 'o') => Some('ℴ'),
		(Font::Fraktur, 'C') => Some('ℭ'),
		(Font::Fraktur, 'H') => Some('ℌ'),
		(Font::Fraktur, 'I') => Some('ℑ'),
		(Font::Fraktur, 'R') => Some('ℜ'),
		(Font::Fraktur, 'Z') => Some('ℨ'),
		(Font::DoubleStruck, 'C') => Some('ℂ'),
		(Font::DoubleStruck, 'H') => Some('ℍ'),
		(Font::DoubleStruck, 'N') => Some('ℕ'),
		(Font::DoubleStruck, 'P') => Some('ℙ'),
		(Font::DoubleStruck, 'Q') => Some('ℚ'),
		(Font::DoubleStruck, 'R') => Some('ℝ'),
		(Font::DoubleStruck, 'Z') => Some('ℤ'),
		_ => None,
	};
	if let Some(letterlike) = letterlike {
		return letterlike;
	}
	// First code points of the upper case letters, the lower case letters and the digits.
	let (upper_case, lower_case, digits) = match font {
		Font::Roman | Font::Italic => return character,
		Font::Bold => (0x1D400, 0x1D41A, Some(0x1D7CE)),
		Font::BoldItalic => (0x1D468, 0x1D482, Some(0x1D7CE)),
		Font::SansSerif => (0x1D5A0, 0x1D5BA, Some(0x1D7E2)),
		Font::Monospace => (0x1D670, 0x1D68A, Some(0x1D7F6)),
		Font::Script => (0x1D49C, 0x1D4B6, None),
		Font::Fraktur => (0x1D504, 0x1D51E, None),
		Font::DoubleStruck => (0x1D538, 0x1D552, Some(0x1D7D8)),
	};
	let code_point = match (character, digits) {
		('A'..='Z', _) => upper_case + (character as u32 - 'A' as u32),
		('a'..='z', _) => lower_case + (character as u32 - 'a' as u32),
		('0'..='9', Some(digits)) => digits + (character as u32 - '0' as u32),
		_ => return character,
	};
	char::from_u32(code_point).unwrap_or(character)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn math(latex: &str) -> String {
		let mut output = String::new();
		push_math(&mut output, latex, false, &mut Equations::new(true));
		output
			.strip_prefix("<math xmlns=\"http://www.w3.org/1998/Math/MathML\">")
			.and_then(|inner| inner.strip_suffix("</math>"))
			.expect("math element")
			.to_owned()
	}

	fn display_math(latex: &str) -> String {
		let mut output = String::new();
		push_math(&mut output, latex, true, &mut Equations::new(true));
		output
	}

	/// The formulas of a document, each as display math.
	fn document(formulas: &[&str], writes_links: bool) -> Vec<String> {
		let mut equations = Equations::new(writes_links);
		let separator = "\n";
		let mut output = String::new();
		for formula in formulas {
			push_math(&mut output, formula, true, &mut equations);
			output.push_str(separator);
		}
		equations.resolve_references(&mut output);
		output.lines().map(str::to_owned).collect()
	}

	#[test]
	fn converts_identifiers_numbers_and_operators() {
		assert_eq!(
			math("x+12.5 = -y"),
			"<mi>x</mi><mo>+</mo><mn>12.5</mn><mo>=</mo><mo>−</mo><mi>y</mi>"
		);
		assert_eq!(math("a<b"), "<mi>a</mi><mo>&lt;</mo><mi>b</mi>");
		assert_eq!(
			math("\\alpha\\Gamma\\leq"),
			"<mi>α</mi><mi mathvariant=\"normal\">Γ</mi><mo>≤</mo>"
		);
	}

	#[test]
	fn converts_fractions_roots_and_scripts() {
		assert_eq!(math("\\frac12"), "<mfrac><mn>1</mn><mn>2</mn></mfrac>");
		assert_eq!(
			math("\\frac{a}{b^2}"),
			"<mfrac><mrow><mi>a</mi></mrow><mrow><msup><mi>b</mi><mn>2</mn></msup></mrow></mfrac>"
		);
		assert_eq!(
			math("\\sqrt[3]{x}"),
			"<mroot><mrow><mi>x</mi></mrow><mrow><mn>3</mn></mrow></mroot>"
		);
		assert_eq!(math("x_i^2"), "<msubsup><mi>x</mi><mi>i</mi><mn>2</mn></msubsup>");
		assert_eq!(math("f'"), "<msup><mi>f</mi><mo>′</mo></msup>");
		assert_eq!(
			math("a \\over b"),
			"<mfrac><mrow><mi>a</mi></mrow><mrow><mi>b</mi></mrow></mfrac>"
		);
	}

	#[test]
	fn places_limits_in_display_math() {
		assert!(math("\\sum_{i=0}^n").starts_with("<msubsup><mo largeop"));
		assert!(display_math("\\sum_{i=0}^n").contains("<munderover><mo largeop"));
		assert!(display_math("\\int_0^1").contains("<msubsup>"));
		assert!(display_math("\\lim_{x \\to 0}").contains("<munder><mi>lim</mi>"));
		assert!(math("\\sin x").starts_with("<mi>sin</mi><mo>\u{2061}</mo>"));
	}

	#[test]
	fn converts_text_fonts_and_accents() {
		assert_eq!(math("\\text{ if } x"), "<mtext>\u{a0}if\u{a0}</mtext><mi>x</mi>");
		assert_eq!(math("\\mathbb{R}\\mathbf x"), "<mrow><mi>ℝ</mi></mrow><mi>𝐱</mi>");
		assert_eq!(math("\\mathrm{d}"), "<mrow><mi mathvariant=\"normal\">d</mi></mrow>");
		assert_eq!(
			math("\\hat x"),
			"<mover accent=\"true\"><mi>x</mi><mo stretchy=\"false\">^</mo></mover>"
		);
	}

	#[test]
	fn converts_delimiters_and_environments() {
		assert_eq!(
			math("\\left( x \\right."),
			"<mrow><mo fence=\"true\" stretchy=\"true\">(</mo><mi>x</mi></mrow>"
		);
		let matrix = math("\\begin{pmatrix} a & b \\\\ c & d \\\\ \\end{pmatrix}");
		assert!(
			matrix.starts_with("<mrow><mo fence=\"true\" stretchy=\"true\">(</mo><mtable>"),
			"{matrix}"
		);
		assert_eq!(matrix.matches("<mtr>").count(), 2, "{matrix}");
		assert_eq!(matrix.matches("<mtd").count(), 4, "{matrix}");
		let cases = math("f = \\begin{cases} 1 & x > 0 \\\\ 0 & \\text{else} \\end{cases}");
		assert!(cases.contains("columnalign=\"left\""), "{cases}");
		let aligned = display_math("a &= b \\\\ c &= d");
		assert!(aligned.contains("<mtable displaystyle=\"true\">"), "{aligned}");
		assert!(aligned.contains("columnalign=\"right\""), "{aligned}");
	}

	#[test]
	fn marks_unknown_commands_and_survives_malformed_input() {
		assert!(math("\\foo").contains(">\\foo</mtext>"));
		for latex in [
			"\\frac{a}",
			"\\sqrt{",
			"x^",
			"}}",
			"\\left(",
			"\\right)",
			"\\end{x}",
			"\\begin{array}{c} a & b & c",
			"\\\\",
			"&&",
			"\\",
			"{\\color{red} x",
			"\\not",
			"\\kern",
			"x''^2_1",
			"\\text{",
		] {
			let output = math(latex);
			assert!(!output.contains("<math"), "{latex}: {output}");
		}
	}

	#[test]
	fn limits_the_nesting_depth() {
		let deep = "{".repeat(100_000) + &"\\frac".repeat(100_000) + &"\\sqrt".repeat(100_000);
		assert!(math(&deep).contains('…'));
		assert!(math(&"x^{".repeat(100_000)).contains('…'));
	}

	#[test]
	fn converts_tex_dimensions_to_css() {
		assert_eq!(css_length("2pt").as_deref(), Some("2.000pt"));
		assert_eq!(css_length("-0.25em").as_deref(), Some("-0.250em"));
		assert_eq!(css_length("18mu").as_deref(), Some("1.000em"));
		assert_eq!(css_length(".5ex").as_deref(), Some("0.500ex"));
		for invalid in ["em", "1.2.3em", "5furlong", "9999999999pt", "-"] {
			assert_eq!(css_length(invalid), None, "{invalid}");
		}
		assert_eq!(math("\\kern1.5em x"), "<mspace width=\"1.500em\"/><mi>x</mi>");
	}

	#[test]
	fn reports_the_first_error_like_katex() {
		assert!(
			latex_to_mathml("x^2+\\frac{1}{2}").is_ok_and(|mathml| mathml.starts_with("<math"))
		);
		for (latex, error) in [
			(
				"\\foo",
				"Undefined control sequence: \\foo at position 1: \\\u{332}f\u{332}o\u{332}o\u{332}",
			),
			("x^", "Expected group after '^' at position 2: x^\u{332}"),
			("{x", "Expected '}', got 'EOF' at end of input: {x"),
			(
				"\\frac{1}",
				"Unexpected end of input in a macro argument, expected '}' at end of input: \\frac{1}",
			),
			("\\left( x", "Expected '\\right', got 'EOF' at end of input: \\left( x"),
			(
				"\\begin{foo}x\\end{foo}",
				"No such environment: foo at position 7: \\begin{\u{332}f\u{332}o\u{332}o\u{332}}\u{332}x\\end{foo}",
			),
			("x}", "Expected 'EOF', got '}' at position 2: x}\u{332}"),
			("x^1^2", "Double superscript at position 4: x^1^\u{332}2"),
			("x&#35;2", "Expected 'EOF', got '&' at position 2: x&\u{332}#35;2"),
		] {
			assert_eq!(latex_to_mathml(latex).unwrap_err(), error, "{latex}");
		}
		let long = format!("{}\\foo{}", "a".repeat(20), "1".repeat(20));
		let message = latex_to_mathml(&long).unwrap_err();
		assert!(message.contains("at position 21: …aaaaaaaaaaaaaaa\\"), "{message}");
		assert!(message.ends_with(&format!("{}…", "1".repeat(15))), "{message}");
	}

	#[test]
	fn converts_the_commands_of_phase_7b() {
		for (latex, expected) in [
			("\\lesssim", "<mo>≲</mo>"),
			("\\nvDash", "<mo>⊭</mo>"),
			("\\coloneqq", "<mo>≔</mo>"),
			("\\leadsto", "<mo>⇝</mo>"),
			("\\oiiint", "<mo largeop=\"true\" movablelimits=\"false\">∰</mo>"),
			("\\smallint", "<mo largeop=\"false\">∫</mo>"),
			("\\complement", "<mi mathvariant=\"normal\">∁</mi>"),
			("\\varGamma", "<mi>Γ</mi>"),
			("\\S", "<mi mathvariant=\"normal\">§</mi>"),
			("\\Bbb R", "<mi>ℝ</mi>"),
			("\\bold x", "<mi>𝐱</mi>"),
			("\\frak g", "<mi>𝔤</mi>"),
			("\\sh", "<mi>sh</mi><mo>\u{2061}</mo>"),
			("\\injlim", "<mi>inj\u{2009}lim</mi>"),
			("\\lgroup", "<mo fence=\"true\" stretchy=\"false\">⟮</mo>"),
			("\\pmb x", "<mstyle style=\"text-shadow: 0.02em 0.01em 0.04px\"><mi>x</mi></mstyle>"),
			("\\smash x", "<mpadded height=\"0px\" depth=\"0px\"><mi>x</mi></mpadded>"),
			("\\smash[t] x", "<mpadded height=\"0px\"><mi>x</mi></mpadded>"),
			("\\rlap{x}", "<mpadded width=\"0px\"><mtext>x</mtext></mpadded>"),
			(
				"\\mathllap x",
				"<mpadded width=\"0px\"><mrow style=\"transform:translateX(-100%)\"><mi>x</mi></mrow></mpadded>",
			),
			(
				"\\hphantom x",
				"<mpadded height=\"0px\" depth=\"0px\"><mphantom><mi>x</mi></mphantom></mpadded>",
			),
			("\\vphantom x", "<mpadded width=\"0px\"><mphantom><mi>x</mi></mphantom></mpadded>"),
			(
				"\\genfrac(]{0pt}{1}12",
				"<mstyle displaystyle=\"false\" scriptlevel=\"0\"><mrow><mo fence=\"true\" stretchy=\"true\">(</mo><mfrac linethickness=\"0.0000em\"><mn>1</mn><mn>2</mn></mfrac><mo fence=\"true\" stretchy=\"true\">]</mo></mrow></mstyle>",
			),
			(
				"\\genfrac{}{}{1pt}{}12",
				"<mfrac linethickness=\"0.1000em\"><mn>1</mn><mn>2</mn></mfrac>",
			),
			(
				"a\\above 1pt b",
				"<mfrac linethickness=\"0.1000em\"><mrow><mi>a</mi></mrow><mrow><mi>b</mi></mrow></mfrac>",
			),
		] {
			assert_eq!(math(latex), expected, "{latex}");
		}
		for latex in [
			"\\xleftrightarrow{a}",
			"\\xRightarrow[b]{a}",
			"\\xlongequal{a}",
			"\\utilde{x}",
			"\\overgroup{x}",
			"\\ddddot{x}",
			"\\overlinesegment{x}",
			"\\varlimsup_a",
			"\\varliminf_a",
			"\\left\\lmoustache x\\right\\rmoustache",
			"\\raisebox{0.5em}{x}",
			"\\rule{1em}{0.5em}",
			"\\rule[0.2em]{1em}{0.5em}",
			"\\colorbox{red}{x}",
			"\\fcolorbox{red}{blue}{x}",
			"\\sum_{\\substack{i\\\\j}}x",
			"\\begin{drcases}a\\\\b\\end{drcases}",
			"\\begin{CD}A@>a>>B\\\\@VbVV@VVcV\\\\C@=D\\end{CD}",
			"\\begin{CD}A@<<<B@.C\\\\@AAA@|\\end{CD}",
		] {
			assert!(latex_to_mathml(latex).is_ok(), "{latex}: {:?}", latex_to_mathml(latex));
			assert!(!math(latex).contains(ERROR_COLOR), "{latex}");
		}
		assert!(math("\\sideset{_a}{_b}\\sum").contains(ERROR_COLOR));
		assert!(latex_to_mathml("\\sideset{_a}{_b}\\sum").is_err());
	}

	#[test]
	fn writes_colors_and_lines_for_browsers() {
		let frame = math("\\fcolorbox{red}{blue}{x}");
		assert!(
			frame.starts_with(
				"<mpadded mathbackground=\"blue\" style=\"padding:3pt;border:0.04em solid red\">"
			),
			"{frame}"
		);
		assert!(!math("\\colorbox{\"x}{y}").contains("mathbackground"));
		let cancel = math("\\cancel x");
		assert!(
			cancel.starts_with(
				"<menclose notation=\"updiagonalstrike\" style=\"background:linear-gradient(to bottom right,"
			),
			"{cancel}"
		);
		assert_eq!(math("\\xcancel x").matches("linear-gradient").count(), 2);
	}

	#[test]
	fn expands_macros_before_converting() {
		assert_eq!(math("\\newcommand{\\a}[1]{#1^2}\\a{y}"), "<msup><mi>y</mi><mn>2</mn></msup>");
		assert_eq!(math("\\def\\a{x}\\a"), "<mi>x</mi>");
		assert_eq!(
			latex_to_mathml("\\def\\a{x}\\a").as_deref(),
			Ok("<math display=\"block\"><mi>x</mi></math>")
		);
		assert_eq!(
			latex_to_mathml("\\def\\a{\\a}\\a").unwrap_err(),
			"Too many expansions: infinite loop or need to increase maxExpand setting"
		);
		assert!(math("\\def\\a{\\a}\\a").contains(">\\a</mtext>"));
		assert!(latex_to_mathml("\\newcommand{\\frac}{x}").is_err());
		assert!(latex_to_mathml("\\renewcommand{\\frac}{x}").is_ok());
	}

	#[test]
	fn links_formulas_with_href() {
		let link = math("\\href{https://x.org/?a=\"b\"}{y}");
		assert!(
			link.starts_with(
				"<mtext><a href=\"https://x.org/?a=&quot;b&quot;\"><math displaystyle=\"false\""
			),
			"{link}"
		);
		assert!(link.ends_with("><mrow><mi>y</mi></mrow></math></a></mtext>"), "{link}");
		assert_eq!(math("\\href{javascript:x}{y}"), "<mrow><mi>y</mi></mrow>");
		let mut office = String::new();
		push_math(&mut office, "\\href{https://x.org}{y}", false, &mut Equations::new(false));
		assert!(!office.contains("<a "), "{office}");
		assert_eq!(
			latex_to_mathml("\\href{https://x.org}{y}").as_deref(),
			Ok(
				"<math display=\"block\"><mrow href=\"https://x.org\"><mrow><mi>y</mi></mrow></mrow></math>"
			)
		);
	}

	#[test]
	fn numbers_equations() {
		let lines = document(
			&[
				"\\eqref{b} \\ref{a} \\ref{z}",
				"x \\label{a}",
				"y",
				"\\begin{align}p &= q \\label{b}\\\\r &= s \\nonumber \\\\t &= u \\tag{A} \\label{c}\\end{align}",
				"\\ref{c}",
				"z \\tag*{Z} \\label{d}",
				"\\eqref{d}",
				"\\begin{equation*}w \\label{e}\\end{equation*}",
				"\\ref{e}",
			],
			true,
		);
		assert!(lines[0].contains("<a href=\"#b\">(2)</a>"), "{}", lines[0]);
		assert!(lines[0].contains("<a href=\"#a\">1</a>"), "{}", lines[0]);
		assert!(lines[0].contains("color:#d1242f\">z</mtext>"), "{}", lines[0]);
		assert!(
			lines[1].contains("<mspace id=\"a\" style=\"scroll-margin-top:5em\"/>"),
			"{}",
			lines[1]
		);
		assert!(
			lines[1].starts_with("<math display=\"block\" style=\"width:100%\""),
			"{}",
			lines[1]
		);
		assert!(lines[1].contains("<mtd><mtext>(1)</mtext></mtd>"), "{}", lines[1]);
		assert!(!lines[2].contains("mtable"), "{}", lines[2]);
		assert!(lines[3].contains("<mtd><mtext>(2)</mtext></mtd>"), "{}", lines[3]);
		assert!(lines[3].contains("<mtd><mtext>(A)</mtext></mtd>"), "{}", lines[3]);
		assert_eq!(lines[3].matches("<mtr>").count(), 3, "{}", lines[3]);
		assert_eq!(lines[3].matches("<mtext>").count(), 2, "{}", lines[3]);
		assert!(lines[4].contains(">A</a>"), "{}", lines[4]);
		assert!(lines[5].contains("<mtd><mtext>Z</mtext></mtd>"), "{}", lines[5]);
		assert!(lines[6].contains(">(Z)</a>"), "{}", lines[6]);
		assert!(!lines[7].contains("(3)"), "{}", lines[7]);
		assert!(lines[8].contains(">e</a>"), "{}", lines[8]);
	}

	#[test]
	fn treats_inline_labels_and_tags_without_numbers() {
		assert_eq!(
			math("x\\label{a}"),
			"<mi>x</mi><mspace id=\"a\" style=\"scroll-margin-top:5em\"/>"
		);
		assert_eq!(math("x\\tag{1}"), "<mi>x</mi>");
		let duplicates = document(&["x\\label{a}", "y\\label{a}"], true);
		assert!(duplicates[1].contains(">\\label{a}</mtext>"), "{}", duplicates[1]);
		assert!(!duplicates[1].contains("(2)"), "{}", duplicates[1]);
		let office = document(&["x\\label{a}", "\\eqref{a}"], false);
		assert!(!office[0].contains("id="), "{}", office[0]);
		assert!(office[0].contains("(1)"), "{}", office[0]);
		assert!(office[1].contains("<mtext>(1)</mtext>"), "{}", office[1]);
	}

	#[test]
	fn matches_katex_for_labels_and_tags() {
		assert_eq!(
			latex_to_mathml("x\\tag{1}").as_deref(),
			Ok(
				"<math display=\"block\"><mtable style=\"width:100%\"><mtr><mtd style=\"width:50%\"></mtd><mtd><mi>x</mi></mtd><mtd style=\"width:50%\"></mtd><mtd><mtext>(1)</mtext></mtd></mtr></mtable></math>"
			)
		);
		assert!(
			latex_to_mathml("x\\tag*{1}").is_ok_and(|mathml| mathml.contains("<mtext>1</mtext>"))
		);
		assert!(latex_to_mathml("x\\tag{1}\\tag{2}").is_err());
		for latex in ["x\\label{a}", "\\ref{a}", "\\eqref{a}"] {
			assert!(
				latex_to_mathml(latex).unwrap_err().starts_with("Undefined control sequence"),
				"{latex}"
			);
		}
		assert_eq!(
			latex_to_mathml("\\left\\ulcorner x\\right\\urcorner").unwrap_err(),
			"Invalid delimiter type 'htmlmathml'"
		);
		assert_eq!(
			latex_to_mathml("a\\\\b").as_deref(),
			Ok("<math display=\"block\"><mrow><mi>a</mi><mspace/><mi>b</mi></mrow></math>")
		);
		assert_eq!(latex_to_mathml("x\\tag{1}\\tag{2}").unwrap_err(), "Multiple \\tag");
		assert_eq!(
			latex_to_mathml("\\sum_{\\substack{a&b}}").unwrap_err(),
			"{subarray} can contain only one column"
		);
		assert!(
			latex_to_mathml("\\genfrac{}{}{2em}{3}{a}{b}")
				.is_ok_and(|mathml| mathml.contains("linethickness=\"4.0000em\""))
		);
		assert!(
			latex_to_mathml("\\rule[-0.2em]{1em}{1em}")
				.is_ok_and(|mathml| mathml.contains(" depth=\"0.200em\" "))
		);
		// The empty cells of KaTeX's numbered environments.
		let aligned = latex_to_mathml("\\begin{align}a&=b\\end{align}").unwrap();
		assert_eq!(aligned.matches("<mtd").count(), 5, "{aligned}");
		let starred = latex_to_mathml("\\begin{align*}a&=b\\end{align*}").unwrap();
		assert_eq!(starred.matches("<mtd").count(), 2, "{starred}");
	}

	#[test]
	fn rejects_unsafe_colors() {
		assert_eq!(math("\\color{\"><script>}x"), "<mi>x</mi>");
		assert!(math("\\textcolor{#FF0000}{x}").contains("mathcolor=\"#FF0000\""));
	}
}
