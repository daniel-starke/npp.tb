//! The links of a Markdown document in the editor: the destination of the link at a position,
//! and the heading that an `#anchor` refers to.

use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};

use crate::markdown_html::heading_anchor::HeadingAnchors;
use crate::markdown_html::parser_options;

/// The destination of the innermost link or image at `offset` of `text`, or of the link
/// reference definition there. A reference link gives the destination of its definition, an
/// e-mail autolink a `mailto:` address.
pub fn destination_at(text: &str, offset: usize) -> Option<String> {
	let mut events = Parser::new_ext(text, parser_options()).into_offset_iter();
	let mut destination = None;
	for (event, range) in events.by_ref() {
		if !range.contains(&offset) {
			continue;
		}
		// Nested links and images start later, so the last one found is the innermost.
		match event {
			Event::Start(Tag::Link { link_type: LinkType::Email, dest_url, .. }) => {
				destination = Some(format!("mailto:{dest_url}"));
			}
			Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
				destination = Some(dest_url.into_string());
			}
			_ => {}
		}
	}
	destination.or_else(|| {
		events
			.reference_definitions()
			.iter()
			.find(|(_, definition)| definition.span.contains(&offset))
			.map(|(_, definition)| definition.dest.to_string())
	})
}

/// The start of the heading whose id is `anchor`, the id the heading has in Render HTML and the
/// Markdown Viewer.
pub fn heading_offset(text: &str, anchor: &str) -> Option<usize> {
	let mut anchors = HeadingAnchors::default();
	// The start and the plain text of the open heading.
	let mut heading: Option<(usize, String)> = None;
	// Like Render HTML, the description of an image is no part of the heading's text.
	let mut image_depth = 0usize;
	for (event, range) in Parser::new_ext(text, parser_options()).into_offset_iter() {
		match event {
			Event::Start(Tag::Heading { .. }) => heading = Some((range.start, String::new())),
			Event::Start(Tag::Image { .. }) => image_depth += 1,
			Event::End(TagEnd::Image) => image_depth = image_depth.saturating_sub(1),
			Event::Text(text) | Event::Code(text) if image_depth == 0 => {
				if let Some((_, plain_text)) = &mut heading {
					plain_text.push_str(&text);
				}
			}
			Event::End(TagEnd::Heading(_)) => {
				let Some((start, plain_text)) = heading.take() else { continue };
				if anchors.anchor_for(&plain_text) == anchor {
					return Some(start);
				}
			}
			_ => {}
		}
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	fn destination_at_text(text: &str, part: &str) -> Option<String> {
		let offset = text.find(part).expect("part of the text");
		destination_at(text, offset)
	}

	#[test]
	fn finds_the_destination_of_a_link() {
		let text = "See [the *docs*](https://example.com \"Title\") and <https://a.b>.\n";
		assert_eq!(destination_at_text(text, "example"), Some("https://example.com".to_owned()));
		assert_eq!(destination_at_text(text, "docs"), Some("https://example.com".to_owned()));
		assert_eq!(destination_at_text(text, "a.b"), Some("https://a.b".to_owned()));
		assert_eq!(destination_at_text(text, "See"), None);
		assert_eq!(
			destination_at_text("<me@example.com>\n", "me@"),
			Some("mailto:me@example.com".to_owned())
		);
		assert_eq!(
			destination_at_text("[a](<my file.md>)\n", "file"),
			Some("my file.md".to_owned())
		);
		assert_eq!(destination_at_text("[a](x\\(1\\).md)\n", "x"), Some("x(1).md".to_owned()));
	}

	#[test]
	fn finds_the_innermost_link() {
		let text = "[![logo](logo.png)](https://example.com)\n";
		assert_eq!(destination_at_text(text, "logo.png"), Some("logo.png".to_owned()));
		assert_eq!(destination_at_text(text, "https"), Some("https://example.com".to_owned()));
	}

	#[test]
	fn resolves_references() {
		let text = "[a][b] and [b]\n\n> [b]: <docs/b.md> \"Title\"\n[b]: second.md\n";
		assert_eq!(destination_at_text(text, "b] and"), Some("docs/b.md".to_owned()));
		assert_eq!(destination_at_text(text, "b]\n"), Some("docs/b.md".to_owned()));
		assert_eq!(destination_at_text(text, "docs/"), Some("docs/b.md".to_owned()));
		// Only the first definition of a label counts.
		assert_eq!(destination_at_text(text, "second"), None);
	}

	#[test]
	fn finds_headings_by_their_anchor() {
		let text = "# Intro\n\nText\n\n## Hello, `World`! ![icon](i.png)\n\n# Intro\n";
		assert_eq!(heading_offset(text, "intro"), Some(0));
		assert_eq!(heading_offset(text, "hello-world-"), text.find("## Hello"));
		assert_eq!(heading_offset(text, "intro-1"), text.rfind("# Intro"));
		assert_eq!(heading_offset(text, "text"), None);
		assert_eq!(heading_offset("Setext\n===\n", "setext"), Some(0));
	}
}
