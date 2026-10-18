//! Creating the WebView2 environment, either with Microsoft's `WebView2Loader.dll` or directly
//! with the installed WebView2 runtime.
//!
//! `WebView2Loader.dll` is not part of Windows, and its static variant only links with MSVC. The
//! installed (Evergreen) runtime is found like the loader does it: the EdgeUpdate registry key
//! names its directory, whose `EBWebView\<arch>\EmbeddedBrowserWebView.dll` exports the
//! undocumented `CreateWebViewEnvironmentWithOptionsInternal` that the loader calls.

use core::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_core::HRESULT;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Registry::{
	HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY, RegGetValueW,
};

use crate::wide::{from_wide_until_null, to_wide_null_terminated};

/// The redistributable loader, looked for next to the plugin DLL.
pub const LOADER_DLL_NAME: &str = "WebView2Loader.dll";

/// The EdgeUpdate client ID of the stable WebView2 runtime channel.
const STABLE_RUNTIME_CLIENT_STATE_KEY: &str =
	r"Software\Microsoft\EdgeUpdate\ClientState\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

#[cfg(target_arch = "x86_64")]
const CLIENT_DLL: &str = r"EBWebView\x64\EmbeddedBrowserWebView.dll";
#[cfg(target_arch = "x86")]
const CLIENT_DLL: &str = r"EBWebView\x86\EmbeddedBrowserWebView.dll";
#[cfg(target_arch = "aarch64")]
const CLIENT_DLL: &str = r"EBWebView\arm64\EmbeddedBrowserWebView.dll";

/// `WebView2RunTimeType::kInstalled` of the loader.
const RUNTIME_TYPE_INSTALLED: i32 = 0;

type CreateEnvironmentWithOptions = unsafe extern "system" fn(
	browser_executable_folder: *const u16,
	user_data_folder: *const u16,
	environment_options: *mut c_void,
	handler: *mut c_void,
) -> HRESULT;

type CreateEnvironmentWithOptionsInternal = unsafe extern "system" fn(
	check_running_instance: bool,
	runtime_type: i32,
	user_data_folder: *const u16,
	environment_options: *mut c_void,
	handler: *mut c_void,
) -> HRESULT;

/// Where the WebView2 environment comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Runtime {
	/// `WebView2Loader.dll` at this path.
	Loader(PathBuf),
	/// `EmbeddedBrowserWebView.dll` of the installed runtime at this path.
	Installed(PathBuf),
}

/// The loader DLL in `plugin_directory` if present, otherwise the installed runtime.
pub fn find_runtime(plugin_directory: Option<&Path>) -> Option<Runtime> {
	let loader = plugin_directory.map(|directory| directory.join(LOADER_DLL_NAME));
	if let Some(loader) = loader.filter(|loader| loader.is_file()) {
		return Some(Runtime::Loader(loader));
	}
	installed_client_dll().map(Runtime::Installed)
}

fn installed_client_dll() -> Option<PathBuf> {
	[HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER].into_iter().find_map(|root| {
		let directory = registry_string(root, STABLE_RUNTIME_CLIENT_STATE_KEY, "EBWebView")?;
		Some(PathBuf::from(directory).join(CLIENT_DLL)).filter(|dll| dll.is_file())
	})
}

/// A `REG_SZ` value from the 32-bit registry view, where EdgeUpdate keeps its keys.
fn registry_string(root: HKEY, key: &str, value: &str) -> Option<String> {
	let key = to_wide_null_terminated(key);
	let value = to_wide_null_terminated(value);
	let mut buffer = vec![0u16; 1024];
	let mut size = (buffer.len() * 2) as u32;
	// SAFETY: All strings are NUL terminated, `size` is the byte size of `buffer`.
	let status = unsafe {
		RegGetValueW(
			root,
			key.as_ptr(),
			value.as_ptr(),
			RRF_RT_REG_SZ | RRF_SUBKEY_WOW6432KEY,
			core::ptr::null_mut(),
			buffer.as_mut_ptr().cast(),
			&mut size,
		)
	};
	(status == ERROR_SUCCESS).then(|| from_wide_until_null(&buffer))
}

/// Starts creating the environment. `handler` (an
/// `ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler`) is invoked later on the UI
/// thread, unless an error is returned.
pub fn create_environment(
	runtime: &Runtime,
	user_data_folder: &Path,
	handler: *mut c_void,
) -> Result<(), HRESULT> {
	let user_data_folder: Vec<u16> =
		user_data_folder.as_os_str().encode_wide().chain(Some(0)).collect();
	let result = match runtime {
		Runtime::Loader(path) => {
			let function = load_function(path, c"CreateCoreWebView2EnvironmentWithOptions")?;
			// SAFETY: The export has this signature (`WebView2.h`).
			let create: CreateEnvironmentWithOptions = unsafe { core::mem::transmute(function) };
			// SAFETY: The folder is NUL terminated, null selects the installed runtime and the
			// default options.
			unsafe {
				create(core::ptr::null(), user_data_folder.as_ptr(), core::ptr::null_mut(), handler)
			}
		}
		Runtime::Installed(path) => {
			let function = load_function(path, c"CreateWebViewEnvironmentWithOptionsInternal")?;
			// SAFETY: The signature the WebView2 loader uses for this export.
			let create: CreateEnvironmentWithOptionsInternal =
				unsafe { core::mem::transmute(function) };
			// SAFETY: As called by the loader for the installed runtime without options.
			unsafe {
				create(
					true,
					RUNTIME_TYPE_INSTALLED,
					user_data_folder.as_ptr(),
					core::ptr::null_mut(),
					handler,
				)
			}
		}
	};
	result.ok().map_err(|error| error.code())
}

/// The DLL stays loaded for the lifetime of the process, like the loader keeps it.
fn load_function(
	path: &Path,
	name: &core::ffi::CStr,
) -> Result<unsafe extern "system" fn() -> isize, HRESULT> {
	let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
	// SAFETY: The path is NUL terminated.
	let module: HMODULE = unsafe { LoadLibraryW(wide_path.as_ptr()) };
	if module.is_null() {
		return Err(file_not_found());
	}
	// SAFETY: `module` is loaded and `name` is NUL terminated.
	unsafe { GetProcAddress(module, name.as_ptr().cast()) }.ok_or_else(file_not_found)
}
/// `HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND)`
fn file_not_found() -> HRESULT {
	HRESULT((0x8007_0000 | ERROR_FILE_NOT_FOUND) as i32)
}
