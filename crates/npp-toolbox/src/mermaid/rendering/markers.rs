//! The arrow heads of edges (`rendering-elements/markers.js`, `edgeMarker.ts`).

use super::super::svg::Svg;

/// A marker definition: the shape is in `body`, written with the paint attributes of its type.
struct MarkerDefinition {
	suffix: &'static str,
	is_cross: bool,
	view_box: &'static str,
	ref_x: &'static str,
	ref_y: &'static str,
	width: &'static str,
	height: &'static str,
	/// Element name and its geometry attributes.
	element: &'static str,
	geometry: &'static [(&'static str, &'static str)],
	style: &'static str,
}

const fn marker(
	suffix: &'static str,
	is_cross: bool,
	view_box: &'static str,
	(ref_x, ref_y): (&'static str, &'static str),
	(width, height): (&'static str, &'static str),
	(element, geometry): (&'static str, &'static [(&'static str, &'static str)]),
	style: &'static str,
) -> MarkerDefinition {
	MarkerDefinition {
		suffix,
		is_cross,
		view_box,
		ref_x,
		ref_y,
		width,
		height,
		element,
		geometry,
		style,
	}
}

const CIRCLE: &[(&str, &str)] = &[("cx", "5"), ("cy", "5"), ("r", "5")];

/// The markers of `point`, `circle` and `cross`, the ones flowcharts use.
const FLOWCHART_MARKERS: &[MarkerDefinition] = &[
	marker(
		"pointEnd",
		false,
		"0 0 10 10",
		("5", "5"),
		("8", "8"),
		("path", &[("d", "M 0 0 L 10 5 L 0 10 z")]),
		"stroke-width: 1; stroke-dasharray: 1, 0;",
	),
	marker(
		"pointStart",
		false,
		"0 0 10 10",
		("4.5", "5"),
		("8", "8"),
		("path", &[("d", "M 0 5 L 10 10 L 10 0 z")]),
		"stroke-width: 1; stroke-dasharray: 1, 0;",
	),
	marker(
		"pointEnd-margin",
		false,
		"0 0 11.5 14",
		("11.5", "7"),
		("10.5", "14"),
		("path", &[("d", "M 0 0 L 11.5 7 L 0 14 z")]),
		"stroke-width: 0; stroke-dasharray: 1, 0;",
	),
	marker(
		"pointStart-margin",
		false,
		"0 0 11.5 14",
		("1", "7"),
		("11.5", "14"),
		("polygon", &[("points", "0,7 11.5,14 11.5,0")]),
		"stroke-width: 0; stroke-dasharray: 1, 0;",
	),
	marker(
		"circleEnd",
		false,
		"0 0 10 10",
		("11", "5"),
		("11", "11"),
		("circle", CIRCLE),
		"stroke-width: 1; stroke-dasharray: 1, 0;",
	),
	marker(
		"circleStart",
		false,
		"0 0 10 10",
		("-1", "5"),
		("11", "11"),
		("circle", CIRCLE),
		"stroke-width: 1; stroke-dasharray: 1, 0;",
	),
	marker(
		"circleEnd-margin",
		false,
		"0 0 10 10",
		("12.25", "5"),
		("14", "14"),
		("circle", CIRCLE),
		"stroke-width: 0; stroke-dasharray: 1, 0;",
	),
	marker(
		"circleStart-margin",
		false,
		"0 0 10 10",
		("-2", "5"),
		("14", "14"),
		("circle", CIRCLE),
		"stroke-width: 0; stroke-dasharray: 1, 0;",
	),
	marker(
		"crossEnd",
		true,
		"0 0 11 11",
		("12", "5.2"),
		("11", "11"),
		("path", &[("d", "M 1,1 l 9,9 M 10,1 l -9,9")]),
		"stroke-width: 2; stroke-dasharray: 1, 0;",
	),
	marker(
		"crossStart",
		true,
		"0 0 11 11",
		("-1", "5.2"),
		("11", "11"),
		("path", &[("d", "M 1,1 l 9,9 M 10,1 l -9,9")]),
		"stroke-width: 2; stroke-dasharray: 1, 0;",
	),
	marker(
		"crossEnd-margin",
		true,
		"0 0 15 15",
		("17.7", "7.5"),
		("12", "12"),
		("path", &[("d", "M 1,1 L 14,14 M 1,14 L 14,1")]),
		"stroke-width: 2.5;",
	),
	marker(
		"crossStart-margin",
		true,
		"0 0 15 15",
		("-3.5", "7.5"),
		("12", "12"),
		("path", &[("d", "M 1,1 L 14,14 M 1,14 L 14,1")]),
		"stroke-width: 2.5; stroke-dasharray: 1, 0;",
	),
];

/// A marker of the other sets: its attributes in Mermaid's order and how its shape is painted.
struct PlainMarker {
	set: &'static str,
	suffix: &'static str,
	attributes: &'static [(&'static str, &'static str)],
	d: &'static str,
	paint: MarkerPaint,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerPaint {
	/// `defs [id$="-barbEnd"]`: filled and stroked in the transition color.
	Transition,
	/// The fill attribute of `barbEnd-margin`.
	TransitionFill,
}

const PLAIN_MARKERS: &[PlainMarker] = &[
	PlainMarker {
		set: "barb",
		suffix: "barbEnd",
		attributes: &[
			("refX", "19"),
			("refY", "7"),
			("markerWidth", "20"),
			("markerHeight", "14"),
			("markerUnits", "userSpaceOnUse"),
			("orient", "auto"),
		],
		d: "M 19,7 L9,13 L14,7 L9,1 Z",
		paint: MarkerPaint::Transition,
	},
	PlainMarker {
		set: "barbNeo",
		suffix: "barbEnd",
		attributes: &[
			("refX", "19"),
			("refY", "7"),
			("markerWidth", "20"),
			("markerHeight", "14"),
			("markerUnits", "strokeWidth"),
			("orient", "auto"),
		],
		d: "M 19,7 L11,14 L13,7 L11,0 Z",
		paint: MarkerPaint::Transition,
	},
	PlainMarker {
		set: "barbNeo",
		suffix: "barbEnd-margin",
		attributes: &[
			("refX", "17"),
			("refY", "7"),
			("markerWidth", "20"),
			("markerHeight", "14"),
			("markerUnits", "userSpaceOnUse"),
			("orient", "auto"),
		],
		d: "M 19,7 L11,14 L13,7 L11,0 Z",
		paint: MarkerPaint::TransitionFill,
	},
];

/// A cardinality marker of ER diagrams: an optional circle, then a path. The marker paints its
/// shapes unfilled in the line color (`.marker` of the ER styles), the circle covers the line.
struct ErMarker {
	set: &'static str,
	suffix: &'static str,
	kind: &'static str,
	ref_x: &'static str,
	ref_y: &'static str,
	width: &'static str,
	height: &'static str,
	circle: Option<(&'static str, &'static str)>,
	d: &'static str,
}

const fn er_marker(
	set: &'static str,
	suffix: &'static str,
	kind: &'static str,
	(ref_x, ref_y): (&'static str, &'static str),
	(width, height): (&'static str, &'static str),
	circle: Option<(&'static str, &'static str)>,
	d: &'static str,
) -> ErMarker {
	ErMarker { set, suffix, kind, ref_x, ref_y, width, height, circle, d }
}

const ONLY_ONE_START: &str = "M9,0 L9,18 M15,0 L15,18";
const ONLY_ONE_END: &str = "M3,0 L3,18 M9,0 L9,18";
const ONE_OR_MORE_START: &str = "M0,18 Q 18,0 36,18 Q 18,36 0,18 M42,9 L42,27";
const ONE_OR_MORE_END: &str = "M3,9 L3,27 M9,18 Q27,0 45,18 Q27,36 9,18";
const ZERO_OR_MORE_START: &str = "M0,18 Q18,0 36,18 Q18,36 0,18";
const ZERO_OR_MORE_END: &str = "M21,18 Q39,0 57,18 Q39,36 21,18";

/// The markers of `only_one`, `zero_or_one`, `one_or_more` and `zero_or_more` and their neo
/// variants, which draw in user space with the theme's stroke width.
const ER_MARKERS: &[ErMarker] = &[
	er_marker(
		"only_one",
		"onlyOneStart",
		"onlyOne",
		("0", "9"),
		("18", "18"),
		None,
		ONLY_ONE_START,
	),
	er_marker("only_one", "onlyOneEnd", "onlyOne", ("18", "9"), ("18", "18"), None, ONLY_ONE_END),
	er_marker(
		"zero_or_one",
		"zeroOrOneStart",
		"zeroOrOne",
		("0", "9"),
		("30", "18"),
		Some(("21", "9")),
		"M9,0 L9,18",
	),
	er_marker(
		"zero_or_one",
		"zeroOrOneEnd",
		"zeroOrOne",
		("30", "9"),
		("30", "18"),
		Some(("9", "9")),
		"M21,0 L21,18",
	),
	er_marker(
		"one_or_more",
		"oneOrMoreStart",
		"oneOrMore",
		("18", "18"),
		("45", "36"),
		None,
		ONE_OR_MORE_START,
	),
	er_marker(
		"one_or_more",
		"oneOrMoreEnd",
		"oneOrMore",
		("27", "18"),
		("45", "36"),
		None,
		ONE_OR_MORE_END,
	),
	er_marker(
		"zero_or_more",
		"zeroOrMoreStart",
		"zeroOrMore",
		("18", "18"),
		("57", "36"),
		Some(("48", "18")),
		ZERO_OR_MORE_START,
	),
	er_marker(
		"zero_or_more",
		"zeroOrMoreEnd",
		"zeroOrMore",
		("39", "18"),
		("57", "36"),
		Some(("9", "18")),
		ZERO_OR_MORE_END,
	),
	er_marker(
		"only_one_neo",
		"onlyOneStart",
		"onlyOne",
		("0", "9"),
		("18", "18"),
		None,
		ONLY_ONE_START,
	),
	er_marker(
		"only_one_neo",
		"onlyOneEnd",
		"onlyOne",
		("18", "9"),
		("18", "18"),
		None,
		ONLY_ONE_END,
	),
	er_marker(
		"zero_or_one_neo",
		"zeroOrOneStart",
		"zeroOrOne",
		("0", "9"),
		("30", "18"),
		Some(("21", "9")),
		"M9,0 L9,18",
	),
	er_marker(
		"zero_or_one_neo",
		"zeroOrOneEnd",
		"zeroOrOne",
		("30", "9"),
		("30", "18"),
		Some(("9", "9")),
		"M21,0 L21,18",
	),
	er_marker(
		"one_or_more_neo",
		"oneOrMoreStart",
		"oneOrMore",
		("18", "18"),
		("45", "36"),
		None,
		ONE_OR_MORE_START,
	),
	er_marker(
		"one_or_more_neo",
		"oneOrMoreEnd",
		"oneOrMore",
		("27", "18"),
		("45", "36"),
		None,
		ONE_OR_MORE_END,
	),
	er_marker(
		"zero_or_more_neo",
		"zeroOrMoreStart",
		"zeroOrMore",
		("18", "18"),
		("57", "36"),
		Some(("45.5", "18")),
		ZERO_OR_MORE_START,
	),
	er_marker(
		"zero_or_more_neo",
		"zeroOrMoreEnd",
		"zeroOrMore",
		("39", "18"),
		("57", "36"),
		Some(("11", "18")),
		ZERO_OR_MORE_END,
	),
];

/// A marker of class relations: its element and the attributes in Mermaid's order.
struct ClassMarker {
	kind: &'static str,
	suffix: &'static str,
	ref_x: &'static str,
	/// `markerWidth` and `markerHeight`.
	size: (&'static str, &'static str),
	view_box: &'static str,
	/// Element name and its attributes after its geometry.
	element: (&'static str, &'static [(&'static str, &'static str)]),
	/// Mermaid writes one margin marker outside of `<defs>`.
	is_in_defs: bool,
}

const DIAMOND: &[(&str, &str)] = &[("d", "M 18,7 L9,13 L1,7 L9,1 Z")];
const DIAMOND_WIDE: &[(&str, &str)] =
	&[("d", "M 18,7 L9,13 L1,7 L9,1 Z"), ("style", "stroke-width: 2;")];
const DIAMOND_FLAT: &[(&str, &str)] =
	&[("d", "M 18,7 L9,13 L1,7 L9,1 Z"), ("style", "stroke-width: 0;")];
const LOLLIPOP: &[(&str, &str)] = &[("fill", "transparent"), ("cx", "7"), ("cy", "7"), ("r", "6")];
const LOLLIPOP_WIDE: &[(&str, &str)] =
	&[("fill", "transparent"), ("cx", "7"), ("cy", "7"), ("r", "6"), ("stroke-width", "2")];
const LARGE: (&str, &str) = ("190", "240");
const SMALL: (&str, &str) = ("20", "28");

const fn class_marker(
	kind: &'static str,
	suffix: &'static str,
	ref_x: &'static str,
	size: (&'static str, &'static str),
	element: (&'static str, &'static [(&'static str, &'static str)]),
) -> ClassMarker {
	ClassMarker { kind, suffix, ref_x, size, view_box: "", element, is_in_defs: true }
}

const CLASS_MARKERS: &[ClassMarker] = &[
	class_marker("aggregation", "aggregationStart", "18", LARGE, ("path", DIAMOND)),
	class_marker("aggregation", "aggregationEnd", "1", SMALL, ("path", DIAMOND)),
	class_marker("aggregation", "aggregationStart-margin", "15", LARGE, ("path", DIAMOND_WIDE)),
	class_marker("aggregation", "aggregationEnd-margin", "1", SMALL, ("path", DIAMOND_WIDE)),
	class_marker(
		"extension",
		"extensionStart",
		"18",
		SMALL,
		("path", &[("d", "M 1,7 L18,13 V 1 Z")]),
	),
	class_marker("extension", "extensionEnd", "1", SMALL, ("path", &[("d", "M 1,1 V 13 L18,7 Z")])),
	ClassMarker {
		kind: "extension",
		suffix: "extensionStart-margin",
		ref_x: "18",
		size: SMALL,
		view_box: "0 0 20 14",
		element: (
			"polygon",
			&[("points", "10,7 18,13 18,1"), ("style", "stroke-width: 2; stroke-dasharray: 0;")],
		),
		is_in_defs: false,
	},
	ClassMarker {
		kind: "extension",
		suffix: "extensionEnd-margin",
		ref_x: "9",
		size: SMALL,
		view_box: "0 0 20 14",
		element: (
			"polygon",
			&[("points", "10,1 10,13 18,7"), ("style", "stroke-width: 2; stroke-dasharray: 0;")],
		),
		is_in_defs: true,
	},
	class_marker("composition", "compositionStart", "18", LARGE, ("path", DIAMOND)),
	class_marker("composition", "compositionEnd", "1", SMALL, ("path", DIAMOND)),
	class_marker("composition", "compositionStart-margin", "15", LARGE, ("path", DIAMOND_FLAT)),
	class_marker("composition", "compositionEnd-margin", "3.5", SMALL, ("path", DIAMOND_FLAT)),
	class_marker(
		"dependency",
		"dependencyStart",
		"6",
		LARGE,
		("path", &[("d", "M 5,7 L9,13 L1,7 L9,1 Z")]),
	),
	class_marker(
		"dependency",
		"dependencyEnd",
		"13",
		SMALL,
		("path", &[("d", "M 18,7 L9,13 L14,7 L9,1 Z")]),
	),
	class_marker(
		"dependency",
		"dependencyStart-margin",
		"4",
		LARGE,
		("path", &[("d", "M 5,7 L9,13 L1,7 L9,1 Z"), ("style", "stroke-width: 0;")]),
	),
	class_marker(
		"dependency",
		"dependencyEnd-margin",
		"16",
		SMALL,
		("path", &[("d", "M 18,7 L9,13 L14,7 L9,1 Z"), ("style", "stroke-width: 0;")]),
	),
	class_marker("lollipop", "lollipopStart", "13", LARGE, ("circle", LOLLIPOP)),
	class_marker("lollipop", "lollipopEnd", "1", LARGE, ("circle", LOLLIPOP)),
	class_marker("lollipop", "lollipopStart-margin", "13", LARGE, ("circle", LOLLIPOP_WIDE)),
	class_marker("lollipop", "lollipopEnd-margin", "1", LARGE, ("circle", LOLLIPOP_WIDE)),
];

const REQUIREMENT_MARKER_SETS: &[&str] = &[
	"requirement_contains",
	"requirement_arrow",
	"requirement_contains_neo",
	"requirement_arrow_neo",
];

/// The suffix of the marker of a requirement marker set: contains marks the start of an edge,
/// the arrow its end.
fn requirement_suffix(set: &str) -> &'static str {
	match set.starts_with("requirement_contains") {
		true => "requirement_containsStart",
		false => "requirement_arrowEnd",
	}
}

/// `arrowTypesMap`: the marker type and whether it is filled.
fn arrow_type(arrow: &str) -> Option<(&'static str, bool)> {
	match arrow {
		"arrow_cross" => Some(("cross", false)),
		"arrow_point" => Some(("point", true)),
		"arrow_barb" | "arrow_barb_neo" => Some(("barb", true)),
		"arrow_circle" => Some(("circle", false)),
		"only_one" => Some(("onlyOne", false)),
		"zero_or_one" => Some(("zeroOrOne", false)),
		"one_or_more" => Some(("oneOrMore", false)),
		"zero_or_more" => Some(("zeroOrMore", false)),
		"requirement_arrow" => Some(("requirement_arrow", false)),
		"requirement_contains" => Some(("requirement_contains", false)),
		"aggregation" => Some(("aggregation", false)),
		"extension" => Some(("extension", false)),
		"composition" => Some(("composition", true)),
		"dependency" => Some(("dependency", true)),
		"lollipop" => Some(("lollipop", false)),
		_ => None,
	}
}

/// `arrowTypesWithMarginSupport`.
fn has_margin_variant(kind: &str) -> bool {
	matches!(
		kind,
		"cross"
			| "point" | "circle"
			| "barb" | "lollipop"
			| "aggregation"
			| "extension"
			| "composition"
			| "dependency"
	)
}

/// A written marker definition.
#[derive(Clone, Copy)]
enum Definition {
	Flowchart(&'static MarkerDefinition),
	Plain(&'static PlainMarker),
	Er(&'static ErMarker),
	Class(&'static ClassMarker),
	/// A requirement marker of a set: `requirement_contains` or `requirement_arrow` and their
	/// neo variants.
	Requirement(&'static str),
}

/// A copy of a marker in the stroke color of an edge.
struct ColoredMarker {
	id: String,
	definition: Definition,
	color: String,
	is_filled: bool,
}

/// The colors of the marker rules.
pub struct MarkerColors {
	pub line: String,
	/// The stroke of the diagram's edges: transitions of state diagrams, relationships of
	/// requirement diagrams.
	pub edge: String,
	/// The circles of the neo ER markers and the width of their lines.
	pub main_bkg: String,
	pub stroke_width: String,
	/// `.marker.extension` of use case diagrams fills the triangle with `mainBkg`.
	pub is_extension_filled: bool,
}

/// The marker ids of a diagram and the colored copies its edges need.
pub struct Markers {
	prefix: String,
	colors: MarkerColors,
	/// The written definitions by suffix.
	written: Vec<(&'static str, Definition)>,
	colored: Vec<ColoredMarker>,
}

impl Markers {
	/// `insertMarkers`: the definitions of the marker `sets` for `diagram_type`.
	pub fn write(
		svg: &mut Svg,
		diagram_id: &str,
		diagram_type: &str,
		sets: &[&str],
		colors: MarkerColors,
	) -> Markers {
		let mut markers = Markers {
			prefix: format!("{diagram_id}_{diagram_type}-"),
			colors,
			written: Vec::new(),
			colored: Vec::new(),
		};
		for set in sets {
			let flowchart =
				FLOWCHART_MARKERS.iter().filter(|marker| marker.suffix.starts_with(set));
			for definition in flowchart {
				let id = format!("{}{}", markers.prefix, definition.suffix);
				markers.write_definition(svg, &id, definition, None, diagram_type);
				markers.written.push((definition.suffix, Definition::Flowchart(definition)));
			}
			for definition in PLAIN_MARKERS.iter().filter(|marker| marker.set == *set) {
				let id = format!("{}{}", markers.prefix, definition.suffix);
				markers.write_plain(svg, &id, definition, None);
				markers.written.push((definition.suffix, Definition::Plain(definition)));
			}
			for definition in ER_MARKERS.iter().filter(|marker| marker.set == *set) {
				let id = format!("{}{}", markers.prefix, definition.suffix);
				markers.write_er(svg, &id, definition, None, diagram_type);
				markers.written.push((definition.suffix, Definition::Er(definition)));
			}
			for definition in CLASS_MARKERS.iter().filter(|marker| marker.kind == *set) {
				let id = format!("{}{}", markers.prefix, definition.suffix);
				markers.write_class(svg, &id, definition, None, diagram_type);
				markers.written.push((definition.suffix, Definition::Class(definition)));
			}
			if let Some(set) = REQUIREMENT_MARKER_SETS.iter().find(|known| *known == set) {
				let suffix = requirement_suffix(set);
				let id = format!("{}{suffix}", markers.prefix);
				markers.write_requirement(svg, &id, set, None);
				markers.written.push((suffix, Definition::Requirement(set)));
			}
		}
		markers
	}

	fn write_definition(
		&self,
		svg: &mut Svg,
		id: &str,
		definition: &MarkerDefinition,
		color: Option<(&str, bool)>,
		diagram_type: &str,
	) {
		let class = match definition.is_cross {
			true => format!("marker cross {diagram_type}"),
			false => format!("marker {diagram_type}"),
		};
		svg.open("marker")
			.attr("id", id)
			.attr("class", &class)
			.attr("viewBox", definition.view_box)
			.attr("refX", definition.ref_x)
			.attr("refY", definition.ref_y)
			.attr("markerUnits", "userSpaceOnUse")
			.attr("markerWidth", definition.width)
			.attr("markerHeight", definition.height)
			.attr("orient", "auto")
			.end();
		svg.open(definition.element);
		for (name, value) in definition.geometry {
			svg.attr(name, value);
		}
		// `.marker` paints with the line color; a colored copy sets its own attributes, except on
		// polygons: Mermaid colors the paths, circles and lines of the copy.
		let line = self.colors.line.as_str();
		let color = color.filter(|_| definition.element != "polygon");
		let (stroke, fill) = match color {
			Some((color, is_filled)) => (color, if is_filled { color } else { line }),
			None => (line, line),
		};
		svg.attr("class", "arrowMarkerPath")
			.attr_if("fill", fill)
			.attr_if("stroke", stroke)
			.attr("style", definition.style)
			.close();
		svg.close_tag("marker");
	}

	fn write_plain(
		&self,
		svg: &mut Svg,
		id: &str,
		definition: &PlainMarker,
		color: Option<(&str, bool)>,
	) {
		svg.open("defs").end();
		svg.open("marker").attr("id", id);
		for (name, value) in definition.attributes {
			svg.attr(name, value);
		}
		svg.end();
		svg.open("path").attr("d", definition.d);
		let transition = self.colors.edge.as_str();
		match (color, definition.paint) {
			(Some((color, is_filled)), _) => {
				svg.attr_if("fill", if is_filled { color } else { transition })
					.attr_if("stroke", color);
			}
			(None, MarkerPaint::Transition) => {
				svg.attr_if("fill", transition).attr_if("stroke", transition);
			}
			(None, MarkerPaint::TransitionFill) => {
				svg.attr_if("fill", transition);
			}
		}
		svg.close();
		svg.close_tag("marker").close_tag("defs");
	}

	fn write_er(
		&self,
		svg: &mut Svg,
		id: &str,
		definition: &ErMarker,
		color: Option<&str>,
		diagram_type: &str,
	) {
		let is_neo = definition.set.ends_with("_neo");
		svg.open("defs").end();
		svg.open("marker")
			.attr("id", id)
			.attr("class", &format!("marker {} {diagram_type}", definition.kind))
			.attr("refX", definition.ref_x)
			.attr("refY", definition.ref_y)
			.attr("markerWidth", definition.width)
			.attr("markerHeight", definition.height)
			.attr("orient", "auto");
		if is_neo {
			svg.attr("markerUnits", "userSpaceOnUse");
		}
		svg.attr("fill", "none")
			.attr_if("stroke", &self.colors.line)
			.attr("stroke-width", "1")
			.end();
		let stroke_width = if is_neo { self.colors.stroke_width.as_str() } else { "" };
		if let Some((cx, cy)) = definition.circle {
			let fill = match is_neo && !self.colors.main_bkg.is_empty() {
				true => self.colors.main_bkg.as_str(),
				false => "white",
			};
			svg.open("circle")
				.attr("fill", fill)
				.attr("cx", cx)
				.attr("cy", cy)
				.attr("r", "6")
				.attr_if("stroke-width", stroke_width);
			if let Some(color) = color {
				svg.attr("stroke", color);
			}
			svg.close();
		}
		svg.open("path").attr("d", definition.d).attr_if("stroke-width", stroke_width);
		if let Some(color) = color {
			svg.attr("stroke", color);
		}
		svg.close();
		svg.close_tag("marker").close_tag("defs");
	}

	/// A class marker. The rules of the style sheet paint the marker, which its shape inherits:
	/// `.marker` and the `!important` rule of its kind.
	fn write_class(
		&self,
		svg: &mut Svg,
		id: &str,
		definition: &ClassMarker,
		color: Option<(&str, bool)>,
		diagram_type: &str,
	) {
		if definition.is_in_defs {
			svg.open("defs").end();
		}
		let line = self.colors.line.as_str();
		let fill = match definition.kind {
			"extension" if self.colors.is_extension_filled => self.colors.main_bkg.as_str(),
			"aggregation" | "extension" => "transparent",
			"lollipop" => self.colors.main_bkg.as_str(),
			_ => line,
		};
		svg.open("marker")
			.attr("id", id)
			.attr("class", &format!("marker {} {diagram_type}", definition.kind))
			.attr("refX", definition.ref_x)
			.attr("refY", "7")
			.attr("markerWidth", definition.size.0)
			.attr("markerHeight", definition.size.1)
			.attr("orient", "auto")
			.attr("markerUnits", "userSpaceOnUse")
			.attr_if("viewBox", definition.view_box)
			.attr_if("fill", fill)
			.attr_if("stroke", line)
			.attr("stroke-width", "1")
			.end();
		let (element, attributes) = definition.element;
		svg.open(element);
		for (name, value) in attributes {
			svg.attr(name, value);
		}
		// Colored copies color paths and circles, not polygons.
		if let Some((color, is_filled)) = color.filter(|_| element != "polygon") {
			svg.attr("stroke", color);
			if is_filled {
				svg.attr("fill", color);
			}
		}
		svg.close();
		svg.close_tag("marker");
		if definition.is_in_defs {
			svg.close_tag("defs");
		}
	}

	/// The markers of requirement relationships, in the color of `marker` or `color`.
	fn write_requirement(&self, svg: &mut Svg, id: &str, set: &str, color: Option<&str>) {
		let is_neo = set.ends_with("_neo");
		let is_contains = set.starts_with("requirement_contains");
		svg.open("defs").end();
		svg.open("marker")
			.attr("id", id)
			.attr("refX", if is_contains { "0" } else { "20" })
			.attr("refY", "10")
			.attr("markerWidth", "20")
			.attr("markerHeight", "20")
			.attr("orient", "auto");
		if is_neo {
			svg.attr("markerUnits", "userSpaceOnUse");
		}
		if is_neo && !is_contains {
			svg.attr("stroke-width", &self.colors.stroke_width).attr("viewBox", "0 0 25 20");
		}
		svg.attr_if("fill", &self.colors.edge).attr_if("stroke", &self.colors.edge).end();
		let stroke_width =
			if is_neo && is_contains { self.colors.stroke_width.as_str() } else { "" };
		let stroke = color.unwrap_or("");
		if is_contains {
			svg.open("g").end();
			svg.open("circle")
				.attr("cx", "10")
				.attr("cy", "10")
				.attr("r", "9")
				.attr("fill", "none")
				.attr_if("stroke-width", stroke_width)
				.attr_if("stroke", stroke)
				.close();
			for (x1, x2, y1, y2) in [("1", "19", "10", "10"), ("10", "10", "1", "19")] {
				svg.open("line")
					.attr("x1", x1)
					.attr("x2", x2)
					.attr("y1", y1)
					.attr("y2", y2)
					.attr_if("stroke-width", stroke_width)
					.attr_if("stroke", stroke)
					.close();
			}
			svg.close_tag("g");
		} else {
			svg.open("path").attr("d", "M0,0 L20,10 M20,10 L0,20");
			if is_neo {
				svg.attr("stroke-linejoin", "miter");
			}
			svg.attr_if("stroke", stroke).close();
		}
		svg.close_tag("marker").close_tag("defs");
	}

	/// `addEdgeMarker`: the `marker-start`/`marker-end` reference for `arrow`, a colored copy if
	/// the edge has a stroke color. `None` for no marker.
	pub fn reference(
		&mut self,
		arrow: &str,
		is_start: bool,
		use_margin: bool,
		stroke_color: Option<&str>,
	) -> Option<String> {
		let (kind, is_filled) = arrow_type(arrow)?;
		let suffix = format!(
			"{kind}{}{}",
			if is_start { "Start" } else { "End" },
			if use_margin && has_margin_variant(kind) { "-margin" } else { "" }
		);
		let id = format!("{}{suffix}", self.prefix);
		// Mermaid references markers the diagram did not write, and colors no copy of them.
		let Some(&(_, definition)) = self.written.iter().rev().find(|(known, _)| *known == suffix)
		else {
			return Some(format!("url(#{id})"));
		};
		let Some(color) = stroke_color.map(str::trim).filter(|color| !color.is_empty()) else {
			return Some(format!("url(#{id})"));
		};
		let color_id: String =
			color.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
		let colored_id = format!("{id}_{color_id}");
		if !self.colored.iter().any(|known| known.id == colored_id) {
			self.colored.push(ColoredMarker {
				id: colored_id.clone(),
				definition,
				color: color.to_owned(),
				is_filled,
			});
		}
		Some(format!("url(#{colored_id})"))
	}

	/// The colored copies the edges referenced.
	pub fn write_colored(&self, svg: &mut Svg, diagram_type: &str) {
		for marker in &self.colored {
			let color = Some((marker.color.as_str(), marker.is_filled));
			match marker.definition {
				Definition::Flowchart(definition) => {
					self.write_definition(svg, &marker.id, definition, color, diagram_type);
				}
				Definition::Plain(definition) => {
					self.write_plain(svg, &marker.id, definition, color);
				}
				Definition::Er(definition) => {
					self.write_er(svg, &marker.id, definition, Some(&marker.color), diagram_type);
				}
				Definition::Requirement(set) => {
					self.write_requirement(svg, &marker.id, set, Some(&marker.color));
				}
				Definition::Class(definition) => {
					self.write_class(svg, &marker.id, definition, color, diagram_type);
				}
			}
		}
	}
}
