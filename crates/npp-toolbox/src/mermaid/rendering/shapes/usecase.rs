//! The shapes of use case diagrams (`usecaseActor.ts` and its variants, `usecaseEllipse.ts`,
//! `usecaseBusiness.ts`, `usecaseJsonTable.ts`) and the system boundary cluster
//! (`clusters.js` `usecaseSystemBoundary`), painted with the rules of `usecase/styles.ts`.

use super::super::super::icons::{self, IconGlyph};
use super::super::super::label::LabelBox;
use super::super::super::rough;
use super::super::super::svg::{Svg, number};
use super::super::super::text::Font;
use super::super::super::{Context, Look};
use super::super::intersect::Bounds;
use super::super::sketch::{self, SketchPart};
use super::super::{BBox, Node, Point, Styles, Theme, layout_label, translate, write_label};
use super::{Intersect, Part, Shape, centered_box};

const ACTOR_FIGURE_WIDTH: f64 = 56.0;
const ACTOR_FIGURE_HEIGHT: f64 = 72.0;
const ACTOR_LABEL_GAP: f64 = 8.0;
const STEREOTYPE_LABEL_GAP: f64 = 2.0;
pub const ICON_FRAME_SIZE: f64 = 52.0;
const ICON_SIZE: f64 = 42.0;
pub const ICON_CENTER_Y: f64 = -2.0;
const CELL_PADDING_X: f64 = 8.0;
const CELL_PADDING_Y: f64 = 4.0;

pub const STICK_FIGURE_PATH: &str = "M 0 -12 C 6.627 -12 12 -17.373 12 -24 C 12 -30.627 6.627 -36 0 -36 \
	C -6.627 -36 -12 -30.627 -12 -24 C -12 -17.373 -6.627 -12 0 -12 Z M 0 -12 V 8 M -17 -5 H 17 \
	M 0 8 L -15 28 M 0 8 L 15 28";
pub const HOLLOW_BODY_PATH: &str =
	"M -22 -10 H 22 V 0 H 6 L 22 17 L 13 28 L 0 13 L -13 28 L -22 17 L -6 0 H -22 Z";
pub const AWESOME_SILHOUETTE_PATH: &str = "M 0 -34 C 7.18 -34 13 -28.18 13 -21 C 13 -13.82 7.18 -8 0 -8 \
	C -7.18 -8 -13 -13.82 -13 -21 C -13 -28.18 -7.18 -34 0 -34 Z M -24 25 C -24 7 -14 -3 0 -3 \
	C 14 -3 24 7 24 25 C 24 28 21 30 18 30 H -18 C -21 30 -24 28 -24 25 Z";

/// The variants of actors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActorVariant {
	#[default]
	Normal,
	Hollow,
	Awesome,
	Icon,
}

impl ActorVariant {
	pub fn name(self) -> &'static str {
		match self {
			ActorVariant::Normal => "normal",
			ActorVariant::Hollow => "hollow",
			ActorVariant::Awesome => "awesome",
			ActorVariant::Icon => "icon",
		}
	}

	/// `variantClass`.
	fn class(self) -> &'static str {
		match self {
			ActorVariant::Normal => "usecase-actor-shape usecase-actor-normal",
			ActorVariant::Hollow => "usecase-actor-hollow",
			ActorVariant::Awesome => "usecase-actor-awesome",
			ActorVariant::Icon => "usecase-actor-icon",
		}
	}
}

/// What use case nodes carry besides the common node data.
#[derive(Clone, Debug, Default)]
pub struct UsecaseNode {
	/// The font of the labels before the label styles: the actor or use case font of the
	/// configuration, else the diagram font.
	pub font: Option<Font>,
	/// The color of the labels.
	pub label_color: String,
	pub variant: ActorVariant,
	/// The icon of icon actors, `None` for the unknown icon.
	pub icon: Option<IconGlyph>,
	pub business: bool,
	/// The stereotype drawn as a label of its own, escaped, without the guillemets.
	pub stereotype: Option<String>,
	/// The key and value cells of JSON tables, escaped.
	pub json_rows: Vec<(String, String)>,
	/// `package` boundaries have a title tab.
	pub is_package: bool,
	/// The color slot of the `redux-color` themes.
	pub color_slot: Option<usize>,
	/// Attributes of the element: data, role and accessible name.
	pub attributes: Vec<(&'static str, String)>,
}

/// The elements of use case shapes.
#[derive(Clone, Debug, PartialEq)]
pub enum UsecasePart {
	/// The invisible box of an actor, which gives its size.
	Outline {
		width: f64,
		height: f64,
	},
	/// The glyph of an actor moved down by `offset_y`, with the roughjs calls of its outline
	/// in the `handDrawn` look.
	Glyph {
		variant: ActorVariant,
		offset_y: f64,
		business: bool,
		sketches: Vec<SketchPart>,
	},
	Ellipse {
		rx: f64,
		ry: f64,
		class: &'static str,
	},
	/// The slash of business use cases.
	BusinessMarker {
		d: String,
	},
	/// The rectangle of `[…]` use cases.
	Body {
		x: f64,
		y: f64,
		width: f64,
		height: f64,
	},
	/// A rectangle of a JSON table, `offset_y` moves its row.
	JsonCell {
		x: f64,
		y: f64,
		width: f64,
		height: f64,
		class: &'static str,
	},
	/// The start of the grid group and of a row group.
	JsonGridStart,
	JsonRowStart {
		index: usize,
		top: f64,
	},
	JsonGroupEnd,
}

/// `businessMarkerPathForCircle`.
fn business_marker_for_circle(center_y: f64, radius: f64) -> String {
	const ANGLE: f64 = std::f64::consts::PI / 3.0;
	const OFFSET_RATIO: f64 = 0.6;
	let center_offset = radius * OFFSET_RATIO;
	let half_chord = radius * (1.0 - OFFSET_RATIO.powi(2)).sqrt();
	let (direction_x, direction_y) = (ANGLE.cos(), -ANGLE.sin());
	let center_x = center_offset * -direction_y;
	let marker_center_y = center_y + center_offset * direction_x;
	let (delta_x, delta_y) = (half_chord * direction_x, half_chord * direction_y);
	let js = super::super::super::usecase::js_number_string;
	format!(
		"M {} {} L {} {}",
		js(center_x - delta_x),
		js(marker_center_y - delta_y),
		js(center_x + delta_x),
		js(marker_center_y + delta_y)
	)
}

fn business_marker(variant: ActorVariant) -> String {
	match variant {
		ActorVariant::Normal => business_marker_for_circle(-24.0, 12.0),
		ActorVariant::Hollow => business_marker_for_circle(-23.0, 9.0),
		ActorVariant::Awesome => business_marker_for_circle(-21.0, 13.0),
		ActorVariant::Icon => "M 12 -8 L 26 -26".to_owned(),
	}
}

/// A laid out use case shape: the shape, its main label and the other labels with centers.
pub struct UsecaseLayout {
	pub shape: Shape,
	pub label: LabelBox,
	pub labels: Vec<(LabelBox, Point)>,
}

/// Lays out the use case shape of `node`, its label in `font` wrapped at `wrap_width` and at
/// least `min_width` wide.
pub fn layout(
	context: &mut Context,
	node: &Node,
	font: &Font,
	styles: &Styles,
	(wrap_width, min_width): (f64, f64),
) -> Option<UsecaseLayout> {
	let usecase = node.usecase.as_deref()?;
	let wraps = styles.get("white-space") != Some("nowrap");
	let wrap = wraps.then_some(wrap_width);
	let stereotype = |context: &mut Context| {
		usecase.stereotype.as_ref().map(|stereotype| {
			layout_label(context, &format!("«{stereotype}»"), true, font, wrap, 0.0)
		})
	};
	match node.shape.as_str() {
		"usecaseActor" | "usecaseActorHollow" | "usecaseActorAwesome" | "usecaseActorIcon" => {
			let label = layout_label(context, &node.label, node.is_markdown, font, wrap, 0.0);
			let stereotype = stereotype(context);
			let padding = node.padding;
			let stereotype_height = stereotype.as_ref().map_or(0.0, |box_| box_.height);
			let content_height = ACTOR_FIGURE_HEIGHT
				+ ACTOR_LABEL_GAP
				+ label.height
				+ stereotype.as_ref().map_or(0.0, |_| stereotype_height + STEREOTYPE_LABEL_GAP);
			let total_width = ACTOR_FIGURE_WIDTH
				.max(label.width)
				.max(stereotype.as_ref().map_or(0.0, |box_| box_.width))
				+ padding * 2.0;
			let total_height = content_height + padding * 2.0;
			let content_top = -content_height / 2.0;
			let mut next = content_top + ACTOR_FIGURE_HEIGHT + ACTOR_LABEL_GAP;
			let mut labels = Vec::new();
			if let Some(stereotype) = stereotype {
				next += stereotype_height / 2.0;
				labels.push((stereotype, Point { x: 0.0, y: next }));
				next += stereotype_height / 2.0 + STEREOTYPE_LABEL_GAP;
			}
			let label_center = Point { x: 0.0, y: next + label.height / 2.0 };
			let shape = Shape {
				parts: vec![
					Part::Usecase(UsecasePart::Outline {
						width: total_width,
						height: total_height,
					}),
					Part::Usecase(UsecasePart::Glyph {
						variant: usecase.variant,
						offset_y: content_top + ACTOR_FIGURE_HEIGHT / 2.0,
						business: usecase.business,
						sketches: Vec::new(),
					}),
				],
				bbox: centered_box(total_width, total_height),
				intersect: Intersect::Rect,
				label_center,
			};
			Some(UsecaseLayout { shape, label, labels })
		}
		"usecaseEllipse" => {
			let label = layout_label(context, &node.label, node.is_markdown, font, wrap, min_width);
			let padding = node.padding / 2.0;
			let rx = label.width / 2.0 + padding * 2.0;
			let ry = label.height / 2.0 + padding * 2.0;
			let shape = Shape {
				parts: vec![Part::Usecase(UsecasePart::Ellipse {
					rx,
					ry,
					class: "basic label-container",
				})],
				bbox: centered_box(rx * 2.0, ry * 2.0),
				intersect: Intersect::Ellipse { rx, ry },
				label_center: Point::default(),
			};
			Some(UsecaseLayout { shape, label, labels: Vec::new() })
		}
		"usecaseBusiness" => {
			let label = layout_label(context, &node.label, node.is_markdown, font, wrap, min_width);
			let stereotype = stereotype(context);
			let label_gap = if stereotype.is_some() { 2.0 } else { 0.0 };
			let stereotype_height = stereotype.as_ref().map_or(0.0, |box_| box_.height);
			let label_height = label.height + stereotype_height + label_gap;
			let label_width = label.width.max(stereotype.as_ref().map_or(0.0, |box_| box_.width));
			let padding = node.padding / 2.0;
			let rx = label_width / 2.0 + padding * 2.0;
			let ry = label_height / 2.0 + padding * 2.0;
			let inset = (padding / 5.0).max(1.0).min(padding / 2.0);
			let (start_x, end_x) = (label_width / 2.0 + inset, rx - inset);
			let (start_nx, end_nx) = (start_x / rx, end_x / rx);
			let start_y = ry * (1.0 - start_nx * start_nx).max(0.0).sqrt();
			let end_y = -ry * (1.0 - end_nx * end_nx).max(0.0).sqrt();
			let js = super::super::super::usecase::js_number_string;
			let mut labels = Vec::new();
			if let Some(stereotype) = stereotype {
				labels.push((
					stereotype,
					Point { x: 0.0, y: -label_height / 2.0 + stereotype_height / 2.0 },
				));
			}
			let shape = Shape {
				parts: vec![
					Part::Usecase(UsecasePart::Ellipse {
						rx,
						ry,
						class: "basic label-container usecase-business-ellipse",
					}),
					Part::Usecase(UsecasePart::BusinessMarker {
						d: format!(
							"M {} {} L {} {}",
							js(start_x),
							js(start_y),
							js(end_x),
							js(end_y)
						),
					}),
				],
				bbox: centered_box(rx * 2.0, ry * 2.0),
				intersect: Intersect::Ellipse { rx, ry },
				label_center: Point { x: 0.0, y: label_height / 2.0 - label.height / 2.0 },
			};
			Some(UsecaseLayout { shape, label, labels })
		}
		"usecaseJsonTable" => {
			Some(json_table(context, node, font, styles, (wrap_width, min_width)))
		}
		"rect" => {
			// `squareRect`, painted as a use case.
			let label = layout_label(context, &node.label, node.is_markdown, font, wrap, min_width);
			let (padding_x, padding_y) = match node.look == Look::Neo {
				true => (16.0, 12.0),
				false => (node.padding * 2.0, node.padding),
			};
			let (width, height) = (label.width + padding_x * 2.0, label.height + padding_y * 2.0);
			let shape = Shape {
				parts: vec![Part::Usecase(UsecasePart::Body {
					x: -width / 2.0,
					y: -height / 2.0,
					width,
					height,
				})],
				bbox: centered_box(width, height),
				intersect: Intersect::Rect,
				label_center: Point::default(),
			};
			Some(UsecaseLayout { shape, label, labels: Vec::new() })
		}
		_ => None,
	}
}

fn json_table(
	context: &mut Context,
	node: &Node,
	font: &Font,
	styles: &Styles,
	(wrap_width, min_width): (f64, f64),
) -> UsecaseLayout {
	let usecase = node.usecase.as_deref().cloned().unwrap_or_default();
	let title =
		layout_label(context, &node.label, node.is_markdown, font, Some(wrap_width), min_width);
	let cell_width = context.config_number("flowchart", "wrappingWidth", 120.0);
	let cells: Vec<(LabelBox, LabelBox)> = usecase
		.json_rows
		.iter()
		.map(|(key, value)| {
			(
				layout_label(context, key, true, font, Some(cell_width), 0.0),
				layout_label(context, value, true, font, Some(cell_width), 0.0),
			)
		})
		.collect();
	let widest =
		|pick: fn(&(LabelBox, LabelBox)) -> f64| cells.iter().map(pick).fold(0.0, f64::max);
	let key_width = widest(|cell| cell.0.width) + CELL_PADDING_X * 2.0;
	let measured_value_width = widest(|cell| cell.1.width) + CELL_PADDING_X * 2.0;
	let title_width = title.width + CELL_PADDING_X * 2.0;
	let inner_width = title_width.max(key_width + measured_value_width);
	let value_width =
		measured_value_width + (inner_width - key_width - measured_value_width).max(0.0);
	let title_height = title.height + CELL_PADDING_Y * 2.0;
	let row_heights: Vec<f64> = cells
		.iter()
		.map(|(key, value)| key.height.max(value.height) + CELL_PADDING_Y * 2.0)
		.collect();
	let inner_height = title_height + row_heights.iter().sum::<f64>();
	let border_width = styles
		.get("stroke-width")
		.and_then(super::super::super::value::parse_leading_number)
		.filter(|width| *width >= 0.0)
		.unwrap_or(1.0);
	let total_width = inner_width + border_width * 2.0;
	let total_height = inner_height + border_width * 2.0;
	let (left, top) = (-inner_width / 2.0, -inner_height / 2.0);
	let mut parts = vec![
		Part::Usecase(UsecasePart::JsonCell {
			x: -total_width / 2.0,
			y: -total_height / 2.0,
			width: total_width,
			height: total_height,
			class: "label-container usecase-json-border",
		}),
		Part::Usecase(UsecasePart::JsonGridStart),
		Part::Usecase(UsecasePart::JsonCell {
			x: left,
			y: top,
			width: inner_width,
			height: title_height,
			class: "usecase-json-cell usecase-json-title-cell",
		}),
	];
	let mut labels = Vec::new();
	let mut row_top = top + title_height;
	for (index, ((key, value), height)) in cells.into_iter().zip(row_heights).enumerate() {
		parts.push(Part::Usecase(UsecasePart::JsonRowStart { index, top: row_top }));
		parts.push(Part::Usecase(UsecasePart::JsonCell {
			x: left + key_width,
			y: 0.0,
			width: value_width,
			height,
			class: "usecase-json-cell usecase-json-value-cell",
		}));
		parts.push(Part::Usecase(UsecasePart::JsonCell {
			x: left,
			y: 0.0,
			width: key_width,
			height,
			class: "usecase-json-cell usecase-json-key-cell",
		}));
		parts.push(Part::Usecase(UsecasePart::JsonGroupEnd));
		labels.push((key, Point { x: left + key_width / 2.0, y: row_top + height / 2.0 }));
		labels.push((
			value,
			Point { x: left + key_width + value_width / 2.0, y: row_top + height / 2.0 },
		));
		row_top += height;
	}
	parts.push(Part::Usecase(UsecasePart::JsonGroupEnd));
	let shape = Shape {
		parts,
		bbox: centered_box(total_width, total_height),
		intersect: Intersect::Rect,
		label_center: Point { x: 0.0, y: top + title_height / 2.0 },
	};
	UsecaseLayout { shape, label: title, labels }
}

/// The paint of the role colors, or of the color slot of `node` under `colorScheme: rotate`.
/// Only actors take their slot: the role rule of use case bodies has the specificity of the
/// slot rule and comes later in Mermaid's style sheet.
struct Paint<'t> {
	fill: &'t str,
	stroke: &'t str,
}

fn slot_paint<'t>(theme: &'t Theme, node: &Node, fill: &'t str, stroke: &'t str) -> Paint<'t> {
	let slot = node.usecase.as_deref().and_then(|usecase| usecase.color_slot);
	match slot.filter(|_| theme.usecase.rotate && !theme.border_colors.is_empty()) {
		Some(slot) => {
			let slot = slot % theme.border_colors.len();
			let fill = match theme.background_colors.is_empty() {
				true => fill,
				false => theme.background_colors[slot % theme.background_colors.len()].as_str(),
			};
			Paint { fill, stroke: &theme.border_colors[slot] }
		}
		None => Paint { fill, stroke },
	}
}

fn write_glyph(
	svg: &mut Svg,
	(variant, offset_y, business, sketches): (ActorVariant, f64, bool, &[SketchPart]),
	node: &Node,
	css: &str,
	theme: &Theme,
) {
	let usecase = &theme.usecase;
	let paint = slot_paint(theme, node, &usecase.actor_bkg, &usecase.actor_border);
	let is_neo = node.look == Look::Neo;
	let filter = if is_neo { theme.neo_filter.as_str() } else { "" };
	svg.open("g")
		.attr("class", &format!("usecase-actor-glyph {}", variant.class()))
		.attr_if("style", css)
		.attr("transform", &format!("translate(0,{})", number(offset_y)))
		.attr_if("fill", paint.fill)
		.attr_if("stroke", paint.stroke)
		.attr("stroke-width", "2px")
		.end();
	// `[data-look="neo"].node path` sets the width of the paths; its stroke is outranked.
	let path_width = if is_neo { theme.node_stroke_width.as_str() } else { "" };
	let path = |svg: &mut Svg, d: &str, class: &str, hollow: bool| {
		svg.open("path")
			.attr("class", class)
			.attr("d", d)
			.attr_if("stroke", paint.stroke)
			.attr_if("stroke-width", path_width);
		if hollow {
			svg.attr("fill", "none");
		}
		svg.close();
	};
	for sketch in sketches {
		sketch::write_part(svg, sketch);
	}
	match variant {
		_ if !sketches.is_empty() && variant != ActorVariant::Icon => {}
		ActorVariant::Normal => path(svg, STICK_FIGURE_PATH, "usecase-actor-stick", false),
		ActorVariant::Hollow => {
			svg.open("circle")
				.attr("class", "usecase-actor-hollow-head")
				.attr("cx", "0")
				.attr("cy", "-23")
				.attr("r", "9")
				.attr("fill", "none")
				.attr_if("stroke", paint.stroke)
				.attr_if("filter", filter)
				.close();
			path(svg, HOLLOW_BODY_PATH, "usecase-actor-hollow-body", true);
		}
		ActorVariant::Awesome => {
			path(svg, AWESOME_SILHOUETTE_PATH, "usecase-actor-awesome-silhouette", false);
		}
		ActorVariant::Icon => {
			// The neo look's `.node rect` rule strokes the frame and the icon tile.
			let rect_stroke = if is_neo { theme.neo_stroke.as_str() } else { "" };
			let frame = -ICON_FRAME_SIZE / 2.0;
			if sketches.is_empty() {
				svg.open("rect")
					.attr("class", "usecase-actor-icon-frame")
					.number_attr("x", frame)
					.number_attr("y", ICON_CENTER_Y + frame)
					.number_attr("width", ICON_FRAME_SIZE)
					.number_attr("height", ICON_FRAME_SIZE)
					.attr("rx", "4")
					.attr("ry", "4")
					.attr_if("stroke", rect_stroke)
					.attr_if("filter", filter)
					.close();
			}
			if let Some(usecase) = &node.usecase
				&& let Some(glyph) = &usecase.icon
			{
				let icon_box = glyph.box_in_square(ICON_SIZE);
				let x = -icon_box.width / 2.0 - icon_box.x;
				let y = ICON_CENTER_Y - icon_box.height / 2.0 - icon_box.y;
				svg.open("g")
					.attr("class", "usecase-actor-icon-symbol")
					.attr("aria-hidden", "true")
					.attr("transform", &format!("translate({},{})", number(x), number(y)))
					.end();
				// `fill="currentColor"` with the color of `.usecase-actor`, and the stroke of
				// the glyph.
				icons::write_glyph_svg(svg, glyph, ICON_SIZE, &|svg| {
					svg.attr_if("fill", &usecase.label_color)
						.attr_if("stroke", paint.stroke)
						.attr_if("stroke-width", path_width);
				});
				svg.close_tag("g");
				svg.close_tag("g");
				return;
			}
			// Mermaid's unknown icon, in a 42 pixel box.
			let half = ICON_SIZE / 2.0;
			svg.open("g")
				.attr("class", "usecase-actor-icon-symbol usecase-actor-icon-fallback")
				.attr("aria-hidden", "true")
				.attr(
					"transform",
					&format!("translate({},{})", number(-half), number(ICON_CENTER_Y - half)),
				)
				.end();
			svg.open("svg")
				.attr("width", "42")
				.attr("height", "42")
				.attr("viewBox", "0 0 80 80")
				.end();
			icons::write_unknown_icon_with(svg, &|svg| {
				svg.attr_if("stroke", rect_stroke).attr_if("filter", filter);
			});
			svg.close_tag("svg").close_tag("g");
		}
	}
	if business {
		svg.open("path")
			.attr("class", "usecase-business-marker usecase-actor-business-marker")
			.attr("d", &business_marker(variant))
			.attr("fill", "none")
			.close();
	}
	svg.close_tag("g");
}

/// Writes a part of a use case shape.
pub fn write(svg: &mut Svg, part: &UsecasePart, node: &Node, styles: &Styles, theme: &Theme) {
	let css = styles.node_css_important();
	let usecase = &theme.usecase;
	let is_neo = node.look == Look::Neo;
	let filter = if is_neo { theme.neo_filter.as_str() } else { "" };
	match part {
		&UsecasePart::Outline { width, height } => {
			svg.open("rect")
				.attr("class", "usecase-actor-outline")
				.number_attr("x", -width / 2.0)
				.number_attr("y", -height / 2.0)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr("opacity", "0")
				.attr("aria-hidden", "true")
				.close();
		}
		UsecasePart::Glyph { variant, offset_y, business, sketches } => {
			write_glyph(svg, (*variant, *offset_y, *business, sketches), node, &css, theme);
		}
		&UsecasePart::Ellipse { rx, ry, class } => {
			let paint = Paint { fill: &usecase.bkg, stroke: &usecase.border };
			svg.open("ellipse")
				.attr("class", class)
				.attr_if("fill", paint.fill)
				.attr_if("stroke", paint.stroke)
				.attr("stroke-width", "2px")
				.attr_if("style", &css)
				.number_attr("rx", rx)
				.number_attr("ry", ry)
				.attr("cx", "0")
				.attr("cy", "0")
				.close();
		}
		UsecasePart::BusinessMarker { d } => {
			let paint = Paint { fill: &usecase.bkg, stroke: &usecase.border };
			svg.open("path")
				.attr("class", "usecase-business-marker")
				.attr("d", d)
				.attr("fill", "none")
				.attr_if("stroke", paint.stroke)
				.attr_if("stroke-width", if is_neo { theme.node_stroke_width.as_str() } else { "" })
				.attr_if("style", &css)
				.close();
		}
		&UsecasePart::Body { x, y, width, height } => {
			let paint = Paint { fill: &usecase.bkg, stroke: &usecase.border };
			svg.open("rect")
				.attr("class", "basic label-container")
				.attr_if("fill", paint.fill)
				.attr_if("stroke", paint.stroke)
				.attr("stroke-width", "2px")
				.attr_if("filter", filter)
				.attr_if("style", &css)
				.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", width)
				.number_attr("height", height)
				.close();
		}
		&UsecasePart::JsonCell { x, y, width, height, class } => {
			svg.open("rect")
				.attr("class", class)
				.attr_if("fill", &usecase.json_bkg)
				.attr_if("stroke", if is_neo { &theme.neo_stroke } else { &usecase.json_border })
				.attr("stroke-width", "1px")
				.attr_if("filter", filter)
				.number_attr("x", x)
				.number_attr("y", y)
				.number_attr("width", width)
				.number_attr("height", height)
				.attr_if("style", &css)
				.close();
		}
		UsecasePart::JsonGridStart => {
			svg.open("g").attr("class", "usecase-json-table-grid").end();
		}
		&UsecasePart::JsonRowStart { index, top } => {
			svg.open("g")
				.attr("class", "usecase-json-row")
				.attr("data-row-index", &index.to_string())
				.attr("transform", &format!("translate(0,{})", number(top)))
				.end();
		}
		UsecasePart::JsonGroupEnd => {
			svg.close_tag("g");
		}
	}
}

/// `getUsecaseSystemBoundaryGeometry` and `usecaseSystemBoundary`: draws the boundary `node` laid
/// out at `bounds`; returns the drawn bounds and `diff`.
pub fn write_boundary(
	svg: &mut Svg,
	bbox: &mut BBox,
	context: &mut Context,
	node: &Node,
	bounds: Bounds,
	theme: &Theme,
) -> (Bounds, f64) {
	let usecase = node.usecase.as_deref().cloned().unwrap_or_default();
	let styles = Styles::compile(&[&node.css_compiled_styles, &node.css_styles]);
	let (font, color) = super::super::node_label_font(context, node, &styles);
	// `createText` treats the title as Markdown and wraps it at 200 pixels.
	let label = layout_label(context, &node.label, true, &font, Some(200.0), 0.0);
	let (label_width, label_height) = (label.width, label.height);
	let padding = node.padding;
	let horizontal_padding = if usecase.is_package { 20.0 } else { padding };
	let tab_height = if usecase.is_package { label_height + 10.0 } else { 0.0 };
	let width = bounds.width.max(label_width + horizontal_padding);
	let height = match usecase.is_package {
		true => bounds.height.max(tab_height + padding * 2.0),
		false => bounds.height,
	};
	let (x, y) = (bounds.x - width / 2.0, bounds.y - height / 2.0);
	let tab_width = match usecase.is_package {
		true => width.min((label_width + 20.0).max(80.0)),
		false => 0.0,
	};
	let diff = match bounds.width <= label_width + padding {
		true => (width - bounds.width) / 2.0 - padding,
		false => -padding,
	};
	let kind = if usecase.is_package { "package" } else { "rect" };
	let is_neo = node.look == Look::Neo;
	svg.open("g")
		.attr(
			"class",
			format!(
				"cluster usecase-system-boundary usecase-system-boundary-{kind} {}",
				node.css_classes
			)
			.trim_end(),
		)
		.attr("id", &format!("{}-{}", context.id, node.dom_id))
		.attr("data-boundary-type", kind)
		.attr("data-look", node.look.data_name());
	let slot = usecase.color_slot.filter(|_| !theme.border_colors.is_empty());
	if let Some(slot) = slot {
		svg.attr("data-color-id", &format!("color-{}", slot % theme.border_colors.len()));
	}
	for (name, value) in &usecase.attributes {
		svg.attr(name, value);
	}
	svg.end();
	let (fill, stroke) = match slot {
		Some(slot) => {
			let slot = slot % theme.border_colors.len();
			let fill = match theme.background_colors.is_empty() {
				true => theme.usecase.boundary_bkg.as_str(),
				false => theme.background_colors[slot % theme.background_colors.len()].as_str(),
			};
			(fill, theme.border_colors[slot].as_str())
		}
		// `[data-look="neo"].cluster rect` comes after the boundary rule of the same specificity.
		None if is_neo => (theme.usecase.boundary_bkg.as_str(), theme.neo_stroke.as_str()),
		None => (theme.usecase.boundary_bkg.as_str(), theme.usecase.boundary_border.as_str()),
	};
	let css = styles.node_css_important();
	let filter = if is_neo { theme.neo_filter.as_str() } else { "" };
	let rect = |svg: &mut Svg, class: &str, (rect_y, rect_width, rect_height): (f64, f64, f64)| {
		svg.open("rect")
			.attr("class", class)
			.attr_if("fill", fill)
			.attr_if("stroke", stroke)
			.attr("stroke-width", "1px")
			.attr_if("filter", filter)
			.attr_if("style", &css)
			.number_attr("x", x)
			.number_attr("y", rect_y)
			.number_attr("width", rect_width)
			.number_attr("height", rect_height)
			.close();
	};
	if node.look == Look::HandDrawn {
		let mut options =
			sketch::user_node_overrides(&styles, theme, context.sketch_seed(&node.id));
		let style = |key: &str| styles.get(key).filter(|value| !value.is_empty());
		options.fill = Some(style("fill").unwrap_or(&theme.cluster_bkg).to_owned());
		options.stroke = style("stroke").unwrap_or(&theme.cluster_border).to_owned();
		options.stroke_width = style("stroke-width")
			.map(|width| width.replacen("px", "", 1))
			.filter(|width| !width.is_empty())
			.unwrap_or_else(|| "1".to_owned());
		// The color slot rules reach both paths.
		let paint = sketch::PathPaint {
			stroke: slot.map(|_| stroke),
			fill: slot.filter(|_| !theme.background_colors.is_empty()).map(|_| fill),
			..sketch::PathPaint::default()
		};
		let body =
			("boundary-body label-container", [x, y + tab_height, width, height - tab_height]);
		let tab = (
			"boundary-tab system-boundary-package-tab label-container",
			[x, y, tab_width, tab_height],
		);
		let rects = match usecase.is_package {
			true => vec![tab, body],
			false => vec![body],
		};
		for (class, [rect_x, rect_y, rect_width, rect_height]) in rects {
			let sketched = rough::rectangle(rect_x, rect_y, rect_width, rect_height, &options);
			sketch::write_group(svg, &sketched, class, &paint);
			if let Some(ink) = sketch::sketch_box(&sketched) {
				bbox.include(&ink, 0.0, 0.0);
			}
		}
	} else {
		if usecase.is_package {
			rect(
				svg,
				"boundary-tab system-boundary-package-tab label-container",
				(y, tab_width, tab_height),
			);
		}
		rect(svg, "boundary-body label-container", (y + tab_height, width, height - tab_height));
		bbox.include_rect(x, y, width, height);
	}
	let (top_margin, _) = super::super::title_margins(context);
	let (label_left, label_top) = match usecase.is_package {
		true => (x + (tab_width - label_width) / 2.0, y + (tab_height - label_height) / 2.0),
		false => (bounds.x - label_width / 2.0, y + top_margin),
	};
	svg.open("g")
		.attr("class", "cluster-label system-boundary-title")
		.attr("transform", &translate(label_left, label_top))
		.end();
	let fill = color.unwrap_or_else(|| theme.usecase.title_color.clone());
	write_label(svg, context, &label, Point { x: label_width / 2.0, y: label_height / 2.0 }, &fill);
	svg.close_tag("g").close_tag("g");
	if !label.is_empty() {
		bbox.include_rect(label_left, label_top, label_width, label_height);
	}
	(Bounds { x: bounds.x, y: bounds.y, width, height }, diff)
}
