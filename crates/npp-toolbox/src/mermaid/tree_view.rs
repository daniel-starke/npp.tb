//! Tree views (`treeView.langium`, `parser.ts`, `boxDrawingPreprocessor.ts`, `db.ts`,
//! `renderer.ts`, `icons.ts`, `styles.ts`).
//!
//! Icons of Iconify packs are not loaded, so `icon(pack:name)` draws Mermaid's unknown icon like
//! an unregistered pack does.

mod parser;

use self::parser::{Node, NodeType, Tree, parse};
use super::icons::{self, IconGlyph, write_unknown_icon};
use super::journey::css_font_size;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Result};

const SECTION: &str = "treeView";
const ICON_SIZE: f64 = 14.0;
const ICON_GAP: f64 = 4.0;
const DESCRIPTION_GAP: f64 = 16.0;

const FOLDER_ICON: &str = "M10.59 4.59A2 2 0 0 0 9.17 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.17z";
const FILE_ICON: &str = "M6 2a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8.83a2 2 0 0 0-.59-1.42l-4.82-4.82A2 2 0 0 0 13.17 2H6Zm7.5 1.9l4.6 4.6h-3.6a1 1 0 0 1-1-1V3.9Z";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Icon {
	Folder,
	File,
	/// An icon of a registered pack.
	Glyph(IconGlyph),
	/// An icon of a pack that is not registered.
	Unknown,
}

struct Settings {
	row_indent: f64,
	padding_x: f64,
	padding_y: f64,
	line_thickness: f64,
	show_icons: bool,
	default_icon_pack: String,
}

/// `qualifyIcon` and `getIconSVG`: built-in names win, then the `defaultIconPack`.
fn resolve_icon(icon: &str, default_icon_pack: &str) -> Icon {
	let qualified = match icon.contains(':') {
		true => icon.to_owned(),
		false if icon == "folder" || icon == "file" || default_icon_pack.is_empty() => {
			format!("mermaid-treeview:{icon}")
		}
		false => format!("{default_icon_pack}:{icon}"),
	};
	match qualified.as_str() {
		"mermaid-treeview:folder" => Icon::Folder,
		"mermaid-treeview:file" => Icon::File,
		_ => icons::registered_glyph(&qualified, None).map_or(Icon::Unknown, Icon::Glyph),
	}
}

/// `detectIcon`: the configured icon of a file name, then of its extension.
fn detect_icon(context: &Context, name: &str) -> Option<String> {
	let map_value = |key: &str, map: &str| {
		context.config_value(SECTION, map).and_then(|icons| icons.get(key)).and_then(Value::to_text)
	};
	if let Some(icon) = map_value(name, "filenameIcons").filter(|icon| !icon.is_empty()) {
		return Some(icon);
	}
	let dot = name.rfind('.').filter(|&dot| dot > 0)?;
	let extension = name[dot..].to_lowercase();
	map_value(&extension, "extensionIcons").or_else(|| map_value(&extension[1..], "extensionIcons"))
}

/// `getNodeIcon`: an explicit `icon(…)` always, the defaults only with `showIcons`.
fn node_icon(context: &Context, settings: &Settings, node: &Node) -> Option<Icon> {
	match node.icon.as_deref() {
		Some("none") => return None,
		Some(icon) => return Some(resolve_icon(icon, &settings.default_icon_pack)),
		None if !settings.show_icons => return None,
		None => {}
	}
	if node.node_type == NodeType::File {
		match detect_icon(context, &node.name).as_deref() {
			Some("none") => return None,
			Some(icon) if !icon.is_empty() => {
				return Some(resolve_icon(icon, &settings.default_icon_pack));
			}
			_ => {}
		}
	}
	Some(match node.node_type {
		NodeType::Directory => Icon::Folder,
		NodeType::File => Icon::File,
	})
}

/// A row as `positionLabel` lays it out.
struct Row {
	node: usize,
	x: f64,
	y: f64,
	height: f64,
	icon: Option<Icon>,
	label_x: f64,
	label_right_edge: f64,
	description_width: f64,
	highlight_width: Option<f64>,
}

/// The drawing order of `drawTree`: rows and the lines between them.
enum Item {
	Row(usize),
	Line { x1: f64, y1: f64, x2: f64, y2: f64 },
}

/// The style options of `styles.ts`, `themeVariables.treeView` over the defaults.
fn style_option(context: &Context, key: &str, default: &str) -> String {
	context
		.theme
		.get("treeView")
		.and_then(|tree_view| tree_view.get(key))
		.and_then(Value::to_text)
		.unwrap_or_else(|| default.to_owned())
}

fn label_font(context: &mut Context, node_type: NodeType) -> Font {
	let root = Font::new(context.font_family(), context.font_size());
	let size_text = style_option(context, "labelFontSize", "16px");
	let size = css_font_size(context, &size_text, &root).unwrap_or(root.size);
	let mut font = Font::new(root.family, size);
	if node_type == NodeType::Directory {
		font.weight = 700;
	}
	font
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let Tree { fields, nodes } = parse(code, context)?;
	let config = |key: &str, default: f64| context.config_number(SECTION, key, default);
	let settings = Settings {
		row_indent: config("rowIndent", 10.0),
		padding_x: config("paddingX", 5.0),
		padding_y: config("paddingY", 5.0),
		line_thickness: config("lineThickness", 1.0),
		show_icons: context.config_bool(SECTION, "showIcons", false),
		default_icon_pack: context.config_text(SECTION, "defaultIconPack").unwrap_or_default(),
	};
	let file_font = label_font(context, NodeType::File);
	let directory_font = label_font(context, NodeType::Directory);
	let mut description_font = file_font.clone();
	description_font.is_italic = true;

	let mut rows: Vec<Row> = Vec::with_capacity(nodes.len());
	let mut items = Vec::with_capacity(nodes.len() * 2);
	let (mut total_width, mut total_height) = (0f64, 0f64);
	// `processNode` in preorder; each entry is a node, its depth and the next child to visit.
	let mut stack = vec![(0usize, 0usize, 0usize)];
	let mut row_of = vec![0usize; nodes.len()];
	while let Some(&(node, depth, next_child)) = stack.last() {
		if next_child == 0 {
			let data = &nodes[node];
			let x = depth as f64 * (settings.row_indent + settings.padding_x);
			let y = total_height;
			let icon = node_icon(context, &settings, data);
			let icon_offset = if icon.is_some() { ICON_SIZE + ICON_GAP } else { 0.0 };
			let font = match data.node_type {
				NodeType::Directory => &directory_font,
				NodeType::File => &file_font,
			};
			let (label_width, label_height) = context.bounding_box_with_spaces(&data.name, font);
			let height = label_height + settings.padding_y * 2.0;
			let label_x = x + settings.padding_x + icon_offset;
			let width = label_width + settings.padding_x * 2.0 + icon_offset;
			row_of[node] = rows.len();
			items.push(Item::Row(rows.len()));
			rows.push(Row {
				node,
				x,
				y,
				height,
				icon,
				label_x,
				label_right_edge: label_x + label_width,
				description_width: 0.0,
				highlight_width: None,
			});
			items.push(Item::Line {
				x1: x - settings.row_indent,
				y1: y + height / 2.0,
				x2: x,
				y2: y + height / 2.0,
			});
			total_width = total_width.max(x + width);
			total_height += height;
		}
		if let Some(&child) = nodes[node].children.get(next_child) {
			if let Some(top) = stack.last_mut() {
				top.2 += 1;
			}
			stack.push((child, depth + 1, 0));
			continue;
		}
		stack.pop();
		if let Some(&last) = nodes[node].children.last() {
			let (row, last) = (&rows[row_of[node]], &rows[row_of[last]]);
			items.push(Item::Line {
				x1: row.x + settings.padding_x,
				y1: row.y + row.height,
				x2: row.x + settings.padding_x,
				y2: last.y + last.height / 2.0 + settings.line_thickness / 2.0,
			});
		}
	}

	let max_label_right =
		rows.iter().map(|row| row.label_right_edge).fold(f64::NEG_INFINITY, f64::max);
	let description_x = max_label_right + DESCRIPTION_GAP;
	for row in &mut rows {
		if let Some(description) = &nodes[row.node].description {
			row.description_width =
				context.bounding_box_with_spaces(description, &description_font).0;
			total_width =
				total_width.max(description_x + row.description_width + settings.padding_x);
		}
	}
	for row in &mut rows {
		if nodes[row.node].class.as_deref() == Some("highlight") {
			let width = total_width - row.x + 8.0;
			row.highlight_width = Some(width);
			total_width = total_width.max(row.x + width + 2.0);
		}
	}

	let label_color = style_option(context, "labelColor", "black");
	let line_color = style_option(context, "lineColor", "black");
	let icon_color = style_option(context, "iconColor", "#546e7a");
	let description_color = style_option(context, "descriptionColor", "#6a9955");
	let highlight_background = style_option(context, "highlightBg", "rgba(255, 193, 7, 0.15)");
	let highlight_stroke = style_option(context, "highlightStroke", "#ffc107");

	let mut body = Svg::new();
	body.open("g").attr("class", "tree-view").end();
	for item in &items {
		let row = match item {
			Item::Line { x1, y1, x2, y2 } => {
				body.open("line")
					.number_attr("x1", *x1)
					.number_attr("y1", *y1)
					.number_attr("x2", *x2)
					.number_attr("y2", *y2)
					.number_attr("stroke-width", settings.line_thickness)
					.attr("class", "treeView-node-line")
					.attr_if("stroke", &line_color)
					.close();
				continue;
			}
			Item::Row(row) => &rows[*row],
		};
		let data = &nodes[row.node];
		body.open("g").end();
		if let Some(width) = row.highlight_width {
			body.open("rect")
				.number_attr("x", row.x)
				.number_attr("y", row.y + 1.0)
				.number_attr("width", width)
				.number_attr("height", row.height - 2.0)
				.attr("rx", "3")
				.attr("class", "treeView-highlight-bg")
				.attr_if("fill", &highlight_background)
				.attr_if("stroke", &highlight_stroke)
				.attr("stroke-width", "1")
				.close();
		}
		if let Some(icon) = row.icon {
			let transform = format!(
				"translate({}, {})",
				number(row.x + settings.padding_x),
				number(row.y + settings.padding_y)
			);
			body.open("g").attr("class", "treeView-node-icon").attr("transform", &transform).end();
			let view_box = match icon {
				Icon::Unknown => Some("0 0 80 80"),
				Icon::Glyph(_) => None,
				Icon::Folder | Icon::File => Some("0 0 24 24"),
			};
			if let Some(view_box) = view_box {
				body.open("svg")
					.attr("xmlns", "http://www.w3.org/2000/svg")
					.number_attr("width", ICON_SIZE)
					.number_attr("height", ICON_SIZE)
					.attr("viewBox", view_box)
					.end();
			}
			match icon {
				Icon::Glyph(glyph) => {
					icons::write_glyph_svg(&mut body, &glyph, ICON_SIZE, &|svg| {
						svg.attr_if("fill", &icon_color);
					});
				}
				Icon::Folder => {
					body.open("path").attr_if("fill", &icon_color).attr("d", FOLDER_ICON).close();
				}
				Icon::File => {
					body.open("path")
						.attr_if("fill", &icon_color)
						.attr("fill-rule", "evenodd")
						.attr("d", FILE_ICON)
						.attr("clip-rule", "evenodd")
						.close();
				}
				Icon::Unknown => {
					body.open("g").end();
					write_unknown_icon(&mut body);
					body.close_tag("g");
				}
			}
			if view_box.is_some() {
				body.close_tag("svg");
			}
			body.close_tag("g");
		}
		let mut class = "treeView-node-label".to_owned();
		if data.node_type == NodeType::Directory {
			class.push_str(" treeView-node-dir");
		}
		if let Some(own_class) = &data.class {
			class.push(' ');
			class.push_str(own_class);
		}
		let label = TextRun {
			x: row.label_x,
			y: row.y + row.height / 2.0,
			anchor: Anchor::Start,
			baseline: Baseline::Middle,
			font: match data.node_type {
				NodeType::Directory => &directory_font,
				NodeType::File => &file_font,
			},
			content: &data.name,
		};
		body.text_with_spaces(label, |svg| {
			svg.attr("class", &class).attr_if("fill", &label_color);
		});
		if let Some(description) = &data.description {
			let run = TextRun {
				x: description_x,
				y: row.y + row.height / 2.0,
				anchor: Anchor::Start,
				baseline: Baseline::Middle,
				font: &description_font,
				content: description,
			};
			body.text_with_spaces(run, |svg| {
				svg.attr("class", "treeView-node-description").attr_if("fill", &description_color);
			});
		}
		body.close_tag("g");
	}
	body.close_tag("g");
	let frame = Frame {
		x: -settings.line_thickness / 2.0,
		y: 0.0,
		width: total_width,
		height: total_height,
		use_max_width: context.config_bool(SECTION, "useMaxWidth", true),
	};
	Ok(document(context, "treeView", &frame, &fields.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::super::{Limits, Options};
	use super::*;

	fn svg(source: &str) -> String {
		let mut measure = FixedMeasure;
		let options = Options { is_dark: false, limits: Limits::default(), screen_width: 800 };
		super::super::render(source, &options, &mut measure).unwrap().svg
	}

	#[test]
	fn lays_out_rows_like_mermaid() {
		let svg = svg("treeView-beta\n  a/\n    b ## x\n  c :::highlight");
		// FixedMeasure: 0.6 em wide, 1.2 em high glyphs, so rows are 29.2 px high.
		assert!(svg.contains(r#"<line x1="-10" y1="14.6" x2="0" y2="14.6""#), "{svg}");
		assert!(svg.contains(r#"<line x1="20" y1="73" x2="30" y2="73""#), "{svg}");
		// The vertical line below `a` ends half a line thickness below the middle of `b`.
		assert!(svg.contains(r#"<line x1="20" y1="58.4" x2="20" y2="73.5""#), "{svg}");
		assert!(svg.contains(r#"x="60.6" y="73""#), "{svg}");
		assert!(svg.contains(r#"class="treeView-node-label treeView-node-dir""#), "{svg}");
		assert!(svg.contains(r#"<rect x="15" y="88.6" width="68.2" height="27.2""#), "{svg}");
	}

	#[test]
	fn resolves_icons_like_mermaid() {
		assert_eq!(resolve_icon("folder", "pack"), Icon::Folder);
		assert_eq!(resolve_icon("mermaid-treeview:file", ""), Icon::File);
		assert_eq!(resolve_icon("docker", "pack"), Icon::Unknown);
		#[cfg(feature = "font-awesome")]
		{
			let car = Icon::Glyph(icons::font_awesome("fa", "car").unwrap());
			assert_eq!(resolve_icon("fa:car", ""), car);
			assert_eq!(resolve_icon("car", "fa"), car);
		}
		assert_eq!(resolve_icon("foo:bar", ""), Icon::Unknown);
		let svg = svg(concat!(
			"---\nconfig:\n  treeView:\n    showIcons: true\n",
			"    extensionIcons: { ts: none }\n---\ntreeView-beta\n  a.ts\n  b icon(x:y)\n  c icon()"
		));
		assert_eq!(svg.matches("treeView-node-icon").count(), 2, "{svg}");
		assert!(svg.contains(r#"viewBox="0 0 80 80""#), "{svg}");
	}
}
