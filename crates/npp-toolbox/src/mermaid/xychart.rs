//! XY charts (`xychart.jison`, `xychartDb.ts`, `chartBuilder/*`, `xychartRenderer.ts`), with d3's
//! `scaleLinear`, `scaleBand`, `ticks` and `line`.

mod parser;

use std::collections::HashMap;

use self::parser::{PlotKind, XAxisData, XyChart};
use super::color;
use super::common::js_number_of;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::value::Value;
use super::{Context, Diagram, Error, Result};

const SECTION: &str = "xyChart";

/// `XYChartAxisConfig`
struct AxisSettings {
	show_label: bool,
	label_font_size: f64,
	label_padding: f64,
	show_title: bool,
	title_font_size: f64,
	title_padding: f64,
	show_tick: bool,
	tick_length: f64,
	tick_width: f64,
	show_axis_line: bool,
	axis_line_width: f64,
	label_rotation: f64,
}

impl AxisSettings {
	fn new(context: &Context, axis: &str) -> Self {
		let value = |key: &str| context.config.path(&[SECTION, axis, key]);
		let number = |key: &str, default: f64| {
			value(key)
				.and_then(Value::as_number)
				.filter(|number| number.is_finite())
				.unwrap_or(default)
		};
		let flag = |key: &str| value(key).and_then(Value::as_bool).unwrap_or(true);
		AxisSettings {
			show_label: flag("showLabel"),
			label_font_size: number("labelFontSize", 14.0),
			label_padding: number("labelPadding", 5.0),
			show_title: flag("showTitle"),
			title_font_size: number("titleFontSize", 16.0),
			title_padding: number("titlePadding", 5.0),
			show_tick: flag("showTick"),
			tick_length: number("tickLength", 5.0),
			tick_width: number("tickWidth", 2.0),
			show_axis_line: flag("showAxisLine"),
			axis_line_width: number("axisLineWidth", 2.0),
			label_rotation: number("labelRotation", 0.0),
		}
	}
}

/// `XYChartConfig`
struct Settings {
	width: f64,
	height: f64,
	title_font_size: f64,
	title_padding: f64,
	show_title: bool,
	show_legend: bool,
	legend_font_size: f64,
	legend_padding: f64,
	show_data_label: bool,
	show_data_label_outside_bar: bool,
	is_horizontal: bool,
	plot_reserved_space_percent: f64,
}

impl Settings {
	fn new(context: &Context, chart: &XyChart) -> Self {
		let number = |key: &str, default: f64| context.config_number(SECTION, key, default);
		let flag = |key: &str, default: bool| context.config_bool(SECTION, key, default);
		let configured_horizontal = context
			.config_text(SECTION, "chartOrientation")
			.is_some_and(|text| text == "horizontal");
		Settings {
			width: number("width", 700.0),
			height: number("height", 500.0),
			title_font_size: number("titleFontSize", 20.0),
			title_padding: number("titlePadding", 10.0),
			show_title: flag("showTitle", true),
			show_legend: flag("showLegend", true),
			legend_font_size: number("legendFontSize", 14.0),
			legend_padding: number("legendPadding", 10.0),
			show_data_label: flag("showDataLabel", false),
			show_data_label_outside_bar: flag("showDataLabelOutsideBar", false),
			is_horizontal: chart.is_horizontal.unwrap_or(configured_horizontal),
			plot_reserved_space_percent: number("plotReservedSpacePercent", 50.0),
		}
	}
}

/// `XYChartThemeConfig`: the theme's `xyChart` colors.
struct Colors<'a> {
	theme: Option<&'a Value>,
}

impl Colors<'_> {
	fn get(&self, key: &str) -> String {
		self.theme.and_then(|colors| colors.get(key)).and_then(Value::to_text).unwrap_or_default()
	}
}

#[derive(Clone, Copy, Debug, Default)]
struct Rect {
	x: f64,
	y: f64,
	width: f64,
	height: f64,
}

/// `TextDimensionCalculatorWithFont`: the largest `getBoundingClientRect` of the texts in a
/// hidden group with the font size.
struct Measurer<'c, 'm> {
	context: &'c mut Context<'m>,
	family: String,
}

impl Measurer<'_, '_> {
	fn max_dimension<'t>(
		&mut self,
		texts: impl IntoIterator<Item = &'t str>,
		size: f64,
	) -> (f64, f64) {
		let font = Font::new(self.family.clone(), size);
		let mut dimension = (0.0f64, 0.0f64);
		for text in texts {
			if text.trim().is_empty() {
				continue;
			}
			let (width, height) = self.context.bounding_box(text, &font);
			dimension = (dimension.0.max(width), dimension.1.max(height));
		}
		dimension
	}
}

/// d3's `ticks(start, stop, 10)` (d3-array 3), the default ticks of `scaleLinear`.
fn ticks(start: f64, stop: f64) -> Vec<f64> {
	if start == stop {
		return vec![start];
	}
	let reverse = stop < start;
	let (low, high) = if reverse { (stop, start) } else { (start, stop) };
	let (first, last, increment) = tick_spec(low, high);
	// Also `NaN` from domains with `NaN` or infinities.
	if last.is_nan() || first.is_nan() || last < first {
		return Vec::new();
	}
	// More than a few thousand ticks only come from numbers without enough precision.
	let length = ((last - first) as usize).saturating_add(1).min(10_000);
	let tick = |index: f64| if increment < 0.0 { index / -increment } else { index * increment };
	(0..length)
		.map(|offset| match reverse {
			true => tick(last - offset as f64),
			false => tick(first + offset as f64),
		})
		.collect()
}

/// d3's `tickSpec` for 10 ticks: the first and last tick index and the increment, negative for
/// its inverse.
fn tick_spec(start: f64, stop: f64) -> (f64, f64, f64) {
	const COUNT: f64 = 10.0;
	let (e10, e5, e2) = (50f64.sqrt(), 10f64.sqrt(), 2f64.sqrt());
	let step = (stop - start) / COUNT;
	let power = step.log10().floor();
	let error = step / 10f64.powf(power);
	let factor = if error >= e10 {
		10.0
	} else if error >= e5 {
		5.0
	} else if error >= e2 {
		2.0
	} else {
		1.0
	};
	if power < 0.0 {
		let inverse = 10f64.powf(-power) / factor;
		let mut first = color::js_round(start * inverse);
		let mut last = color::js_round(stop * inverse);
		if first / inverse < start {
			first += 1.0;
		}
		if last / inverse > stop {
			last -= 1.0;
		}
		(first, last, -inverse)
	} else {
		let increment = 10f64.powf(power) * factor;
		let mut first = color::js_round(start / increment);
		let mut last = color::js_round(stop / increment);
		if first * increment < start {
			first += 1.0;
		}
		if last * increment > stop {
			last -= 1.0;
		}
		(first, last, increment)
	}
}

/// d3's `scaleLinear()(value)` for a domain and range of two numbers.
fn scale_linear(domain: (f64, f64), range: (f64, f64), value: f64) -> f64 {
	let ((d0, d1), (r0, r1)) = match domain.1 < domain.0 {
		true => ((domain.1, domain.0), (range.1, range.0)),
		false => (domain, range),
	};
	let span = d1 - d0;
	let t = if span != 0.0 && !span.is_nan() {
		(value - d0) / span
	} else if span.is_nan() {
		f64::NAN
	} else {
		0.5
	};
	r0 * (1.0 - t) + r1 * t
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
	Left,
	Bottom,
	Top,
}

enum Scale {
	/// The unique categories with their index, and the category of each tick.
	Band {
		domain: HashMap<String, usize>,
		unique: usize,
		categories: Vec<String>,
	},
	Linear {
		min: f64,
		max: f64,
	},
}

/// `BaseAxis` with `BandAxis` and `LinearAxis`.
struct Axis {
	scale: Scale,
	title: String,
	settings: AxisSettings,
	title_color: String,
	label_color: String,
	tick_color: String,
	line_color: String,
	bounds: Rect,
	position: Position,
	range: (f64, f64),
	show_title: bool,
	show_label: bool,
	show_tick: bool,
	show_axis_line: bool,
	outer_padding: f64,
	title_text_height: f64,
	rotation: f64,
	labels: Vec<String>,
	label_dimension: Option<(f64, f64)>,
}

impl Axis {
	fn new(
		scale: Scale,
		title: String,
		settings: AxisSettings,
		colors: &Colors,
		prefix: &str,
	) -> Self {
		let rotation = settings.label_rotation;
		let rotation = match (-90.0..=90.0).contains(&rotation) {
			true => rotation * std::f64::consts::PI / 180.0,
			false => 0.0,
		};
		Axis {
			scale,
			title,
			settings,
			title_color: colors.get(&format!("{prefix}TitleColor")),
			label_color: colors.get(&format!("{prefix}LabelColor")),
			tick_color: colors.get(&format!("{prefix}TickColor")),
			line_color: colors.get(&format!("{prefix}LineColor")),
			bounds: Rect::default(),
			position: Position::Left,
			range: (0.0, 10.0),
			show_title: false,
			show_label: false,
			show_tick: false,
			show_axis_line: false,
			outer_padding: 0.0,
			title_text_height: 0.0,
			rotation,
			labels: Vec::new(),
			label_dimension: None,
		}
	}

	fn band(
		categories: Vec<String>,
		title: String,
		settings: AxisSettings,
		colors: &Colors,
		prefix: &str,
	) -> Self {
		let mut domain = HashMap::new();
		for category in &categories {
			let next = domain.len();
			domain.entry(category.clone()).or_insert(next);
		}
		let unique = domain.len();
		Axis::new(Scale::Band { domain, unique, categories }, title, settings, colors, prefix)
	}

	fn is_vertical(&self) -> bool {
		self.position == Position::Left
	}

	fn set_range(&mut self, range: (f64, f64)) {
		self.range = range;
		match self.is_vertical() {
			true => self.bounds.height = range.1 - range.0,
			false => self.bounds.width = range.1 - range.0,
		}
	}

	fn set_position(&mut self, position: Position) {
		self.position = position;
		self.set_range(self.range);
		// The tick values of a linear scale follow its domain, which the position reverses.
		self.labels = self.tick_values().iter().map(|tick| tick.label()).collect();
	}

	/// `getRange`
	fn scaled_range(&self) -> (f64, f64) {
		(self.range.0 + self.outer_padding, self.range.1 - self.outer_padding)
	}

	fn linear_domain(&self, min: f64, max: f64) -> (f64, f64) {
		match self.position {
			Position::Left => (max, min),
			_ => (min, max),
		}
	}

	fn tick_values(&self) -> Vec<Tick> {
		match &self.scale {
			Scale::Band { categories, .. } => {
				categories.iter().cloned().map(Tick::Category).collect()
			}
			&Scale::Linear { min, max } => {
				let (start, stop) = self.linear_domain(min, max);
				ticks(start, stop).into_iter().map(Tick::Number).collect()
			}
		}
	}

	fn tick_count(&self) -> usize {
		self.labels.len()
	}

	/// `getScaleValue`: a category, or a number given as text like the database keeps it.
	fn scale_value(&self, key: &str) -> f64 {
		match &self.scale {
			Scale::Band { domain, unique, .. } => {
				let range = self.scaled_range();
				let Some(&index) = domain.get(key) else {
					return range.0;
				};
				// d3's `scaleBand` with `paddingInner(1)`, `paddingOuter(0)`, `align(0.5)`.
				let reverse = range.1 < range.0;
				let (start, stop) = if reverse { (range.1, range.0) } else { range };
				let count = *unique as f64;
				let step = (stop - start) / f64::max(1.0, count - 1.0);
				let start = start + (stop - start - step * (count - 1.0)) * 0.5;
				let index = if reverse { unique - 1 - index } else { index };
				start + step * index as f64
			}
			Scale::Linear { .. } => self.scale_number(js_number_of(key)),
		}
	}

	fn scale_number(&self, value: f64) -> f64 {
		match self.scale {
			Scale::Linear { min, max } => {
				scale_linear(self.linear_domain(min, max), self.scaled_range(), value)
			}
			Scale::Band { .. } => self.scaled_range().0,
		}
	}

	fn tick_position(&self, tick: &Tick) -> f64 {
		match tick {
			Tick::Category(category) => self.scale_value(category),
			Tick::Number(value) => self.scale_number(*value),
		}
	}

	fn tick_distance(&self) -> f64 {
		let range = self.scaled_range();
		(range.0 - range.1).abs() / self.tick_count() as f64
	}

	fn label_dimension(&mut self, measurer: &mut Measurer) -> (f64, f64) {
		if let Some(dimension) = self.label_dimension {
			return dimension;
		}
		let dimension = measurer
			.max_dimension(self.labels.iter().map(String::as_str), self.settings.label_font_size);
		self.label_dimension = Some(dimension);
		dimension
	}

	/// `recalculateOuterPaddingToDrawBar`
	fn make_room_for_bars(&mut self) {
		const BAR_WIDTH_TO_TICK_WIDTH_RATIO: f64 = 0.7;
		let width = BAR_WIDTH_TO_TICK_WIDTH_RATIO * self.tick_distance();
		if width > self.outer_padding * 2.0 {
			self.outer_padding = (width / 2.0).floor();
		}
	}

	/// `calculateSpace`
	fn calculate_space(&mut self, available: (f64, f64), measurer: &mut Measurer) -> (f64, f64) {
		const MAX_OUTER_PADDING_PERCENT_FOR_WRT_LABEL: f64 = 0.2;
		let settings = &self.settings;
		let (along, mut across) = match self.is_vertical() {
			true => (available.1, available.0),
			false => (available.0, available.1),
		};
		if settings.show_axis_line && across > settings.axis_line_width {
			across -= settings.axis_line_width;
			self.show_axis_line = true;
		}
		if self.settings.show_label {
			let (width, height) = self.label_dimension(measurer);
			let settings = &self.settings;
			let max_padding = MAX_OUTER_PADDING_PERCENT_FOR_WRT_LABEL * along;
			let required = match self.is_vertical() {
				true => {
					self.outer_padding = f64::min(height / 2.0, max_padding);
					width + settings.label_padding * 2.0
				}
				false => {
					self.outer_padding = f64::min(width / 2.0, max_padding);
					let mut required = height;
					if self.position == Position::Bottom && self.rotation != 0.0 {
						required = required.max(
							(self.rotation.sin() * width).abs()
								+ (self.rotation.cos() * height).abs(),
						);
					}
					required + settings.label_padding * 2.0
				}
			};
			if required <= across {
				across -= required;
				self.show_label = true;
			}
		}
		let settings = &self.settings;
		if settings.show_tick && across >= settings.tick_length {
			self.show_tick = true;
			across -= settings.tick_length;
		}
		if settings.show_title && !self.title.is_empty() {
			let (_, height) =
				measurer.max_dimension([self.title.as_str()], settings.title_font_size);
			let required = height + settings.title_padding * 2.0;
			self.title_text_height = height;
			if required <= across {
				across -= required;
				self.show_title = true;
			}
		}
		match self.is_vertical() {
			true => {
				self.bounds.width = available.0 - across;
				self.bounds.height = available.1;
			}
			false => {
				self.bounds.width = available.0;
				self.bounds.height = available.1 - across;
			}
		}
		(self.bounds.width, self.bounds.height)
	}

	/// `calculateOffsetByRotation`
	fn rotation_offset(&self, reference: f64) -> f64 {
		match self.rotation {
			0.0 => 0.0,
			rotation => rotation.sin() * reference / 2.0,
		}
	}

	fn elements(&self, elements: &mut Vec<Element>) {
		let settings = &self.settings;
		let line_width = if self.show_axis_line { settings.axis_line_width } else { 0.0 };
		let tick_length = if self.show_tick { settings.tick_length } else { 0.0 };
		let ticks = self.tick_values();
		let bounds = self.bounds;
		let path = |path: String, color: &String, width: f64| Shape::Path {
			path,
			fill: String::new(),
			stroke: color.clone(),
			stroke_width: width,
		};
		let label = |text: String,
		             x: f64,
		             y: f64,
		             rotation: f64,
		             vertical: VerticalPosition,
		             horizontal: Anchor| {
			Shape::Text(Label {
				text,
				x,
				y,
				fill: self.label_color.clone(),
				font_size: settings.label_font_size,
				rotation,
				vertical,
				horizontal,
			})
		};
		let title = |x: f64, y: f64, rotation: f64| {
			Shape::Text(Label {
				text: self.title.clone(),
				x,
				y,
				fill: self.title_color.clone(),
				font_size: settings.title_font_size,
				rotation,
				vertical: VerticalPosition::Top,
				horizontal: Anchor::Middle,
			})
		};
		match self.position {
			Position::Left => {
				let groups = ["left-axis", "axisl-line", "label", "ticks", "title"];
				if self.show_axis_line {
					let x = bounds.x + bounds.width - settings.axis_line_width / 2.0;
					let line = format!(
						"M {},{} L {},{}",
						number(x),
						number(bounds.y),
						number(x),
						number(bounds.y + bounds.height)
					);
					let shape = path(line, &self.line_color, settings.axis_line_width);
					elements.push(Element::new(&groups[..2], vec![shape]));
				}
				if self.show_label {
					let x =
						bounds.x + bounds.width - settings.label_padding - tick_length - line_width;
					let shapes = ticks
						.iter()
						.map(|tick| {
							let y = self.tick_position(tick);
							label(tick.label(), x, y, 0.0, VerticalPosition::Middle, Anchor::End)
						})
						.collect();
					elements.push(Element::new(&[groups[0], groups[2]], shapes));
				}
				if self.show_tick {
					let x = bounds.x + bounds.width - line_width;
					let shapes = ticks
						.iter()
						.map(|tick| {
							let y = number(self.tick_position(tick));
							let line = format!(
								"M {},{y} L {},{y}",
								number(x),
								number(x - settings.tick_length)
							);
							path(line, &self.tick_color, settings.tick_width)
						})
						.collect();
					elements.push(Element::new(&[groups[0], groups[3]], shapes));
				}
				if self.show_title {
					let shape = title(
						bounds.x + settings.title_padding,
						bounds.y + bounds.height / 2.0,
						270.0,
					);
					elements.push(Element::new(&[groups[0], groups[4]], vec![shape]));
				}
			}
			Position::Bottom => {
				let groups = ["bottom-axis", "axis-line", "label", "ticks", "title"];
				if self.show_axis_line {
					let y = number(bounds.y + settings.axis_line_width / 2.0);
					let line = format!(
						"M {},{y} L {},{y}",
						number(bounds.x),
						number(bounds.x + bounds.width)
					);
					let shape = path(line, &self.line_color, settings.axis_line_width);
					elements.push(Element::new(&groups[..2], vec![shape]));
				}
				if self.show_label {
					let (width, height) = self.label_dimension.unwrap_or_default();
					let y = bounds.y
						+ settings.label_padding
						+ tick_length + line_width
						+ self.rotation_offset(width).abs();
					let rotation = self.rotation * 180.0 / std::f64::consts::PI;
					let shapes = ticks
						.iter()
						.map(|tick| {
							let x = self.tick_position(tick) + self.rotation_offset(height);
							label(
								tick.label(),
								x,
								y,
								rotation,
								VerticalPosition::Top,
								Anchor::Middle,
							)
						})
						.collect();
					elements.push(Element::new(&[groups[0], groups[2]], shapes));
				}
				if self.show_tick {
					let y = bounds.y + line_width;
					let shapes = ticks
						.iter()
						.map(|tick| {
							let x = number(self.tick_position(tick));
							let line = format!(
								"M {x},{} L {x},{}",
								number(y),
								number(y + settings.tick_length)
							);
							path(line, &self.tick_color, settings.tick_width)
						})
						.collect();
					elements.push(Element::new(&[groups[0], groups[3]], shapes));
				}
				if self.show_title {
					let x = self.range.0 + (self.range.1 - self.range.0) / 2.0;
					let y =
						bounds.y + bounds.height - settings.title_padding - self.title_text_height;
					elements.push(Element::new(&[groups[0], groups[4]], vec![title(x, y, 0.0)]));
				}
			}
			Position::Top => {
				let groups = ["top-axis", "axis-line", "label", "ticks", "title"];
				if self.show_axis_line {
					let y = number(bounds.y + bounds.height - settings.axis_line_width / 2.0);
					let line = format!(
						"M {},{y} L {},{y}",
						number(bounds.x),
						number(bounds.x + bounds.width)
					);
					let shape = path(line, &self.line_color, settings.axis_line_width);
					elements.push(Element::new(&groups[..2], vec![shape]));
				}
				if self.show_label {
					let title_space = match self.show_title {
						true => self.title_text_height + settings.title_padding * 2.0,
						false => 0.0,
					};
					let y = bounds.y + title_space + settings.label_padding;
					let shapes = ticks
						.iter()
						.map(|tick| {
							let x = self.tick_position(tick);
							label(tick.label(), x, y, 0.0, VerticalPosition::Top, Anchor::Middle)
						})
						.collect();
					elements.push(Element::new(&[groups[0], groups[2]], shapes));
				}
				if self.show_tick {
					let start = bounds.y + bounds.height - line_width;
					let shapes = ticks
						.iter()
						.map(|tick| {
							let x = number(self.tick_position(tick));
							let line = format!(
								"M {x},{} L {x},{}",
								number(start),
								number(start - settings.tick_length)
							);
							path(line, &self.tick_color, settings.tick_width)
						})
						.collect();
					elements.push(Element::new(&[groups[0], groups[3]], shapes));
				}
				if self.show_title {
					let x = bounds.x + bounds.width / 2.0;
					let shape = title(x, bounds.y + settings.title_padding, 0.0);
					elements.push(Element::new(&[groups[0], groups[4]], vec![shape]));
				}
			}
		}
	}
}

enum Tick {
	Category(String),
	Number(f64),
}

impl Tick {
	fn label(&self) -> String {
		match self {
			Tick::Category(category) => category.clone(),
			Tick::Number(value) => color::js_number(*value),
		}
	}
}

/// `TextVerticalPos`
#[derive(Clone, Copy)]
enum VerticalPosition {
	Top,
	Middle,
}

/// `TextElem`
struct Label {
	text: String,
	x: f64,
	y: f64,
	fill: String,
	font_size: f64,
	rotation: f64,
	vertical: VerticalPosition,
	horizontal: Anchor,
}

enum Shape {
	Rect { bounds: Rect, fill: String },
	Text(Label),
	Path { path: String, fill: String, stroke: String, stroke_width: f64 },
}

/// `DrawableElem`: shapes in nested groups.
struct Element {
	groups: Vec<String>,
	shapes: Vec<Shape>,
	/// The bar values of the data labels, for bar plots.
	is_bar_plot: bool,
}

impl Element {
	fn new(groups: &[&str], shapes: Vec<Shape>) -> Self {
		Element {
			groups: groups.iter().map(|group| (*group).to_owned()).collect(),
			shapes,
			is_bar_plot: false,
		}
	}
}

/// The legend's sizes relative to its font size.
struct LegendLayout {
	font_size: f64,
	marker_size: f64,
	marker_spacing: f64,
	item_spacing: f64,
}

impl LegendLayout {
	fn new(font_size: f64) -> Self {
		LegendLayout {
			font_size,
			marker_size: font_size * 0.75,
			marker_spacing: font_size * 0.35,
			item_spacing: font_size * 0.5,
		}
	}
}

/// The chart components (`Orchestrator`).
struct Builder<'a> {
	settings: &'a Settings,
	chart: &'a XyChart,
	colors: &'a Colors<'a>,
	x_axis: Axis,
	y_axis: Axis,
	plot: Rect,
	title: Rect,
	title_width: f64,
	show_title: bool,
	legend: Rect,
	legend_plots: Vec<usize>,
}

impl Builder<'_> {
	fn title_space(&mut self, available: (f64, f64), measurer: &mut Measurer) -> (f64, f64) {
		let settings = self.settings;
		let (width, height) =
			measurer.max_dimension([self.chart.title.as_str()], settings.title_font_size);
		let required = (width.max(available.0), height + 2.0 * settings.title_padding);
		if required.1 <= available.1 && settings.show_title && !self.chart.title.is_empty() {
			(self.title.width, self.title.height) = required;
			self.title_width = width;
			self.show_title = true;
		}
		(self.title.width, self.title.height)
	}

	/// `alignTo`: centered over the plot, but inside the chart.
	fn align_title(&mut self, start: f64, end: f64) {
		let half_width = f64::min(self.title_width, self.settings.width) / 2.0;
		let center =
			f64::min(f64::max((start + end) / 2.0, half_width), self.settings.width - half_width);
		self.title.x = center - half_width;
		self.title.width = half_width * 2.0;
	}

	fn legend_space(&mut self, available: (f64, f64), measurer: &mut Measurer) -> (f64, f64) {
		self.legend_plots = match self.settings.show_legend {
			true => (0..self.chart.plots.len())
				.filter(|&index| !self.chart.plots[index].title.is_empty())
				.collect(),
			false => Vec::new(),
		};
		if self.legend_plots.is_empty() {
			(self.legend.width, self.legend.height) = (0.0, 0.0);
			return (0.0, 0.0);
		}
		let layout = LegendLayout::new(self.settings.legend_font_size);
		let titles = self.legend_plots.iter().map(|&index| self.chart.plots[index].title.as_str());
		let (text_width, _) = measurer.max_dimension(titles, layout.font_size);
		let padding = self.settings.legend_padding;
		let count = self.legend_plots.len() as f64;
		let width = padding * 2.0 + layout.marker_size + layout.marker_spacing + text_width;
		let height = padding * 2.0 + count * layout.font_size + (count - 1.0) * layout.item_spacing;
		match width <= available.0 && height <= available.1 {
			true => (self.legend.width, self.legend.height) = (width, height),
			false => {
				self.legend_plots.clear();
				(self.legend.width, self.legend.height) = (0.0, 0.0);
			}
		}
		(self.legend.width, self.legend.height)
	}

	/// `calculateVerticalSpace` and `calculateHorizontalSpace`
	fn calculate_space(&mut self, measurer: &mut Measurer) {
		let settings = self.settings;
		let mut available_width = settings.width;
		let mut available_height = settings.height;
		let mut chart_width =
			(available_width * settings.plot_reserved_space_percent / 100.0).floor();
		let mut chart_height =
			(available_height * settings.plot_reserved_space_percent / 100.0).floor();
		available_width -= chart_width;
		available_height -= chart_height;
		let (_, title_height) = self.title_space((settings.width, available_height), measurer);
		available_height -= title_height;
		let (plot_x, plot_y);
		if settings.is_horizontal {
			let title_end = title_height;
			self.x_axis.set_position(Position::Left);
			let (width, _) =
				self.x_axis.calculate_space((available_width, available_height), measurer);
			available_width -= width;
			plot_x = width;
			self.y_axis.set_position(Position::Top);
			let (_, height) =
				self.y_axis.calculate_space((available_width, available_height), measurer);
			available_height -= height;
			plot_y = title_end + height;
		} else {
			plot_y = title_height;
			self.x_axis.set_position(Position::Bottom);
			let (_, height) =
				self.x_axis.calculate_space((available_width, available_height), measurer);
			available_height -= height;
			self.y_axis.set_position(Position::Left);
			let (width, _) =
				self.y_axis.calculate_space((available_width, available_height), measurer);
			plot_x = width;
			available_width -= width;
		}
		let (legend_width, legend_height) =
			self.legend_space((available_width, chart_height), measurer);
		available_width -= legend_width;
		if available_width > 0.0 {
			chart_width += available_width;
		}
		if available_height > 0.0 {
			chart_height += available_height;
		}
		self.plot = Rect { x: plot_x, y: plot_y, width: chart_width, height: chart_height };
		self.align_title(plot_x, plot_x + chart_width);
		self.legend.x = plot_x + chart_width;
		self.legend.y = plot_y + f64::max((chart_height - legend_height) / 2.0, 0.0);
		if settings.is_horizontal {
			self.y_axis.set_range((plot_x, plot_x + chart_width));
			self.y_axis.bounds.x = plot_x;
			self.y_axis.bounds.y = title_height;
			self.x_axis.set_range((plot_y, plot_y + chart_height));
			self.x_axis.bounds.x = 0.0;
			self.x_axis.bounds.y = plot_y;
		} else {
			self.x_axis.set_range((plot_x, plot_x + chart_width));
			self.x_axis.bounds.x = plot_x;
			self.x_axis.bounds.y = plot_y + chart_height;
			self.y_axis.set_range((plot_y, plot_y + chart_height));
			self.y_axis.bounds.x = 0.0;
			self.y_axis.bounds.y = plot_y;
		}
		if self.chart.plots.iter().any(|plot| plot.kind == PlotKind::Bar) {
			self.x_axis.make_room_for_bars();
		}
	}

	fn title_elements(&self, elements: &mut Vec<Element>) {
		if !self.show_title {
			return;
		}
		let label = Label {
			text: self.chart.title.clone(),
			x: self.title.x + self.title.width / 2.0,
			y: self.title.y + self.title.height / 2.0,
			fill: self.colors.get("titleColor"),
			font_size: self.settings.title_font_size,
			rotation: 0.0,
			vertical: VerticalPosition::Middle,
			horizontal: Anchor::Middle,
		};
		elements.push(Element::new(&["chart-title"], vec![Shape::Text(label)]));
	}

	fn plot_elements(&self, elements: &mut Vec<Element>) {
		let is_horizontal = self.settings.is_horizontal;
		for (index, plot) in self.chart.plots.iter().enumerate() {
			let points: Vec<(f64, f64)> = plot
				.data
				.iter()
				.map(|(key, value)| {
					let y = value.map_or(f64::NAN, |value| self.y_axis.scale_number(value));
					(self.x_axis.scale_value(key), y)
				})
				.collect();
			match &plot.kind {
				PlotKind::Line { point_labels } => {
					let group = format!("line-plot-{index}");
					// d3's `line()`; browsers draw a path up to its first invalid number.
					let mut path = String::new();
					for &(x, y) in &points {
						let (x, y) = if is_horizontal { (y, x) } else { (x, y) };
						if !x.is_finite() || !y.is_finite() {
							break;
						}
						path.push(if path.is_empty() { 'M' } else { 'L' });
						path.push_str(&format!("{},{}", number(x), number(y)));
					}
					if points.is_empty() {
						continue;
					}
					let shape = Shape::Path {
						path,
						fill: String::new(),
						stroke: plot.color.clone(),
						stroke_width: 2.0,
					};
					elements.push(Element::new(&["plot", &group], vec![shape]));
					let Some(point_labels) = point_labels else {
						continue;
					};
					const LABEL_OFFSET: f64 = 10.0;
					let labels: Vec<Shape> = points
						.iter()
						.zip(point_labels)
						.filter(|(_, label)| !label.is_empty())
						.map(|(&(x, y), label)| {
							let (x, y, horizontal) = match is_horizontal {
								true => (y + LABEL_OFFSET, x, Anchor::Start),
								false => (x, y - LABEL_OFFSET, Anchor::Middle),
							};
							Shape::Text(Label {
								text: label.clone(),
								x,
								y,
								fill: plot.color.clone(),
								font_size: 12.0,
								rotation: 0.0,
								vertical: VerticalPosition::Middle,
								horizontal,
							})
						})
						.collect();
					if !labels.is_empty() {
						elements.push(Element::new(&["plot", &group, "labels"], labels));
					}
				}
				PlotKind::Bar => {
					const BAR_PADDING_PERCENT: f64 = 0.05;
					let bar_width =
						f64::min(self.x_axis.outer_padding * 2.0, self.x_axis.tick_distance())
							* (1.0 - BAR_PADDING_PERCENT);
					let half = bar_width / 2.0;
					let plot_area = self.plot;
					let shapes = points
						.iter()
						.map(|&(x, y)| {
							let bounds = match is_horizontal {
								true => Rect {
									x: plot_area.x,
									y: x - half,
									width: y - plot_area.x,
									height: bar_width,
								},
								false => Rect {
									x: x - half,
									y,
									width: bar_width,
									height: plot_area.y + plot_area.height - y,
								},
							};
							Shape::Rect { bounds, fill: plot.color.clone() }
						})
						.collect();
					let mut element = Element::new(&["plot", &format!("bar-plot-{index}")], shapes);
					element.is_bar_plot = true;
					elements.push(element);
				}
			}
		}
	}

	fn legend_elements(&self, elements: &mut Vec<Element>) {
		if self.legend_plots.is_empty() {
			return;
		}
		let layout = LegendLayout::new(self.settings.legend_font_size);
		let row_height = layout.font_size + layout.item_spacing;
		let start_x = self.legend.x + self.settings.legend_padding;
		let start_y = self.legend.y + self.settings.legend_padding;
		let (mut bars, mut lines, mut labels) = (Vec::new(), Vec::new(), Vec::new());
		for (row, &index) in self.legend_plots.iter().enumerate() {
			let plot = &self.chart.plots[index];
			let top = start_y + row as f64 * row_height;
			match plot.kind {
				PlotKind::Bar => bars.push(Shape::Rect {
					bounds: Rect {
						x: start_x,
						y: top,
						width: layout.marker_size,
						height: layout.marker_size,
					},
					fill: plot.color.clone(),
				}),
				PlotKind::Line { .. } => {
					let y = number(top + layout.marker_size / 2.0);
					lines.push(Shape::Path {
						path: format!(
							"M {},{y} L {},{y}",
							number(start_x),
							number(start_x + layout.marker_size)
						),
						fill: String::new(),
						stroke: plot.color.clone(),
						stroke_width: 2.0,
					});
				}
			}
			labels.push(Shape::Text(Label {
				text: plot.title.clone(),
				x: start_x + layout.marker_size + layout.marker_spacing,
				y: top + layout.marker_size / 2.0,
				fill: self.colors.get("legendTextColor"),
				font_size: layout.font_size,
				rotation: 0.0,
				vertical: VerticalPosition::Middle,
				horizontal: Anchor::Start,
			}));
		}
		elements.push(Element::new(&["legend", "markers"], bars));
		elements.push(Element::new(&["legend", "markers"], lines));
		elements.push(Element::new(&["legend", "label"], labels));
	}
}

/// A node of the group tree that `getGroup` builds.
enum Node {
	Group(usize),
	Markup(Svg),
}

#[derive(Default)]
struct Group {
	class: String,
	children: Vec<Node>,
}

struct Renderer<'s> {
	settings: &'s Settings,
	family: String,
	/// The diagram's font size, which texts with an invalid one inherit.
	font_size: f64,
	data_label_color: String,
	/// The values of the first plot, which Mermaid labels every bar plot with.
	label_values: Vec<Option<f64>>,
}

impl Renderer<'_> {
	fn text(&self, svg: &mut Svg, label: &Label) {
		let font = Font::new(self.family.clone(), label.font_size);
		let run = TextRun {
			x: 0.0,
			y: 0.0,
			anchor: label.horizontal,
			baseline: match label.vertical {
				VerticalPosition::Top => Baseline::TextBeforeEdge,
				VerticalPosition::Middle => Baseline::Middle,
			},
			font: &font,
			content: &label.text,
		};
		svg.text(run, |svg| {
			svg.attr_if("fill", &label.fill);
			// Browsers drop a transform with `NaN`.
			if label.x.is_finite() && label.y.is_finite() {
				let transform = format!(
					"translate({}, {}) rotate({})",
					number(label.x),
					number(label.y),
					number(label.rotation)
				);
				svg.attr("transform", &transform);
			}
		});
	}

	fn shapes(&self, element: &Element) -> Result<Svg> {
		let mut svg = Svg::new();
		for shape in &element.shapes {
			match shape {
				Shape::Rect { bounds, fill } => {
					svg.open("rect")
						.number_attr("x", bounds.x)
						.number_attr("y", bounds.y)
						.number_attr("width", bounds.width)
						.number_attr("height", bounds.height)
						.attr_if("fill", fill)
						.attr_if("stroke", fill)
						.attr("stroke-width", "0")
						.close();
				}
				Shape::Text(label) => self.text(&mut svg, label),
				Shape::Path { path, fill, stroke, stroke_width } => {
					svg.open("path")
						.attr("d", path)
						.attr("fill", if fill.is_empty() { "none" } else { fill })
						.attr_if("stroke", stroke)
						.number_attr("stroke-width", *stroke_width)
						.close();
				}
			}
		}
		if element.is_bar_plot && self.settings.show_data_label {
			self.data_labels(&mut svg, element)?;
		}
		Ok(svg)
	}

	/// The value labels of bars, all in the largest font size that fits every bar.
	fn data_labels(&self, svg: &mut Svg, element: &Element) -> Result<()> {
		const CHAR_WIDTH_FACTOR: f64 = 0.7;
		let mut items = Vec::new();
		for (index, shape) in element.shapes.iter().enumerate() {
			let Shape::Rect { bounds, .. } = shape else {
				continue;
			};
			let Some(&Some(value)) = self.label_values.get(index) else {
				return Err(Error::new(format!(
					"The first plot has no value for the data label of bar {}",
					index + 1
				)));
			};
			if bounds.width > 0.0 && bounds.height > 0.0 {
				items.push((*bounds, color::js_number(value)));
			}
		}
		let is_horizontal = self.settings.is_horizontal;
		let outside = self.settings.show_data_label_outside_bar;
		let font_sizes = items.iter().map(|(bounds, label)| {
			let text_width = |font_size: f64| font_size * label.len() as f64 * CHAR_WIDTH_FACTOR;
			let (start, fits): (f64, Box<dyn Fn(f64) -> bool>) = match is_horizontal {
				true => (
					bounds.height * 0.7,
					Box::new(move |font_size| text_width(font_size) <= bounds.width - 10.0),
				),
				false => (
					bounds.width / (label.len() as f64 * CHAR_WIDTH_FACTOR),
					Box::new(move |font_size| {
						let center = bounds.x + bounds.width / 2.0;
						let half = text_width(font_size) / 2.0;
						center - half >= bounds.x
							&& center + half <= bounds.x + bounds.width
							&& bounds.y + 10.0 + font_size <= bounds.y + bounds.height
					}),
				),
			};
			decrease_until_fit(start, fits)
		});
		// `Math.min`, which is `NaN` if any is.
		let uniform = font_sizes
			.fold(f64::INFINITY, |min, size| {
				if min.is_nan() || size.is_nan() { f64::NAN } else { min.min(size) }
			})
			.floor();
		// An invalid font size (negative, `NaN`) inherits the diagram's.
		let font_size =
			if uniform >= 0.0 && uniform.is_finite() { uniform } else { self.font_size };
		let font = Font::new(self.family.clone(), font_size);
		for (bounds, label) in &items {
			let (x, y, anchor, baseline) = match is_horizontal {
				true => (
					if outside {
						bounds.x + bounds.width + 10.0
					} else {
						bounds.x + bounds.width - 10.0
					},
					bounds.y + bounds.height / 2.0,
					if outside { Anchor::Start } else { Anchor::End },
					Baseline::Middle,
				),
				false => (
					bounds.x + bounds.width / 2.0,
					if outside { bounds.y - 10.0 } else { bounds.y + 10.0 },
					Anchor::Middle,
					if outside { Baseline::Alphabetic } else { Baseline::Hanging },
				),
			};
			let run = TextRun { x, y, anchor, baseline, font: &font, content: label };
			svg.text(run, |svg| _ = svg.attr_if("fill", &self.data_label_color));
		}
		Ok(())
	}
}

/// `fontSize -= 1` until the label fits or the size is not positive, as Mermaid loops, but
/// skipping the steps that cannot fit.
fn decrease_until_fit(start: f64, fits: impl Fn(f64) -> bool) -> f64 {
	if !start.is_finite() {
		return f64::NAN;
	}
	let mut font_size = start;
	// Subtracting whole steps from a number below 2^53 is exact, so a jump lands where the
	// loop would.
	if font_size > 64.0 && !fits(font_size) {
		let (mut low, mut high) = (0.0f64, font_size.floor());
		while high - low > 1.0 {
			let middle = ((low + high) / 2.0).floor();
			match fits(font_size - middle) {
				true => high = middle,
				false => low = middle,
			}
		}
		font_size -= low;
	}
	let mut steps = 0;
	while !fits(font_size) && font_size > 0.0 && steps < 1000 {
		font_size -= 1.0;
		steps += 1;
	}
	font_size
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let chart = parser::parse(code, context)?;
	if chart.plots.is_empty() {
		return Err(Error::new("No Plot to render, please provide a plot with some data"));
	}
	let settings = Settings::new(context, &chart);
	let theme_colors = context.theme.get("xyChart").cloned();
	let colors = Colors { theme: theme_colors.as_ref() };
	let x_axis = match &chart.x_axis {
		XAxisData::Band { categories } => Axis::band(
			categories.clone(),
			chart.x_axis_title.clone(),
			AxisSettings::new(context, "xAxis"),
			&colors,
			"xAxis",
		),
		&XAxisData::Linear { min, max } => Axis::new(
			Scale::Linear { min, max },
			chart.x_axis_title.clone(),
			AxisSettings::new(context, "xAxis"),
			&colors,
			"xAxis",
		),
	};
	let y_axis = Axis::new(
		Scale::Linear { min: chart.y_min, max: chart.y_max },
		chart.y_axis_title.clone(),
		AxisSettings::new(context, "yAxis"),
		&colors,
		"yAxis",
	);
	let family = context.font_family();
	let mut builder = Builder {
		settings: &settings,
		chart: &chart,
		colors: &colors,
		x_axis,
		y_axis,
		plot: Rect::default(),
		title: Rect::default(),
		title_width: 0.0,
		show_title: false,
		legend: Rect::default(),
		legend_plots: Vec::new(),
	};
	let mut elements = Vec::new();
	{
		let mut measurer = Measurer { context, family: family.clone() };
		builder.calculate_space(&mut measurer);
		builder.title_elements(&mut elements);
		builder.plot_elements(&mut elements);
		builder.legend_elements(&mut elements);
		builder.x_axis.elements(&mut elements);
		builder.y_axis.elements(&mut elements);
	}

	let renderer = Renderer {
		settings: &settings,
		family,
		font_size: context.font_size(),
		data_label_color: colors.get("dataLabelColor"),
		label_values: chart.plots[0].data.iter().map(|(_, value)| *value).collect(),
	};
	// `getGroup`: a group per prefix of the class list, created on first use.
	let mut groups = vec![Group::default()];
	let mut group_of_prefix: HashMap<String, usize> = HashMap::new();
	for element in &elements {
		if element.shapes.is_empty() {
			continue;
		}
		let (mut parent, mut prefix) = (0, String::new());
		for class in &element.groups {
			prefix.push_str(class);
			parent = match group_of_prefix.get(&prefix) {
				Some(&group) => group,
				None => {
					groups.push(Group { class: class.clone(), children: Vec::new() });
					let group = groups.len() - 1;
					groups[parent].children.push(Node::Group(group));
					group_of_prefix.insert(prefix.clone(), group);
					group
				}
			};
		}
		let markup = renderer.shapes(element)?;
		groups[parent].children.push(Node::Markup(markup));
	}

	let mut body = Svg::new();
	body.open("g").attr("class", "main").end();
	body.open("rect")
		.number_attr("width", settings.width)
		.number_attr("height", settings.height)
		.attr("class", "background")
		.attr_if("fill", &colors.get("backgroundColor"))
		.close();
	write_group(&mut body, &mut groups, 0);
	body.close_tag("g");
	let frame = Frame {
		x: 0.0,
		y: 0.0,
		width: settings.width,
		height: settings.height,
		use_max_width: true,
	};
	Ok(document(context, "xychart", &frame, &chart.accessibility, body))
}

/// Writes the children of a group; the groups nest at most four deep.
fn write_group(svg: &mut Svg, groups: &mut [Group], index: usize) {
	let children = std::mem::take(&mut groups[index].children);
	for child in children {
		match child {
			Node::Markup(markup) => svg.append(markup),
			Node::Group(group) => {
				svg.open("g").attr("class", &groups[group].class).end();
				write_group(svg, groups, group);
				svg.close_tag("g");
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ticks_like_d3() {
		assert_eq!(ticks(0.0, 10.0), [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
		assert_eq!(ticks(3.0, 1.0), [3.0, 2.8, 2.6, 2.4, 2.2, 2.0, 1.8, 1.6, 1.4, 1.2, 1.0]);
		assert_eq!(ticks(4000.0, 11000.0).len(), 15);
		assert_eq!(ticks(-1.5, 0.5)[0], -1.4);
		assert_eq!(ticks(5.0, 5.0), [5.0]);
		assert!(ticks(f64::INFINITY, f64::NEG_INFINITY).is_empty());
		assert!(ticks(f64::NAN, 1.0).is_empty());
	}

	#[test]
	fn scales_like_d3() {
		assert_eq!(scale_linear((0.0, 10.0), (100.0, 200.0), 5.0), 150.0);
		assert_eq!(scale_linear((10.0, 0.0), (100.0, 200.0), 2.0), 180.0);
		assert_eq!(scale_linear((5.0, 5.0), (0.0, 10.0), 7.0), 5.0);
	}

	#[test]
	fn decreases_font_sizes_like_the_loop() {
		let fits = |limit: f64| move |size: f64| size <= limit;
		for (start, limit) in [(30.5, 12.0), (1e9 + 0.25, 7.5), (3.0, -5.0), (100.0, 100.0)] {
			let mut expected = start;
			while !fits(limit)(expected) && expected > 0.0 {
				expected -= 1.0;
			}
			assert_eq!(decrease_until_fit(start, fits(limit)), expected, "{start} {limit}");
		}
	}
}
