//! The Markdown viewer: a live preview of the active document in a docked panel or a separate
//! window, shown with WebView2, EdgeHTML or RichEdit.
//!
//! Notepad++ calls the plugin on its UI thread only, so the state lives in a thread local. Many
//! Win32 and Notepad++ calls send messages back to the viewer windows, so the state is borrowed
//! only briefly and never across such calls. Window procedures that find it borrowed post their
//! work for later or leave the message to the default handling.

mod engine;
mod window;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use npp::ffi::{SciNotification, npp_notification, scintilla_change, scintilla_notification};
use npp::{MessageKind, Notepad};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, WindowFromPoint};

use crate::PLUGIN_NAME;
use crate::commands::{read_setting, settings_file};
use crate::image_embedding::ImageEmbedding;
use crate::lexer_highlighter::LexerHighlighter;
use crate::settings::{self, Setting};
use crate::viewer_page::{LinkTarget, link_base_url};
use engine::{Engine, EngineKind, ImageCache, RenderInput};
use window::{Timer, ViewerWindows};

thread_local! {
	// Clippy 1.99 reports the initializer although it is `const` already.
	#[allow(clippy::missing_const_for_thread_local)]
	static VIEWER: RefCell<Option<Viewer>> = const { RefCell::new(None) };
}

/// Delay between a caret move and following it in the viewer.
const SYNC_DELAY_MILLISECONDS: u32 = 30;

/// Interval of following the mouse while the link bar is shown. RichEdit does not report leaving
/// a link, and the bar stays while the mouse is over it.
const LINK_HOVER_CHECK_MILLISECONDS: u32 = 100;

/// Opened with the default application instead of Notepad++ when a link points to them.
const NON_TEXT_EXTENSIONS: [&str; 30] = [
	"png", "jpg", "jpeg", "gif", "bmp", "ico", "svg", "webp", "avif", "tif", "tiff", "pdf", "doc",
	"docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "zip", "7z", "rar", "gz", "exe", "msi",
	"htm", "html", "mp3", "mp4",
];

struct Viewer {
	notepad: Notepad,
	windows: ViewerWindows,
	engine: Engine,
	is_visible: bool,
	is_detached: bool,
	follows_caret: bool,
	is_dark: bool,
	/// Cleared when switching documents.
	images: ImageCache,
	highlighter: LexerHighlighter,
	/// The location of the hovered link, shown in the link bar.
	hovered_link: Option<String>,
}

/// Runs `action` with the viewer, unless there is none or it is borrowed further up the stack.
fn with_viewer<R>(action: impl FnOnce(&mut Viewer) -> R) -> Option<R> {
	VIEWER.with(|cell| {
		let mut viewer = cell.try_borrow_mut().ok()?;
		viewer.as_mut().map(action)
	})
}

fn setting(notepad: &Notepad, setting: &Setting) -> u32 {
	read_setting(settings_file(notepad).as_ref(), setting)
}

fn write_setting(notepad: &Notepad, setting: &Setting, value: u32) {
	if let Some(file) = settings_file(notepad) {
		file.write_u32(setting.section, setting.key, value);
	}
}

pub fn toggle(notepad: &Notepad) {
	match with_viewer(|viewer| viewer.is_visible) {
		Some(true) => hide(),
		Some(false) => show(),
		None => {
			if create(notepad) {
				show();
			}
		}
	}
}

fn create(notepad: &Notepad) -> bool {
	let is_dark = notepad.is_dark_mode_enabled();
	let follows_caret = setting(notepad, &settings::VIEWER_FOLLOWS_CARET) == 1;
	let is_detached = setting(notepad, &settings::VIEWER_DETACHED) == 1;
	let Some(windows) = ViewerWindows::create(notepad, follows_caret, is_detached) else {
		notepad.show_message(PLUGIN_NAME, "Cannot create the viewer window.", MessageKind::Error);
		return false;
	};
	let engine_kind = EngineKind::from_setting(setting(notepad, &settings::VIEWER_ENGINE));
	let engine = Engine::create(windows.host(), engine_kind, is_dark);
	VIEWER.with(|cell| {
		*cell.borrow_mut() = Some(Viewer {
			notepad: *notepad,
			windows,
			engine,
			is_visible: false,
			is_detached,
			follows_caret,
			is_dark,
			images: ImageCache::default(),
			highlighter: LexerHighlighter::new(notepad),
			hovered_link: None,
		});
	});
	window::apply_theme(notepad, windows.host(), false);
	true
}

fn show() {
	let Some((notepad, mut windows, is_detached)) = with_viewer(|viewer| {
		viewer.is_visible = true;
		(viewer.notepad, viewer.windows, viewer.is_detached)
	}) else {
		return;
	};
	if is_detached {
		windows.open_frame(&notepad);
	} else {
		windows.show_docked(&notepad);
	}
	with_viewer(|viewer| {
		viewer.windows = windows;
		viewer.engine.parent_changed();
		viewer.windows.layout(&viewer.engine);
	});
	set_menu_checked(&notepad, true);
	// Showing a panel focuses it, but the user usually goes on writing.
	window::focus(notepad.current_scintilla().window());
	windows.start_timer(Timer::Render, 0);
}

fn hide() {
	let Some((notepad, mut windows, is_detached)) = with_viewer(|viewer| {
		viewer.is_visible = false;
		viewer.hovered_link = None;
		viewer.windows.show_link_bar(None);
		(viewer.notepad, viewer.windows, viewer.is_detached)
	}) else {
		return;
	};
	if is_detached {
		windows.close_frame();
	} else {
		notepad.hide_docking_panel(windows.panel());
	}
	with_viewer(|viewer| viewer.windows = windows);
	set_menu_checked(&notepad, false);
}

/// The user closed the docked panel.
fn panel_closed() {
	let notepad = with_viewer(|viewer| {
		// Hiding the panel to detach the viewer reports a close as well.
		let is_closed = viewer.is_visible && !viewer.is_detached;
		if is_closed {
			viewer.is_visible = false;
		}
		is_closed.then_some(viewer.notepad)
	});
	if let Some(Some(notepad)) = notepad {
		set_menu_checked(&notepad, false);
	}
}

fn set_menu_checked(notepad: &Notepad, is_checked: bool) {
	if let Some(command_id) = npp::plugin::command_id_of(crate::commands::markdown_viewer) {
		notepad.set_menu_item_checked(command_id, is_checked);
	}
}

/// Moves the viewer between the docked panel and its own window.
fn toggle_detached() {
	let Some((notepad, mut windows, is_visible, is_detached)) = with_viewer(|viewer| {
		viewer.is_detached = !viewer.is_detached;
		(viewer.notepad, viewer.windows, viewer.is_visible, viewer.is_detached)
	}) else {
		return;
	};
	write_setting(&notepad, &settings::VIEWER_DETACHED, u32::from(is_detached));
	if is_visible {
		if is_detached {
			notepad.hide_docking_panel(windows.panel());
			windows.open_frame(&notepad);
		} else {
			windows.close_frame();
			windows.show_docked(&notepad);
		}
	}
	with_viewer(|viewer| {
		viewer.windows = windows;
		viewer.windows.set_detached(is_detached);
		viewer.engine.parent_changed();
		viewer.windows.layout(&viewer.engine);
	});
	windows.start_timer(Timer::Render, 0);
}

fn toggle_follows_caret() {
	let Some((notepad, windows, follows_caret)) = with_viewer(|viewer| {
		viewer.follows_caret = !viewer.follows_caret;
		if viewer.follows_caret {
			viewer.sync();
		} else {
			viewer.engine.clear_selection();
		}
		(viewer.notepad, viewer.windows, viewer.follows_caret)
	}) else {
		return;
	};
	windows.set_follows_caret(follows_caret);
	write_setting(&notepad, &settings::VIEWER_FOLLOWS_CARET, u32::from(follows_caret));
}

pub fn on_notification(notepad: &Notepad, notification: &SciNotification) {
	let header = &notification.header;
	let code = header.code;
	if notepad.is_editor_window(header.hwnd_from) {
		if code == scintilla_notification::MODIFIED {
			let changes = scintilla_change::INSERT_TEXT | scintilla_change::DELETE_TEXT;
			if notification.modification_type & changes != 0 {
				schedule(notepad, Timer::Render);
			}
		} else if code == scintilla_notification::UPDATE_UI
			&& notification.updated & scintilla_change::SELECTION_UPDATED != 0
		{
			schedule(notepad, Timer::Sync);
		}
		return;
	}
	if header.hwnd_from != notepad.window() {
		return;
	}
	match code {
		npp_notification::BUFFER_ACTIVATED => {
			with_viewer(|viewer| viewer.images.clear());
			schedule(notepad, Timer::Render);
		}
		npp_notification::DARK_MODE_CHANGED => {
			let is_dark = notepad.is_dark_mode_enabled();
			// Notepad++ themes the RichEdit as well, so the engine colors are set afterwards.
			if let Some(windows) = with_viewer(|viewer| viewer.windows) {
				window::apply_theme(notepad, windows.host(), true);
				windows.set_dark(is_dark);
				with_viewer(|viewer| {
					viewer.is_dark = is_dark;
					viewer.engine.set_dark(is_dark);
				});
				schedule(notepad, Timer::Render);
			}
		}
		npp_notification::SHUTDOWN => {
			let viewer =
				VIEWER.with(|cell| cell.try_borrow_mut().ok().and_then(|mut viewer| viewer.take()));
			if let Some(viewer) = viewer {
				viewer.windows.destroy(notepad);
				drop(viewer);
			}
		}
		_ => {}
	}
}

/// Starts the render or sync timer of a visible viewer, restarting it on every change.
fn schedule(notepad: &Notepad, timer: Timer) {
	let Some((windows, follows_caret)) =
		with_viewer(|viewer| viewer.is_visible.then_some((viewer.windows, viewer.follows_caret)))
			.flatten()
	else {
		return;
	};
	match timer {
		Timer::Render => {
			let delay = setting(notepad, &settings::VIEWER_UPDATE_DELAY);
			windows.start_timer(timer, delay);
		}
		Timer::Sync if follows_caret => windows.start_timer(timer, SYNC_DELAY_MILLISECONDS),
		Timer::Sync | Timer::LinkHover => {}
	}
}

/// Runs a timer of the host window. Returns whether the timer is done.
fn timer_elapsed(timer: Timer) -> bool {
	if timer == Timer::LinkHover {
		return with_viewer(Viewer::follow_link_hover).unwrap_or(false);
	}
	with_viewer(|viewer| {
		if viewer.is_visible {
			match timer {
				Timer::Render => viewer.render(),
				Timer::Sync => viewer.sync(),
				Timer::LinkHover => {}
			}
		}
	})
	.is_some()
}

/// The mouse moved over a link, or left it with an empty [`LinkSource::Resolved`].
fn link_hovered(link: LinkSource) {
	with_viewer(|viewer| {
		let location = match link {
			LinkSource::Resolved(uri) if uri.is_empty() => None,
			LinkSource::Resolved(uri) => viewer.engine.link_location(&uri),
			LinkSource::Characters(range) => {
				let directory = viewer.document_directory();
				viewer.engine.rich_edit_link_location_at(range, directory.as_deref())
			}
		};
		viewer.show_hovered_link(location);
	});
}

/// A link was activated in the viewer. Opening files and programs happens without the borrow,
/// as Notepad++ notifies the viewer when a document opens.
fn link_activated(link: LinkSource) {
	let Some((notepad, target)) = with_viewer(|viewer| {
		let target = match link {
			LinkSource::Resolved(uri) => viewer.engine.resolved_link_target(&uri),
			LinkSource::Characters(range) => {
				viewer.engine.link_target_at(range, viewer.document_directory().as_deref())
			}
		};
		if let Some(LinkTarget::Anchor(anchor)) = &target {
			viewer.engine.scroll_to_anchor(anchor);
		}
		(viewer.notepad, target)
	}) else {
		return;
	};
	match target {
		Some(LinkTarget::External(url)) => window::open_with_default_application(url.as_ref()),
		Some(LinkTarget::File(path)) => open_file(&notepad, &path),
		Some(LinkTarget::Anchor(_) | LinkTarget::Ignored) | None => {}
	}
}

enum LinkSource {
	/// A URI as the browser reports it.
	Resolved(String),
	/// The RichEdit characters of a clicked or hovered link.
	Characters(std::ops::Range<usize>),
}

fn open_file(notepad: &Notepad, path: &Path) {
	if !path.exists() {
		let text = format!("Cannot find {}.", path.display());
		notepad.show_message(PLUGIN_NAME, &text, MessageKind::Warning);
		return;
	}
	let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("");
	let is_non_text = NON_TEXT_EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(extension));
	if path.is_dir() || is_non_text {
		window::open_with_default_application(path.as_os_str());
	} else if !notepad.open_file(path) {
		let text = format!("Cannot open {}.", path.display());
		notepad.show_message(PLUGIN_NAME, &text, MessageKind::Error);
	}
}

impl Viewer {
	fn document_directory(&self) -> Option<PathBuf> {
		self.notepad.current_file_path()?.parent().map(Path::to_path_buf)
	}

	/// Shows `location` in the link bar, or hides the bar for `None`. The bar stays while the
	/// mouse is over it, as it may cover the link, which would show it again.
	fn show_hovered_link(&mut self, location: Option<String>) {
		if self.hovered_link == location || !self.is_visible {
			return;
		}
		self.hovered_link = location;
		if self.hovered_link.is_none() && self.windows.is_mouse_over_link_bar() {
			return;
		}
		self.windows.show_link_bar(self.hovered_link.as_deref());
		self.windows.layout(&self.engine);
		if self.hovered_link.is_some() {
			self.windows.start_timer(Timer::LinkHover, LINK_HOVER_CHECK_MILLISECONDS);
		}
	}

	/// Follows the mouse while the link bar is shown. Returns whether the bar is hidden.
	fn follow_link_hover(&mut self) -> bool {
		let mut point = POINT { x: 0, y: 0 };
		// SAFETY: Valid output point.
		unsafe { GetCursorPos(&mut point) };
		if let Some(control) = self.engine.rich_edit_control() {
			// SAFETY: No preconditions.
			let is_over_control = unsafe { WindowFromPoint(point) } == control.window();
			let directory = self.document_directory();
			let location = is_over_control
				.then(|| self.engine.rich_edit_link_location(point, directory.as_deref()))
				.flatten();
			self.show_hovered_link(location);
		}
		if self.hovered_link.is_none() && !self.windows.is_mouse_over_link_bar() {
			self.windows.show_link_bar(None);
			self.windows.layout(&self.engine);
		}
		!self.windows.is_link_bar_visible()
	}

	fn render(&mut self) {
		let editor = self.notepad.current_scintilla();
		let markdown = editor.whole_text();
		let directory = self.document_directory();
		let max_image_bytes = setting(&self.notepad, &settings::MAX_EMBEDDED_IMAGE_BYTES);
		let highlights_code = setting(&self.notepad, &settings::HIGHLIGHT_CODE) == 1;
		let highlighter = &mut self.highlighter;
		let mut highlight_code = |language: &str, code: &str| match highlights_code {
			true => highlighter.highlight(language, code),
			false => Vec::new(),
		};
		self.engine.show(&mut RenderInput {
			markdown: &markdown,
			embedding: ImageEmbedding {
				base_directory: directory.as_deref(),
				max_image_bytes: u64::from(max_image_bytes),
			},
			images: &mut self.images,
			highlight_code: &mut highlight_code,
			base_url: link_base_url(directory.as_deref()),
		});
		if self.is_detached {
			self.windows.set_frame_title(&self.notepad.current_file_name());
		}
		if self.follows_caret {
			self.sync();
		}
	}

	/// Scrolls to the caret and marks the selected blocks.
	fn sync(&mut self) {
		let editor = self.notepad.current_scintilla();
		let caret_line = editor.line_of_position(editor.caret());
		let selected_lines = (!editor.is_selection_empty()).then(|| {
			let selection = editor.selection();
			editor.line_of_position(selection.start)..editor.line_of_position(selection.end)
		});
		self.engine.sync(caret_line, selected_lines);
	}
}
