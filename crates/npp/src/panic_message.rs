//! Reports panics to the user before the process aborts.
//!
//! The workspace builds with `panic = "abort"`: unwinding into Notepad++ would be undefined
//! behavior. The hook only shows the message and location; it deliberately captures no backtrace.

use std::sync::OnceLock;

use crate::notepad::{MessageKind, Notepad};

pub(crate) fn install(notepad: Notepad, plugin_name: &'static str) {
	static INSTALLED: OnceLock<()> = OnceLock::new();
	INSTALLED.get_or_init(|| {
		std::panic::set_hook(Box::new(move |panic_info| {
			let text = format!(
				"{plugin_name} hit an internal error and Notepad++ has to close.\n\n{panic_info}"
			);
			notepad.show_message(plugin_name, &text, MessageKind::Error);
		}));
	});
}
