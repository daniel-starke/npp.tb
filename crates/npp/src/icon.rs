//! Pre-rendered toolbar icon data as produced by `npp-build`.

/// A square image in 32-bit BGRA order with straight (not premultiplied) alpha, rows top-down.
#[derive(Debug)]
pub struct IconImage {
	pub size: u32,
	pub bgra: &'static [u8],
}

/// A toolbar icon rendered for the light and the dark theme in several sizes, sorted ascending.
#[derive(Debug)]
pub struct ToolbarIcon {
	pub light: &'static [IconImage],
	pub dark: &'static [IconImage],
}

/// Picks the smallest image at least `target_size` pixels large, falling back to the largest one.
/// Downscaling by Windows looks considerably better than upscaling.
pub fn best_image_for_size(images: &[IconImage], target_size: u32) -> Option<&IconImage> {
	images
		.iter()
		.filter(|image| image.size >= target_size)
		.min_by_key(|image| image.size)
		.or_else(|| images.iter().max_by_key(|image| image.size))
}

#[cfg(test)]
mod tests {
	use super::*;

	const IMAGES: [IconImage; 3] = [
		IconImage { size: 16, bgra: &[] },
		IconImage { size: 24, bgra: &[] },
		IconImage { size: 32, bgra: &[] },
	];

	#[test]
	fn picks_exact_or_next_larger_size() {
		assert_eq!(best_image_for_size(&IMAGES, 16).map(|image| image.size), Some(16));
		assert_eq!(best_image_for_size(&IMAGES, 20).map(|image| image.size), Some(24));
	}

	#[test]
	fn falls_back_to_largest_size() {
		assert_eq!(best_image_for_size(&IMAGES, 64).map(|image| image.size), Some(32));
		assert!(best_image_for_size(&[], 16).is_none());
	}
}
