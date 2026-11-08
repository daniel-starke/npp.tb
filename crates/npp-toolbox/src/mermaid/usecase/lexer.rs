//! The Chevrotain lexer of use case diagrams (`usecase.tokens.ts`).
//!
//! Chevrotain takes the first token type that matches at the current offset, in the order of
//! the mode's token list. A keyword hands over to `IDENTIFIER` if that matches more (`actors`),
//! and `IDENTIFIER` to `NUMBER` (`1.5px`), one level only. Blanks are skipped; `"` and `'` that
//! open no complete string are the only characters of the default mode no token takes.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	MarkdownString,
	UnclosedMarkdownString,
	Comment,
	NewLine,
	AccDescrBlock,
	AccTitleLine,
	AccDescrLine,
	JsonDeclarationStart,
	Usecase,
	Actor,
	SystemBoundary,
	End,
	Direction,
	Td,
	Tb,
	Bt,
	Lr,
	Rl,
	Note,
	For,
	Json,
	ClassDef,
	Class,
	Style,
	Include,
	Extend,
	True,
	False,
	Generalization,
	DependencyArrow,
	StereotypeStart,
	ClassSeparator,
	ForwardSolid,
	BackwardSolid,
	ForwardCircle,
	BackwardCircle,
	ForwardCross,
	BackwardCross,
	MarkerlessSolid,
	MetadataStart,
	At,
	LeftBrace,
	RightBrace,
	LeftBracket,
	RightBracket,
	LeftParen,
	RightParen,
	Comma,
	Colon,
	HashColor,
	PlainString,
	CssIdentifier,
	Identifier,
	Number,
	CssEscapedComma,
	Dash,
	Dot,
	Percent,
	CssPunctuation,
	LabelPunctuation,
	LabelSymbol,
	JsonObjectLiteral,
	UnclosedJsonObjectLiteral,
	StereotypeEnd,
	StereotypeText,
	UnclosedStereotypeText,
	Eof,
}

impl Kind {
	/// The category `WORD`: keywords and identifiers.
	pub fn is_word(self) -> bool {
		use Kind::*;
		matches!(
			self,
			Usecase
				| Actor | SystemBoundary
				| End | Direction
				| Td | Tb | Bt
				| Lr | Rl | Note
				| For | Json | ClassDef
				| Class | Style
				| Include | Extend
				| True | False
				| Identifier
		)
	}

	/// The category `LABEL_TEXT`: what an unquoted label may contain.
	pub fn is_label_text(self) -> bool {
		use Kind::*;
		self.is_word()
			|| matches!(
				self,
				Number
					| At | Comma | Colon
					| HashColor | CssIdentifier
					| CssEscapedComma
					| Dash | Dot | Percent
					| CssPunctuation
					| LabelPunctuation
					| LabelSymbol
			)
	}

	/// A token of an edge label or node label: label text and both string forms.
	pub fn is_label(self) -> bool {
		self.is_label_text() || matches!(self, Kind::PlainString | Kind::MarkdownString)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
	pub kind: Kind,
	/// Byte offsets into the text.
	pub start: usize,
	pub end: usize,
	pub line: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
	Default,
	JsonBody,
	Stereotype,
}

const KEYWORDS: &[(&str, Kind)] = &[
	("usecase-beta", Kind::Usecase),
	("actor", Kind::Actor),
	("systemBoundary", Kind::SystemBoundary),
	("end", Kind::End),
	("direction", Kind::Direction),
	("TD", Kind::Td),
	("TB", Kind::Tb),
	("BT", Kind::Bt),
	("LR", Kind::Lr),
	("RL", Kind::Rl),
	("note", Kind::Note),
	("for", Kind::For),
	("json", Kind::Json),
	("classDef", Kind::ClassDef),
	("class", Kind::Class),
	("style", Kind::Style),
	("include", Kind::Include),
	("extend", Kind::Extend),
	("true", Kind::True),
	("false", Kind::False),
];

fn is_word_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_'
}

/// `\w*` from `start`.
fn word_end(bytes: &[u8], start: usize) -> usize {
	let mut end = start;
	while end < bytes.len() && is_word_byte(bytes[end]) {
		end += 1;
	}
	end
}

/// `[\t ]*` from `start`.
fn blank_end(bytes: &[u8], start: usize) -> usize {
	let mut end = start;
	while end < bytes.len() && matches!(bytes[end], b' ' | b'\t') {
		end += 1;
	}
	end
}

/// Whether only blanks stand between the line start and `offset`.
fn is_indented_line_start(bytes: &[u8], offset: usize) -> bool {
	bytes[..offset]
		.iter()
		.rev()
		.take_while(|&&byte| byte != b'\n' && byte != b'\r')
		.all(|&byte| byte == b' ' || byte == b'\t')
}

fn line_end(bytes: &[u8], start: usize) -> usize {
	bytes[start..]
		.iter()
		.position(|&byte| byte == b'\n' || byte == b'\r')
		.map_or(bytes.len(), |n| n + start)
}

/// `"` followed by a backtick, up to the first backtick followed by `"`.
fn markdown_string(bytes: &[u8], offset: usize) -> Option<usize> {
	if !bytes[offset..].starts_with(b"\"`") {
		return None;
	}
	(offset + 2..bytes.len().saturating_sub(1))
		.find(|&index| bytes[index] == b'`' && bytes[index + 1] == b'"')
		.map(|index| index + 2)
}

/// `%%` at an indented line start, to the line end.
fn comment(bytes: &[u8], offset: usize) -> Option<usize> {
	(bytes[offset..].starts_with(b"%%") && is_indented_line_start(bytes, offset))
		.then(|| line_end(bytes, offset))
}

/// `accDescr[\t ]*{` at an indented line start, to the next `}`.
fn acc_descr_block(bytes: &[u8], offset: usize) -> Option<usize> {
	if !bytes[offset..].starts_with(b"accDescr") || !is_indented_line_start(bytes, offset) {
		return None;
	}
	let brace = blank_end(bytes, offset + 8);
	if bytes.get(brace) != Some(&b'{') {
		return None;
	}
	bytes[brace + 1..].iter().position(|&byte| byte == b'}').map(|n| brace + 1 + n + 1)
}

/// `<keyword>[\t ]*:[^\n\r]*` at an indented line start.
fn accessibility_line(bytes: &[u8], offset: usize, keyword: &[u8]) -> Option<usize> {
	if !bytes[offset..].starts_with(keyword) || !is_indented_line_start(bytes, offset) {
		return None;
	}
	let colon = blank_end(bytes, offset + keyword.len());
	(bytes.get(colon) == Some(&b':')).then(|| line_end(bytes, colon))
}

/// `json[\t ]+\w+[\t ]*@[\t ]*(?={)`
fn json_declaration_start(bytes: &[u8], offset: usize) -> Option<usize> {
	if !bytes[offset..].starts_with(b"json") {
		return None;
	}
	let name = blank_end(bytes, offset + 4);
	if name == offset + 4 {
		return None;
	}
	let name_end = word_end(bytes, name);
	if name_end == name {
		return None;
	}
	let at = blank_end(bytes, name_end);
	if bytes.get(at) != Some(&b'@') {
		return None;
	}
	let brace = blank_end(bytes, at + 1);
	(bytes.get(brace) == Some(&b'{')).then_some(brace)
}

/// The first `}` closing the `{` at `offset`, outside of JSON strings.
fn json_object(bytes: &[u8], offset: usize) -> Option<usize> {
	if bytes.get(offset) != Some(&b'{') {
		return None;
	}
	let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
	for (index, &byte) in bytes.iter().enumerate().skip(offset) {
		if quoted {
			if escaped {
				escaped = false;
			} else if byte == b'\\' {
				escaped = true;
			} else if byte == b'"' {
				quoted = false;
			}
			continue;
		}
		match byte {
			b'"' => quoted = true,
			b'{' => depth += 1,
			b'}' => {
				depth -= 1;
				if depth == 0 {
					return Some(index + 1);
				}
			}
			_ => {}
		}
	}
	None
}

/// The text up to `>>` on the same line, unless it is blank.
fn stereotype_text(text: &str, offset: usize) -> Option<usize> {
	let end = offset + text[offset..].find(">>")?;
	let image = &text[offset..end];
	(!image.contains(['\n', '\r']) && !image.trim().is_empty()).then_some(end)
}

/// `--+>` or `--+`: the end after two or more dashes.
fn dashes(bytes: &[u8], offset: usize) -> Option<usize> {
	let count = bytes[offset..].iter().take_while(|&&byte| byte == b'-').count();
	(count >= 2).then_some(offset + count)
}

/// `[A-Z_a-z]\w*(?:-\w+)+`
fn css_identifier(bytes: &[u8], offset: usize) -> Option<usize> {
	let first = *bytes.get(offset)?;
	if !(first.is_ascii_alphabetic() || first == b'_') {
		return None;
	}
	let mut end = word_end(bytes, offset);
	let mut groups = 0;
	while bytes.get(end) == Some(&b'-')
		&& bytes.get(end + 1).is_some_and(|&byte| is_word_byte(byte))
	{
		end = word_end(bytes, end + 1);
		groups += 1;
	}
	(groups > 0).then_some(end)
}

/// `(?:\d+\.\d+|\d+|\.\d+)(?:[A-Za-z]+)?`
fn number(bytes: &[u8], offset: usize) -> Option<usize> {
	let digits =
		|start: usize| bytes[start..].iter().take_while(|byte| byte.is_ascii_digit()).count();
	let integer = digits(offset);
	let mut end = if integer > 0 {
		let fraction = match bytes.get(offset + integer) {
			Some(b'.') => digits(offset + integer + 1),
			_ => 0,
		};
		if fraction > 0 { offset + integer + 1 + fraction } else { offset + integer }
	} else if bytes.get(offset) == Some(&b'.') && digits(offset + 1) > 0 {
		offset + 1 + digits(offset + 1)
	} else {
		return None;
	};
	end += bytes[end..].iter().take_while(|byte| byte.is_ascii_alphabetic()).count();
	Some(end)
}

/// `[^\t\n\r !-~]+`: characters outside printable ASCII.
fn label_symbol(text: &str, offset: usize) -> Option<usize> {
	let length: usize = text[offset..]
		.chars()
		.take_while(|&character| !matches!(character, '\t' | '\n' | '\r' | ' '..='~'))
		.map(char::len_utf8)
		.sum();
	(length > 0).then_some(offset + length)
}

fn plain_string(bytes: &[u8], offset: usize) -> Option<usize> {
	let quote = *bytes.get(offset)?;
	if quote != b'"' && quote != b'\'' {
		return None;
	}
	let length = bytes[offset + 1..]
		.iter()
		.position(|&byte| matches!(byte, b'\n' | b'\r') || byte == quote)?;
	(bytes[offset + 1 + length] == quote).then_some(offset + length + 2)
}

/// The token of the default mode at `offset`, `None` for skipped blanks.
fn default_token(text: &str, offset: usize) -> Option<(Kind, usize)> {
	let bytes = text.as_bytes();
	let rest = &bytes[offset..];
	if let Some(end) = markdown_string(bytes, offset) {
		return Some((Kind::MarkdownString, end));
	}
	if rest.starts_with(b"\"`") {
		return Some((Kind::UnclosedMarkdownString, bytes.len()));
	}
	if let Some(end) = comment(bytes, offset) {
		return Some((Kind::Comment, end));
	}
	match rest {
		[b'\r', b'\n', ..] => return Some((Kind::NewLine, offset + 2)),
		[b'\n' | b'\r', ..] => return Some((Kind::NewLine, offset + 1)),
		_ => {}
	}
	if let Some(end) = acc_descr_block(bytes, offset) {
		return Some((Kind::AccDescrBlock, end));
	}
	if let Some(end) = accessibility_line(bytes, offset, b"accTitle") {
		return Some((Kind::AccTitleLine, end));
	}
	if let Some(end) = accessibility_line(bytes, offset, b"accDescr") {
		return Some((Kind::AccDescrLine, end));
	}
	if let Some(end) = json_declaration_start(bytes, offset) {
		return Some((Kind::JsonDeclarationStart, end));
	}
	let identifier_end = word_end(bytes, offset);
	for &(keyword, kind) in KEYWORDS {
		let matches = match kind {
			Kind::Include | Kind::Extend => {
				rest.len() >= keyword.len()
					&& rest[..keyword.len()].eq_ignore_ascii_case(keyword.as_bytes())
			}
			_ => rest.starts_with(keyword.as_bytes()),
		};
		if matches {
			let end = offset + keyword.len();
			return Some(match identifier_end > end {
				true => (Kind::Identifier, identifier_end),
				false => (kind, end),
			});
		}
	}
	let fixed: &[(&[u8], Kind)] = &[
		(b"--|>", Kind::Generalization),
		(b"..>", Kind::DependencyArrow),
		(b"<<", Kind::StereotypeStart),
		(b":::", Kind::ClassSeparator),
	];
	for &(pattern, kind) in fixed {
		if rest.starts_with(pattern) {
			return Some((kind, offset + pattern.len()));
		}
	}
	if let Some(end) = dashes(bytes, offset).filter(|&end| bytes.get(end) == Some(&b'>')) {
		return Some((Kind::ForwardSolid, end + 1));
	}
	if rest.starts_with(b"<")
		&& let Some(end) = dashes(bytes, offset + 1)
	{
		return Some((Kind::BackwardSolid, end));
	}
	let fixed: &[(&[u8], Kind)] = &[
		(b"--o", Kind::ForwardCircle),
		(b"o--", Kind::BackwardCircle),
		(b"--x", Kind::ForwardCross),
		(b"x--", Kind::BackwardCross),
	];
	for &(pattern, kind) in fixed {
		if rest.starts_with(pattern) {
			return Some((kind, offset + pattern.len()));
		}
	}
	if let Some(end) = dashes(bytes, offset) {
		return Some((Kind::MarkerlessSolid, end));
	}
	let single = match rest[0] {
		b'@' if rest.get(1) == Some(&b'{') => return Some((Kind::MetadataStart, offset + 2)),
		b'@' => Some(Kind::At),
		b'{' => Some(Kind::LeftBrace),
		b'}' => Some(Kind::RightBrace),
		b'[' => Some(Kind::LeftBracket),
		b']' => Some(Kind::RightBracket),
		b'(' => Some(Kind::LeftParen),
		b')' => Some(Kind::RightParen),
		b',' => Some(Kind::Comma),
		b':' => Some(Kind::Colon),
		_ => None,
	};
	if let Some(kind) = single {
		return Some((kind, offset + 1));
	}
	if rest[0] == b'#' {
		let digits = rest[1..].iter().take_while(|byte| byte.is_ascii_hexdigit()).count();
		if digits > 0 {
			return Some((Kind::HashColor, offset + 1 + digits));
		}
	}
	if let Some(end) = plain_string(bytes, offset) {
		return Some((Kind::PlainString, end));
	}
	if let Some(end) = css_identifier(bytes, offset) {
		return Some((Kind::CssIdentifier, end));
	}
	if identifier_end > offset {
		return Some(match number(bytes, offset) {
			Some(end) if end > identifier_end => (Kind::Number, end),
			_ => (Kind::Identifier, identifier_end),
		});
	}
	if let Some(end) = number(bytes, offset) {
		return Some((Kind::Number, end));
	}
	if rest.starts_with(b"\\,") {
		return Some((Kind::CssEscapedComma, offset + 2));
	}
	let single = match rest[0] {
		b'-' => Some(Kind::Dash),
		b'.' => Some(Kind::Dot),
		b'%' => Some(Kind::Percent),
		b'!' | b'#' | b'$' | b'&' | b'*' | b'+' | b'/' | b'=' | b'?' | b'^' | b'_' | b'|'
		| b'~' => Some(Kind::CssPunctuation),
		b';' | b'<' | b'>' | b'\\' | b'`' => Some(Kind::LabelPunctuation),
		_ => None,
	};
	if let Some(kind) = single {
		return Some((kind, offset + 1));
	}
	label_symbol(text, offset).map(|end| (Kind::LabelSymbol, end))
}

/// A character no token takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexError {
	pub offset: usize,
	pub line: usize,
}

/// The line of each offset: the starts of the lines.
pub struct LineIndex {
	starts: Vec<usize>,
}

impl LineIndex {
	pub fn new(text: &str) -> LineIndex {
		let bytes = text.as_bytes();
		let mut starts = vec![0];
		let mut index = 0;
		while index < bytes.len() {
			match bytes[index] {
				b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
					index += 2;
					starts.push(index);
					continue;
				}
				b'\r' | b'\n' => starts.push(index + 1),
				_ => {}
			}
			index += 1;
		}
		LineIndex { starts }
	}

	/// The 1-based line of `offset`.
	pub fn line(&self, offset: usize) -> usize {
		self.starts.partition_point(|&start| start <= offset)
	}
}

/// The tokens of `text` from `start`, followed by `Eof`.
pub fn tokenize(text: &str, start: usize, lines: &LineIndex) -> Result<Vec<Token>, LexError> {
	let bytes = text.as_bytes();
	let mut tokens = Vec::new();
	let mut modes = vec![Mode::Default];
	let mut offset = start;
	while offset < bytes.len() {
		let mode = modes.last().copied().unwrap_or(Mode::Default);
		let found = match mode {
			Mode::Default => {
				if matches!(bytes[offset], b' ' | b'\t') {
					offset = blank_end(bytes, offset);
					continue;
				}
				default_token(text, offset)
			}
			Mode::JsonBody => match json_object(bytes, offset) {
				Some(end) => Some((Kind::JsonObjectLiteral, end)),
				None if bytes[offset] == b'{' => {
					Some((Kind::UnclosedJsonObjectLiteral, bytes.len()))
				}
				None => None,
			},
			Mode::Stereotype => {
				if bytes[offset..].starts_with(b">>") {
					Some((Kind::StereotypeEnd, offset + 2))
				} else if let Some(end) = stereotype_text(text, offset) {
					Some((Kind::StereotypeText, end))
				} else {
					let end = line_end(bytes, offset);
					(end > offset).then_some((Kind::UnclosedStereotypeText, end))
				}
			}
		};
		let Some((kind, end)) = found else {
			return Err(LexError { offset, line: lines.line(offset) });
		};
		match kind {
			Kind::JsonDeclarationStart => modes.push(Mode::JsonBody),
			Kind::StereotypeStart => modes.push(Mode::Stereotype),
			Kind::JsonObjectLiteral
			| Kind::UnclosedJsonObjectLiteral
			| Kind::StereotypeEnd
			| Kind::UnclosedStereotypeText
				if modes.len() > 1 =>
			{
				modes.pop();
			}
			_ => {}
		}
		tokens.push(Token { kind, start: offset, end, line: lines.line(offset) });
		offset = end;
	}
	tokens.push(Token {
		kind: Kind::Eof,
		start: bytes.len(),
		end: bytes.len(),
		line: lines.line(bytes.len()),
	});
	Ok(tokens)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn kinds(text: &str) -> Vec<Kind> {
		let lines = LineIndex::new(text);
		tokenize(text, 0, &lines).unwrap().into_iter().map(|token| token.kind).collect()
	}

	#[test]
	fn lexes_like_chevrotain() {
		use Kind::*;
		assert_eq!(kinds("actors actor"), [Identifier, Actor, Eof]);
		assert_eq!(kinds("1mg 1.5px .5"), [Identifier, Number, Number, Eof]);
		assert_eq!(
			kinds("a --> b <--- c"),
			[Identifier, ForwardSolid, Identifier, BackwardSolid, Identifier, Eof]
		);
		assert_eq!(
			kinds("o-- x-- --o --x -- --|> ..>"),
			[
				BackwardCircle,
				BackwardCross,
				ForwardCircle,
				ForwardCross,
				MarkerlessSolid,
				Generalization,
				DependencyArrow,
				Eof
			]
		);
		assert_eq!(kinds("stroke-width:2px"), [CssIdentifier, Colon, Identifier, Eof]);
		assert_eq!(kinds("Include EXTEND includes"), [Include, Extend, Identifier, Eof]);
		assert_eq!(
			kinds("a << Core >> b"),
			[Identifier, StereotypeStart, StereotypeText, StereotypeEnd, Identifier, Eof]
		);
		assert_eq!(
			kinds("json P@{ \"a\": \"}\" }:::x"),
			[JsonDeclarationStart, JsonObjectLiteral, ClassSeparator, Identifier, Eof]
		);
		assert_eq!(kinds("  %% note\n%x"), [Comment, NewLine, Percent, Identifier, Eof]);
		assert_eq!(
			kinds("x(\"`a\nb`\") ü"),
			[Identifier, LeftParen, MarkdownString, RightParen, LabelSymbol, Eof]
		);
		assert_eq!(kinds("accDescr {\n a\n}\n"), [AccDescrBlock, NewLine, Eof]);
		let lines = LineIndex::new("a\n\"b");
		assert_eq!(tokenize("a\n\"b", 0, &lines), Err(LexError { offset: 2, line: 2 }));
		// The stereotype mode has no token for a line end.
		let lines = LineIndex::new("a <<\nb");
		assert_eq!(tokenize("a <<\nb", 0, &lines), Err(LexError { offset: 4, line: 1 }));
	}
}
