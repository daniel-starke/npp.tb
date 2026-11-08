//! The entities, relationships, classes and subgraphs of an ER diagram (`erDb.ts`).

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cardinality {
	ZeroOrOne,
	ZeroOrMore,
	OneOrMore,
	OnlyOne,
	MdParent,
}

impl Cardinality {
	/// The marker type of the cardinality, `relSpec.cardA.toLowerCase()`.
	pub fn arrow_type(self) -> &'static str {
		match self {
			Cardinality::ZeroOrOne => "zero_or_one",
			Cardinality::ZeroOrMore => "zero_or_more",
			Cardinality::OneOrMore => "one_or_more",
			Cardinality::OnlyOne => "only_one",
			Cardinality::MdParent => "md_parent",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelSpec {
	pub card_a: Cardinality,
	pub card_b: Cardinality,
	pub is_identifying: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attribute {
	pub type_name: String,
	pub name: String,
	pub keys: Vec<String>,
	pub comment: String,
}

#[derive(Clone, Debug, Default)]
pub struct Entity {
	pub id: String,
	pub label: String,
	pub alias: String,
	pub attributes: Vec<Attribute>,
	/// `default` and the classes of `class` statements and `:::`.
	pub css_classes: String,
	pub css_styles: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Relationship {
	/// Entity ids, or subgraph ids for relationships to subgraphs.
	pub entity_a: String,
	pub role: String,
	pub entity_b: String,
	pub spec: RelSpec,
}

#[derive(Clone, Debug, Default)]
pub struct EntityClass {
	pub styles: Vec<String>,
	pub text_styles: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SubGraph {
	pub id: String,
	/// The names of the entities and the ids of the subgraphs in it.
	pub nodes: Vec<String>,
	pub title: String,
	pub classes: Vec<String>,
	pub css_styles: Vec<String>,
	pub dir: Option<String>,
}

/// What a statement contributes to the subgraph around it.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
	Name(String),
	Direction(&'static str),
}

#[derive(Debug)]
pub struct ErDb {
	pub entities: Vec<Entity>,
	entity_index: HashMap<String, usize>,
	pub relationships: Vec<Relationship>,
	pub classes: Vec<(String, EntityClass)>,
	pub subgraphs: Vec<SubGraph>,
	pub direction: String,
}

impl Default for ErDb {
	fn default() -> Self {
		ErDb {
			entities: Vec::new(),
			entity_index: HashMap::new(),
			relationships: Vec::new(),
			classes: Vec::new(),
			subgraphs: Vec::new(),
			direction: "TB".to_owned(),
		}
	}
}

impl ErDb {
	fn subgraph_mut(&mut self, id: &str) -> Option<&mut SubGraph> {
		self.subgraphs.iter_mut().rev().find(|subgraph| subgraph.id == id)
	}

	/// `addEntity`: the entity of `name`, created on first use; an alias is kept once set.
	pub fn add_entity(&mut self, name: &str, alias: &str) -> usize {
		match self.entity_index.get(name) {
			Some(&index) => {
				let entity = &mut self.entities[index];
				if entity.alias.is_empty() && !alias.is_empty() {
					entity.alias = alias.to_owned();
				}
				index
			}
			None => {
				let index = self.entities.len();
				self.entities.push(Entity {
					id: format!("entity-{name}-{index}"),
					label: name.to_owned(),
					alias: alias.to_owned(),
					css_classes: "default".to_owned(),
					..Entity::default()
				});
				self.entity_index.insert(name.to_owned(), index);
				index
			}
		}
	}

	pub fn add_attributes(&mut self, name: &str, attributes: Vec<Attribute>) {
		let index = self.add_entity(name, "");
		self.entities[index].attributes.extend(attributes);
	}

	/// `addRelationship`: names of subgraphs stand for the subgraphs, others for entities.
	pub fn add_relationship(&mut self, entity_a: &str, role: &str, entity_b: &str, spec: RelSpec) {
		let mut id_of = |name: &str| {
			if self.subgraphs.iter().any(|subgraph| subgraph.id == name) {
				return name.to_owned();
			}
			let index = self.add_entity(name, "");
			self.entities[index].id.clone()
		};
		let entity_a = id_of(entity_a);
		let entity_b = id_of(entity_b);
		self.relationships.push(Relationship { entity_a, role: role.to_owned(), entity_b, spec });
	}

	/// `addCssStyles`: `style` statements of entities and subgraphs.
	pub fn add_css_styles(&mut self, ids: &[String], styles: &[String]) {
		for id in ids {
			if let Some(&index) = self.entity_index.get(id) {
				self.entities[index].css_styles.extend(styles.iter().cloned());
			}
			if let Some(subgraph) = self.subgraph_mut(id) {
				subgraph.css_styles.extend(styles.iter().cloned());
			}
		}
	}

	/// `addClass`: a `classDef`; styles with a color also go to the text styles.
	pub fn add_class(&mut self, ids: &[String], styles: &[String]) {
		for id in ids {
			let index = match self.classes.iter().position(|(known, _)| known == id) {
				Some(index) => index,
				None => {
					self.classes.push((id.clone(), EntityClass::default()));
					self.classes.len() - 1
				}
			};
			let class = &mut self.classes[index].1;
			for style in styles {
				if style.contains("color") {
					class.text_styles.push(style.replacen("fill", "bgFill", 1));
				}
				class.styles.push(style.clone());
			}
		}
	}

	/// `setClass`: classes of entities and subgraphs.
	pub fn set_class(&mut self, ids: &[String], classes: &[String]) {
		for id in ids {
			if let Some(&index) = self.entity_index.get(id) {
				for class in classes {
					let entity = &mut self.entities[index];
					entity.css_classes.push(' ');
					entity.css_classes.push_str(class);
				}
			}
			if let Some(subgraph) = self.subgraph_mut(id) {
				subgraph.classes.extend(classes.iter().cloned());
			}
		}
	}

	/// `addSubGraph`: the subgraph of the items of its document; entities already in an earlier
	/// subgraph stay there. Returns its id.
	pub fn add_subgraph(&mut self, id: &str, items: Vec<Item>, title: &str) -> String {
		let mut nodes: Vec<String> = Vec::new();
		let mut dir = None;
		for item in items {
			match item {
				Item::Direction(direction) => dir = Some(direction.to_owned()),
				Item::Name(name) => {
					let name = name.trim();
					if !name.is_empty() && !nodes.iter().any(|known| known == name) {
						nodes.push(name.to_owned());
					}
				}
			}
		}
		nodes.retain(|node| {
			!self.subgraphs.iter().any(|subgraph| subgraph.nodes.iter().any(|known| known == node))
		});
		let id = id.trim().to_owned();
		self.subgraphs.push(SubGraph {
			id: id.clone(),
			nodes,
			title: title.trim().to_owned(),
			dir,
			..SubGraph::default()
		});
		id
	}

	/// `getCompiledStyles`: the styles and text styles of the classes.
	pub fn compiled_styles<'a>(&self, classes: impl Iterator<Item = &'a str>) -> Vec<String> {
		let mut styles = Vec::new();
		for name in classes {
			let Some((_, class)) = self.classes.iter().find(|(known, _)| known == name) else {
				continue;
			};
			styles.extend(class.styles.iter().map(|style| style.trim().to_owned()));
			styles.extend(class.text_styles.iter().map(|style| style.trim().to_owned()));
		}
		styles
	}
}
