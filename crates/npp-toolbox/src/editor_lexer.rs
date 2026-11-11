//! The Markdown lexer of the editor, the language "Markdown (NppToolBox)" of Notepad++.
//!
//! Notepad++ applies the styles of the Markdown text from the styles file
//! `plugins/Config/NppToolBox.xml`. Code blocks are styled by Notepad++'s lexer of their
//! language, run on a [`MappedDocument`] that moves its styles to free style numbers from
//! [`FIRST_CODE_STYLE`] on. Notepad++ knows nothing about these styles, so the plugin gives them
//! the colors of the language in the current theme, with the font of the `CODE BLOCK` style,
//! each time Notepad++ has reset the styles of an editor. A style whose theme background is the
//! theme's default background takes the background of `CODE BLOCK` instead. The `URL` style
//! becomes a hotspot, whose links `editor_links` opens.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::ffi::c_int;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use npp::ffi::{SciNotification, language, npp_notification, scintilla_constant};
use npp::lexer::{
	Document, ForeignLexer, Lexer, MappedDocument, Position, StyleDescription, StyleMapping,
};
use npp::{LexerDefinition, MessageKind, Notepad};
use windows_sys::Win32::UI::WindowsAndMessaging::{KillTimer, SetTimer};

use crate::PLUGIN_NAME;
use crate::code_highlight::{
	CodeLanguage, StylerEntry, code_language, keyword_sets, lexer_styles, styler_names,
	theme_file_name,
};
use crate::commands::{MARKDOWN_EXTENSIONS, read_setting, settings_file};
use crate::lexer_highlighter::read_configuration;
use crate::markdown_lexer::{
	FIRST_CODE_STYLE, LANGUAGE_NAME, LexerMemory, MarkdownDocument, STYLES, lex_document, style,
	styles_file,
};
use crate::settings;

const MARKDOWN_LEXER: LexerDefinition =
	LexerDefinition { name: LANGUAGE_NAME, create: create_markdown_lexer };

/// `[Lexer] Markdown` values.
const LEXER_OFF: u32 = 0;
const LEXER_FOR_ALL_MARKDOWN_FILES: u32 = 2;

/// The number of styles for code blocks, up to Scintilla's last style 255.
const CODE_STYLE_CAPACITY: usize = 256 - FIRST_CODE_STYLE as usize;

static IS_LEXER_OFFERED: AtomicBool = AtomicBool::new(false);
static IS_USED_FOR_ALL_MARKDOWN_FILES: AtomicBool = AtomicBool::new(false);
/// The thread timer that applies the code block styles, 0 if none is pending.
static CODE_STYLE_TIMER: AtomicUsize = AtomicUsize::new(0);

// Clippy 1.99 reports the initializers although they are `const` already.
thread_local! {
	#[allow(clippy::missing_const_for_thread_local)]
	static CODE_STYLES: RefCell<CodeStyles> =
		const { RefCell::new(CodeStyles { styles: Vec::new() }) };
	#[allow(clippy::missing_const_for_thread_local)]
	static CONFIGURATION: RefCell<Option<Configuration>> = const { RefCell::new(None) };
	/// The documents whose language was checked for `[Lexer] Markdown=2`.
	#[allow(clippy::missing_const_for_thread_local)]
	static CHECKED_BUFFERS: RefCell<BTreeSet<usize>> = const { RefCell::new(BTreeSet::new()) };
}

/// The lexers for Notepad++, see [`npp::PluginDefinition::lexers`].
pub fn lexers(notepad: &Notepad) -> &'static [LexerDefinition] {
	let has_styles_file = ensure_styles_file(notepad);
	let mode = read_setting(settings_file(notepad).as_ref(), &settings::MARKDOWN_LEXER);
	if mode == LEXER_OFF || !has_styles_file {
		return &[];
	}
	IS_LEXER_OFFERED.store(true, Ordering::Relaxed);
	IS_USED_FOR_ALL_MARKDOWN_FILES.store(mode == LEXER_FOR_ALL_MARKDOWN_FILES, Ordering::Relaxed);
	&[MARKDOWN_LEXER]
}

/// Creates the styles file unless Notepad++ finds one. Notepad++ does not load the plugin
/// without it, as the plugin exports lexer functions.
fn ensure_styles_file(notepad: &Notepad) -> bool {
	let file_name = format!("{PLUGIN_NAME}.xml");
	let program_file = notepad
		.program_directory()
		.map(|directory| directory.join("plugins").join("Config").join(&file_name));
	if program_file.is_some_and(|file| file.exists()) {
		return true;
	}
	let Some(directory) = notepad.plugins_config_directory() else { return false };
	let path = directory.join(&file_name);
	if path.exists() {
		return true;
	}
	let created = std::fs::create_dir_all(&directory)
		.and_then(|()| std::fs::write(&path, styles_file(&MARKDOWN_EXTENSIONS, "\r\n")));
	if let Err(error) = created {
		let text = format!(
			"Cannot create {}:\n{error}\n\nNotepad++ needs this file to load {PLUGIN_NAME}.",
			path.display()
		);
		notepad.show_message(PLUGIN_NAME, &text, MessageKind::Error);
		return false;
	}
	true
}

pub fn on_notification(notepad: &Notepad, notification: &SciNotification) {
	if !IS_LEXER_OFFERED.load(Ordering::Relaxed)
		|| notification.header.hwnd_from != notepad.window()
	{
		return;
	}
	match notification.header.code {
		npp_notification::WORD_STYLES_UPDATED | npp_notification::DARK_MODE_CHANGED => {
			CONFIGURATION.with_borrow_mut(|configuration| *configuration = None);
			schedule_code_style_update();
		}
		npp_notification::BUFFER_ACTIVATED => {
			if IS_USED_FOR_ALL_MARKDOWN_FILES.load(Ordering::Relaxed) {
				use_lexer_for_markdown_file(notepad, notification.header.id_from);
			}
			schedule_code_style_update();
		}
		npp_notification::LANGUAGE_CHANGED | npp_notification::READY => {
			schedule_code_style_update();
		}
		// Notepad++ may reuse the ID for another document.
		npp_notification::FILE_CLOSED => {
			CHECKED_BUFFERS.with_borrow_mut(|buffers| buffers.remove(&notification.header.id_from));
		}
		_ => {}
	}
}

/// Gives a Markdown document the language of the lexer when it is first activated, unless it
/// has a built-in language other than Normal Text.
fn use_lexer_for_markdown_file(notepad: &Notepad, buffer_id: usize) {
	if !CHECKED_BUFFERS.with_borrow_mut(|buffers| buffers.insert(buffer_id)) {
		return;
	}
	let extension = notepad.current_file_extension();
	let extension = extension.trim_start_matches('.');
	if !MARKDOWN_EXTENSIONS.iter().any(|markdown| markdown.eq_ignore_ascii_case(extension)) {
		return;
	}
	if !matches!(notepad.current_language(), language::TEXT | language::USER) {
		return;
	}
	let Some(language) = notepad.external_language(LANGUAGE_NAME.to_str().unwrap_or_default())
	else {
		return;
	};
	notepad.set_current_language(language);
}

fn create_markdown_lexer(notepad: &Notepad) -> Box<dyn Lexer> {
	Box::new(MarkdownLexer {
		notepad: *notepad,
		memory: LexerMemory::default(),
		embedded_lexers: HashMap::new(),
	})
}

struct EmbeddedLexer {
	lexer: ForeignLexer,
	line_states: HashMap<Position, c_int>,
}

/// One lexer per document. Notepad++ creates a new one each time it shows the document.
struct MarkdownLexer {
	notepad: Notepad,
	memory: LexerMemory,
	/// The lexers of the code block languages by `langs.xml` name; `None` if Notepad++ has
	/// none.
	embedded_lexers: HashMap<&'static str, Option<EmbeddedLexer>>,
}

impl Lexer for MarkdownLexer {
	fn name(&self) -> &'static std::ffi::CStr {
		LANGUAGE_NAME
	}

	fn style_count(&self) -> usize {
		STYLES.len()
	}

	fn style_description(&self, style: usize) -> Option<StyleDescription> {
		let style = STYLES.get(style)?;
		Some(StyleDescription {
			name: style.name,
			tags: style.tags,
			description: style.description,
		})
	}

	fn lex(&mut self, document: Document, start: usize, length: Position, _initial_style: c_int) {
		let end = start.saturating_add(usize::try_from(length).unwrap_or(0));
		let code_page = document.code_page();
		let mut source = ScintillaSource {
			document,
			is_utf8: usize::try_from(code_page).ok() == Some(scintilla_constant::CODE_PAGE_UTF8),
			is_dbcs: code_page != 0
				&& usize::try_from(code_page).ok() != Some(scintilla_constant::CODE_PAGE_UTF8),
		};
		let lexed = lex_document(&mut source, &mut self.memory, start..end);
		if lexed.styles.is_empty() {
			return;
		}
		document.start_styling(lexed.start as Position);
		document.set_styles(&lexed.styles);
		for code_block in &lexed.code_blocks {
			let Some(language) = code_language(&code_block.language) else { continue };
			let Some(embedded) = self.embedded_lexer(&language) else { continue };
			let EmbeddedLexer { lexer, line_states } = embedded;
			let mut mapping = CodeStyleMapping { language, line_states };
			// The lexer sees only the code, so that lexers going back to the start of a
			// statement cannot style the Markdown before it.
			let content = code_block.content.start as Position..code_block.content.end as Position;
			let mut mapped_document = MappedDocument::new(document, content, &mut mapping);
			// SAFETY: The mapped document wraps Scintilla's live document and outlives the call.
			unsafe {
				lexer.lex(
					0,
					code_block.content.len() as Position,
					c_int::from(style::DEFAULT),
					mapped_document.raw(),
				);
			}
			for gap in &code_block.gaps {
				document.start_styling(gap.start as Position);
				document.set_style_for(gap.len() as Position, code_block.gap_style);
			}
		}
		// The code block lexers moved the end of the styled text.
		document.start_styling((lexed.start + lexed.styles.len()) as Position);
		if lexed.changes_whole_document {
			document.change_lexer_state(0..document.length());
		}
	}
}

/// The line state bit of a restart line, see [`MarkdownDocument::is_restart_line`]. The code
/// block lexers keep their line states apart, see [`CodeStyleMapping`].
const RESTART_LINE: c_int = 1;

struct ScintillaSource {
	document: Document,
	is_utf8: bool,
	is_dbcs: bool,
}

impl MarkdownDocument for ScintillaSource {
	fn length(&self) -> usize {
		usize::try_from(self.document.length()).unwrap_or(0)
	}

	fn read(&self, range: std::ops::Range<usize>, buffer: &mut Vec<u8>) {
		self.document.read(range.start as Position..range.end as Position, buffer);
	}

	fn line_of(&self, position: usize) -> usize {
		usize::try_from(self.document.line_from_position(position as Position)).unwrap_or(0)
	}

	fn line_start(&self, line: usize) -> usize {
		usize::try_from(self.document.line_start(line as Position)).unwrap_or(0)
	}

	fn is_restart_line(&self, line: usize) -> bool {
		self.document.line_state(line as Position) & RESTART_LINE != 0
	}

	fn set_restart_line(&mut self, line: usize, is_restart: bool) {
		let line = line as Position;
		let state = self.document.line_state(line);
		let new_state = if is_restart { state | RESTART_LINE } else { state & !RESTART_LINE };
		// Scintilla notifies every change of a line state.
		if new_state != state {
			self.document.set_line_state(line, new_state);
		}
	}

	fn is_utf8(&self) -> bool {
		self.is_utf8
	}

	fn is_lead_byte(&self, byte: u8) -> bool {
		self.is_dbcs && self.document.is_dbcs_lead_byte(byte)
	}
}

impl MarkdownLexer {
	fn embedded_lexer(&mut self, language: &CodeLanguage) -> Option<&mut EmbeddedLexer> {
		let notepad = self.notepad;
		self.embedded_lexers
			.entry(language.notepad_name)
			.or_insert_with(|| create_embedded_lexer(&notepad, language))
			.as_mut()
	}
}

/// The Lexilla lexer of `language`, configured like Notepad++ does for documents.
fn create_embedded_lexer(notepad: &Notepad, language: &CodeLanguage) -> Option<EmbeddedLexer> {
	// SAFETY: Notepad++ hands over a new lexer or null.
	let lexer = unsafe { ForeignLexer::from_raw(notepad.create_lexer(language.lexer_name)) }?;
	if language.lexer_name == "cpp" {
		// Notepad++ does not gray out inactive preprocessor branches.
		lexer.set_property("lexer.cpp.track.preprocessor", "0");
	}
	for (key, value) in language.properties {
		lexer.set_property(key, value);
	}
	let keywords = with_configuration(notepad, |configuration| {
		let styles = configuration.theme_styles(language);
		keyword_sets(language, &configuration.langs_xml, &styles)
	});
	for (set, words) in keywords {
		lexer.set_word_list(set, &words);
	}
	Some(EmbeddedLexer { lexer, line_states: HashMap::new() })
}

struct CodeStyleMapping<'a> {
	language: CodeLanguage,
	line_states: &'a mut HashMap<Position, c_int>,
}

impl StyleMapping for CodeStyleMapping<'_> {
	fn document_style(&mut self, style: u8) -> u8 {
		CODE_STYLES.with_borrow_mut(|styles| styles.document_style(self.language, style))
	}

	fn lexer_style(&self, style: u8) -> u8 {
		CODE_STYLES.with_borrow(|styles| styles.language_style(self.language, style))
	}

	fn line_state(&self, line: Position) -> c_int {
		self.line_states.get(&line).copied().unwrap_or(0)
	}

	fn set_line_state(&mut self, line: Position, state: c_int) {
		self.line_states.insert(line, state);
	}
}

/// The code block styles in use: style `FIRST_CODE_STYLE + i` is `styles[i]`, a style of a
/// language's lexer. Shared by all documents, as both editors may show any of them.
struct CodeStyles {
	styles: Vec<(CodeLanguage, u8)>,
}

impl CodeStyles {
	/// The document style for `style` of the lexer of `language`. All styles in use, the
	/// code takes the `CODE BLOCK` style.
	fn document_style(&mut self, language: CodeLanguage, style: u8) -> u8 {
		let index = self.styles.iter().position(|&entry| entry == (language, style));
		let index = match index {
			Some(index) => index,
			None if self.styles.len() < CODE_STYLE_CAPACITY => {
				self.styles.push((language, style));
				schedule_code_style_update();
				self.styles.len() - 1
			}
			None => return style::CODE_BLOCK,
		};
		FIRST_CODE_STYLE + index as u8
	}

	/// The style of the lexer of `language` for the document style `style`, 0 if it is none
	/// of that lexer.
	fn language_style(&self, language: CodeLanguage, style: u8) -> u8 {
		style
			.checked_sub(FIRST_CODE_STYLE)
			.and_then(|index| self.styles.get(usize::from(index)))
			.filter(|(entry_language, _)| *entry_language == language)
			.map_or(0, |&(_, language_style)| language_style)
	}
}

/// The parts of Notepad++'s configuration the code block lexers use, read once.
struct Configuration {
	langs_xml: String,
	theme_xml: String,
	/// The theme's styles of each language.
	theme_styles: HashMap<&'static str, Vec<StylerEntry>>,
}

impl Configuration {
	fn read(notepad: &Notepad) -> Self {
		Self {
			langs_xml: read_configuration(notepad, "langs"),
			theme_xml: read_theme(notepad),
			theme_styles: HashMap::new(),
		}
	}

	fn theme_styles(&mut self, language: &CodeLanguage) -> Vec<StylerEntry> {
		let theme_xml = &self.theme_xml;
		self.theme_styles
			.entry(language.notepad_name)
			.or_insert_with(|| {
				let mut styles: Vec<StylerEntry> = Vec::new();
				for name in styler_names(language) {
					for entry in lexer_styles(theme_xml, name) {
						if !styles.iter().any(|existing| existing.style_id == entry.style_id) {
							styles.push(entry);
						}
					}
				}
				styles
			})
			.clone()
	}
}

/// Reading the configuration sends messages to Notepad++, so the configuration is taken out of
/// its cell while it is used instead of holding a borrow.
fn with_configuration<R>(notepad: &Notepad, use_it: impl FnOnce(&mut Configuration) -> R) -> R {
	let mut configuration =
		CONFIGURATION.with_borrow_mut(Option::take).unwrap_or_else(|| Configuration::read(notepad));
	let result = use_it(&mut configuration);
	CONFIGURATION.with_borrow_mut(|cell| *cell = Some(configuration));
	result
}

/// The theme file of the current mode, or `stylers.xml`.
fn read_theme(notepad: &Notepad) -> String {
	let config_xml = notepad
		.settings_directory()
		.and_then(|directory| std::fs::read(directory.join("config.xml")).ok())
		.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
		.unwrap_or_default();
	if let Some(name) = theme_file_name(&config_xml, notepad.is_dark_mode_enabled()) {
		let candidates: [Option<PathBuf>; 2] = [
			notepad.settings_directory().map(|directory| directory.join("themes").join(&name)),
			notepad.program_directory().map(|directory| directory.join("themes").join(&name)),
		];
		let theme = candidates.into_iter().flatten().find_map(|path| std::fs::read(path).ok());
		if let Some(theme) = theme {
			return String::from_utf8_lossy(&theme).into_owned();
		}
	}
	read_configuration(notepad, "stylers")
}

/// Applies the code block styles soon, after Notepad++ has finished styling the editors.
fn schedule_code_style_update() {
	if CODE_STYLE_TIMER.load(Ordering::Relaxed) != 0 {
		return;
	}
	// SAFETY: A thread timer without window; Notepad++'s message loop calls the procedure.
	let timer = unsafe { SetTimer(core::ptr::null_mut(), 0, 0, Some(apply_code_styles_now)) };
	CODE_STYLE_TIMER.store(timer, Ordering::Relaxed);
}

unsafe extern "system" fn apply_code_styles_now(
	_window: windows_sys::Win32::Foundation::HWND,
	_message: u32,
	timer: usize,
	_time: u32,
) {
	// SAFETY: Ends the thread timer that called this procedure.
	unsafe { KillTimer(core::ptr::null_mut(), timer) };
	CODE_STYLE_TIMER.store(0, Ordering::Relaxed);
	if let Some(notepad) = npp::plugin::notepad() {
		apply_code_styles(&notepad);
	}
}

/// Gives the code block styles of the editors showing a document with the lexer the colors and
/// font styles of their language in the theme, and the font of `CODE BLOCK`. Backgrounds that
/// the theme sets apart from its default background are kept, all others are the background of
/// `CODE BLOCK`, so that the code block stands out as a whole.
fn apply_code_styles(notepad: &Notepad) {
	let code_styles = CODE_STYLES.with_borrow(|styles| styles.styles.clone());
	let theme_styles: Vec<Option<StylerEntry>> = with_configuration(notepad, |configuration| {
		code_styles
			.iter()
			.map(|(language, language_style)| {
				let styles = configuration.theme_styles(language);
				let find = |id: u8| styles.iter().find(|entry| entry.style_id == id).cloned();
				find(*language_style).or_else(|| find(0))
			})
			.collect()
	});
	let language_name = LANGUAGE_NAME.to_str().unwrap_or_default();
	for editor in notepad.editors() {
		if editor.lexer_language() != language_name {
			continue;
		}
		let code_color = editor.style_foreground(style::CODE_BLOCK);
		let default_background = editor.style_background(scintilla_constant::STYLE_DEFAULT);
		editor.set_style_hotspot(style::URL, true);
		for code_style in [style::CODE_BLOCK, style::CODE_FENCE] {
			editor.set_style_eol_filled(code_style, true);
		}
		for (index, theme_style) in theme_styles.iter().enumerate() {
			let style = FIRST_CODE_STYLE + index as u8;
			let font_style = theme_style.as_ref().map_or(0, |entry| entry.font_style);
			let color =
				theme_style.as_ref().and_then(|entry| entry.color).map_or(code_color, rgb_to_bgr);
			editor.copy_style_font_and_background(style::CODE_BLOCK, style);
			let background =
				theme_style.as_ref().and_then(|entry| entry.background).map(rgb_to_bgr);
			let own_background = background.filter(|&color| color != default_background);
			if let Some(background) = own_background {
				editor.set_style_background(style, background);
			}
			editor.set_style_foreground(style, color);
			editor.set_style_font_style(
				style,
				font_style & 1 != 0,
				font_style & 2 != 0,
				font_style & 4 != 0,
			);
			// Like Notepad++ does in a file of the language, an own background ends with the
			// text of its line, while the code block background fills the line.
			editor.set_style_eol_filled(style, own_background.is_none());
		}
	}
}

/// `0xRRGGBB` to Scintilla's `0xBBGGRR`.
fn rgb_to_bgr(color: u32) -> u32 {
	((color & 0xFF) << 16) | (color & 0xFF00) | ((color >> 16) & 0xFF)
}
