//! Access to the Notepad++ main window.

use std::ffi::{OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
	MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_TASKMODAL, MESSAGEBOX_STYLE,
	MessageBoxW, SendMessageW,
};

use crate::ffi::{
	DarkModeColors, DockedWidgetData, NppData, ToolbarIconsWithDarkMode, dark_mode_flags,
	menu_command, modeless_dialog, npp_message, toolbar_icon_set,
};
use crate::scintilla::Scintilla;
use crate::wide::{from_wide_until_null, to_wide_null_terminated};

/// Buffer length for paths. Notepad++ supports long paths up to this length.
const MAX_PATH_LENGTH: usize = 32 * 1024;

/// Windows' reference DPI at 100 % scaling.
pub const DEFAULT_DPI: u32 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
	Information,
	Warning,
	Error,
}

#[derive(Clone, Copy, Debug)]
pub struct Notepad {
	handles: NppData,
}

// SAFETY: The window handles are plain identifiers. Notepad++ calls plugins on its UI thread only
// and `SendMessageW` marshals calls from other threads, so sharing them is sound.
unsafe impl Send for Notepad {}
// SAFETY: See `Send`. `Notepad` has no interior mutability.
unsafe impl Sync for Notepad {}

impl Notepad {
	pub fn new(handles: NppData) -> Self {
		Self { handles }
	}

	pub fn window(&self) -> HWND {
		self.handles.npp_handle
	}

	pub fn send_message(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
		// SAFETY: Sending to a valid (or null) window handle is always allowed. Pointer arguments
		// are the caller's responsibility and every caller in this crate passes valid buffers.
		unsafe { SendMessageW(self.handles.npp_handle, message, wparam, lparam) }
	}

	pub fn current_scintilla(&self) -> Scintilla {
		let mut view_index: i32 = 0;
		self.send_message(npp_message::GET_CURRENT_SCINTILLA, 0, &raw mut view_index as LPARAM);
		Scintilla::new(match view_index {
			1 => self.handles.scintilla_second_handle,
			_ => self.handles.scintilla_main_handle,
		})
	}

	/// Whether `window` is one of the two editors of the main window, e.g. the sender of a
	/// Scintilla notification.
	pub fn is_editor_window(&self, window: HWND) -> bool {
		window == self.handles.scintilla_main_handle
			|| window == self.handles.scintilla_second_handle
	}

	pub fn plugins_config_directory(&self) -> Option<PathBuf> {
		self.path_message(npp_message::GET_PLUGINS_CONFIG_DIR)
	}

	/// Where Notepad++ keeps `langs.xml` and `stylers.xml`: `%APPDATA%\Notepad++`, the
	/// program directory or the `-settingsDir` of the command line.
	pub fn settings_directory(&self) -> Option<PathBuf> {
		let length = self.send_message(npp_message::GET_NPP_SETTINGS_DIR_PATH, 0, 0);
		let length = usize::try_from(length).ok().filter(|&length| length > 0)?;
		let mut buffer = vec![0u16; length + 1];
		self.send_message(
			npp_message::GET_NPP_SETTINGS_DIR_PATH,
			buffer.len(),
			buffer.as_mut_ptr() as LPARAM,
		);
		path_from_wide(&buffer)
	}

	/// The directory of `notepad++.exe`.
	pub fn program_directory(&self) -> Option<PathBuf> {
		self.path_message(npp_message::GET_NPP_DIRECTORY)
	}

	/// Path of the active document; `None` for a new, never saved document.
	pub fn current_file_path(&self) -> Option<PathBuf> {
		self.path_message(npp_message::GET_FULL_CURRENT_PATH).filter(|path| path.is_absolute())
	}

	/// File name of the active document, or the tab name of a new document such as `new 1`.
	pub fn current_file_name(&self) -> String {
		let mut buffer = [0u16; MAX_PATH_LENGTH];
		self.send_message(npp_message::GET_FILE_NAME, buffer.len(), buffer.as_mut_ptr() as LPARAM);
		from_wide_until_null(&buffer)
	}

	/// Sends a message that writes a path of at most [`MAX_PATH_LENGTH`] UTF-16 units.
	fn path_message(&self, message: u32) -> Option<PathBuf> {
		let mut buffer = vec![0u16; MAX_PATH_LENGTH];
		let succeeded =
			self.send_message(message, buffer.len(), buffer.as_mut_ptr() as LPARAM) != 0;
		path_from_wide(&buffer).filter(|_| succeeded)
	}

	/// Extension of the active document including the leading dot, e.g. `.md`.
	pub fn current_file_extension(&self) -> String {
		let mut buffer = [0u16; 1024];
		self.send_message(
			npp_message::GET_EXTENSION_PART,
			buffer.len(),
			buffer.as_mut_ptr() as LPARAM,
		);
		from_wide_until_null(&buffer)
	}

	/// Opens a new, empty document and makes it the active one.
	pub fn new_document(&self) {
		self.send_message(npp_message::MENU_COMMAND, 0, menu_command::FILE_NEW as LPARAM);
	}

	/// Sets the language of the active document, see [`crate::ffi::language`].
	pub fn set_current_language(&self, language: isize) {
		self.send_message(npp_message::SET_CURRENT_LANG_TYPE, 0, language);
	}

	/// Opens `path` in Notepad++ or switches to it if it is open already.
	pub fn open_file(&self, path: &Path) -> bool {
		let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
		self.send_message(npp_message::DO_OPEN, 0, wide_path.as_ptr() as LPARAM) != 0
	}

	/// A new hidden Scintilla editor for processing text. Notepad++ destroys it on exit.
	pub fn create_hidden_scintilla(&self) -> Option<Scintilla> {
		let window =
			self.send_message(npp_message::CREATE_SCINTILLA_HANDLE, 0, self.window() as LPARAM);
		(window != 0).then(|| Scintilla::new(window as HWND))
	}

	/// A new instance of the Lexilla lexer `lexer_name` (e.g. `cpp`) for
	/// [`Scintilla::set_lexer`], or null if there is no such lexer.
	pub fn create_lexer(&self, lexer_name: &str) -> *mut c_void {
		let name = to_wide_null_terminated(lexer_name);
		self.send_message(npp_message::CREATE_LEXER, 0, name.as_ptr() as LPARAM) as *mut c_void
	}

	pub fn is_dark_mode_enabled(&self) -> bool {
		self.send_message(npp_message::IS_DARK_MODE_ENABLED, 0, 0) != 0
	}

	pub fn uses_large_toolbar_icons(&self) -> bool {
		matches!(
			self.send_message(npp_message::GET_TOOLBAR_ICON_SET_CHOICE, 0, 0),
			toolbar_icon_set::FLUENT_LARGE | toolbar_icon_set::FILLED_FLUENT_LARGE
		)
	}

	pub fn dpi(&self) -> u32 {
		// SAFETY: `GetDpiForWindow` accepts any handle and returns 0 for invalid ones.
		match unsafe { GetDpiForWindow(self.handles.npp_handle) } {
			0 => DEFAULT_DPI,
			dpi => dpi,
		}
	}

	pub fn scale_for_dpi(&self, size_at_default_dpi: u32) -> u32 {
		(size_at_default_dpi * self.dpi() + DEFAULT_DPI / 2) / DEFAULT_DPI
	}

	/// Applies Notepad++'s dark or light theme to a plugin dialog. Call after its controls exist.
	pub fn apply_theme_to_dialog(&self, dialog: HWND) {
		self.send_message(
			npp_message::DARK_MODE_SUBCLASS_AND_THEME,
			dark_mode_flags::INIT,
			dialog as LPARAM,
		);
	}

	/// Applies a changed dark or light mode to a window themed with
	/// [`Self::apply_theme_to_dialog`] before.
	pub fn update_dialog_theme(&self, dialog: HWND) {
		self.send_message(
			npp_message::DARK_MODE_SUBCLASS_AND_THEME,
			dark_mode_flags::HANDLE_CHANGE,
			dialog as LPARAM,
		);
	}

	/// Notepad++ keeps using the handles. They must stay alive until it exits.
	pub fn add_toolbar_icon(&self, command_id: i32, icons: &ToolbarIconsWithDarkMode) {
		self.send_message(
			npp_message::ADD_TOOLBAR_ICON_FOR_DARK_MODE,
			command_id as WPARAM,
			icons as *const ToolbarIconsWithDarkMode as LPARAM,
		);
	}

	/// Checks the plugin menu item and toolbar button of `command_id`.
	pub fn set_menu_item_checked(&self, command_id: i32, is_checked: bool) {
		self.send_message(
			npp_message::SET_MENU_ITEM_CHECK,
			command_id as WPARAM,
			LPARAM::from(is_checked),
		);
	}

	/// Registers `panel.client` as dockable panel. Notepad++ copies `panel` but keeps its string
	/// pointers, so they must live as long as the DLL.
	pub fn register_docking_panel(&self, panel: &DockedWidgetData) {
		self.send_message(
			npp_message::DOCKING_REGISTER,
			0,
			panel as *const DockedWidgetData as LPARAM,
		);
	}

	/// Lets `window` and its children get their keys (Tab, Ctrl+C, ...) before Notepad++'s
	/// accelerators. Unregister it before it is destroyed.
	pub fn register_modeless_window(&self, window: HWND, is_registered: bool) {
		let action = match is_registered {
			true => modeless_dialog::ADD,
			false => modeless_dialog::REMOVE,
		};
		self.send_message(npp_message::MODELESS_DIALOG, action, window as LPARAM);
	}

	/// The colors of the dark mode, also when it is not enabled.
	pub fn dark_mode_colors(&self) -> Option<DarkModeColors> {
		let mut colors = DarkModeColors::default();
		let succeeded = self.send_message(
			npp_message::GET_DARK_MODE_COLORS,
			size_of::<DarkModeColors>(),
			&raw mut colors as LPARAM,
		) != 0;
		succeeded.then_some(colors)
	}

	pub fn show_docking_panel(&self, client: HWND) {
		self.send_message(npp_message::DOCKING_SHOW, 0, client as LPARAM);
	}

	pub fn hide_docking_panel(&self, client: HWND) {
		self.send_message(npp_message::DOCKING_HIDE, 0, client as LPARAM);
	}

	pub fn show_message(&self, title: &str, text: &str, kind: MessageKind) {
		show_message_box(title, text, kind);
	}
}

fn path_from_wide(buffer: &[u16]) -> Option<PathBuf> {
	let length = buffer.iter().position(|&unit| unit == 0)?;
	(length > 0).then(|| PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn show_message_box(title: &str, text: &str, kind: MessageKind) {
	let icon: MESSAGEBOX_STYLE = match kind {
		MessageKind::Information => MB_ICONINFORMATION,
		MessageKind::Warning => MB_ICONWARNING,
		MessageKind::Error => MB_ICONERROR,
	};
	let title = to_wide_null_terminated(title);
	let text = to_wide_null_terminated(text);
	// Message boxes owned by the Notepad++ window get a far too tall gray button area. Without an
	// owner they lay out correctly, and `MB_TASKMODAL` still disables Notepad++ while open.
	// SAFETY: Both strings are NUL terminated and outlive the modal call.
	unsafe {
		MessageBoxW(
			core::ptr::null_mut(),
			text.as_ptr(),
			title.as_ptr(),
			MB_OK | MB_TASKMODAL | icon,
		)
	};
}
