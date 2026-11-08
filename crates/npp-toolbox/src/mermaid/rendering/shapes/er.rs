//! The entity box of ER diagrams (`erBox.ts`): the name above a table of the attributes, drawn
//! with roughjs. Without roughness its rectangles and lines are a filled and a stroked outline.

use super::super::super::common::parse_generic_types;
use super::super::super::label::{self, LabelBox, Run, Style};
use super::super::super::svg::{Svg, number};
use super::super::super::text::Font;
use super::super::super::value::Value;
use super::super::super::{Context, Look};
use super::super::{Node, Styles, Theme, layout_label};
use super::{Intersect, Part, Point, Shape, centered_box, point};

/// What a part of the entity box is, for its paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErKind {
	Outer,
	RowOdd,
	RowEven,
	Divider,
}

/// An element of the entity box.
#[derive(Clone, Debug, PartialEq)]
pub enum ErPart {
	/// The rectangle of an entity without attributes (`drawRect`).
	Rect { x: f64, y: f64, width: f64, height: f64 },
	/// A roughjs rectangle or divider line: the closed outline of its corners.
	Rough { kind: ErKind, corners: [Point; 4] },
	/// A roughjs line: the divider of requirement boxes in the classic look.
	Line { from: Point, to: Point },
}

/// The laid out entity box: its shape, whose label is the name, and the attribute cells with
/// their centers.
pub struct ErBox {
	pub shape: Shape,
	pub name: LabelBox,
	pub cells: Vec<(LabelBox, Point)>,
}

/// JavaScript's truthiness of a configuration value.
fn is_truthy(value: Option<&Value>) -> bool {
	match value {
		None | Some(Value::Null) => false,
		Some(Value::Bool(value)) => *value,
		Some(Value::Number(number)) => *number != 0.0 && !number.is_nan(),
		Some(Value::String(text)) => !text.is_empty(),
		Some(_) => true,
	}
}

/// `addText`: a label that does not wrap. Generic types show their angle brackets: Mermaid
/// then replaces the content by its text, which drops the formatting and line breaks.
fn cell(context: &mut Context, text: &str, font: &Font) -> LabelBox {
	let parsed = parse_generic_types(text);
	let label = match parsed != text {
		true => {
			let escaped = parsed.replace('<', "&lt;").replace('>', "&gt;");
			let content: String = label::label_lines(&escaped, true)
				.iter()
				.flatten()
				.map(|run| run.text.as_str())
				.collect();
			let plain = content.split_whitespace().collect::<Vec<_>>().join(" ");
			let lines = vec![vec![Run { text: plain, style: Style::default() }]];
			label::layout(context, &lines, font, None, 0.0)
		}
		false => layout_label(context, &parsed, true, font, None, 0.0),
	};
	match label.is_empty() {
		// An empty label is an empty table cell without a line box.
		true => LabelBox { width: 0.0, height: 0.0, ..label },
		false => label,
	}
}

fn rectangle(x: f64, y: f64, width: f64, height: f64) -> [Point; 4] {
	[point(x, y), point(x + width, y), point(x + width, y + height), point(x, y + height)]
}

/// `lineToPolygon`: an axis-parallel line as a polygon of a tiny thickness.
fn line_polygon(from: Point, to: Point) -> [Point; 4] {
	const HALF_THICKNESS: f64 = 0.0001 / 2.0;
	if from.x == to.x {
		return [
			point(from.x - HALF_THICKNESS, from.y),
			point(from.x + HALF_THICKNESS, from.y),
			point(to.x + HALF_THICKNESS, to.y),
			point(to.x - HALF_THICKNESS, to.y),
		];
	}
	[
		point(from.x, from.y - HALF_THICKNESS),
		point(from.x, from.y + HALF_THICKNESS),
		point(to.x, to.y + HALF_THICKNESS),
		point(to.x, to.y - HALF_THICKNESS),
	]
}

/// Lays out the entity box of `node` with its labels in `font`.
pub fn er_box(context: &mut Context, node: &Node, font: &Font) -> ErBox {
	let mut padding = context.config_number("er", "diagramPadding", 20.0);
	let mut text_padding = context.config_number("er", "entityPadding", 15.0);
	if node.attributes.is_empty() && !node.label.is_empty() {
		return plain_box(context, node, font, padding);
	}
	if !is_truthy(context.config.get("htmlLabels")) {
		padding *= 1.25;
		text_padding *= 1.25;
	}
	let name = cell(context, &node.label, font);
	let name_height = name.height + text_padding;
	// The cells of each row and its offset and height.
	let mut rows: Vec<([LabelBox; 4], f64, f64)> = Vec::with_capacity(node.attributes.len());
	let mut column_widths = [0.0_f64; 4];
	let mut widest = name.width;
	let mut y_offset = 0.0;
	for attribute in &node.attributes {
		let cells = [
			cell(context, &attribute[0], font),
			cell(context, &attribute[1], font),
			cell(context, &attribute[2], font),
			cell(context, &attribute[3], font),
		];
		let mut row_height = 0.0_f64;
		for (width, cell) in column_widths.iter_mut().zip(&cells) {
			*width = width.max(cell.width + padding);
			row_height = row_height.max(cell.height);
			widest = widest.max(cell.width);
		}
		let row_height = row_height + text_padding;
		rows.push((cells, y_offset, row_height));
		y_offset += row_height;
	}
	let mut sections = 4.0;
	let has_keys = column_widths[2] > padding;
	if !has_keys {
		column_widths[2] = 0.0;
		sections -= 1.0;
	}
	let has_comments = column_widths[3] > padding;
	if !has_comments {
		column_widths[3] = 0.0;
		sections -= 1.0;
	}
	let difference = name.width + padding * 2.0 - column_widths.iter().sum::<f64>();
	if difference > 0.0 {
		for width in column_widths.iter_mut().filter(|width| **width > 0.0) {
			*width += difference / sections;
		}
		// The type and name columns grow even where they are empty.
		for width in column_widths.iter_mut().take(2).filter(|width| **width == 0.0) {
			*width += difference / sections;
		}
	}
	let total_width: f64 = column_widths.iter().sum();
	let rows_height: f64 = rows.iter().map(|(_, _, height)| height).sum();
	let width = (widest + padding * 2.0).max(total_width);
	let height = rows_height + name_height;
	let (x, y) = (-width / 2.0, -height / 2.0);
	let column_offsets = [
		0.0,
		column_widths[0],
		column_widths[0] + column_widths[1],
		column_widths[0] + column_widths[1] + column_widths[2],
	];
	let mut cells = Vec::with_capacity(rows.len() * 4);
	let mut parts = vec![Part::Er(ErPart::Rough {
		kind: ErKind::Outer,
		corners: rectangle(x, y, width, height),
	})];
	for (index, (row_cells, offset, row_height)) in rows.into_iter().enumerate() {
		let top = offset + y + name_height + text_padding / 2.0;
		for (cell, column_offset) in row_cells.into_iter().zip(column_offsets) {
			let left = x + padding / 2.0 + column_offset;
			let center = point(left + cell.width / 2.0, top + cell.height / 2.0);
			cells.push((cell, center));
		}
		let is_even = (index + 1) % 2 == 0 && offset != 0.0;
		parts.push(Part::Er(ErPart::Rough {
			kind: if is_even { ErKind::RowEven } else { ErKind::RowOdd },
			corners: rectangle(x, name_height + y + offset, width, row_height),
		}));
	}
	let top = y + name_height;
	let bottom = y + height;
	let mut lines = vec![(point(x, top), point(x + width, top))];
	let mut vertical = |column: f64| {
		let line_x = x + column;
		lines.push((point(line_x, top), point(line_x, bottom)));
	};
	vertical(column_offsets[1]);
	if has_keys {
		vertical(column_offsets[2]);
	}
	if has_comments {
		vertical(column_offsets[3]);
	}
	// The divider below the name once more, at the offset of the first row.
	lines.push((point(x, top), point(x + width, top)));
	for (from, to) in lines {
		parts.push(Part::Er(ErPart::Rough {
			kind: ErKind::Divider,
			corners: line_polygon(from, to),
		}));
	}
	let shape = Shape {
		parts,
		bbox: centered_box(width, height),
		intersect: Intersect::Rect,
		label_center: point(0.0, y + text_padding / 2.0 + name.height / 2.0),
	};
	ErBox { shape, name, cells }
}

/// An entity without attributes: `drawRect` around the name, at least `minEntityWidth` wide.
fn plain_box(context: &mut Context, node: &Node, font: &Font, padding: f64) -> ErBox {
	let min_width = context.config_number("er", "minEntityWidth", 100.0);
	let measure_font = context.configured_text_font();
	let text_width = context.text_dimensions(&node.label, &measure_font).0;
	let node_width = (text_width + padding * 2.0 < min_width).then_some(min_width);
	let wrap_width =
		node_width.unwrap_or_else(|| context.config_number("flowchart", "wrappingWidth", 120.0));
	let name = layout_label(context, &node.label, node.is_markdown, font, Some(wrap_width), 0.0);
	let width = (name.width + padding * 2.0).max(node_width.unwrap_or(0.0));
	let height = name.height + padding * 3.0;
	let shape = Shape {
		parts: vec![Part::Er(ErPart::Rect { x: -width / 2.0, y: -height / 2.0, width, height })],
		bbox: centered_box(width, height),
		intersect: Intersect::Rect,
		label_center: Point::default(),
	};
	ErBox { shape, name, cells: Vec::new() }
}

/// The color slot of an entity in the `redux-color` themes: its border and background.
fn slot_colors<'a>(node: &Node, theme: &'a Theme) -> Option<(&'a str, Option<&'a str>)> {
	let slot = theme.color_slot(node)?;
	let background = (!theme.background_colors.is_empty())
		.then(|| theme.background_colors[slot % theme.background_colors.len()].as_str());
	Some((theme.border_colors[slot].as_str(), background))
}

pub(super) fn corners_path(corners: &[Point], is_closed: bool) -> String {
	let mut d = String::new();
	for (index, corner) in corners.iter().enumerate() {
		let command = if index == 0 { "M" } else { " L" };
		d.push_str(&format!("{command}{} {}", number(corner.x), number(corner.y)));
	}
	if is_closed {
		d.push_str(" Z");
	}
	d
}

/// Writes a part of the entity box. The roughjs options give the paths their attributes; the
/// rules of the style sheet replace them: the neo look's stroke, the color slot of the
/// `redux-color` themes, and the user styles as `style` attribute.
pub fn write(svg: &mut Svg, part: &ErPart, styles: &Styles, node: &Node, theme: &Theme) {
	let is_neo = node.look == Look::Neo;
	let slot = slot_colors(node, theme);
	let node_css = styles.node_css();
	match part {
		&ErPart::Rect { x, y, width, height } => {
			// `.node rect` and `[data-look="neo"].node rect`.
			let (mut stroke, mut fill) = match is_neo {
				true => (theme.neo_stroke.as_str(), theme.main_bkg.as_str()),
				false => (theme.node_border.as_str(), theme.main_bkg.as_str()),
			};
			if let Some((border, background)) = slot {
				stroke = border;
				fill = background.unwrap_or(fill);
			}
			svg.open("rect")
				.attr("class", "basic label-container")
				.attr_if("fill", fill)
				.attr_if("stroke", stroke)
				.attr_if("stroke-width", &theme.node_stroke_width);
			if is_neo {
				svg.attr_if("filter", &theme.neo_filter);
			}
			svg.attr_if("style", &node_css)
				.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", width)
				.number_attr("height", height)
				.close();
		}
		ErPart::Rough { kind, corners } => {
			write_rough(svg, (*kind, corners, true), styles, node, theme, slot);
		}
		ErPart::Line { from, to } => {
			write_rough(svg, (ErKind::Divider, &[*from, *to], false), styles, node, theme, slot);
		}
	}
}

/// Writes a roughjs shape of `kind` through `points`: a closed shape is a filled and a
/// stroked outline, a line only stroked.
fn write_rough(
	svg: &mut Svg,
	(kind, points, is_closed): (ErKind, &[Point], bool),
	styles: &Styles,
	node: &Node,
	theme: &Theme,
	slot: Option<(&str, Option<&str>)>,
) {
	let is_requirement = node.shape == "requirementBox";
	let style = |key: &str| styles.get(key).filter(|value| !value.is_empty());
	let is_neo = node.look == Look::Neo;
	// `userNodeOverrides`; rows have their own fill and the border color.
	let (fill, stroke) = match kind {
		ErKind::RowOdd => (theme.er.row_odd.clone(), theme.plain_node_border.clone()),
		ErKind::RowEven => (theme.er.row_even.clone(), theme.plain_node_border.clone()),
		ErKind::Outer | ErKind::Divider => (
			Some(style("fill").unwrap_or(&theme.plain_main_bkg).to_owned()),
			style("stroke").unwrap_or(&theme.plain_node_border).to_owned(),
		),
	};
	let fill = fill.filter(|_| is_closed);
	let stroke_width =
		style("stroke-width").map_or_else(|| "1.3".to_owned(), |w| w.replace("px", ""));
	let dashes = match style("stroke-dasharray") {
		Some(dashes) => {
			let values: Vec<f64> =
				dashes.split_whitespace().map(|value| value.parse().unwrap_or(0.0)).collect();
			match values.as_slice() {
				[single] => format!("{} {}", number(*single), number(*single)),
				[first, second, ..] => format!("{} {}", number(*first), number(*second)),
				[] => "0 0".to_owned(),
			}
		}
		None => "0 0".to_owned(),
	};
	// The rules of the style sheet.
	let neo_width = format!("{}px", theme.stroke_width);
	let (rule_stroke, rule_width) = match is_neo {
		true => (Some(theme.neo_stroke.as_str()), Some(neo_width.as_str())),
		false => (None, None),
	};
	let rule_stroke = slot.map(|(border, _)| border).or(rule_stroke);
	let rule_fill = slot.and_then(|(_, background)| background);
	let node_css = styles.node_css();
	let user_css = match (theme.er.styles_all_paths, kind) {
		_ if node_css.is_empty() || node.look == Look::HandDrawn => String::new(),
		(true, _) | (false, ErKind::RowEven) => node_css,
		(false, _) if is_requirement => String::new(),
		(false, _) => styles
			.entries
			.iter()
			.filter(|(key, _)| key.contains("stroke"))
			.map(|(key, value)| format!("{key}:{value}"))
			.collect::<Vec<_>>()
			.join(";"),
	};
	let group_class = match kind {
		ErKind::Outer if is_requirement => "basic label-container outer-path",
		ErKind::Outer => "outer-path",
		ErKind::RowOdd => "row-rect-odd",
		ErKind::RowEven => "row-rect-even",
		ErKind::Divider => "divider",
	};
	let group_style = match is_requirement {
		true => styles.node_css(),
		false => node.css_styles.concat(),
	};
	svg.open("g").attr("class", group_class);
	if kind != ErKind::Divider {
		svg.attr_if("style", &group_style);
	}
	if kind == ErKind::Outer && is_neo {
		svg.attr_if("filter", &theme.neo_filter);
	}
	svg.end();
	if let Some(fill) = &fill {
		svg.open("path")
			.attr("d", &corners_path(points, false))
			.attr("stroke", rule_stroke.unwrap_or("none"))
			.attr("stroke-width", rule_width.unwrap_or("0"))
			.attr_if("fill", rule_fill.unwrap_or(fill));
		if kind == ErKind::Divider {
			svg.attr("fill-rule", "evenodd");
		}
		svg.attr_if("style", &user_css).close();
	}
	// roughjs draws no outline for the stroke `none`.
	if stroke == "none" {
		svg.close_tag("g");
		return;
	}
	svg.open("path")
		.attr("d", &corners_path(points, is_closed))
		.attr_if("stroke", rule_stroke.unwrap_or(&stroke))
		.attr("stroke-width", rule_width.unwrap_or(&stroke_width))
		.attr("fill", rule_fill.unwrap_or("none"))
		// roughjs writes `0 0` without a dash style; EdgeHTML then draws no line at all.
		.attr_if("stroke-dasharray", if dashes == "0 0" { "" } else { &dashes })
		.attr_if("style", &user_css)
		.close();
	svg.close_tag("g");
}
