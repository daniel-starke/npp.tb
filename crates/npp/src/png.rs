//! Encoding pixels as PNG file: with the Windows Imaging Component, or with tiny-skia under the
//! `pure-rust` feature and on other hosts.

/// A PNG file of `width` x `height` pixels in 32-bit BGRA with straight alpha, rows top-down.
/// `None` for an empty image or a pixel buffer of another size.
pub fn encode_straight_bgra(width: u32, height: u32, bgra: &[u8]) -> Option<Vec<u8>> {
	let buffer_size = width.checked_mul(height)?.checked_mul(4)?;
	if width == 0 || height == 0 || bgra.len() != buffer_size as usize {
		return None;
	}
	#[cfg(all(windows, not(feature = "pure-rust")))]
	return crate::wic::encode_png(width, height, bgra);
	#[cfg(not(all(windows, not(feature = "pure-rust"))))]
	{
		use resvg::tiny_skia::{IntSize, Pixmap};
		let premultiply =
			|channel: u8, alpha: u8| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
		let rgba = bgra
			.as_chunks::<4>()
			.0
			.iter()
			.flat_map(|&[blue, green, red, alpha]| {
				[
					premultiply(red, alpha),
					premultiply(green, alpha),
					premultiply(blue, alpha),
					alpha,
				]
			})
			.collect();
		Pixmap::from_vec(rgba, IntSize::from_wh(width, height)?)?.encode_png().ok()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn encodes_png_files() {
		let png = encode_straight_bgra(2, 1, &[0, 0, 255, 255, 255, 0, 0, 128]).unwrap();
		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
		assert_eq!(encode_straight_bgra(2, 1, &[0; 4]), None);
		assert_eq!(encode_straight_bgra(0, 0, &[]), None);
	}
}
