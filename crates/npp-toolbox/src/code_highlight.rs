//! The platform independent part of code block highlighting with Notepad++'s own lexers:
//! which lexer and keywords a code block language uses, and which GitHub color a lexer style
//! gets.

use std::collections::BTreeMap;

use crate::markdown_html::{HighlightedSpan, TokenKind};

/// How Notepad++ passes the keyword lists of `langs.xml` to a lexer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeywordSets {
	/// `instre1`, `instre2`, `type1`, ... `type7` are the sets 0 to 8.
	InOrder,
	/// The C++ lexer used for C like languages: `instre1` is set 0, `type1` set 1 and `instre2`
	/// set 3.
	CLike,
	/// HTML with embedded scripts: the HTML tags and the keywords of JavaScript and PHP.
	Html,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodeLanguage {
	/// The language name in `langs.xml` and `stylers.xml`.
	pub notepad_name: &'static str,
	/// The Lexilla lexer name for `NPPM_CREATELEXER`.
	pub lexer_name: &'static str,
	pub keyword_sets: KeywordSets,
	/// Lexer properties Notepad++ sets for the language.
	pub properties: &'static [(&'static str, &'static str)],
}

const fn language(
	notepad_name: &'static str,
	lexer_name: &'static str,
	keyword_sets: KeywordSets,
) -> CodeLanguage {
	CodeLanguage { notepad_name, lexer_name, keyword_sets, properties: &[] }
}

const BACKQUOTED_STRINGS: &[(&str, &str)] = &[("lexer.cpp.backquoted.strings", "1")];

/// The code block languages (as GitHub names them, lower case) that can be highlighted.
const LANGUAGES: &[(&[&str], CodeLanguage)] = {
	use KeywordSets::{CLike, Html, InOrder};
	&[
		(&["c", "h"], language("c", "cpp", CLike)),
		(&["cpp", "c++", "cc", "cxx", "hpp", "hxx", "h++", "ino"], language("cpp", "cpp", CLike)),
		(&["cs", "csharp", "c#"], language("cs", "cpp", CLike)),
		(&["java"], language("java", "cpp", CLike)),
		(&["objc", "objective-c", "objectivec"], language("objc", "cpp", CLike)),
		(&["swift"], language("swift", "cpp", CLike)),
		(&["rc"], language("rc", "cpp", CLike)),
		(
			&["js", "javascript", "jsx", "mjs", "cjs", "node"],
			CodeLanguage {
				properties: BACKQUOTED_STRINGS,
				..language("javascript.js", "cpp", CLike)
			},
		),
		(
			&["ts", "typescript", "tsx"],
			CodeLanguage { properties: BACKQUOTED_STRINGS, ..language("typescript", "cpp", CLike) },
		),
		(
			&["go", "golang"],
			CodeLanguage { properties: BACKQUOTED_STRINGS, ..language("go", "cpp", CLike) },
		),
		(&["rust", "rs"], language("rust", "rust", InOrder)),
		(&["python", "py", "py3", "python3", "pyw", "gyp"], language("python", "python", InOrder)),
		(&["bash", "sh", "shell", "zsh", "ksh", "shellscript"], language("bash", "bash", InOrder)),
		(
			&["powershell", "ps1", "pwsh", "posh", "psm1"],
			language("powershell", "powershell", InOrder),
		),
		(&["bat", "batch", "cmd", "dosbatch", "winbatch"], language("batch", "batch", InOrder)),
		(&["html", "htm", "xhtml"], language("html", "hypertext", Html)),
		(&["php"], language("php", "phpscript", Html)),
		(
			&["xml", "svg", "xsd", "xsl", "xslt", "plist", "rss", "csproj", "vcxproj", "xaml"],
			language("xml", "xml", InOrder),
		),
		(&["css"], language("css", "css", InOrder)),
		(&["sql", "mysql", "plsql", "tsql"], language("sql", "sql", InOrder)),
		(&["json", "jsonc", "geojson"], language("json", "json", InOrder)),
		(&["json5"], language("json5", "json", InOrder)),
		(&["yaml", "yml"], language("yaml", "yaml", InOrder)),
		(&["toml"], language("toml", "toml", InOrder)),
		(
			&["ini", "cfg", "dosini", "conf", "editorconfig", "gitconfig"],
			language("ini", "props", InOrder),
		),
		(&["properties"], language("props", "props", InOrder)),
		(&["lua"], language("lua", "lua", InOrder)),
		(&["perl", "pl", "pm"], language("perl", "perl", InOrder)),
		(&["raku", "perl6"], language("raku", "raku", InOrder)),
		(&["ruby", "rb", "gemfile", "rake"], language("ruby", "ruby", InOrder)),
		(&["r", "rscript", "splus"], language("r", "r", InOrder)),
		(&["makefile", "make", "mk", "mak", "gnumake"], language("makefile", "makefile", InOrder)),
		(&["cmake"], language("cmake", "cmake", InOrder)),
		(&["diff", "patch", "udiff"], language("diff", "diff", InOrder)),
		(&["latex", "tex"], language("latex", "latex", InOrder)),
		(&["matlab", "octave"], language("matlab", "matlab", InOrder)),
		(&["pascal", "delphi", "pas", "objectpascal"], language("pascal", "pascal", InOrder)),
		(&["fortran", "f90", "f95", "f03", "f08"], language("fortran", "fortran", InOrder)),
		(&["fortran77", "f77", "for"], language("fortran77", "f77", InOrder)),
		(&["haskell", "hs"], language("haskell", "haskell", InOrder)),
		(&["vb", "vbnet", "vb.net", "vbscript", "vbs", "vba"], language("vb", "vb", InOrder)),
		(&["asm", "assembly", "nasm", "x86asm", "masm"], language("asm", "asm", InOrder)),
		(&["tcl"], language("tcl", "tcl", InOrder)),
		(&["nim"], language("nim", "nim", InOrder)),
		(&["erlang", "erl"], language("erlang", "erlang", InOrder)),
		(&["d"], language("d", "d", InOrder)),
		(&["lisp", "elisp", "emacs-lisp", "common-lisp"], language("lisp", "lisp", InOrder)),
		(&["scheme", "scm", "racket"], language("scheme", "lisp", InOrder)),
		(&["ada", "adb", "ads"], language("ada", "ada", InOrder)),
		(&["verilog", "v", "systemverilog", "sv"], language("verilog", "verilog", InOrder)),
		(&["vhdl"], language("vhdl", "vhdl", InOrder)),
		(&["nsis"], language("nsis", "nsis", InOrder)),
		(&["inno", "iss"], language("inno", "inno", InOrder)),
		(&["autoit", "au3"], language("autoit", "au3", InOrder)),
		(&["coffeescript", "coffee"], language("coffeescript", "coffeescript", InOrder)),
		(&["gdscript", "gd"], language("gdscript", "gdscript", InOrder)),
		(&["postscript", "postscr", "eps"], language("postscript", "ps", InOrder)),
		(&["smalltalk", "st"], language("smalltalk", "smalltalk", InOrder)),
		(&["fsharp", "fs", "f#"], language("fsharp", "fsharp", InOrder)),
		(&["ocaml", "ml", "caml"], language("caml", "caml", InOrder)),
		(&["cobol", "cob", "cbl"], language("cobol", "COBOL", InOrder)),
		(&["forth"], language("forth", "forth", InOrder)),
		(&["registry", "reg"], language("registry", "registry", InOrder)),
		(&["hollywood"], language("hollywood", "hollywood", InOrder)),
		(&["purebasic", "pb"], language("purebasic", "purebasic", InOrder)),
		(&["freebasic", "fb", "bas"], language("freebasic", "freebasic", InOrder)),
		(&["sas"], language("sas", "sas", InOrder)),
		(&["actionscript", "as"], language("actionscript", "cpp", CLike)),
		(&["kixtart", "kix"], language("kix", "kix", InOrder)),
		(&["visualprolog", "prolog"], language("visualprolog", "visualprolog", InOrder)),
	]
};

/// The Notepad++ language for a code block info string such as `rust` or `C++`.
pub fn code_language(info_language: &str) -> Option<CodeLanguage> {
	let info_language = info_language.trim_start_matches('{').trim_end_matches('}');
	let info_language = info_language.strip_prefix('.').unwrap_or(info_language);
	LANGUAGES.iter().find_map(|(aliases, language)| {
		aliases.iter().any(|alias| alias.eq_ignore_ascii_case(info_language)).then_some(*language)
	})
}

/// The code block info string that [`code_language`] maps to the Notepad++ language
/// `notepad_name` (`langs.xml` name, e.g. `javascript.js`).
pub fn info_language_of(notepad_name: &str) -> Option<&'static str> {
	LANGUAGES
		.iter()
		.find(|(_, language)| language.notepad_name.eq_ignore_ascii_case(notepad_name))
		.and_then(|(aliases, _)| aliases.first().copied())
}

/// The keyword set index Notepad++ passes a `langs.xml` keyword list to (`SCI_SETKEYWORDS`).
pub fn keyword_set_index(sets: KeywordSets, list_name: &str) -> Option<usize> {
	let index = match list_name {
		"instre1" => 0,
		"instre2" => 1,
		"type1" => 2,
		"type2" => 3,
		"type3" => 4,
		"type4" => 5,
		"type5" => 6,
		"type6" => 7,
		"type7" => 8,
		_ => list_name.parse().ok().filter(|index| *index <= 8)?,
	};
	match sets {
		KeywordSets::InOrder | KeywordSets::Html => Some(index),
		KeywordSets::CLike => match index {
			0 => Some(0),
			2 => Some(1),
			1 => Some(3),
			_ => None,
		},
	}
}

/// The languages whose keywords the HTML lexer uses for embedded scripts, with their
/// keyword set index.
const HTML_KEYWORD_SOURCES: [(usize, &str); 3] = [(0, "html"), (1, "javascript"), (4, "php")];

/// The keyword sets Notepad++ passes to the lexer of `language` (`SCI_SETKEYWORDS`): the lists
/// of `langs.xml` and the keywords the user added to `styles` in the Style Configurator.
pub fn keyword_sets(
	language: &CodeLanguage,
	langs_xml: &str,
	styles: &[StylerEntry],
) -> Vec<(usize, String)> {
	let mut keyword_sets: Vec<(usize, String)> = Vec::new();
	if language.keyword_sets == KeywordSets::Html {
		for (set, notepad_name) in HTML_KEYWORD_SOURCES {
			let keywords = language_keywords(langs_xml, notepad_name);
			if let Some((_, keywords)) = keywords.into_iter().find(|(list, _)| list == "instre1") {
				keyword_sets.push((set, keywords));
			}
		}
	} else {
		for (list, keywords) in language_keywords(langs_xml, language.notepad_name) {
			if let Some(set) = keyword_set_index(language.keyword_sets, &list) {
				keyword_sets.push((set, keywords));
			}
		}
	}
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
	keyword_sets
}

/// The `stylers.xml` lexers whose styles a language's lexer uses: the HTML lexer also styles
/// embedded JavaScript and PHP, which Notepad++ configures as languages of their own.
pub fn styler_names(language: &CodeLanguage) -> Vec<&'static str> {
	let mut names = vec![language.notepad_name];
	if language.keyword_sets == KeywordSets::Html {
		for name in ["html", "javascript.embedded", "php"] {
			if !names.contains(&name) {
				names.push(name);
			}
		}
	}
	names
}

/// The theme file Notepad++ uses, from the `DarkMode` settings of its `config.xml`: the name of
/// a file in its `themes` directory, or `None` for `stylers.xml`.
pub fn theme_file_name(config_xml: &str, is_dark_mode: bool) -> Option<String> {
	const DEFAULT_DARK_THEME: &str = "DarkModeDefault.xml";
	let dark_mode_settings = XmlItems::new(config_xml).find_map(|item| match item {
		XmlItem::Start { name: "GUIConfig", attributes, .. }
			if attribute(attributes, "name").as_deref() == Some("DarkMode") =>
		{
			Some(attributes)
		}
		_ => None,
	});
	let name = match (dark_mode_settings, is_dark_mode) {
		(Some(attributes), true) => attribute(attributes, "darkThemeName")
			.filter(|name| !name.is_empty())
			.unwrap_or_else(|| DEFAULT_DARK_THEME.to_owned()),
		(Some(attributes), false) => attribute(attributes, "lightThemeName").unwrap_or_default(),
		(None, true) => DEFAULT_DARK_THEME.to_owned(),
		(None, false) => String::new(),
	};
	Some(name).filter(|name| !name.is_empty() && !name.eq_ignore_ascii_case("stylers.xml"))
}

/// The GitHub token kind for a lexer style, from its Lexilla tags (`SCI_TAGSOFSTYLE`, e.g.
/// `literal string`) or its name in `stylers.xml` (e.g. `COMMENT LINE`).
pub fn token_kind_of_style(description: &str) -> Option<TokenKind> {
	let description = description.to_ascii_lowercase();
	let has = |word: &str| description.split([' ', '_', '-']).any(|part| part == word);
	let contains = |text: &str| description.contains(text);
	Some(if contains("comment") || has("doc") || has("docstring") {
		TokenKind::Comment
	} else if has("deleted") || has("removed") {
		TokenKind::DiffDeleted
	} else if has("added") || has("inserted") {
		TokenKind::DiffInserted
	} else if has("position") || has("changed") {
		TokenKind::DiffRange
	} else if has("header") {
		TokenKind::DiffHeader
	} else if contains("string")
		|| contains("char")
		|| contains("regex")
		|| has("verbatim")
		|| has("heredoc")
		|| has("backticks")
		|| has("triple")
		|| has("value") && !has("key")
		|| has("cdata")
	{
		TokenKind::String
	} else if contains("number") || contains("numeric") || has("constant") || has("const") {
		TokenKind::Number
	} else if contains("preprocessor") || has("directive") || has("pragma") || has("macro") {
		TokenKind::Keyword
	} else if has("attribute") || has("property") || has("key") || has("parameter") {
		TokenKind::Attribute
	} else if has("tag") || has("element") || has("section") {
		TokenKind::Tag
	} else if has("type") || has("class") || has("lifetime") || has("builtin") {
		TokenKind::Type
	} else if has("function") || has("method") || has("def") || has("label") {
		TokenKind::Function
	} else if has("variable") || has("var") || has("scalar") || has("array") || has("hash") {
		TokenKind::Variable
	} else if contains("keyword")
		|| contains("word")
		|| has("instruction")
		|| has("statement")
		|| has("command")
		|| has("operator") && has("word")
	{
		TokenKind::Keyword
	} else {
		return None;
	})
}

/// Spans of equally styled bytes, with the token kind of their style.
pub fn spans_of_styles(
	styles: &[u8],
	mut kind_of_style: impl FnMut(u8) -> Option<TokenKind>,
) -> Vec<HighlightedSpan> {
	let mut kinds: BTreeMap<u8, Option<TokenKind>> = BTreeMap::new();
	let mut spans: Vec<HighlightedSpan> = Vec::new();
	let mut start = 0;
	while start < styles.len() {
		let style = styles[start];
		let length = styles[start..].iter().take_while(|&&next| next == style).count();
		let end = start + length;
		let kind = *kinds.entry(style).or_insert_with(|| kind_of_style(style));
		if let Some(kind) = kind {
			match spans.last_mut() {
				Some(previous) if previous.kind == kind && previous.range.end == start => {
					previous.range.end = end;
				}
				_ => spans.push(HighlightedSpan { range: start..end, kind }),
			}
		}
		start = end;
	}
	spans
}

/// Keyword lists of a language in Notepad++'s `langs.xml`: `(list name, keywords)`.
pub fn language_keywords(langs_xml: &str, notepad_name: &str) -> Vec<(String, String)> {
	let mut keywords = Vec::new();
	let mut is_in_language = false;
	let mut current_list: Option<String> = None;
	for item in XmlItems::new(langs_xml) {
		match item {
			XmlItem::Start { name: "Language", attributes, .. } => {
				is_in_language = attribute(attributes, "name").as_deref() == Some(notepad_name);
			}
			XmlItem::End("Language") => is_in_language = false,
			XmlItem::Start { name: "Keywords", attributes, is_empty: false } if is_in_language => {
				current_list = attribute(attributes, "name");
			}
			XmlItem::Text(text) => {
				if let Some(list) = current_list.take() {
					keywords.push((list, decode_xml_text(text)));
				}
			}
			XmlItem::End("Keywords") => current_list = None,
			_ => {}
		}
	}
	keywords
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StylerEntry {
	pub style_id: u8,
	pub name: String,
	/// The `langs.xml` keyword list this style colors (`keywordClass`), if any.
	pub keyword_list: Option<String>,
	/// User defined keywords added to that list.
	pub extra_keywords: String,
	/// The text color (`0xRRGGBB`), unless the style takes the default one (`colorStyle`).
	pub color: Option<u32>,
	/// `fontStyle`: 1 bold, 2 italic, 4 underlined.
	pub font_style: u8,
}

/// Notepad++'s `COLORSTYLE_FOREGROUND` bit of `colorStyle`.
const COLOR_STYLE_FOREGROUND: u32 = 1;

/// The styles of a language's lexer in Notepad++'s `stylers.xml`.
pub fn lexer_styles(stylers_xml: &str, notepad_name: &str) -> Vec<StylerEntry> {
	let mut styles = Vec::new();
	let mut is_in_lexer = false;
	let mut pending: Option<StylerEntry> = None;
	for item in XmlItems::new(stylers_xml) {
		match item {
			XmlItem::Start { name: "LexerType", attributes, .. } => {
				is_in_lexer = attribute(attributes, "name").as_deref() == Some(notepad_name);
			}
			XmlItem::End("LexerType") => is_in_lexer = false,
			XmlItem::Start { name: "WordsStyle", attributes, is_empty } if is_in_lexer => {
				let style_id = attribute(attributes, "styleID").and_then(|id| id.parse().ok());
				let Some(style_id) = style_id else { continue };
				let color_style =
					attribute(attributes, "colorStyle").and_then(|bits| bits.parse().ok());
				let has_color =
					color_style.is_none_or(|bits: u32| bits & COLOR_STYLE_FOREGROUND != 0);
				let entry = StylerEntry {
					style_id,
					name: attribute(attributes, "name").unwrap_or_default(),
					keyword_list: attribute(attributes, "keywordClass"),
					extra_keywords: String::new(),
					color: attribute(attributes, "fgColor")
						.and_then(|color| u32::from_str_radix(color.trim(), 16).ok())
						.filter(|_| has_color),
					font_style: attribute(attributes, "fontStyle")
						.and_then(|bits| bits.trim().parse().ok())
						.unwrap_or(0),
				};
				if is_empty {
					styles.push(entry);
				} else {
					pending = Some(entry);
				}
			}
			XmlItem::Text(text) => {
				if let Some(entry) = &mut pending {
					entry.extra_keywords = decode_xml_text(text);
				}
			}
			XmlItem::End("WordsStyle") => styles.extend(pending.take()),
			_ => {}
		}
	}
	styles
}

#[derive(Debug, PartialEq, Eq)]
enum XmlItem<'a> {
	Start { name: &'a str, attributes: &'a str, is_empty: bool },
	End(&'a str),
	Text(&'a str),
}

/// A minimal reader for the well-formed configuration files of Notepad++. Comments,
/// declarations and processing instructions are skipped.
struct XmlItems<'a> {
	rest: &'a str,
}

impl<'a> XmlItems<'a> {
	fn new(xml: &'a str) -> Self {
		Self { rest: xml }
	}
}

impl<'a> Iterator for XmlItems<'a> {
	type Item = XmlItem<'a>;

	fn next(&mut self) -> Option<XmlItem<'a>> {
		loop {
			if self.rest.is_empty() {
				return None;
			}
			if !self.rest.starts_with('<') {
				let end = self.rest.find('<').unwrap_or(self.rest.len());
				let (text, rest) = self.rest.split_at(end);
				self.rest = rest;
				if !text.trim().is_empty() {
					return Some(XmlItem::Text(text));
				}
				continue;
			}
			let skipped = [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>"), ("<!", ">")]
				.into_iter()
				.find(|(start, _)| self.rest.starts_with(start));
			if let Some((_, end)) = skipped {
				let length = self.rest.find(end).map_or(self.rest.len(), |index| index + end.len());
				self.rest = &self.rest[length..];
				continue;
			}
			let Some(tag_end) = tag_end(self.rest) else {
				self.rest = "";
				return None;
			};
			let tag = &self.rest[1..tag_end];
			self.rest = &self.rest[tag_end + 1..];
			if let Some(name) = tag.strip_prefix('/') {
				return Some(XmlItem::End(name.trim()));
			}
			let is_empty = tag.ends_with('/');
			let tag = tag.trim_end_matches('/');
			let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
			let (name, attributes) = tag.split_at(name_end);
			return Some(XmlItem::Start { name, attributes, is_empty });
		}
	}
}

/// Index of the `>` closing the tag at the start of `xml`, skipping quoted attribute values.
fn tag_end(xml: &str) -> Option<usize> {
	let mut quote = None;
	for (index, character) in xml.char_indices() {
		match (quote, character) {
			(None, '"' | '\'') => quote = Some(character),
			(Some(open), _) if character == open => quote = None,
			(None, '>') => return Some(index),
			_ => {}
		}
	}
	None
}

fn attribute(attributes: &str, name: &str) -> Option<String> {
	let mut rest = attributes;
	while let Some(equals) = rest.find('=') {
		let attribute_name = rest[..equals].trim();
		let after = rest[equals + 1..].trim_start();
		let quote = after.chars().next().filter(|quote| *quote == '"' || *quote == '\'')?;
		let value_end = after[1..].find(quote)? + 1;
		if attribute_name == name {
			return Some(decode_xml_text(&after[1..value_end]));
		}
		rest = &after[value_end + 1..];
	}
	None
}

fn decode_xml_text(text: &str) -> String {
	if !text.contains('&') {
		return text.to_owned();
	}
	text.replace("&lt;", "<")
		.replace("&gt;", ">")
		.replace("&quot;", "\"")
		.replace("&apos;", "'")
		.replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn finds_languages_by_github_names() {
		assert_eq!(code_language("Rust").map(|language| language.lexer_name), Some("rust"));
		assert_eq!(code_language("c++").map(|language| language.notepad_name), Some("cpp"));
		assert_eq!(code_language("{.py}").map(|language| language.notepad_name), Some("python"));
		assert_eq!(
			code_language("js").map(|language| language.properties),
			Some(BACKQUOTED_STRINGS)
		);
		assert_eq!(code_language("text"), None);
		assert_eq!(code_language("mermaid"), None);
	}

	#[test]
	fn finds_info_languages_of_notepad_languages() {
		assert_eq!(info_language_of("javascript.js"), Some("js"));
		assert_eq!(info_language_of("props"), Some("properties"));
		assert_eq!(info_language_of("normal"), None);
		for (_, language) in LANGUAGES {
			let info_language = info_language_of(language.notepad_name).expect("info language");
			assert_eq!(code_language(info_language), Some(*language), "{}", language.notepad_name);
		}
	}

	#[test]
	fn aliases_are_unique() {
		let mut aliases: Vec<&str> =
			LANGUAGES.iter().flat_map(|(aliases, _)| aliases.iter().copied()).collect();
		let count = aliases.len();
		aliases.sort_unstable();
		aliases.dedup();
		assert_eq!(aliases.len(), count);
	}

	#[test]
	fn maps_keyword_lists_like_notepad_plus_plus() {
		assert_eq!(keyword_set_index(KeywordSets::InOrder, "instre2"), Some(1));
		assert_eq!(keyword_set_index(KeywordSets::InOrder, "type1"), Some(2));
		assert_eq!(keyword_set_index(KeywordSets::CLike, "type1"), Some(1));
		assert_eq!(keyword_set_index(KeywordSets::CLike, "instre2"), Some(3));
		assert_eq!(keyword_set_index(KeywordSets::CLike, "type2"), None);
		assert_eq!(keyword_set_index(KeywordSets::InOrder, "5"), Some(5));
		assert_eq!(keyword_set_index(KeywordSets::InOrder, "substyle1"), None);
	}

	#[test]
	fn classifies_lexilla_tags_and_styler_names() {
		let cases = [
			("comment documentation", Some(TokenKind::Comment)),
			("COMMENT LINE", Some(TokenKind::Comment)),
			("literal string", Some(TokenKind::String)),
			("CHARACTER", Some(TokenKind::String)),
			("literal numeric", Some(TokenKind::Number)),
			("keyword", Some(TokenKind::Keyword)),
			("INSTRUCTION WORD", Some(TokenKind::Keyword)),
			("TYPE WORD", Some(TokenKind::Type)),
			("PREPROCESSOR", Some(TokenKind::Keyword)),
			("TAG", Some(TokenKind::Tag)),
			("ATTRIBUTE", Some(TokenKind::Attribute)),
			("DELETED", Some(TokenKind::DiffDeleted)),
			("ADDED", Some(TokenKind::DiffInserted)),
			("operator", None),
			("identifier", None),
			("DEFAULT", None),
		];
		for (description, expected) in cases {
			assert_eq!(token_kind_of_style(description), expected, "{description}");
		}
	}

	#[test]
	fn merges_styles_into_spans() {
		let styles = [0, 5, 5, 6, 6, 1, 1, 0, 5];
		let spans = spans_of_styles(&styles, |style| match style {
			5 | 6 => Some(TokenKind::Keyword),
			1 => Some(TokenKind::String),
			_ => None,
		});
		assert_eq!(
			spans,
			[
				HighlightedSpan { range: 1..5, kind: TokenKind::Keyword },
				HighlightedSpan { range: 5..7, kind: TokenKind::String },
				HighlightedSpan { range: 8..9, kind: TokenKind::Keyword },
			]
		);
	}

	#[test]
	fn reads_keywords_from_langs_xml() {
		let langs = r#"<?xml version="1.0"?>
<NotepadPlus>
	<Languages>
		<!-- <Language name="rust"> in a comment -->
		<Language name="python" ext="py">
			<Keywords name="instre1">and as</Keywords>
		</Language>
		<Language name="rust" ext="rs" commentLine="//">
			<Keywords name="instre1">fn let &amp;mut</Keywords>
			<Keywords name="type1">u8 i32</Keywords>
			<Keywords name="type2"></Keywords>
		</Language>
	</Languages>
</NotepadPlus>"#;
		assert_eq!(
			language_keywords(langs, "rust"),
			[
				("instre1".to_owned(), "fn let &mut".to_owned()),
				("type1".to_owned(), "u8 i32".to_owned())
			]
		);
		assert!(language_keywords(langs, "lua").is_empty());
	}

	#[test]
	fn reads_styles_from_stylers_xml() {
		let stylers = r#"<NotepadPlus><LexerStyles>
	<LexerType name="cpp" desc="C++" ext="">
		<WordsStyle name="PREPROCESSOR" styleID="9" fgColor="804000" />
		<WordsStyle name="INSTRUCTION WORD" styleID="5" fgColor="0000FF" colorStyle="2" fontStyle="1" keywordClass="instre1">my_keyword</WordsStyle>
		<WordsStyle name="BROKEN" styleID="x" />
	</LexerType>
	<LexerType name="c"><WordsStyle name="OTHER" styleID="1" /></LexerType>
</LexerStyles></NotepadPlus>"#;
		assert_eq!(
			lexer_styles(stylers, "cpp"),
			[
				StylerEntry {
					style_id: 9,
					name: "PREPROCESSOR".to_owned(),
					color: Some(0x804000),
					..StylerEntry::default()
				},
				StylerEntry {
					style_id: 5,
					name: "INSTRUCTION WORD".to_owned(),
					keyword_list: Some("instre1".to_owned()),
					extra_keywords: "my_keyword".to_owned(),
					font_style: 1,
					..StylerEntry::default()
				},
			]
		);
	}

	#[test]
	fn combines_keywords_of_langs_xml_and_the_style_configurator() {
		let langs = r#"<Language name="rust"><Keywords name="instre1">fn</Keywords><Keywords name="type1">u8</Keywords></Language>
<Language name="html"><Keywords name="instre1">div</Keywords></Language>
<Language name="javascript"><Keywords name="instre1">var</Keywords></Language>"#;
		let user_keywords = StylerEntry {
			keyword_list: Some("instre1".to_owned()),
			extra_keywords: "my".to_owned(),
			..StylerEntry::default()
		};
		let rust = code_language("rust").expect("rust");
		assert_eq!(
			keyword_sets(&rust, langs, &[user_keywords]),
			[(0, "fn my".to_owned()), (2, "u8".to_owned())]
		);
		let html = code_language("html").expect("html");
		assert_eq!(keyword_sets(&html, langs, &[]), [(0, "div".to_owned()), (1, "var".to_owned())]);
		assert_eq!(styler_names(&html), ["html", "javascript.embedded", "php"]);
		assert_eq!(styler_names(&rust), ["rust"]);
	}

	#[test]
	fn finds_the_theme_of_the_mode() {
		let config = r#"<NotepadPlus><GUIConfigs>
	<GUIConfig name="DarkMode" enable="yes" darkThemeName="Monokai.xml" lightThemeName="" />
</GUIConfigs></NotepadPlus>"#;
		assert_eq!(theme_file_name(config, true).as_deref(), Some("Monokai.xml"));
		assert_eq!(theme_file_name(config, false), None);
		let config = r#"<GUIConfig name="DarkMode" darkThemeName="" lightThemeName="Solarized-light.xml" />"#;
		assert_eq!(theme_file_name(config, true).as_deref(), Some("DarkModeDefault.xml"));
		assert_eq!(theme_file_name(config, false).as_deref(), Some("Solarized-light.xml"));
		assert_eq!(theme_file_name("", false), None);
		let stylers = r#"<GUIConfig name="DarkMode" lightThemeName="stylers.xml"/>"#;
		assert_eq!(theme_file_name(stylers, false), None);
	}

	#[test]
	fn survives_broken_xml() {
		for xml in
			["<", "<Language name=\"rust", "<!--", "<Language name='rust'><Keywords name=", "</>"]
		{
			assert!(language_keywords(xml, "rust").is_empty(), "{xml}");
			assert!(lexer_styles(xml, "rust").is_empty(), "{xml}");
		}
	}
}
