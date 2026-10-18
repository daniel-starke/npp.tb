//! The EdgeHTML engine of the legacy Microsoft Edge as a Win32 control
//! (`Windows.Web.UI.Interop.WebViewControl`, Windows 10 1803 and later) for showing generated
//! HTML.
//!
//! The control runs out of process in `Win32WebViewHost` and needs neither XAML Islands nor an
//! application manifest of the host, unlike `Windows.UI.Xaml.Controls.WebView`. It cannot run host
//! scripts without page scripts, so JavaScript stays enabled and the page has to keep its own
//! scripts out with a content security policy. Indexed DB is disabled. The control shows no
//! context menu, so host scripts report what a menu needs with `window.external.notify`.
//! Navigations other than [`EdgeView::navigate_to_string`] are cancelled and reported as activated
//! links, like [`crate::webview2`] does.
//!
//! Windows' "System (Enhanced)" DPI scaling of a program (compatibility layer `GdiDpiScaling`)
//! draws the out-of-process content of a child window at half its position and size, so the
//! control is shown in an [`overlay::Overlay`] there.

mod host_process;
mod interfaces;
mod overlay;

use core::cell::{Cell, RefCell};
use core::ffi::c_void;
use std::rc::Rc;

use windows_core::{HRESULT, HSTRING, IUnknown, Interface, implement};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows_sys::Win32::System::LibraryLoader::{
	GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

use crate::webview2::{Color, WebViewEvents};
use crate::wide::to_wide_null_terminated;
use host_process::HostProcess;
use interfaces::*;
use overlay::Overlay;

const PROCESS_CLASS: &str = "Windows.Web.UI.Interop.WebViewControlProcess";

/// `E_FAIL`
const E_FAIL: HRESULT = HRESULT(0x8000_4005_u32 as i32);
/// `E_BOUNDS`
const E_BOUNDS: HRESULT = HRESULT(0x8000_000b_u32 as i32);
/// `REGDB_E_CLASSNOTREG`
const CLASS_NOT_REGISTERED: HRESULT = HRESULT(0x8004_0154_u32 as i32);
type RoActivateInstance =
	unsafe extern "system" fn(class_id: *mut c_void, instance: *mut *mut c_void) -> HRESULT;

/// State shared by the event handlers.
struct Shared {
	events: WebViewEvents,
	script_notified: Box<dyn Fn(&str)>,
	/// The next navigation was started by [`EdgeView::navigate_to_string`].
	expects_string_navigation: Cell<bool>,
	/// The navigation of [`EdgeView::navigate_to_string`] is running. Cancelled link navigations
	/// complete as well and must not count as a loaded document.
	is_loading_string: Cell<bool>,
}

/// Called once with the created view, or with the error that prevented its creation.
pub type CreatedCallback = Box<dyn FnOnce(Result<EdgeView, HRESULT>)>;

struct Creation {
	process: IWebViewControlProcess,
	/// The window the control is created in.
	host: HWND,
	background: Color,
	shared: Rc<Shared>,
	created: RefCell<Option<CreatedCallback>>,
	overlay: RefCell<Option<Overlay>>,
}

impl Creation {
	fn finish(&self, result: Result<EdgeView, HRESULT>) {
		if let Some(created) = self.created.borrow_mut().take() {
			created(result);
		}
	}
}

pub struct EdgeView {
	process: IWebViewControlProcess,
	control: IWebViewControl,
	site: IWebViewControlSite,
	shared: Rc<Shared>,
	/// Hosts the control in a window with "System (Enhanced)" DPI scaling.
	overlay: Option<Overlay>,
	host_process: Option<Rc<HostProcess>>,
}

/// Starts creating an EdgeHTML view as child of `parent` with `bounds` in pixels of `parent`.
/// The content follows the DPI of `parent` by itself. `script_notified` receives the strings of
/// `window.external.notify`. `created` is called later on the UI thread, unless creation fails
/// right away with the returned error, e.g. before Windows 10 1803.
pub fn create_edge_view(
	parent: HWND,
	bounds: RECT,
	background: Color,
	events: WebViewEvents,
	script_notified: Box<dyn Fn(&str)>,
	created: CreatedCallback,
) -> Result<(), HRESULT> {
	// WinRT objects with a user interface require a single-threaded apartment. Notepad++ normally
	// initialized it already, so this only adds a reference that is kept for the process lifetime.
	// SAFETY: No preconditions. Failure (another apartment model) makes the creation fail below.
	unsafe { CoInitializeEx(core::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
	let process = activate_process()?;
	let overlay = match overlay::is_gdi_scaled(parent) {
		true => Some(Overlay::create(parent).ok_or(E_FAIL)?),
		false => None,
	};
	// The overlay is placed with the first `EdgeView::set_bounds`.
	let (host, bounds) = match &overlay {
		Some(overlay) => (overlay.window(), RECT { left: 0, top: 0, right: 0, bottom: 0 }),
		None => (parent, bounds),
	};
	let mut operation = None;
	// SAFETY: Valid interface and out pointer. A window handle is passed as a 64-bit integer.
	let mut create = || unsafe {
		process.CreateWebViewControlAsync(host as i64, to_rect(bounds), &mut operation)
	};
	match &overlay {
		Some(overlay) => overlay.in_dpi_context(create),
		None => create(),
	}
	.ok()
	.map_err(|error| error.code())?;
	let operation = operation.ok_or(E_FAIL)?;
	let creation = Rc::new(Creation {
		process,
		host,
		background,
		shared: Rc::new(Shared {
			events,
			script_notified,
			expects_string_navigation: Cell::new(false),
			is_loading_string: Cell::new(false),
		}),
		created: RefCell::new(Some(created)),
		overlay: RefCell::new(overlay),
	});
	let handler: IAsyncOperationCompletedHandler_WebViewControl = ControlCreated(creation).into();
	// SAFETY: Valid interfaces. The operation keeps a reference to the handler.
	unsafe { operation.put_Completed(handler.as_raw()) }.ok().map_err(|error| error.code())
}

/// `RoActivateInstance` is loaded at runtime, so that the plugin still loads where the WinRT API
/// set is missing.
fn activate_process() -> Result<IWebViewControlProcess, HRESULT> {
	let library = to_wide_null_terminated("combase.dll");
	// SAFETY: The name is NUL terminated. The DLL stays loaded for the process lifetime.
	let module = unsafe {
		LoadLibraryExW(library.as_ptr(), core::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
	};
	if module.is_null() {
		return Err(CLASS_NOT_REGISTERED);
	}
	// SAFETY: `module` is loaded, the name is NUL terminated.
	let function = unsafe { GetProcAddress(module, c"RoActivateInstance".as_ptr().cast()) }
		.ok_or(CLASS_NOT_REGISTERED)?;
	// SAFETY: The export has this signature (`roapi.h`).
	let activate: RoActivateInstance = unsafe { core::mem::transmute(function) };
	let class = HSTRING::from(PROCESS_CLASS);
	let mut instance = core::ptr::null_mut();
	// SAFETY: Valid class name and out pointer.
	unsafe { activate(hstring_abi(&class), &mut instance) }.ok().map_err(|error| error.code())?;
	// SAFETY: `RoActivateInstance` returned an owned `IInspectable`.
	let instance = unsafe { IUnknown::from_raw(instance) };
	instance.cast().map_err(|error| error.code())
}

/// The ABI value of a borrowed `HSTRING`.
fn hstring_abi(text: &HSTRING) -> *mut c_void {
	// SAFETY: `HSTRING` is a transparent wrapper of the string handle.
	unsafe { core::mem::transmute_copy(text) }
}

fn to_rect(bounds: RECT) -> Rect {
	Rect {
		x: bounds.left as f32,
		y: bounds.top as f32,
		width: (bounds.right - bounds.left).max(0) as f32,
		height: (bounds.bottom - bounds.top).max(0) as f32,
	}
}

impl EdgeView {
	pub fn set_bounds(&self, bounds: RECT) {
		match &self.overlay {
			Some(overlay) => overlay.set_bounds(bounds),
			// SAFETY: Valid interface, plain value argument.
			None => _ = unsafe { self.site.put_Bounds(to_rect(bounds)) },
		}
	}

	pub fn set_visible(&self, is_visible: bool) {
		// SAFETY: Valid interface, plain value argument.
		let _ = unsafe { self.site.put_IsVisible(BOOLEAN::from(is_visible)) };
		if let Some(overlay) = &self.overlay {
			overlay.set_visible(is_visible);
		}
	}

	pub fn focus(&self) {
		// SAFETY: Valid interface, plain value argument.
		let _ = unsafe { self.site.MoveFocus(MOVE_FOCUS_REASON_PROGRAMMATIC) };
	}

	pub fn set_background(&self, color: Color) {
		// SAFETY: Valid interface, plain value argument.
		let _ = unsafe { self.control.put_DefaultBackgroundColor(color) };
	}

	/// Shows `html`, a page that must keep its own scripts out with a content security policy.
	/// Content should be added with [`Self::execute_script`] once
	/// [`WebViewEvents::document_loaded`] was called.
	pub fn navigate_to_string(&self, html: &str) {
		self.shared.expects_string_navigation.set(true);
		let html = HSTRING::from(html);
		// SAFETY: Valid interface and string, which the control copies.
		let _ = unsafe { self.control.NavigateToString(hstring_abi(&html)) };
	}

	/// Runs `script` in the current document with `eval`, which the content security policy of
	/// the page has to allow with `'unsafe-eval'`.
	pub fn execute_script(&self, script: &str) {
		let function = HSTRING::from("eval");
		let arguments: IIterable_HSTRING = ScriptArguments(HSTRING::from(script)).into();
		let mut operation = None;
		// SAFETY: Valid interfaces and strings. The result is not needed, so the operation is
		// released right away; the script still runs.
		let _ = unsafe {
			self.control.InvokeScriptAsync(
				hstring_abi(&function),
				arguments.as_raw(),
				&mut operation,
			)
		};
	}
}

impl Drop for EdgeView {
	fn drop(&mut self) {
		// SAFETY: Valid interfaces. Closing the control and ending its content process releases
		// the resources of `Win32WebViewHost`. The overlay is destroyed afterwards.
		unsafe {
			let _ = self.site.Close();
			let _ = self.process.Terminate();
		}
		// Terminates `Win32WebViewHost` with the last view of this process.
		self.host_process = None;
	}
}

#[implement(IAsyncOperationCompletedHandler_WebViewControl)]
struct ControlCreated(Rc<Creation>);

impl IAsyncOperationCompletedHandler_WebViewControl_Impl for ControlCreated_Impl {
	unsafe fn Invoke(&self, operation: *mut c_void, status: i32) -> HRESULT {
		let creation = &self.0;
		// SAFETY: The operation passes itself, borrowed for this call.
		let operation = unsafe { IAsyncOperation_WebViewControl::from_raw_borrowed(&operation) };
		let mut control = None;
		if let Some(operation) = operation.filter(|_| status == ASYNC_STATUS_COMPLETED) {
			// SAFETY: Valid interface and out pointer.
			let _ = unsafe { operation.GetResults(&mut control) };
		}
		let result = match control {
			Some(control) => set_up(control, creation),
			None => Err(E_FAIL),
		};
		creation.finish(result);
		HRESULT(0)
	}
}

fn set_up(control: IWebViewControl, creation: &Creation) -> Result<EdgeView, HRESULT> {
	let site: IWebViewControlSite = control.cast().map_err(|error| error.code())?;
	let mut settings = None;
	// SAFETY: Valid interface and out pointer.
	unsafe { control.get_Settings(&mut settings) }.ok().map_err(|error| error.code())?;
	if let Some(settings) = settings {
		// SAFETY: Valid interface, plain value arguments.
		unsafe {
			let _ = settings.put_IsIndexedDBEnabled(0);
			let _ = settings.put_IsScriptNotifyAllowed(1);
		}
	}

	let shared = &creation.shared;
	let mut token = EventRegistrationToken::default();
	let navigation_starting: ITypedEventHandler_NavigationStarting =
		NavigationStarting(Rc::clone(shared)).into();
	let navigation_completed: ITypedEventHandler_NavigationCompleted =
		NavigationCompleted(Rc::clone(shared)).into();
	let new_window_requested: ITypedEventHandler_NewWindowRequested =
		NewWindowRequested(Rc::clone(shared)).into();
	let script_notify: ITypedEventHandler_ScriptNotify = ScriptNotify(Rc::clone(shared)).into();
	// SAFETY: Valid interfaces, handlers and token. The control keeps references to the handlers.
	unsafe {
		let _ = control.add_NavigationStarting(navigation_starting.as_raw(), &mut token);
		let _ = control.add_NavigationCompleted(navigation_completed.as_raw(), &mut token);
		let _ = control.add_NewWindowRequested(new_window_requested.as_raw(), &mut token);
		let _ = control.add_ScriptNotify(script_notify.as_raw(), &mut token);
	}

	let overlay = creation.overlay.borrow_mut().take();
	if let Some(overlay) = &overlay {
		overlay.attach(site.clone());
	}
	let view = EdgeView {
		process: creation.process.clone(),
		control,
		site,
		shared: Rc::clone(shared),
		overlay,
		host_process: HostProcess::of_control_in(creation.host),
	};
	view.set_background(creation.background);
	Ok(view)
}

/// The absolute form of a `Windows.Foundation.Uri`, empty without one.
fn absolute_uri(uri: Option<IUriRuntimeClass>) -> String {
	let mut text = HSTRING::new();
	if let Some(uri) = uri {
		// SAFETY: Valid interface and out pointer, which receives an owned string.
		let _ = unsafe { uri.get_AbsoluteUri(&mut text) };
	}
	text.to_string_lossy()
}

#[implement(ITypedEventHandler_NavigationStarting)]
struct NavigationStarting(Rc<Shared>);

impl ITypedEventHandler_NavigationStarting_Impl for NavigationStarting_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, arguments: *mut c_void) -> HRESULT {
		// SAFETY: The control passes valid arguments, borrowed for this call.
		let arguments =
			unsafe { IWebViewControlNavigationStartingEventArgs::from_raw_borrowed(&arguments) };
		let Some(arguments) = arguments else { return HRESULT(0) };
		if self.0.expects_string_navigation.replace(false) {
			self.0.is_loading_string.set(true);
			return HRESULT(0);
		}
		let mut uri = None;
		// SAFETY: Valid interface, plain value argument and out pointer.
		unsafe {
			let _ = arguments.put_Cancel(1);
			let _ = arguments.get_Uri(&mut uri);
		}
		(self.0.events.link_activated)(&absolute_uri(uri));
		HRESULT(0)
	}
}

#[implement(ITypedEventHandler_NavigationCompleted)]
struct NavigationCompleted(Rc<Shared>);

impl ITypedEventHandler_NavigationCompleted_Impl for NavigationCompleted_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, _arguments: *mut c_void) -> HRESULT {
		if self.0.is_loading_string.replace(false) {
			(self.0.events.document_loaded)();
		}
		HRESULT(0)
	}
}

#[implement(ITypedEventHandler_NewWindowRequested)]
struct NewWindowRequested(Rc<Shared>);

impl ITypedEventHandler_NewWindowRequested_Impl for NewWindowRequested_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, arguments: *mut c_void) -> HRESULT {
		// SAFETY: The control passes valid arguments, borrowed for this call.
		let arguments =
			unsafe { IWebViewControlNewWindowRequestedEventArgs::from_raw_borrowed(&arguments) };
		let Some(arguments) = arguments else { return HRESULT(0) };
		let mut uri = None;
		// SAFETY: Valid interface, plain value argument and out pointer. Handled without a new
		// window opens nothing.
		unsafe {
			let _ = arguments.put_Handled(1);
			let _ = arguments.get_Uri(&mut uri);
		}
		(self.0.events.link_activated)(&absolute_uri(uri));
		HRESULT(0)
	}
}

#[implement(ITypedEventHandler_ScriptNotify)]
struct ScriptNotify(Rc<Shared>);

impl ITypedEventHandler_ScriptNotify_Impl for ScriptNotify_Impl {
	unsafe fn Invoke(&self, _sender: *mut c_void, arguments: *mut c_void) -> HRESULT {
		// SAFETY: The control passes valid arguments, borrowed for this call.
		let arguments =
			unsafe { IWebViewControlScriptNotifyEventArgs::from_raw_borrowed(&arguments) };
		let Some(arguments) = arguments else { return HRESULT(0) };
		let mut value = HSTRING::new();
		// SAFETY: Valid interface and out pointer, which receives an owned string.
		let _ = unsafe { arguments.get_Value(&mut value) };
		(self.0.script_notified)(&value.to_string_lossy());
		HRESULT(0)
	}
}

/// The single argument of `eval` for `InvokeScriptAsync`.
#[implement(IIterable_HSTRING)]
struct ScriptArguments(HSTRING);

impl IIterable_HSTRING_Impl for ScriptArguments_Impl {
	unsafe fn GetIids(&self, count: *mut u32, iids: *mut *mut windows_core::GUID) -> HRESULT {
		// SAFETY: The caller passes valid out pointers.
		unsafe { no_iids(count, iids) }
	}

	unsafe fn GetRuntimeClassName(&self, name: *mut HSTRING) -> HRESULT {
		// SAFETY: The caller passes a valid out pointer.
		unsafe { name.write(HSTRING::new()) };
		HRESULT(0)
	}

	unsafe fn GetTrustLevel(&self, level: *mut i32) -> HRESULT {
		// SAFETY: The caller passes a valid out pointer. 0 is `BaseTrust`.
		unsafe { level.write(0) };
		HRESULT(0)
	}

	unsafe fn First(&self, iterator: *mut *mut c_void) -> HRESULT {
		let first: IIterator_HSTRING =
			ScriptArgumentIterator { value: self.0.clone(), has_current: Cell::new(true) }.into();
		// SAFETY: The caller passes a valid out pointer and takes the reference.
		unsafe { iterator.write(first.into_raw()) };
		HRESULT(0)
	}
}

#[implement(IIterator_HSTRING)]
struct ScriptArgumentIterator {
	value: HSTRING,
	has_current: Cell<bool>,
}

impl IIterator_HSTRING_Impl for ScriptArgumentIterator_Impl {
	unsafe fn GetIids(&self, count: *mut u32, iids: *mut *mut windows_core::GUID) -> HRESULT {
		// SAFETY: The caller passes valid out pointers.
		unsafe { no_iids(count, iids) }
	}

	unsafe fn GetRuntimeClassName(&self, name: *mut HSTRING) -> HRESULT {
		// SAFETY: The caller passes a valid out pointer.
		unsafe { name.write(HSTRING::new()) };
		HRESULT(0)
	}

	unsafe fn GetTrustLevel(&self, level: *mut i32) -> HRESULT {
		// SAFETY: The caller passes a valid out pointer. 0 is `BaseTrust`.
		unsafe { level.write(0) };
		HRESULT(0)
	}

	unsafe fn get_Current(&self, current: *mut HSTRING) -> HRESULT {
		if !self.has_current.get() {
			return E_BOUNDS;
		}
		// SAFETY: The caller passes a valid out pointer and takes the string.
		unsafe { current.write(self.value.clone()) };
		HRESULT(0)
	}

	unsafe fn get_HasCurrent(&self, has_current: *mut BOOLEAN) -> HRESULT {
		// SAFETY: The caller passes a valid out pointer.
		unsafe { has_current.write(BOOLEAN::from(self.has_current.get())) };
		HRESULT(0)
	}

	unsafe fn MoveNext(&self, has_current: *mut BOOLEAN) -> HRESULT {
		self.has_current.set(false);
		// SAFETY: The caller passes a valid out pointer.
		unsafe { has_current.write(0) };
		HRESULT(0)
	}

	unsafe fn GetMany(&self, capacity: u32, items: *mut HSTRING, count: *mut u32) -> HRESULT {
		let is_taken = capacity > 0 && self.has_current.replace(false);
		// SAFETY: The caller passes `capacity` writable items and a valid count.
		unsafe {
			if is_taken {
				items.write(self.value.clone());
			}
			count.write(u32::from(is_taken));
		}
		HRESULT(0)
	}
}

/// # Safety
/// Both pointers must be valid for writes.
unsafe fn no_iids(count: *mut u32, iids: *mut *mut windows_core::GUID) -> HRESULT {
	// SAFETY: Guaranteed by the caller. No IIDs need no `CoTaskMemAlloc`ed array.
	unsafe {
		count.write(0);
		iids.write(core::ptr::null_mut());
	}
	HRESULT(0)
}
