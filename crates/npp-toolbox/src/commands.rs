//! Menu command implementations connecting the editing logic to Notepad++.

use std::borrow::Cow;
use std::path::Path;

use npp::clipboard::{ClipboardContent, set_clipboard};
use npp::ffi::SciNotification;
use npp::ffi::language;
use npp::ini_file::IniFile;
use npp::input_dialog::ask_for_number;
use npp::{Command, MessageKind, Notepad, Scintilla};

use crate::PLUGIN_NAME;
use crate::clipboard_html::clipboard_html;
use crate::code_highlight::{info_language_of, lexer_styles};
use crate::comment_selection::{comment_ranges, is_comment_style};
use crate::image_embedding::ImageEmbedding;
use crate::lexer_highlighter::{LexerHighlighter, read_configuration};
use crate::markdown_html::{
	ImageSyntax, RenderOptions, Styling, fenced_code_block, html_page, inline_styled_fragment,
	render_markdown,
};
use crate::markdown_table::{
	FormattedTable, TableError, caret_column_after_reformat, compact_table, is_row_with_prefix,
	realign_table, row_prefix,
};
use crate::mermaid::DiagramRenderer;
use crate::rewrap::{DocumentSyntax, RewrapSettings, paragraph_at, rewrap_range};
use crate::settings::{self, SETTINGS_FILE_NAME, Setting};
use crate::{editor_lexer, viewer};
use crate::{mermaid_clock, mermaid_image, mermaid_text};
pub(crate) const MARKDOWN_EXTENSIONS: [&str; 7] =
	["md", "markdown", "mdown", "mkd", "mkdn", "mdwn", "mdtxt"];

pub fn realign_markdown_table(notepad: &Notepad) {
	reformat_markdown_table(notepad, realign_table);
}

pub fn compact_markdown_table(notepad: &Notepad) {
	reformat_markdown_table(notepad, compact_table);
}

/// Replaces the table at the caret by `reformat`, keeping the caret in its cell.
fn reformat_markdown_table(
	notepad: &Notepad,
	reformat: fn(&[&str], usize) -> Result<FormattedTable, TableError>,
) {
	let editor = notepad.current_scintilla();
	if editor.is_read_only() {
		return;
	}
	let caret = editor.caret();
	let caret_line = editor.line_of_position(caret);
	let caret_line_text = editor.line_text(caret_line);
	let prefix = row_prefix(&caret_line_text).to_owned();
	if !is_row_with_prefix(&caret_line_text, &prefix) {
		notepad.show_message(
			PLUGIN_NAME,
			"The cursor is not inside a Markdown table.",
			MessageKind::Information,
		);
		return;
	}

	let is_table_row = |line: usize| is_row_with_prefix(&editor.line_text(line), &prefix);
	let mut first_line = caret_line;
	while first_line > 0 && is_table_row(first_line - 1) {
		first_line -= 1;
	}
	let mut end_line = caret_line + 1;
	while end_line < editor.line_count() && is_table_row(end_line) {
		end_line += 1;
	}
	let block = editor.lines_text(first_line..end_line);
	let block: Vec<&str> = block.iter().map(String::as_str).collect();

	let table = match reformat(&block, caret_line - first_line) {
		Ok(table) => table,
		Err(TableError::MissingDelimiterRow | TableError::CaretOutsideTable) => {
			notepad.show_message(
				PLUGIN_NAME,
				"The cursor is not inside a Markdown table (header row followed by a delimiter row like |---|---|).",
				MessageKind::Information,
			);
			return;
		}
	};
	let table_lines = first_line + table.rows.start..first_line + table.rows.end;
	if block[table.rows.clone()].iter().eq(table.lines.iter()) {
		return;
	}

	let caret_row = caret_line - first_line;
	let new_caret_column = caret_column_after_reformat(
		block[caret_row],
		editor.character_column(caret),
		&table.lines[caret_row - table.rows.start],
	);
	editor.as_single_undo_step(|| editor.replace_lines(table_lines, &table.lines));
	editor.move_caret_to(editor.position_at_character_column(caret_line, new_caret_column));
}

pub fn rewrap(notepad: &Notepad) {
	let editor = notepad.current_scintilla();
	if editor.is_read_only() {
		return;
	}
	let document = editor.whole_text();
	let lines = npp::line_ending::split_lines(&document);
	let mut settings = RewrapSettings {
		max_line_width: settings::MAX_LINE_WIDTH.default as usize,
		tab_width: editor.tab_width(),
		syntax: document_syntax(notepad),
	};

	let has_selection = !editor.is_selection_empty();
	let line_range = if has_selection {
		selected_lines(&editor)
	} else {
		match paragraph_at(&lines, editor.line_of_position(editor.caret()), &settings) {
			Some(range) => range,
			None => {
				notepad.show_message(
					PLUGIN_NAME,
					"There is no paragraph to rewrap at the cursor.",
					MessageKind::Information,
				);
				return;
			}
		}
	};

	let settings_file = settings_file(notepad);
	let last_width = read_setting(settings_file.as_ref(), &settings::MAX_LINE_WIDTH);
	let Some(max_line_width) = ask_for_number(
		notepad,
		"Rewrap",
		"Maximum number of characters per line:",
		last_width,
		settings::MAX_LINE_WIDTH.allowed,
	) else {
		return;
	};
	if let Some(file) = &settings_file {
		let setting = &settings::MAX_LINE_WIDTH;
		file.write_u32(setting.section, setting.key, max_line_width);
	}
	settings.max_line_width = max_line_width as usize;

	let new_lines = rewrap_range(&lines, line_range.clone(), &settings);
	if lines[line_range.clone()].iter().eq(new_lines.iter()) {
		return;
	}
	let start = editor.line_start(line_range.start);
	let end = editor.as_single_undo_step(|| editor.replace_lines(line_range, &new_lines));
	if has_selection {
		editor.select(start, end);
	} else {
		editor.move_caret_to(end);
	}
}

/// Selects every comment the lexer styled in the selection, or in the whole document, as
/// multiple selections.
pub fn select_comments(notepad: &Notepad) {
	let editor = notepad.current_scintilla();
	let range = match editor.is_selection_empty() {
		true => 0..editor.length(),
		false => editor.selection(),
	};
	editor.colourise(range.clone());
	let styles = editor.styles_in(range.clone());

	let lexer = editor.lexer_language();
	let styler_entries = notepad.current_language_name().map_or_else(Vec::new, |language| {
		lexer_styles(&read_configuration(notepad, "stylers"), &language)
	});
	let comments = comment_ranges(&styles, |style| {
		let styler_name = styler_entries
			.iter()
			.find(|entry| entry.style_id == style)
			.map(|entry| entry.name.as_str());
		is_comment_style(&lexer, style, styler_name, || editor.style_tags(style))
	});
	let comments: Vec<_> = comments
		.into_iter()
		.map(|comment| range.start + comment.start as isize..range.start + comment.end as isize)
		.collect();
	editor.select_ranges(&comments);
}

/// Renders the selection, or the whole document, into a new HTML document.
pub fn render_html(notepad: &Notepad) {
	let text = selected_or_whole_text(&notepad.current_scintilla());
	let body = render(notepad, &markdown_of_document_text(notepad, &text), Styling::StyleSheet);
	let page = html_page(&notepad.current_file_name(), &body);

	notepad.new_document();
	notepad.set_current_language(language::HTML);
	let editor = notepad.current_scintilla();
	let line_ending = editor.line_ending().as_str();
	match line_ending {
		"\n" => editor.set_text(&page),
		_ => editor.set_text(&page.replace('\n', line_ending)),
	}
	editor.move_caret_to(0);
}

/// Copies the selection, or the whole document, as HTML with inline styles and as the original
/// text.
pub fn copy_as_html(notepad: &Notepad) {
	let text = selected_or_whole_text(&notepad.current_scintilla());
	let body = render(notepad, &markdown_of_document_text(notepad, &text), Styling::Inline);
	let html = clipboard_html(&inline_styled_fragment(&body));
	let contents = [ClipboardContent::Html(&html), ClipboardContent::UnicodeText(&text)];
	if !set_clipboard(notepad, &contents) {
		notepad.show_message(
			PLUGIN_NAME,
			"The clipboard is in use by another program. Please try again.",
			MessageKind::Warning,
		);
	}
}

/// Shows or hides the live preview of the active document.
pub fn markdown_viewer(notepad: &Notepad) {
	viewer::toggle(notepad);
}

pub fn on_notification(notepad: &Notepad, notification: &SciNotification) {
	viewer::on_notification(notepad, notification);
	editor_lexer::on_notification(notepad, notification);
}

/// The `[Toolbar]` settings select the icons. Commands without a setting always show theirs.
pub fn shows_toolbar_icon(notepad: &Notepad, command: &Command) -> bool {
	settings::toolbar_setting(command.menu_name)
		.is_none_or(|setting| read_setting(settings_file(notepad).as_ref(), setting) == 1)
}

/// Opens the settings file in Notepad++. A missing file is created with all defaults and a
/// description of every setting. Settings missing in an existing file are added.
pub fn settings(notepad: &Notepad) {
	let Some(directory) = notepad.plugins_config_directory() else {
		notepad.show_message(
			PLUGIN_NAME,
			"Notepad++ did not report its plugin configuration directory.",
			MessageKind::Error,
		);
		return;
	};
	let path = directory.join(SETTINGS_FILE_NAME);
	if path.exists() {
		let file = IniFile::new(&path);
		for setting in settings::ALL {
			if !file.contains(setting.section, setting.key) {
				file.write_u32(setting.section, setting.key, setting.default);
			}
		}
	} else {
		let created = std::fs::create_dir_all(&directory)
			.and_then(|()| std::fs::write(&path, settings::settings_file_template("\r\n")));
		if let Err(error) = created {
			let text = format!("Cannot create {}:\n{error}", path.display());
			notepad.show_message(PLUGIN_NAME, &text, MessageKind::Error);
			return;
		}
	}
	if !notepad.open_file(&path) {
		let text = format!("Cannot open {}.", path.display());
		notepad.show_message(PLUGIN_NAME, &text, MessageKind::Error);
	}
}

pub fn about(notepad: &Notepad) {
	let text = format!(
		"{PLUGIN_NAME} {}\n\nHelper functions for Notepad++.\nBuilt for the Notepad++ {} plugin interface.\n\nCopyright (c) 2026 Daniel Starke\nLicensed under the MIT license.",
		env!("CARGO_PKG_VERSION"),
		npp::interface::NOTEPAD_PLUS_PLUS_VERSION
	);
	notepad.show_message(PLUGIN_NAME, &text, MessageKind::Information);
}

fn document_syntax(notepad: &Notepad) -> DocumentSyntax {
	match has_markdown_extension(notepad) {
		true => DocumentSyntax::Markdown,
		false => DocumentSyntax::Code,
	}
}

fn has_markdown_extension(notepad: &Notepad) -> bool {
	let extension = notepad.current_file_extension();
	let extension = extension.trim_start_matches('.');
	MARKDOWN_EXTENSIONS.iter().any(|markdown| markdown.eq_ignore_ascii_case(extension))
}

/// The Markdown to render for `text` of the active document: a code block of the document's
/// language for code (`[Html] CodeAsCodeBlock`), otherwise `text` itself.
fn markdown_of_document_text<'a>(notepad: &Notepad, text: &'a str) -> Cow<'a, str> {
	if read_setting(settings_file(notepad).as_ref(), &settings::CODE_AS_CODE_BLOCK) == 0 {
		return Cow::Borrowed(text);
	}
	match code_block_language(notepad) {
		Some(info_language) => Cow::Owned(fenced_code_block(text, info_language)),
		None => Cow::Borrowed(text),
	}
}

/// The info string of a code block for the active document, `None` for Normal Text and Markdown.
/// User defined languages and other plugins' lexers are recognized as Markdown by their name and
/// otherwise give a plain code block.
fn code_block_language(notepad: &Notepad) -> Option<&'static str> {
	let language_type = notepad.current_language();
	if language_type == language::TEXT || has_markdown_extension(notepad) {
		return None;
	}
	if language_type == language::USER || language_type >= language::EXTERNAL {
		let name = notepad.language_name(language_type);
		return match name.to_ascii_lowercase().contains("markdown") {
			true => None,
			false => Some(""),
		};
	}
	let name = notepad.current_language_name().unwrap_or_default();
	Some(info_language_of(&name).unwrap_or(""))
}

/// Lines touched by the selection. A selection ending at the very start of a line does not
/// include that line, matching how full-line selections are usually made.
fn selected_lines(editor: &Scintilla) -> std::ops::Range<usize> {
	let selection = editor.selection();
	let first_line = editor.line_of_position(selection.start);
	let mut last_line = editor.line_of_position(selection.end);
	if last_line > first_line && editor.line_start(last_line) == selection.end {
		last_line -= 1;
	}
	first_line..last_line + 1
}

pub(crate) fn settings_file(notepad: &Notepad) -> Option<IniFile> {
	notepad
		.plugins_config_directory()
		.map(|directory| IniFile::new(&directory.join(SETTINGS_FILE_NAME)))
}

pub(crate) fn read_setting(file: Option<&IniFile>, setting: &Setting) -> u32 {
	file.map_or(setting.default, |file| {
		setting.valid_or_default(file.read_u32(setting.section, setting.key, setting.default))
	})
}

fn selected_or_whole_text(editor: &Scintilla) -> String {
	match editor.is_selection_empty() {
		true => editor.whole_text(),
		false => editor.text_range(editor.selection()),
	}
}

/// Markdown to HTML with the image and highlighting settings of the settings file.
fn render(notepad: &Notepad, markdown: &str, styling: Styling) -> String {
	let settings_file = settings_file(notepad);
	let setting = |setting: &Setting| read_setting(settings_file.as_ref(), setting);
	let embeds_markdown_images = setting(&settings::EMBED_MARKDOWN_IMAGES) == 1;
	let embeds_html_images = setting(&settings::EMBED_HTML_IMAGES) == 1;
	let highlights_code = setting(&settings::HIGHLIGHT_CODE) == 1;
	let renders_diagrams = setting(&settings::RENDER_MERMAID) == 1;

	let document_path = notepad.current_file_path();
	let embedding = ImageEmbedding {
		base_directory: document_path.as_deref().and_then(Path::parent),
		max_image_bytes: u64::from(setting(&settings::MAX_EMBEDDED_IMAGE_BYTES)),
	};
	let mut replace_image_source = |syntax: ImageSyntax, source: &str| {
		let embeds = match syntax {
			ImageSyntax::Markdown => embeds_markdown_images,
			ImageSyntax::Html => embeds_html_images,
		};
		embeds.then(|| embedding.data_uri(source)).flatten()
	};
	let mut highlighter = LexerHighlighter::new(notepad);
	let mut highlight_code = |language: &str, code: &str| match highlights_code {
		true => highlighter.highlight(language, code),
		false => Vec::new(),
	};
	let mut diagrams = DiagramRenderer::new(
		mermaid_text::text_system(),
		mermaid_clock::system_clock(),
		mermaid_image::decoder(),
		viewer::mermaid_limits(notepad),
	);
	diagrams.screen_width = viewer::screen_width(notepad.window());
	let mut render_diagram = |code: &str, output| diagrams.render(code, false, output);
	render_markdown(
		markdown,
		&mut RenderOptions {
			annotates_source_lines: false,
			is_dark: false,
			styling,
			replace_image_source: &mut replace_image_source,
			highlight_code: &mut highlight_code,
			render_diagram: renders_diagrams.then_some(&mut render_diagram),
		},
	)
}
