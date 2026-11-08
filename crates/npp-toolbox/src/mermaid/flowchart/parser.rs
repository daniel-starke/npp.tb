//! The grammar of `flow.jison` as a recursive descent parser. It calls the database in the order
//! of jison's reductions, because vertex dom ids count the calls.
//!
//! The agentflow dialect follows `agentflow.jison`: `flow`, `connector` and `global` instead of
//! `subgraph`, and `%%` comments where a line or a separator may stand.

use super::super::agentflow::db::AgentflowDb;
use super::super::common::CommonFields;
use super::db::{FlowDb, FlowText, LabelType, Link, Member, destruct_link};
use super::lexer::{Dialect, Lexer, Token, TokenKind as T};

/// Subgraphs nested deeper are rejected, against unbounded recursion.
const MAX_DEPTH: usize = 64;

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
	pub line: usize,
	pub message: String,
}

type Result<V> = std::result::Result<V, ParseError>;

/// What the parser reports: the calls of the grammar's actions.
pub trait GraphDb {
	/// `addVertex`. `metadata` is the text of `@{ … }`.
	fn add_vertex(
		&mut self,
		id: &str,
		text: Option<&FlowText>,
		kind: Option<&str>,
		styles: Option<&[String]>,
		metadata: Option<&str>,
	) -> std::result::Result<(), String>;
	/// `addLink` for every pair of the node groups.
	fn add_link(
		&mut self,
		starts: &[String],
		ends: &[String],
		link: &Link,
	) -> std::result::Result<(), String>;
	/// `destructLink`: the arrow type, stroke and length of a link and its start.
	fn destruct_link(&self, end: &str, start: Option<&str>) -> Link {
		destruct_link(end, start)
	}
	/// `updateLinkInterpolate`; `None` is `default`.
	fn update_link_interpolate(
		&mut self,
		positions: &[Option<usize>],
		interpolate: &str,
	) -> std::result::Result<(), String>;
	/// `updateLink`; `None` is `default`.
	fn update_link(
		&mut self,
		positions: &[Option<usize>],
		styles: &[String],
	) -> std::result::Result<(), String>;
	fn add_class(&mut self, ids: &str, styles: &[String]);
	fn set_direction(&mut self, direction: &str);
	fn set_class(&mut self, ids: &str, class: &str);
	fn set_tooltip(&mut self, ids: &str, tooltip: &str);
	fn set_click_event(&mut self, ids: &str);
	fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>);
	/// `addSubGraph`. `id` is `None` for a container without a name; `title_is_id` when the
	/// title is the same text as the id (then ids with whitespace are generated).
	fn add_subgraph(
		&mut self,
		id: Option<&FlowText>,
		members: &[Member],
		title: Option<&FlowText>,
		title_is_id: bool,
	) -> String;
	/// `addConnector` of agentflow: the connector's id.
	fn add_connector(&mut self, _id: &FlowText, _title: &FlowText) -> String {
		String::new()
	}
	/// `addGlobal` of agentflow.
	fn add_global(&mut self, _members: &[Member]) {}
}

/// The database of the dialect being parsed. The parser is compiled once for both: the enum
/// dispatches statically, because a `dyn GraphDb` changes LLVM's inlining throughout the plugin
/// and makes the DLL 400 KB larger.
pub enum Database<'d> {
	Flowchart(&'d mut FlowDb),
	Agentflow(&'d mut AgentflowDb),
}

/// Calls `$call` on the database of either dialect, bound to `$db`.
macro_rules! with_db {
	($database:expr, $db:ident => $call:expr) => {
		match $database {
			Database::Flowchart($db) => $call,
			Database::Agentflow($db) => $call,
		}
	};
}

impl GraphDb for Database<'_> {
	fn add_vertex(
		&mut self,
		id: &str,
		text: Option<&FlowText>,
		kind: Option<&str>,
		styles: Option<&[String]>,
		metadata: Option<&str>,
	) -> std::result::Result<(), String> {
		with_db!(self, db => db.add_vertex(id, text, kind, styles, metadata))
	}
	fn add_link(
		&mut self,
		starts: &[String],
		ends: &[String],
		link: &Link,
	) -> std::result::Result<(), String> {
		with_db!(self, db => db.add_link(starts, ends, link))
	}
	fn destruct_link(&self, end: &str, start: Option<&str>) -> Link {
		with_db!(self, db => db.destruct_link(end, start))
	}
	fn update_link_interpolate(
		&mut self,
		positions: &[Option<usize>],
		interpolate: &str,
	) -> std::result::Result<(), String> {
		with_db!(self, db => db.update_link_interpolate(positions, interpolate))
	}
	fn update_link(
		&mut self,
		positions: &[Option<usize>],
		styles: &[String],
	) -> std::result::Result<(), String> {
		with_db!(self, db => db.update_link(positions, styles))
	}
	fn add_class(&mut self, ids: &str, styles: &[String]) {
		with_db!(self, db => db.add_class(ids, styles))
	}
	fn set_direction(&mut self, direction: &str) {
		with_db!(self, db => db.set_direction(direction))
	}
	fn set_class(&mut self, ids: &str, class: &str) {
		with_db!(self, db => db.set_class(ids, class))
	}
	fn set_tooltip(&mut self, ids: &str, tooltip: &str) {
		with_db!(self, db => db.set_tooltip(ids, tooltip))
	}
	fn set_click_event(&mut self, ids: &str) {
		with_db!(self, db => db.set_click_event(ids))
	}
	fn set_link(&mut self, ids: &str, link: &str, target: Option<&str>) {
		with_db!(self, db => db.set_link(ids, link, target))
	}
	fn add_subgraph(
		&mut self,
		id: Option<&FlowText>,
		members: &[Member],
		title: Option<&FlowText>,
		title_is_id: bool,
	) -> String {
		with_db!(self, db => db.add_subgraph(id, members, title, title_is_id))
	}
	fn add_connector(&mut self, id: &FlowText, title: &FlowText) -> String {
		with_db!(self, db => db.add_connector(id, title))
	}
	fn add_global(&mut self, members: &[Member]) {
		with_db!(self, db => db.add_global(members))
	}
}

struct Parser<'d> {
	tokens: Vec<Token>,
	position: usize,
	db: Database<'d>,
	common: &'d mut CommonFields,
	depth: usize,
	eof: Token,
}

/// Parses the flowchart or agentflow `code` into `db`, the accessibility statements into
/// `common`.
pub fn parse(code: &str, db: Database<'_>, common: &mut CommonFields) -> Result<()> {
	let dialect = match db {
		Database::Flowchart(_) => Dialect::Flowchart,
		Database::Agentflow(_) => Dialect::Agentflow,
	};
	let mut lexer = Lexer::with_dialect(code, dialect);
	let mut tokens = Vec::new();
	loop {
		let token = lexer.next_token().map_err(|line| ParseError {
			line,
			message: "Lexical error: unrecognized text".to_owned(),
		})?;
		let is_end = token.kind == T::Eof;
		tokens.push(token);
		if is_end {
			break;
		}
	}
	let eof =
		Token { kind: T::Eof, text: String::new(), line: tokens.last().map_or(1, |t| t.line) };
	let mut parser = Parser { tokens, position: 0, db, common, depth: 0, eof };
	parser.graph_config()?;
	parser.document(false)?;
	Ok(())
}

fn describe(kind: T) -> String {
	format!("{kind:?}").to_uppercase()
}

impl Parser<'_> {
	fn peek(&self) -> &Token {
		self.tokens.get(self.position).unwrap_or(&self.eof)
	}

	fn peek_kind(&self) -> T {
		self.peek().kind
	}

	fn kind_at(&self, offset: usize) -> T {
		self.tokens.get(self.position + offset).map_or(T::Eof, |token| token.kind)
	}

	fn advance(&mut self) -> Token {
		let token = self.peek().clone();
		if token.kind != T::Eof {
			self.position += 1;
		}
		token
	}

	fn error<V>(&self, expected: &str) -> Result<V> {
		let token = self.peek();
		let found = match token.kind {
			T::Eof => "end of input".to_owned(),
			kind => format!("'{}' ({})", token.text.trim(), describe(kind)),
		};
		Err(ParseError {
			line: token.line,
			message: format!("Parse error: expected {expected}, got {found}"),
		})
	}

	fn expect(&mut self, kind: T, expected: &str) -> Result<Token> {
		if self.peek_kind() == kind { Ok(self.advance()) } else { self.error(expected) }
	}

	fn skip_spaces(&mut self) -> bool {
		let start = self.position;
		while self.peek_kind() == T::Space {
			self.advance();
		}
		self.position > start
	}

	fn db_error(&self, message: String) -> ParseError {
		ParseError { line: self.peek().line, message }
	}

	/// `graphConfig`: `graph`/`flowchart` and the direction.
	fn graph_config(&mut self) -> Result<()> {
		while matches!(self.peek_kind(), T::Space | T::Newline | T::Comment) {
			self.advance();
		}
		self.expect(T::Graph, "'graph' or 'flowchart'")?;
		match self.peek_kind() {
			T::NoDir => {
				self.advance();
				self.db.set_direction("TB");
			}
			T::Dir => {
				let direction = self.advance().text;
				self.db.set_direction(&direction);
				// FirstStmtSeparator
				match self.peek_kind() {
					T::Semi | T::Newline | T::Comment => {
						self.advance();
					}
					T::Space => {
						self.skip_spaces();
						self.expect(T::Newline, "a line end")?;
					}
					_ => return self.error("';' or a line end after the direction"),
				}
			}
			_ => return self.error("a direction or a line end"),
		}
		Ok(())
	}

	/// `document`: statements up to `end` (in a subgraph) or the end of the input.
	fn document(&mut self, in_subgraph: bool) -> Result<Vec<Member>> {
		let mut members = Vec::new();
		loop {
			match self.peek_kind() {
				T::Eof if in_subgraph => return self.error("'end'"),
				T::Eof => return Ok(members),
				T::End if in_subgraph => {
					self.advance();
					return Ok(members);
				}
				T::Semi | T::Newline | T::Space | T::Comment => {
					self.advance();
				}
				_ => members.extend(self.statement()?),
			}
		}
	}

	fn separator(&mut self) -> Result<()> {
		match self.peek_kind() {
			T::Newline | T::Semi | T::Comment => {
				self.advance();
				Ok(())
			}
			T::Eof => Ok(()),
			_ => self.error("a line end or ';'"),
		}
	}

	fn statement(&mut self) -> Result<Vec<Member>> {
		match self.peek_kind() {
			T::Subgraph => self.subgraph(),
			T::Flow => self.flow(),
			T::Global => {
				self.advance();
				self.separator()?;
				let members = self.nested_document()?;
				self.db.add_global(&members);
				Ok(Vec::new())
			}
			T::Connector => self.connector(),
			T::Direction => {
				let text = self.advance().text;
				Ok(vec![Member::Direction(direction_of(&text).to_owned())])
			}
			T::AccTitle => {
				self.advance();
				let value = self.expect(T::AccTitleValue, "a title")?.text.trim().to_owned();
				self.common.accessibility.title = value.clone();
				Ok(vec![Member::Id(value)])
			}
			T::AccDescr => {
				self.advance();
				let value = self.expect(T::AccDescrValue, "a description")?.text.trim().to_owned();
				self.common.accessibility.description = value.clone();
				Ok(vec![Member::Id(value)])
			}
			T::AccDescrMultilineValue => {
				let value = self.advance().text.trim().to_owned();
				self.common.accessibility.description = value.clone();
				Ok(vec![Member::Id(value)])
			}
			T::Style => {
				self.style_statement()?;
				self.separator()?;
				Ok(Vec::new())
			}
			T::LinkStyle => {
				self.link_style_statement()?;
				self.separator()?;
				Ok(Vec::new())
			}
			T::ClassDef => {
				self.advance();
				self.expect(T::Space, "a space")?;
				let ids = self.id_string()?;
				self.expect(T::Space, "a space")?;
				let styles = self.styles()?;
				self.db.add_class(&ids, &styles);
				self.separator()?;
				Ok(Vec::new())
			}
			T::Class => {
				self.advance();
				self.expect(T::Space, "a space")?;
				let ids = self.id_string()?;
				self.expect(T::Space, "a space")?;
				let class = self.id_string()?;
				self.db.set_class(&ids, &class);
				self.separator()?;
				Ok(Vec::new())
			}
			T::Click => {
				self.click_statement()?;
				self.separator()?;
				Ok(Vec::new())
			}
			_ => {
				let nodes = self.vertex_statement()?;
				self.separator()?;
				Ok(nodes.into_iter().map(Member::Id).collect())
			}
		}
	}

	fn subgraph(&mut self) -> Result<Vec<Member>> {
		let line = self.advance().line;
		if self.depth >= MAX_DEPTH {
			return Err(ParseError { line, message: "Subgraphs are nested too deeply".to_owned() });
		}
		self.depth += 1;
		let id = if self.peek_kind() == T::Space {
			self.advance();
			let id = self.text_no_tags()?;
			if self.peek_kind() == T::Sqs {
				self.advance();
				let title = self.text()?;
				self.expect(T::Sqe, "']'")?;
				self.separator()?;
				let members = self.document(true)?;
				self.db.add_subgraph(Some(&id), &members, Some(&title), false)
			} else {
				self.separator()?;
				let members = self.document(true)?;
				self.db.add_subgraph(Some(&id), &members, Some(&id), true)
			}
		} else {
			self.separator()?;
			let members = self.document(true)?;
			self.db.add_subgraph(None, &members, None, false)
		};
		self.depth -= 1;
		Ok(vec![Member::Id(id)])
	}

	/// `document end` of a container, nested at most `MAX_DEPTH` deep.
	fn nested_document(&mut self) -> Result<Vec<Member>> {
		if self.depth >= MAX_DEPTH {
			let line = self.peek().line;
			return Err(ParseError {
				line,
				message: "Containers are nested too deeply".to_owned(),
			});
		}
		self.depth += 1;
		let members = self.document(true);
		self.depth -= 1;
		members
	}

	/// `flow`: an agentflow container, with an optional title and metadata.
	fn flow(&mut self) -> Result<Vec<Member>> {
		self.advance();
		if self.peek_kind() != T::Space {
			self.separator()?;
			let members = self.nested_document()?;
			return Ok(vec![Member::Id(self.db.add_subgraph(None, &members, None, false))]);
		}
		self.advance();
		let id = self.text_no_tags()?;
		let title = self.optional_title()?;
		let data = (self.peek_kind() == T::ShapeData).then(|| self.shape_data());
		self.separator()?;
		let members = self.nested_document()?;
		let id = self.db.add_subgraph(Some(&id), &members, Some(&title), false);
		if let Some(data) = data {
			self.db.add_vertex(&id, None, None, None, Some(&data)).map_err(|m| self.db_error(m))?;
		}
		Ok(vec![Member::Id(id)])
	}

	/// `connector`: an agentflow connector declaration.
	fn connector(&mut self) -> Result<Vec<Member>> {
		self.advance();
		self.expect(T::Space, "a space")?;
		let id = self.text_no_tags()?;
		let title = self.optional_title()?;
		let data = (self.peek_kind() == T::ShapeData).then(|| self.shape_data());
		self.separator()?;
		let id = self.db.add_connector(&id, &title);
		if let Some(data) = data {
			self.db.add_vertex(&id, None, None, None, Some(&data)).map_err(|m| self.db_error(m))?;
		}
		Ok(vec![Member::Id(id)])
	}

	/// The `[title]` of an agentflow container or connector, empty text without one.
	fn optional_title(&mut self) -> Result<FlowText> {
		if self.peek_kind() != T::Sqs {
			return Ok(FlowText { text: String::new(), label_type: LabelType::Text });
		}
		self.advance();
		let title = self.text()?;
		self.expect(T::Sqe, "']'")?;
		Ok(title)
	}

	/// `textNoTags`: the id and title of a subgraph.
	fn text_no_tags(&mut self) -> Result<FlowText> {
		match self.peek_kind() {
			T::Str => {
				let text = self.advance().text;
				return Ok(FlowText { text, label_type: LabelType::Text });
			}
			T::MdStr => {
				let text = self.advance().text;
				return Ok(FlowText { text, label_type: LabelType::Markdown });
			}
			_ => {}
		}
		let mut text = String::new();
		while matches!(
			self.peek_kind(),
			T::Num
				| T::NodeString
				| T::Space | T::Minus
				| T::Amp | T::UnicodeText
				| T::Colon | T::Mult
				| T::Brkt | T::Style
				| T::LinkStyle
				| T::ClassDef
				| T::Class | T::Click
				| T::Graph | T::Dir
				| T::Subgraph
				| T::Flow | T::Connector
				| T::Global | T::End
				| T::Down | T::Up
				| T::StartLink
		) {
			text.push_str(&self.advance().text);
		}
		if text.is_empty() {
			return self.error("a subgraph id");
		}
		Ok(FlowText { text, label_type: LabelType::Text })
	}

	/// `text`: the label in a shape or between pipes.
	fn text(&mut self) -> Result<FlowText> {
		match self.peek_kind() {
			T::Str => {
				let text = self.advance().text;
				return Ok(FlowText { text, label_type: LabelType::String });
			}
			T::MdStr => {
				let text = self.advance().text;
				return Ok(FlowText { text, label_type: LabelType::Markdown });
			}
			_ => {}
		}
		let mut text = String::new();
		while matches!(self.peek_kind(), T::Text | T::TagStart | T::TagEnd | T::UnicodeText) {
			text.push_str(&self.advance().text);
		}
		if text.is_empty() {
			return self.error("a label");
		}
		Ok(FlowText { text, label_type: LabelType::Text })
	}

	fn is_id_token(kind: T) -> bool {
		matches!(
			kind,
			T::Num
				| T::NodeString
				| T::Down | T::Minus
				| T::Default | T::Comma
				| T::Colon | T::Amp
				| T::Brkt | T::Mult
				| T::UnicodeText
		)
	}

	fn id_string(&mut self) -> Result<String> {
		let mut id = String::new();
		while Self::is_id_token(self.peek_kind()) {
			id.push_str(&self.advance().text);
		}
		if id.is_empty() {
			return self.error("an id");
		}
		Ok(id)
	}

	fn shape_data(&mut self) -> String {
		let mut data = String::new();
		while self.peek_kind() == T::ShapeData {
			data.push_str(&self.advance().text);
		}
		data
	}

	fn is_link_start(&self) -> bool {
		matches!(self.peek_kind(), T::Link | T::StartLink | T::LinkId)
	}

	/// `vertexStatement`: nodes joined by links; returns the nodes, later groups first.
	fn vertex_statement(&mut self) -> Result<Vec<String>> {
		let mut group = self.node()?;
		let mut nodes = group.clone();
		loop {
			if self.peek_kind() == T::ShapeData {
				let data = self.shape_data();
				self.add_shape_data(&group, &data)?;
			}
			self.skip_spaces();
			if !self.is_link_start() {
				return Ok(nodes);
			}
			let link = self.link()?;
			let next = self.node()?;
			if self.peek_kind() == T::ShapeData {
				let data = self.shape_data();
				self.add_shape_data(&next, &data)?;
			}
			self.db.add_link(&group, &next, &link).map_err(|message| self.db_error(message))?;
			let mut combined = next.clone();
			combined.extend(nodes);
			nodes = combined;
			group = next;
		}
	}

	fn add_shape_data(&mut self, group: &[String], data: &str) -> Result<()> {
		let Some(last) = group.last() else { return Ok(()) };
		let last = last.clone();
		self.db
			.add_vertex(&last, None, None, None, Some(data))
			.map_err(|message| self.db_error(message))
	}

	/// `node`: vertices joined by ` & `.
	fn node(&mut self) -> Result<Vec<String>> {
		let mut group = vec![self.styled_vertex()?];
		loop {
			let saved = self.position;
			let data = (self.peek_kind() == T::ShapeData).then(|| self.shape_data());
			if !self.skip_spaces() || self.peek_kind() != T::Amp {
				self.position = saved;
				return Ok(group);
			}
			self.advance();
			if !self.skip_spaces() {
				return self.error("a space after '&'");
			}
			let vertex = self.styled_vertex()?;
			if let Some(data) = data {
				self.add_shape_data(&group, &data)?;
			}
			group.push(vertex);
		}
	}

	fn styled_vertex(&mut self) -> Result<String> {
		let id = self.vertex()?;
		if self.peek_kind() == T::StyleSeparator {
			self.advance();
			let class = self.id_string()?;
			self.db.set_class(&id, &class);
		}
		Ok(id)
	}

	/// `vertex`: an id with an optional shape and label.
	fn vertex(&mut self) -> Result<String> {
		let id = self.id_string()?;
		let shape = |kind: T| -> Option<(T, &'static str)> {
			match kind {
				T::Sqs => Some((T::Sqe, "square")),
				T::DoubleCircleStart => Some((T::DoubleCircleEnd, "doublecircle")),
				T::EllipseStart => Some((T::EllipseEnd, "ellipse")),
				T::StadiumStart => Some((T::StadiumEnd, "stadium")),
				T::SubroutineStart => Some((T::SubroutineEnd, "subroutine")),
				T::CylinderStart => Some((T::CylinderEnd, "cylinder")),
				T::TagEnd => Some((T::Sqe, "odd")),
				_ => None,
			}
		};
		let opener = self.peek_kind();
		let (text, kind) = match opener {
			T::Ps if self.kind_at(1) == T::Ps => {
				self.advance();
				self.advance();
				let text = self.text()?;
				self.expect(T::Pe, "')'")?;
				self.expect(T::Pe, "')'")?;
				(text, "circle")
			}
			T::Ps => {
				self.advance();
				let text = self.text()?;
				self.expect(T::Pe, "')'")?;
				(text, "round")
			}
			T::DiamondStart if self.kind_at(1) == T::DiamondStart => {
				self.advance();
				self.advance();
				let text = self.text()?;
				self.expect(T::DiamondStop, "'}'")?;
				self.expect(T::DiamondStop, "'}'")?;
				(text, "hexagon")
			}
			T::DiamondStart => {
				self.advance();
				let text = self.text()?;
				self.expect(T::DiamondStop, "'}'")?;
				(text, "diamond")
			}
			T::TrapStart | T::InvTrapStart => {
				self.advance();
				let text = self.text()?;
				let kind = match (opener, self.peek_kind()) {
					(T::TrapStart, T::TrapEnd) => "trapezoid",
					(T::TrapStart, T::InvTrapEnd) => "lean_right",
					(T::InvTrapStart, T::InvTrapEnd) => "inv_trapezoid",
					(T::InvTrapStart, T::TrapEnd) => "lean_left",
					_ => return self.error("the end of the shape"),
				};
				self.advance();
				(text, kind)
			}
			T::VertexWithPropsStart => {
				self.advance();
				self.expect(T::NodeString, "a property")?;
				self.expect(T::Colon, "':'")?;
				self.expect(T::NodeString, "a value")?;
				self.expect(T::Pipe, "'|'")?;
				let text = self.text()?;
				self.expect(T::Sqe, "']'")?;
				(text, "rect")
			}
			other => match shape(other) {
				Some((closer, kind)) => {
					self.advance();
					let text = self.text()?;
					self.expect(closer, "the end of the shape")?;
					(text, kind)
				}
				None => {
					self.db
						.add_vertex(&id, None, None, None, None)
						.map_err(|m| self.db_error(m))?;
					return Ok(id);
				}
			},
		};
		self.db
			.add_vertex(&id, Some(&text), Some(kind), None, None)
			.map_err(|m| self.db_error(m))?;
		Ok(id)
	}

	/// `link`: an arrow with an optional label (`-- text -->` or `-->|text|`).
	fn link(&mut self) -> Result<Link> {
		let id = (self.peek_kind() == T::LinkId).then(|| self.advance().text);
		if self.peek_kind() == T::StartLink {
			let start = self.advance().text;
			let text = self.edge_text()?;
			let end = self.expect(T::Link, "the end of the arrow")?.text;
			let mut link = self.db.destruct_link(&end, Some(&start));
			link.text = Some(text);
			link.id = id;
			return Ok(link);
		}
		let end = self.expect(T::Link, "an arrow")?.text;
		let mut link = self.db.destruct_link(&end, None);
		link.id = id;
		if self.peek_kind() == T::Pipe {
			self.advance();
			link.text = Some(self.text()?);
			self.expect(T::Pipe, "'|'")?;
			self.skip_spaces();
		}
		Ok(link)
	}

	fn edge_text(&mut self) -> Result<FlowText> {
		match self.peek_kind() {
			T::Str => {
				let text = self.advance().text;
				return Ok(FlowText { text, label_type: LabelType::String });
			}
			T::MdStr => {
				let text = self.advance().text;
				return Ok(FlowText { text, label_type: LabelType::Markdown });
			}
			_ => {}
		}
		let mut text = String::new();
		while matches!(self.peek_kind(), T::EdgeText | T::UnicodeText) {
			text.push_str(&self.advance().text);
		}
		if text.is_empty() {
			return self.error("the text of the arrow");
		}
		Ok(FlowText { text, label_type: LabelType::Text })
	}

	/// `stylesOpt`: styles separated by commas.
	fn styles(&mut self) -> Result<Vec<String>> {
		let mut styles = Vec::new();
		loop {
			let mut style = String::new();
			while matches!(
				self.peek_kind(),
				T::Num | T::NodeString | T::Colon | T::Space | T::Brkt | T::Style
			) {
				style.push_str(&self.advance().text);
			}
			if style.is_empty() {
				return self.error("a style");
			}
			styles.push(style);
			if self.peek_kind() != T::Comma {
				return Ok(styles);
			}
			self.advance();
		}
	}

	fn style_statement(&mut self) -> Result<()> {
		self.advance();
		self.expect(T::Space, "a space")?;
		let id = self.id_string()?;
		self.expect(T::Space, "a space")?;
		let styles = self.styles()?;
		self.db.add_vertex(&id, None, None, Some(&styles), None).map_err(|m| self.db_error(m))
	}

	fn link_style_statement(&mut self) -> Result<()> {
		self.advance();
		self.expect(T::Space, "a space")?;
		let positions: Vec<Option<usize>> = if self.peek_kind() == T::Default {
			self.advance();
			vec![None]
		} else {
			let mut positions = Vec::new();
			loop {
				let number = self.expect(T::Num, "an edge number")?.text;
				positions.push(Some(number.parse().unwrap_or(usize::MAX)));
				if self.peek_kind() != T::Comma {
					break;
				}
				self.advance();
			}
			positions
		};
		self.expect(T::Space, "a space")?;
		if self.peek_kind() == T::Interpolate {
			self.advance();
			self.expect(T::Space, "a space")?;
			let interpolate = self.alpha_num()?;
			self.db
				.update_link_interpolate(&positions, &interpolate)
				.map_err(|m| self.db_error(m))?;
			if self.peek_kind() == T::Space {
				self.advance();
				let styles = self.styles()?;
				self.db.update_link(&positions, &styles).map_err(|m| self.db_error(m))?;
			}
			return Ok(());
		}
		let styles = self.styles()?;
		self.db.update_link(&positions, &styles).map_err(|m| self.db_error(m))
	}

	fn alpha_num(&mut self) -> Result<String> {
		let mut text = String::new();
		while matches!(
			self.peek_kind(),
			T::Num
				| T::UnicodeText
				| T::NodeString
				| T::Dir | T::Down
				| T::Minus | T::Comma
				| T::Colon | T::Amp
				| T::Brkt | T::Mult
		) {
			text.push_str(&self.advance().text);
		}
		if text.is_empty() {
			return self.error("a name");
		}
		Ok(text)
	}

	/// The optional ` "tooltip"` and ` _target` of a click statement.
	fn tooltip_and_target(&mut self, allow_target: bool) -> (Option<String>, Option<String>) {
		let mut tooltip = None;
		let mut target = None;
		if self.peek_kind() == T::Space && self.kind_at(1) == T::Str {
			self.advance();
			tooltip = Some(self.advance().text);
		}
		if allow_target && self.peek_kind() == T::Space && self.kind_at(1) == T::LinkTarget {
			self.advance();
			target = Some(self.advance().text);
		}
		(tooltip, target)
	}

	fn click_statement(&mut self) -> Result<()> {
		let ids = self.advance().text;
		match self.peek_kind() {
			T::CallbackName => {
				self.advance();
				if self.peek_kind() == T::CallbackArgs {
					self.advance();
				}
				self.db.set_click_event(&ids);
				if let (Some(tooltip), _) = self.tooltip_and_target(false) {
					self.db.set_tooltip(&ids, &tooltip);
				}
			}
			T::Href | T::Str => {
				if self.peek_kind() == T::Href {
					self.advance();
				}
				let link = self.expect(T::Str, "a link")?.text;
				let (tooltip, target) = self.tooltip_and_target(true);
				self.db.set_link(&ids, &link, target.as_deref());
				if let Some(tooltip) = tooltip {
					self.db.set_tooltip(&ids, &tooltip);
				}
			}
			_ => {
				self.alpha_num()?;
				self.db.set_click_event(&ids);
				if let (Some(tooltip), _) = self.tooltip_and_target(false) {
					self.db.set_tooltip(&ids, &tooltip);
				}
			}
		}
		Ok(())
	}
}

/// The direction of a `direction` statement: jison tries `TB`, `BT`, `RL`, `LR`, `TD` in turn.
fn direction_of(text: &str) -> &'static str {
	for direction in ["TB", "BT", "RL", "LR", "TD"] {
		let found = text.match_indices("direction").any(|(start, _)| {
			let after = &text[start + "direction".len()..];
			let trimmed = after.trim_start();
			trimmed.len() < after.len() && trimmed.starts_with(direction)
		});
		if found {
			return direction;
		}
	}
	"TB"
}

#[cfg(test)]
mod tests {
	use super::super::db::FlowDb;
	use super::*;

	fn parse_code(code: &str) -> (FlowDb, Result<()>) {
		let mut db = FlowDb::new(500);
		let mut common = CommonFields::default();
		let result = parse(code, Database::Flowchart(&mut db), &mut common);
		(db, result)
	}

	#[test]
	fn parses_vertices_and_links() {
		let (db, result) =
			parse_code("flowchart LR\n  A[Start] -->|yes| B((Circle)) & C{Is it?}\n  B --- D\n");
		assert_eq!(result, Ok(()));
		assert_eq!(db.direction.as_deref(), Some("LR"));
		let vertices: Vec<(&str, &str, &str)> = db
			.vertices
			.iter()
			.map(|v| {
				(v.id.as_str(), v.text.as_deref().unwrap_or(""), v.kind.as_deref().unwrap_or(""))
			})
			.collect();
		assert_eq!(
			vertices,
			[
				("A", "Start", "square"),
				("B", "Circle", "circle"),
				("C", "Is it?", "diamond"),
				("D", "D", "")
			]
		);
		assert_eq!(db.vertices[3].dom_id, "flowchart-D-4");
		let edges: Vec<(&str, &str, &str, &str)> = db
			.edges
			.iter()
			.map(|e| {
				(e.start.as_str(), e.end.as_str(), e.kind.as_deref().unwrap_or(""), e.text.as_str())
			})
			.collect();
		assert_eq!(
			edges,
			[
				("A", "B", "arrow_point", "yes"),
				("A", "C", "arrow_point", "yes"),
				("B", "D", "arrow_open", "")
			]
		);
	}

	#[test]
	fn parses_subgraphs_styles_and_clicks() {
		let code = "graph TD\nsubgraph one [The title]\n direction LR\n a1-->a2\nend\nclassDef green fill:#9f6,stroke:#333\nclass a1 green\nstyle a2 fill:#f9f\nlinkStyle 0 stroke:#ff3\nclick a1 \"https://example.com\" \"Tip\" _blank\na2 -- text --> a3@{ shape: hex }\n";
		let (db, result) = parse_code(code);
		assert_eq!(result, Ok(()));
		assert_eq!(db.subgraphs[0].id, "one");
		assert_eq!(db.subgraphs[0].title, "The title");
		assert_eq!(db.subgraphs[0].nodes, ["a2", "a1"]);
		assert_eq!(db.subgraphs[0].dir.as_deref(), Some("LR"));
		assert_eq!(
			db.class("green").map(|c| c.styles.clone()),
			Some(vec!["fill:#9f6".into(), "stroke:#333".into()])
		);
		assert_eq!(
			db.vertex("a1").map(|v| v.classes.clone()),
			Some(vec!["green".into(), "clickable".into()])
		);
		assert_eq!(
			db.vertex("a1").and_then(|v| v.link.clone()).as_deref(),
			Some("https://example.com")
		);
		assert_eq!(db.tooltip("a1"), Some("Tip"));
		assert_eq!(db.vertex("a2").map(|v| v.styles.clone()), Some(vec!["fill:#f9f".into()]));
		assert_eq!(db.edges[0].style, Some(vec!["stroke:#ff3".into(), "fill:none".into()]));
		assert_eq!(db.edges[1].text, "text");
		assert_eq!(db.vertex("a3").and_then(|v| v.kind.clone()).as_deref(), Some("hex"));
	}

	#[test]
	fn reports_errors_with_lines() {
		let (_, result) = parse_code("flowchart TD\nA-->B\nA-->(");
		assert_eq!(result.map_err(|error| error.line), Err(3));
		let (_, result) = parse_code("flowchart TD\nsubgraph x\nA\n");
		assert!(result.is_err());
		let nested = format!("flowchart TD\n{}", "subgraph a\n".repeat(100));
		assert!(parse_code(&nested).1.is_err());
	}
}
