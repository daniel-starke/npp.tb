//! Shrinks style sheets that are embedded into the plugin DLL.

/// Removes comments and whitespace that does not change the meaning of the CSS. Strings are
/// kept as they are. Whitespace is only dropped next to `{`, `}`, `;`, `,`, `>`, after `:` and
/// before `:` in declarations, because in selectors it may be a descendant combinator
/// (`a :hover`).
pub fn minify_css(css: &str) -> String {
	let mut minified = String::with_capacity(css.len());
	let mut characters = css.chars().peekable();
	let mut pending_space = false;
	// Per open `{`: whether it holds declarations (`true`) or rules, like `@media`.
	let mut blocks: Vec<bool> = Vec::new();
	while let Some(character) = characters.next() {
		match character {
			'/' if characters.peek() == Some(&'*') => {
				characters.next();
				let mut previous = '\0';
				for comment_character in characters.by_ref() {
					if previous == '*' && comment_character == '/' {
						break;
					}
					previous = comment_character;
				}
			}
			_ if character.is_whitespace() => pending_space = true,
			'"' | '\'' => {
				push_pending_space(&mut minified, &mut pending_space, character, false);
				minified.push(character);
				let mut is_escaped = false;
				for string_character in characters.by_ref() {
					minified.push(string_character);
					match string_character {
						'\\' if !is_escaped => is_escaped = true,
						_ if string_character == character && !is_escaped => break,
						_ => is_escaped = false,
					}
				}
			}
			_ => {
				let is_in_declarations = blocks.last() == Some(&true);
				push_pending_space(
					&mut minified,
					&mut pending_space,
					character,
					is_in_declarations,
				);
				match character {
					'{' => {
						let prelude_start =
							minified.rfind(['{', '}', ';']).map_or(0, |index| index + 1);
						blocks.push(!minified[prelude_start..].starts_with('@'));
					}
					'}' => {
						blocks.pop();
					}
					_ => {}
				}
				minified.push(character);
			}
		}
	}
	minified
}

fn push_pending_space(
	minified: &mut String,
	pending_space: &mut bool,
	next: char,
	is_in_declarations: bool,
) {
	let is_separator = |character: char| "{};,>".contains(character);
	if std::mem::take(pending_space)
		&& !minified.is_empty()
		&& !is_separator(next)
		&& !(is_in_declarations && next == ':')
		&& !minified.ends_with(|previous: char| is_separator(previous) || previous == ':')
	{
		minified.push(' ');
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn removes_comments_and_insignificant_whitespace() {
		let css =
			"/* header */\n.a  >  b ,\n.c :hover {\n  color : red ;\n  font: 12px \"A  B\";\n}\n";
		assert_eq!(minify_css(css), ".a>b,.c :hover{color:red;font:12px \"A  B\";}");
	}

	#[test]
	fn keeps_selector_spaces_in_at_rules() {
		assert_eq!(
			minify_css("@media (max-width: 1px) { .a :hover { b : c } }"),
			"@media (max-width:1px){.a :hover{b:c}}"
		);
	}

	#[test]
	fn keeps_strings_with_escaped_quotes() {
		assert_eq!(
			minify_css("a::before { content: '\\'  /* x */'; }"),
			"a::before{content:'\\'  /* x */';}"
		);
	}
}
