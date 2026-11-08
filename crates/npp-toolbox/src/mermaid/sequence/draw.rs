//! The drawing functions of `svgDraw.js`, with the paint of the diagram's style sheet as
//! presentation attributes.

use super::db::ActorKind;
use super::{Align, Layer, LoopModel, Renderer, split_breaks};
use crate::mermaid::color::js_round;
use crate::mermaid::label::{LabelBox, decode_entities_to_text, write as write_label};
use crate::mermaid::math;
use crate::mermaid::svg::{Anchor, Baseline, Svg, TextRun, number};
use crate::mermaid::text::Font;

/// Width of the stick figure.
pub const ACTOR_TYPE_WIDTH: f64 = 36.0;

/// The band model of `actorBands.ts` for the neo look: glyph, gap, label, gap above the
/// lifeline.
const GLYPH_BAND_HEIGHT: f64 = 44.0;
const GLYPH_LABEL_GAP: f64 = 6.0;
const LABEL_LIFELINE_GAP: f64 = 6.0;

/// The stick figure's extent in its own units.
const ACTOR_GLYPH_TOP: f64 = -5.0;
const ACTOR_GLYPH_BOTTOM: f64 = 60.0;
const ACTOR_GLYPH_HEIGHT: f64 = ACTOR_GLYPH_BOTTOM - ACTOR_GLYPH_TOP;

pub fn actor_stack_height(text_height: f64) -> f64 {
	GLYPH_BAND_HEIGHT + GLYPH_LABEL_GAP + text_height + LABEL_LIFELINE_GAP
}

/// What `draw_actor_shape` draws.
struct Shape {
	index: usize,
	kind: ActorKind,
	actor_y: f64,
	is_footer: bool,
	bands: Option<Bands>,
	color_index: usize,
}

/// Where a participant's parts go under the neo look.
struct Bands {
	lifeline_start_y: f64,
	glyph_bottom_y: f64,
	label_center_y: f64,
}

fn header_bands(actor_y: f64, row_height: f64, text_height: f64) -> Bands {
	let lifeline_start_y = actor_y + row_height;
	Bands {
		lifeline_start_y,
		glyph_bottom_y: lifeline_start_y - LABEL_LIFELINE_GAP - text_height - GLYPH_LABEL_GAP,
		label_center_y: lifeline_start_y - LABEL_LIFELINE_GAP - text_height / 2.0,
	}
}

fn footer_bands(actor_y: f64, text_height: f64) -> Bands {
	let glyph_bottom_y = actor_y + LABEL_LIFELINE_GAP + GLYPH_BAND_HEIGHT;
	Bands {
		lifeline_start_y: actor_y,
		glyph_bottom_y,
		label_center_y: glyph_bottom_y + GLYPH_LABEL_GAP + text_height / 2.0,
	}
}

/// The options of `drawText`.
pub struct TextData<'a> {
	pub text: &'a str,
	pub x: f64,
	pub y: f64,
	pub width: Option<f64>,
	pub anchor: Option<Align>,
	/// `valign: 'middle'`: the lines from the middle of the top margin down.
	pub is_centered: bool,
	pub text_margin: f64,
	/// `dy="1em"`
	pub dy_em: bool,
	pub font: &'a Font,
	pub fill: &'a str,
}

/// `getNoteRect`-based rectangles and other shapes as SVG.
fn rect(svg: &mut Svg, x: f64, y: f64, width: f64, height: f64) -> &mut Svg {
	svg.open("rect")
		.number_attr("x", x)
		.number_attr("y", y)
		.number_attr("width", width)
		.number_attr("height", height)
}

fn line(svg: &mut Svg, x1: f64, y1: f64, x2: f64, y2: f64) -> &mut Svg {
	svg.open("line")
		.number_attr("x1", x1)
		.number_attr("y1", y1)
		.number_attr("x2", x2)
		.number_attr("y2", y2)
}

impl Renderer<'_, '_> {
	fn shadow_filter(&self) -> String {
		format!("url(#{}-drop-shadow)", self.context.id)
	}

	/// One line of text with its font.
	fn text_line(
		&mut self,
		svg: &mut Svg,
		position: (f64, f64),
		(anchor, baseline): (Anchor, Baseline),
		font: &Font,
		content: &str,
		fill: &str,
	) {
		let content = decode_entities_to_text(content);
		if content.trim().is_empty() {
			return;
		}
		let run =
			TextRun { x: position.0, y: position.1, anchor, baseline, font, content: &content };
		// Text inside the figures would inherit their stroke.
		svg.text(run, |svg| _ = svg.attr_if("fill", fill).attr("stroke", "none"));
	}

	/// `drawText`: the lines of `data.text` placed in their box; returns the height of the text,
	/// the sum of the line boxes.
	pub(super) fn draw_text(&mut self, svg: &mut Svg, data: TextData) -> f64 {
		let lines = split_breaks(data.text);
		let margin = data.text_margin;
		let (x, anchor, baseline) = match (data.anchor, data.width) {
			(Some(Align::Start), Some(_)) => {
				(js_round(data.x + margin), Anchor::Start, Baseline::Middle)
			}
			(Some(Align::Middle), Some(width)) => {
				(js_round(data.x + width / 2.0), Anchor::Middle, Baseline::Middle)
			}
			(Some(Align::End), Some(width)) => {
				(js_round(data.x + width - margin), Anchor::End, Baseline::Middle)
			}
			(anchor, _) => {
				let anchor = match anchor {
					Some(Align::Middle) => Anchor::Middle,
					Some(Align::End) => Anchor::End,
					_ => Anchor::Start,
				};
				(data.x, anchor, Baseline::Alphabetic)
			}
		};
		let is_aligned = data.is_centered && margin > 0.0;
		let mut text_height = 0.0;
		for (index, content) in lines.iter().enumerate() {
			let y = match is_aligned {
				true => js_round(data.y + (text_height * 2.0 + margin) / 2.0),
				false => data.y,
			};
			let dy = match data.dy_em {
				true => data.font.size,
				false if margin == 0.0 => index as f64 * data.font.size,
				false => 0.0,
			};
			self.text_line(svg, (x, y + dy), (anchor, baseline), data.font, content, data.fill);
			if is_aligned {
				text_height += self.line_height(content, data.font);
			}
		}
		text_height
	}

	/// Writes a formula text with `(center_x, top)`, what `drawKatex` and `byKatex` place.
	pub(super) fn write_math(
		&mut self,
		svg: &mut Svg,
		label: &LabelBox,
		(center_x, top): (f64, f64),
		fill: &str,
	) {
		write_label(svg, self.context, label, center_x, top, &|svg: &mut Svg| {
			svg.attr_if("fill", fill).attr("stroke", "none");
		});
	}

	/// `_drawTextCandidateFunc` with `byTspan`: lines centered in the box around their middle;
	/// `byKatex` for text with formulas.
	fn draw_centered_text(
		&mut self,
		svg: &mut Svg,
		content: &str,
		(x, y, width, height): (f64, f64, f64, f64),
		font: &Font,
		fill: &str,
	) {
		if math::has_math(content) {
			let (_, math_height) = self.math_dimensions(content);
			let label = self.actor_math_label(content);
			let top = y + height / 2.0 - math_height / 2.0;
			self.write_math(svg, &label, (x + width / 2.0, top), fill);
			return;
		}
		let lines = split_breaks(content);
		let count = lines.len() as f64;
		for (index, line) in lines.iter().enumerate() {
			let dy = index as f64 * font.size - font.size * (count - 1.0) / 2.0;
			let position = (x + width / 2.0, y + height / 2.0 + dy);
			self.text_line(svg, position, (Anchor::Middle, Baseline::Central), font, line, fill);
		}
	}

	/// Stroke and fill of the `index`th participant: its palette color or the theme's.
	fn actor_paint(&self, index: usize) -> (String, String) {
		let (stroke, fill) = self.paint.palette(index).unwrap_or((None, None));
		(
			stroke.unwrap_or_else(|| self.paint.actor_border.clone()),
			fill.unwrap_or_else(|| self.paint.actor_bkg.clone()),
		)
	}

	/// `drawActor`: draws the participant `index` as header or footer; returns its height.
	pub(super) fn draw_actor(&mut self, index: usize, is_footer: bool, color_index: usize) -> f64 {
		let kind = self.db.actors[index].1.kind;
		if kind == ActorKind::Unknown {
			return f64::NAN;
		}
		let layout = self.actors[index].clone();
		let actor_y = match is_footer {
			true => layout.stopy.unwrap_or(0.0),
			false => layout.starty,
		};
		let bands = match self.conf.is_neo {
			true => {
				let text_height = layout.text_height.unwrap_or(0.0);
				Some(match is_footer {
					true => footer_bands(actor_y, text_height),
					false => header_bands(actor_y, layout.height, text_height),
				})
			}
			false => None,
		};
		let center = layout.x + layout.width / 2.0;
		let lifeline_start = match (&bands, kind) {
			(_, ActorKind::Participant | ActorKind::Collections | ActorKind::Queue) => {
				actor_y + layout.height
			}
			(Some(bands), _) => bands.lifeline_start_y,
			(None, ActorKind::Control | ActorKind::Entity) => actor_y + 75.0,
			(None, ActorKind::Database) => {
				actor_y + layout.height + 2.0 * self.conf.box_text_margin
			}
			(None, _) => actor_y + 80.0,
		};
		// The lowered group with the lifeline; the shapes drawn as boxes go into it, the figures
		// on top of the diagram.
		let is_box = matches!(
			kind,
			ActorKind::Participant
				| ActorKind::Collections
				| ActorKind::Queue
				| ActorKind::Database
		);
		let mut layers = Vec::new();
		let mut group = Svg::new();
		group.open("g").end();
		if !is_footer {
			layers.push(Layer::Svg(group));
			layers.push(Layer::Lifeline { x: center, y1: lifeline_start, actor: index });
			group = Svg::new();
			if is_box {
				group.open("g").end();
			}
		}
		let mut figure = match is_box {
			true => group,
			false => {
				group.close_tag("g");
				layers.push(Layer::Svg(group));
				Svg::new()
			}
		};
		let shape = Shape { index, kind, actor_y, is_footer, bands, color_index };
		let height = self.draw_actor_shape(&mut figure, shape);
		if is_box {
			if !is_footer {
				figure.close_tag("g");
			}
			figure.close_tag("g");
			layers.push(Layer::Svg(figure));
		} else {
			self.body.push(figure);
		}
		self.lowered.push(layers);
		height
	}

	/// The shape and label of a participant; returns its height, which the classic figures
	/// replace by their measured one.
	fn draw_actor_shape(&mut self, figure: &mut Svg, shape: Shape) -> f64 {
		let Shape { index, kind, actor_y, is_footer, bands, color_index } = shape;
		let layout = self.actors[index].clone();
		let center = layout.x + layout.width / 2.0;
		let (stroke, fill) = self.actor_paint(color_index);
		let font = self.conf.actor_font.draw.clone();
		let text_fill = self.paint.actor_text.clone();
		let stroke_width = self.paint.stroke_width.clone();
		let actor_bkg = self.paint.actor_bkg.clone();
		let filter = if self.conf.is_neo { self.shadow_filter() } else { String::new() };
		let description = layout.description.clone();
		let label_box_height = self.conf.label_box_height;
		let row_height = layout.height;
		let radius = 22.0;
		// The box the label is centered in: its top and height.
		let (label_y, label_height);
		match kind {
			ActorKind::Participant => {
				let corner = if self.conf.is_neo { 6.0 } else { 3.0 };
				rect(figure, layout.x, actor_y, layout.width, row_height)
					.number_attr("rx", corner)
					.number_attr("ry", corner)
					.attr_if("stroke", &stroke)
					.attr_if("fill", &fill)
					.attr("stroke-width", &stroke_width)
					.attr_if("filter", &filter)
					.close();
				(label_y, label_height) = (actor_y, row_height);
			}
			ActorKind::Collections => {
				let offset = 6.0;
				let height = match self.conf.is_neo {
					true => row_height - offset,
					false => row_height,
				};
				figure.open("g").attr_if("filter", &filter).end();
				for (x, y) in [(layout.x, actor_y), (layout.x - offset, actor_y + offset)] {
					rect(figure, x, y, layout.width, height)
						.attr_if("stroke", &stroke)
						.attr_if("fill", &fill)
						.attr("stroke-width", &stroke_width)
						.close();
				}
				figure.close_tag("g");
				let (x, y) = (layout.x - offset, actor_y + offset);
				self.draw_centered_text(
					figure,
					&description,
					(x, y, layout.width, height),
					&font,
					&text_fill,
				);
				return row_height;
			}
			ActorKind::Queue => {
				let (x, y, width, height) = (layout.x, actor_y, layout.width, row_height);
				let ry = height / 2.0;
				let rx = ry / (2.5 + height / 50.0);
				let (x, top, rx_text, ry_text) =
					(number(x), number(y + ry), number(rx), number(ry));
				let body = format!(
					"M {x},{top} a {rx_text},{ry_text} 0 0 0 0,{h} h {w} a {rx_text},{ry_text} 0 0 0 0,-{h} Z",
					h = number(height),
					w = number(width - 2.0 * rx)
				);
				let arc = format!("M {x},{top} a {rx_text},{ry_text} 0 0 0 0,{}", number(height));
				for (path, dx, filter) in [(body, rx, filter.as_str()), (arc, width - rx, "")] {
					let transform = format!("translate({}, {})", number(dx), number(-height / 2.0));
					figure.open("g").attr("transform", &transform).attr_if("filter", filter).end();
					figure
						.open("path")
						.attr("d", &path)
						.attr_if("stroke", &stroke)
						.attr_if("fill", &fill)
						.attr("stroke-width", &stroke_width)
						.close();
					figure.close_tag("g");
				}
				(label_y, label_height) = (actor_y, row_height);
			}
			ActorKind::Database => {
				let width = layout.width / 3.0;
				let rx = width / 2.0;
				let ry = rx / (2.5 + width / 50.0);
				let height = if bands.is_some() { GLYPH_BAND_HEIGHT } else { width };
				let top = match &bands {
					Some(bands) => bands.glyph_bottom_y - height - ry,
					None => actor_y,
				};
				let d = format!(
					"M {x},{top} a {rx},{ry} 0 0 0 {w},0 a {rx},{ry} 0 0 0 -{w},0 l 0,{side} a {rx},{ry} 0 0 0 {w},0 l 0,-{side}",
					x = number(layout.x),
					top = number(top + ry),
					rx = number(rx),
					ry = number(ry),
					w = number(width),
					side = number(height - 2.0 * ry)
				);
				figure
					.open("g")
					.attr("transform", &format!("translate({}, {})", number(width), number(ry)))
					.attr_if("stroke", &stroke)
					.attr_if("fill", &fill)
					.attr("stroke-width", &stroke_width)
					.attr_if("filter", &filter)
					.end();
				figure.open("path").attr("d", &d).close();
				figure.close_tag("g");
				let y = match &bands {
					Some(bands) => bands.label_center_y - row_height / 2.0,
					None => actor_y + 35.0,
				};
				let label = (layout.x, y, layout.width, row_height);
				self.draw_centered_text(figure, &description, label, &font, &text_fill);
				if bands.is_none() {
					self.actors[index].height = width + label_box_height;
				}
				return self.actors[index].height;
			}
			ActorKind::Actor => {
				let scale =
					if bands.is_some() { GLYPH_BAND_HEIGHT / ACTOR_GLYPH_HEIGHT } else { 1.0 };
				let gy = |offset: f64| match &bands {
					Some(bands) => bands.glyph_bottom_y - (ACTOR_GLYPH_BOTTOM - offset) * scale,
					None => actor_y + offset,
				};
				let gx = |offset: f64| center + offset * scale;
				figure.open("g").attr_if("stroke", &stroke).end();
				let half = ACTOR_TYPE_WIDTH / 2.0;
				for (x1, y1, x2, y2) in [
					(center, gy(25.0), center, gy(45.0)),
					(gx(-half), gy(33.0), gx(half), gy(33.0)),
					(gx(-half), gy(60.0), center, gy(45.0)),
					(center, gy(45.0), gx(half - 2.0), gy(60.0)),
				] {
					line(figure, x1, y1, x2, y2)
						.attr_if("fill", &actor_bkg)
						.attr("stroke-width", "2")
						.close();
				}
				figure
					.open("circle")
					.number_attr("cx", center)
					.number_attr("cy", gy(10.0))
					.number_attr("r", 15.0 * scale)
					.attr_if("fill", &actor_bkg)
					.attr("stroke-width", "2")
					.close();
				if bands.is_none() {
					self.actors[index].height = ACTOR_GLYPH_HEIGHT;
				}
				label_height = self.actors[index].height;
				label_y = match &bands {
					Some(bands) => bands.label_center_y - label_height / 2.0,
					None => actor_y + 35.0,
				};
			}
			ActorKind::Boundary => {
				let icon_y = match &bands {
					Some(bands) => bands.glyph_bottom_y - radius,
					None => actor_y + 12.0,
				};
				figure.open("g").attr_if("stroke", &stroke);
				if bands.is_none() {
					let dy = number(radius / 2.0 + 10.0);
					figure.attr("transform", &format!("translate(0,{dy})"));
				}
				figure.attr_if("filter", &filter).end();
				let left = center - radius * 2.5;
				for (x1, y1, x2, y2) in [
					(left, icon_y, center - 15.0, icon_y),
					(left, icon_y - 10.0, left, icon_y + 10.0),
				] {
					line(figure, x1, y1, x2, y2)
						.attr_if("fill", &actor_bkg)
						.attr("stroke-width", "2")
						.close();
				}
				figure
					.open("circle")
					.number_attr("cx", center)
					.number_attr("cy", icon_y)
					.number_attr("r", radius)
					.attr_if("fill", &actor_bkg)
					.attr("stroke-width", "2")
					.close();
				if bands.is_none() {
					self.actors[index].height = 2.0 * radius + label_box_height;
				}
				label_y = match &bands {
					Some(bands) => bands.label_center_y - row_height / 2.0,
					None => actor_y + 15.0,
				};
				label_height = row_height;
			}
			ActorKind::Control => {
				let cy = match &bands {
					Some(bands) => bands.glyph_bottom_y - radius,
					None => actor_y + 32.0,
				};
				figure.open("g").attr_if("stroke", &stroke).attr_if("fill", &fill).end();
				let marker_id = format!("{}-filled-head-control", self.context.id);
				if !self.has_control_marker {
					// The marker id repeats for every control in Mermaid; browsers use the first.
					self.has_control_marker = true;
					figure.open("defs").end();
					figure
						.open("marker")
						.attr("id", &marker_id)
						.attr("refX", "11")
						.attr("refY", "5.8")
						.attr("markerWidth", "20")
						.attr("markerHeight", "28")
						.attr("orient", "172.5")
						.end();
					figure
						.open("path")
						.attr("d", "M 14.4 5.6 L 7.2 10.4 L 8.8 5.6 L 7.2 0.8 Z")
						.attr_if("stroke", &stroke)
						.attr_if("fill", &fill)
						.attr("stroke-width", "1.2")
						.close();
					figure.close_tag("marker").close_tag("defs");
				}
				figure
					.open("circle")
					.number_attr("cx", center)
					.number_attr("cy", cy)
					.number_attr("r", radius)
					.attr_if("fill", &actor_bkg)
					.attr("stroke-width", "2")
					.attr_if("filter", &filter)
					.close();
				let transform = format!("translate({}, {})", number(center), number(cy - radius));
				line(figure, 0.0, 0.0, 0.0, 0.0)
					.attr("stroke-width", "2")
					.attr("marker-end", &format!("url(#{marker_id})"))
					.attr("transform", &transform)
					.close();
				if bands.is_none() {
					self.actors[index].height = 2.0 * radius + 2.0 * label_box_height;
				}
				label_y = match &bands {
					Some(bands) => bands.label_center_y - row_height / 2.0,
					None => actor_y + radius + if is_footer { 5.0 } else { 12.0 },
				};
				label_height = row_height;
			}
			ActorKind::Entity | ActorKind::Unknown => {
				let cy = match &bands {
					Some(bands) => bands.glyph_bottom_y - radius,
					None => actor_y + if is_footer { 10.0 } else { 25.0 },
				};
				figure
					.open("g")
					.attr_if("stroke", &stroke)
					.attr_if("fill", &fill)
					.attr("stroke-width", &stroke_width);
				if bands.is_none() {
					let dy = if is_footer { radius } else { radius / 2.0 - 5.0 };
					figure.attr("transform", &format!("translate(0, {})", number(dy)));
				}
				figure.attr_if("filter", &filter).end();
				figure
					.open("circle")
					.number_attr("cx", center)
					.number_attr("cy", cy)
					.number_attr("r", radius)
					.close();
				line(figure, center - radius, cy + radius, center + radius, cy + radius)
					.attr_if("fill", &actor_bkg)
					.attr("stroke-width", "2")
					.close();
				if bands.is_none() {
					self.actors[index].height = 2.0 * radius + label_box_height;
				}
				label_y = match &bands {
					Some(bands) => bands.label_center_y - row_height / 2.0,
					None => actor_y + if is_footer { 15.0 } else { 30.0 },
				};
				label_height = row_height;
			}
		}
		let label = (layout.x, label_y, layout.width, label_height);
		self.draw_centered_text(figure, &description, label, &font, &text_fill);
		if !matches!(kind, ActorKind::Participant | ActorKind::Queue) {
			figure.close_tag("g");
		}
		self.actors[index].height
	}

	/// `drawActivation`
	pub(super) fn draw_activation(
		&mut self,
		anchor: usize,
		(startx, stopx, starty): (f64, f64, f64),
		vertical_pos: f64,
		color_index: usize,
	) {
		let (stroke, fill) = match self.paint.palette(color_index) {
			Some((stroke, fill)) => (
				stroke.unwrap_or_else(|| self.paint.activation_border.clone()),
				fill.unwrap_or_else(|| self.paint.main_bkg.clone()),
			),
			None => (self.paint.activation_border.clone(), self.paint.activation_bkg.clone()),
		};
		let Some(svg) = self.body.get_mut(anchor) else { return };
		rect(svg, startx, starty, stopx - startx, vertical_pos - starty)
			.attr_if("fill", &fill)
			.attr_if("stroke", &stroke)
			.close();
	}

	/// `drawLoop`: the frame of a block with its label, title and section lines.
	pub(super) fn draw_loop(&mut self, model: &LoopModel, label: &str) {
		let conf_box_margin = self.conf.box_margin;
		let box_text_margin = self.conf.box_text_margin;
		let label_box_width = self.conf.label_box_width;
		let extent = model.extent;
		let startx = extent.startx.unwrap_or(0.0);
		let stopx = extent.stopx.unwrap_or(0.0);
		let starty = extent.starty.unwrap_or(0.0);
		let stopy = extent.stopy.unwrap_or(starty);
		let border = self.paint.label_box_border.clone();
		let mut svg = Svg::new();
		svg.open("g").end();
		let loop_line = |svg: &mut Svg, x1: f64, y1: f64, x2: f64, y2: f64, dash: &str| {
			line(svg, x1, y1, x2, y2)
				.attr_if("stroke", &border)
				.attr_if("fill", &border)
				.attr("stroke-width", "2")
				.attr("stroke-dasharray", dash)
				.close();
		};
		loop_line(&mut svg, startx, starty, stopx, starty, "2, 2");
		loop_line(&mut svg, stopx, starty, stopx, stopy, "2, 2");
		loop_line(&mut svg, startx, stopy, stopx, stopy, "2, 2");
		loop_line(&mut svg, startx, starty, startx, stopy, "2, 2");
		for (y, _) in &model.sections {
			loop_line(&mut svg, startx, *y, stopx, *y, "3, 3");
		}
		let width = label_box_width.max(50.0);
		let mut height = self.conf.label_box_height + if self.conf.is_neo { 15.0 } else { 0.0 };
		if height == 0.0 || height.is_nan() {
			height = 20.0;
		}
		let cut = 7.0;
		let points = [
			(startx, starty),
			(startx + width, starty),
			(startx + width, starty + height - cut),
			(startx + width - cut * 1.2, starty + height),
			(startx, starty + height),
		]
		.iter()
		.map(|(x, y)| format!("{},{}", number(*x), number(*y)))
		.collect::<Vec<_>>()
		.join(" ");
		svg.open("polygon")
			.attr("points", &points)
			.attr_if("fill", &self.paint.label_box_bkg)
			.attr_if("stroke", &border)
			.close();
		let font = self.conf.message_font.draw.clone();
		let label_fill = self.paint.label_text.clone();
		let loop_fill = self.paint.loop_text.clone();
		self.draw_text(
			&mut svg,
			TextData {
				text: label,
				x: startx,
				y: starty + height / 2.0,
				width: Some(width),
				anchor: Some(Align::Middle),
				is_centered: true,
				text_margin: box_text_margin,
				dy_em: false,
				font: &font,
				fill: &label_fill,
			},
		);
		if math::has_math(&model.title) {
			let label = self.math_label(&model.title);
			let x = js_round(startx + (stopx - startx).abs() / 2.0 - label.width / 2.0);
			self.write_math(
				&mut svg,
				&label,
				(x + label.width / 2.0, js_round(starty)),
				&loop_fill,
			);
		} else {
			self.draw_text(
				&mut svg,
				TextData {
					text: &model.title,
					x: startx + label_box_width / 2.0 + (stopx - startx) / 2.0,
					y: starty + conf_box_margin + box_text_margin,
					width: None,
					anchor: Some(Align::Middle),
					is_centered: true,
					text_margin: box_text_margin,
					dy_em: false,
					font: &font,
					fill: &loop_fill,
				},
			);
		}
		for (y, title) in &model.sections {
			if title.is_empty() {
				continue;
			}
			if math::has_math(title) {
				let label = self.math_label(title);
				let x = js_round(startx + (stopx - startx).abs() / 2.0 - label.width / 2.0);
				let top = js_round(y - label.height);
				self.write_math(&mut svg, &label, (x + label.width / 2.0, top), &loop_fill);
				continue;
			}
			self.draw_text(
				&mut svg,
				TextData {
					text: title,
					x: startx + (stopx - startx) / 2.0,
					y: y + conf_box_margin + box_text_margin,
					width: None,
					anchor: Some(Align::Middle),
					is_centered: true,
					text_margin: box_text_margin,
					dy_em: false,
					font: &font,
					fill: &loop_fill,
				},
			);
		}
		svg.close_tag("g");
		self.body.push(svg);
	}

	/// `drawBackgroundRect` of a `rect` block, behind everything.
	pub(super) fn draw_background(&mut self, model: &LoopModel) {
		let extent = model.extent;
		let (startx, starty) = (extent.startx.unwrap_or(0.0), extent.starty.unwrap_or(0.0));
		let (stopx, stopy) = (extent.stopx.unwrap_or(startx), extent.stopy.unwrap_or(starty));
		let mut svg = Svg::new();
		rect(&mut svg, startx, starty, stopx - startx, stopy - starty)
			.attr_if("fill", model.fill.as_deref().unwrap_or(""))
			.close();
		self.lowered.push(vec![Layer::Svg(svg)]);
	}

	/// `drawBox`: a box around participants with its title, behind everything.
	pub(super) fn draw_box(&mut self, box_index: usize, extent: (f64, f64, f64, f64)) {
		let (startx, starty, stopx, stopy) = extent;
		let layout = self.boxes[box_index].clone();
		let mut svg = Svg::new();
		svg.open("g").end();
		rect(&mut svg, startx, starty, stopx - startx, stopy - starty)
			.attr_if("fill", &self.db.boxes[box_index].fill)
			.attr_if("stroke", &self.paint.node_border)
			.close();
		if let Some(name) = &layout.name {
			let font = self.conf.actor_font.draw.clone();
			let fill = self.paint.text_color.clone();
			let y = layout.y + self.conf.box_text_margin + layout.text_max_height / 2.0;
			self.draw_centered_text(&mut svg, name, (layout.x, y, layout.width, 0.0), &font, &fill);
		}
		svg.close_tag("g");
		self.lowered.push(vec![Layer::Svg(svg)]);
	}

	/// `drawPopup`: the menu of a participant's links; its size, drawn only with `forceMenus`
	/// (Mermaid shows it on a click with a script).
	pub(super) fn draw_popup(&mut self, index: usize) -> (f64, f64) {
		let actor = &self.db.actors[index].1;
		if actor.links.is_empty()
			|| !matches!(actor.kind, ActorKind::Participant | ActorKind::Collections)
		{
			return (0.0, 0.0);
		}
		let links = actor.links.clone();
		let layout = self.actors[index].clone();
		let rect_height = match (actor.kind, self.conf.is_neo) {
			(ActorKind::Collections, true) => layout.height - 6.0,
			_ => layout.height,
		};
		let font = self.conf.actor_font.clone();
		let padding = 2.0 * self.conf.wrap_padding + 2.0 * self.conf.box_margin;
		let mut min_width: f64 = 0.0;
		for (label, _) in &links {
			min_width = min_width.max(self.text_width(label, &font) + padding);
		}
		let menu_width = if layout.width > min_width { layout.width } else { min_width };
		let link_height = 20.0 + 30.0 * links.len() as f64;
		if self.conf.force_menus {
			let mut svg = Svg::new();
			svg.open("g").end();
			rect(&mut svg, layout.x, rect_height, menu_width, link_height)
				.attr_if("fill", &self.paint.actor_bkg)
				.attr("stroke", "#666")
				.close();
			let fill = self.paint.actor_text.clone();
			let mut link_y = 20.0;
			for (label, url) in &links {
				svg.open("a").attr("xlink:href", url).attr("target", "_blank").end();
				let position = (layout.x + 10.0, rect_height + link_y + 10.0);
				self.text_line(
					&mut svg,
					position,
					(Anchor::Start, Baseline::Central),
					&font.draw,
					label,
					&fill,
				);
				svg.close_tag("a");
				link_y += 30.0;
			}
			svg.close_tag("g");
			self.body.push(svg);
		}
		(rect_height + link_height, menu_width)
	}

	/// The markers and the drop shadow of the diagram.
	pub(super) fn write_defs(&self, svg: &mut Svg) {
		let id = &self.context.id;
		let signal = &self.paint.signal;
		let text_color = &self.paint.text_color;
		let marker = |svg: &mut Svg, name: &str, attributes: &[(&str, &str)]| {
			svg.open("defs").end();
			svg.open("marker").attr("id", &format!("{id}-{name}"));
			for (key, value) in attributes {
				svg.attr(key, value);
			}
			svg.end();
		};
		let end = |svg: &mut Svg| _ = svg.close_tag("marker").close_tag("defs");
		// Mermaid's `orient="auto-start-reverse"`, which EdgeHTML ignores: a marker for line ends
		// and a turned one for line starts.
		let reversible =
			|svg: &mut Svg, name: &str, reference: (&str, &str), shape: &dyn Fn(&mut Svg)| {
				for (suffix, turn) in [("", None), ("-start", Some(reference))] {
					let mut attributes = vec![
						("refX", reference.0),
						("refY", reference.1),
						("markerUnits", "userSpaceOnUse"),
						("markerWidth", "12"),
						("markerHeight", "12"),
						("orient", "auto"),
					];
					if turn.is_some() {
						// The turned shape leaves the marker's box.
						attributes.push(("overflow", "visible"));
					}
					marker(svg, &format!("{name}{suffix}"), &attributes);
					if let Some((x, y)) = turn {
						svg.open("g").attr("transform", &format!("rotate(180 {x} {y})")).end();
					}
					shape(svg);
					if turn.is_some() {
						svg.close_tag("g");
					}
					end(svg);
				}
			};
		reversible(svg, "arrowhead", ("7.9", "5"), &|svg| {
			svg.open("path")
				.attr("d", "M -1 0 L 10 5 L 0 10 z")
				.attr_if("fill", signal)
				.attr_if("stroke", signal)
				.close();
		});
		marker(
			svg,
			"crosshead",
			&[
				("markerWidth", "15"),
				("markerHeight", "8"),
				("orient", "auto"),
				("refX", "4"),
				("refY", "4.5"),
			],
		);
		svg.open("path")
			.attr_if("fill", signal)
			.attr_if("stroke", signal)
			.attr("stroke-width", &number(4.0 / 3.0))
			.attr("d", "M 1,2 L 6,7 M 6,2 L 1,7")
			.close();
		end(svg);
		marker(
			svg,
			"filled-head",
			&[
				("refX", "15.5"),
				("refY", "7"),
				("markerWidth", "20"),
				("markerHeight", "28"),
				("orient", "auto"),
			],
		);
		svg.open("path").attr("d", "M 18,7 L9,13 L14,7 L9,1 Z").attr_if("fill", text_color).close();
		end(svg);
		marker(
			svg,
			"sequencenumber",
			&[
				("refX", "15"),
				("refY", "15"),
				("markerWidth", "60"),
				("markerHeight", "40"),
				("orient", "auto"),
			],
		);
		svg.open("circle")
			.attr("cx", "15")
			.attr("cy", "15")
			.attr("r", "6")
			.attr_if("fill", signal)
			.close();
		end(svg);
		reversible(svg, "solidTopArrowHead", ("7.9", "7.25"), &|svg| {
			svg.open("path").attr("d", "M 0 0 L 10 8 L 0 8 z").attr_if("fill", text_color).close();
		});
		reversible(svg, "solidBottomArrowHead", ("7.9", "0.75"), &|svg| {
			svg.open("path").attr("d", "M 0 0 L 10 0 L 0 8 z").attr_if("fill", text_color).close();
		});
		for (name, reference_y, d) in [
			("stickTopArrowHead", "7", "M 0 0 L 7 7"),
			("stickBottomArrowHead", "0", "M 0 7 L 7 0"),
		] {
			reversible(svg, name, ("7.5", reference_y), &|svg| {
				svg.open("path")
					.attr("d", d)
					.attr("stroke", "black")
					.attr("stroke-width", "1.5")
					.attr("fill", "none")
					.close();
			});
		}
		if self.conf.is_neo {
			// `feDropShadow` written with the primitives it is defined as, for EdgeHTML.
			svg.open("defs").end();
			svg.open("filter")
				.attr("id", &format!("{id}-drop-shadow"))
				.attr("height", "130%")
				.attr("width", "130%")
				.end();
			svg.open("feOffset")
				.attr("in", "SourceAlpha")
				.attr("dx", "4")
				.attr("dy", "4")
				.attr("result", "offset")
				.close();
			svg.open("feFlood")
				.attr("flood-opacity", "0.06")
				.attr("flood-color", self.paint.shadow_color)
				.close();
			svg.open("feComposite").attr("in2", "offset").attr("operator", "in").close();
			svg.open("feMerge").end();
			svg.open("feMergeNode").close();
			svg.open("feMergeNode").attr("in", "SourceGraphic").close();
			svg.close_tag("feMerge").close_tag("filter").close_tag("defs");
		}
	}
}
