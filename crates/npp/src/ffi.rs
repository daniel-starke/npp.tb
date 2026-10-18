//! Notepad++ plugin ABI (`PluginInterface.h`, `Notepad_plus_msgs.h`, `Scintilla.h`).
//!
//! Only the subset used by the framework is declared, with speaking names. The values come from
//! the generated [`crate::interface`] (see `cargo xtask update-npp-interface`), so a header
//! change that renames or removes something breaks the build here. Struct layouts are written by
//! hand and must match the declarations listed at the end of `interface.rs`.

use crate::interface::{
	docking as dock, docking_resource, menu_cmd_id, notepad_plus_msgs as npp, plugin_interface,
	scintilla as sci,
};

/// Notepad++ messages (`NPPM_*`) sent to the main Notepad++ window.
pub mod npp_message {
	use super::npp;

	/// `lParam`: `*mut i32` receiving 0 (main view) or 1 (second view).
	pub const GET_CURRENT_SCINTILLA: u32 = npp::NPPM_GETCURRENTSCINTILLA;
	/// `wParam`: buffer length in UTF-16 units, `lParam`: buffer.
	pub const GET_PLUGINS_CONFIG_DIR: u32 = npp::NPPM_GETPLUGINSCONFIGDIR;
	/// `wParam`: command ID, `lParam`: `*const ToolbarIconsWithDarkMode`.
	pub const ADD_TOOLBAR_ICON_FOR_DARK_MODE: u32 = npp::NPPM_ADDTOOLBARICON_FORDARKMODE;
	pub const IS_DARK_MODE_ENABLED: u32 = npp::NPPM_ISDARKMODEENABLED;
	/// `wParam`: dark mode flags, `lParam`: dialog window handle.
	pub const DARK_MODE_SUBCLASS_AND_THEME: u32 = npp::NPPM_DARKMODESUBCLASSANDTHEME;
	/// Returns 0..=4, see [`super::toolbar_icon_set`].
	pub const GET_TOOLBAR_ICON_SET_CHOICE: u32 = npp::NPPM_GETTOOLBARICONSETCHOICE;
	/// `wParam`: buffer length in UTF-16 units, `lParam`: buffer. Includes the leading dot.
	pub const GET_EXTENSION_PART: u32 = npp::NPPM_GETEXTPART;
	/// `wParam`: buffer length in UTF-16 units, `lParam`: buffer. The path, or the tab name of a
	/// new document such as `new 1`.
	pub const GET_FULL_CURRENT_PATH: u32 = npp::NPPM_GETFULLCURRENTPATH;
	/// `wParam`: buffer length in UTF-16 units, `lParam`: buffer.
	pub const GET_FILE_NAME: u32 = npp::NPPM_GETFILENAME;
	/// `wParam`: buffer length in UTF-16 units, `lParam`: buffer; returns the length without NUL
	/// when `lParam` is 0.
	pub const GET_NPP_SETTINGS_DIR_PATH: u32 = npp::NPPM_GETNPPSETTINGSDIRPATH;
	/// `wParam`: buffer length in UTF-16 units, `lParam`: buffer.
	pub const GET_NPP_DIRECTORY: u32 = npp::NPPM_GETNPPDIRECTORY;
	/// `lParam`: menu command ID such as [`super::menu_command::FILE_NEW`].
	pub const MENU_COMMAND: u32 = npp::NPPM_MENUCOMMAND;
	/// `lParam`: [`super::language`] of the current document.
	pub const SET_CURRENT_LANG_TYPE: u32 = npp::NPPM_SETCURRENTLANGTYPE;
	/// `lParam`: NUL terminated UTF-16 path. Opens the file or switches to it.
	pub const DO_OPEN: u32 = npp::NPPM_DOOPEN;
	/// `lParam`: parent window. Returns a new Scintilla window owned and destroyed by Notepad++.
	pub const CREATE_SCINTILLA_HANDLE: u32 = npp::NPPM_CREATESCINTILLAHANDLE;
	/// `lParam`: NUL terminated UTF-16 Lexilla lexer name. Returns an `ILexer5*` for
	/// `SCI_SETILEXER`, or null.
	pub const CREATE_LEXER: u32 = npp::NPPM_CREATELEXER;
	/// `lParam`: `*mut DockedWidgetData`, copied by Notepad++. Registers a dockable panel.
	pub const DOCKING_REGISTER: u32 = npp::NPPM_DMMREGASDCKDLG;
	/// `lParam`: client window of a registered panel.
	pub const DOCKING_SHOW: u32 = npp::NPPM_DMMSHOW;
	/// `lParam`: client window of a registered panel.
	pub const DOCKING_HIDE: u32 = npp::NPPM_DMMHIDE;
	/// `wParam`: [`super::modeless_dialog`] action, `lParam`: window. Registered windows get
	/// their keyboard input through `IsDialogMessage` before Notepad++'s accelerators.
	pub const MODELESS_DIALOG: u32 = npp::NPPM_MODELESSDIALOG;
	/// `wParam`: size of [`super::DarkModeColors`], `lParam`: `*mut DarkModeColors`.
	pub const GET_DARK_MODE_COLORS: u32 = npp::NPPM_GETDARKMODECOLORS;
	/// `wParam`: command ID, `lParam`: checked (1) or not (0).
	pub const SET_MENU_ITEM_CHECK: u32 = npp::NPPM_SETMENUITEMCHECK;
}

/// Menu command IDs for [`npp_message::MENU_COMMAND`].
pub mod menu_command {
	use super::menu_cmd_id;

	pub const FILE_NEW: usize = menu_cmd_id::IDM_FILE_NEW as usize;
}

/// Document languages (`LangType`) for [`npp_message::SET_CURRENT_LANG_TYPE`].
pub mod language {
	use super::npp::lang_type;

	pub const HTML: isize = lang_type::L_HTML as isize;
}

/// Actions of [`npp_message::MODELESS_DIALOG`].
pub mod modeless_dialog {
	pub const ADD: usize = super::npp::MODELESSDIALOGADD as usize;
	pub const REMOVE: usize = super::npp::MODELESSDIALOGREMOVE as usize;
}

/// Flags for [`npp_message::DARK_MODE_SUBCLASS_AND_THEME`].
pub mod dark_mode_flags {
	/// Standard flags for a dialog after its children are initialized.
	pub const INIT: usize = super::npp::dmfInit as usize;
	/// Standard flags after Notepad++ switched between dark and light mode.
	pub const HANDLE_CHANGE: usize = super::npp::dmfHandleChange as usize;
}

/// Values returned by [`npp_message::GET_TOOLBAR_ICON_SET_CHOICE`]. The header documents them
/// in a comment only, so they cannot be generated.
pub mod toolbar_icon_set {
	pub const FLUENT_SMALL: isize = 0;
	pub const FLUENT_LARGE: isize = 1;
	pub const FILLED_FLUENT_SMALL: isize = 2;
	pub const FILLED_FLUENT_LARGE: isize = 3;
	pub const STANDARD_SMALL: isize = 4;
}

/// Notepad++ notification codes (`NPPN_*`) passed to `beNotified`.
pub mod npp_notification {
	use super::npp;

	pub const READY: u32 = npp::NPPN_READY;
	pub const TOOLBAR_MODIFICATION: u32 = npp::NPPN_TBMODIFICATION;
	pub const SHUTDOWN: u32 = npp::NPPN_SHUTDOWN;
	/// Another document became the active one, also when switching views.
	pub const BUFFER_ACTIVATED: u32 = npp::NPPN_BUFFERACTIVATED;
	pub const DARK_MODE_CHANGED: u32 = npp::NPPN_DARKMODECHANGED;
}

/// Scintilla notification codes (`SCN_*`) passed to `beNotified` with the editor as sender.
pub mod scintilla_notification {
	use super::sci;

	/// [`super::SciNotification::modification_type`] tells what changed.
	pub const MODIFIED: u32 = sci::SCN_MODIFIED;
	/// [`super::SciNotification::updated`] tells what changed.
	pub const UPDATE_UI: u32 = sci::SCN_UPDATEUI;
}

/// Flags of [`super::SciNotification::modification_type`] and [`super::SciNotification::updated`].
pub mod scintilla_change {
	use super::sci;

	pub const INSERT_TEXT: i32 = sci::SC_MOD_INSERTTEXT as i32;
	pub const DELETE_TEXT: i32 = sci::SC_MOD_DELETETEXT as i32;
	pub const SELECTION_UPDATED: i32 = sci::SC_UPDATE_SELECTION as i32;
}

/// Scintilla messages (`SCI_*`) sent to an editor window.
pub mod scintilla_message {
	use super::sci;

	pub const GET_LENGTH: u32 = sci::SCI_GETLENGTH;
	pub const GET_CURRENT_POS: u32 = sci::SCI_GETCURRENTPOS;
	pub const GOTO_POS: u32 = sci::SCI_GOTOPOS;
	pub const GET_EOL_MODE: u32 = sci::SCI_GETEOLMODE;
	pub const GET_TEXT_RANGE_FULL: u32 = sci::SCI_GETTEXTRANGEFULL;
	pub const BEGIN_UNDO_ACTION: u32 = sci::SCI_BEGINUNDOACTION;
	pub const END_UNDO_ACTION: u32 = sci::SCI_ENDUNDOACTION;
	pub const GET_TAB_WIDTH: u32 = sci::SCI_GETTABWIDTH;
	pub const GET_LINE_END_POSITION: u32 = sci::SCI_GETLINEENDPOSITION;
	pub const GET_CODE_PAGE: u32 = sci::SCI_GETCODEPAGE;
	pub const GET_READ_ONLY: u32 = sci::SCI_GETREADONLY;
	pub const GET_SELECTION_START: u32 = sci::SCI_GETSELECTIONSTART;
	pub const GET_SELECTION_END: u32 = sci::SCI_GETSELECTIONEND;
	pub const GET_LINE_COUNT: u32 = sci::SCI_GETLINECOUNT;
	pub const SET_SEL: u32 = sci::SCI_SETSEL;
	pub const LINE_FROM_POSITION: u32 = sci::SCI_LINEFROMPOSITION;
	pub const POSITION_FROM_LINE: u32 = sci::SCI_POSITIONFROMLINE;
	pub const REPLACE_TARGET: u32 = sci::SCI_REPLACETARGET;
	pub const COUNT_CHARACTERS: u32 = sci::SCI_COUNTCHARACTERS;
	pub const GET_SELECTION_EMPTY: u32 = sci::SCI_GETSELECTIONEMPTY;
	pub const POSITION_RELATIVE: u32 = sci::SCI_POSITIONRELATIVE;
	pub const SET_TARGET_RANGE: u32 = sci::SCI_SETTARGETRANGE;
	pub const SET_CODE_PAGE: u32 = sci::SCI_SETCODEPAGE;
	/// `lParam`: NUL terminated text in the document encoding.
	pub const SET_TEXT: u32 = sci::SCI_SETTEXT;
	pub const SET_UNDO_COLLECTION: u32 = sci::SCI_SETUNDOCOLLECTION;
	pub const EMPTY_UNDO_BUFFER: u32 = sci::SCI_EMPTYUNDOBUFFER;
	/// `lParam`: `ILexer5*`, Scintilla takes ownership.
	pub const SET_ILEXER: u32 = sci::SCI_SETILEXER;
	/// `wParam`: keyword set index, `lParam`: NUL terminated, space separated keywords.
	pub const SET_KEYWORDS: u32 = sci::SCI_SETKEYWORDS;
	/// `wParam`, `lParam`: NUL terminated key and value.
	pub const SET_PROPERTY: u32 = sci::SCI_SETPROPERTY;
	/// `wParam`: start, `lParam`: end or -1. Styles the range with the lexer.
	pub const COLOURISE: u32 = sci::SCI_COLOURISE;
	/// `lParam`: [`TextRangeFull`] whose buffer holds `2 * length + 2` bytes, receiving each
	/// byte followed by its style.
	pub const GET_STYLED_TEXT_FULL: u32 = sci::SCI_GETSTYLEDTEXTFULL;
	/// `wParam`: style, `lParam`: buffer or null to get the length. Space separated tags.
	pub const TAGS_OF_STYLE: u32 = sci::SCI_TAGSOFSTYLE;
}

/// Scintilla constants used as message arguments or results.
pub mod scintilla_constant {
	use super::sci;

	pub const CODE_PAGE_UTF8: usize = sci::SC_CP_UTF8 as usize;
	pub const EOL_CRLF: isize = sci::SC_EOL_CRLF as isize;
	pub const EOL_CR: isize = sci::SC_EOL_CR as isize;
	pub const EOL_LF: isize = sci::SC_EOL_LF as isize;
}

/// Docking panel constants (`Docking.h`, `dockingResource.h`).
pub mod docking {
	use super::{dock, docking_resource};

	/// [`super::DockedWidgetData::mask`]: docked on the right side by default.
	pub const DEFAULT_RIGHT: u32 = dock::DWS_DF_CONT_RIGHT;
	/// [`super::DockedWidgetData::mask`]: `icon_tab` is set.
	pub const HAS_TAB_ICON: u32 = dock::DWS_ICONTAB;
	/// [`super::DockedWidgetData::mask`]: the plugin applies the dark mode itself.
	pub const USES_OWN_DARK_MODE: u32 = dock::DWS_USEOWNDARKMODE;
	/// `NMHDR::code` of the `WM_NOTIFY` sent to the client window when the user closes the panel.
	pub const CLOSED: u32 = docking_resource::DMN_CLOSE;
}

/// Maximum menu item name length including the terminating NUL (`menuItemSize`).
pub const MENU_ITEM_NAME_CAPACITY: usize = plugin_interface::menuItemSize as usize;

#[cfg(windows)]
pub use self::windows_types::*;

#[cfg(windows)]
mod windows_types {
	use core::ffi::c_char;
	use windows_sys::Win32::Foundation::{HWND, RECT};
	use windows_sys::Win32::Graphics::Gdi::HBITMAP;
	use windows_sys::Win32::UI::WindowsAndMessaging::HICON;

	/// `PFUNCPLUGINCMD`: menu command callback (`__cdecl`).
	pub type PluginCommandFunction = extern "C" fn();

	#[repr(C)]
	#[derive(Clone, Copy, Debug)]
	pub struct NppData {
		pub npp_handle: HWND,
		pub scintilla_main_handle: HWND,
		pub scintilla_second_handle: HWND,
	}

	#[repr(C)]
	#[derive(Clone, Copy, Debug, Default)]
	pub struct ShortcutKey {
		pub is_ctrl: bool,
		pub is_alt: bool,
		pub is_shift: bool,
		/// Virtual key code.
		pub key: u8,
	}

	/// A menu entry. A null `function` together with an empty name is a separator.
	/// Notepad++ writes `command_id` after `getFuncsArray` returned, so the array must stay alive
	/// and at a fixed address for the lifetime of the DLL.
	#[repr(C)]
	pub struct FuncItem {
		pub item_name: [u16; super::MENU_ITEM_NAME_CAPACITY],
		pub function: Option<PluginCommandFunction>,
		pub command_id: i32,
		pub init_to_check: bool,
		pub shortcut_key: *mut ShortcutKey,
	}

	#[repr(C)]
	#[derive(Clone, Copy, Debug)]
	pub struct ToolbarIconsWithDarkMode {
		pub toolbar_bitmap: HBITMAP,
		pub toolbar_icon: HICON,
		pub toolbar_icon_dark_mode: HICON,
	}

	/// `NMHDR`, the first member of `SCNotification`.
	#[repr(C)]
	#[derive(Clone, Copy, Debug)]
	pub struct NotificationHeader {
		pub hwnd_from: HWND,
		pub id_from: usize,
		pub code: u32,
	}

	/// `SCNotification`. Notepad++ passes this full structure for its own (`NPPN_*`)
	/// notifications as well, so reading all fields is always valid.
	#[repr(C)]
	#[derive(Clone, Copy, Debug)]
	pub struct SciNotification {
		pub header: NotificationHeader,
		pub position: isize,
		pub ch: i32,
		pub modifiers: i32,
		/// `SC_MOD_*` flags of `SCN_MODIFIED`.
		pub modification_type: i32,
		pub text: *const c_char,
		pub length: isize,
		pub lines_added: isize,
		pub message: i32,
		pub wparam: usize,
		pub lparam: isize,
		pub line: isize,
		pub fold_level_now: i32,
		pub fold_level_previous: i32,
		pub margin: i32,
		pub list_type: i32,
		pub x: i32,
		pub y: i32,
		pub token: i32,
		pub annotation_lines_added: isize,
		/// `SC_UPDATE_*` flags of `SCN_UPDATEUI`.
		pub updated: i32,
		pub list_completion_method: i32,
		pub character_source: i32,
	}

	/// `NppDarkMode::Colors` of [`super::npp_message::GET_DARK_MODE_COLORS`]. Only documented in a
	/// comment of `Notepad_plus_msgs.h`, so it cannot be snapshotted.
	#[repr(C)]
	#[derive(Clone, Copy, Debug, Default)]
	pub struct DarkModeColors {
		pub background: u32,
		pub softer_background: u32,
		pub hot_background: u32,
		pub pure_background: u32,
		pub error_background: u32,
		pub text: u32,
		pub darker_text: u32,
		pub disabled_text: u32,
		pub link_text: u32,
		pub edge: u32,
		pub hot_edge: u32,
		pub disabled_edge: u32,
	}

	/// `DockedWidgetData` (`tTbData`) for [`super::npp_message::DOCKING_REGISTER`]. Notepad++
	/// copies the structure but keeps the string pointers, so they must live as long as the DLL.
	#[repr(C)]
	pub struct DockedWidgetData {
		pub client: HWND,
		pub name: *const u16,
		/// Index of the menu entry that shows the panel. Notepad++ runs it on startup to restore a
		/// panel that was open on exit.
		pub dialog_id: i32,
		/// [`super::docking`] flags.
		pub mask: u32,
		pub icon_tab: HICON,
		pub additional_info: *const u16,
		/// Internal to Notepad++.
		pub float_rectangle: RECT,
		/// Internal to Notepad++.
		pub previous_container: i32,
		/// The plugin's DLL file name, e.g. `NppToolBox.dll`.
		pub module_name: *const u16,
	}

	/// `Sci_TextRangeFull`; `text` must hold `max - min + 1` bytes.
	#[repr(C)]
	pub struct TextRangeFull {
		pub min: isize,
		pub max: isize,
		pub text: *mut c_char,
	}
}
