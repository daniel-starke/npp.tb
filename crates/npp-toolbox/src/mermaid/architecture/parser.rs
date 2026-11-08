//! The architecture grammar (`architecture.langium`, `arch.langium`, `valueConverter.ts`).
//!
//! The direction terminal `L|R|T|B` comes before `ID` in the lexer, so ids cannot start with
//! these capitals, and the bracketed title takes `["…"]` before a quoted icon text would.

use super::super::common::CommonFields;
use super::super::langium::Scanner;
use super::super::{Context, Result};

const KEYWORDS: &[&str] = &["group", "service", "junction", "in", "align", "row", "column"];
const DIRECTIONS: &[&str] = &["L", "R", "T", "B"];

/// `L`, `R`, `T` or `B`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
	Left,
	Right,
	Top,
	Bottom,
}

impl Direction {
	pub fn is_x(self) -> bool {
		matches!(self, Direction::Left | Direction::Right)
	}

	pub fn is_y(self) -> bool {
		!self.is_x()
	}
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
	pub id: String,
	pub icon: Option<String>,
	pub title: Option<String>,
	pub parent: Option<String>,
	pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Service {
	pub id: String,
	pub icon: Option<String>,
	pub icon_text: Option<String>,
	pub title: Option<String>,
	pub parent: Option<String>,
	pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Junction {
	pub id: String,
	pub parent: Option<String>,
	pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
	pub lhs_id: String,
	pub lhs_dir: Direction,
	pub lhs_into: bool,
	pub lhs_group: bool,
	pub rhs_id: String,
	pub rhs_dir: Direction,
	pub rhs_into: bool,
	pub rhs_group: bool,
	pub title: Option<String>,
	pub line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alignment {
	pub is_row: bool,
	pub members: Vec<String>,
	pub line: usize,
}

/// The AST: Langium collects each kind of statement in its own list.
#[derive(Debug)]
pub struct Architecture {
	pub common: CommonFields,
	pub groups: Vec<Group>,
	pub services: Vec<Service>,
	pub junctions: Vec<Junction>,
	pub edges: Vec<Edge>,
	pub alignments: Vec<Alignment>,
}

pub fn parse(code: &str, context: &Context) -> Result<Architecture> {
	let mut architecture = Architecture {
		common: CommonFields::new(context),
		groups: Vec::new(),
		services: Vec::new(),
		junctions: Vec::new(),
		edges: Vec::new(),
		alignments: Vec::new(),
	};
	let mut scanner = Scanner::new(code);
	scanner.skip_newlines();
	if !scanner.keyword("architecture-beta", false) {
		return Err(scanner.error(context, "`architecture-beta`"));
	}
	loop {
		if scanner.is_at_end() {
			break;
		}
		if scanner.newline() {
			continue;
		}
		if scanner.common_field(&mut architecture.common) {
			end_of_line(&mut scanner, context)?;
			continue;
		}
		let line = scanner.line();
		if scanner.keyword("group", false) {
			let id = id(&mut scanner, context)?;
			let icon = icon(&mut scanner);
			let title = title(&mut scanner);
			let parent = parent(&mut scanner, context)?;
			end_of_line(&mut scanner, context)?;
			architecture.groups.push(Group { id, icon, title, parent, line });
		} else if scanner.keyword("service", false) {
			let id = id(&mut scanner, context)?;
			let (mut icon, mut icon_text) = (None, None);
			match scanner.rest().trim_start_matches([' ', '\t']).starts_with(['"', '\'']) {
				true => icon_text = scanner.string(),
				false => icon = self::icon(&mut scanner),
			}
			let title = title(&mut scanner);
			let parent = parent(&mut scanner, context)?;
			end_of_line(&mut scanner, context)?;
			architecture.services.push(Service { id, icon, icon_text, title, parent, line });
		} else if scanner.keyword("junction", false) {
			let id = id(&mut scanner, context)?;
			let parent = parent(&mut scanner, context)?;
			end_of_line(&mut scanner, context)?;
			architecture.junctions.push(Junction { id, parent, line });
		} else if scanner.keyword("align", false) {
			let is_row = match () {
				_ if scanner.keyword("row", false) => true,
				_ if scanner.keyword("column", false) => false,
				_ => return Err(scanner.error(context, "`row` or `column`")),
			};
			let mut members = vec![id(&mut scanner, context)?, id(&mut scanner, context)?];
			while let Some(member) = scanner.id_without_numbers(KEYWORDS, DIRECTIONS) {
				members.push(member.to_owned());
			}
			end_of_line(&mut scanner, context)?;
			architecture.alignments.push(Alignment { is_row, members, line });
		} else if let Some(lhs_id) = scanner.id_without_numbers(KEYWORDS, DIRECTIONS) {
			let edge = edge(&mut scanner, context, lhs_id.to_owned(), line)?;
			architecture.edges.push(edge);
		} else {
			return Err(scanner.error(context, "a statement"));
		}
	}
	Ok(architecture)
}

fn end_of_line(scanner: &mut Scanner, context: &Context) -> Result<()> {
	match scanner.end_of_line() {
		true => Ok(()),
		false => Err(scanner.error(context, "a new line")),
	}
}

fn id(scanner: &mut Scanner, context: &Context) -> Result<String> {
	match scanner.id_without_numbers(KEYWORDS, DIRECTIONS) {
		Some(id) => Ok(id.to_owned()),
		None => Err(scanner.error(context, "an id")),
	}
}

fn parent(scanner: &mut Scanner, context: &Context) -> Result<Option<String>> {
	match scanner.keyword("in", false) {
		true => id(scanner, context).map(Some),
		false => Ok(None),
	}
}

/// `ARCH_ICON`: `\([\w-:]+\)`, without the parentheses.
fn icon(scanner: &mut Scanner) -> Option<String> {
	scanner.skip_hidden();
	let rest = scanner.rest().strip_prefix('(')?;
	let length = rest
		.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':'))
		.unwrap_or(rest.len());
	if length == 0 || !rest[length..].starts_with(')') {
		return None;
	}
	let icon = rest[..length].trim().to_owned();
	scanner.punctuation(&format!("({})", &rest[..length]));
	Some(icon)
}

/// `ARCH_TITLE`: `\[(?:"([^"\\]|\\.)*"|'([^'\\]|\\.)*'|[^\[\]\r\n]+)\]`, converted.
fn title(scanner: &mut Scanner) -> Option<String> {
	scanner.skip_hidden();
	let rest = scanner.rest();
	let length = title_length(rest)?;
	let raw = &rest[1..length - 1];
	scanner.punctuation(&rest[..length]);
	let mut value = raw.trim();
	let is_quoted = value.len() >= 2
		&& ((value.starts_with('"') && value.ends_with('"'))
			|| (value.starts_with('\'') && value.ends_with('\'')));
	let unescaped;
	if is_quoted {
		value = &value[1..value.len() - 1];
		unescaped = value.replace("\\\"", "\"").replace("\\'", "'");
		return Some(unescaped.trim().to_owned());
	}
	Some(value.to_owned())
}

/// The length of an `ARCH_TITLE` match at the start of `text`, with the regex's backtracking
/// from a quoted title to a plain one.
fn title_length(text: &str) -> Option<usize> {
	let inner = text.strip_prefix('[')?;
	if let Some(quote) = inner.chars().next().filter(|&c| c == '"' || c == '\'') {
		let mut characters = inner.char_indices().skip(1);
		while let Some((index, character)) = characters.next() {
			if character == '\\' {
				characters.next();
			} else if character == quote {
				if inner[index + 1..].starts_with(']') {
					return Some(1 + index + 2);
				}
				break;
			}
		}
	}
	let length = inner.find(['[', ']', '\r', '\n']).unwrap_or(inner.len());
	(length > 0 && inner[length..].starts_with(']')).then_some(1 + length + 1)
}

/// `lhsId lhsGroup? ':' lhsDir lhsInto? ('--' | '-' title '-') rhsInto? rhsDir ':' rhsId
/// rhsGroup? EOL`.
fn edge(scanner: &mut Scanner, context: &Context, lhs_id: String, line: usize) -> Result<Edge> {
	let lhs_group = scanner.punctuation("{group}");
	if !scanner.punctuation(":") {
		return Err(scanner.error(context, "`:`"));
	}
	let lhs_dir = direction(scanner, context)?;
	let lhs_into = arrow_into(scanner);
	let title = match scanner.punctuation("--") {
		true => None,
		false => {
			if !scanner.punctuation("-") {
				return Err(scanner.error(context, "`--` or `-[title]-`"));
			}
			let title = self::title(scanner).ok_or_else(|| scanner.error(context, "`[title]`"))?;
			if !scanner.punctuation("-") {
				return Err(scanner.error(context, "`-`"));
			}
			Some(title)
		}
	};
	let rhs_into = arrow_into(scanner);
	let rhs_dir = direction(scanner, context)?;
	if !scanner.punctuation(":") {
		return Err(scanner.error(context, "`:`"));
	}
	let rhs_id = id(scanner, context)?;
	let rhs_group = scanner.punctuation("{group}");
	end_of_line(scanner, context)?;
	Ok(Edge {
		lhs_id,
		lhs_dir,
		lhs_into,
		lhs_group,
		rhs_id,
		rhs_dir,
		rhs_into,
		rhs_group,
		title,
		line,
	})
}

fn direction(scanner: &mut Scanner, context: &Context) -> Result<Direction> {
	for (letter, direction) in [
		("L", Direction::Left),
		("R", Direction::Right),
		("T", Direction::Top),
		("B", Direction::Bottom),
	] {
		if scanner.punctuation(letter) {
			return Ok(direction);
		}
	}
	Err(scanner.error(context, "a direction `L`, `R`, `T` or `B`"))
}

/// `ARROW_INTO`: `<|>`.
fn arrow_into(scanner: &mut Scanner) -> bool {
	scanner.punctuation("<") || scanner.punctuation(">")
}

#[cfg(test)]
mod tests {
	use super::super::super::text::testing::FixedMeasure;
	use super::*;

	fn parse_code(code: &str) -> Result<Architecture> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_statements() {
		let architecture = parse_code(
			"architecture-beta\n  title T\n  group api(cloud)[API] in outer\n  service db(database)[\"Data \\\"base\\\"\"] in api\n  service s \"text\" [ Title ]\n  junction j in api\n  db{group}:L <-[label]-> R:s\n  db:T -- B:j\n  align row db s j\n",
		)
		.unwrap();
		assert_eq!(architecture.common.title.as_deref(), Some("T"));
		assert_eq!(
			architecture.groups,
			vec![Group {
				id: "api".to_owned(),
				icon: Some("cloud".to_owned()),
				title: Some("API".to_owned()),
				parent: Some("outer".to_owned()),
				line: 3,
			}]
		);
		assert_eq!(architecture.services[0].title.as_deref(), Some("Data \"base\""));
		assert_eq!(architecture.services[1].icon_text.as_deref(), Some("text"));
		assert_eq!(architecture.services[1].title.as_deref(), Some("Title"));
		assert_eq!(architecture.junctions[0].parent.as_deref(), Some("api"));
		let edge = &architecture.edges[0];
		assert!(edge.lhs_group && edge.lhs_into && edge.rhs_into && !edge.rhs_group);
		assert_eq!((edge.lhs_dir, edge.rhs_dir), (Direction::Left, Direction::Right));
		assert_eq!(edge.title.as_deref(), Some("label"));
		assert_eq!(architecture.edges[1].line, 8);
		assert_eq!(architecture.alignments[0].members, ["db", "s", "j"]);
	}

	#[test]
	fn rejects_what_langium_rejects() {
		for code in [
			"architecture\n  service a(server)\n",
			"architecture-beta\n  service Lambda(server)\n",
			"architecture-beta\n  service a[\"x\"][y]\n",
			"architecture-beta\n  service a(server) service b(server)\n",
			"architecture-beta\n  align row a\n",
			"architecture-beta\n  a:X -- L:b\n",
			"architecture-beta\n  service in(server)\n",
		] {
			assert!(parse_code(code).is_err(), "{code}");
		}
		assert!(parse_code("architecture-beta service a(server)").is_ok());
		assert_eq!(title_length("[\"a\" b]"), Some(7));
		assert_eq!(title_length("[a[b]"), None);
	}
}
