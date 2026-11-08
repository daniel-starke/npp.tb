//! The requirements, elements, relationships and classes of a requirement diagram
//! (`requirementDb.ts`).

use std::collections::HashMap;

/// What the body of a requirement or element sets, before the definition adds it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fields {
	/// Requirement: id, text, risk and verification method; element: type and document
	/// reference.
	pub values: [String; 4],
}

/// A requirement (`kind` is its type) or an element (`kind` is `None`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
	pub name: String,
	pub requirement_type: Option<&'static str>,
	pub fields: Fields,
	pub css_styles: Vec<String>,
	/// `default` and the classes of `class` statements and `:::`.
	pub classes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
	pub kind: &'static str,
	pub src: String,
	pub dst: String,
}

#[derive(Clone, Debug, Default)]
pub struct RequirementDb {
	/// The requirements first, then the elements, each in the order of their definitions.
	pub requirements: Vec<Item>,
	pub elements: Vec<Item>,
	requirement_index: HashMap<String, usize>,
	element_index: HashMap<String, usize>,
	pub relations: Vec<Relation>,
	/// The styles of each class.
	classes: Vec<(String, Vec<String>)>,
	pub direction: Option<&'static str>,
}

impl RequirementDb {
	/// `addRequirement`: a requirement is defined once, later definitions are dropped.
	pub fn add_requirement(&mut self, name: &str, requirement_type: &'static str, fields: Fields) {
		if !self.requirement_index.contains_key(name) {
			self.requirement_index.insert(name.to_owned(), self.requirements.len());
			self.requirements.push(Item {
				name: name.to_owned(),
				requirement_type: Some(requirement_type),
				fields,
				css_styles: Vec::new(),
				classes: vec!["default".to_owned()],
			});
		}
	}

	/// `addElement`: like requirements.
	pub fn add_element(&mut self, name: &str, fields: Fields) {
		if !self.element_index.contains_key(name) {
			self.element_index.insert(name.to_owned(), self.elements.len());
			self.elements.push(Item {
				name: name.to_owned(),
				requirement_type: None,
				fields,
				css_styles: Vec::new(),
				classes: vec!["default".to_owned()],
			});
		}
	}

	pub fn add_relationship(&mut self, kind: &'static str, src: &str, dst: &str) {
		self.relations.push(Relation { kind, src: src.to_owned(), dst: dst.to_owned() });
	}

	/// The requirement of `name`, else the element.
	fn item_mut(&mut self, name: &str) -> Option<&mut Item> {
		match self.requirement_index.get(name) {
			Some(&index) => Some(&mut self.requirements[index]),
			None => self.element_index.get(name).map(|&index| &mut self.elements[index]),
		}
	}

	/// `setCssStyle`: like Mermaid, an unknown id ends the statement.
	pub fn set_css_style(&mut self, ids: &[String], styles: &[String]) {
		for id in ids {
			let Some(item) = self.item_mut(id) else { return };
			item.css_styles.extend(styles.iter().cloned());
		}
	}

	/// `setClass`: the classes, and the styles their `classDef` has so far.
	pub fn set_class(&mut self, ids: &[String], classes: &[String]) {
		for id in ids {
			let styles: Vec<Vec<String>> = classes
				.iter()
				.map(|class| {
					self.classes
						.iter()
						.find(|(known, _)| known == class)
						.map(|(_, styles)| styles.clone())
						.unwrap_or_default()
				})
				.collect();
			let Some(item) = self.item_mut(id) else { continue };
			for (class, styles) in classes.iter().zip(styles) {
				item.classes.push(class.clone());
				item.css_styles.extend(styles);
			}
		}
	}

	/// `defineClass`: the styles go to the class and to every item that has it already.
	pub fn define_class(&mut self, ids: &[String], styles: &[String]) {
		for id in ids {
			match self.classes.iter_mut().find(|(known, _)| known == id) {
				Some((_, known)) => known.extend(styles.iter().cloned()),
				None => self.classes.push((id.clone(), styles.to_vec())),
			}
			for item in self.requirements.iter_mut().chain(self.elements.iter_mut()) {
				if item.classes.contains(id) {
					item.css_styles.extend(styles.iter().cloned());
				}
			}
		}
	}

	/// Whether a requirement or an element is called `name`.
	pub fn knows(&self, name: &str) -> bool {
		self.requirement_index.contains_key(name) || self.element_index.contains_key(name)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn strings(items: &[&str]) -> Vec<String> {
		items.iter().map(|item| (*item).to_owned()).collect()
	}

	#[test]
	fn applies_classes_and_styles_like_mermaid() {
		let mut db = RequirementDb::default();
		db.add_requirement("a", "Requirement", Fields::default());
		db.add_requirement("a", "Design Constraint", Fields::default());
		db.add_element("e", Fields::default());
		db.define_class(&strings(&["default"]), &strings(&["fill:pink"]));
		db.define_class(&strings(&["blue"]), &strings(&["stroke:blue"]));
		db.set_class(&strings(&["a"]), &strings(&["blue"]));
		db.set_css_style(&strings(&["x", "e"]), &strings(&["color:red"]));
		assert_eq!(db.requirements.len(), 1);
		assert_eq!(db.requirements[0].requirement_type, Some("Requirement"));
		assert_eq!(db.requirements[0].css_styles, ["fill:pink", "stroke:blue"]);
		assert_eq!(db.requirements[0].classes, ["default", "blue"]);
		// The unknown `x` ends the `style` statement before `e`.
		assert_eq!(db.elements[0].css_styles, ["fill:pink"]);
	}
}
