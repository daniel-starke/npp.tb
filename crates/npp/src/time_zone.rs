//! The local time zone of Windows with its daylight saving time rules per year.

use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::Time::{
	DYNAMIC_TIME_ZONE_INFORMATION, FileTimeToSystemTime, GetDynamicTimeZoneInformation,
	SystemTimeToFileTime, SystemTimeToTzSpecificLocalTimeEx,
};

/// 100 ns intervals between 1601-01-01 (`FILETIME`) and 1970-01-01.
const EPOCH_DIFFERENCE: f64 = 116_444_736_000_000_000.0;
/// The range of `SYSTEMTIME`: years 1601 to 30827, in milliseconds since 1970.
const FIRST_TIME: f64 = -11_644_473_600_000.0 + 86_400_000.0;
const LAST_TIME: f64 = 910_692_730_085_000.0 - 86_400_000.0;

/// The time zone Windows was set to when this was created.
pub struct TimeZone {
	information: DYNAMIC_TIME_ZONE_INFORMATION,
}

fn file_time_of(time: f64) -> FILETIME {
	let intervals = (time * 10_000.0 + EPOCH_DIFFERENCE) as u64;
	FILETIME { dwLowDateTime: intervals as u32, dwHighDateTime: (intervals >> 32) as u32 }
}

fn intervals_of(file_time: &FILETIME) -> u64 {
	(u64::from(file_time.dwHighDateTime) << 32) | u64::from(file_time.dwLowDateTime)
}

impl TimeZone {
	pub fn new() -> Self {
		// SAFETY: plain data, all zero is a valid value.
		let mut information: DYNAMIC_TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
		// SAFETY: `information` is a valid output buffer of the right type.
		unsafe { GetDynamicTimeZoneInformation(&mut information) };
		TimeZone { information }
	}

	/// Minutes the local time is ahead of UTC at `time`, in milliseconds since 1970 (UTC). Times
	/// outside of the years Windows handles use the offset of the nearest one.
	pub fn utc_offset(&self, time: f64) -> f64 {
		if time.is_nan() {
			return 0.0;
		}
		let time = time.clamp(FIRST_TIME, LAST_TIME);
		let utc_file_time = file_time_of(time);
		let zero = SYSTEMTIME {
			wYear: 0,
			wMonth: 0,
			wDayOfWeek: 0,
			wDay: 0,
			wHour: 0,
			wMinute: 0,
			wSecond: 0,
			wMilliseconds: 0,
		};
		let (mut utc, mut local) = (zero, zero);
		let mut local_file_time = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
		// SAFETY: all pointers refer to live values of the declared types.
		let is_converted = unsafe {
			FileTimeToSystemTime(&utc_file_time, &mut utc) != 0
				&& SystemTimeToTzSpecificLocalTimeEx(&self.information, &utc, &mut local) != 0
				&& SystemTimeToFileTime(&local, &mut local_file_time) != 0
		};
		if !is_converted {
			return -f64::from(self.information.Bias);
		}
		// `SYSTEMTIME` has whole milliseconds; the sub-millisecond rest cancels out.
		let difference =
			intervals_of(&local_file_time) as f64 - intervals_of(&utc_file_time) as f64;
		(difference / 600_000_000.0).round()
	}
}

impl Default for TimeZone {
	fn default() -> Self {
		Self::new()
	}
}
