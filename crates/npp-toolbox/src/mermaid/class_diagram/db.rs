//! The classes, members, relations, notes and namespaces of a class diagram (`classDb.ts`,
//! `classTypes.ts`).

use std::collections::HashMap;

use super::super::common::{parse_generic_types, sanitize_url};

/// The `relationType` of a relation end, `None` for no marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationType {
	Aggregation,
	Extension,
	Composition,
	Dependency,
	Lollipop,
}

impl RelationType {
	/// `getArrowMarker`.
	pub fn arrow_marker(relation_type: Option<RelationType>) -> &'static str {
		match relation_type {
			Some(RelationType::Aggregation) => "aggregation",
			Some(RelationType::Extension) => "extension",
			Some(RelationType::Composition) => "composition",
			Some(RelationType::Dependency) => "dependency",
			Some(RelationType::Lollipop) => "lollipop",
			None => "none",
		}
	}
}

/// The classifier of a member: `*` abstract (italic), `$` static (underlined).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Classifier {
	#[default]
	None,
	Abstract,
	Static,
}

/// `ClassMember`: an attribute or method with its display text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Member {
	/// The text of the label: the visibility escaped for Markdown, generics as entities.
	pub text: String,
	pub classifier: Classifier,
}

/// Members whose parsing fails in Mermaid, which then fails to render the diagram.
#[derive(Debug, PartialEq, Eq)]
pub struct InvalidMember;

const VISIBILITIES: &str = "#+~-";

fn classifier_of(character: Option<char>) -> Classifier {
	match character {
		Some('*') => Classifier::Abstract,
		Some('$') => Classifier::Static,
		_ => Classifier::None,
	}
}

/// The parts of `/([#+~-])?(.+)\((.*)\)([\s$*])?(.*)([$*])?/`: visibility, id, parameters, the
/// character after `)` and the rest. `.+` is greedy, so the parameters are in the last `(` before
/// the last `)`.
fn method_parts(input: &str) -> Option<(&str, &str, &str, &str, &str)> {
	let close = input.rfind(')')?;
	let open = input[..close].rfind('(')?;
	let has_visibility = input.starts_with(|c| VISIBILITIES.contains(c));
	// The id needs a character; without one after the visibility, the visibility is the id.
	let (visibility, id_start) = match has_visibility && open > 1 {
		true => (&input[..1], 1),
		false => ("", 0),
	};
	if open == 0 {
		return None;
	}
	let after = &input[close + 1..];
	let marker_length = after
		.chars()
		.next()
		.filter(|&c| c.is_whitespace() || c == '$' || c == '*')
		.map_or(0, char::len_utf8);
	Some((
		visibility,
		&input[id_start..open],
		&input[open + 1..close],
		&after[..marker_length],
		&after[marker_length..],
	))
}

impl Member {
	/// `new ClassMember(input, memberType)` and its `parseMember`.
	pub fn parse(input: &str, is_method: bool) -> Result<Member, InvalidMember> {
		let mut visibility = "";
		let mut classifier = None;
		let id;
		let mut parameters = "";
		let mut return_type = String::new();
		if is_method {
			// Without a match the id stays undefined and Mermaid fails with a TypeError.
			let (found_visibility, found_id, found_parameters, marker, rest) =
				method_parts(input).ok_or(InvalidMember)?;
			visibility = found_visibility;
			id = found_id;
			parameters = found_parameters.trim();
			let marker = marker.trim();
			return_type = rest.trim().to_owned();
			if marker.is_empty() {
				if return_type.ends_with(['$', '*']) {
					classifier = return_type.pop();
				}
			} else {
				classifier = marker.chars().next();
			}
		} else {
			if input.starts_with(|c| VISIBILITIES.contains(c)) {
				visibility = &input[..1];
			}
			let mut end = input.len();
			if input.ends_with(['$', '*']) {
				classifier = input.chars().next_back();
				end -= 1;
			}
			id = input.get(visibility.len()..end).unwrap_or("");
		}
		// One leading space is kept.
		let id = match id.starts_with(' ') {
			true => format!(" {}", id.trim()),
			false => id.trim().to_owned(),
		};
		let mut combined = String::new();
		if !visibility.is_empty() {
			combined.push('\\');
			combined.push_str(visibility);
		}
		combined.push_str(&parse_generic_types(&id));
		if is_method {
			combined.push('(');
			combined.push_str(&parse_generic_types(parameters));
			combined.push(')');
			if !return_type.is_empty() {
				combined.push_str(" : ");
				combined.push_str(&parse_generic_types(&return_type));
			}
		}
		let mut text = combined.replace('<', "&lt;").replace('>', "&gt;");
		if text.starts_with("\\&lt;") {
			text.replace_range(.."\\&lt;".len(), "~");
		}
		Ok(Member { text, classifier: classifier_of(classifier) })
	}
}

#[derive(Clone, Debug, Default)]
pub struct ClassNode {
	pub id: String,
	/// The generic type of `Name~Type~`.
	pub type_parameter: String,
	/// The text of the title.
	pub text: String,
	/// `default` and the classes of `:::`, `cssClass` and `click`.
	pub css_classes: String,
	pub members: Vec<Member>,
	pub methods: Vec<Member>,
	pub annotations: Vec<String>,
	pub styles: Vec<String>,
	pub dom_id: String,
	pub parent: Option<String>,
	pub link: Option<String>,
	pub link_target: Option<String>,
	pub tooltip: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Note {
	pub id: String,
	/// The class of `note for`.
	pub class: Option<String>,
	pub text: String,
	pub parent: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Relation {
	pub id1: String,
	pub id2: String,
	/// The cardinalities at the start and the end, empty without one.
	pub title1: String,
	pub title2: String,
	pub title: Option<String>,
	pub type1: Option<RelationType>,
	pub type2: Option<RelationType>,
	pub is_dotted: bool,
}

/// An interface of a lollipop relation, drawn as an invisible node.
#[derive(Clone, Debug)]
pub struct Interface {
	pub id: String,
	pub label: String,
}

#[derive(Clone, Debug)]
pub struct Namespace {
	pub id: String,
	pub label: String,
	pub parent: Option<String>,
	/// Declared by the document, not only an ancestor of a dotted name.
	pub explicit: bool,
}

/// What the grammar reads of a relation.
#[derive(Clone, Debug, Default)]
pub struct RelationStatement {
	pub id1: String,
	pub id2: String,
	pub title1: Option<String>,
	pub title2: Option<String>,
	pub type1: Option<RelationType>,
	pub type2: Option<RelationType>,
	pub is_dotted: bool,
	pub title: Option<String>,
}

#[derive(Debug, Default)]
pub struct ClassDb {
	pub classes: Vec<ClassNode>,
	class_index: HashMap<String, usize>,
	pub relations: Vec<Relation>,
	pub notes: Vec<Note>,
	pub interfaces: Vec<Interface>,
	pub namespaces: Vec<Namespace>,
	namespace_index: HashMap<String, usize>,
	namespace_stack: Vec<String>,
	pub direction: Option<&'static str>,
}

/// `splitClassNameAndType`: `Name~Type~` is the class `Name` of the generic type `Type`.
fn split_class_name_and_type(id: &str) -> (&str, &str) {
	match id.find('~') {
		Some(index) if index > 0 => {
			let mut parts = id.split('~');
			(parts.next().unwrap_or(""), parts.next().unwrap_or(""))
		}
		_ => (id, ""),
	}
}

/// `cleanupLabel`: the text of a `: label` without the colon.
pub fn cleanup_label(label: &str) -> String {
	label.strip_prefix(':').unwrap_or(label).trim().to_owned()
}

impl ClassDb {
	pub fn class(&self, id: &str) -> Option<&ClassNode> {
		self.class_index.get(id).map(|&index| &self.classes[index])
	}

	fn class_mut(&mut self, id: &str) -> Option<&mut ClassNode> {
		self.class_index.get(id).map(|&index| &mut self.classes[index])
	}

	pub fn add_class(&mut self, id: &str) {
		let (name, type_parameter) = split_class_name_and_type(id);
		if self.class_index.contains_key(name) {
			return;
		}
		let text = match type_parameter.is_empty() {
			true => name.to_owned(),
			false => format!("{name}&lt;{type_parameter}&gt;"),
		};
		self.class_index.insert(name.to_owned(), self.classes.len());
		self.classes.push(ClassNode {
			id: name.to_owned(),
			type_parameter: type_parameter.to_owned(),
			text,
			css_classes: "default".to_owned(),
			dom_id: format!("classId-{name}-{}", self.classes.len()),
			..ClassNode::default()
		});
	}

	/// `setClassLabel`: the generic type follows as raw `<Type>`, which HTML labels take for a
	/// tag.
	pub fn set_class_label(&mut self, id: &str, label: &str) {
		let (name, _) = split_class_name_and_type(id);
		if let Some(class) = self.class_mut(name) {
			class.text = match class.type_parameter.is_empty() {
				true => label.to_owned(),
				false => format!("{label}<{}>", class.type_parameter),
			};
		}
	}

	pub fn add_relation(&mut self, statement: RelationStatement) {
		let RelationStatement { mut id1, mut id2, type1, type2, .. } = statement;
		let is_lollipop = |relation_type| relation_type == Some(RelationType::Lollipop);
		// The other end of a lollipop must have no marker.
		if is_lollipop(type1) && type2.is_none() {
			self.add_class(&id2);
			self.interfaces.push(Interface { id: String::new(), label: id1 });
			id1 = self.interface_id();
		} else if is_lollipop(type2) && type1.is_none() {
			self.add_class(&id1);
			self.interfaces.push(Interface { id: String::new(), label: id2 });
			id2 = self.interface_id();
		} else {
			self.add_class(&id1);
			self.add_class(&id2);
		}
		let title = |value: Option<String>| value.map(|text| text.trim().to_owned());
		let none_to_empty =
			|value: Option<String>| title(value).filter(|text| text != "none").unwrap_or_default();
		self.relations.push(Relation {
			id1: split_class_name_and_type(&id1).0.to_owned(),
			id2: split_class_name_and_type(&id2).0.to_owned(),
			title1: none_to_empty(statement.title1),
			title2: none_to_empty(statement.title2),
			title: statement.title,
			type1,
			type2,
			is_dotted: statement.is_dotted,
		});
	}

	/// The id of the interface added last.
	fn interface_id(&mut self) -> String {
		let index = self.interfaces.len() - 1;
		let id = format!("interface{index}");
		self.interfaces[index].id = id.clone();
		id
	}

	/// `addAnnotation`; Mermaid fails with a TypeError for classes it does not know.
	pub fn add_annotation(&mut self, class_name: &str, annotation: &str) -> bool {
		let (name, _) = split_class_name_and_type(class_name);
		match self.class_mut(name) {
			Some(class) => {
				class.annotations.push(annotation.to_owned());
				true
			}
			None => false,
		}
	}

	/// `addMember`: `<<…>>` is an annotation, text with `)` after its start a method, other text
	/// an attribute.
	pub fn add_member(&mut self, class_name: &str, member: &str) -> Result<(), InvalidMember> {
		self.add_class(class_name);
		let (name, _) = split_class_name_and_type(class_name);
		let member = member.trim();
		let Some(class) = self.class_mut(name) else { return Ok(()) };
		if member.starts_with("<<") && member.ends_with(">>") && member.len() >= 4 {
			class.annotations.push(member[2..member.len() - 2].to_owned());
		} else if member.find(')').is_some_and(|index| index > 0) {
			class.methods.push(Member::parse(member, true)?);
		} else if !member.is_empty() {
			class.members.push(Member::parse(member, false)?);
		}
		Ok(())
	}

	pub fn add_note(&mut self, text: &str, class_name: Option<&str>) -> String {
		let id = format!("note{}", self.notes.len());
		self.notes.push(Note {
			id: id.clone(),
			class: class_name.map(str::to_owned),
			text: text.to_owned(),
			parent: None,
		});
		id
	}

	/// `setCssClass`: ids starting with a digit get the DOM id prefix and so match no class.
	pub fn set_css_class(&mut self, ids: &str, class_name: &str) {
		for id in ids.split(',') {
			if id.starts_with(|c: char| c.is_ascii_digit()) {
				continue;
			}
			let (name, _) = split_class_name_and_type(id);
			if let Some(class) = self.class_mut(name) {
				class.css_classes.push(' ');
				class.css_classes.push_str(class_name);
			}
		}
	}

	/// `defineClass`: the styles go to the classes that have the class so far, where Mermaid
	/// compares the class list as text, so `classDef def` matches every `default` class.
	pub fn define_class(&mut self, ids: &[String], styles: &[String]) {
		for id in ids {
			for class in &mut self.classes {
				if class.css_classes.contains(id.as_str()) {
					class.styles.extend(
						styles.iter().flat_map(|style| style.split(',')).map(str::to_owned),
					);
				}
			}
		}
	}

	pub fn set_tooltip(&mut self, ids: &str, tooltip: &str) {
		for id in ids.split(',') {
			let (name, _) = split_class_name_and_type(id);
			if let Some(class) = self.class_mut(name) {
				class.tooltip = Some(tooltip.to_owned());
			}
		}
	}

	pub fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
		for id in ids.split(',') {
			if id.starts_with(|c: char| c.is_ascii_digit()) {
				continue;
			}
			let (name, _) = split_class_name_and_type(id);
			if let Some(class) = self.class_mut(name) {
				class.link = Some(sanitize_url(link));
				class.link_target = Some(target.unwrap_or("_blank").to_owned());
			}
		}
		self.set_css_class(ids, "clickable");
	}

	/// `setClickEvent`: callbacks need `securityLevel: loose`, which documents cannot set, so
	/// only the class remains.
	pub fn set_click_event(&mut self, ids: &str) {
		self.set_css_class(ids, "clickable");
	}

	pub fn set_css_style(&mut self, id: &str, styles: &[String]) {
		if let Some(class) = self.class_mut(id) {
			class
				.styles
				.extend(styles.iter().flat_map(|style| style.split(',')).map(str::to_owned));
		}
	}

	/// `addNamespace`: a dotted name adds its ancestors; returns the qualified id.
	pub fn add_namespace(&mut self, id: &str, label: Option<&str>) -> String {
		let qualified = match self.namespace_stack.last() {
			Some(prefix) => format!("{prefix}.{id}"),
			None => id.to_owned(),
		};
		self.namespace_stack.push(qualified.clone());
		if let Some(&index) = self.namespace_index.get(&qualified) {
			let namespace = &mut self.namespaces[index];
			namespace.explicit = true;
			if let Some(label) = label.filter(|label| !label.is_empty()) {
				namespace.label = label.to_owned();
			}
			return qualified;
		}
		let parts: Vec<&str> = qualified.split('.').collect();
		let mut ancestor = String::new();
		for (index, part) in parts.iter().enumerate() {
			let parent = (index > 0).then(|| ancestor.clone());
			if index > 0 {
				ancestor.push('.');
			}
			ancestor.push_str(part);
			let is_leaf = index + 1 == parts.len();
			match self.namespace_index.get(&ancestor) {
				Some(&existing) => {
					if is_leaf {
						self.namespaces[existing].explicit = true;
					}
				}
				None => {
					let label = match (is_leaf, label.filter(|label| !label.is_empty())) {
						(true, Some(label)) => label.to_owned(),
						_ => (*part).to_owned(),
					};
					self.namespace_index.insert(ancestor.clone(), self.namespaces.len());
					self.namespaces.push(Namespace {
						id: ancestor.clone(),
						label,
						parent: parent.clone(),
						explicit: is_leaf,
					});
				}
			}
			// `linkParentChild` keeps the first parent.
			if let (Some(parent), Some(&child)) = (parent, self.namespace_index.get(&ancestor))
				&& self.namespaces[child].parent.is_none()
			{
				self.namespaces[child].parent = Some(parent);
			}
		}
		qualified
	}

	pub fn pop_namespace(&mut self) {
		self.namespace_stack.pop();
	}

	pub fn add_classes_to_namespace(&mut self, id: &str, classes: &[String], notes: &[String]) {
		if !self.namespace_index.contains_key(id) {
			return;
		}
		for class_name in classes {
			let (name, _) = split_class_name_and_type(class_name);
			if let Some(class) = self.class_mut(name) {
				class.parent = Some(id.to_owned());
			}
		}
		for note_id in notes {
			if let Some(note) = self.notes.iter_mut().find(|note| note.id == *note_id) {
				note.parent = Some(id.to_owned());
			}
		}
	}

	pub fn namespace(&self, id: &str) -> Option<&Namespace> {
		self.namespace_index.get(id).map(|&index| &self.namespaces[index])
	}

	/// `resolveExplicitAncestor`: the nearest declared namespace of `id` or its ancestors.
	pub fn explicit_ancestor(&self, id: Option<&str>) -> Option<String> {
		let mut current = id;
		// A parent chain is acyclic; the bound only guards against surprises.
		for _ in 0..=self.namespaces.len() {
			let namespace = self.namespace(current?)?;
			if namespace.explicit {
				return Some(namespace.id.clone());
			}
			current = namespace.parent.as_deref();
		}
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn member(input: &str, is_method: bool) -> Member {
		Member::parse(input, is_method).unwrap()
	}

	#[test]
	fn parses_members_like_class_member() {
		let static_method = member("+someMethod(List~int~ ids)$ bool", true);
		assert_eq!(static_method.text, "\\+someMethod(List&lt;int&gt; ids) : bool");
		assert_eq!(static_method.classifier, Classifier::Static);
		let abstract_method = member("#run() int*", true);
		assert_eq!(abstract_method.text, "\\#run() : int");
		assert_eq!(abstract_method.classifier, Classifier::Abstract);
		assert_eq!(member("-List~string~ names$", false).text, "\\-List&lt;string&gt; names");
		assert_eq!(member("-List~string~ names$", false).classifier, Classifier::Static);
		assert_eq!(member("String  name", false).text, "String  name");
		// The parameters are in the last parentheses.
		assert_eq!(member("f(a)(b)", true).text, "f(a)(b)");
		assert_eq!(member("+(x)", true).text, "+(x)");
		assert_eq!(Member::parse("(x)", true), Err(InvalidMember));
	}

	#[test]
	fn keeps_namespaces_and_classes() {
		let mut db = ClassDb::default();
		assert_eq!(db.add_namespace("A.B", None), "A.B");
		assert_eq!(db.add_namespace("C", Some("Label")), "A.B.C");
		db.pop_namespace();
		db.pop_namespace();
		assert_eq!(db.namespaces.len(), 3);
		assert!(!db.namespaces[0].explicit);
		assert_eq!(db.namespaces[2].parent.as_deref(), Some("A.B"));
		assert_eq!(db.namespaces[2].label, "Label");
		assert_eq!(db.explicit_ancestor(Some("A.B.C")).as_deref(), Some("A.B.C"));
		db.add_class("Square~Shape~");
		db.add_class("Square");
		assert_eq!(db.classes.len(), 1);
		assert_eq!(db.classes[0].text, "Square&lt;Shape&gt;");
		db.set_css_class("Square", "x");
		db.define_class(&["def".to_owned()], &["fill:#f00,stroke:#00f".to_owned()]);
		assert_eq!(db.classes[0].styles, ["fill:#f00", "stroke:#00f"]);
	}
}
