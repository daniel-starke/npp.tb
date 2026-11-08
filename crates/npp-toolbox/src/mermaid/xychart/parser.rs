//! The XY chart grammar (`xychart.jison`) and its database (`xychartDb.ts`).

use super::super::color;
use super::super::common::{acc_description, js_number_of, sanitize_text};
use super::super::jison::{keyword, keyword_and_delimiter, line_length, span};
use super::super::svg::Accessibility;
use super::super::{Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
	NewLine,
	Semicolon,
	/// The grammar's `EOF` terminal, which the lexer returns once in its initial state.
	EndOfFile,
	/// jison's end of input after `EOF`, or at the end in any other state.
	End,
	Title,
	AccTitle,
	AccTitleValue,
	AccDescription,
	AccDescriptionValue,
	AccDescriptionMultilineValue,
	Header,
	Orientation,
	XAxis,
	YAxis,
	BracketOpen,
	BracketClose,
	Arrow,
	Line,
	Bar,
	/// `NUMBER_WITH_DECIMAL`, only lexed in axis data and inside plot data.
	Number,
	String,
	Alpha,
	Colon,
	Plus,
	Comma,
	Equals,
	Multiply,
	Hash,
	Underscore,
	Dot,
	Ampersand,
	Minus,
	/// `NUM`: digits where no `NUMBER_WITH_DECIMAL` is lexed.
	Digits,
	Invalid,
}

/// The lexer's start conditions. `axis_data`, `axis_band_data`, `data` and `data_inner` are
/// inclusive: the rules without condition apply there too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
	Initial,
	String,
	AccTitle,
	AccDescription,
	AccDescriptionMultiline,
	AxisData,
	AxisBandData,
	Data,
	DataInner,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Token<'a> {
	pub kind: Kind,
	pub text: &'a str,
	pub line: usize,
}

/// What a rule does besides returning its token.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
	None,
	Push(Condition),
	Pop,
}

/// `[+-]?(?:\d+(?:\.\d+)?|\.\d+)`
fn number_length(text: &str) -> Option<usize> {
	let sign = usize::from(text.starts_with(['+', '-']));
	let rest = &text[sign..];
	let digits = span(rest, |character| character.is_ascii_digit());
	let fraction = |after: &str| match after.strip_prefix('.') {
		Some(decimals) => {
			let length = span(decimals, |character| character.is_ascii_digit());
			if length > 0 { 1 + length } else { 0 }
		}
		None => 0,
	};
	match digits {
		0 => {
			let length = fraction(rest);
			(length > 0).then_some(sign + length)
		}
		_ => Some(sign + digits + fraction(&rest[digits..])),
	}
}

fn single_character_kind(character: char) -> Option<Kind> {
	Some(match character {
		'[' => Kind::BracketOpen,
		']' => Kind::BracketClose,
		':' => Kind::Colon,
		'+' => Kind::Plus,
		',' => Kind::Comma,
		'=' => Kind::Equals,
		'*' => Kind::Multiply,
		'#' => Kind::Hash,
		'_' => Kind::Underscore,
		'.' => Kind::Dot,
		'&' => Kind::Ampersand,
		'-' => Kind::Minus,
		_ => return None,
	})
}

/// The rules of the initial state and the inclusive ones, in grammar order: the token kind
/// (`None` for skipped text), its length and the action.
fn inclusive_rule(rest: &str, condition: Condition) -> Option<(Option<Kind>, usize, Action)> {
	use Condition as C;
	let first = rest.chars().next()?;
	if rest.starts_with("%%") && !rest[2..].starts_with('{') {
		return Some((None, line_length(rest), Action::None));
	}
	if first != '}' && rest[first.len_utf8()..].starts_with("%%") {
		return Some((
			None,
			first.len_utf8() + line_length(&rest[first.len_utf8()..]),
			Action::None,
		));
	}
	if matches!(condition, C::AxisData | C::Data) {
		let length = if rest.starts_with("\r\n") { 2 } else { usize::from(first == '\n') };
		if length > 0 {
			return Some((Some(Kind::NewLine), length, Action::Pop));
		}
	}
	if matches!(first, '\n' | '\r') {
		return Some((Some(Kind::NewLine), span(rest, |c| matches!(c, '\n' | '\r')), Action::None));
	}
	if rest.starts_with("%%") {
		return Some((None, line_length(rest), Action::None));
	}
	if keyword(rest, "title") {
		return Some((Some(Kind::Title), "title".len(), Action::None));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accTitle", ':') {
		return Some((Some(Kind::AccTitle), length, Action::Push(C::AccTitle)));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accDescr", ':') {
		return Some((Some(Kind::AccDescription), length, Action::Push(C::AccDescription)));
	}
	if let Some(length) = keyword_and_delimiter(rest, "accDescr", '{') {
		return Some((None, length, Action::Push(C::AccDescriptionMultiline)));
	}
	for header in ["xychart-beta", "xychart"] {
		if keyword(rest, header) {
			return Some((Some(Kind::Header), header.len(), Action::None));
		}
	}
	// `(?:"vertical"|"horizontal")` ends with a parenthesis, so jison adds no word boundary.
	for orientation in ["vertical", "horizontal"] {
		if rest
			.get(..orientation.len())
			.is_some_and(|start| start.eq_ignore_ascii_case(orientation))
		{
			return Some((Some(Kind::Orientation), orientation.len(), Action::None));
		}
	}
	for (word, kind) in [("x-axis", Kind::XAxis), ("y-axis", Kind::YAxis)] {
		if keyword(rest, word) {
			return Some((Some(kind), word.len(), Action::Push(C::AxisData)));
		}
	}
	if condition == C::AxisData {
		if first == '[' {
			return Some((Some(Kind::BracketOpen), 1, Action::Push(C::AxisBandData)));
		}
		if rest.starts_with("-->") {
			return Some((Some(Kind::Arrow), 3, Action::None));
		}
	}
	for (word, kind) in [("line", Kind::Line), ("bar", Kind::Bar)] {
		if keyword(rest, word) {
			return Some((Some(kind), word.len(), Action::Push(C::Data)));
		}
	}
	if condition == C::Data && first == '[' {
		return Some((Some(Kind::BracketOpen), 1, Action::Push(C::DataInner)));
	}
	if matches!(condition, C::AxisData | C::DataInner)
		&& let Some(length) = number_length(rest)
	{
		return Some((Some(Kind::Number), length, Action::None));
	}
	if matches!(condition, C::DataInner | C::AxisBandData) && first == ']' {
		return Some((Some(Kind::BracketClose), 1, Action::Pop));
	}
	if first == '"' {
		return Some((None, 1, Action::Push(C::String)));
	}
	if first.is_ascii_alphabetic() {
		return Some((Some(Kind::Alpha), span(rest, |c| c.is_ascii_alphabetic()), Action::None));
	}
	if let Some(kind) = single_character_kind(first) {
		return Some((Some(kind), 1, Action::None));
	}
	if first.is_ascii_digit() {
		return Some((Some(Kind::Digits), span(rest, |c| c.is_ascii_digit()), Action::None));
	}
	if first.is_whitespace() {
		return Some((None, span(rest, char::is_whitespace), Action::None));
	}
	(first == ';').then_some((Some(Kind::Semicolon), 1, Action::None))
}

/// The tokens of `xychart.jison`. Like jison, the first rule that matches wins; a lexical error
/// ends the tokens with [`Kind::Invalid`].
pub(super) fn lex(code: &str) -> Vec<Token<'_>> {
	use Condition as C;
	let mut tokens = Vec::new();
	let mut conditions = vec![C::Initial];
	let mut position = 0;
	let mut line = 1;
	loop {
		let rest = &code[position..];
		let condition = conditions.last().copied().unwrap_or(C::Initial);
		if rest.is_empty() {
			// jison still tries the rules once at the end, where some match the empty text.
			let kind = match condition {
				C::String => Some(Kind::String),
				C::AccTitle => Some(Kind::AccTitleValue),
				C::AccDescription => Some(Kind::AccDescriptionValue),
				C::AccDescriptionMultiline => Some(Kind::AccDescriptionMultilineValue),
				_ => Some(Kind::EndOfFile),
			};
			if let Some(kind) = kind {
				tokens.push(Token { kind, text: "", line });
			}
			tokens.push(Token { kind: Kind::End, text: "", line });
			return tokens;
		}
		let rule = match condition {
			C::String => Some(match rest.starts_with('"') {
				true => (None, 1, Action::Pop),
				false => {
					(Some(Kind::String), span(rest, |character| character != '"'), Action::None)
				}
			}),
			C::AccTitle => Some((Some(Kind::AccTitleValue), line_length(rest), Action::Pop)),
			C::AccDescription => {
				Some((Some(Kind::AccDescriptionValue), line_length(rest), Action::Pop))
			}
			C::AccDescriptionMultiline => Some(match rest.starts_with('}') {
				true => (None, 1, Action::Pop),
				false => (
					Some(Kind::AccDescriptionMultilineValue),
					span(rest, |character| character != '}'),
					Action::None,
				),
			}),
			_ => inclusive_rule(rest, condition),
		};
		let Some((kind, length, action)) = rule else {
			tokens.push(Token { kind: Kind::Invalid, text: rest, line });
			tokens.push(Token { kind: Kind::End, text: "", line });
			return tokens;
		};
		if let Some(kind) = kind {
			tokens.push(Token { kind, text: &rest[..length], line });
		}
		line += rest[..length].matches('\n').count();
		position += length;
		match action {
			Action::None => {}
			Action::Push(next) => conditions.push(next),
			Action::Pop => _ = conditions.pop(),
		}
	}
}

/// The data of an x axis, which the statements and plots change in order.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum XAxisData {
	Band { categories: Vec<String> },
	Linear { min: f64, max: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PlotKind {
	Line { point_labels: Option<Vec<String>> },
	Bar,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Plot {
	pub kind: PlotKind,
	pub title: String,
	/// `strokeFill` of lines, `fill` of bars.
	pub color: String,
	/// The x value (a category or a number as text) and the y value, which is missing where a
	/// band axis has more categories than the plot values.
	pub data: Vec<(String, Option<f64>)>,
}

#[derive(Debug)]
pub(super) struct XyChart {
	pub is_horizontal: Option<bool>,
	pub title: String,
	pub accessibility: Accessibility,
	pub x_axis: XAxisData,
	pub x_axis_title: String,
	pub y_axis_title: String,
	pub y_min: f64,
	pub y_max: f64,
	pub plots: Vec<Plot>,
	has_set_x_axis: bool,
	has_set_y_axis: bool,
	palette: Vec<String>,
}

impl XyChart {
	fn new(context: &Context) -> Self {
		let palette = context
			.theme
			.get("xyChart")
			.and_then(|colors| colors.get("plotColorPalette"))
			.and_then(|palette| palette.to_text())
			.unwrap_or_default();
		XyChart {
			is_horizontal: None,
			title: context.title.as_deref().map(sanitize_text).unwrap_or_default(),
			accessibility: Accessibility::default(),
			x_axis: XAxisData::Band { categories: Vec::new() },
			x_axis_title: String::new(),
			y_axis_title: String::new(),
			y_min: f64::INFINITY,
			y_max: f64::NEG_INFINITY,
			plots: Vec::new(),
			has_set_x_axis: false,
			has_set_y_axis: false,
			palette: palette.split(',').map(|color| color.trim().to_owned()).collect(),
		}
	}

	fn set_x_axis_range(&mut self, min: f64, max: f64) {
		self.x_axis = XAxisData::Linear { min, max };
		self.has_set_x_axis = true;
	}

	fn set_y_axis_range(&mut self, min: f64, max: f64) {
		(self.y_min, self.y_max) = (min, max);
		self.has_set_y_axis = true;
	}

	/// `transformDataWithoutCategory`
	fn transform(&mut self, mut values: Vec<f64>) -> Vec<(String, Option<f64>)> {
		if values.is_empty() {
			return Vec::new();
		}
		if !self.has_set_x_axis {
			let (min, max) = match self.x_axis {
				XAxisData::Linear { min, max } => (min, max),
				XAxisData::Band { .. } => (f64::INFINITY, f64::NEG_INFINITY),
			};
			self.set_x_axis_range(js_min(min, 1.0), js_max(max, values.len() as f64));
		}
		if let XAxisData::Band { categories } = &self.x_axis {
			values.truncate(categories.len());
		}
		if !self.has_set_y_axis {
			for &value in &values {
				(self.y_min, self.y_max) = (js_min(self.y_min, value), js_max(self.y_max, value));
			}
		}
		match &self.x_axis {
			XAxisData::Band { categories } => categories
				.iter()
				.enumerate()
				.map(|(index, category)| (category.clone(), values.get(index).copied()))
				.collect(),
			&XAxisData::Linear { min, max } => {
				if values.len() == 1 {
					return vec![(color::js_number(min), Some(values[0]))];
				}
				let step = (max - min) / (values.len() - 1) as f64;
				values
					.iter()
					.enumerate()
					.map(|(index, &value)| {
						(color::js_number(min + index as f64 * step), Some(value))
					})
					.collect()
			}
		}
	}

	fn add_plot(&mut self, kind: PlotKind, title: &str, values: Vec<f64>) {
		let data = self.transform(values);
		let index = self.plots.len();
		let color = match self.palette.len() {
			0 => String::new(),
			length => self.palette[index % length].clone(),
		};
		self.plots.push(Plot { kind, title: sanitize_text(title.trim()), color, data });
	}
}

/// `Math.min`, which is `NaN` when either is.
fn js_min(first: f64, second: f64) -> f64 {
	if first.is_nan() || second.is_nan() { f64::NAN } else { first.min(second) }
}

fn js_max(first: f64, second: f64) -> f64 {
	if first.is_nan() || second.is_nan() { f64::NAN } else { first.max(second) }
}

fn is_alpha_numeric_token(kind: Kind) -> bool {
	use Kind as K;
	matches!(
		kind,
		K::Ampersand
			| K::Digits
			| K::Alpha
			| K::Plus | K::Equals
			| K::Multiply
			| K::Dot | K::Hash
			| K::Minus
			| K::Underscore
	)
}

fn is_text_start(kind: Kind) -> bool {
	kind == Kind::String || is_alpha_numeric_token(kind)
}

fn describe(token: &Token) -> String {
	match token.kind {
		Kind::NewLine => "end of line".to_owned(),
		Kind::EndOfFile | Kind::End => "end of input".to_owned(),
		Kind::String => format!("the string {:?}", token.text),
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

	/// `text`: one string, or tokens joined without the whitespace between them.
	fn text(&mut self) -> Result<String> {
		let first = self.current();
		match first.kind {
			Kind::String => {
				self.advance();
				return Ok(first.text.to_owned());
			}
			kind if is_alpha_numeric_token(kind) => {}
			_ => return self.error("a text"),
		}
		let mut text = String::new();
		while is_alpha_numeric_token(self.current().kind) {
			text.push_str(self.advance().text);
		}
		Ok(text)
	}

	/// `NUMBER_WITH_DECIMAL ARROW_DELIMITER NUMBER_WITH_DECIMAL`
	fn range(&mut self) -> Result<(f64, f64)> {
		let min = self.expect(Kind::Number, "a number")?;
		self.expect(Kind::Arrow, "`-->`")?;
		let max = self.expect(Kind::Number, "a number")?;
		Ok((js_number_of(min.text), js_number_of(max.text)))
	}

	/// `plotData`: the values and their labels.
	fn plot_data(&mut self) -> Result<(Vec<f64>, Vec<String>)> {
		self.expect(Kind::BracketOpen, "`[`")?;
		let (mut values, mut labels) = (Vec::new(), Vec::new());
		loop {
			let value = self.expect(Kind::Number, "a number")?;
			values.push(js_number_of(value.text));
			labels.push(match self.is(Kind::String) {
				true => sanitize_text(self.advance().text.trim()),
				false => String::new(),
			});
			match self.current().kind {
				Kind::Comma => _ = self.advance(),
				Kind::BracketClose => {
					self.advance();
					return Ok((values, labels));
				}
				_ => return self.error("`,` or `]`"),
			}
		}
	}

	fn statement(&mut self, chart: &mut XyChart) -> Result<()> {
		let token = self.current();
		match token.kind {
			Kind::Title => {
				self.advance();
				chart.title = sanitize_text(self.text()?.trim());
			}
			Kind::XAxis => {
				self.advance();
				let title = match is_text_start(self.current().kind) {
					true => Some(self.text()?),
					false => None,
				};
				if title.is_none()
					|| matches!(self.current().kind, Kind::BracketOpen | Kind::Number)
				{
					match self.is(Kind::BracketOpen) {
						true => {
							self.advance();
							let mut categories = vec![sanitize_text(self.text()?.trim())];
							while self.is(Kind::Comma) {
								self.advance();
								categories.push(sanitize_text(self.text()?.trim()));
							}
							self.expect(Kind::BracketClose, "`,` or `]`")?;
							chart.x_axis = XAxisData::Band { categories };
							chart.has_set_x_axis = true;
						}
						false => {
							let (min, max) = self.range()?;
							chart.set_x_axis_range(min, max);
						}
					}
				}
				chart.x_axis_title = sanitize_text(title.unwrap_or_default().trim());
			}
			Kind::YAxis => {
				self.advance();
				let title = match is_text_start(self.current().kind) {
					true => Some(self.text()?),
					false => None,
				};
				if title.is_none() || self.is(Kind::Number) {
					let (min, max) = self.range()?;
					chart.set_y_axis_range(min, max);
				}
				chart.y_axis_title = sanitize_text(title.unwrap_or_default().trim());
			}
			Kind::Line | Kind::Bar => {
				self.advance();
				let title = match is_text_start(self.current().kind) {
					true => self.text()?,
					false => String::new(),
				};
				let (values, labels) = self.plot_data()?;
				let kind = match token.kind {
					Kind::Line => PlotKind::Line {
						point_labels: labels
							.iter()
							.any(|label| !label.is_empty())
							.then_some(labels),
					},
					_ => PlotKind::Bar,
				};
				chart.add_plot(kind, &title, values);
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
			_ => return self.error("a statement"),
		}
		while self.is_end_of_line() {
			self.advance();
		}
		Ok(())
	}
}

pub(super) fn parse(code: &str, context: &Context) -> Result<XyChart> {
	let mut chart = XyChart::new(context);
	let mut parser = Parser { tokens: lex(code), index: 0, accepted_line: 1, context };
	// `start`: line ends and headers, each with an optional orientation.
	loop {
		match parser.current().kind {
			Kind::NewLine | Kind::Semicolon | Kind::EndOfFile => _ = parser.advance(),
			Kind::Header => {
				parser.advance();
				if parser.is(Kind::Orientation) {
					chart.is_horizontal =
						Some(parser.advance().text.eq_ignore_ascii_case("horizontal"));
				}
			}
			_ => break,
		}
	}
	while !parser.is(Kind::End) {
		parser.statement(&mut chart)?;
	}
	Ok(chart)
}

#[cfg(test)]
mod tests {
	use super::super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<XyChart> {
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
			kinds("xychart horizontal\nx-axis Q1 [a, 1]"),
			[
				K::Header,
				K::Orientation,
				K::NewLine,
				K::XAxis,
				K::Alpha,
				K::Number,
				K::BracketOpen,
				K::Alpha,
				K::Comma,
				K::Digits,
				K::BracketClose,
				K::EndOfFile,
				K::End
			]
		);
		assert_eq!(kinds("barn titles"), [K::Alpha, K::Alpha, K::EndOfFile, K::End]);
		assert_eq!(
			kinds("line \"a\" [-1.5 \"x\",.5]"),
			[
				K::Line,
				K::String,
				K::BracketOpen,
				K::Number,
				K::String,
				K::Comma,
				K::Number,
				K::BracketClose,
				K::EndOfFile,
				K::End
			]
		);
		// Whitespace before a line end swallows it, so the condition stays.
		assert_eq!(
			kinds("y-axis a \n 5 --> 6"),
			[K::YAxis, K::Alpha, K::Number, K::Arrow, K::Number, K::EndOfFile, K::End]
		);
		assert_eq!(kinds("title ü"), [K::Title, K::Invalid, K::End]);
	}

	#[test]
	fn reads_numbers_like_javascript() {
		assert_eq!(number_length("-1.5x"), Some(4));
		assert_eq!(number_length(".5"), Some(2));
		assert_eq!(number_length("5."), Some(1));
		assert_eq!(number_length("-x"), None);
		assert_eq!(js_number_of("+3"), 3.0);
		assert_eq!(js_number_of(""), 0.0);
		assert!(js_number_of("inf").is_nan());
		assert!(js_number_of("a").is_nan());
		assert_eq!(js_number_of("1e3"), 1000.0);
	}

	#[test]
	fn parses_axes_and_plots_like_the_database() {
		let chart = parsed(
			"xychart-beta horizontal\n  title Sales Revenue\n  x-axis \"Months\" [jan, \"feb x\", 3]\n  y-axis Revenue 4000 --> 11000\n  bar [5000, 6000, 7500, 8200]\n  line \"L\" [1 \"one\", 2]",
		)
		.unwrap();
		assert_eq!(chart.is_horizontal, Some(true));
		assert_eq!(chart.title, "SalesRevenue");
		assert_eq!(chart.x_axis_title, "Months");
		assert_eq!(
			chart.x_axis,
			XAxisData::Band { categories: vec!["jan".into(), "feb x".into(), "3".into()] }
		);
		assert_eq!(
			(chart.y_axis_title.as_str(), chart.y_min, chart.y_max),
			("Revenue", 4000.0, 11000.0)
		);
		assert_eq!(chart.plots[0].data.len(), 3);
		assert_eq!(chart.plots[1].data[2], ("3".to_owned(), None));
		assert_eq!(
			chart.plots[1].kind,
			PlotKind::Line { point_labels: Some(vec!["one".into(), String::new()]) }
		);
		assert_eq!(chart.plots[1].title, "L");
		assert_eq!(chart.plots[1].color, "#8493A6");

		let chart = parsed("xychart\n  line [1, 3, 2]\n  bar [4, 5]").unwrap();
		assert_eq!(chart.x_axis, XAxisData::Linear { min: 1.0, max: 3.0 });
		assert_eq!((chart.y_min, chart.y_max), (1.0, 5.0));
		let keys: Vec<_> = chart.plots[1].data.iter().map(|(key, _)| key.as_str()).collect();
		assert_eq!(keys, ["1", "3"]);
	}

	#[test]
	fn rejects_what_mermaid_rejects() {
		for code in [
			"xychart abc",
			"xychart-1",
			"xychart\n  x-axis Q1 [a, b]",
			"xychart\n  barn [1, 2]",
			"xychart\n  bar [1, 3e5]",
			"xychart\n  x-axis [a, \"\"]",
			"xychart\n  y-axis [a]",
			"xychart\n  title",
			"xychart\n  title \"Ümlaut\"\n  x-axis [ä]",
		] {
			assert!(parsed(code).is_err(), "{code}");
		}
		let error = parsed("xychart\n  bar [1, 2\n  line [3]").err().unwrap();
		assert_eq!(error.line, Some(2));
		assert!(parsed("xychart; bar [1,2]; title hi").is_ok());
	}
}
