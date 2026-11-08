//! Mermaid's preprocessing (`preprocess.ts`): line endings, front matter, `%%{…}%%` directives
//! and `%%` comments, and the detection of the diagram type.
//!
//! Unlike Mermaid, comments and directives become empty lines, so that error messages can name
//! the line in the code block.

use super::value::{Value, parse_json, parse_yaml};

pub struct Preprocessed {
	/// The diagram code without front matter, directives and comments.
	pub code: String,
	/// Lines of the code block before `code`.
	pub line_offset: usize,
	/// `title` of the front matter.
	pub title: Option<String>,
	/// Front matter `config` merged with the `init` directives.
	pub config: Value,
}

/// Configuration keys documents may not set, Mermaid's default `secure` list.
const SECURE_KEYS: [&str; 6] =
	["secure", "securityLevel", "startOnLoad", "maxTextSize", "suppressErrorRendering", "maxEdges"];

pub fn preprocess(source: &str) -> Preprocessed {
	let text = quote_html_attributes(&source.replace("\r\n", "\n").replace('\r', "\n"));
	let (text, line_offset, front_matter) = match match_front_matter(&text) {
		Some(front_matter) => {
			let line_offset = text[..front_matter.length].matches('\n').count();
			(text[front_matter.length..].to_owned(), line_offset, Some(front_matter))
		}
		None => (text.clone(), 0, None),
	};
	let mut config = Value::Object(Vec::new());
	let mut title = None;
	if let Some(front_matter) = front_matter
		&& let Some(Value::Object(metadata)) = parse_yaml(&front_matter.body)
	{
		let metadata = Value::Object(metadata);
		title = metadata.get("title").and_then(Value::to_text).filter(|title| !title.is_empty());
		if let Some(front_config) =
			metadata.get("config").filter(|config| matches!(config, Value::Object(_)))
		{
			config.merge(front_config);
		}
		if let Some(display_mode) = metadata.get("displayMode").and_then(Value::to_text) {
			let gantt =
				Value::Object(vec![("displayMode".to_owned(), Value::String(display_mode))]);
			config.merge(&Value::Object(vec![("gantt".to_owned(), gantt)]));
		}
	}
	let (code, directives) = extract_directives(&text);
	let mut directive_config = Value::Object(Vec::new());
	let mut has_wrap = false;
	for directive in &directives {
		match directive.kind.as_str() {
			"init" | "initialize" => {
				if let Some(arguments) = &directive.arguments {
					directive_config.merge(arguments);
				}
			}
			"wrap" => has_wrap = true,
			_ => {}
		}
	}
	if has_wrap {
		directive_config.merge(&Value::Object(vec![("wrap".to_owned(), Value::Bool(true))]));
	}
	let code = blank_comment_lines(&code);
	// A `config` key of a directive applies to the detected diagram type.
	if let Some(diagram_config) = directive_config.remove("config")
		&& let Some(kind) = super::DiagramKind::detect(&code)
	{
		let key = kind.config_key().to_owned();
		directive_config.merge(&Value::Object(vec![(key, diagram_config)]));
	}
	config.merge(&directive_config);
	sanitize(&mut config, 0);
	Preprocessed { code, line_offset, title, config }
}

/// Mermaid's `cleanupText`: `"` around attribute values of HTML tags become `'`.
fn quote_html_attributes(text: &str) -> String {
	let mut output = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(start) = rest.find('<') {
		output.push_str(&rest[..start]);
		let after = &rest[start + 1..];
		let name_length =
			after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
		let Some(end) = after[name_length..].find('>').map(|end| end + name_length) else {
			output.push_str(&rest[start..]);
			return output;
		};
		if name_length == 0 {
			output.push('<');
			rest = after;
			continue;
		}
		output.push('<');
		output.push_str(&after[..name_length]);
		output.push_str(&requote_attributes(&after[name_length..end]));
		output.push('>');
		rest = &after[end + 1..];
	}
	output.push_str(rest);
	output
}

/// `="value"` to `='value'`.
fn requote_attributes(attributes: &str) -> String {
	let mut output = String::with_capacity(attributes.len());
	let mut rest = attributes;
	while let Some(start) = rest.find("=\"") {
		let value_start = start + 2;
		let Some(length) = rest[value_start..].find('"') else { break };
		output.push_str(&rest[..start]);
		output.push_str("='");
		output.push_str(&rest[value_start..value_start + length]);
		output.push('\'');
		rest = &rest[value_start + length + 1..];
	}
	output.push_str(rest);
	output
}

pub struct FrontMatter {
	body: String,
	/// Bytes including the closing fence and the following line breaks.
	length: usize,
}

/// Mermaid's `matchFrontMatter`.
pub fn match_front_matter(text: &str) -> Option<FrontMatter> {
	let bytes = text.as_bytes();
	let is_space = |byte: u8| byte.is_ascii_whitespace() || byte == 0x0b;
	let is_line_break = |byte: u8| byte == b'\n' || byte == b'\r';
	let mut cursor = 0;
	while cursor < bytes.len() && is_space(bytes[cursor]) && !is_line_break(bytes[cursor]) {
		cursor += 1;
	}
	let indent = &text[..cursor];
	if !text[cursor..].starts_with("---") {
		return None;
	}
	let mut opening_ends = Vec::new();
	let mut index = cursor + 3;
	while index < bytes.len() && is_space(bytes[index]) {
		if is_line_break(bytes[index]) {
			opening_ends.push(index);
		}
		index += 1;
	}
	let first_opening_end = *opening_ends.first()?;
	let closing_fence = format!("{indent}---");
	let closing_end = |index: usize| -> Option<usize> {
		if !is_line_break(bytes[index]) || !text[index + 1..].starts_with(&closing_fence) {
			return None;
		}
		// The last line break in the whitespace after the fence.
		let mut last = None;
		let mut position = index + 1 + closing_fence.len();
		while position < bytes.len() && is_space(bytes[position]) {
			if is_line_break(bytes[position]) {
				last = Some(position);
			}
			position += 1;
		}
		last
	};
	let last_closing_fence =
		(first_opening_end + 1..bytes.len()).rev().find(|&index| closing_end(index).is_some())?;
	let opening_end = opening_ends
		.iter()
		.copied()
		.rfind(|&candidate| candidate < last_closing_fence)
		.unwrap_or(first_opening_end);
	let body_start = opening_end + 1;
	(body_start..=last_closing_fence).find_map(|index| {
		let end = closing_end(index)?;
		let body = text.get(body_start..index)?;
		let body = match indent.is_empty() {
			true => body.to_owned(),
			false => body
				.split('\n')
				.map(|line| line.strip_prefix(indent).unwrap_or(line))
				.collect::<Vec<_>>()
				.join("\n"),
		};
		Some(FrontMatter { body, length: end + 1 })
	})
}

struct Directive {
	kind: String,
	arguments: Option<Value>,
}

/// Removes the `%%{type: arguments}%%` directives, keeping their line breaks, and parses them like
/// Mermaid's `detectDirective` (single quotes become double quotes, then JSON).
fn extract_directives(text: &str) -> (String, Vec<Directive>) {
	let mut code = String::with_capacity(text.len());
	let mut directives = Vec::new();
	let mut rest = text;
	while let Some(start) = rest.find("%%{") {
		code.push_str(&rest[..start]);
		let body_start = start + 3;
		let (body, consumed) = match rest[body_start..].find("}%%") {
			Some(length) => (&rest[body_start..body_start + length], body_start + length + 3),
			// Mermaid's regular expression also matches an unclosed directive to the end.
			None => (&rest[body_start..], rest.len()),
		};
		code.extend(rest[start..consumed].chars().filter(|&character| character == '\n'));
		if let Some(directive) = parse_directive(body) {
			directives.push(directive);
		}
		rest = &rest[consumed..];
	}
	code.push_str(rest);
	(code, directives)
}

fn parse_directive(body: &str) -> Option<Directive> {
	let body = body.trim_start();
	let name_length = body.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(body.len());
	if name_length == 0 {
		return None;
	}
	let kind = body[..name_length].to_owned();
	let after = body[name_length..].trim_start();
	let arguments = match after.strip_prefix(':') {
		Some(arguments) => {
			let arguments = arguments.trim().replace('\'', "\"");
			let first_word = arguments.split_whitespace().next().unwrap_or("");
			match first_word.chars().all(|c| c.is_alphanumeric() || c == '_')
				&& !first_word.is_empty()
			{
				// `%%{wrap: word}%%` has a word argument, not JSON.
				true => Some(Value::String(first_word.to_owned())),
				// Invalid JSON makes Mermaid ignore the directive.
				false => Some(parse_json(&arguments)?),
			}
		}
		None => None,
	};
	Some(Directive { kind: kind.to_ascii_lowercase(), arguments })
}

/// Mermaid's `cleanupComments` (`^\s*%%(?!{)[^\n]+\n?`), keeping the lines empty.
fn blank_comment_lines(code: &str) -> String {
	let mut output = String::with_capacity(code.len());
	for (index, line) in code.split('\n').enumerate() {
		if index > 0 {
			output.push('\n');
		}
		let trimmed = line.trim_start();
		let is_comment =
			trimmed.starts_with("%%") && !trimmed.starts_with("%%{") && trimmed.len() > 2;
		if !is_comment {
			output.push_str(line);
		}
	}
	output
}

/// Removes keys documents may not set and values that could inject markup, like Mermaid's
/// `sanitizeDirective`.
fn sanitize(value: &mut Value, depth: usize) {
	let Value::Object(entries) = value else { return };
	entries.retain(|(key, value)| {
		let is_unsafe_key =
			key.starts_with("__") || key.starts_with("proto") || key.starts_with("constr");
		let is_unsafe_value = matches!(value, Value::String(text)
			if text.contains('<') || text.contains('>') || text.contains("url(data:"));
		let is_secure_key = depth == 0 && SECURE_KEYS.contains(&key.as_str());
		!(is_unsafe_key || is_unsafe_value || is_secure_key)
	});
	for (key, value) in entries.iter_mut() {
		// `themeCSS` would be checked for balanced braces by Mermaid; it is not used here.
		if key == "themeCSS" {
			*value = Value::Null;
		}
		sanitize(value, depth + 1);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn extracts_front_matter_and_directives() {
		let source = "---\ntitle: Pets\nconfig:\n  theme: forest\n  pie:\n    textPosition: 0.5\n---\n%%{init: {'theme': 'dark', 'themeVariables': {'pie1': '#ff0000'}}}%%\n%% a comment\npie\n  \"Dogs\" : 3\n";
		let result = preprocess(source);
		assert_eq!(result.title.as_deref(), Some("Pets"));
		assert_eq!(result.line_offset, 7);
		assert_eq!(result.config.path(&["theme"]).and_then(Value::as_str), Some("dark"));
		assert_eq!(
			result.config.path(&["themeVariables", "pie1"]).and_then(Value::as_str),
			Some("#ff0000")
		);
		assert_eq!(result.config.path(&["pie", "textPosition"]), Some(&Value::Number(0.5)));
		assert_eq!(result.code, "\n\npie\n  \"Dogs\" : 3\n");
	}

	#[test]
	fn moves_directive_config_to_the_diagram() {
		let result =
			preprocess("%%{init: {\"config\": {\"curve\": \"linear\"}}}%%\ngraph TD\nA-->B");
		assert_eq!(
			result.config.path(&["flowchart", "curve"]).and_then(Value::as_str),
			Some("linear")
		);
	}

	#[test]
	fn drops_secure_and_unsafe_keys() {
		let result = preprocess(
			"%%{init: {\"maxTextSize\": 1, \"__proto__\": {}, \"fontFamily\": \"<script>\", \"theme\": \"dark\"}}%%\npie",
		);
		assert_eq!(
			result.config,
			Value::Object(vec![("theme".to_owned(), Value::String("dark".to_owned()))])
		);
	}

	#[test]
	fn ignores_invalid_directives() {
		let result = preprocess("%%{init: {theme: dark}}%%\n%%{wrap}%%\npie");
		assert_eq!(result.config.get("theme"), None);
		assert_eq!(result.config.get("wrap"), Some(&Value::Bool(true)));
		assert_eq!(preprocess("%%{init: ").code, "");
	}

	#[test]
	fn requotes_html_attributes() {
		assert_eq!(
			quote_html_attributes(r#"A["<b class="x">bold</b>"] --> B<1"#),
			r#"A["<b class='x'>bold</b>"] --> B<1"#
		);
	}

	#[test]
	fn matches_front_matter_like_mermaid() {
		assert!(match_front_matter("---\n---\n").is_none());
		assert_eq!(
			match_front_matter("---\na: 1\n---\nx").map(|f| (f.body, f.length)),
			Some(("a: 1".to_owned(), 13))
		);
		assert_eq!(
			match_front_matter("  ---\n  a: 1\n  ---\nx").map(|f| f.body),
			Some("a: 1".to_owned())
		);
		assert!(match_front_matter("---\na: 1\n").is_none());
		assert!(match_front_matter("--").is_none());
	}
}
