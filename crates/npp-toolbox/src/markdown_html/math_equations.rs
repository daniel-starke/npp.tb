//! `\label`, `\ref` and `\eqref` of a Markdown document: the numbers of its labelled formulas.
//!
//! A reference may come before its label, so it is written as a placeholder in the HTML, and
//! [`Equations::resolve_references`] fills in what the whole document has defined.

use super::escape::{push_escaped_attribute, push_escaped_text};

/// Around the index of a reference; noncharacters that no well-formed document contains.
const REFERENCE_START: char = '\u{FDD0}';
const REFERENCE_END: char = '\u{FDD1}';

const ERROR_COLOR: &str = "#d1242f";

struct Label {
	name: String,
	/// `None` for formulas that show no number: inline math and `\nonumber`.
	number: Option<String>,
}

struct Reference {
	name: String,
	has_parentheses: bool,
}

pub struct Equations {
	/// Office drops links and ids inside of formulas, and may refuse the whole formula.
	writes_links: bool,
	last_number: u32,
	labels: Vec<Label>,
	references: Vec<Reference>,
}

impl Equations {
	pub fn new(writes_links: bool) -> Self {
		Self { writes_links, last_number: 0, labels: Vec::new(), references: Vec::new() }
	}

	pub fn writes_links(&self) -> bool {
		self.writes_links
	}

	pub fn next_number(&mut self) -> String {
		self.last_number += 1;
		self.last_number.to_string()
	}

	/// Registers `\label{name}`; `false` if the name is already taken.
	pub fn add_label(&mut self, name: &str) -> bool {
		if self.labels.iter().any(|label| label.name == name) {
			return false;
		}
		self.labels.push(Label { name: name.to_owned(), number: None });
		true
	}

	pub fn set_number(&mut self, name: &str, number: &str) {
		if let Some(label) = self.labels.iter_mut().find(|label| label.name == name) {
			label.number = Some(number.to_owned());
		}
	}

	/// The placeholder of `\ref{name}` or `\eqref{name}`.
	pub fn reference(&mut self, name: &str, has_parentheses: bool) -> String {
		let index = self.references.len();
		self.references.push(Reference { name: name.to_owned(), has_parentheses });
		format!("{REFERENCE_START}{index}{REFERENCE_END}")
	}

	/// Replaces the placeholders in `html` by the numbers, as links to the formulas.
	pub fn resolve_references(&self, html: &mut String) {
		if self.references.is_empty() {
			return;
		}
		let mut resolved = String::with_capacity(html.len());
		let mut rest = html.as_str();
		while let Some(start) = rest.find(REFERENCE_START) {
			resolved.push_str(&rest[..start]);
			let after = &rest[start + REFERENCE_START.len_utf8()..];
			let reference = after.find(REFERENCE_END).and_then(|end| {
				let reference = self.references.get(after[..end].parse::<usize>().ok()?)?;
				Some((reference, end))
			});
			match reference {
				Some((reference, end)) => {
					self.push_reference(&mut resolved, reference);
					rest = &after[end + REFERENCE_END.len_utf8()..];
				}
				None => {
					resolved.push(REFERENCE_START);
					rest = after;
				}
			}
		}
		resolved.push_str(rest);
		*html = resolved;
	}

	fn push_reference(&self, output: &mut String, reference: &Reference) {
		let Some(label) = self.labels.iter().find(|label| label.name == reference.name) else {
			output.push_str(&format!(
				"<mtext mathcolor=\"{ERROR_COLOR}\" style=\"color:{ERROR_COLOR}\">"
			));
			push_escaped_text(output, &reference.name);
			output.push_str("</mtext>");
			return;
		};
		let text = match (&label.number, reference.has_parentheses) {
			(Some(number), true) => format!("({number})"),
			(Some(number), false) => number.clone(),
			(None, _) => label.name.clone(),
		};
		output.push_str("<mtext>");
		if self.writes_links {
			output.push_str("<a href=\"#");
			push_escaped_attribute(output, &label.name);
			output.push_str("\">");
		}
		push_escaped_text(output, &text);
		if self.writes_links {
			output.push_str("</a>");
		}
		output.push_str("</mtext>");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn resolves_references_before_and_after_their_label() {
		let mut equations = Equations::new(true);
		let early = equations.reference("a", true);
		assert!(equations.add_label("a"));
		assert!(!equations.add_label("a"));
		let number = equations.next_number();
		equations.set_number("a", &number);
		let late = equations.reference("a", false);
		let mut html = format!("x{early}y{late}z");
		equations.resolve_references(&mut html);
		assert_eq!(
			html,
			"x<mtext><a href=\"#a\">(1)</a></mtext>y<mtext><a href=\"#a\">1</a></mtext>z"
		);
	}

	#[test]
	fn marks_unknown_labels_and_links_labels_without_a_number() {
		let mut equations = Equations::new(true);
		equations.add_label("inline");
		let mut html = format!(
			"{}{}",
			equations.reference("<missing>", false),
			equations.reference("inline", true)
		);
		equations.resolve_references(&mut html);
		assert!(html.contains("color:#d1242f\">&lt;missing&gt;</mtext>"), "{html}");
		assert!(html.ends_with("<mtext><a href=\"#inline\">inline</a></mtext>"), "{html}");
	}

	#[test]
	fn omits_links_for_office() {
		let mut equations = Equations::new(false);
		equations.add_label("a");
		equations.set_number("a", "7");
		let mut html = equations.reference("a", true);
		equations.resolve_references(&mut html);
		assert_eq!(html, "<mtext>(7)</mtext>");
	}

	#[test]
	fn keeps_placeholders_it_did_not_write() {
		let mut equations = Equations::new(true);
		let mut html = format!("{REFERENCE_START}9{REFERENCE_END} {REFERENCE_START}x");
		let original = html.clone();
		equations.resolve_references(&mut html);
		assert_eq!(html, original);
		equations.reference("a", false);
		equations.resolve_references(&mut html);
		assert_eq!(html, original);
	}
}
