//! The box of class diagrams (`classBox.ts`, `shapeUtil.ts`): the annotation and the bold name
//! centered at the top, the attributes and methods below dividers. Each part is a group of
//! labels that `getBBox` measures; the box is laid out from these measurements.

use super::super::super::label::{self, LabelBox};
use super::super::super::math;
use super::super::super::svg::Svg;
use super::super::super::text::Font;
use super::super::super::{Context, Look};
use super::super::{BBox, ClassMember, Node, Styles, Theme, layout_label};
use super::er::corners_path;
use super::{Intersect, Part, Point, Shape, point};

/// An element of the class box.
#[derive(Clone, Debug, PartialEq)]
pub enum ClassPart {
	/// The roughjs rectangle: a filled and a stroked outline of its corners.
	Outer([Point; 4]),
	/// A roughjs line between the sections, 0.001 pixels from horizontal.
	Divider { from: Point, to: Point },
}

/// The laid out class box: its shape, whose label is the name, and the other labels with their
/// centers.
pub struct ClassBox {
	pub shape: Shape,
	pub name: LabelBox,
	pub labels: Vec<(LabelBox, Point)>,
}

/// A group of labels (`g.text`): each label with its offset and the area of the group.
#[derive(Default)]
struct TextGroup {
	/// The labels and the top of their `<foreignObject>`.
	labels: Vec<(LabelBox, f64)>,
	bbox: BBox,
}

impl TextGroup {
	/// `addText` at `offset`: a label is moved up by half the height of one of its lines.
	/// Returns its height.
	fn add(&mut self, context: &mut Context, text: &str, font: &Font, offset: f64) -> f64 {
		self.add_underlined(context, text, font, offset, false)
	}

	fn add_underlined(
		&mut self,
		context: &mut Context,
		text: &str,
		font: &Font,
		offset: f64,
		is_underlined: bool,
	) -> f64 {
		let measure_font = context.configured_text_font();
		// Mermaid wraps at the width of the text with some room for errors.
		let width = context.text_dimensions(text, &measure_font).0 + 50.0;
		let mut label = layout_label(context, text, true, font, Some(width), 0.0);
		label.is_underlined = is_underlined;
		// `numberOfLines`: rows of formulas count too.
		let lines = match math::has_math(text) {
			true => 1 + math::row_count(text),
			false => label::label_lines(text, true).len().max(1),
		} as f64;
		let top = offset - label.height / (2.0 * lines);
		self.bbox.include_rect(0.0, top, label.width, label.height);
		let height = label.height;
		self.labels.push((label, top));
		height
	}

	fn width(&self) -> f64 {
		self.bbox.width()
	}

	fn height(&self) -> f64 {
		self.bbox.height()
	}

	/// The labels with their centers, the group moved to `x`, `y`.
	fn place(self, x: f64, y: f64) -> impl Iterator<Item = (LabelBox, Point)> {
		self.labels.into_iter().map(move |(label, top)| {
			let center = point(x + label.width / 2.0, y + top + label.height / 2.0);
			(label, center)
		})
	}
}

/// The font of a member: its classifier makes it italic.
fn member_font(font: &Font, member: &ClassMember) -> Font {
	Font { is_italic: font.is_italic || member.is_italic, ..font.clone() }
}

/// Lays out the box of `node` with its labels in `font`.
pub fn class_box(context: &mut Context, node: &Node, font: &Font, styles: &Styles) -> ClassBox {
	let body = node.class_body.clone().unwrap_or_default();
	let padding = context.config_number("class", "padding", 12.0);
	let gap = padding;
	let hides_empty_box = context.config_bool("class", "hideEmptyMembersBox", false);
	let mut annotation = TextGroup::default();
	if let Some(text) = &body.annotation {
		annotation.add(context, &format!("«{text}»"), font, 0.0);
	}
	let annotation_height = annotation.height();
	// `font-weight: bolder` on the label, unless the node's style sets the weight.
	let bold = match styles.get("font-weight") {
		Some(_) => font.clone(),
		None => Font { weight: 700, ..font.clone() },
	};
	let mut name = TextGroup::default();
	name.add(context, &node.label, &bold, 0.0);
	let name_height = name.height();
	let mut members = TextGroup::default();
	let mut offset = 0.0;
	for member in &body.members {
		let font = member_font(font, member);
		offset +=
			members.add_underlined(context, &member.text, &font, offset, member.is_underlined);
	}
	let members_height = match members.height() {
		height if height <= 0.0 => gap / 2.0,
		height => height,
	};
	let mut methods = TextGroup::default();
	let mut offset = 0.0;
	for method in &body.methods {
		let font = member_font(font, method);
		offset +=
			methods.add_underlined(context, &method.text, &font, offset, method.is_underlined);
	}
	// The groups as `textHelper` moves them, and the area of all of them.
	let members_y = annotation_height + name_height + gap * 2.0;
	let methods_y = annotation_height
		+ name_height
		+ match members_height != 0.0 {
			true => members_height + gap * 4.0,
			false => gap * 2.0,
		};
	let mut bbox = BBox::default();
	bbox.include(&annotation.bbox, -annotation.width() / 2.0, 0.0);
	bbox.include(&name.bbox, -name.width() / 2.0, annotation_height);
	bbox.include(&members.bbox, 0.0, members_y);
	bbox.include(&methods.bbox, 0.0, methods_y);

	let is_empty = body.members.is_empty() && body.methods.is_empty();
	let width = bbox.width();
	let mut height = bbox.height();
	if is_empty {
		height += gap;
	} else if body.methods.is_empty() {
		height += gap * 2.0;
	}
	let (x, y) = (-width / 2.0, -height / 2.0);
	let has_extra_box = is_empty && !hides_empty_box;
	let top_shift = match (has_extra_box, is_empty) {
		(true, _) => padding,
		(false, true) => -padding / 2.0,
		(false, false) => 0.0,
	};
	let extra_height = match (has_extra_box, is_empty) {
		(true, _) => padding * 2.0,
		(false, true) => -padding,
		(false, false) => 0.0,
	};
	let rect_x = x - padding;
	let rect_y = y - padding - top_shift;
	let rect_width = width + 2.0 * padding;
	let rect_height = height + 2.0 * padding + extra_height;
	// The heights `classBox` reads again; `a - b || 0` keeps negative ones.
	let shrink = if has_extra_box { padding / 2.0 } else { 0.0 };
	let annotation_height_again = annotation.height() - shrink;
	let name_height_again = name.height() - shrink;
	let members_height_again = members.height() - shrink;
	let section_top = y + padding - top_shift;
	let hidden_empty = is_empty && hides_empty_box;
	let hidden_y = |translate_y: f64| match body.annotation.is_some() {
		true => translate_y - gap,
		false => translate_y,
	};
	// The annotation group's transform has no y, which `classBox` then reads as 0.
	let annotation_y = if hidden_empty { hidden_y(0.0) } else { section_top };
	let name_y =
		if hidden_empty { hidden_y(annotation_height) } else { annotation_height + section_top };
	let members_top = if hidden_empty { hidden_y(members_y) } else { members_y + section_top };
	let methods_top = match hidden_empty {
		true => hidden_y(methods_y),
		false => {
			annotation_height_again
				+ name_height_again
				+ members_height_again.max(gap / 2.0)
				+ y + gap * 4.0
				+ padding
		}
	};
	let annotation_x = -annotation.width() / 2.0;
	let name_x = -name.width() / 2.0;
	let mut placed: Vec<(LabelBox, Point)> = name.place(name_x, name_y).collect();
	let (name_label, name_center) = placed.remove(0);
	placed.extend(annotation.place(annotation_x, annotation_y));
	placed.extend(members.place(x, members_top));
	placed.extend(methods.place(x, methods_top));

	let mut parts = vec![Part::Class(ClassPart::Outer([
		point(rect_x, rect_y),
		point(rect_x + rect_width, rect_y),
		point(rect_x + rect_width, rect_y + rect_height),
		point(rect_x, rect_y + rect_height),
	]))];
	if !hidden_empty {
		const THICKNESS: f64 = 0.001;
		let first = annotation_height_again + name_height_again + y + padding;
		let second = first + members_height_again + gap * 2.0;
		for line_y in [first, second] {
			parts.push(Part::Class(ClassPart::Divider {
				from: point(rect_x, line_y),
				to: point(rect_x + rect_width, line_y + THICKNESS),
			}));
		}
	}
	let mut shape_box = BBox::default();
	shape_box.include_rect(rect_x, rect_y, rect_width, rect_height);
	let shape =
		Shape { parts, bbox: shape_box, intersect: Intersect::Rect, label_center: name_center };
	ClassBox { shape, name: name_label, labels: placed }
}

/// The color slot of a class in the `redux-color` themes: its border and background.
fn slot_colors<'a>(node: &Node, theme: &'a Theme) -> Option<(&'a str, Option<&'a str>)> {
	let slot = theme.color_slot(node)?;
	let background = (!theme.background_colors.is_empty())
		.then(|| theme.background_colors[slot % theme.background_colors.len()].as_str());
	Some((theme.border_colors[slot].as_str(), background))
}

/// Writes a part of the class box. The roughjs attributes of its paths give way to the rules of
/// the style sheet (`.node path`, the neo look, the color slots), which the user styles
/// (`style`, `classDef`) override as `style` attribute.
pub fn write(svg: &mut Svg, part: &ClassPart, node: &Node, theme: &Theme) {
	// A linked node's `data-look` is on its `<a>`, so the rules of the look and the color slots
	// miss it.
	let has_look = node.link.is_none();
	let is_neo = has_look && node.look == Look::Neo;
	let uses_gradient = theme.neo_stroke.starts_with("url(");
	let slot = slot_colors(node, theme).filter(|_| has_look);
	let user_css = node.css_styles.join(";");
	let user = Styles::compile(&[&node.css_styles]);
	let width = match is_neo {
		true => format!("{}px", theme.stroke_width),
		false => theme.stroke_width.clone(),
	};
	let fill = theme.plain_main_bkg.as_str();
	// roughjs dashes from the user styles; Mermaid's `0 0` would hide the line in EdgeHTML.
	let dashes = user.get("stroke-dasharray").map(|dashes| {
		let values: Vec<&str> = dashes.split_whitespace().collect();
		match values.as_slice() {
			[single] => format!("{single} {single}"),
			[first, second, ..] => format!("{first} {second}"),
			[] => String::new(),
		}
	});
	match part {
		ClassPart::Outer(corners) => {
			let stroke = match (slot, is_neo) {
				(Some((border, _)), _) => border,
				(None, true) => theme.neo_stroke.as_str(),
				(None, false) => theme.plain_node_border.as_str(),
			};
			let fill = slot.and_then(|(_, background)| background).unwrap_or(fill);
			svg.open("g").attr("class", "basic label-container outer-path");
			if is_neo {
				svg.attr_if("filter", &theme.neo_filter);
			}
			svg.end();
			for is_outline in [false, true] {
				svg.open("path")
					.attr("d", &corners_path(corners, is_outline))
					.attr_if("stroke", stroke)
					.attr_if("stroke-width", &width)
					.attr_if("fill", fill);
				if is_outline && let Some(dashes) = &dashes {
					svg.attr_if("stroke-dasharray", dashes);
				}
				svg.attr_if("style", &user_css).close();
			}
			svg.close_tag("g");
		}
		ClassPart::Divider { from, to } => {
			let is_neo_line = is_neo && !uses_gradient;
			let stroke = match (slot, is_neo) {
				(Some((border, _)), _) => border,
				(None, true) if !is_neo_line => theme.neo_stroke.as_str(),
				(None, _) => theme.plain_node_border.as_str(),
			};
			svg.open("g")
				.attr("class", if is_neo_line { "divider neo-line" } else { "divider" })
				.attr_if("style", &user_css)
				.end();
			svg.open("path")
				.attr("d", &corners_path(&[*from, *to], false))
				.attr_if("stroke", stroke)
				.attr_if("stroke-width", &width)
				.attr_if("fill", fill);
			if let Some(dashes) = &dashes {
				svg.attr_if("stroke-dasharray", dashes);
			}
			svg.attr_if("style", &user_css).close();
			svg.close_tag("g");
		}
	}
}
