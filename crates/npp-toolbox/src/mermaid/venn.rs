//! Venn diagrams (`venn.jison`, `vennDB.ts`, `vennRenderer.ts`, `styles.ts`) with the layout of
//! @upsetjs/venn.js (`VennDiagram` and `layout`).
//!
//! venn.js starts layouts of eight or more areas from random positions (`Math.random`); here the
//! random numbers come from mulberry32 seeded with 1, so a diagram always looks the same. Text
//! nodes, which Mermaid writes as HTML in a `foreignObject`, are SVG text laid out like it.

mod layout;

use std::collections::HashMap;

use self::layout::{
	Area, Circle, Point, Solution, area_path, normalize, scale, text_centres, venn,
};
use super::color::{darken, is_dark, js_number, lighten, transparentize};
use super::label::{Run, wrap_normal};
use super::rendering::sketch;
use super::rough;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::value::parse_leading_number;
use super::{Context, Diagram, Error, Result};

const SECTION: &str = "venn";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	NewLine,
	IndentText,
	Title,
	Venn,
	Set,
	Union,
	Text,
	Style,
	BracketLabel,
	Numeric,
	HexColor,
	RgbaColor,
	RgbColor,
	Identifier,
	String,
	Comma,
	Colon,
}

#[derive(Clone, Debug)]
struct Token {
	kind: Kind,
	text: String,
	/// The line after the token, where jison's lexer stands once it has read it.
	line: usize,
}

/// The state of the lexer that the grammar's actions change.
struct Lexer<'a> {
	code: &'a str,
	position: usize,
	line: usize,
	is_line_start: bool,
	consumes_indent_text: bool,
}

fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
	text.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// `[+-]?(\d+(\.\d+)?|\.\d+)`
fn numeric_length(text: &str) -> Option<usize> {
	let sign = usize::from(text.starts_with(['+', '-']));
	let rest = &text[sign..];
	let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
	let fraction = |after: &str| {
		after
			.strip_prefix('.')
			.map(|decimals| decimals.bytes().take_while(u8::is_ascii_digit).count())
			.filter(|&count| count > 0)
	};
	match digits {
		0 => fraction(rest).map(|count| sign + 1 + count),
		_ => Some(sign + digits + fraction(&rest[digits..]).map_or(0, |count| 1 + count)),
	}
}

/// `rgb(…)` with three numbers or `rgba(…)` with four.
fn color_function_length(text: &str, name: &str, count: usize) -> Option<usize> {
	if !starts_with_ignoring_case(text, name) || !text[name.len()..].starts_with('(') {
		return None;
	}
	let mut position = name.len() + 1;
	for index in 0..count {
		let rest = &text[position..];
		let blanks = rest.len() - rest.trim_start().len();
		let number =
			rest[blanks..].bytes().take_while(|b| b.is_ascii_digit() || *b == b'.').count();
		if number == 0 {
			return None;
		}
		position += blanks + number;
		let rest = &text[position..];
		let blanks = rest.len() - rest.trim_start().len();
		position += blanks;
		let separator = if index + 1 == count { ')' } else { ',' };
		if !text[position..].starts_with(separator) {
			return None;
		}
		position += 1;
	}
	Some(position)
}

impl Lexer<'_> {
	/// The next token, `Ok(None)` at the end, `Err` with the line of an unexpected character.
	fn next(&mut self, is_indent_mode: &mut bool) -> std::result::Result<Option<Token>, usize> {
		loop {
			let rest = &self.code[self.position..];
			if rest.is_empty() {
				return Ok(None);
			}
			let (kind, length): (Option<Kind>, usize) = self.token_at(rest, is_indent_mode)?;
			let text = rest[..length].to_owned();
			self.line += text.matches('\n').count();
			self.position += length;
			if let Some(kind) = kind {
				return Ok(Some(Token { kind, text, line: self.line }));
			}
		}
	}

	fn token_at(
		&mut self,
		rest: &str,
		is_indent_mode: &mut bool,
	) -> std::result::Result<(Option<Kind>, usize), usize> {
		let to_line_end = |text: &str| text.find('\n').unwrap_or(text.len());
		if rest.starts_with("%%") && !rest.starts_with("%%{") {
			return Ok((None, to_line_end(rest)));
		}
		let mut characters = rest.chars();
		if let Some(first) = characters.next()
			&& first != '}'
			&& characters.as_str().starts_with("%%")
		{
			return Ok((None, first.len_utf8() + to_line_end(characters.as_str())));
		}
		if self.is_line_start {
			let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
			let after = &rest[blanks..];
			if blanks > 0 && after.starts_with(['\n', '\r']) {
				return Ok((None, blanks));
			}
			if blanks > 0
				&& starts_with_ignoring_case(after, "text")
				&& !after[4..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
			{
				if *is_indent_mode {
					self.consumes_indent_text = true;
					self.is_line_start = false;
					return Ok((Some(Kind::IndentText), blanks));
				}
				return Ok((None, blanks));
			}
			if blanks > 0 {
				return Ok((None, blanks));
			}
			if !rest.starts_with(['\n', '\r']) {
				*is_indent_mode = false;
				self.is_line_start = false;
				return Ok((None, 0));
			}
		}
		let line_ends = rest.len() - rest.trim_start_matches(['\n', '\r']).len();
		if line_ends > 0 {
			self.is_line_start = true;
			return Ok((Some(Kind::NewLine), line_ends));
		}
		if rest.starts_with("%%") {
			return Ok((None, to_line_end(rest)));
		}
		let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
		if blanks > 0 {
			return Ok((None, blanks));
		}
		// `"title"\s[^#\n;]+`
		if starts_with_ignoring_case(rest, "title") && rest[5..].starts_with(char::is_whitespace) {
			let space = rest[5..].chars().next().map_or(1, char::len_utf8);
			let body = &rest[5 + space..];
			let length = body.find(['#', '\n', ';']).unwrap_or(body.len());
			if length > 0 {
				return Ok((Some(Kind::Title), 5 + space + length));
			}
		}
		for (keyword, kind) in [
			("venn-beta", Kind::Venn),
			("set", Kind::Set),
			("union", Kind::Union),
			("text", Kind::Text),
			("style", Kind::Style),
		] {
			// jison ends literal rules that end with a word character with a word boundary.
			let has_word_end = !rest[keyword.len().min(rest.len())..]
				.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
			if starts_with_ignoring_case(rest, keyword) && has_word_end {
				if kind == Kind::Text && self.consumes_indent_text {
					self.consumes_indent_text = false;
					return Ok((None, keyword.len()));
				}
				return Ok((Some(kind), keyword.len()));
			}
		}
		if let Some(inner) = rest.strip_prefix("[\"")
			&& let Some(end) = inner.find('"')
			&& inner[end + 1..].starts_with(']')
		{
			return Ok((Some(Kind::BracketLabel), 2 + end + 2));
		}
		if let Some(inner) = rest.strip_prefix('[') {
			let length = inner.bytes().take_while(|&b| b != b']' && b != b'"').count();
			if length > 0 && inner[length..].starts_with(']') {
				return Ok((Some(Kind::BracketLabel), length + 2));
			}
		}
		if let Some(length) = numeric_length(rest) {
			return Ok((Some(Kind::Numeric), length));
		}
		if let Some(hex) = rest.strip_prefix('#') {
			let digits = hex.bytes().take_while(u8::is_ascii_hexdigit).take(8).count();
			if digits >= 3 {
				return Ok((Some(Kind::HexColor), 1 + digits));
			}
		}
		if let Some(length) = color_function_length(rest, "rgba", 4) {
			return Ok((Some(Kind::RgbaColor), length));
		}
		if let Some(length) = color_function_length(rest, "rgb", 3) {
			return Ok((Some(Kind::RgbColor), length));
		}
		if rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
			let length = rest
				.bytes()
				.take_while(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
				.count();
			return Ok((Some(Kind::Identifier), length));
		}
		if let Some(inner) = rest.strip_prefix('"')
			&& let Some(end) = inner.find('"')
		{
			return Ok((Some(Kind::String), end + 2));
		}
		if rest.starts_with(',') {
			return Ok((Some(Kind::Comma), 1));
		}
		if rest.starts_with(':') {
			return Ok((Some(Kind::Colon), 1));
		}
		Err(self.line)
	}
}

/// `normalizeText`: trimmed, without surrounding double quotes.
fn normalize_text(text: &str) -> String {
	let trimmed = text.trim();
	match trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
		true => trimmed[1..trimmed.len() - 1].to_owned(),
		false => trimmed.to_owned(),
	}
}

/// JavaScript's default `sort`: by UTF-16 code units.
fn sorted(list: &[String]) -> Vec<String> {
	let mut sets: Vec<String> = list.iter().map(|set| normalize_text(set)).collect();
	npp::sort::sort_by(&mut sets[..], |a, b| a.encode_utf16().cmp(b.encode_utf16()));
	sets
}

#[derive(Clone, Debug)]
struct Subset {
	sets: Vec<String>,
	size: f64,
	label: Option<String>,
}

#[derive(Clone, Debug)]
struct TextNode {
	sets: Vec<String>,
	id: String,
	label: Option<String>,
}

#[derive(Default)]
struct Venn {
	title: Option<String>,
	subsets: Vec<Subset>,
	text_nodes: Vec<TextNode>,
	/// The styles of each area by its sets joined with `|`, merged.
	styles: HashMap<String, Vec<(String, String)>>,
}

struct Parser<'a, 'c, 'm> {
	lexer: Lexer<'a>,
	lookahead: Option<Token>,
	is_indent_mode: bool,
	current_sets: Option<Vec<String>>,
	known_sets: Vec<String>,
	context: &'c Context<'m>,
	last_line: usize,
}

impl Parser<'_, '_, '_> {
	fn peek(&mut self) -> Result<Option<&Token>> {
		if self.lookahead.is_none() {
			self.lookahead = self
				.lexer
				.next(&mut self.is_indent_mode)
				.map_err(|line| self.context.error_at(line, "unexpected character"))?;
		}
		Ok(self.lookahead.as_ref())
	}

	fn peek_kind(&mut self) -> Result<Option<Kind>> {
		Ok(self.peek()?.map(|token| token.kind))
	}

	fn take(&mut self) -> Result<Option<Token>> {
		self.peek()?;
		let token = self.lookahead.take();
		if let Some(token) = &token {
			self.last_line = token.line;
		}
		Ok(token)
	}

	fn error<T>(&mut self, expected: &str) -> Result<T> {
		let (line, found) = match self.peek()?.cloned() {
			Some(token) if token.kind == Kind::NewLine => {
				(token.line, "the end of the line".to_owned())
			}
			Some(token) => (token.line, format!("{:?}", token.text)),
			None => (self.last_line.max(1), "the end of the diagram".to_owned()),
		};
		Err(self.context.error_at(line, format!("expected {expected}, found {found}")))
	}

	fn expect(&mut self, kinds: &[Kind], expected: &str) -> Result<Token> {
		match self.peek_kind()? {
			Some(kind) if kinds.contains(&kind) => Ok(self.take()?.expect("peeked")),
			_ => self.error(expected),
		}
	}

	fn is(&mut self, kind: Kind) -> Result<bool> {
		Ok(self.peek_kind()? == Some(kind))
	}

	/// `identifierList`
	fn identifiers(&mut self) -> Result<Vec<String>> {
		let mut list = vec![self.expect(&[Kind::Identifier, Kind::String], "a set name")?.text];
		while self.is(Kind::Comma)? {
			self.take()?;
			list.push(self.expect(&[Kind::Identifier, Kind::String], "a set name")?.text);
		}
		Ok(list)
	}

	/// `BRACKET_LABEL`'s value.
	fn bracket_label(&mut self) -> Result<Option<String>> {
		if !self.is(Kind::BracketLabel)? {
			return Ok(None);
		}
		let text = self.take()?.map(|token| token.text).unwrap_or_default();
		Ok(Some(match text.starts_with("[\"") {
			true => text[2..text.len() - 2].to_owned(),
			false => text[1..text.len() - 1].trim().to_owned(),
		}))
	}

	/// `COLON NUMERIC`'s value.
	fn size(&mut self) -> Result<Option<f64>> {
		if !self.is(Kind::Colon)? {
			return Ok(None);
		}
		self.take()?;
		let number = self.expect(&[Kind::Numeric], "a number")?;
		Ok(Some(super::common::js_number_of(&number.text)))
	}

	fn add_subset(
		&mut self,
		venn: &mut Venn,
		list: &[String],
		label: Option<String>,
		size: Option<f64>,
	) {
		let sets = sorted(list);
		let size = size.unwrap_or(10.0 / (list.len() as f64).powi(2));
		if sets.len() == 1 && !self.known_sets.contains(&sets[0]) {
			self.known_sets.push(sets[0].clone());
		}
		self.current_sets = Some(sets.clone());
		venn.subsets.push(Subset { sets, size, label: label.map(|label| normalize_text(&label)) });
		self.is_indent_mode = true;
	}

	/// The id (`IDENTIFIER`, `STRING` or `NUMERIC`) and label of a text node.
	fn text_node(&mut self, venn: &mut Venn, sets: &[String]) -> Result<()> {
		let id = self.expect(&[Kind::Identifier, Kind::String, Kind::Numeric], "a text")?;
		let label = match id.kind {
			Kind::Numeric => None,
			_ => self.bracket_label()?,
		};
		venn.text_nodes.push(TextNode {
			sets: sorted(sets),
			id: normalize_text(&id.text),
			label: label.map(|label| normalize_text(&label)),
		});
		Ok(())
	}

	fn parse(&mut self) -> Result<Venn> {
		let mut venn = Venn::default();
		while self.is(Kind::NewLine)? {
			self.take()?;
		}
		self.expect(&[Kind::Venn], "`venn-beta`")?;
		while let Some(kind) = self.peek_kind()? {
			let token = self.take()?.expect("peeked");
			match kind {
				Kind::NewLine => {}
				Kind::Title => {
					venn.title = Some(token.text.get(6..).unwrap_or_default().to_owned())
				}
				Kind::Set => {
					let name = self.expect(&[Kind::Identifier, Kind::String], "a set name")?.text;
					let label = self.bracket_label()?;
					let size = self.size()?;
					self.add_subset(&mut venn, &[name], label, size);
				}
				Kind::Union => {
					let list = self.identifiers()?;
					let label = self.bracket_label()?;
					let size = self.size()?;
					if list.len() < 2 {
						return Err(self
							.context
							.error_at(self.last_line, "union requires multiple identifiers"));
					}
					let unknown: Vec<String> = list
						.iter()
						.map(|set| normalize_text(set))
						.filter(|set| !self.known_sets.contains(set))
						.collect();
					if !unknown.is_empty() {
						return Err(self.context.error_at(
							self.last_line,
							format!("unknown set identifier: {}", unknown.join(", ")),
						));
					}
					self.add_subset(&mut venn, &list, label, size);
				}
				Kind::Text => {
					let list = self.identifiers()?;
					self.text_node(&mut venn, &list)?;
				}
				Kind::IndentText => {
					let Some(sets) = self.current_sets.clone() else {
						return Err(self.context.error_at(token.line, "text requires set"));
					};
					self.text_node(&mut venn, &sets)?;
				}
				Kind::Style => {
					let targets = sorted(&self.identifiers()?);
					let mut styles = Vec::new();
					loop {
						let key = self.expect(&[Kind::Identifier], "a style property")?.text;
						self.expect(&[Kind::Colon], "`:`")?;
						let value = match self.is(Kind::String)? {
							true => self.take()?.map(|token| token.text).unwrap_or_default(),
							false => {
								let value_kinds = [
									Kind::Identifier,
									Kind::Numeric,
									Kind::HexColor,
									Kind::RgbColor,
									Kind::RgbaColor,
								];
								let mut values =
									vec![self.expect(&value_kinds, "a style value")?.text];
								while self
									.peek_kind()?
									.is_some_and(|kind| value_kinds.contains(&kind))
								{
									values.push(
										self.take()?.map(|token| token.text).unwrap_or_default(),
									);
								}
								values.join(" ")
							}
						};
						styles.push((key, normalize_text(&value)));
						if !self.is(Kind::Comma)? {
							break;
						}
						self.take()?;
					}
					let entry = venn.styles.entry(targets.join("|")).or_default();
					for (key, value) in styles {
						match entry.iter_mut().find(|(existing, _)| *existing == key) {
							Some(existing) => existing.1 = value,
							None => entry.push((key, value)),
						}
					}
				}
				_ => {
					self.lookahead = Some(token);
					return self.error("a statement");
				}
			}
		}
		Ok(venn)
	}
}

fn parse(code: &str, context: &Context) -> Result<Venn> {
	let lexer =
		Lexer { code, position: 0, line: 1, is_line_start: false, consumes_indent_text: false };
	let mut parser = Parser {
		lexer,
		lookahead: None,
		is_indent_mode: false,
		current_sets: None,
		known_sets: Vec::new(),
		context,
		last_line: 1,
	};
	parser.parse()
}

/// `ensurePairwiseSubsets`: the missing pairs of sets in unions of three or more.
fn add_pairwise_subsets(subsets: &[Subset]) -> Vec<Subset> {
	let mut existing: Vec<String> = subsets.iter().map(|subset| subset.sets.join("|")).collect();
	let mut sizes: HashMap<&str, f64> = HashMap::new();
	for subset in subsets.iter().filter(|subset| subset.sets.len() == 1) {
		sizes.insert(&subset.sets[0], subset.size);
	}
	let mut result = subsets.to_vec();
	for subset in subsets.iter().filter(|subset| subset.sets.len() >= 3) {
		let members = &subset.sets;
		for i in 0..members.len() {
			for j in i + 1..members.len() {
				let key = format!("{}|{}", members[i], members[j]);
				if existing.contains(&key) {
					continue;
				}
				existing.push(key);
				let size = match (sizes.get(members[i].as_str()), sizes.get(members[j].as_str())) {
					(Some(a), Some(b)) => a.min(*b) / 4.0,
					_ => 2.5,
				};
				result.push(Subset {
					sets: vec![members[i].clone(), members[j].clone()],
					size,
					label: Some(String::new()),
				});
			}
		}
	}
	result
}

/// mulberry32, Mermaid's `Math.random` here.
fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
	let mut state = seed;
	move || {
		state = state.wrapping_add(0x6D2B_79F5);
		let mut t = (state ^ (state >> 15)).wrapping_mul(1 | state);
		t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
		f64::from(t ^ (t >> 14)) / 4_294_967_296.0
	}
}

/// The circles and text centres of `areas` in a box of `width` × `height`.
fn lay_out(
	areas: &[Area],
	(width, height, padding): (f64, f64, f64),
	random: &mut dyn FnMut() -> f64,
) -> Result<(Solution, Vec<Option<Point>>)> {
	let solution = venn(areas, random).map_err(Error::new)?;
	let solution = scale(&normalize(&solution), width, height, padding);
	let centres = text_centres(&solution, areas);
	Ok((solution, centres))
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let venn_data = parse(code, context)?;
	let render_sets = add_pairwise_subsets(&venn_data.subsets);
	// Every pair of sets is an area of the layout, like an edge between them.
	let mut set_names: Vec<&str> = Vec::new();
	for subset in render_sets.iter().filter(|subset| subset.sets.len() == 1) {
		if !set_names.contains(&subset.sets[0].as_str()) {
			set_names.push(&subset.sets[0]);
		}
	}
	let pairs = set_names.len() * set_names.len().saturating_sub(1) / 2;
	if pairs > context.limits.max_edges {
		return Err(Error::new(format!(
			"Edge limit exceeded. {} sets need {pairs} pairs, but the limit is {}.",
			set_names.len(),
			context.limits.max_edges
		)));
	}
	let width = context.config_number(SECTION, "width", 800.0);
	let height = context.config_number(SECTION, "height", 450.0);
	let padding = context.config_number(SECTION, "padding", 8.0);
	let use_max_width = context.config_bool(SECTION, "useMaxWidth", true);
	let ratio = width / 1600.0;
	let title = venn_data.title.as_deref().filter(|title| !title.is_empty());
	let title_height = if title.is_some() { 48.0 * ratio } else { 0.0 };
	let chart_height = height - title_height;
	let theme = |key: &str| context.theme.text(key).filter(|value| !value.is_empty());
	let theme_colors: Vec<String> =
		(1..=8).filter_map(|index| theme(&format!("venn{index}"))).collect();
	let primary_color = theme("primaryColor").unwrap_or_default();
	let default_text_color =
		theme("primaryTextColor").or_else(|| theme("textColor")).unwrap_or_default();
	let set_text_color = theme("vennSetTextColor").unwrap_or_else(|| default_text_color.clone());
	let is_theme_dark =
		is_dark(&theme("background").unwrap_or_else(|| "#f4f4f4".to_owned())) == Some(true);
	let family = context.font_family();
	let mut random = mulberry32(1);
	let is_hand_drawn = context.look == super::Look::HandDrawn;

	// `VennDiagram`: areas of empty sets are left out.
	let empty: Vec<&str> = render_sets
		.iter()
		.filter(|subset| subset.size == 0.0 && subset.sets.len() == 1)
		.map(|subset| subset.sets[0].as_str())
		.collect();
	let chart_sets: Vec<&Subset> = render_sets
		.iter()
		.filter(|subset| !subset.sets.iter().any(|set| empty.contains(&set.as_str())))
		.collect();
	let to_area = |subset: &Subset| Area { sets: subset.sets.clone(), size: subset.size };
	let chart_areas: Vec<Area> = chart_sets.iter().map(|subset| to_area(subset)).collect();
	let (circles, centres) = match chart_areas.is_empty() {
		true => (Solution::default(), Vec::new()),
		false => lay_out(&chart_areas, (width, chart_height, 15.0), &mut random)?,
	};
	// `venn.layout` for the text nodes, on all areas.
	let all_areas: Vec<Area> = render_sets.iter().map(to_area).collect();
	let (text_circles, text_centres) =
		lay_out(&all_areas, (width, chart_height, padding), &mut random)?;

	let mut body = Svg::new();
	if let Some(title) = title {
		let font = Font::new(family.clone(), 32.0);
		let run = TextRun {
			x: width / 2.0,
			y: 32.0 * ratio,
			anchor: Anchor::Middle,
			baseline: Baseline::Middle,
			font: &font,
			content: title,
		};
		let fill = theme("vennTitleTextColor").or_else(|| theme("titleColor")).unwrap_or_default();
		body.text(run, |svg| _ = svg.attr("class", "venn-title").attr_if("fill", &fill));
	}
	body.open("g").attr("transform", &format!("translate(0, {})", number(title_height))).end();
	let label_font = Font::new(family.clone(), 48.0 * ratio);
	let mut circle_index = 0;
	for (subset, centre) in chart_sets.iter().zip(&centres) {
		let key = subset.sets.join("|");
		let custom = venn_data.styles.get(&key);
		let style = |name: &str| {
			custom
				.and_then(|styles| styles.iter().find(|(key, _)| key == name))
				.map(|(_, value)| value.clone())
		};
		let is_circle = subset.sets.len() == 1;
		let class = match is_circle {
			true => format!("venn-area venn-circle venn-set-{}", circle_index % 8),
			false => "venn-area venn-intersection".to_owned(),
		};
		body.open("g").attr("class", &class).attr("data-venn-sets", &subset.sets.join("_")).end();
		let members: Vec<Circle> =
			subset.sets.iter().filter_map(|set| circles.get(set).copied()).collect();
		let path = area_path(&members, None);
		let rough_options = |fill: &str| rough::Options {
			roughness: 0.7,
			seed: context.sketch_seed(&key),
			fill: Some(fill.to_owned()),
			fill_weight: 2.0,
			..rough::Options::default()
		};
		let text_color = if is_circle && is_hand_drawn {
			let palette = (!theme_colors.is_empty())
				.then(|| theme_colors[circle_index % theme_colors.len()].clone());
			let base = style("fill")
				.filter(|fill| !fill.is_empty())
				.or(palette)
				.unwrap_or_else(|| primary_color.clone());
			let stroke =
				style("stroke").filter(|stroke| !stroke.is_empty()).unwrap_or_else(|| base.clone());
			// `parseFloat` of the style, else of `5 * scale`.
			let stroke_width = style("stroke-width")
				.filter(|width| !width.is_empty())
				.map_or(Some(5.0 * ratio), |width| parse_leading_number(&width))
				.unwrap_or(f64::NAN);
			// The circle of the set in `venn.layout`.
			let circle = subset.sets.first().and_then(|set| text_circles.get(set).copied());
			if let Some(circle) = circle {
				let options = rough::Options {
					hachure_gap: 8.0,
					hachure_angle: -41.0 + circle_index as f64 * 60.0,
					stroke,
					stroke_width: js_number(stroke_width),
					..rough_options(&transparentize(&base, 0.7).unwrap_or_else(|| base.clone()))
				};
				let sketched = rough::circle(circle.x, circle.y, circle.radius * 2.0, &options);
				sketch::write_group(&mut body, &sketched, "", &sketch::PathPaint::default());
			}
			circle_index += 1;
			style("color").filter(|color| !color.is_empty()).unwrap_or_else(|| {
				let adjusted =
					if is_theme_dark { lighten(&base, 30.0) } else { darken(&base, 30.0) };
				adjusted.unwrap_or(base)
			})
		} else if let Some(fill) = style("fill").filter(|_| is_hand_drawn && !is_circle) {
			let options = rough::Options {
				fill_style: rough::FillStyle::CrossHatch,
				hachure_gap: 6.0,
				hachure_angle: 60.0,
				stroke: "none".to_owned(),
				..rough_options(&transparentize(&fill, 0.3).unwrap_or_else(|| fill.clone()))
			};
			sketch::write_group(&mut body, &rough::path(&path, &options), "", &Default::default());
			style("color").unwrap_or_else(|| set_text_color.clone())
		} else {
			body.open("path").attr("d", &path);
			let text_color = if is_circle {
				let palette = (!theme_colors.is_empty())
					.then(|| theme_colors[circle_index % theme_colors.len()].clone());
				let base = style("fill")
					.filter(|fill| !fill.is_empty())
					.or(palette)
					.unwrap_or_else(|| primary_color.clone());
				let stroke = style("stroke")
					.filter(|stroke| !stroke.is_empty())
					.unwrap_or_else(|| base.clone());
				let stroke_width = style("stroke-width")
					.filter(|width| !width.is_empty())
					.unwrap_or_else(|| js_number(5.0 * ratio));
				body.attr_if("fill", &base)
					.attr(
						"fill-opacity",
						&style("fill-opacity").unwrap_or_else(|| "0.1".to_owned()),
					)
					.attr_if("stroke", &stroke)
					.attr_if("stroke-width", &stroke_width)
					.attr("stroke-opacity", "0.95");
				circle_index += 1;
				style("color").filter(|color| !color.is_empty()).unwrap_or_else(|| {
					let adjusted =
						if is_theme_dark { lighten(&base, 30.0) } else { darken(&base, 30.0) };
					adjusted.unwrap_or(base)
				})
			} else {
				let fill = style("fill");
				body.attr("fill-opacity", if fill.is_some() { "1" } else { "0" })
					.attr("fill", fill.as_deref().unwrap_or("transparent"));
				style("color").unwrap_or_else(|| set_text_color.clone())
			};
			body.close();
			text_color
		};
		let label = match subset.label.as_deref().filter(|label| !label.is_empty()) {
			Some(label) => Some(label.to_owned()),
			None if is_circle => Some(subset.sets[0].clone()),
			None => None,
		};
		// `wrapText` cannot measure the detached chart and keeps one line.
		let label = label.unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
		if let Some(centre) = centre {
			let (x, y) = (centre.x.floor(), centre.y.floor());
			let run = TextRun {
				x,
				y: y + 0.35 * label_font.size,
				anchor: Anchor::Middle,
				baseline: Baseline::Alphabetic,
				font: &label_font,
				content: &label,
			};
			body.text(run, |svg| _ = svg.attr("class", "label").attr_if("fill", &text_color));
		}
		body.close_tag("g");
	}
	if !venn_data.text_nodes.is_empty() {
		write_text_nodes(
			&mut body,
			context,
			&venn_data,
			(&render_sets, &text_circles, &text_centres),
			ratio,
			&set_text_color,
		);
	}
	body.close_tag("g");
	let frame = Frame { x: 0.0, y: 0.0, width, height, use_max_width };
	Ok(document(context, "venn", &frame, &Default::default(), body))
}

/// `renderTextNodes`: the text nodes of each area in a grid inside it, each centered in its
/// cell and wrapped at its width like the `<span>` Mermaid writes.
fn write_text_nodes(
	body: &mut Svg,
	context: &mut Context,
	venn_data: &Venn,
	(areas, circles, centres): (&[Subset], &Solution, &[Option<Point>]),
	ratio: f64,
	set_text_color: &str,
) {
	let font = Font::new(context.font_family(), 40.0 * ratio);
	let line_height = context.normal_line_height(&font);
	let (ascent, descent) = context.vertical_metrics(&font);
	let baseline =
		((line_height - ascent.round() - descent.round()) / 2.0).floor() + ascent.round();
	body.open("g").attr("class", "venn-text-nodes").end();
	let mut groups: Vec<(String, Vec<&TextNode>)> = Vec::new();
	for node in &venn_data.text_nodes {
		let key = node.sets.join("|");
		match groups.iter_mut().find(|(other, _)| *other == key) {
			Some(group) => group.1.push(node),
			None => groups.push((key, vec![node])),
		}
	}
	for (key, nodes) in &groups {
		// `layoutByKey`: the last area with these sets.
		let Some(index) = areas.iter().rposition(|area| area.sets.join("|") == *key) else {
			continue;
		};
		let Some(centre) = centres.get(index).copied().flatten() else { continue };
		let area_circles: Vec<Circle> =
			areas[index].sets.iter().filter_map(|set| circles.get(set).copied()).collect();
		let min_radius =
			area_circles.iter().map(|circle| circle.radius).fold(f64::INFINITY, f64::min);
		let inner_raw = area_circles
			.iter()
			.map(|circle| circle.radius - (centre.x - circle.x).hypot(centre.y - circle.y))
			.fold(f64::INFINITY, f64::min);
		let mut inner_radius = if inner_raw.is_finite() { inner_raw.max(0.0) } else { 0.0 };
		if inner_radius == 0.0 && min_radius.is_finite() {
			inner_radius = min_radius * 0.6;
		}
		body.open("g").attr("class", "venn-text-area").end();
		let inner_width = (80.0 * ratio).max(inner_radius * 2.0 * 0.95);
		let inner_height = (60.0 * ratio).max(inner_radius * 2.0 * 0.95);
		let has_label = areas[index].label.as_deref().is_some_and(|label| !label.is_empty());
		let label_offset = if has_label { (32.0 * ratio).min(inner_radius * 0.25) } else { 0.0 }
			+ if nodes.len() <= 2 { 30.0 * ratio } else { 0.0 };
		let start_x = centre.x - inner_width / 2.0;
		let start_y = centre.y - inner_height / 2.0 + label_offset;
		let columns = ((nodes.len() as f64).sqrt().ceil() as usize).max(1);
		let rows = nodes.len().div_ceil(columns).max(1);
		let (cell_width, cell_height) = (inner_width / columns as f64, inner_height / rows as f64);
		for (position, node) in nodes.iter().enumerate() {
			let x = start_x + cell_width * ((position % columns) as f64 + 0.5);
			let y = start_y + cell_height * ((position / columns) as f64 + 0.5);
			let box_width = cell_width * 0.9;
			let text = node.label.clone().unwrap_or_else(|| node.id.clone());
			let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
			if text.is_empty() {
				continue;
			}
			let runs = [Run { text, style: Default::default() }];
			let (lines, _) = wrap_normal(context, &font, &runs, box_width.max(0.0));
			let color = venn_data
				.styles
				.get(&node.id)
				.and_then(|styles| styles.iter().find(|(key, _)| key == "color"))
				.map_or(set_text_color.to_owned(), |(_, color)| color.clone());
			let top = y - line_height * lines.len() as f64 / 2.0;
			for (row, line) in lines.iter().enumerate() {
				let content: String = line.runs.iter().map(|(run, _)| run.text.as_str()).collect();
				let run = TextRun {
					x,
					y: top + row as f64 * line_height + baseline,
					anchor: Anchor::Middle,
					baseline: Baseline::Alphabetic,
					font: &font,
					content: content.trim_end(),
				};
				body.text(run, |svg| {
					_ = svg.attr("class", "venn-text-node").attr_if("fill", &color)
				});
			}
		}
		body.close_tag("g");
	}
	body.close_tag("g");
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<Venn> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_like_jison() {
		let venn = parsed(
			"venn-beta\ntitle Fruits\nset B[\"Bananas\"]:5\nset A\n  text a1\n  text \"two words\"[\"Label\"]\nunion A,B:2\n  text ab\ntext A, B c [x]\nstyle A, B fill: #f00, color: rgb(1, 2, 3), stroke-width: 3 px\nSET C\n",
		)
		.unwrap();
		assert_eq!(venn.title.as_deref(), Some("Fruits"));
		let sets: Vec<(Vec<String>, f64)> =
			venn.subsets.iter().map(|subset| (subset.sets.clone(), subset.size)).collect();
		assert_eq!(sets[0], (vec!["B".to_owned()], 5.0));
		assert_eq!(sets[1], (vec!["A".to_owned()], 10.0));
		assert_eq!(sets[2], (vec!["A".to_owned(), "B".to_owned()], 2.0));
		assert_eq!(venn.subsets[0].label.as_deref(), Some("Bananas"));
		let nodes: Vec<(String, String, Option<String>)> = venn
			.text_nodes
			.iter()
			.map(|node| (node.sets.join("|"), node.id.clone(), node.label.clone()))
			.collect();
		assert_eq!(nodes[0], ("A".to_owned(), "a1".to_owned(), None));
		assert_eq!(nodes[1], ("A".to_owned(), "two words".to_owned(), Some("Label".to_owned())));
		assert_eq!(nodes[2], ("A|B".to_owned(), "ab".to_owned(), None));
		assert_eq!(nodes[3], ("A|B".to_owned(), "c".to_owned(), Some("x".to_owned())));
		assert_eq!(
			venn.styles.get("A|B").unwrap(),
			&[
				("fill".to_owned(), "#f00".to_owned()),
				("color".to_owned(), "rgb(1, 2, 3)".to_owned()),
				("stroke-width".to_owned(), "3 px".to_owned())
			]
		);
		let error = |code: &str| parsed(code).err().map(|error| (error.line, error.message));
		assert_eq!(
			error("venn-beta\nset A\nunion A,Z\n"),
			Some((Some(3), "unknown set identifier: Z".to_owned()))
		);
		assert_eq!(
			error("venn-beta\nset A\nunion A\n"),
			Some((Some(3), "union requires multiple identifiers".to_owned()))
		);
		// jison's implicit word end after keywords.
		assert_eq!(parsed("venn-beta\nset settings\n").unwrap().subsets[0].sets, ["settings"]);
	}

	#[test]
	fn adds_pairs_of_unions_like_mermaid() {
		let subset = |sets: &[&str], size: f64| Subset {
			sets: sets.iter().map(|set| (*set).to_owned()).collect(),
			size,
			label: None,
		};
		let added = add_pairwise_subsets(&[
			subset(&["A"], 8.0),
			subset(&["B"], 12.0),
			subset(&["C"], 4.0),
			subset(&["A", "B"], 1.0),
			subset(&["A", "B", "C"], 0.5),
		]);
		let pairs: Vec<(String, f64)> =
			added[5..].iter().map(|subset| (subset.sets.join("|"), subset.size)).collect();
		assert_eq!(pairs, [("A|C".to_owned(), 1.0), ("B|C".to_owned(), 1.0)]);
	}
}
