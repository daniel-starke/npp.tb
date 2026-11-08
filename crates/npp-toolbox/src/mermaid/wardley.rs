//! Wardley maps (`wardley.langium`, `wardleyParser.ts`, `wardleyBuilder.ts`, `wardleyDb.ts`,
//! `wardleyRenderer.ts`, `styles.ts`).
//!
//! The style sheet of `styles.ts` overrides several presentation attributes of the renderer
//! (all circles of a node, labels, links, annotations); the paint written here is the result.

use super::common::CommonFields;
use super::langium::Scanner;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Error, Result};

const DEFAULT_STAGES: [&str; 4] = ["Genesis", "Custom Built", "Product", "Commodity"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	/// `[`, `,`, `]`, `(`, `)`, `{`, `}`, `-`, `@`, `/`
	Punctuation,
	WardleyNumber,
	Arrow,
	LinkPort,
	LinkArrow,
	LinkLabel,
	Strategy,
	Keyword,
	Name,
	CommonField,
	Int,
	String,
	Id,
	NewLine,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
	kind: Kind,
	text: &'a str,
	line: usize,
}

const KEYWORD_TERMINALS: [&str; 15] = [
	"wardley-beta",
	"size",
	"evolution",
	"anchor",
	"component",
	"label",
	"inertia",
	"evolve",
	"pipeline",
	"note",
	"annotations",
	"annotation",
	"accelerator",
	"deaccelerator",
	"",
];

fn is_name_character(character: char) -> bool {
	character.is_ascii_alphanumeric() || "_()&".contains(character)
}

/// The length of the characters of a `NAME_WITH_SPACES` word after its first: name characters
/// and hyphens not followed by `>`.
fn name_word_tail(text: &str) -> usize {
	let mut length = 0;
	let bytes = text.as_bytes();
	while length < bytes.len() {
		let character = bytes[length] as char;
		if is_name_character(character)
			|| (character == '-' && bytes.get(length + 1) != Some(&b'>'))
		{
			length += 1;
		} else {
			break;
		}
	}
	length
}

/// `NAME_WITH_SPACES`: words separated by blanks, each starting with a letter (the first) or a
/// letter or `(` (the others), not starting with `title `, `accTitle` or `accDescr`.
fn name_length(text: &str) -> Option<usize> {
	let after_title = text.strip_prefix("title");
	if after_title.is_some_and(|after| after.starts_with(char::is_whitespace))
		|| text.starts_with("accTitle")
		|| text.starts_with("accDescr")
		|| !text.starts_with(|character: char| character.is_ascii_alphabetic())
	{
		return None;
	}
	let mut length = 1 + name_word_tail(&text[1..]);
	loop {
		let rest = &text[length..];
		let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
		let word = &rest[blanks..];
		if blanks == 0 || !word.starts_with(|c: char| c.is_ascii_alphabetic() || c == '(') {
			return Some(length);
		}
		length += blanks + 1 + name_word_tail(&word[1..]);
	}
}

/// `LINK_ARROW`'s `\+'[^']*'(<>|<|>)`.
fn quoted_flow_length(text: &str) -> Option<usize> {
	let after = text.strip_prefix("+'")?;
	let end = after.find('\'')?;
	let rest = &after[end + 1..];
	let flow = ["<>", "<", ">"].iter().find(|flow| rest.starts_with(**flow))?;
	Some(2 + end + 1 + flow.len())
}

/// The token at the start of `rest`, `None` for hidden text, `Err` where nothing matches.
fn token_at(rest: &str) -> std::result::Result<(Option<Kind>, usize), ()> {
	let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
	let field_start = &rest[blanks..];
	if ["accDescr", "accTitle", "title"].iter().any(|word| field_start.starts_with(word))
		&& name_length(field_start).is_none()
	{
		let mut scanner = Scanner::new(rest);
		if scanner.common_field(&mut CommonFields::default()) {
			return Ok((Some(Kind::CommonField), rest.len() - scanner.rest().len()));
		}
	}
	if blanks > 0 {
		return Ok((None, blanks));
	}
	if rest.starts_with("%%") {
		return Ok((None, rest.find(['\n', '\r']).unwrap_or(rest.len())));
	}
	// The keywords; `-` gives way to the longer `->`, `-->` and `-.->`.
	if let Some(first) = rest.chars().next().filter(|c| "[,](){}@/".contains(*c)) {
		return Ok((Some(Kind::Punctuation), first.len_utf8()));
	}
	if rest.starts_with('-') {
		for (arrow, kind) in
			[("-->", Kind::LinkArrow), ("-.->", Kind::LinkArrow), ("->", Kind::Arrow)]
		{
			if rest.starts_with(arrow) {
				return Ok((Some(kind), arrow.len()));
			}
		}
		return Ok((Some(Kind::Punctuation), 1));
	}
	let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
	if digits > 0 && rest[digits..].starts_with('.') {
		let decimals = rest[digits + 1..].bytes().take_while(u8::is_ascii_digit).count();
		if decimals > 0 {
			return Ok((Some(Kind::WardleyNumber), digits + 1 + decimals));
		}
	}
	for port in ["+<>", "+>", "+<"] {
		if rest.starts_with(port) {
			return Ok((Some(Kind::LinkPort), port.len()));
		}
	}
	if rest.starts_with('>') {
		return Ok((Some(Kind::LinkArrow), 1));
	}
	if let Some(length) = quoted_flow_length(rest) {
		return Ok((Some(Kind::LinkArrow), length));
	}
	if rest.starts_with(';') {
		let length = rest.find(['\n', '\r']).unwrap_or(rest.len());
		if length > 1 {
			return Ok((Some(Kind::LinkLabel), length));
		}
	}
	for strategy in ["build", "buy", "outsource", "market"] {
		if rest.starts_with(strategy) {
			return Ok((Some(Kind::Strategy), strategy.len()));
		}
	}
	for keyword in KEYWORD_TERMINALS.iter().filter(|keyword| !keyword.is_empty()) {
		if rest.starts_with(keyword) {
			return Ok((Some(Kind::Keyword), keyword.len()));
		}
	}
	if let Some(length) = name_length(rest) {
		return Ok((Some(Kind::Name), length));
	}
	// `INT`: `0|[1-9][0-9]*(?!\.)`
	if digits > 0 {
		let length = if rest.starts_with('0') { 1 } else { digits };
		if !rest[length..].starts_with('.') {
			return Ok((Some(Kind::Int), length));
		}
	}
	if let Some(quote) = rest.chars().next().filter(|&c| c == '"' || c == '\'') {
		let mut escaped = false;
		for (index, character) in rest.char_indices().skip(1) {
			match character {
				_ if escaped => escaped = false,
				'\\' => escaped = true,
				_ if character == quote => return Ok((Some(Kind::String), index + 1)),
				_ => {}
			}
		}
	}
	// `ID`: `[\w]([-\w]*\w)?`
	let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
	if rest.bytes().next().is_some_and(word) {
		let mut length = rest.bytes().take_while(|&byte| word(byte) || byte == b'-').count();
		while rest.as_bytes()[length - 1] == b'-' {
			length -= 1;
		}
		return Ok((Some(Kind::Id), length));
	}
	if rest.starts_with("\r\n") {
		return Ok((Some(Kind::NewLine), 2));
	}
	if rest.starts_with('\n') {
		return Ok((Some(Kind::NewLine), 1));
	}
	Err(())
}

fn lex(code: &str) -> std::result::Result<Vec<Token<'_>>, usize> {
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	let mut first_error = None;
	while position < code.len() {
		let rest = &code[position..];
		let length = match token_at(rest) {
			Ok((kind, length)) => {
				if let Some(kind) = kind {
					tokens.push(Token { kind, text: &rest[..length], line });
				}
				length
			}
			Err(()) => {
				first_error.get_or_insert(line);
				rest.chars().next().map_or(1, char::len_utf8)
			}
		};
		line += rest[..length].matches('\n').count();
		position += length;
	}
	match first_error {
		Some(line) => Err(line),
		None => Ok(tokens),
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeClass {
	Anchor,
	Component,
	PipelineComponent,
}

#[derive(Clone, Debug)]
struct Node {
	id: String,
	label: String,
	x: f64,
	y: f64,
	class: NodeClass,
	label_offset: (Option<f64>, Option<f64>),
	inertia: bool,
	strategy: Option<String>,
	in_pipeline: bool,
	is_pipeline_parent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
	Forward,
	Backward,
	Bidirectional,
}

struct Link {
	source: String,
	target: String,
	dashed: bool,
	label: Option<String>,
	flow: Option<Flow>,
}

#[derive(Default)]
struct Map {
	fields: CommonFields,
	nodes: Vec<Node>,
	links: Vec<Link>,
	/// Node id, target x and y, in the order of their first `evolve`.
	trends: Vec<(String, f64, f64)>,
	/// Parent and components.
	pipelines: Vec<(String, Vec<String>)>,
	/// Number, x, y, text.
	annotations: Vec<(f64, f64, f64, String)>,
	notes: Vec<(String, f64, f64)>,
	accelerators: Vec<(String, f64, f64)>,
	deaccelerators: Vec<(String, f64, f64)>,
	annotations_box: Option<(f64, f64)>,
	stages: Vec<String>,
	stage_boundaries: Vec<f64>,
	size: Option<(f64, f64)>,
}

impl Map {
	fn node(&self, id: &str) -> Option<&Node> {
		self.nodes.iter().find(|node| node.id == id)
	}

	/// `addNode`: a known id is updated in place, keeping offsets it had.
	fn add_node(&mut self, mut node: Node) {
		match self.nodes.iter_mut().find(|existing| existing.id == node.id) {
			Some(existing) => {
				node.label_offset.0 = node.label_offset.0.or(existing.label_offset.0);
				node.label_offset.1 = node.label_offset.1.or(existing.label_offset.1);
				node.in_pipeline |= existing.in_pipeline;
				node.is_pipeline_parent |= existing.is_pipeline_parent;
				*existing = node;
			}
			None => self.nodes.push(node),
		}
	}

	/// `resolveNodeId`: an id, else the first node with the label.
	fn resolve(&self, name: &str) -> String {
		if self.node(name).is_some() {
			return name.to_owned();
		}
		self.nodes
			.iter()
			.find(|node| node.label == name)
			.map_or_else(|| name.to_owned(), |node| node.id.clone())
	}
}

/// `toPercent`: fractions up to 1 as percent, values up to 100 as they are.
fn to_percent(value: f64, what: &str) -> Result<f64> {
	let normalized = if value <= 1.0 { value * 100.0 } else { value };
	if !(0.0..=100.0).contains(&normalized) {
		return Err(Error::new(format!(
			"{what} must be between 0-1 (decimal) or 0-100 (percentage). Received: {}",
			super::color::js_number(value)
		)));
	}
	Ok(normalized)
}

struct Parser<'a, 'c, 'm> {
	tokens: Vec<Token<'a>>,
	index: usize,
	context: &'c Context<'m>,
}

impl<'a> Parser<'a, '_, '_> {
	fn peek(&self, offset: usize) -> Option<Token<'a>> {
		self.tokens.get(self.index + offset).copied()
	}

	fn is(&self, kind: Kind, text: Option<&str>) -> bool {
		self.peek(0)
			.is_some_and(|token| token.kind == kind && text.is_none_or(|text| token.text == text))
	}

	fn error<T>(&self, expected: &str) -> Result<T> {
		let token = self.peek(0);
		let line = token.or(self.tokens.last().copied()).map_or(1, |token| token.line);
		let found = match token {
			Some(token) if token.kind == Kind::NewLine => "the end of the line".to_owned(),
			Some(token) => format!("{:?}", token.text.chars().take(20).collect::<String>()),
			None => "the end of the diagram".to_owned(),
		};
		Err(self.context.error_at(line, format!("expected {expected}, found {found}")))
	}

	fn take(&mut self, kind: Kind, text: Option<&str>, expected: &str) -> Result<&'a str> {
		match self.is(kind, text) {
			true => {
				self.index += 1;
				Ok(self.tokens[self.index - 1].text)
			}
			false => self.error(expected),
		}
	}

	fn punctuation(&mut self, text: &str) -> Result<()> {
		self.take(Kind::Punctuation, Some(text), &format!("`{text}`")).map(|_| ())
	}

	/// `EOL`: `NEWLINE+ | EOF`.
	fn end_of_line(&mut self) -> Result<()> {
		if self.index >= self.tokens.len() {
			return Ok(());
		}
		self.take(Kind::NewLine, None, "a new line")?;
		while self.is(Kind::NewLine, None) {
			self.index += 1;
		}
		Ok(())
	}

	/// `STRING | ID | NAME_WITH_SPACES`
	fn name(&mut self) -> Result<String> {
		match self.peek(0) {
			Some(token) if token.kind == Kind::String => {
				self.index += 1;
				Ok(Scanner::new(token.text).string().unwrap_or_default())
			}
			Some(token) if token.kind == Kind::Id || token.kind == Kind::Name => {
				self.index += 1;
				Ok(token.text.to_owned())
			}
			_ => self.error("a name"),
		}
	}

	fn wardley_number(&mut self) -> Result<f64> {
		let text = self.take(Kind::WardleyNumber, None, "a decimal number")?;
		Ok(text.parse().unwrap_or(0.0))
	}

	fn int(&mut self) -> Result<f64> {
		let text = self.take(Kind::Int, None, "a whole number")?;
		Ok(text.parse().unwrap_or(0.0))
	}

	/// `CoordinateValue`: `WARDLEY_NUMBER | INT`.
	fn coordinate(&mut self) -> Result<f64> {
		match self.is(Kind::WardleyNumber, None) {
			true => self.wardley_number(),
			false => self.int(),
		}
	}

	/// `'[' visibility ',' evolution ']'` of decimal numbers.
	fn position(&mut self) -> Result<(f64, f64)> {
		self.punctuation("[")?;
		let visibility = self.wardley_number()?;
		self.punctuation(",")?;
		let evolution = self.wardley_number()?;
		self.punctuation("]")?;
		Ok((visibility, evolution))
	}

	/// `Label`: `label [-?INT, -?INT]`.
	fn label(&mut self) -> Result<(Option<f64>, Option<f64>)> {
		if !self.is(Kind::Keyword, Some("label")) {
			return Ok((None, None));
		}
		self.index += 1;
		self.punctuation("[")?;
		let offset = |parser: &mut Self| -> Result<f64> {
			let negative = parser.is(Kind::Punctuation, Some("-"));
			if negative {
				parser.index += 1;
			}
			Ok(if negative { -parser.int()? } else { parser.int()? })
		};
		let x = offset(self)?;
		self.punctuation(",")?;
		let y = offset(self)?;
		self.punctuation("]")?;
		Ok((Some(x), Some(y)))
	}

	/// `EvolutionStage`: the name, maybe `@boundary` and `/ second name`.
	fn stage(&mut self) -> Result<(String, Option<f64>)> {
		let name = self.name()?;
		let mut boundary = None;
		if self.is(Kind::Punctuation, Some("@")) {
			self.index += 1;
			boundary = Some(self.wardley_number()?);
		}
		if self.is(Kind::Punctuation, Some("/")) {
			self.index += 1;
			let second = self.name()?;
			return Ok((format!("{} / {}", name.trim(), second.trim()), boundary));
		}
		Ok((name.trim().to_owned(), boundary))
	}
}

struct Statements {
	map: Map,
	anchors: Vec<(String, f64, f64)>,
	components: Vec<ParsedComponent>,
	pipelines: Vec<(String, Vec<PipelineComponent>)>,
	links: Vec<ParsedLink>,
	evolves: Vec<(String, f64)>,
	annotations_box: Option<(f64, f64)>,
	annotations: Vec<(f64, f64, f64, String)>,
	notes: Vec<(String, f64, f64)>,
	accelerators: Vec<(String, f64, f64)>,
	deaccelerators: Vec<(String, f64, f64)>,
}

/// The `label [x, y]` offset of a component.
type LabelOffset = (Option<f64>, Option<f64>);

/// A component of a pipeline: name, evolution and label offset.
type PipelineComponent = (String, f64, LabelOffset);

struct ParsedLink {
	from: String,
	from_port: Option<String>,
	arrow: Option<String>,
	to: String,
	to_port: Option<String>,
	label: Option<String>,
}

struct ParsedComponent {
	name: String,
	visibility: f64,
	evolution: f64,
	label_offset: (Option<f64>, Option<f64>),
	strategy: Option<String>,
	inertia: bool,
}

fn parse_statements(code: &str, context: &Context) -> Result<Statements> {
	let tokens = lex(code).map_err(|line| context.error_at(line, "unexpected character"))?;
	let mut parser = Parser { tokens, index: 0, context };
	let mut statements = Statements {
		map: Map { fields: CommonFields::new(context), ..Map::default() },
		anchors: Vec::new(),
		components: Vec::new(),
		pipelines: Vec::new(),
		links: Vec::new(),
		evolves: Vec::new(),
		annotations_box: None,
		annotations: Vec::new(),
		notes: Vec::new(),
		accelerators: Vec::new(),
		deaccelerators: Vec::new(),
	};
	while parser.is(Kind::NewLine, None) {
		parser.index += 1;
	}
	parser.take(Kind::Keyword, Some("wardley-beta"), "`wardley-beta`")?;
	while let Some(token) = parser.peek(0) {
		match (token.kind, token.text) {
			(Kind::NewLine, _) => parser.index += 1,
			(Kind::CommonField, _) => {
				parser.index += 1;
				Scanner::new(token.text).common_field(&mut statements.map.fields);
				parser.end_of_line()?;
			}
			(Kind::Keyword, "size") => {
				parser.index += 1;
				parser.punctuation("[")?;
				let width = parser.int()?;
				parser.punctuation(",")?;
				let height = parser.int()?;
				parser.punctuation("]")?;
				parser.end_of_line()?;
				statements.map.size = Some((width, height));
			}
			(Kind::Keyword, "evolution") => {
				parser.index += 1;
				let mut stages = vec![parser.stage()?];
				parser.take(Kind::Arrow, None, "`->`")?;
				stages.push(parser.stage()?);
				while parser.is(Kind::Arrow, None) {
					parser.index += 1;
					stages.push(parser.stage()?);
				}
				parser.end_of_line()?;
				statements.map.stage_boundaries =
					stages.iter().filter_map(|stage| stage.1).collect();
				statements.map.stages = stages.into_iter().map(|stage| stage.0).collect();
			}
			(Kind::Keyword, "anchor") => {
				parser.index += 1;
				let name = parser.name()?;
				let (visibility, evolution) = parser.position()?;
				parser.end_of_line()?;
				statements.anchors.push((name, visibility, evolution));
			}
			(Kind::Keyword, "component") => {
				parser.index += 1;
				let name = parser.name()?;
				let (visibility, evolution) = parser.position()?;
				let label_offset = parser.label()?;
				let mut strategy = None;
				let is_strategy = |parser: &Parser| {
					parser.is(Kind::Punctuation, Some("("))
						&& parser.peek(1).is_some_and(|token| token.kind == Kind::Strategy)
				};
				if is_strategy(&parser) {
					parser.index += 1;
					strategy = Some(parser.take(Kind::Strategy, None, "a strategy")?.to_owned());
					parser.punctuation(")")?;
				}
				let mut inertia = false;
				if parser.is(Kind::Keyword, Some("inertia")) {
					parser.index += 1;
					inertia = true;
				} else if parser.is(Kind::Punctuation, Some("(")) {
					parser.index += 1;
					parser.take(Kind::Keyword, Some("inertia"), "`inertia`")?;
					parser.punctuation(")")?;
					inertia = true;
				}
				parser.end_of_line()?;
				statements.components.push(ParsedComponent {
					name,
					visibility,
					evolution,
					label_offset,
					strategy,
					inertia,
				});
			}
			(Kind::Keyword, "evolve") => {
				parser.index += 1;
				let name = parser.name()?;
				let target = parser.wardley_number()?;
				parser.end_of_line()?;
				statements.evolves.push((name, target));
			}
			(Kind::Keyword, "pipeline") => {
				parser.index += 1;
				let parent = parser.name()?;
				parser.punctuation("{")?;
				parser.take(Kind::NewLine, None, "a new line")?;
				while parser.is(Kind::NewLine, None) {
					parser.index += 1;
				}
				let mut components = Vec::new();
				loop {
					parser.take(Kind::Keyword, Some("component"), "`component`")?;
					let name = parser.name()?;
					parser.punctuation("[")?;
					let evolution = parser.wardley_number()?;
					parser.punctuation("]")?;
					let label_offset = parser.label()?;
					parser.end_of_line()?;
					components.push((name, evolution, label_offset));
					if !parser.is(Kind::Keyword, Some("component")) {
						break;
					}
				}
				parser.punctuation("}")?;
				parser.end_of_line()?;
				statements.pipelines.push((parent, components));
			}
			(Kind::Keyword, "note") => {
				parser.index += 1;
				let text = parser.take(Kind::String, None, "a quoted text")?;
				let text = Scanner::new(text).string().unwrap_or_default();
				let (visibility, evolution) = parser.position()?;
				parser.end_of_line()?;
				statements.notes.push((text, visibility, evolution));
			}
			(Kind::Keyword, "annotations") => {
				parser.index += 1;
				parser.punctuation("[")?;
				let x = parser.coordinate()?;
				parser.punctuation(",")?;
				let y = parser.coordinate()?;
				parser.punctuation("]")?;
				parser.end_of_line()?;
				statements.annotations_box.get_or_insert((x, y));
			}
			(Kind::Keyword, "annotation") => {
				parser.index += 1;
				let number = parser.int()?;
				parser.punctuation(",")?;
				parser.punctuation("[")?;
				let x = parser.coordinate()?;
				parser.punctuation(",")?;
				let y = parser.coordinate()?;
				parser.punctuation("]")?;
				let text = parser.take(Kind::String, None, "a quoted text")?;
				let text = Scanner::new(text).string().unwrap_or_default();
				parser.end_of_line()?;
				statements.annotations.push((number, x, y, text));
			}
			(Kind::Keyword, keyword @ ("accelerator" | "deaccelerator")) => {
				parser.index += 1;
				let name = parser.name()?;
				let (x, y) = parser.position()?;
				parser.end_of_line()?;
				match keyword {
					"accelerator" => statements.accelerators.push((name, x, y)),
					_ => statements.deaccelerators.push((name, x, y)),
				}
			}
			(Kind::String | Kind::Id | Kind::Name, _) => {
				let from = parser.name()?;
				let take_optional = |parser: &mut Parser, kinds: &[Kind]| {
					let token = parser.peek(0).filter(|token| kinds.contains(&token.kind))?;
					parser.index += 1;
					Some(token.text.to_owned())
				};
				let from_port = take_optional(&mut parser, &[Kind::LinkPort]);
				let arrow = take_optional(&mut parser, &[Kind::LinkArrow, Kind::Arrow]);
				let to = parser.name()?;
				let to_port = take_optional(&mut parser, &[Kind::LinkPort]);
				let label = take_optional(&mut parser, &[Kind::LinkLabel])
					.map(|label| label[1..].trim().to_owned());
				parser.end_of_line()?;
				statements.links.push(ParsedLink { from, from_port, arrow, to, to_port, label });
			}
			_ => return parser.error("a statement"),
		}
	}
	Ok(statements)
}

fn flow_of_port(port: Option<&str>) -> Option<Flow> {
	match port? {
		"+<>" => Some(Flow::Bidirectional),
		"+<" => Some(Flow::Backward),
		"+>" => Some(Flow::Forward),
		_ => None,
	}
}

/// `populateDb`: the statements in Mermaid's order of kinds, with its checks.
fn parse(code: &str, context: &Context) -> Result<Map> {
	let statements = parse_statements(code, context)?;
	let mut map = statements.map;
	let node = |id: &str, label: &str, (x, y): (f64, f64), class: NodeClass| Node {
		id: id.to_owned(),
		label: label.to_owned(),
		x,
		y,
		class,
		label_offset: (None, None),
		inertia: false,
		strategy: None,
		in_pipeline: false,
		is_pipeline_parent: false,
	};
	let coordinates = |visibility: f64, evolution: f64, what: &str| -> Result<(f64, f64)> {
		Ok((
			to_percent(evolution, &format!("{what} evolution"))?,
			to_percent(visibility, &format!("{what} visibility"))?,
		))
	};
	for (name, visibility, evolution) in &statements.anchors {
		let position = coordinates(*visibility, *evolution, &format!("Anchor \"{name}\""))?;
		map.add_node(node(name, name, position, NodeClass::Anchor));
	}
	for component in &statements.components {
		let what = format!("Component \"{}\"", component.name);
		let position = coordinates(component.visibility, component.evolution, &what)?;
		let mut added = node(&component.name, &component.name, position, NodeClass::Component);
		added.label_offset = component.label_offset;
		added.inertia = component.inertia;
		added.strategy = component.strategy.clone();
		map.add_node(added);
	}
	for (text, visibility, evolution) in &statements.notes {
		let (x, y) = coordinates(*visibility, *evolution, &format!("Note \"{text}\""))?;
		map.notes.push((text.clone(), x, y));
	}
	for (parent, components) in &statements.pipelines {
		let Some(parent_y) = map.node(parent).map(|node| node.y) else {
			return Err(Error::new(format!(
				"Pipeline \"{parent}\" must reference an existing component with coordinates."
			)));
		};
		match map.pipelines.iter_mut().find(|(id, _)| id == parent) {
			Some(pipeline) => pipeline.1.clear(),
			None => map.pipelines.push((parent.clone(), Vec::new())),
		}
		if let Some(parent_node) = map.nodes.iter_mut().find(|node| node.id == *parent) {
			parent_node.is_pipeline_parent = true;
		}
		for (name, evolution, label_offset) in components {
			let id = format!("{parent}_{name}");
			let what = format!("Pipeline component \"{name}\" evolution");
			let x = to_percent(*evolution, &what)?;
			let mut added = node(&id, name, (x, parent_y), NodeClass::PipelineComponent);
			added.label_offset = *label_offset;
			map.add_node(added);
			if let Some(pipeline) = map.pipelines.iter_mut().find(|(id, _)| id == parent) {
				pipeline.1.push(id.clone());
			}
			if let Some(component) = map.nodes.iter_mut().find(|node| node.id == id) {
				component.in_pipeline = true;
			}
		}
	}
	for ParsedLink { from, from_port, arrow, to, to_port, label: annotation } in &statements.links {
		let arrow = arrow.as_deref().unwrap_or("");
		let dashed = arrow.contains("-.->") || arrow.contains(".-.");
		let mut flow = flow_of_port(from_port.as_deref()).or(flow_of_port(to_port.as_deref()));
		let mut flow_label = None;
		if let Some(after) = arrow.strip_prefix('+') {
			flow_label = after
				.strip_prefix('\'')
				.and_then(|quoted| quoted.find('\'').map(|end| quoted[..end].to_owned()));
			let arrow_flow = if arrow.contains("<>") {
				Some(Flow::Bidirectional)
			} else if arrow.contains('<') {
				Some(Flow::Backward)
			} else if arrow.contains('>') {
				Some(Flow::Forward)
			} else {
				None
			};
			flow = flow.or(arrow_flow);
		}
		let link = Link {
			source: map.resolve(from),
			target: map.resolve(to),
			dashed,
			label: flow_label.or_else(|| annotation.clone()),
			flow,
		};
		map.links.push(link);
	}
	for (name, target) in &statements.evolves {
		if let Some(y) = map.node(name).map(|node| node.y) {
			let x = to_percent(*target, &format!("Evolve target for \"{name}\""))?;
			match map.trends.iter_mut().find(|(id, _, _)| id == name) {
				Some(trend) => *trend = (name.clone(), x, y),
				None => map.trends.push((name.clone(), x, y)),
			}
		}
	}
	if let Some((x, y)) = statements.annotations_box {
		map.annotations_box = Some(coordinates(x, y, "Annotations box")?);
	}
	for (number, x, y, text) in &statements.annotations {
		let what = format!("Annotation {}", super::color::js_number(*number));
		let (x, y) = coordinates(*x, *y, &what)?;
		map.annotations.push((*number, x, y, text.clone()));
	}
	for (name, x, y) in &statements.accelerators {
		let (x, y) = coordinates(*x, *y, &format!("Accelerator \"{name}\""))?;
		map.accelerators.push((name.clone(), x, y));
	}
	for (name, x, y) in &statements.deaccelerators {
		let (x, y) = coordinates(*x, *y, &format!("Deaccelerator \"{name}\""))?;
		map.deaccelerators.push((name.clone(), x, y));
	}
	Ok(map)
}

/// The colors of `getTheme`: the theme's `wardley` block, else the fallbacks.
struct Colors {
	block: Option<Value>,
}

impl Colors {
	fn get(&self, key: &str, fallback: Option<String>, last: &str) -> String {
		self.block
			.as_ref()
			.and_then(|block| block.get(key))
			.filter(|value| !matches!(value, Value::Null))
			.and_then(Value::to_text)
			.or(fallback)
			.unwrap_or_else(|| last.to_owned())
	}
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let map = parse(code, context)?;
	// Mermaid reads `getConfig()['wardley-beta']`, which documents cannot set: neither
	// directives nor front matter reach it, so the defaults always apply.
	let (width, height) = map.size.unwrap_or((900.0, 600.0));
	let padding = 48.0;
	let node_radius = 6.0;
	let node_label_offset = 8.0;
	let axis_font_size = 12.0;
	let label_font_size = 10.0;
	let show_grid = false;
	let use_max_width = true;
	let colors = Colors { block: context.theme.get("wardley").cloned() };
	let background = context.theme.text("background");
	let primary_text = context.theme.text("primaryTextColor");
	let axis_text_color = colors.get("axisTextColor", primary_text.clone(), "#222");
	let axis_color = colors.get("axisColor", None, "#000");
	let component_stroke = colors.get("componentStroke", None, "#000");
	let component_fill = colors.get("componentFill", None, "#fff");
	let component_label_color = colors.get("componentLabelColor", primary_text.clone(), "#222");
	let link_stroke = colors.get("linkStroke", None, "#000");
	let evolution_stroke = colors.get("evolutionStroke", None, "#dc3545");
	// Mermaid removes the diagram's style sheet with all children of the `<svg>`, so its texts
	// are in the page's default font, Times New Roman in Edge.
	let family = "Times New Roman".to_owned();
	let font = |size: f64, is_bold: bool| {
		let mut font = Font::new(family.clone(), size);
		if is_bold {
			font.weight = 700;
		}
		font
	};
	let square_size = node_radius * 1.6;
	let chart_width = width - padding * 2.0;
	let chart_height = height - padding * 2.0;
	let project_x = |value: f64| padding + value / 100.0 * chart_width;
	let project_y = |value: f64| height - padding - value / 100.0 * chart_height;
	let id = &context.id.clone();

	let mut body = Svg::new();
	body.open("g").attr("class", "wardley-map").end();
	body.open("rect")
		.attr("class", "wardley-background")
		.number_attr("width", width)
		.number_attr("height", height)
		.attr_if("fill", &colors.get("backgroundColor", background.clone(), "#fff"))
		.close();
	if let Some(title) = map.fields.title.as_deref().filter(|title| !title.is_empty()) {
		let title_font = font(axis_font_size * 1.05, true);
		let run = TextRun {
			x: width / 2.0,
			y: padding / 2.0,
			anchor: Anchor::Middle,
			baseline: Baseline::Middle,
			font: &title_font,
			content: title,
		};
		body.text(run, |svg| {
			_ = svg.attr("class", "wardley-title").attr_if("fill", &axis_text_color)
		});
	}
	let line = |body: &mut Svg, [x1, y1, x2, y2]: [f64; 4]| {
		body.open("line")
			.number_attr("x1", x1)
			.number_attr("y1", y1)
			.number_attr("x2", x2)
			.number_attr("y2", y2);
	};
	body.open("g").attr("class", "wardley-axes").end();
	for coordinates in [
		[padding, height - padding, width - padding, height - padding],
		[padding, padding, padding, height - padding],
	] {
		line(&mut body, coordinates);
		body.attr_if("stroke", &axis_color).attr("stroke-width", "1").close();
	}
	let axis_label_color = axis_text_color.clone();
	let axis_font = font(axis_font_size, true);
	let x_label = TextRun {
		x: padding + chart_width / 2.0,
		y: height - padding / 4.0,
		anchor: Anchor::Middle,
		baseline: Baseline::Alphabetic,
		font: &axis_font,
		content: "Evolution",
	};
	body.text(x_label, |svg| {
		svg.attr("class", "wardley-axis-label wardley-axis-label-x")
			.attr_if("fill", &axis_label_color);
	});
	let (y_label_x, y_label_y) = (padding / 3.0, padding + chart_height / 2.0);
	let y_label = TextRun {
		x: y_label_x,
		y: y_label_y,
		anchor: Anchor::Middle,
		baseline: Baseline::Alphabetic,
		font: &axis_font,
		content: "Visibility",
	};
	let rotate = format!("rotate(-90 {} {})", number(y_label_x), number(y_label_y));
	body.text(y_label, |svg| {
		svg.attr("class", "wardley-axis-label wardley-axis-label-y")
			.attr_if("fill", &axis_label_color)
			.attr("transform", &rotate);
	});
	body.close_tag("g");

	let stages: Vec<String> = match map.stages.is_empty() {
		true => DEFAULT_STAGES.iter().map(|stage| (*stage).to_owned()).collect(),
		false => map.stages.clone(),
	};
	body.open("g").attr("class", "wardley-stages").end();
	let stage_font = font(axis_font_size - 2.0, false);
	let count = stages.len() as f64;
	let mut previous_boundary = 0.0;
	for (index, stage) in stages.iter().enumerate() {
		let (start, end) = match map.stage_boundaries.len() == stages.len() {
			true => {
				let boundary = map.stage_boundaries[index];
				let range = (previous_boundary, boundary);
				previous_boundary = boundary;
				range
			}
			false => (index as f64 / count, (index + 1) as f64 / count),
		};
		let start_x = padding + start * chart_width;
		let end_x = padding + end * chart_width;
		if index > 0 {
			line(&mut body, [start_x, padding, start_x, height - padding]);
			body.attr("stroke", "#000")
				.attr("stroke-width", "1")
				.attr("stroke-dasharray", "5 5")
				.attr("opacity", "0.8")
				.close();
		}
		let run = TextRun {
			x: (start_x + end_x) / 2.0,
			y: height - padding / 1.5,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &stage_font,
			content: stage,
		};
		body.text(run, |svg| {
			svg.attr("class", "wardley-stage-label").attr_if("fill", &axis_label_color);
		});
	}
	body.close_tag("g");
	if show_grid {
		body.open("g").attr("class", "wardley-grid").end();
		let grid_color = colors.get("gridColor", None, "rgba(100, 100, 100, 0.2)");
		for step in 1..4 {
			let ratio = f64::from(step) / 4.0;
			let x = padding + chart_width * ratio;
			let y = height - padding - chart_height * ratio;
			for coordinates in [[x, padding, x, height - padding], [padding, y, width - padding, y]]
			{
				line(&mut body, coordinates);
				body.attr_if("stroke", &grid_color).attr("stroke-dasharray", "2 6").close();
			}
		}
		body.close_tag("g");
	}

	let mut positions: Vec<(f64, f64)> =
		map.nodes.iter().map(|node| (project_x(node.x), project_y(node.y))).collect();
	let position_of = |map: &Map, id: &str| map.nodes.iter().position(|node| node.id == id);
	if !map.pipelines.is_empty() {
		let mut boxes = Svg::new();
		let mut links = Svg::new();
		boxes.open("g").attr("class", "wardley-pipelines").end();
		links.open("g").attr("class", "wardley-pipeline-links").end();
		for (parent, components) in &map.pipelines {
			if components.is_empty() {
				continue;
			}
			let mut sorted: Vec<usize> =
				components.iter().filter_map(|component| position_of(&map, component)).collect();
			// A stable sort by evolution, like `Array.prototype.sort`.
			npp::sort::sort_by(&mut sorted[..], |&a, &b| {
				map.nodes[a].x.partial_cmp(&map.nodes[b].x).unwrap_or(std::cmp::Ordering::Equal)
			});
			for pair in sorted.windows(2) {
				let (current, next) = (positions[pair[0]], positions[pair[1]]);
				line(&mut links, [current.0, current.1, next.0, next.1]);
				links
					.attr("class", "wardley-pipeline-evolution-link")
					.attr_if("stroke", &link_stroke)
					.attr("stroke-width", "1")
					.attr("stroke-dasharray", "4 4")
					.close();
			}
			let mut extent: Option<(f64, f64, f64)> = None;
			for component in components {
				if let Some(index) = position_of(&map, component) {
					let (x, y) = positions[index];
					extent = Some(match extent {
						Some((min, max, _)) => (min.min(x), max.max(x), y),
						None => (x, x, y),
					});
				}
			}
			if let Some((min_x, max_x, y)) = extent {
				let box_height = node_radius * 4.0;
				let box_top = y - box_height / 2.0;
				if let Some(parent) = position_of(&map, parent) {
					positions[parent] = ((min_x + max_x) / 2.0, box_top - square_size / 6.0);
				}
				boxes
					.open("rect")
					.attr("class", "wardley-pipeline-box")
					.number_attr("x", min_x - 15.0)
					.number_attr("y", box_top)
					.number_attr("width", max_x - min_x + 30.0)
					.number_attr("height", box_height)
					.attr("fill", "none")
					.attr_if("stroke", &axis_color)
					.attr("stroke-width", "1.5")
					.attr("rx", "4")
					.attr("ry", "4")
					.close();
			}
		}
		boxes.close_tag("g");
		links.close_tag("g");
		body.append(boxes);
		body.append(links);
	}

	body.open("g").attr("class", "wardley-links").end();
	let radius_of =
		|node: &Node| if node.is_pipeline_parent { square_size / 2f64.sqrt() } else { node_radius };
	let valid_links: Vec<&Link> = map
		.links
		.iter()
		.filter(|link| {
			position_of(&map, &link.source).is_some()
				&& position_of(&map, &link.target).is_some()
				&& !map.pipelines.iter().any(|(parent, components)| {
					*parent == link.target && components.contains(&link.source)
				})
		})
		.collect();
	for link in &valid_links {
		let (source, target) = (position_of(&map, &link.source), position_of(&map, &link.target));
		let (Some(source), Some(target)) = (source, target) else { continue };
		let (from, to) = (positions[source], positions[target]);
		let (dx, dy) = (to.0 - from.0, to.1 - from.1);
		let distance = (dx * dx + dy * dy).sqrt();
		let (source_radius, target_radius) =
			(radius_of(&map.nodes[source]), radius_of(&map.nodes[target]));
		line(
			&mut body,
			[
				from.0 + dx / distance * source_radius,
				from.1 + dy / distance * source_radius,
				to.0 - dx / distance * target_radius,
				to.1 - dy / distance * target_radius,
			],
		);
		let class = if link.dashed { "wardley-link wardley-link--dashed" } else { "wardley-link" };
		body.attr("class", class).attr_if("stroke", &link_stroke).attr("stroke-width", "1");
		if link.dashed {
			body.attr("stroke-dasharray", "6 6");
		}
		if matches!(link.flow, Some(Flow::Forward | Flow::Bidirectional)) {
			body.attr("marker-end", &format!("url(#{id}-link-arrow-end)"));
		}
		if matches!(link.flow, Some(Flow::Backward | Flow::Bidirectional)) {
			body.attr("marker-start", &format!("url(#{id}-link-arrow-start)"));
		}
		body.close();
	}
	let link_label_font = font(label_font_size, false);
	for link in valid_links
		.iter()
		.filter(|link| link.label.as_deref().is_some_and(|label| !label.is_empty()))
	{
		let (source, target) = (position_of(&map, &link.source), position_of(&map, &link.target));
		let (Some(source), Some(target)) = (source, target) else { continue };
		let (from, to) = (positions[source], positions[target]);
		let (dx, dy) = (to.0 - from.0, to.1 - from.1);
		let distance = (dx * dx + dy * dy).sqrt();
		let label_x = (from.0 + to.0) / 2.0 + dy / distance * 8.0;
		let label_y = (from.1 + to.1) / 2.0 - dx / distance * 8.0;
		let mut angle = dy.atan2(dx).to_degrees();
		if !(-90.0..=90.0).contains(&angle) {
			angle += 180.0;
		}
		let run = TextRun {
			x: label_x,
			y: label_y,
			anchor: Anchor::Middle,
			baseline: Baseline::Middle,
			font: &link_label_font,
			content: link.label.as_deref().unwrap_or_default(),
		};
		let transform =
			format!("rotate({} {} {})", number(angle), number(label_x), number(label_y));
		body.text(run, |svg| {
			svg.attr("class", "wardley-link-label")
				.attr_if("fill", &axis_label_color)
				.attr("transform", &transform);
		});
	}
	body.close_tag("g");

	body.open("g").attr("class", "wardley-trends").end();
	for (node, target_x, target_y) in &map.trends {
		let Some(origin) = position_of(&map, node).map(|index| positions[index]) else { continue };
		let (target_x, target_y) = (project_x(*target_x), project_y(*target_y));
		let (dx, dy) = (target_x - origin.0, target_y - origin.1);
		let distance = (dx * dx + dy * dy).sqrt();
		let shorten_by = node_radius + 2.0;
		let (x2, y2) = match distance > shorten_by {
			true => (target_x - dx / distance * shorten_by, target_y - dy / distance * shorten_by),
			false => (target_x, target_y),
		};
		line(&mut body, [origin.0, origin.1, x2, y2]);
		body.attr("class", "wardley-trend")
			.attr_if("stroke", &evolution_stroke)
			.attr("stroke-width", "1")
			.attr("stroke-dasharray", "4 4")
			.attr("marker-end", &format!("url(#{id}-arrow)"))
			.close();
	}
	body.close_tag("g");

	body.open("g").attr("class", "wardley-nodes").end();
	let circle = |body: &mut Svg,
	              class: Option<&str>,
	              (cx, cy): (f64, f64),
	              r: f64,
	              (fill, stroke, stroke_width): (&str, &str, &str)| {
		body.open("circle");
		if let Some(class) = class {
			body.attr("class", class);
		}
		body.number_attr("cx", cx)
			.number_attr("cy", cy)
			.number_attr("r", r)
			.attr_if("fill", fill)
			.attr_if("stroke", stroke)
			.attr("stroke-width", stroke_width)
			.close();
	};
	let node_label_font = font(label_font_size, false);
	let anchor_label_font = font(label_font_size, true);
	for (index, node) in map.nodes.iter().enumerate() {
		let (x, y) = positions[index];
		let class_name = match node.class {
			NodeClass::Anchor => "anchor",
			NodeClass::Component => "component",
			NodeClass::PipelineComponent => "pipeline-component",
		};
		body.open("g").attr("class", &format!("wardley-node wardley-node--{class_name}")).end();
		let strategy = node.strategy.as_deref();
		if let Some(strategy) = strategy {
			let (fill, stroke) = match strategy {
				"outsource" => ("#666", component_stroke.as_str()),
				"buy" => ("#ccc", component_stroke.as_str()),
				"build" => ("#eee", "#000"),
				_ => ("white", component_stroke.as_str()),
			};
			let class = format!("wardley-{strategy}-overlay");
			circle(&mut body, Some(&class), (x, y), node_radius * 2.0, (fill, stroke, "1"));
		}
		if !node.is_pipeline_parent && strategy != Some("market") && node.class != NodeClass::Anchor
		{
			let paint = (component_fill.as_str(), component_stroke.as_str(), "1");
			circle(&mut body, None, (x, y), node_radius, paint);
		}
		if strategy == Some("market") {
			let triangle = node_radius * 1.2;
			let (cos, sin) =
				((std::f64::consts::PI / 6.0).cos(), (std::f64::consts::PI / 6.0).sin());
			let corners = [
				(x, y - triangle),
				(x - triangle * cos, y + triangle * sin),
				(x + triangle * cos, y + triangle * sin),
			];
			for side in 0..3 {
				let (from, to) = (corners[side], corners[(side + 1) % 3]);
				line(&mut body, [from.0, from.1, to.0, to.1]);
				body.attr("class", "wardley-market-line")
					.attr_if("stroke", &component_stroke)
					.attr("stroke-width", "1")
					.close();
			}
			for corner in corners {
				circle(
					&mut body,
					Some("wardley-market-dot"),
					corner,
					node_radius * 0.7,
					("white", &component_stroke, "2"),
				);
			}
		}
		if node.is_pipeline_parent {
			body.open("rect")
				.number_attr("x", x - square_size / 2.0)
				.number_attr("y", y - square_size / 2.0)
				.number_attr("width", square_size)
				.number_attr("height", square_size)
				.attr_if("fill", &component_fill)
				.attr_if("stroke", &component_stroke)
				.attr("stroke-width", "1")
				.close();
		}
		if node.inertia {
			let mut offset =
				if node.is_pipeline_parent { square_size / 2.0 + 15.0 } else { node_radius + 15.0 };
			if strategy.is_some() {
				offset += node_radius + 10.0;
			}
			let line_height = if node.is_pipeline_parent { square_size } else { node_radius * 2.0 };
			line(&mut body, [x + offset, y - line_height / 2.0, x + offset, y + line_height / 2.0]);
			body.attr("class", "wardley-inertia")
				.attr_if("stroke", &component_stroke)
				.attr("stroke-width", "6")
				.close();
		}
		let (offset_x, offset_y) = node.label_offset;
		let is_anchor = node.class == NodeClass::Anchor;
		let extra = if strategy.is_some() { 10.0 } else { 0.0 };
		let (label_x, label_y) = match is_anchor {
			true => (x + offset_x.unwrap_or(0.0), offset_y.map_or(y - 3.0, |offset| y + offset)),
			false => (
				x + offset_x.unwrap_or(node_label_offset + extra),
				y + offset_y.unwrap_or(-node_label_offset - extra),
			),
		};
		let run = TextRun {
			x: label_x,
			y: label_y,
			anchor: if is_anchor { Anchor::Middle } else { Anchor::Start },
			baseline: if is_anchor { Baseline::Middle } else { Baseline::Alphabetic },
			font: if is_anchor { &anchor_label_font } else { &node_label_font },
			content: &node.label,
		};
		let label_color = if is_anchor { "#000" } else { component_label_color.as_str() };
		body.text(run, |svg| {
			_ = svg.attr("class", "wardley-node-label").attr_if("fill", label_color)
		});
		body.close_tag("g");
	}
	body.close_tag("g");

	let small_bold = |size: f64| font(size, true);
	if !map.annotations.is_empty() {
		body.open("g").attr("class", "wardley-annotations").end();
		let (annotation_fill, annotation_stroke) = ("white", axis_color.as_str());
		let annotation_text = axis_text_color.clone();
		let number_font = small_bold(10.0);
		for (number_value, x, y) in map.annotations.iter().map(|(number, x, y, _)| (number, x, y)) {
			let (cx, cy) = (project_x(*x), project_y(*y));
			body.open("g").attr("class", "wardley-annotation").end();
			body.open("circle")
				.number_attr("cx", cx)
				.number_attr("cy", cy)
				.attr("r", "10")
				.attr_if("fill", annotation_fill)
				.attr_if("stroke", annotation_stroke)
				.attr("stroke-width", "1.5")
				.close();
			let label = super::color::js_number(*number_value);
			let run = TextRun {
				x: cx,
				y: cy,
				anchor: Anchor::Middle,
				baseline: Baseline::Central,
				font: &number_font,
				content: &label,
			};
			body.text(run, |svg| _ = svg.attr_if("fill", &annotation_text));
			body.close_tag("g");
		}
		if let Some((box_x, box_y)) = map.annotations_box {
			let (mut box_x, mut box_y) = (project_x(box_x), project_y(box_y));
			let mut sorted: Vec<&(f64, f64, f64, String)> =
				map.annotations.iter().filter(|annotation| !annotation.3.is_empty()).collect();
			npp::sort::sort_by(&mut sorted[..], |a, b| {
				a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
			});
			body.open("g").attr("class", "wardley-annotations-box").end();
			if !sorted.is_empty() {
				let box_font = font(11.0, false);
				let texts: Vec<String> = sorted
					.iter()
					.map(|(number, _, _, text)| {
						format!("{}. {text}", super::color::js_number(*number))
					})
					.collect();
				let (mut max_width, mut max_height) = (0f64, 0f64);
				for text in &texts {
					max_width = max_width.max(context.measure(text, &box_font).0);
					max_height = max_height.max(context.bounding_box(text, &box_font).1);
				}
				let box_width = max_width + 20.0 + 105.0;
				let box_height = sorted.len() as f64 * 16.0 + 20.0 + max_height / 2.0;
				box_x = padding.max(box_x.min(width - padding - box_width));
				box_y = padding.max(box_y.min(height - padding - box_height));
				body.open("rect")
					.number_attr("x", box_x)
					.number_attr("y", box_y)
					.number_attr("width", box_width)
					.number_attr("height", box_height)
					.attr_if("fill", annotation_fill)
					.attr_if("stroke", annotation_stroke)
					.attr("stroke-width", "1.5")
					.attr("rx", "4")
					.attr("ry", "4")
					.close();
				for (index, text) in texts.iter().enumerate() {
					let run = TextRun {
						x: box_x + 10.0,
						y: box_y + 10.0 + (index + 1) as f64 * 16.0,
						anchor: Anchor::Start,
						baseline: Baseline::Middle,
						font: &box_font,
						content: text,
					};
					body.text(run, |svg| _ = svg.attr_if("fill", &annotation_text));
				}
			}
			body.close_tag("g");
		}
		body.close_tag("g");
	}
	if !map.notes.is_empty() {
		body.open("g").attr("class", "wardley-notes").end();
		let note_font = small_bold(11.0);
		for (text, x, y) in &map.notes {
			let run = TextRun {
				x: project_x(*x),
				y: project_y(*y),
				anchor: Anchor::Start,
				baseline: Baseline::Alphabetic,
				font: &note_font,
				content: text,
			};
			body.text(run, |svg| _ = svg.attr_if("fill", &axis_label_color));
		}
		body.close_tag("g");
	}
	let accelerator_font = small_bold(10.0);
	for (class, items, is_reversed) in [
		("wardley-accelerators", &map.accelerators, false),
		("wardley-deaccelerators", &map.deaccelerators, true),
	] {
		if items.is_empty() {
			continue;
		}
		body.open("g").attr("class", class).end();
		for (name, x, y) in items.iter() {
			let (x, y) = (project_x(*x), project_y(*y));
			// The arrow, 60 wide and 30 high with a head of 20, pointing right or left.
			let points: [(f64, f64); 7] = [
				(0.0, -15.0),
				(40.0, -15.0),
				(40.0, -23.0),
				(60.0, 0.0),
				(40.0, 23.0),
				(40.0, 15.0),
				(0.0, 15.0),
			];
			let path: Vec<String> = points
				.iter()
				.enumerate()
				.map(|(index, &(px, py))| {
					let px = if is_reversed { 60.0 - px } else { px };
					format!(
						"{} {} {}",
						if index == 0 { "M" } else { "L" },
						number(x + px),
						number(y + py)
					)
				})
				.collect();
			body.open("path")
				.attr("d", &format!("{} Z", path.join(" ")))
				.attr("fill", "white")
				.attr_if("stroke", &component_stroke)
				.attr("stroke-width", "1")
				.close();
			let run = TextRun {
				x: x + 30.0,
				y: y + 30.0,
				anchor: Anchor::Middle,
				baseline: Baseline::Alphabetic,
				font: &accelerator_font,
				content: name,
			};
			body.text(run, |svg| _ = svg.attr_if("fill", &axis_text_color));
		}
		body.close_tag("g");
	}
	body.close_tag("g");

	body.open("defs").end();
	let marker =
		|body: &mut Svg, marker_id: &str, reference_x: &str, size: &str, path: &str, fill: &str| {
			body.open("marker")
				.attr("id", marker_id)
				.attr("viewBox", "0 0 10 10")
				.attr("refX", reference_x)
				.attr("refY", "5")
				.attr("markerWidth", size)
				.attr("markerHeight", size)
				.attr("orient", "auto")
				.end()
				.open("path")
				.attr("d", path)
				.attr_if("fill", fill)
				.attr("stroke", "none")
				.close()
				.close_tag("marker");
		};
	// `orient="auto"`: EdgeHTML ignores `auto-start-reverse`, and only ends of trends have it.
	marker(&mut body, &format!("{id}-arrow"), "9", "6", "M 0 0 L 10 5 L 0 10 z", &evolution_stroke);
	marker(
		&mut body,
		&format!("{id}-link-arrow-end"),
		"9",
		"5",
		"M 0 0 L 10 5 L 0 10 z",
		&link_stroke,
	);
	marker(
		&mut body,
		&format!("{id}-link-arrow-start"),
		"1",
		"5",
		"M 10 0 L 0 5 L 10 10 z",
		&link_stroke,
	);
	body.close_tag("defs");
	let frame = Frame { x: 0.0, y: 0.0, width, height, use_max_width };
	Ok(document(context, "wardley-beta", &frame, &map.fields.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<Map> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn lexes_names_and_keywords_like_chevrotain() {
		assert_eq!(name_length("Campfire Kettle [0.5"), Some(15));
		assert_eq!(name_length("byte pair encoding (BPE)"), Some(24));
		assert_eq!(name_length("real-time processing->x"), Some(20));
		assert_eq!(name_length("A->B"), Some(1));
		assert_eq!(name_length("title x"), None);
		assert_eq!(name_length("titles"), Some(6));
		let kinds =
			|code: &str| lex(code).unwrap().iter().map(|token| token.kind).collect::<Vec<_>>();
		assert_eq!(kinds("A -> B"), [Kind::Name, Kind::Arrow, Kind::Name]);
		assert_eq!(kinds("A +'x'<> B"), [Kind::Name, Kind::LinkArrow, Kind::Name]);
		assert_eq!(kinds("components"), [Kind::Keyword, Kind::Name]);
		assert_eq!(kinds("label [-10, 5]")[2..4], [Kind::Punctuation, Kind::Int]);
	}

	#[test]
	fn builds_the_map_like_mermaid() {
		let map = parsed(
			"wardley-beta\ntitle Tea\nsize [800, 500]\nevolution Genesis@0.2 -> Custom@0.5 -> Product@0.8 / Rental -> Commodity@1.0\nanchor Business [0.95, 0.63]\ncomponent Cup of Tea [0.79, 0.61] label [19, -4] (buy) inertia\ncomponent Kettle [0.43, 0.35]\npipeline Kettle {\n  component Electric [0.62]\n  component Campfire [0.35] label [-5, 3]\n}\nBusiness -> Cup of Tea\nCup of Tea +'boils'> Kettle_Electric ; ignored\nKettle -.-> Electric\nevolve Kettle 0.62\nnote \"standardising\" [0.75, 0.42]\nannotations [0.97, 0.3]\nannotation 1,[0.48, 0.85] \"Standardising\"\naccelerator Fast [0.5, 0.5]\n",
		)
		.unwrap();
		assert_eq!(map.fields.title.as_deref(), Some("Tea"));
		assert_eq!(map.size, Some((800.0, 500.0)));
		assert_eq!(map.stages, ["Genesis", "Custom", "Product / Rental", "Commodity"]);
		assert_eq!(map.stage_boundaries, [0.2, 0.5, 0.8, 1.0]);
		let ids: Vec<&str> = map.nodes.iter().map(|node| node.id.as_str()).collect();
		assert_eq!(ids, ["Business", "Cup of Tea", "Kettle", "Kettle_Electric", "Kettle_Campfire"]);
		assert_eq!(map.nodes[1].strategy.as_deref(), Some("buy"));
		assert!(map.nodes[1].inertia);
		assert!(map.nodes[2].is_pipeline_parent);
		assert_eq!(map.nodes[4].label_offset, (Some(-5.0), Some(3.0)));
		assert_eq!(map.links[1].label.as_deref(), Some("boils"));
		assert_eq!(map.links[1].flow, Some(Flow::Forward));
		assert_eq!(map.links[2].target, "Kettle_Electric");
		assert!(map.links[2].dashed);
		assert_eq!(map.trends, [("Kettle".to_owned(), 62.0, 43.0)]);
		assert_eq!(map.annotations[0].1, 85.0);
		let line = |code: &str| parsed(code).err().and_then(|error| error.line);
		assert_eq!(line("wardley-beta\ncomponent A [1, 0.5]\n"), Some(2));
		assert_eq!(line("wardley-beta\ncomponent market share [0.5, 0.5]\n"), Some(2));
		let error = parsed("wardley-beta\ncomponent A [150.5, 0.5]\n").err().unwrap();
		assert!(error.message.contains("visibility must be between 0-1"));
		assert!(parsed("wardley-beta\npipeline X {\n  component A [0.5]\n}\n").is_err());
	}
}
