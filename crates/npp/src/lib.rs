//! Minimal framework for Notepad++ plugins written in Rust.
//!
//! The platform independent modules (`icon`, `interface`, `line_ending`, `wide`, `ffi` constants) compile
//! everywhere so that plugin logic can be unit tested on any host. Everything touching Win32 is
//! only available on Windows.

pub mod ffi;
pub mod icon;
pub mod interface;
pub mod line_ending;
pub mod wide;

#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
mod dialog_template;
#[cfg(windows)]
pub mod ini_file;
#[cfg(windows)]
pub mod input_dialog;
#[cfg(windows)]
pub mod notepad;
#[cfg(windows)]
mod panic_message;
#[cfg(windows)]
pub mod plugin;
#[cfg(windows)]
pub mod scintilla;
#[cfg(windows)]
mod toolbar_icon_handles;

pub use icon::{IconImage, ToolbarIcon};
pub use line_ending::LineEnding;

#[cfg(windows)]
pub use notepad::{MessageKind, Notepad};
#[cfg(windows)]
pub use plugin::{Command, MenuEntry, PluginDefinition, Shortcut};
#[cfg(windows)]
pub use scintilla::Scintilla;
