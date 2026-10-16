//! Tiny persistent settings store based on Win32 private profile (INI) files.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::System::WindowsProgramming::{
	GetPrivateProfileIntW, GetPrivateProfileStringW, WritePrivateProfileStringW,
};

use crate::wide::to_wide_null_terminated;

pub struct IniFile {
	wide_path: Vec<u16>,
}

impl IniFile {
	pub fn new(path: &Path) -> Self {
		Self { wide_path: path.as_os_str().encode_wide().chain(Some(0)).collect() }
	}

	pub fn contains(&self, section: &str, key: &str) -> bool {
		// A default that no real value has, to tell "missing" from "empty".
		const MISSING: &str = "\u{1}missing";
		let section = to_wide_null_terminated(section);
		let key = to_wide_null_terminated(key);
		let missing = to_wide_null_terminated(MISSING);
		let mut value = [0u16; 16];
		// SAFETY: All strings are NUL terminated and alive for the call. The output buffer
		// length is passed.
		let length = unsafe {
			GetPrivateProfileStringW(
				section.as_ptr(),
				key.as_ptr(),
				missing.as_ptr(),
				value.as_mut_ptr(),
				value.len() as u32,
				self.wide_path.as_ptr(),
			)
		} as usize;
		value.get(..length) != Some(&missing[..missing.len() - 1])
	}

	pub fn read_u32(&self, section: &str, key: &str, default: u32) -> u32 {
		let section = to_wide_null_terminated(section);
		let key = to_wide_null_terminated(key);
		// SAFETY: All strings are NUL terminated and alive for the call.
		let value = unsafe {
			GetPrivateProfileIntW(section.as_ptr(), key.as_ptr(), -1, self.wide_path.as_ptr())
		};
		u32::try_from(value).unwrap_or(default)
	}

	/// Best effort. A read-only settings directory is not worth bothering the user about.
	pub fn write_u32(&self, section: &str, key: &str, value: u32) {
		let section = to_wide_null_terminated(section);
		let key = to_wide_null_terminated(key);
		let value = to_wide_null_terminated(&value.to_string());
		// SAFETY: All strings are NUL terminated and alive for the call.
		unsafe {
			WritePrivateProfileStringW(
				section.as_ptr(),
				key.as_ptr(),
				value.as_ptr(),
				self.wide_path.as_ptr(),
			)
		};
	}
}
