//! Radar charts (`radar.langium`, `radar/db.ts`, `renderer.ts`, `styles.ts`).

use std::f64::consts::PI;

use super::common::CommonFields;
use super::langium::Scanner;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::{Font, parse_font_size};
use super::theme::ThemeVariables;
use super::value::Value;
use super::{Context, Diagram, Result};

/// Mermaid's `MAX_TICKS`.
const MAX_TICKS: f64 = 32.0;

const KEYWORDS: [&str; 9] = [
	"radar-beta",
	"radar-beta:",
	"axis",
	"curve",
	"showLegend",
	"ticks",
	"max",
	"min",
	"graticule",
];
const TERMINALS: [&str; 2] = ["circle", "polygon"];

struct Axis {
	name: String,
	label: String,
}

/// Values in axis order, or by axis name.
enum Entries {
	Values(Vec<f64>),
	Named(Vec<(String, f64)>),
}

struct Curve {
	label: String,
	entries: Entries,
	line: usize,
}

struct Options {
	show_legend: bool,
	ticks: f64,
	max: Option<f64>,
	min: f64,
	is_polygon: bool,
}

struct Radar {
	common: CommonFields,
	axes: Vec<Axis>,
	curves: Vec<(String, Vec<f64>)>,
	options: Options,
}

fn parse(code: &str, context: &Context) -> Result<Radar> {
	let mut common = CommonFields::new(context);
	let mut axes = Vec::new();
	let mut curves = Vec::new();
	let mut options =
		Options { show_legend: true, ticks: 5.0, max: None, min: 0.0, is_polygon: false };
	let mut scanner = Scanner::new(code);
	scanner.skip_newlines();
	let has_keyword = scanner.keyword("radar-beta:", false)
		|| (scanner.keyword("radar-beta", true) && {
			scanner.punctuation(":");
			true
		});
	if !has_keyword {
		return Err(scanner.error(context, "`radar-beta`"));
	}
	let name = |scanner: &mut Scanner| {
		scanner
			.id(&KEYWORDS, &TERMINALS)
			.map(str::to_owned)
			.ok_or_else(|| scanner.error(context, "a name"))
	};
	let label = |scanner: &mut Scanner| -> Result<Option<String>> {
		if !scanner.punctuation("[") {
			return Ok(None);
		}
		let label = scanner.string().ok_or_else(|| scanner.error(context, "a quoted label"))?;
		match scanner.punctuation("]") {
			true => Ok(Some(label)),
			false => Err(scanner.error(context, "`]`")),
		}
	};
	loop {
		if scanner.is_at_end() {
			break;
		}
		if scanner.newline() {
			continue;
		}
		if scanner.common_field(&mut common) {
			if !scanner.end_of_line() {
				return Err(scanner.error(context, "a new line"));
			}
			continue;
		}
		if scanner.keyword("axis", false) {
			loop {
				let name = name(&mut scanner)?;
				let label = label(&mut scanner)?.unwrap_or_else(|| name.clone());
				axes.push(Axis { name, label });
				if !scanner.punctuation(",") {
					break;
				}
			}
			continue;
		}
		if scanner.keyword("curve", false) {
			loop {
				let line = scanner.line();
				let name = name(&mut scanner)?;
				let label = label(&mut scanner)?.unwrap_or_else(|| name.clone());
				if !scanner.punctuation("{") {
					return Err(scanner.error(context, "`{`"));
				}
				let entries = parse_entries(&mut scanner, context)?;
				if !scanner.punctuation("}") {
					return Err(scanner.error(context, "`}`"));
				}
				curves.push(Curve { label, entries, line });
				if !scanner.punctuation(",") {
					break;
				}
			}
			continue;
		}
		if parse_option(&mut scanner, &mut options, context)? {
			while scanner.punctuation(",") {
				if !parse_option(&mut scanner, &mut options, context)? {
					return Err(scanner.error(context, "an option"));
				}
			}
			continue;
		}
		return Err(scanner.error(context, "`axis`, `curve` or an option"));
	}
	options.ticks = options.ticks.min(MAX_TICKS);
	let curves = curves
		.into_iter()
		.map(|curve| {
			let values = match curve.entries {
				Entries::Values(values) => values,
				Entries::Named(named) => {
					if axes.is_empty() {
						return Err(context.error_at(
							curve.line,
							"Axes must be populated before curves for reference entries",
						));
					}
					let mut values = Vec::with_capacity(axes.len());
					for axis in &axes {
						let entry = named.iter().find(|(name, _)| *name == axis.name);
						let Some((_, value)) = entry else {
							return Err(context.error_at(
								curve.line,
								format!("Missing entry for axis {}", axis.label),
							));
						};
						values.push(*value);
					}
					values
				}
			};
			Ok((curve.label, values))
		})
		.collect::<Result<_>>()?;
	Ok(Radar { common, axes, curves, options })
}

/// `Entries`: numbers, or `axis: number` pairs (the colon is optional), separated by commas
/// with line ends after the commas.
fn parse_entries(scanner: &mut Scanner, context: &Context) -> Result<Entries> {
	scanner.skip_newlines();
	let first_axis = scanner.id(&KEYWORDS, &TERMINALS).map(str::to_owned);
	let is_named = first_axis.is_some();
	let mut values = Vec::new();
	let mut named = Vec::new();
	let mut axis = first_axis;
	loop {
		if is_named {
			scanner.punctuation(":");
		}
		let value = scanner.number().ok_or_else(|| scanner.error(context, "a number"))?;
		match axis.take() {
			Some(axis) => named.push((axis, value)),
			None => values.push(value),
		}
		if !scanner.punctuation(",") {
			break;
		}
		scanner.skip_newlines();
		if is_named {
			let name = scanner.id(&KEYWORDS, &TERMINALS);
			axis = Some(name.ok_or_else(|| scanner.error(context, "an axis name"))?.to_owned());
		}
	}
	scanner.skip_newlines();
	Ok(match is_named {
		true => Entries::Named(named),
		false => Entries::Values(values),
	})
}

/// One `Option`; later values win.
fn parse_option(scanner: &mut Scanner, options: &mut Options, context: &Context) -> Result<bool> {
	if scanner.keyword("showLegend", false) {
		options.show_legend = match () {
			_ if scanner.keyword("true", false) => true,
			_ if scanner.keyword("false", false) => false,
			_ => return Err(scanner.error(context, "`true` or `false`")),
		};
		return Ok(true);
	}
	for (keyword, slot) in [("ticks", 0), ("max", 1), ("min", 2)] {
		if scanner.keyword(keyword, false) {
			let value = scanner.number().ok_or_else(|| scanner.error(context, "a number"))?;
			match slot {
				0 => options.ticks = value,
				1 => options.max = Some(value),
				_ => options.min = value,
			}
			return Ok(true);
		}
	}
	if scanner.keyword("graticule", false) {
		options.is_polygon = match () {
			_ if scanner.punctuation("circle") => false,
			_ if scanner.punctuation("polygon") => true,
			_ => return Err(scanner.error(context, "`circle` or `polygon`")),
		};
		return Ok(true);
	}
	Ok(false)
}

/// The radar style options (`buildRadarStyleOptions`): the theme's `radar` over the default
/// theme's.
struct RadarStyle {
	theme: Option<Value>,
	default: Option<Value>,
}

impl RadarStyle {
	fn text(&self, key: &str) -> String {
		[&self.theme, &self.default]
			.into_iter()
			.flatten()
			.find_map(|radar| radar.get(key).and_then(Value::to_text))
			.unwrap_or_default()
	}
}

/// `closedRoundCurve`: a closed Catmull-Rom spline as cubic Béziers.
fn closed_round_curve(points: &[(f64, f64)], tension: f64) -> String {
	let count = points.len();
	let mut path = format!("M{},{}", number(points[0].0), number(points[0].1));
	for index in 0..count {
		let p0 = points[(index + count - 1) % count];
		let p1 = points[index];
		let p2 = points[(index + 1) % count];
		let p3 = points[(index + 2) % count];
		let control1 = (p1.0 + (p2.0 - p0.0) * tension, p1.1 + (p2.1 - p0.1) * tension);
		let control2 = (p2.0 - (p3.0 - p1.0) * tension, p2.1 - (p3.1 - p1.1) * tension);
		path.push_str(&format!(
			" C{},{} {},{} {},{}",
			number(control1.0),
			number(control1.1),
			number(control2.0),
			number(control2.1),
			number(p2.0),
			number(p2.1)
		));
	}
	path.push_str(" Z");
	path
}

fn points_attribute(points: &[(f64, f64)]) -> String {
	points
		.iter()
		.map(|(x, y)| format!("{},{}", number(*x), number(*y)))
		.collect::<Vec<_>>()
		.join(" ")
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let radar = parse(code, context)?;
	let config = |key: &str, default: f64| context.config_number("radar", key, default);
	let width = config("width", 600.0);
	let height = config("height", 600.0);
	let margin_top = config("marginTop", 50.0);
	let margin_right = config("marginRight", 50.0);
	let margin_bottom = config("marginBottom", 50.0);
	let margin_left = config("marginLeft", 50.0);
	let axis_scale_factor = config("axisScaleFactor", 1.0);
	let axis_label_factor = config("axisLabelFactor", 1.05);
	let curve_tension = config("curveTension", 0.17);
	let use_max_width = context.config_bool("radar", "useMaxWidth", true);
	let total_width = width + margin_left + margin_right;
	let total_height = height + margin_top + margin_bottom;
	let center = (margin_left + width / 2.0, margin_top + height / 2.0);
	let radius = width.min(height) / 2.0;
	let options = &radar.options;
	let max_value = options.max.unwrap_or_else(|| {
		radar
			.curves
			.iter()
			.flat_map(|(_, values)| values.iter().copied())
			.fold(f64::NEG_INFINITY, f64::max)
	});
	let min_value = options.min;

	let default_theme = ThemeVariables::new("default", None);
	let style = RadarStyle {
		theme: context.theme.get("radar").cloned(),
		default: default_theme.get("radar").cloned(),
	};
	let color_limit = context
		.theme
		.get("THEME_COLOR_LIMIT")
		.or_else(|| default_theme.get("THEME_COLOR_LIMIT"))
		.and_then(Value::as_number)
		.unwrap_or(12.0);
	let scale_color = |index: usize| -> Option<String> {
		if index as f64 >= color_limit {
			return None;
		}
		let key = format!("cScale{index}");
		Some(context.theme.text(&key).or_else(|| default_theme.text(&key)).unwrap_or_default())
	};
	let font_family = context.font_family();
	let root_font_size = context.font_size();
	// `${size}px` is valid CSS only for a plain number; otherwise the root size is inherited.
	let pixel_font = |key: &str| {
		let size = style.text(key).trim().parse::<f64>().ok().filter(|size| size.is_finite());
		Font::new(font_family.clone(), size.unwrap_or(root_font_size))
	};
	let axis_label_font = pixel_font("axisLabelFontSize");
	let legend_font = pixel_font("legendFontSize");
	let title_font = Font::new(
		font_family.clone(),
		context
			.theme
			.text("fontSize")
			.and_then(|size| parse_font_size(&size, 16.0))
			.unwrap_or(16.0),
	);

	let mut body = Svg::new();
	body.open("g")
		.attr("transform", &format!("translate({}, {})", number(center.0), number(center.1)))
		.end();
	let graticule_color = style.text("graticuleColor");
	let graticule_paint = |svg: &mut Svg| {
		svg.attr("class", "radarGraticule")
			.attr_if("fill", &graticule_color)
			.attr_if("fill-opacity", &style.text("graticuleOpacity"))
			.attr_if("stroke", &graticule_color)
			.attr_if("stroke-width", &style.text("graticuleStrokeWidth"));
	};
	let axis_count = radar.axes.len();
	let angle_of = |index: usize| 2.0 * index as f64 * PI / axis_count as f64 - PI / 2.0;
	let mut tick = 0.0;
	while tick < options.ticks {
		let tick_radius = radius * (tick + 1.0) / options.ticks;
		match options.is_polygon {
			false => {
				body.open("circle").number_attr("r", tick_radius);
			}
			true => {
				let points: Vec<_> = (0..axis_count)
					.map(|index| {
						let angle = angle_of(index);
						(tick_radius * angle.cos(), tick_radius * angle.sin())
					})
					.collect();
				body.open("polygon").attr("points", &points_attribute(&points));
			}
		}
		graticule_paint(&mut body);
		body.close();
		tick += 1.0;
	}
	let axis_color = style.text("axisColor");
	for (index, axis) in radar.axes.iter().enumerate() {
		let angle = angle_of(index);
		let (cos, sin) = (angle.cos(), angle.sin());
		body.open("line")
			.attr("x1", "0")
			.attr("y1", "0")
			.number_attr("x2", radius * axis_scale_factor * cos)
			.number_attr("y2", radius * axis_scale_factor * sin)
			.attr("class", "radarAxisLine")
			.attr_if("stroke", &axis_color)
			.attr_if("stroke-width", &style.text("axisStrokeWidth"))
			.close();
		let anchor = match cos {
			_ if cos > 0.01 => Anchor::Start,
			_ if cos < -0.01 => Anchor::End,
			_ => Anchor::Middle,
		};
		let baseline = match sin {
			_ if sin > 0.01 => Baseline::Hanging,
			_ if sin < -0.01 => Baseline::Alphabetic,
			_ => Baseline::Central,
		};
		const LABEL_PAD: f64 = 4.0;
		let run = TextRun {
			x: radius * axis_label_factor * cos + LABEL_PAD * cos,
			y: radius * axis_label_factor * sin + LABEL_PAD * sin,
			anchor,
			baseline,
			font: &axis_label_font,
			content: &axis.label,
		};
		body.text(run, |svg| _ = svg.attr("class", "radarAxisLabel"));
	}
	let curve_opacity = style.text("curveOpacity");
	let curve_stroke_width = style.text("curveStrokeWidth");
	for (index, (_, values)) in radar.curves.iter().enumerate() {
		if values.len() != axis_count {
			continue;
		}
		let points: Vec<(f64, f64)> = values
			.iter()
			.enumerate()
			.map(|(axis, &value)| {
				let clipped = value.max(min_value).min(max_value);
				let relative = radius * (clipped - min_value) / (max_value - min_value);
				let angle = angle_of(axis);
				(relative * angle.cos(), relative * angle.sin())
			})
			.collect();
		// Browsers draw nothing for the NaN of an empty value range.
		if points.iter().any(|(x, y)| !x.is_finite() || !y.is_finite()) {
			continue;
		}
		match options.is_polygon {
			false => body.open("path").attr("d", &closed_round_curve(&points, curve_tension)),
			true => body.open("polygon").attr("points", &points_attribute(&points)),
		};
		body.attr("class", &format!("radarCurve-{index}"));
		if let Some(color) = scale_color(index) {
			body.attr_if("fill", &color)
				.attr_if("fill-opacity", &curve_opacity)
				.attr_if("stroke", &color)
				.attr_if("stroke-width", &curve_stroke_width);
		}
		body.close();
	}
	if options.show_legend {
		let legend_x = (width / 2.0 + margin_right) * 3.0 / 4.0;
		let legend_y = -(height / 2.0 + margin_top) * 3.0 / 4.0;
		const LINE_HEIGHT: f64 = 20.0;
		for (index, (label, _)) in radar.curves.iter().enumerate() {
			body.open("g")
				.attr(
					"transform",
					&format!(
						"translate({}, {})",
						number(legend_x),
						number(legend_y + index as f64 * LINE_HEIGHT)
					),
				)
				.end();
			body.open("rect")
				.attr("width", "12")
				.attr("height", "12")
				.attr("class", &format!("radarLegendBox-{index}"));
			if let Some(color) = scale_color(index) {
				body.attr_if("fill", &color)
					.attr_if("fill-opacity", &curve_opacity)
					.attr_if("stroke", &color);
			}
			body.close();
			let run = TextRun {
				x: 16.0,
				y: 0.0,
				anchor: Anchor::Start,
				baseline: Baseline::Hanging,
				font: &legend_font,
				content: label,
			};
			body.text(run, |svg| _ = svg.attr("class", "radarLegendText"));
			body.close_tag("g");
		}
	}
	let title = radar.common.title.clone().unwrap_or_default();
	if !title.is_empty() {
		let run = TextRun {
			x: 0.0,
			y: -height / 2.0 - margin_top,
			anchor: Anchor::Middle,
			baseline: Baseline::Hanging,
			font: &title_font,
			content: &title,
		};
		body.text(run, |svg| _ = svg.attr("class", "radarTitle"));
	}
	body.close_tag("g");
	let frame = Frame { x: 0.0, y: 0.0, width: total_width, height: total_height, use_max_width };
	Ok(document(context, "radar", &frame, &radar.common.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<Radar> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_axes_curves_and_options() {
		let radar = parsed(
			"radar-beta: title Skills\naxis A[\"Alpha\"], B, C curve c{C 3, A: 1, B: 2}\ncurve d[\"Dee\"]{\n1,\n2, 3\n}, e{4,5,6}\nmax 10, ticks 40\nshowLegend false\ngraticule polygon",
		)
		.unwrap();
		assert_eq!(radar.common.title.as_deref(), Some("Skills"));
		let labels: Vec<_> = radar.axes.iter().map(|axis| axis.label.as_str()).collect();
		assert_eq!(labels, ["Alpha", "B", "C"]);
		assert_eq!(radar.curves[0].1, [1.0, 2.0, 3.0]);
		assert_eq!(radar.curves[1].0, "Dee");
		assert_eq!(radar.curves.len(), 3);
		assert_eq!(radar.options.max, Some(10.0));
		assert_eq!(radar.options.ticks, MAX_TICKS);
		assert!(!radar.options.show_legend && radar.options.is_polygon);
	}

	#[test]
	fn rejects_what_langium_rejects() {
		for code in [
			"radar-beta\naxis circleA",
			"radar-beta\naxis trueX",
			"radar-beta\naxis 2D",
			"radar-beta\naxis A\ncurve c{1\n,2}",
			"radar-beta\naxis A\ncurve c{-1}",
			"radar-betas",
			"radar-beta\ncurve c{A: 1}",
		] {
			assert!(parsed(code).is_err(), "{code}");
		}
		let error = parsed("radar-beta\naxis A, B, C\ncurve c{A: 1, B: 2}").err().unwrap();
		assert_eq!(error.message, "Missing entry for axis C");
	}

	#[test]
	fn draws_closed_round_curves() {
		let path = closed_round_curve(&[(0.0, -10.0), (10.0, 0.0), (0.0, 10.0)], 0.5);
		assert_eq!(path, "M0,-10 C5,-15 10,-10 10,0 C10,10 5,15 0,10 C-5,5 -5,-5 0,-10 Z");
	}
}
