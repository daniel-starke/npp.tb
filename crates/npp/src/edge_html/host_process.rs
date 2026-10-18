//! The `Win32WebViewHost` process that shows the control in a host window.
//!
//! Windows starts one such packaged DCOM server per client process. Its app lifecycle manager
//! suspends it once it shows no control anymore, so it neither exits after closing the control
//! (`WebViewControlProcess.Terminate` only ends the content process `WWAHost`) nor when the client
//! exits; every client would leave an idle process behind. The views of a client share the host,
//! which is therefore terminated with the last of them.

use core::cell::RefCell;
use std::rc::{Rc, Weak};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, MAX_PATH};
use windows_sys::Win32::System::Threading::{
	GetCurrentProcessId, GetProcessId, OpenProcess, PROCESS_NAME_WIN32,
	PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
	TerminateProcess,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumChildWindows, GetWindowThreadProcessId};

const EXECUTABLE: &str = "Win32WebViewHost.exe";

thread_local! {
	// Clippy 1.99 reports the initializer although it is `const` already.
	#[allow(clippy::missing_const_for_thread_local)]
	static HOSTS: RefCell<Vec<Weak<HostProcess>>> = const { RefCell::new(Vec::new()) };
}

pub struct HostProcess(HANDLE);

impl HostProcess {
	/// Finds the host process of the control in `window`: the owner of the first descendant
	/// window of another process, the host's `CoreWindow` below the `CoreApplicationBridgeWindow`
	/// of this process. The content process `WWAHost` owns windows further down.
	pub fn of_control_in(window: HWND) -> Option<Rc<Self>> {
		let mut process_id = 0_u32;
		// SAFETY: The callback gets a pointer to `process_id`, which outlives the enumeration.
		unsafe {
			EnumChildWindows(window, Some(find_foreign_window), (&raw mut process_id) as LPARAM)
		};
		if process_id == 0 {
			return None;
		}
		let shared = HOSTS.with_borrow_mut(|hosts| {
			hosts.retain(|host| host.strong_count() > 0);
			hosts.iter().filter_map(Weak::upgrade).find(|host| host.id() == process_id)
		});
		if shared.is_some() {
			return shared;
		}
		// SAFETY: No preconditions. The handle keeps the process ID from being reused.
		let process = unsafe {
			OpenProcess(PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id)
		};
		if process.is_null() {
			return None;
		}
		let process = Self(process);
		if !process.is_host() {
			return None;
		}
		let process = Rc::new(process);
		HOSTS.with_borrow_mut(|hosts| hosts.push(Rc::downgrade(&process)));
		Some(process)
	}

	fn id(&self) -> u32 {
		// SAFETY: Valid process handle.
		unsafe { GetProcessId(self.0) }
	}

	fn is_host(&self) -> bool {
		let mut path = [0_u16; MAX_PATH as usize];
		let mut length = path.len() as u32;
		// SAFETY: Valid process handle, buffer and length in characters.
		let has_path = unsafe {
			QueryFullProcessImageNameW(self.0, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut length)
		} != 0;
		let path = String::from_utf16_lossy(&path[..length as usize]);
		let name = path.rsplit('\\').next().unwrap_or_default();
		has_path && name.eq_ignore_ascii_case(EXECUTABLE)
	}
}

impl Drop for HostProcess {
	fn drop(&mut self) {
		// SAFETY: The handle is owned, has `PROCESS_TERMINATE` access and is closed once.
		unsafe {
			TerminateProcess(self.0, 0);
			CloseHandle(self.0);
		}
	}
}

unsafe extern "system" fn find_foreign_window(window: HWND, process_id: LPARAM) -> i32 {
	let mut owner = 0;
	// SAFETY: Valid out pointer. Invalid windows report process 0.
	unsafe { GetWindowThreadProcessId(window, &mut owner) };
	// SAFETY: No preconditions.
	if owner == 0 || owner == unsafe { GetCurrentProcessId() } {
		return 1;
	}
	// SAFETY: `HostProcess::of_control_in` passes a pointer to its `u32`.
	unsafe { (process_id as *mut u32).write(owner) };
	0
}
