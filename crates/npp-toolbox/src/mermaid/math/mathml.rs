//! A reader for the MathML that `markdown_html::math` writes: machine generated and well-formed,
//! into an arena of elements. What the layout asks of an element's descendants again and again is
//! computed once here, as Blink's recursive queries would take exponential time on deep input.

/// Deeper input is refused; the layout recurses over the elements.
const MAX_DEPTH: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
	Math,
	/// `mrow`: a row that is transparent for embellished operators.
	Mrow,
	/// `mstyle`, `merror`: the same with other properties.
	Group,
	Phantom,
	/// `mpadded`: a group with a box of its own.
	Padded,
	/// `menclose`: a row for Blink, whose lines are drawn here.
	Enclose,
	/// Unknown elements.
	Row,
	Identifier,
	Number,
	Operator,
	Text,
	Space,
	Subscript,
	Superscript,
	SubSuperscript,
	Under,
	Over,
	UnderOver,
	Fraction,
	SquareRoot,
	Root,
	Table,
	TableRow,
	TableCell,
}

impl Tag {
	fn of(name: &str) -> Self {
		match name {
			"math" => Self::Math,
			"mrow" => Self::Mrow,
			"mstyle" | "merror" => Self::Group,
			"mphantom" => Self::Phantom,
			"mpadded" => Self::Padded,
			"menclose" => Self::Enclose,
			"mi" => Self::Identifier,
			"mn" => Self::Number,
			"mo" => Self::Operator,
			"mtext" | "ms" => Self::Text,
			"mspace" => Self::Space,
			"msub" => Self::Subscript,
			"msup" => Self::Superscript,
			"msubsup" => Self::SubSuperscript,
			"munder" => Self::Under,
			"mover" => Self::Over,
			"munderover" => Self::UnderOver,
			"mfrac" => Self::Fraction,
			"msqrt" => Self::SquareRoot,
			"mroot" => Self::Root,
			"mtable" => Self::Table,
			"mtr" => Self::TableRow,
			"mtd" => Self::TableCell,
			_ => Self::Row,
		}
	}

	pub fn is_token(self) -> bool {
		matches!(self, Self::Identifier | Self::Number | Self::Operator | Self::Text)
	}

	fn is_scripted(self) -> bool {
		matches!(
			self,
			Self::Subscript
				| Self::Superscript
				| Self::SubSuperscript
				| Self::Under
				| Self::Over | Self::UnderOver
		)
	}

	fn is_group(self) -> bool {
		matches!(self, Self::Mrow | Self::Group | Self::Phantom | Self::Padded)
	}
}

#[derive(Debug)]
pub struct Element {
	pub tag: Tag,
	attributes: Vec<(String, String)>,
	/// The text of a token.
	pub text: String,
	pub children: Vec<usize>,
	pub has_previous_sibling: bool,
	pub has_next_sibling: bool,
	/// The `mo` this element is an embellished operator of, and the child that leads to it.
	pub core_operator: Option<usize>,
	pub core_child: usize,
	/// A row around one embellished operator and spaces, which adds no operator spaces: groups
	/// and the anonymous rows inside of `msqrt` and `mtd`.
	pub is_embellished_row: bool,
	is_space_like: bool,
}

impl Element {
	pub fn attribute(&self, name: &str) -> Option<&str> {
		self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
	}

	/// A boolean attribute, ASCII case-insensitive like Blink.
	pub fn is_true(&self, name: &str) -> Option<bool> {
		let value = self.attribute(name)?;
		if value.eq_ignore_ascii_case("true") {
			return Some(true);
		}
		value.eq_ignore_ascii_case("false").then_some(false)
	}
}

/// The elements of a formula; the root is the last one.
#[derive(Debug)]
pub struct Document {
	pub elements: Vec<Element>,
}

impl Document {
	pub fn root(&self) -> usize {
		self.elements.len().saturating_sub(1)
	}

	pub fn element(&self, index: usize) -> &Element {
		&self.elements[index]
	}
}

/// `None` if the elements are nested too deeply.
pub fn parse(mathml: &str) -> Option<Document> {
	let mut elements: Vec<Element> = Vec::new();
	// The open elements, each with its children so far.
	let mut open: Vec<Element> = Vec::new();
	let mut rest = mathml;
	while let Some(start) = rest.find('<') {
		if let Some(element) = open.last_mut()
			&& element.tag.is_token()
		{
			push_unescaped(&mut element.text, &rest[..start]);
		}
		rest = &rest[start + 1..];
		let end = tag_end(rest)?;
		let tag = &rest[..end];
		rest = &rest[end + 1..];
		if tag.starts_with('/') {
			let element = open.pop()?;
			close(element, &mut elements, &mut open);
			continue;
		}
		let (tag, is_empty) = match tag.strip_suffix('/') {
			Some(tag) => (tag, true),
			None => (tag, false),
		};
		let element = start_element(tag);
		if is_empty {
			close(element, &mut elements, &mut open);
		} else {
			if open.len() >= MAX_DEPTH {
				return None;
			}
			open.push(element);
		}
	}
	open.is_empty().then_some(())?;
	(!elements.is_empty()).then_some(Document { elements })
}

/// The `>` of a tag, outside of attribute values.
fn tag_end(text: &str) -> Option<usize> {
	let mut in_value = false;
	for (index, character) in text.char_indices() {
		match character {
			'"' => in_value = !in_value,
			'>' if !in_value => return Some(index),
			_ => {}
		}
	}
	None
}

fn start_element(tag: &str) -> Element {
	let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
	let mut attributes = Vec::new();
	let mut rest = &tag[name_end..];
	while let Some(equals) = rest.find("=\"") {
		let name = rest[..equals].trim();
		let after = &rest[equals + 2..];
		let Some(end) = after.find('"') else { break };
		let mut value = String::new();
		push_unescaped(&mut value, &after[..end]);
		attributes.push((name.to_owned(), value));
		rest = &after[end + 1..];
	}
	Element {
		tag: Tag::of(&tag[..name_end]),
		attributes,
		text: String::new(),
		children: Vec::new(),
		has_previous_sibling: false,
		has_next_sibling: false,
		core_operator: None,
		core_child: 0,
		is_embellished_row: false,
		is_space_like: false,
	}
}

/// Moves the finished `element` to the arena and links it to its parent.
fn close(mut element: Element, elements: &mut Vec<Element>, open: &mut [Element]) {
	let index = elements.len();
	classify(&mut element, index, elements);
	if let Some(parent) = open.last_mut() {
		if let Some(&previous) = parent.children.last() {
			element.has_previous_sibling = true;
			elements[previous].has_next_sibling = true;
		}
		parent.children.push(index);
	}
	elements.push(element);
}

/// Blink's `IsSpaceLike` and `GetCoreOperator`.
fn classify(element: &mut Element, index: usize, elements: &[Element]) {
	match element.tag {
		Tag::Text | Tag::Space => element.is_space_like = true,
		Tag::Operator => element.core_operator = Some(index),
		Tag::Fraction => element.core_operator = first_child_core(element, elements),
		tag if tag.is_scripted() => element.core_operator = first_child_core(element, elements),
		tag if tag.is_group() || matches!(tag, Tag::SquareRoot | Tag::TableCell) => {
			let mut is_space_like = true;
			let mut core = None;
			let mut is_embellished = true;
			for (position, &child) in element.children.iter().enumerate() {
				let child = &elements[child];
				if child.is_space_like {
					continue;
				}
				is_space_like = false;
				if core.is_some() || child.core_operator.is_none() {
					is_embellished = false;
				}
				if core.is_none() {
					core = child.core_operator.map(|core| (core, position));
				}
			}
			element.is_space_like = is_space_like && tag.is_group();
			if is_embellished && let Some((core, position)) = core {
				element.is_embellished_row = true;
				if tag.is_group() {
					element.core_operator = Some(core);
					element.core_child = position;
				}
			}
		}
		_ => {}
	}
}

fn first_child_core(element: &Element, elements: &[Element]) -> Option<usize> {
	elements[*element.children.first()?].core_operator
}

fn push_unescaped(output: &mut String, text: &str) {
	let mut rest = text;
	while let Some(start) = rest.find('&') {
		output.push_str(&rest[..start]);
		let after = &rest[start..];
		let entity =
			[("&lt;", '<'), ("&gt;", '>'), ("&amp;", '&'), ("&quot;", '"'), ("&#39;", '\'')]
				.into_iter()
				.find(|(entity, _)| after.starts_with(entity));
		match entity {
			Some((entity, character)) => {
				output.push(character);
				rest = &after[entity.len()..];
			}
			None => {
				output.push('&');
				rest = &after[1..];
			}
		}
	}
	output.push_str(rest);
}

#[cfg(test)]
mod tests {
	use super::*;

	fn tags(document: &Document, index: usize) -> Vec<Tag> {
		document.element(index).children.iter().map(|&child| document.element(child).tag).collect()
	}

	#[test]
	fn reads_elements_attributes_and_text() {
		let document = parse(
			"<math display=\"block\"><mi mathvariant=\"normal\">a&lt;b</mi><mspace width=\"1em\"/><mfrac><mn>1</mn><mn>2</mn></mfrac></math>",
		)
		.unwrap();
		let root = document.root();
		assert_eq!(document.element(root).tag, Tag::Math);
		assert_eq!(document.element(root).attribute("display"), Some("block"));
		assert_eq!(tags(&document, root), [Tag::Identifier, Tag::Space, Tag::Fraction]);
		let identifier = document.element(document.element(root).children[0]);
		assert_eq!(identifier.text, "a<b");
		assert_eq!(identifier.attribute("mathvariant"), Some("normal"));
		assert!(!identifier.has_previous_sibling && identifier.has_next_sibling);
	}

	#[test]
	fn finds_embellished_operators() {
		let document = parse(
			"<math><mrow><mtext>a</mtext><msup><mo>+</mo><mn>2</mn></msup></mrow><mrow><mo>+</mo><mo>-</mo></mrow><mi>x</mi></math>",
		)
		.unwrap();
		let root = document.element(document.root());
		let embellished = document.element(root.children[0]);
		assert!(embellished.core_operator.is_some());
		assert_eq!(embellished.core_child, 1);
		assert!(document.element(root.children[1]).core_operator.is_none());
		assert!(document.element(root.children[2]).core_operator.is_none());
		assert!(root.core_operator.is_none());
	}

	#[test]
	fn refuses_broken_and_too_deep_input() {
		assert!(parse("").is_none());
		assert!(parse("<math><mi>x</mi>").is_none());
		assert!(parse("</math>").is_none());
		let deep = "<mrow>".repeat(MAX_DEPTH + 1) + &"</mrow>".repeat(MAX_DEPTH + 1);
		assert!(parse(&deep).is_none());
		let deep = "<mrow>".repeat(MAX_DEPTH - 1) + &"</mrow>".repeat(MAX_DEPTH - 1);
		assert!(parse(&deep).is_some());
	}
}
