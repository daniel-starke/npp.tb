//! A read-only RichEdit 4.1+ control (`msftedit.dll`, part of Windows) showing RTF.

use std::ops::Range;

use windows_sys::Win32::Foundation::{HWND, LPARAM, POINT, POINTL, RECT, WPARAM};
use windows_sys::Win32::Globalization::CP_UTF8;
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::Graphics::Gdi::{
	RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RedrawWindow,
};
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
use windows_sys::Win32::UI::Controls::{
	EM_CHARFROMPOS, EM_POSFROMCHAR, EM_SETMARGINS, EM_SETREADONLY, NMHDR,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
	CreateWindowExW, EC_LEFTMARGIN, EC_RIGHTMARGIN, ES_MULTILINE, ES_NOHIDESEL, ES_READONLY,
	GetClientRect, HMENU, SendMessageW, WM_COPY, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SETCURSOR,
	WM_SETREDRAW, WM_USER, WS_CHILD, WS_CLIPSIBLINGS, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::wide::to_wide_null_terminated;

// `Richedit.h`, which windows-sys does not cover. Its structures are packed to 4 bytes.
const MSFTEDIT_CLASS: &str = "RICHEDIT50W";
const EM_EXLIMITTEXT: u32 = WM_USER + 53;
const EM_EXGETSEL: u32 = WM_USER + 52;
const EM_EXSETSEL: u32 = WM_USER + 55;
const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
const EM_SETEVENTMASK: u32 = WM_USER + 69;
const EM_GETTEXTLENGTHEX: u32 = WM_USER + 95;
const EM_SETTEXTEX: u32 = WM_USER + 97;
const EM_GETSCROLLPOS: u32 = WM_USER + 221;
const EM_SETSCROLLPOS: u32 = WM_USER + 222;
const ENM_LINK: u32 = 0x0400_0000;
const EN_LINK: u32 = 0x070b;
const ST_DEFAULT: u32 = 0;
const GTL_PRECISE: u32 = 2;
const GTL_NUMCHARS: u32 = 8;

/// `SETTEXTEX`
#[repr(C, packed(4))]
struct SetTextOptions {
	flags: u32,
	codepage: u32,
}

/// `GETTEXTLENGTHEX`
#[repr(C, packed(4))]
struct TextLengthOptions {
	flags: u32,
	codepage: u32,
}

/// `CHARRANGE`
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
struct CharacterRange {
	cp_min: i32,
	cp_max: i32,
}

/// `ENLINK`
#[repr(C, packed(4))]
struct LinkNotification {
	header: NMHDR,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
	characters: CharacterRange,
}

/// Left and right margin in pixels.
const MARGIN: usize = 12;

pub struct RichEdit {
	window: HWND,
}

impl RichEdit {
	/// A new control filling no area yet. `None` if `msftedit.dll` is missing.
	#[allow(clippy::not_unsafe_ptr_arg_deref, reason = "window handles are not dereferenced")]
	pub fn create(parent: HWND, control_id: usize) -> Option<Self> {
		let library = to_wide_null_terminated("msftedit.dll");
		let class_name = to_wide_null_terminated(MSFTEDIT_CLASS);
		// SAFETY: The name is NUL terminated. The library stays loaded for the window class.
		if unsafe { LoadLibraryW(library.as_ptr()) }.is_null() {
			return None;
		}
		let style =
			WS_CHILD
				| WS_VISIBLE | WS_VSCROLL
				| WS_TABSTOP | WS_CLIPSIBLINGS
				| (ES_MULTILINE | ES_READONLY | ES_NOHIDESEL) as u32;
		// SAFETY: The class is registered by `msftedit.dll`, the parent is a valid window.
		let window = unsafe {
			CreateWindowExW(
				0,
				class_name.as_ptr(),
				core::ptr::null(),
				style,
				0,
				0,
				0,
				0,
				parent,
				control_id as HMENU,
				core::ptr::null_mut(),
				core::ptr::null(),
			)
		};
		if window.is_null() {
			return None;
		}
		let control = Self { window };
		control.send(EM_EXLIMITTEXT, 0, 0x7fff_fffe);
		control.send(EM_SETEVENTMASK, 0, ENM_LINK as LPARAM);
		control.send(
			EM_SETMARGINS,
			(EC_LEFTMARGIN | EC_RIGHTMARGIN) as WPARAM,
			(MARGIN | MARGIN << 16) as LPARAM,
		);
		Some(control)
	}

	pub fn window(&self) -> HWND {
		self.window
	}

	fn send(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
		// SAFETY: Sending to the own control. Pointer arguments are valid for the call at every
		// call site.
		unsafe { SendMessageW(self.window, message, wparam, lparam) }
	}

	/// Replaces the content, keeping the scroll position and without flickering.
	pub fn set_rtf(&self, rtf: &str) {
		let scroll_position = self.scroll_position();
		self.send(WM_SETREDRAW, 0, 0);
		let options = SetTextOptions { flags: ST_DEFAULT, codepage: CP_UTF8 };
		let text: Vec<u8> = rtf.bytes().chain(Some(0)).collect();
		// A read-only RichEdit drops the pictures of the RTF.
		self.send(EM_SETREADONLY, 0, 0);
		self.send(EM_SETTEXTEX, &raw const options as WPARAM, text.as_ptr() as LPARAM);
		self.send(EM_SETREADONLY, 1, 0);
		self.send(EM_SETSCROLLPOS, 0, &raw const scroll_position as LPARAM);
		self.send(WM_SETREDRAW, 1, 0);
		// SAFETY: Valid window, null region and rectangle redraw everything.
		unsafe {
			RedrawWindow(
				self.window,
				core::ptr::null(),
				core::ptr::null_mut(),
				RDW_ERASE | RDW_FRAME | RDW_INVALIDATE | RDW_ALLCHILDREN,
			)
		};
	}

	/// The width available to the text in twips (1/1440 inch), e.g. for tables and pictures.
	pub fn text_width_in_twips(&self) -> u32 {
		let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
		// SAFETY: Valid window and output rectangle.
		unsafe { GetClientRect(self.window, &mut client) };
		// One more margin of room keeps tables clear of the edge.
		let width = (client.right - client.left - 3 * MARGIN as i32).max(0) as u32;
		width.saturating_mul(1440) / self.dpi()
	}

	/// The DPI the control lays out its content with.
	pub fn dpi(&self) -> u32 {
		// SAFETY: Valid window.
		match unsafe { GetDpiForWindow(self.window) } {
			0 => crate::notepad::DEFAULT_DPI,
			dpi => dpi,
		}
	}

	/// `color` as `COLORREF` (`0x00BBGGRR`).
	pub fn set_background(&self, color: u32) {
		self.send(EM_SETBKGNDCOLOR, 0, color as LPARAM);
	}

	/// Number of characters, the end of the last position.
	pub fn text_length(&self) -> usize {
		let options = TextLengthOptions { flags: GTL_PRECISE | GTL_NUMCHARS, codepage: 1200 };
		self.send(EM_GETTEXTLENGTHEX, &raw const options as WPARAM, 0).max(0) as usize
	}

	fn scroll_position(&self) -> POINT {
		let mut position = POINT { x: 0, y: 0 };
		self.send(EM_GETSCROLLPOS, 0, &raw mut position as LPARAM);
		position
	}

	/// Scrolls `position` to `fraction_from_top` of the visible height.
	pub fn scroll_to(&self, position: usize, fraction_from_top: f32) {
		let mut point = POINTL { x: 0, y: 0 };
		self.send(EM_POSFROMCHAR, &raw mut point as WPARAM, position as LPARAM);
		let scroll_position = self.scroll_position();
		let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
		// SAFETY: Valid window and output rectangle.
		unsafe { GetClientRect(self.window, &mut client) };
		let offset = ((client.bottom - client.top) as f32 * fraction_from_top) as i32;
		let target =
			POINT { x: scroll_position.x, y: (point.y + scroll_position.y - offset).max(0) };
		self.send(EM_SETSCROLLPOS, 0, &raw const target as LPARAM);
	}

	/// Selects `range`, an empty range only moves the caret.
	pub fn select(&self, range: Range<usize>) {
		let range = CharacterRange { cp_min: range.start as i32, cp_max: range.end as i32 };
		self.send(EM_EXSETSEL, 0, &raw const range as LPARAM);
	}

	pub fn has_selection(&self) -> bool {
		let mut range = CharacterRange { cp_min: 0, cp_max: 0 };
		self.send(EM_EXGETSEL, 0, &raw mut range as LPARAM);
		let (start, end) = (range.cp_min, range.cp_max);
		start != end
	}

	pub fn select_all(&self) {
		self.select(0..i32::MAX as usize);
	}

	pub fn copy(&self) {
		self.send(WM_COPY, 0, 0);
	}

	/// The character nearest to the screen position `point`, `None` outside the control.
	pub fn character_at(&self, point: POINT) -> Option<usize> {
		let mut client_point = point;
		let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
		// SAFETY: Valid window, point and output rectangle.
		unsafe {
			ScreenToClient(self.window, &mut client_point);
			GetClientRect(self.window, &mut client);
		}
		let is_inside = (client.left..client.right).contains(&client_point.x)
			&& (client.top..client.bottom).contains(&client_point.y);
		let point = POINTL { x: client_point.x, y: client_point.y };
		// RichEdit takes a pointer to the point, unlike the edit control.
		let character = self.send(EM_CHARFROMPOS, 0, &raw const point as LPARAM);
		is_inside.then_some(character.max(0) as usize)
	}
}

/// The characters of a link clicked in a RichEdit, given the `lParam` of a `WM_NOTIFY`.
///
/// # Safety
/// `notification` must point to the `NMHDR` of a `WM_NOTIFY` message.
pub unsafe fn clicked_link(notification: LPARAM) -> Option<Range<usize>> {
	// SAFETY: Guaranteed by the caller.
	unsafe { link_mouse_message(notification, &[WM_LBUTTONUP]) }
}

/// The characters of the link the mouse moves over, given the `lParam` of a `WM_NOTIFY`.
/// RichEdit reports no leaving of a link. `msftedit.dll` reports hovering with `WM_SETCURSOR`.
///
/// # Safety
/// `notification` must point to the `NMHDR` of a `WM_NOTIFY` message.
pub unsafe fn hovered_link(notification: LPARAM) -> Option<Range<usize>> {
	// SAFETY: Guaranteed by the caller.
	unsafe { link_mouse_message(notification, &[WM_MOUSEMOVE, WM_SETCURSOR]) }
}

/// The characters of the link of an `EN_LINK` notification for one of the mouse `messages`.
///
/// # Safety
/// `notification` must point to the `NMHDR` of a `WM_NOTIFY` message.
unsafe fn link_mouse_message(notification: LPARAM, messages: &[u32]) -> Option<Range<usize>> {
	// SAFETY: Guaranteed by the caller.
	let header = unsafe { &*(notification as *const NMHDR) };
	if header.code != EN_LINK {
		return None;
	}
	// SAFETY: `EN_LINK` notifications carry an `LinkNotification`.
	let link = unsafe { &*(notification as *const LinkNotification) };
	let characters = link.characters;
	messages
		.contains(&link.message)
		.then(|| characters.cp_min.max(0) as usize..characters.cp_max.max(0) as usize)
}
