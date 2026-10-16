//! Declarative plugin definition and the glue behind the exported plugin ABI.
//!
//! A plugin declares a `static` [`PluginDefinition`] and invokes [`export_plugin!`](crate::export_plugin)
//! once. Notepad++ calls the exported functions on its UI thread only.

use std::sync::OnceLock;

use crate::ffi::{
	FuncItem, MENU_ITEM_NAME_CAPACITY, NotificationHeader, NppData, PluginCommandFunction,
	ShortcutKey, npp_notification,
};
use crate::icon::ToolbarIcon;
use crate::notepad::Notepad;
use crate::toolbar_icon_handles::create_toolbar_icon_handles;
use crate::wide::{to_fixed_wide, to_wide_null_terminated};

/// Upper bound of menu entries. Each needs its own `extern "C"` trampoline.
pub const MAX_MENU_ENTRIES: usize = 32;

/// Toolbar icon size at 100 % scaling for the small and the large Notepad++ toolbar.
const SMALL_TOOLBAR_ICON_SIZE: u32 = 16;
const LARGE_TOOLBAR_ICON_SIZE: u32 = 32;

pub struct PluginDefinition {
	/// Shown as plugin menu title. Notepad++ requires the DLL to be named `<name>.dll`.
	pub name: &'static str,
	pub menu: &'static [MenuEntry],
}

pub enum MenuEntry {
	Command(Command),
	Separator,
}

pub struct Command {
	pub menu_name: &'static str,
	pub run: fn(&Notepad),
	pub shortcut: Option<Shortcut>,
	pub toolbar_icon: Option<&'static ToolbarIcon>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
	pub ctrl: bool,
	pub alt: bool,
	pub shift: bool,
	/// Virtual key code. For letters and digits this equals the uppercase ASCII character.
	pub key: u8,
}

impl Shortcut {
	pub const fn ctrl_alt(key: u8) -> Self {
		Self { ctrl: true, alt: true, shift: false, key }
	}

	pub const fn ctrl_alt_shift(key: u8) -> Self {
		Self { ctrl: true, alt: true, shift: true, key }
	}
}

struct PluginState {
	definition: &'static PluginDefinition,
	notepad: Notepad,
}

/// Owner of the `FuncItem` array handed to Notepad++, which writes the command IDs into it.
struct MenuItems(*mut FuncItem, usize);

// SAFETY: The array is leaked (never freed) and only accessed from Notepad++'s UI thread.
unsafe impl Send for MenuItems {}
// SAFETY: See `Send`.
unsafe impl Sync for MenuItems {}

static PLUGIN_STATE: OnceLock<PluginState> = OnceLock::new();
static PLUGIN_NAME: OnceLock<Vec<u16>> = OnceLock::new();
static MENU_ITEMS: OnceLock<MenuItems> = OnceLock::new();

/// `FuncItem` carries no context pointer, so every menu entry gets its own monomorphized
/// trampoline that knows its index.
extern "C" fn run_menu_entry<const INDEX: usize>() {
	let Some(state) = PLUGIN_STATE.get() else { return };
	if let Some(MenuEntry::Command(command)) = state.definition.menu.get(INDEX) {
		(command.run)(&state.notepad);
	}
}

macro_rules! menu_entry_trampolines {
	($($index:literal)*) => {
		[$(run_menu_entry::<$index> as PluginCommandFunction),*]
	};
}

const MENU_ENTRY_TRAMPOLINES: [PluginCommandFunction; MAX_MENU_ENTRIES] = menu_entry_trampolines!(
	0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
);

/// Implementation of the exported functions. Only meant to be called by `export_plugin!`.
#[doc(hidden)]
pub mod exports {
	use super::*;

	pub fn set_info(definition: &'static PluginDefinition, handles: NppData) {
		assert!(definition.menu.len() <= MAX_MENU_ENTRIES, "too many menu entries");
		let notepad = Notepad::new(handles);
		crate::panic_message::install(notepad, definition.name);
		let _ = PLUGIN_STATE.set(PluginState { definition, notepad });
	}

	pub fn name(definition: &'static PluginDefinition) -> *const u16 {
		PLUGIN_NAME.get_or_init(|| to_wide_null_terminated(definition.name)).as_ptr()
	}

	/// # Safety
	/// `count` must be null or valid for writes.
	pub unsafe fn menu_items(
		definition: &'static PluginDefinition,
		count: *mut i32,
	) -> *mut FuncItem {
		let items = MENU_ITEMS.get_or_init(|| {
			let items: Box<[FuncItem]> = definition
				.menu
				.iter()
				.enumerate()
				.map(|(index, entry)| to_func_item(index, entry))
				.collect();
			let length = items.len();
			MenuItems(Box::leak(items).as_mut_ptr(), length)
		});
		if !count.is_null() {
			// SAFETY: Guaranteed by the caller.
			unsafe { *count = items.1 as i32 };
		}
		items.0
	}

	/// # Safety
	/// `notification` must be null or point to a valid `SCNotification`.
	pub unsafe fn notify(notification: *const NotificationHeader) {
		let Some(state) = PLUGIN_STATE.get() else { return };
		// SAFETY: Guaranteed by the caller.
		let Some(header) = (unsafe { notification.as_ref() }) else { return };
		if header.hwnd_from != state.notepad.window() {
			return;
		}
		if header.code == npp_notification::TOOLBAR_MODIFICATION {
			register_toolbar_icons(state);
		}
	}

	fn to_func_item(index: usize, entry: &MenuEntry) -> FuncItem {
		let (item_name, function, shortcut_key) = match entry {
			MenuEntry::Separator => ([0; MENU_ITEM_NAME_CAPACITY], None, core::ptr::null_mut()),
			MenuEntry::Command(command) => (
				to_fixed_wide(command.menu_name),
				Some(MENU_ENTRY_TRAMPOLINES[index]),
				command.shortcut.map_or(core::ptr::null_mut(), |shortcut| {
					// Notepad++ keeps the pointer, so the key lives as long as the DLL.
					Box::leak(Box::new(ShortcutKey {
						is_ctrl: shortcut.ctrl,
						is_alt: shortcut.alt,
						is_shift: shortcut.shift,
						key: shortcut.key,
					})) as *mut ShortcutKey
				}),
			),
		};
		FuncItem { item_name, function, command_id: 0, init_to_check: false, shortcut_key }
	}

	fn register_toolbar_icons(state: &PluginState) {
		let Some(items) = MENU_ITEMS.get() else { return };
		let notepad = &state.notepad;
		let bitmap_size = notepad.scale_for_dpi(SMALL_TOOLBAR_ICON_SIZE);
		let icon_size = notepad.scale_for_dpi(if notepad.uses_large_toolbar_icons() {
			LARGE_TOOLBAR_ICON_SIZE
		} else {
			SMALL_TOOLBAR_ICON_SIZE
		});
		for (index, entry) in state.definition.menu.iter().enumerate().take(items.1) {
			let MenuEntry::Command(Command { toolbar_icon: Some(icon), .. }) = entry else {
				continue;
			};
			// SAFETY: `index < items.1`. Notepad++ has filled in the command IDs by now.
			let command_id = unsafe { (*items.0.add(index)).command_id };
			if let Some(handles) = create_toolbar_icon_handles(icon, bitmap_size, icon_size) {
				notepad.add_toolbar_icon(command_id, &handles);
			}
		}
	}
}

/// Exports the Notepad++ plugin ABI (`setInfo`, `getName`, `getFuncsArray`, `beNotified`,
/// `messageProc`, `isUnicode`) for the given `static` [`PluginDefinition`].
#[macro_export]
macro_rules! export_plugin {
	($definition:path) => {
		#[allow(non_snake_case)]
		#[unsafe(no_mangle)]
		pub extern "C" fn isUnicode() -> i32 {
			1
		}

		#[allow(non_snake_case)]
		#[unsafe(no_mangle)]
		pub extern "C" fn setInfo(handles: $crate::ffi::NppData) {
			$crate::plugin::exports::set_info(&$definition, handles);
		}

		#[allow(non_snake_case)]
		#[unsafe(no_mangle)]
		pub extern "C" fn getName() -> *const u16 {
			$crate::plugin::exports::name(&$definition)
		}

		/// # Safety
		/// Called by Notepad++ with a valid out pointer.
		#[allow(non_snake_case)]
		#[unsafe(no_mangle)]
		pub unsafe extern "C" fn getFuncsArray(count: *mut i32) -> *mut $crate::ffi::FuncItem {
			// SAFETY: Notepad++ passes a valid pointer.
			unsafe { $crate::plugin::exports::menu_items(&$definition, count) }
		}

		/// # Safety
		/// Called by Notepad++ with a valid `SCNotification`.
		#[allow(non_snake_case)]
		#[unsafe(no_mangle)]
		pub unsafe extern "C" fn beNotified(notification: *const $crate::ffi::NotificationHeader) {
			// SAFETY: Notepad++ passes a valid notification.
			unsafe { $crate::plugin::exports::notify(notification) }
		}

		#[allow(non_snake_case)]
		#[unsafe(no_mangle)]
		pub extern "C" fn messageProc(_message: u32, _wparam: usize, _lparam: isize) -> isize {
			1
		}
	};
}
