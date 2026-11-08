//! The C4 grammar (`c4Diagram.jison`): a hand-written lexer with jison's start conditions and
//! first-match rules, and the grammar actions on [`C4Db`].

use super::db::{Attribute, C4Db};

#[derive(Debug, PartialEq)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type ParseResult<T> = Result<T, ParseError>;

/// The statements that take arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keyword {
	Element(&'static str),
	/// Containers and components, which take a technology.
	ElementWithTechn(&'static str),
	/// The default type and the type the grammar inserts as third argument.
	Boundary(&'static str, Option<&'static str>),
	ContainerBoundary,
	DeploymentNode(&'static str),
	Rel(&'static str),
	RelIndex,
	UpdateElementStyle,
	UpdateRelStyle,
	UpdateLayoutConfig,
}

/// The keywords of the initial condition after the diagram types, in rule order. Each ends
/// with jison's implicit `\b`.
const KEYWORDS: [(&str, Keyword); 43] = [
	("Person_Ext", Keyword::Element("external_person")),
	("Person", Keyword::Element("person")),
	("SystemQueue_Ext", Keyword::Element("external_system_queue")),
	("SystemDb_Ext", Keyword::Element("external_system_db")),
	("System_Ext", Keyword::Element("external_system")),
	("SystemQueue", Keyword::Element("system_queue")),
	("SystemDb", Keyword::Element("system_db")),
	("System", Keyword::Element("system")),
	("Boundary", Keyword::Boundary("system", None)),
	("Enterprise_Boundary", Keyword::Boundary("system", Some("ENTERPRISE"))),
	("System_Boundary", Keyword::Boundary("system", Some("SYSTEM"))),
	("ContainerQueue_Ext", Keyword::ElementWithTechn("external_container_queue")),
	("ContainerDb_Ext", Keyword::ElementWithTechn("external_container_db")),
	("Container_Ext", Keyword::ElementWithTechn("external_container")),
	("ContainerQueue", Keyword::ElementWithTechn("container_queue")),
	("ContainerDb", Keyword::ElementWithTechn("container_db")),
	("Container", Keyword::ElementWithTechn("container")),
	("Container_Boundary", Keyword::ContainerBoundary),
	("ComponentQueue_Ext", Keyword::ElementWithTechn("external_component_queue")),
	("ComponentDb_Ext", Keyword::ElementWithTechn("external_component_db")),
	("Component_Ext", Keyword::ElementWithTechn("external_component")),
	("ComponentQueue", Keyword::ElementWithTechn("component_queue")),
	("ComponentDb", Keyword::ElementWithTechn("component_db")),
	("Component", Keyword::ElementWithTechn("component")),
	("Deployment_Node", Keyword::DeploymentNode("node")),
	("Node", Keyword::DeploymentNode("node")),
	("Node_L", Keyword::DeploymentNode("nodeL")),
	("Node_R", Keyword::DeploymentNode("nodeR")),
	("Rel", Keyword::Rel("rel")),
	("BiRel", Keyword::Rel("birel")),
	("Rel_Up", Keyword::Rel("rel_u")),
	("Rel_U", Keyword::Rel("rel_u")),
	("Rel_Down", Keyword::Rel("rel_d")),
	("Rel_D", Keyword::Rel("rel_d")),
	("Rel_Left", Keyword::Rel("rel_l")),
	("Rel_L", Keyword::Rel("rel_l")),
	("Rel_Right", Keyword::Rel("rel_r")),
	("Rel_R", Keyword::Rel("rel_r")),
	("Rel_Back", Keyword::Rel("rel_b")),
	("RelIndex", Keyword::RelIndex),
	("UpdateElementStyle", Keyword::UpdateElementStyle),
	("UpdateRelStyle", Keyword::UpdateRelStyle),
	("UpdateLayoutConfig", Keyword::UpdateLayoutConfig),
];

const DIAGRAM_TYPES: [&str; 5] =
	["C4Context", "C4Container", "C4Component", "C4Dynamic", "C4Deployment"];

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
	Direction,
	Title,
	AccDescription,
	AccTitle,
	AccTitleValue,
	AccDescr,
	AccDescrValue,
	AccDescrMultilineValue,
	Newline,
	DiagramType,
	Keyword(Keyword),
	EofInStruct,
	AttributeEmpty,
	Str,
	StrKey,
	StrValue,
	LeftBrace,
	RightBrace,
	Eof,
	/// jison's own end of input (`$end`) in conditions without an `<<EOF>>` rule, which the
	/// grammar never accepts.
	End,
	Invalid,
}

#[derive(Clone, Debug)]
struct Token {
	kind: TokenKind,
	text: String,
	line: usize,
}

/// The lexer's start conditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Condition {
	Initial,
	/// After a keyword that takes arguments, before its `(`.
	Struct,
	Attribute,
	String,
	StringKv,
	StringKvKey,
	StringKvValue,
	AccTitle,
	AccDescr,
	AccDescrMultiline,
}

struct Lexer<'a> {
	input: &'a str,
	position: usize,
	line: usize,
	conditions: Vec<Condition>,
	/// jison's `done`: the end was reached, so the next token is the end.
	is_done: bool,
}

/// JavaScript's `\s`.
fn is_space(character: char) -> bool {
	character.is_whitespace() || character == '\u{feff}'
}

fn space_length(text: &str) -> usize {
	text.find(|character: char| !is_space(character)).unwrap_or(text.len())
}

/// `keyword` followed by jison's `\b`.
fn starts_with_keyword(text: &str, keyword: &str) -> bool {
	text.starts_with(keyword)
		&& !text[keyword.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
}

impl<'a> Lexer<'a> {
	fn new(input: &'a str, line: usize) -> Self {
		Self { input, position: 0, line, conditions: vec![Condition::Initial], is_done: false }
	}

	fn condition(&self) -> Condition {
		self.conditions.last().copied().unwrap_or(Condition::Initial)
	}

	fn begin(&mut self, condition: Condition) {
		self.conditions.push(condition);
	}

	/// `popState`, which never removes the initial condition.
	fn pop(&mut self) {
		if self.conditions.len() > 1 {
			self.conditions.pop();
		}
	}

	fn take(&mut self, length: usize) -> String {
		let text = &self.input[self.position..self.position + length];
		self.position += length;
		self.line += text.matches('\n').count();
		text.to_owned()
	}

	fn token(&mut self, kind: TokenKind, length: usize) -> Token {
		let line = self.line;
		let text = self.take(length);
		Token { kind, text, line }
	}

	fn next_token(&mut self) -> Token {
		loop {
			if self.is_done {
				return Token { kind: TokenKind::End, text: String::new(), line: self.line };
			}
			let rest = &self.input[self.position..];
			if rest.is_empty() {
				self.is_done = true;
			}
			if let Some(token) = self.rule(rest) {
				return token;
			}
		}
	}

	/// The first rule of the current condition that matches `rest`: a token, `None` for
	/// skipped input.
	fn rule(&mut self, rest: &str) -> Option<Token> {
		let line = self.line;
		let invalid = || Some(Token { kind: TokenKind::Invalid, text: String::new(), line });
		let end = || Some(Token { kind: TokenKind::End, text: String::new(), line });
		match self.condition() {
			Condition::Initial => self.initial_rule(rest),
			Condition::Struct => {
				if rest.is_empty() {
					return Some(self.token(TokenKind::EofInStruct, 0));
				}
				if let Some(after) = rest.strip_prefix('(') {
					let spaces = after.len() - after.trim_start_matches(' ').len();
					self.begin(Condition::Attribute);
					if after[spaces..].starts_with(',') {
						return Some(self.token(TokenKind::AttributeEmpty, 1 + spaces + 1));
					}
					self.take(1);
					return None;
				}
				if rest.starts_with(')') {
					self.take(1);
					self.pop();
					self.pop();
					return None;
				}
				invalid()
			}
			Condition::Attribute => {
				if rest.starts_with(')') {
					self.take(1);
					self.pop();
					self.pop();
					return None;
				}
				if rest.starts_with(",,") {
					return Some(self.token(TokenKind::AttributeEmpty, 2));
				}
				if rest.starts_with(',') {
					self.take(1);
					return None;
				}
				let spaces = rest.len() - rest.trim_start_matches(' ').len();
				let after = &rest[spaces..];
				if after.starts_with("\"\"") {
					return Some(self.token(TokenKind::AttributeEmpty, spaces + 2));
				}
				if after.starts_with('"') {
					self.take(spaces + 1);
					self.begin(Condition::String);
					return None;
				}
				if after.starts_with('$') {
					self.take(spaces + 1);
					self.begin(Condition::StringKv);
					return None;
				}
				let length = rest.find(',').unwrap_or(rest.len());
				if length == 0 {
					return end();
				}
				Some(self.token(TokenKind::Str, length))
			}
			Condition::String => {
				if rest.starts_with('"') {
					self.take(1);
					self.pop();
					return None;
				}
				let length = rest.find('"').unwrap_or(rest.len());
				Some(self.token(TokenKind::Str, length))
			}
			Condition::StringKv => {
				let length = rest.find('=').unwrap_or(rest.len());
				self.begin(Condition::StringKvKey);
				Some(self.token(TokenKind::StrKey, length))
			}
			Condition::StringKvKey => {
				if rest.is_empty() {
					return end();
				}
				let Some(after) = rest.strip_prefix('=') else { return invalid() };
				let spaces = after.len() - after.trim_start_matches(' ').len();
				if !after[spaces..].starts_with('"') {
					return invalid();
				}
				self.take(1 + spaces + 1);
				self.pop();
				self.begin(Condition::StringKvValue);
				None
			}
			Condition::StringKvValue => {
				if rest.starts_with('"') {
					self.take(1);
					self.pop();
					self.pop();
					return None;
				}
				let length = rest.find('"').unwrap_or(rest.len());
				if length == 0 {
					return end();
				}
				Some(self.token(TokenKind::StrValue, length))
			}
			Condition::AccTitle | Condition::AccDescr => {
				let kind = match self.condition() {
					Condition::AccTitle => TokenKind::AccTitleValue,
					_ => TokenKind::AccDescrValue,
				};
				self.pop();
				let length = rest.find('\n').unwrap_or(rest.len());
				Some(self.token(kind, length))
			}
			Condition::AccDescrMultiline => {
				if rest.starts_with('}') {
					self.take(1);
					self.pop();
					return None;
				}
				let length = rest.find('}').unwrap_or(rest.len());
				Some(self.token(TokenKind::AccDescrMultilineValue, length))
			}
		}
	}

	fn initial_rule(&mut self, rest: &str) -> Option<Token> {
		if rest.is_empty() {
			return Some(self.token(TokenKind::Eof, 0));
		}
		if let Some(length) = direction_length(rest) {
			return Some(self.token(TokenKind::Direction, length));
		}
		for (keyword, kind, prefix) in
			[("title", TokenKind::Title, 6), ("accDescription", TokenKind::AccDescription, 15)]
		{
			// `"title"\s[^#\n;]+`
			if let Some(after) = rest.strip_prefix(keyword)
				&& let Some(space) = after.chars().next().filter(|c| is_space(*c))
			{
				let value = &after[space.len_utf8()..];
				let length = value.find(['#', '\n', ';']).unwrap_or(value.len());
				if length > 0 {
					let mut token = self.token(kind, keyword.len() + space.len_utf8() + length);
					token.text = token.text.chars().skip(prefix).collect();
					return Some(token);
				}
			}
		}
		for (keyword, delimiter, condition) in [
			("accTitle", ':', Some(Condition::AccTitle)),
			("accDescr", ':', Some(Condition::AccDescr)),
			("accDescr", '{', None),
		] {
			if let Some(after) = rest.strip_prefix(keyword) {
				let spaces = space_length(after);
				if after[spaces..].starts_with(delimiter) {
					let value = &after[spaces + 1..];
					let length = keyword.len() + spaces + 1 + space_length(value);
					return match condition {
						Some(condition) => {
							let kind = match condition {
								Condition::AccTitle => TokenKind::AccTitle,
								_ => TokenKind::AccDescr,
							};
							let token = self.token(kind, length);
							self.begin(condition);
							Some(token)
						}
						None => {
							self.take(length);
							self.begin(Condition::AccDescrMultiline);
							None
						}
					};
				}
			}
		}
		if rest.starts_with("%%") {
			let line_end = rest.find('\n').unwrap_or(rest.len());
			let breaks =
				rest[line_end..].len() - rest[line_end..].trim_start_matches(['\r', '\n']).len();
			self.take(line_end + breaks);
			return None;
		}
		let spaces = space_length(rest);
		if spaces > 0 {
			// `\s*(\r?\n)+`: the blanks up to their last line end.
			return match rest[..spaces].rfind('\n') {
				Some(last) => Some(self.token(TokenKind::Newline, last + 1)),
				None => {
					self.take(spaces);
					None
				}
			};
		}
		if let Some(name) = DIAGRAM_TYPES.iter().find(|name| starts_with_keyword(rest, name)) {
			return Some(self.token(TokenKind::DiagramType, name.len()));
		}
		if let Some((name, keyword)) =
			KEYWORDS.iter().find(|(name, _)| starts_with_keyword(rest, name))
		{
			let token = self.token(TokenKind::Keyword(*keyword), name.len());
			self.begin(Condition::Struct);
			return Some(token);
		}
		match rest.as_bytes()[0] {
			b'{' => Some(self.token(TokenKind::LeftBrace, 1)),
			b'}' => Some(self.token(TokenKind::RightBrace, 1)),
			_ => Some(Token { kind: TokenKind::Invalid, text: String::new(), line: self.line }),
		}
	}
}

/// `.*direction\s+(TB|BT|RL|LR)[^\n]*`, tried before every other rule of the initial
/// condition: the length of the match.
fn direction_length(rest: &str) -> Option<usize> {
	let line = &rest[..rest.find('\n').unwrap_or(rest.len())];
	let mut search = 0;
	while let Some(found) = line[search..].find("direction") {
		let after = &rest[search + found + "direction".len()..];
		let spaces = space_length(after);
		if spaces > 0 && ["TB", "BT", "RL", "LR"].iter().any(|dir| after[spaces..].starts_with(dir))
		{
			let end = rest.len() - after.len() + spaces + 2;
			return Some(end + rest[end..].find('\n').unwrap_or(rest.len() - end));
		}
		search += found + 1;
	}
	None
}

/// Boundaries nested deeper are refused, which bounds the recursion of the grammar and of the
/// layout.
pub const MAX_NESTING: usize = 100;

struct Parser<'a, 'd> {
	lexer: Lexer<'a>,
	current: Token,
	db: &'d mut C4Db,
	/// The line after the last token the grammar took.
	accepted_line: usize,
	depth: usize,
}

/// What a statement was.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Statement {
	Other,
	Diagram,
}

impl Parser<'_, '_> {
	fn advance(&mut self) -> Token {
		let next = self.lexer.next_token();
		let token = std::mem::replace(&mut self.current, next);
		self.accepted_line = token.line + token.text.matches('\n').count();
		token
	}

	fn error<T>(&self, expected: &str) -> ParseResult<T> {
		let line = match self.current.kind {
			TokenKind::Invalid => self.current.line,
			_ => self.accepted_line,
		};
		let message = match &self.current.kind {
			TokenKind::Invalid => format!("Lexical error on line {line}. Unrecognized text."),
			TokenKind::Eof | TokenKind::EofInStruct | TokenKind::End => {
				format!("expected {expected}, found end of input")
			}
			TokenKind::Newline => format!("expected {expected}, found end of line"),
			TokenKind::Direction => format!("expected {expected}, found a direction"),
			_ => format!("expected {expected}, found {:?}", self.current.text.trim()),
		};
		Err(ParseError { line, message })
	}

	fn expect(&mut self, kind: TokenKind, expected: &str) -> ParseResult<Token> {
		if self.current.kind != kind {
			return self.error(expected);
		}
		Ok(self.advance())
	}

	fn is(&self, kind: &TokenKind) -> bool {
		self.current.kind == *kind
	}

	fn document(&mut self) -> ParseResult<()> {
		let diagram_type = self.expect(TokenKind::DiagramType, "a C4 diagram type")?;
		self.expect(TokenKind::Newline, "a line end")?;
		self.statements(&TokenKind::Eof, false)?;
		self.expect(TokenKind::Eof, "a statement")?;
		self.db.c4_type = diagram_type.text;
		Ok(())
	}

	/// Statements up to `end`, separated by line ends; the first must be a diagram statement
	/// where `needs_diagram_statement`.
	fn statements(&mut self, end: &TokenKind, needs_diagram_statement: bool) -> ParseResult<()> {
		let mut allows_other = !needs_diagram_statement;
		loop {
			let statement = self.statement(allows_other)?;
			allows_other = true;
			if self.is(&TokenKind::Newline) {
				self.advance();
				if self.is(end) {
					return Ok(());
				}
				continue;
			}
			if self.is(end) {
				return Ok(());
			}
			// `otherStatements diagramStatements`: no line end needed after a multi-line
			// `accDescr`.
			if statement == Statement::Other && matches!(self.current.kind, TokenKind::Keyword(_)) {
				allows_other = false;
				continue;
			}
			return self.error("a line end");
		}
	}

	fn statement(&mut self, allows_other: bool) -> ParseResult<Statement> {
		let expected = if allows_other { "a statement" } else { "an element or relationship" };
		let is_other = matches!(
			self.current.kind,
			TokenKind::Title
				| TokenKind::AccDescription
				| TokenKind::AccTitle
				| TokenKind::AccDescr
				| TokenKind::AccDescrMultilineValue
		);
		if is_other && !allows_other {
			return self.error(expected);
		}
		match self.current.kind {
			TokenKind::Title => {
				let text = self.advance().text;
				self.db.set_title(&text);
			}
			TokenKind::AccDescription => {
				let text = self.advance().text;
				self.db.acc_description = super::super::common::acc_description(&text);
			}
			TokenKind::AccTitle => {
				self.advance();
				let value = self.expect(TokenKind::AccTitleValue, "a title")?.text;
				// The grammar sets the diagram title.
				self.db.set_title(value.trim());
			}
			TokenKind::AccDescr | TokenKind::AccDescrMultilineValue => {
				if self.is(&TokenKind::AccDescr) {
					self.advance();
				}
				let value = match self.current.kind {
					TokenKind::AccDescrValue | TokenKind::AccDescrMultilineValue => {
						self.advance().text
					}
					_ => return self.error("a description"),
				};
				self.db.acc_description = super::super::common::acc_description(&value);
			}
			TokenKind::Keyword(keyword) => {
				self.advance();
				let attributes = self.attributes()?;
				self.apply(keyword, attributes)?;
				return Ok(Statement::Diagram);
			}
			_ => return self.error(expected),
		}
		Ok(Statement::Other)
	}

	fn attributes(&mut self) -> ParseResult<Vec<Attribute>> {
		let mut attributes = Vec::new();
		loop {
			match self.current.kind {
				TokenKind::Str => {
					attributes.push(Attribute::Text(self.advance().text.trim().into()))
				}
				TokenKind::AttributeEmpty => {
					self.advance();
					attributes.push(Attribute::Text(String::new()));
				}
				TokenKind::StrKey => {
					let key = self.advance().text;
					let value = self.expect(TokenKind::StrValue, "a value")?.text;
					attributes.push(Attribute::Named(key.trim().into(), value.trim().into()));
				}
				_ if attributes.is_empty() => return self.error("an argument"),
				_ => return Ok(attributes),
			}
		}
	}

	fn apply(&mut self, keyword: Keyword, mut attributes: Vec<Attribute>) -> ParseResult<()> {
		match keyword {
			Keyword::Element(kind) => self.db.add_element(kind, false, &attributes),
			Keyword::ElementWithTechn(kind) => self.db.add_element(kind, true, &attributes),
			Keyword::Rel(kind) => self.db.add_rel(kind, &attributes),
			Keyword::RelIndex => {
				attributes.remove(0);
				self.db.add_rel("rel", &attributes);
			}
			Keyword::UpdateElementStyle => self.db.update_element_style(&attributes),
			Keyword::UpdateRelStyle => self.db.update_rel_style(&attributes),
			Keyword::UpdateLayoutConfig => self.db.update_layout_config(&attributes),
			Keyword::Boundary(default_type, inserted) => {
				if let Some(inserted) = inserted {
					// `splice(2, 0, type)`.
					attributes.insert(attributes.len().min(2), Attribute::Text(inserted.into()));
				}
				self.boundary(&attributes, default_type, None)?;
			}
			Keyword::ContainerBoundary => {
				attributes.insert(attributes.len().min(2), Attribute::Text("CONTAINER".into()));
				self.boundary(&attributes, "container", None)?;
			}
			Keyword::DeploymentNode(node_type) => {
				self.boundary(&attributes, "node", Some(node_type))?;
			}
		}
		Ok(())
	}

	fn boundary(
		&mut self,
		attributes: &[Attribute],
		default_type: &str,
		node_type: Option<&str>,
	) -> ParseResult<()> {
		match self.current.kind {
			TokenKind::LeftBrace => {
				self.advance();
				self.expect(TokenKind::Newline, "a line end")?;
			}
			TokenKind::Newline => {
				self.advance();
				self.expect(TokenKind::LeftBrace, "\"{\"")?;
				if self.is(&TokenKind::Newline) {
					self.advance();
				}
			}
			_ => return self.error("\"{\""),
		}
		if self.depth == MAX_NESTING {
			return Err(ParseError {
				line: self.accepted_line,
				message: format!("Boundaries nested more than {MAX_NESTING} levels deep"),
			});
		}
		self.db.add_boundary(default_type, node_type, attributes);
		self.depth += 1;
		self.statements(&TokenKind::RightBrace, true)?;
		self.depth -= 1;
		self.expect(TokenKind::RightBrace, "\"}\"")?;
		self.db.pop_boundary();
		Ok(())
	}
}

/// Parses the encoded diagram `code` into `db`.
pub fn parse(code: &str, db: &mut C4Db) -> ParseResult<()> {
	// Mermaid trims the start of the text before it parses it.
	let start = space_length(code);
	let line = 1 + code[..start].matches('\n').count();
	let mut lexer = Lexer::new(&code[start..], line);
	let current = lexer.next_token();
	let mut parser = Parser { lexer, current, db, accepted_line: line, depth: 0 };
	parser.document()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parsed(code: &str) -> C4Db {
		let mut db = C4Db::new(false);
		parse(code, &mut db).unwrap_or_else(|error| panic!("{error:?}"));
		db
	}

	fn error(code: &str) -> ParseError {
		let mut db = C4Db::new(false);
		parse(code, &mut db).expect_err(code)
	}

	#[test]
	fn parses_elements_boundaries_and_relationships() {
		let db = parsed(
			"\nC4Context\n  title System Context\n  Enterprise_Boundary(b0, \"Bank\") {\n    \
			 Person(customer, \"Customer\", \"A customer\")\n    System_Boundary(b1, \"Inner\")\n    {\n      \
			 SystemDb_Ext(db, \"DB\", $descr=\"stores\", $tags=\"db\")\n    }\n  }\n  \
			 BiRel(customer, db, \"Uses\", \"SQL\")\n  RelIndex(1, db, customer, \"Answers\")\n  \
			 UpdateRelStyle(customer, db, $offsetY=\"-20\")\n",
		);
		assert_eq!(db.c4_type, "C4Context");
		assert_eq!(db.title, "System Context");
		assert_eq!(db.shapes.len(), 2);
		assert_eq!(db.shapes[0].parent_boundary, "b0");
		assert_eq!(db.shapes[1].descr.as_deref(), Some("stores"));
		assert_eq!(db.shapes[1].tags.as_deref(), Some("db"));
		assert_eq!(db.shapes[1].parent_boundary, "b1");
		assert_eq!(db.boundaries[1].kind.as_deref(), Some("ENTERPRISE"));
		assert_eq!(db.boundaries[2].kind.as_deref(), Some("SYSTEM"));
		assert_eq!(db.rels.len(), 2);
		assert_eq!(db.rels[0].techn, "SQL");
		assert_eq!(db.rels[0].offset_y, Some(-20.0));
		assert_eq!((db.rels[1].from.as_str(), db.rels[1].label.as_str()), ("db", "Answers"));
	}

	#[test]
	fn lexes_like_jison() {
		// An unquoted last argument takes everything up to the next comma.
		let db = parsed("C4Context\nPerson(a, b)\nPerson(c, \"C\")\n");
		assert_eq!(db.shapes.len(), 1);
		assert_eq!(db.shapes[0].label, "b)\nPerson(c");
		// Empty arguments. The end of the text closes statements but not strings: there jison's
		// own end of input is no `EOF` token.
		let db = parsed("C4Context\nSystem(, \"\", \"descr\")");
		assert_eq!((db.shapes[0].alias.as_str(), db.shapes[0].label.as_str()), ("", ""));
		assert_eq!(db.shapes[0].descr.as_deref(), Some("descr"));
		for code in ["C4Context\nSystem(, \"\", \"descr", "C4Context\nSystem(s, \"A\", $descr=\"x"]
		{
			assert_eq!(error(code).message, "expected a line end, found end of input");
		}
		// `accTitle` sets the title; a multi-line `accDescr` needs no line end after it.
		let db =
			parsed("C4Context\naccTitle: Shown\naccDescr {\n  Two\n  lines }Person(p, \"P\")\n");
		assert_eq!(db.title, "Shown");
		assert_eq!(db.acc_description, "Two\nlines");
		assert_eq!(db.shapes.len(), 1);
		// Lines that mention a direction are a token of their own.
		assert_eq!(error("C4Context\nPerson(a, \"direction TB\")\n").line, 2);
	}

	#[test]
	fn reports_errors_like_jison() {
		assert_eq!(error("C4Context\n").message, "expected a statement, found end of input");
		let empty = error("C4Context\nBoundary(b, \"B\") {\n}\n");
		assert_eq!(
			(empty.line, empty.message.as_str()),
			(3, "expected an element or relationship, found \"}\"")
		);
		let two = error("C4Context\nPerson(a, \"A\") Person(b, \"B\")\n");
		assert_eq!(two.line, 2);
		assert_eq!(
			error("C4Context\nPerson (a, \"A\")\n").message,
			"Lexical error on line 2. Unrecognized text."
		);
		assert_eq!(
			error("C4Context\nPerson(a, \"A\")\n}\n").message,
			"expected a statement, found \"}\""
		);
		let deep = "C4Context\n".to_owned() + &"Boundary(b, \"B\") {\n".repeat(MAX_NESTING + 1);
		assert!(error(&deep).message.contains("nested"));
	}
}
