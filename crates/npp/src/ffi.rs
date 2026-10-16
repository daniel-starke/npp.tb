//! Notepad++ plugin ABI (`PluginInterface.h`, `Notepad_plus_msgs.h`, `Scintilla.h`).
//!
//! Only the subset used by the framework is declared, with speaking names. The values come from
//! the generated [`crate::interface`] (see `cargo xtask update-npp-interface`), so a header
//! change that renames or removes something breaks the build here. Struct layouts are written by
//! hand and must match the declarations listed at the end of `interface.rs`.

use crate::interface::{menu_cmd_id, notepad_plus_msgs as npp, plugin_interface, scintilla as sci};

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

/// Flags for [`npp_message::DARK_MODE_SUBCLASS_AND_THEME`].
pub mod dark_mode_flags {
	/// Standard flags for a dialog after its children are initialized.
	pub const INIT: usize = super::npp::dmfInit as usize;
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

/// Maximum menu item name length including the terminating NUL (`menuItemSize`).
pub const MENU_ITEM_NAME_CAPACITY: usize = plugin_interface::menuItemSize as usize;

#[cfg(windows)]
pub use self::windows_types::*;

#[cfg(windows)]
mod windows_types {
	use core::ffi::c_char;
	use windows_sys::Win32::Foundation::HWND;
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

	/// `NMHDR`, the first member of `SCNotification`. Only the header is declared because the
	/// framework never reads past it. The full structure is larger.
	#[repr(C)]
	#[derive(Clone, Copy, Debug)]
	pub struct NotificationHeader {
		pub hwnd_from: HWND,
		pub id_from: usize,
		pub code: u32,
	}

	/// `Sci_TextRangeFull`; `text` must hold `max - min + 1` bytes.
	#[repr(C)]
	pub struct TextRangeFull {
		pub min: isize,
		pub max: isize,
		pub text: *mut c_char,
	}
}
