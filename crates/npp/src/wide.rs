//! UTF-16 helpers for Win32 string parameters.

pub fn to_wide_null_terminated(text: &str) -> Vec<u16> {
	text.encode_utf16().chain(Some(0)).collect()
}

/// Decodes up to the first NUL (or the whole buffer if there is none), replacing invalid
/// surrogates.
pub fn from_wide_until_null(buffer: &[u16]) -> String {
	let length = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
	String::from_utf16_lossy(&buffer[..length])
}

/// Copies `text` into a fixed size NUL terminated buffer, truncating at a character boundary if
/// needed.
pub fn to_fixed_wide<const CAPACITY: usize>(text: &str) -> [u16; CAPACITY] {
	let mut buffer = [0u16; CAPACITY];
	let mut used = 0;
	for character in text.chars() {
		let mut encoded = [0u16; 2];
		let units = character.encode_utf16(&mut encoded);
		if used + units.len() >= CAPACITY {
			break;
		}
		buffer[used..used + units.len()].copy_from_slice(units);
		used += units.len();
	}
	buffer
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trips_through_null_terminated_buffer() {
		let wide = to_wide_null_terminated("Tabelle ä 表");
		assert_eq!(wide.last(), Some(&0));
		assert_eq!(from_wide_until_null(&wide), "Tabelle ä 表");
	}

	#[test]
	fn fixed_buffer_truncates_and_keeps_terminator() {
		let buffer: [u16; 4] = to_fixed_wide("abcdef");
		assert_eq!(buffer, [b'a' as u16, b'b' as u16, b'c' as u16, 0]);
	}

	#[test]
	fn fixed_buffer_never_splits_surrogate_pairs() {
		let buffer: [u16; 3] = to_fixed_wide("a😀");
		assert_eq!(from_wide_until_null(&buffer), "a");
	}
}
