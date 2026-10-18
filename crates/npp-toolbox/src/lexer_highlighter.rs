//! Highlights code blocks with Notepad++'s own lexers in a hidden Scintilla editor, with the
//! keywords from Notepad++'s `langs.xml` and `stylers.xml`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};

use npp::{Notepad, Scintilla};

use crate::code_highlight::{
	CodeLanguage, KeywordSets, StylerEntry, code_language, keyword_set_index, language_keywords,
	lexer_styles, spans_of_styles, token_kind_of_style,
};
use crate::markdown_html::HighlightedSpan;

/// The hidden editor, created once and reused. Notepad++ destroys it on exit. Stored as an
/// integer because window handles are raw pointers.
static HIDDEN_EDITOR: AtomicIsize = AtomicIsize::new(0);

/// The languages whose keywords the HTML lexer uses for embedded scripts, with their
/// keyword set index.
const HTML_KEYWORD_SOURCES: [(usize, &str); 3] = [(0, "html"), (1, "javascript"), (4, "php")];

pub struct LexerHighlighter {
	notepad: Notepad,
	/// Read on the first code block that needs them.
	langs_xml: Option<String>,
	stylers_xml: Option<String>,
}

impl LexerHighlighter {
	pub fn new(notepad: &Notepad) -> Self {
		Self { notepad: *notepad, langs_xml: None, stylers_xml: None }
	}

	pub fn highlight(&mut self, info_language: &str, code: &str) -> Vec<HighlightedSpan> {
		let Some(language) = code_language(info_language) else { return Vec::new() };
		let Some(editor) = self.hidden_editor() else { return Vec::new() };
		let lexer = self.notepad.create_lexer(language.lexer_name);
		if lexer.is_null() {
			return Vec::new();
		}
		editor.set_lexer(lexer);
		for (key, value) in language.properties {
			editor.set_lexer_property(key, value);
		}
		let styler_entries = lexer_styles(self.stylers_xml(), language.notepad_name);
		self.set_keywords(&editor, &language, &styler_entries);

		editor.set_text(code);
		editor.colourise_all();
		let styles = editor.styles();
		// Releases the copy of the code.
		editor.set_text("");
		spans_of_styles(&styles, |style| {
			let styler_name = styler_entries
				.iter()
				.find(|entry| entry.style_id == style)
				.map_or("", |entry| entry.name.as_str());
			token_kind_of_style(styler_name)
				.or_else(|| token_kind_of_style(&editor.style_tags(style)))
		})
	}

	fn hidden_editor(&self) -> Option<Scintilla> {
		let window = HIDDEN_EDITOR.load(Ordering::Relaxed);
		if window != 0 {
			return Some(Scintilla::new(window as _));
		}
		let editor = self.notepad.create_hidden_scintilla()?;
		editor.use_utf8();
		editor.disable_undo();
		HIDDEN_EDITOR.store(editor.window() as isize, Ordering::Relaxed);
		Some(editor)
	}

	fn set_keywords(
		&mut self,
		editor: &Scintilla,
		language: &CodeLanguage,
		styles: &[StylerEntry],
	) {
		let mut keyword_sets: Vec<(usize, String)> = Vec::new();
		if language.keyword_sets == KeywordSets::Html {
			for (set, notepad_name) in HTML_KEYWORD_SOURCES {
				let keywords = language_keywords(self.langs_xml(), notepad_name);
				if let Some((_, keywords)) =
					keywords.into_iter().find(|(list, _)| list == "instre1")
				{
					keyword_sets.push((set, keywords));
				}
			}
		} else {
			for (list, keywords) in language_keywords(self.langs_xml(), language.notepad_name) {
				if let Some(set) = keyword_set_index(language.keyword_sets, &list) {
					keyword_sets.push((set, keywords));
				}
			}
		}
		// Keywords the user added in the Style Configurator.
		for style in styles {
			let Some(list) = &style.keyword_list else { continue };
			let Some(set) = keyword_set_index(language.keyword_sets, list) else { continue };
			if style.extra_keywords.trim().is_empty() {
				continue;
			}
			match keyword_sets.iter_mut().find(|(existing, _)| *existing == set) {
				Some((_, keywords)) => {
					keywords.push(' ');
					keywords.push_str(&style.extra_keywords);
				}
				None => keyword_sets.push((set, style.extra_keywords.clone())),
			}
		}
		for (set, keywords) in keyword_sets {
			editor.set_keywords(set, &keywords);
		}
	}

	fn langs_xml(&mut self) -> &str {
		let notepad = self.notepad;
		self.langs_xml.get_or_insert_with(|| read_configuration(&notepad, "langs"))
	}

	fn stylers_xml(&mut self) -> &str {
		let notepad = self.notepad;
		self.stylers_xml.get_or_insert_with(|| read_configuration(&notepad, "stylers"))
	}
}

/// `<settings dir>/<name>.xml`, or the `<name>.model.xml` Notepad++ ships with. Empty if
/// neither can be read. The code is then highlighted without keywords.
fn read_configuration(notepad: &Notepad, name: &str) -> String {
	let candidates: [Option<PathBuf>; 2] = [
		notepad.settings_directory().map(|directory| directory.join(format!("{name}.xml"))),
		notepad.program_directory().map(|directory| directory.join(format!("{name}.model.xml"))),
	];
	candidates
		.into_iter()
		.flatten()
		.find_map(|path| std::fs::read(path).ok())
		.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
		.unwrap_or_default()
}
