// The library name must equal the plugin name ("NppToolBox") because Notepad++ loads
// plugins/<Name>/<Name>.dll. rustc applies the snake case lint to crate names.
#![allow(non_snake_case)]
// The editing logic is platform independent so that it can be unit tested on any host. Only the
// Windows build wires it up.
#![cfg_attr(not(windows), allow(dead_code))]

mod base64;
mod clipboard_html;
mod code_highlight;
mod emoji;
mod emoji_table;
mod image_embedding;
#[cfg(windows)]
mod lexer_highlighter;
mod markdown_html;
mod markdown_table;
mod rewrap;
mod settings;
mod text_width;

#[cfg(windows)]
mod commands;

#[cfg(windows)]
mod toolbar_icons {
	include!(concat!(env!("OUT_DIR"), "/toolbar_icons.rs"));
}

#[cfg(windows)]
use npp::{Command, MenuEntry, PluginDefinition, Shortcut};

#[cfg(windows)]
pub const PLUGIN_NAME: &str = "NppToolBox";

#[cfg(windows)]
static PLUGIN: PluginDefinition = PluginDefinition {
	name: PLUGIN_NAME,
	menu: &[
		MenuEntry::Command(Command {
			menu_name: "Realign Markdown Table",
			run: commands::realign_markdown_table,
			shortcut: Some(Shortcut::ctrl_alt(b'T')),
			toolbar_icon: Some(&toolbar_icons::MARKDOWN_TABLE),
		}),
		MenuEntry::Command(Command {
			menu_name: "Rewrap...",
			run: commands::rewrap,
			shortcut: Some(Shortcut::ctrl_alt(b'W')),
			toolbar_icon: Some(&toolbar_icons::REWRAP),
		}),
		MenuEntry::Separator,
		MenuEntry::Command(Command {
			menu_name: "Render HTML",
			run: commands::render_html,
			shortcut: Some(Shortcut::ctrl_alt(b'H')),
			toolbar_icon: Some(&toolbar_icons::RENDER_HTML),
		}),
		MenuEntry::Command(Command {
			menu_name: "Copy as HTML",
			run: commands::copy_as_html,
			shortcut: Some(Shortcut::ctrl_alt_shift(b'H')),
			toolbar_icon: Some(&toolbar_icons::COPY_AS_HTML),
		}),
		MenuEntry::Separator,
		MenuEntry::Command(Command {
			menu_name: "Settings",
			run: commands::settings,
			shortcut: None,
			toolbar_icon: None,
		}),
		MenuEntry::Command(Command {
			menu_name: "About",
			run: commands::about,
			shortcut: None,
			toolbar_icon: None,
		}),
	],
};

#[cfg(windows)]
npp::export_plugin!(PLUGIN);
