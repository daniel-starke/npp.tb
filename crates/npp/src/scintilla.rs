//! Access to a Scintilla editor view.
//!
//! Positions are byte offsets in the document encoding, as everywhere in Scintilla. Text is
//! converted between the document code page and Rust strings at this boundary.

use std::ffi::c_void;
use std::ops::Range;

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar, WideCharToMultiByte};
use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

use crate::ffi::{TextRangeFull, scintilla_constant, scintilla_message};
use crate::line_ending::LineEnding;

#[derive(Clone, Copy, Debug)]
pub struct Scintilla {
	window: HWND,
}

impl Scintilla {
	pub fn new(window: HWND) -> Self {
		Self { window }
	}

	pub fn window(&self) -> HWND {
		self.window
	}

	pub fn send_message(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
		// SAFETY: See `Notepad::send_message`.
		unsafe { SendMessageW(self.window, message, wparam, lparam) }
	}

	pub fn length(&self) -> isize {
		self.send_message(scintilla_message::GET_LENGTH, 0, 0)
	}

	pub fn is_read_only(&self) -> bool {
		self.send_message(scintilla_message::GET_READ_ONLY, 0, 0) != 0
	}

	pub fn tab_width(&self) -> usize {
		self.send_message(scintilla_message::GET_TAB_WIDTH, 0, 0).max(1) as usize
	}

	pub fn line_ending(&self) -> LineEnding {
		match self.send_message(scintilla_message::GET_EOL_MODE, 0, 0) {
			scintilla_constant::EOL_CR => LineEnding::Cr,
			scintilla_constant::EOL_LF => LineEnding::Lf,
			_ => LineEnding::CrLf,
		}
	}

	pub fn caret(&self) -> isize {
		self.send_message(scintilla_message::GET_CURRENT_POS, 0, 0)
	}

	pub fn move_caret_to(&self, position: isize) {
		self.send_message(scintilla_message::GOTO_POS, position as WPARAM, 0);
	}

	pub fn selection(&self) -> Range<isize> {
		self.send_message(scintilla_message::GET_SELECTION_START, 0, 0)
			..self.send_message(scintilla_message::GET_SELECTION_END, 0, 0)
	}

	pub fn is_selection_empty(&self) -> bool {
		self.send_message(scintilla_message::GET_SELECTION_EMPTY, 0, 0) != 0
	}

	pub fn select(&self, anchor: isize, caret: isize) {
		self.send_message(scintilla_message::SET_SEL, anchor as WPARAM, caret);
	}

	pub fn line_count(&self) -> usize {
		self.send_message(scintilla_message::GET_LINE_COUNT, 0, 0) as usize
	}

	pub fn line_of_position(&self, position: isize) -> usize {
		self.send_message(scintilla_message::LINE_FROM_POSITION, position as WPARAM, 0) as usize
	}

	pub fn line_start(&self, line: usize) -> isize {
		self.send_message(scintilla_message::POSITION_FROM_LINE, line, 0)
	}

	/// Position before the line ending.
	pub fn line_end(&self, line: usize) -> isize {
		self.send_message(scintilla_message::GET_LINE_END_POSITION, line, 0)
	}

	pub fn line_text(&self, line: usize) -> String {
		self.text_range(self.line_start(line)..self.line_end(line))
	}

	/// Text of each line in `lines`, without line endings.
	pub fn lines_text(&self, lines: Range<usize>) -> Vec<String> {
		lines.map(|line| self.line_text(line)).collect()
	}

	pub fn whole_text(&self) -> String {
		self.text_range(0..self.length())
	}

	/// Number of characters (not bytes) in `range`.
	pub fn count_characters(&self, range: Range<isize>) -> usize {
		self.send_message(scintilla_message::COUNT_CHARACTERS, range.start as WPARAM, range.end)
			as usize
	}

	/// Character offset of `position` within its line.
	pub fn character_column(&self, position: isize) -> usize {
		self.count_characters(self.line_start(self.line_of_position(position))..position)
	}

	/// Position of the `character_offset`th character in `line`, clamped to the line end.
	pub fn position_at_character_column(&self, line: usize, character_offset: usize) -> isize {
		let line_start = self.line_start(line);
		let line_end = self.line_end(line);
		if character_offset >= self.count_characters(line_start..line_end) {
			return line_end;
		}
		self.send_message(
			scintilla_message::POSITION_RELATIVE,
			line_start as WPARAM,
			character_offset as LPARAM,
		)
	}

	pub fn text_range(&self, range: Range<isize>) -> String {
		if range.end <= range.start {
			return String::new();
		}
		// Scintilla writes a terminating NUL after the requested bytes.
		let mut bytes = vec![0u8; (range.end - range.start) as usize + 1];
		let mut request =
			TextRangeFull { min: range.start, max: range.end, text: bytes.as_mut_ptr().cast() };
		let copied = self.send_message(
			scintilla_message::GET_TEXT_RANGE_FULL,
			0,
			&raw mut request as LPARAM,
		);
		bytes.truncate(copied.max(0) as usize);
		self.decode(&bytes)
	}

	/// Replaces `range` and returns the end position of the inserted text.
	pub fn replace_range(&self, range: Range<isize>, text: &str) -> isize {
		let bytes = self.encode(text);
		self.send_message(scintilla_message::SET_TARGET_RANGE, range.start as WPARAM, range.end);
		self.send_message(scintilla_message::REPLACE_TARGET, bytes.len(), bytes.as_ptr() as LPARAM);
		range.start + bytes.len() as isize
	}

	/// Replaces the main selection by `text`, or inserts it at the caret, and moves the caret
	/// after it.
	pub fn replace_selection(&self, text: &str) {
		let mut bytes = self.encode(text);
		bytes.push(0);
		self.send_message(scintilla_message::REPLACE_SELECTION, 0, bytes.as_ptr() as LPARAM);
	}

	/// Replaces whole lines (excluding the line ending after the last one) with `new_lines`
	/// joined by the document's line ending. Returns the position after the inserted text.
	pub fn replace_lines(&self, lines: Range<usize>, new_lines: &[String]) -> isize {
		let range = self.line_start(lines.start)..self.line_end(lines.end - 1);
		self.replace_range(range, &new_lines.join(self.line_ending().as_str()))
	}

	/// Groups all changes made by `change` into one undo step.
	pub fn as_single_undo_step<R>(&self, change: impl FnOnce() -> R) -> R {
		self.send_message(scintilla_message::BEGIN_UNDO_ACTION, 0, 0);
		let result = change();
		self.send_message(scintilla_message::END_UNDO_ACTION, 0, 0);
		result
	}

	/// Replaces the whole document.
	pub fn set_text(&self, text: &str) {
		let mut bytes = self.encode(text);
		bytes.push(0);
		self.send_message(scintilla_message::SET_TEXT, 0, bytes.as_ptr() as LPARAM);
	}

	/// Makes the editor treat text as UTF-8, for editors that are not Notepad++ documents.
	pub fn use_utf8(&self) {
		self.send_message(scintilla_message::SET_CODE_PAGE, scintilla_constant::CODE_PAGE_UTF8, 0);
	}

	/// Stops recording undo steps, so that repeated [`Self::set_text`] keeps no history.
	pub fn disable_undo(&self) {
		self.send_message(scintilla_message::SET_UNDO_COLLECTION, 0, 0);
		self.send_message(scintilla_message::EMPTY_UNDO_BUFFER, 0, 0);
	}

	/// Sets a lexer from `Notepad::create_lexer`. Scintilla takes ownership of it.
	pub fn set_lexer(&self, lexer: *mut c_void) {
		self.send_message(scintilla_message::SET_ILEXER, 0, lexer as LPARAM);
	}

	/// Sets keyword list `set` of the lexer to the space separated `keywords`.
	pub fn set_keywords(&self, set: usize, keywords: &str) {
		let keywords = null_terminated_bytes(&self.encode(keywords));
		self.send_message(scintilla_message::SET_KEYWORDS, set, keywords.as_ptr() as LPARAM);
	}

	pub fn set_lexer_property(&self, key: &str, value: &str) {
		let key = null_terminated_bytes(key.as_bytes());
		let value = null_terminated_bytes(value.as_bytes());
		self.send_message(
			scintilla_message::SET_PROPERTY,
			key.as_ptr() as WPARAM,
			value.as_ptr() as LPARAM,
		);
	}

	/// Styles the whole document with the lexer.
	pub fn colourise_all(&self) {
		self.send_message(scintilla_message::COLOURISE, 0, -1);
	}

	/// Styles at least `range` with the lexer.
	pub fn colourise(&self, range: Range<isize>) {
		self.send_message(scintilla_message::COLOURISE, range.start as WPARAM, range.end);
	}

	/// The style of every byte of the document.
	pub fn styles(&self) -> Vec<u8> {
		self.styles_in(0..self.length())
	}

	/// The style of every byte in `range`.
	pub fn styles_in(&self, range: Range<isize>) -> Vec<u8> {
		let length = (range.end - range.start).max(0) as usize;
		// Scintilla writes byte and style pairs plus two terminating NULs.
		let mut styled_text = vec![0u8; 2 * length + 2];
		let mut request = TextRangeFull {
			min: range.start,
			max: range.end,
			text: styled_text.as_mut_ptr().cast(),
		};
		self.send_message(scintilla_message::GET_STYLED_TEXT_FULL, 0, &raw mut request as LPARAM);
		styled_text.as_chunks::<2>().0.iter().take(length).map(|&[_, style]| style).collect()
	}

	/// The lexer's tags of `style`, e.g. `literal string`. Empty if the lexer has none.
	pub fn style_tags(&self, style: u8) -> String {
		self.ascii_result(scintilla_message::TAGS_OF_STYLE, usize::from(style))
	}

	/// The Lexilla name of the document's lexer, e.g. `cpp`, or `user` for Notepad++'s user
	/// defined languages.
	pub fn lexer_language(&self) -> String {
		self.ascii_result(scintilla_message::GET_LEXER_LANGUAGE, 0)
	}

	/// Copies the font, size and background of `source` to `style`, as Scintilla stores them.
	pub fn copy_style_font_and_background(&self, source: u8, style: u8) {
		let font = self.ascii_result(scintilla_message::STYLE_GET_FONT, usize::from(source));
		let font = null_terminated_bytes(font.as_bytes());
		let size =
			self.send_message(scintilla_message::STYLE_GET_SIZE_FRACTIONAL, usize::from(source), 0);
		let background = self.style_background(source);
		let style = usize::from(style);
		self.send_message(scintilla_message::STYLE_SET_FONT, style, font.as_ptr() as LPARAM);
		self.send_message(scintilla_message::STYLE_SET_SIZE_FRACTIONAL, style, size);
		self.send_message(scintilla_message::STYLE_SET_BACK, style, background as LPARAM);
	}

	/// The text color of `style` as `0xBBGGRR`.
	pub fn style_foreground(&self, style: u8) -> u32 {
		self.send_message(scintilla_message::STYLE_GET_FORE, usize::from(style), 0) as u32
	}

	/// The background color of `style` as `0xBBGGRR`.
	pub fn style_background(&self, style: u8) -> u32 {
		self.send_message(scintilla_message::STYLE_GET_BACK, usize::from(style), 0) as u32
	}

	/// Sets the text color of `style` (`0xBBGGRR`).
	pub fn set_style_foreground(&self, style: u8, color: u32) {
		self.send_message(scintilla_message::STYLE_SET_FORE, usize::from(style), color as LPARAM);
	}

	/// Sets the background color of `style` (`0xBBGGRR`).
	pub fn set_style_background(&self, style: u8, color: u32) {
		self.send_message(scintilla_message::STYLE_SET_BACK, usize::from(style), color as LPARAM);
	}

	pub fn set_style_font_style(
		&self,
		style: u8,
		is_bold: bool,
		is_italic: bool,
		is_underlined: bool,
	) {
		let style = usize::from(style);
		self.send_message(scintilla_message::STYLE_SET_BOLD, style, LPARAM::from(is_bold));
		self.send_message(scintilla_message::STYLE_SET_ITALIC, style, LPARAM::from(is_italic));
		self.send_message(
			scintilla_message::STYLE_SET_UNDERLINE,
			style,
			LPARAM::from(is_underlined),
		);
	}

	/// Makes the text of `style` a hotspot: a hand cursor over it, underlined while hovered, and
	/// `SCN_HOTSPOT*` notifications for clicks on it.
	pub fn set_style_hotspot(&self, style: u8, is_hotspot: bool) {
		self.send_message(
			scintilla_message::STYLE_SET_HOTSPOT,
			usize::from(style),
			LPARAM::from(is_hotspot),
		);
	}

	/// Whether `indicator` is set at `position`, whatever its value.
	pub fn has_indicator_at(&self, indicator: u32, position: isize) -> bool {
		let indicators =
			self.send_message(scintilla_message::INDICATOR_ALL_ON_FOR, position as WPARAM, 0);
		indicator < 32 && indicators & (1 << indicator) != 0
	}

	/// The runs of `indicator` that overlap `range`, each in full.
	pub fn indicator_runs(&self, indicator: u32, range: Range<isize>) -> Vec<Range<isize>> {
		let mut runs = Vec::new();
		let mut position = range.start;
		while position < range.end {
			let run_end =
				self.send_message(scintilla_message::INDICATOR_END, indicator as WPARAM, position);
			if run_end <= position {
				break;
			}
			let value = self.send_message(
				scintilla_message::INDICATOR_VALUE_AT,
				indicator as WPARAM,
				position,
			);
			if value != 0 {
				runs.push(position..run_end);
			}
			position = run_end;
		}
		runs
	}

	/// Clears `indicator` in `range`, keeping the current indicator of other callers.
	pub fn clear_indicator(&self, indicator: u32, range: Range<isize>) {
		let current = self.send_message(scintilla_message::GET_INDICATOR_CURRENT, 0, 0);
		self.send_message(scintilla_message::SET_INDICATOR_CURRENT, indicator as WPARAM, 0);
		self.send_message(
			scintilla_message::INDICATOR_CLEAR_RANGE,
			range.start as WPARAM,
			range.end - range.start,
		);
		self.send_message(scintilla_message::SET_INDICATOR_CURRENT, current as WPARAM, 0);
	}

	/// The position up to which the lexer has styled the document.
	pub fn end_styled(&self) -> isize {
		self.send_message(scintilla_message::GET_END_STYLED, 0, 0)
	}

	/// The position after the first `characters` characters of the document.
	pub fn position_of_character(&self, characters: usize) -> isize {
		self.send_message(scintilla_message::POSITION_RELATIVE, 0, characters as LPARAM)
	}

	/// Moves the caret to `position`, unfolding and scrolling its line into view.
	pub fn show_position(&self, position: isize) {
		let line = self.line_of_position(position);
		self.send_message(scintilla_message::ENSURE_VISIBLE_ENFORCE_POLICY, line, 0);
		self.move_caret_to(position);
	}

	/// Whether the background of `style` fills the rest of the line after its last character.
	pub fn set_style_eol_filled(&self, style: u8, is_filled: bool) {
		self.send_message(
			scintilla_message::STYLE_SET_EOL_FILLED,
			usize::from(style),
			LPARAM::from(is_filled),
		);
	}

	/// Replaces the selection by one selection per range, each with the caret at its end. The
	/// first range becomes the main selection and is scrolled into view.
	pub fn select_ranges(&self, ranges: &[Range<isize>]) {
		let Some((first, others)) = ranges.split_first() else { return };
		self.send_message(scintilla_message::SET_SELECTION, first.end as WPARAM, first.start);
		for range in others {
			self.send_message(scintilla_message::ADD_SELECTION, range.end as WPARAM, range.start);
		}
		self.send_message(scintilla_message::SET_MAIN_SELECTION, 0, 0);
		self.send_message(scintilla_message::SCROLL_CARET, 0, 0);
	}

	/// The text of a message that writes a string to `lParam`, or returns its length without
	/// NUL for a null `lParam`.
	fn ascii_result(&self, message: u32, wparam: WPARAM) -> String {
		let length = self.send_message(message, wparam, 0);
		let Ok(length) = usize::try_from(length) else { return String::new() };
		if length == 0 {
			return String::new();
		}
		let mut text = vec![0u8; length + 1];
		self.send_message(message, wparam, text.as_mut_ptr() as LPARAM);
		text.truncate(length);
		String::from_utf8_lossy(&text).into_owned()
	}

	fn is_utf8(&self) -> bool {
		self.send_message(scintilla_message::GET_CODE_PAGE, 0, 0) as usize
			== scintilla_constant::CODE_PAGE_UTF8
	}

	/// Notepad++ stores Unicode documents as UTF-8. ANSI documents use the system code page,
	/// which Scintilla reports as code page 0.
	fn ansi_code_page(&self) -> u32 {
		match self.send_message(scintilla_message::GET_CODE_PAGE, 0, 0) {
			0 => CP_ACP,
			code_page => code_page as u32,
		}
	}

	fn decode(&self, bytes: &[u8]) -> String {
		if bytes.is_empty() {
			return String::new();
		}
		if self.is_utf8() {
			return String::from_utf8_lossy(bytes).into_owned();
		}
		let code_page = self.ansi_code_page();
		// SAFETY: Sizing call with a null output buffer followed by a call with a buffer of the
		// reported size. Both read exactly `bytes.len()` bytes.
		unsafe {
			let length = MultiByteToWideChar(
				code_page,
				0,
				bytes.as_ptr(),
				bytes.len() as i32,
				core::ptr::null_mut(),
				0,
			);
			let mut wide = vec![0u16; length.max(0) as usize];
			MultiByteToWideChar(
				code_page,
				0,
				bytes.as_ptr(),
				bytes.len() as i32,
				wide.as_mut_ptr(),
				length,
			);
			String::from_utf16_lossy(&wide)
		}
	}

	fn encode(&self, text: &str) -> Vec<u8> {
		if self.is_utf8() || text.is_empty() {
			return text.as_bytes().to_vec();
		}
		let code_page = self.ansi_code_page();
		let wide: Vec<u16> = text.encode_utf16().collect();
		// SAFETY: Sizing call with a null output buffer followed by a call with a buffer of the
		// reported size. Both read exactly `wide.len()` UTF-16 units.
		unsafe {
			let length = WideCharToMultiByte(
				code_page,
				0,
				wide.as_ptr(),
				wide.len() as i32,
				core::ptr::null_mut(),
				0,
				core::ptr::null(),
				core::ptr::null_mut(),
			);
			let mut bytes = vec![0u8; length.max(0) as usize];
			WideCharToMultiByte(
				code_page,
				0,
				wide.as_ptr(),
				wide.len() as i32,
				bytes.as_mut_ptr(),
				length,
				core::ptr::null(),
				core::ptr::null_mut(),
			);
			bytes
		}
	}
}

fn null_terminated_bytes(bytes: &[u8]) -> Vec<u8> {
	bytes.iter().copied().chain(Some(0)).collect()
}
