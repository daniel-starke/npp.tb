//! Mermaid's `rendering-util`: laid out graphs of nodes, clusters and edges as SVG, shared by the
//! flowchart and the other graph diagrams. The data model follows `LayoutData` of `types.ts`.
//!
//! Mermaid styles the SVG with a style sheet; here the rules of the diagram's styles become
//! presentation attributes, and user styles (`style`, `classDef`) a `style` attribute, which
//! outranks them like Mermaid's `!important` styles.

mod clusters;
mod curves;
mod dagre_wrapper;
pub mod edges;
mod elk_wrapper;
pub mod intersect;
mod line_jumps;
pub mod markers;
pub mod shapes;
pub mod sketch;
pub mod swimlane;

use super::label::{self, LabelBox};
use super::svg::{Svg, number};
use super::text::{Font, parse_font_size};
use super::{Context, Look, Result};

pub use super::layout::dagre::Point;
pub use shapes::is_shape_name;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Thickness {
	#[default]
	Normal,
	Thick,
	Invisible,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pattern {
	#[default]
	Solid,
	Dotted,
	Dashed,
}

/// The diagram whose style sheet paints the shapes, clusters and edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiagramStyle {
	#[default]
	Flowchart,
	State,
	Er,
	Requirement,
	Class,
	Usecase,
	Agentflow,
}

/// A node or a cluster (`isGroup`).
#[derive(Clone, Debug, Default)]
pub struct Node {
	pub id: String,
	/// The element id, without the diagram id prefix.
	pub dom_id: String,
	pub label: String,
	pub is_markdown: bool,
	pub parent_id: Option<String>,
	pub padding: f64,
	/// `style` statements of the node.
	pub css_styles: Vec<String>,
	/// The styles of its classes.
	pub css_compiled_styles: Vec<String>,
	pub css_classes: String,
	pub dir: Option<String>,
	pub look: Look,
	pub link: Option<String>,
	pub link_target: Option<String>,
	pub tooltip: Option<String>,
	pub is_group: bool,
	/// A shape name of the registry, `rect` for clusters.
	pub shape: String,
	pub min_width: Option<f64>,
	/// The color slot of clusters in the `redux-color` themes.
	pub color_index: Option<usize>,
	/// The picture of `imageSquare` nodes.
	pub image: Option<NodeImage>,
	/// The icon of the `icon` shapes.
	pub icon: Option<NodeIcon>,
	/// The rows below the title of `rectWithTitle`.
	pub description: Vec<String>,
	/// Where the label wraps, `flowchart.wrappingWidth` without one.
	pub wrapping_width: Option<f64>,
	/// The corner radius of rounded rectangles where a style sheet rule sets it.
	pub corner_radius: Option<f64>,
	/// The attribute rows of `erBox` entities: type, name, keys and comment.
	pub attributes: Vec<[String; 4]>,
	/// The body of `requirementBox` nodes.
	pub requirement: Option<RequirementBody>,
	/// The annotation and members of `classBox` nodes.
	pub class_body: Option<ClassBody>,
	/// The size a shape grows to, set by block diagrams for their second drawing.
	pub width: Option<f64>,
	pub height: Option<f64>,
	/// Block diagrams: the node is drawn at its laid out size.
	pub is_positioned: bool,
	/// Actors, use cases, notes, JSON tables and boundaries of use case diagrams.
	pub usecase: Option<Box<shapes::usecase::UsecaseNode>>,
	/// The directions of block arrows.
	pub directions: Vec<String>,
	/// The columns a block spans, at least 1.
	pub width_in_columns: f64,
	/// The color slot of the node's kind in the `redux-color` themes (agentflow).
	pub kind_slot: Option<usize>,
}

/// What a class box shows besides its name.
#[derive(Clone, Debug, Default)]
pub struct ClassBody {
	/// The first annotation, without the guillemets.
	pub annotation: Option<String>,
	pub members: Vec<ClassMember>,
	pub methods: Vec<ClassMember>,
}

/// A member row of a class box: its text and the style of its classifier.
#[derive(Clone, Debug, Default)]
pub struct ClassMember {
	pub text: String,
	pub is_italic: bool,
	pub is_underlined: bool,
}

/// What a requirement box shows below its name.
#[derive(Clone, Debug, Default)]
pub struct RequirementBody {
	/// The requirement type, `None` for elements.
	pub kind: Option<&'static str>,
	/// Requirements: id, text, risk and verification method; elements: type and document
	/// reference.
	pub fields: [String; 4],
}

/// The icon of a node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeIcon {
	/// `None` draws the unknown icon.
	pub glyph: Option<super::icons::IconGlyph>,
	/// The largest of its width and height, by which `getIconSVG` scales it.
	pub size: f64,
	/// `pos: t`: the label above the icon instead of below.
	pub is_label_on_top: bool,
}

/// An image of a node in its drawn size.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeImage {
	pub href: String,
	pub width: f64,
	pub height: f64,
	/// `pos: t`: the label above the image instead of below.
	pub is_label_on_top: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Edge {
	pub id: String,
	pub start: String,
	pub end: String,
	pub label: String,
	pub is_markdown: bool,
	pub thickness: Thickness,
	pub pattern: Pattern,
	/// `arrow_point`, `arrow_circle`, `arrow_cross` or `none`.
	pub arrow_type_start: String,
	pub arrow_type_end: String,
	/// `linkStyle` and default styles.
	pub style: Vec<String>,
	pub css_compiled_styles: Vec<String>,
	pub classes: String,
	pub minlen: usize,
	/// dagre's `labelpos`: `l`, `c` or `r`.
	pub labelpos: &'static str,
	pub curve: Option<String>,
	pub look: Look,
	pub animate: bool,
	pub animation: Option<String>,
	/// The labels at the ends (`startLabelRight`, `endLabelLeft`): cardinalities of class
	/// relations.
	pub start_label_right: String,
	pub end_label_left: String,
	/// The stroke and its width from the rules of a diagram that paints each edge itself, like
	/// the sections of mindmaps.
	pub paint: Option<(String, String)>,
	/// The `stroke-dasharray` of a diagram that paints each edge itself.
	pub dasharray: Option<&'static str>,
	/// The element id without the diagram id prefix, the edge id without one.
	pub dom_id: Option<String>,
	/// Further attributes of the path: data, role and accessible name.
	pub attributes: Vec<(&'static str, String)>,
}

/// What a graph diagram hands to the layout.
pub struct LayoutData {
	pub nodes: Vec<Node>,
	pub edges: Vec<Edge>,
	pub direction: String,
	pub node_spacing: f64,
	pub rank_spacing: f64,
	/// The diagram type of marker ids, e.g. `flowchart-v2`.
	pub diagram_type: &'static str,
	/// The configuration section with `curve`, `wrappingWidth`, …
	pub config_section: &'static str,
	pub style: DiagramStyle,
	/// The marker definitions to write (`markers.js`).
	pub markers: &'static [&'static str],
}

/// The area painted so far, like `getBBox` of the elements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
	pub min: Point,
	pub max: Point,
}

impl Default for BBox {
	fn default() -> Self {
		Self {
			min: Point { x: f64::INFINITY, y: f64::INFINITY },
			max: Point { x: f64::NEG_INFINITY, y: f64::NEG_INFINITY },
		}
	}
}

impl BBox {
	pub fn is_empty(&self) -> bool {
		!(self.min.x <= self.max.x && self.min.y <= self.max.y)
	}

	pub fn include_point(&mut self, point: Point) {
		if point.x.is_finite() && point.y.is_finite() {
			self.min = Point { x: self.min.x.min(point.x), y: self.min.y.min(point.y) };
			self.max = Point { x: self.max.x.max(point.x), y: self.max.y.max(point.y) };
		}
	}

	pub fn include_rect(&mut self, x: f64, y: f64, width: f64, height: f64) {
		self.include_point(Point { x, y });
		self.include_point(Point { x: x + width, y: y + height });
	}

	/// Includes `other` moved by `dx`, `dy`.
	pub fn include(&mut self, other: &BBox, dx: f64, dy: f64) {
		if !other.is_empty() {
			self.include_point(Point { x: other.min.x + dx, y: other.min.y + dy });
			self.include_point(Point { x: other.max.x + dx, y: other.max.y + dy });
		}
	}

	/// Includes `getBBox` of a `<text>` centered on `baseline`: the advances from their start,
	/// widened by glyphs beyond them, and the rounded ascent and descent.
	pub fn include_centered_text(
		&mut self,
		context: &mut Context,
		text: &str,
		font: &Font,
		baseline: Point,
	) {
		let advance = context.measure(text, font).0;
		let (width, height) = context.bounding_box(text, font);
		let ascent = context.vertical_metrics(font).0.round();
		self.include_rect(baseline.x - advance / 2.0, baseline.y - ascent, width, height);
	}

	pub fn width(&self) -> f64 {
		if self.is_empty() { 0.0 } else { self.max.x - self.min.x }
	}

	pub fn height(&self) -> f64 {
		if self.is_empty() { 0.0 } else { self.max.y - self.min.y }
	}
}

/// `isLabelStyle`: the style properties that go to the label instead of the shape.
pub fn is_label_style(key: &str) -> bool {
	matches!(
		key,
		"color"
			| "font-size"
			| "font-family"
			| "font-weight"
			| "font-style"
			| "text-decoration"
			| "text-align"
			| "text-transform"
			| "line-height"
			| "letter-spacing"
			| "word-spacing"
			| "text-shadow"
			| "text-overflow"
			| "white-space"
			| "word-wrap"
			| "word-break"
			| "overflow-wrap"
			| "hyphens"
	)
}

/// `compileStyles` and `styles2String`: the class styles, then the node's own; a later value of
/// a property replaces an earlier one in its place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styles {
	pub entries: Vec<(String, String)>,
}

impl Styles {
	pub fn compile(lists: &[&[String]]) -> Styles {
		let mut entries: Vec<(String, String)> = Vec::new();
		for style in lists.iter().flat_map(|list| list.iter()) {
			let mut parts = style.split(':');
			let key = parts.next().unwrap_or("").trim().to_owned();
			let value = parts.next().unwrap_or("").trim().to_owned();
			match entries.iter_mut().find(|(known, _)| *known == key) {
				Some(entry) => entry.1 = value,
				None => entries.push((key, value)),
			}
		}
		Styles { entries }
	}

	pub fn get(&self, key: &str) -> Option<&str> {
		self.entries.iter().find(|(known, _)| known == key).map(|(_, value)| value.as_str())
	}

	/// `styles2String`'s `nodeStyles`: the shape styles, each `!important`.
	pub fn node_css_important(&self) -> String {
		self.entries
			.iter()
			.filter(|(key, _)| !key.is_empty() && !is_label_style(key))
			.map(|(key, value)| format!("{key}:{value} !important"))
			.collect::<Vec<_>>()
			.join(";")
	}

	/// The `style` attribute of the shape: the styles that are no label styles.
	pub fn node_css(&self) -> String {
		self.entries
			.iter()
			.filter(|(key, _)| !key.is_empty() && !is_label_style(key))
			.map(|(key, value)| format!("{key}:{value}"))
			.collect::<Vec<_>>()
			.join(";")
	}
}

/// The paint of the diagram's style sheet rules.
#[derive(Clone, Debug, Default)]
pub struct Theme {
	pub main_bkg: String,
	pub node_border: String,
	pub node_stroke_width: String,
	pub text_color: String,
	pub line_color: String,
	pub edge_stroke_width: String,
	pub edge_label_background: String,
	pub cluster_bkg: String,
	pub cluster_border: String,
	pub title_color: String,
	/// `filter` of neo shapes, empty without a drop shadow.
	pub neo_filter: String,
	/// `stroke` of neo shapes.
	pub neo_stroke: String,
	/// The color slots of clusters: border and background.
	pub border_colors: Vec<String>,
	pub background_colors: Vec<String>,
	/// The stroke of edges (`.flowchart-link`, `.transition`) and their label color.
	pub edge_stroke: String,
	pub edge_label_color: String,
	/// `mainBkg` and `nodeBorder` themselves, which shapes take as attributes.
	pub plain_main_bkg: String,
	pub plain_node_border: String,
	/// `strokeWidth` without a unit, `1` without one.
	pub stroke_width: String,
	/// `stroke-dasharray` of dashed edges.
	pub dashed_edge: &'static str,
	/// The font size of edge labels where a rule sets it.
	pub edge_label_font_size: Option<f64>,
	pub state: StateTheme,
	pub er: ErTheme,
	pub usecase: UsecaseTheme,
	/// The horizontal padding of edge label backgrounds (`.labelBkg`).
	pub edge_label_padding: f64,
	/// The stroke of agentflow containers: `flowContainerStroke || secondaryBorderColor`.
	pub flow_container_stroke: String,
}

/// The role colors of use case diagrams (`roleColors` of `usecase/styles.ts`).
#[derive(Clone, Debug, Default)]
pub struct UsecaseTheme {
	pub actor_bkg: String,
	pub actor_border: String,
	pub bkg: String,
	pub border: String,
	pub boundary_bkg: String,
	pub boundary_border: String,
	pub title_color: String,
	pub json_bkg: String,
	pub json_border: String,
	/// `colorScheme: rotate`: actors and use cases take color slots too.
	pub rotate: bool,
}

/// The variables of the ER diagram's style sheet and its entity boxes, which requirement boxes
/// share.
#[derive(Clone, Debug, Default)]
pub struct ErTheme {
	/// The fill of the attribute rows, none where the theme has no `rowOdd`/`rowEven`.
	pub row_odd: Option<String>,
	pub row_even: Option<String>,
	/// Every path of a box gets the user styles: entities in the `redux` themes, requirement
	/// boxes where the theme has `requirementEdgeLabelBackground` or `borderColorArray`.
	pub styles_all_paths: bool,
}

/// The variables of the state diagram's style sheet.
#[derive(Clone, Debug, Default)]
pub struct StateTheme {
	pub special_state_color: String,
	/// The inner circle of end states: `stateBorder ?? nodeBorder`.
	pub end_inner: String,
	pub composite_background: String,
	pub composite_title_background: String,
	pub alt_background: String,
	pub note_bkg: String,
	pub note_border: String,
	pub note_text: String,
	/// `nodeShadow`: start and end states get the small drop shadow.
	pub has_node_shadow: bool,
	/// The small drop shadow filter.
	pub small_shadow: String,
}

impl Theme {
	/// The color slot of a cluster or collapsed subgraph `node` in the `redux-color` themes.
	pub fn color_slot(&self, node: &Node) -> Option<usize> {
		node.color_index
			.filter(|_| !self.border_colors.is_empty())
			.map(|index| index % self.border_colors.len())
	}

	pub fn new(context: &Context, style: DiagramStyle) -> Theme {
		let theme = &context.theme;
		let stroke_width = theme.text("strokeWidth");
		let array = |key: &str| -> Vec<String> {
			match theme.get(key) {
				Some(super::value::Value::Array(items)) => {
					items.iter().filter_map(super::value::Value::to_text).collect()
				}
				_ => Vec::new(),
			}
		};
		let is_color_theme =
			matches!(context.theme_name.as_str(), "redux-color" | "redux-dark-color");
		let border_colors = if is_color_theme { array("borderColorArray") } else { Vec::new() };
		let background_colors =
			if border_colors.is_empty() { Vec::new() } else { array("bkgColorArray") };
		let node_text = theme.text("nodeTextColor").filter(|color| !color.is_empty());
		let uses_gradient =
			matches!(theme.get("useGradient"), Some(super::value::Value::Bool(true)));
		let neo_stroke = match uses_gradient {
			true => format!("url(#{}-gradient)", context.id),
			false => theme.css("nodeBorder"),
		};
		let neo_filter = theme
			.text("dropShadow")
			// A value may end with the `;` of its CSS declaration.
			.map(|shadow| shadow.trim().trim_end_matches(';').trim_end().to_owned())
			.filter(|shadow| !shadow.is_empty() && shadow != "none")
			.map(|shadow| {
				shadow.replace("url(#drop-shadow)", &format!("url(#{}-drop-shadow)", context.id))
			})
			.unwrap_or_default();
		// JavaScript's `a || b` of theme variables.
		let either = |first: &str, second: &str| {
			theme.text(first).filter(|value| !value.is_empty()).unwrap_or_else(|| theme.css(second))
		};
		let is_state = style == DiagramStyle::State;
		let is_er = style == DiagramStyle::Er;
		let is_requirement = style == DiagramStyle::Requirement;
		let is_class = style == DiagramStyle::Class;
		// `.node rect` and `.relationshipLine` of ER diagrams: 1px unless the look is neo.
		let er_stroke_width = match is_er && context.look != Look::Neo {
			true => Some("1px".to_owned()),
			false => None,
		};
		let requirement_label_background =
			theme.text("requirementEdgeLabelBackground").filter(|_| is_requirement);
		let er = match (is_er, is_requirement) {
			(true, _) => ErTheme {
				row_odd: theme.text("rowOdd").filter(|color| !color.is_empty()),
				row_even: theme.text("rowEven").filter(|color| !color.is_empty()),
				styles_all_paths: context.theme_name.starts_with("redux"),
			},
			// `requirementBox` styles its paths where the theme has either variable.
			(_, true) => ErTheme {
				styles_all_paths: requirement_label_background
					.as_ref()
					.is_some_and(|c| !c.is_empty())
					|| !array("borderColorArray").is_empty(),
				..ErTheme::default()
			},
			_ => ErTheme::default(),
		};
		let er_label_background = theme
			.text("erEdgeLabelBackground")
			.filter(|color| is_er && is_color_theme && !color.is_empty());
		let is_usecase = style == DiagramStyle::Usecase;
		// Class and use case diagrams share the note colors.
		// Flowcharts draw end states too (`fr-circ`).
		let end_inner = theme.text("stateBorder").unwrap_or_else(|| theme.css("nodeBorder"));
		let state = match is_state || is_class || is_usecase {
			true => StateTheme {
				special_state_color: theme.css("specialStateColor"),
				end_inner,
				composite_background: either("compositeBackground", "background"),
				composite_title_background: theme.css("compositeTitleBackground"),
				alt_background: theme
					.text("altBackground")
					.filter(|value| !value.is_empty())
					.unwrap_or_else(|| "#efefef".to_owned()),
				note_bkg: theme.css("noteBkgColor"),
				note_border: theme.css("noteBorderColor"),
				note_text: theme.css("noteTextColor"),
				has_node_shadow: matches!(
					theme.get("nodeShadow"),
					Some(super::value::Value::Bool(true))
				),
				small_shadow: format!("url(#{}-drop-shadow-small)", context.id),
			},
			false => StateTheme { end_inner, ..StateTheme::default() },
		};
		// `??` of theme variables.
		let first = |keys: &[&str]| -> String {
			keys.iter().find_map(|key| theme.text(key)).unwrap_or_default()
		};
		let usecase = match is_usecase {
			true => UsecaseTheme {
				actor_bkg: first(&["usecaseActorBkg", "actorBkg", "mainBkg"]),
				actor_border: first(&["usecaseActorBorder", "actorBorder", "primaryColor"]),
				bkg: first(&["usecaseBkg", "mainBkg"]),
				border: first(&["usecaseBorder", "nodeBorder", "primaryColor"]),
				boundary_bkg: first(&["usecaseBoundaryBkg", "clusterBkg"]),
				boundary_border: first(&["usecaseBoundaryBorder", "clusterBorder"]),
				title_color: first(&["titleColor", "primaryTextColor"]),
				json_bkg: first(&["mainBkg"]),
				json_border: first(&["nodeBorder", "primaryColor"]),
				rotate: context
					.config_value("usecase", "colorScheme")
					.and_then(super::value::Value::as_str)
					== Some("rotate"),
			},
			false => UsecaseTheme::default(),
		};
		Theme {
			usecase,
			flow_container_stroke: either("flowContainerStroke", "secondaryBorderColor"),
			edge_label_padding: if is_usecase { 2.0 } else { 0.0 },
			main_bkg: if is_state { either("stateBkg", "mainBkg") } else { theme.css("mainBkg") },
			node_border: match is_state {
				true => either("stateBorder", "nodeBorder"),
				false => theme.css("nodeBorder"),
			},
			plain_main_bkg: theme.css("mainBkg"),
			plain_node_border: theme.css("nodeBorder"),
			stroke_width: stroke_width.clone().unwrap_or_else(|| "1".to_owned()),
			edge_stroke: match (is_state, is_requirement) {
				(true, _) => theme.css("transitionColor"),
				(_, true) => theme.css("relationColor"),
				_ => theme.css("lineColor"),
			},
			edge_label_color: match (is_state, is_class) {
				_ if is_usecase => first(&["primaryTextColor"]),
				(true, _) => either("transitionLabelColor", "tertiaryTextColor"),
				// `.nodeLabel, .edgeLabel`.
				(_, true) => theme.css("classText"),
				_ => node_text.clone().unwrap_or_else(|| theme.css("textColor")),
			},
			state,
			er,
			dashed_edge: if is_er { "8,8" } else { "3" },
			edge_label_font_size: match (is_er, is_usecase) {
				(true, _) => Some(14.0),
				(_, true) => Some(10.0),
				_ => None,
			},
			node_stroke_width: er_stroke_width.clone().unwrap_or_else(|| {
				format!("{}px", stroke_width.clone().unwrap_or_else(|| "1".to_owned()))
			}),
			text_color: match (is_state, is_class) {
				(true, _) => theme.css("stateLabelColor"),
				(_, true) => theme.css("classText"),
				_ => node_text.unwrap_or_else(|| theme.css("textColor")),
			},
			line_color: theme.css("lineColor"),
			edge_stroke_width: match is_requirement && context.look != Look::Neo {
				// `.relationshipLine`.
				true => "1px".to_owned(),
				false => er_stroke_width.unwrap_or_else(|| {
					format!("{}px", stroke_width.unwrap_or_else(|| "1".to_owned()))
				}),
			},
			edge_label_background: match is_class {
				// `.edgeLabel .label rect` and `.labelBkg`.
				true => theme.css("mainBkg"),
				false => er_label_background
					.or(requirement_label_background)
					.unwrap_or_else(|| theme.css("edgeLabelBackground")),
			},
			cluster_bkg: theme.css("clusterBkg"),
			cluster_border: theme.css("clusterBorder"),
			title_color: if is_state {
				theme.css("stateLabelColor")
			} else {
				theme.css("titleColor")
			},
			neo_filter,
			neo_stroke,
			border_colors,
			background_colors,
		}
	}
}

/// The font and color of a label with the label styles of its element.
pub fn label_font(context: &Context, styles: &Styles) -> (Font, Option<String>) {
	let mut font = Font::new(context.font_family(), context.font_size());
	if let Some(size) = styles.get("font-size").and_then(|size| parse_font_size(size, font.size)) {
		font.size = size;
	}
	if let Some(family) = styles.get("font-family").filter(|family| !family.is_empty()) {
		font.family = family.to_owned();
	}
	match styles.get("font-weight") {
		Some("bold" | "bolder") => font.weight = 700,
		Some(weight) => {
			if let Ok(weight) = weight.parse::<u16>() {
				font.weight = weight;
			}
		}
		None => {}
	}
	if matches!(styles.get("font-style"), Some("italic" | "oblique")) {
		font.is_italic = true;
	}
	let color = styles.get("color").filter(|color| !color.is_empty()).map(str::to_owned);
	(font, color)
}

/// The font and color of `node`'s label: its own base font, else the diagram's, with the label
/// styles of its element.
pub fn node_label_font(context: &Context, node: &Node, styles: &Styles) -> (Font, Option<String>) {
	let (font, color) = label_font(context, styles);
	let Some(base) = node.usecase.as_ref().and_then(|usecase| usecase.font.clone()) else {
		return (font, color);
	};
	let mut font = Font { size: base.size, family: base.family, weight: base.weight, ..font };
	if let Some(size) = styles.get("font-size").and_then(|size| parse_font_size(size, base.size)) {
		font.size = size;
	}
	if let Some(family) = styles.get("font-family").filter(|family| !family.is_empty()) {
		font.family = family.to_owned();
	}
	match styles.get("font-weight") {
		Some("bold" | "bolder") => font.weight = 700,
		Some(weight) => {
			if let Ok(weight) = weight.parse::<u16>() {
				font.weight = weight;
			}
		}
		None => {}
	}
	(font, color)
}

/// Lays out a label like `createText`.
pub fn layout_label(
	context: &mut Context,
	text: &str,
	is_markdown: bool,
	font: &Font,
	wrap_width: Option<f64>,
	min_width: f64,
) -> LabelBox {
	if super::math::has_math(text) && !has_svg_labels(context) {
		return label::layout_with_math(context, text, font, min_width);
	}
	let lines = label::label_lines_with_icons(text, is_markdown);
	let mut label = label::layout(context, &lines, font, wrap_width, min_width);
	// A label of only empty elements, such as those of unknown icons, has no lines, but its size.
	if label.lines.is_empty() && !text.is_empty() {
		label.width = min_width;
	}
	label
}

/// Whether the document turns HTML labels off (`getEffectiveHtmlLabels`): Mermaid's SVG labels
/// show `$$…$$` as text.
fn has_svg_labels(context: &Context) -> bool {
	use super::value::Value;
	let setting = context.config.get("htmlLabels").or_else(|| {
		context.config.get("flowchart").and_then(|flowchart| flowchart.get("htmlLabels"))
	});
	match setting {
		Some(Value::Bool(value)) => !value,
		Some(Value::String(value)) => value == "false" || value == "0",
		Some(Value::Number(value)) => *value == 0.0,
		_ => false,
	}
}

/// Lays out the label of `node` and the shape around it (`labelHelper` and the shape
/// handler): `text` in `font`, styled by `styles`, wrapped at the width of `wrap` unless the
/// styles keep it on one line, and at least as wide as the minimum width of `wrap`.
pub fn layout_shape(
	context: &mut Context,
	node: &Node,
	(text, font, styles): (&str, &Font, &Styles),
	(wrap_width, min_width): (Option<f64>, f64),
	is_horizontal: bool,
	style: DiagramStyle,
) -> (shapes::Shape, LabelBox) {
	// The label styles of the span: `nowrap` keeps the lines of a note together, and
	// `text-align: left` reaches its paragraph.
	let wraps = styles.get("white-space") != Some("nowrap");
	let mut label = layout_label(
		context,
		text,
		node.is_markdown,
		font,
		wrap_width.filter(|_| wraps),
		min_width,
	);
	label.is_left_aligned = styles.get("text-align") == Some("left");
	let radius = node.corner_radius.unwrap_or_else(|| {
		context.theme.text("radius").and_then(|radius| radius.parse().ok()).unwrap_or(5.0)
	});
	let shape = shapes::shape(node, &label, radius, is_horizontal, style);
	(shape, label)
}

/// `getBBox` of a node: its shape and its label. A sketched shape's box is Blink's, which the
/// union with the label rounds again.
pub fn node_box(shape: &shapes::Shape, label: &LabelBox) -> BBox {
	fn has_sketch(parts: &[shapes::Part]) -> bool {
		parts.iter().any(|part| match part {
			shapes::Part::Sketch(_) => true,
			shapes::Part::Group { parts, .. } => has_sketch(parts),
			_ => false,
		})
	}
	if has_sketch(&shape.parts) {
		use shapes::blink_path::{RectF, union_all};
		let drawn = shape.bbox;
		let shape_rect = RectF {
			x: drawn.min.x as f32,
			y: drawn.min.y as f32,
			width: drawn.width() as f32,
			height: drawn.height() as f32,
		};
		// The label's group is moved by half its size.
		let label_rect = (!label.is_empty()).then(|| {
			let center = shape.label_center;
			let size =
				RectF { x: 0.0, y: 0.0, width: label.width as f32, height: label.height as f32 };
			size.translated(
				(center.x - label.width / 2.0) as f32,
				(center.y - label.height / 2.0) as f32,
			)
		});
		if let Some(rect) = union_all(std::iter::once(shape_rect).chain(label_rect)) {
			return sketch::bounds_box(rect);
		}
	}
	let mut bbox = shape.bbox;
	if !label.is_empty() {
		let center = shape.label_center;
		bbox.include_rect(
			center.x - label.width / 2.0,
			center.y - label.height / 2.0,
			label.width,
			label.height,
		);
	}
	bbox
}

/// A node's laid out shape and labels: the main label and others with their centers.
pub struct NodeDrawing<'a> {
	pub shape: &'a shapes::Shape,
	pub label: &'a LabelBox,
	pub extra_labels: &'a [(LabelBox, Point)],
}

/// `insertNode` and `positionNode`: writes `node` centered on `center` and adds its area to
/// `bbox`.
#[allow(clippy::too_many_arguments)]
pub fn write_node(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	theme: &Theme,
	style: DiagramStyle,
	node: &Node,
	drawing: &NodeDrawing,
	center: Point,
) {
	let (shape, label) = (drawing.shape, drawing.label);
	let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
	let look = node.look.data_name();
	let transform = translate(center.x, center.y);
	// `insertNode` moves `data-look` and the position to the link; state diagrams wrap the
	// drawn node in it afterwards, so their node keeps both.
	let link_takes_look = node.link.is_some() && style != DiagramStyle::State;
	if let Some(link) = &node.link {
		svg.open("a").attr("xlink:href", link);
		if let Some(target) = &node.link_target {
			svg.attr("target", target);
		}
		if link_takes_look {
			svg.attr("data-look", look).attr("transform", &transform);
		}
		svg.end();
	}
	let is_special_state =
		matches!(node.shape.as_str(), "stateStart" | "stateEnd") && style == DiagramStyle::State;
	let class = match node.image.is_some() {
		true => "image-shape default".to_owned(),
		false if node.icon.is_some() => "icon-shape default".to_owned(),
		false if is_special_state => "node default".to_owned(),
		// `composite` joins `cssClasses` and `class`, which block diagrams both set, and draws
		// a plain rectangle in every look.
		false if node.shape == "composite" => {
			format!("node {0} {0}", node.css_classes).trim_end().to_owned()
		}
		// `getNodeClasses`. `stateStart` and `stateEnd` do not use it.
		false
			if node.look == Look::HandDrawn
				&& !matches!(
					node.shape.as_str(),
					"sm-circ"
						| "start" | "small-circle"
						| "stateStart" | "fr-circ"
						| "stop" | "framed-circle"
						| "stateEnd"
				) =>
		{
			format!("rough-node {}", node.css_classes).trim_end().to_owned()
		}
		false => format!("node {}", node.css_classes).trim_end().to_owned(),
	};
	svg.open("g").attr("class", &class).attr("id", &format!("{}-{}", context.id, node.dom_id));
	let has_color_slot = matches!(
		node.shape.as_str(),
		"collapsedGroup" | "erBox" | "requirementBox" | "classBox" | "composite"
	);
	if let Some(slot) = theme.color_slot(node).filter(|_| has_color_slot) {
		svg.attr("data-color-id", &format!("color-{slot}"));
	}
	if let Some(usecase) = &node.usecase {
		if let Some(slot) = usecase.color_slot.filter(|_| !theme.border_colors.is_empty()) {
			svg.attr("data-color-id", &format!("color-{}", slot % theme.border_colors.len()));
		}
		for (name, value) in &usecase.attributes {
			svg.attr(name, value);
		}
	}
	if !link_takes_look {
		svg.attr("data-look", look).attr("transform", &transform);
	}
	svg.end();
	if let Some(tooltip) = &node.tooltip {
		svg.open("title").text_element("title", tooltip);
	}
	// Without `data-look` on the node, the rules keyed on it (the neo look, the agentflow kind
	// palette) miss its shapes.
	let unlooked;
	let loses_look = node.look != Look::Classic || node.kind_slot.is_some();
	let painted = match link_takes_look && loses_look {
		true => {
			unlooked = Node { look: Look::Classic, kind_slot: None, ..node.clone() };
			&unlooked
		}
		false => node,
	};
	shapes::write(svg, shape, painted, &styles, theme);
	svg.open("g").attr("class", "label").end();
	let (_, color) = label_font(context, &styles);
	let has_note_text = matches!(style, DiagramStyle::State | DiagramStyle::Class);
	let default_fill = match node.shape == "note" && has_note_text {
		true => &theme.state.note_text,
		false => &theme.text_color,
	};
	let usecase_fill = node.usecase.as_ref().map(|usecase| &usecase.label_color);
	let fill = color.unwrap_or_else(|| usecase_fill.unwrap_or(default_fill).clone());
	write_label(svg, context, label, shape.label_center, &fill);
	for (extra, center) in drawing.extra_labels {
		write_label(svg, context, extra, *center, &fill);
	}
	svg.close_tag("g").close_tag("g");
	if node.link.is_some() {
		svg.close_tag("a");
	}
	bbox.include(&node_box(shape, label), center.x, center.y);
	sketch::include_ink(shape, bbox, center);
}

/// The background node of a hand-drawn entity (`erBox`) centered on `center`, if it has one:
/// the box without roughness, filled with the background color and without strokes. Mermaid
/// draws its labels too, under the same labels of the entity; they are left out.
pub fn write_er_background(
	svg: &mut Svg,
	context: &Context,
	theme: &Theme,
	node: &Node,
	shape: &shapes::Shape,
	center: Point,
) {
	let Some(shapes::Part::ErBackground(parts)) = shape.parts.first() else { return };
	let transform = translate(center.x, center.y);
	let background = context.theme.text("background").unwrap_or_default();
	let painted = Node {
		look: Look::Classic,
		css_styles: vec!["stroke: none".to_owned(), format!("fill: {background}")],
		css_compiled_styles: Vec::new(),
		..node.clone()
	};
	let styles = Styles::compile(&[&painted.css_styles]);
	svg.open("g")
		.attr("class", "node default")
		.attr("id", &format!("{}-{}-background", context.id, node.dom_id));
	if let Some(slot) = theme.color_slot(node) {
		svg.attr("data-color-id", &format!("color-{slot}"));
	}
	svg.attr("transform", &transform).end();
	let background_shape = shapes::Shape { parts: parts.to_vec(), ..shape.clone() };
	shapes::write(svg, &background_shape, &painted, &styles, theme);
	svg.close_tag("g");
}

/// Writes `label` centered on `center` with its fill.
pub fn write_label(
	svg: &mut Svg,
	context: &mut Context,
	label: &LabelBox,
	center: Point,
	fill: &str,
) {
	let top = center.y - label.height / 2.0;
	label::write(svg, context, label, center.x, top, &|svg: &mut Svg| {
		svg.attr_if("fill", fill);
	});
}

/// The spacing of the layout of renderers that read the state diagram's configuration: the
/// top-level setting, else the state diagram's, else the flowchart's.
pub fn state_spacing(context: &Context, key: &str) -> f64 {
	let positive = |value: Option<&super::value::Value>| {
		value.and_then(super::value::Value::as_number).filter(|n| *n != 0.0)
	};
	positive(context.config.get(key))
		.or_else(|| match context.config_value("state", key) {
			None | Some(super::value::Value::Null) => Some(50.0),
			value => positive(value),
		})
		.or_else(|| positive(context.config_value("flowchart", key)))
		.unwrap_or(50.0)
}

/// `getSubGraphTitleMargins`: the top and bottom margin of cluster titles.
pub fn title_margins(context: &Context) -> (f64, f64) {
	let margin = |side: &str| {
		context
			.config
			.path(&["flowchart", "subGraphTitleMargin", side])
			.and_then(super::value::Value::as_number)
			.filter(|margin| margin.is_finite())
			.unwrap_or(0.0)
	};
	(margin("top"), margin("bottom"))
}

/// `translate(x, y)`.
pub fn translate(x: f64, y: f64) -> String {
	format!("translate({}, {})", number(x), number(y))
}

/// A node measured for the layout: its shape, main label, other labels with their centers and
/// size.
pub struct MeasuredNode {
	pub shape: shapes::Shape,
	pub label: LabelBox,
	pub extra_labels: Vec<(LabelBox, Point)>,
	pub width: f64,
	pub height: f64,
}

impl MeasuredNode {
	fn new(
		shape: shapes::Shape,
		label: LabelBox,
		extra_labels: Vec<(LabelBox, Point)>,
		bbox: &BBox,
	) -> Self {
		Self { shape, label, extra_labels, width: bbox.width(), height: bbox.height() }
	}
}

/// The shape, label and size of the layout data node `index` (`insertNode` in
/// `createGraphWithElements`): the measured node of every layout.
pub fn measure_node(
	context: &mut Context,
	theme: &Theme,
	data: &LayoutData,
	index: usize,
	is_horizontal: bool,
) -> MeasuredNode {
	let node = &data.nodes[index];
	let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
	let (font, _) = node_label_font(context, node, &styles);
	let mut wrap_width = node
		.wrapping_width
		.unwrap_or_else(|| context.config_number("flowchart", "wrappingWidth", 120.0));
	let text = if shapes::has_no_label(&node.shape) { "" } else { node.label.as_str() };
	let mut min_width = match text.is_empty() || shapes::ignores_min_width(&node.shape) {
		true => 0.0,
		false => node.min_width.unwrap_or(0.0),
	};
	// Images and icons set the node width, which wraps the label instead of the minimum
	// width. Its paragraph has a padding inside that width.
	let asset_width = node.image.as_ref().map(|image| image.width);
	if let Some(asset_width) = asset_width.or(node.icon.as_ref().map(|icon| icon.size)) {
		wrap_width = wrap_width.max(asset_width) - shapes::ASSET_LABEL_PADDING * 2.0;
		min_width = 0.0;
	}
	if node.usecase.is_some()
		&& let Some(laid) =
			shapes::usecase::layout(context, node, &font, &styles, (wrap_width, min_width))
	{
		let mut laid = laid;
		laid.shape = sketch::apply(laid.shape, context, node, &styles, theme, data.style);
		let mut bbox = laid.shape.bbox;
		for (label, center) in std::iter::once((&laid.label, laid.shape.label_center))
			.chain(laid.labels.iter().map(|(label, center)| (label, *center)))
		{
			if !label.is_empty() {
				bbox.include_rect(
					center.x - label.width / 2.0,
					center.y - label.height / 2.0,
					label.width,
					label.height,
				);
			}
		}
		return MeasuredNode::new(laid.shape, laid.label, laid.labels, &bbox);
	}
	if node.shape == "rectWithTitle" {
		// `createLabel`: unwrapped HTML labels.
		let title = layout_label(context, text, false, &font, None, 0.0);
		let rows = node.description.join("<br/>");
		let description = layout_label(context, &rows, false, &font, None, 0.0);
		let (shape, center) = shapes::rect_with_title(&title, &description, node.padding);
		let mut bbox = shape.bbox;
		bbox.include_rect(
			center.x - description.width / 2.0,
			center.y - description.height / 2.0,
			description.width,
			description.height,
		);
		return MeasuredNode::new(shape, title, vec![(description, center)], &bbox);
	}
	if node.shape == "erBox" {
		let mut er_box = shapes::er_box(context, node, &font);
		er_box.shape = sketch::apply(er_box.shape, context, node, &styles, theme, data.style);
		let mut bbox = er_box.shape.bbox;
		let center = er_box.shape.label_center;
		let name = &er_box.name;
		bbox.include_rect(
			center.x - name.width / 2.0,
			center.y - name.height / 2.0,
			name.width,
			name.height,
		);
		return MeasuredNode::new(er_box.shape, er_box.name, er_box.cells, &bbox);
	}
	if node.shape == "classBox" {
		// The labels are inside the box.
		let mut class_box = shapes::class_box(context, node, &font, &styles);
		class_box.shape = sketch::apply(class_box.shape, context, node, &styles, theme, data.style);
		let bbox = class_box.shape.bbox;
		return MeasuredNode::new(class_box.shape, class_box.name, class_box.labels, &bbox);
	}
	if node.shape == "requirementBox" {
		// The labels are inside the box.
		let mut requirement_box = shapes::requirement_box(context, node, &font);
		requirement_box.shape =
			sketch::apply(requirement_box.shape, context, node, &styles, theme, data.style);
		let bbox = requirement_box.shape.bbox;
		return MeasuredNode::new(
			requirement_box.shape,
			requirement_box.type_label,
			requirement_box.labels,
			&bbox,
		);
	}
	let (shape, label) = layout_shape(
		context,
		node,
		(text, &font, &styles),
		(Some(wrap_width), min_width),
		is_horizontal,
		data.style,
	);
	let shape = sketch::apply(shape, context, node, &styles, theme, data.style);
	let bbox = node_box(&shape, &label);
	MeasuredNode::new(shape, label, Vec::new(), &bbox)
}

/// Lays out and draws `data` with the configured layout (dagre or elk); returns the body (marker
/// definitions and the root group) and its painted area.
pub fn render(context: &mut Context, data: &LayoutData) -> Result<(Svg, BBox)> {
	match uses_elk(context) {
		true => elk_wrapper::render(context, data),
		false => dagre_wrapper::render(context, data),
	}
}

/// Whether the configured layout is `elk`; the other ELK algorithms Mermaid registers
/// (`elk.stress`, `elk.mrtree`, …) are drawn like it.
pub fn uses_elk(context: &Context) -> bool {
	context.layout.as_deref().is_some_and(|layout| layout == "elk" || layout.starts_with("elk."))
}

/// The definitions of the neo look's drop shadows (`insertLookDefs`), after the root group.
pub fn write_look_defs(svg: &mut Svg, context: &Context) {
	let flood_color = if context.theme_name.contains("dark") { "#FFFFFF" } else { "#000000" };
	for (suffix, size, offset) in [("drop-shadow", "130%", "4"), ("drop-shadow-small", "150%", "2")]
	{
		svg.open("defs").end();
		svg.open("filter")
			.attr("id", &format!("{}-{suffix}", context.id))
			.attr("height", size)
			.attr("width", size)
			.end();
		// `feDropShadow` with `stdDeviation="0"`, written with the primitives it is defined as:
		// EdgeHTML does not know it and then does not draw the shapes at all.
		svg.open("feOffset")
			.attr("in", "SourceAlpha")
			.attr("dx", offset)
			.attr("dy", offset)
			.attr("result", "offset")
			.close();
		svg.open("feFlood").attr("flood-opacity", "0.06").attr("flood-color", flood_color).close();
		svg.open("feComposite").attr("in2", "offset").attr("operator", "in").close();
		svg.open("feMerge").end();
		svg.open("feMergeNode").close();
		svg.open("feMergeNode").attr("in", "SourceGraphic").close();
		svg.close_tag("feMerge").close_tag("filter").close_tag("defs");
	}
	let theme = &context.theme;
	if matches!(theme.get("useGradient"), Some(super::value::Value::Bool(true))) {
		svg.open("linearGradient")
			.attr("id", &format!("{}-gradient", context.id))
			.attr("gradientUnits", "objectBoundingBox")
			.attr("x1", "0%")
			.attr("y1", "0%")
			.attr("x2", "100%")
			.attr("y2", "0%")
			.end();
		for (offset, key) in [("0%", "gradientStart"), ("100%", "gradientStop")] {
			svg.open("stop")
				.attr("offset", offset)
				.attr_if("stop-color", &theme.css(key))
				.attr("stop-opacity", "1")
				.close();
		}
		svg.close_tag("linearGradient");
	}
}
