//! Renders SVG images at runtime, so that icons are sharp at every size and DPI.

use resvg::{tiny_skia, usvg};

/// Larger images are not rendered, against documents that ask for huge pixmaps.
const MAX_SIDE: u32 = 4096;

/// `svg` scaled to `width` x `height` pixels, in premultiplied RGBA.
fn render_pixmap(svg: &str, width: u32, height: u32) -> Option<tiny_skia::Pixmap> {
	if width > MAX_SIDE || height > MAX_SIDE {
		return None;
	}
	let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
	let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
	let size = tree.size();
	let transform = tiny_skia::Transform::from_scale(
		width as f32 / size.width(),
		height as f32 / size.height(),
	);
	resvg::render(&tree, transform, &mut pixmap.as_mut());
	Some(pixmap)
}

/// `svg` as a square image of `size` pixels in 32-bit BGRA order with straight (not
/// premultiplied) alpha, rows top-down, as GDI bitmaps and icons expect it.
pub fn render_bgra(svg: &str, size: u32) -> Option<Vec<u8>> {
	let pixmap = render_pixmap(svg, size, size)?;
	Some(
		pixmap
			.pixels()
			.iter()
			.flat_map(|pixel| {
				let color = pixel.demultiply();
				[color.blue(), color.green(), color.red(), color.alpha()]
			})
			.collect(),
	)
}

/// `svg` scaled to `width` x `height` pixels as PNG file.
pub fn render_png(svg: &str, width: u32, height: u32) -> Option<Vec<u8>> {
	render_pixmap(svg, width, height)?.encode_png().ok()
}

#[cfg(test)]
mod tests {
	use super::*;

	const LEFT_HALF: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="8" height="16" fill="#102030"/></svg>"##;

	#[test]
	fn renders_straight_alpha_bgra() {
		let pixels = render_bgra(LEFT_HALF, 32).unwrap();
		assert_eq!(pixels.len(), 32 * 32 * 4);
		assert_eq!(&pixels[0..4], &[0x30, 0x20, 0x10, 0xFF]);
		assert_eq!(pixels[31 * 4 + 3], 0);
	}

	#[test]
	fn encodes_png() {
		let png = render_png(LEFT_HALF, 20, 10).unwrap();
		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
		// The IHDR chunk follows the signature with width and height.
		assert_eq!(&png[16..24], &[0, 0, 0, 20, 0, 0, 0, 10]);
	}

	#[test]
	fn rejects_invalid_input() {
		assert!(render_bgra("<svg", 16).is_none());
		assert!(render_bgra("", 16).is_none());
		assert!(render_bgra(LEFT_HALF, 0).is_none());
		assert!(render_png(LEFT_HALF, MAX_SIDE + 1, 1).is_none());
	}
}
