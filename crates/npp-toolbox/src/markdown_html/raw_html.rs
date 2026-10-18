//! Processing of raw HTML written in Markdown: the GFM tag filter and `<img src>` rewriting.

/// Tags the GFM "disallowed raw HTML" extension neutralizes by escaping their `<`.
const FILTERED_TAGS: [&str; 9] =
	["title", "textarea", "style", "xmp", "iframe", "noembed", "noframes", "script", "plaintext"];

/// Writes `html` to `output`, escaping filtered tags and replacing the `src` of `<img>` tags
/// with the result of `rewrite_image_source` where it returns `Some`.
pub fn write_raw_html(
	html: &str,
	output: &mut String,
	rewrite_image_source: &mut dyn FnMut(&str) -> Option<String>,
) {
	let mut written = 0;
	let mut search_from = 0;
	while let Some(offset) = html[search_from..].find('<') {
		let tag_start = search_from + offset;
		search_from = tag_start + 1;
		let after_bracket = &html[tag_start + 1..];
		let name = after_bracket.strip_prefix('/').unwrap_or(after_bracket);
		if FILTERED_TAGS.iter().any(|tag| starts_with_tag_name(name, tag)) {
			output.push_str(&html[written..tag_start]);
			output.push_str("&lt;");
			written = tag_start + 1;
		} else if starts_with_tag_name(after_bracket, "img") {
			let Some(source) = source_attribute(html, tag_start + "<img".len()) else { continue };
			let Some(new_source) =
				rewrite_image_source(&decode_entities(&html[source.value.clone()]))
			else {
				continue;
			};
			output.push_str(&html[written..source.quoted.start]);
			output.push('"');
			output.push_str(&new_source.replace('"', "&quot;"));
			output.push('"');
			written = source.quoted.end;
			search_from = written;
		}
	}
	output.push_str(&html[written..]);
}

/// What a plain text view keeps of raw HTML.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HtmlPart {
	Text(String),
	LineBreak,
	Image { source: String },
}

/// End tags (and `<br>`) after which text continues on a new line.
const LINE_BREAKING_TAGS: [&str; 17] = [
	"br",
	"p",
	"div",
	"li",
	"tr",
	"h1",
	"h2",
	"h3",
	"h4",
	"h5",
	"h6",
	"blockquote",
	"table",
	"pre",
	"details",
	"summary",
	"hr",
];

/// The text of `html` with collapsed whitespace and decoded entities, line breaks after block
/// elements and the `src` of `<img>` tags. Other tags and comments are dropped.
pub fn plain_text_parts(html: &str) -> Vec<HtmlPart> {
	let mut parts = Vec::new();
	let push_text = |text: &str, parts: &mut Vec<HtmlPart>| {
		let text = decode_entities(text);
		let mut collapsed = String::with_capacity(text.len());
		for (index, word) in text.split_ascii_whitespace().enumerate() {
			if index > 0 || text.starts_with(|character: char| character.is_ascii_whitespace()) {
				collapsed.push(' ');
			}
			collapsed.push_str(word);
		}
		if text.ends_with(|character: char| character.is_ascii_whitespace())
			&& !collapsed.is_empty()
		{
			collapsed.push(' ');
		}
		if !collapsed.is_empty() {
			parts.push(HtmlPart::Text(collapsed));
		}
	};
	let mut rest = html;
	while let Some(tag_start) = rest.find('<') {
		push_text(&rest[..tag_start], &mut parts);
		let tag = &rest[tag_start..];
		let tag_end = if tag.starts_with("<!--") {
			tag.find("-->").map(|end| end + 3)
		} else {
			tag.find('>').map(|end| end + 1)
		};
		let Some(tag_end) = tag_end else {
			push_text(tag, &mut parts);
			return parts;
		};
		let after_bracket = &tag[1..];
		let name = after_bracket.strip_prefix('/').unwrap_or(after_bracket);
		if starts_with_tag_name(after_bracket, "img") {
			if let Some(source) = source_attribute(tag, "<img".len()) {
				let source = decode_entities(&tag[source.value]);
				parts.push(HtmlPart::Image { source });
			}
		} else if LINE_BREAKING_TAGS
			.iter()
			.any(|&line_breaking| starts_with_tag_name(name, line_breaking))
			&& (after_bracket.starts_with('/')
				|| starts_with_tag_name(name, "br")
				|| starts_with_tag_name(name, "hr"))
		{
			parts.push(HtmlPart::LineBreak);
		}
		rest = &tag[tag_end..];
	}
	push_text(rest, &mut parts);
	parts
}

/// `text` starts with `name` (ASCII case-insensitive) followed by whitespace, `>`, `/` or the
/// end, like the GFM tag filter checks.
fn starts_with_tag_name(text: &str, name: &str) -> bool {
	let bytes = text.as_bytes();
	bytes.len() >= name.len()
		&& bytes[..name.len()].eq_ignore_ascii_case(name.as_bytes())
		&& bytes
			.get(name.len())
			.is_none_or(|&next| next.is_ascii_whitespace() || b">/".contains(&next))
}

struct SourceAttribute {
	/// The value including its quotes, if any.
	quoted: std::ops::Range<usize>,
	value: std::ops::Range<usize>,
}

/// Finds the `src` attribute of the tag whose attributes start at `position`.
fn source_attribute(html: &str, mut position: usize) -> Option<SourceAttribute> {
	let bytes = html.as_bytes();
	let skip_whitespace = |position: &mut usize| {
		while bytes.get(*position).is_some_and(u8::is_ascii_whitespace) {
			*position += 1;
		}
	};
	loop {
		skip_whitespace(&mut position);
		match bytes.get(position)? {
			b'>' => return None,
			b'/' => {
				position += 1;
				continue;
			}
			_ => {}
		}
		let name_start = position;
		while bytes
			.get(position)
			.is_some_and(|&byte| !byte.is_ascii_whitespace() && !b"=>/".contains(&byte))
		{
			position += 1;
		}
		let name = &html[name_start..position];
		if name.is_empty() {
			return None;
		}
		skip_whitespace(&mut position);
		if bytes.get(position) != Some(&b'=') {
			continue;
		}
		position += 1;
		skip_whitespace(&mut position);
		let quoted_start = position;
		let (value, quoted_end) = match bytes.get(position)? {
			&quote @ (b'"' | b'\'') => {
				let value_start = position + 1;
				let value_end = value_start + html[value_start..].find(char::from(quote))?;
				(value_start..value_end, value_end + 1)
			}
			_ => {
				let value_start = position;
				while bytes
					.get(position)
					.is_some_and(|&byte| !byte.is_ascii_whitespace() && byte != b'>')
				{
					position += 1;
				}
				(value_start..position, position)
			}
		};
		position = quoted_end;
		if name.eq_ignore_ascii_case("src") {
			return Some(SourceAttribute { quoted: quoted_start..quoted_end, value });
		}
	}
}

/// Decodes the character references that matter in file paths.
fn decode_entities(value: &str) -> String {
	if !value.contains('&') {
		return value.to_owned();
	}
	let mut decoded = String::with_capacity(value.len());
	let mut rest = value;
	while let Some(ampersand) = rest.find('&') {
		decoded.push_str(&rest[..ampersand]);
		rest = &rest[ampersand..];
		let reference = rest.find(';').map(|semicolon| (&rest[1..semicolon], semicolon + 1));
		let character = reference.and_then(|(name, length)| {
			let character = match name {
				"amp" => Some('&'),
				"lt" => Some('<'),
				"gt" => Some('>'),
				"quot" => Some('"'),
				"apos" => Some('\''),
				_ => name
					.strip_prefix("#x")
					.or_else(|| name.strip_prefix("#X"))
					.and_then(|hex| u32::from_str_radix(hex, 16).ok())
					.or_else(|| name.strip_prefix('#').and_then(|decimal| decimal.parse().ok()))
					.and_then(char::from_u32),
			};
			character.map(|character| (character, length))
		});
		match character {
			Some((character, length)) => {
				decoded.push(character);
				rest = &rest[length..];
			}
			None => {
				decoded.push('&');
				rest = &rest[1..];
			}
		}
	}
	decoded.push_str(rest);
	decoded
}

#[cfg(test)]
mod tests {
	use super::*;

	fn processed(html: &str) -> String {
		let mut output = String::new();
		write_raw_html(html, &mut output, &mut |source| {
			source.ends_with(".png").then(|| format!("data:{source}"))
		});
		output
	}

	#[test]
	fn extracts_plain_text_parts() {
		use HtmlPart::{Image, LineBreak, Text};
		assert_eq!(
			plain_text_parts(
				"<p align=\"center\">\n  <img src=\"a&amp;b.png\" alt=x>\n</p>\nTom &amp;  Jerry<br/>end<!-- <b>no</b> -->"
			),
			vec![
				Image { source: "a&b.png".to_owned() },
				LineBreak,
				Text(" Tom & Jerry".to_owned()),
				LineBreak,
				Text("end".to_owned()),
			]
		);
		assert_eq!(plain_text_parts("a < b"), vec![Text("a ".to_owned()), Text("< b".to_owned())]);
		assert_eq!(
			plain_text_parts("<details><summary>More</summary>"),
			vec![Text("More".to_owned()), LineBreak]
		);
	}

	#[test]
	fn escapes_filtered_tags_only() {
		assert_eq!(
			processed("<strong> <title> <style> <em> </SCRIPT> <scripts> <xmp/> <iframe"),
			"<strong> &lt;title> &lt;style> <em> &lt;/SCRIPT> <scripts> &lt;xmp/> &lt;iframe"
		);
	}

	#[test]
	fn rewrites_image_sources() {
		assert_eq!(
			processed(r#"<p><IMG alt="x>y" width=10 SRC = 'a&amp;b.png' /></p>"#),
			r#"<p><IMG alt="x>y" width=10 SRC = "data:a&b.png" /></p>"#
		);
		assert_eq!(processed("<img src=a.png>"), r#"<img src="data:a.png">"#);
		assert_eq!(
			processed(r#"<img src="a.gif"><img src="b.png">"#),
			r#"<img src="a.gif"><img src="data:b.png">"#
		);
	}

	#[test]
	fn leaves_other_tags_and_broken_markup() {
		for html in
			[r#"<image src="a.png">"#, "<img alt=a.png>", r#"<img src="a.png"#, "<img", "a < b"]
		{
			assert_eq!(processed(html), html);
		}
	}

	#[test]
	fn decodes_character_references() {
		assert_eq!(decode_entities("a&amp;b&#32;c&#x41;&unknown;&"), "a&b cA&unknown;&");
	}
}
