//! The host window of EdgeHTML in a window with "System (Enhanced)" DPI scaling.
//!
//! With GDI scaling (compatibility layer `GdiDpiScaling`), Windows composes the out-of-process
//! content of the control at its logical position and size inside the doubled surface of the GDI
//! scaling, so it is drawn at half its place on a 175 % screen, while its input window covers the
//! correct area. Every clip of the window tree applies to both, so no geometry of a child window
//! fixes it. A top-level window created with a per-monitor DPI awareness context escapes the GDI
//! scaling of its process; child windows cannot, they get the awareness of their parent.
//!
//! An [`Overlay`] is such a borderless top-level window owned by the top-level window of the
//! parent, kept over the bounds of the parent in physical pixels and clipped to the visible client
//! areas of the parent and its ancestors. A WinEvent hook follows moves, size changes, visibility
//! and reparenting of the parent and its ancestors.

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
	ClientToScreen, CreateRectRgn, EqualRect, IntersectRect, IsRectEmpty, SetWindowRgn,
};
use windows_sys::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::HiDpi::{
	AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT,
	DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED,
	GetWindowDpiAwarenessContext, LogicalToPhysicalPointForPerMonitorDPI,
	SetThreadDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
	CreateWindowExW, DefWindowProcW, DestroyWindow, EVENT_OBJECT_PARENTCHANGE, EVENT_OBJECT_SHOW,
	GA_ROOT, GW_OWNER, GWLP_HWNDPARENT, GetAncestor, GetClientRect, GetParent, GetWindow,
	GetWindowThreadProcessId, IsChild, IsIconic, IsWindowVisible, OBJID_WINDOW, RegisterClassExW,
	SW_HIDE, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos,
	ShowWindow, WINEVENT_OUTOFCONTEXT, WM_DPICHANGED, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
};

use super::interfaces::{BOOLEAN, IWebViewControlSite, Rect};
use crate::wide::to_wide_null_terminated;

const CLASS: &str = "NppEdgeHtmlOverlay";

thread_local! {
	// Clippy 1.99 reports the initializers although they are `const` already.
	#[allow(clippy::missing_const_for_thread_local)]
	static OVERLAYS: RefCell<Vec<Rc<State>>> = const { RefCell::new(Vec::new()) };
	#[allow(clippy::missing_const_for_thread_local)]
	static HOOK: Cell<HWINEVENTHOOK> = const { Cell::new(core::ptr::null_mut()) };
}

/// Whether Windows draws `window` with "System (Enhanced)" DPI scaling.
pub fn is_gdi_scaled(window: HWND) -> bool {
	// SAFETY: Returns a null context for invalid windows, which compares unequal.
	unsafe {
		AreDpiAwarenessContextsEqual(
			GetWindowDpiAwarenessContext(window),
			DPI_AWARENESS_CONTEXT_UNAWARE_GDISCALED,
		) != 0
	}
}

struct State {
	window: HWND,
	parent: HWND,
	/// In logical pixels of the parent.
	bounds: Cell<RECT>,
	is_visible: Cell<bool>,
	/// An update is running. Calls into the control pump messages, so updates can nest.
	is_updating: Cell<bool>,
	/// A nested update was requested and runs when the current one is done, so that the last
	/// geometry wins.
	needs_update: Cell<bool>,
	/// The control, once created.
	site: RefCell<Option<IWebViewControlSite>>,
}

pub struct Overlay(Rc<State>);

impl Overlay {
	/// Creates the hidden overlay of `parent`.
	pub fn create(parent: HWND) -> Option<Self> {
		register_class();
		let _context = ThreadDpiContext::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
		let class = to_wide_null_terminated(CLASS);
		// SAFETY: The class is registered. The owner is a valid top-level window.
		let window = unsafe {
			CreateWindowExW(
				WS_EX_TOOLWINDOW,
				class.as_ptr(),
				core::ptr::null(),
				WS_POPUP,
				0,
				0,
				0,
				0,
				GetAncestor(parent, GA_ROOT),
				core::ptr::null_mut(),
				crate::plugin::module_handle(),
				core::ptr::null(),
			)
		};
		if window.is_null() {
			return None;
		}
		let state = Rc::new(State {
			window,
			parent,
			bounds: Cell::new(RECT { left: 0, top: 0, right: 0, bottom: 0 }),
			is_visible: Cell::new(false),
			is_updating: Cell::new(false),
			needs_update: Cell::new(false),
			site: RefCell::new(None),
		});
		OVERLAYS.with(|overlays| overlays.borrow_mut().push(Rc::clone(&state)));
		install_hook(parent);
		Some(Self(state))
	}

	pub fn window(&self) -> HWND {
		self.0.window
	}

	/// Runs `call` in the per-monitor DPI context, which calls creating or moving windows of the
	/// control in the overlay need.
	pub fn in_dpi_context<T>(&self, call: impl FnOnce() -> T) -> T {
		let _context = ThreadDpiContext::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
		call()
	}

	/// The control is created and follows the overlay from now on. It scales its content with the
	/// DPI of the per-monitor aware overlay by itself.
	pub fn attach(&self, site: IWebViewControlSite) {
		*self.0.site.borrow_mut() = Some(site);
		self.0.update();
	}

	/// `bounds` are logical pixels of the parent.
	pub fn set_bounds(&self, bounds: RECT) {
		self.0.bounds.set(bounds);
		self.0.update();
	}

	pub fn set_visible(&self, is_visible: bool) {
		self.0.is_visible.set(is_visible);
		self.0.update();
	}
}

impl Drop for Overlay {
	fn drop(&mut self) {
		let is_last = OVERLAYS.with(|overlays| {
			let mut overlays = overlays.borrow_mut();
			overlays.retain(|state| !Rc::ptr_eq(state, &self.0));
			overlays.is_empty()
		});
		if is_last {
			remove_hook();
		}
		self.0.site.borrow_mut().take();
		// SAFETY: The window was created by `create` and is not used afterwards.
		unsafe { DestroyWindow(self.0.window) };
	}
}

impl State {
	fn update(&self) {
		if self.is_updating.replace(true) {
			self.needs_update.set(true);
			return;
		}
		loop {
			self.needs_update.set(false);
			self.place();
			if !self.needs_update.get() {
				break;
			}
		}
		self.is_updating.set(false);
	}

	/// Moves the overlay over the bounds of the parent, or hides it when nothing of them is
	/// visible.
	fn place(&self) {
		// SAFETY: Valid windows. The coordinates are computed in the DPI awareness of the parent
		// and converted to physical pixels of the screen. The awareness of the thread is set
		// explicitly, because it can be the per-monitor context below, e.g. in a window event
		// dispatched while the out-of-process control is called.
		let placement = unsafe {
			let _context = ThreadDpiContext::enter(GetWindowDpiAwarenessContext(self.parent));
			let root = GetAncestor(self.parent, GA_ROOT);
			if GetWindow(self.window, GW_OWNER) != root {
				// `SetWindowLongPtrW` takes a pointer-sized integer, `i32` on x86.
				SetWindowLongPtrW(self.window, GWLP_HWNDPARENT, root as _);
			}
			let is_shown =
				self.is_visible.get() && IsWindowVisible(self.parent) != 0 && IsIconic(root) == 0;
			is_shown.then(|| self.physical_placement(root)).flatten()
		};
		let Some((area, visible)) = placement else {
			// SAFETY: Valid window.
			unsafe { ShowWindow(self.window, SW_HIDE) };
			return;
		};
		let width = area.right - area.left;
		let height = area.bottom - area.top;
		let _context = ThreadDpiContext::enter(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
		// SAFETY: Valid window. The region belongs to the window afterwards.
		unsafe {
			let region = match EqualRect(&visible, &area) != 0 {
				true => core::ptr::null_mut(),
				false => CreateRectRgn(
					visible.left - area.left,
					visible.top - area.top,
					visible.right - area.left,
					visible.bottom - area.top,
				),
			};
			SetWindowRgn(self.window, region, 1);
			SetWindowPos(
				self.window,
				core::ptr::null_mut(),
				area.left,
				area.top,
				width,
				height,
				SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
			);
		}
		if let Some(site) = self.site.borrow().as_ref() {
			let bounds = Rect { x: 0.0, y: 0.0, width: width as f32, height: height as f32 };
			// SAFETY: Valid interface, plain value arguments. The control moves its window in this
			// process with the thread's DPI context, which must be the per-monitor one.
			unsafe {
				let _ = site.put_Bounds(bounds);
				let _ = site.put_IsVisible(BOOLEAN::from(true));
			}
		}
	}

	/// The bounds of the parent and their visible part in physical screen pixels, `None` when
	/// nothing is visible.
	///
	/// # Safety
	/// `root` must be the top-level ancestor of the parent.
	unsafe fn physical_placement(&self, root: HWND) -> Option<(RECT, RECT)> {
		let area = physical_screen_rect(self.parent, self.bounds.get());
		let mut visible = area;
		let mut ancestor = self.parent;
		while !ancestor.is_null() {
			let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
			// SAFETY: Valid window and output rectangles.
			unsafe {
				GetClientRect(ancestor, &mut client);
				let client = physical_screen_rect(ancestor, client);
				let clipped = visible;
				IntersectRect(&mut visible, &clipped, &client);
			}
			if ancestor == root {
				break;
			}
			// SAFETY: Valid window.
			ancestor = unsafe { GetParent(ancestor) };
		}
		// SAFETY: Valid rectangle.
		(unsafe { IsRectEmpty(&visible) } == 0).then_some((area, visible))
	}
}

/// Converts `rect` in logical client pixels of `window` to physical screen pixels.
fn physical_screen_rect(window: HWND, rect: RECT) -> RECT {
	let mut top_left = POINT { x: rect.left, y: rect.top };
	let mut bottom_right = POINT { x: rect.right, y: rect.bottom };
	// SAFETY: Valid window and points.
	unsafe {
		ClientToScreen(window, &mut top_left);
		ClientToScreen(window, &mut bottom_right);
		LogicalToPhysicalPointForPerMonitorDPI(window, &mut top_left);
		LogicalToPhysicalPointForPerMonitorDPI(window, &mut bottom_right);
	}
	RECT { left: top_left.x, top: top_left.y, right: bottom_right.x, bottom: bottom_right.y }
}

/// The DPI awareness of the thread while it is alive. In the per-monitor context, windows are
/// created and moved in physical pixels and escape GDI scaling.
struct ThreadDpiContext(DPI_AWARENESS_CONTEXT);

impl ThreadDpiContext {
	fn enter(context: DPI_AWARENESS_CONTEXT) -> Self {
		// SAFETY: No preconditions. Fails with a null result for an invalid context, which
		// leaves the context unchanged.
		Self(unsafe { SetThreadDpiAwarenessContext(context) })
	}
}

impl Drop for ThreadDpiContext {
	fn drop(&mut self) {
		if !self.0.is_null() {
			// SAFETY: Restores the context returned by `enter`.
			unsafe { SetThreadDpiAwarenessContext(self.0) };
		}
	}
}

fn register_class() {
	let name = to_wide_null_terminated(CLASS);
	let class = WNDCLASSEXW {
		cbSize: size_of::<WNDCLASSEXW>() as u32,
		lpfnWndProc: Some(overlay_procedure),
		hInstance: crate::plugin::module_handle(),
		lpszClassName: name.as_ptr(),
		..Default::default()
	};
	// SAFETY: The class description is complete. Windows copies the name. Registering an existing
	// class fails harmlessly.
	unsafe { RegisterClassExW(&class) };
}

unsafe extern "system" fn overlay_procedure(
	window: HWND,
	message: u32,
	wparam: WPARAM,
	lparam: LPARAM,
) -> LRESULT {
	if message == WM_DPICHANGED {
		update_overlays(|state| state.window == window);
		return 0;
	}
	// SAFETY: Forwards the arguments Windows passed.
	unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Updates the overlays that `is_affected`, without holding the borrow while Windows sends
/// messages.
fn update_overlays(is_affected: impl Fn(&State) -> bool) {
	let affected: Vec<Rc<State>> = OVERLAYS.with(|overlays| match overlays.try_borrow() {
		Ok(overlays) => overlays.iter().filter(|state| is_affected(state)).cloned().collect(),
		Err(_) => Vec::new(),
	});
	for state in affected {
		state.update();
	}
}

/// Hooks the window events of the thread of `parent`, which is this thread.
fn install_hook(parent: HWND) {
	if !HOOK.get().is_null() {
		return;
	}
	let mut process = 0;
	// SAFETY: Valid window and output pointer. Out-of-context events are delivered by the message
	// loop of this thread, so the callback runs on this thread.
	let hook = unsafe {
		let thread = GetWindowThreadProcessId(parent, &mut process);
		SetWinEventHook(
			EVENT_OBJECT_SHOW,
			EVENT_OBJECT_PARENTCHANGE,
			core::ptr::null_mut(),
			Some(window_event),
			process,
			thread,
			WINEVENT_OUTOFCONTEXT,
		)
	};
	HOOK.set(hook);
}

fn remove_hook() {
	let hook = HOOK.replace(core::ptr::null_mut());
	if !hook.is_null() {
		// SAFETY: The hook was installed by `install_hook`.
		unsafe { UnhookWinEvent(hook) };
	}
}

/// Showing, hiding, moving, resizing or reparenting the parent of an overlay or one of its
/// ancestors.
unsafe extern "system" fn window_event(
	_hook: HWINEVENTHOOK,
	_event: u32,
	window: HWND,
	object: i32,
	_child: i32,
	_thread: u32,
	_time: u32,
) {
	if object != OBJID_WINDOW || window.is_null() {
		return;
	}
	update_overlays(|state| {
		// SAFETY: `IsChild` accepts any window handles.
		window == state.parent || unsafe { IsChild(window, state.parent) } != 0
	});
}
