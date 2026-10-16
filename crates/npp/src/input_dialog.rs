//! Small modal dialog asking the user for a single value.

use core::cell::Cell;
use core::ops::RangeInclusive;

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::System::LibraryLoader::{
	GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
	GetModuleHandleExW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
	BS_DEFPUSHBUTTON, BS_PUSHBUTTON, DS_MODALFRAME, DialogBoxIndirectParamW, ES_AUTOHSCROLL,
	ES_NUMBER, EndDialog, GetDlgItem, GetDlgItemTextW, GetWindowRect, IDCANCEL, IDOK,
	MB_ICONWARNING, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetWindowPos, SetWindowTextW,
	WM_COMMAND, WM_INITDIALOG, WS_BORDER, WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU, WS_TABSTOP,
	WS_VISIBLE,
};

use crate::dialog_template::{ControlClass, DialogRect, DialogTemplate};
use crate::notepad::Notepad;
use crate::wide::{from_wide_until_null, to_wide_null_terminated};

const VALUE_EDIT_ID: u16 = 100;
const PROMPT_LABEL_ID: u16 = 101;
/// `EM_SETSEL`, declared here to avoid pulling in the large `Win32_UI_Controls` feature.
const EDIT_SET_SELECTION: u32 = 0x00B1;
const MAX_VALUE_LENGTH: usize = 256;

pub struct TextRequest<'a> {
	pub title: &'a str,
	pub prompt: &'a str,
	pub initial_value: &'a str,
	pub digits_only: bool,
	/// The dialog stays open (with a warning beep) until this accepts the entered text.
	pub is_acceptable: &'a dyn Fn(&str) -> bool,
}

pub fn ask_for_number(
	notepad: &Notepad,
	title: &str,
	prompt: &str,
	initial_value: u32,
	allowed: RangeInclusive<u32>,
) -> Option<u32> {
	let parse =
		|text: &str| text.trim().parse::<u32>().ok().filter(|value| allowed.contains(value));
	let text = ask_for_text(
		notepad,
		&TextRequest {
			title,
			prompt,
			initial_value: &initial_value.to_string(),
			digits_only: true,
			is_acceptable: &|text| parse(text).is_some(),
		},
	)?;
	parse(&text)
}

/// Returns `None` if the user cancelled.
pub fn ask_for_text(notepad: &Notepad, request: &TextRequest) -> Option<String> {
	let template = build_template(request);
	let mut session = DialogSession { notepad: *notepad, request, accepted_value: None };
	// The dialog is modal, so the session outlives every dialog procedure callback. The lifetime
	// is erased only for the duration of this call.
	ACTIVE_SESSION.set((&raw mut session).cast());
	// SAFETY: The template buffer is DWORD aligned and well formed.
	unsafe {
		DialogBoxIndirectParamW(
			current_module(),
			template.as_ptr().cast(),
			notepad.window(),
			Some(dialog_procedure),
			0,
		);
	}
	ACTIVE_SESSION.set(core::ptr::null_mut());
	session.accepted_value
}

struct DialogSession<'a> {
	notepad: Notepad,
	request: &'a TextRequest<'a>,
	accepted_value: Option<String>,
}

thread_local! {
	/// Session of the open dialog. `DialogBoxIndirectParamW` is modal, so there is at most one per
	/// thread, and a thread local spares the pointer-width specific `SetWindowLongPtrW`.
	static ACTIVE_SESSION: Cell<*mut DialogSession<'static>> = const { Cell::new(core::ptr::null_mut()) };
}

fn build_template(request: &TextRequest) -> Vec<u32> {
	let style = DS_MODALFRAME as u32 | WS_POPUP | WS_CAPTION | WS_SYSMENU;
	let child = WS_CHILD | WS_VISIBLE;
	let edit_style = child
		| WS_BORDER
		| WS_TABSTOP
		| ES_AUTOHSCROLL as u32
		| if request.digits_only { ES_NUMBER as u32 } else { 0 };
	let mut template = DialogTemplate::new(
		style,
		request.title,
		DialogRect { x: 0, y: 0, width: 200, height: 62 },
		"Segoe UI",
		9,
	);
	template.add_control(
		ControlClass::Static,
		PROMPT_LABEL_ID,
		child,
		DialogRect { x: 7, y: 7, width: 186, height: 9 },
		request.prompt,
	);
	template.add_control(
		ControlClass::Edit,
		VALUE_EDIT_ID,
		edit_style,
		DialogRect { x: 7, y: 19, width: 186, height: 13 },
		"",
	);
	template.add_control(
		ControlClass::Button,
		IDOK as u16,
		child | WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
		DialogRect { x: 89, y: 40, width: 50, height: 14 },
		"OK",
	);
	template.add_control(
		ControlClass::Button,
		IDCANCEL as u16,
		child | WS_TABSTOP | BS_PUSHBUTTON as u32,
		DialogRect { x: 143, y: 40, width: 50, height: 14 },
		"Cancel",
	);
	template.into_dword_buffer()
}

unsafe extern "system" fn dialog_procedure(
	dialog: HWND,
	message: u32,
	wparam: WPARAM,
	_lparam: LPARAM,
) -> isize {
	let session_pointer = ACTIVE_SESSION.get();
	if session_pointer.is_null() {
		return 0;
	}
	// SAFETY: Set by `ask_for_text` and valid while its modal dialog is open.
	let session = unsafe { &mut *session_pointer };
	match message {
		WM_INITDIALOG => {
			initialize_dialog(dialog, session);
			// Focus was set explicitly.
			0
		}
		WM_COMMAND => {
			let command_id = (wparam & 0xFFFF) as i32;
			match command_id {
				IDOK => {
					let value = dialog_item_text(dialog, VALUE_EDIT_ID);
					if (session.request.is_acceptable)(&value) {
						session.accepted_value = Some(value);
						close_dialog(dialog);
					} else {
						// SAFETY: No preconditions.
						unsafe { MessageBeep(MB_ICONWARNING) };
					}
					1
				}
				IDCANCEL => {
					close_dialog(dialog);
					1
				}
				_ => 0,
			}
		}
		_ => 0,
	}
}

fn initialize_dialog(dialog: HWND, session: &DialogSession) {
	let initial_value = to_wide_null_terminated(session.request.initial_value);
	// SAFETY: `dialog` is the live dialog. The string is NUL terminated.
	unsafe {
		let edit = GetDlgItem(dialog, i32::from(VALUE_EDIT_ID));
		SetWindowTextW(edit, initial_value.as_ptr());
		SendMessageW(edit, EDIT_SET_SELECTION, 0, -1);
		SetFocus(edit);
	}
	center_on_owner(dialog, session.notepad.window());
	session.notepad.apply_theme_to_dialog(dialog);
}

fn close_dialog(dialog: HWND) {
	// SAFETY: `dialog` is the live modal dialog.
	unsafe { EndDialog(dialog, 0) };
}

fn dialog_item_text(dialog: HWND, item_id: u16) -> String {
	let mut buffer = [0u16; MAX_VALUE_LENGTH];
	// SAFETY: The buffer length is passed along.
	unsafe {
		GetDlgItemTextW(dialog, i32::from(item_id), buffer.as_mut_ptr(), buffer.len() as i32)
	};
	from_wide_until_null(&buffer)
}

fn center_on_owner(dialog: HWND, owner: HWND) {
	let mut owner_rect = RECT::default();
	let mut dialog_rect = RECT::default();
	// SAFETY: Valid window handles and out pointers.
	unsafe {
		if GetWindowRect(owner, &mut owner_rect) == 0
			|| GetWindowRect(dialog, &mut dialog_rect) == 0
		{
			return;
		}
		let dialog_width = dialog_rect.right - dialog_rect.left;
		let dialog_height = dialog_rect.bottom - dialog_rect.top;
		let x = owner_rect.left + (owner_rect.right - owner_rect.left - dialog_width) / 2;
		let y = owner_rect.top + (owner_rect.bottom - owner_rect.top - dialog_height) / 2;
		SetWindowPos(dialog, core::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER);
	}
}

/// Handle of the DLL containing this code (not of `notepad++.exe`).
fn current_module() -> windows_sys::Win32::Foundation::HMODULE {
	let mut module = core::ptr::null_mut();
	// SAFETY: With `FROM_ADDRESS` the "name" is any address inside the module.
	unsafe {
		GetModuleHandleExW(
			GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
			current_module as *const () as *const u16,
			&mut module,
		);
	}
	module
}
