//! Treemaps (`treemap.langium`, `parser.ts`, `db.ts`, `utils.ts`, `renderer.ts`, `styles.ts`),
//! with d3-hierarchy's squarified `treemap` and d3-format's `format`.

mod parser;

use std::cmp::Ordering;
use std::collections::HashMap;

use self::parser::{TreeNode, build_hierarchy};
use super::color::js_round;
use super::common::{BlinkBox, stable_sort};
use super::number_format::NumberFormat;
use super::rendering::{Styles, is_label_style};
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::{Font, parse_font_size};
use super::value::Value;
use super::{Context, Diagram, Error, Result};

const SECTION: &str = "treemap";
const SECTION_INNER_PADDING: f64 = 10.0;
const SECTION_HEADER_HEIGHT: f64 = 25.0;

/// `Math.max`, which is `NaN` when either is.
fn js_max(first: f64, second: f64) -> f64 {
	if first.is_nan() || second.is_nan() { f64::NAN } else { first.max(second) }
}

/// JavaScript's truthiness of a number.
fn is_truthy(value: f64) -> bool {
	value != 0.0 && !value.is_nan()
}

/// A node of d3's hierarchy with its treemap cell.
#[derive(Clone, Debug, Default)]
struct Node {
	/// The row of the treemap, `None` for the root.
	data: Option<usize>,
	value: f64,
	children: Vec<usize>,
	parent: Option<usize>,
	depth: usize,
	x0: f64,
	y0: f64,
	x1: f64,
	y1: f64,
}

/// d3's `hierarchy(root).sum(…).sort(…)`: the nodes in preorder, root first.
fn hierarchy(rows: &[TreeNode], roots: &[usize]) -> Vec<Node> {
	let mut nodes = vec![Node::default()];
	let mut stack = vec![(0, roots.to_vec())];
	// Preorder: the children of a node follow it directly.
	let mut pending: Vec<(usize, Vec<usize>)> = Vec::new();
	while let Some((node, children)) = stack.pop() {
		for &row in &children {
			let child = nodes.len();
			let depth = nodes[node].depth + 1;
			nodes.push(Node { data: Some(row), parent: Some(node), depth, ..Node::default() });
			nodes[node].children.push(child);
			if let Some(grandchildren) = rows[row].children.as_ref().filter(|list| !list.is_empty())
			{
				pending.push((child, grandchildren.clone()));
			}
		}
		stack.extend(pending.drain(..).rev());
	}
	// `sum`: the own value (`NaN` counts as 0), then the children from the last.
	for index in (0..nodes.len()).rev() {
		let own = nodes[index].data.and_then(|row| rows[row].value).unwrap_or(0.0);
		let mut sum = if own.is_nan() { 0.0 } else { own };
		for &child in nodes[index].children.iter().rev() {
			sum += nodes[child].value;
		}
		nodes[index].value = sum;
	}
	// `sort`: larger values first, stable.
	for index in 0..nodes.len() {
		let mut children = std::mem::take(&mut nodes[index].children);
		stable_sort(&mut children, |a, b| {
			let difference = nodes[b].value - nodes[a].value;
			difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
		});
		nodes[index].children = children;
	}
	nodes
}

/// `treemapDice` (`is_dice`) and `treemapSlice` for a row of nodes worth `value`.
fn place_row(nodes: &mut [Node], row: &[usize], value: f64, is_dice: bool, bounds: [f64; 4]) {
	let [mut x0, mut y0, x1, y1] = bounds;
	let extent = if is_dice { x1 - x0 } else { y1 - y0 };
	// `parent.value && extent / parent.value`
	let factor = if value == 0.0 || value.is_nan() { value } else { extent / value };
	for &node in row {
		let node = &mut nodes[node];
		if is_dice {
			(node.y0, node.y1, node.x0) = (y0, y1, x0);
			x0 += node.value * factor;
			node.x1 = x0;
		} else {
			(node.x0, node.x1, node.y0) = (x0, x1, y0);
			y0 += node.value * factor;
			node.y1 = y0;
		}
	}
}

/// d3's `squarifyRatio` with the golden ratio.
fn squarify(nodes: &mut [Node], parent: usize, bounds: [f64; 4]) {
	let ratio = (1.0 + 5f64.sqrt()) / 2.0;
	let [mut x0, mut y0, x1, y1] = bounds;
	let children = nodes[parent].children.clone();
	let count = children.len();
	let mut value = nodes[parent].value;
	let (mut start, mut end) = (0, 0);
	while start < count {
		let (dx, dy) = (x1 - x0, y1 - y0);
		// The next node that is not empty.
		let mut sum;
		loop {
			sum = nodes[children[end]].value;
			end += 1;
			if is_truthy(sum) || end >= count {
				break;
			}
		}
		let (mut min_value, mut max_value) = (sum, sum);
		let alpha = js_max(dy / dx, dx / dy) / (value * ratio);
		let mut beta = sum * sum * alpha;
		let mut min_ratio = js_max(max_value / beta, beta / min_value);
		while end < count {
			let node_value = nodes[children[end]].value;
			sum += node_value;
			if node_value < min_value {
				min_value = node_value;
			}
			if node_value > max_value {
				max_value = node_value;
			}
			beta = sum * sum * alpha;
			let new_ratio = js_max(max_value / beta, beta / min_value);
			if new_ratio > min_ratio {
				sum -= node_value;
				break;
			}
			min_ratio = new_ratio;
			end += 1;
		}
		let row = &children[start..end];
		// The row takes a strip of the remaining area; the arguments are read before it moves.
		if dx < dy {
			let top = y0;
			let bottom = match is_truthy(value) {
				true => {
					y0 += dy * sum / value;
					y0
				}
				false => y1,
			};
			place_row(nodes, row, sum, true, [x0, top, x1, bottom]);
		} else {
			let left = x0;
			let right = match is_truthy(value) {
				true => {
					x0 += dx * sum / value;
					x0
				}
				false => x1,
			};
			place_row(nodes, row, sum, false, [left, y0, right, y1]);
		}
		value -= sum;
		start = end;
	}
}

/// d3's `treemap()` with Mermaid's paddings, rounded.
fn layout(nodes: &mut [Node], width: f64, height: f64, inner_padding: f64) {
	(nodes[0].x0, nodes[0].y0, nodes[0].x1, nodes[0].y1) = (0.0, 0.0, width, height);
	// Preorder, so parents are placed before their children.
	for index in 0..nodes.len() {
		let outer = if nodes[index].depth == 0 { 0.0 } else { inner_padding / 2.0 };
		let node = &mut nodes[index];
		let (mut x0, mut y0, mut x1, mut y1) =
			(node.x0 + outer, node.y0 + outer, node.x1 - outer, node.y1 - outer);
		if x1 < x0 {
			(x0, x1) = ((x0 + x1) / 2.0, (x0 + x1) / 2.0);
		}
		if y1 < y0 {
			(y0, y1) = ((y0 + y1) / 2.0, (y0 + y1) / 2.0);
		}
		(node.x0, node.y0, node.x1, node.y1) = (x0, y0, x1, y1);
		if node.children.is_empty() {
			continue;
		}
		let inner = inner_padding / 2.0;
		x0 += SECTION_INNER_PADDING - inner;
		y0 += SECTION_HEADER_HEIGHT + SECTION_INNER_PADDING - inner;
		x1 -= SECTION_INNER_PADDING - inner;
		y1 -= SECTION_INNER_PADDING - inner;
		if x1 < x0 {
			(x0, x1) = ((x0 + x1) / 2.0, (x0 + x1) / 2.0);
		}
		if y1 < y0 {
			(y0, y1) = ((y0 + y1) / 2.0, (y0 + y1) / 2.0);
		}
		squarify(nodes, index, [x0, y0, x1, y1]);
	}
	for node in nodes.iter_mut() {
		(node.x0, node.y0, node.x1, node.y1) =
			(js_round(node.x0), js_round(node.y0), js_round(node.x1), js_round(node.y1));
	}
}

/// d3's `scaleOrdinal` with a range: each new name takes the next value.
struct OrdinalScale {
	range: Vec<String>,
	domain: HashMap<String, usize>,
}

impl OrdinalScale {
	fn new(range: Vec<String>) -> Self {
		OrdinalScale { range, domain: HashMap::new() }
	}

	fn get(&mut self, name: &str) -> String {
		let next = self.domain.len();
		let index = *self.domain.entry(name.to_owned()).or_insert(next);
		self.range[index % self.range.len()].clone()
	}
}

/// The renderer's `valueFormat`, with its special cases for dollar amounts.
enum ValueFormat {
	Plain(NumberFormat),
	Dollar(NumberFormat),
	/// A `$` format whose rest d3 rejects, which fails when a value is formatted.
	Invalid(String),
}

impl ValueFormat {
	fn new(context: &Context) -> Self {
		let fallback = || {
			NumberFormat::new(",").map_or(ValueFormat::Invalid(",".to_owned()), ValueFormat::Plain)
		};
		let specifier = match context.config_value(SECTION, "valueFormat") {
			None | Some(Value::Null) => ",".to_owned(),
			Some(Value::String(text)) if text.is_empty() => ",".to_owned(),
			Some(Value::String(text)) => text.clone(),
			// `startsWith` of a number throws, and the renderer falls back to `,`.
			Some(_) => return fallback(),
		};
		let dollar = |rest: &str| match NumberFormat::new(rest) {
			Some(format) => ValueFormat::Dollar(format),
			None => ValueFormat::Invalid(rest.to_owned()),
		};
		if specifier == "$0,0" {
			return dollar(",");
		}
		if let Some(rest) = specifier.strip_prefix('$') {
			if specifier.contains(',') {
				// `/\.\d+/.exec(formatStr)`
				let precision = specifier.match_indices('.').find_map(|(dot, _)| {
					let digits =
						specifier[dot + 1..].bytes().take_while(u8::is_ascii_digit).count();
					(digits > 0).then(|| &specifier[dot..dot + 1 + digits])
				});
				return dollar(&format!(",{}", precision.unwrap_or_default()));
			}
			return dollar(rest);
		}
		NumberFormat::new(&specifier).map_or_else(fallback, ValueFormat::Plain)
	}

	fn format(&self, value: f64, prefix_exponent: &mut Option<i32>) -> Result<String> {
		match self {
			ValueFormat::Plain(format) => Ok(format.format(value, prefix_exponent)),
			ValueFormat::Dollar(format) => {
				Ok(format!("${}", format.format(value, prefix_exponent)))
			}
			ValueFormat::Invalid(specifier) => {
				Err(Error::new(format!("invalid format: {specifier}")))
			}
		}
	}
}

/// The label styles of a class: the paint and font of a text, and the rest as CSS.
#[derive(Default)]
struct TextStyle {
	fill: Option<String>,
	font_size: Option<f64>,
	family: Option<String>,
	weight: Option<u16>,
	is_italic: Option<bool>,
	css: String,
}

impl TextStyle {
	fn new(styles: &Styles) -> Self {
		let mut style = TextStyle::default();
		let mut css = Vec::new();
		for (key, value) in &styles.entries {
			if !is_label_style(key) {
				continue;
			}
			match key.as_str() {
				"color" => style.fill = Some(value.clone()),
				"font-size" => style.font_size = parse_font_size(value, 16.0),
				"font-family" => style.family = Some(value.clone()),
				"font-weight" => {
					style.weight = match value.as_str() {
						"bold" | "bolder" => Some(700),
						"normal" | "lighter" => Some(400),
						number => number.parse().ok(),
					}
				}
				"font-style" => style.is_italic = Some(value != "normal"),
				_ => css.push(format!("{key}:{value}")),
			}
		}
		style.css = css.join(";");
		style
	}

	fn font(&self, family: &str, size: f64, weight: u16, is_italic: bool) -> Font {
		let mut font = Font::new(self.family.clone().unwrap_or_else(|| family.to_owned()), size);
		font.weight = self.weight.unwrap_or(weight);
		font.is_italic = self.is_italic.unwrap_or(is_italic);
		font
	}
}

/// A text as it is drawn: content, font, position and paint.
struct Text {
	content: String,
	font: Font,
	x: f64,
	y: f64,
	anchor: Anchor,
	baseline: Baseline,
	fill: String,
	clip: Option<String>,
	css: String,
}

/// The constants of `renderer.ts` for simple and complex treemaps.
struct LeafLayout {
	is_complex: bool,
	label_font_size: f64,
	value_font_size: f64,
	min_label_font_size: f64,
	min_value_font_size: f64,
	label_padding: f64,
	min_display_threshold: f64,
	spacing: f64,
}

impl LeafLayout {
	fn new(leaf_count: usize) -> Self {
		let is_complex = leaf_count > 20;
		let pick = |complex: f64, simple: f64| if is_complex { complex } else { simple };
		LeafLayout {
			is_complex,
			label_font_size: pick(16.0, 38.0),
			value_font_size: pick(14.0, 28.0),
			min_label_font_size: pick(4.0, 8.0),
			min_value_font_size: pick(4.0, 6.0),
			label_padding: pick(2.0, 4.0),
			min_display_threshold: pick(8.0, 10.0),
			spacing: pick(1.0, 2.0),
		}
	}

	/// The value's font size for a label font size.
	fn value_size(&self, label_size: f64) -> f64 {
		f64::max(
			self.min_value_font_size,
			f64::min(self.value_font_size, js_round(label_size * 0.6)),
		)
	}
}

struct Renderer<'c, 'm> {
	context: &'c mut Context<'m>,
	bbox: BlinkBox,
}

impl Renderer<'_, '_> {
	/// `getComputedTextLength`
	fn text_length(&mut self, text: &str, font: &Font) -> f64 {
		self.context.measure(text, font).0
	}

	/// Includes `getBBox` of a text drawn at `offset` into the diagram's area.
	fn include_text(&mut self, text: &Text, offset: (f64, f64)) {
		if text.content.trim().is_empty() {
			return;
		}
		let context = &mut *self.context;
		// Blink's layout units: 1/64 pixel, rounded up.
		let advance = (context.measure(&text.content, &text.font).0 * 64.0 - 0.001).ceil() / 64.0;
		let (width, height) = context.bounding_box(&text.content, &text.font);
		let left = context.bounding_box_left(&text.content, &text.font);
		let ascent = context.vertical_metrics(&text.font).0.round();
		let top = context.bounding_box_top(&text.content, &text.font);
		let start = match text.anchor {
			Anchor::Start => text.x,
			Anchor::Middle => text.x - advance / 2.0,
			Anchor::End => text.x - advance,
		};
		let baseline = match text.baseline {
			Baseline::Middle => text.y + context.x_height(&text.font) / 2.0,
			Baseline::Hanging => text.y + (ascent * 0.8 * 64.0).round() / 64.0,
			_ => text.y,
		};
		self.bbox.include(offset.0 + start + left, offset.1 + baseline + top, width, height);
	}

	fn write_text(svg: &mut Svg, text: &Text) {
		let run = TextRun {
			x: text.x,
			y: text.y,
			anchor: text.anchor,
			baseline: text.baseline,
			font: &text.font,
			content: &text.content,
		};
		// The clip path goes on a group: the glyph outlines of PNGs replace the text in other
		// coordinates.
		if let Some(clip) = &text.clip {
			svg.open("g").attr("clip-path", &format!("url(#{clip})")).end();
		}
		svg.text(run, |svg| {
			svg.attr_if("fill", &text.fill);
			svg.attr_if("style", &text.css);
		});
		if text.clip.is_some() {
			svg.close_tag("g");
		}
	}

	/// The section label, cut off with `...` where it does not fit the header.
	fn section_label(&mut self, name: &str, font: &Font, available: f64) -> String {
		if self.text_length(name, font) <= available {
			return name.to_owned();
		}
		// JavaScript cuts the name in UTF-16 code units.
		let units: Vec<u16> = name.encode_utf16().collect();
		let candidate = |length: usize| String::from_utf16_lossy(&units[..length]) + "...";
		// Mermaid tries every shorter length from the longest. The widths grow with the length,
		// so start a little above the longest that fits.
		let mut start = units.len().saturating_sub(1);
		if units.len() > 64 {
			let (mut low, mut high) = (0, start);
			while low < high {
				let middle = (low + high).div_ceil(2);
				match self.text_length(&candidate(middle), font) <= available {
					true => low = middle,
					false => high = middle - 1,
				}
			}
			start = (low + 8).min(start);
		}
		for length in (1..=start).rev() {
			let text = candidate(length);
			if self.text_length(&text, font) <= available {
				return text;
			}
		}
		match self.text_length("...", font) <= available {
			true => "...".to_owned(),
			false => String::new(),
		}
	}
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let treemap = parser::parse(code, context)?;
	let (rows, roots) = build_hierarchy(&treemap);
	let config_number = |key: &str, default: f64| context.config_number(SECTION, key, default);
	let inner_padding = config_number("padding", 10.0);
	let diagram_padding = config_number("diagramPadding", 8.0);
	let show_values = context.config_bool(SECTION, "showValues", true);
	let use_max_width = context.config_bool(SECTION, "useMaxWidth", true);
	let width = match config_number("nodeWidth", 100.0) {
		0.0 => 960.0,
		node_width => node_width * SECTION_INNER_PADDING,
	};
	let height = match config_number("nodeHeight", 40.0) {
		0.0 => 500.0,
		node_height => node_height * SECTION_INNER_PADDING,
	};
	let title = treemap.fields.title.clone().unwrap_or_default();
	let title_height = if title.is_empty() { 0.0 } else { 30.0 };
	let value_format = ValueFormat::new(context);
	let mut prefix_exponent = None;

	let mut nodes = hierarchy(&rows, &roots);
	layout(&mut nodes, width, height, inner_padding);
	// `descendants()` is breadth first, `leaves()` preorder.
	let mut breadth_first: Vec<usize> = Vec::with_capacity(nodes.len());
	let mut level = vec![0];
	while !level.is_empty() {
		breadth_first.extend(&level);
		level = level.iter().flat_map(|&node| nodes[node].children.iter().copied()).collect();
	}
	let sections: Vec<usize> =
		breadth_first.iter().copied().filter(|&node| !nodes[node].children.is_empty()).collect();
	let mut leaves = Vec::new();
	let mut stack = vec![0];
	while let Some(node) = stack.pop() {
		match nodes[node].children.is_empty() {
			true => leaves.push(node),
			false => stack.extend(nodes[node].children.iter().rev()),
		}
	}
	let name = |node: usize| nodes[node].data.map_or("", |row| rows[row].name.as_str());
	let styles_of = |node: usize| {
		let styles = nodes[node].data.and_then(|row| rows[row].styles.clone()).unwrap_or_default();
		Styles::compile(&[&styles])
	};

	let theme_list = |prefix: &str, count: usize| -> Vec<String> {
		(0..count)
			.map(|index| context.theme.text(&format!("{prefix}{index}")).unwrap_or_default())
			.collect()
	};
	let mut fill_scale = OrdinalScale::new(
		std::iter::once("transparent".to_owned()).chain(theme_list("cScale", 12)).collect(),
	);
	let mut stroke_scale = OrdinalScale::new(
		std::iter::once("transparent".to_owned()).chain(theme_list("cScalePeer", 12)).collect(),
	);
	let mut label_scale = OrdinalScale::new(theme_list("cScaleLabel", 12));
	let treemap_theme = context.theme.get(SECTION).cloned();
	let theme_option = |key: &str| {
		treemap_theme.as_ref().and_then(|theme| theme.get(key)).and_then(Value::to_text)
	};
	let title_color =
		theme_option("titleColor").or_else(|| context.theme.text("titleColor")).unwrap_or_default();
	let title_font_size =
		theme_option("titleFontSize").and_then(|size| parse_font_size(&size, 16.0)).unwrap_or(14.0);
	let id = context.id.clone();

	let family = context.font_family();
	let mut renderer = Renderer { context, bbox: BlinkBox::default() };
	let mut body = Svg::new();
	if !title.is_empty() {
		let text = Text {
			content: title.clone(),
			font: Font::new(family.clone(), title_font_size),
			x: width / 2.0,
			y: title_height / 2.0,
			anchor: Anchor::Middle,
			baseline: Baseline::Middle,
			fill: title_color,
			clip: None,
			css: String::new(),
		};
		renderer.include_text(&text, (0.0, 0.0));
		Renderer::write_text(&mut body, &text);
	}
	body.open("g")
		.attr("transform", &format!("translate(0, {})", number(title_height)))
		.attr("class", "treemapContainer")
		.end();

	// The scales take their names in the order Mermaid draws: section fills, section strokes,
	// section labels, then leaves.
	let section_fills: Vec<String> =
		sections.iter().map(|&node| fill_scale.get(name(node))).collect();
	let section_strokes: Vec<String> =
		sections.iter().map(|&node| stroke_scale.get(name(node))).collect();
	let section_label_colors: Vec<String> = sections
		.iter()
		.map(
			|&node| {
				if nodes[node].depth == 0 { String::new() } else { label_scale.get(name(node)) }
			},
		)
		.collect();
	for (index, &node) in sections.iter().enumerate() {
		let cell = &nodes[node];
		let (x0, y0, cell_width, cell_height) =
			(cell.x0, cell.y0, cell.x1 - cell.x0, cell.y1 - cell.y0);
		body.open("g")
			.attr("class", "treemapSection")
			.attr("transform", &format!("translate({},{})", number(x0), number(y0)))
			.end();
		if cell.depth == 0 {
			// Hidden, but its value is formatted like the others.
			if show_values && is_truthy(cell.value) {
				value_format.format(cell.value, &mut prefix_exponent)?;
			}
			body.close_tag("g");
			continue;
		}
		let offset = (x0, y0 + title_height);
		let styles = styles_of(node);
		let text_style = TextStyle::new(&styles);
		body.open("rect")
			.number_attr("width", cell_width)
			.number_attr("height", SECTION_HEADER_HEIGHT)
			.attr("class", "treemapSectionHeader")
			.attr("fill", "none")
			.attr("fill-opacity", "0.6")
			.attr("stroke-width", "0.6")
			.close();
		renderer.bbox.include(offset.0, offset.1, cell_width, SECTION_HEADER_HEIGHT);
		let clip = format!("clip-section-{id}-{index}");
		body.open("clipPath").attr("id", &clip).end();
		body.open("rect")
			.number_attr("width", f64::max(0.0, cell_width - 12.0))
			.number_attr("height", SECTION_HEADER_HEIGHT)
			.close();
		body.close_tag("clipPath");
		body.open("rect")
			.number_attr("width", cell_width)
			.number_attr("height", cell_height)
			.attr("class", &format!("treemapSection section{index}"))
			.attr_if("fill", &section_fills[index])
			.attr("fill-opacity", "0.6")
			.attr_if("stroke", &section_strokes[index])
			.attr("stroke-width", "2")
			.attr("stroke-opacity", "0.4")
			.attr_if("style", &styles.node_css())
			.close();
		renderer.bbox.include(offset.0, offset.1, cell_width, cell_height);
		let fill = text_style.fill.clone().unwrap_or_else(|| section_label_colors[index].clone());
		let label_font = text_style.font(&family, text_style.font_size.unwrap_or(12.0), 700, false);
		let has_value = show_values && is_truthy(cell.value);
		let space = match has_value {
			true => cell_width - 10.0 - 30.0 - 10.0 - 6.0,
			false => cell_width - 6.0 - 6.0,
		};
		let content = renderer.section_label(name(node), &label_font, f64::max(15.0, space));
		let label = Text {
			content,
			font: label_font,
			x: 6.0,
			y: SECTION_HEADER_HEIGHT / 2.0,
			anchor: Anchor::Start,
			baseline: Baseline::Middle,
			fill: fill.clone(),
			clip: Some(clip),
			css: text_style.css.clone(),
		};
		renderer.include_text(&label, offset);
		if !label.content.is_empty() {
			Renderer::write_text(&mut body, &label);
		}
		if show_values {
			let content = match is_truthy(cell.value) {
				true => value_format.format(cell.value, &mut prefix_exponent)?,
				false => String::new(),
			};
			let value = Text {
				content,
				font: text_style.font(&family, text_style.font_size.unwrap_or(10.0), 400, true),
				x: cell_width - 10.0,
				y: SECTION_HEADER_HEIGHT / 2.0,
				anchor: Anchor::End,
				baseline: Baseline::Middle,
				fill,
				clip: None,
				css: text_style.css.clone(),
			};
			renderer.include_text(&value, offset);
			if !value.content.is_empty() {
				Renderer::write_text(&mut body, &value);
			}
		}
		body.close_tag("g");
	}

	let leaf_layout = LeafLayout::new(leaves.len());
	for (index, &node) in leaves.iter().enumerate() {
		let cell = &nodes[node];
		let (x0, y0, cell_width, cell_height) =
			(cell.x0, cell.y0, cell.x1 - cell.x0, cell.y1 - cell.y0);
		let offset = (x0, y0 + title_height);
		let class_selector = cell.data.and_then(|row| rows[row].class_selector.as_deref());
		let class = match class_selector {
			Some(selector) => format!("treemapNode treemapLeafGroup leaf{index} {selector}x"),
			None => format!("treemapNode treemapLeafGroup leaf{index}x"),
		};
		body.open("g")
			.attr("class", &class)
			.attr("transform", &format!("translate({},{})", number(x0), number(y0)))
			.end();
		let color = fill_scale.get(cell.parent.map_or(name(node), name));
		let styles = styles_of(node);
		let text_style = TextStyle::new(&styles);
		body.open("rect")
			.number_attr("width", cell_width)
			.number_attr("height", cell_height)
			.attr("class", "treemapLeaf")
			.attr_if("fill", &color)
			.attr_if("style", &styles.node_css())
			.attr("fill-opacity", "0.3")
			.attr_if("stroke", &color)
			.attr("stroke-width", "3")
			.close();
		renderer.bbox.include(offset.0, offset.1, cell_width, cell_height);
		let clip = format!("clip-{id}-{index}");
		body.open("clipPath").attr("id", &clip).end();
		body.open("rect")
			.number_attr("width", f64::max(0.0, cell_width - 4.0))
			.number_attr("height", f64::max(0.0, cell_height - 4.0))
			.close();
		body.close_tag("clipPath");
		// The scale takes the name even where a class sets the color.
		let scale_color = label_scale.get(name(node));
		let fill = text_style.fill.clone().unwrap_or(scale_color);

		// Fit the label: smaller until it fits the width, then until label and value fit the
		// height.
		let leaf = &leaf_layout;
		let available_width = cell_width - 2.0 * leaf.label_padding;
		let available_height = cell_height - 2.0 * leaf.label_padding;
		let label_name = name(node).to_owned();
		let mut label_size = None;
		if available_width >= leaf.min_display_threshold
			&& available_height >= leaf.min_display_threshold
		{
			let initial = text_style.font_size.unwrap_or(leaf.label_font_size);
			let mut size = initial.trunc();
			let mut measured = initial;
			let font = |size: f64| text_style.font(&family, size, 400, false);
			while renderer.text_length(&label_name, &font(measured)) > available_width
				&& size > leaf.min_label_font_size
			{
				size -= 1.0;
				measured = size;
			}
			let mut combined = size + leaf.spacing + leaf.value_size(size);
			while combined > available_height && size > leaf.min_label_font_size {
				size -= 1.0;
				combined = size + leaf.spacing + leaf.value_size(size);
			}
			let is_hidden = match leaf.is_complex {
				true => {
					size < leaf.min_label_font_size || available_height < leaf.min_label_font_size
				}
				false => {
					renderer.text_length(&label_name, &font(size)) > available_width
						|| size < leaf.min_label_font_size
						|| available_height < size
				}
			};
			if !is_hidden {
				label_size = Some(size);
			}
		}
		if let Some(size) = label_size {
			let label = Text {
				content: label_name,
				font: text_style.font(&family, size, 400, false),
				x: cell_width / 2.0,
				y: cell_height / 2.0,
				anchor: Anchor::Middle,
				baseline: Baseline::Middle,
				fill: fill.clone(),
				clip: Some(clip.clone()),
				css: text_style.css.clone(),
			};
			renderer.include_text(&label, offset);
			Renderer::write_text(&mut body, &label);
		}
		if show_values {
			let content = match is_truthy(cell.value) {
				true => value_format.format(cell.value, &mut prefix_exponent)?,
				false => String::new(),
			};
			if let Some(label_size) = label_size {
				let value_size = leaf.value_size(label_size);
				let y = cell_height / 2.0 + label_size / 2.0 + leaf.spacing;
				let font = text_style.font(&family, value_size, 400, false);
				let is_hidden = renderer.text_length(&content, &font) > available_width
					|| y + value_size > cell_height - 4.0
					|| value_size < leaf.min_value_font_size;
				let value = Text {
					content,
					font,
					x: cell_width / 2.0,
					y,
					anchor: Anchor::Middle,
					baseline: Baseline::Hanging,
					fill,
					clip: Some(clip),
					css: text_style.css.clone(),
				};
				if !is_hidden && !value.content.is_empty() {
					renderer.include_text(&value, offset);
					Renderer::write_text(&mut body, &value);
				}
			}
		}
		body.close_tag("g");
	}
	body.close_tag("g");

	let (x, y, right, bottom) = renderer.bbox.extent.unwrap_or_default();
	let frame = Frame {
		x: x - diagram_padding,
		y: y - diagram_padding,
		width: right - x + diagram_padding * 2.0,
		height: bottom - y + diagram_padding * 2.0,
		use_max_width,
	};
	Ok(document(renderer.context, "treemap", &frame, &treemap.fields.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn cells(values: &[f64]) -> Vec<(f64, f64, f64, f64)> {
		let rows: Vec<TreeNode> = values
			.iter()
			.map(|&value| TreeNode {
				name: String::new(),
				children: None,
				value: Some(value),
				class_selector: None,
				styles: None,
			})
			.collect();
		let roots: Vec<usize> = (0..rows.len()).collect();
		let mut nodes = hierarchy(&rows, &roots);
		layout(&mut nodes, 1000.0, 400.0, 10.0);
		nodes.iter().skip(1).map(|node| (node.x0, node.y0, node.x1, node.y1)).collect()
	}

	#[test]
	fn lays_out_like_d3() {
		// Sorted by value: 10 first; the root's header takes 35 px.
		assert_eq!(cells(&[5.0, 10.0]), [(670.0, 35.0, 990.0, 390.0), (10.0, 35.0, 660.0, 390.0)]);
		assert_eq!(cells(&[0.0, 0.0]), [(10.0, 30.0, 990.0, 30.0), (10.0, 30.0, 990.0, 30.0)]);
	}

	#[test]
	fn assigns_ordinal_colors_in_order() {
		let mut scale = OrdinalScale::new(vec!["a".into(), "b".into()]);
		assert_eq!(
			[scale.get("x"), scale.get("y"), scale.get("x"), scale.get("z")],
			["a", "b", "a", "a"]
		);
	}
}
