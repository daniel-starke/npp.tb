//! The elements, boundaries and relationships of C4 diagrams (`c4Db.ts`).

use super::super::common;

/// An argument of a statement: a positional value or a named `$key="value"` one.
#[derive(Clone, Debug, PartialEq)]
pub enum Attribute {
	Text(String),
	Named(String, String),
}

impl Attribute {
	/// The value where a function takes a plain string: JavaScript's `String()` of an object
	/// is `[object Object]`.
	fn text(&self) -> String {
		match self {
			Attribute::Text(text) => text.clone(),
			Attribute::Named(..) => "[object Object]".to_owned(),
		}
	}

	/// The value of a text field: `{ text: object }` of a named value shows no text.
	fn label(&self) -> String {
		match self {
			Attribute::Text(text) => text.clone(),
			Attribute::Named(..) => String::new(),
		}
	}

	/// The value the first entry of a named value or the value itself holds.
	fn value(&self) -> &str {
		match self {
			Attribute::Text(text) | Attribute::Named(_, text) => text,
		}
	}
}

/// A person, system, container or component, or a boundary or deployment node.
#[derive(Clone, Debug, Default)]
pub struct Element {
	pub alias: String,
	pub label: String,
	pub descr: Option<String>,
	pub techn: Option<String>,
	/// `type` of boundaries.
	pub kind: Option<String>,
	/// `typeC4Shape` of elements: `person`, `external_system_db`, ….
	pub type_c4_shape: String,
	pub sprite: Option<String>,
	pub tags: Option<String>,
	pub link: Option<String>,
	pub bg_color: Option<String>,
	pub font_color: Option<String>,
	pub border_color: Option<String>,
	pub shape: Option<String>,
	/// `nodeType` of deployment nodes.
	pub node_type: Option<String>,
	pub parent_boundary: String,
	pub wrap: bool,
}

impl Element {
	/// `bag[key] = value` of a named value; `as_text` stores the text fields as `{ text }`. A
	/// text field set to a plain string shows nothing, as `field?.text` is undefined then.
	fn set(&mut self, key: &str, value: &str, as_text: bool) {
		let text = as_text.then(|| value.to_owned());
		match key {
			"alias" => self.alias = value.to_owned(),
			"label" => self.label = text.unwrap_or_default(),
			"descr" => self.descr = text,
			"techn" => self.techn = text,
			"type" => self.kind = text,
			"sprite" => self.sprite = Some(value.to_owned()),
			"tags" => self.tags = Some(value.to_owned()),
			"link" => self.link = Some(value.to_owned()),
			"bgColor" => self.bg_color = Some(value.to_owned()),
			"fontColor" => self.font_color = Some(value.to_owned()),
			"borderColor" => self.border_color = Some(value.to_owned()),
			"shape" => self.shape = Some(value.to_owned()),
			"nodeType" => self.node_type = Some(value.to_owned()),
			// Fields the functions set afterwards, layout fields the renderer sets, and fields
			// nothing reads.
			_ => {}
		}
	}

	/// An optional text argument: a positional value is the field's text, a named one sets
	/// its key; without one the field keeps its earlier value or becomes empty.
	fn set_text(&mut self, field: &str, value: Option<&Attribute>) {
		match value {
			Some(Attribute::Named(key, value)) => self.set(key, value, true),
			Some(Attribute::Text(text)) => self.set(field, text, true),
			None => {
				let slot = match field {
					"descr" => &mut self.descr,
					"techn" => &mut self.techn,
					_ => return,
				};
				slot.get_or_insert_with(String::new);
			}
		}
	}

	/// `assignAttributes`: the optional `sprite`, `tags` and `link` arguments.
	fn assign(&mut self, attributes: &[(&str, Option<&Attribute>)]) {
		for (field, value) in attributes {
			match value {
				Some(Attribute::Named(key, value)) => self.set(
					key,
					value,
					matches!(key.as_str(), "label" | "descr" | "techn" | "type"),
				),
				Some(Attribute::Text(text)) => self.set(field, text, false),
				None => {}
			}
		}
	}
}

/// A relationship between two elements or boundaries.
#[derive(Clone, Debug, Default)]
pub struct Rel {
	/// `rel`, `birel`, `rel_u`, … `rel_b`; anything else where `$type` replaced it.
	pub kind: String,
	pub from: String,
	pub to: String,
	pub label: String,
	/// The label was a named value: `{ text: object }`, which only C4Dynamic's numbering
	/// turns into a string (`[object Object]`).
	pub label_is_object: bool,
	pub techn: String,
	pub descr: String,
	pub text_color: Option<String>,
	pub line_color: Option<String>,
	/// `parseInt` of the offsets, `NaN` where it fails.
	pub offset_x: Option<f64>,
	pub offset_y: Option<f64>,
	pub wrap: bool,
}

impl Rel {
	fn set(&mut self, key: &str, value: &str) {
		match key {
			"label" => self.label = value.to_owned(),
			"techn" => self.techn = value.to_owned(),
			"descr" => self.descr = value.to_owned(),
			// `{ text }` where a string belongs: no known type any more.
			"type" => self.kind = "object".to_owned(),
			"from" => self.from = value.to_owned(),
			"to" => self.to = value.to_owned(),
			"textColor" => self.text_color = Some(value.to_owned()),
			"lineColor" => self.line_color = Some(value.to_owned()),
			"offsetX" => self.offset_x = Some(parse_int(value)),
			"offsetY" => self.offset_y = Some(parse_int(value)),
			_ => {}
		}
	}
}

/// The alias of the boundary that holds every top-level element.
pub const GLOBAL: &str = "global";

#[derive(Debug)]
pub struct C4Db {
	/// `C4Context`, `C4Container`, `C4Component`, `C4Dynamic` or `C4Deployment`.
	pub c4_type: String,
	pub shapes: Vec<Element>,
	/// The global boundary first.
	pub boundaries: Vec<Element>,
	pub rels: Vec<Rel>,
	pub title: String,
	pub acc_description: String,
	pub shapes_in_row: f64,
	pub boundaries_in_row: f64,
	/// The top-level `wrap` setting, which decides whether boundaries and relationships wrap.
	wrap: bool,
	boundary_stack: Vec<String>,
	current_boundary: String,
	parent_boundary: String,
}

impl C4Db {
	pub fn new(wrap: bool) -> Self {
		Self {
			c4_type: String::new(),
			shapes: Vec::new(),
			boundaries: vec![Element {
				alias: GLOBAL.to_owned(),
				label: GLOBAL.to_owned(),
				kind: Some(GLOBAL.to_owned()),
				..Element::default()
			}],
			rels: Vec::new(),
			title: String::new(),
			acc_description: String::new(),
			shapes_in_row: 4.0,
			boundaries_in_row: 2.0,
			wrap,
			boundary_stack: vec![String::new()],
			current_boundary: GLOBAL.to_owned(),
			parent_boundary: String::new(),
		}
	}

	pub fn set_title(&mut self, title: &str) {
		self.title = common::sanitize_text(title);
	}

	/// `addRel(type, from, to, label, ?techn, ?descr, ?sprite, ?tags, $link)`.
	pub fn add_rel(&mut self, kind: &str, attributes: &[Attribute]) {
		let [Some(from), Some(to), Some(label)] = [0, 1, 2].map(|index| attributes.get(index))
		else {
			return;
		};
		let (from, to) = (from.text(), to.text());
		// A named `from` is an object, which equals no other.
		let is_object = matches!(attributes[0], Attribute::Named(..))
			|| matches!(attributes[1], Attribute::Named(..));
		let index = match self.rels.iter().position(|rel| rel.from == from && rel.to == to) {
			Some(index) if !is_object => index,
			_ => {
				self.rels.push(Rel::default());
				self.rels.len() - 1
			}
		};
		let wrap = self.wrap;
		let rel = &mut self.rels[index];
		rel.kind = kind.to_owned();
		rel.from = from;
		rel.to = to;
		rel.label = label.text();
		rel.label_is_object = matches!(label, Attribute::Named(..));
		for (field, value) in [("techn", attributes.get(3)), ("descr", attributes.get(4))] {
			match value {
				Some(Attribute::Named(key, value)) => rel.set(key, value),
				Some(Attribute::Text(text)) => rel.set(field, text),
				None => {}
			}
		}
		for value in attributes.iter().skip(5).take(3) {
			if let Attribute::Named(key, value) = value {
				rel.set(key, value);
			}
		}
		rel.wrap = wrap;
	}

	/// The element or boundary with `alias` in `list`, or a new one.
	fn element(list: &mut Vec<Element>, alias: &Attribute) -> usize {
		let is_object = matches!(alias, Attribute::Named(..));
		let alias = alias.text();
		match list.iter().position(|element| element.alias == alias) {
			Some(index) if !is_object => index,
			_ => {
				list.push(Element { alias, ..Element::default() });
				list.len() - 1
			}
		}
	}

	/// `addPersonOrSystem(type, alias, label, ?descr, ?sprite, ?tags, $link)`, and with
	/// `has_techn` `addContainer`/`addComponent(type, alias, label, ?techn, ?descr, …)`.
	pub fn add_element(&mut self, type_c4_shape: &str, has_techn: bool, attributes: &[Attribute]) {
		let Some(alias) = attributes.first() else { return };
		let index = Self::element(&mut self.shapes, alias);
		let (wrap, parent) = (self.wrap, self.current_boundary.clone());
		let element = &mut self.shapes[index];
		element.label = attributes.get(1).map(Attribute::label).unwrap_or_default();
		let mut position = 2;
		if has_techn {
			element.set_text("techn", attributes.get(position));
			position += 1;
		}
		element.set_text("descr", attributes.get(position));
		element.assign(&[
			("sprite", attributes.get(position + 1)),
			("tags", attributes.get(position + 2)),
			("link", attributes.get(position + 3)),
		]);
		element.type_c4_shape = type_c4_shape.to_owned();
		element.parent_boundary = parent;
		element.wrap = wrap;
	}

	/// `addPersonOrSystemBoundary`/`addContainerBoundary(alias, label, ?type, ?tags, $link)`
	/// and `addDeploymentNode(nodeType, alias, label, ?type, ?descr, ?sprite, ?tags, $link)`.
	pub fn add_boundary(
		&mut self,
		default_type: &str,
		node_type: Option<&str>,
		attributes: &[Attribute],
	) {
		let Some(alias) = attributes.first() else { return };
		let index = Self::element(&mut self.boundaries, alias);
		let (wrap, parent) = (self.wrap, self.current_boundary.clone());
		let boundary = &mut self.boundaries[index];
		boundary.label = attributes.get(1).map(Attribute::label).unwrap_or_default();
		match attributes.get(2) {
			Some(Attribute::Named(key, value)) => boundary.set(key, value, true),
			Some(Attribute::Text(text)) => boundary.kind = Some(text.clone()),
			None => boundary.kind = Some(default_type.to_owned()),
		}
		let rest = match node_type {
			Some(_) => {
				boundary.set_text("descr", attributes.get(3));
				// The sprite argument is taken but not stored.
				&attributes[attributes.len().min(5)..]
			}
			None => &attributes[attributes.len().min(3)..],
		};
		boundary.assign(&[("tags", rest.first()), ("link", rest.get(1))]);
		if let Some(node_type) = node_type {
			boundary.node_type = Some(node_type.to_owned());
		}
		boundary.parent_boundary = parent;
		boundary.wrap = wrap;
		let alias = boundary.alias.clone();
		self.parent_boundary = std::mem::replace(&mut self.current_boundary, alias);
		self.boundary_stack.push(self.parent_boundary.clone());
	}

	/// `popBoundaryParseStack`.
	pub fn pop_boundary(&mut self) {
		self.current_boundary = self.parent_boundary.clone();
		self.boundary_stack.pop();
		self.parent_boundary = self.boundary_stack.pop().unwrap_or_default();
		self.boundary_stack.push(self.parent_boundary.clone());
	}

	/// `updateElStyle(elementName, ?bgColor, ?fontColor, ?borderColor, ?shadowing, ?shape,
	/// ?sprite, ?techn, ?legendText, ?legendSprite)`: values are stored as they are.
	pub fn update_element_style(&mut self, attributes: &[Attribute]) {
		let Some(Attribute::Text(name)) = attributes.first() else { return };
		let element = match self.shapes.iter_mut().find(|element| element.alias == *name) {
			Some(element) => element,
			None => match self.boundaries.iter_mut().find(|element| element.alias == *name) {
				Some(element) => element,
				None => return,
			},
		};
		const FIELDS: [&str; 9] = [
			"bgColor",
			"fontColor",
			"borderColor",
			"shadowing",
			"shape",
			"sprite",
			"techn",
			"legendText",
			"legendSprite",
		];
		for (field, value) in FIELDS.iter().zip(&attributes[1..]) {
			match value {
				Attribute::Named(key, value) => element.set(key, value, false),
				Attribute::Text(text) => element.set(field, text, false),
			}
		}
	}

	/// `updateRelStyle(from, to, ?textColor, ?lineColor, ?offsetX, ?offsetY)`.
	pub fn update_rel_style(&mut self, attributes: &[Attribute]) {
		let (Some(Attribute::Text(from)), Some(Attribute::Text(to))) =
			(attributes.first(), attributes.get(1))
		else {
			return;
		};
		let Some(rel) = self.rels.iter_mut().find(|rel| rel.from == *from && rel.to == *to) else {
			return;
		};
		const FIELDS: [&str; 4] = ["textColor", "lineColor", "offsetX", "offsetY"];
		for (field, value) in FIELDS.iter().zip(&attributes[2..]) {
			match value {
				Attribute::Named(key, value) => rel.set(key, value),
				Attribute::Text(text) => rel.set(field, text),
			}
		}
	}

	/// `updateLayoutConfig(?c4ShapeInRow, ?c4BoundaryInRow)`.
	pub fn update_layout_config(&mut self, attributes: &[Attribute]) {
		let value = |index: usize| attributes.get(index).map_or(f64::NAN, |a| parse_int(a.value()));
		let (shapes, boundaries) = (value(0), value(1));
		if shapes >= 1.0 {
			self.shapes_in_row = shapes;
		}
		if boundaries >= 1.0 {
			self.boundaries_in_row = boundaries;
		}
	}

	/// `getC4Shape`: the element with `alias`, else the boundary.
	pub fn find(&self, alias: &str) -> Option<Endpoint> {
		match self.shapes.iter().position(|shape| shape.alias == alias) {
			Some(index) => Some(Endpoint::Shape(index)),
			None => self
				.boundaries
				.iter()
				.position(|boundary| boundary.alias == alias)
				.map(Endpoint::Boundary),
		}
	}
}

/// An end of a relationship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
	Shape(usize),
	Boundary(usize),
}

/// JavaScript's `parseInt` without a radix: an optional sign and decimal digits, or `0x` and
/// hexadecimal ones, after leading whitespace; `NaN` without digits.
pub fn parse_int(text: &str) -> f64 {
	let text = text.trim_start();
	let (sign, unsigned) = match text.as_bytes().first() {
		Some(b'-') => (-1.0, &text[1..]),
		Some(b'+') => (1.0, &text[1..]),
		_ => (1.0, text),
	};
	let (radix, digits) = match unsigned.get(..2) {
		Some("0x" | "0X") => (16, &unsigned[2..]),
		_ => (10, unsigned),
	};
	let length = digits.find(|character: char| !character.is_digit(radix)).unwrap_or(digits.len());
	if length == 0 {
		return f64::NAN;
	}
	let value = digits[..length].chars().fold(0.0, |value, digit| {
		value * f64::from(radix) + f64::from(digit.to_digit(radix).unwrap_or(0))
	});
	sign * value
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_integers_like_javascript() {
		assert_eq!(parse_int(" -50px"), -50.0);
		assert_eq!(parse_int("0x1f"), 31.0);
		assert_eq!(parse_int("1e3"), 1.0);
		assert!(parse_int("abc").is_nan());
		assert!(parse_int("").is_nan());
	}

	fn text(value: &str) -> Attribute {
		Attribute::Text(value.to_owned())
	}

	#[test]
	fn merges_and_styles_elements_like_mermaid() {
		let mut db = C4Db::new(false);
		db.add_element("person", false, &[text("a"), text("A"), text("first")]);
		db.add_element(
			"system",
			false,
			&[text("a"), text("A2"), Attribute::Named("bgColor".into(), "red".into())],
		);
		let element = &db.shapes[0];
		assert_eq!(db.shapes.len(), 1);
		assert_eq!(element.label, "A2");
		// A named value in the description slot sets its key and keeps the description.
		assert_eq!(element.descr.as_deref(), Some("first"));
		assert_eq!(element.bg_color.as_deref(), Some("red"));
		assert_eq!(element.type_c4_shape, "system");
		db.update_element_style(&[
			text("a"),
			text("blue"),
			Attribute::Named("techn".into(), "x".into()),
		]);
		assert_eq!(db.shapes[0].bg_color.as_deref(), Some("blue"));
		// A plain string where `{ text }` belongs shows nothing.
		assert_eq!(db.shapes[0].techn, None);
	}

	#[test]
	fn tracks_boundaries_like_mermaid() {
		let mut db = C4Db::new(false);
		db.add_boundary("system", None, &[text("b0"), text("B0"), text("ENTERPRISE")]);
		db.add_element("person", false, &[text("p"), text("P")]);
		db.add_boundary("node", Some("node"), &[text("n"), text("N")]);
		db.pop_boundary();
		db.pop_boundary();
		db.add_element("system", false, &[text("s"), text("S")]);
		assert_eq!(db.shapes[0].parent_boundary, "b0");
		assert_eq!(db.boundaries[2].parent_boundary, "b0");
		assert_eq!(db.boundaries[2].kind.as_deref(), Some("node"));
		assert_eq!(db.shapes[1].parent_boundary, GLOBAL);
	}

	#[test]
	fn updates_relationships_and_layout() {
		let mut db = C4Db::new(false);
		db.add_rel("rel", &[text("a"), text("b")]);
		assert!(db.rels.is_empty());
		db.add_rel("rel", &[text("a"), text("b"), text("uses"), text("HTTP")]);
		db.add_rel("birel", &[text("a"), text("b"), text("calls")]);
		assert_eq!(db.rels.len(), 1);
		assert_eq!((db.rels[0].kind.as_str(), db.rels[0].label.as_str()), ("birel", "calls"));
		assert_eq!(db.rels[0].techn, "HTTP");
		db.update_rel_style(&[text("a"), text("b"), text("red"), text("blue"), text("5")]);
		assert_eq!(db.rels[0].offset_x, Some(5.0));
		db.update_layout_config(&[Attribute::Named("c4ShapeInRow".into(), "2".into()), text("0")]);
		assert_eq!((db.shapes_in_row, db.boundaries_in_row), (2.0, 2.0));
	}
}
