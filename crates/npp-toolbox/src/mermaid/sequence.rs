//! Sequence diagrams (`sequenceDiagram.jison`, `sequenceDb.ts`, `sequenceRenderer.ts`,
//! `svgDraw.js`, `styles.js`): participants side by side, messages from top to bottom.
//!
//! The layout follows Mermaid step by step, including its text measurement by SVG `getBBox`:
//! widths of whole lines rounded to pixels, heights from the rounded ascent and descent.

mod db;
mod draw;
mod parser;

use std::collections::HashMap;

use super::color::js_round;
use super::common::{self, CommonFields, split_breaks};
use super::label::{self, encode_entities};
use super::math;
use super::svg::{self, Frame, Svg, number};
use super::text::{Font, parse_font_size};
use super::value::Value;
use super::{Context, Diagram, Look, Result};
use db::{ActorKind, CentralConnection, LineType, Placement, SequenceDb};

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let code = encode_entities(code) + "\n";
	let mut common = CommonFields::new(context);
	let ops = parser::parse(&code, &mut common)
		.map_err(|error| context.error_at(error.line, error.message))?;
	let auto_wrap = context
		.config
		.get("wrap")
		.and_then(Value::as_bool)
		.unwrap_or_else(|| context.config_bool("sequence", "wrap", false));
	let mut db = SequenceDb::new(auto_wrap);
	for op in ops {
		db.apply(op).map_err(super::Error::new)?;
	}
	let line_messages = db.messages.iter().filter(|message| message.line_type.is_line()).count();
	if line_messages > context.limits.max_edges {
		return Err(super::Error::new(format!(
			"Edge limit exceeded. {line_messages} edges found, but the limit is {}.",
			context.limits.max_edges
		)));
	}
	let conf = Conf::new(context);
	let paint = Paint::new(context);
	let renderer = Renderer::new(context, db, conf, paint);
	renderer.draw(&common)
}

/// `textAlign`-like placements of `drawText`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Align {
	Start,
	Middle,
	End,
}

impl Align {
	fn parse(text: &str) -> Option<Align> {
		match text {
			"left" | "start" => Some(Align::Start),
			"middle" | "center" => Some(Align::Middle),
			"right" | "end" => Some(Align::End),
			_ => None,
		}
	}
}

/// A font of the diagram: drawn in `draw`, measured in `measure`.
#[derive(Clone, Debug)]
struct TextFont {
	draw: Font,
	measure: Font,
}

/// The `sequence` configuration with the top-level font settings, like `setConf`.
struct Conf {
	activation_width: f64,
	diagram_margin_x: f64,
	diagram_margin_y: f64,
	actor_margin: f64,
	width: f64,
	height: f64,
	box_margin: f64,
	box_text_margin: f64,
	note_margin: f64,
	message_align: Option<Align>,
	note_align: Option<Align>,
	mirror_actors: bool,
	force_menus: bool,
	bottom_margin_adj: f64,
	right_angles: bool,
	show_sequence_numbers: bool,
	wrap_padding: f64,
	label_box_width: f64,
	label_box_height: f64,
	hide_unused_participants: bool,
	use_max_width: bool,
	is_neo: bool,
	actor_font: TextFont,
	note_font: TextFont,
	message_font: TextFont,
}

impl Conf {
	fn new(context: &Context) -> Conf {
		let number = |key: &str, default: f64| context.config_number("sequence", key, default);
		let flag = |key: &str, default: bool| context.config_bool("sequence", key, default);
		let theme_family = context.font_family();
		let font = |prefix: &str| -> TextFont {
			let top = |key: &str| context.config.get(key).filter(|value| value.to_text().is_some());
			let section =
				|key: &str| context.config_value("sequence", &format!("{prefix}{key}")).cloned();
			// `setConf` replaces the families of the section by the top-level one, which Mermaid
			// sets even where the document does not.
			let family =
				top("fontFamily").and_then(Value::to_text).filter(|family| !family.is_empty());
			// The top-level `fontSize` (16 by default) replaces the sizes of the section.
			let size = top("fontSize")
				.and_then(Value::to_text)
				.and_then(|size| parse_font_size(&size, 16.0))
				.filter(|size| *size > 0.0)
				.unwrap_or(16.0);
			let weight = top("fontWeight")
				.cloned()
				.or_else(|| section("FontWeight"))
				.and_then(|weight| weight.to_text())
				.map_or(400, |weight| match weight.as_str() {
					"bold" | "bolder" => 700,
					other => other.parse().unwrap_or(400),
				});
			// Without a configured family Mermaid's text gets an invalid `font-family`: it is drawn
			// in the theme's font and measured in the page's default font.
			let mut draw = Font::new(family.clone().unwrap_or_else(|| theme_family.clone()), size);
			draw.weight = weight;
			let mut measure =
				Font::new(family.unwrap_or_else(|| "\"Times New Roman\"".to_owned()), size);
			measure.weight = weight;
			TextFont { draw, measure }
		};
		let align = |key: &str| {
			Align::parse(&context.config_text("sequence", key).unwrap_or_else(|| "center".into()))
		};
		Conf {
			activation_width: number("activationWidth", 10.0),
			diagram_margin_x: number("diagramMarginX", 50.0),
			diagram_margin_y: number("diagramMarginY", 10.0),
			actor_margin: number("actorMargin", 50.0),
			width: number("width", 150.0),
			height: number("height", 65.0),
			box_margin: number("boxMargin", 10.0),
			box_text_margin: number("boxTextMargin", 5.0),
			note_margin: number("noteMargin", 10.0),
			message_align: align("messageAlign"),
			note_align: align("noteAlign"),
			mirror_actors: flag("mirrorActors", true),
			force_menus: flag("forceMenus", false),
			bottom_margin_adj: number("bottomMarginAdj", 1.0),
			right_angles: flag("rightAngles", false),
			show_sequence_numbers: flag("showSequenceNumbers", false),
			wrap_padding: number("wrapPadding", 10.0),
			label_box_width: number("labelBoxWidth", 50.0),
			label_box_height: number("labelBoxHeight", 20.0),
			hide_unused_participants: flag("hideUnusedParticipants", false),
			use_max_width: flag("useMaxWidth", true),
			is_neo: context.look == Look::Neo,
			actor_font: font("actor"),
			note_font: font("note"),
			message_font: font("message"),
		}
	}
}

/// The colors of the diagram's style sheet rules, written as presentation attributes.
struct Paint {
	actor_border: String,
	actor_bkg: String,
	actor_text: String,
	actor_line: String,
	signal: String,
	signal_text: String,
	label_box_bkg: String,
	label_box_border: String,
	label_text: String,
	loop_text: String,
	note_border: String,
	note_bkg: String,
	note_text: String,
	activation_bkg: String,
	activation_border: String,
	sequence_number: String,
	node_border: String,
	text_color: String,
	main_bkg: String,
	/// The default fill of `rect` blocks.
	rect_bkg: String,
	/// `strokeWidth` of the `.actor` rule.
	stroke_width: String,
	/// The palettes of the `redux-color` themes.
	border_colors: Option<Vec<String>>,
	bkg_colors: Option<Vec<String>>,
	/// Flood color of the neo drop shadow.
	shadow_color: &'static str,
}

impl Paint {
	fn new(context: &Context) -> Paint {
		let theme = &context.theme;
		let css = |key: &str| theme.css(key);
		let array = |key: &str| -> Vec<String> {
			match theme.get(key) {
				Some(Value::Array(items)) => items.iter().filter_map(Value::to_text).collect(),
				_ => Vec::new(),
			}
		};
		let is_color_theme =
			matches!(context.theme_name.as_str(), "redux-color" | "redux-dark-color");
		let rect_bkg = [css("rectBkgColor"), css("actorBkg")]
			.into_iter()
			.find(|color| !color.is_empty())
			.unwrap_or_else(|| "rgba(128, 128, 128, 0.5)".to_owned());
		Paint {
			actor_border: css("actorBorder"),
			actor_bkg: css("actorBkg"),
			actor_text: css("actorTextColor"),
			actor_line: css("actorLineColor"),
			signal: css("signalColor"),
			signal_text: css("signalTextColor"),
			label_box_bkg: css("labelBoxBkgColor"),
			label_box_border: css("labelBoxBorderColor"),
			label_text: css("labelTextColor"),
			loop_text: css("loopTextColor"),
			note_border: css("noteBorderColor"),
			note_bkg: css("noteBkgColor"),
			note_text: css("noteTextColor"),
			activation_bkg: css("activationBkgColor"),
			activation_border: css("activationBorderColor"),
			sequence_number: css("sequenceNumberColor"),
			node_border: css("nodeBorder"),
			text_color: css("textColor"),
			main_bkg: css("mainBkg"),
			rect_bkg,
			stroke_width: theme.text("strokeWidth").unwrap_or_else(|| "1".to_owned()),
			border_colors: is_color_theme.then(|| array("borderColorArray")),
			bkg_colors: is_color_theme.then(|| array("bkgColorArray")),
			shadow_color: match context.theme_name.as_str() {
				"redux" | "redux-color" => "#000000",
				_ => "#FFFFFF",
			},
		}
	}

	/// `paletteColor`: the stroke and fill of the `index`th participant in the color themes;
	/// `None` leaves them to the style sheet.
	fn palette(&self, index: usize) -> Option<(Option<String>, Option<String>)> {
		let pick = |palette: &Vec<String>| match palette.is_empty() {
			true => None,
			false => Some(palette[index % palette.len()].clone()),
		};
		Some((pick(self.border_colors.as_ref()?), pick(self.bkg_colors.as_ref()?)))
	}
}

/// The layout of a participant, next to its database entry.
#[derive(Clone, Debug, Default)]
struct ActorLayout {
	x: f64,
	width: f64,
	height: f64,
	margin: Option<f64>,
	starty: f64,
	stopy: Option<f64>,
	/// The measured label height under the neo look.
	text_height: Option<f64>,
	/// The label, wrapped.
	description: String,
}

#[derive(Clone, Debug, Default)]
struct BoxLayout {
	x: f64,
	y: f64,
	width: f64,
	margin: f64,
	text_max_height: f64,
	name: Option<String>,
}

/// What the layout keeps of every message (`msgModel`, `noteModel`).
#[derive(Clone, Debug, Default)]
struct MessageModel {
	width: f64,
	height: f64,
	startx: f64,
	stopx: f64,
	starty: f64,
	stopy: f64,
	text: String,
	from_bounds: f64,
	to_bounds: f64,
	sequence_index: f64,
	sequence_visible: bool,
}

/// The area of a block, a note or the diagram; unset like JavaScript's `undefined`.
#[derive(Clone, Copy, Debug, Default)]
struct Extent {
	startx: Option<f64>,
	starty: Option<f64>,
	stopx: Option<f64>,
	stopy: Option<f64>,
}

/// `updateVal`: the value, or `combine` of it and the present one.
fn update(slot: &mut Option<f64>, value: f64, combine: fn(f64, f64) -> f64) {
	*slot = Some(match *slot {
		None => value,
		Some(present) => combine(value, present),
	});
}

/// An open block on the stack of `sequenceItems`.
#[derive(Clone, Debug, Default)]
struct LoopModel {
	extent: Extent,
	title: String,
	fill: Option<String>,
	/// The `y` of the sections (`else`, `and`, `option`) and their titles.
	sections: Vec<(f64, String)>,
}

#[derive(Clone, Debug)]
struct Activation {
	startx: f64,
	stopx: f64,
	starty: Option<f64>,
	stopy: Option<f64>,
	actor: String,
	/// The body element the activation is drawn into.
	anchor: usize,
}

/// The width of the blocks, computed before the layout (`calculateLoopBounds`).
#[derive(Clone, Copy, Debug)]
struct LoopWidth {
	/// The message that starts the block.
	id: usize,
	from: f64,
	to: f64,
	width: f64,
}

/// Parts of the SVG whose content is known only at the end.
enum Layer {
	Svg(Svg),
	/// The lifeline of a participant, which ends where its footer is.
	Lifeline {
		x: f64,
		y1: f64,
		actor: usize,
	},
}

struct Renderer<'c, 'm> {
	context: &'c mut Context<'m>,
	db: SequenceDb,
	conf: Conf,
	paint: Paint,
	actors: Vec<ActorLayout>,
	boxes: Vec<BoxLayout>,
	/// The boxes in drawing order (`bounds.models.boxes`).
	box_models: Vec<usize>,
	messages: Vec<Option<MessageModel>>,
	notes: Vec<Option<MessageModel>>,
	data: Extent,
	vertical_pos: f64,
	sequence_items: Vec<LoopModel>,
	activations: Vec<Activation>,
	/// Elements moved to the front of the SVG (`lower()`), most recent first at the end.
	lowered: Vec<Vec<Layer>>,
	/// Elements in document order.
	body: Vec<Svg>,
	has_control_marker: bool,
}

/// `common.getMax`: the largest value that is a number.
fn get_max(values: &[f64]) -> f64 {
	values.iter().copied().filter(|value| !value.is_nan()).fold(f64::NEG_INFINITY, f64::max)
}

fn get_min(values: &[f64]) -> f64 {
	values.iter().copied().filter(|value| !value.is_nan()).fold(f64::INFINITY, f64::min)
}

impl<'c, 'm> Renderer<'c, 'm> {
	fn new(context: &'c mut Context<'m>, db: SequenceDb, conf: Conf, paint: Paint) -> Self {
		let actors = db
			.actors
			.iter()
			.map(|(_, actor)| ActorLayout {
				description: actor.description.clone(),
				..ActorLayout::default()
			})
			.collect();
		let boxes = db
			.boxes
			.iter()
			.map(|sequence_box| BoxLayout {
				name: sequence_box.name.clone(),
				..BoxLayout::default()
			})
			.collect();
		let messages = vec![None; db.messages.len()];
		let notes = vec![None; db.messages.len()];
		Renderer {
			context,
			db,
			conf,
			paint,
			actors,
			boxes,
			box_models: Vec::new(),
			messages,
			notes,
			data: Extent::default(),
			vertical_pos: 0.0,
			sequence_items: Vec::new(),
			activations: Vec::new(),
			lowered: Vec::new(),
			body: Vec::new(),
			has_control_marker: false,
		}
	}

	// Text measurement.

	/// Height of the box `getBBox` gives a line of text: the rounded ascent and descent, and
	/// glyphs reaching beyond them.
	fn line_height(&mut self, line: &str, font: &Font) -> f64 {
		self.context.bounding_box(line, font).1
	}

	/// `calculateTextDimensions` in the measuring font.
	/// `calculateMathMLDimensions`: the size in a hidden element of the page, whose default font
	/// is Times New Roman in 16 pixels, as whole pixels.
	fn math_dimensions(&mut self, text: &str) -> (f64, f64) {
		let font = Font::new("Times New Roman", 16.0);
		let label = label::layout_math_lines(self.context, text, &font, false);
		(js_round(label.width), js_round(label.height))
	}

	/// The size of a text in the sequence diagram's own places with math.
	fn math_or_text_dimensions(&mut self, text: &str, font: &TextFont) -> (f64, f64) {
		match math::has_math(text) {
			true => self.math_dimensions(text),
			false => self.text_dimensions(text, font),
		}
	}

	/// The formula text as `drawKatex` draws it, in the font of the diagram's root.
	fn math_label(&mut self, text: &str) -> label::LabelBox {
		let font = Font::new(self.context.font_family(), self.context.font_size());
		label::layout_math_lines(self.context, text, &font, true)
	}

	/// The formula text of a participant as `byKatex` draws it, in an element of a set size.
	fn actor_math_label(&mut self, text: &str) -> label::LabelBox {
		let font = Font::new(self.context.font_family(), self.context.font_size());
		label::layout_math_lines(self.context, text, &font, false)
	}

	fn text_dimensions(&mut self, text: &str, font: &TextFont) -> (f64, f64) {
		self.context.text_dimensions(text, &font.measure)
	}

	fn text_width(&mut self, text: &str, font: &TextFont) -> f64 {
		self.text_dimensions(text, font).0
	}

	/// `wrapLabel` in the measuring font.
	fn wrap_label(&mut self, label: &str, max_width: f64, font: &TextFont) -> String {
		let context = &mut *self.context;
		common::wrap_label(label, max_width, &mut |text| {
			context.text_dimensions(text, &font.measure).0
		})
	}

	// Bounds.

	fn insert(&mut self, startx: f64, starty: f64, stopx: f64, stopy: f64) {
		let (startx, stopx) = (get_min(&[startx, stopx]), get_max(&[startx, stopx]));
		let (starty, stopy) = (get_min(&[starty, stopy]), get_max(&[starty, stopy]));
		update(&mut self.data.startx, startx, f64::min);
		update(&mut self.data.starty, starty, f64::min);
		update(&mut self.data.stopx, stopx, f64::max);
		update(&mut self.data.stopy, stopy, f64::max);
		self.update_bounds(startx, starty, stopx, stopy);
	}

	/// `updateBounds`: grows the open blocks, outer ones by more margin, and the activations.
	fn update_bounds(&mut self, startx: f64, starty: f64, stopx: f64, stopy: f64) {
		let margin = self.conf.box_margin;
		let count = self.sequence_items.len() as f64;
		let mut counter = 0.0;
		for item in &mut self.sequence_items {
			counter += 1.0;
			let n = count - counter + 1.0;
			update(&mut item.extent.starty, starty - n * margin, f64::min);
			update(&mut item.extent.stopy, stopy + n * margin, f64::max);
			update(&mut self.data.startx, startx - n * margin, f64::min);
			update(&mut self.data.stopx, stopx + n * margin, f64::max);
			update(&mut item.extent.startx, startx - n * margin, f64::min);
			update(&mut item.extent.stopx, stopx + n * margin, f64::max);
			update(&mut self.data.starty, starty - n * margin, f64::min);
			update(&mut self.data.stopy, stopy + n * margin, f64::max);
		}
		for activation in &mut self.activations {
			counter += 1.0;
			let n = count - counter + 1.0;
			update(&mut activation.starty, starty - n * margin, f64::min);
			update(&mut activation.stopy, stopy + n * margin, f64::max);
			update(&mut self.data.startx, startx - n * margin, f64::min);
			update(&mut self.data.stopx, stopx + n * margin, f64::max);
		}
	}

	fn bump_vertical_pos(&mut self, bump: f64) {
		self.vertical_pos += bump;
		self.data.stopy = Some(get_max(&[self.data.stopy.unwrap_or(f64::NAN), self.vertical_pos]));
	}

	fn actor_index(&self, id: &str) -> Option<usize> {
		self.db.index_of(id)
	}

	fn actor_activations(&self, actor: &str) -> usize {
		self.activations.iter().filter(|activation| activation.actor == actor).count()
	}

	/// `activationBounds`: the left and right edge of the lifeline with its activations.
	fn activation_bounds(&self, actor: &str) -> (f64, f64) {
		let Some(index) = self.actor_index(actor) else { return (f64::NAN, f64::NAN) };
		let layout = &self.actors[index];
		let center = layout.x + layout.width / 2.0;
		let activations = self.activations.iter().filter(|activation| activation.actor == actor);
		let left = activations
			.clone()
			.fold(center - 1.0, |left, activation| get_min(&[left, activation.startx]));
		let right =
			activations.fold(center + 1.0, |right, activation| get_max(&[right, activation.stopx]));
		(left, right)
	}

	fn new_activation(&mut self, actor: &str) {
		let Some(index) = self.actor_index(actor) else { return };
		let layout = &self.actors[index];
		let stacked = self.actor_activations(actor) as f64;
		let x = layout.x + layout.width / 2.0 + (stacked - 1.0) * self.conf.activation_width / 2.0;
		self.body.push(Svg::new());
		self.activations.push(Activation {
			startx: x,
			stopx: x + self.conf.activation_width,
			starty: Some(self.vertical_pos + 2.0),
			stopy: None,
			actor: actor.to_owned(),
			anchor: self.body.len() - 1,
		});
	}

	fn end_activation(&mut self, actor: &str) -> Option<Activation> {
		let index = self.activations.iter().rposition(|activation| activation.actor == actor);
		// `splice(-1, 1)` removes the last activation when the participant has none.
		let index = index.or_else(|| self.activations.len().checked_sub(1))?;
		Some(self.activations.remove(index))
	}

	// Layout.

	fn actor_keys(&self) -> Vec<String> {
		let keys = self.db.actor_keys();
		if !self.conf.hide_unused_participants {
			return keys;
		}
		let used: std::collections::HashSet<&str> = self
			.db
			.messages
			.iter()
			.flat_map(|message| [message.from.as_deref(), message.to.as_deref()])
			.flatten()
			.collect();
		keys.into_iter().filter(|key| used.contains(key.as_str())).collect()
	}

	/// `getMaxMessageWidthPerActor`
	fn max_message_width_per_actor(&mut self) -> HashMap<String, f64> {
		let mut widths: HashMap<String, f64> = HashMap::new();
		let grow = |widths: &mut HashMap<String, f64>, key: &str, width: f64| {
			let entry = widths.entry(key.to_owned()).or_insert(0.0);
			*entry = get_max(&[*entry, width]);
		};
		for index in 0..self.db.messages.len() {
			let message = &self.db.messages[index];
			let (Some(from), Some(to)) = (message.from.clone(), message.to.clone()) else {
				continue;
			};
			let (Some(actor), Some(_)) = (self.db.actor(&to), self.db.actor(&from)) else {
				continue;
			};
			let (prev_actor, next_actor) = (actor.prev_actor.clone(), actor.next_actor.clone());
			let placement = message.placement;
			if placement == Some(Placement::LeftOf) && prev_actor.is_none() {
				continue;
			}
			if placement == Some(Placement::RightOf) && next_actor.is_none() {
				continue;
			}
			let is_note = placement.is_some();
			let font = match is_note {
				true => self.conf.note_font.clone(),
				false => self.conf.message_font.clone(),
			};
			let text = message.text.clone();
			let original_text = text.clone();
			let wrapped = match message.wrap {
				true => {
					let width = self.conf.width - 2.0 * self.conf.wrap_padding;
					self.wrap_label(&text, width, &font)
				}
				false => text,
			};
			let message_width = match math::has_math(&wrapped) {
				true => self.math_dimensions(&original_text).0,
				false => self.text_width(&wrapped, &font),
			} + 2.0 * self.conf.wrap_padding;
			if !is_note && next_actor.as_deref() == Some(from.as_str()) {
				grow(&mut widths, &to, message_width);
			} else if !is_note && prev_actor.as_deref() == Some(from.as_str()) {
				grow(&mut widths, &from, message_width);
			} else if !is_note && from == to {
				grow(&mut widths, &from, message_width / 2.0);
				grow(&mut widths, &to, message_width / 2.0);
			} else if placement == Some(Placement::RightOf) {
				grow(&mut widths, &from, message_width);
			} else if placement == Some(Placement::LeftOf) {
				if let Some(previous) = &prev_actor {
					grow(&mut widths, previous, message_width);
				}
			} else if placement == Some(Placement::Over) {
				if let Some(previous) = &prev_actor {
					grow(&mut widths, previous, message_width / 2.0);
				}
				if next_actor.is_some() {
					grow(&mut widths, &from, message_width / 2.0);
				}
			}
		}
		widths
	}

	/// `calculateActorMargins`: sizes of the participants and boxes; returns the row height.
	fn calculate_actor_margins(&mut self, widths: &HashMap<String, f64>) -> f64 {
		let conf_width = self.conf.width;
		let wrap_padding = self.conf.wrap_padding;
		let font = self.conf.actor_font.clone();
		let mut max_height: f64 = 0.0;
		for index in 0..self.actors.len() {
			let wrap = self.db.actors[index].1.wrap;
			if wrap {
				let description = self.actors[index].description.clone();
				self.actors[index].description =
					self.wrap_label(&description, conf_width - 2.0 * wrap_padding, &font);
			}
			let description = self.actors[index].description.clone();
			let (text_width, text_height) = self.math_or_text_dimensions(&description, &font);
			let layout = &mut self.actors[index];
			layout.width = match wrap {
				true => conf_width,
				false => get_max(&[conf_width, text_width + 2.0 * wrap_padding]),
			};
			layout.height = if self.conf.is_neo {
				layout.text_height = Some(text_height);
				get_max(&[draw::actor_stack_height(text_height), self.conf.height])
			} else if wrap {
				get_max(&[text_height, self.conf.height])
			} else {
				self.conf.height
			};
			max_height = get_max(&[max_height, layout.height]);
		}
		for (key, message_width) in widths {
			let Some(index) = self.actor_index(key) else { continue };
			let next_width = self.db.actors[index]
				.1
				.next_actor
				.as_deref()
				.and_then(|next| self.actor_index(next))
				.map(|next| self.actors[next].width);
			let layout = &self.actors[index];
			let actor_width = match next_width {
				None => message_width + self.conf.actor_margin - layout.width / 2.0,
				Some(next_width) => {
					message_width + self.conf.actor_margin - layout.width / 2.0 - next_width / 2.0
				}
			};
			self.actors[index].margin = Some(get_max(&[actor_width, self.conf.actor_margin]));
		}
		let mut max_box_height: f64 = 0.0;
		let message_font = self.conf.message_font.clone();
		for box_index in 0..self.boxes.len() {
			let mut total_width: f64 = self.db.boxes[box_index]
				.actor_keys
				.iter()
				.filter_map(|key| self.actor_index(key))
				.map(|index| self.actors[index].width + self.actors[index].margin.unwrap_or(0.0))
				.sum();
			total_width += self.conf.box_margin * 8.0;
			total_width -= 2.0 * self.conf.box_text_margin;
			if self.db.boxes[box_index].wrap
				&& let Some(name) = self.boxes[box_index].name.clone()
			{
				self.boxes[box_index].name =
					Some(self.wrap_label(&name, total_width - 2.0 * wrap_padding, &message_font));
			}
			let name = self.boxes[box_index].name.clone().unwrap_or_default();
			let (name_width, name_height) = self.text_dimensions(&name, &message_font);
			max_box_height = get_max(&[name_height, max_box_height]);
			let min_width = get_max(&[total_width, name_width + 2.0 * wrap_padding]);
			let layout = &mut self.boxes[box_index];
			layout.margin = self.conf.box_text_margin;
			if total_width < min_width {
				layout.margin += (min_width - total_width) / 2.0;
			}
		}
		for layout in &mut self.boxes {
			layout.text_max_height = max_box_height;
		}
		get_max(&[max_height, self.conf.height])
	}

	/// `addActorRenderingData`: places the participants and boxes from left to right.
	fn add_actor_rendering_data(&mut self, actor_keys: &[String]) {
		let mut prev_width = 0.0;
		let mut prev_margin = 0.0;
		let mut prev_box: Option<usize> = None;
		let mut max_height: f64 = 0.0;
		for key in actor_keys {
			let Some(index) = self.actor_index(key) else { continue };
			let actor_box = self.db.actors[index].1.box_index;
			if let Some(previous) = prev_box
				&& prev_box != actor_box
			{
				self.box_models.push(previous);
				prev_margin += self.conf.box_margin + self.boxes[previous].margin;
			}
			if let Some(current) = actor_box
				&& actor_box != prev_box
			{
				self.boxes[current].x = prev_width + prev_margin;
				self.boxes[current].y = 0.0;
				prev_margin += self.boxes[current].margin;
			}
			let conf_width = self.conf.width;
			let conf_height = self.conf.height;
			let layout = &mut self.actors[index];
			layout.width = get_max(&[nonzero(layout.width).unwrap_or(conf_width), conf_width]);
			layout.height = get_max(&[nonzero(layout.height).unwrap_or(conf_height), conf_height]);
			let margin = nonzero(layout.margin.unwrap_or(0.0)).unwrap_or(self.conf.actor_margin);
			layout.margin = Some(margin);
			max_height = get_max(&[max_height, layout.height]);
			if self.db.created_actors.get(key).is_some_and(|&message| message != 0) {
				prev_margin += layout.width / 2.0;
			}
			layout.x = prev_width + prev_margin;
			layout.starty = self.vertical_pos;
			let (x, width, height) = (layout.x, layout.width, layout.height);
			self.insert(x, 0.0, x + width, height);
			prev_width += width + prev_margin;
			if let Some(current) = actor_box {
				let layout = &mut self.boxes[current];
				layout.width = prev_width + layout.margin - layout.x;
			}
			prev_margin = margin;
			prev_box = actor_box;
		}
		if let Some(previous) = prev_box {
			self.box_models.push(previous);
		}
		self.bump_vertical_pos(max_height);
	}

	/// `buildNoteModel`
	fn build_note_model(&mut self, message_index: usize) -> Option<MessageModel> {
		let message = self.db.messages[message_index].clone();
		let from = self.actors[self.actor_index(message.from.as_deref()?)?].clone();
		let to = self.actors[self.actor_index(message.to.as_deref()?)?].clone();
		let (startx, stopx) = (from.x, to.x);
		let should_wrap = message.wrap && !message.text.is_empty();
		let font = self.conf.note_font.clone();
		let (conf_width, note_margin, actor_margin) =
			(self.conf.width, self.conf.note_margin, self.conf.actor_margin);
		let measured = match should_wrap {
			true => self.wrap_label(&message.text, conf_width, &font),
			false => message.text.clone(),
		};
		let mut text_width = match math::has_math(&message.text) {
			true => self.math_dimensions(&message.text).0,
			false => self.text_width(&measured, &font),
		};
		let mut model = MessageModel {
			width: match should_wrap {
				true => conf_width,
				false => get_max(&[conf_width, text_width + 2.0 * note_margin]),
			},
			startx: from.x,
			text: message.text.clone(),
			..MessageModel::default()
		};
		match message.placement {
			Some(Placement::RightOf) => {
				model.width = match should_wrap {
					true => get_max(&[conf_width, text_width]),
					false => get_max(&[
						from.width / 2.0 + to.width / 2.0,
						text_width + 2.0 * note_margin,
					]),
				};
				model.startx = startx + (from.width + actor_margin) / 2.0;
			}
			Some(Placement::LeftOf) => {
				model.width = match should_wrap {
					true => get_max(&[conf_width, text_width + 2.0 * note_margin]),
					false => get_max(&[
						from.width / 2.0 + to.width / 2.0,
						text_width + 2.0 * note_margin,
					]),
				};
				model.startx = startx - model.width + (from.width - actor_margin) / 2.0;
			}
			_ if message.to == message.from => {
				let measured = match should_wrap {
					true => {
						let width = get_max(&[conf_width, from.width]);
						self.wrap_label(&message.text, width, &font)
					}
					false => message.text.clone(),
				};
				text_width = self.text_width(&measured, &font);
				model.width = match should_wrap {
					true => get_max(&[conf_width, from.width]),
					false => get_max(&[from.width, conf_width, text_width + 2.0 * note_margin]),
				};
				model.startx = startx + (from.width - model.width) / 2.0;
			}
			_ => {
				model.width =
					(startx + from.width / 2.0 - (stopx + to.width / 2.0)).abs() + actor_margin;
				model.startx = match startx < stopx {
					true => startx + from.width / 2.0 - actor_margin / 2.0,
					false => stopx + to.width / 2.0 - actor_margin / 2.0,
				};
			}
		}
		if should_wrap {
			let width = model.width - 2.0 * self.conf.wrap_padding;
			model.text = self.wrap_label(&message.text, width, &font);
		}
		Some(model)
	}

	/// `buildMessageModel`: where the line of a message starts and ends.
	fn build_message_model(&mut self, message_index: usize) -> Option<MessageModel> {
		let message = self.db.messages[message_index].clone();
		let line_type = message.line_type;
		if !line_type.is_line() {
			return None;
		}
		let (from, to) = (message.from.clone()?, message.to.clone()?);
		let (from_left, from_right) = self.activation_bounds(&from);
		let (to_left, to_right) = self.activation_bounds(&to);
		let is_arrow_to_right = from_left <= to_left;
		let mut startx = if is_arrow_to_right { from_right } else { from_left };
		let mut stopx = if is_arrow_to_right { to_left } else { to_right };
		if self.conf.is_neo {
			let offset = 3.0;
			if line_type != LineType::SolidOpen {
				stopx += if is_arrow_to_right { -offset } else { offset };
			}
			if line_type.is_bidirectional() {
				startx += if is_arrow_to_right { offset } else { -offset };
			}
		}
		// `calculateCentralConnectionOffset`
		if matches!(message.central, CentralConnection::Reverse | CentralConnection::Dual) {
			startx += 4.0;
			if line_type.is_bidirectional() && !is_arrow_to_right {
				startx -= 6.0;
			}
		}
		let is_arrow_to_activation = (to_left - to_right).abs() > 2.0;
		let adjust = |value: f64| if is_arrow_to_right { -value } else { value };
		if from == to {
			stopx = startx;
		} else {
			if message.activate && !is_arrow_to_activation {
				stopx += adjust(self.conf.activation_width / 2.0 - 1.0);
			}
			use LineType::*;
			if !matches!(
				line_type,
				SolidOpen
					| DottedOpen | StickTop
					| StickBottom | StickTopDotted
					| StickBottomDotted
					| SolidArrowTopReverseDotted
					| SolidArrowBottomReverseDotted
					| StickArrowTopReverse
					| StickArrowBottomReverse
					| StickArrowTopReverseDotted
					| StickArrowBottomReverseDotted
					| SolidArrowTopReverse
					| SolidArrowBottomReverse
			) {
				stopx += adjust(3.0);
			}
			if matches!(
				line_type,
				BidirectionalSolid
					| BidirectionalDotted
					| SolidArrowTopReverseDotted
					| SolidArrowBottomReverseDotted
					| SolidArrowTopReverse
					| SolidArrowBottomReverse
			) {
				startx -= adjust(3.0);
			}
		}
		let all_bounds = [from_left, from_right, to_left, to_right];
		let bounded_width = (startx - stopx).abs();
		let font = self.conf.message_font.clone();
		let mut text = message.text.clone();
		if message.wrap && !text.is_empty() {
			let width = get_max(&[bounded_width + 2.0 * self.conf.wrap_padding, self.conf.width]);
			text = self.wrap_label(&text, width, &font);
		}
		let text_width = self.text_width(&text, &font);
		Some(MessageModel {
			width: get_max(&[
				if message.wrap { 0.0 } else { text_width + 2.0 * self.conf.wrap_padding },
				bounded_width + 2.0 * self.conf.wrap_padding,
				self.conf.width,
			]),
			startx,
			stopx,
			text,
			from_bounds: get_min(&all_bounds),
			to_bounds: get_max(&all_bounds),
			..MessageModel::default()
		})
	}

	/// `calculateLoopBounds`: the widths of the blocks, and the message and note models.
	fn calculate_loop_bounds(&mut self) -> HashMap<usize, LoopWidth> {
		let mut arena: Vec<LoopWidth> = Vec::new();
		let mut loops: HashMap<usize, usize> = HashMap::new();
		let mut stack: Vec<usize> = Vec::new();
		let label_box_width = self.conf.label_box_width;
		for index in 0..self.db.messages.len() {
			let message = self.db.messages[index].clone();
			use LineType::*;
			match message.line_type {
				LoopStart | AltStart | OptStart | ParStart | ParOverStart | CriticalStart
				| BreakStart => {
					arena.push(LoopWidth {
						id: index,
						from: 9_007_199_254_740_991.0,
						to: -9_007_199_254_740_991.0,
						width: 0.0,
					});
					stack.push(arena.len() - 1);
				}
				AltElse | ParAnd | CriticalOption => {
					if !message.text.is_empty()
						&& let Some(&current) = stack.last()
					{
						loops.insert(arena[current].id, current);
						loops.insert(index, current);
					}
				}
				LoopEnd | AltEnd | OptEnd | ParEnd | CriticalEnd | BreakEnd => {
					if let Some(current) = stack.pop() {
						loops.insert(arena[current].id, current);
					}
				}
				ActiveStart => {
					if let Some(actor) = message.from.as_deref()
						&& let Some(actor_index) = self.actor_index(actor)
					{
						let layout = &self.actors[actor_index];
						let stacked = self.actor_activations(actor) as f64;
						let x = layout.x
							+ layout.width / 2.0 + (stacked - 1.0) * self.conf.activation_width
							/ 2.0;
						self.activations.push(Activation {
							startx: x,
							stopx: x + self.conf.activation_width,
							starty: None,
							stopy: None,
							actor: actor.to_owned(),
							anchor: usize::MAX,
						});
					}
				}
				ActiveEnd => {
					if let Some(actor) = message.from.as_deref() {
						let _ = self.end_activation(actor);
					}
				}
				_ => {}
			}
			if message.placement.is_some() {
				let Some(model) = self.build_note_model(index) else { continue };
				for &current in &stack {
					let item = &mut arena[current];
					item.from = get_min(&[item.from, model.startx]);
					item.to = get_max(&[item.to, model.startx + model.width]);
					item.width =
						get_max(&[item.width, (item.from - item.to).abs()]) - label_box_width;
				}
				self.notes[index] = Some(model);
			} else {
				let model = self.build_message_model(index);
				if let Some(model) = &model
					&& model.startx != 0.0
					&& model.stopx != 0.0
				{
					let (from, to) = (message.from.as_deref(), message.to.as_deref());
					let from =
						from.and_then(|id| self.actor_index(id)).map(|i| self.actors[i].clone());
					let to = to.and_then(|id| self.actor_index(id)).map(|i| self.actors[i].clone());
					for &current in &stack {
						let item = &mut arena[current];
						if model.startx == model.stopx {
							let (Some(from), Some(to)) = (&from, &to) else { continue };
							item.from = get_min(&[
								from.x - model.width / 2.0,
								from.x - from.width / 2.0,
								item.from,
							]);
							item.to = get_max(&[
								to.x + model.width / 2.0,
								to.x + from.width / 2.0,
								item.to,
							]);
							item.width = get_max(&[item.width, (item.to - item.from).abs()])
								- label_box_width;
						} else {
							item.from = get_min(&[model.startx, item.from]);
							item.to = get_max(&[model.stopx, item.to]);
							item.width = get_max(&[item.width, model.width]) - label_box_width;
						}
					}
				}
				self.messages[index] = model;
			}
		}
		self.activations.clear();
		loops.into_iter().map(|(id, index)| (id, arena[index])).collect()
	}
}

fn nonzero(value: f64) -> Option<f64> {
	(value != 0.0 && !value.is_nan()).then_some(value)
}

/// What a block statement does to the open blocks (`adjustLoopHeightForWrap`'s callback).
enum BlockAction {
	Open,
	Rect,
	Section,
}

impl Renderer<'_, '_> {
	/// `draw`: lays out and draws the diagram.
	fn draw(mut self, common: &CommonFields) -> Result<Diagram> {
		let widths = self.max_message_width_per_actor();
		self.conf.height = self.calculate_actor_margins(&widths);
		if !self.boxes.is_empty() {
			self.bump_vertical_pos(self.conf.box_margin);
			if self.boxes.iter().any(|layout| layout.name.is_some()) {
				self.bump_vertical_pos(self.boxes[0].text_max_height);
			}
		}
		let actor_keys = self.actor_keys();
		let color_index: HashMap<String, usize> =
			actor_keys.iter().enumerate().map(|(index, key)| (key.clone(), index)).collect();
		let color_of = |actor: &str| color_index.get(actor).copied().unwrap_or(0);
		self.add_actor_rendering_data(&actor_keys);
		let loop_widths = self.calculate_loop_bounds();

		let box_margin = self.conf.box_margin;
		let box_text_margin = self.conf.box_text_margin;
		let mut sequence_index = 1.0;
		let mut sequence_step = 1.0;
		let mut numbers_visible = false;
		let mut messages_to_draw: Vec<(usize, f64)> = Vec::new();
		let mut backgrounds: Vec<LoopModel> = Vec::new();
		for index in 0..self.db.messages.len() {
			let message = self.db.messages[index].clone();
			use LineType::*;
			match message.line_type {
				Note => self.draw_note(index),
				ActiveStart | CentralConnection | CentralConnectionReverse => {
					if let Some(from) = &message.from {
						self.new_activation(from);
					}
				}
				ActiveEnd => {
					if let Some(from) = &message.from {
						self.active_end(from, color_of(from));
					}
				}
				LoopStart | OptStart | AltStart | ParStart | ParOverStart | CriticalStart
				| BreakStart => self.adjust_loop_height_for_wrap(
					&loop_widths,
					index,
					(box_margin, box_margin + box_text_margin),
					BlockAction::Open,
				),
				RectStart => self.adjust_loop_height_for_wrap(
					&loop_widths,
					index,
					(box_margin, box_margin),
					BlockAction::Rect,
				),
				AltElse | ParAnd | CriticalOption => self.adjust_loop_height_for_wrap(
					&loop_widths,
					index,
					(box_margin + box_text_margin, box_margin),
					BlockAction::Section,
				),
				LoopEnd | OptEnd | AltEnd | ParEnd | CriticalEnd | BreakEnd | RectEnd => {
					let Some(model) = self.sequence_items.pop() else { continue };
					let label = match message.line_type {
						LoopEnd => "loop",
						OptEnd => "opt",
						AltEnd => "alt",
						ParEnd => "par",
						CriticalEnd => "critical",
						BreakEnd => "break",
						_ => "",
					};
					match label.is_empty() {
						true => backgrounds.push(model.clone()),
						false => self.draw_loop(&model, label),
					}
					let stopy = model.extent.stopy.unwrap_or(self.vertical_pos);
					self.bump_vertical_pos(stopy - self.vertical_pos);
				}
				Autonumber => {
					if let Some(settings) = message.autonumber {
						sequence_index =
							settings.start.filter(|start| *start != 0.0).unwrap_or(sequence_index);
						sequence_step =
							settings.step.filter(|step| *step != 0.0).unwrap_or(sequence_step);
						numbers_visible = settings.visible;
					}
				}
				_ => {
					let Some(mut model) = self.messages[index].take() else { continue };
					model.starty = self.vertical_pos;
					model.sequence_index = sequence_index;
					model.sequence_visible = numbers_visible;
					let line_start_y = self.bound_message(&mut model);
					self.adjust_created_destroyed(index, &mut model, line_start_y);
					self.messages[index] = Some(model);
					messages_to_draw.push((index, line_start_y));
				}
			}
			if message.line_type.is_line() {
				sequence_index = js_round((sequence_index + sequence_step) * 100.0) / 100.0;
			}
		}

		for key in &actor_keys {
			if let Some(index) = self.actor_index(key) {
				self.draw_actor(index, false, color_of(key));
			}
		}
		for (index, line_start_y) in messages_to_draw {
			self.draw_message(index, line_start_y);
		}
		if self.conf.mirror_actors {
			self.bump_vertical_pos(box_margin * 2.0);
			let mut max_height: f64 = 0.0;
			for key in &actor_keys {
				let Some(index) = self.actor_index(key) else { continue };
				if self.actors[index].stopy.is_none_or(|stopy| stopy == 0.0) {
					self.actors[index].stopy = Some(self.vertical_pos);
				}
				let height = self.draw_actor(index, true, color_of(key));
				max_height = get_max(&[max_height, height]);
			}
			self.bump_vertical_pos(max_height + box_margin);
		}
		for model in &backgrounds {
			self.draw_background(model);
		}
		for box_index in self.box_models.clone() {
			let layout = self.boxes[box_index].clone();
			let height = self.vertical_pos - layout.y;
			self.insert(layout.x, layout.y, layout.x + layout.width, height);
			let padding = box_margin * 2.0;
			let startx = layout.x - padding;
			let starty = layout.y - padding * 0.25;
			let stopx = startx + layout.width + 2.0 * padding;
			let stopy = starty + height + padding * 0.75;
			self.draw_box(box_index, (startx, starty, stopx, stopy));
		}
		if !self.boxes.is_empty() {
			self.bump_vertical_pos(box_margin);
		}
		let (mut popup_height, mut popup_width): (f64, f64) = (0.0, 0.0);
		for key in &actor_keys {
			let Some(index) = self.actor_index(key) else { continue };
			let (height, width) = self.draw_popup(index);
			popup_height = popup_height.max(height);
			popup_width = popup_width.max(width + self.actors[index].x);
		}
		self.finish(common, (popup_height, popup_width))
	}

	/// The view box, the title and the document.
	fn finish(mut self, common: &CommonFields, popup: (f64, f64)) -> Result<Diagram> {
		let lowered = std::mem::take(&mut self.lowered);
		let conf = &self.conf;
		let startx = self.data.startx.unwrap_or(0.0);
		let starty = self.data.starty.unwrap_or(0.0);
		let stopx = self.data.stopx.unwrap_or(0.0);
		let stopy = self.data.stopy.unwrap_or(0.0);
		let box_height = (stopy - starty).max(popup.0);
		let mut height = box_height + 2.0 * conf.diagram_margin_y;
		if conf.mirror_actors {
			height = height - conf.box_margin + conf.bottom_margin_adj;
		}
		let box_width = (stopx - startx).max(popup.1);
		let width = box_width + 2.0 * conf.diagram_margin_x;
		let title = common.title.as_deref().filter(|title| !title.is_empty());
		let extra_for_title = if title.is_some() { 40.0 } else { 0.0 };
		let extra_for_neo = if !self.db.actors.is_empty() && conf.is_neo { 30.0 } else { 0.0 };

		let mut svg = Svg::new();
		for layers in lowered.into_iter().rev() {
			for layer in layers {
				match layer {
					Layer::Svg(part) => svg.append(part),
					Layer::Lifeline { x, y1, actor } => {
						let layout = &self.actors[actor];
						let y2 = match (conf.mirror_actors, layout.stopy) {
							(true, stopy) => stopy.unwrap_or(0.0),
							(false, Some(stopy)) if stopy != 0.0 => stopy + layout.height / 2.0,
							(false, _) => 2000.0,
						};
						svg.open("line")
							.number_attr("x1", x)
							.number_attr("y1", y1)
							.number_attr("x2", x)
							.number_attr("y2", y2)
							.attr_if("stroke", &self.paint.actor_line)
							.attr("stroke-width", "2")
							.close();
					}
				}
			}
		}
		self.write_defs(&mut svg);
		for part in self.body {
			svg.append(part);
		}
		if let Some(title) = title {
			let font = Font::new(self.context.font_family(), self.context.font_size());
			let run = svg::TextRun {
				x: (stopx - startx) / 2.0 - 2.0 * conf.diagram_margin_x,
				y: -25.0,
				anchor: svg::Anchor::Start,
				baseline: svg::Baseline::Alphabetic,
				font: &font,
				content: &super::label::decode_entities_to_text(title),
			};
			svg.text(run, |svg| _ = svg.attr_if("fill", &self.paint.text_color));
		}
		let frame = Frame {
			x: startx - conf.diagram_margin_x,
			y: -(conf.diagram_margin_y + extra_for_title),
			width,
			height: height + extra_for_title + extra_for_neo,
			use_max_width: conf.use_max_width,
		};
		Ok(svg::document(self.context, "sequence", &frame, &common.accessibility, svg))
	}

	/// `activeEnd`: closes the latest activation of `actor` and draws it.
	fn active_end(&mut self, actor: &str, color_index: usize) {
		let Some(mut activation) = self.end_activation(actor) else { return };
		let mut vertical_pos = self.vertical_pos;
		let mut starty = activation.starty.unwrap_or(vertical_pos);
		if starty + 18.0 > vertical_pos {
			starty = vertical_pos - 6.0;
			vertical_pos += 12.0;
		}
		activation.starty = Some(starty);
		let geometry = (activation.startx, activation.stopx, starty);
		self.draw_activation(activation.anchor, geometry, vertical_pos, color_index);
		self.insert(activation.startx, vertical_pos - 10.0, activation.stopx, vertical_pos);
	}

	/// `adjustLoopHeightForWrap`: opens a block or section with its bracketed, wrapped title.
	fn adjust_loop_height_for_wrap(
		&mut self,
		loop_widths: &HashMap<usize, LoopWidth>,
		index: usize,
		(pre_margin, post_margin): (f64, f64),
		action: BlockAction,
	) {
		self.bump_vertical_pos(pre_margin);
		let mut height_adjust = post_margin;
		let mut text = self.db.messages[index].text.clone();
		if let Some(loop_width) = loop_widths.get(&index)
			&& !text.is_empty()
		{
			let font = self.conf.message_font.clone();
			let width = loop_width.width - 2.0 * self.conf.wrap_padding;
			text = self.wrap_label(&format!("[{text}]"), width, &font);
			let height = self.text_dimensions(&text, &font).1;
			height_adjust = post_margin + get_max(&[height, self.conf.label_box_height]);
		}
		let starty = Some(self.vertical_pos);
		match action {
			BlockAction::Open => self.sequence_items.push(LoopModel {
				extent: Extent { starty, ..Extent::default() },
				title: text,
				..LoopModel::default()
			}),
			BlockAction::Rect => {
				let fill = match text.is_empty() {
					true => self.paint.rect_bkg.clone(),
					false => text,
				};
				self.sequence_items.push(LoopModel {
					extent: Extent { starty, ..Extent::default() },
					fill: Some(fill),
					..LoopModel::default()
				});
			}
			BlockAction::Section => {
				let y = self.vertical_pos;
				if let Some(model) = self.sequence_items.last_mut() {
					model.sections.push((y, text));
				}
			}
		}
		self.bump_vertical_pos(height_adjust);
	}

	/// `drawNote`
	fn draw_note(&mut self, index: usize) {
		let Some(mut model) = self.notes[index].clone() else { return };
		let note_margin = self.conf.note_margin;
		self.bump_vertical_pos(self.conf.box_margin);
		model.starty = self.vertical_pos;
		let width = nonzero(model.width).unwrap_or(self.conf.width);
		let font = self.conf.note_font.draw.clone();
		let mut text_height = 0.0;
		for line in split_breaks(&model.text) {
			text_height += self.line_height(line, &font);
		}
		let math_label = math::has_math(&model.text).then(|| self.math_label(&model.text));
		let text_height = js_round(math_label.as_ref().map_or(text_height, |label| label.height));
		let mut svg = Svg::new();
		svg.open("g").end();
		svg.open("rect")
			.number_attr("x", model.startx)
			.number_attr("y", model.starty)
			.number_attr("width", width)
			.number_attr("height", text_height + 2.0 * note_margin)
			.attr_if("fill", &self.paint.note_bkg)
			.attr_if("stroke", &self.paint.note_border)
			.close();
		let fill = self.paint.note_text.clone();
		match &math_label {
			Some(label) => {
				// `drawKatex` centers the formula in the rect of its size.
				let left = js_round(model.startx + width / 2.0 - label.width / 2.0);
				let top = js_round(model.starty + note_margin);
				self.write_math(&mut svg, label, (left + label.width / 2.0, top), &fill);
			}
			None => {
				self.draw_text(
					&mut svg,
					draw::TextData {
						text: &model.text,
						x: model.startx,
						y: model.starty,
						width: Some(width),
						anchor: self.conf.note_align,
						is_centered: true,
						text_margin: note_margin,
						dy_em: true,
						font: &font,
						fill: &fill,
					},
				);
			}
		}
		svg.close_tag("g");
		self.body.push(svg);
		self.bump_vertical_pos(text_height + 2.0 * note_margin);
		model.stopy = model.starty + text_height + 2.0 * note_margin;
		model.stopx = model.startx + width;
		self.insert(model.startx, model.starty, model.stopx, model.stopy);
	}

	/// `boundMessage`: makes room for a message; returns the `y` of its line.
	fn bound_message(&mut self, model: &mut MessageModel) -> f64 {
		self.bump_vertical_pos(10.0);
		let font = self.conf.message_font.clone();
		let lines = split_breaks(&model.text).len() as f64;
		let is_math = math::has_math(&model.text);
		let (text_width, text_height) = self.math_or_text_dimensions(&model.text, &font);
		if !is_math {
			let line_height = text_height / lines;
			model.height += line_height;
			self.bump_vertical_pos(line_height);
		}
		let mut total_offset = text_height - 10.0;
		let line_start_y;
		if model.startx == model.stopx {
			if !self.conf.right_angles {
				total_offset += self.conf.box_margin;
			}
			line_start_y = self.vertical_pos + total_offset;
			total_offset += 30.0;
			let dx = get_max(&[text_width / 2.0, self.conf.width / 2.0]);
			let vertical_pos = self.vertical_pos;
			self.insert(
				model.startx - dx,
				vertical_pos - 10.0 + total_offset,
				model.stopx + dx,
				vertical_pos + 30.0 + total_offset,
			);
		} else {
			total_offset += self.conf.box_margin;
			line_start_y = self.vertical_pos + total_offset;
			self.insert(model.startx, line_start_y - 10.0, model.stopx, line_start_y);
		}
		self.bump_vertical_pos(total_offset);
		model.height += total_offset;
		model.stopy = model.starty + model.height;
		self.insert(model.from_bounds, model.starty, model.to_bounds, model.stopy);
		line_start_y
	}

	/// `adjustCreatedDestroyedData`: a message that creates or destroys a participant ends at
	/// its box, which sits on the message.
	fn adjust_created_destroyed(
		&mut self,
		index: usize,
		model: &mut MessageModel,
		line_start_y: f64,
	) {
		let message = self.db.messages[index].clone();
		let (Some(from), Some(to)) = (message.from.as_deref(), message.to.as_deref()) else {
			return;
		};
		let (Some(from_index), Some(to_index)) = (self.actor_index(from), self.actor_index(to))
		else {
			return;
		};
		let is_figure = |kind: ActorKind| {
			matches!(
				kind,
				ActorKind::Actor | ActorKind::Control | ActorKind::Entity | ActorKind::Database
			)
		};
		let note_margin = self.conf.note_margin;
		let adjustment = |renderer: &Self, actor: usize, extra: f64| match is_figure(
			renderer.db.actors[actor].1.kind,
		) {
			true => draw::ACTOR_TYPE_WIDTH / 2.0 + extra,
			false => renderer.actors[actor].width / 2.0 + extra,
		};
		let receiver =
			|renderer: &mut Self, model: &mut MessageModel, actor: usize, adjustment: f64| {
				let bottom = model.stopy + renderer.actors[actor].height / 2.0 + note_margin;
				if renderer.actors[actor].x < renderer.actors[from_index].x {
					renderer.insert(model.stopx - adjustment, model.starty, model.startx, bottom);
					model.stopx += adjustment;
				} else {
					renderer.insert(model.startx, model.starty, model.stopx + adjustment, bottom);
					model.stopx -= adjustment;
				}
			};
		if self.db.created_actors.get(to) == Some(&index) {
			let adjustment = adjustment(self, to_index, 3.0);
			receiver(self, model, to_index, adjustment);
			let height = self.actors[to_index].height;
			self.actors[to_index].starty = line_start_y - height / 2.0;
			self.bump_vertical_pos(height / 2.0);
		} else if self.db.destroyed_actors.get(from) == Some(&index) {
			if self.conf.mirror_actors {
				let adjustment = adjustment(self, from_index, 0.0);
				let bottom = model.stopy + self.actors[from_index].height / 2.0 + note_margin;
				if self.actors[from_index].x < self.actors[to_index].x {
					self.insert(model.startx - adjustment, model.starty, model.stopx, bottom);
					model.startx += adjustment;
				} else {
					self.insert(model.stopx, model.starty, model.startx + adjustment, bottom);
					model.startx -= adjustment;
				}
			}
			let height = self.actors[from_index].height;
			self.actors[from_index].stopy = Some(line_start_y - height / 2.0);
			self.bump_vertical_pos(height / 2.0);
		} else if self.db.destroyed_actors.get(to) == Some(&index) {
			if self.conf.mirror_actors {
				let adjustment = adjustment(self, to_index, 3.0);
				receiver(self, model, to_index, adjustment);
			}
			let height = self.actors[to_index].height;
			self.actors[to_index].stopy = Some(line_start_y - height / 2.0);
			self.bump_vertical_pos(height / 2.0);
		}
	}

	/// `drawMessage`: the text, the line with its markers and the sequence number.
	fn draw_message(&mut self, index: usize, line_start_y: f64) {
		let Some(model) = self.messages[index].clone() else { return };
		let message = self.db.messages[index].clone();
		let line_type = message.line_type;
		let (startx, stopx) = (model.startx, model.stopx);
		let font = self.conf.message_font.clone();
		let fill = self.paint.signal_text.clone();
		let mut svg = Svg::new();
		if math::has_math(&model.text) {
			let label = self.math_label(&model.text);
			let (left, right) = (startx.min(stopx), startx.max(stopx));
			let x = js_round(left + (right - left) / 2.0 - label.width / 2.0);
			let top = js_round(line_start_y - label.height);
			self.write_math(&mut svg, &label, (x + label.width / 2.0, top), &fill);
		} else {
			self.draw_text(
				&mut svg,
				draw::TextData {
					text: &model.text,
					x: startx.min(stopx),
					y: model.starty + 10.0,
					width: Some((stopx - startx).abs()),
					anchor: self.conf.message_align,
					is_centered: true,
					text_margin: self.conf.wrap_padding,
					dy_em: true,
					font: &font.draw,
					fill: &fill,
				},
			);
		}
		let text_width = self.text_width(&model.text, &font);
		let numbers_on = model.sequence_visible || self.conf.show_sequence_numbers;
		let has_central = message.central != db::CentralConnection::None;
		let id = self.context.id.clone();
		let marker = |name: &str| format!("url(#{id}-{name})");
		let (mut x1, mut x2) = (startx, stopx);
		if numbers_on {
			let shifts_start = matches!(
				message.central,
				db::CentralConnection::Dual | db::CentralConnection::Reverse
			);
			if line_type.is_bidirectional() {
				x1 = match startx < stopx {
					true => startx + 12.0,
					false => {
						startx - 6.0
							+ if has_central { -5.0 } else { 0.0 }
							+ if shifts_start { -7.5 } else { 0.0 }
					}
				};
			} else if line_type.is_reverse() {
				let mut line_stop = stopx;
				let mut line_start = startx;
				if stopx > startx {
					line_stop = stopx - 12.0;
				} else {
					line_stop -= 6.0;
					line_start += if shifts_start { -7.5 } else { 0.0 };
				}
				line_stop += if has_central { 15.0 } else { 0.0 };
				(x1, x2) = (line_start, line_stop);
			} else {
				x1 = startx + 6.0;
			}
		}
		let stroke = self.paint.signal.clone();
		if startx == stopx {
			let line_start_x = startx
				+ if numbers_on && (line_type.is_reverse() || line_type.is_bidirectional()) {
					10.0
				} else {
					0.0
				};
			let d = match self.conf.right_angles {
				true => format!(
					"M  {},{} H {} V {} H {}",
					number(line_start_x),
					number(line_start_y),
					number(startx + get_max(&[self.conf.width / 2.0, text_width / 2.0])),
					number(line_start_y + 25.0),
					number(startx)
				),
				false => format!(
					"M {},{} C {},{} {},{} {},{}",
					number(line_start_x),
					number(line_start_y),
					number(line_start_x + 60.0),
					number(line_start_y - 10.0),
					number(startx + 60.0),
					number(line_start_y + 30.0),
					number(startx),
					number(line_start_y + 20.0)
				),
			};
			svg.open("path").attr("d", &d);
		} else {
			svg.open("line")
				.number_attr("x1", x1)
				.number_attr("y1", line_start_y)
				.number_attr("x2", x2)
				.number_attr("y2", line_start_y);
		}
		svg.attr_if("stroke", &stroke).attr("stroke-width", "1.5").attr("fill", "none");
		if line_type.is_dotted() {
			svg.attr("stroke-dasharray", "3, 3");
		}
		use LineType::*;
		let (start_marker, end_marker) = match line_type {
			SolidTop | SolidTopDotted => (None, Some("solidTopArrowHead")),
			SolidBottom | SolidBottomDotted => (None, Some("solidBottomArrowHead")),
			StickTop | StickTopDotted => (None, Some("stickTopArrowHead")),
			StickBottom | StickBottomDotted => (None, Some("stickBottomArrowHead")),
			SolidArrowTopReverse | SolidArrowTopReverseDotted => {
				(Some("solidBottomArrowHead"), None)
			}
			SolidArrowBottomReverse | SolidArrowBottomReverseDotted => {
				(Some("solidTopArrowHead"), None)
			}
			StickArrowTopReverse | StickArrowTopReverseDotted => {
				(Some("stickBottomArrowHead"), None)
			}
			StickArrowBottomReverse | StickArrowBottomReverseDotted => {
				(Some("stickTopArrowHead"), None)
			}
			Solid | Dotted => (None, Some("arrowhead")),
			BidirectionalSolid | BidirectionalDotted => (Some("arrowhead"), Some("arrowhead")),
			SolidPoint | DottedPoint => (None, Some("filled-head")),
			SolidCross | DottedCross => (None, Some("crosshead")),
			_ => (None, None),
		};
		if let Some(name) = start_marker {
			svg.attr("marker-start", &marker(&format!("{name}-start")));
		}
		if let Some(name) = end_marker {
			svg.attr("marker-end", &marker(name));
		}
		svg.close();
		if has_central {
			self.draw_central_connection(&mut svg, &message, &model, line_start_y);
		}
		if numbers_on {
			let is_left_to_right = startx <= stopx;
			let number_x = if startx == stopx {
				model.from_bounds + 1.0
			} else if line_type.is_reverse() == is_left_to_right {
				model.to_bounds - 1.0
			} else {
				model.from_bounds + 1.0
			};
			let text = super::color::js_number(model.sequence_index);
			let size = match text.encode_utf16().count() {
				length if length > 5 => 7.0,
				length if length > 3 => 9.0,
				_ => 12.0,
			};
			svg.open("line")
				.number_attr("x1", number_x)
				.number_attr("y1", line_start_y)
				.number_attr("x2", number_x)
				.number_attr("y2", line_start_y)
				.attr("stroke-width", "2")
				.attr("marker-start", &marker("sequencenumber"))
				.close();
			let font = Font::new("sans-serif", size);
			let run = svg::TextRun {
				x: number_x,
				y: line_start_y + 4.0,
				anchor: svg::Anchor::Middle,
				baseline: svg::Baseline::Alphabetic,
				font: &font,
				content: &text,
			};
			svg.text(run, |svg| _ = svg.attr_if("fill", &self.paint.sequence_number));
		}
		self.body.push(svg);
	}

	/// `drawCentralConnection`: the circles of `()` on the lifelines.
	fn draw_central_connection(
		&self,
		svg: &mut Svg,
		message: &db::Message,
		model: &MessageModel,
		line_start_y: f64,
	) {
		let center = |id: Option<&str>| {
			id.and_then(|id| self.actor_index(id))
				.map(|index| self.actors[index].x + self.actors[index].width / 2.0)
		};
		let (Some(mut from_center), Some(mut to_center)) =
			(center(message.from.as_deref()), center(message.to.as_deref()))
		else {
			return;
		};
		let is_left_to_right = from_center <= to_center;
		let is_reverse = message.line_type.is_reverse();
		let offset = |reverse: bool| {
			let base = if is_left_to_right { 16.5 } else { -16.5 };
			if reverse { -base } else { base }
		};
		if model.sequence_visible {
			match message.central {
				CentralConnection::Forward if is_reverse => to_center += offset(true),
				CentralConnection::Reverse if !is_reverse => from_center += offset(false),
				CentralConnection::Dual if is_reverse => to_center += offset(true),
				CentralConnection::Dual => from_center += offset(false),
				_ => {}
			}
		}
		let centers: &[f64] = match message.central {
			CentralConnection::Forward => &[to_center],
			CentralConnection::Reverse => &[from_center],
			CentralConnection::Dual => &[from_center, to_center],
			CentralConnection::None => &[],
		};
		for &cx in centers {
			svg.open("circle")
				.number_attr("cx", cx)
				.number_attr("cy", line_start_y)
				.attr("r", "5")
				.attr_if("fill", &self.paint.text_color)
				.close();
		}
	}
}
