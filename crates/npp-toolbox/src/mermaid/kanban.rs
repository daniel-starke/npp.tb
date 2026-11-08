//! Kanban boards (`kanban.jison`, `kanbanDb.ts`, `kanbanRenderer.ts`, `styles.ts`, the
//! `kanbanSection` cluster and the `kanbanItem` shape).

use super::color;
use super::label::LabelBox;
use super::rendering::{self, BBox, Styles, sketch, translate};
use super::rough;
use super::svg::{Frame, Svg, document};
use super::value::{self, Value};
use super::{Context, Diagram, Look, Result};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token {
	/// The diagram keyword.
	Kanban,
	SpaceLine,
	NewLine,
	SpaceList(usize),
	NodeId(String),
	NodeStart(String),
	NodeDescription(String),
	NodeEnd(String),
	ShapeData(String),
	Icon(String),
	Class(String),
	End,
}

/// The tokens of `kanban.jison` with the line each starts on. Like jison, the first rule that
/// matches wins. `mindmap.jison`, which kanban's grammar was copied from, has another
/// `keyword` and no `@{…}` shape data.
pub(super) fn lex(code: &str, context: &Context, keyword: &str) -> Result<Vec<(Token, usize)>> {
	let has_shape_data = keyword == "kanban";
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	let error = |line: usize, rest: &str| {
		let found: String = rest.chars().take(10).collect();
		context.error_at(line, format!("unexpected {found:?}"))
	};
	macro_rules! take {
		($length:expr) => {{
			let length = $length;
			let taken = &code[position..position + length];
			position += length;
			let start = line;
			line += taken.matches('\n').count();
			(taken, start)
		}};
	}
	while position < code.len() {
		let rest = &code[position..];
		let whitespace = rest.len() - rest.trim_start().len();
		if has_shape_data && rest.starts_with("@{") {
			let (_, start) = take!(2);
			let mut data = String::new();
			loop {
				let rest = &code[position..];
				match rest.as_bytes().first() {
					None => return Err(error(line, rest)),
					Some(b'}') => {
						take!(1);
						break;
					}
					Some(b'"') => {
						take!(1);
						data.push('"');
						let inner = &code[position..];
						let end = inner.find('"').ok_or_else(|| error(line, inner))?;
						let (text, _) = take!(end);
						data.push_str(&replace_line_breaks(text));
						take!(1);
						data.push('"');
					}
					Some(b'^') => return Err(error(line, rest)),
					Some(_) => {
						let end = rest.find(['}', '^', '"']).unwrap_or(rest.len());
						let (text, _) = take!(end);
						data.push_str(text);
					}
				}
			}
			tokens.push((Token::ShapeData(data), start));
			continue;
		}
		// `\s*\%\%.*`
		if rest[whitespace..].starts_with("%%") {
			let length =
				whitespace + rest[whitespace..].find('\n').unwrap_or(rest.len() - whitespace);
			let (_, start) = take!(length);
			tokens.push((Token::SpaceLine, start));
			continue;
		}
		if rest.get(..keyword.len()).is_some_and(|word| word.eq_ignore_ascii_case(keyword)) {
			let (_, start) = take!(keyword.len());
			tokens.push((Token::Kanban, start));
			continue;
		}
		if rest.starts_with(":::") {
			take!(3);
			let rest = &code[position..];
			let length = rest.find('\n').unwrap_or(rest.len());
			if length > 0 {
				let (class, start) = take!(length);
				tokens.push((Token::Class(class.to_owned()), start));
			} else if !rest.is_empty() {
				take!(1);
			}
			continue;
		}
		if rest.get(..7).is_some_and(|start| start.eq_ignore_ascii_case("::icon(")) {
			take!(7);
			let rest = &code[position..];
			let length = rest.find(')').unwrap_or(rest.len());
			if length > 0 {
				let (icon, start) = take!(length);
				tokens.push((Token::Icon(icon.to_owned()), start));
			}
			if code[position..].starts_with(')') {
				take!(1);
			}
			continue;
		}
		// `[\s]+[\n]`: whitespace up to its last line end.
		if whitespace > 0
			&& let Some(last_line_end) = rest[..whitespace].rfind('\n')
			&& last_line_end > 0
		{
			let (_, start) = take!(last_line_end + 1);
			tokens.push((Token::SpaceLine, start));
			continue;
		}
		if rest.starts_with('\n') {
			let length = rest.len() - rest.trim_start_matches('\n').len();
			let (_, start) = take!(length);
			tokens.push((Token::NewLine, start));
			continue;
		}
		if let Some(start_token) =
			["-)", "(-", "))", ")", "((", "{{", "(", "["].into_iter().find(|t| rest.starts_with(t))
		{
			let (text, start) = take!(start_token.len());
			tokens.push((Token::NodeStart(text.to_owned()), start));
			lex_node(code, &mut position, &mut line, &mut tokens, context)?;
			continue;
		}
		if whitespace > 0 {
			let (spaces, start) = take!(whitespace);
			tokens.push((Token::SpaceList(spaces.chars().count()), start));
			continue;
		}
		let length = rest.find(['(', '[', '\n', ')', '{', '}', '@']).unwrap_or(rest.len());
		if length == 0 {
			return Err(error(line, rest));
		}
		let (id, start) = take!(length);
		tokens.push((Token::NodeId(id.to_owned()), start));
	}
	tokens.push((Token::End, line));
	Ok(tokens)
}

/// `\n\s*` of a shape data string becomes `<br/>`.
fn replace_line_breaks(text: &str) -> String {
	let mut output = String::with_capacity(text.len());
	let mut characters = text.chars().peekable();
	while let Some(character) = characters.next() {
		if character == '\n' {
			output.push_str("<br/>");
			while characters.next_if(|c| c.is_whitespace()).is_some() {}
		} else {
			output.push(character);
		}
	}
	output
}

/// The `NODE`, `NSTR` and `NSTR2` start conditions after a node start.
fn lex_node(
	code: &str,
	position: &mut usize,
	line: &mut usize,
	tokens: &mut Vec<(Token, usize)>,
	context: &Context,
) -> Result<()> {
	let advance = |length: usize, position: &mut usize, line: &mut usize| {
		let taken = &code[*position..*position + length];
		*position += length;
		let start = *line;
		*line += taken.matches('\n').count();
		(taken.to_owned(), start)
	};
	loop {
		let rest = &code[*position..];
		if rest.is_empty() {
			return Err(context.error_at(*line, "unexpected end of the diagram in a node"));
		}
		let quoted = if rest.starts_with("\"`") {
			Some(("\"`".len(), "`\""))
		} else if rest.starts_with('"') {
			Some((1, "\""))
		} else {
			None
		};
		if let Some((open, close)) = quoted {
			advance(open, position, line);
			let inner = &code[*position..];
			let excluded: &[char] = if close == "\"" { &['"'] } else { &['`', '"'] };
			let length = inner.find(excluded).unwrap_or(inner.len());
			if length > 0 {
				let (text, start) = advance(length, position, line);
				tokens.push((Token::NodeDescription(text), start));
			}
			if !code[*position..].starts_with(close) {
				let found: String = code[*position..].chars().take(10).collect();
				return Err(context.error_at(*line, format!("unexpected {found:?}")));
			}
			advance(close.len(), position, line);
			continue;
		}
		let end =
			["))", ")", "]", "}}", "(-", "-)", "((", "("].into_iter().find(|t| rest.starts_with(t));
		if let Some(end) = end {
			let (text, start) = advance(end.len(), position, line);
			tokens.push((Token::NodeEnd(text), start));
			return Ok(());
		}
		let length = rest.find([')', ']', '(', '}']).unwrap_or(rest.len());
		// `.+(?!\(\()`: the rest of the line.
		let length = match length {
			0 => rest.find('\n').unwrap_or(rest.len()),
			length => length,
		};
		if length == 0 {
			return Err(context.error_at(*line, "unexpected line end in a node"));
		}
		let (text, start) = advance(length, position, line);
		tokens.push((Token::NodeDescription(text), start));
	}
}

#[derive(Clone, Debug, Default)]
struct KanbanNode {
	id: String,
	level: usize,
	label: String,
	parent_id: Option<String>,
	ticket: Option<String>,
	assigned: Option<String>,
	priority: Option<String>,
}

#[derive(Default)]
struct Board {
	nodes: Vec<KanbanNode>,
	/// Indices of the sections in `nodes`.
	sections: Vec<usize>,
	count: usize,
}

impl Board {
	/// `addNode`.
	fn add_node(
		&mut self,
		level: usize,
		id: &str,
		description: &str,
		shape_data: Option<&str>,
		line: usize,
		context: &Context,
	) -> Result<()> {
		let id = match id.is_empty() {
			true => {
				self.count += 1;
				format!("kbn{}", self.count - 1)
			}
			false => id.to_owned(),
		};
		let mut node =
			KanbanNode { id, level, label: description.to_owned(), ..KanbanNode::default() };
		if let Some(shape_data) = shape_data {
			let yaml = match shape_data.contains('\n') {
				true => format!("{shape_data}\n"),
				false => format!("{{\n{shape_data}\n}}"),
			};
			let document = value::parse_yaml(&yaml)
				.filter(|document| matches!(document, Value::Object(_)))
				.ok_or_else(|| context.error_at(line, "invalid shape data"))?;
			if let Some(shape) = document.get("shape").and_then(Value::as_str)
				&& (shape != shape.to_lowercase() || shape.contains('_'))
			{
				return Err(context.error_at(
					line,
					format!("No such shape: {shape}. Shape names should be lowercase."),
				));
			}
			let truthy_text = |key: &str| {
				document.get(key).filter(|value| is_truthy(value)).and_then(Value::to_text)
			};
			if let Some(label) = truthy_text("label") {
				node.label = label;
			}
			node.assigned = truthy_text("assigned");
			node.ticket = truthy_text("ticket");
			node.priority = truthy_text("priority");
		}
		// `getSection`.
		let section = match self.nodes.first() {
			None => None,
			Some(first) => {
				let section_level = first.level;
				let mut last_section = None;
				for node in self.nodes.iter().rev() {
					if node.level == section_level && last_section.is_none() {
						last_section = Some(node);
					}
					if node.level < section_level {
						return Err(context.error_at(
							line,
							format!(
								"Items without section detected, found section (\"{}\")",
								node.label
							),
						));
					}
				}
				last_section.filter(|section| section.level != level)
			}
		};
		match section {
			Some(section) => node.parent_id = Some(section.id.clone()),
			None => self.sections.push(self.nodes.len()),
		}
		self.nodes.push(node);
		Ok(())
	}
}

/// JavaScript's truthiness of a YAML value.
fn is_truthy(value: &Value) -> bool {
	match value {
		Value::Null => false,
		Value::Bool(value) => *value,
		Value::Number(number) => *number != 0.0 && !number.is_nan(),
		Value::String(text) => !text.is_empty(),
		_ => true,
	}
}

fn parse(code: &str, context: &Context) -> Result<Board> {
	let tokens = lex(code, context, "kanban")?;
	let mut index = 0;
	let token = |index: usize| &tokens[index.min(tokens.len() - 1)].0;
	let line_of = |index: usize| tokens[index.min(tokens.len() - 1)].1;
	let unexpected = |index: usize, expected: &str| {
		let found = match token(index) {
			Token::End => "the end of the diagram".to_owned(),
			Token::NewLine | Token::SpaceLine => "the end of the line".to_owned(),
			Token::SpaceList(_) => "indentation".to_owned(),
			Token::NodeId(text) | Token::NodeDescription(text) => format!("{text:?}"),
			other => format!("{other:?}"),
		};
		context.error_at(line_of(index), format!("expected {expected}, found {found}"))
	};
	// `spaceLines`, and the lines the preprocessing left of front matter and directives.
	while matches!(token(index), Token::SpaceLine | Token::NewLine) {
		index += 1;
	}
	if *token(index) != Token::Kanban {
		return Err(unexpected(index, "`kanban`"));
	}
	index += 1;
	if *token(index) == Token::NewLine {
		index += 1;
	}
	let mut board = Board::default();
	loop {
		// A statement.
		let line = line_of(index);
		let level = match token(index) {
			Token::SpaceList(level) => {
				index += 1;
				Some(*level)
			}
			_ => None,
		};
		match token(index).clone() {
			Token::SpaceLine if level.is_none() => index += 1,
			// `getData` passes neither icons nor classes on to the rendering.
			Token::Icon(_) | Token::Class(_) => index += 1,
			Token::NodeId(_) | Token::NodeStart(_) => {
				let (id, description) = match token(index).clone() {
					Token::NodeId(id) => match token(index + 1) {
						Token::NodeStart(_) => {
							let Token::NodeDescription(description) = token(index + 2).clone()
							else {
								return Err(unexpected(index + 2, "a node description"));
							};
							if !matches!(token(index + 3), Token::NodeEnd(_)) {
								return Err(unexpected(index + 3, "the end of the node"));
							}
							index += 4;
							(id, description)
						}
						_ => {
							index += 1;
							(id.clone(), id)
						}
					},
					_ => {
						let Token::NodeDescription(description) = token(index + 1).clone() else {
							return Err(unexpected(index + 1, "a node description"));
						};
						if !matches!(token(index + 2), Token::NodeEnd(_)) {
							return Err(unexpected(index + 2, "the end of the node"));
						}
						index += 3;
						(description.clone(), description)
					}
				};
				let mut shape_data: Option<String> = None;
				while let Token::ShapeData(data) = token(index) {
					shape_data.get_or_insert_with(String::new).push_str(data);
					index += 1;
				}
				board.add_node(
					level.unwrap_or(0),
					&id,
					&description,
					shape_data.as_deref(),
					line,
					context,
				)?;
			}
			// `statement: SPACELIST`.
			_ if level.is_some() => {}
			_ => return Err(unexpected(index, "a node")),
		}
		// `stop`.
		match token(index) {
			Token::NewLine | Token::SpaceLine => index += 1,
			Token::End => break,
			_ => return Err(unexpected(index, "the end of the line")),
		}
		while *token(index) == Token::NewLine {
			index += 1;
		}
		if *token(index) == Token::End {
			break;
		}
	}
	Ok(board)
}

fn priority_color(priority: &str) -> Option<&'static str> {
	match priority {
		"Very High" => Some("red"),
		"High" => Some("orange"),
		"Low" => Some("blue"),
		"Very Low" => Some("lightblue"),
		_ => None,
	}
}

/// A laid out label and the size `getBoundingClientRect` gives its `<div>`.
fn label(context: &mut Context, text: &str, wrap_width: f64, is_left_aligned: bool) -> LabelBox {
	let (font, _) = rendering::label_font(context, &Styles::default());
	let mut label = rendering::layout_label(context, text, true, &font, Some(wrap_width), 0.0);
	label.is_left_aligned = is_left_aligned;
	label
}

fn write_label(
	svg: &mut Svg,
	context: &mut Context,
	label: &LabelBox,
	left: f64,
	top: f64,
	fill: &str,
) {
	if label.is_empty() {
		return;
	}
	super::label::write(svg, context, label, left + label.width / 2.0, top, &|svg: &mut Svg| {
		svg.attr_if("fill", fill);
	});
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let board = parse(code, context)?;
	let section_width = match context.config_number("kanban", "sectionWidth", 200.0) {
		0.0 => 200.0,
		width => width,
	};
	let ticket_base_url =
		context.config_text("kanban", "ticketBaseUrl").filter(|url| !url.is_empty());
	let padding = context.config_number("mindmap", "padding", 10.0);
	let use_max_width = context.config_bool("mindmap", "useMaxWidth", true);
	const GAP: f64 = 10.0;
	let theme = &context.theme;
	let text_color = theme.css("textColor");
	let background = theme.css("background");
	let node_border = theme.css("nodeBorder");
	let color_limit = theme.get("THEME_COLOR_LIMIT").and_then(Value::as_number).unwrap_or(12.0);
	let is_dark = matches!(theme.get("darkMode"), Some(Value::Bool(true)));
	// `.section-N rect` for N up to `THEME_COLOR_LIMIT - 2`, from `cScale(N + 1)`.
	let section_color = |section: usize| -> Option<String> {
		if section as f64 + 1.0 >= color_limit {
			return None;
		}
		let scale = theme.text(&format!("cScale{}", section + 1))?;
		Some(match is_dark {
			true => color::darken(&scale, 10.0)?,
			false => color::lighten(&scale, 10.0)?,
		})
	};
	let section_colors: Vec<Option<String>> =
		(1..=board.sections.len()).map(section_color).collect();

	let mut bbox = BBox::default();
	let mut sections_svg = Svg::new();
	let mut items_svg = Svg::new();
	let mut section_labels = Vec::new();
	let mut max_label_height: f64 = 25.0;
	for (number_of_section, &section_index) in board.sections.iter().enumerate() {
		let section = &board.nodes[section_index];
		let label = label(context, &section.label, section_width, false);
		max_label_height = max_label_height.max(label.height);
		let count = number_of_section as f64 + 1.0;
		let x = section_width * count + (count - 1.0) * GAP / 2.0;
		section_labels.push((x, label));
	}
	let top = -section_width * 3.0 / 2.0 + max_label_height;
	for (number_of_section, &section_index) in board.sections.iter().enumerate() {
		let section = &board.nodes[section_index];
		let (section_x, section_label) = &section_labels[number_of_section];
		let section_x = *section_x;
		let mut y = top;
		for item in board.nodes.iter().filter(|node| node.parent_id.as_ref() == Some(&section.id)) {
			let total_width = section_width - 1.5 * GAP;
			let title = label(context, &item.label, total_width - 10.0, true);
			let ticket = item.ticket.clone().unwrap_or_default();
			let mut ticket_label = label(context, &ticket, total_width - 10.0, true);
			// `.kanban-ticket-link { text-decoration: underline }` reaches the linked label.
			ticket_label.is_underlined = ticket_base_url.is_some() && !ticket.is_empty();
			let assigned = item.assigned.clone().unwrap_or_default();
			let assigned_label = label(context, &assigned, total_width - 10.0, true);
			const LABEL_PADDING: f64 = 10.0;
			let height_adjust = ticket_label.height.max(assigned_label.height) / 2.0;
			let total_height = title.height + LABEL_PADDING * 2.0 + height_adjust;
			let (x, rect_y) = (-total_width / 2.0, -total_height / 2.0);
			// `getBBox` of the node: its rectangle and labels.
			let mut node_box = BBox::default();
			node_box.include_rect(x, rect_y, total_width, total_height);
			let title_position =
				(LABEL_PADDING - total_width / 2.0, -height_adjust - title.height / 2.0);
			let ticket_position =
				(LABEL_PADDING - total_width / 2.0, -height_adjust + title.height / 2.0);
			let assigned_position = (
				LABEL_PADDING + total_width / 2.0 - assigned_label.width - 2.0 * LABEL_PADDING,
				-height_adjust + title.height / 2.0,
			);
			for (label, (left, label_top)) in [
				(&title, title_position),
				(&ticket_label, ticket_position),
				(&assigned_label, assigned_position),
			] {
				if !label.is_empty() {
					node_box.include_rect(left, label_top, label.width, label.height);
				}
			}
			let item_y = y + node_box.height() / 2.0;
			y = item_y + node_box.height() / 2.0 + GAP / 2.0;
			bbox.include(&node_box, section_x, item_y);

			items_svg
				.open("g")
				.attr("class", "node undefined")
				.attr("id", &format!("{}-{}", context.id, item.id))
				.attr("transform", &translate(section_x, item_y))
				.end();
			items_svg
				.open("rect")
				// Items get no `look`, so they keep the classic rectangle in every look.
				.attr("class", "basic label-container __APA__")
				.attr("rx", "5")
				.attr("ry", "5")
				.number_attr("x", x)
				.number_attr("y", rect_y)
				.number_attr("width", total_width)
				.number_attr("height", total_height)
				.attr_if("fill", &background)
				.attr_if("stroke", &node_border)
				.attr("stroke-width", "1px")
				.close();
			write_label(
				&mut items_svg,
				context,
				&title,
				title_position.0,
				title_position.1,
				&text_color,
			);
			let link = ticket_base_url
				.as_ref()
				.filter(|_| !ticket.is_empty())
				.map(|url| super::common::sanitize_url(&url.replace("#TICKET#", &ticket)));
			if let Some(link) = &link {
				items_svg
					.open("a")
					.attr("class", "kanban-ticket-link")
					.attr("xlink:href", link)
					.attr("target", "_blank")
					.end();
			}
			write_label(
				&mut items_svg,
				context,
				&ticket_label,
				ticket_position.0,
				ticket_position.1,
				&text_color,
			);
			if link.is_some() {
				items_svg.close_tag("a");
			}
			write_label(
				&mut items_svg,
				context,
				&assigned_label,
				assigned_position.0,
				assigned_position.1,
				&text_color,
			);
			if let Some(priority) = &item.priority {
				const RADIUS: f64 = 5.0;
				items_svg
					.open("line")
					.number_attr("x1", x + 2.0)
					.number_attr("y1", rect_y + (RADIUS / 2.0).floor())
					.number_attr("x2", x + 2.0)
					.number_attr("y2", rect_y + total_height - (RADIUS / 2.0).floor())
					.attr("stroke-width", "4");
				if let Some(color) = priority_color(priority) {
					items_svg.attr("stroke", color);
				}
				items_svg.close();
			}
			items_svg.close_tag("g");
		}
		let height = (y - top + 3.0 * GAP).max(50.0) + (max_label_height - 25.0);
		let rect_x = section_x - section_width / 2.0;
		let rect_y = -section_width * 3.0 / 2.0;
		let classes = format!("cluster undefined section-{}", number_of_section + 1);
		sections_svg
			.open("g")
			.attr("class", &classes)
			.attr("id", &format!("{}-{}", context.id, section.id));
		let is_hand_drawn = context.look == Look::HandDrawn;
		if is_hand_drawn {
			sections_svg.attr("data-look", Look::HandDrawn.data_name());
		}
		sections_svg.end();
		let color = section_colors[number_of_section].clone().unwrap_or_default();
		match is_hand_drawn {
			// `kanbanSection`: the section keeps the height it was drawn with, three times its
			// width, because the renderer then sets the height of a `rect` that is not there.
			true => {
				let options = rough::Options {
					seed: context.sketch_seed(&section.id),
					roughness: 0.7,
					fill: Some(context.theme.css("clusterBkg")),
					stroke: context.theme.css("clusterBorder"),
					stroke_width: "1.3".to_owned(),
					fill_weight: 4.0,
					hachure_gap: 1.5,
					fill_line_dash: Some("0 0".to_owned()),
					stroke_line_dash: Some("0 0".to_owned()),
					..rough::Options::default()
				};
				let full_height = section_width * 3.0;
				let d = sketch::rounded_rect_path(rect_x, rect_y, section_width, full_height, 5.0);
				let sketched = rough::path(&d, &options);
				// `.section-N path` paints both paths.
				let section_paint = Some(color.as_str()).filter(|color| !color.is_empty());
				let paint = sketch::PathPaint {
					stroke: section_paint,
					fill: section_paint,
					..sketch::PathPaint::default()
				};
				sketch::write_group(&mut sections_svg, &sketched, "", &paint);
				if let Some(drawn) = sketch::sketch_box(&sketched) {
					bbox.include(&drawn, 0.0, 0.0);
				}
			}
			false => {
				bbox.include_rect(rect_x, rect_y, section_width, height);
				sections_svg
					.open("rect")
					.attr("rx", "5")
					.attr("ry", "5")
					.number_attr("x", rect_x)
					.number_attr("y", rect_y)
					.number_attr("width", section_width)
					.number_attr("height", height)
					.attr_if("fill", &color)
					.attr_if("stroke", &color)
					.close();
			}
		}
		let (title_top, _) = rendering::title_margins(context);
		let label_left = section_x - section_label.width / 2.0;
		let label_top = rect_y + title_top;
		if !section_label.is_empty() {
			bbox.include_rect(label_left, label_top, section_label.width, section_label.height);
		}
		write_label(&mut sections_svg, context, section_label, label_left, label_top, &text_color);
		sections_svg.close_tag("g");
	}

	let mut body = Svg::new();
	body.open("g").attr("class", "sections").end();
	body.append(sections_svg);
	body.close_tag("g");
	body.open("g").attr("class", "items").end();
	body.append(items_svg);
	body.close_tag("g");
	let (x, y, width, height) = match bbox.is_empty() {
		true => (0.0, 0.0, 0.0, 0.0),
		false => (bbox.min.x, bbox.min.y, bbox.width(), bbox.height()),
	};
	let frame = Frame {
		x: x - padding,
		y: y - padding,
		width: width + 2.0 * padding,
		height: height + 2.0 * padding,
		use_max_width,
	};
	Ok(document(context, "kanban", &frame, &Default::default(), body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn board(code: &str) -> Result<Board> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_sections_and_items() {
		let board = board(
			"kanban\n  Todo\n    [Create Documentation]\n    docs[Blog %% post]@{ assigned: 'knsv', ticket: 12 }\n  id9[Ready]\n  :::red\n    a[\"`**b**`\"]@{ priority: 'High', label: \"L\n   two\" }\n    b:::red\n",
		)
		.unwrap();
		let labels: Vec<_> = board.nodes.iter().map(|node| node.label.as_str()).collect();
		assert_eq!(
			labels,
			["Todo", "Create Documentation", "Blog %% post", "Ready", "L<br/>two", "b:::red"]
		);
		assert_eq!(board.sections, [0, 3]);
		assert_eq!(board.nodes[2].ticket.as_deref(), Some("12"));
		assert_eq!(board.nodes[2].assigned.as_deref(), Some("knsv"));
		assert_eq!(board.nodes[4].priority.as_deref(), Some("High"));
		assert_eq!(board.nodes[5].parent_id.as_deref(), Some("id9"));
	}

	#[test]
	fn reports_what_mermaid_rejects() {
		// A blank line after `kanban` is a `SPACELINE` statement followed by indentation.
		let error = board("kanban\n\n  Todo\n    a\n").err().unwrap();
		assert_eq!(error.line, Some(3));
		assert!(board("kanban\n  Todo\n    Kanban board\n").is_err());
		let error = board("kanban\n  Todo\n    a\nb\n  c\n").err().unwrap();
		assert_eq!(error.message, "Items without section detected, found section (\"b\")");
		assert!(board("kanban\n  Todo\n    a\nb\n").is_ok());
		assert!(board("kanban\n  Todo\n    a@{ shape: Rect }\n").is_err());
		assert!(board("kanban\n  Todo\n    a@{ x^ }\n").is_err());
		assert!(board("kanban\n  a}").is_err());
		assert!(board("kanban\nTodo\n  a\n\n\n  b\n").is_ok());
	}
}
