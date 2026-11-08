//! Pie charts (`pie.langium`, `pieRenderer.ts`, `pieStyles.ts`).

use std::f64::consts::PI;

use super::common::{CommonFields, Lines, parse_quoted, starts_with_word, strip_comment};
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::{Font, parse_font_size, to_fixed};
use super::{Context, Diagram, Result};

struct Pie {
	common: CommonFields,
	show_data: bool,
	/// Labels with values in source order; later duplicates are ignored like Mermaid's `Map`.
	sections: Vec<(String, f64)>,
}

fn parse(code: &str, context: &Context) -> Result<Pie> {
	let mut pie =
		Pie { common: CommonFields::new(context), show_data: false, sections: Vec::new() };
	let mut lines = Lines::new(code);
	let mut has_header = false;
	while let Some((line_number, line)) = lines.next_line() {
		let mut statement = strip_comment(line).trim();
		if statement.is_empty() {
			continue;
		}
		if !has_header {
			if !starts_with_word(statement, "pie") {
				return Err(context.error_at(line_number, "expected `pie`"));
			}
			has_header = true;
			statement = statement["pie".len()..].trim_start();
			if starts_with_word(statement, "showData") {
				pie.show_data = true;
				statement = statement["showData".len()..].trim_start();
			}
			if statement.is_empty() {
				continue;
			}
		}
		if let Some(result) = pie.common.parse(statement, &mut lines) {
			result.map_err(|error| {
				context.error_at(error.line.unwrap_or(line_number), error.message)
			})?;
			continue;
		}
		let (label, value) = parse_section(statement).ok_or_else(|| {
			context
				.error_at(line_number, format!("expected `\"label\" : value`, found {statement:?}"))
		})?;
		if value < 0.0 {
			return Err(context.error_at(
				line_number,
				format!("\"{label}\" has invalid value: {value}. Negative values are not allowed in pie charts. All slice values must be >= 0."),
			));
		}
		if !pie.sections.iter().any(|(known, _)| *known == label) {
			pie.sections.push((label, value));
		}
	}
	if !has_header {
		return Err(context.error_at(1, "expected `pie`"));
	}
	Ok(pie)
}

/// `"label" : number` with Mermaid's `NUMBER_PIE`: `-?(0|[1-9][0-9]*)(\.[0-9]+)?`.
fn parse_section(statement: &str) -> Option<(String, f64)> {
	let (label, rest) = parse_quoted(statement)?;
	let number = rest.trim_start().strip_prefix(':')?.trim();
	let digits = number.strip_prefix('-').unwrap_or(number);
	let (integer, fraction) = match digits.split_once('.') {
		Some((integer, fraction)) => (integer, Some(fraction)),
		None => (digits, None),
	};
	let is_valid_integer = !integer.is_empty()
		&& integer.bytes().all(|byte| byte.is_ascii_digit())
		&& (integer == "0" || !integer.starts_with('0'));
	let is_valid_fraction = fraction.is_none_or(|fraction| {
		!fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
	});
	if !is_valid_integer || !is_valid_fraction {
		return None;
	}
	let value: f64 = number.parse().ok()?;
	value.is_finite().then(|| (label.trim().to_owned(), value))
}

/// d3's `arc` path of a slice from `start` to `end` (radians clockwise from 12 o'clock).
fn arc_path(inner: f64, outer: f64, start: f64, end: f64) -> String {
	let point = |radius: f64, angle: f64| (radius * angle.sin(), -radius * angle.cos());
	let delta = end - start;
	let tau = 2.0 * PI;
	let epsilon = 1e-12;
	if delta >= tau - epsilon {
		// A full circle: two half circles, and the hole in the other direction.
		let mut path = format!(
			"M0,{}A{r},{r},0,1,1,0,{}A{r},{r},0,1,1,0,{}",
			number(-outer),
			number(outer),
			number(-outer),
			r = number(outer)
		);
		if inner > epsilon {
			path.push_str(&format!(
				"M0,{}A{r},{r},0,1,0,0,{}A{r},{r},0,1,0,0,{}",
				number(-inner),
				number(inner),
				number(-inner),
				r = number(inner)
			));
		}
		path.push('Z');
		return path;
	}
	let large_arc = u8::from(delta > PI);
	let (x0, y0) = point(outer, start);
	let (x1, y1) = point(outer, end);
	let mut path = format!(
		"M{},{}A{r},{r},0,{large_arc},1,{},{}",
		number(x0),
		number(y0),
		number(x1),
		number(y1),
		r = number(outer)
	);
	if inner > epsilon {
		let (x2, y2) = point(inner, end);
		let (x3, y3) = point(inner, start);
		path.push_str(&format!(
			"L{},{}A{r},{r},0,{large_arc},0,{},{}",
			number(x2),
			number(y2),
			number(x3),
			number(y3),
			r = number(inner)
		));
	} else {
		path.push_str("L0,0");
	}
	path.push('Z');
	path
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let pie = parse(code, context)?;
	const MARGIN: f64 = 40.0;
	const LEGEND_RECT_SIZE: f64 = 18.0;
	const LEGEND_SPACING: f64 = 4.0;
	let height: f64 = 450.0;
	let pie_width = height;
	let outer_stroke_width = context
		.theme
		.text("pieOuterStrokeWidth")
		.and_then(|text| parse_font_size(&text, 16.0))
		.unwrap_or(2.0);
	let text_position = context.config_number("pie", "textPosition", 0.75);
	let donut_hole = context.config_number("pie", "donutHole", 0.0);
	let inner_hole = if donut_hole > 0.0 && donut_hole <= 0.9 { donut_hole } else { 0.0 };
	let legend_position =
		context.config_text("pie", "legendPosition").unwrap_or_else(|| "right".to_owned());
	let highlight_slice = context.config_text("pie", "highlightSlice").unwrap_or_default();
	let use_max_width = context.config_bool("pie", "useMaxWidth", true);
	let radius = pie_width.min(height) / 2.0 - MARGIN;
	let font_family = context.font_family();
	let colors: Vec<String> =
		(1..=12).map(|index| context.theme.css(&format!("pie{index}"))).collect();
	let color_of = |label: &str| {
		let index = pie.sections.iter().position(|(known, _)| known == label).unwrap_or(0);
		colors[index % colors.len()].clone()
	};
	let sum: f64 = pie.sections.iter().map(|(_, value)| value).sum();

	let mut body = Svg::new();
	// Slices below 1 % are left out, like Mermaid's `createPieArcs`.
	let visible: Vec<(&str, f64)> = pie
		.sections
		.iter()
		.filter(|(_, value)| value / sum * 100.0 >= 1.0)
		.map(|(label, value)| (label.as_str(), *value))
		.collect();
	let visible_sum: f64 = visible.iter().map(|(_, value)| value).sum();
	let mut arcs = Vec::new();
	let mut angle = 0.0;
	for &(label, value) in &visible {
		let span = if visible_sum > 0.0 { value / visible_sum * 2.0 * PI } else { 0.0 };
		arcs.push((label, value, angle, angle + span));
		angle += span;
	}
	let arcs: Vec<_> = arcs
		.into_iter()
		.filter(|(_, value, _, _)| to_fixed(value / sum * 100.0, 0) != "0")
		.collect();

	let font_size = |key: &str, default: f64| {
		context.theme.text(key).and_then(|size| parse_font_size(&size, 16.0)).unwrap_or(default)
	};
	let legend_font = Font::new(font_family.clone(), font_size("pieLegendTextSize", 17.0));
	let title_font = Font::new(font_family.clone(), font_size("pieTitleTextSize", 25.0));
	let section_font = Font::new(font_family.clone(), font_size("pieSectionTextSize", 17.0));
	let legend_texts: Vec<String> = pie
		.sections
		.iter()
		.map(|(label, value)| match pie.show_data {
			true => format!("{label} [{}]", super::color::js_number(*value)),
			false => label.clone(),
		})
		.collect();
	let longest_text_width =
		legend_texts.iter().map(|text| context.measure(text, &legend_font).0).fold(0.0, f64::max);
	let title = pie.common.title.clone().unwrap_or_default();
	let title_width = if title.is_empty() { 0.0 } else { context.measure(&title, &title_font).0 };

	let theme = &context.theme;
	let legend_height = LEGEND_RECT_SIZE + LEGEND_SPACING;
	let count = pie.sections.len() as f64;
	let total_legend_height = count * legend_height;
	let mut chart_height = height;
	let mut chart_width = pie_width + MARGIN;
	let mut pie_translation = (0.0, 0.0);
	let horizontal_center = -longest_text_width / 2.0 - (LEGEND_RECT_SIZE + LEGEND_SPACING);
	let legend_translation = |index: f64| -> (f64, f64) {
		match legend_position.as_str() {
			"center" => (horizontal_center, index * legend_height - legend_height * count / 2.0),
			"top" => (horizontal_center, index * legend_height - radius),
			"bottom" => (horizontal_center, index * legend_height + radius + legend_height),
			"left" => (
				-radius - (LEGEND_RECT_SIZE + LEGEND_SPACING),
				index * legend_height - legend_height * count / 2.0,
			),
			_ => (12.0 * LEGEND_RECT_SIZE, index * legend_height - legend_height * count / 2.0),
		}
	};
	match legend_position.as_str() {
		"top" => {
			chart_height += total_legend_height;
			pie_translation = (0.0, total_legend_height + legend_height);
		}
		"bottom" => chart_height += total_legend_height,
		"left" => {
			chart_width += LEGEND_RECT_SIZE + LEGEND_SPACING + longest_text_width;
			pie_translation = (longest_text_width + LEGEND_RECT_SIZE + LEGEND_SPACING, 0.0);
		}
		"center" => {}
		_ => chart_width += LEGEND_RECT_SIZE + LEGEND_SPACING + longest_text_width,
	}

	body.open("g")
		.attr(
			"transform",
			&format!("translate({},{})", number(pie_width / 2.0), number(height / 2.0)),
		)
		.end();
	body.open("g");
	if pie_translation != (0.0, 0.0) {
		body.attr(
			"transform",
			&format!("translate({}, {})", number(pie_translation.0), number(pie_translation.1)),
		);
	}
	body.end();
	body.open("circle")
		.attr("cx", "0")
		.attr("cy", "0")
		.number_attr("r", radius + outer_stroke_width / 2.0)
		.attr("class", "pieOuterCircle")
		.attr_if("stroke", &theme.css("pieOuterStrokeColor"))
		.attr_if("stroke-width", &theme.css("pieOuterStrokeWidth"))
		.attr("fill", "none")
		.close();
	for &(label, _, start, end) in &arcs {
		let is_highlighted = highlight_slice == label;
		body.open("path")
			.attr("d", &arc_path(inner_hole * radius, radius, start, end))
			.attr("fill", &color_of(label))
			.attr("class", if is_highlighted { "pieCircle highlighted" } else { "pieCircle" })
			.attr_if("stroke", &theme.css("pieStrokeColor"))
			.attr_if("stroke-width", &theme.css("pieStrokeWidth"));
		match is_highlighted {
			true => body.attr("opacity", "1").attr("transform", "scale(1.05)"),
			false => body.attr_if("opacity", &theme.css("pieOpacity")),
		};
		body.close();
	}
	let label_radius = radius * text_position;
	for &(_, value, start, end) in &arcs {
		let middle = (start + end) / 2.0 - PI / 2.0;
		let transform = format!(
			"translate({},{})",
			number(middle.cos() * label_radius),
			number(middle.sin() * label_radius)
		);
		let label = format!("{}%", to_fixed(value / sum * 100.0, 0));
		let run = TextRun {
			x: 0.0,
			y: 0.0,
			anchor: Anchor::Middle,
			baseline: Baseline::Alphabetic,
			font: &section_font,
			content: &label,
		};
		body.text(run, |svg| {
			svg.attr("transform", &transform)
				.attr("class", "slice")
				.attr_if("fill", &theme.css("pieSectionTextColor"));
		});
	}
	body.close_tag("g");
	let title_run = TextRun {
		x: 0.0,
		y: -(height - 50.0) / 2.0,
		anchor: Anchor::Middle,
		baseline: Baseline::Alphabetic,
		font: &title_font,
		content: &title,
	};
	body.text(title_run, |svg| {
		svg.attr("class", "pieTitleText").attr_if("fill", &theme.css("pieTitleTextColor"));
	});
	for (index, text) in legend_texts.iter().enumerate() {
		let (x, y) = legend_translation(index as f64);
		let color = color_of(&pie.sections[index].0);
		body.open("g")
			.attr("class", "legend")
			.attr("transform", &format!("translate({},{})", number(x), number(y)))
			.end();
		body.open("rect")
			.number_attr("width", LEGEND_RECT_SIZE)
			.number_attr("height", LEGEND_RECT_SIZE)
			.attr("fill", &color)
			.attr("stroke", &color)
			.close();
		let run = TextRun {
			x: LEGEND_RECT_SIZE + LEGEND_SPACING,
			y: LEGEND_RECT_SIZE - LEGEND_SPACING,
			anchor: Anchor::Start,
			baseline: Baseline::Alphabetic,
			font: &legend_font,
			content: text,
		};
		body.text(run, |svg| _ = svg.attr_if("fill", &theme.css("pieLegendTextColor")));
		body.close_tag("g");
	}
	body.close_tag("g");

	let title_left = pie_width / 2.0 - title_width / 2.0;
	let title_right = pie_width / 2.0 + title_width / 2.0;
	let view_box_x = title_left.min(0.0);
	let view_box_right = chart_width.max(title_right);
	let frame = Frame {
		x: view_box_x,
		y: 0.0,
		width: view_box_right - view_box_x,
		height: chart_height,
		use_max_width,
	};
	Ok(document(context, "pie", &frame, &pie.common.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_sections() {
		assert_eq!(parse_section("\"Dogs\" : 386"), Some(("Dogs".to_owned(), 386.0)));
		assert_eq!(parse_section("'Cats':85.5"), Some(("Cats".to_owned(), 85.5)));
		assert_eq!(parse_section("\"Rats\" : -1"), Some(("Rats".to_owned(), -1.0)));
		for invalid in ["Dogs : 1", "\"a\" 1", "\"a\" : 01", "\"a\" : 1.", "\"a\" : 1e5", "\"a\" :"]
		{
			assert_eq!(parse_section(invalid), None, "{invalid}");
		}
	}

	#[test]
	fn writes_d3_arcs() {
		assert_eq!(arc_path(0.0, 10.0, 0.0, PI / 2.0), "M0,-10A10,10,0,0,1,10,0L0,0Z");
		assert_eq!(
			arc_path(0.0, 10.0, 0.0, 2.0 * PI),
			"M0,-10A10,10,0,1,1,0,10A10,10,0,1,1,0,-10Z"
		);
		assert_eq!(arc_path(5.0, 10.0, 0.0, PI), "M0,-10A10,10,0,0,1,0,10L0,5A5,5,0,0,0,0,-5Z");
	}
}
