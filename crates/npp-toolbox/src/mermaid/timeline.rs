//! Timelines (`timeline.jison`, `timelineDb.js`, `timelineRenderer.ts`,
//! `timelineRendererVertical.ts`, `svgDraw.js`, `styles.js`).

use super::color;
use super::jison::{keyword, keyword_and_delimiter, line_length, span, starts_with_ignoring_case};
use super::svg::{Accessibility, Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Look, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Direction {
	/// Periods side by side, events below them.
	#[default]
	LeftRight,
	/// Periods below each other, events right of the axis.
	TopDown,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
	NewLine,
	Header(Direction),
	Title(String),
	AccTitle(String),
	AccDescription(String),
	Section(String),
	Period(String),
	Event(String),
	Invalid(char),
	End,
}

/// `keyword\s<value>+`: the length of the match and of the keyword with its whitespace
/// character, which `substr` removes.
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

/// `"timeline"[ \t]+<direction>` with the word boundary jison adds.
fn header_with_direction(text: &str, direction: &str) -> Option<usize> {
	if !starts_with_ignoring_case(text, "timeline") {
		return None;
	}
	let blanks = span(&text["timeline".len()..], |character| matches!(character, ' ' | '\t'));
	let start = "timeline".len() + blanks;
	(blanks > 0 && keyword(&text[start..], direction)).then_some(start + direction.len())
}

/// `":"\s(?:[^:\n]|":"(?!\s))+`: an event, whose colon needs whitespace after it.
fn event_length(text: &str) -> Option<usize> {
	let space =
		text.strip_prefix(':')?.chars().next().filter(|character| character.is_whitespace())?;
	let start = 1 + space.len_utf8();
	let mut end = start;
	let mut characters = text[start..].chars().peekable();
	while let Some(character) = characters.next() {
		let is_separator = match character {
			'\n' => true,
			':' => characters.peek().is_some_and(|next| next.is_whitespace()),
			_ => false,
		};
		if is_separator {
			break;
		}
		end += character.len_utf8();
	}
	(end > start).then_some(end)
}

/// `[^#:\n]`
fn is_period_character(character: char) -> bool {
	!matches!(character, '#' | ':' | '\n')
}

/// The tokens of `timeline.jison` with the line each ends on. Like jison, the first rule that
/// matches wins.
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
		} else if let Some(length) = header_with_direction(rest, "LR") {
			token = Some(Token::Header(Direction::LeftRight));
			length
		} else if let Some(length) = header_with_direction(rest, "TD") {
			token = Some(Token::Header(Direction::TopDown));
			length
		} else if keyword(rest, "timeline") {
			token = Some(Token::Header(Direction::LeftRight));
			"timeline".len()
		} else if let Some((length, prefix)) =
			keyword_statement(rest, "title", |character| character != '\n')
		{
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
		} else if let Some((length, prefix)) =
			keyword_statement(rest, "section", |character| !matches!(character, ':' | '\n'))
		{
			token = Some(Token::Section(rest[prefix..length].to_owned()));
			length
		} else if let Some(length) = event_length(rest) {
			token = Some(Token::Event(rest[2..length].to_owned()));
			length
		} else if is_period_character(first) {
			let length = span(rest, is_period_character);
			token = Some(Token::Period(rest[..length].to_owned()));
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
	period: String,
	events: Vec<String>,
}

#[derive(Debug, Default)]
struct Timeline {
	direction: Direction,
	title: Option<String>,
	accessibility: Accessibility,
	sections: Vec<String>,
	tasks: Vec<Task>,
}

fn describe(token: &Token) -> String {
	match token {
		Token::NewLine => "end of line".to_owned(),
		Token::End => "end of input".to_owned(),
		Token::Header(_) => "`timeline`".to_owned(),
		Token::Title(text) => format!("{:?}", format!("title {text}")),
		Token::AccTitle(_) => "`accTitle`".to_owned(),
		Token::AccDescription(_) => "`accDescr`".to_owned(),
		Token::Section(text) => format!("{:?}", format!("section {text}")),
		Token::Period(text) | Token::Event(text) => format!("{:?}", text.trim()),
		Token::Invalid(character) => format!("{character:?}"),
	}
}

fn parse(code: &str, context: &Context) -> Result<Timeline> {
	let mut timeline = Timeline { title: context.title.clone(), ..Timeline::default() };
	let mut tokens = lex(code).into_iter().skip_while(|(token, _)| *token == Token::NewLine);
	let error = |(token, line): &(Token, usize), expected: &str| {
		context.error_at(*line, format!("expected {expected}, found {}", describe(token)))
	};
	match tokens.next() {
		Some((Token::Header(direction), _)) => timeline.direction = direction,
		Some(token) => return Err(error(&token, "`timeline`")),
		None => return Err(context.error_at(1, "expected `timeline`")),
	}
	let mut section = String::new();
	for token in tokens {
		match token.0 {
			Token::NewLine => {}
			Token::End => break,
			Token::Title(title) => timeline.title = Some(title),
			Token::AccTitle(title) => timeline.accessibility.title = title,
			Token::AccDescription(description) => {
				timeline.accessibility.description = description;
			}
			Token::Section(name) => {
				section.clone_from(&name);
				timeline.sections.push(name);
			}
			Token::Period(period) => {
				timeline.tasks.push(Task { section: section.clone(), period, events: Vec::new() });
			}
			Token::Event(event) => match timeline.tasks.last_mut() {
				Some(task) => task.events.push(event),
				// Mermaid fails with a TypeError.
				None => {
					return Err(context.error_at(token.1, "an event needs a time period before it"));
				}
			},
			_ => return Err(error(&token, "a section, a time period, an event or the end")),
		}
	}
	Ok(timeline)
}

/// JavaScript's `text.split(/(\s+|<br>)/)`: words and the separators between them.
fn split_words(text: &str) -> Vec<&str> {
	let mut pieces = Vec::new();
	let mut word_start = 0;
	let mut position = 0;
	while position < text.len() {
		let rest = &text[position..];
		let separator = match rest.starts_with("<br>") {
			true => 4,
			false => span(rest, char::is_whitespace),
		};
		if separator == 0 {
			position += rest.chars().next().map_or(1, char::len_utf8);
			continue;
		}
		pieces.push(&text[word_start..position]);
		pieces.push(&rest[..separator]);
		position += separator;
		word_start = position;
	}
	pieces.push(&text[word_start..]);
	pieces
}

/// A node's text after `wrap`, in the coordinates of its `<text>`.
struct WrappedText {
	/// The lines that have characters, with the `y` of their middle baseline.
	lines: Vec<(String, f64)>,
	/// Top and bottom of the bounding box; `None` without characters.
	vertical: Option<(f64, f64)>,
	/// The widest line's bounding box width.
	width: f64,
}

/// `wrap`: a `<tspan>` per line, starting a new one when the text gets wider than `width` or at
/// `<br>`. A line that overflows with its first word leaves an empty `<tspan>` behind, whose
/// `dy` browsers drop with it.
fn wrap(context: &mut Context, font: &Font, text: &str, width: f64) -> WrappedText {
	let mut tspans = vec![String::new()];
	let mut line: Vec<&str> = Vec::new();
	let join = |line: &[&str]| line.join(" ").trim().to_owned();
	for word in split_words(text) {
		line.push(word);
		let current = join(&line);
		let is_too_wide = context.measure(&current, font).0 > width;
		if is_too_wide || word == "<br>" {
			line.pop();
			if let Some(last) = tspans.last_mut() {
				*last = join(&line);
			}
			line = vec![if word == "<br>" { "" } else { word }];
			tspans.push(word.to_owned());
		} else if let Some(last) = tspans.last_mut() {
			*last = current;
		}
	}
	let (ascent, _) = context.vertical_metrics(font);
	let half_x_height = context.x_height(font) / 2.0;
	let mut wrapped = WrappedText { lines: Vec::new(), vertical: None, width: 0.0 };
	let mut y = 0.0;
	for (index, tspan) in tspans.into_iter().enumerate() {
		if tspan.split_whitespace().next().is_none() {
			continue;
		}
		y += if index == 0 { font.size } else { font.size * 1.1 };
		let (box_width, box_height) = context.bounding_box(&tspan, font);
		let top = y + half_x_height - ascent.round();
		wrapped.vertical = Some(match wrapped.vertical {
			Some((first_top, bottom)) => (first_top.min(top), f64::max(bottom, top + box_height)),
			None => (top, top + box_height),
		});
		wrapped.width = wrapped.width.max(box_width);
		wrapped.lines.push((tspan, y));
	}
	wrapped
}

impl WrappedText {
	fn height(&self) -> f64 {
		self.vertical.map_or(0.0, |(top, bottom)| bottom - top)
	}
}

/// The extent of the drawing, like `getBBox` of the root.
#[derive(Clone, Copy, Debug, Default)]
struct BBox {
	extent: Option<(f64, f64, f64, f64)>,
}

impl BBox {
	fn include(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
		let (x1, x2) = (x1.min(x2), x1.max(x2));
		let (y1, y2) = (y1.min(y2), y1.max(y2));
		self.extent = Some(match self.extent {
			Some((left, top, right, bottom)) => {
				(left.min(x1), top.min(y1), f64::max(right, x2), f64::max(bottom, y2))
			}
			None => (x1, y1, x2, y2),
		});
	}

	fn x(&self) -> f64 {
		self.extent.map_or(0.0, |extent| extent.0)
	}

	fn y(&self) -> f64 {
		self.extent.map_or(0.0, |extent| extent.1)
	}

	fn width(&self) -> f64 {
		self.extent.map_or(0.0, |extent| extent.2 - extent.0)
	}

	fn height(&self) -> f64 {
		self.extent.map_or(0.0, |extent| extent.3 - extent.1)
	}
}

/// The style rules of a node (`.section-N` and `.eventWrapper`) as presentation attributes.
#[derive(Clone, Debug, Default)]
struct NodePaint {
	fill: Option<String>,
	stroke: Option<String>,
	stroke_width: Option<String>,
	filter: Option<String>,
	line_stroke: Option<String>,
	line_width: Option<String>,
	text_fill: Option<String>,
	font_weight: Option<u16>,
}

/// `filter: brightness(120%)`: the channels of 8 bit colors times 1.2.
fn brighten(color: &str) -> String {
	let Some(mut channels) = color::Channels::parse(color) else {
		return color.to_owned();
	};
	let [red, green, blue, alpha] = channels.rgba();
	let bright = |channel: f64| (channel.round() * 1.2).round().min(255.0);
	color::rgba_of_numbers(bright(red), bright(green), bright(blue), alpha)
}

impl NodePaint {
	fn brightened(&self, bright_gradient: &str) -> NodePaint {
		let paint = |color: &Option<String>| {
			color.as_ref().map(|color| match color.starts_with("url(") {
				true => bright_gradient.to_owned(),
				false => brighten(color),
			})
		};
		NodePaint {
			fill: paint(&self.fill),
			stroke: paint(&self.stroke),
			line_stroke: paint(&self.line_stroke),
			text_fill: paint(&self.text_fill),
			..self.clone()
		}
	}
}

struct Paint {
	font: Font,
	/// `THEME_COLOR_LIMIT`, by which section numbers wrap.
	max_sections: Option<f64>,
	/// The number of `.section-N` rules.
	slot_count: usize,
	is_redux: bool,
	is_dark_theme: bool,
	/// `borderColorArray` of the color themes.
	palette: Vec<String>,
	/// The gradient strokes of the neo look; the URL is `none` where the gradient is missing.
	gradient: Option<String>,
	/// The neo look's drop shadow of the redux themes.
	shadow: Option<String>,
	is_neo: bool,
	connector_stroke: String,
	connector_width: Option<String>,
	nodes: Vec<NodePaint>,
}

/// `colorSlotCount`
fn color_slot_count(limit: Option<&Value>) -> usize {
	match limit.and_then(Value::as_number) {
		Some(limit) if limit.fract() == 0.0 && (1.0..=64.0).contains(&limit) => limit as usize,
		_ => 12,
	}
}

impl Paint {
	fn new(context: &Context, has_gradient: bool) -> Paint {
		let theme = &context.theme;
		let truthy = |key: &str| theme.text(key).filter(|value| !value.is_empty());
		let theme_name = context.theme_name.as_str();
		let is_redux = theme_name.contains("redux");
		let is_neo = context.look == Look::Neo;
		let limit = theme.get("THEME_COLOR_LIMIT");
		let max_sections = match limit {
			Some(limit) => limit.as_number().filter(|limit| *limit > 0.0),
			None => Some(12.0),
		};
		let slot_count = color_slot_count(limit);
		let palette: Vec<String> = match theme.get("borderColorArray") {
			Some(Value::Array(items))
				if matches!(theme_name, "redux-color" | "redux-dark-color") =>
			{
				items.iter().filter_map(Value::to_text).collect()
			}
			_ => Vec::new(),
		};
		let uses_gradient = matches!(theme.get("useGradient"), Some(Value::Bool(true)))
			&& theme_name != "neutral"
			&& limit.is_some_and(|limit| !matches!(limit, Value::Number(0.0) | Value::Null));
		let gradient = (is_neo && uses_gradient).then(|| match has_gradient {
			true => format!("url(#{}-gradient)", context.id),
			false => "none".to_owned(),
		});
		let font_weight = truthy("fontWeight").and_then(|weight| weight.trim().parse().ok());
		let mut paint = Paint {
			font: Font::new(context.font_family(), context.font_size()),
			max_sections,
			slot_count,
			is_redux,
			is_dark_theme: theme_name.contains("dark"),
			palette,
			gradient,
			shadow: (is_neo && is_redux).then(|| format!("url(#{}-drop-shadow)", context.id)),
			is_neo,
			connector_stroke: String::new(),
			connector_width: None,
			nodes: Vec::new(),
		};
		let stroke_width = truthy("strokeWidth");
		match is_redux {
			true => {
				paint.connector_stroke = truthy("nodeBorder").unwrap_or_else(|| "black".to_owned());
				paint.connector_width = stroke_width.clone();
			}
			false => {
				paint.connector_stroke = truthy(&format!("cScaleLabel{}", slot_count - 1))
					.unwrap_or_else(|| "black".to_owned());
			}
		}
		paint.nodes = (0..slot_count)
			.map(|slot| paint.slot(context, slot, stroke_width.as_deref(), font_weight))
			.collect();
		paint
	}

	fn slot(
		&self,
		context: &Context,
		slot: usize,
		stroke_width: Option<&str>,
		font_weight: Option<u16>,
	) -> NodePaint {
		let theme = &context.theme;
		let truthy = |key: String| theme.text(&key).filter(|value| !value.is_empty());
		let mut node = match self.is_redux {
			true => {
				let main_bkg = truthy("mainBkg".to_owned());
				let color = self.palette.get(slot % self.palette.len().max(1)).cloned();
				let is_color_theme = !self.palette.is_empty();
				NodePaint {
					fill: match self.is_dark_theme && is_color_theme {
						true => main_bkg.clone(),
						false => color.clone().or(main_bkg),
					},
					stroke: color.or_else(|| truthy("nodeBorder".to_owned())),
					stroke_width: stroke_width.map(str::to_owned),
					filter: self.shadow.clone(),
					line_stroke: truthy(format!("cScaleInv{slot}")),
					line_width: Some("3".to_owned()),
					text_fill: truthy("nodeBorder".to_owned()),
					font_weight,
				}
			}
			false => NodePaint {
				fill: truthy(format!("cScale{slot}")),
				line_stroke: truthy(format!("cScaleInv{slot}")),
				line_width: Some("3".to_owned()),
				text_fill: truthy(format!("cScaleLabel{slot}")),
				..NodePaint::default()
			},
		};
		if let Some(gradient) = &self.gradient {
			node.fill = truthy("mainBkg".to_owned());
			node.stroke = Some(gradient.clone());
			node.stroke_width = Some("2".to_owned());
			node.line_stroke = Some(gradient.clone());
			node.line_width = Some("2".to_owned());
		}
		node
	}

	/// The rules of the node of color `number`: `.section-<number % THEME_COLOR_LIMIT - 1>`.
	fn node(&self, number: usize) -> NodePaint {
		let slot = self.max_sections.map(|limit| number as f64 % limit);
		match slot {
			Some(slot) if slot.fract() == 0.0 && (slot as usize) < self.slot_count => {
				self.nodes[slot as usize].clone()
			}
			_ => NodePaint::default(),
		}
	}
}

#[derive(Clone, Copy)]
enum Connector {
	/// The time axis.
	Axis,
	/// The dashed line from a period to its events.
	Event,
}

/// A box with its text (`drawNode`).
struct Node<'a> {
	text: &'a str,
	/// The wrapping width; the box is `2 * padding` wider.
	width: f64,
	padding: f64,
	max_height: f64,
	color: usize,
	is_event: bool,
}

struct Renderer<'a, 'b> {
	context: &'a mut Context<'b>,
	paint: Paint,
	/// The top-level `fontSize` of the configuration, which sizes the nodes.
	font_size: f64,
	bright_gradient: String,
	/// The marker at the end of connectors; the top-down layout refers to a missing one.
	arrowhead: Option<String>,
	node_count: usize,
	bbox: BBox,
}

impl Renderer<'_, '_> {
	/// `getVirtualNodeHeight`
	fn virtual_height(&mut self, text: &str, width: f64, padding: f64) -> f64 {
		let font = self.font(None);
		let wrapped = wrap(self.context, &font, text, width);
		wrapped.height() + self.font_size * 1.1 * 0.5 + padding
	}

	fn font(&self, weight: Option<u16>) -> Font {
		Font { weight: weight.unwrap_or(400), ..self.paint.font.clone() }
	}

	/// Draws a node at `(x, y)` and returns its size.
	fn draw_node(&mut self, svg: &mut Svg, node: &Node, x: f64, y: f64) -> (f64, f64) {
		let mut paint = self.paint.node(node.color);
		if node.is_event {
			paint = paint.brightened(&self.bright_gradient);
		}
		let font = self.font(paint.font_weight);
		let wrapped = wrap(self.context, &font, node.text, node.width);
		let height =
			f64::max(wrapped.height() + self.font_size * 1.1 * 0.5 + node.padding, node.max_height);
		let width = node.width + 2.0 * node.padding;
		let is_redux = self.paint.is_redux;
		let text_y = match (is_redux, node.is_event) {
			(true, true) => node.padding / 2.0 + 3.0,
			(true, false) => node.padding,
			(false, _) => node.padding / 2.0,
		};

		svg.open("g").attr("transform", &format!("translate({}, {})", number(x), number(y))).end();
		svg.open("g").attr("class", "timeline-node").end();
		let corner = 5.0;
		let d = match is_redux {
			true => format!(
				"M0 {} v{} h{} v{} H0 Z",
				number(height - corner),
				number(-(height - corner)),
				number(width),
				number(height)
			),
			false => format!(
				"M0 {} v{} q0,-5,5,-5 h{} q5,0,5,5 v{} H0 Z",
				number(height - corner),
				number(-height + 2.0 * corner),
				number(width - 2.0 * corner),
				number(height - corner)
			),
		};
		svg.open("path")
			.attr("id", &format!("{}-node-{}", self.context.id, self.node_count))
			.attr("d", &d)
			.attr_if("fill", paint.fill.as_deref().unwrap_or_default())
			.attr_if("stroke", paint.stroke.as_deref().unwrap_or_default())
			.attr_if("stroke-width", paint.stroke_width.as_deref().unwrap_or_default())
			.attr_if("filter", paint.filter.as_deref().unwrap_or_default())
			.close();
		self.node_count += 1;
		self.bbox.include(x, y, x + width, y + height);
		if !is_redux {
			svg.open("line")
				.attr("x1", "0")
				.number_attr("y1", height)
				.number_attr("x2", width)
				.number_attr("y2", height)
				.attr_if("stroke", paint.line_stroke.as_deref().unwrap_or_default())
				.attr_if("stroke-width", paint.line_width.as_deref().unwrap_or_default())
				.close();
		}
		let text_x = width / 2.0;
		if !wrapped.lines.is_empty() {
			let transform = format!("translate({}, {})", number(text_x), number(text_y));
			svg.open("g").attr("transform", &transform).end();
			for (line, line_y) in &wrapped.lines {
				let run = TextRun {
					x: 0.0,
					y: *line_y,
					anchor: Anchor::Middle,
					baseline: Baseline::Middle,
					font: &font,
					content: line,
				};
				svg.text(run, |svg| {
					_ = svg.attr_if("fill", paint.text_fill.as_deref().unwrap_or_default())
				});
			}
			svg.close_tag("g");
		}
		if let Some((top, bottom)) = wrapped.vertical {
			let center = x + text_x;
			let half_width = wrapped.width / 2.0;
			self.bbox.include(
				center - half_width,
				y + text_y + top,
				center + half_width,
				y + text_y + bottom,
			);
		}
		svg.close_tag("g").close_tag("g");
		(width, height)
	}

	fn line(&mut self, svg: &mut Svg, from: (f64, f64), to: (f64, f64), connector: Connector) {
		let (width, is_dashed) = match connector {
			Connector::Axis => ("4", false),
			Connector::Event => ("2", true),
		};
		svg.open("line")
			.number_attr("x1", from.0)
			.number_attr("y1", from.1)
			.number_attr("x2", to.0)
			.number_attr("y2", to.1)
			.attr("stroke-width", self.paint.connector_width.as_deref().unwrap_or(width))
			.attr("stroke", &self.paint.connector_stroke);
		if let Some(arrowhead) = &self.arrowhead {
			svg.attr("marker-end", &format!("url(#{arrowhead})"));
		}
		if is_dashed {
			svg.attr("stroke-dasharray", "5,5");
		}
		svg.close();
		self.bbox.include(from.0, from.1, to.0, to.1);
	}

	/// The bounding box of a bold `4ex` title with its baseline at `y`, and its font.
	fn title_font(&mut self) -> Font {
		let parent = self.paint.font.clone();
		let size = super::journey::css_font_size(self.context, "4ex", &parent);
		Font { weight: 700, ..Font::new(parent.family.clone(), size.unwrap_or(parent.size)) }
	}

	fn draw_title(&mut self, svg: &mut Svg, title: &str, x: f64) {
		let font = self.title_font();
		let run = TextRun {
			x,
			y: 20.0,
			anchor: Anchor::Start,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: title,
		};
		svg.text(run, |_| {});
		if title.split_whitespace().next().is_some() {
			let (width, height) = self.context.bounding_box(title, &font);
			let ascent = self.context.vertical_metrics(&font).0.round();
			self.bbox.include(x, 20.0 - ascent, x + width, 20.0 - ascent + height);
		}
	}
}

struct Settings {
	left_margin: f64,
	padding: f64,
	use_max_width: bool,
	is_multicolor: bool,
}

/// `getVirtualNodeHeight` gets the task itself, which shows as this text.
const TASK_OBJECT_TEXT: &str = "[object Object]";

/// `timelineRenderer.ts`: sections side by side, their periods below them, and the events of
/// each period below the time axis.
fn draw_left_right(
	renderer: &mut Renderer,
	svg: &mut Svg,
	timeline: &Timeline,
	settings: &Settings,
) {
	let mut max_section_height: f64 = 0.0;
	for section in &timeline.sections {
		let height = renderer.virtual_height(section, 150.0, 20.0);
		max_section_height = max_section_height.max(height + 20.0);
	}
	let mut max_task_height: f64 = 0.0;
	let mut max_event_line_length: f64 = 0.0;
	if !timeline.tasks.is_empty() {
		max_task_height = renderer.virtual_height(TASK_OBJECT_TEXT, 150.0, 20.0) + 20.0;
	}
	for task in &timeline.tasks {
		let mut length = 0.0;
		for event in &task.events {
			length += renderer.virtual_height(event, 150.0, 20.0);
		}
		length += task.events.len().saturating_sub(1) as f64 * 10.0;
		max_event_line_length = max_event_line_length.max(length);
	}

	let draw_tasks = |renderer: &mut Renderer,
	                  svg: &mut Svg,
	                  tasks: &[&Task],
	                  mut color: usize,
	                  mut x: f64,
	                  y: f64,
	                  is_without_sections: bool| {
		let mut max_task_height = max_task_height;
		for task in tasks {
			let node = Node {
				text: &task.period,
				width: 150.0,
				padding: 20.0,
				max_height: max_task_height,
				color,
				is_event: false,
			};
			let (_, height) = renderer.draw_node(svg, &node, x, y);
			max_task_height = max_task_height.max(height);
			let line_x = x + 190.0 / 2.0;
			let line_bottom = y + max_task_height + 100.0 + max_event_line_length + 100.0;
			renderer.line(
				svg,
				(line_x, y + max_task_height),
				(line_x, line_bottom),
				Connector::Event,
			);
			let mut event_y = y + 200.0;
			for event in &task.events {
				let node = Node {
					text: event,
					width: 150.0,
					padding: 20.0,
					max_height: 50.0,
					color,
					is_event: true,
				};
				let (_, height) = renderer.draw_node(svg, &node, x, event_y);
				event_y += 10.0 + height;
			}
			x += 200.0;
			if is_without_sections && settings.is_multicolor {
				color += 1;
			}
		}
	};

	let mut master_x = 50.0 + settings.left_margin;
	let has_sections = !timeline.sections.is_empty();
	if has_sections {
		for (number, section) in timeline.sections.iter().enumerate() {
			let tasks: Vec<&Task> =
				timeline.tasks.iter().filter(|task| task.section == *section).collect();
			let node = Node {
				text: section,
				width: 200.0 * tasks.len().max(1) as f64 - 50.0,
				padding: 20.0,
				max_height: max_section_height,
				color: number,
				is_event: false,
			};
			renderer.draw_node(svg, &node, master_x, 50.0);
			let tasks_y = 50.0 + max_section_height + 50.0;
			draw_tasks(renderer, svg, &tasks, number, master_x, tasks_y, false);
			master_x += 200.0 * tasks.len().max(1) as f64;
		}
	} else {
		let tasks: Vec<&Task> = timeline.tasks.iter().collect();
		draw_tasks(renderer, svg, &tasks, 0, master_x, 50.0, true);
	}

	let content = renderer.bbox;
	if let Some(title) = timeline.title.as_deref().filter(|title| !title.is_empty()) {
		let x = match renderer.paint.is_neo {
			true => content.x() * 2.0 + settings.left_margin,
			false => content.width() / 2.0 - settings.left_margin,
		};
		renderer.draw_title(svg, title, x);
	}
	let depth_y = match has_sections {
		true => max_section_height + max_task_height + 150.0,
		false => max_task_height + 100.0,
	};
	let axis_end = content.width() + 3.0 * settings.left_margin;
	renderer.line(svg, (settings.left_margin, depth_y), (axis_end, depth_y), Connector::Axis);
}

const NODE_WIDTH: f64 = 200.0;
const NODE_PADDING: f64 = 5.0;
const EVENT_WIDTH: f64 = NODE_WIDTH + 100.0;
const EVENT_SPACING: f64 = 10.0;
const SECTION_TASK_GAP: f64 = 20.0;
const TASK_AXIS_GAP: f64 = 20.0;
const TASK_VERTICAL_GAP: f64 = 30.0;
const EVENT_AXIS_GAP: f64 = 50.0;

/// `timelineRendererVertical.ts`: sections below each other, their periods left of the time
/// axis and the events of each period right of it.
fn draw_top_down(renderer: &mut Renderer, svg: &mut Svg, timeline: &Timeline, settings: &Settings) {
	let master_x = 50.0 + settings.left_margin;
	let left_width = NODE_WIDTH + NODE_PADDING * 2.0 + TASK_AXIS_GAP;
	let right_width = EVENT_WIDTH + NODE_PADDING * 2.0 + EVENT_AXIS_GAP;
	let axis_x = master_x + left_width;
	let section_width = f64::max(50.0, left_width + right_width - NODE_PADDING * 2.0);

	let mut max_section_height: f64 = 0.0;
	for section in &timeline.sections {
		let height = renderer.virtual_height(section, section_width, NODE_PADDING);
		max_section_height = max_section_height.max(height);
	}
	let mut max_task_height: f64 = 0.0;
	let mut max_event_stack_height: f64 = 0.0;
	if !timeline.tasks.is_empty() {
		max_task_height = renderer.virtual_height(TASK_OBJECT_TEXT, NODE_WIDTH, NODE_PADDING);
	}
	for task in &timeline.tasks {
		let mut height = 0.0;
		for event in &task.events {
			height += renderer.virtual_height(event, EVENT_WIDTH, NODE_PADDING);
		}
		height += task.events.len().saturating_sub(1) as f64 * EVENT_SPACING;
		max_event_stack_height = max_event_stack_height.max(height);
	}
	let task_spacing = max_task_height.max(max_event_stack_height) + TASK_VERTICAL_GAP;

	let draw_tasks = |renderer: &mut Renderer,
	                  svg: &mut Svg,
	                  tasks: &[&Task],
	                  mut color: usize,
	                  mut y: f64,
	                  is_without_sections: bool| {
		let mut max_task_height = max_task_height;
		for task in tasks {
			let node = Node {
				text: &task.period,
				width: NODE_WIDTH,
				padding: NODE_PADDING,
				max_height: max_task_height,
				color,
				is_event: false,
			};
			let task_x = axis_x - TASK_AXIS_GAP - (NODE_WIDTH + 2.0 * NODE_PADDING);
			let (_, height) = renderer.draw_node(svg, &node, task_x, y);
			max_task_height = max_task_height.max(height);
			let events_x = axis_x + EVENT_AXIS_GAP;
			let mut event_y = y;
			for event in &task.events {
				let node = Node {
					text: event,
					width: EVENT_WIDTH,
					padding: NODE_PADDING,
					max_height: 0.0,
					color,
					is_event: true,
				};
				let (_, height) = renderer.draw_node(svg, &node, events_x, event_y);
				let line_y = event_y + height / 2.0;
				renderer.line(svg, (axis_x, line_y), (events_x, line_y), Connector::Event);
				event_y += height + EVENT_SPACING;
			}
			y += task_spacing;
			if is_without_sections && settings.is_multicolor {
				color += 1;
			}
		}
	};

	let content_top = 50.0;
	if timeline.sections.is_empty() {
		let tasks: Vec<&Task> = timeline.tasks.iter().collect();
		draw_tasks(renderer, svg, &tasks, 0, content_top, true);
	} else {
		let mut master_y = content_top;
		for (number, section) in timeline.sections.iter().enumerate() {
			let tasks: Vec<&Task> =
				timeline.tasks.iter().filter(|task| task.section == *section).collect();
			let node = Node {
				text: section,
				width: section_width,
				padding: NODE_PADDING,
				max_height: max_section_height,
				color: number,
				is_event: false,
			};
			let (_, height) = renderer.draw_node(svg, &node, axis_x - left_width, master_y);
			let tasks_y = master_y + height + SECTION_TASK_GAP;
			draw_tasks(renderer, svg, &tasks, number, tasks_y, false);
			let count = tasks.len();
			let gaps = if count > 0 { TASK_VERTICAL_GAP * 2.0 } else { 0.0 };
			master_y += height + SECTION_TASK_GAP + task_spacing * count.max(1) as f64 - gaps;
		}
	}

	if let Some(title) = timeline.title.as_deref().filter(|title| !title.is_empty()) {
		let x = renderer.bbox.width() / 2.0 - settings.left_margin;
		renderer.draw_title(svg, title, x);
	}
	let font_size = renderer.font_size.trunc();
	let bbox = renderer.bbox;
	let mut axis = Svg::new();
	let top = content_top - font_size * 2.0;
	let bottom = bbox.y() + bbox.height() + font_size * 0.5 + 20.0;
	renderer.line(&mut axis, (axis_x, top), (axis_x, bottom), Connector::Axis);
	axis.append(std::mem::take(svg));
	*svg = axis;
}

/// `feDropShadow` written with the primitives it is defined as, for EdgeHTML.
fn write_drop_shadow(svg: &mut Svg, id: &str, is_dark: bool) {
	svg.open("filter")
		.attr("id", &format!("{id}-drop-shadow"))
		.attr("height", "130%")
		.attr("width", "130%")
		.end();
	svg.open("feOffset")
		.attr("in", "SourceAlpha")
		.attr("dx", "4")
		.attr("dy", "4")
		.attr("result", "offset")
		.close();
	svg.open("feFlood")
		.attr("flood-opacity", if is_dark { "0.2" } else { "0.06" })
		.attr("flood-color", if is_dark { "#FFFFFF" } else { "#000000" })
		.close();
	svg.open("feComposite").attr("in2", "offset").attr("operator", "in").close();
	svg.open("feMerge").end();
	svg.open("feMergeNode").close();
	svg.open("feMergeNode").attr("in", "SourceGraphic").close();
	svg.close_tag("feMerge").close_tag("filter");
}

fn write_gradient(svg: &mut Svg, id: &str, stops: [String; 2]) {
	svg.open("linearGradient")
		.attr("id", id)
		.attr("gradientUnits", "objectBoundingBox")
		.attr("x1", "0%")
		.attr("y1", "0%")
		.attr("x2", "100%")
		.attr("y2", "0%")
		.end();
	for (offset, color) in ["0%", "100%"].into_iter().zip(stops) {
		svg.open("stop")
			.attr("offset", offset)
			.attr_if("stop-color", &color)
			.attr("stop-opacity", "1")
			.close();
	}
	svg.close_tag("linearGradient");
}

/// The top-level `fontSize` as `drawNode` reads it: a number, or text with `px` removed.
fn configured_font_size(context: &Context) -> f64 {
	let size = match context.config.get("fontSize") {
		Some(Value::Number(size)) => Some(*size),
		Some(Value::String(size)) => size.replacen("px", "", 1).trim().parse().ok(),
		_ => None,
	};
	size.filter(|size: &f64| size.is_finite()).unwrap_or(16.0)
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let timeline = parse(code, context)?;
	let settings = Settings {
		left_margin: context.config_number("timeline", "leftMargin", 150.0),
		padding: context.config_number("timeline", "padding", 50.0),
		use_max_width: context.config_bool("timeline", "useMaxWidth", true),
		is_multicolor: !context.config_bool("timeline", "disableMulticolor", false),
	};
	let is_left_right = timeline.direction == Direction::LeftRight;
	let paint = Paint::new(context, is_left_right);
	let id = context.id.clone();
	let has_events = timeline.tasks.iter().any(|task| !task.events.is_empty());
	let mut svg = Svg::new();
	svg.open("defs").end();
	if is_left_right {
		svg.open("marker")
			.attr("id", &format!("{id}-arrowhead"))
			.attr("refX", "5")
			.attr("refY", "2")
			.attr("markerWidth", "6")
			.attr("markerHeight", "4")
			.attr("orient", "auto")
			.end();
		svg.open("path").attr("d", "M 0,0 V 4 L6,2 Z").close();
		svg.close_tag("marker");
	}
	if paint.shadow.is_some() {
		write_drop_shadow(&mut svg, &id, paint.is_dark_theme);
	}
	if paint.gradient.is_some() && is_left_right {
		let theme = &context.theme;
		let stops = [theme.css("gradientStart"), theme.css("gradientStop")];
		let bright = stops.clone().map(|stop| match stop.is_empty() {
			true => stop,
			false => brighten(&stop),
		});
		write_gradient(&mut svg, &format!("{id}-gradient"), stops);
		if has_events {
			write_gradient(&mut svg, &format!("{id}-gradient-bright"), bright);
		}
	}
	svg.close_tag("defs");
	let mut renderer = Renderer {
		paint,
		font_size: configured_font_size(context),
		bright_gradient: format!("url(#{id}-gradient-bright)"),
		arrowhead: is_left_right.then(|| format!("{id}-arrowhead")),
		node_count: 0,
		bbox: BBox::default(),
		context,
	};
	let mut body = Svg::new();
	match timeline.direction {
		Direction::LeftRight => draw_left_right(&mut renderer, &mut body, &timeline, &settings),
		Direction::TopDown => draw_top_down(&mut renderer, &mut body, &timeline, &settings),
	}
	let bbox = renderer.bbox;
	svg.append(body);
	let frame = Frame {
		x: bbox.x() - settings.padding,
		y: bbox.y() - settings.padding,
		width: bbox.width() + 2.0 * settings.padding,
		height: bbox.height() + 2.0 * settings.padding,
		use_max_width: settings.use_max_width,
	};
	Ok(document(context, "timeline", &frame, &timeline.accessibility, svg))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn timeline(code: &str) -> Result<Timeline> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_sections_periods_and_events() {
		let parsed = timeline(
			"timeline TD\n  title History: of things\n  accTitle: A\n  accDescr { D }\n  early : e0\n  section One\n  2002 : a : b:c # x\n       : d\n  2003 # c : e\n  Section Two\n  2004\n",
		)
		.unwrap();
		assert_eq!(parsed.direction, Direction::TopDown);
		assert_eq!(parsed.title.as_deref(), Some("History: of things"));
		assert_eq!(parsed.accessibility.title, "A");
		assert_eq!(parsed.accessibility.description, "D");
		assert_eq!(parsed.sections, ["One", "Two"]);
		let summary: Vec<_> = parsed
			.tasks
			.iter()
			.map(|task| (task.section.as_str(), task.period.as_str(), task.events.clone()))
			.collect();
		assert_eq!(summary[0], ("", "early ", vec!["e0".to_owned()]));
		assert_eq!(
			summary[1],
			("One", "2002 ", vec!["a ".to_owned(), "b:c # x".to_owned(), "d".to_owned()])
		);
		assert_eq!(summary[2], ("One", "2003 ", Vec::new()));
		assert_eq!(summary[3], ("Two", "2004", Vec::new()));
	}

	#[test]
	fn lexes_like_jison() {
		assert_eq!(timeline("timeline lr\n").unwrap().direction, Direction::LeftRight);
		assert_eq!(timeline("timeline\n").unwrap().direction, Direction::LeftRight);
		// `timeline\b`: a period may start with `timelines`, not with `timeline`.
		assert_eq!(timeline("timeline\n  timelines : t\n").unwrap().tasks[0].period, "timelines ");
		assert!(timeline("timeline\n  timeline x\n").is_err());
		// `TDx` is no direction but a period.
		assert_eq!(timeline("timeline TDx\n").unwrap().tasks[0].period, "TDx");
		let error = timeline("timeline\n  a :e1\n").unwrap_err();
		assert_eq!(error.line, Some(2));
		assert!(timeline("timeline\n  section A\n  : e1\n").is_err());
		assert!(timeline("pie\n").is_err());
		assert!(timeline("\n\ntimeline\n").is_ok());
	}

	#[test]
	fn splits_words_like_javascript() {
		assert_eq!(split_words("a  b"), ["a", "  ", "b"]);
		assert_eq!(split_words("x<br> y "), ["x", "<br>", "", " ", "y", " ", ""]);
		assert_eq!(split_words(""), [""]);
	}

	#[test]
	fn wraps_like_mermaid() {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		// 6 pixels per character at 10 px.
		let font = Font::new("sans-serif", 10.0);
		let lines = |wrapped: WrappedText| -> Vec<(String, f64)> { wrapped.lines };
		assert_eq!(
			lines(wrap(&mut context, &font, "ab cd ef", 30.0)),
			[("ab   cd".to_owned(), 10.0), ("ef".to_owned(), 21.0)]
		);
		// A first word that is too wide leaves an empty line whose `dy` is dropped.
		assert_eq!(
			lines(wrap(&mut context, &font, "abcdefgh", 30.0)),
			[("abcdefgh".to_owned(), 11.0)]
		);
		assert_eq!(
			lines(wrap(&mut context, &font, "a<br>b<br/>c", 300.0)),
			[("a".to_owned(), 10.0), ("b<br/>c".to_owned(), 21.0)]
		);
		assert!(wrap(&mut context, &font, "  ", 30.0).vertical.is_none());
	}

	#[test]
	fn brightens_like_the_css_filter() {
		assert_eq!(brighten("#808080"), "#9a9a9a");
		assert_eq!(brighten("#ffffff"), "#ffffff");
		assert_eq!(brighten("none"), "none");
	}
}
