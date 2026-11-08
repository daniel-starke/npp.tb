//! User journeys (`journey.jison`, `journeyDb.js`, `journeyRenderer.ts`, `svgDraw.js`,
//! `styles.js`).

use super::jison::{keyword_and_delimiter, line_length, span, starts_with_ignoring_case};
use super::label::{Run, wrap_normal};
use super::svg::{Accessibility, Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::{Font, collapse_whitespace};
use super::{Context, Diagram, Result};

/// The defaults of `actorColours` and `sectionFills`. Mermaid drops arrays from the diagram's
/// configuration, so these are always used.
const ACTOR_COLOURS: [&str; 6] = ["#8FBC8F", "#7CFC00", "#00FFFF", "#20B2AA", "#B0E0E6", "#FFFFE0"];
const SECTION_FILLS: [&str; 7] =
	["#191970", "#8B008B", "#4B0082", "#2F4F4F", "#800000", "#8B4513", "#00008B"];
/// The only entry of the default `sectionColours`.
const SECTION_COLOUR: &str = "#fff";
/// The bottom of the task lines; a score of 5 sits at 300.
const TASK_LINE_END: f64 = 300.0 + 5.0 * 30.0;
const FACE_RADIUS: f64 = 15.0;
const SMILE: &str = "M7.5,0A7.5,7.5,0,1,1,-7.5,0L-6.818,0A6.818,6.818,0,1,0,6.818,0Z";
const FROWN: &str = "M-7.5,0A7.5,7.5,0,1,1,7.5,0L6.818,0A6.818,6.818,0,1,0,-6.818,0Z";

#[derive(Clone, Debug, PartialEq)]
enum Token {
	NewLine,
	Journey,
	Title(String),
	AccTitle(String),
	AccDescription(String),
	Section(String),
	TaskName(String),
	TaskData(String),
	Colon,
	Invalid(char),
	End,
}

/// `[^#\n;]`
fn is_text_character(character: char) -> bool {
	!matches!(character, '#' | '\n' | ';')
}

/// `[^#:\n;]`
fn is_name_character(character: char) -> bool {
	is_text_character(character) && character != ':'
}

/// `keyword\s[^#\n;]+` style rules: the length of the match and of the keyword with its
/// whitespace character, which `substr` removes.
fn keyword_statement(
	text: &str,
	keyword: &str,
	is_value_character: fn(char) -> bool,
) -> Option<(usize, usize)> {
	if !starts_with_ignoring_case(text, keyword) {
		return None;
	}
	let space =
		text[keyword.len()..].chars().next().filter(|character| character.is_whitespace())?;
	let prefix = keyword.len() + space.len_utf8();
	let value = span(&text[prefix..], is_value_character);
	(value > 0).then_some((prefix + value, prefix))
}

/// The tokens of `journey.jison` with the line each ends on, where jison reports errors. Like
/// jison, the first rule that matches wins.
fn lex(code: &str) -> Vec<(Token, usize)> {
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	while position < code.len() {
		let rest = &code[position..];
		let first = rest.chars().next().unwrap_or_default();
		let mut token = None;
		let length = if rest.starts_with("%%") && !rest[2..].starts_with('{') {
			line_length(rest)
		} else if first != '}' && rest[first.len_utf8()..].starts_with("%%") {
			// `[^\}]\%\%[^\n]*`, whose first character may be a line end.
			first.len_utf8() + line_length(&rest[first.len_utf8()..])
		} else if first == '\n' {
			token = Some(Token::NewLine);
			span(rest, |character| character == '\n')
		} else if first.is_whitespace() {
			span(rest, char::is_whitespace)
		} else if first == '#' {
			line_length(rest)
		} else if starts_with_ignoring_case(rest, "journey") {
			token = Some(Token::Journey);
			"journey".len()
		} else if let Some((length, prefix)) = keyword_statement(rest, "title", is_text_character) {
			token = Some(Token::Title(rest[prefix..length].to_owned()));
			length
		} else if let Some(length) = keyword_and_delimiter(rest, "accTitle", ':') {
			let value = line_length(&rest[length..]);
			token = Some(Token::AccTitle(rest[length..length + value].trim().to_owned()));
			length + value
		} else if let Some(length) = keyword_and_delimiter(rest, "accDescr", ':') {
			let value = line_length(&rest[length..]);
			token = Some(Token::AccDescription(rest[length..length + value].trim().to_owned()));
			length + value
		} else if let Some(length) = keyword_and_delimiter(rest, "accDescr", '{') {
			let value = span(&rest[length..], |character| character != '}');
			if value > 0 {
				let text = rest[length..length + value].trim().to_owned();
				token = Some(Token::AccDescription(text));
			}
			let closing = usize::from(rest[length + value..].starts_with('}'));
			length + value + closing
		} else if let Some((length, prefix)) = keyword_statement(rest, "section", is_name_character)
		{
			token = Some(Token::Section(rest[prefix..length].to_owned()));
			length
		} else if is_name_character(first) {
			let length = span(rest, is_name_character);
			token = Some(Token::TaskName(rest[..length].to_owned()));
			length
		} else if first == ':' {
			let length = 1 + span(&rest[1..], is_text_character);
			token = Some(match length {
				1 => Token::Colon,
				_ => Token::TaskData(rest[..length].to_owned()),
			});
			length
		} else {
			token = Some(Token::Invalid(first));
			first.len_utf8()
		};
		line += rest[..length].matches('\n').count();
		position += length;
		if let Some(token) = token {
			tokens.push((token, line));
		}
	}
	tokens.push((Token::End, line));
	tokens
}

#[derive(Clone, Debug, PartialEq)]
struct Task {
	section: String,
	name: String,
	score: f64,
	people: Vec<String>,
}

#[derive(Debug, Default)]
struct Journey {
	title: Option<String>,
	accessibility: Accessibility,
	tasks: Vec<Task>,
}

impl Journey {
	/// The people of all tasks in JavaScript's sort order (UTF-16 code units).
	fn actors(&self) -> Vec<&str> {
		let mut actors: Vec<&str> =
			self.tasks.iter().flat_map(|task| task.people.iter().map(String::as_str)).collect();
		npp::sort::sort_by(&mut actors[..], |a, b| a.encode_utf16().cmp(b.encode_utf16()));
		actors.dedup();
		actors
	}
}

/// JavaScript's `Number(text)`.
fn js_number_of_string(text: &str) -> f64 {
	let text =
		text.trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}');
	if text.is_empty() {
		return 0.0;
	}
	match text {
		"Infinity" | "+Infinity" => return f64::INFINITY,
		"-Infinity" => return f64::NEG_INFINITY,
		_ => {}
	}
	for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
		if let Some(digits) = text.strip_prefix(prefix) {
			return match digits.chars().all(|digit| digit.is_digit(radix)) && !digits.is_empty() {
				true => digits.chars().fold(0.0, |value, digit| {
					value * f64::from(radix) + f64::from(digit.to_digit(radix).unwrap_or(0))
				}),
				false => f64::NAN,
			};
		}
	}
	let is_decimal = text
		.bytes()
		.all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-'));
	match is_decimal {
		true => text.parse().unwrap_or(f64::NAN),
		false => f64::NAN,
	}
}

/// `addTask`: the score and the people of `: score: people`.
fn task(section: &str, name: &str, data: &str) -> Task {
	let pieces: Vec<&str> = data[1..].split(':').collect();
	let score = js_number_of_string(pieces[0]);
	let people = match pieces.get(1) {
		Some(people) => people.split(',').map(|person| person.trim().to_owned()).collect(),
		None => Vec::new(),
	};
	Task { section: section.to_owned(), name: name.to_owned(), score, people }
}

fn describe(token: &Token) -> String {
	match token {
		Token::NewLine => "end of line".to_owned(),
		Token::End => "end of input".to_owned(),
		Token::Journey => "`journey`".to_owned(),
		Token::Title(text) => format!("{:?}", format!("title {text}")),
		Token::AccTitle(_) => "`accTitle`".to_owned(),
		Token::AccDescription(_) => "`accDescr`".to_owned(),
		Token::Section(text) => format!("{:?}", format!("section {text}")),
		Token::TaskName(text) | Token::TaskData(text) => format!("{:?}", text.trim()),
		Token::Colon => "\":\"".to_owned(),
		Token::Invalid(character) => format!("{character:?}"),
	}
}

fn parse(code: &str, context: &Context) -> Result<Journey> {
	let mut journey = Journey { title: context.title.clone(), ..Journey::default() };
	let mut tokens = lex(code).into_iter().skip_while(|(token, _)| *token == Token::NewLine);
	let error = |(token, line): &(Token, usize), expected: &str| {
		context.error_at(*line, format!("expected {expected}, found {}", describe(token)))
	};
	match tokens.next() {
		Some((Token::Journey, _)) => {}
		Some(token) => return Err(error(&token, "`journey`")),
		None => return Err(context.error_at(1, "expected `journey`")),
	}
	let mut section = String::new();
	while let Some(token) = tokens.next() {
		match token.0 {
			Token::NewLine => {}
			Token::End => break,
			Token::Title(title) => journey.title = Some(title),
			Token::AccTitle(title) => journey.accessibility.title = title,
			Token::AccDescription(description) => {
				journey.accessibility.description = description;
			}
			Token::Section(name) => section = name,
			Token::TaskName(name) => match tokens.next() {
				Some((Token::TaskData(data), _)) => {
					journey.tasks.push(task(&section, &name, &data))
				}
				Some(token) => {
					return Err(error(
						&token,
						&format!("`: score: people` after {:?}", name.trim()),
					));
				}
				None => break,
			},
			_ => return Err(error(&token, "a section, a task or the end")),
		}
	}
	Ok(journey)
}

/// How task and section texts are drawn (`textPlacement`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum TextPlacement {
	/// An HTML `<div>` in a `<foreignObject>`, the default.
	ForeignObject,
	/// One `<text>` per `<br>` separated line in the task font.
	Tspan,
	/// One `<text>` in the diagram font.
	Old,
}

struct Settings {
	margin_x: f64,
	margin_y: f64,
	left_margin: f64,
	max_label_width: f64,
	width: f64,
	height: f64,
	box_text_margin: f64,
	task_margin: f64,
	task_font: Font,
	text_placement: TextPlacement,
	title_color: String,
	title_font_family: String,
	title_font_size: String,
	use_max_width: bool,
}

fn settings(context: &Context) -> Settings {
	let number = |key: &str, default: f64| context.config_number("journey", key, default);
	let text = |key: &str, default: &str| {
		context.config_text("journey", key).unwrap_or_else(|| default.to_owned())
	};
	let task_font_size = context
		.config_text("journey", "taskFontSize")
		.and_then(|size| super::text::parse_font_size(&size, 16.0))
		.unwrap_or(14.0);
	Settings {
		margin_x: number("diagramMarginX", 50.0),
		margin_y: number("diagramMarginY", 10.0),
		left_margin: number("leftMargin", 150.0),
		max_label_width: number("maxLabelWidth", 360.0),
		width: number("width", 150.0),
		height: number("height", 50.0),
		box_text_margin: number("boxTextMargin", 5.0),
		task_margin: number("taskMargin", 50.0),
		task_font: Font::new(text("taskFontFamily", "\"Open Sans\", sans-serif"), task_font_size),
		text_placement: match text("textPlacement", "fo").as_str() {
			"fo" => TextPlacement::ForeignObject,
			"old" => TextPlacement::Old,
			_ => TextPlacement::Tspan,
		},
		title_color: text("titleColor", ""),
		title_font_family: text("titleFontFamily", "\"trebuchet ms\", verdana, arial, sans-serif"),
		title_font_size: text("titleFontSize", "4ex"),
		use_max_width: context.config_bool("journey", "useMaxWidth", true),
	}
}

/// The pixels of a CSS `font-size` of an element whose parent has `parent_font`; `None` if the
/// value is invalid and the size is inherited.
pub(super) fn css_font_size(context: &mut Context, value: &str, parent_font: &Font) -> Option<f64> {
	let value = value.trim();
	let number_length = value
		.find(|character: char| {
			!(character.is_ascii_digit() || matches!(character, '.' | '+' | '-'))
		})
		.unwrap_or(value.len());
	let number: f64 = value[..number_length].parse().ok()?;
	let size = match value[number_length..].to_ascii_lowercase().as_str() {
		"" | "px" => number,
		"em" => number * parent_font.size,
		"rem" => number * 16.0,
		"ex" => number * context.x_height(parent_font),
		"%" => number * parent_font.size / 100.0,
		"pt" => number * 4.0 / 3.0,
		"pc" => number * 16.0,
		"in" => number * 96.0,
		"cm" => number * 96.0 / 2.54,
		"mm" => number * 96.0 / 25.4,
		_ => return None,
	};
	(size.is_finite() && size >= 0.0).then_some(size)
}

/// The lines of an actor's legend label: the whole name if it fits `max_width`, else words,
/// and words too wide broken with hyphens (`drawActorLegend`).
fn legend_lines(context: &mut Context, name: &str, font: &Font, max_width: f64) -> Vec<String> {
	let mut width = |text: &str| context.bounding_box(text, font).0;
	if width(name) <= max_width {
		return vec![name.to_owned()];
	}
	let mut lines = Vec::new();
	let mut current = String::new();
	for word in name.split(' ') {
		let test_line = match current.is_empty() {
			true => word.to_owned(),
			false => format!("{current} {word}"),
		};
		if width(&test_line) <= max_width {
			current = test_line;
			continue;
		}
		if !current.is_empty() {
			lines.push(std::mem::take(&mut current));
		}
		current = word.to_owned();
		if width(word) > max_width {
			let mut broken = String::new();
			for character in word.chars() {
				broken.push(character);
				if width(&format!("{broken}-")) > max_width {
					broken.pop();
					lines.push(format!("{broken}-"));
					broken = character.to_string();
				}
			}
			current = broken;
		}
	}
	if !current.is_empty() {
		lines.push(current);
	}
	lines
}

/// `/<br\s*\/?>/gi` replaced by `replacement`.
fn replace_breaks(text: &str, replacement: &str) -> String {
	super::common::split_breaks(text).join(replacement)
}

/// Writes a coordinate; browsers ignore `NaN` and infinite values, which leaves 0.
fn coordinate(value: f64) -> f64 {
	if value.is_finite() { value } else { 0.0 }
}

struct Paint {
	text_color: String,
	font: Font,
	face_color: String,
	/// `fillType0` … `fillType7`, if the theme has `fillType0`.
	fill_types: Option<Vec<Option<String>>>,
	/// `actor0` … `actor5`.
	actors: Vec<Option<String>>,
}

impl Paint {
	fn new(context: &Context) -> Self {
		let theme = &context.theme;
		let truthy = |key: &str| theme.text(key).filter(|value| !value.is_empty());
		let fill_types = truthy("fillType0")
			.map(|_| (0..8).map(|index| truthy(&format!("fillType{index}"))).collect());
		Paint {
			text_color: theme.css("textColor"),
			font: Font::new(context.font_family(), context.font_size()),
			face_color: truthy("faceColor").unwrap_or_else(|| "#FFF8DC".to_owned()),
			fill_types,
			actors: (0..6).map(|index| truthy(&format!("actor{index}"))).collect(),
		}
	}

	/// The rule `.task-type-N, .section-type-N` for color `number`.
	fn class_fill(&self, number: usize) -> Option<String> {
		self.fill_types.as_ref()?.get(number)?.clone()
	}

	/// The fill of sections and tasks of color `number` with the `fill` attribute.
	fn fill(&self, number: usize, fill: &str) -> String {
		self.class_fill(number).unwrap_or_else(|| fill.to_owned())
	}

	/// The fill of the circle of actor `position`: `.actor-N`.
	fn actor(&self, position: usize) -> String {
		match self.actors.get(position) {
			Some(Some(color)) => color.clone(),
			_ => ACTOR_COLOURS[position % ACTOR_COLOURS.len()].to_owned(),
		}
	}
}

/// A box with its text.
struct TextBox<'a> {
	text: &'a str,
	x: f64,
	y: f64,
	width: f64,
	height: f64,
	/// The fill of the section rule, which reaches the text of sections in the SVG placements.
	class_fill: Option<String>,
	/// The `fill` attribute of the `tspan` placement.
	colour: &'a str,
}

/// Draws the text of a box with the configured placement (`_drawTextCandidateFunc`).
fn write_box_text(
	svg: &mut Svg,
	context: &mut Context,
	settings: &Settings,
	paint: &Paint,
	text_box: &TextBox,
	clip_count: &mut usize,
) {
	let center = text_box.x + text_box.width / 2.0;
	match settings.text_placement {
		TextPlacement::ForeignObject => {
			write_html_text(svg, context, paint, text_box, clip_count);
		}
		TextPlacement::Tspan => {
			let font = &settings.task_font;
			let lines = super::common::split_breaks(text_box.text);
			let count = lines.len() as f64;
			for (index, line) in lines.iter().enumerate() {
				let dy = index as f64 * font.size - font.size * (count - 1.0) / 2.0;
				let run = TextRun {
					x: center,
					y: text_box.y + text_box.height / 2.0 + dy,
					anchor: Anchor::Middle,
					baseline: Baseline::Central,
					font,
					content: line,
				};
				let fill = text_box.class_fill.as_deref().unwrap_or(text_box.colour);
				svg.text(run, |svg| _ = svg.attr_if("fill", fill));
			}
		}
		TextPlacement::Old => {
			let run = TextRun {
				x: center,
				y: text_box.y + text_box.height / 2.0 + 5.0,
				anchor: Anchor::Middle,
				baseline: Baseline::Alphabetic,
				font: &paint.font,
				content: text_box.text,
			};
			let fill = text_box.class_fill.as_deref().unwrap_or_default();
			svg.text(run, |svg| _ = svg.attr_if("fill", fill));
		}
	}
}

/// The `<div>` of `byFo`: a table cell filling the box, centered both ways. Text wraps at
/// spaces, a wider word widens the cell, and the `<foreignObject>` clips what overflows.
fn write_html_text(
	svg: &mut Svg,
	context: &mut Context,
	paint: &Paint,
	text_box: &TextBox,
	clip_count: &mut usize,
) {
	let font = &paint.font;
	let text = collapse_whitespace(text_box.text);
	if text.is_empty() {
		return;
	}
	let natural_width = context.measure_exact(&text, font);
	let (lines, cell_width) = match natural_width <= text_box.width {
		true => (vec![text], text_box.width),
		false => {
			let runs = [Run { text, style: Default::default() }];
			let (lines, widest) = wrap_normal(context, font, &runs, text_box.width);
			let lines = lines
				.into_iter()
				.map(|line| line.runs.into_iter().map(|(run, _)| run.text).collect::<String>())
				.collect();
			(lines, text_box.width.max(widest))
		}
	};
	let line_height = context.normal_line_height(font);
	let (ascent, descent) = context.vertical_metrics(font);
	let (ascent, descent) = (ascent.round(), descent.round());
	let content_height = line_height * lines.len() as f64;
	let top = text_box.y + ((text_box.height - content_height) / 2.0).max(0.0);
	let is_clipped = content_height > text_box.height || cell_width > text_box.width;
	if is_clipped {
		*clip_count += 1;
		let id = format!("{}-text-clip-{clip_count}", context.id);
		svg.open("clipPath").attr("id", &id).end();
		svg.open("rect")
			.number_attr("x", text_box.x)
			.number_attr("y", text_box.y)
			.number_attr("width", text_box.width)
			.number_attr("height", text_box.height)
			.close();
		svg.close_tag("clipPath");
		svg.open("g").attr("clip-path", &format!("url(#{id})")).end();
	}
	let baseline_offset = ((line_height - ascent - descent) / 2.0).floor() + ascent;
	for (index, line) in lines.iter().enumerate() {
		let run = TextRun {
			x: text_box.x + cell_width / 2.0,
			y: top + index as f64 * line_height + baseline_offset,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font,
			content: line,
		};
		svg.text(run, |svg| _ = svg.attr_if("fill", &paint.text_color));
	}
	if is_clipped {
		svg.close_tag("g");
	}
}

/// The extent of the drawing (`bounds`).
#[derive(Default)]
struct Bounds {
	start_x: Option<f64>,
	start_y: Option<f64>,
	stop_x: Option<f64>,
	stop_y: Option<f64>,
}

impl Bounds {
	fn insert(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
		let update = |slot: &mut Option<f64>, value: f64, pick: fn(f64, f64) -> f64| {
			*slot = Some(slot.map_or(value, |old| pick(value, old)));
		};
		update(&mut self.start_x, x1.min(x2), f64::min);
		update(&mut self.start_y, y1.min(y2), f64::min);
		update(&mut self.stop_x, x1.max(x2), f64::max);
		update(&mut self.stop_y, y1.max(y2), f64::max);
	}
}

fn write_face(svg: &mut Svg, center_x: f64, score: f64, face_color: &str) {
	let center_y = 300.0 + (5.0 - score) * 30.0;
	svg.open("circle")
		.number_attr("cx", center_x)
		.number_attr("cy", coordinate(center_y))
		.attr("class", "face")
		.number_attr("r", FACE_RADIUS)
		.attr("stroke-width", "2")
		.attr("overflow", "visible")
		.attr("fill", face_color)
		.attr("stroke", "#999")
		.close();
	svg.open("g").end();
	for eye_x in [center_x - FACE_RADIUS / 3.0, center_x + FACE_RADIUS / 3.0] {
		svg.open("circle")
			.number_attr("cx", eye_x)
			.number_attr("cy", coordinate(center_y - FACE_RADIUS / 3.0))
			.attr("r", "1.5")
			.attr("stroke-width", "2")
			.attr("fill", "#666")
			.attr("stroke", "#666")
			.close();
	}
	let mouth = match score {
		score if score > 3.0 => Some((SMILE, 2.0)),
		score if score < 3.0 => Some((FROWN, 7.0)),
		_ => None,
	};
	match mouth {
		Some((path, offset)) => {
			svg.open("path").attr("class", "mouth").attr("d", path);
			if (center_y + offset).is_finite() {
				let transform =
					format!("translate({},{})", number(center_x), number(center_y + offset));
				svg.attr("transform", &transform);
			}
			svg.attr("stroke", "#666").close();
		}
		None => {
			let mouth_y = coordinate(center_y + 7.0);
			svg.open("line")
				.attr("class", "mouth")
				.number_attr("x1", center_x - 5.0)
				.number_attr("y1", mouth_y)
				.number_attr("x2", center_x + 5.0)
				.number_attr("y2", mouth_y)
				.attr("stroke-width", "1px")
				.attr("stroke", "#666")
				.close();
		}
	}
	svg.close_tag("g");
}

fn write_actor_circle(svg: &mut Svg, x: f64, y: f64, fill: &str, title: Option<&str>) {
	svg.open("circle")
		.number_attr("cx", x)
		.number_attr("cy", y)
		.attr("fill", fill)
		.attr("stroke", "#000")
		.attr("r", "7");
	match title {
		Some(title) => _ = svg.end().open("title").text_element("title", title).close_tag("circle"),
		None => _ = svg.close(),
	}
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let journey = parse(code, context)?;
	let settings = settings(context);
	let paint = Paint::new(context);
	let mut body = Svg::new();
	let arrowhead = format!("{}-arrowhead", context.id);
	body.open("defs").end();
	body.open("marker")
		.attr("id", &arrowhead)
		.attr("refX", "5")
		.attr("refY", "2")
		.attr("markerWidth", "6")
		.attr("markerHeight", "4")
		.attr("orient", "auto")
		.end();
	body.open("path").attr("d", "M 0,0 V 4 L6,2 Z").close();
	body.close_tag("marker").close_tag("defs");

	// The legend (`drawActorLegend`).
	let actors = journey.actors();
	let mut max_width: f64 = 0.0;
	let mut legend_y = 60.0;
	for (position, actor) in actors.iter().enumerate() {
		write_actor_circle(&mut body, 20.0, legend_y, &paint.actor(position), None);
		let lines = legend_lines(context, actor, &paint.font, settings.max_label_width);
		for (index, line) in lines.iter().enumerate() {
			let line = replace_breaks(line, " ");
			let run = TextRun {
				x: 40.0 + settings.box_text_margin * 2.0,
				y: legend_y + 7.0 + index as f64 * 20.0,
				anchor: Anchor::Start,
				baseline: Baseline::Alphabetic,
				font: &paint.font,
				content: &line,
			};
			body.text(run, |svg| _ = svg.attr_if("fill", &paint.text_color));
			let line_width = context.bounding_box(&line, &paint.font).0;
			if line_width > max_width && line_width > settings.left_margin - line_width {
				max_width = line_width;
			}
		}
		legend_y += f64::max(20.0, lines.len() as f64 * 20.0);
	}
	let left_margin = settings.left_margin + max_width;
	let mut bounds = Bounds::default();
	bounds.insert(0.0, 0.0, left_margin, actors.len() as f64 * 50.0);

	// The sections and tasks (`drawTasks`).
	let task_y = settings.height * 2.0 + settings.margin_y;
	let mut last_section = "";
	let mut section_number = 0;
	let mut fill = "#CCC";
	let mut colour = "black";
	let mut color_number = 0;
	let mut clip_count = 0;
	for (index, task) in journey.tasks.iter().enumerate() {
		let x = index as f64 * (settings.task_margin + settings.width) + left_margin;
		if task.section != last_section {
			color_number = section_number % SECTION_FILLS.len();
			fill = SECTION_FILLS[color_number];
			colour = SECTION_COLOUR;
			let task_count = journey.tasks[index..]
				.iter()
				.take_while(|other| other.section == task.section)
				.count() as f64;
			let width = settings.width * task_count + settings.margin_x * (task_count - 1.0);
			let section_fill = paint.fill(color_number, fill);
			body.open("g").end();
			body.open("rect")
				.number_attr("x", x)
				.attr("y", "50")
				.attr("fill", &section_fill)
				.attr("stroke", "#666")
				.number_attr("width", width)
				.number_attr("height", settings.height)
				.attr("rx", "3")
				.attr("ry", "3")
				.attr("class", &format!("journey-section section-type-{color_number}"))
				.close();
			let text_box = TextBox {
				text: &task.section,
				x,
				y: 50.0,
				width,
				height: settings.height,
				class_fill: paint.class_fill(color_number),
				colour,
			};
			write_box_text(&mut body, context, &settings, &paint, &text_box, &mut clip_count);
			body.close_tag("g");
			last_section = &task.section;
			section_number += 1;
		}

		let center = x + settings.width / 2.0;
		body.open("g").end();
		body.open("line")
			.attr("id", &format!("{}-task{index}", context.id))
			.number_attr("x1", center)
			.number_attr("y1", task_y)
			.number_attr("x2", center)
			.number_attr("y2", TASK_LINE_END)
			.attr("class", "task-line")
			.attr("stroke-width", "1px")
			.attr("stroke-dasharray", "4 2")
			.attr_if("stroke", &paint.text_color)
			.close();
		write_face(&mut body, center, task.score, &paint.face_color);
		body.open("rect")
			.number_attr("x", x)
			.number_attr("y", task_y)
			.attr("fill", &paint.fill(color_number, fill))
			.attr("stroke", "#666")
			.number_attr("width", settings.width)
			.number_attr("height", settings.height)
			.attr("rx", "3")
			.attr("ry", "3")
			.attr("class", &format!("task task-type-{color_number}"))
			.close();
		for (offset, person) in task.people.iter().enumerate() {
			let position =
				actors.binary_search_by(|actor| actor.encode_utf16().cmp(person.encode_utf16()));
			let fill = position.map(|position| paint.actor(position)).unwrap_or_default();
			let circle_x = x + 14.0 + offset as f64 * 10.0;
			write_actor_circle(&mut body, circle_x, task_y, &fill, Some(person));
		}
		let text_box = TextBox {
			text: &task.name,
			x,
			y: task_y,
			width: settings.width,
			height: settings.height,
			class_fill: None,
			colour,
		};
		write_box_text(&mut body, context, &settings, &paint, &text_box, &mut clip_count);
		body.close_tag("g");
		bounds.insert(x, task_y, x + settings.margin_x + settings.task_margin, TASK_LINE_END);
	}

	let title = journey.title.as_deref().filter(|title| !title.is_empty());
	if let Some(title) = title {
		let size = css_font_size(context, &settings.title_font_size, &paint.font);
		let font = Font {
			weight: 700,
			..Font::new(settings.title_font_family.clone(), size.unwrap_or(paint.font.size))
		};
		let run = TextRun {
			x: left_margin,
			y: 25.0,
			anchor: Anchor::Start,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: title,
		};
		body.text(run, |svg| _ = svg.attr_if("fill", &settings.title_color));
	}

	let start_x = bounds.start_x.unwrap_or_default();
	let height = bounds.stop_y.unwrap_or_default() - bounds.start_y.unwrap_or_default()
		+ 2.0 * settings.margin_y;
	let width = left_margin + bounds.stop_x.unwrap_or_default() + 2.0 * settings.margin_x;
	let activity_y = settings.height * 4.0;
	body.open("line")
		.number_attr("x1", left_margin)
		.number_attr("y1", activity_y)
		.number_attr("x2", width - left_margin - 4.0)
		.number_attr("y2", activity_y)
		.attr("stroke-width", "4")
		.attr("stroke", if paint.text_color.is_empty() { "black" } else { &paint.text_color })
		.attr("marker-end", &format!("url(#{arrowhead})"))
		.close();
	let extra_height = if title.is_some() { 70.0 } else { 0.0 };
	let frame = Frame {
		x: start_x,
		y: -25.0,
		width,
		height: height + extra_height,
		use_max_width: settings.use_max_width,
	};
	Ok(document(context, "journey", &frame, &journey.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn journey(code: &str) -> Result<Journey> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_sections_and_tasks() {
		let journey = journey(
			"journey\n  title My day\n  accTitle: A\n  accDescr { D }\n  early: 4\n  section Go to work\n    Make tea: 5: Me\n    Do work: 1: Me, Cat %% x\n    Odd: abc: A: B\n  Section Home # c\n    Rest:0x10:\n",
		)
		.unwrap();
		assert_eq!(journey.title.as_deref(), Some("My day"));
		assert_eq!(journey.accessibility.title, "A");
		assert_eq!(journey.accessibility.description, "D");
		let summary: Vec<_> = journey
			.tasks
			.iter()
			.map(|task| {
				(task.section.as_str(), task.name.as_str(), task.score, task.people.clone())
			})
			.collect();
		assert_eq!(summary[0], ("", "early", 4.0, Vec::new()));
		assert_eq!(summary[1], ("Go to work", "Make tea", 5.0, vec!["Me".to_owned()]));
		assert_eq!(summary[2].3, ["Me", "Cat %% x"]);
		assert!(summary[3].2.is_nan());
		assert_eq!(summary[3].3, ["A"]);
		assert_eq!(summary[4], ("Home ", "Rest", 16.0, vec![String::new()]));
		assert_eq!(journey.actors(), ["", "A", "Cat %% x", "Me"]);
	}

	#[test]
	fn lexes_keywords_like_jison() {
		// `title\s[^#\n;]+` takes the whole line, colons included.
		let parsed = journey("journey\n  Title bar: 5: Me\n  titles: 3\n").unwrap();
		assert_eq!(parsed.title.as_deref(), Some("bar: 5: Me"));
		assert_eq!(parsed.tasks[0].name, "titles");
		let error = journey("journey\n  section S\n    Journey to work: 5: Me\n").unwrap_err();
		assert_eq!(error.line, Some(3));
		assert!(journey("journey\n  section S: x\n").is_err());
		assert!(journey("journey\n  a: 5: Me; b: 3\n").is_err());
		assert!(journey("journey\n  empty:\n").is_err());
		assert!(journey("journey\n  name\n").is_err());
		assert!(
			journey("\n\njourney\ntitle\nT\n")
				.is_ok_and(|journey| journey.title.as_deref() == Some("T"))
		);
		assert!(journey("pie\n").is_err());
	}

	#[test]
	fn converts_scores_like_javascript() {
		assert_eq!(js_number_of_string(" 5 "), 5.0);
		assert_eq!(js_number_of_string(""), 0.0);
		assert_eq!(js_number_of_string("1e2"), 100.0);
		assert_eq!(js_number_of_string("-Infinity"), f64::NEG_INFINITY);
		assert!(js_number_of_string("inf").is_nan());
		assert!(js_number_of_string("5a").is_nan());
		assert!(js_number_of_string("0x").is_nan());
	}

	#[test]
	fn wraps_legend_labels() {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		// 6 pixels per character at 10 px.
		let font = Font::new("sans-serif", 10.0);
		assert_eq!(legend_lines(&mut context, "Me", &font, 30.0), ["Me"]);
		assert_eq!(legend_lines(&mut context, "ab cd ef", &font, 30.0), ["ab cd", "ef"]);
		assert_eq!(legend_lines(&mut context, "abcdefgh", &font, 30.0), ["abcd-", "efgh"]);
	}

	#[test]
	fn resolves_title_font_sizes() {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let font = Font::new("sans-serif", 16.0);
		assert_eq!(css_font_size(&mut context, "4ex", &font), Some(32.0));
		assert_eq!(css_font_size(&mut context, "5rem", &font), Some(80.0));
		assert_eq!(css_font_size(&mut context, "20", &font), Some(20.0));
		assert_eq!(css_font_size(&mut context, "1.5em", &font), Some(24.0));
		assert_eq!(css_font_size(&mut context, "big", &font), None);
	}
}
