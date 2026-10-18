//! The page and the scripts of the browser viewers, and what all viewers share: resolving
//! links and mapping editor lines to RichEdit positions.
//!
//! The page itself runs no scripts. Its content security policy only allows inline styles and
//! `data:` images, so nothing is loaded from elsewhere. The viewer changes it with its own scripts
//! through WebView2's `ExecuteScript` or EdgeHTML's `InvokeScriptAsync("eval")`, which the policy
//! does not restrict.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::image_embedding::{local_file_path, uri_scheme};
use crate::markdown_html::{DARK_STYLE_SHEET, LIGHT_STYLE_SHEET};
use crate::markdown_rtf::SourceBlock;

/// The empty page whose `#content` receives the rendered Markdown with [`content_script`].
pub fn shell_page(is_dark: bool) -> String {
	let (style_sheet, color_scheme, background, selection) = match is_dark {
		true => (DARK_STYLE_SHEET, "dark", "#0d1117", "rgba(56,139,253,0.18)"),
		false => (LIGHT_STYLE_SHEET, "light", "#ffffff", "rgba(84,174,255,0.22)"),
	};
	let mut page = String::with_capacity(style_sheet.len() + 1024);
	page.push_str(concat!(
		"<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n",
		"<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'\">\n"
	));
	page.push_str("<base>\n<style>\n");
	page.push_str(style_sheet);
	page.push_str(&format!(
		"\n:root{{color-scheme:{color_scheme}}}body{{margin:0;background-color:{background}}}\
		.markdown-body{{box-sizing:border-box;min-width:0;padding:16px 20px}}\
		.npp-selected{{background-color:{selection};border-radius:4px}}\n"
	));
	page.push_str(
		"</style>\n</head>\n<body>\n<article class=\"markdown-body\" id=\"content\"></article>\n</body>\n</html>\n",
	);
	page
}

/// Replaces the content of the [`shell_page`]. `base_url` (see [`link_base_url`]) resolves
/// relative links, so that clicking them reports the full path.
pub fn content_script(body_html: &str, base_url: &str) -> String {
	format!(
		"document.querySelector('base').href={};\
		document.getElementById('content').innerHTML={};",
		json_string(base_url),
		json_string(body_html)
	)
}

/// Scrolls the block at `caret_line` (interpolated within the block) to a third of the view and
/// marks the innermost blocks of `selected_lines`. Blocks carry their line in `data-line`.
pub fn sync_script(caret_line: usize, selected_lines: Option<Range<usize>>) -> String {
	let (first, last) = selected_lines.map_or((1, 0), |lines| (lines.start, lines.end));
	format!(
		"(function(caret,first,last){{\
		var blocks=Array.prototype.slice.call(document.querySelectorAll('[data-line]'));\
		var lineOf=function(e){{return +e.getAttribute('data-line');}};\
		{CLEAR_SELECTION_SCRIPT}\
		if(first<=last){{\
		var from=-1;blocks.forEach(function(e){{var l=lineOf(e);if(l<=first&&l>from)from=l;}});\
		if(from<0)from=first;\
		var selected=blocks.filter(function(e){{var l=lineOf(e);return l>=from&&l<=last;}});\
		selected.forEach(function(e){{if(!selected.some(function(c){{return c!==e&&e.contains(c);}}))e.classList.add('npp-selected');}});\
		}}\
		var index=-1;blocks.forEach(function(e,i){{if(lineOf(e)<=caret)index=i;}});\
		if(index<0){{window.scrollTo(0,0);return;}}\
		var target=blocks[index],line=lineOf(target),box=target.getBoundingClientRect(),y=box.top;\
		for(var i=index+1;i<blocks.length;i++){{var next=lineOf(blocks[i]);if(next>line){{\
		y=box.top+(blocks[i].getBoundingClientRect().top-box.top)*Math.min(1,(caret-line)/(next-line));break;}}}}\
		window.scrollBy(0,y-window.innerHeight/3);\
		}})({caret_line},{first},{last});"
	)
}

/// Removes the marks of [`sync_script`].
pub const CLEAR_SELECTION_SCRIPT: &str = "document.querySelectorAll('.npp-selected').forEach(function(e){e.classList.remove('npp-selected');});";

/// Scrolls to the element with the id `anchor`.
pub fn anchor_script(anchor: &str) -> String {
	format!(
		"(function(e){{if(e)e.scrollIntoView();}})(document.getElementById({}));",
		json_string(anchor)
	)
}

/// Makes the page report right clicks and the context menu key with `window.external.notify` as
/// a [`ContextMenuRequest`], for EdgeHTML, which shows no context menu itself. Listeners added by
/// the host's scripts run despite the content security policy.
pub const CONTEXT_MENU_SCRIPT: &str = "if(!window.nppContextMenu){window.nppContextMenu=true;\
	document.addEventListener('contextmenu',function(e){e.preventDefault();\
	var s=window.getSelection(),t=s.toString(),d=document.createElement('div');\
	for(var i=0;t&&i<s.rangeCount;i++)d.appendChild(s.getRangeAt(i).cloneContents());\
	var a=e.target;while(a&&typeof a.href!=='string')a=a.parentElement;\
	window.external.notify(JSON.stringify(['context-menu',t,d.innerHTML,a?a.href:'']));});}";

/// Makes the page report the link under the mouse with `window.external.notify` as
/// `["link-hover", href]` (see [`parse_hovered_link`]) whenever it changes, an empty `href` when
/// the mouse leaves links or the page. For EdgeHTML, which has no status bar event.
pub const LINK_HOVER_SCRIPT: &str = "if(window.nppHoveredLink===undefined){window.nppHoveredLink='';\
	var report=function(h){if(h!==window.nppHoveredLink){window.nppHoveredLink=h;\
	window.external.notify(JSON.stringify(['link-hover',h]));}};\
	document.addEventListener('mouseover',function(e){\
	var a=e.target;while(a&&typeof a.href!=='string')a=a.parentElement;report(a?a.href:'');});\
	document.addEventListener('mouseout',function(e){if(!e.relatedTarget)report('');});}";

/// The absolute URL sent by [`LINK_HOVER_SCRIPT`], empty when no link is hovered.
pub fn parse_hovered_link(message: &str) -> Option<String> {
	let mut strings = parse_json_strings(message)?.into_iter();
	if strings.next()? != "link-hover" {
		return None;
	}
	let link = strings.next()?;
	strings.next().is_none().then_some(link)
}

/// The URL of the hovered link in WebView2's status bar text. Chromium may leave out the
/// `http://` of the address it shows.
pub fn status_text_link(text: &str) -> String {
	let text = text.trim();
	match text.strip_prefix(&LINK_HOST_URL["http://".len()..]) {
		Some(path) => format!("{LINK_HOST_URL}{path}"),
		None => text.to_owned(),
	}
}

/// Selects the whole content.
pub const SELECT_ALL_SCRIPT: &str =
	"window.getSelection().selectAllChildren(document.getElementById('content'));";

/// What [`CONTEXT_MENU_SCRIPT`] reports.
#[derive(Debug, PartialEq, Eq)]
pub struct ContextMenuRequest {
	pub selected_text: String,
	/// The selected elements, styled by the page's style sheet.
	pub selected_html: String,
	/// The absolute URL of the link under the mouse, empty without one.
	pub link: String,
}

impl ContextMenuRequest {
	/// Parses the JSON array of strings sent by [`CONTEXT_MENU_SCRIPT`].
	pub fn parse(message: &str) -> Option<Self> {
		let mut strings = parse_json_strings(message)?.into_iter();
		if strings.next()? != "context-menu" {
			return None;
		}
		let request = ContextMenuRequest {
			selected_text: strings.next()?,
			selected_html: strings.next()?,
			link: strings.next()?,
		};
		strings.next().is_none().then_some(request)
	}
}

/// The strings of a JSON array that contains only strings. Unpaired surrogates become U+FFFD.
fn parse_json_strings(json: &str) -> Option<Vec<String>> {
	let mut characters = json.trim().chars().peekable();
	let mut strings = Vec::new();
	if characters.next()? != '[' {
		return None;
	}
	if characters.peek() == Some(&']') {
		characters.next();
		return characters.next().is_none().then_some(strings);
	}
	loop {
		if characters.next()? != '"' {
			return None;
		}
		let mut code_units = Vec::new();
		loop {
			let character = match characters.next()? {
				'"' => break,
				'\\' => match characters.next()? {
					'"' => '"',
					'\\' => '\\',
					'/' => '/',
					'b' => '\u{8}',
					'f' => '\u{c}',
					'n' => '\n',
					'r' => '\r',
					't' => '\t',
					'u' => {
						let digits: String = characters.by_ref().take(4).collect();
						if digits.len() != 4 {
							return None;
						}
						code_units.push(u16::from_str_radix(&digits, 16).ok()?);
						continue;
					}
					_ => return None,
				},
				character => character,
			};
			let mut buffer = [0; 2];
			code_units.extend_from_slice(character.encode_utf16(&mut buffer));
		}
		strings.push(char::decode_utf16(code_units).map(|c| c.unwrap_or('\u{fffd}')).collect());
		match characters.next()? {
			',' => {}
			']' => break,
			_ => return None,
		}
	}
	characters.next().is_none().then_some(strings)
}

/// A JavaScript string literal of `text`.
pub fn json_string(text: &str) -> String {
	let mut literal = String::with_capacity(text.len() + text.len() / 8 + 2);
	literal.push('"');
	for character in text.chars() {
		match character {
			'"' => literal.push_str("\\\""),
			'\\' => literal.push_str("\\\\"),
			'\n' => literal.push_str("\\n"),
			'\r' => literal.push_str("\\r"),
			'\t' => literal.push_str("\\t"),
			// Line terminators in older JavaScript, and controls.
			'\u{2028}' | '\u{2029}' | '\0'..='\u{1f}' => {
				literal.push_str(&format!("\\u{:04x}", u32::from(character)));
			}
			character => literal.push(character),
		}
	}
	literal.push('"');
	literal
}

/// The host of [`link_base_url`]. `.invalid` never resolves, and WebView2 reports navigations to
/// it, unlike navigations to `file:` URLs, which it blocks silently in a page from a string.
const LINK_HOST_URL: &str = "http://npp-toolbox.invalid/";

/// The base URL for the links of a document in `directory`: its path below [`LINK_HOST_URL`],
/// e.g. `http://npp-toolbox.invalid/C:/docs/`. Relative links, also with `..`, resolve to the
/// path of their file.
pub fn link_base_url(directory: Option<&Path>) -> String {
	let mut url = String::from(LINK_HOST_URL);
	let Some(directory) = directory else { return url };
	let text = directory.to_string_lossy().replace('\\', "/");
	for byte in text.trim_matches('/').bytes() {
		match byte {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
				url.push(char::from(byte));
			}
			byte => url.push_str(&format!("%{byte:02X}")),
		}
	}
	url.push('/');
	url
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
	/// A heading id in the document.
	Anchor(String),
	/// A web page or e-mail address for the default application.
	External(String),
	File(PathBuf),
	/// Scripts, data and unknown schemes.
	Ignored,
}

/// What a link clicked in a browser refers to, resolved against `base_url` of
/// [`link_base_url`].
pub fn resolved_link_target(uri: &str, base_url: &str) -> LinkTarget {
	let uri = uri.trim();
	if let Some(anchor) = uri.strip_prefix(base_url).and_then(|rest| rest.strip_prefix('#')) {
		return LinkTarget::Anchor(anchor.to_owned());
	}
	match uri.strip_prefix(LINK_HOST_URL) {
		// The absolute path of a relative link, e.g. `C:/docs/other.md`.
		Some(path) => local_file_path(path, None).map_or(LinkTarget::Ignored, LinkTarget::File),
		None => link_target(uri, None),
	}
}

/// The text "Copy Link" puts on the clipboard for a link reported by a browser, see
/// [`target_location`].
pub fn link_location(uri: &str, base_url: &str) -> Option<String> {
	target_location(resolved_link_target(uri, base_url))
}

/// The text "Copy Link" puts on the clipboard: the address, the path of a local file or the
/// `#anchor`. `None` for links that lead nowhere.
pub fn target_location(target: LinkTarget) -> Option<String> {
	match target {
		LinkTarget::Anchor(anchor) => Some(format!("#{anchor}")),
		LinkTarget::External(url) => Some(url),
		LinkTarget::File(path) => Some(path.to_string_lossy().replace('/', "\\")),
		LinkTarget::Ignored => None,
	}
}

/// What a link written in the Markdown refers to.
pub fn link_target(link: &str, document_directory: Option<&Path>) -> LinkTarget {
	let link = link.trim();
	if let Some(anchor) = link.strip_prefix('#') {
		return LinkTarget::Anchor(anchor.to_owned());
	}
	match uri_scheme(link).map(str::to_ascii_lowercase).as_deref() {
		Some("http" | "https" | "mailto" | "ftp") => LinkTarget::External(link.to_owned()),
		Some("file") | None => {
			local_file_path(link, document_directory).map_or(LinkTarget::Ignored, LinkTarget::File)
		}
		// A Windows drive letter looks like a one letter scheme.
		Some(scheme) if scheme.len() == 1 => {
			local_file_path(link, document_directory).map_or(LinkTarget::Ignored, LinkTarget::File)
		}
		Some(_) => LinkTarget::Ignored,
	}
}

/// The RichEdit position of `line`: the start of the block at the line, moved on in proportion
/// to the line's place within the block.
pub fn position_of_line(blocks: &[SourceBlock], line: usize) -> Option<usize> {
	let index = blocks.iter().rposition(|block| block.line <= line)?;
	let block = blocks[index];
	let next = blocks[index + 1..].iter().find(|next| next.line > block.line);
	Some(match next {
		Some(next) if next.position > block.position => {
			let lines = (next.line - block.line) as u64;
			let offset = (line - block.line).min(next.line - block.line) as u64;
			block.position + ((next.position - block.position) as u64 * offset / lines) as usize
		}
		_ => block.position,
	})
}

/// The RichEdit positions of the blocks touching the source `lines` (inclusive), up to
/// `document_length` for the last block.
pub fn positions_of_lines(
	blocks: &[SourceBlock],
	lines: Range<usize>,
	document_length: usize,
) -> Option<Range<usize>> {
	let first = blocks.iter().rposition(|block| block.line <= lines.start).unwrap_or(0);
	let start = blocks.get(first)?;
	let end = blocks[first..]
		.iter()
		.find(|block| block.line > lines.end && block.position > start.position)
		.map_or(document_length, |block| block.position);
	(start.line <= lines.end).then_some(start.position..end)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn builds_a_locked_down_page() {
		let page = shell_page(false);
		assert!(page.contains("default-src 'none'; img-src data:; style-src 'unsafe-inline'"));
		assert!(page.contains("<base>"));
		assert!(page.contains("<article class=\"markdown-body\" id=\"content\"></article>"));
		assert!(!page.contains("<script"));
		assert!(shell_page(true).contains("color-scheme:dark"));
	}

	#[test]
	fn quotes_javascript_strings() {
		assert_eq!(json_string("a\"b\\c\n\u{2028}\u{1}"), "\"a\\\"b\\\\c\\n\\u2028\\u0001\"");
		assert_eq!(
			content_script("<p>x</p>", "http://npp-toolbox.invalid/C:/a/"),
			"document.querySelector('base').href=\"http://npp-toolbox.invalid/C:/a/\";\
			document.getElementById('content').innerHTML=\"<p>x</p>\";"
		);
		assert!(sync_script(5, Some(2..3)).ends_with("})(5,2,3);"));
		assert!(sync_script(5, None).ends_with("})(5,1,0);"));
	}

	#[test]
	fn parses_context_menu_requests() {
		assert_eq!(
			ContextMenuRequest::parse(
				r#" ["context-menu","a\"b\\c\n\u00e4\ud83d\ude00\/","<b>x</b>",""] "#
			),
			Some(ContextMenuRequest {
				selected_text: "a\"b\\c\n\u{e4}\u{1f600}/".to_owned(),
				selected_html: "<b>x</b>".to_owned(),
				link: String::new(),
			})
		);
		assert_eq!(ContextMenuRequest::parse(r#"["other","","",""]"#), None);
		assert_eq!(ContextMenuRequest::parse(r#"["context-menu","",""]"#), None);
		assert_eq!(ContextMenuRequest::parse(r#"["context-menu","","","",""]"#), None);
		assert_eq!(parse_json_strings("[ ]"), None);
		assert_eq!(parse_json_strings("[]"), Some(Vec::new()));
		assert_eq!(parse_json_strings(r#"["\ud83d"]"#), Some(vec!["\u{fffd}".to_owned()]));
		for broken in ["", "[", "[1]", r#"["\x"]"#, r#"["\u12"]"#, r#"["a",]"#, r#"["a""b"]"#] {
			assert_eq!(parse_json_strings(broken), None, "{broken}");
		}
		assert_eq!(parse_json_strings(r#"["a"]x"#), None);
	}

	#[test]
	fn parses_hovered_links() {
		assert_eq!(
			parse_hovered_link(r#"["link-hover","https://example.com/"]"#),
			Some("https://example.com/".to_owned())
		);
		assert_eq!(parse_hovered_link(r#"["link-hover",""]"#), Some(String::new()));
		assert_eq!(parse_hovered_link(r#"["context-menu","","",""]"#), None);
		assert_eq!(parse_hovered_link(r#"["link-hover","a","b"]"#), None);
		assert_eq!(ContextMenuRequest::parse(r#"["link-hover",""]"#), None);
	}

	#[test]
	fn reads_links_of_status_texts() {
		let base = "http://npp-toolbox.invalid/C:/docs/";
		assert_eq!(
			link_location(&status_text_link("npp-toolbox.invalid/C:/docs/a b.md"), base),
			Some(r"C:\docs\a b.md".to_owned())
		);
		assert_eq!(
			link_location(&status_text_link("http://npp-toolbox.invalid/C:/docs/#x"), base),
			Some("#x".to_owned())
		);
		assert_eq!(status_text_link(" https://example.com/ "), "https://example.com/");
		assert_eq!(status_text_link(""), "");
	}

	#[test]
	fn copies_link_locations() {
		let base = "http://npp-toolbox.invalid/C:/docs/";
		let location = |uri| link_location(uri, base);
		assert_eq!(
			location("http://npp-toolbox.invalid/C:/docs/#intro"),
			Some("#intro".to_owned())
		);
		assert_eq!(
			location("http://npp-toolbox.invalid/C:/docs/a%20b.md"),
			Some(r"C:\docs\a b.md".to_owned())
		);
		assert_eq!(location("https://example.com/"), Some("https://example.com/".to_owned()));
		assert_eq!(location("javascript:alert(1)"), None);
	}

	#[test]
	fn encodes_link_base_urls() {
		assert_eq!(
			link_base_url(Some(Path::new(r"C:\My Docs\ä"))),
			"http://npp-toolbox.invalid/C:/My%20Docs/%C3%A4/"
		);
		assert_eq!(link_base_url(None), "http://npp-toolbox.invalid/");
	}

	#[test]
	fn resolves_links_reported_by_webview2() {
		let base = "http://npp-toolbox.invalid/C:/docs/";
		assert_eq!(
			resolved_link_target("http://npp-toolbox.invalid/C:/docs/#intro", base),
			LinkTarget::Anchor("intro".to_owned())
		);
		assert_eq!(
			resolved_link_target("http://npp-toolbox.invalid/C:/other%20file.md?x#y", base),
			LinkTarget::File(PathBuf::from("C:/other file.md"))
		);
		assert_eq!(
			resolved_link_target("https://example.com/", base),
			LinkTarget::External("https://example.com/".to_owned())
		);
		assert_eq!(
			resolved_link_target(
				"http://npp-toolbox.invalid/relative.md",
				"http://npp-toolbox.invalid/"
			),
			LinkTarget::Ignored
		);
	}

	#[test]
	fn resolves_links_written_in_markdown() {
		let directory = Some(Path::new("C:/docs"));
		assert_eq!(link_target("#intro", None), LinkTarget::Anchor("intro".to_owned()));
		assert_eq!(
			link_target("https://example.com", directory),
			LinkTarget::External("https://example.com".to_owned())
		);
		assert_eq!(
			link_target("file:///C:/docs/other%20file.md", directory),
			LinkTarget::File(PathBuf::from("C:/docs/other file.md"))
		);
		assert_eq!(
			link_target("sub/x.md#part", directory),
			LinkTarget::File(Path::new("C:/docs").join("sub/x.md"))
		);
		assert_eq!(link_target("javascript:alert(1)", directory), LinkTarget::Ignored);
		assert_eq!(link_target("relative.md", None), LinkTarget::Ignored);
	}

	#[test]
	fn maps_lines_to_positions() {
		let blocks = [
			SourceBlock { line: 0, position: 0 },
			SourceBlock { line: 2, position: 10 },
			SourceBlock { line: 6, position: 50 },
		];
		assert_eq!(position_of_line(&blocks, 0), Some(0));
		assert_eq!(position_of_line(&blocks, 4), Some(30));
		assert_eq!(position_of_line(&blocks, 9), Some(50));
		assert_eq!(position_of_line(&[], 3), None);
		assert_eq!(positions_of_lines(&blocks, 3..3, 80), Some(10..50));
		assert_eq!(positions_of_lines(&blocks, 1..6, 80), Some(0..80));
	}
}
