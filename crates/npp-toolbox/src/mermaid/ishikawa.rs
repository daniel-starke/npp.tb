//! Ishikawa (fishbone) diagrams (`ishikawa.jison`, `ishikawaDb.ts`, `ishikawaRenderer.ts`,
//! `ishikawaStyles.ts`).
//!
//! The view box is `getBBox` of the drawing, so text boxes follow Blink: the advances from the
//! anchored start, widened by glyphs beyond them, and the rounded ascent and descent around the
//! baseline that `dominant-baseline` sets. Multi-line labels are one `<text>` per line where
//! Mermaid writes a `<tspan>` per line.

use super::color::js_number;
use super::common::sanitize_text;
use super::rendering::{BBox, sketch};
use super::rough;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Look, Result};

const SECTION: &str = "ishikawa";
const SPINE_BASE_LENGTH: f64 = 250.0;
const BONE_STUB: f64 = 30.0;
const BONE_BASE: f64 = 60.0;
const BONE_PER_CHILD: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	SpaceLine,
	Keyword,
	NewLine,
	SpaceList,
	Text,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
	kind: Kind,
	text: &'a str,
	/// The line after the token, where jison's lexer stands once it has read it.
	line_after: usize,
}

/// The jison lexer: the first rule that matches.
fn lex(code: &str) -> Vec<Token<'_>> {
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	while position < code.len() {
		let rest = &code[position..];
		let whitespace = rest.len() - rest.trim_start().len();
		let lowercase_start = rest.get(..13).unwrap_or(rest).to_ascii_lowercase();
		let (kind, length) = if rest[whitespace..].starts_with("%%") {
			// `\s*\%\%.*`
			let after = &rest[whitespace..];
			(Kind::SpaceLine, whitespace + after.find('\n').unwrap_or(after.len()))
		} else if lowercase_start.starts_with("ishikawa-beta") {
			(Kind::Keyword, 13)
		} else if lowercase_start.starts_with("ishikawa") {
			(Kind::Keyword, 8)
		} else if let Some(last_newline) =
			rest[..whitespace].rfind('\n').filter(|&newline| newline > 0)
		{
			// `[\s]+[\n]`: the longest run of whitespace that ends with a line end.
			(Kind::SpaceLine, last_newline + 1)
		} else if rest.starts_with('\n') {
			(Kind::NewLine, rest.len() - rest.trim_start_matches('\n').len())
		} else if whitespace > 0 {
			(Kind::SpaceList, whitespace)
		} else {
			(Kind::Text, rest.find('\n').unwrap_or(rest.len()))
		};
		let text = &rest[..length];
		line += text.matches('\n').count();
		tokens.push(Token { kind, text, line_after: line });
		position += length;
	}
	tokens
}

#[derive(Debug, Default)]
struct Node {
	text: String,
	children: Vec<usize>,
}

/// The nodes of the diagram, the effect first; empty without any.
fn parse(code: &str, context: &Context) -> Result<Vec<Node>> {
	// Mermaid's preprocessing trims the start of the code.
	let trimmed = code.trim_start();
	let line_offset = code[..code.len() - trimmed.len()].matches('\n').count();
	// Mermaid parses the code with a line end appended.
	let code = format!(
		"{trimmed}
"
	);
	let tokens = lex(&code);
	let mut index = 0;
	let error = |index: usize| {
		let line = tokens.get(index).or(tokens.last()).map_or(1, |token| token.line_after);
		let found = match tokens.get(index) {
			None => "the end of the diagram".to_owned(),
			Some(token) if token.kind == Kind::Text || token.kind == Kind::Keyword => {
				format!("{:?}", token.text.trim().chars().take(20).collect::<String>())
			}
			Some(_) => "a line end or blanks".to_owned(),
		};
		context.error_at(line + line_offset, format!("unexpected {found}"))
	};
	let kind = |index: usize| tokens.get(index).map(|token| token.kind);
	if kind(0) != Some(Kind::Keyword) {
		return Err(error(0));
	}
	index += 1;
	if kind(index) == Some(Kind::NewLine) {
		index += 1;
	}
	let mut nodes: Vec<Node> = Vec::new();
	let mut stack: Vec<(i64, usize)> = Vec::new();
	let mut base_level = None;
	let mut add_node = |raw_level: i64, text: &str| {
		let node = nodes.len();
		nodes.push(Node { text: sanitize_text(text.trim()), children: Vec::new() });
		if node == 0 {
			stack.push((0, 0));
			return;
		}
		let base = *base_level.get_or_insert(raw_level);
		let level = (raw_level - base + 1).max(1);
		while stack.len() > 1 && stack.last().is_some_and(|&(top, _)| top >= level) {
			stack.pop();
		}
		if let Some(&(_, parent)) = stack.last() {
			nodes[parent].children.push(node);
		}
		stack.push((level, node));
	};
	// `document: (statement stop)+`, then the end.
	loop {
		match kind(index) {
			Some(Kind::SpaceList) if kind(index + 1) == Some(Kind::Text) => {
				let level = tokens[index].text.encode_utf16().count() as i64;
				add_node(level, tokens[index + 1].text);
				index += 2;
			}
			Some(Kind::Text) => {
				add_node(0, tokens[index].text);
				index += 1;
			}
			Some(Kind::SpaceLine | Kind::SpaceList) => index += 1,
			_ => return Err(error(index)),
		}
		// `stop: (NL | EOF | SPACELINE) (NL | EOF)*`
		match kind(index) {
			None => break,
			Some(Kind::NewLine | Kind::SpaceLine) => index += 1,
			_ => return Err(error(index)),
		}
		while kind(index) == Some(Kind::NewLine) {
			index += 1;
		}
		if index >= tokens.len() {
			break;
		}
	}
	Ok(nodes)
}

/// `wrapText`: words joined into lines of at most `max_chars` UTF-16 units.
fn wrap_text(text: &str, max_chars: usize) -> String {
	let length = |text: &str| text.encode_utf16().count();
	if length(text) <= max_chars {
		return text.to_owned();
	}
	let mut lines: Vec<String> = Vec::new();
	for word in text.split(char::is_whitespace).filter(|word| !word.is_empty()) {
		match lines.last_mut() {
			Some(last) if length(last) + 1 + length(word) <= max_chars => {
				last.push(' ');
				last.push_str(word);
			}
			_ => lines.push(word.to_owned()),
		}
	}
	lines.join("\n")
}

/// `splitLines`: at `<br>`, `<br/>`, `<br />` and line ends.
fn split_lines(text: &str) -> Vec<&str> {
	let mut lines = Vec::new();
	let mut start = 0;
	let mut position = 0;
	while position < text.len() {
		let rest = &text[position..];
		let break_length = if rest.starts_with('\n') {
			Some(1)
		} else if let Some(after) = rest.strip_prefix("<br") {
			let spaces = after.len() - after.trim_start().len();
			let after = &after[spaces..];
			let slash = usize::from(after.starts_with('/'));
			after[slash..].starts_with('>').then_some(3 + spaces + slash + 1)
		} else {
			None
		};
		match break_length {
			Some(length) => {
				lines.push(&text[start..position]);
				position += length;
				start = position;
			}
			None => position += rest.chars().next().map_or(1, char::len_utf8),
		}
	}
	lines.push(&text[start..]);
	lines
}

struct Writer<'c, 'm> {
	context: &'c mut Context<'m>,
	body: Svg,
	bbox: BBox,
	label_font: Font,
	head_font: Font,
	/// `fontSize` of the configuration, which spaces the lines and bones.
	font_size: f64,
	line_color: String,
	main_background: String,
	text_color: String,
	marker_url: String,
	is_hand_drawn: bool,
	/// The roughjs calls so far, for the seeds of `handDrawnSeed: 0`.
	sketch_count: usize,
}

impl Writer<'_, '_> {
	/// The options of the next roughjs call.
	fn rough_options(&mut self, roughness: f64) -> rough::Options {
		self.sketch_count += 1;
		rough::Options {
			roughness,
			seed: self.context.sketch_seed(&format!("ishikawa-{}", self.sketch_count)),
			stroke: self.line_color.clone(),
			stroke_width: "2".to_owned(),
			..rough::Options::default()
		}
	}

	/// The options of the head and the label boxes.
	fn rough_box_options(&mut self) -> rough::Options {
		rough::Options {
			fill: Some(self.main_background.clone()),
			fill_weight: 2.5,
			hachure_gap: 5.0,
			..self.rough_options(1.5)
		}
	}

	/// Writes a roughjs call as a group with `class`, offset by `dy` in the drawing.
	fn write_sketch(&mut self, sketched: &rough::Sketch, class: &str, dy: f64) {
		sketch::write_group(&mut self.body, sketched, class, &sketch::PathPaint::default());
		if let Some(ink) = sketch::sketch_box(sketched) {
			self.bbox.include(&ink, 0.0, dy);
		}
	}

	/// `drawArrowMarker`: the arrow head at `x`, `y` pointing along `dx`, `dy`.
	fn arrow(&mut self, (x, y): (f64, f64), (dx, dy): (f64, f64)) {
		let length = (dx * dx + dy * dy).sqrt();
		if length == 0.0 || !length.is_finite() {
			return;
		}
		let (ux, uy) = (dx / length, dy / length);
		let size = 6.0;
		let (px, py) = (-uy * size, ux * size);
		let d = format!(
			"M {} {} L {} {} L {} {} Z",
			js_number(x),
			js_number(y),
			js_number(x - ux * size * 2.0 + px),
			js_number(y - uy * size * 2.0 + py),
			js_number(x - ux * size * 2.0 - px),
			js_number(y - uy * size * 2.0 - py)
		);
		let options = rough::Options {
			fill: Some(self.line_color.clone()),
			fill_style: rough::FillStyle::Solid,
			stroke_width: "1".to_owned(),
			..self.rough_options(1.0)
		};
		self.write_sketch(&rough::path(&d, &options), "", 0.0);
	}

	fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, class: &str) {
		if self.is_hand_drawn {
			let options = self.rough_options(1.5);
			self.write_sketch(&rough::line([x1, y1], [x2, y2], &options), class, 0.0);
			if class != "ishikawa-spine" {
				self.arrow((x1, y1), (x1 - x2, y1 - y2));
			}
			return;
		}
		let width = if class == "ishikawa-sub-branch" { "1" } else { "2" };
		self.body
			.open("line")
			.attr("class", class)
			.number_attr("x1", x1)
			.number_attr("y1", y1)
			.number_attr("x2", x2)
			.number_attr("y2", y2)
			.attr_if("stroke", &self.line_color)
			.attr("stroke-width", width)
			.attr("fill", "none");
		if class != "ishikawa-spine" {
			self.body.attr("marker-start", &self.marker_url);
		}
		self.body.close();
		self.bbox.include_rect(x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
	}

	/// The box of `drawMultilineText` (`getBBox` of the `<text>`) relative to its position.
	fn text_box(
		&mut self,
		lines: &[&str],
		font: &Font,
		anchor: Anchor,
		baseline: Baseline,
	) -> BBox {
		let line_height = self.font_size * 1.05;
		let first = -((lines.len() - 1) as f64 * line_height) / 2.0;
		let offset = match baseline {
			Baseline::Middle => self.context.x_height(font) / 2.0,
			Baseline::Hanging => {
				let ascent = self.context.vertical_metrics(font).0.round();
				(ascent * 0.8 * 64.0).round() / 64.0
			}
			_ => 0.0,
		};
		let mut bbox = BBox::default();
		for (index, line) in lines.iter().enumerate() {
			if line.split_whitespace().next().is_none() {
				continue;
			}
			let advance = self.context.measure(line, font).0;
			let start = match anchor {
				Anchor::Start => 0.0,
				Anchor::Middle => -advance / 2.0,
				Anchor::End => -advance,
			};
			let (width, height) = self.context.bounding_box(line, font);
			let left = start + self.context.bounding_box_left(line, font);
			let top = first
				+ index as f64 * line_height
				+ offset + self.context.bounding_box_top(line, font);
			bbox.include_rect(left, top, width, height);
		}
		bbox
	}

	/// `drawMultilineText` at `x`, `y`, with `transform` on each line, and its box.
	fn text(
		&mut self,
		text: &str,
		(x, y): (f64, f64),
		class: &str,
		(anchor, baseline): (Anchor, Baseline),
		is_head: bool,
		transform: Option<(f64, f64)>,
	) -> BBox {
		let lines = split_lines(text);
		let font = if is_head { self.head_font.clone() } else { self.label_font.clone() };
		let line_height = self.font_size * 1.05;
		let first = y - (lines.len() - 1) as f64 * line_height / 2.0;
		let transform_text =
			transform.map(|(dx, dy)| format!("translate({},{})", number(dx), number(dy)));
		for (index, line) in lines.iter().enumerate() {
			let run = TextRun {
				x,
				y: first + index as f64 * line_height,
				anchor,
				baseline,
				font: &font,
				content: line,
			};
			let fill = &self.text_color;
			self.body.text(run, |svg| {
				svg.attr("class", class).attr_if("fill", fill);
				if let Some(transform) = &transform_text {
					svg.attr("transform", transform);
				}
			});
		}
		let mut bbox = self.text_box(&lines, &font, anchor, baseline);
		if !bbox.is_empty() {
			bbox.min.x += x;
			bbox.max.x += x;
			bbox.min.y += y;
			bbox.max.y += y;
		}
		bbox
	}
}

fn style_text(context: &Context, key: &str) -> String {
	context.theme.text(key).unwrap_or_default()
}

/// `parseFontSize(config.fontSize)[0] ?? 14`: numbers as they are, texts by `parseInt`.
fn configured_font_size(context: &Context) -> f64 {
	match context.config.get("fontSize") {
		None => 16.0,
		Some(Value::Number(size)) => *size,
		Some(Value::String(text)) => {
			let trimmed = text.trim_start();
			let sign = usize::from(trimmed.starts_with(['-', '+']));
			let digits = trimmed[sign..].bytes().take_while(u8::is_ascii_digit).count();
			trimmed[..sign + digits].parse().unwrap_or(14.0)
		}
		Some(_) => 14.0,
	}
}

/// The descendants of each node, counted bottom up.
fn descendant_counts(nodes: &[Node]) -> Vec<usize> {
	let mut counts = vec![0; nodes.len()];
	// Children always come after their parents.
	for node in (0..nodes.len()).rev() {
		counts[node] = nodes[node].children.iter().map(|&child| 1 + counts[child]).sum();
	}
	counts
}

struct Entry {
	text: String,
	depth: usize,
	parent: Option<usize>,
	child_count: usize,
}

/// `flattenTree`: even depths in preorder, odd depths in postorder.
fn flatten_tree(nodes: &[Node], children: &[usize], direction: f64) -> (Vec<Entry>, Vec<usize>) {
	let ordered = |list: &[usize]| -> Vec<usize> {
		match direction < 0.0 {
			true => list.iter().rev().copied().collect(),
			false => list.to_vec(),
		}
	};
	struct Frame {
		children: Vec<usize>,
		next: usize,
		parent: Option<usize>,
		depth: usize,
		postorder_entry: Option<usize>,
	}
	let mut entries = Vec::new();
	let mut y_order = Vec::new();
	let mut stack = vec![Frame {
		children: ordered(children),
		next: 0,
		parent: None,
		depth: 2,
		postorder_entry: None,
	}];
	while let Some(frame) = stack.last_mut() {
		let Some(&child) = frame.children.get(frame.next) else {
			if let Some(entry) = frame.postorder_entry {
				y_order.push(entry);
			}
			stack.pop();
			continue;
		};
		frame.next += 1;
		let (depth, parent) = (frame.depth, frame.parent);
		let index = entries.len();
		let grandchildren = &nodes[child].children;
		entries.push(Entry {
			text: wrap_text(&nodes[child].text, 15),
			depth,
			parent,
			child_count: grandchildren.len(),
		});
		let is_even = depth % 2 == 0;
		if is_even {
			y_order.push(index);
		}
		if grandchildren.is_empty() {
			if !is_even {
				y_order.push(index);
			}
		} else {
			stack.push(Frame {
				children: ordered(grandchildren),
				next: 0,
				parent: Some(index),
				depth: depth + 1,
				postorder_entry: (!is_even).then_some(index),
			});
		}
	}
	(entries, y_order)
}

struct Bone {
	x0: f64,
	y0: f64,
	x1: f64,
	y1: f64,
	child_count: usize,
	children_drawn: usize,
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
	a + (b - a) * t
}

/// `drawBranch`: a cause, its label and its sub-causes; the box of its texts.
fn draw_branch(
	writer: &mut Writer,
	nodes: &[Node],
	cause: usize,
	(start_x, start_y): (f64, f64),
	direction: f64,
	length: f64,
) -> BBox {
	let angle = 82f64.to_radians();
	let (cos, sin) = (angle.cos(), angle.sin());
	let children = &nodes[cause].children;
	let line_length = length * if children.is_empty() { 0.2 } else { 1.0 };
	let (dx, dy) = (-cos * line_length, sin * line_length * direction);
	let (end_x, end_y) = (start_x + dx, start_y + dy);
	let mut texts = BBox::default();
	writer.line(start_x, start_y, end_x, end_y, "ishikawa-branch");

	writer.body.open("g").attr("class", "ishikawa-label-group").end();
	// The box goes behind the text, so the text is written after it.
	let label_position = (end_x, end_y + 11.0 * direction);
	let text = nodes[cause].text.clone();
	let lines = split_lines(&text);
	let font = writer.label_font.clone();
	let mut label_box = writer.text_box(&lines, &font, Anchor::Middle, Baseline::Middle);
	if !label_box.is_empty() {
		label_box.min.x += label_position.0;
		label_box.max.x += label_position.0;
		label_box.min.y += label_position.1;
		label_box.max.y += label_position.1;
	}
	let (box_x, box_y) =
		if label_box.is_empty() { (0.0, 0.0) } else { (label_box.min.x, label_box.min.y) };
	let (box_width, box_height) = (label_box.width(), label_box.height());
	if writer.is_hand_drawn {
		let options = writer.rough_box_options();
		let rectangle = rough::rectangle(
			box_x - 20.0,
			box_y - 2.0,
			box_width + 40.0,
			box_height + 4.0,
			&options,
		);
		writer.write_sketch(&rectangle, "ishikawa-label-box", 0.0);
	} else {
		writer
			.body
			.open("rect")
			.attr("class", "ishikawa-label-box")
			.number_attr("x", box_x - 20.0)
			.number_attr("y", box_y - 2.0)
			.number_attr("width", box_width + 40.0)
			.number_attr("height", box_height + 4.0)
			.attr_if("fill", &writer.main_background)
			.attr_if("stroke", &writer.line_color)
			.attr("stroke-width", "2")
			.close();
		writer.bbox.include_rect(box_x - 20.0, box_y - 2.0, box_width + 40.0, box_height + 4.0);
	}
	let drawn = writer.text(
		&text,
		label_position,
		"ishikawa-label cause",
		(Anchor::Middle, Baseline::Middle),
		false,
		None,
	);
	texts.include(&drawn, 0.0, 0.0);
	writer.bbox.include(&drawn, 0.0, 0.0);
	writer.body.close_tag("g");
	if children.is_empty() {
		return texts;
	}

	let (entries, y_order) = flatten_tree(nodes, children, direction);
	let mut ys = vec![0.0; entries.len()];
	for (slot, &entry) in y_order.iter().enumerate() {
		ys[entry] = start_y + dy * ((slot + 1) as f64 / (entries.len() + 1) as f64);
	}
	let mut bones: Vec<Option<Bone>> = (0..entries.len()).map(|_| None).collect();
	let mut root_bone = Bone {
		x0: start_x,
		y0: start_y,
		x1: end_x,
		y1: end_y,
		child_count: children.len(),
		children_drawn: 0,
	};
	let (diagonal_x, diagonal_y) = (-cos, sin * direction);
	let (odd_class, odd_baseline) = match direction < 0.0 {
		true => ("ishikawa-label up", Baseline::Alphabetic),
		false => ("ishikawa-label down", Baseline::Hanging),
	};
	for (index, entry) in entries.iter().enumerate() {
		let y = ys[index];
		let parent = match entry.parent {
			Some(parent) => bones[parent].as_mut(),
			None => Some(&mut root_bone),
		};
		let Some(parent) = parent else { continue };
		writer.body.open("g").attr("class", "ishikawa-sub-group").end();
		let (bone_x0, bone_y0, bone_x1);
		let drawn = if entry.depth % 2 == 0 {
			let parent_dy = parent.y1 - parent.y0;
			let t = if parent_dy != 0.0 { (y - parent.y0) / parent_dy } else { 0.5 };
			bone_x0 = lerp(parent.x0, parent.x1, t);
			bone_y0 = y;
			bone_x1 = bone_x0
				- match entry.child_count {
					0 => BONE_STUB,
					count => BONE_BASE + count as f64 * BONE_PER_CHILD,
				};
			writer.line(bone_x0, y, bone_x1, y, "ishikawa-sub-branch");
			writer.text(
				&entry.text,
				(bone_x1, y),
				"ishikawa-label align",
				(Anchor::End, Baseline::Middle),
				false,
				None,
			)
		} else {
			let drawn_before = parent.children_drawn;
			parent.children_drawn += 1;
			let t =
				(parent.child_count as f64 - drawn_before as f64) / (parent.child_count + 1) as f64;
			bone_x0 = lerp(parent.x0, parent.x1, t);
			bone_y0 = parent.y0;
			bone_x1 = bone_x0 + diagonal_x * ((y - bone_y0) / diagonal_y);
			writer.line(bone_x0, bone_y0, bone_x1, y, "ishikawa-sub-branch");
			writer.text(
				&entry.text,
				(bone_x1, y),
				odd_class,
				(Anchor::End, odd_baseline),
				false,
				None,
			)
		};
		texts.include(&drawn, 0.0, 0.0);
		writer.bbox.include(&drawn, 0.0, 0.0);
		writer.body.close_tag("g");
		if entry.child_count > 0 {
			bones[index] = Some(Bone {
				x0: bone_x0,
				y0: bone_y0,
				x1: bone_x1,
				y1: y,
				child_count: entry.child_count,
				children_drawn: 0,
			});
		}
	}
	texts
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let nodes = parse(code, context)?;
	let padding = context.config_number(SECTION, "diagramPadding", 20.0);
	let use_max_width = context.config_bool(SECTION, "useMaxWidth", false);
	let font_size = configured_font_size(context);
	let family = context.font_family();
	let label_font = Font::new(family.clone(), context.font_size());
	let mut head_font = Font::new(family, 14.0);
	head_font.weight = 600;
	let marker_id = format!("{}-ishikawa-arrow", context.id);
	let mut writer = Writer {
		body: Svg::new(),
		bbox: BBox::default(),
		label_font,
		head_font,
		font_size,
		line_color: style_text(context, "lineColor"),
		main_background: style_text(context, "mainBkg"),
		text_color: style_text(context, "textColor"),
		marker_url: format!("url(#{marker_id})"),
		is_hand_drawn: context.look == Look::HandDrawn,
		sketch_count: 0,
		context,
	};
	writer.body.open("g").attr("class", "ishikawa").end();
	if !nodes.is_empty() && !writer.is_hand_drawn {
		writer
			.body
			.open("defs")
			.end()
			.open("marker")
			.attr("id", &marker_id)
			.attr("viewBox", "0 0 10 10")
			.attr("refX", "0")
			.attr("refY", "5")
			.attr("markerWidth", "6")
			.attr("markerHeight", "6")
			.attr("orient", "auto")
			.end()
			.open("path")
			.attr("d", "M 10 0 L 0 5 L 10 10 Z")
			.attr("class", "ishikawa-arrow")
			.attr_if("fill", &writer.line_color)
			.close()
			.close_tag("marker")
			.close_tag("defs");
	}
	if !nodes.is_empty() {
		draw(&mut writer, &nodes);
	}
	writer.body.close_tag("g");
	let Writer { body, bbox, .. } = writer;
	let (x, y) = if bbox.is_empty() { (0.0, 0.0) } else { (bbox.min.x, bbox.min.y) };
	let frame = Frame {
		x: x - padding,
		y: y - padding,
		width: bbox.width() + padding * 2.0,
		height: bbox.height() + padding * 2.0,
		use_max_width,
	};
	Ok(document(context, "ishikawa", &frame, &Default::default(), body))
}

fn draw(writer: &mut Writer, nodes: &[Node]) {
	let causes = &nodes[0].children;
	let font_size = writer.font_size;
	let (mut upper_length, mut lower_length) = (SPINE_BASE_LENGTH, SPINE_BASE_LENGTH);
	let counts = descendant_counts(nodes);
	let side_stats = |parity: usize| {
		causes.iter().skip(parity).step_by(2).fold((0usize, 0usize), |(total, max), &cause| {
			(total + counts[cause], max.max(counts[cause]))
		})
	};
	let (upper_total, upper_max) = side_stats(0);
	let (lower_total, lower_max) = side_stats(1);
	let total = upper_total + lower_total;
	if total > 0 {
		let pool = SPINE_BASE_LENGTH * 2.0;
		let min_length = SPINE_BASE_LENGTH * 0.3;
		upper_length = min_length.max(pool * (upper_total as f64 / total as f64));
		lower_length = min_length.max(pool * (lower_total as f64 / total as f64));
	}
	let min_spacing = font_size * 2.0;
	upper_length = upper_length.max(upper_max as f64 * min_spacing);
	lower_length = lower_length.max(lower_max as f64 * min_spacing);
	let spine_y =
		if causes.is_empty() { SPINE_BASE_LENGTH } else { upper_length.max(SPINE_BASE_LENGTH) };

	// The spine is drawn first; its left end is known after the causes.
	let spine_at = writer.body.out.len();
	let texts_before = writer.body.texts.len();

	// `drawHead`
	let max_chars = 6usize.max((110.0 / (font_size * 0.6)).floor().max(0.0) as usize);
	let label = wrap_text(&nodes[0].text, max_chars);
	let lines = split_lines(&label);
	let head_font = writer.head_font.clone();
	let text_box = writer.text_box(&lines, &head_font, Anchor::Middle, Baseline::Middle);
	let (tb_x, tb_y) =
		if text_box.is_empty() { (0.0, 0.0) } else { (text_box.min.x, text_box.min.y) };
	let (tb_width, tb_height) = (text_box.width(), text_box.height());
	let width = 60f64.max(tb_width + 6.0);
	let height = 40f64.max(tb_height * 2.0 + 40.0);
	let head_path = format!(
		"M 0 {} L 0 {} Q {} 0 0 {} Z",
		number(-height / 2.0),
		number(height / 2.0),
		number(width * 2.4),
		number(-height / 2.0)
	);
	let translate = format!("translate(0,{})", number(spine_y));
	writer.body.open("g").attr("class", "ishikawa-head-group").attr("transform", &translate).end();
	if writer.is_hand_drawn {
		let head_path = format!(
			"M 0 {} L 0 {} Q {} 0 0 {} Z",
			js_number(-height / 2.0),
			js_number(height / 2.0),
			js_number(width * 2.4),
			js_number(-height / 2.0)
		);
		let options = writer.rough_box_options();
		writer.write_sketch(&rough::path(&head_path, &options), "ishikawa-head", spine_y);
	} else {
		writer
			.body
			.open("path")
			.attr("class", "ishikawa-head")
			.attr("d", &head_path)
			.attr_if("fill", &writer.main_background)
			.attr_if("stroke", &writer.line_color)
			.attr("stroke-width", "2")
			.close();
		// The tight box of the quadratic curve reaches 1.2 times its control point's x.
		writer.bbox.include_rect(0.0, spine_y - height / 2.0, width * 1.2, height);
	}
	let shift = ((width - tb_width) / 2.0 - tb_x + 3.0, -tb_y - tb_height / 2.0);
	writer.text(
		&label,
		(0.0, 0.0),
		"ishikawa-head-label",
		(Anchor::Middle, Baseline::Middle),
		true,
		Some(shift),
	);
	writer.bbox.include(&text_box, shift.0, shift.1 + spine_y);
	writer.body.close_tag("g");

	let mut spine_x = 0.0;
	if !causes.is_empty() {
		spine_x = -20.0;
		for pair in causes.chunks(2) {
			writer.body.open("g").attr("class", "ishikawa-pair").end();
			let mut texts = BBox::default();
			for (&cause, (direction, length)) in
				pair.iter().zip([(-1.0, upper_length), (1.0, lower_length)])
			{
				let drawn =
					draw_branch(writer, nodes, cause, (spine_x, spine_y), direction, length);
				texts.include(&drawn, 0.0, 0.0);
			}
			writer.body.close_tag("g");
			if !texts.is_empty() {
				spine_x = texts.min.x;
			}
		}
	}
	// The sketched spine is drawn last, over the causes.
	if writer.is_hand_drawn {
		writer.line(spine_x, spine_y, 0.0, spine_y, "ishikawa-spine");
		return;
	}
	let mut spine = Svg::new();
	spine
		.open("line")
		.attr("class", "ishikawa-spine")
		.number_attr("x1", spine_x)
		.number_attr("y1", spine_y)
		.number_attr("x2", 0.0)
		.number_attr("y2", spine_y)
		.attr_if("stroke", &writer.line_color)
		.attr("stroke-width", "2")
		.attr("fill", "none")
		.close();
	writer.bbox.include_rect(spine_x.min(0.0), spine_y, spine_x.abs(), 0.0);
	insert(&mut writer.body, spine_at, texts_before, spine);
}

/// Inserts `part`, which has no text, at byte `at` of `svg`, before the texts from
/// `texts_before` on.
fn insert(svg: &mut Svg, at: usize, texts_before: usize, part: Svg) {
	let length = part.out.len();
	svg.out.insert_str(at, &part.out);
	for text in &mut svg.texts[texts_before..] {
		text.element.start += length;
		text.element.end += length;
		text.attributes.start += length;
		text.attributes.end += length;
	}
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn tree(code: &str) -> Result<Vec<(usize, String)>> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		let nodes = parse(code, &context)?;
		let mut rows = Vec::new();
		let mut stack = if nodes.is_empty() { vec![] } else { vec![(0, 0)] };
		while let Some((node, depth)) = stack.pop() {
			rows.push((depth, nodes[node].text.clone()));
			stack.extend(nodes[node].children.iter().rev().map(|&child| (child, depth + 1)));
		}
		Ok(rows)
	}

	#[test]
	fn parses_indentation_like_mermaid() {
		let rows =
			tree("ishikawa\n  Effect\n    Cause A\n      Sub\n    Cause B\n  Other\n").unwrap();
		let texts: Vec<(usize, &str)> = rows.iter().map(|(d, t)| (*d, t.as_str())).collect();
		assert_eq!(
			texts,
			[(0, "Effect"), (1, "Cause A"), (2, "Sub"), (1, "Cause B"), (1, "Other")]
		);
		// The level counts from the first cause, so a less indented cause is one too.
		let rows = tree("ishikawa-beta\n      Effect\n    A\n  B\n").unwrap();
		assert_eq!(rows.iter().map(|(d, _)| *d).collect::<Vec<_>>(), [0, 1, 1]);
		assert!(tree("ishikawa Effect\n").is_ok());
		assert!(tree("ishikawa\n  E\n\n  A\n   \n  B").is_ok());
		let line = |code: &str| tree(code).err().and_then(|error| error.line);
		assert_eq!(line("ishikawa\n\n  Effect\n"), Some(3));
		assert_eq!(line("ishikawa\n  E\n  ishikawa\n"), Some(3));
		assert!(tree("ishikawa\n").unwrap().is_empty());
		assert_eq!(line("ishikawa"), Some(2));
	}

	#[test]
	fn wraps_and_splits_labels() {
		assert_eq!(wrap_text("short", 15), "short");
		assert_eq!(wrap_text("a much longer cause text", 15), "a much longer\ncause text");
		assert_eq!(split_lines("a<br>b<br/>c<br />d\ne<bR>"), ["a", "b", "c", "d", "e<bR>"]);
	}
}
