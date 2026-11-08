//! Sankey diagrams (`sankey.jison`, `sankeyDB.ts`, `sankeyUtils.ts`, `sankeyRenderer.ts`,
//! `styles.js`) with the layout of d3-sankey 0.12.3 (`sankey.js`, `align.js`).

use std::cmp::Ordering;

use super::color::{js_number, js_round};
use super::common::{BlinkBox, sanitize_text, stable_sort};
use super::label::encode_entities;
use super::svg::Accessibility;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::value::{Value, parse_leading_number};
use super::{Context, Diagram, Result};

/// d3's `schemeTableau10`.
const TABLEAU10: [&str; 10] = [
	"#4e79a7", "#f28e2c", "#e15759", "#76b7b2", "#59a14f", "#edc949", "#af7aa1", "#ff9da7",
	"#9c755f", "#bab0ab",
];
const LABEL_FONT_SIZE: f64 = 14.0;
/// d3-sankey's default number of relaxation passes.
const ITERATIONS: usize = 6;

struct Graph {
	nodes: Vec<Node>,
	links: Vec<Link>,
}

#[derive(Default)]
struct Node {
	id: String,
	source_links: Vec<usize>,
	target_links: Vec<usize>,
	value: f64,
	depth: usize,
	height: usize,
	layer: usize,
	x0: f64,
	x1: f64,
	y0: f64,
	y1: f64,
}

struct Link {
	source: usize,
	target: usize,
	value: f64,
	width: f64,
	y0: f64,
	y1: f64,
}

/// A token of the jison lexer, with its byte range.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
	Newline,
	Comma,
	Quote,
	Text,
	EscapedText,
	End,
}

/// The CSV lexer of `sankey.jison` (first match; `TEXTDATA` is printable ASCII without `"`
/// and `,`, so every other character ends a field as an empty token).
struct Lexer<'a> {
	text: &'a str,
	position: usize,
	is_escaped: bool,
}

fn is_text_data(byte: u8) -> bool {
	matches!(byte, 0x20..=0x7E) && byte != b'"' && byte != b','
}

fn is_line_end(byte: u8) -> bool {
	matches!(byte, b'\r' | b'\n')
}

impl<'a> Lexer<'a> {
	fn next(&mut self) -> (Token, &'a str, usize) {
		let start = self.position;
		let bytes = &self.text.as_bytes()[start..];
		let length_while = |is_part: &dyn Fn(&[u8]) -> usize| {
			let mut length = 0;
			loop {
				match is_part(&bytes[length..]) {
					0 => return length,
					part => length += part,
				}
			}
		};
		let (token, length) = if self.is_escaped {
			if bytes.first() == Some(&b'"') && bytes.get(1) != Some(&b'"') {
				self.is_escaped = false;
				(Token::Quote, 1)
			} else {
				let length = length_while(&|rest| match rest {
					[b'"', b'"', ..] => 2,
					[byte, ..] if is_text_data(*byte) || *byte == b',' || is_line_end(*byte) => 1,
					_ => 0,
				});
				(Token::EscapedText, length)
			}
		} else {
			match bytes.first() {
				None => (Token::End, 0),
				// `prepareTextForParsing` turns every run of line end characters into one `\n`.
				Some(byte) if is_line_end(*byte) => {
					(Token::Newline, bytes.iter().take_while(|byte| is_line_end(**byte)).count())
				}
				Some(b',') => (Token::Comma, 1),
				Some(b'"') => {
					self.is_escaped = true;
					(Token::Quote, 1)
				}
				Some(_) => {
					(Token::Text, bytes.iter().take_while(|byte| is_text_data(**byte)).count())
				}
			}
		};
		self.position += length;
		(token, &self.text[start..self.position], start)
	}
}

fn describe(token: Token) -> &'static str {
	match token {
		Token::Newline => "a line end",
		Token::Comma => "`,`",
		Token::Quote => "`\"`",
		Token::Text | Token::EscapedText => "a character that is not printable ASCII",
		Token::End => "the end",
	}
}

fn parse(code: &str, context: &Context) -> Result<Graph> {
	// `prepareTextForParsing`: trimmed, and every run of line end characters becomes one `\n`.
	let code = encode_entities(code);
	let trimmed = code.trim();
	let mut line = 1 + code[..code.len() - code.trim_start().len()].matches('\n').count();
	let mut text = String::with_capacity(trimmed.len());
	let mut line_starts = vec![(0, line)];
	for character in trimmed.chars() {
		if character == '\r' || character == '\n' {
			line += usize::from(character == '\n');
			if !text.ends_with('\n') {
				text.push('\n');
			}
			continue;
		}
		if text.ends_with('\n') {
			line_starts.push((text.len(), line));
		}
		text.push(character);
	}
	let line_of = |position: usize| {
		let index = line_starts.partition_point(|(start, _)| *start <= position);
		line_starts[index.saturating_sub(1)].1
	};
	let mut lexer = Lexer { text: &text, position: 0, is_escaped: false };
	let error = |found: Token, position: usize, expected: &str| {
		context
			.error_at(line_of(position), format!("expected {expected}, found {}", describe(found)))
	};
	let keyword = ["sankey-beta", "sankey"].into_iter().find(|keyword| {
		text.get(..keyword.len()).is_some_and(|word| word.eq_ignore_ascii_case(keyword))
	});
	let Some(keyword) = keyword else {
		return Err(context.error_at(line_of(0), "expected `sankey`"));
	};
	lexer.position += keyword.len();
	let mut graph = Graph { nodes: Vec::new(), links: Vec::new() };
	let mut node_ids = std::collections::HashMap::new();
	let (token, _, position) = lexer.next();
	if token != Token::Newline {
		return Err(error(token, position, "a line end"));
	}
	loop {
		let mut fields = [""; 3];
		for (index, field) in fields.iter_mut().enumerate() {
			if index > 0 {
				let (token, _, position) = lexer.next();
				if token != Token::Comma {
					return Err(error(token, position, "`,`"));
				}
			}
			*field = match lexer.next() {
				(Token::Text, text, _) => text,
				(Token::Quote, _, _) => {
					let (token, text, position) = lexer.next();
					if token != Token::EscapedText || text.is_empty() {
						return Err(error(token, position, "a quoted text"));
					}
					let (token, _, position) = lexer.next();
					if token != Token::Quote {
						return Err(error(token, position, "`\"`"));
					}
					text
				}
				(token, _, position) => return Err(error(token, position, "a field")),
			};
		}
		let mut node = |field: &str| {
			let id = sanitize_text(&field.trim().replace("\"\"", "\""));
			*node_ids.entry(id.clone()).or_insert_with(|| {
				graph.nodes.push(Node { id, ..Node::default() });
				graph.nodes.len() - 1
			})
		};
		let source = node(fields[0]);
		let target = node(fields[1]);
		let value = parse_leading_number(fields[2]).unwrap_or(f64::NAN);
		graph.links.push(Link { source, target, value, width: 0.0, y0: 0.0, y1: 0.0 });
		match lexer.next() {
			(Token::Newline, _, _) => continue,
			(Token::End, _, _) => return Ok(graph),
			(token, _, position) => return Err(error(token, position, "`,` or a line end")),
		}
	}
}

/// d3's `sum`: ignores `NaN` (and zeros).
fn sum(values: impl Iterator<Item = f64>) -> f64 {
	values.filter(|value| *value != 0.0 && !value.is_nan()).fold(0.0, |sum, value| sum + value)
}

/// d3's `min`, ignoring `NaN`; `NaN` (undefined) without values.
fn min(values: impl Iterator<Item = f64>) -> f64 {
	values.filter(|value| !value.is_nan()).reduce(f64::min).unwrap_or(f64::NAN)
}

/// The order of a comparator returning `a - b`, which JavaScript's stable sort treats as
/// equal for zero and `NaN`.
fn ascending(a: f64, b: f64) -> Ordering {
	match a - b {
		difference if difference < 0.0 => Ordering::Less,
		difference if difference > 0.0 => Ordering::Greater,
		_ => Ordering::Equal,
	}
}

#[derive(Clone, Copy, PartialEq)]
enum Alignment {
	Left,
	Right,
	Center,
	Justify,
}

/// d3-sankey's layout with the extent `[[0, 0], [width, height]]`.
struct Layout {
	width: f64,
	height: f64,
	node_width: f64,
	node_padding: f64,
	alignment: Alignment,
}

impl Layout {
	fn run(&self, graph: &mut Graph) -> std::result::Result<(), &'static str> {
		let Graph { nodes, links } = graph;
		for (index, link) in links.iter().enumerate() {
			nodes[link.source].source_links.push(index);
			nodes[link.target].target_links.push(index);
		}
		for node in nodes.iter_mut() {
			node.value = f64::max(
				sum(node.source_links.iter().map(|&link| links[link].value)),
				sum(node.target_links.iter().map(|&link| links[link].value)),
			);
		}
		self.compute_depths(nodes, links, true)?;
		self.compute_depths(nodes, links, false)?;
		let mut columns = self.compute_layers(nodes, links);
		let padding = self
			.node_padding
			.min(self.height / (columns.iter().map(Vec::len).max().unwrap_or(0) as f64 - 1.0));
		self.initialize_breadths(nodes, links, &mut columns, padding);
		for iteration in 0..ITERATIONS {
			let alpha = 0.99_f64.powi(iteration as i32);
			let beta = f64::max(1.0 - alpha, (iteration + 1) as f64 / ITERATIONS as f64);
			self.relax(nodes, links, &mut columns, alpha, beta, padding, false);
			self.relax(nodes, links, &mut columns, alpha, beta, padding, true);
		}
		for node in nodes.iter() {
			let mut y = node.y0;
			for &link in &node.source_links {
				links[link].y0 = y + links[link].width / 2.0;
				y += links[link].width;
			}
			let mut y = node.y0;
			for &link in &node.target_links {
				links[link].y1 = y + links[link].width / 2.0;
				y += links[link].width;
			}
		}
		Ok(())
	}

	/// `computeNodeDepths` (`is_depth`) and `computeNodeHeights`.
	fn compute_depths(
		&self,
		nodes: &mut [Node],
		links: &[Link],
		is_depth: bool,
	) -> std::result::Result<(), &'static str> {
		let count = nodes.len();
		let mut current: Vec<usize> = (0..count).collect();
		let mut is_next = vec![false; count];
		let mut distance = 0;
		while !current.is_empty() {
			let mut next = Vec::new();
			for &index in &current {
				let node = &mut nodes[index];
				let neighbors = match is_depth {
					true => {
						node.depth = distance;
						&node.source_links
					}
					false => {
						node.height = distance;
						&node.target_links
					}
				};
				for &link in neighbors {
					let neighbor = if is_depth { links[link].target } else { links[link].source };
					if !is_next[neighbor] {
						is_next[neighbor] = true;
						next.push(neighbor);
					}
				}
			}
			distance += 1;
			if distance > count {
				return Err("circular link");
			}
			for &index in &next {
				is_next[index] = false;
			}
			current = next;
		}
		Ok(())
	}

	fn compute_layers(&self, nodes: &mut [Node], links: &[Link]) -> Vec<Vec<usize>> {
		let layer_count = nodes.iter().map(|node| node.depth).max().unwrap_or(0) + 1;
		let layer_step = (self.width - self.node_width) / (layer_count as f64 - 1.0);
		let mut columns = vec![Vec::new(); layer_count];
		for index in 0..nodes.len() {
			let layer = match self.alignment {
				Alignment::Left => nodes[index].depth,
				Alignment::Right => (layer_count - 1).saturating_sub(nodes[index].height),
				Alignment::Justify => match nodes[index].source_links.is_empty() {
					false => nodes[index].depth,
					true => layer_count - 1,
				},
				Alignment::Center => match () {
					_ if !nodes[index].target_links.is_empty() => nodes[index].depth,
					_ => nodes[index]
						.source_links
						.iter()
						.map(|&link| nodes[links[link].target].depth)
						.min()
						.map_or(0, |depth| depth.saturating_sub(1)),
				},
			};
			let layer = layer.min(layer_count - 1);
			let node = &mut nodes[index];
			node.layer = layer;
			node.x0 = layer as f64 * layer_step;
			node.x1 = node.x0 + self.node_width;
			columns[layer].push(index);
		}
		columns
	}

	fn initialize_breadths(
		&self,
		nodes: &mut [Node],
		links: &mut [Link],
		columns: &mut [Vec<usize>],
		padding: f64,
	) {
		let scale = min(columns.iter().map(|column| {
			(self.height - (column.len() as f64 - 1.0) * padding)
				/ sum(column.iter().map(|&node| nodes[node].value))
		}));
		for column in columns.iter() {
			let mut y = 0.0;
			for &index in column {
				let node = &mut nodes[index];
				node.y0 = y;
				node.y1 = y + node.value * scale;
				y = node.y1 + padding;
				for &link in &node.source_links {
					links[link].width = links[link].value * scale;
				}
			}
			let spacing = (self.height - y + padding) / (column.len() as f64 + 1.0);
			for (position, &index) in column.iter().enumerate() {
				nodes[index].y0 += spacing * (position + 1) as f64;
				nodes[index].y1 += spacing * (position + 1) as f64;
			}
			for &index in column {
				sort_source_links(nodes, links, index);
				sort_target_links(nodes, links, index);
			}
		}
	}

	/// `relaxLeftToRight` (`is_left_to_right`, by incoming links) and `relaxRightToLeft`.
	#[allow(clippy::too_many_arguments)]
	fn relax(
		&self,
		nodes: &mut [Node],
		links: &[Link],
		columns: &mut [Vec<usize>],
		alpha: f64,
		beta: f64,
		padding: f64,
		is_left_to_right: bool,
	) {
		let order: Vec<usize> = match is_left_to_right {
			true => (1..columns.len()).collect(),
			false => (0..columns.len().saturating_sub(1)).rev().collect(),
		};
		for column_index in order {
			for &index in &columns[column_index] {
				let mut y = 0.0;
				let mut weight = 0.0;
				let node_links = match is_left_to_right {
					true => &nodes[index].target_links,
					false => &nodes[index].source_links,
				};
				for &link in node_links {
					let Link { source, target, value, .. } = links[link];
					let v = value * (nodes[target].layer as f64 - nodes[source].layer as f64);
					let top = match is_left_to_right {
						true => target_top(nodes, links, source, target, padding),
						false => source_top(nodes, links, source, target, padding),
					};
					y += top * v;
					weight += v;
				}
				// Also skips the `NaN` weight of links without a value.
				if weight.partial_cmp(&0.0) != Some(Ordering::Greater) {
					continue;
				}
				let dy = (y / weight - nodes[index].y0) * alpha;
				nodes[index].y0 += dy;
				nodes[index].y1 += dy;
				let (targets, sources) =
					(nodes[index].target_links.clone(), nodes[index].source_links.clone());
				for link in targets {
					sort_source_links(nodes, links, links[link].source);
				}
				for link in sources {
					sort_target_links(nodes, links, links[link].target);
				}
			}
			stable_sort(&mut columns[column_index], |a, b| ascending(nodes[a].y0, nodes[b].y0));
			self.resolve_collisions(nodes, &columns[column_index], beta, padding);
		}
	}

	fn resolve_collisions(&self, nodes: &mut [Node], column: &[usize], alpha: f64, padding: f64) {
		let Some(&subject) = column.get(column.len() >> 1) else { return };
		let middle = column.len() >> 1;
		let (subject_top, subject_bottom) = (nodes[subject].y0, nodes[subject].y1);
		push_up(nodes, &column[..middle], subject_top - padding, alpha, padding);
		push_down(nodes, &column[middle + 1..], subject_bottom + padding, alpha, padding);
		push_up(nodes, column, self.height, alpha, padding);
		push_down(nodes, column, 0.0, alpha, padding);
	}
}

/// `resolveCollisionsTopToBottom`: pushes overlapping nodes down.
fn push_down(nodes: &mut [Node], column: &[usize], mut y: f64, alpha: f64, padding: f64) {
	for &index in column {
		let node = &mut nodes[index];
		let dy = (y - node.y0) * alpha;
		if dy > 1e-6 {
			node.y0 += dy;
			node.y1 += dy;
		}
		y = node.y1 + padding;
	}
}

/// `resolveCollisionsBottomToTop`: pushes overlapping nodes up.
fn push_up(nodes: &mut [Node], column: &[usize], mut y: f64, alpha: f64, padding: f64) {
	for &index in column.iter().rev() {
		let node = &mut nodes[index];
		let dy = (node.y1 - y) * alpha;
		if dy > 1e-6 {
			node.y0 -= dy;
			node.y1 -= dy;
		}
		y = node.y0 - padding;
	}
}

/// Sorts the outgoing links of a node by their targets (`ascendingTargetBreadth`).
fn sort_source_links(nodes: &mut [Node], links: &[Link], index: usize) {
	let mut sorted = std::mem::take(&mut nodes[index].source_links);
	stable_sort(&mut sorted, |a, b| {
		ascending(nodes[links[a].target].y0, nodes[links[b].target].y0).then(a.cmp(&b))
	});
	nodes[index].source_links = sorted;
}

/// Sorts the incoming links of a node by their sources (`ascendingSourceBreadth`).
fn sort_target_links(nodes: &mut [Node], links: &[Link], index: usize) {
	let mut sorted = std::mem::take(&mut nodes[index].target_links);
	stable_sort(&mut sorted, |a, b| {
		ascending(nodes[links[a].source].y0, nodes[links[b].source].y0).then(a.cmp(&b))
	});
	nodes[index].target_links = sorted;
}

/// `targetTop`: the `target.y0` that gives an ideal link from `source`.
fn target_top(nodes: &[Node], links: &[Link], source: usize, target: usize, padding: f64) -> f64 {
	let source_node = &nodes[source];
	let mut y = source_node.y0 - (source_node.source_links.len() as f64 - 1.0) * padding / 2.0;
	for &link in &source_node.source_links {
		if links[link].target == target {
			break;
		}
		y += links[link].width + padding;
	}
	for &link in &nodes[target].target_links {
		if links[link].source == source {
			break;
		}
		y -= links[link].width;
	}
	y
}

/// `sourceTop`: the `source.y0` that gives an ideal link to `target`.
fn source_top(nodes: &[Node], links: &[Link], source: usize, target: usize, padding: f64) -> f64 {
	let target_node = &nodes[target];
	let mut y = target_node.y0 - (target_node.target_links.len() as f64 - 1.0) * padding / 2.0;
	for &link in &target_node.target_links {
		if links[link].source == source {
			break;
		}
		y += links[link].width + padding;
	}
	for &link in &nodes[source].source_links {
		if links[link].target == target {
			break;
		}
		y -= links[link].width;
	}
	y
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let mut graph = parse(code, context)?;
	let width = context.config_number("sankey", "width", 600.0);
	let height = context.config_number("sankey", "height", 400.0);
	let use_max_width = context.config_bool("sankey", "useMaxWidth", true);
	let show_values = context.config_bool("sankey", "showValues", true);
	let prefix = context.config_text("sankey", "prefix").unwrap_or_default();
	let suffix = context.config_text("sankey", "suffix").unwrap_or_default();
	let node_width = context.config_number("sankey", "nodeWidth", 10.0);
	let node_padding = context.config_number("sankey", "nodePadding", 12.0);
	let is_outlined = context.config_text("sankey", "labelStyle").as_deref() == Some("outlined");
	let link_color = context.config_text("sankey", "linkColor");
	let alignment = match context.config_text("sankey", "nodeAlignment").as_deref() {
		None | Some("justify") => Alignment::Justify,
		Some("left") => Alignment::Left,
		Some("right") => Alignment::Right,
		Some("center") => Alignment::Center,
		Some(other) => {
			return Err(context.error_at(1, format!("unknown sankey.nodeAlignment `{other}`")));
		}
	};
	let layout = Layout {
		width,
		height,
		node_width,
		node_padding: node_padding + if show_values { 15.0 } else { 0.0 },
		alignment,
	};
	layout.run(&mut graph).map_err(|message| context.error_at(1, message))?;

	let custom_colors = context.config_value("sankey", "nodeColors").cloned();
	let mut next_color = 0;
	let colors: Vec<String> = graph
		.nodes
		.iter()
		.map(|node| {
			let custom = custom_colors.as_ref().and_then(|colors| colors.get(&node.id));
			match custom.and_then(Value::to_text) {
				Some(color) => color,
				None => {
					next_color += 1;
					TABLEAU10[(next_color - 1) % TABLEAU10.len()].to_owned()
				}
			}
		})
		.collect();

	let mut bbox = BlinkBox::default();
	let mut body = Svg::new();
	body.open("g").close();
	body.open("g").attr("class", "nodes").end();
	for (index, node) in graph.nodes.iter().enumerate() {
		let (node_height, node_width) = (node.y1 - node.y0, node.x1 - node.x0);
		body.open("g")
			.attr("class", "node")
			.attr("id", &format!("{}-node-{}", context.id, index + 1));
		if node.x0.is_finite() && node.y0.is_finite() {
			body.attr("transform", &format!("translate({},{})", number(node.x0), number(node.y0)));
		}
		body.end();
		body.open("rect");
		if node_height.is_finite() {
			body.number_attr("height", node_height);
		}
		if node_width.is_finite() {
			body.number_attr("width", node_width);
		}
		body.attr("fill", &colors[index]).attr("shape-rendering", "crispEdges").close();
		body.close_tag("g");
		bbox.include(node.x0, node.y0, node_width, node_height);
	}
	body.close_tag("g");

	let central_layer = graph
		.nodes
		.iter()
		.fold((0.0, 0), |(max, layer), node| match node.value > max {
			true => (node.value, node.layer),
			false => (max, layer),
		})
		.1;
	let font = Font::new(context.font_family(), LABEL_FONT_SIZE);
	let (ascent, _) = context.vertical_metrics(&font);
	let labels: Vec<_> = graph
		.nodes
		.iter()
		.map(|node| {
			let content = match show_values {
				true => format!(
					"{}\n{prefix}{}{suffix}",
					node.id,
					js_number(js_round(node.value * 100.0) / 100.0)
				),
				false => node.id.clone(),
			};
			let is_left = match is_outlined {
				true => node.layer < central_layer,
				false => node.x0.partial_cmp(&(width / 2.0)) != Some(Ordering::Less),
			};
			let (x, anchor) = match is_left {
				true => (node.x0 - 6.0, Anchor::End),
				false => (node.x1 + 6.0, Anchor::Start),
			};
			let dy = if show_values { 0.0 } else { 0.35 * LABEL_FONT_SIZE };
			// Browsers read the `NaN` of degenerate layouts as zero.
			let finite_or_zero = |value: f64| if value.is_finite() { value } else { 0.0 };
			let y = finite_or_zero((node.y1 + node.y0) / 2.0) + dy;
			(content, finite_or_zero(x), y, anchor)
		})
		.collect();
	for (content, x, y, anchor) in &labels {
		// Blink's layout units: 1/64 pixel, rounded up.
		let advance = (context.measure(content, &font).0 * 64.0 - 0.001).ceil() / 64.0;
		let (box_width, box_height) = context.bounding_box(content, &font);
		let start = if *anchor == Anchor::End { x - advance } else { *x };
		let left = start + context.bounding_box_left(content, &font);
		bbox.include(left, y - ascent.round(), box_width, box_height);
	}
	body.open("g").attr("class", "node-labels").attr("font-size", "14").end();
	let background = ["mainBkg", "background"]
		.into_iter()
		.find_map(|key| context.theme.text(key).filter(|color| !color.is_empty()))
		.unwrap_or_else(|| "#fff".to_owned());
	let text_color = context.theme.css("textColor");
	let passes: &[Option<&str>] =
		if is_outlined { &[Some("sankey-label-bg"), Some("sankey-label-fg")] } else { &[None] };
	for class in passes {
		for (content, x, y, anchor) in &labels {
			let run = TextRun {
				x: *x,
				y: *y,
				anchor: *anchor,
				baseline: Baseline::Alphabetic,
				font: &font,
				content,
			};
			body.text(run, |svg| match class {
				Some("sankey-label-bg") => {
					svg.attr("class", "sankey-label-bg")
						.attr_if("stroke", &background)
						.attr("stroke-width", "4px")
						.attr("stroke-linejoin", "round")
						.attr("paint-order", "stroke");
				}
				Some(class) => _ = svg.attr("class", class).attr_if("fill", &text_color),
				None => {}
			});
		}
	}
	body.close_tag("g");

	body.open("g").attr("class", "links").attr("fill", "none").attr("stroke-opacity", "0.5").end();
	for (index, link) in graph.links.iter().enumerate() {
		let (source, target) = (&graph.nodes[link.source], &graph.nodes[link.target]);
		body.open("g").attr("class", "link").attr("style", "mix-blend-mode: multiply;").end();
		let stroke = match link_color.as_deref() {
			None | Some("gradient") => {
				let id = format!("{}-linearGradient-{}", context.id, index + 1);
				body.open("linearGradient")
					.attr("id", &id)
					.attr("gradientUnits", "userSpaceOnUse")
					.number_attr("x1", source.x1)
					.number_attr("x2", target.x0)
					.end();
				body.open("stop")
					.attr("offset", "0%")
					.attr("stop-color", &colors[link.source])
					.close();
				body.open("stop")
					.attr("offset", "100%")
					.attr("stop-color", &colors[link.target])
					.close();
				body.close_tag("linearGradient");
				format!("url(#{id})")
			}
			Some("source") => colors[link.source].clone(),
			Some("target") => colors[link.target].clone(),
			Some(color) => color.to_owned(),
		};
		let (x0, y0, x1, y1) = (source.x1, link.y0, target.x0, link.y1);
		let points = [x0, y0, x1, y1];
		body.open("path");
		if points.iter().all(|value| value.is_finite()) {
			let middle = (x0 + x1) / 2.0;
			body.attr(
				"d",
				&format!(
					"M{},{}C{},{},{},{},{},{}",
					number(x0),
					number(y0),
					number(middle),
					number(y0),
					number(middle),
					number(y1),
					number(x1),
					number(y1)
				),
			);
			bbox.include(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
		}
		body.attr_if("stroke", &stroke);
		if !link.width.is_nan() {
			body.number_attr("stroke-width", link.width.max(1.0));
		}
		body.close();
		body.close_tag("g");
	}
	body.close_tag("g");

	let (left, top, right, bottom) = bbox.extent.unwrap_or_default();
	let frame = Frame { x: left, y: top, width: right - left, height: bottom - top, use_max_width };
	Ok(document(context, "sankey", &frame, &Accessibility::default(), body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<Graph> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_csv_like_jison() {
		let graph =
			parsed("sankey-beta\n\n  A ,\"B, \"\"b\"\"\",1.5\r\n\n\nA,C,12abc\n\"x\n\ny\",C,x\n")
				.unwrap();
		let ids: Vec<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
		assert_eq!(ids, ["A", "B, \"b\"", "C", "x\ny"]);
		assert_eq!(graph.links[0].value, 1.5);
		assert_eq!(graph.links[1].value, 12.0);
		assert!(graph.links[2].value.is_nan());
		for code in [
			"sankey\nMüll,B,1",
			"sankey\nA,B,1\n   \nC,D,2",
			"sankey\nA,,1",
			"sankey\nA,\"\",1",
			"sankey\nA,B,1,2",
			"sankey\nA, \"B\",1",
			"sankey\nA,B#35;,1",
			"sankey A,B,1",
			"sankey\n",
		] {
			assert!(parsed(code).is_err(), "{code}");
		}
		let error = parsed("sankey\nA,B,1\n\nC,,2").err().unwrap();
		assert_eq!(error.line, Some(4));
	}

	fn laid_out(code: &str, alignment: Alignment) -> Graph {
		let mut graph = parsed(code).unwrap();
		let layout =
			Layout { width: 600.0, height: 400.0, node_width: 10.0, node_padding: 27.0, alignment };
		layout.run(&mut graph).unwrap();
		graph
	}

	#[test]
	fn lays_out_like_d3_sankey() {
		// Reference values from Mermaid 12.2.0 (`s04` with `nodeAlignment: right`).
		let graph =
			laid_out("sankey\nA,B,10\nA,C,5\nB,D,7\nC,D,3\nB,E,3\nE,F,2\nX,F,4", Alignment::Right);
		let node = |id: &str| graph.nodes.iter().find(|node| node.id == id).unwrap();
		assert!((node("B").x0 - 196.66666666666666).abs() < 1e-9);
		assert!((node("B").y0 - 127.70272352373331).abs() < 1e-9);
		assert!((node("A").y1 - 349.6875).abs() < 1e-9);
		assert!((node("E").y0 - 209.81250000000006).abs() < 1e-9);
		assert!((node("X").y0 - 306.75).abs() < 1e-9);
		assert!((node("F").y0 - 260.125).abs() < 1e-9);
	}

	#[test]
	fn rejects_circular_links() {
		let mut graph = parsed("sankey\nA,B,1\nB,A,2").unwrap();
		let layout = Layout {
			width: 600.0,
			height: 400.0,
			node_width: 10.0,
			node_padding: 27.0,
			alignment: Alignment::Justify,
		};
		assert_eq!(layout.run(&mut graph), Err("circular link"));
	}
}
