//! Writing several formats of the same content to the Windows clipboard, and reading it.

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
	CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
	GetClipboardFormatNameW, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
	GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};

use crate::notepad::Notepad;
use crate::wide::to_wide_null_terminated;

/// Name of the registered clipboard format of HTML ("CF_HTML").
const HTML_FORMAT_NAME: &str = "HTML Format";

pub enum ClipboardContent<'a> {
	/// A complete "HTML Format" payload with its header, see the Win32 documentation.
	Html(&'a str),
	UnicodeText(&'a str),
}

/// Replaces the clipboard content with all `contents` at once, so that applications pick the
/// format they prefer. `false` if the clipboard is unavailable, e.g. opened by another program.
pub fn set_clipboard(notepad: &Notepad, contents: &[ClipboardContent]) -> bool {
	// SAFETY: Opening with the Notepad++ window as owner has no preconditions. It fails if
	// another window has the clipboard open.
	if unsafe { OpenClipboard(notepad.window()) } == 0 {
		return false;
	}
	// SAFETY: The clipboard is open.
	let mut is_complete = unsafe { EmptyClipboard() } != 0;
	for content in contents {
		let (format, bytes) = match content {
			ClipboardContent::Html(html) => {
				let name = to_wide_null_terminated(HTML_FORMAT_NAME);
				// SAFETY: `name` is NUL terminated and alive for the call.
				let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
				(format, html.bytes().chain(Some(0)).collect::<Vec<u8>>())
			}
			ClipboardContent::UnicodeText(text) => (
				u32::from(CF_UNICODETEXT),
				to_wide_null_terminated(text).iter().flat_map(|unit| unit.to_le_bytes()).collect(),
			),
		};
		is_complete &= format != 0 && put_on_clipboard(format, &bytes);
	}
	// SAFETY: The clipboard was opened above.
	unsafe { CloseClipboard() };
	is_complete
}

/// Copies `bytes` into a movable global memory block and hands it to the open clipboard.
fn put_on_clipboard(format: u32, bytes: &[u8]) -> bool {
	// SAFETY: Allocating has no preconditions. The result is checked before use.
	let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
	if memory.is_null() {
		return false;
	}
	// SAFETY: `memory` is a valid movable block of `bytes.len()` bytes. It is locked for the
	// copy only. On success the clipboard owns it, otherwise it is freed here.
	unsafe {
		let destination = GlobalLock(memory).cast::<u8>();
		if destination.is_null() {
			GlobalFree(memory);
			return false;
		}
		core::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
		GlobalUnlock(memory);
		if SetClipboardData(format, memory as HANDLE).is_null() {
			GlobalFree(memory);
			return false;
		}
	}
	true
}

/// Standard clipboard formats, as [`ClipboardReader::formats`] reports them.
pub mod format {
	use super::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};

	/// NUL terminated UTF-16 text.
	pub const UNICODE_TEXT: u32 = CF_UNICODETEXT as u32;
	/// `BITMAPINFO` followed by the pixels.
	pub const DIB: u32 = CF_DIB as u32;
	/// `BITMAPV5HEADER` followed by the pixels.
	pub const DIB_V5: u32 = CF_DIBV5 as u32;
	/// `DROPFILES` followed by the file names, e.g. of files copied in Explorer.
	pub const FILE_DROP: u32 = CF_HDROP as u32;
}

/// The clipboard while [`read_clipboard`] keeps it open.
pub struct ClipboardReader {
	_open: (),
}

/// Runs `read` with the opened clipboard. `None` if the clipboard is unavailable, e.g. opened by
/// another program.
pub fn read_clipboard<T>(notepad: &Notepad, read: impl FnOnce(&ClipboardReader) -> T) -> Option<T> {
	// SAFETY: See `set_clipboard`.
	if unsafe { OpenClipboard(notepad.window()) } == 0 {
		return None;
	}
	let result = read(&ClipboardReader { _open: () });
	// SAFETY: The clipboard was opened above.
	unsafe { CloseClipboard() };
	Some(result)
}

impl ClipboardReader {
	/// The available formats in the order the clipboard lists them: the formats placed by the
	/// application first, in its order of preference, then the ones Windows converts to.
	pub fn formats(&self) -> Vec<u32> {
		let mut formats = Vec::new();
		let mut format = 0;
		loop {
			// SAFETY: The clipboard is open; 0 starts the enumeration.
			format = unsafe { EnumClipboardFormats(format) };
			if format == 0 {
				return formats;
			}
			formats.push(format);
		}
	}

	/// The name of a registered format, such as `PNG` or `image/svg+xml`. `None` for the
	/// standard formats.
	pub fn format_name(&self, format: u32) -> Option<String> {
		let mut name = [0u16; 256];
		// SAFETY: `name` is writable for the given number of UTF-16 units including the NUL.
		let length =
			unsafe { GetClipboardFormatNameW(format, name.as_mut_ptr(), name.len() as i32) };
		(length > 0).then(|| String::from_utf16_lossy(&name[..length as usize]))
	}

	/// A copy of the content in `format`, which must be stored in global memory (not a GDI handle
	/// like `CF_BITMAP`). Global memory may be larger than the data written into it, so binary
	/// formats can end with padding.
	pub fn data(&self, format: u32) -> Option<Vec<u8>> {
		// SAFETY: The clipboard is open. The handle stays owned by the clipboard.
		let memory = unsafe { GetClipboardData(format) };
		if memory.is_null() {
			return None;
		}
		// SAFETY: `memory` is global memory as documented above. It is locked for the copy only
		// and `GlobalSize` bytes are readable while locked.
		unsafe {
			let size = GlobalSize(memory);
			let source = GlobalLock(memory).cast::<u8>();
			if source.is_null() {
				return None;
			}
			let bytes = core::slice::from_raw_parts(source, size).to_vec();
			GlobalUnlock(memory);
			Some(bytes)
		}
	}
}
