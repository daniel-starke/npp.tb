//! GitHub Flavored Markdown to HTML that looks like GitHub.
//!
//! `pulldown-cmark` parses CommonMark with the GFM tables, task lists, strikethrough, footnotes
//! and alerts. This module adds what GitHub renders on top: extended autolinks, the tag filter,
//! heading anchors, emoji shortcodes, math (as MathML, so no JavaScript is needed), front matter
//! tables and highlighted code blocks.

mod autolink;
mod escape;
mod github_style;
mod heading_anchor;
mod math;
mod math_symbols;
mod raw_html;

use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::emoji::{TextPart, for_each_part};
use escape::{is_safe_url, push_escaped_attribute, push_escaped_text};
use github_style::{AlertKind, inline};
use heading_anchor::HeadingAnchors;

pub use github_style::TokenKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Styling {
	/// Classes for the embedded GitHub style sheet of [`html_page`].
	StyleSheet,
	/// `style` attributes on the elements, for pasting into Office and e-mail applications.
	Inline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSyntax {
	/// `![alt](source)`
	Markdown,
	/// `<img src="source">` in raw HTML.
	Html,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightedSpan {
	/// Byte range in the code.
	pub range: Range<usize>,
	pub kind: TokenKind,
}

pub struct RenderOptions<'a> {
	pub styling: Styling,
	/// Replaces an image source, e.g. with a `data:` URI. `None` keeps the source as written.
	pub replace_image_source: &'a mut dyn FnMut(ImageSyntax, &str) -> Option<String>,
	/// Colored tokens of a code block, given its language (the first word of the info string)
	/// and its code. The spans must be ordered and must not overlap.
	pub highlight_code: &'a mut dyn FnMut(&str, &str) -> Vec<HighlightedSpan>,
}

/// The HTML for `markdown`, to be placed in [`html_page`] or [`inline_styled_fragment`].
pub fn render_markdown(markdown: &str, options: &mut RenderOptions) -> String {
	let markdown = markdown.replace("\r\n", "\n");
	let events: Vec<Event> = Parser::new_ext(&markdown, parser_options()).collect();
	let mut writer = HtmlWriter::new(&events, options);
	writer.write_all();
	writer.output
}

/// A complete page with the GitHub style sheet around `body` from [`Styling::StyleSheet`].
pub fn html_page(title: &str, body: &str) -> String {
	let mut page = String::with_capacity(body.len() + github_style::STYLE_SHEET.len() + 400);
	page.push_str(concat!(
		"<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n",
		"<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>"
	));
	push_escaped_text(&mut page, title);
	page.push_str("</title>\n<style>\n");
	page.push_str(github_style::STYLE_SHEET);
	page.push_str("\n</style>\n</head>\n<body>\n<article class=\"markdown-body\">\n");
	page.push_str(body);
	page.push_str("</article>\n</body>\n</html>\n");
	page
}

/// `body` from [`Styling::Inline`] in a block with GitHub's font and colors.
pub fn inline_styled_fragment(body: &str) -> String {
	format!("<div style=\"{}\">\n{body}</div>\n", inline::BODY)
}

fn parser_options() -> Options {
	Options::ENABLE_TABLES
		| Options::ENABLE_FOOTNOTES
		| Options::ENABLE_STRIKETHROUGH
		| Options::ENABLE_TASKLISTS
		| Options::ENABLE_MATH
		| Options::ENABLE_GFM
		| Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

struct HeadingState {
	level: usize,
	plain_text: String,
}

struct ImageState {
	source: String,
	title: String,
	alt: String,
	/// Images nested in the alt text, whose alt text becomes part of this one.
	nested_images: usize,
}

struct CodeBlockState {
	language: String,
	code: String,
}

#[derive(Default)]
struct TableState {
	alignments: Vec<Alignment>,
	is_in_head: bool,
	has_body: bool,
	body_rows: usize,
	cell: usize,
}

struct FootnoteReference {
	/// Lower case, as labels match case-insensitively.
	label: String,
	number: usize,
	uses: usize,
}

struct HtmlWriter<'events, 'input, 'options, 'callbacks> {
	events: &'events [Event<'input>],
	options: &'options mut RenderOptions<'callbacks>,
	output: String,
	/// Outputs put aside while a heading or footnote definition is written on its own.
	suspended_outputs: Vec<String>,
	/// Adjacent text events, merged so that autolinks and shortcodes are found as a whole.
	pending_text: String,
	link_depth: usize,
	list_depth: usize,
	heading: Option<HeadingState>,
	image: Option<ImageState>,
	code_block: Option<CodeBlockState>,
	html_block: Option<String>,
	metadata_block: Option<String>,
	table: TableState,
	heading_anchors: HeadingAnchors,
	footnote_references: Vec<FootnoteReference>,
	footnote_definition_label: Option<String>,
	footnote_definitions: Vec<(String, String)>,
	/// The `$` closing `` $`math`$ `` starts the next text event.
	skips_dollar_sign: bool,
}

impl<'events, 'input, 'options, 'callbacks> HtmlWriter<'events, 'input, 'options, 'callbacks> {
	fn new(
		events: &'events [Event<'input>],
		options: &'options mut RenderOptions<'callbacks>,
	) -> Self {
		Self {
			events,
			options,
			output: String::new(),
			suspended_outputs: Vec::new(),
			pending_text: String::new(),
			link_depth: 0,
			list_depth: 0,
			heading: None,
			image: None,
			code_block: None,
			html_block: None,
			metadata_block: None,
			table: TableState::default(),
			heading_anchors: HeadingAnchors::default(),
			footnote_references: Vec::new(),
			footnote_definition_label: None,
			footnote_definitions: Vec::new(),
			skips_dollar_sign: false,
		}
	}

	fn write_all(&mut self) {
		let events = self.events;
		for (index, event) in events.iter().enumerate() {
			match event {
				Event::Text(text) => self.text(text),
				Event::Code(code) if self.is_dollar_math(index) => {
					self.pending_text.pop();
					self.flush_text();
					self.push_math(code, false);
					self.skips_dollar_sign = true;
				}
				_ => {
					self.flush_text();
					self.event(index);
				}
			}
		}
		self.flush_text();
		self.write_footnotes();
	}

	fn is_inline_styled(&self) -> bool {
		self.options.styling == Styling::Inline
	}

	/// Writes `<tag` with the class or the inline style. The caller closes the tag.
	fn start_tag(&mut self, tag: &str, class: &str, inline_style: &str) {
		self.output.push('<');
		self.output.push_str(tag);
		let (name, value) = match self.options.styling {
			Styling::StyleSheet => ("class", class),
			Styling::Inline => ("style", inline_style),
		};
		if !value.is_empty() {
			self.output.push(' ');
			self.output.push_str(name);
			self.output.push_str("=\"");
			self.output.push_str(value);
			self.output.push('"');
		}
	}

	fn simple_tag(&mut self, tag: &str, inline_style: &str) {
		self.start_tag(tag, "", inline_style);
		self.output.push('>');
	}

	fn push_attribute(&mut self, name: &str, value: &str) {
		self.output.push(' ');
		self.output.push_str(name);
		self.output.push_str("=\"");
		push_escaped_attribute(&mut self.output, value);
		self.output.push('"');
	}

	fn text(&mut self, text: &str) {
		let text = match self.skips_dollar_sign {
			true => {
				self.skips_dollar_sign = false;
				&text[1..]
			}
			false => text,
		};
		if let Some(code_block) = &mut self.code_block {
			code_block.code.push_str(text);
		} else if let Some(metadata) = &mut self.metadata_block {
			metadata.push_str(text);
		} else if let Some(image) = &mut self.image {
			image.alt.push_str(text);
		} else {
			if let Some(heading) = &mut self.heading {
				heading.plain_text.push_str(text);
			}
			self.pending_text.push_str(text);
		}
	}

	/// GitHub's `` $`a^2`$ `` math syntax, seen at the code span `index`.
	fn is_dollar_math(&self, index: usize) -> bool {
		self.image.is_none()
			&& self.pending_text.ends_with('$')
			&& matches!(self.events.get(index + 1), Some(Event::Text(next)) if next.starts_with('$'))
	}

	fn flush_text(&mut self) {
		if self.pending_text.is_empty() {
			return;
		}
		let text = std::mem::take(&mut self.pending_text);
		if self.link_depth > 0 {
			self.push_text_with_emoji(&text);
			return;
		}
		let mut written = 0;
		for link in autolink::find_autolinks(&text) {
			self.push_text_with_emoji(&text[written..link.range.start]);
			self.start_link(&link.href, "");
			push_escaped_text(&mut self.output, &text[link.range.clone()]);
			self.output.push_str("</a>");
			written = link.range.end;
		}
		self.push_text_with_emoji(&text[written..]);
	}

	fn push_text_with_emoji(&mut self, text: &str) {
		let is_inline_styled = self.is_inline_styled();
		for_each_part(text, |part| match part {
			TextPart::Text(text) => push_escaped_text(&mut self.output, text),
			TextPart::Emoji(emoji) if is_inline_styled => self.output.push_str(emoji),
			TextPart::Emoji(emoji) => {
				self.output.push_str("<g-emoji>");
				self.output.push_str(emoji);
				self.output.push_str("</g-emoji>");
			}
		});
	}

	fn start_link(&mut self, href: &str, title: &str) {
		self.start_tag("a", "", inline::LINK);
		if is_safe_url(href, false) {
			self.push_attribute("href", href);
		}
		if !title.is_empty() {
			self.push_attribute("title", title);
		}
		self.output.push('>');
	}

	fn suspend_output(&mut self) {
		self.suspended_outputs.push(std::mem::take(&mut self.output));
	}

	/// Ends a [`Self::suspend_output`] and returns what was written since.
	fn resume_output(&mut self) -> String {
		let suspended = self.suspended_outputs.pop().unwrap_or_default();
		std::mem::replace(&mut self.output, suspended)
	}

	fn event(&mut self, index: usize) {
		let events = self.events;
		let event = &events[index];
		if self.code_block.is_some() || self.metadata_block.is_some() {
			match event {
				Event::End(TagEnd::CodeBlock) => self.end_code_block(),
				Event::End(TagEnd::MetadataBlock(_)) => self.end_metadata_block(),
				_ => {}
			}
			return;
		}
		if let Some(image) = &mut self.image {
			match event {
				Event::Start(Tag::Image { .. }) => image.nested_images += 1,
				Event::End(TagEnd::Image) if image.nested_images > 0 => image.nested_images -= 1,
				Event::End(TagEnd::Image) => self.end_image(),
				Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
					image.alt.push_str(text);
				}
				Event::SoftBreak | Event::HardBreak => image.alt.push(' '),
				_ => {}
			}
			return;
		}
		match event {
			Event::Start(tag) => self.start(tag, index),
			Event::End(tag) => self.end(*tag),
			Event::Text(_) => {}
			Event::Code(code) => {
				if let Some(heading) = &mut self.heading {
					heading.plain_text.push_str(code);
				}
				self.start_tag("code", "", inline::INLINE_CODE);
				self.output.push('>');
				push_escaped_text(&mut self.output, code);
				self.output.push_str("</code>");
			}
			Event::InlineMath(latex) => self.push_math(latex, false),
			Event::DisplayMath(latex) => self.push_math(latex, true),
			Event::Html(html) => match &mut self.html_block {
				Some(block) => block.push_str(html),
				None => self.push_raw_html(html),
			},
			Event::InlineHtml(html) => self.push_raw_html(html),
			Event::FootnoteReference(label) => self.footnote_reference(label),
			Event::SoftBreak => self.output.push('\n'),
			Event::HardBreak => self.output.push_str("<br>\n"),
			Event::Rule => {
				self.simple_tag("hr", inline::RULE);
				self.output.push('\n');
			}
			Event::TaskListMarker(checked) => self.task_list_marker(*checked),
		}
	}

	fn start(&mut self, tag: &Tag, index: usize) {
		let is_block = matches!(
			tag,
			Tag::Heading { .. }
				| Tag::BlockQuote(_)
				| Tag::CodeBlock(_)
				| Tag::HtmlBlock
				| Tag::List(_)
				| Tag::Table(_)
				| Tag::MetadataBlock(_)
		);
		// Blocks start on their own line, also inside tight list items.
		if is_block && !self.output.is_empty() && !self.output.ends_with('\n') {
			self.output.push('\n');
		}
		match tag {
			Tag::Paragraph => self.simple_tag("p", inline::PARAGRAPH),
			Tag::Heading { level, .. } => {
				self.heading =
					Some(HeadingState { level: *level as usize, plain_text: String::new() });
				self.suspend_output();
			}
			Tag::BlockQuote(None) => {
				self.simple_tag("blockquote", inline::BLOCK_QUOTE);
				self.output.push('\n');
			}
			Tag::BlockQuote(Some(kind)) => self.start_alert(AlertKind::from(*kind)),
			Tag::CodeBlock(kind) => {
				let language = match kind {
					CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or(""),
					CodeBlockKind::Indented => "",
				};
				self.code_block =
					Some(CodeBlockState { language: language.to_owned(), code: String::new() });
			}
			Tag::HtmlBlock => self.html_block = Some(String::new()),
			Tag::List(start) => {
				self.list_depth += 1;
				let style = if self.list_depth > 1 { inline::NESTED_LIST } else { inline::LIST };
				let class = if self.has_task_items(index) { "contains-task-list" } else { "" };
				match start {
					Some(start) => {
						self.start_tag("ol", class, style);
						if *start != 1 {
							self.push_attribute("start", &start.to_string());
						}
					}
					None => self.start_tag("ul", class, style),
				}
				self.output.push_str(">\n");
			}
			Tag::Item => {
				if self.is_task_item(index) {
					self.start_tag("li", "task-list-item", inline::TASK_LIST_ITEM);
					self.output.push('>');
				} else {
					self.output.push_str("<li>");
				}
			}
			Tag::FootnoteDefinition(label) => {
				self.footnote_definition_label = Some(label.to_lowercase());
				self.suspend_output();
			}
			Tag::Table(alignments) => {
				self.table = TableState { alignments: alignments.clone(), ..TableState::default() };
				self.simple_tag("table", inline::TABLE);
				self.output.push('\n');
			}
			Tag::TableHead => {
				self.table.is_in_head = true;
				self.output.push_str("<thead>\n");
				self.simple_tag("tr", inline::TABLE_ROW);
				self.output.push('\n');
				self.table.cell = 0;
			}
			Tag::TableRow => {
				if !self.table.has_body {
					self.table.has_body = true;
					self.output.push_str("<tbody>\n");
				}
				self.table.body_rows += 1;
				let is_even = self.table.body_rows % 2 == 0;
				self.simple_tag(
					"tr",
					if is_even { inline::EVEN_TABLE_ROW } else { inline::TABLE_ROW },
				);
				self.output.push('\n');
				self.table.cell = 0;
			}
			Tag::TableCell => {
				let (tag, style) = match self.table.is_in_head {
					true => ("th", inline::TABLE_HEADER_CELL),
					false => ("td", inline::TABLE_CELL),
				};
				self.start_tag(tag, "", style);
				let alignment = match self.table.alignments.get(self.table.cell) {
					Some(Alignment::Left) => "left",
					Some(Alignment::Center) => "center",
					Some(Alignment::Right) => "right",
					Some(Alignment::None) | None => "",
				};
				if !alignment.is_empty() {
					self.push_attribute("align", alignment);
				}
				self.output.push('>');
			}
			Tag::Emphasis => self.output.push_str("<em>"),
			Tag::Strong => self.simple_tag("strong", inline::STRONG),
			Tag::Strikethrough => self.output.push_str("<del>"),
			Tag::Superscript => self.output.push_str("<sup>"),
			Tag::Subscript => self.output.push_str("<sub>"),
			Tag::Link { link_type, dest_url, title, .. } => {
				self.link_depth += 1;
				match link_type {
					LinkType::Email => self.start_link(&format!("mailto:{dest_url}"), title),
					_ => self.start_link(dest_url, title),
				}
			}
			Tag::Image { dest_url, title, .. } => {
				self.image = Some(ImageState {
					source: dest_url.to_string(),
					title: title.to_string(),
					alt: String::new(),
					nested_images: 0,
				});
			}
			Tag::MetadataBlock(_) => self.metadata_block = Some(String::new()),
			Tag::DefinitionList => self.output.push_str("<dl>\n"),
			Tag::DefinitionListTitle => self.output.push_str("<dt>"),
			Tag::DefinitionListDefinition => self.output.push_str("<dd>"),
		}
	}

	fn end(&mut self, tag: TagEnd) {
		match tag {
			TagEnd::Paragraph => self.output.push_str("</p>\n"),
			TagEnd::Heading(_) => self.end_heading(),
			TagEnd::BlockQuote(None) => self.output.push_str("</blockquote>\n"),
			TagEnd::BlockQuote(Some(_)) => self.output.push_str("</div>\n"),
			TagEnd::CodeBlock => self.end_code_block(),
			TagEnd::HtmlBlock => {
				if let Some(block) = self.html_block.take() {
					self.push_raw_html(&block);
				}
			}
			TagEnd::List(is_ordered) => {
				self.list_depth = self.list_depth.saturating_sub(1);
				self.output.push_str(if is_ordered { "</ol>\n" } else { "</ul>\n" });
			}
			TagEnd::Item => self.output.push_str("</li>\n"),
			TagEnd::FootnoteDefinition => {
				let html = self.resume_output();
				if let Some(label) = self.footnote_definition_label.take() {
					self.footnote_definitions.push((label, html));
				}
			}
			TagEnd::Table => {
				if self.table.has_body {
					self.output.push_str("</tbody>\n");
				}
				self.output.push_str("</table>\n");
			}
			TagEnd::TableHead => {
				self.table.is_in_head = false;
				self.output.push_str("</tr>\n</thead>\n");
			}
			TagEnd::TableRow => self.output.push_str("</tr>\n"),
			TagEnd::TableCell => {
				self.output.push_str(if self.table.is_in_head { "</th>\n" } else { "</td>\n" });
				self.table.cell += 1;
			}
			TagEnd::Emphasis => self.output.push_str("</em>"),
			TagEnd::Strong => self.output.push_str("</strong>"),
			TagEnd::Strikethrough => self.output.push_str("</del>"),
			TagEnd::Superscript => self.output.push_str("</sup>"),
			TagEnd::Subscript => self.output.push_str("</sub>"),
			TagEnd::Link => {
				self.link_depth = self.link_depth.saturating_sub(1);
				self.output.push_str("</a>");
			}
			TagEnd::Image => self.end_image(),
			TagEnd::MetadataBlock(_) => self.end_metadata_block(),
			TagEnd::DefinitionList => self.output.push_str("</dl>\n"),
			TagEnd::DefinitionListTitle => self.output.push_str("</dt>\n"),
			TagEnd::DefinitionListDefinition => self.output.push_str("</dd>\n"),
		}
	}

	fn end_heading(&mut self) {
		let content = self.resume_output();
		let Some(heading) = self.heading.take() else { return };
		let tag = format!("h{}", heading.level);
		let style = inline::HEADINGS[heading.level.clamp(1, 6) - 1];
		self.start_tag(&tag, "", style);
		let anchor = self.heading_anchors.anchor_for(&heading.plain_text);
		self.push_attribute("id", &anchor);
		self.output.push('>');
		self.output.push_str(&content);
		self.output.push_str("</");
		self.output.push_str(&tag);
		self.output.push_str(">\n");
	}

	fn start_alert(&mut self, kind: AlertKind) {
		let alert_style = format!("{}{}", inline::ALERT, kind.border_color());
		self.start_tag("div", kind.class_name(), &alert_style);
		self.output.push_str(">\n");
		let title_style = format!("{}{}", inline::ALERT_TITLE, kind.title_color());
		self.start_tag("p", "markdown-alert-title", &title_style);
		self.output.push('>');
		self.output.push_str("<svg class=\"octicon mr-2\" viewBox=\"0 0 16 16\" width=\"16\" height=\"16\" aria-hidden=\"true\"");
		if self.is_inline_styled() {
			self.push_attribute("fill", kind.title_color());
			self.push_attribute("style", "vertical-align:text-bottom;margin-right:8px");
		}
		self.output.push_str("><path d=\"");
		self.output.push_str(kind.icon_path());
		self.output.push_str("\"></path></svg>");
		self.output.push_str(kind.title());
		self.output.push_str("</p>\n");
	}

	fn end_code_block(&mut self) {
		let Some(CodeBlockState { language, mut code }) = self.code_block.take() else { return };
		if code.ends_with('\n') {
			code.pop();
		}
		if language.eq_ignore_ascii_case("math") {
			// In a paragraph for its margins, like `$$` math.
			self.simple_tag("p", inline::PARAGRAPH);
			self.push_math(&code, true);
			self.output.push_str("</p>\n");
			return;
		}
		let spans = match language.is_empty() {
			true => Vec::new(),
			false => (self.options.highlight_code)(&language, &code),
		};
		self.simple_tag("pre", inline::CODE_BLOCK);
		self.simple_tag("code", inline::CODE_BLOCK_CODE);
		self.push_highlighted_code(&code, &spans);
		self.output.push_str("</code></pre>\n");
	}

	fn push_highlighted_code(&mut self, code: &str, spans: &[HighlightedSpan]) {
		let mut written = 0;
		for span in spans {
			let Range { start, end } = span.range;
			let is_valid = written <= start
				&& start < end
				&& code.is_char_boundary(start)
				&& code.is_char_boundary(end);
			if !is_valid {
				continue;
			}
			push_escaped_text(&mut self.output, &code[written..start]);
			self.start_tag("span", span.kind.class_name(), span.kind.inline_style());
			self.output.push('>');
			push_escaped_text(&mut self.output, &code[start..end]);
			self.output.push_str("</span>");
			written = end;
		}
		push_escaped_text(&mut self.output, &code[written..]);
	}

	fn end_image(&mut self) {
		let Some(image) = self.image.take() else { return };
		let source = (self.options.replace_image_source)(ImageSyntax::Markdown, &image.source)
			.unwrap_or(image.source);
		self.start_tag("img", "", inline::IMAGE);
		if is_safe_url(&source, true) {
			self.push_attribute("src", &source);
		}
		self.push_attribute("alt", &image.alt);
		if !image.title.is_empty() {
			self.push_attribute("title", &image.title);
		}
		self.output.push('>');
	}

	fn push_raw_html(&mut self, html: &str) {
		let replace_image_source = &mut *self.options.replace_image_source;
		raw_html::write_raw_html(html, &mut self.output, &mut |source| {
			replace_image_source(ImageSyntax::Html, source)
		});
	}

	/// Like GitHub, front matter of simple `key: value` lines becomes a table, anything else a
	/// YAML code block.
	fn end_metadata_block(&mut self) {
		let Some(yaml) = self.metadata_block.take() else { return };
		let entries: Option<Vec<(&str, &str)>> = yaml
			.lines()
			.filter(|line| !line.trim().is_empty())
			.map(|line| {
				let is_top_level = !line.starts_with([' ', '\t', '-', '#']);
				let (key, value) = line.split_once(':').filter(|_| is_top_level)?;
				Some((key.trim(), value.trim().trim_matches(|quote| quote == '"' || quote == '\'')))
			})
			.collect();
		let Some(entries) = entries.filter(|entries| !entries.is_empty()) else {
			self.code_block = Some(CodeBlockState { language: "yaml".to_owned(), code: yaml });
			self.end_code_block();
			return;
		};
		self.simple_tag("table", inline::TABLE);
		self.output.push_str("\n<thead>\n");
		self.simple_tag("tr", inline::TABLE_ROW);
		for (key, _) in &entries {
			self.simple_tag("th", inline::TABLE_HEADER_CELL);
			push_escaped_text(&mut self.output, key);
			self.output.push_str("</th>");
		}
		self.output.push_str("</tr>\n</thead>\n<tbody>\n");
		self.simple_tag("tr", inline::TABLE_ROW);
		for (_, value) in &entries {
			self.simple_tag("td", inline::TABLE_CELL);
			push_escaped_text(&mut self.output, value);
			self.output.push_str("</td>");
		}
		self.output.push_str("</tr>\n</tbody>\n</table>\n");
	}

	fn push_math(&mut self, latex: &str, is_display: bool) {
		math::push_math(&mut self.output, latex, is_display);
	}

	fn task_list_marker(&mut self, is_checked: bool) {
		if self.is_inline_styled() {
			// Office drops form controls, so the box is written as a character.
			self.output.push_str(if is_checked { "\u{2611} " } else { "\u{2610} " });
			return;
		}
		self.output.push_str("<input type=\"checkbox\" class=\"task-list-item-checkbox\" disabled");
		if is_checked {
			self.output.push_str(" checked");
		}
		self.output.push_str("> ");
	}

	/// Whether the item starting at `index` begins with a task list marker.
	fn is_task_item(&self, index: usize) -> bool {
		match self.events.get(index + 1) {
			Some(Event::TaskListMarker(_)) => true,
			Some(Event::Start(Tag::Paragraph)) => {
				matches!(self.events.get(index + 2), Some(Event::TaskListMarker(_)))
			}
			_ => false,
		}
	}

	/// Whether the list starting at `index` has a task item of its own (not in a nested list).
	fn has_task_items(&self, index: usize) -> bool {
		let mut depth = 0usize;
		for (offset, event) in self.events[index..].iter().enumerate() {
			match event {
				Event::Start(Tag::List(_)) => depth += 1,
				Event::End(TagEnd::List(_)) => {
					depth -= 1;
					if depth == 0 {
						return false;
					}
				}
				Event::Start(Tag::Item) if depth == 1 && self.is_task_item(index + offset) => {
					return true;
				}
				_ => {}
			}
		}
		false
	}

	fn footnote_reference(&mut self, label: &str) {
		let label = label.to_lowercase();
		let position =
			self.footnote_references.iter().position(|reference| reference.label == label);
		let position = position.unwrap_or_else(|| {
			let number = self.footnote_references.len() + 1;
			self.footnote_references.push(FootnoteReference { label, number, uses: 0 });
			number - 1
		});
		let reference = &mut self.footnote_references[position];
		reference.uses += 1;
		let (number, uses) = (reference.number, reference.uses);
		let id = footnote_reference_id(number, uses);
		self.output.push_str("<sup>");
		self.start_tag("a", "", inline::LINK);
		self.push_attribute("href", &format!("#fn-{number}"));
		self.push_attribute("id", &id);
		self.output.push_str(" data-footnote-ref>");
		if self.is_inline_styled() {
			// The brackets come from the style sheet otherwise.
			self.output.push_str(&format!("[{number}]"));
		} else {
			self.output.push_str(&number.to_string());
		}
		self.output.push_str("</a></sup>");
	}

	/// GitHub lists the footnotes at the end, in the order of their first reference.
	fn write_footnotes(&mut self) {
		let references = std::mem::take(&mut self.footnote_references);
		let definitions = std::mem::take(&mut self.footnote_definitions);
		let mut items = Vec::new();
		for reference in &references {
			let definition = definitions.iter().find(|(label, _)| *label == reference.label);
			if let Some((_, content)) = definition {
				items.push((reference, content));
			}
		}
		if items.is_empty() {
			return;
		}
		self.start_tag("section", "footnotes", inline::FOOTNOTES);
		self.output.push_str(" data-footnotes>\n");
		self.simple_tag("ol", inline::FOOTNOTE_LIST);
		self.output.push('\n');
		for (reference, content) in items {
			let number = reference.number;
			self.output.push_str(&format!("<li id=\"fn-{number}\">\n"));
			let mut back_references = String::new();
			for use_number in 1..=reference.uses {
				back_references.push_str(&format!(
					" <a href=\"#{}\" class=\"data-footnote-backref\" aria-label=\"Back to reference {number}\">\u{21a9}",
					footnote_reference_id(number, use_number)
				));
				if use_number > 1 {
					back_references.push_str(&format!("<sup>{use_number}</sup>"));
				}
				back_references.push_str("</a>");
			}
			// Inside the last paragraph, like GitHub.
			let content = content.trim_end();
			match content.strip_suffix("</p>") {
				Some(paragraph) => {
					self.output.push_str(paragraph);
					self.output.push_str(&back_references);
					self.output.push_str("</p>");
				}
				None => {
					self.output.push_str(content);
					self.output.push_str(&back_references);
				}
			}
			self.output.push_str("\n</li>\n");
		}
		self.output.push_str("</ol>\n</section>\n");
	}
}

fn footnote_reference_id(number: usize, use_number: usize) -> String {
	match use_number {
		1 => format!("fnref-{number}"),
		_ => format!("fnref-{number}-{use_number}"),
	}
}

impl From<pulldown_cmark::BlockQuoteKind> for AlertKind {
	fn from(kind: pulldown_cmark::BlockQuoteKind) -> Self {
		use pulldown_cmark::BlockQuoteKind;
		match kind {
			BlockQuoteKind::Note => AlertKind::Note,
			BlockQuoteKind::Tip => AlertKind::Tip,
			BlockQuoteKind::Important => AlertKind::Important,
			BlockQuoteKind::Warning => AlertKind::Warning,
			BlockQuoteKind::Caution => AlertKind::Caution,
		}
	}
}

#[cfg(test)]
mod tests;
