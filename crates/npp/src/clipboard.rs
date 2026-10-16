//! Writing several formats of the same content to the Windows clipboard.

use windows_sys::Win32::Foundation::{GlobalFree, HANDLE};
use windows_sys::Win32::System::DataExchange::{
	CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

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
