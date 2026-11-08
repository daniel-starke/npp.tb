//! The state database (`stateDb.ts`) and its conversion into nodes and edges (`dataFetcher.ts`).

use std::collections::HashMap;

use crate::mermaid::Look;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateType {
	Default,
	Fork,
	Join,
	Choice,
	Divider,
}

impl StateType {
	fn shape(self) -> &'static str {
		match self {
			StateType::Default => SHAPE_STATE,
			StateType::Fork => "fork",
			StateType::Join => "join",
			StateType::Choice => "choice",
			StateType::Divider => SHAPE_DIVIDER,
		}
	}
}

/// A description: a text, or the pair of `state "A" as B:C`.
#[derive(Clone, Debug, PartialEq)]
pub enum Description {
	Text(String),
	Pair(String, String),
}

impl Description {
	fn is_empty(&self) -> bool {
		matches!(self, Description::Text(text) if text.is_empty())
	}

	/// The first string, which labels a node.
	fn first(&self) -> &str {
		match self {
			Description::Text(text) | Description::Pair(text, _) => text,
		}
	}

	/// `sanitizeTextOrArray` flattens the descriptions of a node.
	fn flatten(&self) -> Vec<Description> {
		match self {
			Description::Text(_) => vec![self.clone()],
			Description::Pair(first, second) => {
				vec![Description::Text(first.clone()), Description::Text(second.clone())]
			}
		}
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
	pub is_left: bool,
	pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateStmt {
	pub id: String,
	/// `None` for the state of a note statement, which has no type.
	pub kind: Option<StateType>,
	pub description: Option<Description>,
	pub doc: Option<Vec<Stmt>>,
	pub note: Option<Note>,
	/// `[*]`: `Some(true)` as the start of a transition, `Some(false)` as its end.
	pub start: Option<bool>,
	pub classes: Vec<String>,
}

impl StateStmt {
	pub fn new(id: String, kind: Option<StateType>) -> Self {
		Self {
			id,
			kind,
			description: None,
			doc: None,
			note: None,
			start: None,
			classes: Vec::new(),
		}
	}

	fn has_description(&self) -> bool {
		self.description.as_ref().is_some_and(|description| !description.is_empty())
	}
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
	State(StateStmt),
	Relation {
		state1: StateStmt,
		state2: StateStmt,
		description: Option<String>,
	},
	ClassDef {
		id: String,
		classes: String,
	},
	Style {
		id: String,
		style_class: String,
	},
	ApplyClass {
		id: String,
		style_class: String,
	},
	Direction(String),
	Click {
		id: String,
		url: String,
		tooltip: String,
	},
	/// Statements that only have side effects in the parser or none (`hide empty description`,
	/// `scale`, floating notes, accessibility).
	Ignored,
}

pub const SHAPE_STATE: &str = "rect";
pub const SHAPE_STATE_WITH_DESC: &str = "rectWithTitle";
pub const SHAPE_START: &str = "stateStart";
pub const SHAPE_END: &str = "stateEnd";
pub const SHAPE_DIVIDER: &str = "divider";
pub const SHAPE_GROUP: &str = "roundedWithTitle";
pub const SHAPE_NOTE: &str = "note";
pub const SHAPE_NOTEGROUP: &str = "noteGroup";

/// The label of a node: a text, or a title with descriptions.
#[derive(Clone, Debug, PartialEq)]
enum Label {
	Text(Description),
	List(Vec<Description>),
}

/// A node for the layout (`NodeData`).
#[derive(Clone, Debug, PartialEq)]
pub struct NodeData {
	pub id: String,
	pub dom_id: String,
	/// `None` for a state that only a note statement introduced.
	pub shape: Option<String>,
	pub label: String,
	/// The rows below the title of `rectWithTitle`.
	pub description: Vec<String>,
	pub css_classes: String,
	pub css_compiled_styles: Vec<String>,
	pub css_styles: Vec<String>,
	pub dir: Option<String>,
	pub is_group: bool,
	pub color_index: Option<usize>,
	pub padding: Option<f64>,
	/// `rx`/`ry` of state rectangles.
	pub is_rounded: bool,
	pub parent_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgeData {
	pub id: String,
	pub start: String,
	pub end: String,
	pub label: String,
	pub is_note_edge: bool,
}

/// What `nodeDb` keeps of a state across its statements.
struct KnownNode {
	shape: Option<String>,
	description: Label,
	css_classes: String,
	css_styles: Vec<String>,
	is_group: bool,
	dir: Option<String>,
	color_index: Option<usize>,
}

/// The classes and styles a state got from statements of the root document.
#[derive(Default)]
struct StateInfo {
	classes: Vec<String>,
	styles: Vec<String>,
}

#[derive(Default)]
pub struct StyleClass {
	pub styles: Vec<String>,
}

#[derive(Default)]
pub struct StateDb {
	pub nodes: Vec<NodeData>,
	pub edges: Vec<EdgeData>,
	/// Clickable states: the id, URL and tooltip.
	pub links: Vec<(String, String, String)>,
	pub direction: String,
	states: Vec<(String, StateInfo)>,
	classes: Vec<(String, StyleClass)>,
	known_nodes: HashMap<String, KnownNode>,
	graph_item_count: usize,
	next_color_index: usize,
	container_color_index: HashMap<String, Option<usize>>,
	generated_ids: usize,
}

impl StateDb {
	/// `setRootDoc` with `extract(getRootDocV2())` and `dataFetcher`.
	pub fn new(mut root: Vec<Stmt>, look: Look) -> Result<Self, String> {
		let mut db = StateDb::default();
		db.translate_document("root", &mut root, 0);
		db.direction = root
			.iter()
			.filter_map(|statement| match statement {
				Stmt::Direction(dir) => Some(dir.clone()),
				_ => None,
			})
			.next_back()
			.unwrap_or_else(|| "TB".to_owned());
		for statement in &root {
			db.extract(statement);
		}
		for statement in &root {
			db.fetch_statement(None, statement, false, look, 0);
		}
		for node in &db.nodes {
			if node.is_group && !node.description.is_empty() {
				return Err(format!(
					"Group nodes can only have label. Remove the additional description for node [{}]",
					node.id
				));
			}
		}
		Ok(db)
	}

	/// `docTranslator` of a document whose parent has `parent_id`.
	fn translate_document(&mut self, parent_id: &str, doc: &mut Vec<Stmt>, depth: usize) {
		let is_divider = |statement: &Stmt| matches!(statement, Stmt::State(state) if state.kind == Some(StateType::Divider));
		let region_count = doc.iter().filter(|statement| is_divider(statement)).count();
		let has_trailing =
			doc.iter().rposition(is_divider).is_some_and(|last| last + 1 < doc.len());
		// Only when statements follow the last divider, the document becomes its regions.
		if region_count > 0 && has_trailing {
			let mut regions = Vec::new();
			let mut current = Vec::new();
			for statement in doc.drain(..) {
				match statement {
					Stmt::State(mut divider) if divider.kind == Some(StateType::Divider) => {
						divider.doc = Some(std::mem::take(&mut current));
						regions.push(Stmt::State(divider));
					}
					other => current.push(other),
				}
			}
			self.generated_ids += 1;
			let mut tail = StateStmt::new(
				format!("divider-id-end-{}", self.generated_ids),
				Some(StateType::Divider),
			);
			tail.doc = Some(current);
			regions.push(Stmt::State(tail));
			*doc = regions;
		}
		for statement in doc {
			match statement {
				Stmt::Relation { state1, state2, .. } => {
					self.translate_state(parent_id, state1, true, depth);
					self.translate_state(parent_id, state2, false, depth);
				}
				Stmt::State(state) => self.translate_state(parent_id, state, true, depth),
				_ => {}
			}
		}
	}

	fn translate_state(
		&mut self,
		parent_id: &str,
		state: &mut StateStmt,
		first: bool,
		depth: usize,
	) {
		if state.id == "[*]" {
			state.id = format!("{parent_id}{}", if first { "_start" } else { "_end" });
			state.start = Some(first);
		} else {
			state.id = state.id.trim().to_owned();
		}
		if let Some(mut doc) = state.doc.take() {
			let id = state.id.clone();
			self.translate_document(&id, &mut doc, depth + 1);
			state.doc = Some(doc);
		}
	}

	fn state_info(&mut self, id: &str) -> &mut StateInfo {
		let index = match self.states.iter().position(|(known, _)| known == id) {
			Some(index) => index,
			None => {
				self.states.push((id.to_owned(), StateInfo::default()));
				self.states.len() - 1
			}
		};
		&mut self.states[index].1
	}

	/// `extract` of a statement of the root document.
	fn extract(&mut self, statement: &Stmt) {
		match statement {
			Stmt::State(state) => {
				self.state_info(state.id.trim());
			}
			Stmt::Relation { state1, state2, .. } => {
				for state in [state1, state2] {
					let id = state.id.trim().to_owned();
					let classes = state.classes.clone();
					self.state_info(&id).classes.extend(classes);
				}
			}
			Stmt::ClassDef { id, classes } => self.add_style_class(id.trim(), classes),
			Stmt::Style { id, style_class } => {
				let styles: Vec<String> = style_class
					.split(',')
					.map(|style| style.replace(';', "").trim().to_owned())
					.collect();
				for id in id.trim().split(',') {
					self.state_info(id.trim()).styles = styles.clone();
				}
			}
			Stmt::ApplyClass { id, style_class } => {
				for id in id.trim().split(',') {
					self.state_info(id.trim()).classes.push(style_class.clone());
				}
			}
			Stmt::Click { id, url, tooltip } => {
				self.links.retain(|(known, _, _)| known != id);
				self.links.push((id.clone(), url.clone(), tooltip.clone()));
			}
			Stmt::Direction(_) | Stmt::Ignored => {}
		}
	}

	/// `addStyleClass`.
	fn add_style_class(&mut self, id: &str, attributes: &str) {
		let index = match self.classes.iter().position(|(known, _)| known == id) {
			Some(index) => index,
			None => {
				self.classes.push((id.to_owned(), StyleClass::default()));
				self.classes.len() - 1
			}
		};
		if attributes.is_empty() {
			return;
		}
		let class = &mut self.classes[index].1;
		for attribute in attributes.split(',') {
			// `replace(/([^;]*);/, '$1')`: the first `;` goes.
			let fixed = attribute.replacen(';', "", 1);
			class.styles.push(fixed.trim().to_owned());
		}
	}

	fn compiled_styles(&self, css_classes: &str) -> Vec<String> {
		let mut styles = Vec::new();
		for class in css_classes.split(' ') {
			if let Some((_, definition)) = self.classes.iter().find(|(known, _)| known == class) {
				styles.extend(definition.styles.iter().cloned());
			}
		}
		styles
	}

	/// `insertOrUpdateNode`.
	fn insert_or_update(&mut self, mut node: NodeData) {
		if node.id.is_empty() || node.id == "</join></fork>" || node.id == "</choice>" {
			return;
		}
		if !node.css_classes.is_empty() {
			node.css_compiled_styles = self.compiled_styles(&node.css_classes);
		}
		match self.nodes.iter_mut().find(|known| known.id == node.id) {
			Some(known) => {
				let parent_id = node.parent_id.take().or_else(|| known.parent_id.take());
				*known = NodeData { parent_id, ..node };
			}
			None => self.nodes.push(node),
		}
	}

	/// `colorSlotFor`.
	fn color_slot(
		&mut self,
		shape: &str,
		id: &str,
		parent_id: Option<&str>,
		is_user_styled: bool,
	) -> Option<usize> {
		if shape == SHAPE_DIVIDER
			&& let Some(inherited) =
				parent_id.and_then(|parent| self.container_color_index.get(parent))
		{
			let inherited = *inherited;
			self.container_color_index.insert(id.to_owned(), inherited);
			return inherited;
		}
		let slot = self.next_color_index;
		self.next_color_index += 1;
		let effective = if is_user_styled { None } else { Some(slot) };
		self.container_color_index.insert(id.to_owned(), effective);
		effective
	}

	/// `setupDoc`.
	fn fetch_document(&mut self, parent: &StateStmt, alt: bool, look: Look, depth: usize) {
		let Some(doc) = &parent.doc else { return };
		for statement in doc {
			self.fetch_statement(Some(parent), statement, alt, look, depth);
		}
	}

	fn fetch_statement(
		&mut self,
		parent: Option<&StateStmt>,
		statement: &Stmt,
		alt: bool,
		look: Look,
		depth: usize,
	) {
		match statement {
			Stmt::State(state) => self.fetch(parent, state, alt, look, depth),
			Stmt::Relation { state1, state2, description } => {
				self.fetch(parent, state1, alt, look, depth);
				self.fetch(parent, state2, alt, look, depth);
				self.edges.push(EdgeData {
					id: format!("edge{}", self.graph_item_count),
					start: state1.id.clone(),
					end: state2.id.clone(),
					label: description.clone().unwrap_or_default(),
					is_note_edge: false,
				});
				self.graph_item_count += 1;
			}
			_ => {}
		}
	}

	/// `dataFetcher` of `item`, a child of `parent` (`None` for the root document).
	fn fetch(
		&mut self,
		parent: Option<&StateStmt>,
		item: &StateStmt,
		alt: bool,
		look: Look,
		depth: usize,
	) {
		let id = item.id.clone();
		let (class_text, style) = match self.states.iter().find(|(known, _)| *known == id) {
			Some((_, info)) => (info.classes.join(" "), info.styles.clone()),
			None => (String::new(), Vec::new()),
		};
		let is_user_styled = !class_text.trim().is_empty() || !style.is_empty();
		let mut shape = Some(SHAPE_STATE.to_owned());
		match item.start {
			Some(true) => shape = Some(SHAPE_START.to_owned()),
			Some(false) => shape = Some(SHAPE_END.to_owned()),
			None => {}
		}
		if item.kind != Some(StateType::Default) {
			shape = item.kind.map(|kind| kind.shape().to_owned());
		}
		let known = self.known_nodes.entry(id.clone()).or_insert_with(|| KnownNode {
			shape,
			description: Label::Text(Description::Text(id.clone())),
			css_classes: format!("{class_text} statediagram-state"),
			css_styles: style,
			is_group: false,
			dir: None,
			color_index: None,
		});
		if let Some(description) = item.description.as_ref().filter(|_| item.has_description()) {
			match &mut known.description {
				Label::List(list) => {
					known.shape = Some(SHAPE_STATE_WITH_DESC.to_owned());
					list.extend(description.flatten());
				}
				Label::Text(previous) if !previous.is_empty() => {
					known.shape = Some(SHAPE_STATE_WITH_DESC.to_owned());
					known.description = match previous.first() == id {
						true => Label::List(description.flatten()),
						false => {
							let mut list = previous.flatten();
							list.extend(description.flatten());
							Label::List(list)
						}
					};
				}
				Label::Text(_) => {
					known.shape = Some(SHAPE_STATE.to_owned());
					known.description = match description {
						Description::Pair(first, second) => Label::List(vec![
							Description::Text(first.clone()),
							Description::Text(second.clone()),
						]),
						text => Label::Text(text.clone()),
					};
				}
			}
		}
		if matches!(&known.description, Label::List(list) if list.len() == 1)
			&& known.shape.as_deref() == Some(SHAPE_STATE_WITH_DESC)
		{
			known.shape = Some(if known.is_group { SHAPE_GROUP } else { SHAPE_STATE }.to_owned());
		}
		let mut became_group = false;
		if !known.is_group && item.doc.is_some() {
			known.is_group = true;
			known.dir = Some(direction_of(item));
			known.shape = Some(
				if item.kind == Some(StateType::Divider) { SHAPE_DIVIDER } else { SHAPE_GROUP }
					.to_owned(),
			);
			became_group = true;
		}
		if became_group {
			let shape = known.shape.clone().unwrap_or_default();
			let color_index = self.color_slot(
				&shape,
				&id,
				parent.map(|parent| parent.id.as_str()),
				is_user_styled,
			);
			let known = self.known_nodes.get_mut(&id).expect("inserted above");
			known.color_index = color_index;
			known.css_classes = format!(
				"{} statediagram-cluster {}",
				known.css_classes,
				if alt { "statediagram-cluster-alt" } else { "" }
			);
		}
		let known = &self.known_nodes[&id];
		let (label, description) = match &known.description {
			Label::Text(text) => (text.first().to_owned(), Vec::new()),
			Label::List(list) => (
				list.first().map(|first| first.first().to_owned()).unwrap_or_default(),
				list.iter().skip(1).map(|description| description.first().to_owned()).collect(),
			),
		};
		let shape = known.shape.clone();
		let parent_id = parent.filter(|parent| parent.id != "root").map(|parent| parent.id.clone());
		let node = NodeData {
			id: id.clone(),
			dom_id: state_dom_id(&id, self.graph_item_count, ""),
			label: if shape.as_deref() == Some(SHAPE_DIVIDER) { String::new() } else { label },
			description,
			css_classes: known.css_classes.clone(),
			css_compiled_styles: Vec::new(),
			css_styles: known.css_styles.clone(),
			dir: known.dir.clone(),
			is_group: known.is_group,
			color_index: known.color_index,
			padding: Some(8.0),
			is_rounded: true,
			parent_id,
			shape,
		};
		if let Some(note) = &item.note {
			let note_id = format!("{id}----note-{}", self.graph_item_count);
			let group_id = format!("{id}----parent");
			let note_node = NodeData {
				id: note_id.clone(),
				dom_id: state_dom_id(&id, self.graph_item_count, "note"),
				shape: Some(SHAPE_NOTE.to_owned()),
				label: note.text.clone(),
				description: Vec::new(),
				css_classes: "statediagram-note".to_owned(),
				css_compiled_styles: Vec::new(),
				css_styles: Vec::new(),
				dir: None,
				is_group: false,
				color_index: None,
				// `config.flowchart.padding`, filled in by the renderer.
				padding: None,
				is_rounded: false,
				parent_id: Some(group_id.clone()),
			};
			let group_node = NodeData {
				id: group_id,
				dom_id: state_dom_id(&id, self.graph_item_count, "parent"),
				shape: Some(SHAPE_NOTEGROUP.to_owned()),
				label: note.text.clone(),
				description: Vec::new(),
				css_classes: node.css_classes.clone(),
				css_compiled_styles: Vec::new(),
				css_styles: Vec::new(),
				dir: None,
				is_group: true,
				color_index: None,
				padding: Some(16.0),
				is_rounded: false,
				parent_id: None,
			};
			self.graph_item_count += 1;
			self.insert_or_update(group_node);
			self.insert_or_update(note_node);
			self.insert_or_update(node);
			let (start, end) = match note.is_left {
				true => (note_id, id),
				false => (id, note_id),
			};
			self.edges.push(EdgeData {
				id: format!("{start}-{end}"),
				start,
				end,
				label: String::new(),
				is_note_edge: true,
			});
		} else {
			self.insert_or_update(node);
		}
		if item.doc.is_some() && depth < 256 {
			self.fetch_document(item, !alt, look, depth + 1);
		}
	}
}

/// `getDir`: the last direction statement of the document, `TB` without one.
fn direction_of(item: &StateStmt) -> String {
	item.doc
		.iter()
		.flatten()
		.filter_map(|statement| match statement {
			Stmt::Direction(dir) => Some(dir.clone()),
			_ => None,
		})
		.next_back()
		.unwrap_or_else(|| "TB".to_owned())
}

/// `stateDomId`.
fn state_dom_id(id: &str, counter: usize, kind: &str) -> String {
	let kind = if kind.is_empty() { String::new() } else { format!("----{kind}") };
	format!("state-{id}{kind}-{counter}")
}

#[cfg(test)]
mod tests {
	use super::super::parser::parse;
	use super::*;
	use crate::mermaid::common::CommonFields;

	fn db(code: &str) -> StateDb {
		let doc = parse(code, &mut CommonFields::default()).unwrap();
		StateDb::new(doc, Look::Neo).unwrap()
	}

	#[test]
	fn fetches_nodes_and_edges() {
		let db = db("stateDiagram-v2\n  [*] --> A\n  A: first\n  A: second\n  A --> B : go\n  \
		             state C {\n    [*] --> D\n    --\n    E\n  }\n  note right of B : hi\n");
		let ids: Vec<&str> = db.nodes.iter().map(|node| node.id.as_str()).collect();
		assert_eq!(
			ids,
			[
				"root_start",
				"A",
				"B",
				"C",
				"divider-id-1",
				"divider-id-1_start",
				"D",
				"divider-id-end-1",
				"E",
				"B----parent",
				"B----note-3",
			]
		);
		let a = &db.nodes[1];
		assert_eq!(a.shape.as_deref(), Some(SHAPE_STATE_WITH_DESC));
		assert_eq!((a.label.as_str(), a.description.clone()), ("first", vec!["second".to_owned()]));
		let c = &db.nodes[3];
		assert!(c.is_group);
		assert_eq!(c.color_index, Some(0));
		assert_eq!(c.css_classes, " statediagram-state statediagram-cluster ");
		let region = &db.nodes[4];
		assert_eq!((region.shape.as_deref(), region.color_index), (Some(SHAPE_DIVIDER), Some(0)));
		assert_eq!(region.parent_id.as_deref(), Some("C"));
		assert_eq!(db.nodes[6].parent_id.as_deref(), Some("divider-id-1"));
		assert_eq!(db.edges.len(), 4);
		assert_eq!(db.edges[1].label, "go");
		assert!(db.edges[3].is_note_edge);
		assert_eq!(db.edges[3].id, "B-B----note-3");
	}

	#[test]
	fn applies_classes_and_styles() {
		let db = db("stateDiagram-v2\n  A --> B\n  classDef hot fill:#f00;,color:red\n  \
		             class A hot\n  style B stroke:#0f0;\n");
		assert_eq!(db.nodes[0].css_classes, "hot statediagram-state");
		assert_eq!(db.nodes[0].css_compiled_styles, ["fill:#f00", "color:red"]);
		assert_eq!(db.nodes[1].css_styles, ["stroke:#0f0"]);
	}

	#[test]
	fn rejects_descriptions_of_groups() {
		let doc = parse(
			"stateDiagram-v2\n  A: one\n  A: two\n  state A {\n    B\n  }\n",
			&mut CommonFields::default(),
		)
		.unwrap();
		assert!(StateDb::new(doc, Look::Classic).is_err());
	}
}
