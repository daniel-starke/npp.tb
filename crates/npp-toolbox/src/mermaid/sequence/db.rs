//! The participants, boxes and messages of a sequence diagram (`sequenceDb.ts`).

use std::collections::HashMap;

use crate::mermaid::color;
use crate::mermaid::value::{self, Value};

/// Mermaid's `LINETYPE`: the kinds of messages, including the block markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineType {
	Solid,
	Dotted,
	Note,
	SolidCross,
	DottedCross,
	SolidOpen,
	DottedOpen,
	LoopStart,
	LoopEnd,
	AltStart,
	AltElse,
	AltEnd,
	OptStart,
	OptEnd,
	ActiveStart,
	ActiveEnd,
	ParStart,
	ParAnd,
	ParEnd,
	RectStart,
	RectEnd,
	SolidPoint,
	DottedPoint,
	Autonumber,
	CriticalStart,
	CriticalOption,
	CriticalEnd,
	BreakStart,
	BreakEnd,
	ParOverStart,
	BidirectionalSolid,
	BidirectionalDotted,
	SolidTop,
	SolidBottom,
	StickTop,
	StickBottom,
	SolidArrowTopReverse,
	SolidArrowBottomReverse,
	StickArrowTopReverse,
	StickArrowBottomReverse,
	SolidTopDotted,
	SolidBottomDotted,
	StickTopDotted,
	StickBottomDotted,
	SolidArrowTopReverseDotted,
	SolidArrowBottomReverseDotted,
	StickArrowTopReverseDotted,
	StickArrowBottomReverseDotted,
	CentralConnection,
	CentralConnectionReverse,
}

impl LineType {
	/// A message drawn as a line between participants.
	pub fn is_line(self) -> bool {
		use LineType::*;
		matches!(
			self,
			SolidOpen
				| DottedOpen | Solid
				| SolidTop | SolidBottom
				| StickTop | StickBottom
				| SolidTopDotted
				| SolidBottomDotted
				| StickTopDotted
				| StickBottomDotted
				| SolidArrowTopReverse
				| SolidArrowBottomReverse
				| StickArrowTopReverse
				| StickArrowBottomReverse
				| SolidArrowTopReverseDotted
				| SolidArrowBottomReverseDotted
				| StickArrowTopReverseDotted
				| StickArrowBottomReverseDotted
				| Dotted | SolidCross
				| DottedCross
				| SolidPoint | DottedPoint
				| BidirectionalSolid
				| BidirectionalDotted
		)
	}

	pub fn is_dotted(self) -> bool {
		use LineType::*;
		matches!(
			self,
			Dotted
				| DottedCross
				| DottedPoint
				| DottedOpen | BidirectionalDotted
				| SolidTopDotted
				| SolidBottomDotted
				| StickTopDotted
				| StickBottomDotted
				| SolidArrowTopReverseDotted
				| SolidArrowBottomReverseDotted
				| StickArrowTopReverseDotted
				| StickArrowBottomReverseDotted
		)
	}

	/// The half arrows drawn at the start of the line.
	pub fn is_reverse(self) -> bool {
		use LineType::*;
		matches!(
			self,
			SolidArrowTopReverse
				| SolidArrowTopReverseDotted
				| SolidArrowBottomReverse
				| SolidArrowBottomReverseDotted
				| StickArrowTopReverse
				| StickArrowTopReverseDotted
				| StickArrowBottomReverse
				| StickArrowBottomReverseDotted
		)
	}

	pub fn is_bidirectional(self) -> bool {
		matches!(self, LineType::BidirectionalSolid | LineType::BidirectionalDotted)
	}
}

/// `centralConnection` of a message: `()` at its end, start or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CentralConnection {
	#[default]
	None,
	Forward,
	Reverse,
	Dual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
	LeftOf,
	RightOf,
	Over,
}

/// A text with its `wrap:`/`nowrap:` setting.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
	pub text: String,
	pub wrap: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteActors {
	One(String),
	Pair(String, String),
}

/// A statement as `SequenceDB.apply` receives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
	Autonumber {
		start: Option<f64>,
		step: Option<f64>,
		visible: bool,
	},
	AddParticipant {
		actor: String,
		description: Option<Text>,
		/// `participant` or `actor`; `None` for participants named by messages.
		draw: Option<&'static str>,
		/// The `@{ … }` data.
		config: Option<String>,
	},
	CreateParticipant {
		actor: String,
		description: Option<Text>,
		draw: Option<&'static str>,
		config: Option<String>,
	},
	DestroyParticipant {
		actor: String,
	},
	ActiveStart(String),
	ActiveEnd(String),
	CentralConnection(String),
	CentralConnectionReverse(String),
	AddNote {
		actors: NoteActors,
		placement: Placement,
		text: Text,
	},
	AddLinks(String, String),
	AddLink(String, String),
	AddProperties(String, String),
	/// `details` reads an element of the page, which a document does not have.
	AddDetails,
	AddMessage {
		from: String,
		to: String,
		line_type: LineType,
		text: Text,
		activate: bool,
		central: CentralConnection,
	},
	BoxStart(String),
	BoxEnd,
	/// The start, section or end of a block (`loop`, `alt`, `else`, …) with its text.
	Block(LineType, Text),
}

impl Op {
	/// A participant named by a statement other than its declaration.
	pub fn actor(name: &str) -> Op {
		Op::AddParticipant { actor: name.to_owned(), description: None, draw: None, config: None }
	}
}

/// The shape of a participant (`PARTICIPANT_TYPE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorKind {
	Participant,
	Actor,
	Boundary,
	Control,
	Entity,
	Database,
	Collections,
	Queue,
	/// A type Mermaid does not draw.
	Unknown,
}

impl ActorKind {
	fn parse(name: &str) -> ActorKind {
		match name {
			"participant" => ActorKind::Participant,
			"actor" => ActorKind::Actor,
			"boundary" => ActorKind::Boundary,
			"control" => ActorKind::Control,
			"entity" => ActorKind::Entity,
			"database" => ActorKind::Database,
			"collections" => ActorKind::Collections,
			"queue" => ActorKind::Queue,
			_ => ActorKind::Unknown,
		}
	}
}

#[derive(Clone, Debug)]
pub struct Actor {
	pub name: String,
	pub description: String,
	pub wrap: bool,
	pub prev_actor: Option<String>,
	pub next_actor: Option<String>,
	/// Menu entries: label and URL.
	pub links: Vec<(String, String)>,
	pub kind: ActorKind,
	pub box_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct SequenceBox {
	pub name: Option<String>,
	pub wrap: bool,
	pub fill: String,
	pub actor_keys: Vec<String>,
}

/// `autonumber` settings carried by its message.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Autonumber {
	pub start: Option<f64>,
	pub step: Option<f64>,
	pub visible: bool,
}

#[derive(Clone, Debug)]
pub struct Message {
	pub from: Option<String>,
	pub to: Option<String>,
	pub text: String,
	pub wrap: bool,
	pub line_type: LineType,
	pub placement: Option<Placement>,
	pub activate: bool,
	pub central: CentralConnection,
	pub autonumber: Option<Autonumber>,
}

#[derive(Default)]
pub struct SequenceDb {
	/// In declaration order; a redeclared participant keeps its place.
	pub actors: Vec<(String, Actor)>,
	actor_index: HashMap<String, usize>,
	/// The participants created or destroyed by a message, with the index of that message.
	pub created_actors: HashMap<String, usize>,
	pub destroyed_actors: HashMap<String, usize>,
	pub boxes: Vec<SequenceBox>,
	pub messages: Vec<Message>,
	current_box: Option<usize>,
	prev_actor: Option<String>,
	last_created: Option<String>,
	last_destroyed: Option<String>,
	/// `autoWrap()`: the `wrap` configuration.
	auto_wrap: bool,
}

/// `extractWrap`: the trimmed text without a `wrap:` or `nowrap:` prefix, and the setting.
pub fn extract_wrap(text: &str) -> (String, Option<bool>) {
	let text = text.trim();
	let without_colon = text.strip_prefix(':').unwrap_or(text);
	if let Some(rest) = without_colon.strip_prefix("wrap:") {
		return (rest.trim().to_owned(), Some(true));
	}
	if let Some(rest) = without_colon.strip_prefix("nowrap:") {
		return (rest.trim().to_owned(), Some(false));
	}
	(text.to_owned(), None)
}

/// `parseBoxData`: a leading CSS color, then the title.
pub fn parse_box_data(text: &str) -> (Option<String>, String, Option<bool>) {
	let has_function = ["rgba", "rgb", "hsla", "hsl"].iter().find_map(|name| {
		let rest = text.strip_prefix(name)?.trim_start();
		rest.starts_with('(').then_some(())?;
		let close = text.rfind(')')?;
		Some(close + 1)
	});
	let color_end = has_function.unwrap_or_else(|| {
		text.find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
			.unwrap_or(text.len())
	});
	let mut color = text[..color_end].trim().to_owned();
	let mut title = text[color_end..].trim().to_owned();
	if color.is_empty() {
		color = "transparent".to_owned();
	}
	if !color::is_color(&color) {
		color = "transparent".to_owned();
		title = text.trim().to_owned();
	}
	let (cleaned, wrap) = extract_wrap(&title);
	let name = (!cleaned.is_empty()).then_some(cleaned);
	(name, color, wrap)
}

impl SequenceDb {
	pub fn new(auto_wrap: bool) -> Self {
		Self { auto_wrap, ..Self::default() }
	}

	/// The position of the participant `id` in [`Self::actors`].
	pub fn index_of(&self, id: &str) -> Option<usize> {
		self.actor_index.get(id).copied()
	}

	pub fn actor(&self, id: &str) -> Option<&Actor> {
		self.actor_index.get(id).map(|&index| &self.actors[index].1)
	}

	fn actor_mut(&mut self, id: &str) -> Option<&mut Actor> {
		self.actor_index.get(id).map(|&index| &mut self.actors[index].1)
	}

	pub fn actor_keys(&self) -> Vec<String> {
		self.actors.iter().map(|(key, _)| key.clone()).collect()
	}

	fn add_actor(
		&mut self,
		id: &str,
		description: Option<Text>,
		draw: Option<&'static str>,
		metadata: Option<&str>,
	) -> Result<(), String> {
		let name = id;
		let mut assigned_box = self.current_box;
		let data = metadata.and_then(|metadata| {
			let yaml = match metadata.contains('\n') {
				false => format!("{{\n{metadata}\n}}"),
				true => format!("{metadata}\n"),
			};
			value::parse_yaml(&yaml)
		});
		let data_text =
			|key: &str| data.as_ref().and_then(|data| data.get(key)).and_then(Value::to_text);
		let kind_name = data_text("type").or_else(|| draw.map(str::to_owned));
		let mut description = description;
		if let Some(alias) = data_text("alias")
			&& description.as_ref().is_none_or(|description| description.text == name)
		{
			let wrap = description.as_ref().and_then(|description| description.wrap);
			description = Some(Text { text: alias, wrap });
		}
		if let Some(old) = self.actor(id) {
			if let (Some(current), Some(old_box)) = (self.current_box, old.box_index)
				&& current != old_box
			{
				let box_name = |index: usize| self.boxes[index].name.clone().unwrap_or_default();
				return Err(format!(
					"A same participant should only be defined in one Box: {} can't be in '{}' and in '{}' at the same time.",
					old.name,
					box_name(old_box),
					box_name(current)
				));
			}
			assigned_box = old.box_index.or(self.current_box);
			let keeps = description.is_none();
			if let Some(old) = self.actor_mut(id) {
				old.box_index = assigned_box;
			}
			if keeps {
				return Ok(());
			}
		}
		let description = match (&kind_name, description) {
			(Some(_), Some(description)) => description,
			_ => Text { text: name.to_owned(), wrap: None },
		};
		let actor = Actor {
			name: name.to_owned(),
			description: description.text,
			wrap: description.wrap.unwrap_or(self.auto_wrap),
			prev_actor: self.prev_actor.clone(),
			next_actor: None,
			links: Vec::new(),
			kind: kind_name.as_deref().map_or(ActorKind::Participant, ActorKind::parse),
			box_index: assigned_box,
		};
		match self.actor_index.get(id) {
			Some(&index) => self.actors[index].1 = actor,
			None => {
				self.actor_index.insert(id.to_owned(), self.actors.len());
				self.actors.push((id.to_owned(), actor));
			}
		}
		if let Some(previous) = self.prev_actor.clone()
			&& let Some(previous) = self.actor_mut(&previous)
		{
			previous.next_actor = Some(id.to_owned());
		}
		if let Some(current) = self.current_box {
			self.boxes[current].actor_keys.push(id.to_owned());
		}
		self.prev_actor = Some(id.to_owned());
		Ok(())
	}

	fn activation_count(&self, part: &str) -> isize {
		self.messages
			.iter()
			.filter(|message| message.from.as_deref() == Some(part))
			.map(|message| match message.line_type {
				LineType::ActiveStart => 1,
				LineType::ActiveEnd => -1,
				_ => 0,
			})
			.sum()
	}

	fn push_message(&mut self, message: Message) {
		self.messages.push(message);
	}

	fn add_signal(
		&mut self,
		from: Option<&str>,
		to: Option<&str>,
		text: Option<&Text>,
		line_type: LineType,
		activate: bool,
		central: CentralConnection,
	) -> Result<(), String> {
		if line_type == LineType::ActiveEnd && self.activation_count(from.unwrap_or("")) < 1 {
			return Err(format!(
				"Trying to inactivate an inactive participant ({})",
				from.unwrap_or("undefined")
			));
		}
		self.push_message(Message {
			from: from.map(str::to_owned),
			to: to.map(str::to_owned),
			text: text.map(|text| text.text.clone()).unwrap_or_default(),
			wrap: text.and_then(|text| text.wrap).unwrap_or(self.auto_wrap),
			line_type,
			placement: None,
			activate,
			central,
			autonumber: None,
		});
		Ok(())
	}

	/// `sanitizeText` and the entity replacements of the link statements.
	fn link_text(text: &str) -> String {
		text.replace("&equals;", "=").replace("&amp;", "&")
	}

	fn insert_links(&mut self, actor: &str, links: Vec<(String, String)>) {
		let Some(actor) = self.actor_mut(actor) else { return };
		for (label, url) in links {
			match actor.links.iter_mut().find(|(known, _)| *known == label) {
				Some(entry) => entry.1 = url,
				None => actor.links.push((label, url)),
			}
		}
	}

	pub fn apply(&mut self, op: Op) -> Result<(), String> {
		match op {
			Op::Autonumber { start, step, visible } => self.push_message(Message {
				from: None,
				to: None,
				text: String::new(),
				wrap: false,
				line_type: LineType::Autonumber,
				placement: None,
				activate: false,
				central: CentralConnection::None,
				autonumber: Some(Autonumber { start, step, visible }),
			}),
			Op::AddParticipant { actor, description, draw, config } => {
				self.add_actor(&actor, description, draw, config.as_deref())?;
			}
			Op::CreateParticipant { actor, description, draw, config } => {
				if self.actor_index.contains_key(&actor) {
					return Err("It is not possible to have actors with the same id, even if one is destroyed before the next is created. Use 'AS' aliases to simulate the behavior".to_owned());
				}
				self.last_created = Some(actor.clone());
				self.add_actor(&actor, description, draw, config.as_deref())?;
				self.created_actors.insert(actor, self.messages.len());
			}
			Op::DestroyParticipant { actor } => {
				self.last_destroyed = Some(actor.clone());
				self.destroyed_actors.insert(actor, self.messages.len());
			}
			Op::ActiveStart(actor) => {
				self.add_signal(
					Some(&actor),
					None,
					None,
					LineType::ActiveStart,
					false,
					Default::default(),
				)?;
			}
			Op::ActiveEnd(actor) => {
				self.add_signal(
					Some(&actor),
					None,
					None,
					LineType::ActiveEnd,
					false,
					Default::default(),
				)?;
			}
			Op::CentralConnection(actor) => {
				let line_type = LineType::CentralConnection;
				self.add_signal(Some(&actor), None, None, line_type, false, Default::default())?;
			}
			Op::CentralConnectionReverse(actor) => {
				let line_type = LineType::CentralConnectionReverse;
				self.add_signal(Some(&actor), None, None, line_type, false, Default::default())?;
			}
			Op::AddNote { actors, placement, text } => {
				let (from, to) = match actors {
					NoteActors::One(actor) => (actor.clone(), actor),
					NoteActors::Pair(first, second) => (first, second),
				};
				self.push_message(Message {
					from: Some(from),
					to: Some(to),
					text: text.text,
					wrap: text.wrap.unwrap_or(self.auto_wrap),
					line_type: LineType::Note,
					placement: Some(placement),
					activate: false,
					central: CentralConnection::None,
					autonumber: None,
				});
			}
			Op::AddLinks(actor, text) => {
				if let Some(Value::Object(entries)) = value::parse_json(&Self::link_text(&text)) {
					let links = entries
						.into_iter()
						.filter_map(|(label, url)| url.to_text().map(|url| (label, url)))
						.collect();
					self.insert_links(&actor, links);
				}
			}
			Op::AddLink(actor, text) => {
				let separator = text.find('@');
				let text = Self::link_text(&text);
				// `slice(0, sep - 1)` and `slice(sep + 1)` with JavaScript's negative indices.
				let (label, url) = match separator {
					Some(index) => (
						text.get(..index.saturating_sub(1)).unwrap_or("").trim().to_owned(),
						text.get(index + 1..).unwrap_or("").trim().to_owned(),
					),
					None => {
						let end = text.char_indices().rev().nth(1).map_or(0, |(index, _)| index);
						(text[..end].trim().to_owned(), text.trim().to_owned())
					}
				};
				self.insert_links(&actor, vec![(label, url)]);
			}
			Op::AddProperties(..) | Op::AddDetails => {}
			Op::AddMessage { from, to, line_type, text, activate, central } => {
				if let Some(created) = self.last_created.take() {
					if to != created {
						return Err(format!(
							"The created participant {created} does not have an associated creating message after its declaration. Please check the sequence diagram."
						));
					}
				} else if let Some(destroyed) = self.last_destroyed.take()
					&& to != destroyed
					&& from != destroyed
				{
					return Err(format!(
						"The destroyed participant {destroyed} does not have an associated destroying message after its declaration. Please check the sequence diagram."
					));
				}
				self.add_signal(Some(&from), Some(&to), Some(&text), line_type, activate, central)?;
			}
			Op::BoxStart(text) => {
				let (name, fill, wrap) = parse_box_data(&text);
				self.boxes.push(SequenceBox {
					name,
					wrap: wrap.unwrap_or(self.auto_wrap),
					fill,
					actor_keys: Vec::new(),
				});
				self.current_box = Some(self.boxes.len() - 1);
			}
			Op::BoxEnd => self.current_box = None,
			Op::Block(line_type, text) => {
				let has_text = !matches!(
					line_type,
					LineType::LoopEnd
						| LineType::RectEnd
						| LineType::OptEnd | LineType::AltEnd
						| LineType::ParEnd | LineType::CriticalEnd
						| LineType::BreakEnd
				);
				let text = has_text.then_some(&text);
				self.add_signal(None, None, text, line_type, false, Default::default())?;
			}
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_box_data() {
		assert_eq!(
			parse_box_data("Purple Alice & John"),
			(Some("Alice & John".to_owned()), "Purple".to_owned(), None)
		);
		assert_eq!(
			parse_box_data("Another Group"),
			(Some("Another Group".to_owned()), "transparent".to_owned(), None)
		);
		assert_eq!(
			parse_box_data("rgb(1, 2, 3) wrap:Group"),
			(Some("Group".to_owned()), "rgb(1, 2, 3)".to_owned(), Some(true))
		);
		assert_eq!(parse_box_data(""), (None, "transparent".to_owned(), None));
	}

	#[test]
	fn keeps_participants_in_order() {
		let mut db = SequenceDb::new(false);
		db.apply(Op::actor("A")).unwrap();
		db.apply(Op::actor("B")).unwrap();
		let description = Some(Text { text: "Alice".to_owned(), wrap: None });
		let op = Op::AddParticipant {
			actor: "A".to_owned(),
			description,
			draw: Some("participant"),
			config: Some("type: queue, alias: Q".to_owned()),
		};
		db.apply(op).unwrap();
		assert_eq!(db.actor_keys(), ["A", "B"]);
		let actor = db.actor("A").unwrap();
		assert_eq!((actor.description.as_str(), actor.kind), ("Alice", ActorKind::Queue));
		assert_eq!(actor.prev_actor.as_deref(), Some("B"));
		assert!(db.apply(Op::ActiveEnd("A".to_owned())).is_err());
	}
}
