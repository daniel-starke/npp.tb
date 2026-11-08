//! The gitGraph grammar (`gitGraph.langium`, `reference.langium`, `gitGraphParser.ts`) and the
//! database it fills (`gitGraphAst.ts`).
//!
//! Langium's lexer tries the keywords first, the longest first, then the terminals in the order
//! of the grammars: `ACC_DESCR`, `ACC_TITLE`, `TITLE`, `INT`, `STRING`, `NEWLINE`, the hidden
//! whitespace and comments, and `REFERENCE` last. A keyword yields to `REFERENCE` when that
//! matches more (`commitX`). So `TITLE` takes the blanks before it (`branch titleX` fails on
//! ` title`), `INT` comes before `REFERENCE` (`branch 1abc` fails on `1`) and branch names
//! cannot be keywords (`branch commit`).

use std::collections::HashMap;

use super::super::common::{CommonFields, sanitize_text};
use super::super::langium::Scanner;
use super::super::{Context, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
	Keyword(&'static str),
	/// `TITLE`, `ACC_TITLE` or `ACC_DESCR`.
	CommonField,
	Int,
	String,
	Newline,
	Reference,
	/// A character no terminal matches, which Mermaid reports as a lexer error.
	Invalid,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Token<'a> {
	pub kind: Kind,
	pub text: &'a str,
	pub line: usize,
}

/// The keywords of the grammar, the longest first like Langium sorts them.
const KEYWORDS: [&str; 20] = [
	"cherry-pick",
	"HIGHLIGHT",
	"gitGraph:",
	"checkout",
	"gitGraph",
	"REVERSE",
	"parent:",
	"commit",
	"branch",
	"switch",
	"NORMAL",
	"order:",
	"merge",
	"type:",
	"msg:",
	"tag:",
	"id:",
	"LR",
	"TB",
	"BT",
];

fn is_word_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Length of `REFERENCE`, `\w([-\./\w]*[-\w])?`, at the start of `text`.
fn reference_length(text: &str) -> usize {
	let bytes = text.as_bytes();
	if !bytes.first().copied().is_some_and(is_word_byte) {
		return 0;
	}
	let run = bytes[1..]
		.iter()
		.take_while(|&&byte| is_word_byte(byte) || matches!(byte, b'-' | b'.' | b'/'))
		.count();
	// Backtrack to the last character that may end a reference.
	let tail = bytes[1..1 + run].iter().rposition(|&byte| byte != b'.' && byte != b'/');
	1 + tail.map_or(0, |index| index + 1)
}

/// The kind and length of the token at the start of `rest`; `None` for hidden terminals.
fn token_at(rest: &str) -> (Option<Kind>, usize) {
	let reference = reference_length(rest);
	for keyword in KEYWORDS {
		let Some(after) = rest.strip_prefix(keyword) else { continue };
		// The token builder restricts `gitGraph`: only whitespace or a comment may follow.
		if keyword == "gitGraph"
			&& !(after.is_empty()
				|| after.starts_with("%%")
				|| after.starts_with(char::is_whitespace))
		{
			continue;
		}
		let could_be_reference = keyword.bytes().all(|byte| is_word_byte(byte) || byte == b'-');
		if could_be_reference && reference > keyword.len() {
			return (Some(Kind::Reference), reference);
		}
		return (Some(Kind::Keyword(keyword)), keyword.len());
	}
	if rest.starts_with(':') {
		return (Some(Kind::Keyword(":")), 1);
	}
	let mut scanner = Scanner::new(rest);
	let mut fields = CommonFields::default();
	if !rest.starts_with("%%") && scanner.common_field(&mut fields) {
		return (Some(Kind::CommonField), rest.len() - scanner.rest().len());
	}
	let mut scanner = Scanner::new(rest);
	if rest.starts_with(|c: char| c.is_ascii_digit()) && scanner.int().is_some() {
		return (Some(Kind::Int), rest.len() - scanner.rest().len());
	}
	let first = rest.as_bytes()[0];
	if first == b'"' || first == b'\'' {
		let mut scanner = Scanner::new(rest);
		if scanner.string().is_some() {
			return (Some(Kind::String), rest.len() - scanner.rest().len());
		}
	}
	match first {
		b'\n' => (Some(Kind::Newline), 1),
		b'\r' if rest.starts_with("\r\n") => (Some(Kind::Newline), 2),
		b' ' | b'\t' => {
			(None, rest.bytes().take_while(|&byte| byte == b' ' || byte == b'\t').count())
		}
		b'%' if rest.starts_with("%%") => (None, rest.find(['\n', '\r']).unwrap_or(rest.len())),
		_ if reference > 0 => (Some(Kind::Reference), reference),
		_ => (Some(Kind::Invalid), rest.chars().next().map_or(1, char::len_utf8)),
	}
}

pub(super) fn lex(code: &str) -> Vec<Token<'_>> {
	let mut tokens = Vec::new();
	let mut position = 0;
	let mut line = 1;
	while position < code.len() {
		let rest = &code[position..];
		let (kind, length) = token_at(rest);
		if let Some(kind) = kind {
			tokens.push(Token { kind, text: &rest[..length], line });
		}
		line += rest[..length].matches('\n').count();
		position += length;
	}
	tokens
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Direction {
	#[default]
	LeftRight,
	TopBottom,
	BottomTop,
}

/// `commitType`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommitType {
	Normal,
	Reverse,
	Highlight,
	Merge,
	CherryPick,
}

fn commit_type(keyword: &str) -> CommitType {
	match keyword {
		"REVERSE" => CommitType::Reverse,
		"HIGHLIGHT" => CommitType::Highlight,
		_ => CommitType::Normal,
	}
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Commit {
	pub id: String,
	pub seq: usize,
	pub kind: CommitType,
	/// The `type` of a merge, which only changes its symbol.
	pub custom_type: Option<CommitType>,
	pub custom_id: bool,
	pub tags: Vec<String>,
	pub parents: Vec<String>,
	pub branch: String,
}

/// A statement with the values of its options; the last of a repeated option wins.
#[derive(Debug, Default)]
struct Options {
	id: Option<String>,
	kind: Option<CommitType>,
	tags: Vec<String>,
	parent: Option<String>,
}

enum Statement {
	Commit(Options),
	Branch {
		name: String,
		order: f64,
	},
	Merge {
		branch: String,
		options: Options,
	},
	Checkout(String),
	/// The options and whether any `tag:` was given.
	CherryPick(Options, bool),
}

/// Mermaid's `random` with `Math.random` seeded like the reference renders: mulberry32 from 1.
/// Mermaid's ids are random; these are stable, so diagrams render the same every time.
struct Random {
	state: u32,
}

impl Random {
	fn next(&mut self) -> f64 {
		self.state = self.state.wrapping_add(0x6D2B_79F5);
		let seed = self.state;
		let mut t = (seed ^ (seed >> 15)).wrapping_mul(1 | seed);
		t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
		f64::from(t ^ (t >> 14)) / 4_294_967_296.0
	}

	/// `random({ length })`: hexadecimal digits.
	fn hex(&mut self, length: usize) -> String {
		(0..length)
			.map(|_| char::from(b"0123456789abcdef"[(self.next() * 16.0) as usize & 15]))
			.collect()
	}
}

/// The state of `gitGraphAst.ts`.
pub(super) struct Database {
	pub common: CommonFields,
	pub direction: Direction,
	/// `commits`: a JavaScript `Map` in insertion order; a repeated id replaces the commit in
	/// its old place.
	pub commits: Vec<Commit>,
	commit_index: HashMap<String, usize>,
	/// `branchConfig`: names and orders in creation order.
	pub branch_config: Vec<(String, f64)>,
	/// `branches`: the head commit of each branch.
	branches: HashMap<String, Option<String>>,
	head: Option<String>,
	current_branch: String,
	seq: usize,
	random: Random,
}

impl Database {
	pub fn new(context: &Context, main_branch: &str, main_order: f64) -> Self {
		let main_branch = main_branch.to_owned();
		Self {
			common: CommonFields::new(context),
			direction: Direction::default(),
			commits: Vec::new(),
			commit_index: HashMap::new(),
			branch_config: vec![(main_branch.clone(), main_order)],
			branches: HashMap::from([(main_branch.clone(), None)]),
			head: None,
			current_branch: main_branch,
			seq: 0,
			random: Random { state: 1 },
		}
	}

	pub fn commit_by_id(&self, id: &str) -> Option<&Commit> {
		self.commit_index.get(id).map(|&index| &self.commits[index])
	}

	fn insert(&mut self, commit: Commit) {
		let id = commit.id.clone();
		self.head = Some(id.clone());
		self.branches.insert(self.current_branch.clone(), Some(id.clone()));
		match self.commit_index.get(&id) {
			Some(&index) => self.commits[index] = commit,
			None => {
				self.commit_index.insert(id, self.commits.len());
				self.commits.push(commit);
			}
		}
	}

	fn generated_id(&mut self) -> String {
		format!("{}-{}", self.seq, self.random.hex(7))
	}

	fn head_parents(&self) -> Vec<String> {
		self.head.iter().cloned().collect()
	}

	fn commit(&mut self, options: Options) {
		let id = options.id.map(|id| sanitize_text(&id)).filter(|id| !id.is_empty());
		let id = id.unwrap_or_else(|| self.generated_id());
		let commit = Commit {
			id,
			seq: self.seq,
			kind: options.kind.unwrap_or(CommitType::Normal),
			custom_type: None,
			custom_id: false,
			tags: options.tags.iter().map(|tag| sanitize_text(tag)).collect(),
			parents: self.head_parents(),
			branch: self.current_branch.clone(),
		};
		self.seq += 1;
		self.insert(commit);
	}

	fn branch(&mut self, name: &str, order: f64) -> std::result::Result<(), String> {
		let name = sanitize_text(name);
		if self.branches.contains_key(&name) {
			return Err(format!(
				"Trying to create an existing branch. (Help: Either use a new name if you want create a new branch or try using \"checkout {name}\")"
			));
		}
		self.branches.insert(name.clone(), self.head.clone());
		self.branch_config.push((name.clone(), order));
		self.checkout(&name)
	}

	fn branch_head(&self, branch: &str) -> Option<&Commit> {
		let id = self.branches.get(branch)?.as_deref()?;
		self.commit_by_id(id).filter(|_| !id.is_empty())
	}

	fn merge(&mut self, branch: &str, options: Options) -> std::result::Result<(), String> {
		let other = sanitize_text(branch);
		let custom_id = options.id.map(|id| sanitize_text(&id)).filter(|id| !id.is_empty());
		let current = self.branch_head(&self.current_branch).map(|commit| commit.id.clone());
		let other_head = self.branch_head(&other).map(|commit| commit.id.clone());
		if let (Some(current), Some(_)) = (&current, &other_head)
			&& self.commit_by_id(current).is_some_and(|commit| commit.branch == other)
		{
			return Err(format!("Cannot merge branch '{other}' into itself."));
		}
		if self.current_branch == other {
			return Err("Incorrect usage of \"merge\". Cannot merge a branch to itself".to_owned());
		}
		let Some(current) = current else {
			return Err(format!(
				"Incorrect usage of \"merge\". Current branch ({})has no commits",
				self.current_branch
			));
		};
		if !self.branches.contains_key(&other) {
			return Err(format!(
				"Incorrect usage of \"merge\". Branch to be merged ({other}) does not exist"
			));
		}
		let Some(other_head) = other_head else {
			return Err(format!(
				"Incorrect usage of \"merge\". Branch to be merged ({other}) has no commits"
			));
		};
		if current == other_head {
			return Err("Incorrect usage of \"merge\". Both branches have same head".to_owned());
		}
		if let Some(id) = &custom_id
			&& self.commit_index.contains_key(id)
		{
			return Err(format!(
				"Incorrect usage of \"merge\". Commit with id:{id} already exists, use different custom id"
			));
		}
		let id = match &custom_id {
			Some(id) => id.clone(),
			None => self.generated_id(),
		};
		let parents = match &self.head {
			Some(head) => vec![head.clone(), other_head],
			None => Vec::new(),
		};
		let commit = Commit {
			id,
			seq: self.seq,
			kind: CommitType::Merge,
			custom_type: options.kind,
			custom_id: custom_id.is_some(),
			tags: options.tags,
			parents,
			branch: self.current_branch.clone(),
		};
		self.seq += 1;
		self.insert(commit);
		Ok(())
	}

	fn cherry_pick(&mut self, options: Options, has_tags: bool) -> std::result::Result<(), String> {
		let source_id = options.id.map(|id| sanitize_text(&id)).unwrap_or_default();
		let parent = options.parent.map(|parent| sanitize_text(&parent)).filter(|p| !p.is_empty());
		let Some(source) = self.commit_by_id(&source_id).filter(|_| !source_id.is_empty()) else {
			return Err(
				"Incorrect usage of \"cherryPick\". Source commit id should exist and provided"
					.to_owned(),
			);
		};
		if let Some(parent) = &parent
			&& !source.parents.contains(parent)
		{
			return Err("Invalid operation: The specified parent commit is not an immediate parent of the cherry-picked commit.".to_owned());
		}
		if source.kind == CommitType::Merge && parent.is_none() {
			return Err("Incorrect usage of cherry-pick: If the source commit is a merge commit, an immediate parent commit must be specified.".to_owned());
		}
		if source.branch == self.current_branch {
			return Err(
				"Incorrect usage of \"cherryPick\". Source commit is already on current branch"
					.to_owned(),
			);
		}
		if self.branch_head(&self.current_branch).is_none() {
			return Err(format!(
				"Incorrect usage of \"cherry-pick\". Current branch ({})has no commits",
				self.current_branch
			));
		}
		let source_id = source.id.clone();
		let default_tag = match source.kind == CommitType::Merge {
			true => format!("cherry-pick:{source_id}|parent:{}", parent.unwrap_or_default()),
			false => format!("cherry-pick:{source_id}"),
		};
		let tags = match has_tags {
			true => options
				.tags
				.iter()
				.map(|tag| sanitize_text(tag))
				.filter(|t| !t.is_empty())
				.collect(),
			false => vec![default_tag],
		};
		let id = self.generated_id();
		let parents = match &self.head {
			Some(head) => vec![head.clone(), source_id],
			None => Vec::new(),
		};
		let commit = Commit {
			id,
			seq: self.seq,
			kind: CommitType::CherryPick,
			custom_type: None,
			custom_id: false,
			tags,
			parents,
			branch: self.current_branch.clone(),
		};
		self.seq += 1;
		self.insert(commit);
		Ok(())
	}

	fn checkout(&mut self, branch: &str) -> std::result::Result<(), String> {
		let branch = sanitize_text(branch);
		let Some(head) = self.branches.get(&branch) else {
			return Err(format!(
				"Trying to checkout branch which is not yet created. (Help try using \"branch {branch}\")"
			));
		};
		self.head = head
			.as_deref()
			.filter(|id| !id.is_empty())
			.and_then(|id| self.commit_index.contains_key(id).then(|| id.to_owned()));
		self.current_branch = branch;
		Ok(())
	}

	/// `getBranchesAsObjArray`: the branch names sorted by their order, stable.
	pub fn sorted_branches(&self) -> Vec<String> {
		let mut branches: Vec<&(String, f64)> = self.branch_config.iter().collect();
		npp::sort::sort_by(&mut branches[..], |a, b| {
			a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
		});
		branches.into_iter().map(|(name, _)| name.clone()).collect()
	}
}

fn describe(token: Option<&Token>) -> String {
	match token {
		None => "the end of the diagram".to_owned(),
		Some(token) if token.kind == Kind::Newline => "the end of the line".to_owned(),
		Some(token) => format!("{:?}", token.text.trim_start()),
	}
}

struct Parser<'a, 'c, 'm> {
	tokens: Vec<Token<'a>>,
	index: usize,
	context: &'c Context<'m>,
}

impl<'a> Parser<'a, '_, '_> {
	fn peek(&self) -> Option<Token<'a>> {
		self.tokens.get(self.index).copied()
	}

	fn take(&mut self, kind: Kind) -> Option<Token<'a>> {
		let token = self.peek().filter(|token| token.kind == kind)?;
		self.index += 1;
		Some(token)
	}

	fn line(&self) -> usize {
		self.peek().or(self.tokens.last().copied()).map_or(1, |token| token.line)
	}

	fn error<T>(&self, expected: &str) -> Result<T> {
		let token = self.tokens.get(self.index);
		let message = match token {
			Some(token) if token.kind == Kind::Invalid => {
				format!("unexpected character {:?}", token.text)
			}
			_ => format!("expected {expected}, found {}", describe(token)),
		};
		Err(self.context.error_at(self.line(), message))
	}

	/// `EOL`: `NEWLINE+ | EOF`.
	fn end_of_line(&mut self) -> Result<()> {
		if self.peek().is_none() {
			return Ok(());
		}
		if self.take(Kind::Newline).is_none() {
			return self.error("the end of the line");
		}
		while self.take(Kind::Newline).is_some() {}
		Ok(())
	}

	fn string(&mut self) -> Option<String> {
		let token = self.take(Kind::String)?;
		Scanner::new(token.text).string()
	}

	fn expect_string(&mut self) -> Result<String> {
		match self.string() {
			Some(value) => Ok(value),
			None => self.error("a string"),
		}
	}

	/// `(REFERENCE | STRING)`
	fn branch_name(&mut self) -> Result<String> {
		if let Some(token) = self.take(Kind::Reference) {
			return Ok(token.text.to_owned());
		}
		match self.string() {
			Some(value) => Ok(value),
			None => self.error("a branch name"),
		}
	}

	fn commit_type(&mut self) -> Result<CommitType> {
		for keyword in ["NORMAL", "REVERSE", "HIGHLIGHT"] {
			if self.take(Kind::Keyword(keyword)).is_some() {
				return Ok(commit_type(keyword));
			}
		}
		self.error("NORMAL, REVERSE or HIGHLIGHT")
	}

	/// The options of `commit`, `merge` and `cherry-pick` (`allowed` keywords; `msg:` and a
	/// bare string are the message, which no drawing shows), and whether a `tag:` was given.
	fn options(&mut self, allowed: &[&str], allows_message: bool) -> Result<(Options, bool)> {
		let mut options = Options::default();
		let mut has_tags = false;
		while let Some(token) = self.peek() {
			match token.kind {
				Kind::Keyword(keyword) if allowed.contains(&keyword) => {
					self.index += 1;
					match keyword {
						"id:" => options.id = Some(self.expect_string()?),
						"tag:" => {
							options.tags.push(self.expect_string()?);
							has_tags = true;
						}
						"type:" => options.kind = Some(self.commit_type()?),
						"parent:" => options.parent = Some(self.expect_string()?),
						_ => _ = self.expect_string()?,
					}
				}
				Kind::String if allows_message => _ = self.string(),
				_ => break,
			}
		}
		Ok((options, has_tags))
	}

	fn statement(&mut self) -> Result<Option<Statement>> {
		let Some(Token { kind: Kind::Keyword(keyword), .. }) = self.peek() else {
			return Ok(None);
		};
		let statement = match keyword {
			"commit" => {
				self.index += 1;
				let allowed = ["id:", "msg:", "tag:", "type:"];
				Statement::Commit(self.options(&allowed, true)?.0)
			}
			"branch" => {
				self.index += 1;
				let name = self.branch_name()?;
				let mut order = 0.0;
				if self.take(Kind::Keyword("order:")).is_some() {
					match self.take(Kind::Int) {
						Some(token) => order = token.text.parse().unwrap_or(0.0),
						None => return self.error("an integer"),
					}
				}
				Statement::Branch { name, order }
			}
			"merge" => {
				self.index += 1;
				let branch = self.branch_name()?;
				let options = self.options(&["id:", "tag:", "type:"], false)?.0;
				Statement::Merge { branch, options }
			}
			"checkout" | "switch" => {
				self.index += 1;
				Statement::Checkout(self.branch_name()?)
			}
			"cherry-pick" => {
				self.index += 1;
				let (options, has_tags) = self.options(&["id:", "tag:", "parent:"], false)?;
				Statement::CherryPick(options, has_tags)
			}
			_ => return Ok(None),
		};
		self.end_of_line()?;
		Ok(Some(statement))
	}

	fn header(&mut self, database: &mut Database) -> Result<()> {
		if self.take(Kind::Keyword("gitGraph:")).is_some() {
			return Ok(());
		}
		if self.take(Kind::Keyword("gitGraph")).is_none() {
			return self.error("`gitGraph`");
		}
		if self.take(Kind::Keyword(":")).is_some() {
			return Ok(());
		}
		let direction = match self.peek().map(|token| token.kind) {
			Some(Kind::Keyword("LR")) => Direction::LeftRight,
			Some(Kind::Keyword("TB")) => Direction::TopBottom,
			Some(Kind::Keyword("BT")) => Direction::BottomTop,
			_ => return Ok(()),
		};
		self.index += 1;
		if self.take(Kind::Keyword(":")).is_none() {
			return self.error("`:`");
		}
		database.direction = direction;
		Ok(())
	}
}

/// Parses `code` and runs its statements on a new database.
pub(super) fn parse(
	code: &str,
	context: &Context,
	main_branch: &str,
	main_order: f64,
) -> Result<Database> {
	let mut database = Database::new(context, main_branch, main_order);
	let mut parser = Parser { tokens: lex(code), index: 0, context };
	while parser.take(Kind::Newline).is_some() {}
	parser.header(&mut database)?;
	let mut statements: Vec<(usize, Statement)> = Vec::new();
	while let Some(token) = parser.peek() {
		match token.kind {
			Kind::Newline => parser.index += 1,
			Kind::CommonField => {
				parser.index += 1;
				let mut scanner = Scanner::new(token.text);
				scanner.common_field(&mut database.common);
				parser.end_of_line()?;
			}
			_ => match parser.statement()? {
				Some(statement) => statements.push((token.line, statement)),
				None => return parser.error("a statement"),
			},
		}
	}
	for (line, statement) in statements {
		let result = match statement {
			Statement::Commit(options) => {
				database.commit(options);
				Ok(())
			}
			Statement::Branch { name, order } => database.branch(&name, order),
			Statement::Merge { branch, options } => database.merge(&branch, options),
			Statement::Checkout(branch) => database.checkout(&branch),
			Statement::CherryPick(options, has_tags) => database.cherry_pick(options, has_tags),
		};
		result.map_err(|message| context.error_at(line, message))?;
	}
	Ok(database)
}

#[cfg(test)]
mod tests {
	use super::super::super::text::testing::FixedMeasure;
	use super::*;

	fn kinds(code: &str) -> Vec<(Kind, &str)> {
		lex(code).into_iter().map(|token| (token.kind, token.text)).collect()
	}

	fn parsed(code: &str) -> Result<Database> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context, "main", 0.0)
	}

	#[test]
	fn lexes_like_langium() {
		use Kind::*;
		assert_eq!(
			kinds("gitGraph LR:\n commitX id:\"a\" cherry-picked cherry-pick commit."),
			[
				(Keyword("gitGraph"), "gitGraph"),
				(Keyword("LR"), "LR"),
				(Keyword(":"), ":"),
				(Newline, "\n"),
				(Reference, "commitX"),
				(Keyword("id:"), "id:"),
				(String, "\"a\""),
				(Reference, "cherry-picked"),
				(Keyword("cherry-pick"), "cherry-pick"),
				(Keyword("commit"), "commit"),
				(Invalid, "."),
			]
		);
		assert_eq!(
			kinds("branch 1abc titleX release/1.0-x a- gitGraphX"),
			[
				(Keyword("branch"), "branch"),
				(Int, "1"),
				(Reference, "abc"),
				(CommonField, " title"),
				(Reference, "X"),
				(Reference, "release/1.0-x"),
				(Reference, "a-"),
				(Reference, "gitGraphX"),
			]
		);
		assert_eq!(kinds("x/y. a./b"), [(Reference, "x/y"), (Invalid, "."), (Reference, "a./b")]);
		assert_eq!(
			kinds("order: 01 %% c\n"),
			[(Keyword("order:"), "order:"), (Int, "0"), (Int, "1"), (Newline, "\n")]
		);
	}

	#[test]
	fn builds_the_history() {
		let database = parsed(
			"gitGraph BT:\n commit \"m\" id: \"a\" msg: \"x\" tag: \"t1\" tag: \"t2\" type: HIGHLIGHT id: \"b\"\n branch dev order: 2\n commit\n checkout main\n merge dev tag: \"v1\" type: REVERSE\n switch dev\n commit id: \"c\"\n checkout main\n cherry-pick id: \"c\" tag: \"\"\n",
		)
		.unwrap();
		assert_eq!(database.direction, Direction::BottomTop);
		let ids: Vec<&str> = database.commits.iter().map(|commit| commit.id.as_str()).collect();
		assert_eq!(ids, ["b", "1-a08ff49", "2-b6f7726", "c", "4-32716c4"]);
		let first = &database.commits[0];
		assert_eq!(
			(first.kind, first.tags.clone()),
			(CommitType::Highlight, vec!["t1".to_owned(), "t2".to_owned()])
		);
		let merge = &database.commits[2];
		assert_eq!(merge.parents, ["b", "1-a08ff49"]);
		assert_eq!(merge.custom_type, Some(CommitType::Reverse));
		assert!(!merge.custom_id);
		assert!(database.commits[4].tags.is_empty());
		assert_eq!(database.sorted_branches(), ["main", "dev"]);
	}

	#[test]
	fn reports_errors_like_mermaid() {
		for (code, line, message) in [
			("gitGraph\n commit\n branch main\n", 3, "Trying to create an existing branch"),
			("gitGraph\n commit\n checkout dev\n", 3, "Trying to checkout branch"),
			("gitGraph\n commit\n merge main\n", 3, "Cannot merge branch 'main' into itself."),
			("gitGraph\n commit id: \"a\"\n cherry-pick id: \"a\"\n", 3, "already on current"),
			("gitGraph\n commit\n branch commit\n", 3, "expected a branch name"),
			("gitGraph TB\n commit\n", 1, "expected `:`"),
			("gitGraph\n commit;\n", 2, "unexpected character \";\""),
			("gitGraphX\n", 1, "expected `gitGraph`"),
			("gitGraph\n commit commit\n", 2, "expected the end of the line"),
		] {
			let error = parsed(code).err().unwrap();
			assert_eq!(error.line, Some(line), "{code}");
			assert!(error.message.contains(message), "{code}: {}", error.message);
		}
		assert!(parsed("gitGraph :\n commit\n title  A   b\n accDescr {\n x\n }\n").is_ok());
	}
}
