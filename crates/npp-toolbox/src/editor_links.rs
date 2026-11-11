//! Opens the links of the Markdown language on a double click on their destination, like
//! Notepad++ opens the URLs it detects. The lexer styles destinations as
//! [`style::URL`](crate::markdown_lexer::style::URL), which
//! `editor_lexer` makes a hotspot. Web addresses open in the default application, local files in
//! Notepad++ (or their default application) and `#anchors` move to their heading.
//!
//! Notepad++ finds URLs in the text by itself, underlines them with its indicator and opens the
//! underlined text on a double click. In a link destination that URL may run past the
//! destination (the `))` of `[a](https://x.y/a-b))`) and keeps escapes such as `\_`, so the
//! indicator is removed there as soon as Notepad++ sets it, before it is drawn, and this module
//! opens the link.

use std::cell::RefCell;
use std::path::Path;

use npp::ffi::{
	SciNotification, npp_notification, scintilla_change, scintilla_constant, scintilla_notification,
};
use npp::{Notepad, Scintilla};
use windows_sys::Win32::UI::WindowsAndMessaging::{
	KillTimer, PostMessageW, SetTimer, WM_LBUTTONUP,
};

use crate::markdown_lexer::{LANGUAGE_NAME, style};
use crate::markdown_links::{destination_at, heading_offset};
use crate::viewer::open_link_target;
use crate::viewer_page::{LinkTarget, link_target};

struct PendingLink {
	editor: Scintilla,
	target: LinkTarget,
}

// Clippy 1.99 reports the initializer although it is `const` already.
thread_local! {
	/// The link to open once the double click is over.
	#[allow(clippy::missing_const_for_thread_local)]
	static PENDING_LINK: RefCell<Option<PendingLink>> = const { RefCell::new(None) };
}

pub fn on_notification(notepad: &Notepad, notification: &SciNotification) {
	let code = notification.header.code;
	if notification.header.hwnd_from == notepad.window() {
		// Notepad++ has set its URL indicator before the document became Markdown, e.g. before
		// `editor_lexer` gave a document the language on its first activation, and does not set it
		// again until the next scroll or edit.
		if code == npp_notification::BUFFER_ACTIVATED || code == npp_notification::LANGUAGE_CHANGED
		{
			let editor = notepad.current_scintilla();
			remove_detected_urls_in_links(&editor, 0..editor.length());
		}
		return;
	}
	let is_indicator_change = code == scintilla_notification::MODIFIED
		&& notification.modification_type & scintilla_change::CHANGE_INDICATOR != 0;
	if !is_indicator_change
		&& code != scintilla_notification::DOUBLE_CLICK
		&& code != scintilla_notification::HOTSPOT_DOUBLE_CLICK
	{
		return;
	}
	let Some(editor) = notepad
		.editors()
		.into_iter()
		.find(|editor| editor.window() == notification.header.hwnd_from)
	else {
		return;
	};
	let position = notification.position;
	if is_indicator_change {
		// Notepad++ sets its URL indicator one URL at a time while it handles a paint, scroll or
		// edit. Removing it in the notification of each change keeps it from being drawn. The
		// removal notifies again, and then finds no indicator left.
		remove_detected_urls_in_links(&editor, position..position + notification.length);
		return;
	}
	if code == scintilla_notification::DOUBLE_CLICK {
		// Plugins get the notification before Notepad++, which opens the URL under its
		// indicator.
		let line = editor.line_of_position(position);
		remove_detected_urls_in_links(&editor, editor.line_start(line)..editor.line_end(line));
		return;
	}
	if notification.modifiers != scintilla_constant::MODIFIER_NONE || !is_markdown(&editor) {
		return;
	}
	// Notepad++ has opened a URL it detected outside of link destinations already.
	if editor.has_indicator_at(scintilla_constant::NOTEPAD_URL_INDICATOR, position) {
		return;
	}
	let text = editor.whole_text();
	let offset = byte_offset_of_character(&text, editor.count_characters(0..position));
	let Some(destination) = destination_at(&text, offset) else { return };
	let file_path = notepad.current_file_path();
	let target = link_target(&destination, file_path.as_deref().and_then(Path::parent));
	if target == LinkTarget::Ignored {
		return;
	}
	// Like Notepad++ for its URLs: the double click selected a word, and Scintilla would extend
	// the selection while the mouse moves until the button goes up, which an opened browser or
	// document takes.
	editor.select(position, position);
	// SAFETY: Posting a message without pointers to the editor window.
	unsafe { PostMessageW(editor.window(), WM_LBUTTONUP, 0, 0) };
	PENDING_LINK.set(Some(PendingLink { editor, target }));
	// Opening a document switches the editor's document, which must not happen inside its mouse
	// handling. SAFETY: A thread timer without window; Notepad++'s message loop calls it.
	unsafe { SetTimer(core::ptr::null_mut(), 0, 0, Some(open_pending_link)) };
}

fn is_markdown(editor: &Scintilla) -> bool {
	editor.lexer_language() == LANGUAGE_NAME.to_str().unwrap_or_default()
}

/// Removes Notepad++'s URL indicator in `range` from every URL it detected that overlaps a link
/// destination. URLs in plain text, code or HTML stay for Notepad++ to open.
fn remove_detected_urls_in_links(editor: &Scintilla, range: std::ops::Range<isize>) {
	const INDICATOR: u32 = scintilla_constant::NOTEPAD_URL_INDICATOR;
	// Most indicator changes are other indicators, such as Notepad++'s highlighting of the
	// selected word, so the cheap check comes first.
	let urls = editor.indicator_runs(INDICATOR, range);
	if urls.is_empty() || !is_markdown(editor) {
		return;
	}
	for url in urls {
		// Notepad++ detects URLs on scrolling before Scintilla styles the lines that came into
		// view.
		if editor.end_styled() < url.end {
			editor.colourise(url.clone());
		}
		if editor.styles_in(url.clone()).contains(&style::URL) {
			editor.clear_indicator(INDICATOR, url);
		}
	}
}

unsafe extern "system" fn open_pending_link(
	_window: windows_sys::Win32::Foundation::HWND,
	_message: u32,
	timer: usize,
	_time: u32,
) {
	// SAFETY: Ends the thread timer that called this procedure.
	unsafe { KillTimer(core::ptr::null_mut(), timer) };
	let Some(PendingLink { editor, target }) = PENDING_LINK.take() else { return };
	let Some(notepad) = npp::plugin::notepad() else { return };
	match target {
		LinkTarget::Anchor(anchor) => show_heading(&editor, &anchor),
		target => open_link_target(&notepad, target),
	}
}

fn show_heading(editor: &Scintilla, anchor: &str) {
	let text = editor.whole_text();
	let Some(offset) = heading_offset(&text, anchor) else { return };
	let characters = text.get(..offset).map_or(0, |before| before.chars().count());
	editor.show_position(editor.position_of_character(characters));
}

/// The byte offset in `text` of the character with the index `character`, or the end of `text`.
fn byte_offset_of_character(text: &str, character: usize) -> usize {
	text.char_indices().nth(character).map_or(text.len(), |(offset, _)| offset)
}
