//! Event models (`event-modeling.langium`, `parser.ts`, `db.ts`, `renderer.ts`).
//!
//! Mermaid writes the text of a box as HTML in a `<foreignObject>`, which EdgeHTML and resvg do
//! not draw; here it is SVG text laid out like that HTML: a table cell centered in the
//! `<foreignObject>`, the bold name, an empty line and the data in a left-aligned `<code>`
//! block, clipped to the `<foreignObject>`.

use super::common::{sanitize_text, wrap_label};
use super::label::{Run, wrap_normal};
use super::rendering::BBox;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::{Context, Diagram, Result};

const SECTION: &str = "eventmodeling";
const SWIMLANE_MIN_HEIGHT: f64 = 70.0;
const SWIMLANE_PADDING: f64 = 15.0;
const SWIMLANE_GAP: f64 = 10.0;
const BOX_PADDING: f64 = 10.0;
const BOX_OVERLAP: f64 = 90.0;
const BOX_MIN_WIDTH: f64 = 80.0;
const BOX_MAX_WIDTH: f64 = 450.0;
const BOX_MIN_HEIGHT: f64 = 80.0;
const BOX_MAX_HEIGHT: f64 = 750.0;
const CONTENT_START_X: f64 = 250.0;
const TEXT_MAX_WIDTH: f64 = 430.0;
const BOX_TEXT_PADDING: f64 = 10.0;

const KEYWORDS: [&str; 33] = [
	"eventmodeling",
	"resetframe",
	"timeframe",
	"processor",
	"readmodel",
	"command",
	"entity",
	"event",
	"figma",
	"given",
	"jsobj",
	"data",
	"html",
	"json",
	"note",
	"salt",
	"text",
	"then",
	"when",
	"->>",
	"cmd",
	"evt",
	"gwt",
	"pcr",
	"rmo",
	"uri",
	"[[",
	"]]",
	"md",
	"rf",
	"tf",
	"ui",
	"`",
];
const ENTITY_TYPES: [&str; 9] =
	["rmo", "readmodel", "ui", "cmd", "command", "evt", "event", "pcr", "processor"];
const DATA_TYPES: [&str; 8] = ["json", "jsobj", "figma", "salt", "uri", "md", "html", "text"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	Keyword,
	Id,
	FrameId,
	DataInline,
	DataBlock,
	/// `.`
	Dot,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
	kind: Kind,
	text: &'a str,
	line: usize,
}

fn id_length(text: &str) -> usize {
	let bytes = text.as_bytes();
	if !bytes.first().is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_') {
		return 0;
	}
	bytes.iter().take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'_').count()
}

/// `EM_DATA_BLOCK`: `\{[\t ]*\r?\n(?:[\S\s]*?\r?\n)?\}(?:\r?\n|(?!\S))`.
fn data_block_length(text: &str) -> Option<usize> {
	let after = text.strip_prefix('{')?;
	let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
	let after = &after[blanks..];
	let newline = if after.starts_with("\r\n") { 2 } else { usize::from(after.starts_with('\n')) };
	if newline == 0 {
		return None;
	}
	let body_start = 1 + blanks + newline;
	// The first `}` at the start of a line (or right after the first line end) that is
	// followed by a line end, whitespace or the end.
	let mut search = body_start;
	loop {
		let candidate = if search == body_start && text[search..].starts_with('}') {
			Some(search)
		} else {
			text[search..].find("\n}").map(|offset| search + offset + 1)
		};
		let close = candidate?;
		let rest = &text[close + 1..];
		if rest.is_empty() || rest.starts_with(char::is_whitespace) {
			let newline =
				if rest.starts_with("\r\n") { 2 } else { usize::from(rest.starts_with('\n')) };
			return Some(close + 1 + newline);
		}
		search = close;
	}
}

/// `EM_DATA_INLINE`: `\{(.*)\}|"(.*)"|'(.*)'` within the line, greedy.
fn data_inline_length(text: &str) -> Option<usize> {
	let line = &text[..text.find(['\n', '\r']).unwrap_or(text.len())];
	let close = match line.chars().next()? {
		'{' => '}',
		'"' => '"',
		'\'' => '\'',
		_ => return None,
	};
	line.rfind(close).filter(|&end| end > 0).map(|end| end + 1)
}

/// The tokens, or the line of the first character no terminal matches.
fn lex(code: &str) -> std::result::Result<Vec<Token<'_>>, usize> {
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	let mut first_error = None;
	while position < code.len() {
		let rest = &code[position..];
		let whitespace = rest.len() - rest.trim_start().len();
		let hidden = if whitespace > 0 {
			Some(whitespace)
		} else if rest.starts_with("%%") || rest.starts_with("//") {
			Some(rest.find(['\n', '\r']).unwrap_or(rest.len()))
		} else if let Some(comment) = rest.strip_prefix("/*") {
			comment.find("*/").map(|end| end + 4)
		} else {
			None
		};
		let token = match hidden {
			Some(_) => None,
			None => {
				let id = id_length(rest);
				let keyword = KEYWORDS.iter().find(|keyword| rest.starts_with(**keyword));
				match keyword {
					// `EM_ID` is the longer alternative of the keywords.
					Some(keyword) if id <= keyword.len() => Some((Kind::Keyword, keyword.len())),
					_ if id > 0 => Some((Kind::Id, id)),
					_ if rest.starts_with(|c: char| c.is_ascii_digit()) => Some((
						Kind::FrameId,
						rest.bytes().take(3).take_while(u8::is_ascii_digit).count(),
					)),
					_ => data_inline_length(rest)
						.map(|length| (Kind::DataInline, length))
						.or_else(|| data_block_length(rest).map(|length| (Kind::DataBlock, length)))
						.or_else(|| rest.starts_with('.').then_some((Kind::Dot, 1))),
				}
			}
		};
		let length = match (hidden, token) {
			(Some(length), _) => length,
			(None, Some((kind, length))) => {
				tokens.push(Token { kind, text: &rest[..length], line });
				length
			}
			(None, None) => {
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

#[derive(Clone, Debug)]
struct ModelFrame {
	name: String,
	is_reset: bool,
	entity_type: String,
	entity: String,
	sources: Vec<String>,
	data_reference: Option<String>,
	data_inline: Option<String>,
	line: usize,
}

#[derive(Debug, Default)]
struct Model {
	frames: Vec<ModelFrame>,
	/// The data entities: name and block.
	data: Vec<(String, String)>,
}

struct Parser<'a, 'c, 'm> {
	tokens: Vec<Token<'a>>,
	index: usize,
	context: &'c Context<'m>,
}

impl<'a> Parser<'a, '_, '_> {
	fn peek(&self) -> Option<Token<'a>> {
		self.tokens.get(self.index).copied()
	}

	fn is_keyword(&self, keywords: &[&str]) -> bool {
		self.peek()
			.is_some_and(|token| token.kind == Kind::Keyword && keywords.contains(&token.text))
	}

	fn error<T>(&self, expected: &str) -> Result<T> {
		let token = self.peek();
		let line = token.or(self.tokens.last().copied()).map_or(1, |token| token.line);
		let found = match token {
			Some(token) => format!("{:?}", token.text.chars().take(20).collect::<String>()),
			None => "the end of the diagram".to_owned(),
		};
		Err(self.context.error_at(line, format!("expected {expected}, found {found}")))
	}

	fn expect(&mut self, kind: Kind, expected: &str) -> Result<&'a str> {
		match self.peek() {
			Some(token) if token.kind == kind => {
				self.index += 1;
				Ok(token.text)
			}
			_ => self.error(expected),
		}
	}

	fn expect_keyword(&mut self, keywords: &[&str], expected: &str) -> Result<&'a str> {
		match self.is_keyword(keywords) {
			true => self.expect(Kind::Keyword, expected),
			false => self.error(expected),
		}
	}

	fn qualified_name(&mut self) -> Result<String> {
		let mut name = self.expect(Kind::Id, "a name")?.to_owned();
		while self.peek().is_some_and(|token| token.kind == Kind::Dot) {
			self.index += 1;
			name.push('.');
			name.push_str(self.expect(Kind::Id, "a name")?);
		}
		Ok(name)
	}

	/// `` ('`' EmDataType '`')? `` before data.
	fn data_type(&mut self) -> Result<()> {
		if self.is_keyword(&["`"]) {
			self.index += 1;
			self.expect_keyword(&DATA_TYPES, "a data type")?;
			self.expect_keyword(&["`"], "`` ` ``")?;
		}
		Ok(())
	}

	fn parse(mut self) -> Result<Model> {
		let mut model = Model::default();
		self.expect_keyword(&["eventmodeling"], "`eventmodeling`")?;
		while let Some(token) = self.peek() {
			if token.kind != Kind::Keyword {
				return self.error("a statement or the end of the diagram");
			}
			match token.text {
				"tf" | "timeframe" | "rf" | "resetframe" => {
					self.index += 1;
					let name = self.expect(Kind::FrameId, "a frame number")?.to_owned();
					let entity_type =
						self.expect_keyword(&ENTITY_TYPES, "an entity type")?.to_owned();
					let entity = self.qualified_name()?;
					let mut sources = Vec::new();
					while self.is_keyword(&["->>"]) {
						self.index += 1;
						sources.push(self.expect(Kind::FrameId, "a frame number")?.to_owned());
					}
					let mut data_reference = None;
					if self.is_keyword(&["[["]) {
						self.index += 1;
						data_reference = Some(self.expect(Kind::Id, "a data name")?.to_owned());
						self.expect_keyword(&["]]"], "`]]`")?;
					}
					let mut data_inline = None;
					let has_data_type = self.is_keyword(&["`"]);
					if has_data_type
						|| self.peek().is_some_and(|token| token.kind == Kind::DataInline)
					{
						self.data_type()?;
						data_inline =
							Some(self.expect(Kind::DataInline, "inline data")?.to_owned());
					}
					model.frames.push(ModelFrame {
						name,
						is_reset: token.text.starts_with('r'),
						entity_type,
						entity,
						sources,
						data_reference,
						data_inline,
						line: token.line,
					});
				}
				"entity" => {
					self.index += 1;
					self.qualified_name()?;
				}
				"data" => {
					self.index += 1;
					let name = self.expect(Kind::Id, "a data name")?.to_owned();
					self.data_type()?;
					let block = self.expect(Kind::DataBlock, "a data block")?.to_owned();
					model.data.push((name, block));
				}
				"note" => {
					self.index += 1;
					self.expect(Kind::FrameId, "a frame number")?;
					self.data_type()?;
					self.expect(Kind::DataBlock, "a data block")?;
				}
				"gwt" => {
					self.index += 1;
					self.expect(Kind::FrameId, "a frame number")?;
					self.expect_keyword(&["given"], "`given`")?;
					self.statements()?;
					if self.is_keyword(&["when"]) {
						self.index += 1;
						self.statements()?;
					}
					self.expect_keyword(&["then"], "`then`")?;
					self.statements()?;
				}
				_ => return self.error("a statement or the end of the diagram"),
			}
		}
		Ok(model)
	}

	/// `EmGwtStatement+`
	fn statements(&mut self) -> Result<()> {
		loop {
			self.expect_keyword(&ENTITY_TYPES, "an entity type")?;
			self.expect(Kind::Id, "an entity name")?;
			if !self.is_keyword(&ENTITY_TYPES) {
				return Ok(());
			}
		}
	}
}

fn parse(code: &str, context: &Context) -> Result<Model> {
	let tokens = lex(code).map_err(|line| context.error_at(line, "unexpected character"))?;
	let model = Parser { tokens, index: 0, context }.parse()?;
	let mut names = std::collections::HashSet::new();
	for frame in &model.frames {
		if !names.insert(frame.name.as_str()) {
			return Err(context.error_at(
				frame.line,
				format!("Duplicate event modeling frame ID \"{}\"", frame.name),
			));
		}
	}
	Ok(model)
}

struct Swimlane {
	index: i64,
	label: String,
	r: f64,
	y: f64,
	height: f64,
	max_height: f64,
}

struct ModelBox {
	x: f64,
	r: f64,
	width: f64,
	height: f64,
	swimlane: usize,
	fill: String,
	stroke: String,
	name_lines: Vec<String>,
	data: Option<String>,
}

/// `extractNamespace` and `extractName`: `a.b` splits, other names have no namespace.
fn split_entity(entity: &str) -> (Option<&str>, &str) {
	let parts: Vec<&str> = entity.split('.').collect();
	match parts.as_slice() {
		[namespace, name] => (Some(namespace), name),
		_ => (None, entity),
	}
}

/// `calculateSwimlaneProps`: the swimlane index and label of a frame. Mermaid's swimlanes keep
/// no namespace, so each frame with a namespace opens the next free swimlane of its kind.
fn swimlane_props(
	swimlanes: &[Swimlane],
	entity_type: &str,
	namespace: Option<&str>,
) -> (i64, String) {
	let (base, label, prefix) = match entity_type {
		"ui" | "pcr" | "processor" => (0, "UI/Automation", "UI/A: "),
		"rmo" | "readmodel" | "cmd" | "command" => (100, "Command/Read Model", "C/RM: "),
		_ => (200, "Events", "Stream: "),
	};
	match namespace.filter(|namespace| !namespace.is_empty()) {
		Some(namespace) => {
			// `findNextAvailableIndex`
			let next = swimlanes
				.iter()
				.map(|lane| lane.index)
				.filter(|&index| index > base && index < base + 100)
				.fold(base, i64::max)
				+ 1;
			(next, format!("{prefix}{namespace}"))
		}
		None => (base, label.to_owned()),
	}
}

/// The parts of a box text: the name lines and the data text with `<br/>` breaks.
fn box_text(
	context: &mut Context,
	frame: &ModelFrame,
	model: &Model,
) -> (String, Vec<String>, Option<String>) {
	let mut wrap_font = Font::new("\"trebuchet ms\", verdana, arial, sans-serif", 16.0);
	wrap_font.weight = 700;
	let mut width_of = |text: &str| context.text_dimensions(text, &wrap_font).0;
	let name = sanitize_text(split_entity(&frame.entity).1);
	let wrapped_name = wrap_label(&name, TEXT_MAX_WIDTH, &mut width_of);
	let mut content = format!("<b>{wrapped_name}</b>");
	let mut data = None;
	if let Some(inline) = &frame.data_inline {
		let after = inline.find('{').map_or(inline.as_str(), |start| &inline[start + 1..]);
		let text = js_substring_to_last(after, '}');
		data = Some(
			wrap_label(&sanitize_text(text), TEXT_MAX_WIDTH, &mut width_of).replace(' ', "&nbsp;"),
		);
	}
	if let Some(reference) = &frame.data_reference
		&& let Some((_, block)) = model.data.iter().find(|(name, _)| name == reference)
	{
		let after = block.find("{\n").map_or(block.as_str(), |start| &block[start + 2..]);
		let text = js_substring_to_last(after, '}');
		let wrapped = wrap_label(&sanitize_text(text), TEXT_MAX_WIDTH, &mut width_of);
		data = Some(format!("{}<br/>", wrapped.replace(' ', "&nbsp;")));
	}
	if let Some(data) = &data {
		content.push_str(&format!(
			"<br/><br/><code style=\"text-align: left; display: block;max-width:{TEXT_MAX_WIDTH}px\">{data}</code>"
		));
	}
	let name_lines =
		super::common::split_breaks(&wrapped_name).iter().map(|line| (*line).to_owned()).collect();
	(content, name_lines, data)
}

/// `text.substring(0, text.lastIndexOf(close) - 1)`, which also drops the character before
/// `close`, and everything without it.
fn js_substring_to_last(text: &str, close: char) -> &str {
	let Some(end) = text.rfind(close) else { return "" };
	let units: Vec<usize> = text[..end].char_indices().map(|(index, _)| index).collect();
	match units.last() {
		Some(&last) => &text[..last],
		None => "",
	}
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let model = parse(code, context)?;
	let theme_or = |context: &Context, key: &str, default: &str| {
		context.theme.text(key).unwrap_or_else(|| default.to_owned())
	};
	let mut text_font = Font::new("\"trebuchet ms\", verdana, arial, sans-serif", 16.0);
	text_font.weight = 700;

	// `getState`: frames placed one after another, swimlanes stacked by index.
	let mut swimlanes: Vec<Swimlane> = Vec::new();
	let mut boxes: Vec<ModelBox> = Vec::new();
	let mut relations: Vec<(usize, usize)> = Vec::new();
	let mut max_r = 0f64;
	let mut previous_swimlane: Option<usize> = None;
	for (index, frame) in model.frames.iter().enumerate() {
		let (content, name_lines, data) = box_text(context, frame, &model);
		let (text_width, text_height) = context.text_dimensions(&content, &text_font);
		let text_width = if data.is_some() { text_width / 3.0 } else { text_width };
		let (fill, stroke) = match frame.entity_type.as_str() {
			"ui" => {
				(theme_or(context, "emUiFill", "white"), theme_or(context, "emUiStroke", "#dbdada"))
			}
			"pcr" | "processor" => (
				theme_or(context, "emProcessorFill", "#edb3f6"),
				theme_or(context, "emProcessorStroke", "#b88cbf"),
			),
			"rmo" | "readmodel" => (
				theme_or(context, "emReadModelFill", "#d3f1a2"),
				theme_or(context, "emReadModelStroke", "#a3b732"),
			),
			"cmd" | "command" => (
				theme_or(context, "emCommandFill", "#bcd6fe"),
				theme_or(context, "emCommandStroke", "#679ac3"),
			),
			_ => (
				theme_or(context, "emEventFill", "#ffb778"),
				theme_or(context, "emEventStroke", "#c19a0f"),
			),
		};
		let (namespace, _) = split_entity(&frame.entity);
		let (lane_index, label) = swimlane_props(&swimlanes, &frame.entity_type, namespace);
		let lane = match swimlanes.iter().position(|lane| lane.index == lane_index) {
			Some(lane) => lane,
			None => {
				swimlanes.push(Swimlane {
					index: lane_index,
					label,
					r: 0.0,
					y: lane_index as f64 * SWIMLANE_MIN_HEIGHT + SWIMLANE_GAP,
					height: SWIMLANE_MIN_HEIGHT,
					max_height: SWIMLANE_MIN_HEIGHT,
				});
				swimlanes.len() - 1
			}
		};
		let width = BOX_MIN_WIDTH.max(BOX_MAX_WIDTH.min(text_width + 2.0 * BOX_TEXT_PADDING))
			+ 2.0 * BOX_PADDING;
		let height = BOX_MIN_HEIGHT.max(BOX_MAX_HEIGHT.min(text_height + 2.0 * BOX_TEXT_PADDING))
			+ 2.0 * BOX_PADDING;
		// `calculateX`
		let x = match previous_swimlane {
			None => CONTENT_START_X,
			Some(previous)
				if swimlanes[previous].index == lane_index && swimlanes[lane].r != 0.0 =>
			{
				swimlanes[lane].r + BOX_PADDING
			}
			Some(_) => match boxes.last() {
				Some(last) => last.r - BOX_OVERLAP + BOX_PADDING,
				None => CONTENT_START_X,
			},
		};
		let r = x + width + BOX_PADDING;
		max_r = swimlanes.iter().map(|lane| lane.r).fold(r, f64::max);
		let swimlane = &mut swimlanes[lane];
		swimlane.r = x + width;
		swimlane.max_height = swimlane.max_height.max(height);
		swimlane.height = SWIMLANE_MIN_HEIGHT.max(swimlane.max_height) + 2.0 * SWIMLANE_PADDING;
		boxes.push(ModelBox {
			x,
			r,
			width,
			height,
			swimlane: lane,
			fill,
			stroke,
			name_lines,
			data,
		});
		previous_swimlane = Some(lane);
		let mut order: Vec<usize> = (0..swimlanes.len()).collect();
		npp::sort::sort_by_key(&mut order[..], |&lane| swimlanes[lane].index);
		let mut y = 0.0;
		for &lane in &order {
			swimlanes[lane].y = y;
			y += swimlanes[lane].height + SWIMLANE_GAP;
		}

		// `decidePositionRelation`
		let target = boxes.len() - 1;
		if index == 0 && frame.sources.is_empty() {
			continue;
		}
		if frame.sources.is_empty() {
			if frame.is_reset {
				continue;
			}
			let source = (0..index).rev().find(|&earlier| boxes[earlier].swimlane != lane);
			if let Some(source) = source {
				relations.push((source, target));
			}
		} else {
			for (earlier, other) in model.frames.iter().enumerate() {
				if frame.sources.contains(&other.name) && earlier < boxes.len() {
					relations.push((earlier, target));
				}
			}
		}
	}

	let mut body = Svg::new();
	let mut bbox = BBox::default();
	let lane_background = theme_or(context, "emSwimlaneBackgroundOdd", "rgb(250,250,250)");
	let lane_stroke = theme_or(context, "emSwimlaneBackgroundStroke", "rgb(240,240,240)");
	let mut lane_font = Font::new(context.font_family(), context.font_size());
	lane_font.weight = 700;
	let mut order: Vec<usize> = (0..swimlanes.len()).collect();
	npp::sort::sort_by_key(&mut order[..], |&lane| swimlanes[lane].index);
	for &lane in &order {
		let lane = &swimlanes[lane];
		body.open("g").attr("class", "em-swimlane").end();
		body.open("rect")
			.attr("x", "0")
			.number_attr("y", lane.y)
			.attr("rx", "3")
			.number_attr("width", max_r + SWIMLANE_PADDING)
			.number_attr("height", lane.height)
			.attr_if("fill", &lane_background)
			.attr_if("stroke", &lane_stroke)
			.close();
		bbox.include_rect(0.0, lane.y, max_r + SWIMLANE_PADDING, lane.height);
		let run = TextRun {
			x: 30.0,
			y: lane.y + 30.0,
			anchor: Anchor::Start,
			baseline: Baseline::Alphabetic,
			font: &lane_font,
			content: &lane.label,
		};
		body.text(run, |_| {});
		let (width, height) = context.bounding_box(&lane.label, &lane_font);
		if width > 0.0 {
			let left = context.bounding_box_left(&lane.label, &lane_font);
			let top = context.bounding_box_top(&lane.label, &lane_font);
			bbox.include_rect(30.0 + left, lane.y + 30.0 + top, width, height);
		}
		body.close_tag("g");
	}
	let mut clip_count = 0;
	for model_box in &boxes {
		let y = swimlanes[model_box.swimlane].y + SWIMLANE_PADDING;
		body.open("g").attr("class", "em-box").end();
		body.open("rect")
			.number_attr("x", model_box.x)
			.number_attr("y", y)
			.attr("rx", "3")
			.number_attr("width", model_box.width)
			.number_attr("height", model_box.height)
			.attr_if("stroke", &model_box.stroke)
			.attr_if("fill", &model_box.fill)
			.close();
		bbox.include_rect(model_box.x, y, model_box.width, model_box.height);
		let area = [
			model_box.x + BOX_PADDING,
			y + 10.0,
			model_box.width - 2.0 * BOX_PADDING,
			model_box.height - 2.0 * BOX_PADDING,
		];
		write_box_text(&mut body, context, model_box, area, &mut clip_count);
		body.close_tag("g");
	}
	let relation_stroke = theme_or(context, "emRelationStroke", "#000");
	let marker_id = format!("{}-em-arrowhead", context.id);
	for &(source, target) in &relations {
		let (source, target) = (&boxes[source], &boxes[target]);
		let source_y = swimlanes[source.swimlane].y + SWIMLANE_PADDING;
		let target_y = swimlanes[target.swimlane].y + SWIMLANE_PADDING;
		let source_x = source.x + source.width * 2.0 / 3.0;
		let target_x = target.x + target.width / 3.0;
		let (from_y, to_y) = match source_y > target_y {
			true => (source_y, target_y + target.height),
			false => (source_y + source.height, target_y),
		};
		let path = format!(
			"M{} {} L{} {}",
			number(source_x),
			number(from_y),
			number(target_x),
			number(to_y)
		);
		body.open("path")
			.attr("class", "em-relation")
			.attr("fill", "none")
			.attr_if("stroke", &relation_stroke)
			.attr("stroke-width", "1")
			.attr("marker-end", &format!("url(#{marker_id})"))
			.attr("d", &path)
			.close();
		bbox.include_point(super::rendering::Point { x: source_x, y: from_y });
		bbox.include_point(super::rendering::Point { x: target_x, y: to_y });
	}
	body.open("defs")
		.end()
		.open("marker")
		.attr("id", &marker_id)
		.attr("markerWidth", "10")
		.attr("markerHeight", "7")
		.attr("refX", "10")
		.attr("refY", "3.5")
		.attr("orient", "auto")
		.end()
		.open("polygon")
		.attr("points", "0 0, 10 3.5, 0 7")
		.attr_if("fill", &theme_or(context, "emArrowhead", "#000000"))
		.close()
		.close_tag("marker")
		.close_tag("defs");

	let padding = context.config_number(SECTION, "padding", 30.0);
	let (x, y) = if bbox.is_empty() { (0.0, 0.0) } else { (bbox.min.x, bbox.min.y) };
	let frame = Frame {
		x: x - padding,
		y: y - padding,
		width: bbox.width() + 2.0 * padding,
		height: bbox.height() + 2.0 * padding,
		use_max_width: context.config_bool(SECTION, "useMaxWidth", true),
	};
	Ok(document(context, "eventmodeling", &frame, &Default::default(), body))
}

/// The text of HTML without tags: entities decoded and runs of ASCII whitespace collapsed like
/// `white-space: normal`; no-break spaces stay.
fn html_text(html: &str) -> String {
	const ENTITIES: [(&str, char); 6] = [
		("&nbsp;", '\u{a0}'),
		("&lt;", '<'),
		("&gt;", '>'),
		("&quot;", '"'),
		("&#39;", '\''),
		("&amp;", '&'),
	];
	let mut text = String::with_capacity(html.len());
	let mut rest = html;
	while let Some(character) = rest.chars().next() {
		if character == '<'
			&& let Some(end) = rest.find('>')
		{
			rest = &rest[end + 1..];
			continue;
		}
		if let Some((entity, decoded)) =
			ENTITIES.iter().find(|(entity, _)| rest.starts_with(entity))
		{
			text.push(*decoded);
			rest = &rest[entity.len()..];
			continue;
		}
		text.push(character);
		rest = &rest[character.len_utf8()..];
	}
	super::text::collapse_whitespace(&text)
}

/// A line of the box text: its pieces and whether it is the bold name.
struct TextLine {
	text: String,
	is_name: bool,
}

/// The HTML of a box as SVG text: the `<span>` is a table cell filling `area`, centered both
/// ways; a wider word widens the cell, and the `<foreignObject>` clips what overflows.
fn write_box_text(
	svg: &mut Svg,
	context: &mut Context,
	model_box: &ModelBox,
	[x, y, width, height]: [f64; 4],
	clip_count: &mut usize,
) {
	let root = Font::new(context.font_family(), context.font_size());
	// The HTML takes the color of the page: black in Mermaid's renders, which the boxes of
	// dark themes cannot show, so these use the theme's text color.
	let background = context.theme.text("background").unwrap_or_default();
	let is_dark = super::color::is_dark(&background) == Some(true);
	let text_color = match is_dark {
		true => context.theme.text("textColor").unwrap_or_default(),
		false => "black".to_owned(),
	};
	let mut name_font = root.clone();
	name_font.weight = 700;
	let code_font = Font::new("monospace", root.size);
	let mut lines: Vec<TextLine> = Vec::new();
	let mut cell_width = width;
	for name in &model_box.name_lines {
		let text = html_text(name);
		cell_width = cell_width.max(context.measure(&text, &name_font).0);
		lines.push(TextLine { text, is_name: true });
	}
	let mut code_lines = Vec::new();
	if let Some(data) = &model_box.data {
		lines.push(TextLine { text: String::new(), is_name: true });
		let parts: Vec<String> = super::common::split_breaks(data)
			.into_iter()
			.map(html_text)
			.filter(|text| !text.is_empty())
			.collect();
		// The block's `max-width` caps what it adds to the width of the table cell.
		let mut widest = 0f64;
		for part in &parts {
			let runs = [Run { text: part.clone(), style: Default::default() }];
			widest = widest.max(wrap_normal(context, &code_font, &runs, f64::INFINITY).1);
		}
		cell_width = cell_width.max(widest.min(TEXT_MAX_WIDTH));
		for part in parts {
			let runs = [Run { text: part, style: Default::default() }];
			let (laid, _) = wrap_normal(context, &code_font, &runs, TEXT_MAX_WIDTH.min(cell_width));
			for line in laid {
				let text: String = line.runs.into_iter().map(|(run, _)| run.text).collect();
				code_lines.push(text.trim_end_matches(' ').to_owned());
			}
		}
	}
	let name_height = context.normal_line_height(&root);
	let code_height = context.normal_line_height(&code_font);
	let content_height = name_height * lines.len() as f64 + code_height * code_lines.len() as f64;
	let top = y + ((height - content_height) / 2.0).max(0.0);
	let is_clipped = content_height > height || cell_width > width;
	if is_clipped {
		*clip_count += 1;
		let id = format!("{}-em-clip-{clip_count}", context.id);
		svg.open("clipPath").attr("id", &id).end();
		svg.open("rect")
			.number_attr("x", x)
			.number_attr("y", y)
			.number_attr("width", width)
			.number_attr("height", height)
			.close();
		svg.close_tag("clipPath");
		svg.open("g").attr("clip-path", &format!("url(#{id})")).end();
	}
	let baseline = |context: &mut Context, font: &Font, line_height: f64| {
		let (ascent, descent) = context.vertical_metrics(font);
		let (ascent, descent) = (ascent.round(), descent.round());
		((line_height - ascent - descent) / 2.0).floor() + ascent
	};
	let name_baseline = baseline(context, &root, name_height);
	let code_baseline = baseline(context, &code_font, code_height);
	let mut line_top = top;
	for line in &lines {
		if !line.text.is_empty() {
			let run = TextRun {
				x: x + cell_width / 2.0,
				y: line_top + name_baseline,
				anchor: Anchor::Middle,
				baseline: Baseline::Alphabetic,
				font: if line.is_name { &name_font } else { &root },
				content: &line.text,
			};
			svg.text(run, |svg| _ = svg.attr_if("fill", &text_color));
		}
		line_top += name_height;
	}
	for line in &code_lines {
		let run = TextRun {
			x,
			y: line_top + code_baseline,
			anchor: Anchor::Start,
			baseline: Baseline::Alphabetic,
			font: &code_font,
			content: line,
		};
		svg.text_with_spaces(run, |svg| _ = svg.attr_if("fill", &text_color));
		line_top += code_height;
	}
	if is_clipped {
		svg.close_tag("g");
	}
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn model(code: &str) -> Result<Model> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_like_langium() {
		let parsed = model(
			"eventmodeling\ntf 01 ui A\nentity Foo.Bar\nnote 01 {\n  hi\n}\ngwt 01 given evt Foo then cmd Bar\n// c\n/* a\nb */\ntf 02 cmd X.Y ->> 01 [[D]] `json` { a: 1 }\nrf 03 evt Z\ndata D {\n  a: 1\n}\n",
		)
		.unwrap();
		assert_eq!(parsed.frames.len(), 3);
		assert_eq!(parsed.frames[1].entity, "X.Y");
		assert_eq!(parsed.frames[1].sources, ["01"]);
		assert_eq!(parsed.frames[1].data_reference.as_deref(), Some("D"));
		assert_eq!(parsed.frames[1].data_inline.as_deref(), Some("{ a: 1 }"));
		assert!(parsed.frames[2].is_reset);
		assert_eq!(parsed.data, [("D".to_owned(), "{\n  a: 1\n}\n".to_owned())]);
		let line = |code: &str| model(code).err().and_then(|error| error.line);
		assert_eq!(line("eventmodeling\ntitle Hello\ntf 01 ui A\n"), Some(2));
		assert_eq!(line("eventmodeling\naccTitle: x\n"), Some(2));
		assert_eq!(line("eventmodeling\ntf 01 ui A\ntf 01 cmd B\n"), Some(3));
		assert_eq!(line("eventmodeling\ntf 01 ui event\n"), Some(2));
	}

	#[test]
	fn cuts_data_like_mermaid() {
		assert_eq!(js_substring_to_last(" productId: 7 }", '}'), " productId: 7");
		assert_eq!(js_substring_to_last("quoted", '}'), "");
		assert_eq!(js_substring_to_last("}", '}'), "");
	}
}
