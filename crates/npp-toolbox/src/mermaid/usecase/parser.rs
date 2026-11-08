//! The Chevrotain grammar of use case diagrams (`usecase.parser.ts`) and the drafts its visitor
//! hands to the model builder (`usecase.visitor.ts`).
//!
//! The parser decides like Chevrotain's LL(3) lookahead: an alternative or repetition is taken
//! when the next tokens start it, without backtracking, and the first error ends the parse.

use super::json::{self, JsonValue};
use super::lexer::{Kind, Token};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelType {
	Text,
	Markdown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Label {
	pub text: String,
	pub label_type: LabelType,
}

/// Where a draft starts: its byte offset (for the source order) and line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Location {
	pub offset: usize,
	pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MetadataValue {
	Text(String),
	Bool(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetadataProperty {
	pub key: String,
	pub value: MetadataValue,
	pub location: Location,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementKind {
	Actor,
	Usecase,
}

impl ElementKind {
	pub fn name(self) -> &'static str {
		match self {
			ElementKind::Actor => "actor",
			ElementKind::Usecase => "usecase",
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseCaseShape {
	Ellipse,
	Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftElement {
	pub id: String,
	pub kind: ElementKind,
	pub label: Label,
	pub location: Location,
	pub generated: bool,
	pub parent: Option<(String, Location)>,
	pub shape: Option<UseCaseShape>,
	pub metadata: Option<Vec<MetadataProperty>>,
	pub stereotype: Option<String>,
	pub classes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftBoundary {
	pub id: String,
	pub label: Label,
	pub location: Location,
	pub generated: bool,
	pub classes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftJson {
	pub id: String,
	pub value: JsonValue,
	pub location: Location,
	pub classes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftEndpoint {
	pub id: String,
	pub label: Label,
	pub location: Location,
	pub generated: bool,
	pub declaration: bool,
	pub classes_on_reference: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationshipType {
	Association,
	Include,
	Extend,
	Generalization,
}

impl RelationshipType {
	pub fn name(self) -> &'static str {
		match self {
			RelationshipType::Association => "association",
			RelationshipType::Include => "include",
			RelationshipType::Extend => "extend",
			RelationshipType::Generalization => "generalization",
		}
	}
}

/// `ARROW_TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowType {
	SolidArrow,
	BackArrow,
	LineSolid,
	CircleArrow,
	CrossArrow,
	CircleArrowReversed,
	CrossArrowReversed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftRelationship {
	pub source: DraftEndpoint,
	pub target: DraftEndpoint,
	pub location: Location,
	pub explicit_id: Option<(String, Location)>,
	pub relationship_type: RelationshipType,
	pub arrow_type: ArrowType,
	pub label: Option<Label>,
	pub minlen: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DraftNote {
	pub target: String,
	pub target_location: Location,
	pub label: Label,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MetadataAssignment {
	pub target: String,
	pub target_location: Location,
	pub properties: Vec<MetadataProperty>,
}

/// A `class` statement: the targets with their locations and the classes.
pub type ClassAssignment = (Vec<(String, Location)>, Vec<String>);

/// Everything the visitor collects, in the order of its calls.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drafts {
	pub elements: Vec<DraftElement>,
	pub boundaries: Vec<DraftBoundary>,
	pub jsons: Vec<DraftJson>,
	pub relationships: Vec<DraftRelationship>,
	pub notes: Vec<DraftNote>,
	pub metadata_assignments: Vec<MetadataAssignment>,
	/// `class` statements: the targets with their locations and the classes.
	pub class_assignments: Vec<ClassAssignment>,
	pub style_assignments: Vec<(String, Location, Vec<String>)>,
	pub class_definitions: Vec<(Vec<String>, Vec<String>)>,
	pub directions: Vec<&'static str>,
	pub acc_title: Option<String>,
	pub acc_description: Option<String>,
}

/// A syntax error: the message and its line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type Parsed<T> = Result<T, ParseError>;

/// The words that open PlantUML statements Mermaid rejects.
const FORBIDDEN_PLANTUML_STATEMENTS: &[&str] =
	&["allowmixing", "newpage", "package", "rectangle", "skinparam"];

/// The first tokens of a relation tail: an edge id or an arrow.
fn starts_relation_tail(kind: Kind) -> bool {
	use Kind::*;
	matches!(
		kind,
		Identifier
			| DependencyArrow
			| Generalization
			| ForwardSolid
			| BackwardSolid
			| MarkerlessSolid
			| ForwardCircle
			| BackwardCircle
			| ForwardCross
			| BackwardCross
	)
}

/// The first tokens of a `line`.
fn starts_line(kind: Kind) -> bool {
	use Kind::*;
	matches!(
		kind,
		NewLine
			| Comment | AccTitleLine
			| AccDescrLine
			| AccDescrBlock
			| Direction
			| Actor | SystemBoundary
			| Note | JsonDeclarationStart
			| ClassDef
			| Class | Style
			| Identifier
			| PlainString
			| MarkdownString
	)
}

fn describe(kind: Kind) -> &'static str {
	use Kind::*;
	match kind {
		NewLine => "a line end",
		Eof => "the end of the diagram",
		Identifier => "an identifier",
		PlainString => "a string",
		MarkdownString => "a Markdown string",
		RightParen => "')'",
		RightBracket => "']'",
		RightBrace => "'}'",
		Colon => "':'",
		End => "'end'",
		For => "'for'",
		Usecase => "'usecase-beta'",
		StereotypeText => "a stereotype",
		StereotypeEnd => "'>>'",
		JsonObjectLiteral => "a JSON object",
		MarkerlessSolid => "'--'",
		_ => "another token",
	}
}

/// A parsed actor or use case reference (`actorItem`, `entityName`).
#[derive(Clone, Debug)]
struct Entity {
	id: String,
	label: Label,
	location: Location,
	generated: bool,
	shape: Option<UseCaseShape>,
	metadata: Option<Vec<MetadataProperty>>,
	stereotype: Option<String>,
	classes: Vec<String>,
	explicit_declaration: bool,
}

struct Arrow {
	relationship_type: RelationshipType,
	arrow_type: ArrowType,
	label: Option<Label>,
	minlen: usize,
}

struct RelationTail {
	explicit_id: Option<(String, Location)>,
	arrow: Arrow,
	target: Entity,
	location: Location,
}

struct Parser<'a> {
	text: &'a str,
	tokens: &'a [Token],
	position: usize,
	drafts: Drafts,
	parent_boundary: Option<(String, Location)>,
}

/// `label.replace(/\W/g, '_')`, per UTF-16 unit like JavaScript.
pub fn generate_id(label: &str) -> String {
	label
		.chars()
		.flat_map(|character| {
			let units = character.len_utf16();
			let keep = character.is_ascii_alphanumeric() || character == '_';
			std::iter::repeat_n(if keep { character } else { '_' }, if keep { 1 } else { units })
		})
		.collect()
}

impl<'a> Parser<'a> {
	fn peek(&self, ahead: usize) -> Token {
		let last = self.tokens.len() - 1;
		self.tokens[(self.position + ahead).min(last)]
	}

	fn kind(&self) -> Kind {
		self.peek(0).kind
	}

	fn image(&self, token: Token) -> &'a str {
		&self.text[token.start..token.end]
	}

	fn error(&self, expected: &str) -> ParseError {
		let token = self.peek(0);
		let found = match token.kind {
			Kind::Eof => "the end of the diagram".to_owned(),
			Kind::NewLine => "a line end".to_owned(),
			_ => {
				let image: String = self.image(token).chars().take(40).collect();
				format!("'{image}'")
			}
		};
		ParseError { line: token.line, message: format!("Expecting {expected}, but found {found}") }
	}

	fn advance(&mut self) -> Token {
		let token = self.peek(0);
		if self.position < self.tokens.len() - 1 {
			self.position += 1;
		}
		token
	}

	fn consume(&mut self, kind: Kind) -> Parsed<Token> {
		match self.kind() == kind {
			true => Ok(self.advance()),
			false => Err(self.error(describe(kind))),
		}
	}

	fn accept(&mut self, kind: Kind) -> Option<Token> {
		(self.kind() == kind).then(|| self.advance())
	}

	fn location(&self, token: Token) -> Location {
		Location { offset: token.start, line: token.line }
	}

	/// `tokenLabel`: the text of an identifier or string without its quotes.
	fn token_label(&self, token: Token) -> Label {
		let image = self.image(token);
		match token.kind {
			Kind::MarkdownString => Label {
				text: image.get(2..image.len().saturating_sub(2)).unwrap_or("").to_owned(),
				label_type: LabelType::Markdown,
			},
			Kind::PlainString => Label {
				text: image.get(1..image.len().saturating_sub(1)).unwrap_or("").to_owned(),
				label_type: LabelType::Text,
			},
			_ => Label { text: image.to_owned(), label_type: LabelType::Text },
		}
	}

	fn line_end(&mut self) -> Parsed<()> {
		match self.kind() {
			Kind::NewLine => {
				self.advance();
				Ok(())
			}
			Kind::Eof => Ok(()),
			_ => Err(self.error("a line end")),
		}
	}

	/// `nodeLabel` and `edgeLabel`: a string, or label text up to a delimiter, taken from the
	/// source.
	fn node_label(&mut self) -> Parsed<Label> {
		match self.kind() {
			Kind::PlainString | Kind::MarkdownString => {
				let token = self.advance();
				Ok(self.token_label(token))
			}
			kind if kind.is_label_text() => {
				let first = self.advance();
				let mut last = first;
				while self.kind().is_label_text() {
					last = self.advance();
				}
				Ok(Label {
					text: self.text[first.start..last.end].to_owned(),
					label_type: LabelType::Text,
				})
			}
			_ => Err(self.error("a label")),
		}
	}

	/// `metadata`: `@{ key: value, … }` over several lines.
	fn metadata(&mut self) -> Parsed<Vec<MetadataProperty>> {
		self.consume(Kind::MetadataStart)?;
		while self.accept(Kind::NewLine).is_some() {}
		let mut properties = Vec::new();
		if matches!(self.kind(), Kind::Identifier | Kind::PlainString) {
			properties.push(self.metadata_property()?);
			while self.enters_metadata_separator() {
				self.metadata_separator();
				properties.push(self.metadata_property()?);
			}
			self.accept(Kind::Comma);
			while self.accept(Kind::NewLine).is_some() {}
		}
		self.consume(Kind::RightBrace)?;
		Ok(properties)
	}

	/// The lookahead of `MANY2(metadataSeparator metadataProperty)`: the paths that tell another
	/// property from the trailing comma and line ends, ambiguous ones of three tokens included.
	fn enters_metadata_separator(&self) -> bool {
		use Kind::*;
		let is_key = |kind: Kind| matches!(kind, Identifier | PlainString);
		let (first, second, third) = (self.peek(0).kind, self.peek(1).kind, self.peek(2).kind);
		match (first, second) {
			(Comma, second) if is_key(second) => true,
			(Comma, NewLine) => third == NewLine || is_key(third),
			(NewLine, second) if is_key(second) => true,
			(NewLine, Comma) => true,
			(NewLine, NewLine) => matches!(third, NewLine | Comma) || is_key(third),
			_ => false,
		}
	}

	fn metadata_separator(&mut self) {
		if self.accept(Kind::Comma).is_some() {
			while self.accept(Kind::NewLine).is_some() {}
			return;
		}
		while self.accept(Kind::NewLine).is_some() {}
		self.accept(Kind::Comma);
		while self.accept(Kind::NewLine).is_some() {}
	}

	fn metadata_property(&mut self) -> Parsed<MetadataProperty> {
		let key = match self.kind() {
			Kind::Identifier | Kind::PlainString => self.advance(),
			_ => return Err(self.error("a metadata key")),
		};
		self.consume(Kind::Colon)?;
		let value = match self.kind() {
			Kind::Identifier | Kind::PlainString => {
				let token = self.advance();
				MetadataValue::Text(self.plain_text(token))
			}
			Kind::True => {
				self.advance();
				MetadataValue::Bool(true)
			}
			Kind::False => {
				self.advance();
				MetadataValue::Bool(false)
			}
			_ => return Err(self.error("a metadata value")),
		};
		Ok(MetadataProperty { key: self.plain_text(key), value, location: self.location(key) })
	}

	/// `decodePlain`: a string without its quotes, an identifier as is.
	fn plain_text(&self, token: Token) -> String {
		let image = self.image(token);
		match token.kind {
			Kind::PlainString => {
				image.get(1..image.len().saturating_sub(1)).unwrap_or("").to_owned()
			}
			_ => image.to_owned(),
		}
	}

	fn stereotype(&mut self) -> Parsed<String> {
		self.consume(Kind::StereotypeStart)?;
		let text = self.consume(Kind::StereotypeText)?;
		self.consume(Kind::StereotypeEnd)?;
		Ok(self.image(text).trim().to_owned())
	}

	fn class_suffix(&mut self) -> Parsed<Vec<String>> {
		self.consume(Kind::ClassSeparator)?;
		let first = self.consume(Kind::Identifier)?;
		let mut classes = vec![self.image(first).to_owned()];
		while self.accept(Kind::Comma).is_some() {
			let class = self.consume(Kind::Identifier)?;
			classes.push(self.image(class).to_owned());
		}
		Ok(classes)
	}

	/// `actorItem`.
	fn actor_item(&mut self) -> Parsed<Entity> {
		let mut entity = match self.kind() {
			Kind::Identifier => {
				let identifier = self.advance();
				let label = match self.accept(Kind::LeftParen) {
					Some(_) => {
						let label = self.node_label()?;
						self.consume(Kind::RightParen)?;
						label
					}
					None => self.token_label(identifier),
				};
				self.entity(self.image(identifier).to_owned(), label, identifier, false)
			}
			Kind::PlainString | Kind::MarkdownString => {
				let token = self.advance();
				let label = self.token_label(token);
				self.entity(generate_id(&label.text), label, token, true)
			}
			_ => return Err(self.error("an actor name")),
		};
		if self.kind() == Kind::MetadataStart {
			entity.metadata = Some(self.metadata()?);
		}
		if self.kind() == Kind::StereotypeStart {
			entity.stereotype = Some(self.stereotype()?);
		}
		if self.kind() == Kind::ClassSeparator {
			entity.classes = self.class_suffix()?;
		}
		Ok(entity)
	}

	fn entity(&self, id: String, label: Label, token: Token, generated: bool) -> Entity {
		Entity {
			id,
			label,
			location: self.location(token),
			generated,
			shape: None,
			metadata: None,
			stereotype: None,
			classes: Vec::new(),
			explicit_declaration: false,
		}
	}

	/// `entityName`.
	fn entity_name(&mut self) -> Parsed<Entity> {
		let mut entity = match self.kind() {
			Kind::Identifier => {
				let identifier = self.advance();
				let (label, shape) = match self.kind() {
					Kind::LeftParen => {
						self.advance();
						let label = self.node_label()?;
						self.consume(Kind::RightParen)?;
						(label, Some(UseCaseShape::Ellipse))
					}
					Kind::LeftBracket => {
						self.advance();
						let label = self.node_label()?;
						self.consume(Kind::RightBracket)?;
						(label, Some(UseCaseShape::Rect))
					}
					_ => (self.token_label(identifier), None),
				};
				let mut entity =
					self.entity(self.image(identifier).to_owned(), label, identifier, false);
				entity.shape = shape;
				entity
			}
			Kind::PlainString | Kind::MarkdownString => {
				let token = self.advance();
				let label = self.token_label(token);
				self.entity(generate_id(&label.text), label, token, true)
			}
			_ => return Err(self.error("a use case name")),
		};
		if self.kind() == Kind::MetadataStart {
			entity.metadata = Some(self.metadata()?);
		}
		if self.kind() == Kind::StereotypeStart {
			entity.stereotype = Some(self.stereotype()?);
		}
		if self.kind() == Kind::ClassSeparator {
			entity.classes = self.class_suffix()?;
		}
		entity.explicit_declaration =
			entity.shape.is_some() || entity.metadata.is_some() || entity.stereotype.is_some();
		Ok(entity)
	}

	/// `hasLabeledRight`: label tokens up to one of `allowed`.
	fn has_labeled_right(&self, allowed: &[Kind]) -> bool {
		if !self.peek(0).kind.is_label() {
			return false;
		}
		let mut ahead = 1;
		loop {
			let kind = self.peek(ahead).kind;
			if allowed.contains(&kind) {
				return true;
			}
			if !kind.is_label() || kind == Kind::Eof {
				return false;
			}
			ahead += 1;
		}
	}

	/// `Math.max(1, dashes - 1)`.
	fn solid_minlen(&self, token: Token) -> usize {
		let dashes = self.image(token).bytes().filter(|&byte| byte == b'-').count();
		dashes.saturating_sub(1).max(1)
	}

	fn association(arrow_type: ArrowType, minlen: usize, label: Option<Label>) -> Arrow {
		Arrow { relationship_type: RelationshipType::Association, arrow_type, label, minlen }
	}

	fn arrow(&mut self) -> Parsed<Arrow> {
		let token = self.peek(0);
		match token.kind {
			Kind::DependencyArrow => {
				self.advance();
				self.consume(Kind::Colon)?;
				let relationship_type = match self.kind() {
					Kind::Include => RelationshipType::Include,
					Kind::Extend => RelationshipType::Extend,
					_ => return Err(self.error("'include' or 'extend'")),
				};
				self.advance();
				Ok(Arrow {
					relationship_type,
					arrow_type: ArrowType::SolidArrow,
					label: Some(Label {
						text: relationship_type.name().to_owned(),
						label_type: LabelType::Text,
					}),
					minlen: 1,
				})
			}
			Kind::Generalization => {
				self.advance();
				Ok(Arrow {
					relationship_type: RelationshipType::Generalization,
					arrow_type: ArrowType::SolidArrow,
					label: None,
					minlen: 1,
				})
			}
			Kind::ForwardSolid => {
				self.advance();
				Ok(Self::association(ArrowType::SolidArrow, self.solid_minlen(token), None))
			}
			Kind::BackwardSolid => {
				self.advance();
				let is_labeled = self.image(token) == "<--"
					&& self.kind().is_label()
					&& self.has_labeled_right(&[Kind::MarkerlessSolid]);
				if !is_labeled {
					return Ok(Self::association(
						ArrowType::BackArrow,
						self.solid_minlen(token),
						None,
					));
				}
				let label = self.node_label()?;
				let length = self.consume(Kind::MarkerlessSolid)?;
				Ok(Self::association(ArrowType::BackArrow, self.solid_minlen(length), Some(label)))
			}
			Kind::MarkerlessSolid => {
				self.advance();
				let ends = [
					Kind::ForwardSolid,
					Kind::MarkerlessSolid,
					Kind::ForwardCircle,
					Kind::ForwardCross,
				];
				let is_labeled = self.image(token) == "--" && self.has_labeled_right(&ends);
				if !is_labeled {
					return Ok(Self::association(
						ArrowType::LineSolid,
						self.solid_minlen(token),
						None,
					));
				}
				let label = Some(self.node_label()?);
				let last = self.peek(0);
				let (arrow_type, minlen) = match last.kind {
					Kind::ForwardSolid => (ArrowType::SolidArrow, self.solid_minlen(last)),
					Kind::MarkerlessSolid => (ArrowType::LineSolid, self.solid_minlen(last)),
					Kind::ForwardCircle => (ArrowType::CircleArrow, 1),
					Kind::ForwardCross => (ArrowType::CrossArrow, 1),
					_ => return Err(self.error("an arrow")),
				};
				self.advance();
				Ok(Self::association(arrow_type, minlen, label))
			}
			Kind::ForwardCircle => {
				self.advance();
				Ok(Self::association(ArrowType::CircleArrow, 1, None))
			}
			Kind::ForwardCross => {
				self.advance();
				Ok(Self::association(ArrowType::CrossArrow, 1, None))
			}
			Kind::BackwardCircle | Kind::BackwardCross => {
				self.advance();
				let arrow_type = match token.kind {
					Kind::BackwardCircle => ArrowType::CircleArrowReversed,
					_ => ArrowType::CrossArrowReversed,
				};
				let mut label = None;
				if self.has_labeled_right(&[Kind::MarkerlessSolid]) {
					label = Some(self.node_label()?);
					self.consume(Kind::MarkerlessSolid)?;
				}
				Ok(Self::association(arrow_type, 1, label))
			}
			_ => Err(self.error("an arrow")),
		}
	}

	fn relation_tail(&mut self) -> Parsed<RelationTail> {
		let location = self.location(self.peek(0));
		let mut explicit_id = None;
		if self.kind() == Kind::Identifier && self.peek(1).kind == Kind::At {
			let identifier = self.advance();
			self.advance();
			explicit_id = Some((self.image(identifier).to_owned(), self.location(identifier)));
		}
		let arrow = self.arrow()?;
		let target = self.entity_name()?;
		Ok(RelationTail { explicit_id, arrow, target, location })
	}

	fn element_draft(&self, entity: &Entity, kind: ElementKind) -> DraftElement {
		DraftElement {
			id: entity.id.clone(),
			kind,
			label: entity.label.clone(),
			location: entity.location,
			generated: entity.generated,
			parent: self.parent_boundary.clone(),
			shape: if kind == ElementKind::Usecase { entity.shape } else { None },
			metadata: entity.metadata.clone(),
			stereotype: entity.stereotype.clone(),
			classes: entity.classes.clone(),
		}
	}

	fn endpoint(entity: &Entity, declaration: bool) -> DraftEndpoint {
		DraftEndpoint {
			id: entity.id.clone(),
			label: entity.label.clone(),
			location: entity.location,
			generated: entity.generated,
			declaration,
			classes_on_reference: !entity.classes.is_empty(),
		}
	}

	fn add_relationship(&mut self, source: DraftEndpoint, tail: RelationTail) {
		let target = Self::endpoint(&tail.target, tail.target.explicit_declaration);
		self.drafts.relationships.push(DraftRelationship {
			source,
			target,
			location: tail.location,
			explicit_id: tail.explicit_id,
			relationship_type: tail.arrow.relationship_type,
			arrow_type: tail.arrow.arrow_type,
			label: tail.arrow.label,
			minlen: tail.arrow.minlen,
		});
	}

	fn actor_statement(&mut self) -> Parsed<()> {
		self.consume(Kind::Actor)?;
		let mut items = vec![self.actor_item()?];
		let mut tail = None;
		if self.kind() == Kind::Comma {
			while self.accept(Kind::Comma).is_some() {
				items.push(self.actor_item()?);
			}
		} else if starts_relation_tail(self.kind()) {
			tail = Some(self.relation_tail()?);
		}
		self.line_end()?;
		for item in &items {
			let draft = self.element_draft(item, ElementKind::Actor);
			self.drafts.elements.push(draft);
		}
		if let Some(tail) = tail {
			if tail.target.explicit_declaration {
				let draft = self.element_draft(&tail.target, ElementKind::Usecase);
				self.drafts.elements.push(draft);
			}
			self.add_relationship(Self::endpoint(&items[0], true), tail);
		}
		Ok(())
	}

	fn entity_statement(&mut self) -> Parsed<()> {
		let mut source = self.entity_name()?;
		let tail = match starts_relation_tail(self.kind()) {
			true => Some(self.relation_tail()?),
			false => None,
		};
		self.line_end()?;
		let Some(tail) = tail else {
			source.explicit_declaration = true;
			source.shape.get_or_insert(UseCaseShape::Ellipse);
			let draft = self.element_draft(&source, ElementKind::Usecase);
			self.drafts.elements.push(draft);
			return Ok(());
		};
		if source.explicit_declaration {
			let draft = self.element_draft(&source, ElementKind::Usecase);
			self.drafts.elements.push(draft);
		}
		if tail.target.explicit_declaration {
			let draft = self.element_draft(&tail.target, ElementKind::Usecase);
			self.drafts.elements.push(draft);
		}
		let endpoint = Self::endpoint(&source, source.explicit_declaration);
		self.add_relationship(endpoint, tail);
		Ok(())
	}

	/// `systemBoundaryName`: an id with an optional title, or a string.
	fn boundary_name(&mut self) -> Parsed<DraftBoundary> {
		match self.kind() {
			Kind::Identifier => {
				let identifier = self.advance();
				let label = match self.kind() {
					Kind::LeftParen | Kind::LeftBracket => {
						let open = self.advance();
						let label = self.node_label()?;
						let close = match open.kind {
							Kind::LeftParen => Kind::RightParen,
							_ => Kind::RightBracket,
						};
						self.consume(close)?;
						label
					}
					_ => self.token_label(identifier),
				};
				Ok(DraftBoundary {
					id: self.image(identifier).to_owned(),
					label,
					location: self.location(identifier),
					generated: false,
					classes: Vec::new(),
				})
			}
			Kind::PlainString | Kind::MarkdownString => {
				let token = self.advance();
				let label = self.token_label(token);
				Ok(DraftBoundary {
					id: generate_id(&label.text),
					label,
					location: self.location(token),
					generated: true,
					classes: Vec::new(),
				})
			}
			_ => Err(self.error("a boundary name")),
		}
	}

	fn system_boundary_statement(&mut self) -> Parsed<()> {
		self.consume(Kind::SystemBoundary)?;
		let mut boundary = self.boundary_name()?;
		let metadata = match self.kind() {
			Kind::MetadataStart => Some(self.metadata()?),
			_ => None,
		};
		if self.kind() == Kind::ClassSeparator {
			boundary.classes = self.class_suffix()?;
		}
		self.line_end()?;
		let id = boundary.id.clone();
		let location = boundary.location;
		self.drafts.boundaries.push(boundary);
		let previous = self.parent_boundary.replace((id.clone(), location));
		loop {
			match self.kind() {
				Kind::NewLine => {
					self.advance();
				}
				Kind::Comment => {
					self.advance();
					self.line_end()?;
				}
				Kind::Actor => {
					self.advance();
					let mut items = vec![self.actor_item()?];
					while self.accept(Kind::Comma).is_some() {
						items.push(self.actor_item()?);
					}
					self.line_end()?;
					for item in &items {
						let draft = self.element_draft(item, ElementKind::Actor);
						self.drafts.elements.push(draft);
					}
				}
				Kind::Identifier | Kind::PlainString | Kind::MarkdownString => {
					let mut entity = self.entity_name()?;
					self.line_end()?;
					entity.explicit_declaration = true;
					entity.shape.get_or_insert(UseCaseShape::Ellipse);
					let draft = self.element_draft(&entity, ElementKind::Usecase);
					self.drafts.elements.push(draft);
				}
				_ => break,
			}
		}
		self.parent_boundary = previous;
		self.consume(Kind::End)?;
		self.line_end()?;
		if let Some(properties) = metadata {
			self.drafts.metadata_assignments.push(MetadataAssignment {
				target: id,
				target_location: location,
				properties,
			});
		}
		Ok(())
	}

	fn note_statement(&mut self) -> Parsed<()> {
		self.consume(Kind::Note)?;
		self.consume(Kind::For)?;
		let target = self.consume(Kind::Identifier)?;
		let label = self.node_label()?;
		self.line_end()?;
		self.drafts.notes.push(DraftNote {
			target: self.image(target).to_owned(),
			target_location: self.location(target),
			label,
		});
		Ok(())
	}

	fn json_statement(&mut self) -> Parsed<()> {
		let start = self.consume(Kind::JsonDeclarationStart)?;
		let literal = self.consume(Kind::JsonObjectLiteral)?;
		let classes = match self.kind() {
			Kind::ClassSeparator => self.class_suffix()?,
			_ => Vec::new(),
		};
		self.line_end()?;
		let image = self.image(start);
		let name_start = 4 + image[4..].len() - image[4..].trim_start_matches([' ', '\t']).len();
		let name_length = image[name_start..]
			.bytes()
			.take_while(|&byte| byte.is_ascii_alphanumeric() || byte == b'_')
			.count();
		let id = image[name_start..name_start + name_length].to_owned();
		let value = json::parse_object(self.image(literal)).map_err(|error| {
			let line = literal.line
				+ self.text[literal.start..literal.start + error.offset].matches('\n').count();
			ParseError { line, message: error.message }
		})?;
		self.drafts.jsons.push(DraftJson {
			id,
			value,
			location: Location { offset: start.start + name_start, line: start.line },
			classes,
		});
		Ok(())
	}

	fn direction_statement(&mut self) -> Parsed<()> {
		self.consume(Kind::Direction)?;
		let direction = match self.kind() {
			Kind::Td | Kind::Tb => "TB",
			Kind::Bt => "BT",
			Kind::Lr => "LR",
			Kind::Rl => "RL",
			_ => return Err(self.error("a direction")),
		};
		self.advance();
		self.line_end()?;
		self.drafts.directions.push(direction);
		Ok(())
	}

	/// `styles`: comma separated `property:value` items, each taken from the source.
	fn styles(&mut self) -> Parsed<Vec<String>> {
		let mut styles = vec![self.style_value()?];
		while self.accept(Kind::Comma).is_some() {
			styles.push(self.style_value()?);
		}
		Ok(styles)
	}

	fn style_value(&mut self) -> Parsed<String> {
		use Kind::*;
		let first = self.peek(0);
		match first.kind {
			kind if kind.is_word() || kind == CssIdentifier => {
				self.advance();
			}
			MarkerlessSolid => {
				self.advance();
				if !self.kind().is_word() {
					return Err(self.error("a style property"));
				}
				self.advance();
			}
			_ => return Err(self.error("a style property")),
		}
		self.consume(Colon)?;
		let is_component = |kind: Kind| {
			kind.is_word()
				|| matches!(
					kind,
					PlainString
						| Number | HashColor
						| CssIdentifier | CssEscapedComma
						| Dash | Dot | Percent
						| CssPunctuation | Colon
						| LeftParen | RightParen
						| LeftBracket | RightBracket
						| LeftBrace | RightBrace
						| At | MarkerlessSolid
				)
		};
		if !is_component(self.kind()) {
			return Err(self.error("a style value"));
		}
		let mut last = self.advance();
		while is_component(self.kind()) {
			last = self.advance();
		}
		Ok(self.text[first.start..last.end].replace("\\,", ","))
	}

	fn identifiers(&mut self) -> Parsed<Vec<Token>> {
		let mut identifiers = vec![self.consume(Kind::Identifier)?];
		while self.accept(Kind::Comma).is_some() {
			identifiers.push(self.consume(Kind::Identifier)?);
		}
		Ok(identifiers)
	}

	fn class_def_statement(&mut self) -> Parsed<()> {
		self.consume(Kind::ClassDef)?;
		let ids = self.identifiers()?;
		let styles = self.styles()?;
		self.line_end()?;
		let ids = ids.iter().map(|&token| self.image(token).to_owned()).collect();
		self.drafts.class_definitions.push((ids, styles));
		Ok(())
	}

	fn class_statement(&mut self) -> Parsed<()> {
		self.consume(Kind::Class)?;
		let targets = self.identifiers()?;
		let classes = self.identifiers()?;
		self.line_end()?;
		let targets = targets
			.iter()
			.map(|&token| (self.image(token).to_owned(), self.location(token)))
			.collect();
		let classes = classes.iter().map(|&token| self.image(token).to_owned()).collect();
		self.drafts.class_assignments.push((targets, classes));
		Ok(())
	}

	fn style_statement(&mut self) -> Parsed<()> {
		self.consume(Kind::Style)?;
		let target = self.consume(Kind::Identifier)?;
		let styles = self.styles()?;
		self.line_end()?;
		self.drafts.style_assignments.push((
			self.image(target).to_owned(),
			self.location(target),
			styles,
		));
		Ok(())
	}

	fn metadata_assignment_statement(&mut self) -> Parsed<()> {
		let token = self.advance();
		let target = match token.kind {
			Kind::Identifier => self.image(token).to_owned(),
			_ => generate_id(&self.token_label(token).text),
		};
		let properties = self.metadata()?;
		self.line_end()?;
		self.drafts.metadata_assignments.push(MetadataAssignment {
			target,
			target_location: self.location(token),
			properties,
		});
		Ok(())
	}

	fn accessibility_value(&self, token: Token) -> String {
		let image = self.image(token);
		match token.kind {
			Kind::AccDescrBlock => {
				let open = image.find('{').map_or(0, |index| index + 1);
				let close = image.rfind('}').unwrap_or(image.len()).max(open);
				image[open..close].trim().to_owned()
			}
			_ => image[image.find(':').map_or(image.len(), |index| index + 1)..].trim().to_owned(),
		}
	}

	fn statement(&mut self) -> Parsed<()> {
		let token = self.peek(0);
		match token.kind {
			Kind::AccTitleLine => {
				self.advance();
				self.line_end()?;
				self.drafts.acc_title = Some(self.accessibility_value(token));
				Ok(())
			}
			Kind::AccDescrLine | Kind::AccDescrBlock => {
				self.advance();
				self.line_end()?;
				self.drafts.acc_description = Some(self.accessibility_value(token));
				Ok(())
			}
			Kind::Direction => self.direction_statement(),
			Kind::Actor => self.actor_statement(),
			Kind::SystemBoundary => self.system_boundary_statement(),
			Kind::Note => self.note_statement(),
			Kind::JsonDeclarationStart => self.json_statement(),
			Kind::ClassDef => self.class_def_statement(),
			Kind::Class => self.class_statement(),
			Kind::Style => self.style_statement(),
			Kind::Identifier | Kind::PlainString | Kind::MarkdownString
				if self.peek(1).kind == Kind::MetadataStart =>
			{
				self.metadata_assignment_statement()
			}
			Kind::Identifier
				if FORBIDDEN_PLANTUML_STATEMENTS
					.contains(&self.image(token).to_ascii_lowercase().as_str()) =>
			{
				Err(self.error("a statement"))
			}
			Kind::Identifier | Kind::PlainString | Kind::MarkdownString => self.entity_statement(),
			_ => Err(self.error("a statement")),
		}
	}

	fn start(&mut self) -> Parsed<()> {
		self.consume(Kind::Usecase)?;
		self.line_end()?;
		while starts_line(self.kind()) {
			match self.kind() {
				Kind::NewLine => {
					self.advance();
				}
				Kind::Comment => {
					self.advance();
					self.line_end()?;
				}
				_ => self.statement()?,
			}
		}
		match self.kind() {
			Kind::Eof => Ok(()),
			_ => Err(self.error("a statement")),
		}
	}
}

/// Parses the tokens of `text`.
pub fn parse(text: &str, tokens: &[Token]) -> Result<Drafts, ParseError> {
	let mut parser =
		Parser { text, tokens, position: 0, drafts: Drafts::default(), parent_boundary: None };
	parser.start()?;
	Ok(parser.drafts)
}
