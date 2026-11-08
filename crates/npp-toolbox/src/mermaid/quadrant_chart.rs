//! Quadrant charts (`quadrant.jison`, `quadrantDb.ts`, `quadrantBuilder.ts`,
//! `quadrantRenderer.ts`, `utils.ts`).

use std::collections::HashMap;

use super::color;
use super::common::{acc_description, sanitize_text};
use super::jison::{
	keyword, keyword_and_delimiter, line_length, span, starts_with_ignoring_case, word_length,
};
use super::svg::{Accessibility, Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::{Context, Diagram, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	NewLine,
	Semicolon,
	/// The grammar's `EOF` terminal, which the lexer returns once in its initial state.
	EndOfFile,
	/// jison's end of input after `EOF`, or at the end in any other state.
	End,
	Space,
	Title,
	TitleValue,
	AccTitle,
	AccTitleValue,
	AccDescription,
	AccDescriptionValue,
	AccDescriptionMultilineValue,
	XAxis,
	YAxis,
	AxisDelimiter,
	Quadrant(usize),
	ClassDef,
	String,
	MarkdownString,
	ClassName,
	PointStart,
	PointX,
	PointY,
	Header,
	Alpha,
	UnicodeText,
	Colon,
	Plus,
	Comma,
	Equals,
	Multiply,
	Bracket,
	Underscore,
	Dot,
	Ampersand,
	Minus,
	Number,
	Punctuation,
	Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
	Initial,
	Title,
	AccTitle,
	AccDescription,
	AccDescriptionMultiline,
	MarkdownString,
	String,
	ClassName,
	PointStart,
	PointX,
	PointY,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
	kind: Kind,
	text: &'a str,
	line: usize,
}

/// `" "*<word>" "*`, ignoring case.
fn spaced_word(text: &str, word: &str) -> Option<usize> {
	let spaces = span(text, |character| character == ' ');
	if !starts_with_ignoring_case(&text[spaces..], word) {
		return None;
	}
	let end = spaces + word.len();
	Some(end + span(&text[end..], |character| character == ' '))
}

/// `" "*\-\-+\>" "*`
fn axis_delimiter_length(text: &str) -> Option<usize> {
	let spaces = span(text, |character| character == ' ');
	let dashes = span(&text[spaces..], |character| character == '-');
	let arrow = spaces + dashes;
	if dashes < 2 || !text[arrow..].starts_with('>') {
		return None;
	}
	Some(arrow + 1 + span(&text[arrow + 1..], |character| character == ' '))
}

/// `\s*\:\s*\[\s*`
fn point_start_length(text: &str) -> Option<usize> {
	let colon = span(text, char::is_whitespace);
	if !text[colon..].starts_with(':') {
		return None;
	}
	let bracket = colon + 1 + span(&text[colon + 1..], char::is_whitespace);
	if !text[bracket..].starts_with('[') {
		return None;
	}
	Some(bracket + 1 + span(&text[bracket + 1..], char::is_whitespace))
}

/// `(1)|(0(.\d+)?)`, whose `.` is any character but a line end.
fn coordinate_length(text: &str) -> Option<usize> {
	if text.starts_with('1') {
		return Some(1);
	}
	let after_zero = text.strip_prefix('0')?;
	let fraction =
		after_zero.chars().next().filter(|&character| character != '\n').map_or(0, |any| {
			let digits = span(&after_zero[any.len_utf8()..], |digit| digit.is_ascii_digit());
			if digits > 0 { any.len_utf8() + digits } else { 0 }
		});
	Some(1 + fraction)
}

/// `\s*\]" "*`
fn point_end_length(text: &str) -> Option<usize> {
	let bracket = span(text, char::is_whitespace);
	text[bracket..]
		.starts_with(']')
		.then(|| bracket + 1 + span(&text[bracket + 1..], |character| character == ' '))
}

fn single_character_kind(character: char) -> Option<Kind> {
	Some(match character {
		':' => Kind::Colon,
		'+' => Kind::Plus,
		',' => Kind::Comma,
		'=' => Kind::Equals,
		'*' => Kind::Multiply,
		'#' => Kind::Bracket,
		'_' => Kind::Underscore,
		'.' => Kind::Dot,
		'&' => Kind::Ampersand,
		'-' => Kind::Minus,
		';' => Kind::Semicolon,
		'!' | '"' | '$' | '%' | '\'' | '`' | '?' | '\\' | '/' => Kind::Punctuation,
		_ => return None,
	})
}

/// The rules of the initial state: the token kind (`None` for skipped text and state changes),
/// its length and the state to enter.
fn initial_rule(rest: &str) -> Option<(Option<Kind>, usize, Option<Condition>)> {
	let first = rest.chars().next()?;
	if rest.starts_with("%%") {
		return Some((None, line_length(rest), None));
	}
	if first != '}' && rest[first.len_utf8()..].starts_with("%%") {
		return Some((None, first.len_utf8() + line_length(&rest[first.len_utf8()..]), None));
	}
	if matches!(first, '\n' | '\r') {
		return Some((Some(Kind::NewLine), span(rest, |c| matches!(c, '\n' | '\r')), None));
	}
	if keyword(rest, "title") {
		return Some((Some(Kind::Title), "title".len(), Some(Condition::Title)));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accTitle", ':') {
		return Some((Some(Kind::AccTitle), length, Some(Condition::AccTitle)));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accDescr", ':') {
		return Some((Some(Kind::AccDescription), length, Some(Condition::AccDescription)));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accDescr", '{') {
		return Some((None, length, Some(Condition::AccDescriptionMultiline)));
	}
	if let Some(length) = spaced_word(rest, "x-axis") {
		return Some((Some(Kind::XAxis), length, None));
	}
	if let Some(length) = spaced_word(rest, "y-axis") {
		return Some((Some(Kind::YAxis), length, None));
	}
	if let Some(length) = axis_delimiter_length(rest) {
		return Some((Some(Kind::AxisDelimiter), length, None));
	}
	for (index, word) in ["quadrant-1", "quadrant-2", "quadrant-3", "quadrant-4"].iter().enumerate()
	{
		if let Some(length) = spaced_word(rest, word) {
			return Some((Some(Kind::Quadrant(index)), length, None));
		}
	}
	if keyword(rest, "classDef") {
		return Some((Some(Kind::ClassDef), "classDef".len(), None));
	}
	if rest.starts_with("\"`") {
		return Some((None, 2, Some(Condition::MarkdownString)));
	}
	if first == '"' {
		return Some((None, 1, Some(Condition::String)));
	}
	if rest.starts_with(":::") {
		return Some((None, 3, Some(Condition::ClassName)));
	}
	if let Some(length) = point_start_length(rest) {
		return Some((Some(Kind::PointStart), length, Some(Condition::PointStart)));
	}
	if let Some(length) = spaced_word(rest, "quadrantChart") {
		return Some((Some(Kind::Header), length, None));
	}
	if first.is_ascii_alphabetic() {
		return Some((Some(Kind::Alpha), span(rest, |c| c.is_ascii_alphabetic()), None));
	}
	if !first.is_ascii() {
		return Some((Some(Kind::UnicodeText), span(rest, |c| !c.is_ascii()), None));
	}
	if first.is_ascii_digit() {
		return Some((Some(Kind::Number), span(rest, |c| c.is_ascii_digit()), None));
	}
	// `\s` is tried after the single characters, which do not include whitespace.
	if first.is_whitespace() {
		return Some((Some(Kind::Space), 1, None));
	}
	single_character_kind(first).map(|kind| (Some(kind), 1, None))
}

/// The tokens of `quadrant.jison`. Like jison, the first rule that matches wins; a lexical
/// error ends the tokens with [`Kind::Invalid`].
fn lex(code: &str) -> Vec<Token<'_>> {
	let mut tokens = Vec::new();
	let mut conditions = vec![Condition::Initial];
	let mut position = 0;
	let mut line = 1;
	loop {
		let rest = &code[position..];
		let condition = conditions.last().copied().unwrap_or(Condition::Initial);
		if rest.is_empty() {
			// jison still tries the rules once at the end, where some match the empty text.
			let kind = match condition {
				Condition::Initial => Some(Kind::EndOfFile),
				Condition::Title => Some(Kind::TitleValue),
				Condition::AccTitle => Some(Kind::AccTitleValue),
				Condition::AccDescription => Some(Kind::AccDescriptionValue),
				Condition::AccDescriptionMultiline => Some(Kind::AccDescriptionMultilineValue),
				_ => None,
			};
			if let Some(kind) = kind {
				tokens.push(Token { kind, text: "", line });
			}
			tokens.push(Token { kind: Kind::End, text: "", line });
			return tokens;
		}
		let mut pop = false;
		let mut enter = None;
		let rule = match condition {
			Condition::Initial => initial_rule(rest).map(|(kind, length, next)| {
				enter = next;
				(kind, length)
			}),
			Condition::Title | Condition::AccTitle | Condition::AccDescription => {
				pop = true;
				let kind = match condition {
					Condition::Title => Kind::TitleValue,
					Condition::AccTitle => Kind::AccTitleValue,
					_ => Kind::AccDescriptionValue,
				};
				Some((Some(kind), line_length(rest)))
			}
			Condition::AccDescriptionMultiline => match rest.starts_with('}') {
				true => {
					pop = true;
					Some((None, 1))
				}
				false => Some((
					Some(Kind::AccDescriptionMultilineValue),
					span(rest, |character| character != '}'),
				)),
			},
			Condition::MarkdownString => match rest.starts_with("`\"") {
				true => {
					pop = true;
					Some((None, 2))
				}
				false => {
					let length = span(rest, |character| !matches!(character, '`' | '"'));
					(length > 0).then_some((Some(Kind::MarkdownString), length))
				}
			},
			Condition::String => match rest.starts_with('"') {
				true => {
					pop = true;
					Some((None, 1))
				}
				false => Some((Some(Kind::String), span(rest, |character| character != '"'))),
			},
			Condition::ClassName => {
				pop = true;
				let length = word_length(rest);
				(length > 0).then_some((Some(Kind::ClassName), length))
			}
			Condition::PointStart => match coordinate_length(rest) {
				Some(length) => {
					enter = Some(Condition::PointX);
					Some((Some(Kind::PointX), length))
				}
				None => point_end_length(rest).map(|length| {
					pop = true;
					(None, length)
				}),
			},
			Condition::PointX => {
				let comma = span(rest, char::is_whitespace);
				rest[comma..].starts_with(',').then(|| {
					pop = true;
					enter = Some(Condition::PointY);
					(None, comma + 1 + span(&rest[comma + 1..], char::is_whitespace))
				})
			}
			Condition::PointY => coordinate_length(rest).map(|length| {
				pop = true;
				(Some(Kind::PointY), length)
			}),
		};
		let Some((kind, length)) = rule else {
			tokens.push(Token { kind: Kind::Invalid, text: rest, line });
			tokens.push(Token { kind: Kind::End, text: "", line });
			return tokens;
		};
		if let Some(kind) = kind {
			tokens.push(Token { kind, text: &rest[..length], line });
		}
		line += rest[..length].matches('\n').count();
		position += length;
		if pop {
			conditions.pop();
		}
		if let Some(next) = enter {
			conditions.push(next);
		}
	}
}

/// JavaScript's `Number()` of a coordinate token: `0x…`, `0o…` and `0b…` are integers in their
/// base, other letters make it `NaN`.
fn coordinate_value(text: &str) -> f64 {
	let radix = match text.as_bytes().get(1) {
		Some(b'x' | b'X') => 16,
		Some(b'o' | b'O') => 8,
		Some(b'b' | b'B') => 2,
		_ => return text.parse().unwrap_or(f64::NAN),
	};
	u128::from_str_radix(&text[2..], radix).map_or(f64::NAN, |value| value as f64)
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Styles {
	radius: Option<f64>,
	color: Option<String>,
	stroke_color: Option<String>,
	stroke_width: Option<String>,
}

impl Styles {
	/// `{ ...class, ...point }`: the point's own styles win.
	fn or(&self, class: &Styles) -> Styles {
		Styles {
			radius: self.radius.or(class.radius),
			color: self.color.clone().or_else(|| class.color.clone()),
			stroke_color: self.stroke_color.clone().or_else(|| class.stroke_color.clone()),
			stroke_width: self.stroke_width.clone().or_else(|| class.stroke_width.clone()),
		}
	}
}

#[derive(Clone, Debug, PartialEq)]
struct Point {
	text: String,
	class_name: String,
	x: f64,
	y: f64,
	styles: Styles,
}

#[derive(Debug, Default)]
struct QuadrantChart {
	title: Option<String>,
	accessibility: Accessibility,
	quadrant_texts: [String; 4],
	x_axis_left: String,
	x_axis_right: String,
	y_axis_bottom: String,
	y_axis_top: String,
	/// Newest first, like `addPoints`.
	points: Vec<Point>,
	classes: HashMap<String, Styles>,
}

/// `[key, value] = style.trim().split(/\s*:\s*/)`
fn split_style(style: &str) -> (&str, Option<&str>) {
	let style = style.trim();
	let Some((key, rest)) = style.split_once(':') else {
		return (style, None);
	};
	let rest = rest.trim_start();
	let value = rest.split_once(':').map_or(rest, |(value, _)| value.trim_end());
	(key.trim_end(), Some(value))
}

fn is_hex_color(value: &str) -> bool {
	let digits = value.strip_prefix('#').unwrap_or(value);
	matches!(digits.len(), 3 | 6) && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// `parseStyles`, with the messages of Mermaid's `InvalidStyleError`.
fn parse_styles(styles: &[String]) -> std::result::Result<Styles, String> {
	let mut parsed = Styles::default();
	for style in styles {
		let (key, value) = split_style(style);
		let invalid = |kind: &str| {
			Err(format!(
				"value for {key} {} is invalid, please use a valid {kind}",
				value.unwrap_or("undefined")
			))
		};
		match key {
			"radius" => match value.filter(|value| {
				!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
			}) {
				Some(value) => parsed.radius = Some(value.parse().unwrap_or(f64::INFINITY)),
				None => return invalid("number"),
			},
			"color" | "stroke-color" => match value.filter(|value| is_hex_color(value)) {
				Some(value) if key == "color" => parsed.color = Some(value.to_owned()),
				Some(value) => parsed.stroke_color = Some(value.to_owned()),
				None => return invalid("hex code"),
			},
			"stroke-width" => match value.filter(|value| {
				value.strip_suffix("px").is_some_and(|digits| {
					!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
				})
			}) {
				Some(value) => parsed.stroke_width = Some(value.to_owned()),
				None => return invalid("number of pixels (eg. 10px)"),
			},
			_ => return Err(format!("style named {key} is not supported.")),
		}
	}
	Ok(parsed)
}

fn is_alpha_numeric_token(kind: Kind) -> bool {
	use Kind as K;
	matches!(
		kind,
		K::Punctuation
			| K::Ampersand
			| K::Number
			| K::Alpha
			| K::Comma
			| K::Plus | K::Equals
			| K::Multiply
			| K::Dot | K::Bracket
			| K::Underscore
			| K::UnicodeText
	)
}

fn is_id_token(kind: Kind) -> bool {
	use Kind as K;
	matches!(
		kind,
		K::Alpha
			| K::Number
			| K::Minus
			| K::Comma
			| K::Colon
			| K::Ampersand
			| K::Bracket
			| K::Multiply
			| K::UnicodeText
	)
}

fn is_style_token(kind: Kind) -> bool {
	use Kind as K;
	matches!(kind, K::Alpha | K::Number | K::Colon | K::Space | K::Bracket | K::Minus)
}

fn describe(token: &Token) -> String {
	match token.kind {
		Kind::NewLine => "end of line".to_owned(),
		Kind::EndOfFile | Kind::End => "end of input".to_owned(),
		Kind::Space => "a space".to_owned(),
		Kind::PointStart => "`: [`".to_owned(),
		Kind::PointX | Kind::PointY => format!("the coordinate {:?}", token.text),
		_ => format!("{:?}", token.text.trim()),
	}
}

struct Parser<'a, 'c, 'm> {
	tokens: Vec<Token<'a>>,
	index: usize,
	/// The line after the last token the grammar took, which jison names in parse errors.
	accepted_line: usize,
	context: &'c Context<'m>,
}

impl<'a> Parser<'a, '_, '_> {
	fn current(&self) -> Token<'a> {
		let last = Token { kind: Kind::End, text: "", line: self.accepted_line };
		self.tokens.get(self.index).copied().unwrap_or(last)
	}

	fn is(&self, kind: Kind) -> bool {
		self.current().kind == kind
	}

	fn advance(&mut self) -> Token<'a> {
		let token = self.current();
		self.index += 1;
		self.accepted_line = token.line + token.text.matches('\n').count();
		token
	}

	fn error<T>(&self, expected: &str) -> Result<T> {
		let token = self.current();
		match token.kind {
			Kind::Invalid => Err(self.context.error_at(
				token.line,
				format!("Lexical error on line {}. Unrecognized text.", token.line),
			)),
			_ => Err(self.context.error_at(
				self.accepted_line,
				format!("expected {expected}, found {}", describe(&token)),
			)),
		}
	}

	fn expect(&mut self, kind: Kind, expected: &str) -> Result<Token<'a>> {
		match self.is(kind) {
			true => Ok(self.advance()),
			false => self.error(expected),
		}
	}

	fn is_end_of_line(&self) -> bool {
		matches!(self.current().kind, Kind::NewLine | Kind::Semicolon | Kind::EndOfFile)
	}

	/// `text`: a token and further ones with spaces and minus signs, or one string.
	fn text(&mut self) -> Result<String> {
		let first = self.current();
		match first.kind {
			Kind::String | Kind::MarkdownString => {
				self.advance();
				return Ok(first.text.to_owned());
			}
			kind if is_alpha_numeric_token(kind) => {}
			_ => return self.error("a text"),
		}
		let mut text = String::new();
		while is_alpha_numeric_token(self.current().kind)
			|| (!text.is_empty() && matches!(self.current().kind, Kind::Space | Kind::Minus))
		{
			text.push_str(self.advance().text);
		}
		Ok(text)
	}

	/// `stylesOpt`: styles separated by commas, each trimmed.
	fn styles(&mut self) -> Result<Vec<String>> {
		let mut styles = Vec::new();
		loop {
			if !is_style_token(self.current().kind) {
				return self.error("a style");
			}
			let mut style = String::new();
			while is_style_token(self.current().kind) {
				style.push_str(self.advance().text);
			}
			styles.push(style.trim().to_owned());
			if !self.is(Kind::Comma) {
				return Ok(styles);
			}
			self.advance();
		}
	}

	fn checked_styles(&self, line: usize, styles: &[String]) -> Result<Styles> {
		parse_styles(styles).map_err(|message| self.context.error_at(line, message))
	}

	fn statement(&mut self, chart: &mut QuadrantChart) -> Result<()> {
		while self.is(Kind::Space) {
			self.advance();
		}
		let token = self.current();
		match token.kind {
			Kind::ClassDef => {
				self.advance();
				self.expect(Kind::Space, "a space")?;
				if !is_id_token(self.current().kind) {
					return self.error("a class name");
				}
				let mut name = String::new();
				while is_id_token(self.current().kind) {
					name.push_str(self.advance().text);
				}
				self.expect(Kind::Space, "a space")?;
				let styles = self.styles()?;
				let styles = self.checked_styles(token.line, &styles)?;
				chart.classes.insert(name, styles);
			}
			Kind::XAxis | Kind::YAxis => {
				self.advance();
				let mut first = self.text()?;
				let mut second = None;
				if self.is(Kind::AxisDelimiter) {
					self.advance();
					match self.is_end_of_line() {
						true => first.push_str(" ⟶ "),
						false => second = Some(self.text()?),
					}
				}
				let (start, end) = match token.kind {
					Kind::XAxis => (&mut chart.x_axis_left, &mut chart.x_axis_right),
					_ => (&mut chart.y_axis_bottom, &mut chart.y_axis_top),
				};
				*start = sanitize_text(first.trim());
				if let Some(second) = second {
					*end = sanitize_text(second.trim());
				}
			}
			Kind::Quadrant(index) => {
				self.advance();
				chart.quadrant_texts[index] = sanitize_text(self.text()?.trim());
			}
			Kind::Title => {
				self.advance();
				let value = self.expect(Kind::TitleValue, "a title")?;
				chart.title = Some(sanitize_text(value.text.trim()));
			}
			Kind::AccTitle => {
				self.advance();
				let value = self.expect(Kind::AccTitleValue, "a title")?;
				chart.accessibility.title = sanitize_text(value.text.trim());
			}
			Kind::AccDescription => {
				self.advance();
				let value = self.expect(Kind::AccDescriptionValue, "a description")?;
				chart.accessibility.description = acc_description(value.text);
			}
			Kind::AccDescriptionMultilineValue => {
				self.advance();
				chart.accessibility.description = acc_description(token.text);
			}
			kind if kind == Kind::String
				|| kind == Kind::MarkdownString
				|| is_alpha_numeric_token(kind) =>
			{
				let text = self.text()?;
				let class_name = match self.is(Kind::ClassName) {
					true => self.advance().text.to_owned(),
					false => String::new(),
				};
				self.expect(Kind::PointStart, "`: [`")?;
				let x = self.expect(Kind::PointX, "a coordinate (0 to 1)")?;
				let y = self.expect(Kind::PointY, "a coordinate (0 to 1)")?;
				let styles = match self.is_end_of_line() {
					true => Vec::new(),
					false => self.styles()?,
				};
				let styles = self.checked_styles(token.line, &styles)?;
				chart.points.insert(
					0,
					Point {
						text: sanitize_text(text.trim()),
						class_name,
						x: coordinate_value(x.text),
						y: coordinate_value(y.text),
						styles,
					},
				);
			}
			_ => {}
		}
		match self.is_end_of_line() {
			true => {
				self.advance();
				Ok(())
			}
			false => self.error("end of line"),
		}
	}
}

fn parse(code: &str, context: &Context) -> Result<QuadrantChart> {
	let mut chart = QuadrantChart {
		title: context.title.as_deref().map(sanitize_text),
		..QuadrantChart::default()
	};
	let mut parser = Parser { tokens: lex(code), index: 0, accepted_line: 1, context };
	while matches!(
		parser.current().kind,
		Kind::NewLine | Kind::Semicolon | Kind::EndOfFile | Kind::Space
	) {
		parser.advance();
	}
	parser.expect(Kind::Header, "`quadrantChart`")?;
	while !parser.is(Kind::End) {
		parser.statement(&mut chart)?;
	}
	Ok(chart)
}

/// `quadrantChart` configuration with the defaults of `QuadrantBuilder`.
struct Settings {
	chart_width: f64,
	chart_height: f64,
	title_padding: f64,
	title_font_size: f64,
	quadrant_padding: f64,
	x_axis_label_padding: f64,
	y_axis_label_padding: f64,
	x_axis_label_font_size: f64,
	y_axis_label_font_size: f64,
	quadrant_label_font_size: f64,
	quadrant_text_top_padding: f64,
	point_text_padding: f64,
	point_label_font_size: f64,
	point_radius: f64,
	x_axis_position: String,
	y_axis_position: String,
	internal_border_width: f64,
	external_border_width: f64,
	show_x_axis: bool,
	show_y_axis: bool,
	show_title: bool,
}

impl Settings {
	fn new(context: &Context) -> Self {
		let number = |key: &str, default: f64| context.config_number("quadrantChart", key, default);
		let flag = |key: &str| context.config_bool("quadrantChart", key, true);
		let text = |key: &str, default: &str| {
			context.config_text("quadrantChart", key).unwrap_or_else(|| default.to_owned())
		};
		Settings {
			chart_width: number("chartWidth", 500.0),
			chart_height: number("chartHeight", 500.0),
			title_padding: number("titlePadding", 10.0),
			title_font_size: number("titleFontSize", 20.0),
			quadrant_padding: number("quadrantPadding", 5.0),
			x_axis_label_padding: number("xAxisLabelPadding", 5.0),
			y_axis_label_padding: number("yAxisLabelPadding", 5.0),
			x_axis_label_font_size: number("xAxisLabelFontSize", 16.0),
			y_axis_label_font_size: number("yAxisLabelFontSize", 16.0),
			quadrant_label_font_size: number("quadrantLabelFontSize", 16.0),
			quadrant_text_top_padding: number("quadrantTextTopPadding", 5.0),
			point_text_padding: number("pointTextPadding", 5.0),
			point_label_font_size: number("pointLabelFontSize", 12.0),
			point_radius: number("pointRadius", 5.0),
			x_axis_position: text("xAxisPosition", "top"),
			y_axis_position: text("yAxisPosition", "left"),
			internal_border_width: number("quadrantInternalBorderStrokeWidth", 1.0),
			external_border_width: number("quadrantExternalBorderStrokeWidth", 2.0),
			show_x_axis: flag("showXAxis"),
			show_y_axis: flag("showYAxis"),
			show_title: flag("showTitle"),
		}
	}
}

/// A text of `QuadrantBuilder`: `verticalPos` `left` is `start`, `top` is `hanging`.
struct Label<'a> {
	text: &'a str,
	fill: String,
	x: f64,
	y: f64,
	font_size: f64,
	is_centered: bool,
	is_top: bool,
	rotation: f64,
}

struct Renderer<'c, 'm> {
	context: &'c Context<'m>,
	font_family: String,
}

impl Renderer<'_, '_> {
	/// A theme color, which browsers ignore when it is not valid (such as the default
	/// `quadrantPointFill`, `hsl(240, 100%, NaN%)`).
	fn color(&self, key: &str) -> String {
		valid_color(self.context.theme.css(key))
	}

	fn label(&self, svg: &mut Svg, label: &Label) {
		let font = Font::new(self.font_family.clone(), label.font_size);
		let run = TextRun {
			x: 0.0,
			y: 0.0,
			anchor: if label.is_centered { Anchor::Middle } else { Anchor::Start },
			baseline: if label.is_top { Baseline::Hanging } else { Baseline::Middle },
			font: &font,
			content: label.text,
		};
		svg.text(run, |svg| {
			svg.attr_if("fill", &label.fill);
			// Browsers drop a transform with `NaN`.
			if label.x.is_finite() && label.y.is_finite() {
				let transform = format!(
					"translate({}, {}) rotate({})",
					number(label.x),
					number(label.y),
					number(label.rotation)
				);
				svg.attr("transform", &transform);
			}
		});
	}
}

fn valid_color(color: String) -> String {
	match color::is_color(&color) {
		true => color,
		false => String::new(),
	}
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let chart = parse(code, context)?;
	let settings = Settings::new(context);
	let renderer = Renderer { context, font_family: context.font_family() };
	let title = chart.title.clone().unwrap_or_default();
	let show_x_axis =
		settings.show_x_axis && !(chart.x_axis_left.is_empty() && chart.x_axis_right.is_empty());
	let show_y_axis =
		settings.show_y_axis && !(chart.y_axis_top.is_empty() && chart.y_axis_bottom.is_empty());
	let show_title = settings.show_title && !title.is_empty();
	let x_axis_position = match chart.points.is_empty() {
		true => settings.x_axis_position.as_str(),
		false => "bottom",
	};
	let is_x_axis_top = x_axis_position == "top";
	let is_y_axis_left = settings.y_axis_position == "left";

	// `calculateSpace`
	let x_axis_space = settings.x_axis_label_padding * 2.0 + settings.x_axis_label_font_size;
	let x_axis_top = if is_x_axis_top && show_x_axis { x_axis_space } else { 0.0 };
	let x_axis_bottom = if x_axis_position == "bottom" && show_x_axis { x_axis_space } else { 0.0 };
	let y_axis_space = settings.y_axis_label_padding * 2.0 + settings.y_axis_label_font_size;
	let y_axis_left = if is_y_axis_left && show_y_axis { y_axis_space } else { 0.0 };
	let y_axis_right =
		if settings.y_axis_position == "right" && show_y_axis { y_axis_space } else { 0.0 };
	let title_top = match show_title {
		true => settings.title_font_size + settings.title_padding * 2.0,
		false => 0.0,
	};
	let padding = settings.quadrant_padding;
	let left = padding + y_axis_left;
	let top = padding + x_axis_top + title_top;
	let width = settings.chart_width - padding * 2.0 - y_axis_left - y_axis_right;
	let height = settings.chart_height - padding * 2.0 - x_axis_top - x_axis_bottom - title_top;
	let (half_width, half_height) = (width / 2.0, height / 2.0);

	let mut body = Svg::new();
	body.open("g").attr("class", "main").end();

	body.open("g").attr("class", "quadrants").end();
	let quadrant_origins = [
		(left + half_width, top),
		(left, top),
		(left, top + half_height),
		(left + half_width, top + half_height),
	];
	for (index, (x, y)) in quadrant_origins.into_iter().enumerate() {
		body.open("g").attr("class", "quadrant").end();
		body.open("rect")
			.number_attr("x", x)
			.number_attr("y", y)
			.number_attr("width", half_width)
			.number_attr("height", half_height)
			.attr_if("fill", &renderer.color(&format!("quadrant{}Fill", index + 1)))
			.close();
		let is_top = !chart.points.is_empty();
		let label = Label {
			text: &chart.quadrant_texts[index],
			fill: renderer.color(&format!("quadrant{}TextFill", index + 1)),
			x: x + half_width / 2.0,
			y: match is_top {
				true => y + settings.quadrant_text_top_padding,
				false => y + half_height / 2.0,
			},
			font_size: settings.quadrant_label_font_size,
			is_centered: true,
			is_top,
			rotation: 0.0,
		};
		renderer.label(&mut body, &label);
		body.close_tag("g");
	}
	body.close_tag("g");

	// `getBorders`
	body.open("g").attr("class", "border").end();
	let half_border = settings.external_border_width / 2.0;
	let external = (settings.external_border_width, "quadrantExternalBorderStrokeFill");
	let internal = (settings.internal_border_width, "quadrantInternalBorderStrokeFill");
	let borders = [
		(external, [left - half_border, top, left + width + half_border, top]),
		(external, [left + width, top + half_border, left + width, top + height - half_border]),
		(external, [left - half_border, top + height, left + width + half_border, top + height]),
		(external, [left, top + half_border, left, top + height - half_border]),
		(
			internal,
			[left + half_width, top + half_border, left + half_width, top + height - half_border],
		),
		(
			internal,
			[left + half_border, top + half_height, left + width - half_border, top + half_height],
		),
	];
	for ((stroke_width, color_key), [x1, y1, x2, y2]) in borders {
		body.open("line")
			.number_attr("x1", x1)
			.number_attr("y1", y1)
			.number_attr("x2", x2)
			.number_attr("y2", y2)
			.attr_if("stroke", &renderer.color(color_key))
			.number_attr("stroke-width", stroke_width)
			.close();
	}
	body.close_tag("g");

	// `getQuadrantPoints`
	body.open("g").attr("class", "data-points").end();
	let point_fill = renderer.color("quadrantPointFill");
	let point_text_fill = renderer.color("quadrantPointTextFill");
	let no_styles = Styles::default();
	for point in &chart.points {
		let class = chart.classes.get(&point.class_name).unwrap_or(&no_styles);
		let styles = point.styles.or(class);
		// d3's `scaleLinear` interpolates `a * (1 - t) + b * t`.
		let x = left * (1.0 - point.x) + (left + width) * point.x;
		let y = (top + height) * (1.0 - point.y) + top * point.y;
		body.open("g").attr("class", "data-point").end();
		let fill = styles.color.map_or_else(|| point_fill.clone(), valid_color);
		let stroke = styles.stroke_color.map_or_else(|| point_fill.clone(), valid_color);
		body.open("circle")
			.number_attr("cx", x)
			.number_attr("cy", y)
			.number_attr("r", styles.radius.unwrap_or(settings.point_radius))
			.attr_if("fill", &fill)
			.attr_if("stroke", &stroke)
			.attr("stroke-width", styles.stroke_width.as_deref().unwrap_or("0px"))
			.close();
		let label = Label {
			text: &point.text,
			fill: point_text_fill.clone(),
			x,
			y: y + settings.point_text_padding,
			font_size: settings.point_label_font_size,
			is_centered: true,
			is_top: true,
			rotation: 0.0,
		};
		renderer.label(&mut body, &label);
		body.close_tag("g");
	}
	body.close_tag("g");

	// `getAxisLabels`
	body.open("g").attr("class", "labels").end();
	let x_axis_y = match is_x_axis_top {
		true => settings.x_axis_label_padding + title_top,
		false => settings.x_axis_label_padding + top + height + padding,
	};
	let x_in_middle = !chart.x_axis_right.is_empty();
	let x_offset = if x_in_middle { half_width / 2.0 } else { 0.0 };
	let x_fill = renderer.color("quadrantXAxisTextFill");
	let y_axis_x = match is_y_axis_left {
		true => settings.y_axis_label_padding,
		false => settings.y_axis_label_padding + left + width + padding,
	};
	let y_in_middle = !chart.y_axis_top.is_empty();
	let y_offset = if y_in_middle { half_height / 2.0 } else { 0.0 };
	let y_fill = renderer.color("quadrantYAxisTextFill");
	let axis_labels = [
		(show_x_axis, &chart.x_axis_left, left + x_offset, x_axis_y, true),
		(show_x_axis, &chart.x_axis_right, left + half_width + x_offset, x_axis_y, true),
		(show_y_axis, &chart.y_axis_bottom, y_axis_x, top + height - y_offset, false),
		(show_y_axis, &chart.y_axis_top, y_axis_x, top + half_height - y_offset, false),
	];
	for (is_shown, text, x, y, is_x_axis) in axis_labels {
		if !is_shown || text.is_empty() {
			continue;
		}
		let label = Label {
			text,
			fill: if is_x_axis { x_fill.clone() } else { y_fill.clone() },
			x,
			y,
			font_size: match is_x_axis {
				true => settings.x_axis_label_font_size,
				false => settings.y_axis_label_font_size,
			},
			is_centered: if is_x_axis { x_in_middle } else { y_in_middle },
			is_top: true,
			rotation: if is_x_axis { 0.0 } else { -90.0 },
		};
		body.open("g").attr("class", "label").end();
		renderer.label(&mut body, &label);
		body.close_tag("g");
	}
	body.close_tag("g");

	body.open("g").attr("class", "title").end();
	if show_title {
		let label = Label {
			text: &title,
			fill: renderer.color("quadrantTitleFill"),
			x: settings.chart_width / 2.0,
			y: settings.title_padding,
			font_size: settings.title_font_size,
			is_centered: true,
			is_top: true,
			rotation: 0.0,
		};
		renderer.label(&mut body, &label);
	}
	body.close_tag("g");
	body.close_tag("g");

	let frame = Frame {
		x: 0.0,
		y: 0.0,
		width: settings.chart_width,
		height: settings.chart_height,
		use_max_width: context.config_bool("quadrantChart", "useMaxWidth", true),
	};
	Ok(document(context, "quadrantChart", &frame, &chart.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<QuadrantChart> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	fn kinds(code: &str) -> Vec<Kind> {
		lex(code).iter().map(|token| token.kind).collect()
	}

	#[test]
	fn lexes_like_jison() {
		use Kind as K;
		assert_eq!(
			kinds("quadrantChart\n  A:::c: [0.5, 1] radius: 3"),
			[
				K::Header,
				K::NewLine,
				K::Space,
				K::Space,
				K::Alpha,
				K::ClassName,
				K::PointStart,
				K::PointX,
				K::PointY,
				K::Alpha,
				K::Colon,
				K::Space,
				K::Number,
				K::EndOfFile,
				K::End
			]
		);
		assert_eq!(kinds("titleX x-axis"), [K::Alpha, K::XAxis, K::EndOfFile, K::End]);
		assert_eq!(kinds("title"), [K::Title, K::TitleValue, K::End]);
		assert_eq!(kinds("A: [1.5"), [K::Alpha, K::PointStart, K::PointX, K::Invalid, K::End]);
		assert_eq!(kinds("a --> b"), [K::Alpha, K::AxisDelimiter, K::Alpha, K::EndOfFile, K::End]);
	}

	#[test]
	fn reads_coordinates_like_javascript() {
		assert_eq!(coordinate_value("0.25"), 0.25);
		assert_eq!(coordinate_value("1"), 1.0);
		assert_eq!(coordinate_value("0x5"), 5.0);
		assert_eq!(coordinate_value("0e5"), 0.0);
		assert!(coordinate_value("0a5").is_nan());
		assert!(coordinate_value("0o9").is_nan());
		assert_eq!(coordinate_length("0.5]"), Some(3));
		assert_eq!(coordinate_length("0.]"), Some(1));
	}

	#[test]
	fn parses_axes_quadrants_points_and_classes() {
		let chart = parsed(
			"quadrantChart\n  title Reach & <b>x</b>\n  x-axis Low Reach --> High\n  y-axis Engagement -->\n  quadrant-1 We should expand\n  A:::c1: [0.3, 0.6] radius: 20\n  B: [0.45, 0.23]  stroke-width  :  3px , color: #ff0000\n  classDef c1 color: #908342, radius : 10",
		)
		.unwrap();
		assert_eq!(chart.title.as_deref(), Some("Reach &amp; <b>x</b>"));
		assert_eq!(
			(chart.x_axis_left.as_str(), chart.x_axis_right.as_str()),
			("Low Reach", "High")
		);
		assert_eq!(chart.y_axis_bottom, "Engagement ⟶");
		assert_eq!(chart.quadrant_texts[0], "We should expand");
		let names: Vec<_> = chart.points.iter().map(|point| point.text.as_str()).collect();
		assert_eq!(names, ["B", "A"]);
		assert_eq!(chart.points[0].styles.stroke_width.as_deref(), Some("3px"));
		assert_eq!(chart.points[0].styles.color.as_deref(), Some("#ff0000"));
		assert_eq!(chart.points[1].class_name, "c1");
		assert_eq!(chart.classes["c1"].radius, Some(10.0));
	}

	#[test]
	fn rejects_what_mermaid_rejects() {
		let error = |code: &str| parsed(code).err().unwrap().message;
		assert_eq!(
			error("quadrantChart\nA: [0.5, 0.5] foo: 3"),
			"style named foo is not supported."
		);
		assert_eq!(
			error("quadrantChart\nA: [0.5, 0.5] radius: x"),
			"value for radius x is invalid, please use a valid number"
		);
		assert_eq!(
			error("quadrantChart\nA: [0.5, 0.5] color"),
			"value for color undefined is invalid, please use a valid hex code"
		);
		assert_eq!(
			error("quadrantChart\n  A: [1.5, 0.2]"),
			"Lexical error on line 2. Unrecognized text."
		);
		for code in [
			"x-axis A",
			"quadrantChart\nx-axis",
			"quadrantChart\n\"a\" b: [0.1, 0.1]",
			"quadrantChart\nA: [0.1, 0.1] radius: 1,",
			"quadrantChart\nclassDef  c radius: 1",
			"quadrantChart\ntitle",
		] {
			assert!(parsed(code).is_err(), "{code}");
		}
	}
}
