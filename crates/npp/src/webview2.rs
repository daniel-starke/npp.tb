//! A locked down Microsoft Edge WebView2 control for showing generated HTML.
//!
//! Page scripts, dev tools, web messages, host objects and browser accelerator keys are disabled.
//! The host still runs its own scripts with [`WebView::execute_script`]. Navigations other than
//! [`WebView::navigate_to_string`] are cancelled and reported as activated links instead. Context
//! menus are left to the host as [`ContextMenuRequest`], the status bar text (the address of the
//! hovered link) is reported instead of shown.

mod interfaces;
mod loader;

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use std::path::Path;
use std::rc::Rc;

use windows_core::{Interface, implement};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree};

use crate::wide::to_wide_null_terminated;
use interfaces::*;

pub use interfaces::Color;
pub use loader::{LOADER_DLL_NAME, Runtime, find_runtime};
pub use windows_core::HRESULT;

pub struct WebViewOptions<'a> {
	pub runtime: &'a Runtime,
	/// Must be writable. WebView2 keeps its browser profile and caches there.
	pub user_data_folder: &'a Path,
	/// Shown before the first document is rendered.
	pub background: Color,
}

pub struct WebViewEvents {
	/// A link was clicked or opened in a new window. The navigation itself is cancelled.
	pub link_activated: Box<dyn Fn(&str)>,
	/// The document of [`WebView::navigate_to_string`] has been loaded.
	pub document_loaded: Box<dyn Fn()>,
}

/// State shared by the event handlers.
struct Shared {
	events: WebViewEvents,
	context_menu_requested: Box<dyn Fn(ContextMenuRequest)>,
	status_bar_text_changed: Box<dyn Fn(&str)>,
	/// The next navigation was started by [`WebView::navigate_to_string`].
	expects_string_navigation: Cell<bool>,
	/// The navigation of [`WebView::navigate_to_string`] is running. Cancelled link navigations
	/// complete as well and must not count as a loaded document.
	is_loading_string: Cell<bool>,
}

/// Called once with the created view, or with the error that prevented its creation.
pub type CreatedCallback = Box<dyn FnOnce(Result<WebView, HRESULT>)>;

struct Creation {
	parent: HWND,
	background: Color,
	shared: Rc<Shared>,
	created: RefCell<Option<CreatedCallback>>,
}

impl Creation {
	fn finish(&self, result: Result<WebView, HRESULT>) {
		if let Some(created) = self.created.borrow_mut().take() {
			created(result);
		}
	}
}

pub struct WebView {
	controller: ICoreWebView2Controller,
	webview: ICoreWebView2,
	shared: Rc<Shared>,
}

/// Starts creating a WebView as child of `parent`. `created` is called later on the UI thread,
/// unless creation fails right away with the returned error.
pub fn create_webview(
	parent: HWND,
	options: &WebViewOptions,
	events: WebViewEvents,
	context_menu_requested: Box<dyn Fn(ContextMenuRequest)>,
	status_bar_text_changed: Box<dyn Fn(&str)>,
	created: CreatedCallback,
) -> Result<(), HRESULT> {
	// WebView2 requires a single-threaded apartment. Notepad++ normally initialized it already,
	// so this only adds a reference that is kept for the lifetime of the process.
	// SAFETY: No preconditions. Failure (another apartment model) makes the creation fail below.
	unsafe { CoInitializeEx(core::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
	let creation = Rc::new(Creation {
		parent,
		background: options.background,
		shared: Rc::new(Shared {
			events,
			context_menu_requested,
			status_bar_text_changed,
			expects_string_navigation: Cell::new(false),
			is_loading_string: Cell::new(false),
		}),
		created: RefCell::new(Some(created)),
	});
	let handler: ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler =
		EnvironmentCompleted(creation).into();
	loader::create_environment(options.runtime, options.user_data_folder, handler.as_raw())
}

impl WebView {
	pub fn set_bounds(&self, bounds: RECT) {
		// SAFETY: Valid interface, plain value argument.
		let _ = unsafe { self.controller.put_Bounds(bounds) };
	}

	pub fn set_visible(&self, is_visible: bool) {
		// SAFETY: Valid interface, plain value argument.
		let _ = unsafe { self.controller.put_IsVisible(BOOL::from(is_visible)) };
	}

	/// Moves the view into another window, e.g. from a docked panel into a top-level window.
	#[allow(clippy::not_unsafe_ptr_arg_deref, reason = "window handles are not dereferenced")]
	pub fn set_parent(&self, parent: HWND) {
		// SAFETY: Valid interface. WebView2 accepts any window of this thread.
		let _ = unsafe { self.controller.put_ParentWindow(parent) };
	}

	/// Call when a top-level ancestor moved, so that popups and IME windows follow.
	pub fn notify_parent_moved(&self) {
		// SAFETY: Valid interface.
		let _ = unsafe { self.controller.NotifyParentWindowPositionChanged() };
	}

	pub fn focus(&self) {
		// SAFETY: Valid interface, plain value argument.
		let _ = unsafe { self.controller.MoveFocus(MOVE_FOCUS_REASON_PROGRAMMATIC) };
	}

	pub fn set_background(&self, color: Color) {
		if let Ok(controller) = self.controller.cast::<ICoreWebView2Controller2>() {
			// SAFETY: Valid interface, plain value argument.
			let _ = unsafe { controller.put_DefaultBackgroundColor(color) };
		}
	}

	/// Shows `html`. WebView2 limits it to 2 MB, so large content should be added with
	/// [`Self::execute_script`] once [`WebViewEvents::document_loaded`] was called.
	pub fn navigate_to_string(&self, html: &str) {
		self.shared.expects_string_navigation.set(true);
		let html = to_wide_null_terminated(html);
		// SAFETY: Valid interface, the string is NUL terminated and copied by WebView2.
		let _ = unsafe { self.webview.NavigateToString(html.as_ptr()) };
	}

	/// Runs `script` in the current document. Works although page scripts are disabled.
	pub fn execute_script(&self, script: &str) {
		let script = to_wide_null_terminated(script);
		// SAFETY: Valid interface, the string is NUL terminated and copied by WebView2. A null
		// handler is allowed when the result is not needed.
		let _ = unsafe { self.webview.ExecuteScript(script.as_ptr(), core::ptr::null_mut()) };
	}
}

impl Drop for WebView {
	fn drop(&mut self) {
		// SAFETY: Valid interface. Closing releases the browser process resources.
		let _ = unsafe { self.controller.Close() };
	}
}

#[implement(ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler)]
struct EnvironmentCompleted(Rc<Creation>);

impl ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler_Impl for EnvironmentCompleted_Impl {
	unsafe fn Invoke(&self, result: HRESULT, environment: *mut c_void) -> HRESULT {
		let creation = &self.0;
		// SAFETY: WebView2 passes a valid environment, borrowed for this call.
		let environment = unsafe { ICoreWebView2Environment::from_raw_borrowed(&environment) };
		let Some(environment) = environment.filter(|_| result.is_ok()) else {
			creation.finish(Err(if result.is_ok() { HRESULT(-1) } else { result }));
			return HRESULT(0);
		};
		let handler: ICoreWebView2CreateCoreWebView2ControllerCompletedHandler =
			ControllerCompleted(Rc::clone(creation)).into();
		// SAFETY: Valid interface, window and handler.
		let started =
			unsafe { environment.CreateCoreWebView2Controller(creation.parent, handler.as_raw()) };
		if started.is_err() {
			creation.finish(Err(started));
		}
		HRESULT(0)
	}
}

#[implement(ICoreWebView2CreateCoreWebView2ControllerCompletedHandler)]
struct ControllerCompleted(Rc<Creation>);

impl ICoreWebView2CreateCoreWebView2ControllerCompletedHandler_Impl for ControllerCompleted_Impl {
	unsafe fn Invoke(&self, result: HRESULT, controller: *mut c_void) -> HRESULT {
		let creation = &self.0;
		// SAFETY: WebView2 passes a valid controller, borrowed for this call.
		let controller = unsafe { ICoreWebView2Controller::from_raw_borrowed(&controller) };
		let Some(controller) = controller.filter(|_| result.is_ok()) else {
			creation.finish(Err(if result.is_ok() { HRESULT(-1) } else { result }));
			return HRESULT(0);
		};
		creation.finish(set_up(controller.clone(), creation));
		HRESULT(0)
	}
}

fn set_up(controller: ICoreWebView2Controller, creation: &Creation) -> Result<WebView, HRESULT> {
	let mut webview = None;
	// SAFETY: Valid interface and out pointer.
	unsafe { controller.get_CoreWebView2(&mut webview) }.ok().map_err(|error| error.code())?;
	let webview = webview.ok_or(HRESULT(-1))?;
	let mut settings = None;
	// SAFETY: Valid interface and out pointer.
	unsafe { webview.get_Settings(&mut settings) }.ok().map_err(|error| error.code())?;
	let settings = settings.ok_or(HRESULT(-1))?;
	// SAFETY: Valid interface, plain value arguments.
	unsafe {
		let _ = settings.put_IsScriptEnabled(0);
		let _ = settings.put_IsWebMessageEnabled(0);
		let _ = settings.put_AreDefaultScriptDialogsEnabled(0);
		let _ = settings.put_IsStatusBarEnabled(0);
		let _ = settings.put_AreDevToolsEnabled(0);
		let _ = settings.put_AreHostObjectsAllowed(0);
		let _ = settings.put_IsBuiltInErrorPageEnabled(0);
		if let Ok(settings) = settings.cast::<ICoreWebView2Settings3>() {
			let _ = settings.put_AreBrowserAcceleratorKeysEnabled(0);
		}
	}

	let shared = &creation.shared;
	let mut token = EventRegistrationToken::default();
	let navigation_starting: ICoreWebView2NavigationStartingEventHandler =
		NavigationStarting(Rc::clone(shared)).into();
	let navigation_completed: ICoreWebView2NavigationCompletedEventHandler =
		NavigationCompleted(Rc::clone(shared)).into();
	let new_window_requested: ICoreWebView2NewWindowRequestedEventHandler =
		NewWindowRequested(Rc::clone(shared)).into();
	// SAFETY: Valid interfaces, handlers and token. WebView2 keeps references to the handlers.
	unsafe {
		let _ = webview.add_NavigationStarting(navigation_starting.as_raw(), &mut token);
		let _ = webview.add_NavigationCompleted(navigation_completed.as_raw(), &mut token);
		let _ = webview.add_NewWindowRequested(new_window_requested.as_raw(), &mut token);
	}
	// The context menu event needs a newer runtime. Without it the default menu remains.
	if let Ok(webview) = webview.cast::<ICoreWebView2_11>() {
		let context_menu_requested: ICoreWebView2ContextMenuRequestedEventHandler =
			ContextMenuRequested(Rc::clone(shared)).into();
		// SAFETY: Valid interface, handler and token.
		let _ = unsafe {
			webview.add_ContextMenuRequested(context_menu_requested.as_raw(), &mut token)
		};
	}
	if let Ok(webview) = webview.cast::<ICoreWebView2_12>() {
		let status_bar_text_changed: ICoreWebView2StatusBarTextChangedEventHandler =
			StatusBarTextChanged(Rc::clone(shared)).into();
		// SAFETY: Valid interface, handler and token.
		let _ = unsafe {
			webview.add_StatusBarTextChanged(status_bar_text_changed.as_raw(), &mut token)
		};
	}

	let view = WebView { controller, webview, shared: Rc::clone(shared) };
	view.set_background(creation.background);
	Ok(view)
}

/// Takes a string WebView2 allocated with `CoTaskMemAlloc`.
fn take_co_task_string(text: *mut u16) -> String {
	if text.is_null() {
		return String::new();
	}
	let mut length = 0;
	// SAFETY: WebView2 strings are NUL terminated.
	while unsafe { *text.add(length) } != 0 {
		length += 1;
	}
	// SAFETY: `length` units were read above.
	let result = String::from_utf16_lossy(unsafe { core::slice::from_raw_parts(text, length) });
	// SAFETY: Allocated with `CoTaskMemAlloc` and not used afterwards.
	unsafe { CoTaskMemFree(text.cast()) };
	result
}

#[implement(ICoreWebView2NavigationStartingEventHandler)]
struct NavigationStarting(Rc<Shared>);

impl ICoreWebView2NavigationStartingEventHandler_Impl for NavigationStarting_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, arguments: *mut c_void) -> HRESULT {
		// SAFETY: WebView2 passes valid arguments, borrowed for this call.
		let arguments =
			unsafe { ICoreWebView2NavigationStartingEventArgs::from_raw_borrowed(&arguments) };
		let Some(arguments) = arguments else { return HRESULT(0) };
		if self.0.expects_string_navigation.replace(false) {
			self.0.is_loading_string.set(true);
			return HRESULT(0);
		}
		let mut uri = core::ptr::null_mut();
		// SAFETY: Valid interface and out pointer.
		let _ = unsafe { arguments.put_Cancel(1) };
		// SAFETY: Valid interface and out pointer.
		let _ = unsafe { arguments.get_Uri(&mut uri) };
		(self.0.events.link_activated)(&take_co_task_string(uri));
		HRESULT(0)
	}
}

#[implement(ICoreWebView2NavigationCompletedEventHandler)]
struct NavigationCompleted(Rc<Shared>);

impl ICoreWebView2NavigationCompletedEventHandler_Impl for NavigationCompleted_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, _arguments: *mut c_void) -> HRESULT {
		if self.0.is_loading_string.replace(false) {
			(self.0.events.document_loaded)();
		}
		HRESULT(0)
	}
}

#[implement(ICoreWebView2StatusBarTextChangedEventHandler)]
struct StatusBarTextChanged(Rc<Shared>);

impl ICoreWebView2StatusBarTextChangedEventHandler_Impl for StatusBarTextChanged_Impl {
	unsafe fn Invoke(&self, sender: *mut c_void, _arguments: *mut c_void) -> HRESULT {
		// SAFETY: WebView2 passes the view, borrowed for this call.
		let Some(webview) = (unsafe { ICoreWebView2::from_raw_borrowed(&sender) }) else {
			return HRESULT(0);
		};
		let Ok(webview) = webview.cast::<ICoreWebView2_12>() else { return HRESULT(0) };
		let mut text = core::ptr::null_mut();
		// SAFETY: Valid interface and out pointer.
		let _ = unsafe { webview.get_StatusBarText(&mut text) };
		(self.0.status_bar_text_changed)(&take_co_task_string(text));
		HRESULT(0)
	}
}

#[implement(ICoreWebView2NewWindowRequestedEventHandler)]
struct NewWindowRequested(Rc<Shared>);

impl ICoreWebView2NewWindowRequestedEventHandler_Impl for NewWindowRequested_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, arguments: *mut c_void) -> HRESULT {
		// SAFETY: WebView2 passes valid arguments, borrowed for this call.
		let arguments =
			unsafe { ICoreWebView2NewWindowRequestedEventArgs::from_raw_borrowed(&arguments) };
		let Some(arguments) = arguments else { return HRESULT(0) };
		let mut uri = core::ptr::null_mut();
		// SAFETY: Valid interface and out pointer. Handled without a new window opens nothing.
		unsafe {
			let _ = arguments.put_Handled(1);
			let _ = arguments.get_Uri(&mut uri);
		}
		(self.0.events.link_activated)(&take_co_task_string(uri));
		HRESULT(0)
	}
}

/// A context menu that WebView2 leaves to the host, which shows its own menu and can run one of
/// the default commands with [`Self::run`]. WebView2 waits until the request is dropped.
pub struct ContextMenuRequest {
	arguments: ICoreWebView2ContextMenuRequestedEventArgs,
	deferral: ICoreWebView2Deferral,
	/// Names and command ids of the enabled default items, e.g. `copy` or `selectAll`.
	commands: Vec<(String, i32)>,
	/// The absolute URI of the link under the mouse.
	pub link_uri: Option<String>,
	pub selected_text: Option<String>,
}

impl ContextMenuRequest {
	pub fn has_command(&self, name: &str) -> bool {
		self.commands.iter().any(|(command, _)| command == name)
	}

	/// Runs the enabled default command `name`. Returns whether there is one.
	pub fn run(self, name: &str) -> bool {
		let Some(&(_, command_id)) = self.commands.iter().find(|(command, _)| command == name)
		else {
			return false;
		};
		// SAFETY: Valid interface, plain value argument. The command runs when the deferral
		// completes in `drop`.
		let _ = unsafe { self.arguments.put_SelectedCommandId(command_id) };
		true
	}
}

impl Drop for ContextMenuRequest {
	fn drop(&mut self) {
		// SAFETY: Valid interface, completed exactly once.
		let _ = unsafe { self.deferral.Complete() };
	}
}

#[implement(ICoreWebView2ContextMenuRequestedEventHandler)]
struct ContextMenuRequested(Rc<Shared>);

impl ICoreWebView2ContextMenuRequestedEventHandler_Impl for ContextMenuRequested_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, arguments: *mut c_void) -> HRESULT {
		// SAFETY: WebView2 passes valid arguments, borrowed for this call.
		let arguments =
			unsafe { ICoreWebView2ContextMenuRequestedEventArgs::from_raw_borrowed(&arguments) };
		let Some(arguments) = arguments else { return HRESULT(0) };
		let (mut items, mut target, mut deferral) = (None, None, None);
		// SAFETY: Valid interface and out pointers.
		unsafe {
			let _ = arguments.get_MenuItems(&mut items);
			let _ = arguments.get_ContextMenuTarget(&mut target);
			let _ = arguments.GetDeferral(&mut deferral);
		}
		// Without them WebView2 shows its default menu.
		let (Some(items), Some(target), Some(deferral)) = (items, target, deferral) else {
			return HRESULT(0);
		};
		let mut commands = Vec::new();
		let mut count = 0;
		// SAFETY: Valid interface and out pointer.
		let _ = unsafe { items.get_Count(&mut count) };
		for index in 0..count {
			let mut item = None;
			// SAFETY: Valid interface, index in range and out pointer.
			let _ = unsafe { items.GetValueAtIndex(index, &mut item) };
			let Some(item) = item else { continue };
			let (mut name, mut command_id, mut is_enabled) = (core::ptr::null_mut(), 0, 0);
			// SAFETY: Valid interface and out pointers.
			unsafe {
				let _ = item.get_Name(&mut name);
				let _ = item.get_CommandId(&mut command_id);
				let _ = item.get_IsEnabled(&mut is_enabled);
			}
			let name = take_co_task_string(name);
			if is_enabled != 0 {
				commands.push((name, command_id));
			}
		}
		let (mut has_link_uri, mut has_selection) = (0, 0);
		let (mut link_uri, mut selected_text) = (None, None);
		// SAFETY: Valid interface and out pointers. The strings are only read when present.
		unsafe {
			let _ = target.get_HasLinkUri(&mut has_link_uri);
			let _ = target.get_HasSelection(&mut has_selection);
			if has_link_uri != 0 {
				let mut uri = core::ptr::null_mut();
				let _ = target.get_LinkUri(&mut uri);
				link_uri = Some(take_co_task_string(uri));
			}
			if has_selection != 0 {
				let mut text = core::ptr::null_mut();
				let _ = target.get_SelectionText(&mut text);
				selected_text = Some(take_co_task_string(text));
			}
			let _ = arguments.put_Handled(1);
		}
		(self.0.context_menu_requested)(ContextMenuRequest {
			arguments: arguments.clone(),
			deferral,
			commands,
			link_uri,
			selected_text,
		});
		HRESULT(0)
	}
}
