//! GitHub style heading ids, so that `[link](#heading-text)` works in the rendered page.

use std::collections::BTreeMap;

#[derive(Default)]
pub struct HeadingAnchors {
	uses_by_slug: BTreeMap<String, usize>,
}

impl HeadingAnchors {
	/// The id for a heading with the plain text `text`. Repeated headings get `-1`, `-2`, ...
	pub fn anchor_for(&mut self, text: &str) -> String {
		let slug = slug(text);
		let uses = self.uses_by_slug.entry(slug.clone()).or_insert(0);
		*uses += 1;
		match *uses {
			1 => slug,
			uses => format!("{slug}-{}", uses - 1),
		}
	}
}

/// Lower case text without punctuation and with spaces replaced by hyphens, like GitHub.
fn slug(text: &str) -> String {
	text.chars()
		.filter(|&character| character.is_alphanumeric() || "_- ".contains(character))
		.flat_map(char::to_lowercase)
		.map(|character| if character == ' ' { '-' } else { character })
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn creates_github_slugs() {
		let mut anchors = HeadingAnchors::default();
		assert_eq!(anchors.anchor_for("Hello, World!"), "hello-world");
		assert_eq!(anchors.anchor_for("Größe & Ölpreis 2.0"), "größe--ölpreis-20");
		assert_eq!(anchors.anchor_for("snake_case-name 😀"), "snake_case-name-");
	}

	#[test]
	fn numbers_repeated_headings() {
		let mut anchors = HeadingAnchors::default();
		assert_eq!(anchors.anchor_for("Usage"), "usage");
		assert_eq!(anchors.anchor_for("Usage"), "usage-1");
		assert_eq!(anchors.anchor_for("usage"), "usage-2");
	}
}
