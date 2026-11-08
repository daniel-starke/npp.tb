//! Mermaid diagrams to SVG, without JavaScript.
//!
//! The parsers follow Mermaid's grammars and the renderers its JavaScript implementation
//! (Mermaid 12.2.0), with presentation attributes instead of a style sheet, so that the SVG
//! looks the same inline in a browser and rendered by resvg. Text is measured by a
//! [`TextMeasure`], so that the layouts are pure and testable.

mod agentflow;
mod architecture;
mod block;
mod c4;
mod class_diagram;
mod color;
mod common;
mod cynefin;
mod er;
mod eventmodeling;
mod flowchart;
mod formatted_text;
mod gantt;
mod git_graph;
#[cfg(feature = "font-awesome")]
mod icon_table;
mod icons;
mod image;
mod info;
mod ishikawa;
mod jison;
mod journey;
mod kanban;
mod label;
mod langium;
mod layout;
mod math;
mod mindmap;
mod number_format;
mod packet;
mod pie;
mod preprocess;
mod quadrant_chart;
mod radar;
mod railroad;
mod rendering;
mod requirement;
mod rough;
mod sankey;
mod sequence;
mod state;
mod svg;
mod text;
mod theme;
mod theme_table;
mod timeline;
mod tree_view;
mod treemap;
mod usecase;
mod value;
mod venn;
mod wardley;
mod xychart;

use std::fmt;
use std::rc::Rc;

use theme::ThemeVariables;
use value::Value;

pub use image::ImageDecoder;
#[cfg(any(test, feature = "pure-rust"))]
pub use image::NoImageDecoder;
pub use text::{Font, Outline, TextMeasure, TextOutline};

/// Limits against documents that would block the editor, like Mermaid's `maxTextSize` and
/// `maxEdges`. Documents cannot change them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
	pub max_text_size: usize,
	pub max_edges: usize,
}

impl Default for Limits {
	fn default() -> Self {
		Self { max_text_size: 50_000, max_edges: 500 }
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Options {
	/// Mermaid's `dark` theme instead of `default`, unless the document chooses a theme.
	pub is_dark: bool,
	pub limits: Limits,
	/// `screen.availWidth` in CSS pixels, the width at which C4 diagrams wrap their rows.
	pub screen_width: u32,
}

/// `screen.availWidth` of headless Edge, used where the real screen is unknown.
pub const DEFAULT_SCREEN_WIDTH: u32 = 800;

/// A rendered diagram.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagram {
	/// A standalone `<svg>` element whose ids are unique for the diagram source and options.
	pub svg: String,
	/// Natural size in CSS pixels.
	pub width: f64,
	pub height: f64,
	/// The `<text>` elements in `svg`.
	pub texts: Vec<svg::RecordedText>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
	pub message: String,
	/// One-based line in the code block.
	pub line: Option<usize>,
}

impl Error {
	pub fn new(message: impl Into<String>) -> Self {
		Self { message: message.into(), line: None }
	}

	pub fn at(line: usize, message: impl Into<String>) -> Self {
		Self { message: message.into(), line: Some(line) }
	}
}

impl fmt::Display for Error {
	fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		match self.line {
			Some(line) => write!(formatter, "line {line}: {}", self.message),
			None => formatter.write_str(&self.message),
		}
	}
}

pub type Result<T> = std::result::Result<T, Error>;

/// The diagram types in the order of Mermaid's detectors: the first match wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramKind {
	Error,
	FrontMatterOnly,
	FlowchartElk,
	Mindmap,
	Architecture,
	Agentflow,
	C4,
	Kanban,
	Class,
	EntityRelationship,
	Gantt,
	Info,
	Pie,
	Requirement,
	Sequence,
	Swimlanes,
	Flowchart,
	Timeline,
	GitGraph,
	State,
	Journey,
	QuadrantChart,
	Sankey,
	Packet,
	XyChart,
	Block,
	EventModeling,
	TreeView,
	Radar,
	Ishikawa,
	Treemap,
	Railroad,
	RailroadEbnf,
	RailroadAbnf,
	RailroadPeg,
	Venn,
	Wardley,
	Cynefin,
	Usecase,
}

impl DiagramKind {
	/// The diagrams whose style sheet calls `getIconStyles`.
	fn has_icon_styles(self) -> bool {
		use DiagramKind::*;
		matches!(
			self,
			Flowchart | FlowchartElk | Swimlanes | Class | Block | Agentflow | Kanban | Journey
		)
	}

	/// Mermaid's `detectType` for `code` without front matter and directives.
	pub fn detect(code: &str) -> Option<DiagramKind> {
		let stripped: String = code
			.split('\n')
			.map(|line| match line.find("%%") {
				Some(start) => line[..start].trim_end(),
				None => line,
			})
			.collect::<Vec<_>>()
			.join("\n");
		let text = stripped.trim_start();
		let starts = |keyword: &str| text.starts_with(keyword);
		let starts_ignoring_case = |keyword: &str| {
			text.get(..keyword.len()).is_some_and(|start| start.eq_ignore_ascii_case(keyword))
		};
		// A word boundary after the keyword, like `\b`.
		let starts_word = |keyword: &str, ignore_case: bool| {
			let matches = match ignore_case {
				true => starts_ignoring_case(keyword),
				false => starts(keyword),
			};
			matches
				&& text[keyword.len()..]
					.chars()
					.next()
					.is_none_or(|next| !(next.is_alphanumeric() || next == '_'))
		};
		use DiagramKind::*;
		let kind = if text.trim_end().eq_ignore_ascii_case("error") {
			Error
		} else if text.to_ascii_lowercase().starts_with("---") {
			FrontMatterOnly
		} else if starts("flowchart-elk") {
			FlowchartElk
		} else if starts("mindmap") {
			Mindmap
		} else if starts("architecture") {
			Architecture
		} else if starts_word("agentflow-beta", false) {
			Agentflow
		// Mermaid's expression is `/^\s*C4Context|C4Container|…/`, so the other keywords match
		// anywhere.
		} else if starts("C4Context")
			|| ["C4Container", "C4Component", "C4Dynamic", "C4Deployment"]
				.iter()
				.any(|keyword| stripped.contains(keyword))
		{
			C4
		} else if starts("kanban") {
			Kanban
		} else if starts("classDiagram") {
			Class
		} else if starts("erDiagram") {
			EntityRelationship
		} else if starts("gantt") {
			Gantt
		} else if starts("info") {
			Info
		} else if starts("pie") {
			Pie
		} else if starts("requirement") {
			Requirement
		} else if starts("sequenceDiagram") {
			Sequence
		} else if starts_word("swimlane-beta", false) {
			Swimlanes
		} else if starts("graph") || starts("flowchart") {
			Flowchart
		} else if starts("timeline") {
			Timeline
		} else if starts("gitGraph") {
			GitGraph
		} else if starts("stateDiagram") {
			State
		} else if starts("journey") {
			Journey
		} else if starts("quadrantChart") {
			QuadrantChart
		} else if starts("sankey") {
			Sankey
		} else if starts("packet") {
			Packet
		} else if starts("xychart") {
			XyChart
		} else if starts("block") {
			Block
		} else if starts("eventmodeling") {
			EventModeling
		} else if starts("treeView-beta") {
			TreeView
		} else if starts("radar-beta") {
			Radar
		} else if starts_word("ishikawa", true) || starts_word("ishikawa-beta", true) {
			Ishikawa
		} else if starts("treemap") {
			Treemap
		} else if starts_ignoring_case("railroad-beta") {
			Railroad
		} else if starts_ignoring_case("railroad-ebnf-beta") {
			RailroadEbnf
		} else if starts_ignoring_case("railroad-abnf-beta") {
			RailroadAbnf
		} else if starts_ignoring_case("railroad-peg-beta") {
			RailroadPeg
		} else if starts("venn-beta") {
			Venn
		} else if starts_ignoring_case("wardley-beta") {
			Wardley
		} else if starts("cynefin-beta")
			&& text["cynefin-beta".len()..]
				.chars()
				.next()
				.is_none_or(|next| next.is_whitespace() || next == ':')
		{
			Cynefin
		} else if starts("usecase-beta")
			&& text["usecase-beta".len()..].chars().next().is_none_or(char::is_whitespace)
		{
			Usecase
		} else {
			return None;
		};
		Some(kind)
	}

	/// The section of the configuration with this type's settings (`getDiagramConfigKey`).
	pub fn config_section(self) -> &'static str {
		match self {
			DiagramKind::Flowchart | DiagramKind::FlowchartElk => "flowchart",
			DiagramKind::Class => "class",
			DiagramKind::State => "state",
			DiagramKind::XyChart => "xyChart",
			DiagramKind::RailroadAbnf | DiagramKind::RailroadEbnf | DiagramKind::RailroadPeg => {
				"railroad"
			}
			other => other.config_key(),
		}
	}

	/// Mermaid 12.2 draws these types with the `redux-color` theme and the `neo` look unless the
	/// document chooses.
	pub fn uses_redux_defaults(self) -> bool {
		use DiagramKind::*;
		matches!(
			self,
			Flowchart
				| FlowchartElk
				| Swimlanes | Class
				| EntityRelationship
				| Requirement
				| Sequence | State
				| Usecase | Venn
				| Agentflow
		)
	}

	/// The configuration key of a directive's `config`, Mermaid's detector id.
	pub fn config_key(self) -> &'static str {
		use DiagramKind::*;
		match self {
			Error => "error",
			FrontMatterOnly => "---",
			FlowchartElk => "flowchart-elk",
			Mindmap => "mindmap",
			Architecture => "architecture",
			Agentflow => "agentflow",
			C4 => "c4",
			Kanban => "kanban",
			Class => "classDiagram",
			EntityRelationship => "er",
			Gantt => "gantt",
			Info => "info",
			Pie => "pie",
			Requirement => "requirement",
			Sequence => "sequence",
			Swimlanes => "swimlane",
			Flowchart => "flowchart",
			Timeline => "timeline",
			GitGraph => "gitGraph",
			State => "stateDiagram",
			Journey => "journey",
			QuadrantChart => "quadrantChart",
			Sankey => "sankey",
			Packet => "packet",
			XyChart => "xychart",
			Block => "block",
			EventModeling => "eventmodeling",
			TreeView => "treeView",
			Radar => "radar",
			Ishikawa => "ishikawa",
			Treemap => "treemap",
			Railroad => "railroad",
			RailroadEbnf => "railroadEbnf",
			RailroadAbnf => "railroadAbnf",
			RailroadPeg => "railroadPeg",
			Venn => "venn",
			Wardley => "wardley",
			Cynefin => "cynefin",
			Usecase => "usecase",
		}
	}
}

/// Mermaid's `look`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Look {
	#[default]
	Classic,
	Neo,
	HandDrawn,
}

/// What a diagram renderer gets: the configuration, theme and text measurement.
pub struct Context<'a> {
	pub config: Value,
	pub theme: ThemeVariables,
	pub theme_name: String,
	/// Front matter `title`.
	pub title: Option<String>,
	/// Prefix of all ids in the SVG.
	pub id: String,
	/// Added to parser line numbers for the line in the code block.
	pub line_offset: usize,
	pub look: Look,
	/// The configured `layout` of graph diagrams, if any.
	pub layout: Option<String>,
	pub limits: Limits,
	measurer: &'a mut dyn TextMeasure,
	pub clock: &'a dyn Clock,
	pub images: &'a dyn ImageDecoder,
	/// `screen.availWidth`, the width limit of C4 diagrams.
	pub screen_width: f64,
	/// Whether the style sheet of the diagram has the rules of icons in labels (`getIconStyles`).
	pub has_icon_styles: bool,
	/// An error found where results have no way to report it, which fails the diagram at its end.
	failure: Option<Error>,
	/// The `MATH` table of the math font, read once.
	math_table: Option<Option<Rc<math::MathTable>>>,
}

impl<'a> Context<'a> {
	/// Fails the diagram when it is finished; the first error wins.
	pub fn fail(&mut self, message: impl Into<String>) {
		self.failure.get_or_insert_with(|| Error::new(message));
	}

	#[cfg(test)]
	pub fn failure(&self) -> Option<&Error> {
		self.failure.as_ref()
	}

	/// The `MATH` table of `font`, which is the math font.
	fn math_table(&mut self, font: &Font) -> Option<Rc<math::MathTable>> {
		if self.math_table.is_none() {
			let table = self.measurer.font_table(font, *b"MATH");
			self.math_table = Some(table.map(|data| Rc::new(math::MathTable::new(data))));
		}
		self.math_table.clone().flatten()
	}

	pub fn design_unit(&mut self, font: &Font) -> f64 {
		self.measurer.design_unit(font)
	}

	pub fn ink_bounds(&mut self, text: &str, font: &Font) -> Option<[f64; 4]> {
		self.measurer.ink_bounds(text, font)
	}

	pub fn glyph_index(&mut self, font: &Font, character: char) -> u16 {
		self.measurer.glyph_index(font, character)
	}

	pub fn glyph_advance(&mut self, font: &Font, glyph: u16) -> f64 {
		self.measurer.glyph_advance(font, glyph)
	}

	pub fn glyph_bounds(&mut self, font: &Font, glyph: u16) -> Option<[f64; 4]> {
		self.measurer.glyph_bounds(font, glyph)
	}

	pub fn glyph_outline(&mut self, font: &Font, glyph: u16) -> Option<String> {
		self.measurer.glyph_outline(font, glyph)
	}

	/// Width and height of a line of text, as [`svg::Svg::text`] shows it.
	pub fn measure(&mut self, text: &str, font: &Font) -> (f64, f64) {
		self.measurer.measure(&text::collapse_whitespace(text), font)
	}

	/// Size of the bounding box of `text` as an SVG `<text>` shows it.
	pub fn bounding_box(&mut self, text: &str, font: &Font) -> (f64, f64) {
		self.measurer.bounding_box(&text::collapse_whitespace(text), font)
	}

	/// [`Context::bounding_box`] of a `<text>` written by [`svg::Svg::text_with_spaces`].
	pub fn bounding_box_with_spaces(&mut self, text: &str, font: &Font) -> (f64, f64) {
		match text.is_empty() {
			true => (0.0, 0.0),
			false => self.measurer.bounding_box(text, font),
		}
	}

	/// The left edge of [`Context::bounding_box`] relative to the start of the text.
	pub fn bounding_box_left(&mut self, text: &str, font: &Font) -> f64 {
		self.measurer.bounding_box_left(&text::collapse_whitespace(text), font)
	}

	/// The top edge of [`Context::bounding_box`] relative to the baseline, negative above it.
	pub fn bounding_box_top(&mut self, text: &str, font: &Font) -> f64 {
		self.measurer.bounding_box_top(&text::collapse_whitespace(text), font)
	}

	/// Width of `text` as is, spaces included.
	pub fn measure_exact(&mut self, text: &str, font: &Font) -> f64 {
		self.measurer.measure(text, font).0
	}

	pub fn vertical_metrics(&mut self, font: &Font) -> (f64, f64) {
		self.measurer.vertical_metrics(font)
	}

	pub fn underline(&mut self, font: &Font) -> (f64, f64) {
		self.measurer.underline(font)
	}

	pub fn x_height(&mut self, font: &Font) -> f64 {
		self.measurer.x_height(font)
	}

	/// CSS `line-height: normal` like Blink: the rounded ascent, descent and line gap.
	pub fn normal_line_height(&mut self, font: &Font) -> f64 {
		let (ascent, descent) = self.measurer.vertical_metrics(font);
		ascent.round() + descent.round() + self.measurer.line_gap(font).round()
	}

	/// `calculateTextDimensions`: the widest line of `text` and the sum of the line heights,
	/// measured in `sans-serif` and in `font`; `font` wins unless `sans-serif` is larger in
	/// everything.
	pub fn text_dimensions(&mut self, text: &str, font: &Font) -> (f64, f64) {
		if text.is_empty() {
			return (0.0, 0.0);
		}
		let lines = common::split_breaks(text);
		let mut dimensions = [(0.0, 0.0, 0.0); 2];
		let sans_serif = Font { family: "sans-serif".to_owned(), ..font.clone() };
		for (slot, font) in dimensions.iter_mut().zip([&sans_serif, font]) {
			for line in &lines {
				let (width, line_height) = self.bounding_box(line, font);
				let line_height = color::js_round(line_height);
				slot.0 = color::js_round(f64::max(slot.0, width));
				slot.1 += line_height;
				slot.2 = f64::max(slot.2, line_height);
			}
		}
		let [first, second] = dimensions;
		let index = usize::from(!(first.1 > second.1 && first.0 > second.0 && first.2 > second.2));
		let (width, height, _) = dimensions[index];
		(width, height)
	}

	/// The font `calculateTextWidth` measures in with the top-level configuration. Mermaid's
	/// default family ends with `;`, which makes it invalid: the page's default font remains.
	pub fn configured_text_font(&self) -> Font {
		let family = self.config.get("fontFamily").and_then(Value::to_text);
		let size = self
			.config
			.get("fontSize")
			.and_then(Value::to_text)
			.and_then(|size| text::parse_font_size(&size, 16.0))
			.filter(|size| *size > 0.0)
			.unwrap_or(16.0);
		let family = family.filter(|family| !family.is_empty());
		Font::new(family.unwrap_or_else(|| "\"Times New Roman\"".to_owned()), size)
	}

	/// The theme's font size in pixels.
	pub fn font_size(&self) -> f64 {
		self.theme
			.text("fontSize")
			.and_then(|size| text::parse_font_size(&size, 16.0))
			.filter(|size| *size > 0.0)
			.unwrap_or(16.0)
	}

	#[cfg(test)]
	pub fn for_tests(measurer: &'a mut dyn TextMeasure) -> Self {
		Context {
			config: Value::Object(Vec::new()),
			theme: ThemeVariables::new("default", None),
			theme_name: "default".to_owned(),
			title: None,
			id: "test".to_owned(),
			line_offset: 0,
			look: Look::Classic,
			layout: None,
			limits: Limits::default(),
			measurer,
			clock: &TEST_CLOCK,
			images: &NoImageDecoder,
			screen_width: f64::from(DEFAULT_SCREEN_WIDTH),
			has_icon_styles: true,
			failure: None,
			math_table: None,
		}
	}

	/// The theme's font family.
	pub fn font_family(&self) -> String {
		self.theme
			.text("fontFamily")
			.unwrap_or_else(|| "\"trebuchet ms\", verdana, arial, sans-serif".to_owned())
	}

	/// `config[section][key]`, or the top-level `config[key]` Mermaid also reads for some keys.
	pub fn config_value(&self, section: &str, key: &str) -> Option<&Value> {
		self.config.path(&[section, key])
	}

	pub fn config_number(&self, section: &str, key: &str, default: f64) -> f64 {
		self.config_value(section, key)
			.and_then(Value::as_number)
			.filter(|number| number.is_finite())
			.unwrap_or(default)
	}

	pub fn config_bool(&self, section: &str, key: &str, default: bool) -> bool {
		self.config_value(section, key).and_then(Value::as_bool).unwrap_or(default)
	}

	pub fn config_text(&self, section: &str, key: &str) -> Option<String> {
		self.config_value(section, key).and_then(Value::to_text)
	}

	/// An error at `line` of the diagram code, as line of the code block.
	pub fn error_at(&self, line: usize, message: impl Into<String>) -> Error {
		Error::at(line + self.line_offset, message)
	}
}

/// The clock of [`render`]: the Unix epoch in UTC.
#[cfg(test)]
const TEST_CLOCK: FixedClock = FixedClock { now: 0.0, utc_offset: 0.0 };

/// The system clock and local time zone, for gantt charts: Mermaid computes their dates as
/// JavaScript `Date`s in the local time zone.
pub trait Clock {
	/// Milliseconds since the Unix epoch.
	fn now(&self) -> f64;
	/// Minutes the local time is ahead of UTC at `time`, in milliseconds since the Unix epoch.
	fn utc_offset(&self, time: f64) -> f64;
}

/// A fixed time in a time zone without daylight saving time.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub struct FixedClock {
	pub now: f64,
	pub utc_offset: f64,
}

#[cfg(test)]
impl Clock for FixedClock {
	fn now(&self) -> f64 {
		self.now
	}

	fn utc_offset(&self, _time: f64) -> f64 {
		self.utc_offset
	}
}

/// [`render_with`] at the Unix epoch in UTC.
#[cfg(test)]
pub fn render(source: &str, options: &Options, measurer: &mut dyn TextMeasure) -> Result<Diagram> {
	render_with(source, options, measurer, &TEST_CLOCK, &NoImageDecoder)
}

/// Renders the Mermaid `source` of a code block, with `clock` for the dates of gantt charts and
/// `images` for the natural size of images in node shapes.
pub fn render_with(
	source: &str,
	options: &Options,
	measurer: &mut dyn TextMeasure,
	clock: &dyn Clock,
	images: &dyn ImageDecoder,
) -> Result<Diagram> {
	if source.len() > options.limits.max_text_size {
		return Err(Error::new(format!(
			"Maximum text size in diagram exceeded ({} > {} bytes)",
			source.len(),
			options.limits.max_text_size
		)));
	}
	let preprocessed = preprocess::preprocess(source);
	let kind = DiagramKind::detect(&preprocessed.code)
		.ok_or_else(|| Error::new("No diagram type detected matching given configuration"))?;
	let mut config = preprocessed.config;
	if let Some(font_family) = config.get("fontFamily").cloned()
		&& config.path(&["themeVariables", "fontFamily"]).is_none()
	{
		config.merge(&Value::Object(vec![(
			"themeVariables".to_owned(),
			Value::Object(vec![("fontFamily".to_owned(), font_family)]),
		)]));
	}
	// Mermaid's `resolveAppearance`: the document's setting for the diagram type, its general
	// setting, then the diagram type's default; unknown values are passed over.
	let section = kind.config_section();
	let appearance = |key: &str, is_valid: &dyn Fn(&str) -> bool| {
		[config.path(&[section, key]), config.get(key)]
			.into_iter()
			.flatten()
			.filter_map(Value::as_str)
			.find(|value| is_valid(value))
			.map(str::to_owned)
	};
	let default_theme = match (kind.uses_redux_defaults(), options.is_dark) {
		(true, false) => "redux-color",
		(true, true) => "redux-dark-color",
		(false, false) => "default",
		(false, true) => "dark",
	};
	let theme_name =
		appearance("theme", &ThemeVariables::exists).unwrap_or_else(|| default_theme.to_owned());
	let look = match appearance("look", &|look| matches!(look, "classic" | "neo" | "handDrawn")) {
		Some(look) => match look.as_str() {
			"neo" => Look::Neo,
			"handDrawn" => Look::HandDrawn,
			_ => Look::Classic,
		},
		None if kind.uses_redux_defaults() => Look::Neo,
		None => Look::Classic,
	};
	// `flowchart-elk` only sets the layout; any other name counts, unknown ones lay out with dagre.
	// Mermaid's default is `elk`, for swimlane diagrams `swimlane`.
	let layout = match kind {
		DiagramKind::FlowchartElk => "elk".to_owned(),
		_ => appearance("layout", &|layout| !layout.is_empty()).unwrap_or_else(|| {
			let default = if kind == DiagramKind::Swimlanes { "swimlane" } else { "elk" };
			default.to_owned()
		}),
	};
	let theme = ThemeVariables::new(&theme_name, config.get("themeVariables"));
	let id = format!("m{:016x}", fnv1a(&[source.as_bytes(), &[u8::from(options.is_dark)]]));
	let mut context = Context {
		config,
		theme,
		theme_name,
		title: preprocessed.title,
		id,
		line_offset: preprocessed.line_offset,
		look,
		layout: Some(layout),
		limits: options.limits,
		measurer,
		clock,
		images,
		screen_width: f64::from(options.screen_width),
		has_icon_styles: kind.has_icon_styles(),
		failure: None,
		math_table: None,
	};
	let code = preprocessed.code;
	let result = match kind {
		DiagramKind::Info => info::render(&code, &mut context),
		DiagramKind::Pie => pie::render(&code, &mut context),
		DiagramKind::Flowchart | DiagramKind::FlowchartElk => {
			flowchart::render(&code, &mut context)
		}
		DiagramKind::Swimlanes => flowchart::render_swimlanes(&code, &mut context),
		DiagramKind::Sequence => sequence::render(&code, &mut context),
		DiagramKind::State => state::render(&code, &mut context),
		DiagramKind::EntityRelationship => er::render(&code, &mut context),
		DiagramKind::Requirement => requirement::render(&code, &mut context),
		DiagramKind::Class => class_diagram::render(&code, &mut context),
		DiagramKind::Packet => packet::render(&code, &mut context),
		DiagramKind::Radar => radar::render(&code, &mut context),
		DiagramKind::Kanban => kanban::render(&code, &mut context),
		DiagramKind::Journey => journey::render(&code, &mut context),
		DiagramKind::Timeline => timeline::render(&code, &mut context),
		DiagramKind::Sankey => sankey::render(&code, &mut context),
		DiagramKind::QuadrantChart => quadrant_chart::render(&code, &mut context),
		DiagramKind::XyChart => xychart::render(&code, &mut context),
		DiagramKind::Treemap => treemap::render(&code, &mut context),
		DiagramKind::Gantt => gantt::render(&code, &mut context),
		DiagramKind::GitGraph => git_graph::render(&code, &mut context),
		DiagramKind::Block => block::render(&code, &mut context),
		DiagramKind::C4 => c4::render(&code, &mut context),
		DiagramKind::Mindmap => mindmap::render(&code, &mut context),
		DiagramKind::Architecture => architecture::render(&code, &mut context),
		DiagramKind::TreeView => tree_view::render(&code, &mut context),
		DiagramKind::Cynefin => cynefin::render(&code, &mut context),
		DiagramKind::Ishikawa => ishikawa::render(&code, &mut context),
		DiagramKind::EventModeling => eventmodeling::render(&code, &mut context),
		DiagramKind::Wardley => wardley::render(&code, &mut context),
		DiagramKind::Venn => venn::render(&code, &mut context),
		DiagramKind::Usecase => usecase::render(&code, &mut context),
		DiagramKind::Agentflow => agentflow::render(&code, &mut context),
		kind @ (DiagramKind::Railroad
		| DiagramKind::RailroadEbnf
		| DiagramKind::RailroadAbnf
		| DiagramKind::RailroadPeg) => railroad::render(&code, kind, &mut context),
		DiagramKind::Error => Err(Error::new("Syntax error in text")),
		DiagramKind::FrontMatterOnly => Err(Error::new(
			"Diagrams beginning with --- are not valid. If you were trying to use a YAML front-matter, please ensure that you've correctly opened and closed the YAML front-matter with un-indented `---` blocks",
		)),
	};
	match context.failure.take() {
		Some(failure) if result.is_ok() => Err(failure),
		_ => result,
	}
}

/// What the Markdown writers need of a diagram.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Output {
	/// Inline SVG for browsers.
	Svg,
	/// A picture at `scale` times the natural size, for RichEdit and Office.
	Png { scale: f32 },
}

#[derive(Clone, Debug)]
pub enum Image {
	Svg(Rc<Diagram>),
	Png(Rc<PngImage>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct PngImage {
	pub bytes: Vec<u8>,
	/// Natural size in CSS pixels.
	pub width: u32,
	pub height: u32,
	/// Size of the picture.
	pub pixel_width: u32,
	pub pixel_height: u32,
}

/// Renders the code of a Mermaid block for the Markdown writers.
pub type RenderDiagram<'a> = dyn FnMut(&str, Output) -> Result<Image> + 'a;

/// Measures text for the layouts and outlines it for PNG pictures.
pub trait TextSystem: TextMeasure + TextOutline {}

impl<T: TextMeasure + TextOutline> TextSystem for T {}

struct CachedDiagram {
	key: u64,
	options: Options,
	result: Result<Rc<Diagram>>,
	/// The last picture and its scale.
	png: Option<(f32, Rc<PngImage>)>,
}

/// Renders diagrams and keeps the recent ones, so that a document is not laid out again for
/// every change of other blocks.
pub struct DiagramRenderer {
	text: Box<dyn TextSystem>,
	clock: Box<dyn Clock>,
	images: &'static dyn ImageDecoder,
	pub limits: Limits,
	pub screen_width: u32,
	/// Most recently used last.
	cache: Vec<CachedDiagram>,
}

/// Recent diagrams kept by [`DiagramRenderer`].
const CACHED_DIAGRAMS: usize = 32;

impl DiagramRenderer {
	pub fn new(
		text: Box<dyn TextSystem>,
		clock: Box<dyn Clock>,
		images: &'static dyn ImageDecoder,
		limits: Limits,
	) -> Self {
		Self { text, clock, images, limits, screen_width: DEFAULT_SCREEN_WIDTH, cache: Vec::new() }
	}

	pub fn render(&mut self, source: &str, is_dark: bool, output: Output) -> Result<Image> {
		let options = Options { is_dark, limits: self.limits, screen_width: self.screen_width };
		let key = fnv1a(&[source.as_bytes()]);
		let index =
			self.cache.iter().position(|cached| cached.key == key && cached.options == options);
		let mut entry = match index {
			Some(index) => self.cache.remove(index),
			None => {
				if self.cache.len() == CACHED_DIAGRAMS {
					let _ = self.cache.remove(0);
				}
				let result =
					render_with(source, &options, &mut *self.text, &*self.clock, self.images)
						.map(Rc::new);
				CachedDiagram { key, options, result, png: None }
			}
		};
		let image = self.image(&mut entry, output);
		self.cache.push(entry);
		image
	}

	fn image(&mut self, entry: &mut CachedDiagram, output: Output) -> Result<Image> {
		let diagram = entry.result.clone()?;
		let Output::Png { scale } = output else { return Ok(Image::Svg(diagram)) };
		if let Some((cached_scale, png)) = &entry.png
			&& *cached_scale == scale
		{
			return Ok(Image::Png(Rc::clone(png)));
		}
		let svg = svg::outline_text(&diagram, &mut *self.text);
		let (bytes, pixel_width, pixel_height) = npp::svg::render_png_scaled(&svg, scale)
			.ok_or_else(|| Error::new("The diagram could not be drawn"))?;
		let png = Rc::new(PngImage {
			bytes,
			width: diagram.width.round() as u32,
			height: diagram.height.round() as u32,
			pixel_width,
			pixel_height,
		});
		entry.png = Some((scale, Rc::clone(&png)));
		Ok(Image::Png(png))
	}
}

/// FNV-1a over `parts`, for stable ids.
fn fnv1a(parts: &[&[u8]]) -> u64 {
	let mut hash = 0xcbf2_9ce4_8422_2325u64;
	for part in parts {
		for &byte in *part {
			hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
		}
		hash = (hash ^ 0xff).wrapping_mul(0x0100_0000_01b3);
	}
	hash
}

#[cfg(test)]
mod tests;
