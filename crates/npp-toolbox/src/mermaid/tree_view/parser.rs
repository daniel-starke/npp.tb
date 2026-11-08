//! The treeView grammar (`treeView.langium`, `valueConverter.ts`), the conversion of
//! box-drawing input (`boxDrawingPreprocessor.ts`) and the database (`parser.ts`, `db.ts`).
//!
//! Chevrotain takes the first terminal that matches, in the order of the grammar: `TITLE`,
//! `ACC_TITLE` and `ACC_DESCR` take the blanks before them, the annotations take theirs before
//! `INDENTATION`, and line ends are hidden. So `"a" "b"` are two rows, an annotation at the
//! start of the next line belongs to the row before, and a row whose name starts with `title`
//! loses that word.

use super::super::common::{CommonFields, sanitize_text};
use super::super::langium::Scanner;
use super::super::{Context, Result};

const KEYWORD: &str = "treeView-beta";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	Keyword,
	/// `TITLE`, `ACC_TITLE` or `ACC_DESCR`.
	CommonField,
	ClassAnnotation,
	IconAnnotation,
	DescriptionAnnotation,
	Indentation,
	QuotedName,
	BareName,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
	kind: Kind,
	text: &'a str,
	line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NodeType {
	File,
	Directory,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Node {
	pub name: String,
	pub node_type: NodeType,
	pub class: Option<String>,
	/// The `icon(…)` annotation, `none` for `icon()`.
	pub icon: Option<String>,
	pub description: Option<String>,
	pub children: Vec<usize>,
	level: i64,
}

/// The rows below the root `/`, which is node 0.
#[derive(Debug)]
pub(super) struct Tree {
	pub fields: CommonFields,
	pub nodes: Vec<Node>,
}

fn blank_length(text: &str) -> usize {
	text.len() - text.trim_start_matches([' ', '\t']).len()
}

fn word_length(text: &str) -> usize {
	text.bytes()
		.take_while(|&byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
		.count()
}

/// `[ \t]+:::[ \t]*[A-Za-z_][\w-]*`
fn class_annotation(text: &str) -> Option<usize> {
	let blanks = blank_length(text);
	let after = text[blanks..].strip_prefix(":::").filter(|_| blanks > 0)?;
	let inner_blanks = blank_length(after);
	let name = &after[inner_blanks..];
	if !name.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_') {
		return None;
	}
	Some(blanks + 3 + inner_blanks + word_length(name))
}

/// `[ \t]+icon\([\w-]*(?::[\w-]+)?\)`
fn icon_annotation(text: &str) -> Option<usize> {
	let blanks = blank_length(text);
	let after = text[blanks..].strip_prefix("icon(").filter(|_| blanks > 0)?;
	let mut length = word_length(after);
	if let Some(name) = after[length..].strip_prefix(':') {
		let name_length = word_length(name);
		if name_length > 0 {
			length += 1 + name_length;
		}
	}
	after[length..].starts_with(')').then_some(blanks + 5 + length + 1)
}

/// `[ \t]+##[^\n\r]*`
fn description_annotation(text: &str) -> Option<usize> {
	let blanks = blank_length(text);
	let after = text[blanks..].strip_prefix("##").filter(|_| blanks > 0)?;
	Some(blanks + 2 + after.find(['\n', '\r']).unwrap_or(after.len()))
}

/// Whether an annotation starts at `text`, which ends a bare name:
/// `[ \t]+:::[ \t]*[A-Za-z_]|[ \t]+icon\(|[ \t]+##`.
fn starts_annotation(text: &str) -> bool {
	let blanks = blank_length(text);
	if blanks == 0 {
		return false;
	}
	let after = &text[blanks..];
	if let Some(class) = after.strip_prefix(":::") {
		return class[blank_length(class)..]
			.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_');
	}
	after.starts_with("icon(") || after.starts_with("##")
}

/// `(?!:::|icon\(|##)[^ \t\n\r"'](?:(?!<annotation>)[^\n\r])*`
fn bare_name(text: &str) -> Option<usize> {
	if text.starts_with(":::") || text.starts_with("icon(") || text.starts_with("##") {
		return None;
	}
	let first = text.chars().next().filter(|first| !" \t\n\r\"'".contains(*first))?;
	let mut length = first.len_utf8();
	while let Some(character) = text[length..].chars().next() {
		if character == '\n' || character == '\r' || starts_annotation(&text[length..]) {
			break;
		}
		length += character.len_utf8();
	}
	Some(length)
}

/// The kind and length of the token at the start of `rest`; `None` as kind for hidden text,
/// `Err` for a character no terminal matches.
fn token_at(rest: &str) -> std::result::Result<(Option<Kind>, usize), ()> {
	if let Some(after) = rest.strip_prefix(KEYWORD) {
		let is_restricted_end =
			after.starts_with("%%") || !after.starts_with(|c: char| !c.is_whitespace());
		// The keyword's longer alternative `BARE_NAME` wins when it matches more.
		let bare = bare_name(rest).unwrap_or(0);
		if is_restricted_end && bare <= KEYWORD.len() {
			return Ok((Some(Kind::Keyword), KEYWORD.len()));
		}
	}
	let field_start = &rest[blank_length(rest)..];
	if ["accDescr", "accTitle", "title"].iter().any(|word| field_start.starts_with(word)) {
		let mut scanner = Scanner::new(rest);
		if scanner.common_field(&mut CommonFields::default()) {
			return Ok((Some(Kind::CommonField), rest.len() - scanner.rest().len()));
		}
	}
	type Terminal = fn(&str) -> Option<usize>;
	let annotations: [(Kind, Terminal); 3] = [
		(Kind::ClassAnnotation, class_annotation),
		(Kind::IconAnnotation, icon_annotation),
		(Kind::DescriptionAnnotation, description_annotation),
	];
	for (kind, terminal) in annotations {
		if let Some(length) = terminal(rest) {
			return Ok((Some(kind), length));
		}
	}
	match blank_length(rest) {
		0 => {}
		blanks => return Ok((Some(Kind::Indentation), blanks)),
	}
	if let Some(quote) = rest.chars().next().filter(|&first| first == '"' || first == '\'')
		&& let Some(end) = rest[1..].find(quote)
	{
		return Ok((Some(Kind::QuotedName), end + 2));
	}
	if rest.starts_with("%%") {
		return Ok((None, rest.find('\n').unwrap_or(rest.len())));
	}
	if rest.starts_with('\n') {
		return Ok((None, 1));
	}
	if rest.starts_with("\r\n") {
		return Ok((None, 2));
	}
	match bare_name(rest) {
		Some(length) => Ok((Some(Kind::BareName), length)),
		None => Err(()),
	}
}

/// The tokens, or the line of the first character no terminal matches.
fn lex(code: &str) -> std::result::Result<Vec<Token<'_>>, usize> {
	let mut tokens = Vec::new();
	// Mermaid's preprocessing trims the start of the code.
	let mut position = code.len() - code.trim_start().len();
	let mut line = 1 + code[..position].matches('\n').count();
	let mut first_error = None;
	while position < code.len() {
		let rest = &code[position..];
		let length = match token_at(rest) {
			Ok((kind, length)) => {
				if let Some(kind) = kind {
					tokens.push(Token { kind, text: &rest[..length], line });
				}
				length
			}
			// Chevrotain skips the character and goes on.
			Err(()) => {
				first_error.get_or_insert(line);
				rest.chars().next().map_or(1, char::len_utf8)
			}
		};
		line += rest[..length].matches('\n').count();
		position += length;
	}
	match first_error {
		Some(line) => Err(line),
		None => Ok(tokens),
	}
}

/// `boxDrawingPreprocessor.ts`: `tree` output (`├── │ └──` and the heavy variants) as indented
/// rows. The original line of each line of the result, `None` where the input is unchanged.
fn convert_box_drawing(input: &str, context: &Context) -> Result<Option<(String, Vec<usize>)>> {
	const BRANCHES: [char; 4] = ['└', '┗', '├', '┣'];
	const DASHES: [char; 2] = ['─', '━'];
	const BOX_CHARACTERS: [char; 8] = ['─', '━', '│', '┃', '└', '┗', '├', '┣'];
	let is_decoration = |line: &str| {
		!line.is_empty() && line.chars().all(|c| c.is_whitespace() || c == '│' || c == '┃')
	};
	let is_metadata = |line: &str| {
		let trimmed = line.trim_start();
		let after_blanks = |word: &str| {
			trimmed.strip_prefix(word).map(|after| after.trim_start_matches([' ', '\t']))
		};
		trimmed.strip_prefix("title").is_some_and(|after| after.starts_with([' ', '\t']))
			|| after_blanks("accTitle").is_some_and(|after| after.starts_with(':'))
			|| after_blanks("accDescr").is_some_and(|after| after.starts_with([':', '{']))
	};
	let is_comment = |line: &str| line.trim_start().starts_with("%%");
	let is_passed_through =
		|line: &str| line.trim().is_empty() || is_comment(line) || is_metadata(line);
	// JavaScript string indices count UTF-16 code units.
	let column_of = |line: &str, byte: usize| line[..byte].encode_utf16().count();

	let lines: Vec<&str> = input.split('\n').collect();
	let Some(keyword_index) = lines.iter().position(|line| line.trim() == KEYWORD) else {
		return Ok(None);
	};
	let content: Vec<String> = lines[keyword_index + 1..]
		.iter()
		.filter(|line| !is_passed_through(line) && !is_decoration(line))
		.map(|line| line.replace('\t', "    "))
		.collect();
	if !content.iter().any(|line| line.contains(BOX_CHARACTERS)) {
		return Ok(None);
	}
	let segment_width = content
		.iter()
		.filter_map(|line| line.find(BRANCHES).map(|byte| column_of(line, byte)))
		.find(|&column| column > 0)
		.unwrap_or(4);

	let mut output: Vec<String> = Vec::with_capacity(lines.len());
	let mut line_map = Vec::with_capacity(lines.len());
	for (index, line) in lines.iter().enumerate() {
		let original_line = index + 1;
		if index <= keyword_index || is_passed_through(line) {
			output.push((*line).to_owned());
			line_map.push(original_line);
			continue;
		}
		if is_decoration(line) {
			continue;
		}
		let normalized = line.replace('\t', "    ");
		if let Some(branch) = normalized.find(BRANCHES) {
			let column = column_of(&normalized, branch);
			let depth = super::super::color::js_round(column as f64 / segment_width as f64) + 1.0;
			let after_branch = &normalized[branch..];
			let after_branch =
				&after_branch[after_branch.chars().next().map_or(0, char::len_utf8)..];
			let text = after_branch.trim_start_matches(DASHES).trim_start_matches(' ').trim_end();
			if text.is_empty() {
				return Err(context.error_at(
					original_line,
					"Empty node: expected a file or directory name after the box-drawing prefix",
				));
			}
			output.push(format!("{}{text}", "    ".repeat(depth.max(0.0) as usize)));
			line_map.push(original_line);
		} else if normalized.chars().all(|c| c.is_whitespace() || BOX_CHARACTERS.contains(&c)) {
			continue;
		} else if normalized.contains(BOX_CHARACTERS) {
			output.push((*line).to_owned());
			line_map.push(original_line);
		} else if normalized.starts_with(char::is_whitespace) {
			return Err(context.error_at(
				original_line,
				"Unexpected indentation without box-drawing characters: use ├── or └── prefixes for indented nodes",
			));
		} else {
			output.push((*line).to_owned());
			line_map.push(original_line);
		}
	}
	Ok(Some((output.join("\n"), line_map)))
}

fn describe(token: Option<&Token>) -> String {
	match token {
		None => "the end of the diagram".to_owned(),
		Some(token) if token.kind == Kind::Indentation => "blanks".to_owned(),
		Some(token) => {
			let text: String = token.text.trim().chars().take(20).collect();
			format!("{text:?}")
		}
	}
}

/// The value of an annotation without its marker (`:::`, `icon(…)`, `##`), trimmed.
fn annotation_value(token: &Token) -> String {
	let trimmed = token.text.trim();
	match token.kind {
		Kind::ClassAnnotation => trimmed[3..].trim().to_owned(),
		Kind::IconAnnotation => trimmed[5..trimmed.len() - 1].to_owned(),
		_ => trimmed[2..].trim().to_owned(),
	}
}

pub(super) fn parse(code: &str, context: &Context) -> Result<Tree> {
	let converted = convert_box_drawing(code, context)?;
	let (text, line_map) = match &converted {
		Some((text, line_map)) => (text.as_str(), Some(line_map)),
		None => (code, None),
	};
	let original_line = |line: usize| {
		line_map.and_then(|map| map.get(line.wrapping_sub(1)).copied()).unwrap_or(line)
	};
	let tokens = match lex(text) {
		Ok(tokens) => tokens,
		Err(line) => {
			return Err(context.error_at(original_line(line), "unexpected character"));
		}
	};
	let error = |index: usize, expected: &str| {
		let token = tokens.get(index);
		let line = token.or(tokens.last()).map_or(1, |token| token.line);
		context.error_at(
			original_line(line),
			format!("expected {expected}, found {}", describe(token)),
		)
	};

	let mut index = 0;
	if tokens.first().map(|token| token.kind) != Some(Kind::Keyword) {
		return Err(error(0, KEYWORD));
	}
	index += 1;
	let mut fields = CommonFields::new(context);
	while let Some(token) = tokens.get(index).filter(|token| token.kind == Kind::CommonField) {
		Scanner::new(token.text).common_field(&mut fields);
		index += 1;
	}
	let root = Node {
		name: "/".to_owned(),
		node_type: NodeType::Directory,
		class: None,
		icon: None,
		description: None,
		children: Vec::new(),
		level: -1,
	};
	let mut nodes = vec![root];
	let mut stack = vec![0];
	while index < tokens.len() {
		let mut level = 0;
		if tokens[index].kind == Kind::Indentation {
			level = tokens[index].text.len() as i64;
			index += 1;
		}
		let mut name = match tokens.get(index) {
			Some(token) if token.kind == Kind::QuotedName => {
				token.text[1..token.text.len() - 1].to_owned()
			}
			Some(token) if token.kind == Kind::BareName => {
				token.text.trim_end_matches([' ', '\t']).to_owned()
			}
			_ if level > 0 => return Err(error(index, "a name")),
			_ => return Err(error(index, "a name or the end of the diagram")),
		};
		index += 1;
		let (mut class, mut icon, mut description) = (None, None, None);
		while let Some(token) = tokens.get(index) {
			let annotation = match token.kind {
				Kind::ClassAnnotation => &mut class,
				Kind::IconAnnotation => &mut icon,
				Kind::DescriptionAnnotation => &mut description,
				_ => break,
			};
			*annotation = Some(annotation_value(token));
			index += 1;
		}
		let node_type = match name.strip_suffix('/') {
			Some(directory) => {
				name = directory.to_owned();
				NodeType::Directory
			}
			None => NodeType::File,
		};
		while stack.len() > 1 && stack.last().is_some_and(|&top| level <= nodes[top].level) {
			stack.pop();
		}
		let node = nodes.len();
		nodes.push(Node {
			name,
			node_type,
			class: class.filter(|class| !class.is_empty()),
			icon: icon.map(|icon| if icon.is_empty() { "none".to_owned() } else { icon }),
			description: description
				.filter(|description| !description.is_empty())
				.map(|description| sanitize_text(&description)),
			children: Vec::new(),
			level,
		});
		if let Some(&parent) = stack.last() {
			nodes[parent].children.push(node);
		}
		stack.push(node);
	}
	Ok(Tree { fields, nodes })
}

#[cfg(test)]
mod tests {
	use super::super::super::text::testing::FixedMeasure;
	use super::*;

	fn tree(code: &str) -> Result<Tree> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	/// The rows as `level name` in preorder, with their annotations.
	fn rows(code: &str) -> Vec<String> {
		let tree = tree(code).unwrap();
		let mut rows = Vec::new();
		let mut stack = vec![(0usize, 0usize)];
		while let Some((node, depth)) = stack.pop() {
			let data = &tree.nodes[node];
			let mut row = format!("{depth} {}", data.name);
			if data.node_type == NodeType::Directory {
				row.push('/');
			}
			for (marker, value) in
				[(":::", &data.class), (" icon=", &data.icon), (" ## ", &data.description)]
			{
				if let Some(value) = value {
					row.push_str(&format!("{marker}{value}"));
				}
			}
			rows.push(row);
			stack.extend(data.children.iter().rev().map(|&child| (child, depth + 1)));
		}
		rows
	}

	fn error_line(code: &str) -> Option<usize> {
		tree(code).err().and_then(|error| error.line)
	}

	#[test]
	fn parses_rows_and_annotations() {
		assert_eq!(
			rows("treeView-beta\n    src/\n        index.js\n    README.md\n"),
			["0 //", "1 src/", "2 index.js", "1 README.md"]
		);
		assert_eq!(
			rows("treeView-beta\n  a :::highlight\n  b icon(file) ## desc here\n  'c' :::x :::y"),
			["0 //", "1 a:::highlight", "1 b icon=file ## desc here", "1 c:::y"]
		);
		assert_eq!(rows("treeView-beta\n  b icon() ## x <b>"), ["0 //", "1 b icon=none ## x <b>"]);
		// Line ends are hidden: blanks between names indent the next row.
		assert_eq!(rows("treeView-beta\n  \"a\" \"b\"\n"), ["0 //", "1 a", "1 b"]);
		assert_eq!(rows("treeView-beta\n  a:::x\n  b %% c"), ["0 //", "1 a:::x", "1 b %% c"]);
		// `TITLE` matches before `BARE_NAME`.
		assert_eq!(rows("treeView-beta\n  titles.txt"), ["0 //", "1 s.txt"]);
		assert_eq!(
			rows("treeView-beta\n  a\n  b\n    c\n      d\n  e\n\t\tf\n"),
			["0 //", "1 a", "1 b", "2 c", "3 d", "1 e", "1 f"]
		);
		let tree = tree("treeView-beta\ntitle T\naccTitle: A\naccDescr { D }\n  a").unwrap();
		assert_eq!(tree.fields.title.as_deref(), Some("T"));
		assert_eq!(tree.fields.accessibility.title, "A");
	}

	#[test]
	fn converts_box_drawing() {
		assert_eq!(
			rows("treeView-beta\n├── src\n│   ├── a.js\n│   └── b.js\n│\n└── README.md\n"),
			["0 //", "1 src", "2 a.js", "2 b.js", "1 README.md"]
		);
		assert_eq!(
			rows("treeView-beta\nroot/\n┣━━ a\n┃   ┗━━ b ## d\nSection ─ A.txt"),
			["0 //", "1 root/", "2 a", "3 b ## d", "1 Section ─ A.txt"]
		);
		assert_eq!(error_line("treeView-beta\n├── a\n├──\n"), Some(3));
		assert_eq!(error_line("treeView-beta\n├── a\n  b\n"), Some(3));
		// Lines of errors after dropped decoration lines are the original ones.
		assert_eq!(error_line("treeView-beta\n├── a\n│\n├── \"b\n"), Some(4));
	}

	#[test]
	fn rejects_what_mermaid_rejects() {
		assert_eq!(error_line("treeView-beta\n  a\n    \n  b\n"), Some(4));
		assert_eq!(error_line("treeView-beta\n:::x\n"), Some(2));
		assert_eq!(error_line("treeView-beta\n  \"unterminated\n"), Some(2));
		assert_eq!(error_line("treeView-beta  \n  a\n"), Some(1));
		assert_eq!(error_line("treeView-beta\n  a\ntitle x\n"), Some(3));
		assert_eq!(error_line("treeView-beta\n  a\n  "), Some(3));
		assert!(tree("treeView-beta").is_ok());
		assert!(tree("\n  treeView-beta\n  a").is_ok());
	}
}
