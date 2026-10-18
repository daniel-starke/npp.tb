//! Menu command implementations connecting the editing logic to Notepad++.

use std::path::Path;

use npp::clipboard::{ClipboardContent, set_clipboard};
use npp::ffi::SciNotification;
use npp::ffi::language;
use npp::ini_file::IniFile;
use npp::input_dialog::ask_for_number;
use npp::{Command, MessageKind, Notepad, Scintilla};

use crate::PLUGIN_NAME;
use crate::clipboard_html::clipboard_html;
use crate::image_embedding::ImageEmbedding;
use crate::lexer_highlighter::LexerHighlighter;
use crate::markdown_html::{
	ImageSyntax, RenderOptions, Styling, html_page, inline_styled_fragment, render_markdown,
};
use crate::markdown_table::{
	TableError, caret_column_after_realign, is_row_with_prefix, realign_table, row_prefix,
};
use crate::rewrap::{DocumentSyntax, RewrapSettings, paragraph_at, rewrap_range};
use crate::settings::{self, SETTINGS_FILE_NAME, Setting};
use crate::viewer;
pub(crate) const MARKDOWN_EXTENSIONS: [&str; 7] =
	["md", "markdown", "mdown", "mkd", "mkdn", "mdwn", "mdtxt"];

pub fn realign_markdown_table(notepad: &Notepad) {
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

	let table = match realign_table(&block, caret_line - first_line) {
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
	let new_caret_column = caret_column_after_realign(
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

/// Renders the selection, or the whole document, into a new HTML document.
pub fn render_html(notepad: &Notepad) {
	let markdown = selected_or_whole_text(&notepad.current_scintilla());
	let body = render(notepad, &markdown, Styling::StyleSheet);
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

/// Copies the selection, or the whole document, as HTML with inline styles and as Markdown
/// text.
pub fn copy_as_html(notepad: &Notepad) {
	let markdown = selected_or_whole_text(&notepad.current_scintilla());
	let body = render(notepad, &markdown, Styling::Inline);
	let html = clipboard_html(&inline_styled_fragment(&body));
	let contents = [ClipboardContent::Html(&html), ClipboardContent::UnicodeText(&markdown)];
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
	let extension = notepad.current_file_extension();
	let extension = extension.trim_start_matches('.');
	if MARKDOWN_EXTENSIONS.iter().any(|markdown| markdown.eq_ignore_ascii_case(extension)) {
		DocumentSyntax::Markdown
	} else {
		DocumentSyntax::Code
	}
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
	render_markdown(
		markdown,
		&mut RenderOptions {
			annotates_source_lines: false,
			is_dark: false,
			styling,
			replace_image_source: &mut replace_image_source,
			highlight_code: &mut highlight_code,
		},
	)
}
