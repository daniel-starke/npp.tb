//! LaTeX math to MathML, rendered by current browsers and converted to equations by Word.
//!
//! Covers the LaTeX commonly written in Markdown: fractions, roots, scripts and limits, Greek
//! letters and symbols, accents, fonts, `\text`, `\left`/`\right`, matrices, cases and aligned
//! environments. Unknown commands are shown in red. Malformed input never fails: missing
//! arguments stay empty and stray tokens are skipped. The nesting depth is limited because a
//! stack overflow would abort Notepad++.

use super::escape::push_escaped_text;
use super::math_symbols::{AccentPosition, Symbol, accent, delimiter_command, negated, symbol};

const MAX_DEPTH: usize = 48;
const ERROR_COLOR: &str = "#d1242f";
const FUNCTION_APPLICATION: &str = "<mo>\u{2061}</mo>";

pub fn push_math(output: &mut String, latex: &str, is_display: bool) {
	output.push_str(match is_display {
		true => "<math display=\"block\" xmlns=\"http://www.w3.org/1998/Math/MathML\">",
		false => "<math xmlns=\"http://www.w3.org/1998/Math/MathML\">",
	});
	let mut converter = Converter { source: latex, position: 0, depth: 0, is_display, font: None };
	output.push_str(&converter.top_level());
	output.push_str("</math>");
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
}

impl<'a> Converter<'a> {
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
			return false;
		}
		self.depth += 1;
		true
	}

	fn leave(&mut self) {
		self.depth = self.depth.saturating_sub(1);
	}

	fn top_level(&mut self) -> String {
		let mut rows = self.rows(false);
		if rows.len() == 1 && rows.first().is_some_and(|row| row.len() == 1) {
			return rows.pop().and_then(|mut row| row.pop()).unwrap_or_default();
		}
		let has_columns = rows.iter().any(|row| row.len() > 1);
		let alignment =
			if has_columns { ColumnAlignment::RightLeft } else { ColumnAlignment::Center };
		table(&rows, &[], alignment, true)
	}

	/// Cells separated by `&` and rows by `\\`, up to `\end` of an environment or the end of
	/// the input (top level).
	fn rows(&mut self, is_environment: bool) -> Vec<Vec<String>> {
		let mut rows = vec![Vec::new()];
		loop {
			let cell = self.sequence(Context::Cell);
			if let Some(row) = rows.last_mut() {
				row.push(cell);
			}
			match self.peek_token() {
				None => break,
				Some(Token::Alignment) => {
					self.next_token();
				}
				Some(LINE_BREAK | Token::Command("cr" | "newline")) => {
					self.next_token();
					self.skip_optional_bracket();
					rows.push(Vec::new());
				}
				Some(Token::Command("end")) if is_environment => break,
				Some(Token::Command("end")) => {
					self.next_token();
					let name = self.raw_argument().unwrap_or("");
					if let Some(row) = rows.last_mut() {
						row.push(error_text(&format!("\\end{{{name}}}")));
					}
				}
				// A stray `}` or anything else the cell stopped at.
				Some(_) => {
					self.next_token();
				}
			}
		}
		// `\\` after the last row does not start a new one.
		if rows.len() > 1 && rows.last().is_some_and(|row| row.iter().all(String::is_empty)) {
			rows.pop();
		}
		rows
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
				Token::Command(
					command @ ("displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle"),
				) => {
					self.next_token();
					let rest = self.nested_sequence(context);
					let attributes = match command {
						"displaystyle" => "displaystyle=\"true\" scriptlevel=\"0\"",
						"textstyle" => "displaystyle=\"false\" scriptlevel=\"0\"",
						"scriptstyle" => "displaystyle=\"false\" scriptlevel=\"1\"",
						_ => "displaystyle=\"false\" scriptlevel=\"2\"",
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
			)
			| None => "<mrow></mrow>".to_owned(),
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
				'|' => fence("‖", false),
				'%' | '&' | '#' | '$' | '_' => operator_element(&control_symbol.to_string()),
				'\\' => return None,
				_ => match symbol(&control_symbol.to_string()) {
					Some(Symbol::Space(width)) => space(width),
					_ => error_text(&format!("\\{control_symbol}")),
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
			'~' => "<mtext>\u{a0}</mtext>".to_owned(),
			'\'' => operator_element("′"),
			'-' => operator_element("−"),
			'*' => operator_element("∗"),
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
				let numerator = self.argument();
				let denominator = self.argument();
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
				let top = self.argument();
				let bottom = self.argument();
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
			| "mathfrak" | "mathbb" | "boldsymbol" | "bm" | "mathbfit" | "mathnormal" => {
				let font = match name {
					"mathrm" => Font::Roman,
					"mathbf" => Font::Bold,
					"mathit" | "mathnormal" => Font::Italic,
					"mathsf" => Font::SansSerif,
					"mathtt" => Font::Monospace,
					"mathcal" | "mathscr" => Font::Script,
					"mathfrak" => Font::Fraktur,
					"mathbb" => Font::DoubleStruck,
					_ => Font::BoldItalic,
				};
				self.with_font(font, Self::argument)
			}
			"operatorname" | "operatornamewithlimits" => {
				let operator_name = self.raw_argument().unwrap_or("").trim().trim_end_matches('*');
				let mut mathml = String::from("<mi>");
				push_escaped_text(&mut mathml, operator_name);
				mathml.push_str("</mi>");
				let limits = match name {
					"operatornamewithlimits" => Limits::InDisplay,
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
			"xrightarrow" | "xleftarrow" => {
				let below = self.optional_bracket();
				let above = self.argument();
				let arrow = format!(
					"<mo stretchy=\"true\" minsize=\"2em\">{}</mo>",
					if name == "xrightarrow" { "→" } else { "←" }
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
				let notation = match name {
					"cancel" => "updiagonalstrike",
					"bcancel" => "downdiagonalstrike",
					"xcancel" => "updiagonalstrike downdiagonalstrike",
					_ => "horizontalstrike",
				};
				let content = self.argument();
				format!("<menclose notation=\"{notation}\">{content}</menclose>")
			}
			"phantom" | "hphantom" | "vphantom" => {
				format!("<mphantom>{}</mphantom>", self.argument())
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
				// The link target is dropped. Links inside formulas are rare.
				self.raw_argument();
				self.argument()
			}
			"label" | "tag" | "cline" => {
				self.raw_argument();
				return None;
			}
			"limits" | "nolimits" | "displaylimits" | "hline" | "hdashline" | "nonumber"
			| "notag" | "relax" | "strut" | "mathstrut" | "nobreak" | "allowbreak" => return None,
			_ => return Some(self.accent_or_symbol(name)),
		};
		Some(Atom::new(mathml))
	}

	fn accent_or_symbol(&mut self, name: &str) -> Atom {
		let Some((character, position, is_stretchy)) = accent(name) else {
			return symbol_atom(name);
		};
		let base = self.argument();
		let accent = format!("<mo stretchy=\"{is_stretchy}\">{character}</mo>");
		let mathml = match position {
			AccentPosition::Over => format!("<mover accent=\"true\">{base}{accent}</mover>"),
			AccentPosition::Under => {
				format!("<munder accentunder=\"true\">{base}{accent}</munder>")
			}
		};
		let limits = match name {
			"overbrace" | "underbrace" => Limits::Always,
			_ => Limits::Never,
		};
		Atom { mathml, limits }
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
					superscript = Some(self.argument());
				}
				Some(Token::Subscript) if subscript.is_none() => {
					self.next_token();
					subscript = Some(self.argument());
				}
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

	fn left_right(&mut self) -> String {
		let open = self.delimiter().unwrap_or("");
		if !self.enter() {
			return error_text("…");
		}
		let content = self.sequence(Context::LeftRight);
		self.leave();
		let close = match self.peek_token() {
			Some(Token::Command("right")) => {
				self.next_token();
				self.delimiter().unwrap_or("")
			}
			_ => "",
		};
		format!("<mrow>{}{content}{}</mrow>", fence(open, true), fence(close, true))
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
			Token::Command(name) => delimiter_command(name)?,
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
		let name = self.raw_argument().unwrap_or("").trim();
		let (open, close, alignment, is_display_style) = match name {
			"pmatrix" | "pmatrix*" => ("(", ")", ColumnAlignment::Center, false),
			"bmatrix" | "bmatrix*" => ("[", "]", ColumnAlignment::Center, false),
			"Bmatrix" | "Bmatrix*" => ("{", "}", ColumnAlignment::Center, false),
			"vmatrix" | "vmatrix*" => ("|", "|", ColumnAlignment::Center, false),
			"Vmatrix" | "Vmatrix*" => ("‖", "‖", ColumnAlignment::Center, false),
			"cases" | "dcases" => ("{", "", ColumnAlignment::Left, name == "dcases"),
			"rcases" => ("", "}", ColumnAlignment::Left, false),
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
		let rows = self.rows(true);
		self.leave();
		if self.peek_token() == Some(Token::Command("end")) {
			self.next_token();
			self.raw_argument();
		}
		let table = table(&rows, &column_alignments, alignment, is_display_style);
		match (open, close) {
			("", "") => table,
			_ => format!("<mrow>{}{table}{}</mrow>", fence(open, true), fence(close, true)),
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
	rows: &[Vec<String>],
	column_alignments: &[&str],
	alignment: ColumnAlignment,
	is_display_style: bool,
) -> String {
	let mut mathml = String::from(match is_display_style {
		true => "<mtable displaystyle=\"true\">",
		false => "<mtable>",
	});
	for row in rows {
		mathml.push_str("<mtr>");
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

/// `\text{...}` content. Leading and trailing spaces are kept as no-break spaces.
fn text_element(text: &str, attributes: &str) -> String {
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

fn with_color(color: &str, content: &str) -> String {
	let color = color.trim();
	let is_valid = !color.is_empty()
		&& color.len() <= 32
		&& color.chars().all(|character| character.is_ascii_alphanumeric() || character == '#');
	match is_valid {
		true => format!("<mstyle mathcolor=\"{color}\" style=\"color:{color}\">{content}</mstyle>"),
		false => content.to_owned(),
	}
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
		push_math(&mut output, latex, false);
		output
			.strip_prefix("<math xmlns=\"http://www.w3.org/1998/Math/MathML\">")
			.and_then(|inner| inner.strip_suffix("</math>"))
			.expect("math element")
			.to_owned()
	}

	fn display_math(latex: &str) -> String {
		let mut output = String::new();
		push_math(&mut output, latex, true);
		output
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
	fn rejects_unsafe_colors() {
		assert_eq!(math("\\color{\"><script>}x"), "<mi>x</mi>");
		assert!(math("\\textcolor{#FF0000}{x}").contains("mathcolor=\"#FF0000\""));
	}
}
