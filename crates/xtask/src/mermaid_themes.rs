//! `update-mermaid-themes`: regenerates `crates/npp-toolbox/src/mermaid/theme_table.rs`, Mermaid's
//! themes compiled to bytecode.
//!
//! Mermaid derives its theme variables in JavaScript (`packages/mermaid/src/themes/theme-*.js`):
//! a `constructor` sets the base values, `updateColors` derives the others, and the order of the
//! statements decides which values user overrides reach. The bodies of both methods are compiled
//! for a small stack machine (`mermaid/theme.rs`), so that the plugin computes exactly Mermaid's
//! values without a hand port of every theme. `calculate`, which applies the overrides, is
//! recognized as one of its known variants. The downloads are cached in
//! `target/mermaid-themes/<version>/`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::npp_interface::{download, run_rustfmt};

const RAW_FILES_URL: &str = "https://raw.githubusercontent.com/mermaid-js/mermaid";
const THEMES_PATH: &str = "packages/mermaid/src/themes";
const HARDCODED_VALUES_FILE: &str = "erDiagram-oldHardcodedValues.ts";
pub const GENERATED_FILE: &str = "crates/npp-toolbox/src/mermaid/theme_table.rs";

/// Theme names of `themes/index.js` and their files.
const THEMES: [(&str, &str); 11] = [
	("base", "theme-base.js"),
	("dark", "theme-dark.js"),
	("default", "theme-default.js"),
	("forest", "theme-forest.js"),
	("neutral", "theme-neutral.js"),
	("neo", "theme-neo.js"),
	("neo-dark", "theme-neo-dark.js"),
	("redux", "theme-redux.js"),
	("redux-dark", "theme-redux-dark.js"),
	("redux-color", "theme-redux-color.js"),
	("redux-dark-color", "theme-redux-dark-color.js"),
];

/// The functions the themes import from `khroma` and `theme-helpers.js`, in the order of their
/// ids in the bytecode.
const FUNCTIONS: [&str; 9] = [
	"adjust",
	"darken",
	"lighten",
	"invert",
	"rgba",
	"isDark",
	"isLight",
	"mkBorder",
	"transparentize",
];

/// The instruction set, written into the generated file for the interpreter. Operands are
/// little-endian `u16` unless noted.
const OPCODES: [(&str, &str); 33] = [
	("UNDEFINED", "push undefined"),
	("NULL", "push null"),
	("TRUE", "push true"),
	("FALSE", "push false"),
	("STRING", "push STRINGS[operand]"),
	("NUMBER", "push NUMBERS[operand]"),
	("THIS_GET", "push this[STRINGS[operand]]"),
	("THIS_GET_DYNAMIC", "pop key, push this[key]"),
	("THIS_SET", "pop value, this[STRINGS[operand]] = value"),
	("THIS_SET_DYNAMIC", "pop value, pop key, this[key] = value"),
	("PROPERTY", "pop object, push object[STRINGS[operand]] (undefined for non-objects)"),
	("INDEX", "pop index, pop array, push array[index]"),
	("LOCAL_GET", "push locals[operand]"),
	("LOCAL_SET", "pop value, locals[operand] = value"),
	("ADD", "pop b, pop a, push a + b"),
	("SUBTRACT", "pop b, pop a, push a - b"),
	("MULTIPLY", "pop b, pop a, push a * b"),
	("DIVIDE", "pop b, pop a, push a / b"),
	("REMAINDER", "pop b, pop a, push a % b"),
	("NEGATE", "pop a, push -a"),
	("NOT", "pop a, push !a"),
	("STRICT_EQUAL", "pop b, pop a, push a === b"),
	("LESS", "pop b, pop a, push a < b"),
	("JUMP", "jump to operand"),
	("JUMP_IF_FALSE", "pop a, jump to operand if a is falsy"),
	("OR", "jump to operand if the top is truthy, else pop it"),
	("AND", "jump to operand if the top is falsy, else pop it"),
	("NULLISH", "jump to operand if the top is neither null nor undefined, else pop it"),
	(
		"CALL",
		"operand: function id (u8) and argument count (u8); pop the arguments, push the result",
	),
	("OBJECT", "operand: entry count; pop key and value pairs, push the object"),
	("ARRAY", "operand: item count; pop the items, push the array"),
	("UPDATE_COLORS", "run updateColors"),
	("POP", "pop and discard"),
];

pub fn update(workspace_root: &Path, version: Option<&str>) -> Result<(), String> {
	let version = version.unwrap_or("12.2.0");
	let tag = format!("mermaid@{version}");
	let cache_directory = workspace_root.join("target/mermaid-themes").join(version);
	fs::create_dir_all(&cache_directory)
		.map_err(|error| format!("cannot create {}: {error}", cache_directory.display()))?;
	let read = |file: &str| -> Result<String, String> {
		let path = cache_directory.join(file);
		if !path.exists() {
			download(&format!("{RAW_FILES_URL}/{tag}/{THEMES_PATH}/{file}"), &path)?;
		}
		fs::read_to_string(&path)
			.map_err(|error| format!("cannot read {}: {error}", path.display()))
	};
	let mut compiler = Compiler::default();
	for (name, value) in exported_string_constants(&read(HARDCODED_VALUES_FILE)?) {
		compiler.globals.insert(name, value);
	}
	let mut themes = Vec::new();
	for (name, file) in THEMES {
		let source = read(file)?;
		let theme = compiler.compile_theme(&source).map_err(|error| format!("{file}: {error}"))?;
		themes.push((name, theme));
	}
	let generated = generated_source(&compiler, &themes, version);
	let generated_file = workspace_root.join(GENERATED_FILE);
	fs::write(&generated_file, generated)
		.map_err(|error| format!("cannot write {}: {error}", generated_file.display()))?;
	run_rustfmt(&generated_file)?;
	let code_size: usize =
		themes.iter().map(|(_, theme)| theme.constructor.len() + theme.update_colors.len()).sum();
	println!(
		"{GENERATED_FILE}: {} themes of Mermaid {version}, {code_size} bytes of code",
		themes.len()
	);
	Ok(())
}

/// `export const NAME = 'value';` lines.
fn exported_string_constants(source: &str) -> Vec<(String, String)> {
	source
		.lines()
		.filter_map(|line| {
			let rest = line.trim().strip_prefix("export const ")?;
			let (name, value) = rest.split_once('=')?;
			let value = value.trim().trim_end_matches(';').trim();
			let value = value.strip_prefix('\'').and_then(|value| value.strip_suffix('\''))?;
			Some((name.trim().to_owned(), value.to_owned()))
		})
		.collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Calculate {
	/// Overrides, `updateColors`, overrides again.
	Standard,
	/// First resets all `'calculated'` placeholders to undefined (`theme-default.js`).
	ClearsCalculated,
	/// Turns `useGradient` off when only `nodeBorder` is overridden (`theme-base.js`).
	NodeBorderDisablesGradient,
}

struct CompiledTheme {
	constructor: Vec<u8>,
	update_colors: Vec<u8>,
	calculate: Calculate,
	local_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
	Identifier(String),
	Number(f64),
	String(String),
	Punctuation(&'static str),
}

const PUNCTUATION: [&str; 33] = [
	"===", "!==", "...", "&&", "||", "??", "?.", "==", "!=", "<=", ">=", "=>", "++", "--", "+=",
	"-=", "{", "}", "(", ")", "[", "]", ",", ";", ":", ".", "?", "=", "<", ">", "+", "-", "*",
];
const SINGLE_PUNCTUATION: [&str; 4] = ["/", "%", "!", "#"];

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
	let bytes = source.as_bytes();
	let mut tokens = Vec::new();
	let mut position = 0;
	while position < bytes.len() {
		let rest = &source[position..];
		let byte = bytes[position];
		if byte.is_ascii_whitespace() {
			position += 1;
		} else if rest.starts_with("//") {
			position += rest.find('\n').unwrap_or(rest.len());
		} else if rest.starts_with("/*") {
			position += rest.find("*/").ok_or("unterminated comment")? + 2;
		} else if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' {
			let length = rest
				.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
				.unwrap_or(rest.len());
			tokens.push(Token::Identifier(rest[..length].to_owned()));
			position += length;
		} else if byte.is_ascii_digit()
			|| (byte == b'.' && bytes.get(position + 1).is_some_and(u8::is_ascii_digit))
		{
			let length =
				rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
			let number =
				rest[..length].parse().map_err(|_| format!("bad number {}", &rest[..length]))?;
			tokens.push(Token::Number(number));
			position += length;
		} else if byte == b'\'' || byte == b'"' {
			let end = rest[1..].find(char::from(byte)).ok_or("unterminated string")? + 1;
			let text = &rest[1..end];
			if text.contains('\\') {
				return Err(format!("escapes in strings are not supported: {text}"));
			}
			tokens.push(Token::String(text.to_owned()));
			position += end + 1;
		} else if let Some(punctuation) =
			PUNCTUATION.iter().chain(&SINGLE_PUNCTUATION).find(|p| rest.starts_with(**p))
		{
			tokens.push(Token::Punctuation(punctuation));
			position += punctuation.len();
		} else {
			return Err(format!("unexpected character {:?}", rest.chars().next().unwrap_or(' ')));
		}
	}
	Ok(tokens)
}

#[derive(Default)]
struct Compiler {
	strings: Vec<String>,
	string_ids: HashMap<String, u16>,
	numbers: Vec<f64>,
	/// Imported string constants.
	globals: HashMap<String, String>,
}

impl Compiler {
	fn string_id(&mut self, text: &str) -> u16 {
		if let Some(&id) = self.string_ids.get(text) {
			return id;
		}
		let id = self.strings.len() as u16;
		self.strings.push(text.to_owned());
		self.string_ids.insert(text.to_owned(), id);
		id
	}

	fn number_id(&mut self, number: f64) -> u16 {
		match self.numbers.iter().position(|&known| known.to_bits() == number.to_bits()) {
			Some(index) => index as u16,
			None => {
				self.numbers.push(number);
				(self.numbers.len() - 1) as u16
			}
		}
	}

	fn compile_theme(&mut self, source: &str) -> Result<CompiledTheme, String> {
		let tokens = tokenize(source)?;
		let class_start = tokens
			.windows(3)
			.position(|window| {
				window[0] == Token::Identifier("class".into())
					&& window[1] == Token::Identifier("Theme".into())
					&& window[2] == Token::Punctuation("{")
			})
			.ok_or("no `class Theme {`")?;
		let methods = class_methods(&tokens, class_start + 3)?;
		let body = |name: &str| {
			methods
				.iter()
				.find(|(method, _)| method == name)
				.map(|(_, range)| &tokens[range.clone()])
				.ok_or(format!("no method {name}"))
		};
		let mut locals = Locals::default();
		let constructor = self.compile_body(body("constructor")?, &mut locals)?;
		let update_colors = self.compile_body(body("updateColors")?, &mut locals)?;
		let calculate = calculate_variant(body("calculate")?)?;
		Ok(CompiledTheme { constructor, update_colors, calculate, local_count: locals.count })
	}

	fn compile_body(&mut self, tokens: &[Token], locals: &mut Locals) -> Result<Vec<u8>, String> {
		let mut parser = Parser { tokens, position: 0, compiler: self, code: Vec::new(), locals };
		while parser.position < tokens.len() {
			parser.statement()?;
		}
		Ok(parser.code)
	}
}

/// Method names and their body token ranges (without braces).
fn class_methods(
	tokens: &[Token],
	mut position: usize,
) -> Result<Vec<(String, std::ops::Range<usize>)>, String> {
	let mut methods = Vec::new();
	while position < tokens.len() && tokens[position] != Token::Punctuation("}") {
		let Token::Identifier(name) = &tokens[position] else {
			return Err(format!("expected a method at token {position}"));
		};
		let open = tokens[position..]
			.iter()
			.position(|token| *token == Token::Punctuation("{"))
			.ok_or("no method body")?
			+ position;
		let close = matching_brace(tokens, open)?;
		methods.push((name.clone(), open + 1..close));
		position = close + 1;
	}
	Ok(methods)
}

fn matching_brace(tokens: &[Token], open: usize) -> Result<usize, String> {
	let mut depth = 0;
	for (index, token) in tokens.iter().enumerate().skip(open) {
		match token {
			Token::Punctuation("{") => depth += 1,
			Token::Punctuation("}") => {
				depth -= 1;
				if depth == 0 {
					return Ok(index);
				}
			}
			_ => {}
		}
	}
	Err("unbalanced braces".into())
}

fn calculate_variant(tokens: &[Token]) -> Result<Calculate, String> {
	let has = |text: &str| {
		tokens
			.iter()
			.any(|token| matches!(token, Token::String(s) | Token::Identifier(s) if s == text))
	};
	if !has("updateColors") || !has("applyOverride") {
		return Err("unknown calculate method".into());
	}
	match (has("calculated"), has("hasOwn")) {
		(false, false) => Ok(Calculate::Standard),
		(true, false) => Ok(Calculate::ClearsCalculated),
		(false, true) if has("nodeBorder") && has("useGradient") => {
			Ok(Calculate::NodeBorderDisablesGradient)
		}
		_ => Err("unknown calculate variant".into()),
	}
}

#[derive(Default)]
struct Locals {
	names: Vec<(String, u16)>,
	count: usize,
}

impl Locals {
	fn get(&self, name: &str) -> Option<u16> {
		self.names.iter().rev().find(|(known, _)| known == name).map(|&(_, slot)| slot)
	}

	fn declare(&mut self, name: &str) -> u16 {
		let slot = self.count as u16;
		self.count += 1;
		self.names.push((name.to_owned(), slot));
		slot
	}
}

struct Parser<'a> {
	tokens: &'a [Token],
	position: usize,
	compiler: &'a mut Compiler,
	code: Vec<u8>,
	locals: &'a mut Locals,
}

fn opcode(name: &str) -> u8 {
	OPCODES.iter().position(|(known, _)| *known == name).expect("known opcode") as u8
}

impl Parser<'_> {
	fn peek(&self) -> Option<&Token> {
		self.tokens.get(self.position)
	}

	fn peek_at(&self, offset: usize) -> Option<&Token> {
		self.tokens.get(self.position + offset)
	}

	fn is(&self, punctuation: &str) -> bool {
		matches!(self.peek(), Some(Token::Punctuation(p)) if *p == punctuation)
	}

	fn is_identifier(&self, name: &str) -> bool {
		matches!(self.peek(), Some(Token::Identifier(known)) if known == name)
	}

	fn eat(&mut self, punctuation: &str) -> bool {
		let is_match = self.is(punctuation);
		if is_match {
			self.position += 1;
		}
		is_match
	}

	fn expect(&mut self, punctuation: &str) -> Result<(), String> {
		match self.eat(punctuation) {
			true => Ok(()),
			false => Err(format!(
				"expected `{punctuation}`, found {:?} at token {}",
				self.peek(),
				self.position
			)),
		}
	}

	fn identifier(&mut self) -> Result<String, String> {
		match self.peek().cloned() {
			Some(Token::Identifier(name)) => {
				self.position += 1;
				Ok(name)
			}
			other => Err(format!("expected an identifier, found {other:?}")),
		}
	}

	fn emit(&mut self, name: &str) {
		self.code.push(opcode(name));
	}

	fn emit_u16(&mut self, name: &str, operand: u16) {
		self.code.push(opcode(name));
		self.code.extend_from_slice(&operand.to_le_bytes());
	}

	/// Emits a jump and returns the position of its operand for [`Self::patch`].
	fn emit_jump(&mut self, name: &str) -> usize {
		self.emit_u16(name, 0);
		self.code.len() - 2
	}

	fn patch(&mut self, operand_position: usize) {
		let target = self.code.len() as u16;
		self.code[operand_position..operand_position + 2].copy_from_slice(&target.to_le_bytes());
	}

	fn emit_string(&mut self, text: &str) {
		let id = self.compiler.string_id(text);
		self.emit_u16("STRING", id);
	}

	fn emit_number(&mut self, number: f64) {
		let id = self.compiler.number_id(number);
		self.emit_u16("NUMBER", id);
	}

	fn statement(&mut self) -> Result<(), String> {
		if self.eat(";") {
			return Ok(());
		}
		if self.eat("{") {
			while !self.eat("}") {
				self.statement()?;
			}
			return Ok(());
		}
		if self.is_identifier("const") || self.is_identifier("let") {
			self.position += 1;
			let name = self.identifier()?;
			self.expect("=")?;
			self.expression()?;
			let slot = self.locals.declare(&name);
			self.emit_u16("LOCAL_SET", slot);
			self.eat(";");
			return Ok(());
		}
		if self.is_identifier("if") {
			self.position += 1;
			self.expect("(")?;
			self.expression()?;
			self.expect(")")?;
			let skip_then = self.emit_jump("JUMP_IF_FALSE");
			self.statement()?;
			if self.is_identifier("else") {
				self.position += 1;
				let skip_else = self.emit_jump("JUMP");
				self.patch(skip_then);
				self.statement()?;
				self.patch(skip_else);
			} else {
				self.patch(skip_then);
			}
			return Ok(());
		}
		if self.is_identifier("for") {
			return self.for_statement();
		}
		if self.is_identifier("this")
			&& self.peek_at(1) == Some(&Token::Punctuation("."))
			&& self.peek_at(2) == Some(&Token::Identifier("updateColors".into()))
		{
			self.position += 3;
			self.expect("(")?;
			self.expect(")")?;
			self.emit("UPDATE_COLORS");
			self.eat(";");
			return Ok(());
		}
		if let Some(Token::Identifier(name)) = self.peek().cloned()
			&& self.peek_at(1) == Some(&Token::Punctuation("."))
			&& self.peek_at(2) == Some(&Token::Identifier("forEach".into()))
		{
			self.position += 3;
			return self.for_each_statement(&name);
		}
		self.assignment()?;
		self.eat(";");
		Ok(())
	}

	/// `for (let i = start; i < end; i++) statement`
	fn for_statement(&mut self) -> Result<(), String> {
		self.position += 1;
		self.expect("(")?;
		if !(self.is_identifier("let") || self.is_identifier("const")) {
			return Err("expected `let` in for".into());
		}
		self.position += 1;
		let name = self.identifier()?;
		self.expect("=")?;
		self.expression()?;
		let slot = self.locals.declare(&name);
		self.emit_u16("LOCAL_SET", slot);
		self.expect(";")?;
		let condition = self.code.len();
		self.expression()?;
		let exit = self.emit_jump("JUMP_IF_FALSE");
		self.expect(";")?;
		if self.identifier()? != name {
			return Err("unsupported for increment".into());
		}
		self.expect("++")?;
		self.expect(")")?;
		self.statement()?;
		self.emit_u16("LOCAL_GET", slot);
		self.emit_number(1.0);
		self.emit("ADD");
		self.emit_u16("LOCAL_SET", slot);
		self.emit_u16("JUMP", condition as u16);
		self.patch(exit);
		self.locals.names.retain(|(known, _)| *known != name);
		Ok(())
	}

	/// `array.forEach((item, index) => { … })`, compiled as a counting loop.
	fn for_each_statement(&mut self, array_name: &str) -> Result<(), String> {
		let array_slot =
			self.locals.get(array_name).ok_or(format!("forEach on unknown {array_name}"))?;
		self.expect("(")?;
		self.expect("(")?;
		let item = self.identifier()?;
		let index_name = match self.eat(",") {
			true => Some(self.identifier()?),
			false => None,
		};
		self.expect(")")?;
		self.expect("=>")?;
		let item_slot = self.locals.declare(&item);
		let index_slot = self.locals.declare(index_name.as_deref().unwrap_or("forEach index"));
		self.emit_number(0.0);
		self.emit_u16("LOCAL_SET", index_slot);
		let condition = self.code.len();
		self.emit_u16("LOCAL_GET", index_slot);
		self.emit_u16("LOCAL_GET", array_slot);
		self.property("length");
		self.emit("LESS");
		let exit = self.emit_jump("JUMP_IF_FALSE");
		self.emit_u16("LOCAL_GET", array_slot);
		self.emit_u16("LOCAL_GET", index_slot);
		self.emit("INDEX");
		self.emit_u16("LOCAL_SET", item_slot);
		self.statement()?;
		self.emit_u16("LOCAL_GET", index_slot);
		self.emit_number(1.0);
		self.emit("ADD");
		self.emit_u16("LOCAL_SET", index_slot);
		self.emit_u16("JUMP", condition as u16);
		self.patch(exit);
		self.expect(")")?;
		self.eat(";");
		self.locals.names.retain(|(known, _)| *known != item && Some(known) != index_name.as_ref());
		Ok(())
	}

	fn property(&mut self, name: &str) {
		let id = self.compiler.string_id(name);
		self.emit_u16("PROPERTY", id);
	}

	/// `this.name = …`, `this[key] = …` or `local = …`.
	fn assignment(&mut self) -> Result<(), String> {
		if self.is_identifier("this") {
			self.position += 1;
			if self.eat(".") {
				let name = self.identifier()?;
				self.expect("=")?;
				self.expression()?;
				let id = self.compiler.string_id(&name);
				self.emit_u16("THIS_SET", id);
				return Ok(());
			}
			self.expect("[")?;
			self.expression()?;
			self.expect("]")?;
			self.expect("=")?;
			self.expression()?;
			self.emit("THIS_SET_DYNAMIC");
			return Ok(());
		}
		let name = self.identifier()?;
		let slot = self.locals.get(&name).ok_or(format!("assignment to unknown {name}"))?;
		self.expect("=")?;
		self.expression()?;
		self.emit_u16("LOCAL_SET", slot);
		Ok(())
	}

	fn expression(&mut self) -> Result<(), String> {
		self.conditional()
	}

	fn conditional(&mut self) -> Result<(), String> {
		self.logical()?;
		if self.eat("?") {
			let skip_then = self.emit_jump("JUMP_IF_FALSE");
			self.conditional()?;
			let skip_else = self.emit_jump("JUMP");
			self.patch(skip_then);
			self.expect(":")?;
			self.conditional()?;
			self.patch(skip_else);
		}
		Ok(())
	}

	/// `||` and `??` (same level here; the themes do not mix them unparenthesized) above `&&`.
	fn logical(&mut self) -> Result<(), String> {
		self.and()?;
		loop {
			let operator = match () {
				_ if self.eat("||") => "OR",
				_ if self.eat("??") => "NULLISH",
				_ => return Ok(()),
			};
			let end = self.emit_jump(operator);
			self.and()?;
			self.patch(end);
		}
	}

	fn and(&mut self) -> Result<(), String> {
		self.equality()?;
		while self.eat("&&") {
			let end = self.emit_jump("AND");
			self.equality()?;
			self.patch(end);
		}
		Ok(())
	}

	fn equality(&mut self) -> Result<(), String> {
		self.relational()?;
		loop {
			let negate = match () {
				_ if self.eat("===") => false,
				_ if self.eat("!==") => true,
				_ => return Ok(()),
			};
			self.relational()?;
			self.emit("STRICT_EQUAL");
			if negate {
				self.emit("NOT");
			}
		}
	}

	fn relational(&mut self) -> Result<(), String> {
		self.additive()?;
		loop {
			match () {
				_ if self.eat("<") => {
					self.additive()?;
					self.emit("LESS");
				}
				_ if self.eat(">") => {
					// a > b is b < a: swap by evaluating into locals is not needed, as only
					// `<` occurs in the themes.
					return Err("`>` is not supported".into());
				}
				_ => return Ok(()),
			}
		}
	}

	fn additive(&mut self) -> Result<(), String> {
		self.multiplicative()?;
		loop {
			let operator = match () {
				_ if self.eat("+") => "ADD",
				_ if self.eat("-") => "SUBTRACT",
				_ => return Ok(()),
			};
			self.multiplicative()?;
			self.emit(operator);
		}
	}

	fn multiplicative(&mut self) -> Result<(), String> {
		self.unary()?;
		loop {
			let operator = match () {
				_ if self.eat("*") => "MULTIPLY",
				_ if self.eat("/") => "DIVIDE",
				_ if self.eat("%") => "REMAINDER",
				_ => return Ok(()),
			};
			self.unary()?;
			self.emit(operator);
		}
	}

	fn unary(&mut self) -> Result<(), String> {
		if self.eat("!") {
			self.unary()?;
			self.emit("NOT");
		} else if self.eat("-") {
			self.unary()?;
			self.emit("NEGATE");
		} else if self.eat("+") {
			self.unary()?;
		} else {
			self.postfix()?;
		}
		Ok(())
	}

	fn postfix(&mut self) -> Result<(), String> {
		self.primary()?;
		loop {
			if self.eat(".") || self.eat("?.") {
				let name = self.identifier()?;
				self.property(&name);
			} else if self.eat("[") {
				self.expression()?;
				self.expect("]")?;
				self.emit("INDEX");
			} else {
				return Ok(());
			}
		}
	}

	fn primary(&mut self) -> Result<(), String> {
		let token = self.peek().cloned().ok_or("unexpected end")?;
		self.position += 1;
		match token {
			Token::Number(number) => self.emit_number(number),
			Token::String(text) => self.emit_string(&text),
			Token::Punctuation("(") => {
				self.expression()?;
				self.expect(")")?;
			}
			Token::Punctuation("{") => {
				let mut count = 0u16;
				while !self.eat("}") {
					let key = match self.peek().cloned() {
						Some(Token::Identifier(key) | Token::String(key)) => key,
						other => return Err(format!("bad object key {other:?}")),
					};
					self.position += 1;
					self.emit_string(&key);
					self.expect(":")?;
					self.expression()?;
					count += 1;
					if !self.eat(",") {
						self.expect("}")?;
						break;
					}
				}
				self.emit_u16("OBJECT", count);
			}
			Token::Punctuation("[") => {
				let mut count = 0u16;
				while !self.eat("]") {
					self.expression()?;
					count += 1;
					if !self.eat(",") {
						self.expect("]")?;
						break;
					}
				}
				self.emit_u16("ARRAY", count);
			}
			Token::Identifier(name) => self.identifier_expression(&name)?,
			other => return Err(format!("unexpected {other:?} at token {}", self.position - 1)),
		}
		Ok(())
	}

	fn identifier_expression(&mut self, name: &str) -> Result<(), String> {
		match name {
			"true" => self.emit("TRUE"),
			"false" => self.emit("FALSE"),
			"null" => self.emit("NULL"),
			"undefined" => self.emit("UNDEFINED"),
			"this" => {
				if self.eat(".") {
					let property = self.identifier()?;
					let id = self.compiler.string_id(&property);
					self.emit_u16("THIS_GET", id);
				} else {
					self.expect("[")?;
					self.expression()?;
					self.expect("]")?;
					self.emit("THIS_GET_DYNAMIC");
				}
			}
			_ if self.is("(") => {
				let function = FUNCTIONS
					.iter()
					.position(|known| *known == name)
					.ok_or(format!("unknown function {name}"))?;
				self.position += 1;
				let mut count = 0u8;
				while !self.eat(")") {
					self.expression()?;
					count += 1;
					if !self.eat(",") {
						self.expect(")")?;
						break;
					}
				}
				self.code.push(opcode("CALL"));
				self.code.extend_from_slice(&[function as u8, count]);
			}
			_ => {
				if let Some(slot) = self.locals.get(name) {
					self.emit_u16("LOCAL_GET", slot);
				} else if let Some(value) = self.compiler.globals.get(name).cloned() {
					self.emit_string(&value);
				} else {
					return Err(format!("unknown identifier {name}"));
				}
			}
		}
		Ok(())
	}
}

fn generated_source(
	compiler: &Compiler,
	themes: &[(&str, CompiledTheme)],
	version: &str,
) -> String {
	let mut source = format!(
		"//! Mermaid's themes, generated by `cargo xtask update-mermaid-themes` from Mermaid {version}\n\
		 //! (`packages/mermaid/src/themes`). Do not edit by hand.\n\
		 //!\n\
		 //! Mermaid: <https://github.com/mermaid-js/mermaid>, MIT License, Copyright (c) 2014 - 2022\n\
		 //! Knut Sveidqvist.\n\n\
		 #![allow(dead_code)]\n\n\
		 use super::theme::{{Calculate, ThemeCode}};\n\n"
	);
	for (index, (name, description)) in OPCODES.iter().enumerate() {
		let _ = writeln!(source, "/// {description}\npub const {name}: u8 = {index};");
	}
	let _ = writeln!(
		source,
		"\n/// The functions of `CALL`.\npub const FUNCTIONS: [&str; {}] = {FUNCTIONS:?};\n",
		FUNCTIONS.len()
	);
	let _ = writeln!(source, "pub const STRINGS: [&str; {}] = [", compiler.strings.len());
	for text in &compiler.strings {
		let _ = writeln!(source, "\t{text:?},");
	}
	let _ = writeln!(source, "];\n\npub const NUMBERS: [f64; {}] = [", compiler.numbers.len());
	for number in &compiler.numbers {
		let _ = writeln!(source, "\t{number:?},");
	}
	let _ = writeln!(source, "];\n\npub const THEMES: [(&str, ThemeCode); {}] = [", themes.len());
	for (name, theme) in themes {
		let _ = writeln!(
			source,
			"\t({name:?}, ThemeCode {{ constructor: {}, update_colors: {}, calculate: Calculate::{:?}, local_count: {} }}),",
			byte_string(&theme.constructor),
			byte_string(&theme.update_colors),
			theme.calculate,
			theme.local_count,
		);
	}
	source.push_str("];\n");
	source
}

fn byte_string(bytes: &[u8]) -> String {
	let mut literal = String::from("b\"");
	for byte in bytes {
		let _ = write!(literal, "\\x{byte:02x}");
	}
	literal.push('"');
	literal
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn compiles_theme_statements() {
		let source = "import x from 'y';\nclass Theme {\n constructor() { this.a = '#fff'; this.updateColors(); }\n updateColors() {\n  const m = this.darkMode ? -4 : -1;\n  for (let i = 0; i < 2; i++) { this['c' + i] = this['c' + i] || adjust(this.a, { h: m * (5 + i) }); }\n  this.o = { x: this.o?.x || 1 };\n }\n calculate(overrides) { this.updateColors(); applyOverride(); }\n}\n";
		let mut compiler = Compiler::default();
		let theme = compiler.compile_theme(source).unwrap();
		assert_eq!(
			theme.constructor,
			[opcode("STRING"), 0, 0, opcode("THIS_SET"), 1, 0, opcode("UPDATE_COLORS")]
		);
		assert_eq!(theme.calculate, Calculate::Standard);
		assert!(compiler.strings.contains(&"darkMode".to_owned()));
		assert_eq!(theme.local_count, 2);
	}

	#[test]
	fn rejects_unknown_functions() {
		let source =
			"class Theme { constructor() { this.a = foo(1); } updateColors() {} calculate() {} }";
		assert!(Compiler::default().compile_theme(source).is_err());
	}
}
