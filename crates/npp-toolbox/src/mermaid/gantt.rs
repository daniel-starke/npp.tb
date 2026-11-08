//! Gantt charts (`gantt.jison`, `ganttDb.js`, `ganttRenderer.js`, `styles.js`), with dayjs
//! and d3's time scale, time axes and time formats.

mod d3_time;
mod date;
mod dayjs;
mod db;
mod parser;

use std::cmp::Ordering;

use self::d3_time::{Interval, MAX_TICKS, Unit, ticks, time_format};
use self::date::Calendar;
use self::db::{Database, Task};
use super::color::{is_color, js_number, js_round};
use super::common::{split_breaks, stable_sort};
use super::label::{decode_entities_to_text, encode_entities};
use super::layout::graph::array_index;
use super::svg::{Accessibility, Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::{Context, Diagram, Result};

const SECTION: &str = "gantt";
/// The width of the chart without `useWidth`: Mermaid takes the width of the element the
/// diagram is rendered in, and 1200 where it has none.
const DEFAULT_WIDTH: f64 = 1200.0;
/// d3-axis' offset on displays without high DPI, which moves lines onto pixel centers.
const AXIS_OFFSET: f64 = 0.5;

/// `GanttDiagramConfig`
struct Settings {
	title_top_margin: f64,
	bar_height: f64,
	bar_gap: f64,
	top_padding: f64,
	right_padding: f64,
	left_padding: f64,
	grid_line_start_padding: f64,
	font_size: f64,
	section_font_size: f64,
	number_section_styles: f64,
	axis_format: String,
	tick_interval: Option<String>,
	top_axis: bool,
	is_compact: bool,
	width: f64,
	use_max_width: bool,
}

impl Settings {
	fn new(context: &Context) -> Self {
		let number = |key: &str, default: f64| context.config_number(SECTION, key, default);
		Settings {
			title_top_margin: number("titleTopMargin", 25.0),
			bar_height: number("barHeight", 20.0),
			bar_gap: number("barGap", 4.0),
			top_padding: number("topPadding", 50.0),
			right_padding: number("rightPadding", 75.0),
			left_padding: number("leftPadding", 75.0),
			grid_line_start_padding: number("gridLineStartPadding", 35.0),
			font_size: number("fontSize", 11.0),
			section_font_size: number("sectionFontSize", 11.0),
			number_section_styles: number("numberSectionStyles", 4.0),
			axis_format: context
				.config_text(SECTION, "axisFormat")
				.unwrap_or_else(|| "%Y-%m-%d".to_owned()),
			tick_interval: context.config_text(SECTION, "tickInterval"),
			top_axis: context.config_bool(SECTION, "topAxis", false),
			is_compact: context.config_text(SECTION, "displayMode").as_deref() == Some("compact"),
			width: number("useWidth", DEFAULT_WIDTH),
			use_max_width: context.config_bool(SECTION, "useMaxWidth", true),
		}
	}
}

/// A value of the style sheet: a theme variable or a literal.
#[derive(Clone, Copy)]
enum StyleValue {
	Theme(&'static str),
	Literal(&'static str),
}

/// A rule of `styles.js` for elements with all of `classes`.
struct Rule {
	classes: &'static [&'static str],
	declarations: &'static [(&'static str, StyleValue, bool)],
}

macro_rules! rules {
	($([$($class:literal),+] => {$($property:literal : $value:expr $(, $important:ident)?);+ $(;)?})*) => {
		&[$(Rule {
			classes: &[$($class),+],
			declarations: &[$(($property, $value, rules!(@important $($important)?))),+],
		}),*]
	};
	(@important important) => { true };
	(@important) => { false };
}

use StyleValue::{Literal as L, Theme as T};

/// The rules of `styles.js` for the bars, rows and their texts, in style sheet order. Every
/// rule has the specificity of its classes.
const RULES: &[Rule] = rules! {
	["exclude-range"] => { "fill": T("excludeBkgColor") }
	["section"] => { "stroke": L("none"); "opacity": L("0.2") }
	["section0"] => { "fill": T("sectionBkgColor") }
	["section2"] => { "fill": T("sectionBkgColor2") }
	["section1"] => { "fill": T("altSectionBkgColor"); "opacity": L("0.2") }
	["section3"] => { "fill": T("altSectionBkgColor"); "opacity": L("0.2") }
	["sectionTitle0"] => { "fill": T("titleColor") }
	["sectionTitle1"] => { "fill": T("titleColor") }
	["sectionTitle2"] => { "fill": T("titleColor") }
	["sectionTitle3"] => { "fill": T("titleColor") }
	["sectionTitle"] => { "text-anchor": L("start") }
	["task"] => { "stroke-width": L("2") }
	["taskText"] => { "text-anchor": L("middle") }
	["taskTextOutsideRight"] => { "fill": T("taskTextDarkColor"); "text-anchor": L("start") }
	["taskTextOutsideLeft"] => { "fill": T("taskTextDarkColor"); "text-anchor": L("end") }
	["taskText", "clickable"] => {
		"fill": T("taskTextClickableColor"), important; "font-weight": L("bold")
	}
	["taskTextOutsideLeft", "clickable"] => {
		"fill": T("taskTextClickableColor"), important; "font-weight": L("bold")
	}
	["taskTextOutsideRight", "clickable"] => {
		"fill": T("taskTextClickableColor"), important; "font-weight": L("bold")
	}
	["taskText0"] => { "fill": T("taskTextColor") }
	["taskText1"] => { "fill": T("taskTextColor") }
	["taskText2"] => { "fill": T("taskTextColor") }
	["taskText3"] => { "fill": T("taskTextColor") }
	["task0"] => { "fill": T("taskBkgColor"); "stroke": T("taskBorderColor") }
	["task1"] => { "fill": T("taskBkgColor"); "stroke": T("taskBorderColor") }
	["task2"] => { "fill": T("taskBkgColor"); "stroke": T("taskBorderColor") }
	["task3"] => { "fill": T("taskBkgColor"); "stroke": T("taskBorderColor") }
	["taskTextOutside0"] => { "fill": T("taskTextOutsideColor") }
	["taskTextOutside2"] => { "fill": T("taskTextOutsideColor") }
	["taskTextOutside1"] => { "fill": T("taskTextOutsideColor") }
	["taskTextOutside3"] => { "fill": T("taskTextOutsideColor") }
	["active0"] => { "fill": T("activeTaskBkgColor"); "stroke": T("activeTaskBorderColor") }
	["active1"] => { "fill": T("activeTaskBkgColor"); "stroke": T("activeTaskBorderColor") }
	["active2"] => { "fill": T("activeTaskBkgColor"); "stroke": T("activeTaskBorderColor") }
	["active3"] => { "fill": T("activeTaskBkgColor"); "stroke": T("activeTaskBorderColor") }
	["activeText0"] => { "fill": T("taskTextDarkColor"), important }
	["activeText1"] => { "fill": T("taskTextDarkColor"), important }
	["activeText2"] => { "fill": T("taskTextDarkColor"), important }
	["activeText3"] => { "fill": T("taskTextDarkColor"), important }
	["done0"] => {
		"stroke": T("doneTaskBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2")
	}
	["done1"] => {
		"stroke": T("doneTaskBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2")
	}
	["done2"] => {
		"stroke": T("doneTaskBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2")
	}
	["done3"] => {
		"stroke": T("doneTaskBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2")
	}
	["doneText0"] => { "fill": T("taskTextDarkColor"), important }
	["doneText1"] => { "fill": T("taskTextDarkColor"), important }
	["doneText2"] => { "fill": T("taskTextDarkColor"), important }
	["doneText3"] => { "fill": T("taskTextDarkColor"), important }
	["doneText0", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneText0", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["doneText1", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneText1", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["doneText2", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneText2", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["doneText3", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneText3", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["crit0"] => { "stroke": T("critBorderColor"); "fill": T("critBkgColor"); "stroke-width": L("2") }
	["crit1"] => { "stroke": T("critBorderColor"); "fill": T("critBkgColor"); "stroke-width": L("2") }
	["crit2"] => { "stroke": T("critBorderColor"); "fill": T("critBkgColor"); "stroke-width": L("2") }
	["crit3"] => { "stroke": T("critBorderColor"); "fill": T("critBkgColor"); "stroke-width": L("2") }
	["activeCrit0"] => {
		"stroke": T("critBorderColor"); "fill": T("activeTaskBkgColor"); "stroke-width": L("2")
	}
	["activeCrit1"] => {
		"stroke": T("critBorderColor"); "fill": T("activeTaskBkgColor"); "stroke-width": L("2")
	}
	["activeCrit2"] => {
		"stroke": T("critBorderColor"); "fill": T("activeTaskBkgColor"); "stroke-width": L("2")
	}
	["activeCrit3"] => {
		"stroke": T("critBorderColor"); "fill": T("activeTaskBkgColor"); "stroke-width": L("2")
	}
	["doneCrit0"] => {
		"stroke": T("critBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2");
		"shape-rendering": L("crispEdges")
	}
	["doneCrit1"] => {
		"stroke": T("critBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2");
		"shape-rendering": L("crispEdges")
	}
	["doneCrit2"] => {
		"stroke": T("critBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2");
		"shape-rendering": L("crispEdges")
	}
	["doneCrit3"] => {
		"stroke": T("critBorderColor"); "fill": T("doneTaskBkgColor"); "stroke-width": L("2");
		"shape-rendering": L("crispEdges")
	}
	["milestone"] => { "transform": L("milestone") }
	["milestoneText"] => { "font-style": L("italic") }
	["doneCritText0"] => { "fill": T("taskTextDarkColor"), important }
	["doneCritText1"] => { "fill": T("taskTextDarkColor"), important }
	["doneCritText2"] => { "fill": T("taskTextDarkColor"), important }
	["doneCritText3"] => { "fill": T("taskTextDarkColor"), important }
	["doneCritText0", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneCritText0", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["doneCritText1", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneCritText1", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["doneCritText2", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneCritText2", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["doneCritText3", "taskTextOutsideLeft"] => { "fill": T("taskTextOutsideColor"), important }
	["doneCritText3", "taskTextOutsideRight"] => { "fill": T("taskTextOutsideColor"), important }
	["vert"] => { "stroke": T("vertLineColor") }
	["vertText"] => {
		"font-size": L("15"); "text-anchor": L("middle"); "fill": T("vertLineColor"), important
	}
	["activeCritText0"] => { "fill": T("taskTextDarkColor"), important }
	["activeCritText1"] => { "fill": T("taskTextDarkColor"), important }
	["activeCritText2"] => { "fill": T("taskTextDarkColor"), important }
	["activeCritText3"] => { "fill": T("taskTextDarkColor"), important }
};

/// The computed values of the properties the rules set for an element with `classes`: the
/// matching declaration of the highest importance and specificity, the later one on a tie.
fn computed_style(context: &Context, classes: &str) -> Vec<(&'static str, String)> {
	let classes: Vec<&str> = classes.split_whitespace().collect();
	let mut winners: Vec<(&'static str, String, bool, usize)> = Vec::new();
	for rule in RULES {
		if !rule.classes.iter().all(|class| classes.contains(class)) {
			continue;
		}
		let specificity = rule.classes.len();
		for &(property, value, is_important) in rule.declarations {
			let value = match value {
				StyleValue::Literal(text) => text.to_owned(),
				StyleValue::Theme(key) => context.theme.css(key),
			};
			let is_paint = matches!(property, "fill" | "stroke");
			if value.is_empty() || (is_paint && value != "none" && !is_color(&value)) {
				continue;
			}
			let rank = (is_important, specificity);
			match winners.iter_mut().find(|winner| winner.0 == property) {
				Some(winner) if (winner.2, winner.3) <= rank => {
					*winner = (property, value, is_important, specificity)
				}
				Some(_) => {}
				None => winners.push((property, value, is_important, specificity)),
			}
		}
	}
	winners.into_iter().map(|(property, value, _, _)| (property, value)).collect()
}

fn style_value<'a>(style: &'a [(&'static str, String)], property: &str) -> Option<&'a str> {
	style.iter().find(|(name, _)| *name == property).map(|(_, value)| value.as_str())
}

/// Writes the paint properties of `style` as presentation attributes.
fn write_paint(svg: &mut Svg, style: &[(&'static str, String)]) {
	for property in ["fill", "stroke", "stroke-width", "opacity", "shape-rendering"] {
		if let Some(value) = style_value(style, property) {
			svg.attr(property, value);
		}
	}
}

/// A d3 time scale with `rangeRound([0, range])`.
struct TimeScale {
	start: f64,
	end: f64,
	range: f64,
}

impl TimeScale {
	fn scale(&self, time: Option<f64>) -> f64 {
		let Some(time) = time else { return f64::NAN };
		let span = self.end - self.start;
		let t = if span == 0.0 {
			0.5
		} else if span.is_nan() {
			f64::NAN
		} else {
			(time - self.start) / span
		};
		js_round(self.range * t)
	}
}

/// Sorts `keys` like the keys of a JavaScript object that got them in this order.
fn object_key_order(keys: &mut [String]) {
	let order = |key: &String| match array_index(key) {
		Some(index) => (0, index),
		None => (1, 0),
	};
	npp::sort::sort_by_key(&mut keys[..], order);
}

/// `getMaxIntersections`: puts overlapping tasks of a section into rows from `offset` on and
/// returns the index of the last row used.
fn max_intersections(tasks: &mut [Task], members: &[usize], offset: f64) -> f64 {
	let mut sorted = members.to_vec();
	stable_sort(&mut sorted, |a, b| {
		let (a, b) = (&tasks[a], &tasks[b]);
		let difference = a.start_time.unwrap_or(f64::NAN) - b.start_time.unwrap_or(f64::NAN);
		let difference =
			if difference == 0.0 || difference.is_nan() { a.order - b.order } else { difference };
		difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
	});
	let mut rows = vec![f64::NEG_INFINITY; members.len()];
	let mut maximum = 0.0;
	for index in sorted {
		let task = &mut tasks[index];
		let start = task.start_time.unwrap_or(f64::NAN);
		for (row, end) in rows.iter_mut().enumerate() {
			if start >= *end {
				*end = task.end_time.unwrap_or(f64::NAN);
				task.order = row as f64 + offset;
				if row as f64 > maximum {
					maximum = row as f64;
				}
				break;
			}
		}
	}
	maximum
}

/// The weekday of `weekday <day>` for weekly ticks.
fn weekday_index(day: &str) -> u8 {
	["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"]
		.iter()
		.position(|name| *name == day)
		.unwrap_or(0) as u8
}

/// The interval of `tickInterval`: `^([1-9]\d*)(millisecond|second|…|month)$`.
fn tick_interval(text: &str, weekday: &str) -> Option<Interval> {
	let digits = text.bytes().take_while(u8::is_ascii_digit).count();
	if digits == 0 || text.starts_with('0') {
		return None;
	}
	let unit = match &text[digits..] {
		"millisecond" => Unit::Millisecond,
		"second" => Unit::Second,
		"minute" => Unit::Minute,
		"hour" => Unit::Hour,
		"day" => Unit::Day,
		"week" => Unit::Week(weekday_index(weekday)),
		"month" => Unit::Month,
		_ => return None,
	};
	Interval::every(unit, text[..digits].parse::<f64>().ok()?)
}

/// The text of a task, entity placeholders back to their characters.
fn shown(text: &str) -> String {
	decode_entities_to_text(text)
}

struct Renderer<'a> {
	settings: &'a Settings,
	calendar: Calendar<'a>,
	scale: TimeScale,
	width: f64,
	height: f64,
	categories: Vec<String>,
	/// The tasks that are no vertical markers.
	row_count: f64,
}

impl Renderer<'_> {
	fn section_number(&self, section: &str) -> String {
		let index = self.categories.iter().rposition(|category| category == section).unwrap_or(0);
		js_number(index as f64 % self.settings.number_section_styles)
	}

	/// The `x` of a time in the chart.
	fn x(&self, time: Option<f64>) -> f64 {
		self.scale.scale(time)
	}

	/// `drawExcludeDays`
	fn exclude_days(&self, svg: &mut Svg, context: &Context, database: &Database, tasks: &[Task]) {
		if database.excludes.is_empty() && database.includes.is_empty() {
			return;
		}
		let (mut minimum, mut maximum): (Option<f64>, Option<f64>) = (None, None);
		for task in tasks {
			if minimum.is_none()
				|| matches!((task.start_time, minimum), (Some(s), Some(m)) if s < m)
			{
				minimum = task.start_time;
			}
			if maximum.is_none() || matches!((task.end_time, maximum), (Some(e), Some(m)) if e > m)
			{
				maximum = task.end_time;
			}
		}
		let (Some(minimum), Some(maximum)) = (minimum, maximum) else { return };
		if self.calendar.diff_years(maximum, minimum) > 5.0 {
			return;
		}
		let mut ranges: Vec<(f64, f64)> = Vec::new();
		let mut range: Option<(f64, f64)> = None;
		let mut day = minimum;
		while day <= maximum {
			// Formats that need other dayjs plugins fail the whole chart in Mermaid; here they
			// only leave the excluded days out.
			let is_excluded = database.is_excluded(&self.calendar, day).unwrap_or(false);
			if is_excluded {
				range = Some(match range {
					None => (day, day),
					Some((start, _)) => (start, day),
				});
			} else if let Some(finished) = range.take() {
				ranges.push(finished);
			}
			day = self.calendar.add(day, 1.0, "d");
		}
		svg.open("g").end();
		let fill = context.theme.css("excludeBkgColor");
		for (start, end) in ranges {
			let start_of_day = self.calendar.start_of_day(start);
			let x = self.x(Some(start_of_day)) + self.settings.left_padding;
			let width = self.x(Some(self.calendar.end_of_day(end))) - self.x(Some(start_of_day));
			let date = self.calendar.format(start, "YYYY-MM-DD").unwrap_or_default();
			svg.open("rect")
				.attr("id", &format!("{}-exclude-{date}", context.id))
				.number_attr("x", x)
				.number_attr("y", self.settings.grid_line_start_padding)
				.number_attr("width", width)
				.number_attr(
					"height",
					self.height - self.settings.top_padding - self.settings.grid_line_start_padding,
				)
				.attr("class", "exclude-range")
				.attr_if("fill", &fill)
				.close();
		}
		svg.close_tag("g");
	}

	/// The ticks of an axis: the custom interval, or d3's default ones where it would make too
	/// many (Mermaid means to skip it then, but its estimate is always `NaN`).
	fn ticks(&self, interval: Option<Interval>) -> Vec<f64> {
		let (start, end) = (self.scale.start, self.scale.end);
		if let Some(interval) = interval
			&& start.partial_cmp(&end) != Some(Ordering::Greater)
		{
			let custom = ticks(&self.calendar, start, end, Some(interval));
			if custom.len() < MAX_TICKS {
				return custom;
			}
		}
		ticks(&self.calendar, start, end, None)
	}

	/// `makeGrid`: the bottom axis and, with `topAxis`, the top one.
	fn grid(&self, svg: &mut Svg, context: &Context, database: &Database) {
		let settings = self.settings;
		let axis_format = if !database.axis_format.is_empty() {
			database.axis_format.clone()
		} else if database.date_format == "D" {
			"%d".to_owned()
		} else {
			settings.axis_format.clone()
		};
		let interval_text = database
			.tick_interval
			.clone()
			.filter(|text| !text.is_empty())
			.or_else(|| settings.tick_interval.clone());
		let interval =
			interval_text.as_deref().and_then(|text| tick_interval(text, &database.weekday));
		let values = self.ticks(interval);
		let tick_size = -self.height + settings.top_padding + settings.grid_line_start_padding;
		let font = Font::new(context.font_family(), 10.0);
		let text_color = Some(context.theme.css("textColor"))
			.filter(|color| is_color(color))
			.unwrap_or_else(|| "#000".to_owned());
		let range_end = self.scale.range + AXIS_OFFSET;
		let axis = |svg: &mut Svg, is_top: bool, y: f64| {
			let direction = if is_top { -1.0 } else { 1.0 };
			svg.open("g")
				.attr("class", "grid")
				.attr(
					"transform",
					&format!("translate({}, {})", number(settings.left_padding), number(y)),
				)
				.attr("fill", "none")
				.attr("font-size", "10")
				.attr("font-family", "sans-serif")
				.attr("text-anchor", "middle")
				.end();
			let outer = number(direction * tick_size);
			svg.open("path")
				.attr("class", "domain")
				.attr("stroke", "#000")
				.attr("stroke-width", "0")
				.attr(
					"d",
					&format!(
						"M{},{outer}V{}H{}V{outer}",
						number(AXIS_OFFSET),
						number(AXIS_OFFSET),
						number(range_end)
					),
				)
				.close();
			for &value in &values {
				let x = self.x(Some(value)) + AXIS_OFFSET;
				svg.open("g")
					.attr("class", "tick")
					.attr("opacity", "0.8")
					.attr("shape-rendering", "crispEdges")
					.attr("transform", &format!("translate({},0)", number(x)))
					.end();
				svg.open("line")
					.attr("stroke", "#000")
					.number_attr("y2", direction * tick_size)
					.close();
				let label = time_format(&self.calendar, &axis_format, value);
				// `dy` is `1em` below the bottom axis and `0em` above the top one.
				let y = if is_top { -3.0 } else { 3.0 + font.size };
				let run = TextRun {
					x: 0.0,
					y,
					anchor: Anchor::Middle,
					baseline: Baseline::Alphabetic,
					font: &font,
					content: &label,
				};
				svg.text(run, |svg| _ = svg.attr("fill", &text_color).attr("stroke", "none"));
				svg.close_tag("g");
			}
			svg.close_tag("g");
		};
		axis(svg, false, self.height - 50.0);
		if settings.top_axis {
			axis(svg, true, settings.top_padding);
		}
	}

	/// `drawRects`: the section rows, the bars and their texts.
	fn rects(
		&self,
		svg: &mut Svg,
		context: &mut Context,
		database: &Database,
		tasks: &mut Vec<Task>,
	) {
		let settings = self.settings;
		let gap = settings.bar_height + settings.bar_gap;
		sort_tasks(tasks, |a, b| a.is_vertical.cmp(&b.is_vertical));
		let mut seen_orders: Vec<f64> = Vec::new();
		svg.open("g").end();
		for task in tasks.iter().filter(|task| !task.is_vertical) {
			if seen_orders.iter().any(|order| order.to_bits() == task.order.to_bits()) {
				continue;
			}
			seen_orders.push(task.order);
			let class = match self.categories.iter().position(|category| *category == task.section)
			{
				Some(index) => format!(
					"section section{}",
					js_number(index as f64 % settings.number_section_styles)
				),
				None => "section section0".to_owned(),
			};
			let style = computed_style(context, &class);
			svg.open("rect")
				.number_attr("x", 0.0)
				.number_attr("y", task.order * gap + settings.top_padding - 2.0)
				.number_attr("width", self.width - settings.right_padding / 2.0)
				.number_attr("height", gap)
				.attr("class", &class);
			write_paint(svg, &style);
			svg.close();
		}
		svg.close_tag("g");

		let font = Font::new(context.font_family(), settings.font_size);
		let rows = self.row_count;
		svg.open("g").end();
		let mut texts = Svg::new();
		for task in tasks.iter() {
			let section_number = self.section_number(&task.section);
			let class_names = task.classes.join(" ");
			let link = database.links.get(task.id_text()).map(|link| shown(link));
			let start = self.x(task.start_time);
			let end = self.x(task.end_time);
			let render_end = self.x(task.render_end_time.or(task.end_time));

			// The bar.
			let mut task_class = String::new();
			if task.is_active {
				task_class = if task.is_critical { " activeCrit" } else { " active" }.to_owned();
			} else if task.is_done {
				task_class = if task.is_critical { " doneCrit" } else { " done" }.to_owned();
			} else if task.is_critical {
				task_class = " crit".to_owned();
			}
			if task_class.is_empty() {
				task_class = " task".to_owned();
			}
			if task.is_milestone {
				task_class = format!(" milestone {task_class}");
			}
			if task.is_vertical {
				task_class = format!(" vert {task_class}");
			}
			let class = format!("task{task_class}{section_number} {class_names}");
			let style = computed_style(context, &class);
			let x = if task.is_milestone {
				start + settings.left_padding + 0.5 * (end - start) - 0.5 * settings.bar_height
			} else {
				start + settings.left_padding
			};
			let y = if task.is_vertical {
				settings.grid_line_start_padding
			} else {
				task.order * gap + settings.top_padding
			};
			let width = if task.is_milestone {
				settings.bar_height
			} else if task.is_vertical {
				0.08 * settings.bar_height
			} else {
				render_end - start
			};
			let height = if task.is_vertical {
				rows * gap + settings.bar_height * 2.0
			} else {
				settings.bar_height
			};
			if let Some(link) = &link {
				svg.open("a").attr("xlink:href", link).end();
			}
			svg.open("rect")
				.attr("id", &format!("{}-{}", context.id, shown(task.id_text())))
				.attr("rx", "3")
				.attr("ry", "3")
				.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr("class", class.trim_end());
			write_paint(svg, &style);
			if style_value(&style, "transform").is_some() {
				// `rotate(45deg) scale(0.8)` around the `transform-origin` of the bar.
				let origin_x = start + settings.left_padding + 0.5 * (end - start);
				let origin_y = task.order * gap + settings.top_padding + 0.5 * settings.bar_height;
				svg.attr(
					"transform",
					&format!(
						"translate({} {}) rotate(45) scale(0.8) translate({} {})",
						number(origin_x),
						number(origin_y),
						number(-origin_x),
						number(-origin_y)
					),
				);
			}
			svg.close();
			if link.is_some() {
				svg.close_tag("a");
			}

			// The text, placed with its width before the classes apply.
			let text_width = context.bounding_box(&task.name, &font).0;
			let (mut text_start, mut text_end) = (start, render_end);
			if task.is_milestone {
				text_start += 0.5 * (end - start) - 0.5 * settings.bar_height;
				text_end = text_start + settings.bar_height;
			}
			let text_x = if task.is_vertical {
				start + settings.left_padding
			} else if text_width > text_end - text_start {
				if text_end + text_width + 1.5 * settings.left_padding > self.width {
					text_start + settings.left_padding - 5.0
				} else {
					text_end + settings.left_padding + 5.0
				}
			} else {
				(text_end - text_start) / 2.0 + text_start + settings.left_padding
			};
			let text_y = if task.is_vertical {
				settings.grid_line_start_padding + rows * gap + 60.0
			} else {
				task.order * gap
					+ settings.bar_height / 2.0
					+ (settings.font_size / 2.0 - 2.0)
					+ settings.top_padding
			};
			// The class uses the end without the excluded days.
			let class_end = if task.is_milestone { start + settings.bar_height } else { end };
			let mut text_type = String::new();
			if task.is_active {
				text_type = format!(
					"{}{section_number}",
					if task.is_critical { "activeCritText" } else { "activeText" }
				);
			}
			if task.is_done {
				let kind = if task.is_critical { "doneCritText" } else { "doneText" };
				text_type = format!("{text_type} {kind}{section_number}");
			} else if task.is_critical {
				text_type = format!("{text_type} critText{section_number}");
			}
			if task.is_milestone {
				text_type.push_str(" milestoneText");
			}
			if task.is_vertical {
				text_type.push_str(" vertText");
			}
			let width_class = js_number(text_width);
			let text_class = if text_width > class_end - start {
				if class_end + text_width + 1.5 * settings.left_padding > self.width {
					format!(
						"{class_names} taskTextOutsideLeft taskTextOutside{section_number} {text_type}"
					)
				} else {
					format!(
						"{class_names} taskTextOutsideRight taskTextOutside{section_number} \
						 {text_type} width-{width_class}"
					)
				}
			} else {
				format!(
					"{class_names} taskText taskText{section_number} {text_type} width-{width_class}"
				)
			};
			let text_style = computed_style(context, &text_class);
			let anchor = match style_value(&text_style, "text-anchor") {
				Some("middle") => Anchor::Middle,
				Some("end") => Anchor::End,
				_ => Anchor::Start,
			};
			let mut text_font = font.clone();
			if let Some(size) =
				style_value(&text_style, "font-size").and_then(|size| size.parse().ok())
			{
				text_font.size = size;
			}
			if style_value(&text_style, "font-weight") == Some("bold") {
				text_font.weight = 700;
			}
			text_font.is_italic = style_value(&text_style, "font-style") == Some("italic");
			let content = shown(&task.name);
			if let Some(link) = &link {
				texts.open("a").attr("xlink:href", link).end();
			}
			let run = TextRun {
				x: text_x,
				y: text_y,
				anchor,
				baseline: Baseline::Alphabetic,
				font: &text_font,
				content: &content,
			};
			let id = format!("{}-{}-text", context.id, shown(task.id_text()));
			texts.text(run, |svg| {
				svg.attr("id", &id).attr("class", text_class.trim());
				if let Some(fill) = style_value(&text_style, "fill") {
					svg.attr("fill", fill);
				}
			});
			if link.is_some() {
				texts.close_tag("a");
			}
		}
		svg.append(texts);
		svg.close_tag("g");
	}

	/// `vertLabels`: the section names.
	fn section_titles(&self, svg: &mut Svg, context: &Context, heights: &[(String, f64)]) {
		let settings = self.settings;
		let gap = settings.bar_height + settings.bar_gap;
		let font = Font::new(context.font_family(), settings.section_font_size);
		let mut previous_rows = 0.0;
		svg.open("g").end();
		for (index, (name, rows)) in heights.iter().enumerate() {
			if index > 0 {
				previous_rows += heights[index - 1].1;
			}
			let y = rows * gap / 2.0 + previous_rows * gap + settings.top_padding;
			let class = match self.categories.iter().position(|category| category == name) {
				Some(position) => format!(
					"sectionTitle sectionTitle{}",
					js_number(position as f64 % settings.number_section_styles)
				),
				None => "sectionTitle".to_owned(),
			};
			let style = computed_style(context, &class);
			let fill = style_value(&style, "fill").unwrap_or_default().to_owned();
			let shown_name = shown(name);
			let lines = split_breaks(&shown_name);
			let first_offset = -((lines.len() - 1) as f64) / 2.0;
			for (line_index, line) in lines.iter().enumerate() {
				let run = TextRun {
					x: 10.0,
					y: y + (first_offset + line_index as f64) * font.size,
					anchor: Anchor::Start,
					baseline: Baseline::Central,
					font: &font,
					content: line,
				};
				svg.text(run, |svg| _ = svg.attr("class", &class).attr_if("fill", &fill));
			}
		}
		svg.close_tag("g");
	}

	/// `drawToday`
	fn today(&self, svg: &mut Svg, context: &Context, marker: &str) {
		if marker == "off" {
			return;
		}
		let settings = self.settings;
		let x = self.x(Some(self.calendar.clock.now())) + settings.left_padding;
		let stroke = context.theme.css("todayLineColor");
		svg.open("g").attr("class", "today").end();
		svg.open("line")
			.number_attr("x1", x)
			.number_attr("x2", x)
			.number_attr("y1", settings.title_top_margin)
			.number_attr("y2", self.height - settings.title_top_margin)
			.attr("class", "today")
			.attr("fill", "none")
			.attr_if("stroke", &stroke)
			.attr("stroke-width", "2");
		if !marker.is_empty() {
			svg.attr("style", &marker.replace(',', ";"));
		}
		svg.close();
		svg.close_tag("g");
	}
}

/// Sorts the tasks stably like JavaScript's `sort`, also with comparisons that are no total
/// order.
fn sort_tasks(tasks: &mut Vec<Task>, compare: impl Fn(&Task, &Task) -> Ordering) {
	let mut order: Vec<usize> = (0..tasks.len()).collect();
	stable_sort(&mut order, |a, b| compare(&tasks[a], &tasks[b]));
	let mut taken: Vec<Option<Task>> = std::mem::take(tasks).into_iter().map(Some).collect();
	tasks.extend(order.into_iter().filter_map(|index| taken[index].take()));
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	let mut database = Database::new(context.title.as_deref());
	parser::parse(&code, context, &mut database)?;
	let clock = context.clock;
	let calendar = Calendar { clock };
	database.compile(&calendar).map_err(|(line, message)| context.error_at(line, message))?;
	let settings = Settings::new(context);
	let mut tasks = std::mem::take(&mut database.tasks);

	// The rows: one per task, or in compact mode as many as tasks overlap in a section.
	let row_tasks: Vec<usize> =
		(0..tasks.len()).filter(|&index| !tasks[index].is_vertical).collect();
	let mut categories: Vec<String> = Vec::new();
	for &index in &row_tasks {
		if !categories.contains(&tasks[index].section) {
			categories.push(tasks[index].section.clone());
		}
	}
	let gap = settings.bar_height + settings.bar_gap;
	let mut height = 2.0 * settings.top_padding;
	let mut heights: Vec<(String, f64)> = Vec::new();
	if settings.is_compact {
		let mut sections: Vec<String> = categories.clone();
		object_key_order(&mut sections);
		let mut offset = 0.0;
		for section in sections {
			let members: Vec<usize> = row_tasks
				.iter()
				.copied()
				.filter(|&index| tasks[index].section == section)
				.collect();
			let rows = max_intersections(&mut tasks, &members, offset) + 1.0;
			offset += rows;
			height += rows * gap;
			heights.push((section, rows));
		}
	} else {
		height += row_tasks.len() as f64 * gap;
		let mut keys = categories.clone();
		object_key_order(&mut keys);
		for key in keys {
			let count = row_tasks.iter().filter(|&&index| tasks[index].section == key).count();
			heights.push((key, count as f64));
		}
	}

	let starts = tasks.iter().filter_map(|task| task.start_time).filter(|time| !time.is_nan());
	let ends = tasks.iter().filter_map(|task| task.end_time).filter(|time| !time.is_nan());
	let width = settings.width;
	let scale = TimeScale {
		start: starts.fold(f64::NAN, |minimum, time| if minimum <= time { minimum } else { time }),
		end: ends.fold(f64::NAN, |maximum, time| if maximum >= time { maximum } else { time }),
		range: width - settings.left_padding - settings.right_padding,
	};
	sort_tasks(&mut tasks, |a, b| match (a.start_time, b.start_time) {
		(Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
		_ => Ordering::Equal,
	});

	let renderer = Renderer {
		settings: &settings,
		calendar,
		scale,
		width,
		height,
		categories,
		row_count: row_tasks.len() as f64,
	};
	let mut body = Svg::new();
	renderer.exclude_days(&mut body, context, &database, &tasks);
	renderer.grid(&mut body, context, &database);
	renderer.rects(&mut body, context, &database, &mut tasks);
	renderer.section_titles(&mut body, context, &heights);
	renderer.today(&mut body, context, &database.today_marker);

	let title = shown(&database.title);
	let title_font = Font::new(context.font_family(), 18.0);
	let title_fill = context
		.theme
		.text("titleColor")
		.filter(|color| !color.is_empty())
		.unwrap_or_else(|| context.theme.css("textColor"));
	let run = TextRun {
		x: width / 2.0,
		y: settings.title_top_margin,
		anchor: Anchor::Middle,
		baseline: Baseline::Alphabetic,
		font: &title_font,
		content: &title,
	};
	body.text(run, |svg| _ = svg.attr("class", "titleText").attr_if("fill", &title_fill));

	let accessibility = Accessibility {
		title: database.acc_title.clone(),
		description: database.acc_description.clone(),
	};
	let frame = Frame { x: 0.0, y: 0.0, width, height, use_max_width: settings.use_max_width };
	Ok(document(context, "gantt", &frame, &accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Development aid: runs the cases of `GANTT_PROBE=<file>` (one per line, fields separated
	/// by tabs) with the system time zone and writes the results to `<file>.rust.out`, for
	/// comparison with the JavaScript originals.
	#[cfg(windows)]
	#[test]
	#[ignore]
	fn probe() {
		let Ok(path) = std::env::var("GANTT_PROBE") else { return };
		let clock = crate::mermaid_clock::system_clock();
		let calendar = Calendar { clock: &*clock };
		let cases = std::fs::read_to_string(&path).unwrap();
		let number = |text: &str| text.parse::<f64>().unwrap_or(f64::NAN);
		let mut output = String::new();
		for line in cases.lines() {
			let fields: Vec<&str> = line.split('\t').collect();
			let result = match fields.as_slice() {
				["parse", text] => js_number(calendar.parse(text)),
				["strict", input, format] => match calendar.parse_strict(input, format) {
					Ok(time) => js_number(time),
					Err(_) => "throw".to_owned(),
				},
				["format", time, format] => {
					calendar.format(number(time), format).unwrap_or_else(|_| "throw".to_owned())
				}
				["add", time, amount, unit] => {
					js_number(calendar.add(number(time), number(amount), unit))
				}
				["diffy", first, second] => {
					js_number(calendar.diff_years(number(first), number(second)))
				}
				["ticks", start, stop, interval] => {
					let interval = interval.split_once(':').and_then(|(unit, step)| {
						let unit = match unit {
							"timeMillisecond" => Unit::Millisecond,
							"timeSecond" => Unit::Second,
							"timeMinute" => Unit::Minute,
							"timeHour" => Unit::Hour,
							"timeDay" => Unit::Day,
							"timeSunday" => Unit::Week(0),
							"timeMonday" => Unit::Week(1),
							"timeThursday" => Unit::Week(4),
							"timeMonth" => Unit::Month,
							_ => Unit::Year,
						};
						Interval::every(unit, number(step))
					});
					ticks(&calendar, number(start), number(stop), interval)
						.iter()
						.map(|&tick| js_number(tick))
						.collect::<Vec<_>>()
						.join(",")
				}
				["d3fmt", time, specifier] => time_format(&calendar, specifier, number(time)),
				["tz", time] => js_number(calendar.timezone_offset(number(time))),
				["local", year, month, date, hours, minutes] => {
					js_number(calendar.make(&date::Fields {
						year: number(year),
						month: number(month),
						date: number(date),
						weekday: 0.0,
						hours: number(hours),
						minutes: number(minutes),
						seconds: 0.0,
						milliseconds: 0.0,
					}))
				}
				_ => "?".to_owned(),
			};
			output.push_str(&result);
			output.push('\n');
		}
		std::fs::write(format!("{path}.rust.out"), output).unwrap();
	}

	#[test]
	fn orders_keys_like_javascript_objects() {
		let mut keys =
			vec!["b".to_owned(), "10".to_owned(), "a".to_owned(), "2".to_owned(), "01".to_owned()];
		object_key_order(&mut keys);
		assert_eq!(keys, ["2", "10", "b", "a", "01"]);
	}

	#[test]
	fn reads_tick_intervals() {
		assert_eq!(tick_interval("2week", "monday"), Interval::every(Unit::Week(1), 2.0));
		assert_eq!(tick_interval("1day", "sunday"), Interval::every(Unit::Day, 1.0));
		assert_eq!(tick_interval("0day", "sunday"), None);
		assert_eq!(tick_interval("1 day", "sunday"), None);
		assert_eq!(tick_interval("1year", "sunday"), None);
	}
}
