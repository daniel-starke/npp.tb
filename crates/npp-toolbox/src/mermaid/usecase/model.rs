//! The model of use case diagrams (`usecaseModelBuilder.ts`): resolves the drafts of the parser
//! into actors, use cases, boundaries, relationships, notes and JSON tables, with Mermaid's
//! checks and error messages.

use std::collections::HashMap;

use super::json::JsonValue;
use super::parser::{
	ArrowType, DraftElement, DraftEndpoint, DraftRelationship, Drafts, ElementKind, Label,
	LabelType, Location, MetadataProperty, MetadataValue, RelationshipType, UseCaseShape,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolKind {
	Actor,
	Usecase,
	Boundary,
	Json,
	Edge,
}

impl SymbolKind {
	fn name(self) -> &'static str {
		match self {
			SymbolKind::Actor => "actor",
			SymbolKind::Usecase => "usecase",
			SymbolKind::Boundary => "boundary",
			SymbolKind::Json => "json",
			SymbolKind::Edge => "edge",
		}
	}

	fn of(kind: ElementKind) -> SymbolKind {
		match kind {
			ElementKind::Actor => SymbolKind::Actor,
			ElementKind::Usecase => SymbolKind::Usecase,
		}
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorType {
	Normal,
	Hollow,
	Awesome,
	Icon,
}

impl ActorType {
	pub fn name(self) -> &'static str {
		match self {
			ActorType::Normal => "normal",
			ActorType::Hollow => "hollow",
			ActorType::Awesome => "awesome",
			ActorType::Icon => "icon",
		}
	}
}

/// An actor or a use case.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
	pub id: String,
	pub label: Label,
	pub actor_type: ActorType,
	pub icon: Option<String>,
	pub shape: UseCaseShape,
	pub business: bool,
	pub stereotype: Option<String>,
	pub parent: Option<String>,
	pub classes: Vec<String>,
	pub styles: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryType {
	Rect,
	Package,
}

impl BoundaryType {
	pub fn name(self) -> &'static str {
		match self {
			BoundaryType::Rect => "rect",
			BoundaryType::Package => "package",
		}
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
	pub id: String,
	pub label: Label,
	pub boundary_type: BoundaryType,
	pub classes: Vec<String>,
	pub styles: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relationship {
	pub id: String,
	pub is_explicit: bool,
	pub source: String,
	pub target: String,
	pub relationship_type: RelationshipType,
	pub arrow_type: ArrowType,
	pub label: Option<Label>,
	pub minlen: usize,
	pub classes: Vec<String>,
	pub styles: Vec<String>,
	pub animate: bool,
	pub animation: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
	pub id: String,
	pub target: String,
	pub label: Label,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JsonNode {
	pub id: String,
	pub value: JsonValue,
	pub classes: Vec<String>,
	pub styles: Vec<String>,
}

/// The resolved diagram.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
	/// Actors, then use cases, each in the order of their first appearance.
	pub actors: Vec<Element>,
	pub use_cases: Vec<Element>,
	pub boundaries: Vec<Boundary>,
	pub relationships: Vec<Relationship>,
	pub notes: Vec<Note>,
	pub jsons: Vec<JsonNode>,
	pub class_defs: Vec<(String, Vec<String>)>,
	pub direction: &'static str,
	pub acc_title: Option<String>,
	pub acc_description: Option<String>,
}

/// A model error: its line and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelError {
	pub line: usize,
	pub message: String,
}

type Checked<T> = Result<T, ModelError>;

#[derive(Clone, Debug)]
struct Symbol {
	kind: SymbolKind,
	location: Location,
	generated: bool,
	/// The label a generated id was derived from.
	label: Option<String>,
}

#[derive(Clone, Debug)]
struct ElementState {
	kind: ElementKind,
	id: String,
	label: Label,
	location: Location,
	parent: Option<(String, Location)>,
	shape: Option<UseCaseShape>,
	stereotype: Option<String>,
	stereotype_location: Option<Location>,
	classes: Vec<String>,
	actor_type: Option<ActorType>,
	icon: Option<String>,
	business: Option<bool>,
}

#[derive(Clone, Debug)]
struct BoundaryState {
	id: String,
	label: Label,
	location: Location,
	boundary_type: Option<BoundaryType>,
	classes: Vec<String>,
}

fn error(line: usize, message: impl Into<String>) -> ModelError {
	ModelError { line, message: message.into() }
}

/// `conflict`: an error at `current` that names the earlier declaration.
fn conflict(
	message: &str,
	current: Location,
	previous: Location,
	labels: (Option<&str>, Option<&str>),
) -> ModelError {
	let suffix = |label: Option<&str>| {
		label.map(|label| format!(" (label \"{label}\")")).unwrap_or_default()
	};
	error(
		current.line,
		format!(
			"{message}{}; previous declaration on line {}{}",
			suffix(labels.0),
			previous.line,
			suffix(labels.1)
		),
	)
}

fn generated_from(symbol: &Symbol) -> Option<&str> {
	symbol.label.as_deref().filter(|_| symbol.generated)
}

fn push_unique(target: &mut Vec<String>, values: &[String]) {
	for value in values {
		if !target.contains(value) {
			target.push(value.clone());
		}
	}
}

fn invalid_metadata(id: &str, kind: &str, property: &MetadataProperty) -> ModelError {
	error(
		property.location.line,
		format!("Metadata property '{}' is invalid for {kind} '{id}'", property.key),
	)
}

/// An ordered map of states, like a JavaScript `Map`.
struct Ordered<T> {
	order: Vec<String>,
	items: HashMap<String, T>,
}

impl<T> Ordered<T> {
	fn new() -> Self {
		Ordered { order: Vec::new(), items: HashMap::new() }
	}

	fn get(&self, id: &str) -> Option<&T> {
		self.items.get(id)
	}

	fn get_mut(&mut self, id: &str) -> Option<&mut T> {
		self.items.get_mut(id)
	}

	fn insert(&mut self, id: &str, value: T) {
		if self.items.insert(id.to_owned(), value).is_none() {
			self.order.push(id.to_owned());
		}
	}

	fn values(&self) -> impl Iterator<Item = &T> {
		self.order.iter().filter_map(|id| self.items.get(id))
	}
}

struct Builder<'d> {
	drafts: &'d Drafts,
	symbols: HashMap<String, Symbol>,
	elements: Ordered<ElementState>,
	boundaries: Ordered<BoundaryState>,
	first: HashMap<String, usize>,
}

impl Builder<'_> {
	fn record_first(&mut self, id: &str, offset: usize) {
		let entry = self.first.entry(id.to_owned()).or_insert(offset);
		*entry = (*entry).min(offset);
	}

	fn first_of(&self, id: &str) -> usize {
		self.first.get(id).copied().unwrap_or(0)
	}

	fn collect_element(&mut self, draft: &DraftElement) -> Checked<()> {
		let kind = SymbolKind::of(draft.kind);
		let draft_label = draft.generated.then_some(draft.label.text.as_str());
		if let Some(origin) = self.symbols.get(&draft.id) {
			if origin.kind != kind {
				return Err(conflict(
					&format!(
						"ID '{}' is declared as both {} and {}",
						draft.id,
						origin.kind.name(),
						kind.name()
					),
					draft.location,
					origin.location,
					(draft_label, generated_from(origin)),
				));
			}
			if origin.generated || draft.generated {
				return Err(conflict(
					&format!("Generated ID '{}' collides with another declaration", draft.id),
					draft.location,
					origin.location,
					(draft_label, generated_from(origin)),
				));
			}
		}
		let Some(existing) = self.elements.get_mut(&draft.id) else {
			let mut state = ElementState {
				kind: draft.kind,
				id: draft.id.clone(),
				label: draft.label.clone(),
				location: draft.location,
				parent: draft.parent.clone(),
				shape: draft.shape,
				stereotype: draft.stereotype.clone(),
				stereotype_location: draft.stereotype.as_ref().map(|_| draft.location),
				classes: draft.classes.clone(),
				actor_type: None,
				icon: None,
				business: None,
			};
			if let Some(metadata) = &draft.metadata {
				apply_declaration_metadata(&mut state, metadata)?;
			}
			self.elements.insert(&draft.id, state);
			self.symbols.insert(
				draft.id.clone(),
				Symbol {
					kind,
					location: draft.location,
					generated: draft.generated,
					label: Some(draft.label.text.clone()),
				},
			);
			return Ok(());
		};
		if existing.label != draft.label {
			return Err(conflict(
				&format!("ID '{}' has conflicting labels", draft.id),
				draft.location,
				existing.location,
				(None, None),
			));
		}
		if let (Some(shape), Some(known)) = (draft.shape, existing.shape)
			&& shape != known
		{
			return Err(conflict(
				&format!("Use case '{}' has conflicting shapes", draft.id),
				draft.location,
				existing.location,
				(None, None),
			));
		}
		if let (Some((parent, location)), Some((known, known_location))) =
			(&draft.parent, &existing.parent)
			&& parent != known
		{
			return Err(conflict(
				&format!("Element '{}' belongs to more than one system boundary", draft.id),
				*location,
				*known_location,
				(None, None),
			));
		}
		if let (Some(stereotype), Some(known)) = (&draft.stereotype, &existing.stereotype)
			&& stereotype != known
		{
			return Err(conflict(
				&format!("Element '{}' has conflicting stereotypes", draft.id),
				draft.location,
				existing.stereotype_location.unwrap_or(existing.location),
				(None, None),
			));
		}
		if existing.shape.is_none() {
			existing.shape = draft.shape;
		}
		if existing.parent.is_none() {
			existing.parent = draft.parent.clone();
		}
		if existing.stereotype.is_none() {
			existing.stereotype = draft.stereotype.clone();
		}
		if existing.stereotype_location.is_none() && draft.stereotype.is_some() {
			existing.stereotype_location = Some(draft.location);
		}
		push_unique(&mut existing.classes, &draft.classes);
		if let Some(metadata) = &draft.metadata {
			apply_declaration_metadata(existing, metadata)?;
		}
		Ok(())
	}

	fn collect_boundary(&mut self, index: usize) -> Checked<()> {
		let draft = &self.drafts.boundaries[index];
		let draft_label = draft.generated.then_some(draft.label.text.as_str());
		if let Some(origin) = self.symbols.get(&draft.id) {
			if origin.kind != SymbolKind::Boundary {
				return Err(conflict(
					&format!(
						"ID '{}' is declared as both {} and boundary",
						draft.id,
						origin.kind.name()
					),
					draft.location,
					origin.location,
					(draft_label, generated_from(origin)),
				));
			}
			if origin.generated || draft.generated {
				return Err(conflict(
					&format!("Generated ID '{}' collides with another declaration", draft.id),
					draft.location,
					origin.location,
					(draft_label, generated_from(origin)),
				));
			}
		}
		if let Some(existing) = self.boundaries.get_mut(&draft.id) {
			if existing.label != draft.label {
				return Err(conflict(
					&format!("Boundary '{}' has conflicting titles", draft.id),
					draft.location,
					existing.location,
					(None, None),
				));
			}
			push_unique(&mut existing.classes, &draft.classes);
			return Ok(());
		}
		self.boundaries.insert(
			&draft.id,
			BoundaryState {
				id: draft.id.clone(),
				label: draft.label.clone(),
				location: draft.location,
				boundary_type: None,
				classes: draft.classes.clone(),
			},
		);
		self.symbols.insert(
			draft.id.clone(),
			Symbol {
				kind: SymbolKind::Boundary,
				location: draft.location,
				generated: draft.generated,
				label: Some(draft.label.text.clone()),
			},
		);
		Ok(())
	}

	fn register_unique(&mut self, id: &str, kind: SymbolKind, location: Location) -> Checked<()> {
		if let Some(previous) = self.symbols.get(id) {
			return Err(conflict(
				&format!(
					"ID '{id}' is declared more than once ({} and {})",
					previous.kind.name(),
					kind.name()
				),
				location,
				previous.location,
				(None, generated_from(previous)),
			));
		}
		self.symbols
			.insert(id.to_owned(), Symbol { kind, location, generated: false, label: None });
		Ok(())
	}

	fn resolve_endpoint(&mut self, endpoint: &DraftEndpoint) -> Checked<SymbolKind> {
		if endpoint.classes_on_reference && !endpoint.declaration {
			return Err(error(
				endpoint.location.line,
				format!(
					"Relationship endpoint '{}' uses ::: without declaring the node",
					endpoint.id
				),
			));
		}
		if let Some(origin) = self.symbols.get(&endpoint.id) {
			return Ok(origin.kind);
		}
		self.elements.insert(
			&endpoint.id,
			ElementState {
				kind: ElementKind::Usecase,
				id: endpoint.id.clone(),
				label: endpoint.label.clone(),
				location: endpoint.location,
				parent: None,
				shape: Some(UseCaseShape::Ellipse),
				stereotype: None,
				stereotype_location: None,
				classes: Vec::new(),
				actor_type: None,
				icon: None,
				business: None,
			},
		);
		self.symbols.insert(
			endpoint.id.clone(),
			Symbol {
				kind: SymbolKind::Usecase,
				location: endpoint.location,
				generated: endpoint.generated,
				label: Some(endpoint.label.text.clone()),
			},
		);
		self.record_first(&endpoint.id, endpoint.location.offset);
		Ok(SymbolKind::Usecase)
	}

	fn validate_relationship(
		&self,
		draft: &DraftRelationship,
		source: SymbolKind,
		target: SymbolKind,
	) -> Checked<()> {
		let allowed = |kind: SymbolKind| {
			matches!(kind, SymbolKind::Actor | SymbolKind::Usecase | SymbolKind::Json)
		};
		for (endpoint, kind, role) in
			[(&draft.source, source, "source"), (&draft.target, target, "target")]
		{
			if !allowed(kind) {
				let previous =
					self.symbols.get(&endpoint.id).map_or(endpoint.location, |s| s.location);
				return Err(conflict(
					&format!("Relationship {role} '{}' cannot be {}", endpoint.id, kind.name()),
					endpoint.location,
					previous,
					(None, None),
				));
			}
		}
		let line = draft.location.line;
		let relationship = draft.relationship_type;
		if matches!(relationship, RelationshipType::Include | RelationshipType::Extend)
			&& (source != SymbolKind::Usecase || target != SymbolKind::Usecase)
		{
			return Err(error(
				line,
				format!("{} relationship requires use-case endpoints", relationship.name()),
			));
		}
		if relationship == RelationshipType::Generalization
			&& (!matches!(source, SymbolKind::Actor | SymbolKind::Usecase) || source != target)
		{
			return Err(error(
				line,
				"Generalization requires actor-to-actor or use-case-to-use-case endpoints",
			));
		}
		let is_point_or_line = matches!(
			draft.arrow_type,
			ArrowType::SolidArrow | ArrowType::BackArrow | ArrowType::LineSolid
		);
		if relationship == RelationshipType::Association
			&& (source == SymbolKind::Json || target == SymbolKind::Json)
			&& !is_point_or_line
		{
			return Err(error(
				line,
				format!(
					"JSON relationship '{}' to '{}' permits only point, reversed-point, or markerless solid association",
					draft.source.id, draft.target.id
				),
			));
		}
		Ok(())
	}

	/// `setElementModel` for every state, in the order of first appearance.
	fn materialize(&self) -> (Vec<Element>, Vec<Element>) {
		let mut states: Vec<&ElementState> = self.elements.values().collect();
		npp::sort::sort_by_key(&mut states[..], |state| self.first_of(&state.id));
		let mut actors = Vec::new();
		let mut use_cases = Vec::new();
		for state in states {
			let element = Element {
				id: state.id.clone(),
				label: state.label.clone(),
				actor_type: match state.icon {
					Some(_) => ActorType::Icon,
					None => state.actor_type.unwrap_or(ActorType::Normal),
				},
				icon: state.icon.clone(),
				shape: state.shape.unwrap_or(UseCaseShape::Ellipse),
				business: state.business.unwrap_or(false),
				stereotype: state.stereotype.clone(),
				parent: state.parent.as_ref().map(|(parent, _)| parent.clone()),
				classes: state.classes.clone(),
				styles: Vec::new(),
			};
			match state.kind {
				ElementKind::Actor => actors.push(element),
				ElementKind::Usecase => use_cases.push(element),
			}
		}
		(actors, use_cases)
	}
}

fn apply_declaration_metadata(
	state: &mut ElementState,
	metadata: &[MetadataProperty],
) -> Checked<()> {
	for property in metadata {
		match state.kind {
			ElementKind::Actor => apply_actor_property(state, property, false)?,
			ElementKind::Usecase => match (&property.key[..], &property.value) {
				("business", &MetadataValue::Bool(value)) => {
					if state.business.is_some_and(|known| known != value) {
						return Err(conflict(
							&format!("Use case '{}' has conflicting business metadata", state.id),
							property.location,
							state.location,
							(None, None),
						));
					}
					state.business = Some(value);
				}
				_ => return Err(invalid_metadata(&state.id, state.kind.name(), property)),
			},
		}
	}
	Ok(())
}

fn apply_actor_property(
	state: &mut ElementState,
	property: &MetadataProperty,
	replace: bool,
) -> Checked<()> {
	let conflicting = |what: &str| {
		conflict(
			&format!("Actor '{}' has conflicting {what} metadata", state.id),
			property.location,
			state.location,
			(None, None),
		)
	};
	match (&property.key[..], &property.value) {
		("type", MetadataValue::Text(value))
			if matches!(&value[..], "normal" | "hollow" | "awesome") =>
		{
			let actor_type = match &value[..] {
				"hollow" => ActorType::Hollow,
				"awesome" => ActorType::Awesome,
				_ => ActorType::Normal,
			};
			if !replace && state.actor_type.is_some_and(|known| known != actor_type) {
				return Err(conflicting("type"));
			}
			state.actor_type = Some(actor_type);
		}
		("icon", MetadataValue::Text(value)) => {
			if !replace && state.icon.as_ref().is_some_and(|known| known != value) {
				return Err(conflicting("icon"));
			}
			state.icon = Some(value.clone());
		}
		("business", &MetadataValue::Bool(value)) => {
			if !replace && state.business.is_some_and(|known| known != value) {
				return Err(conflicting("business"));
			}
			state.business = Some(value);
		}
		_ => return Err(invalid_metadata(&state.id, state.kind.name(), property)),
	}
	Ok(())
}

/// `inferMetadataKind`.
fn infer_metadata_kind(properties: &[MetadataProperty]) -> Option<&'static str> {
	let mut possible = vec!["actor", "usecase", "boundary", "edge"];
	for property in properties {
		let value = match &property.value {
			MetadataValue::Text(text) => Some(text.as_str()),
			MetadataValue::Bool(_) => None,
		};
		match &property.key[..] {
			"icon" => possible = vec!["actor"],
			"animate" | "animation" => possible = vec!["edge"],
			"type" => {
				possible = match value {
					Some("rect" | "package") => vec!["boundary"],
					Some("normal" | "hollow" | "awesome") => vec!["actor"],
					_ => Vec::new(),
				}
			}
			"business" => possible.retain(|kind| !matches!(*kind, "boundary" | "edge")),
			_ => return None,
		}
	}
	(possible.len() == 1).then(|| possible[0])
}

/// `finalize`: the model of `drafts`.
pub fn build(drafts: &Drafts) -> Result<Model, ModelError> {
	let mut builder = Builder {
		drafts,
		symbols: HashMap::new(),
		elements: Ordered::new(),
		boundaries: Ordered::new(),
		first: HashMap::new(),
	};
	for relation in &drafts.relationships {
		builder.record_first(&relation.source.id, relation.source.location.offset);
		builder.record_first(&relation.target.id, relation.target.location.offset);
	}
	for draft in &drafts.elements {
		builder.record_first(&draft.id, draft.location.offset);
	}
	for draft in &drafts.boundaries {
		builder.record_first(&draft.id, draft.location.offset);
	}
	for draft in &drafts.jsons {
		builder.record_first(&draft.id, draft.location.offset);
	}

	// Declarations in source order.
	enum Declaration {
		Element(usize),
		Boundary(usize),
		Json(usize),
		Edge(usize),
	}
	let mut declarations: Vec<(usize, Declaration)> = Vec::new();
	declarations.extend(
		drafts
			.elements
			.iter()
			.enumerate()
			.map(|(i, d)| (d.location.offset, Declaration::Element(i))),
	);
	declarations.extend(
		drafts
			.boundaries
			.iter()
			.enumerate()
			.map(|(i, d)| (d.location.offset, Declaration::Boundary(i))),
	);
	declarations.extend(
		drafts.jsons.iter().enumerate().map(|(i, d)| (d.location.offset, Declaration::Json(i))),
	);
	for (index, relation) in drafts.relationships.iter().enumerate() {
		if let Some((_, location)) = &relation.explicit_id {
			declarations.push((location.offset, Declaration::Edge(index)));
		}
	}
	npp::sort::sort_by_key(&mut declarations[..], |(offset, _)| *offset);
	for (_, declaration) in &declarations {
		match *declaration {
			Declaration::Element(index) => builder.collect_element(&drafts.elements[index])?,
			Declaration::Boundary(index) => builder.collect_boundary(index)?,
			Declaration::Json(index) => {
				let draft = &drafts.jsons[index];
				builder.register_unique(&draft.id, SymbolKind::Json, draft.location)?;
			}
			Declaration::Edge(index) => {
				if let Some((id, location)) = &drafts.relationships[index].explicit_id {
					builder.register_unique(id, SymbolKind::Edge, *location)?;
				}
			}
		}
	}

	let mut model = Model { direction: "LR", ..Model::default() };
	let mut boundary_order: Vec<&BoundaryState> = builder.boundaries.values().collect();
	npp::sort::sort_by_key(&mut boundary_order[..], |state| builder.first_of(&state.id));
	let boundary_ids: Vec<String> = boundary_order.iter().map(|state| state.id.clone()).collect();
	let mut jsons: Vec<&super::parser::DraftJson> = drafts.jsons.iter().collect();
	npp::sort::sort_by_key(&mut jsons[..], |draft| builder.first_of(&draft.id));
	model.jsons = jsons
		.into_iter()
		.map(|draft| JsonNode {
			id: draft.id.clone(),
			value: draft.value.clone(),
			classes: draft.classes.clone(),
			styles: Vec::new(),
		})
		.collect();

	let mut anonymous = 0;
	for draft in &drafts.relationships {
		let source = builder.resolve_endpoint(&draft.source)?;
		let target = builder.resolve_endpoint(&draft.target)?;
		builder.validate_relationship(draft, source, target)?;
		let id = match &draft.explicit_id {
			Some((id, _)) => id.clone(),
			None => {
				anonymous += 1;
				format!("edge-{}", anonymous - 1)
			}
		};
		model.relationships.push(Relationship {
			id,
			is_explicit: draft.explicit_id.is_some(),
			source: draft.source.id.clone(),
			target: draft.target.id.clone(),
			relationship_type: draft.relationship_type,
			arrow_type: draft.arrow_type,
			label: draft.label.clone(),
			minlen: draft.minlen,
			classes: Vec::new(),
			styles: Vec::new(),
			animate: false,
			animation: None,
		});
	}

	// `applyMetadataAssignments`.
	for assignment in &drafts.metadata_assignments {
		let Some(origin) = builder.symbols.get(&assignment.target).cloned() else {
			let implied = infer_metadata_kind(&assignment.properties)
				.map(|kind| format!(" (metadata implies {kind})"))
				.unwrap_or_default();
			return Err(error(
				assignment.target_location.line,
				format!("Metadata target '{}' is unresolved{implied}", assignment.target),
			));
		};
		match origin.kind {
			SymbolKind::Actor | SymbolKind::Usecase => {
				let Some(state) = builder.elements.get_mut(&assignment.target) else { continue };
				for property in &assignment.properties {
					match state.kind {
						ElementKind::Actor => apply_actor_property(state, property, true)?,
						ElementKind::Usecase => match (&property.key[..], &property.value) {
							("business", &MetadataValue::Bool(value)) => {
								state.business = Some(value)
							}
							_ => {
								return Err(invalid_metadata(
									&state.id,
									state.kind.name(),
									property,
								));
							}
						},
					}
				}
			}
			SymbolKind::Boundary => {
				let Some(boundary) = builder.boundaries.get_mut(&assignment.target) else {
					continue;
				};
				for property in &assignment.properties {
					let boundary_type = match (&property.key[..], &property.value) {
						("type", MetadataValue::Text(value)) if value == "rect" => {
							BoundaryType::Rect
						}
						("type", MetadataValue::Text(value)) if value == "package" => {
							BoundaryType::Package
						}
						_ => {
							return Err(invalid_metadata(&assignment.target, "boundary", property));
						}
					};
					boundary.boundary_type = Some(boundary_type);
				}
			}
			SymbolKind::Edge => {
				let Some(relationship) = model
					.relationships
					.iter_mut()
					.find(|relationship| relationship.id == assignment.target)
				else {
					return Err(error(
						assignment.target_location.line,
						format!("Metadata target '{}' is not an explicit edge", assignment.target),
					));
				};
				for property in &assignment.properties {
					match (&property.key[..], &property.value) {
						("animate", &MetadataValue::Bool(value)) => relationship.animate = value,
						("animation", MetadataValue::Text(value))
							if matches!(&value[..], "fast" | "slow") =>
						{
							relationship.animation = Some(value.clone());
							relationship.animate = true;
						}
						_ => return Err(invalid_metadata(&assignment.target, "edge", property)),
					}
				}
			}
			SymbolKind::Json => {
				if let Some(property) = assignment.properties.first() {
					return Err(invalid_metadata(&assignment.target, "json", property));
				}
			}
		}
	}
	for relationship in &mut model.relationships {
		if relationship.animation.is_some() {
			relationship.animate = true;
		}
	}

	// `validateAndRefreshElements`.
	for state in builder.elements.values() {
		match state.kind {
			ElementKind::Actor => {
				if let (Some(_), Some(actor_type)) = (&state.icon, state.actor_type)
					&& actor_type != ActorType::Normal
				{
					return Err(error(
						state.location.line,
						format!(
							"Actor '{}' cannot combine icon with type '{}'",
							state.id,
							actor_type.name()
						),
					));
				}
				let actor_type = match state.icon {
					Some(_) => ActorType::Icon,
					None => state.actor_type.unwrap_or(ActorType::Normal),
				};
				if state.business == Some(true)
					&& matches!(actor_type, ActorType::Icon | ActorType::Awesome)
				{
					return Err(error(
						state.location.line,
						format!("Business actor '{}' must use normal or hollow geometry", state.id),
					));
				}
			}
			ElementKind::Usecase => {
				if state.shape == Some(UseCaseShape::Rect) && state.business == Some(true) {
					return Err(error(
						state.location.line,
						format!(
							"Rectangular use case '{}' cannot be a business use case",
							state.id
						),
					));
				}
			}
		}
	}
	let (actors, use_cases) = builder.materialize();
	model.actors = actors;
	model.use_cases = use_cases;
	model.boundaries = boundary_ids
		.iter()
		.filter_map(|id| builder.boundaries.get(id))
		.map(|state| Boundary {
			id: state.id.clone(),
			label: state.label.clone(),
			boundary_type: state.boundary_type.unwrap_or(BoundaryType::Rect),
			classes: state.classes.clone(),
			styles: Vec::new(),
		})
		.collect();

	// `materializeNotes`.
	for (index, draft) in drafts.notes.iter().enumerate() {
		let Some(origin) = builder.symbols.get(&draft.target) else {
			return Err(error(
				draft.target_location.line,
				format!("Note target '{}' is unresolved", draft.target),
			));
		};
		if !matches!(origin.kind, SymbolKind::Actor | SymbolKind::Usecase) {
			return Err(conflict(
				&format!(
					"Note target '{}' must be an actor or use case, not {}",
					draft.target,
					origin.kind.name()
				),
				draft.target_location,
				origin.location,
				(None, None),
			));
		}
		model.notes.push(Note {
			id: format!("note-{index}"),
			target: draft.target.clone(),
			label: draft.label.clone(),
		});
	}

	for (ids, styles) in &drafts.class_definitions {
		for id in ids {
			match model.class_defs.iter_mut().find(|(known, _)| known == id) {
				Some(entry) => entry.1 = styles.clone(),
				None => model.class_defs.push((id.clone(), styles.clone())),
			}
		}
	}

	// `applyClassesAndStyles`.
	enum Stylable<'m> {
		Element(&'m mut Element),
		Boundary(&'m mut Boundary),
		Json(&'m mut JsonNode),
		Edge(&'m mut Relationship),
	}
	fn stylable<'m>(
		model: &'m mut Model,
		symbols: &HashMap<String, Symbol>,
		id: &str,
		location: Location,
	) -> Checked<(&'m mut Vec<String>, &'m mut Vec<String>)> {
		let target = match symbols.get(id).map(|symbol| symbol.kind) {
			Some(SymbolKind::Actor) => {
				model.actors.iter_mut().find(|e| e.id == id).map(Stylable::Element)
			}
			Some(SymbolKind::Usecase) => {
				model.use_cases.iter_mut().find(|e| e.id == id).map(Stylable::Element)
			}
			Some(SymbolKind::Boundary) => {
				model.boundaries.iter_mut().find(|b| b.id == id).map(Stylable::Boundary)
			}
			Some(SymbolKind::Json) => {
				model.jsons.iter_mut().find(|j| j.id == id).map(Stylable::Json)
			}
			Some(SymbolKind::Edge) => model
				.relationships
				.iter_mut()
				.find(|r| r.id == id && r.is_explicit)
				.map(Stylable::Edge),
			None => None,
		};
		match target {
			Some(Stylable::Element(e)) => Ok((&mut e.classes, &mut e.styles)),
			Some(Stylable::Boundary(b)) => Ok((&mut b.classes, &mut b.styles)),
			Some(Stylable::Json(j)) => Ok((&mut j.classes, &mut j.styles)),
			Some(Stylable::Edge(r)) => Ok((&mut r.classes, &mut r.styles)),
			None => Err(error(
				location.line,
				format!("Class/style target '{id}' is unresolved or anonymous"),
			)),
		}
	}
	for (targets, classes) in &drafts.class_assignments {
		for (id, location) in targets {
			let (target_classes, _) = stylable(&mut model, &builder.symbols, id, *location)?;
			push_unique(target_classes, classes);
		}
	}
	for (id, location, styles) in &drafts.style_assignments {
		let (_, target_styles) = stylable(&mut model, &builder.symbols, id, *location)?;
		target_styles.extend(styles.iter().cloned());
	}
	if let Some(direction) = drafts.directions.last() {
		model.direction = direction;
	}
	model.acc_title = drafts.acc_title.clone();
	model.acc_description = drafts.acc_description.clone();
	let _ = LabelType::Text;
	Ok(model)
}
