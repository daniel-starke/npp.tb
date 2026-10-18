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

	/// The style of every byte of the document.
	pub fn styles(&self) -> Vec<u8> {
		let length = self.length().max(0);
		// Scintilla writes byte and style pairs plus two terminating NULs.
		let mut styled_text = vec![0u8; 2 * length as usize + 2];
		let mut request =
			TextRangeFull { min: 0, max: length, text: styled_text.as_mut_ptr().cast() };
		self.send_message(scintilla_message::GET_STYLED_TEXT_FULL, 0, &raw mut request as LPARAM);
		styled_text
			.as_chunks::<2>()
			.0
			.iter()
			.take(length as usize)
			.map(|&[_, style]| style)
			.collect()
	}

	/// The lexer's tags of `style`, e.g. `literal string`. Empty if the lexer has none.
	pub fn style_tags(&self, style: u8) -> String {
		let length = self.send_message(scintilla_message::TAGS_OF_STYLE, usize::from(style), 0);
		let Ok(length) = usize::try_from(length) else { return String::new() };
		if length == 0 {
			return String::new();
		}
		let mut tags = vec![0u8; length + 1];
		self.send_message(
			scintilla_message::TAGS_OF_STYLE,
			usize::from(style),
			tags.as_mut_ptr() as LPARAM,
		);
		tags.truncate(length);
		String::from_utf8_lossy(&tags).into_owned()
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
