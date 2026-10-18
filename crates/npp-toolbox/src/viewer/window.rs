//! The viewer windows: the docking panel registered with Notepad++, the separate top-level
//! window, and the host inside one of them with the buttons and the engine.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};

use npp::Notepad;
use npp::clipboard::{ClipboardContent, set_clipboard};
use npp::ffi::{DockedWidgetData, docking};
use npp::webview2::{ContextMenuRequest as WebView2ContextMenu, HRESULT};
use npp::wide::to_wide_null_terminated;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{COLOR_BTNFACE, CreateFontIndirectW, DeleteObject, HFONT};
use windows_sys::Win32::System::SystemServices::{
	SS_CENTERIMAGE, SS_LEFT, SS_NOPREFIX, SS_PATHELLIPSIS,
};
use windows_sys::Win32::UI::Controls::NMHDR;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
	AppendMenuW, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CW_USEDEFAULT, CreatePopupMenu,
	CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GW_CHILD, GetClientRect,
	GetCursorPos, GetWindow, GetWindowRect, HMENU, IDC_ARROW, IsWindowVisible, KillTimer,
	LoadCursorW, MF_GRAYED, MF_STRING, MoveWindow, NONCLIENTMETRICSW, PostMessageW,
	RegisterClassExW, SPI_GETNONCLIENTMETRICS, SW_HIDE, SW_SHOW, SW_SHOWNORMAL, SWP_FRAMECHANGED,
	SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetParent, SetTimer, SetWindowPos,
	SetWindowTextW, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_CLOSE,
	WM_COMMAND, WM_CONTEXTMENU, WM_MOVE, WM_NOTIFY, WM_SETFOCUS, WM_SETFONT, WM_SIZE, WM_TIMER,
	WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
	WS_VISIBLE,
};

use super::engine::{BrowserView, Engine};
use super::{
	LinkSource, hide, link_activated, link_hovered, panel_closed, timer_elapsed, toggle_detached,
	toggle_follows_caret, with_viewer,
};
use crate::clipboard_html::clipboard_html;
use crate::viewer_page::ContextMenuRequest;

const PANEL_CLASS: &str = "NppToolBoxViewerPanel";
const HOST_CLASS: &str = "NppToolBoxViewerHost";
const FRAME_CLASS: &str = "NppToolBoxViewerWindow";
const TITLE: &str = "Markdown Viewer";

const FOLLOW_CARET_ID: usize = 1;
const DETACH_ID: usize = 2;

/// Posted to the host with a boxed [`ViewerMessage`].
const WM_VIEWER_MESSAGE: u32 = WM_APP + 1;
/// Posted to the host when a resize found the viewer busy.
const WM_VIEWER_LAYOUT: u32 = WM_APP + 2;

/// Heights and widths at 96 DPI.
const BUTTON_BAR_HEIGHT: i32 = 28;
const BUTTON_HEIGHT: i32 = 22;
const FOLLOW_CARET_WIDTH: i32 = 100;
const DETACH_WIDTH: i32 = 76;
const MARGIN: i32 = 6;
const LINK_BAR_HEIGHT: i32 = 22;
const FRAME_WIDTH: i32 = 820;
const FRAME_HEIGHT: i32 = 900;

/// Milliseconds after the last size change before RichEdit renders for the new width.
const RESIZE_RENDER_DELAY: u32 = 150;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
	Render = 1,
	Sync = 2,
	/// Follows the mouse while the link bar is shown.
	LinkHover = 3,
}

/// Events from the browser, handled when the message loop delivers them.
pub enum ViewerMessage {
	WebViewCreated(Result<BrowserView, HRESULT>),
	DocumentLoaded,
	LinkActivated(String),
	/// The URI of the link under the mouse, empty when it left the link.
	LinkHovered(String),
	ContextMenu(ContextMenu),
}

/// A request for the context menu, which looks the same for every engine.
pub enum ContextMenu {
	/// The `lParam` of `WM_CONTEXTMENU`: the screen position, -1 from the keyboard.
	RichEdit(LPARAM),
	WebView2(WebView2ContextMenu),
	/// EdgeHTML shows no context menu itself.
	EdgeHtml(ContextMenuRequest),
}

#[derive(Clone, Copy)]
pub struct ViewerWindows {
	panel: HWND,
	host: HWND,
	follow_caret_button: HWND,
	detach_button: HWND,
	/// Shows the location of the hovered link below the engine, hidden otherwise.
	link_bar: HWND,
	font: HFONT,
	/// The separate window, null while docked or hidden.
	frame: HWND,
	is_docking_registered: bool,
}

impl ViewerWindows {
	pub fn create(notepad: &Notepad, follows_caret: bool, is_detached: bool) -> Option<Self> {
		register_classes();
		let panel =
			create_window(0, PANEL_CLASS, TITLE, WS_CHILD | WS_CLIPCHILDREN, notepad.window(), 0)?;
		let host =
			create_window(0, HOST_CLASS, "", WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN, panel, 0)?;
		let button_style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
		let follow_caret_button = create_window(
			0,
			"BUTTON",
			"Follow caret",
			button_style | BS_AUTOCHECKBOX as u32,
			host,
			FOLLOW_CARET_ID,
		)?;
		let detach_button =
			create_window(0, "BUTTON", "", button_style | BS_PUSHBUTTON as u32, host, DETACH_ID)?;
		let link_bar = create_window(
			0,
			"STATIC",
			"",
			WS_CHILD | SS_LEFT | SS_CENTERIMAGE | SS_PATHELLIPSIS | SS_NOPREFIX,
			host,
			0,
		)?;
		let font = message_font(host);
		for control in [follow_caret_button, detach_button, link_bar] {
			send(control, WM_SETFONT, font as WPARAM, 0);
		}
		let windows = Self {
			panel,
			host,
			follow_caret_button,
			detach_button,
			link_bar,
			font,
			frame: core::ptr::null_mut(),
			is_docking_registered: false,
		};
		windows.set_follows_caret(follows_caret);
		windows.set_detached(is_detached);
		notepad.register_modeless_window(host, true);
		Some(windows)
	}

	pub fn host(&self) -> HWND {
		self.host
	}

	pub fn panel(&self) -> HWND {
		self.panel
	}

	pub fn set_follows_caret(&self, follows_caret: bool) {
		send(self.follow_caret_button, BM_SETCHECK, WPARAM::from(follows_caret), 0);
	}

	pub fn set_detached(&self, is_detached: bool) {
		let text = to_wide_null_terminated(if is_detached { "Dock" } else { "Detach" });
		// SAFETY: Valid window, NUL terminated text.
		unsafe { SetWindowTextW(self.detach_button, text.as_ptr()) };
	}

	/// Follows a change between dark and light mode in the title bar of the separate window.
	pub fn set_dark(&self, is_dark: bool) {
		if !self.frame.is_null() {
			set_dark_title_bar(self.frame, is_dark);
		}
	}

	pub fn set_frame_title(&self, file_name: &str) {
		if self.frame.is_null() {
			return;
		}
		let title = to_wide_null_terminated(&format!("{file_name} - {TITLE}"));
		// SAFETY: Valid window, NUL terminated text.
		unsafe { SetWindowTextW(self.frame, title.as_ptr()) };
	}

	/// Shows the host in the docking panel, registering the panel on first use.
	pub fn show_docked(&mut self, notepad: &Notepad) {
		self.move_host_to(self.panel);
		if self.is_docking_registered {
			notepad.show_docking_panel(self.panel);
			return;
		}
		self.is_docking_registered = true;
		// Notepad++ keeps the strings for the lifetime of the process.
		let name = Box::leak(to_wide_null_terminated(TITLE).into_boxed_slice());
		let module_name = Box::leak(to_wide_null_terminated("NppToolBox.dll").into_boxed_slice());
		let menu_index = npp::plugin::menu_index_of(crate::commands::markdown_viewer).unwrap_or(0);
		let panel = DockedWidgetData {
			client: self.panel,
			name: name.as_ptr(),
			dialog_id: menu_index as i32,
			mask: docking::DEFAULT_RIGHT | docking::USES_OWN_DARK_MODE,
			icon_tab: core::ptr::null_mut(),
			additional_info: core::ptr::null(),
			float_rectangle: RECT { left: 0, top: 0, right: 0, bottom: 0 },
			previous_container: 0,
			module_name: module_name.as_ptr(),
		};
		notepad.register_docking_panel(&panel);
		notepad.show_docking_panel(self.panel);
	}

	/// Shows the host in a new top-level window owned by Notepad++.
	pub fn open_frame(&mut self, notepad: &Notepad) {
		if self.frame.is_null() {
			let scale = |size: i32| scale_for_dpi(notepad.window(), size);
			// SAFETY: The class is registered, the owner is the Notepad++ window.
			let frame = unsafe {
				let class = to_wide_null_terminated(FRAME_CLASS);
				let title = to_wide_null_terminated(TITLE);
				CreateWindowExW(
					WS_EX_APPWINDOW,
					class.as_ptr(),
					title.as_ptr(),
					WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
					CW_USEDEFAULT,
					CW_USEDEFAULT,
					scale(FRAME_WIDTH),
					scale(FRAME_HEIGHT),
					notepad.window(),
					core::ptr::null_mut(),
					npp::plugin::module_handle(),
					core::ptr::null(),
				)
			};
			if frame.is_null() {
				return;
			}
			self.frame = frame;
			notepad.apply_theme_to_dialog(frame);
			set_dark_title_bar(frame, notepad.is_dark_mode_enabled());
		}
		self.move_host_to(self.frame);
		// SAFETY: Valid window.
		unsafe { ShowWindow(self.frame, SW_SHOWNORMAL) };
	}

	/// Moves the host back into the hidden panel and destroys the separate window.
	pub fn close_frame(&mut self) {
		if self.frame.is_null() {
			return;
		}
		self.move_host_to(self.panel);
		// SAFETY: The frame was created by this module and is not used afterwards.
		unsafe { DestroyWindow(self.frame) };
		self.frame = core::ptr::null_mut();
	}

	fn move_host_to(&self, parent: HWND) {
		// SAFETY: Both are valid windows of this thread.
		unsafe {
			SetParent(self.host, parent);
			ShowWindow(self.host, SW_SHOW);
		}
		fill_with_first_child(parent);
	}

	/// Shows `location` in the link bar, or hides the bar for `None`. Needs a [`Self::layout`]
	/// afterwards.
	pub fn show_link_bar(&self, location: Option<&str>) {
		if let Some(location) = location {
			let text = to_wide_null_terminated(location);
			// SAFETY: Valid window, NUL terminated text.
			unsafe { SetWindowTextW(self.link_bar, text.as_ptr()) };
		}
		// SAFETY: Valid window.
		unsafe { ShowWindow(self.link_bar, if location.is_some() { SW_SHOW } else { SW_HIDE }) };
	}

	pub fn is_link_bar_visible(&self) -> bool {
		// SAFETY: Valid window.
		unsafe { IsWindowVisible(self.link_bar) != 0 }
	}

	/// Whether the mouse is over the visible link bar, which may cover the hovered link.
	pub fn is_mouse_over_link_bar(&self) -> bool {
		let mut point = POINT { x: 0, y: 0 };
		let mut bounds = RECT { left: 0, top: 0, right: 0, bottom: 0 };
		// SAFETY: Valid window and output structures.
		unsafe {
			GetCursorPos(&mut point);
			GetWindowRect(self.link_bar, &mut bounds);
		}
		self.is_link_bar_visible()
			&& (bounds.left..bounds.right).contains(&point.x)
			&& (bounds.top..bounds.bottom).contains(&point.y)
	}

	pub fn layout(&self, engine: &Engine) {
		let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
		// SAFETY: Valid window and output rectangle.
		unsafe { GetClientRect(self.host, &mut client) };
		let scale = |size: i32| scale_for_dpi(self.host, size);
		let bar_height = scale(BUTTON_BAR_HEIGHT);
		let button_top = (bar_height - scale(BUTTON_HEIGHT)) / 2;
		let follow_caret_width = scale(FOLLOW_CARET_WIDTH);
		move_window(
			self.follow_caret_button,
			RECT {
				left: scale(MARGIN),
				top: button_top,
				right: scale(MARGIN) + follow_caret_width,
				bottom: button_top + scale(BUTTON_HEIGHT),
			},
		);
		let detach_left = scale(MARGIN) * 2 + follow_caret_width;
		move_window(
			self.detach_button,
			RECT {
				left: detach_left,
				top: button_top,
				right: detach_left + scale(DETACH_WIDTH),
				bottom: button_top + scale(BUTTON_HEIGHT),
			},
		);
		let mut engine_bounds = RECT { top: bar_height.min(client.bottom), ..client };
		if self.is_link_bar_visible() {
			let link_bar_top = (client.bottom - scale(LINK_BAR_HEIGHT)).max(engine_bounds.top);
			move_window(self.link_bar, RECT { left: scale(MARGIN), top: link_bar_top, ..client });
			engine_bounds.bottom = link_bar_top;
		}
		engine.set_bounds(engine_bounds);
	}

	pub fn start_timer(&self, timer: Timer, milliseconds: u32) {
		// SAFETY: Valid window. Starting an existing timer again restarts it.
		unsafe { SetTimer(self.host, timer as usize, milliseconds, None) };
	}

	/// Destroys all windows on shutdown.
	pub fn destroy(&self, notepad: &Notepad) {
		notepad.register_modeless_window(self.host, false);
		// SAFETY: The windows were created by this module and are not used afterwards. The
		// panel is destroyed with Notepad++.
		unsafe {
			if !self.frame.is_null() {
				DestroyWindow(self.frame);
			}
			DestroyWindow(self.host);
			DeleteObject(self.font);
		}
	}
}

/// Themes the host and its buttons like Notepad++, in dark or light mode.
pub fn apply_theme(notepad: &Notepad, host: HWND, is_change: bool) {
	match is_change {
		true => notepad.update_dialog_theme(host),
		false => notepad.apply_theme_to_dialog(host),
	}
}

pub fn post(host: HWND, message: ViewerMessage) {
	let message = Box::into_raw(Box::new(message));
	// SAFETY: The host takes ownership of the box when it handles the message.
	if unsafe { PostMessageW(host, WM_VIEWER_MESSAGE, 0, message as LPARAM) } == 0 {
		// SAFETY: Not posted, so still owned here.
		drop(unsafe { Box::from_raw(message) });
	}
}

pub fn move_window(window: HWND, bounds: RECT) {
	// SAFETY: Valid window.
	unsafe {
		MoveWindow(
			window,
			bounds.left,
			bounds.top,
			bounds.right - bounds.left,
			bounds.bottom - bounds.top,
			1,
		)
	};
}

pub fn focus(window: HWND) {
	// SAFETY: Valid window of this thread.
	unsafe { SetFocus(window) };
}

/// Opens a URL or file with its default application.
pub fn open_with_default_application(target: &OsStr) {
	let operation = to_wide_null_terminated("open");
	let target: Vec<u16> = target.encode_wide().chain(Some(0)).collect();
	// SAFETY: All strings are NUL terminated and alive for the call.
	unsafe {
		ShellExecuteW(
			core::ptr::null_mut(),
			operation.as_ptr(),
			target.as_ptr(),
			core::ptr::null(),
			core::ptr::null(),
			SW_SHOWNORMAL,
		)
	};
}

fn window_dpi(window: HWND) -> u32 {
	// SAFETY: Returns 0 for invalid windows.
	match unsafe { GetDpiForWindow(window) } {
		0 => npp::notepad::DEFAULT_DPI,
		dpi => dpi,
	}
}

fn scale_for_dpi(window: HWND, size: i32) -> i32 {
	size * window_dpi(window) as i32 / npp::notepad::DEFAULT_DPI as i32
}

fn send(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
	// SAFETY: Valid window. Pointer arguments are valid at every call site.
	unsafe { SendMessageW(window, message, wparam, lparam) }
}

/// The Windows message font at the DPI of `window`.
fn message_font(window: HWND) -> HFONT {
	let mut metrics =
		NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
	// SAFETY: `metrics` has its size set and is valid for writes.
	unsafe {
		SystemParametersInfoForDpi(
			SPI_GETNONCLIENTMETRICS,
			metrics.cbSize,
			(&raw mut metrics).cast(),
			0,
			window_dpi(window),
		);
		CreateFontIndirectW(&metrics.lfMessageFont)
	}
}

fn create_window(
	extended_style: u32,
	class: &str,
	title: &str,
	style: u32,
	parent: HWND,
	control_id: usize,
) -> Option<HWND> {
	let class = to_wide_null_terminated(class);
	let title = to_wide_null_terminated(title);
	// SAFETY: The strings are NUL terminated, the parent is valid.
	let window = unsafe {
		CreateWindowExW(
			extended_style,
			class.as_ptr(),
			title.as_ptr(),
			style,
			0,
			0,
			0,
			0,
			parent,
			control_id as HMENU,
			npp::plugin::module_handle(),
			core::ptr::null(),
		)
	};
	(!window.is_null()).then_some(window)
}

fn register_classes() {
	static IS_REGISTERED: AtomicBool = AtomicBool::new(false);
	if IS_REGISTERED.swap(true, Ordering::Relaxed) {
		return;
	}
	let classes: [(&str, WindowProcedure); 3] = [
		(PANEL_CLASS, panel_procedure),
		(HOST_CLASS, host_procedure),
		(FRAME_CLASS, frame_procedure),
	];
	for (name, procedure) in classes {
		let name = to_wide_null_terminated(name);
		let class = WNDCLASSEXW {
			cbSize: size_of::<WNDCLASSEXW>() as u32,
			lpfnWndProc: Some(procedure),
			hInstance: npp::plugin::module_handle(),
			// SAFETY: Loading a system cursor has no preconditions.
			hCursor: unsafe { LoadCursorW(core::ptr::null_mut(), IDC_ARROW) },
			hbrBackground: (COLOR_BTNFACE + 1) as usize as _,
			lpszClassName: name.as_ptr(),
			..Default::default()
		};
		// SAFETY: The class description is complete. Windows copies the name.
		unsafe { RegisterClassExW(&class) };
	}
}

type WindowProcedure = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn default_procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
	// SAFETY: Forwards the arguments Windows passed.
	unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// The panel and the frame only contain the host, which fills them.
fn fill_with_first_child(window: HWND) {
	// SAFETY: Valid window.
	let child = unsafe { GetWindow(window, GW_CHILD) };
	if child.is_null() {
		return;
	}
	let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
	// SAFETY: Valid window and output rectangle.
	unsafe { GetClientRect(window, &mut client) };
	move_window(child, client);
}

fn focus_engine() {
	with_viewer(|viewer| viewer.engine.focus());
}

unsafe extern "system" fn panel_procedure(
	window: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> LRESULT {
	match message {
		WM_SIZE => fill_with_first_child(window),
		WM_SETFOCUS => focus_engine(),
		WM_NOTIFY => {
			// SAFETY: `WM_NOTIFY` carries an `NMHDR`.
			let header = unsafe { &*(lparam as *const NMHDR) };
			if header.code == docking::CLOSED {
				panel_closed();
			}
			return default_procedure(window, message, wparam, lparam);
		}
		_ => return default_procedure(window, message, wparam, lparam),
	}
	0
}

unsafe extern "system" fn frame_procedure(
	window: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> LRESULT {
	match message {
		WM_SIZE => fill_with_first_child(window),
		WM_SETFOCUS => focus_engine(),
		WM_MOVE => {
			with_viewer(|viewer| viewer.engine.parent_moved());
		}
		WM_CLOSE => hide(),
		_ => return default_procedure(window, message, wparam, lparam),
	}
	0
}

unsafe extern "system" fn host_procedure(
	window: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> LRESULT {
	match message {
		WM_SIZE | WM_VIEWER_LAYOUT => {
			let laid_out = with_viewer(|viewer| {
				viewer.windows.layout(&viewer.engine);
				// RTF tables and pictures are as wide as the view was.
				(viewer.is_visible && viewer.engine.rich_edit_control().is_some())
					.then_some(viewer.windows)
			});
			match laid_out {
				Some(Some(windows)) => windows.start_timer(Timer::Render, RESIZE_RENDER_DELAY),
				Some(None) => {}
				None if message == WM_SIZE => {
					// SAFETY: Valid window.
					unsafe { PostMessageW(window, WM_VIEWER_LAYOUT, 0, 0) };
				}
				None => {}
			}
		}
		WM_SETFOCUS => focus_engine(),
		WM_TIMER => {
			let timer = match wparam {
				timer if timer == Timer::Render as usize => Timer::Render,
				timer if timer == Timer::Sync as usize => Timer::Sync,
				timer if timer == Timer::LinkHover as usize => Timer::LinkHover,
				_ => return default_procedure(window, message, wparam, lparam),
			};
			// A busy viewer keeps the timer for a retry.
			if timer_elapsed(timer) {
				// SAFETY: Valid window and timer.
				unsafe { KillTimer(window, wparam) };
			}
		}
		WM_COMMAND => match wparam & 0xffff {
			FOLLOW_CARET_ID => toggle_follows_caret(),
			DETACH_ID => toggle_detached(),
			_ => return default_procedure(window, message, wparam, lparam),
		},
		WM_NOTIFY => {
			// SAFETY: Windows passes the `NMHDR` of the notification.
			if let Some(characters) = unsafe { npp::rich_edit::clicked_link(lparam) } {
				link_activated(LinkSource::Characters(characters));
			// SAFETY: As above.
			} else if let Some(characters) = unsafe { npp::rich_edit::hovered_link(lparam) } {
				link_hovered(LinkSource::Characters(characters));
			} else {
				return default_procedure(window, message, wparam, lparam);
			}
		}
		WM_CONTEXTMENU => show_context_menu(window, ContextMenu::RichEdit(lparam)),
		WM_VIEWER_MESSAGE => {
			// SAFETY: Posted by `post`, which gave up the ownership of the box.
			let message = unsafe { Box::from_raw(lparam as *mut ViewerMessage) };
			handle_viewer_message(window, *message);
		}
		_ => return default_procedure(window, message, wparam, lparam),
	}
	0
}

fn handle_viewer_message(host: HWND, message: ViewerMessage) {
	match message {
		ViewerMessage::WebViewCreated(result) => {
			let needs_render = with_viewer(|viewer| {
				let is_fallback = viewer.engine.webview_created(result);
				viewer.windows.layout(&viewer.engine);
				(is_fallback || viewer.is_visible).then_some(viewer.windows)
			});
			if let Some(Some(windows)) = needs_render {
				windows.start_timer(Timer::Render, 0);
			}
		}
		ViewerMessage::DocumentLoaded => {
			with_viewer(|viewer| viewer.engine.document_loaded());
		}
		ViewerMessage::LinkActivated(uri) => link_activated(LinkSource::Resolved(uri)),
		ViewerMessage::LinkHovered(uri) => link_hovered(LinkSource::Resolved(uri)),
		ViewerMessage::ContextMenu(menu) => show_context_menu(host, menu),
	}
}

/// Shows the context menu of the engines: Copy, Copy Link over a link, and Select All. WebView2
/// copies and selects with its own commands, which keep the formatting of the copied HTML.
fn show_context_menu(host: HWND, menu: ContextMenu) {
	const COPY: usize = 1;
	const COPY_LINK: usize = 2;
	const SELECT_ALL: usize = 3;
	let mut point = POINT { x: 0, y: 0 };
	match menu {
		ContextMenu::RichEdit(position) if position != -1 => {
			point = POINT {
				x: (position & 0xffff) as i16 as i32,
				y: ((position >> 16) & 0xffff) as i16 as i32,
			};
		}
		// SAFETY: Valid output point.
		_ => _ = unsafe { GetCursorPos(&mut point) },
	}
	let Some((notepad, has_selection, link_location)) = with_viewer(|viewer| {
		let engine = &viewer.engine;
		let (has_selection, link_location) = match &menu {
			ContextMenu::RichEdit(position) => {
				let control = engine.rich_edit_control()?;
				let directory = viewer.document_directory();
				let link = match *position {
					-1 => None,
					_ => engine.rich_edit_link_location(point, directory.as_deref()),
				};
				(control.has_selection(), link)
			}
			ContextMenu::WebView2(request) => (
				request.selected_text.is_some(),
				request.link_uri.as_deref().and_then(|uri| engine.link_location(uri)),
			),
			ContextMenu::EdgeHtml(request) => {
				(!request.selected_text.is_empty(), engine.link_location(&request.link))
			}
		};
		Some((viewer.notepad, has_selection, link_location))
	})
	.flatten() else {
		return;
	};
	let mut items = vec![MenuItem::new(COPY, "Copy	Ctrl+C", has_selection)];
	if link_location.is_some() {
		items.push(MenuItem::new(COPY_LINK, "Copy Link", true));
	}
	items.push(MenuItem::new(SELECT_ALL, "Select All	Ctrl+A", true));
	let command = track_menu(host, point, &items);
	let set_text = |text: &str| set_clipboard(&notepad, &[ClipboardContent::UnicodeText(text)]);
	match (command, menu) {
		(COPY_LINK, _) => {
			if let Some(location) = &link_location {
				set_text(location);
			}
		}
		(COPY, ContextMenu::WebView2(request)) => {
			let text = request.selected_text.clone().unwrap_or_default();
			if !request.run("copy") {
				set_text(&text);
			}
		}
		(COPY, ContextMenu::EdgeHtml(request)) => {
			let html = clipboard_html(&request.selected_html);
			let contents = [
				ClipboardContent::Html(&html),
				ClipboardContent::UnicodeText(&request.selected_text),
			];
			set_clipboard(&notepad, &contents);
		}
		(SELECT_ALL, ContextMenu::WebView2(request)) => {
			if !request.run("selectAll") {
				with_viewer(|viewer| viewer.engine.select_all_in_browser());
			}
		}
		(SELECT_ALL, ContextMenu::EdgeHtml(_)) => {
			with_viewer(|viewer| viewer.engine.select_all_in_browser());
		}
		(COPY | SELECT_ALL, ContextMenu::RichEdit(_)) => {
			with_viewer(|viewer| {
				if let Some(control) = viewer.engine.rich_edit_control() {
					match command {
						COPY => control.copy(),
						_ => control.select_all(),
					}
				}
			});
		}
		_ => {}
	}
}

struct MenuItem {
	command: usize,
	text: &'static str,
	is_enabled: bool,
}

impl MenuItem {
	fn new(command: usize, text: &'static str, is_enabled: bool) -> Self {
		MenuItem { command, text, is_enabled }
	}
}

/// Shows a popup menu of `items` at the screen `point` and returns the chosen command, 0 if none.
/// The viewer must not be borrowed, the menu runs its own message loop.
fn track_menu(owner: HWND, point: POINT, items: &[MenuItem]) -> usize {
	// SAFETY: The menu is created, shown modally and destroyed here. The texts are NUL terminated
	// and outlive the calls.
	unsafe {
		let menu = CreatePopupMenu();
		for item in items {
			let text = to_wide_null_terminated(item.text);
			let flags = if item.is_enabled { MF_STRING } else { MF_STRING | MF_GRAYED };
			AppendMenuW(menu, flags, item.command, text.as_ptr());
		}
		let command = TrackPopupMenu(
			menu,
			TPM_RETURNCMD | TPM_RIGHTBUTTON,
			point.x,
			point.y,
			0,
			owner,
			core::ptr::null(),
		);
		DestroyMenu(menu);
		command as usize
	}
}

/// Windows draws the title bar of top-level windows itself, also in dark mode only on request.
fn set_dark_title_bar(window: HWND, is_dark: bool) {
	let value = i32::from(is_dark);
	// SAFETY: Valid window, the attribute takes a 4 byte BOOL.
	unsafe {
		DwmSetWindowAttribute(
			window,
			DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
			(&raw const value).cast(),
			size_of::<i32>() as u32,
		);
		SetWindowPos(
			window,
			core::ptr::null_mut(),
			0,
			0,
			0,
			0,
			SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
		);
	}
}
