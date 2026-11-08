//! The database of block diagrams (`blockDB.ts`): blocks are JavaScript objects that the
//! hierarchy and the id map share, kept here in an arena that both refer to by index.

use std::collections::HashMap;

use super::parser::{BlockType, Statement};
use crate::mermaid::rendering::{Point, shapes::Intersect};

/// Bounds the blocks that `space:N` statements create.
const MAX_SPACES: usize = 100_000;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
	pub width: f64,
	pub height: f64,
	/// The center.
	pub x: f64,
	pub y: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Block {
	/// `None` for JavaScript's `undefined`.
	pub id: Option<String>,
	pub label: Option<String>,
	pub kind: Option<BlockType>,
	pub directions: Option<Vec<String>>,
	pub width_in_columns: Option<f64>,
	pub columns: Option<f64>,
	pub children: Vec<usize>,
	pub classes: Vec<String>,
	pub styles: Option<Vec<String>>,
	/// The color slot of composites.
	pub color_index: Option<usize>,
	/// `space:N`
	pub space_width: Option<f64>,
	pub size: Option<Size>,
	/// Where edges end on the drawn block: the shape's intersection and the node it took.
	pub intersect: Option<(Intersect, Point, f64, f64)>,
}

impl Block {
	pub fn is(&self, kind: BlockType) -> bool {
		self.kind == Some(kind)
	}

	/// `widthInColumns ?? 1`
	pub fn columns_spanned(&self) -> f64 {
		self.width_in_columns.unwrap_or(1.0)
	}
}

#[derive(Clone, Debug)]
pub struct Edge {
	pub id: String,
	pub start: Option<String>,
	pub end: String,
	pub label: String,
	pub is_thick: bool,
	pub is_dotted: bool,
	pub arrow_type_start: &'static str,
	pub arrow_type_end: &'static str,
}

#[derive(Default)]
pub struct BlockDb {
	/// The blocks; the first one is `root`.
	pub blocks: Vec<Block>,
	/// `blockDatabase`: a later block of the same id replaces the earlier one.
	index: HashMap<Option<String>, usize>,
	pub edges: Vec<Edge>,
	edge_counts: HashMap<String, usize>,
	/// `classes`: the styles of each `classDef`, in definition order.
	pub classes: Vec<(String, Vec<String>)>,
	next_color_index: usize,
	spaces: usize,
}

pub const ROOT: usize = 0;

impl BlockDb {
	/// `clear` and `setHierarchy`.
	pub fn new(statements: Vec<Statement>) -> Result<BlockDb, String> {
		let root = Block {
			id: Some("root".to_owned()),
			kind: Some(BlockType::Composite),
			columns: Some(-1.0),
			..Block::default()
		};
		let mut db = BlockDb::default();
		db.blocks.push(root);
		db.set(Some("root".to_owned()), ROOT);
		db.populate(statements, ROOT)?;
		Ok(db)
	}

	/// `getBlock`
	pub fn get(&self, id: &Option<String>) -> Option<usize> {
		self.index.get(id).copied()
	}

	fn set(&mut self, id: Option<String>, index: usize) {
		self.index.insert(id, index);
	}

	/// The styles of the classes of `block`, in the order of its classes.
	pub fn class_styles(&self, block: &Block) -> Vec<String> {
		block
			.classes
			.iter()
			.filter_map(|class| self.classes.iter().find(|(id, _)| id == class))
			.flat_map(|(_, styles)| styles.iter().cloned())
			.collect()
	}

	/// `addStyleClass`
	fn add_style_class(&mut self, id: String, css: &str) {
		let position = match self.classes.iter().position(|(known, _)| *known == id) {
			Some(position) => position,
			None => {
				self.classes.push((id, Vec::new()));
				self.classes.len() - 1
			}
		};
		for attribute in css.split(',') {
			// `attrib.replace(/([^;]*);/, '$1')` drops the first `;`.
			let fixed = match attribute.find(';') {
				Some(semicolon) => {
					format!("{}{}", &attribute[..semicolon], &attribute[semicolon + 1..])
				}
				None => attribute.to_owned(),
			};
			self.classes[position].1.push(fixed.trim().to_owned());
		}
	}

	/// `setCssClass`: unknown blocks are created, under their trimmed id.
	fn set_css_class(&mut self, ids: &str, class: &str) {
		for id in ids.split(',') {
			let index = match self.get(&Some(id.to_owned())) {
				Some(index) => index,
				None => {
					let trimmed = id.trim().to_owned();
					self.blocks.push(Block {
						id: Some(trimmed.clone()),
						kind: Some(BlockType::Na),
						..Block::default()
					});
					let index = self.blocks.len() - 1;
					self.set(Some(trimmed), index);
					index
				}
			};
			self.blocks[index].classes.push(class.to_owned());
		}
	}

	/// `populateBlockDatabase`: the blocks of `statements` become the children of `parent`.
	fn populate(&mut self, statements: Vec<Statement>, parent: usize) -> Result<(), String> {
		let mut children = Vec::new();
		for statement in statements {
			let (block, statements) = match statement {
				Statement::ClassDef { id, css } => {
					self.add_style_class(id, &css);
					continue;
				}
				Statement::ApplyClass { ids, class } => {
					self.set_css_class(&ids, &class);
					continue;
				}
				Statement::ApplyStyles { ids, styles } => {
					if !styles.is_empty() {
						// `addStyle2Node` fails with a TypeError on unknown ids.
						let Some(index) = self.get(&Some(ids)) else {
							return Err(
								"Cannot set properties of undefined (setting 'styles')".to_owned()
							);
						};
						self.blocks[index].styles =
							Some(styles.split(',').map(str::to_owned).collect());
					}
					continue;
				}
				Statement::Columns(columns) => {
					self.blocks[parent].columns = Some(columns);
					continue;
				}
				Statement::Edge(edge) => {
					let count = self.edge_counts.entry(edge.id.clone()).or_insert(0);
					*count += 1;
					self.edges.push(Edge {
						id: format!("{count}-{}", edge.id),
						start: edge.start,
						end: edge.end,
						label: edge.label,
						is_thick: edge.is_thick,
						is_dotted: edge.is_dotted,
						arrow_type_start: edge.arrow_type_start,
						arrow_type_end: edge.arrow_type_end,
					});
					continue;
				}
				Statement::Block(block) => (
					Block {
						id: block.id,
						label: block.label,
						kind: block.kind,
						directions: block.directions,
						width_in_columns: block.width_in_columns,
						..Block::default()
					},
					None,
				),
				Statement::Space { id, width } => (
					Block {
						id: Some(id),
						label: Some(String::new()),
						kind: Some(BlockType::Space),
						space_width: Some(width),
						..Block::default()
					},
					Some(Vec::new()),
				),
				Statement::Composite { block, children } => (
					Block {
						id: block.id,
						label: block.label,
						kind: block.kind,
						directions: block.directions,
						width_in_columns: block.width_in_columns,
						..Block::default()
					},
					Some(children),
				),
			};
			let mut block = block;
			if block.label.as_deref().is_none_or(str::is_empty) {
				block.label = match block.is(BlockType::Composite) {
					true => Some(String::new()),
					false => block.id.clone(),
				};
			}
			let existing = self.get(&block.id);
			let is_space = block.is(BlockType::Space);
			let id = block.id.clone();
			self.blocks.push(block);
			let index = self.blocks.len() - 1;
			match existing {
				None => {
					if self.blocks[index].is(BlockType::Composite) {
						self.blocks[index].color_index = Some(self.next_color_index);
						self.next_color_index += 1;
					}
					self.set(id, index);
				}
				Some(existing) => {
					let (kind, label) = (self.blocks[index].kind, self.blocks[index].label.clone());
					if kind != Some(BlockType::Na) {
						self.blocks[existing].kind = kind;
					}
					if label != self.blocks[index].id {
						self.blocks[existing].label = label;
					}
				}
			}
			if let Some(statements) = statements {
				self.populate(statements, index)?;
			}
			if is_space {
				let width = self.blocks[index].space_width.unwrap_or(1.0);
				let mut column = 0.0;
				while column < width {
					self.spaces += 1;
					if self.spaces > MAX_SPACES {
						return Err("Too many blocks.".to_owned());
					}
					let mut clone = self.blocks[index].clone();
					let clone_id =
						format!("{}-{column}", clone.id.as_deref().unwrap_or("undefined"));
					clone.id = Some(clone_id.clone());
					self.blocks.push(clone);
					let clone_index = self.blocks.len() - 1;
					self.set(Some(clone_id), clone_index);
					children.push(clone_index);
					column += 1.0;
				}
			} else if existing.is_none() {
				children.push(index);
			}
		}
		self.blocks[parent].children = children;
		Ok(())
	}
}
