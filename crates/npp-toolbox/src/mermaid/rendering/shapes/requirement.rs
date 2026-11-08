//! The box of requirements and elements (`requirementBox.ts`): the type and the bold name
//! centered above the fields, which start at the left. Its rectangle and divider are roughjs
//! shapes like the entity box of ER diagrams.

use super::super::super::label::LabelBox;
use super::super::super::text::Font;
use super::super::super::{Context, Look};
use super::super::{Node, layout_label};
use super::er::{ErKind, ErPart};
use super::{Intersect, Part, Point, Shape, centered_box, point};

const PADDING: f64 = 20.0;
const GAP: f64 = 20.0;

/// The laid out box: its shape, whose label is the type, and the other labels with their
/// centers.
pub struct RequirementBox {
	pub shape: Shape,
	pub type_label: LabelBox,
	pub labels: Vec<(LabelBox, Point)>,
}

/// `addText`: a label that wraps only beyond the width of its text, which Mermaid measures
/// with the entities and some room for errors.
fn add_text(context: &mut Context, text: &str, font: &Font) -> LabelBox {
	let measure_font = context.configured_text_font();
	let width = context.text_dimensions(text, &measure_font).0 + 50.0;
	layout_label(context, text, true, font, Some(width), 0.0)
}

/// `label` prefixed with `prefix`, empty without a value.
fn field(prefix: &str, value: &str) -> String {
	if value.is_empty() { String::new() } else { format!("{prefix}: {value}") }
}

/// Lays out the box of `node` with its labels in `font`.
pub fn requirement_box(context: &mut Context, node: &Node, font: &Font) -> RequirementBox {
	let body = node.requirement.clone().unwrap_or_default();
	let bold = Font { weight: 700, ..font.clone() };
	let type_text = format!("&lt;&lt;{}&gt;&gt;", body.kind.unwrap_or("Element"));
	let type_label = add_text(context, &type_text, font);
	let type_height = type_label.height;
	// The other labels with the offsets of their centers.
	let mut labels: Vec<(LabelBox, f64)> = Vec::new();
	let mut add = |context: &mut Context, text: &str, font: &Font, offset: f64| -> f64 {
		// Empty text has no label.
		if text.is_empty() {
			return 0.0;
		}
		let label = add_text(context, text, font);
		let height = label.height;
		labels.push((label, offset));
		height
	};
	let name_height = add(context, &node.label, &bold, type_height);
	let mut offset = type_height + name_height + GAP;
	let prefixes: &[&str] = match body.kind {
		Some(_) => &["ID", "Text", "Risk", "Verification"],
		None => &["Type", "Doc Ref"],
	};
	for (index, prefix) in prefixes.iter().enumerate() {
		let height = add(context, &field(prefix, &body.fields[index]), font, offset);
		// The last row does not count for the divider.
		if index + 1 < prefixes.len() {
			offset += height;
		}
	}
	// `getBBox` of the labels, each centered on its offset.
	let mut top = -type_height / 2.0;
	let mut bottom = type_height / 2.0;
	let mut widest = type_label.width;
	for (label, center) in &labels {
		top = top.min(center - label.height / 2.0);
		bottom = bottom.max(center + label.height / 2.0);
		widest = widest.max(label.width);
	}
	let width = widest + PADDING;
	let height = bottom - top + PADDING;
	let (x, y) = (-width / 2.0, -height / 2.0);
	let label_center = point(0.0, PADDING - height / 2.0);
	// Only the configured layout `elk` aligns the lines of the fields left.
	let is_body_left_aligned = context.layout.as_deref() == Some("elk");
	let others: Vec<(LabelBox, Point)> = labels
		.into_iter()
		.enumerate()
		.map(|(index, (mut label, offset))| {
			label.is_left_aligned = index > 0 && is_body_left_aligned;
			// The name stays centered like the type; the fields start at the left.
			let center_x = if index == 0 { 0.0 } else { x + PADDING / 2.0 + label.width / 2.0 };
			(label, point(center_x, offset - height / 2.0 + PADDING))
		})
		.collect();
	let mut parts = vec![Part::Er(ErPart::Rough {
		kind: ErKind::Outer,
		corners: [
			point(x, y),
			point(x + width, y),
			point(x + width, y + height),
			point(x, y + height),
		],
	})];
	if offset > type_height + name_height + GAP {
		let line_y = y + type_height + name_height + GAP;
		parts.push(Part::Er(match node.look {
			Look::Neo => {
				const THICKNESS: f64 = 0.001;
				ErPart::Rough {
					kind: ErKind::Divider,
					corners: [
						point(x, line_y),
						point(x + width, line_y),
						point(x + width, line_y + THICKNESS),
						point(x, line_y + THICKNESS),
					],
				}
			}
			_ => ErPart::Line { from: point(x, line_y), to: point(x + width, line_y) },
		}));
	}
	let shape = Shape {
		parts,
		bbox: centered_box(width, height),
		intersect: Intersect::Rect,
		label_center,
	};
	RequirementBox { shape, type_label, labels: others }
}
