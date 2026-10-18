//! GitHub Flavored Markdown to RTF for the RichEdit viewer.
//!
//! RichEdit cannot import HTML, so this is a second, simpler writer with the GitHub colors. It
//! records where the source blocks, links and heading anchors are in RichEdit's character
//! positions: RichEdit counts one character per text unit (UTF-16), paragraph end, line break,
//! tab and picture, the instruction and the result text of a hyperlink field, one per table cell
//! end and two per table row start and end (`U+FFF9 CR` and `U+FFFB CR`).

use std::ops::Range;
use std::rc::Rc;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Parser, Tag, TagEnd};

use crate::emoji::{TextPart, for_each_part};
use crate::image_embedding::EmbeddedImage;
use crate::markdown_html::autolink::find_autolinks;
use crate::markdown_html::heading_anchor::HeadingAnchors;
use crate::markdown_html::raw_html::{HtmlPart, plain_text_parts};
use crate::markdown_html::{
	AlertKind, HighlightedSpan, TokenKind, parser_options, source_lines_of_offsets,
};

pub type Rgb = [u8; 3];

/// Colors of GitHub's light and dark theme.
pub struct RtfTheme {
	pub background: Rgb,
	pub text: Rgb,
	pub muted: Rgb,
	pub link: Rgb,
	pub border: Rgb,
	pub code_background: Rgb,
	pub table_header_background: Rgb,
	/// Note, tip, important, warning, caution.
	pub alerts: [Rgb; 5],
	/// Comment, keyword, entity, string, constant, variable, inserted, deleted.
	pub tokens: [Rgb; 8],
}

pub const LIGHT_THEME: RtfTheme = RtfTheme {
	background: [0xff, 0xff, 0xff],
	text: [0x1f, 0x23, 0x28],
	muted: [0x59, 0x63, 0x6e],
	link: [0x09, 0x69, 0xda],
	border: [0xd1, 0xd9, 0xe0],
	code_background: [0xef, 0xf1, 0xf3],
	table_header_background: [0xf6, 0xf8, 0xfa],
	alerts: [
		[0x09, 0x69, 0xda],
		[0x1a, 0x7f, 0x37],
		[0x82, 0x50, 0xdf],
		[0x9a, 0x67, 0x00],
		[0xd1, 0x24, 0x2f],
	],
	tokens: [
		[0x59, 0x63, 0x6e],
		[0xcf, 0x22, 0x2e],
		[0x66, 0x39, 0xba],
		[0x0a, 0x30, 0x69],
		[0x05, 0x50, 0xae],
		[0x95, 0x38, 0x00],
		[0x11, 0x63, 0x29],
		[0x82, 0x07, 0x1e],
	],
};

pub const DARK_THEME: RtfTheme = RtfTheme {
	background: [0x0d, 0x11, 0x17],
	text: [0xf0, 0xf6, 0xfc],
	muted: [0x91, 0x98, 0xa1],
	link: [0x44, 0x93, 0xf8],
	border: [0x3d, 0x44, 0x4d],
	code_background: [0x26, 0x2c, 0x36],
	table_header_background: [0x15, 0x1b, 0x23],
	alerts: [
		[0x44, 0x93, 0xf8],
		[0x3f, 0xb9, 0x50],
		[0xab, 0x7d, 0xf8],
		[0xd2, 0x99, 0x22],
		[0xf8, 0x51, 0x49],
	],
	tokens: [
		[0x91, 0x98, 0xa1],
		[0xff, 0x7b, 0x72],
		[0xd2, 0xa8, 0xff],
		[0xa5, 0xd6, 0xff],
		[0x79, 0xc0, 0xff],
		[0xff, 0xa6, 0x57],
		[0xaf, 0xf5, 0xb4],
		[0xff, 0xdc, 0xd7],
	],
};

impl RtfTheme {
	fn alert_color(&self, kind: AlertKind) -> Rgb {
		self.alerts[match kind {
			AlertKind::Note => 0,
			AlertKind::Tip => 1,
			AlertKind::Important => 2,
			AlertKind::Warning => 3,
			AlertKind::Caution => 4,
		}]
	}

	fn token_color(&self, kind: TokenKind) -> Rgb {
		self.tokens[match kind {
			TokenKind::Comment => 0,
			TokenKind::Keyword => 1,
			TokenKind::Type | TokenKind::Function | TokenKind::Attribute => 2,
			TokenKind::String => 3,
			TokenKind::Number | TokenKind::Tag | TokenKind::DiffHeader | TokenKind::DiffRange => 4,
			TokenKind::Variable => 5,
			TokenKind::DiffInserted => 6,
			TokenKind::DiffDeleted => 7,
		}]
	}
}

pub struct RtfOptions<'a> {
	pub theme: &'a RtfTheme,
	/// Width of the view in twips, for the widths of tables and images.
	pub page_width: u32,
	/// DPI of the view, for the size in pixels of rendered icons.
	pub pixels_per_inch: u32,
	/// The local image `source` refers to.
	pub load_image: &'a mut dyn FnMut(&str) -> Option<Rc<EmbeddedImage>>,
	/// Colored tokens of a code block, see [`crate::markdown_html::RenderOptions`].
	pub highlight_code: &'a mut dyn FnMut(&str, &str) -> Vec<HighlightedSpan>,
}

/// A block starting at a source line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceBlock {
	/// Zero based.
	pub line: usize,
	/// RichEdit character position.
	pub position: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtfLink {
	/// RichEdit character positions of the link text.
	pub range: Range<usize>,
	/// RichEdit character positions of the destination in the hidden field instruction, which
	/// RichEdit reports for a clicked link.
	pub target_range: Range<usize>,
	/// The destination as written in the Markdown.
	pub target: String,
}

#[derive(Debug, Default)]
pub struct RtfDocument {
	pub rtf: String,
	/// In document order.
	pub blocks: Vec<SourceBlock>,
	pub links: Vec<RtfLink>,
	/// Heading ids (like the HTML `id`) and their positions.
	pub anchors: Vec<(String, usize)>,
}

pub fn render_rtf(markdown: &str, options: &mut RtfOptions) -> RtfDocument {
	let markdown = markdown.replace("\r\n", "\n");
	let (events, offsets): (Vec<Event>, Vec<usize>) = Parser::new_ext(&markdown, parser_options())
		.into_offset_iter()
		.map(|(event, range)| (event, range.start))
		.unzip();
	let lines = source_lines_of_offsets(&markdown, &offsets);
	let mut writer = RtfWriter::new(&events, &lines, options);
	writer.write_all();
	writer.finish()
}

const BASE_FONT_SIZE: u32 = 22;
const CODE_FONT_SIZE: u32 = 19;
const HEADING_FONT_SIZES: [u32; 6] = [40, 32, 27, 22, 20, 19];
/// Indentation per block quote or list level.
const INDENT: u32 = 360;
const PARAGRAPH_SPACING: u32 = 160;
const LIST_ITEM_SPACING: u32 = 40;
/// RichEdit lays out pictures at 96 DPI.
const TWIPS_PER_PIXEL: u32 = 15;
/// Alert icons are 16 pixels at 96 DPI, like on GitHub.
const ICON_SIZE: u32 = 16;
const PARAGRAPH_END: &str = "\\par\n";
const HIMETRIC_PER_INCH: u64 = 2540;
/// Tables, rules and pictures are at least this wide, in twips, also in a narrow view.
const MINIMUM_WIDTH: u32 = 1440;

enum Container {
	Quote,
	Alert,
	List {
		next_number: Option<u64>,
	},
	/// A list item, footnote or definition with a hanging label for its first paragraph.
	Item {
		label: Option<String>,
	},
}

struct TableLayout {
	alignments: Vec<Alignment>,
	cell_right_edges: Vec<u32>,
	left: u32,
	is_in_head: bool,
	/// The column of the current cell.
	column: usize,
}

struct ImageState {
	source: String,
	alt: String,
	nested_images: usize,
}

/// The space before the open paragraph, while it is on its first line.
struct SpaceBefore {
	/// Offset of the paragraph in the RTF body.
	paragraph_start: usize,
	/// Offsets of its `\sb` control word.
	control_word: Range<usize>,
	twips: u32,
}

struct RtfWriter<'events, 'input, 'options, 'callbacks> {
	events: &'events [Event<'input>],
	lines: &'events [usize],
	options: &'options mut RtfOptions<'callbacks>,
	body: String,
	/// RichEdit character position at the end of `body`.
	position: usize,
	colors: Vec<Rgb>,
	document: RtfDocument,
	containers: Vec<Container>,
	is_paragraph_open: bool,
	is_first_paragraph: bool,
	space_before: Option<SpaceBefore>,
	link_depth: usize,
	/// Result start positions, target ranges and targets of the open link fields.
	open_links: Vec<(usize, Range<usize>, String)>,
	heading: Option<(usize, String)>,
	code_block: Option<(usize, String, String)>,
	html_block: Option<(usize, String)>,
	metadata_block: Option<(usize, String)>,
	image: Option<ImageState>,
	table: Option<TableLayout>,
	heading_anchors: HeadingAnchors,
	/// Footnote labels (lower case) in the order of their first reference.
	footnote_labels: Vec<String>,
}

impl<'events, 'input, 'options, 'callbacks> RtfWriter<'events, 'input, 'options, 'callbacks> {
	fn new(
		events: &'events [Event<'input>],
		lines: &'events [usize],
		options: &'options mut RtfOptions<'callbacks>,
	) -> Self {
		let mut footnote_labels: Vec<String> = Vec::new();
		for event in events {
			if let Event::FootnoteReference(label) = event {
				let label = label.to_lowercase();
				if !footnote_labels.contains(&label) {
					footnote_labels.push(label);
				}
			}
		}
		Self {
			events,
			lines,
			options,
			body: String::new(),
			position: 0,
			colors: Vec::new(),
			document: RtfDocument::default(),
			containers: Vec::new(),
			is_paragraph_open: false,
			is_first_paragraph: true,
			space_before: None,
			link_depth: 0,
			open_links: Vec::new(),
			heading: None,
			code_block: None,
			html_block: None,
			metadata_block: None,
			image: None,
			table: None,
			heading_anchors: HeadingAnchors::default(),
			footnote_labels,
		}
	}

	fn finish(mut self) -> RtfDocument {
		let mut rtf = String::with_capacity(self.body.len() + 512);
		rtf.push_str(
			"{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1{\\fonttbl{\\f0\\fswiss Segoe UI;}{\\f1\\fmodern Consolas;}}\n{\\colortbl;",
		);
		for [red, green, blue] in &self.colors {
			rtf.push_str(&format!("\\red{red}\\green{green}\\blue{blue};"));
		}
		rtf.push_str("}\n");
		rtf.push_str(&self.body);
		rtf.push('}');
		self.document.rtf = rtf;
		self.document
	}

	/// The color table index of `color`.
	fn color(&mut self, color: Rgb) -> usize {
		match self.colors.iter().position(|&known| known == color) {
			Some(index) => index + 1,
			None => {
				self.colors.push(color);
				self.colors.len()
			}
		}
	}

	fn write_all(&mut self) {
		for index in 0..self.events.len() {
			self.event(index);
		}
		self.end_paragraph();
	}

	fn event(&mut self, index: usize) {
		let events = self.events;
		let event = &events[index];
		if let Some((_, _, code)) = &mut self.code_block {
			match event {
				Event::Text(text) => code.push_str(text),
				Event::End(TagEnd::CodeBlock) => self.end_code_block(),
				_ => {}
			}
			return;
		}
		if let Some((_, yaml)) = &mut self.metadata_block {
			match event {
				Event::Text(text) => yaml.push_str(text),
				Event::End(TagEnd::MetadataBlock(_)) => {
					if let Some((start_index, yaml)) = self.metadata_block.take() {
						self.code_block = Some((start_index, "yaml".to_owned(), yaml));
						self.end_code_block();
					}
				}
				_ => {}
			}
			return;
		}
		if let Some(image) = &mut self.image {
			match event {
				Event::Start(Tag::Image { .. }) => image.nested_images += 1,
				Event::End(TagEnd::Image) if image.nested_images > 0 => image.nested_images -= 1,
				Event::End(TagEnd::Image) => self.end_image(index),
				Event::Text(text)
				| Event::Code(text)
				| Event::InlineMath(text)
				| Event::DisplayMath(text) => image.alt.push_str(text),
				Event::SoftBreak | Event::HardBreak => image.alt.push(' '),
				_ => {}
			}
			return;
		}
		if let Some((_, html)) = &mut self.html_block {
			match event {
				Event::Html(text) | Event::Text(text) => html.push_str(text),
				Event::End(TagEnd::HtmlBlock) => self.end_html_block(),
				_ => {}
			}
			return;
		}
		match event {
			Event::Start(tag) => self.start(tag, index),
			Event::End(tag) => self.end(*tag),
			Event::Text(text) => {
				if let Some((_, plain_text)) = &mut self.heading {
					plain_text.push_str(text);
				}
				self.inline_text(index, text);
			}
			Event::Code(code) => {
				if let Some((_, plain_text)) = &mut self.heading {
					plain_text.push_str(code);
				}
				self.inline_code(index, code);
			}
			Event::InlineMath(latex) | Event::DisplayMath(latex) => self.inline_code(index, latex),
			Event::Html(html) | Event::InlineHtml(html) => self.inline_html(index, html),
			Event::FootnoteReference(label) => {
				let number = self.footnote_number(label);
				self.ensure_paragraph(index);
				self.body.push_str("{\\super ");
				self.push_text(&format!("[{number}]"));
				self.body.push('}');
			}
			Event::SoftBreak => {
				self.ensure_paragraph(index);
				self.push_text(" ");
			}
			Event::HardBreak => self.line_break(index),
			Event::Rule => {
				self.end_paragraph();
				self.rule(index);
			}
			Event::TaskListMarker(is_checked) => {
				self.ensure_paragraph(index);
				self.push_text(if *is_checked { "\u{2611} " } else { "\u{2610} " });
			}
		}
	}

	fn start(&mut self, tag: &Tag, index: usize) {
		match tag {
			Tag::Paragraph => {
				self.end_paragraph();
				self.begin_paragraph(index, "");
			}
			Tag::Heading { level, .. } => {
				self.end_paragraph();
				let level = (*level as usize).clamp(1, 6);
				let size = HEADING_FONT_SIZES[level - 1];
				let mut format = format!("\\sb{}\\sa80\\b\\fs{size}", PARAGRAPH_SPACING * 2);
				if level <= 2 {
					let border = self.color(self.options.theme.border);
					format.push_str(&format!("\\brdrb\\brdrs\\brdrw10\\brsp40\\brdrcf{border}"));
				}
				if level == 6 {
					let muted = self.color(self.options.theme.muted);
					format.push_str(&format!("\\cf{muted}"));
				}
				self.begin_paragraph(index, &format);
				self.heading = Some((self.position, String::new()));
			}
			Tag::BlockQuote(None) => {
				self.end_paragraph();
				self.containers.push(Container::Quote);
			}
			Tag::BlockQuote(Some(kind)) => {
				self.end_paragraph();
				let kind = AlertKind::from(*kind);
				let color = self.options.theme.alert_color(kind);
				self.containers.push(Container::Alert);
				let color_index = self.color(color);
				self.begin_paragraph(index, &format!("\\b\\cf{color_index}"));
				self.alert_icon(kind, color);
				self.push_text(kind.title());
				self.end_paragraph();
			}
			Tag::CodeBlock(kind) => {
				self.end_paragraph();
				let language = match kind {
					CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or(""),
					CodeBlockKind::Indented => "",
				};
				self.code_block = Some((index, language.to_owned(), String::new()));
			}
			Tag::HtmlBlock => {
				self.end_paragraph();
				self.html_block = Some((index, String::new()));
			}
			Tag::List(start) => {
				self.end_paragraph();
				self.containers.push(Container::List { next_number: *start });
			}
			Tag::Item => {
				self.end_paragraph();
				let is_task_item =
					matches!(self.events.get(index + 1), Some(Event::TaskListMarker(_)))
						|| matches!(
							(self.events.get(index + 1), self.events.get(index + 2)),
							(Some(Event::Start(Tag::Paragraph)), Some(Event::TaskListMarker(_)))
						);
				let depth = self.list_depth();
				let label = match self.containers.last_mut() {
					Some(Container::List { next_number: Some(number) }) => {
						*number += 1;
						format!("{}.", *number - 1)
					}
					_ if is_task_item => String::new(),
					_ => {
						["\u{2022}", "\u{25e6}", "\u{25aa}"][depth.saturating_sub(1) % 3].to_owned()
					}
				};
				self.containers.push(Container::Item { label: Some(label) });
			}
			Tag::FootnoteDefinition(label) => {
				self.end_paragraph();
				let number = self.footnote_number(label);
				self.containers.push(Container::Item { label: Some(format!("[{number}]")) });
			}
			Tag::DefinitionList => self.end_paragraph(),
			Tag::DefinitionListTitle => {
				self.end_paragraph();
				self.begin_paragraph(index, "\\b");
			}
			Tag::DefinitionListDefinition => {
				self.end_paragraph();
				self.containers.push(Container::Item { label: None });
			}
			Tag::Table(alignments) => {
				self.end_paragraph();
				self.start_table(index, alignments);
			}
			Tag::TableHead => {
				if let Some(table) = &mut self.table {
					table.is_in_head = true;
				}
				self.start_table_row(index, true);
			}
			Tag::TableRow => self.start_table_row(index, false),
			Tag::TableCell => self.start_table_cell(),
			Tag::Emphasis => self.start_group(index, "\\i"),
			Tag::Strong => self.start_group(index, "\\b"),
			Tag::Strikethrough => self.start_group(index, "\\strike"),
			Tag::Superscript => self.start_group(index, "\\super"),
			Tag::Subscript => self.start_group(index, "\\sub"),
			Tag::Link { link_type, dest_url, .. } => {
				let target = match link_type {
					LinkType::Email => format!("mailto:{dest_url}"),
					_ => dest_url.to_string(),
				};
				self.start_link(index, target);
			}
			Tag::Image { dest_url, .. } => {
				self.image = Some(ImageState {
					source: dest_url.to_string(),
					alt: String::new(),
					nested_images: 0,
				});
			}
			Tag::MetadataBlock(_) => self.metadata_block = Some((index, String::new())),
		}
	}

	fn end(&mut self, tag: TagEnd) {
		match tag {
			TagEnd::Paragraph | TagEnd::DefinitionListTitle => self.end_paragraph(),
			TagEnd::Heading(_) => {
				self.end_paragraph();
				if let Some((position, plain_text)) = self.heading.take() {
					let anchor = self.heading_anchors.anchor_for(&plain_text);
					self.document.anchors.push((anchor, position));
				}
			}
			TagEnd::BlockQuote(_)
			| TagEnd::List(_)
			| TagEnd::Item
			| TagEnd::FootnoteDefinition
			| TagEnd::DefinitionListDefinition => {
				self.end_paragraph();
				self.containers.pop();
			}
			TagEnd::Table => {
				self.table = None;
			}
			TagEnd::TableHead | TagEnd::TableRow => {
				self.body.push_str("\\row\n");
				self.position += 2;
				if let Some(table) = &mut self.table {
					table.is_in_head = false;
				}
			}
			TagEnd::TableCell => {
				if self.table.as_ref().is_some_and(|table| table.is_in_head) {
					self.body.push('}');
				}
				self.body.push_str("\\cell ");
				self.position += 1;
				if let Some(table) = &mut self.table {
					table.column += 1;
				}
			}
			TagEnd::Emphasis
			| TagEnd::Strong
			| TagEnd::Strikethrough
			| TagEnd::Superscript
			| TagEnd::Subscript => self.body.push('}'),
			TagEnd::Link => self.end_link(),
			TagEnd::Image
			| TagEnd::CodeBlock
			| TagEnd::HtmlBlock
			| TagEnd::MetadataBlock(_)
			| TagEnd::DefinitionList => {}
		}
	}

	fn list_depth(&self) -> usize {
		self.containers
			.iter()
			.filter(|container| matches!(container, Container::List { .. }))
			.count()
	}

	fn footnote_number(&mut self, label: &str) -> usize {
		let label = label.to_lowercase();
		match self.footnote_labels.iter().position(|known| *known == label) {
			Some(index) => index + 1,
			None => {
				self.footnote_labels.push(label);
				self.footnote_labels.len()
			}
		}
	}

	fn left_indent(&self) -> u32 {
		self.containers
			.iter()
			.map(|container| match container {
				Container::Quote | Container::Alert | Container::Item { .. } => INDENT,
				Container::List { .. } => 0,
			})
			.sum()
	}

	/// Starts a paragraph for the event `index`. `format` adds paragraph and character properties.
	fn begin_paragraph(&mut self, index: usize, format: &str) {
		self.record_block(index);
		let text_color =
			match self.containers.iter().any(|container| matches!(container, Container::Quote)) {
				true => self.options.theme.muted,
				false => self.options.theme.text,
			};
		let text_color = self.color(text_color);
		let is_in_item = matches!(self.containers.last(), Some(Container::Item { .. }));
		let space_before = match (self.is_first_paragraph, is_in_item) {
			(true, _) => 0,
			(false, true) => LIST_ITEM_SPACING,
			(false, false) => PARAGRAPH_SPACING,
		};
		self.is_first_paragraph = false;
		let left_indent = self.left_indent();
		let paragraph_start = self.body.len();
		self.body.push_str(&format!("\\pard\\plain\\f0\\fs{BASE_FONT_SIZE}\\cf{text_color}"));
		let control_word_start = self.body.len();
		self.body.push_str(&format!("\\sb{space_before}"));
		self.space_before = (space_before > 0).then_some(SpaceBefore {
			paragraph_start,
			control_word: control_word_start..self.body.len(),
			twips: space_before,
		});
		self.body.push_str(&format!("\\li{left_indent}"));
		let label = match self.containers.last_mut() {
			Some(Container::Item { label }) => label.take(),
			_ => None,
		};
		if label.is_some() {
			self.body.push_str(&format!("\\fi-{INDENT}\\tx{left_indent}"));
		}
		self.body.push_str(format);
		self.body.push(' ');
		self.is_paragraph_open = true;
		if let Some(label) = label {
			self.push_text(&label);
			self.body.push_str("\\tab ");
			self.position += 1;
		}
	}

	fn ensure_paragraph(&mut self, index: usize) {
		if !self.is_paragraph_open && self.table.is_none() {
			self.begin_paragraph(index, "");
		}
	}

	fn end_paragraph(&mut self) {
		if self.is_paragraph_open {
			self.body.push_str(PARAGRAPH_END);
			self.position += 1;
			self.is_paragraph_open = false;
			self.space_before = None;
		}
	}

	/// RichEdit draws a picture on the first line of a paragraph as if the paragraph had no space
	/// before: over the paragraph above and with its bottom cut off. The previous paragraph takes
	/// the space after it instead.
	fn move_space_before_to_previous_paragraph(&mut self) {
		let Some(space_before) = self.space_before.take() else { return };
		let Some(previous) = self.body[..space_before.paragraph_start].strip_suffix(PARAGRAPH_END)
		else {
			// Tables and rules end with a row instead.
			return;
		};
		let previous_end = previous.len();
		self.body.replace_range(space_before.control_word, "\\sb0");
		self.body.insert_str(previous_end, &format!("\\sa{}", space_before.twips));
	}

	fn record_block(&mut self, index: usize) {
		if let Some(&line) = self.lines.get(index) {
			self.document.blocks.push(SourceBlock { line, position: self.position });
		}
	}

	fn line_break(&mut self, index: usize) {
		self.ensure_paragraph(index);
		self.space_before = None;
		self.body.push_str("\\line ");
		self.position += 1;
	}

	fn start_group(&mut self, index: usize, format: &str) {
		self.ensure_paragraph(index);
		self.body.push('{');
		self.body.push_str(format);
		self.body.push(' ');
	}

	/// Escapes `text` and counts its characters.
	fn push_text(&mut self, text: &str) {
		for character in text.chars() {
			match character {
				'\\' | '{' | '}' => {
					self.body.push('\\');
					self.body.push(character);
				}
				'\t' => self.body.push_str("\\tab "),
				'\n' | '\r' => self.body.push(' '),
				' '..='~' => self.body.push(character),
				_ => {
					let mut units = [0u16; 2];
					for &unit in character.encode_utf16(&mut units).iter() {
						self.body.push_str(&format!("\\u{}?", unit as i16));
					}
				}
			}
			self.position += character.len_utf16();
		}
	}

	fn push_text_with_emoji(&mut self, text: &str) {
		let mut parts = Vec::new();
		for_each_part(text, |part| {
			parts.push(match part {
				TextPart::Text(text) => text.to_owned(),
				TextPart::Emoji(emoji) => emoji.to_owned(),
			});
		});
		for part in parts {
			self.push_text(&part);
		}
	}

	fn inline_text(&mut self, index: usize, text: &str) {
		self.ensure_paragraph(index);
		if self.link_depth > 0 {
			self.push_text_with_emoji(text);
			return;
		}
		let mut written = 0;
		for link in find_autolinks(text) {
			self.push_text_with_emoji(&text[written..link.range.start]);
			self.start_link(index, link.href);
			self.push_text(&text[link.range.clone()]);
			self.end_link();
			written = link.range.end;
		}
		self.push_text_with_emoji(&text[written..]);
	}

	fn inline_code(&mut self, index: usize, code: &str) {
		self.ensure_paragraph(index);
		let background = self.color(self.options.theme.code_background);
		self.body.push_str(&format!("{{\\f1\\fs{CODE_FONT_SIZE}\\highlight{background} "));
		self.push_text(code);
		self.body.push('}');
	}

	fn inline_html(&mut self, index: usize, html: &str) {
		for part in plain_text_parts(html) {
			match part {
				HtmlPart::Text(text) => {
					self.ensure_paragraph(index);
					self.push_text(&text);
				}
				HtmlPart::LineBreak => {
					if self.is_paragraph_open {
						self.body.push_str("\\line ");
						self.position += 1;
					}
				}
				HtmlPart::Image { source } => self.image(index, &source, ""),
			}
		}
	}

	fn end_html_block(&mut self) {
		let Some((index, html)) = self.html_block.take() else { return };
		// Lines of text and images, trimmed at both ends, without empty lines.
		let mut lines: Vec<Vec<HtmlPart>> = vec![Vec::new()];
		for part in plain_text_parts(&html) {
			match part {
				HtmlPart::LineBreak => lines.push(Vec::new()),
				part => lines.last_mut().unwrap_or(&mut Vec::new()).push(part),
			}
		}
		for line in &mut lines {
			if let Some(HtmlPart::Text(text)) = line.first_mut() {
				*text = text.trim_start().to_owned();
			}
			if let Some(HtmlPart::Text(text)) = line.last_mut() {
				*text = text.trim_end().to_owned();
			}
			line.retain(|part| !matches!(part, HtmlPart::Text(text) if text.is_empty()));
		}
		lines.retain(|line| !line.is_empty());
		if lines.is_empty() {
			return;
		}
		self.begin_paragraph(index, "");
		for (number, line) in lines.into_iter().enumerate() {
			if number > 0 {
				self.body.push_str("\\line ");
				self.position += 1;
			}
			for part in line {
				match part {
					HtmlPart::Text(text) => self.push_text(&text),
					HtmlPart::Image { source } => self.image(index, &source, ""),
					HtmlPart::LineBreak => {}
				}
			}
		}
		self.end_paragraph();
	}

	fn start_link(&mut self, index: usize, target: String) {
		self.ensure_paragraph(index);
		self.link_depth += 1;
		// Quotes and backslashes would end or escape the field argument.
		let field_target: String = target
			.chars()
			.map(|character| match character {
				'"' | '\\' | '{' | '}' => '_',
				character => character,
			})
			.collect();
		self.body.push_str("{\\field{\\*\\fldinst{HYPERLINK \"");
		self.position += "HYPERLINK \"".len();
		let target_start = self.position;
		self.push_text(&field_target);
		let target_range = target_start..self.position;
		// The closing quote.
		self.position += 1;
		let link = self.color(self.options.theme.link);
		self.body.push_str(&format!("\"}}}}{{\\fldrslt{{\\cf{link}\\ul "));
		self.open_links.push((self.position, target_range, target));
	}

	fn end_link(&mut self) {
		self.link_depth = self.link_depth.saturating_sub(1);
		self.body.push_str("}}}");
		if let Some((start, target_range, target)) = self.open_links.pop() {
			self.document.links.push(RtfLink { range: start..self.position, target_range, target });
		}
	}

	fn end_code_block(&mut self) {
		let Some((index, language, mut code)) = self.code_block.take() else { return };
		if code.ends_with('\n') {
			code.pop();
		}
		let spans = match language.is_empty() {
			true => Vec::new(),
			false => (self.options.highlight_code)(&language, &code),
		};
		let background = self.color(self.options.theme.code_background);
		self.begin_paragraph(index, &format!("\\f1\\fs{CODE_FONT_SIZE}\\highlight{background}"));
		let mut written = 0;
		for span in &spans {
			let Range { start, end } = span.range;
			let is_valid = written <= start
				&& start < end
				&& end <= code.len()
				&& code.is_char_boundary(start)
				&& code.is_char_boundary(end);
			if !is_valid {
				continue;
			}
			self.push_code(&code[written..start]);
			let color = self.color(self.options.theme.token_color(span.kind));
			self.body.push_str(&format!("{{\\cf{color} "));
			self.push_code(&code[start..end]);
			self.body.push('}');
			written = end;
		}
		self.push_code(&code[written..]);
		self.end_paragraph();
	}

	/// Code with its line breaks, which keep the code block one paragraph.
	fn push_code(&mut self, code: &str) {
		for (number, line) in code.split('\n').enumerate() {
			if number > 0 {
				self.body.push_str("\\line ");
				self.position += 1;
			}
			self.push_text(line);
		}
	}

	fn end_image(&mut self, index: usize) {
		let Some(image) = self.image.take() else { return };
		self.image(index, &image.source, &image.alt);
	}

	/// A PNG or JPEG picture, or the alternative text for other or missing images.
	fn image(&mut self, index: usize, source: &str, alt: &str) {
		self.ensure_paragraph(index);
		let loaded = (self.options.load_image)(source);
		let picture = loaded.as_deref().and_then(|image| {
			let format = match image.media_type {
				"image/png" => "\\pngblip",
				"image/jpeg" => "\\jpegblip",
				_ => return None,
			};
			Some((format, image_size(&image.bytes)?, &image.bytes))
		});
		let Some((format, (width, height), bytes)) = picture else {
			let muted = self.color(self.options.theme.muted);
			self.body.push_str(&format!("{{\\i\\cf{muted} "));
			self.push_text(&format!("[{}]", if alt.is_empty() { source } else { alt }));
			self.body.push('}');
			return;
		};
		let available =
			self.options.page_width.saturating_sub(self.left_indent()).max(MINIMUM_WIDTH);
		let natural_width = width.saturating_mul(TWIPS_PER_PIXEL);
		let scale = |size: u32| match natural_width > available {
			true => (u64::from(size) * u64::from(available) / u64::from(natural_width)) as u32,
			false => size,
		};
		let goal_size = (scale(natural_width), scale(height.saturating_mul(TWIPS_PER_PIXEL)));
		self.push_picture(format, (width, height), goal_size, bytes);
	}

	/// The Octicon of an alert title in `color`, followed by a space. It is rendered for the DPI
	/// of the view, so that RichEdit does not need to scale it.
	fn alert_icon(&mut self, kind: AlertKind, color: Rgb) {
		let pixels = ICON_SIZE * self.options.pixels_per_inch / 96;
		let [red, green, blue] = color;
		let svg = kind.icon_svg(&format!("#{red:02x}{green:02x}{blue:02x}"));
		let Some(png) = npp::svg::render_png(&svg, pixels, pixels) else { return };
		let goal = ICON_SIZE * TWIPS_PER_PIXEL;
		self.push_picture("\\pngblip", (ICON_SIZE, ICON_SIZE), (goal, goal), &png);
		self.push_text(" ");
	}

	/// A picture of `size` pixels at 96 DPI, shown with `goal_size` in twips.
	fn push_picture(
		&mut self,
		format: &str,
		size: (u32, u32),
		goal_size: (u32, u32),
		bytes: &[u8],
	) {
		self.move_space_before_to_previous_paragraph();
		// RichEdit only shows the picture with its size in HIMETRIC (1/100 mm), as Word writes it.
		let himetric = |pixels: u32| (u64::from(pixels) * HIMETRIC_PER_INCH / 96) as u32;
		let (width, height) = (himetric(size.0), himetric(size.1));
		let (goal_width, goal_height) = goal_size;
		self.body.reserve(bytes.len() * 2 + 100);
		self.body.push_str(&format!(
			"{{\\pict{format}\\picw{width}\\pich{height}\\picwgoal{goal_width}\\pichgoal{goal_height}\n"
		));
		const HEX: &[u8; 16] = b"0123456789abcdef";
		for (count, byte) in bytes.iter().enumerate() {
			self.body.push(char::from(HEX[usize::from(byte >> 4)]));
			self.body.push(char::from(HEX[usize::from(byte & 15)]));
			if count % 64 == 63 {
				self.body.push('\n');
			}
		}
		self.body.push('}');
		self.position += 1;
	}

	/// RichEdit ignores paragraph borders, so a rule is an empty table cell with a top border.
	/// The other sides get borders in the background color, as RichEdit draws grid lines for
	/// cells without borders.
	fn rule(&mut self, index: usize) {
		self.record_block(index);
		let border = self.color(self.options.theme.border);
		let background = self.color(self.options.theme.background);
		let left = self.left_indent();
		let right = left + self.options.page_width.saturating_sub(left).max(MINIMUM_WIDTH);
		let mut row =
			format!("\\trowd\\trgaph0\\trleft{left}\\clbrdrt\\brdrs\\brdrw30\\brdrcf{border}");
		for side in ["l", "b", "r"] {
			row.push_str(&format!("\\clbrdr{side}\\brdrs\\brdrw10\\brdrcf{background}"));
		}
		self.body.push_str(&row);
		self.body.push_str(&format!(
			"\\cellx{right}\n\\pard\\intbl\\plain\\fs8\\sb{PARAGRAPH_SPACING} \\cell\\row\n"
		));
		// Row start, cell end and row end.
		self.position += 5;
		self.is_first_paragraph = false;
	}

	fn start_table(&mut self, index: usize, alignments: &[Alignment]) {
		// The longest cell text and the longest word of each column, in characters.
		let mut lengths = vec![(3usize, 3usize); alignments.len()];
		let mut column = 0;
		let mut cell_length = 0;
		for event in &self.events[index + 1..] {
			match event {
				Event::Start(Tag::TableCell) => cell_length = 0,
				Event::End(TagEnd::TableCell) => {
					if let Some((longest_cell, _)) = lengths.get_mut(column) {
						*longest_cell = (*longest_cell).max(cell_length);
					}
					column += 1;
				}
				Event::Start(Tag::TableRow | Tag::TableHead) => column = 0,
				Event::Text(text) | Event::Code(text) => {
					cell_length += text.chars().count();
					let longest_word = text.split_whitespace().map(|word| word.chars().count());
					if let Some((_, word)) = lengths.get_mut(column) {
						*word = longest_word.fold(*word, usize::max);
					}
				}
				Event::End(TagEnd::Table) => break,
				_ => {}
			}
		}
		let left = self.left_indent();
		let available = u64::from(self.options.page_width.saturating_sub(left).max(MINIMUM_WIDTH));
		const TWIPS_PER_CHARACTER: u64 = 140;
		const CELL_PADDING: u64 = 240;
		let width_of =
			|characters: usize| characters.min(60) as u64 * TWIPS_PER_CHARACTER + CELL_PADDING;
		let natural: Vec<u64> = lengths.iter().map(|&(cell, _)| width_of(cell)).collect();
		let minimum: Vec<u64> =
			lengths.iter().map(|&(cell, word)| width_of(word.min(cell))).collect();
		let mut right_edge = u64::from(left);
		let cell_right_edges = column_widths(&natural, &minimum, available)
			.into_iter()
			.map(|width| {
				right_edge += width;
				right_edge as u32
			})
			.collect();
		self.table = Some(TableLayout {
			alignments: alignments.to_vec(),
			cell_right_edges,
			left,
			is_in_head: false,
			column: 0,
		});
	}

	fn start_table_row(&mut self, index: usize, is_head: bool) {
		self.record_block(index);
		let border = self.color(self.options.theme.border);
		let header_background = self.color(self.options.theme.table_header_background);
		let Some(table) = &mut self.table else { return };
		table.column = 0;
		let mut row = format!("\\trowd\\trgaph100\\trleft{}", table.left);
		for right_edge in &table.cell_right_edges {
			for side in ["t", "l", "b", "r"] {
				row.push_str(&format!("\\clbrdr{side}\\brdrs\\brdrw10\\brdrcf{border}"));
			}
			if is_head {
				row.push_str(&format!("\\clcbpat{header_background}"));
			}
			row.push_str(&format!("\\cellx{right_edge}"));
		}
		row.push('\n');
		self.body.push_str(&row);
		self.position += 2;
	}

	fn start_table_cell(&mut self) {
		let text_color = self.color(self.options.theme.text);
		let Some(table) = &self.table else { return };
		let alignment = match table.alignments.get(table.column) {
			Some(Alignment::Center) => "\\qc",
			Some(Alignment::Right) => "\\qr",
			_ => "\\ql",
		};
		let is_in_head = table.is_in_head;
		self.body.push_str(&format!(
			"\\pard\\intbl\\plain\\f0\\fs{BASE_FONT_SIZE}\\cf{text_color}{alignment} "
		));
		if is_in_head {
			self.body.push_str("{\\b ");
		}
	}
}

/// Column widths within `available`: the `natural` widths if they fit, otherwise at least the
/// `minimum` widths (e.g. of the longest word) with the rest shared in proportion to what each
/// column lacks of its natural width. Too narrow views shrink the minimum widths proportionally.
pub fn column_widths(natural: &[u64], minimum: &[u64], available: u64) -> Vec<u64> {
	let natural_total: u64 = natural.iter().sum();
	if natural_total <= available {
		return natural.to_vec();
	}
	let minimum_total: u64 = minimum.iter().sum();
	if minimum_total >= available {
		let minimum_total = minimum_total.max(1);
		return minimum.iter().map(|&width| width * available / minimum_total).collect();
	}
	let wanted_total = (natural_total - minimum_total).max(1);
	let rest = available - minimum_total;
	natural
		.iter()
		.zip(minimum)
		.map(|(&natural, &minimum)| minimum + (natural - minimum) * rest / wanted_total)
		.collect()
}

/// Width and height in pixels of a PNG or baseline/progressive JPEG image.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
	const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
	if bytes.starts_with(PNG_SIGNATURE) {
		let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
		let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
		return Some((width, height)).filter(|&(width, height)| width > 0 && height > 0);
	}
	if !bytes.starts_with(&[0xff, 0xd8]) {
		return None;
	}
	let mut position = 2;
	loop {
		while *bytes.get(position)? == 0xff && bytes.get(position + 1) == Some(&0xff) {
			position += 1;
		}
		if *bytes.get(position)? != 0xff {
			return None;
		}
		let marker = *bytes.get(position + 1)?;
		let length =
			usize::from(u16::from_be_bytes([*bytes.get(position + 2)?, *bytes.get(position + 3)?]));
		// Start of frame markers, except DHT (C4), JPG (C8) and DAC (CC).
		if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
			let height = u16::from_be_bytes([*bytes.get(position + 5)?, *bytes.get(position + 6)?]);
			let width = u16::from_be_bytes([*bytes.get(position + 7)?, *bytes.get(position + 8)?]);
			return Some((u32::from(width), u32::from(height)))
				.filter(|&(width, height)| width > 0 && height > 0);
		}
		if length < 2 {
			return None;
		}
		position += 2 + length;
	}
}

#[cfg(test)]
pub(crate) mod tests;
