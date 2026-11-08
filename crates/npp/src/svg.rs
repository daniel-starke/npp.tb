//! Renders SVG images at runtime, so that icons are sharp at every size and DPI.

#[cfg(all(windows, not(feature = "pure-rust")))]
mod wic_images;

use resvg::{tiny_skia, usvg};

/// Larger images are not rendered, against documents that ask for huge pixmaps.
const MAX_SIDE: u32 = 4096;

/// `svg` scaled to `width` x `height` pixels, in premultiplied RGBA.
fn render_pixmap(svg: &str, width: u32, height: u32) -> Option<tiny_skia::Pixmap> {
	if width > MAX_SIDE || height > MAX_SIDE {
		return None;
	}
	let tree = usvg::Tree::from_str(svg, &options()).ok()?;
	render_tree(&tree, width, height)
}

fn options() -> usvg::Options<'static> {
	#[cfg(all(windows, not(feature = "pure-rust")))]
	return wic_images::options();
	#[cfg(not(all(windows, not(feature = "pure-rust"))))]
	usvg::Options::default()
}

fn render_tree(tree: &usvg::Tree, width: u32, height: u32) -> Option<tiny_skia::Pixmap> {
	let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
	let size = tree.size();
	let transform = tiny_skia::Transform::from_scale(
		width as f32 / size.width(),
		height as f32 / size.height(),
	);
	#[cfg(all(windows, not(feature = "pure-rust")))]
	wic_images::render(tree, transform, &mut pixmap.as_mut());
	#[cfg(not(all(windows, not(feature = "pure-rust"))))]
	resvg::render(tree, transform, &mut pixmap.as_mut());
	Some(pixmap)
}

/// `pixmap` in 32-bit BGRA order with straight alpha.
fn straight_bgra(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
	pixmap
		.pixels()
		.iter()
		.flat_map(|pixel| {
			let color = pixel.demultiply();
			[color.blue(), color.green(), color.red(), color.alpha()]
		})
		.collect()
}

fn encode_png(pixmap: &tiny_skia::Pixmap) -> Option<Vec<u8>> {
	#[cfg(all(windows, not(feature = "pure-rust")))]
	return crate::wic::encode_png(pixmap.width(), pixmap.height(), &straight_bgra(pixmap));
	#[cfg(not(all(windows, not(feature = "pure-rust"))))]
	pixmap.encode_png().ok()
}

/// `svg` as a square image of `size` pixels in 32-bit BGRA order with straight (not
/// premultiplied) alpha, rows top-down, as GDI bitmaps and icons expect it.
pub fn render_bgra(svg: &str, size: u32) -> Option<Vec<u8>> {
	Some(straight_bgra(&render_pixmap(svg, size, size)?))
}

/// `svg` scaled to `width` x `height` pixels as PNG file.
pub fn render_png(svg: &str, width: u32, height: u32) -> Option<Vec<u8>> {
	encode_png(&render_pixmap(svg, width, height)?)
}

/// `svg` at `scale` times its own size as PNG file with its size in pixels. Images larger than
/// the maximum side are scaled down to fit. resvg is built without text support: text must be
/// converted to paths before.
pub fn render_png_scaled(svg: &str, scale: f32) -> Option<(Vec<u8>, u32, u32)> {
	let tree = usvg::Tree::from_str(svg, &options()).ok()?;
	let size = tree.size();
	let scale = scale.min(MAX_SIDE as f32 / size.width().max(size.height()));
	if !scale.is_finite() || scale <= 0.0 {
		return None;
	}
	let width = (size.width() * scale).ceil().max(1.0) as u32;
	let height = (size.height() * scale).ceil().max(1.0) as u32;
	let png = encode_png(&render_tree(&tree, width, height)?)?;
	Some((png, width, height))
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
	fn scales_png_to_fit() {
		let (_, width, height) = render_png_scaled(LEFT_HALF, 2.0).unwrap();
		assert_eq!((width, height), (32, 32));
		let huge = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10000" height="100"/>"#;
		let (_, width, height) = render_png_scaled(huge, 2.0).unwrap();
		assert_eq!((width, height), (MAX_SIDE, 41));
	}

	#[test]
	#[cfg(any(windows, feature = "pure-rust"))]
	fn draws_raster_images() {
		// A red 2 x 1 PNG of the image nodes of diagrams.
		let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><image width="16" height="16" preserveAspectRatio="none" href="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAIAAAB7QOjdAAAADUlEQVR4nGP4z8AARAAI/gH/xp559wAAAABJRU5ErkJggg=="/></svg>"#;
		let pixels = render_bgra(svg, 4).unwrap();
		assert_eq!(&pixels[0..4], &[0x00, 0x00, 0xFF, 0xFF]);
	}

	/// A 2 x 1 BMP with a red and a blue pixel, a format resvg cannot decode.
	#[cfg(all(windows, not(feature = "pure-rust")))]
	const RED_BLUE_BMP: &str = "data:image/bmp;base64,Qk0+AAAAAAAAADYAAAAoAAAAAgAAAAEAAAABABgAAAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAD//wAAAAA=";

	#[test]
	#[cfg(all(windows, not(feature = "pure-rust")))]
	fn draws_raster_images_of_windows_codecs() {
		let svg = format!(
			r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><image width="16" height="16" preserveAspectRatio="none" href="{RED_BLUE_BMP}"/></svg>"#
		);
		let pixels = render_bgra(&svg, 4).unwrap();
		assert_eq!(&pixels[0..4], &[0x00, 0x00, 0xFF, 0xFF]);
		assert_eq!(&pixels[12..16], &[0xFF, 0x00, 0x00, 0xFF]);
	}

	#[test]
	#[cfg(all(windows, not(feature = "pure-rust")))]
	fn draws_raster_images_in_clipped_and_transparent_groups() {
		// `slice` clips the image scaled to 4 x 2 to its right half, the blue pixel.
		let svg = format!(
			r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 2"><rect width="4" height="2" fill="#00ff00"/><g opacity="0.5"><image width="2" height="2" preserveAspectRatio="xMaxYMid slice" style="image-rendering:pixelated" href="{RED_BLUE_BMP}"/></g></svg>"##
		);
		let pixels = render_bgra(&svg, 4).unwrap();
		let pixel = |x: usize| &pixels[x * 4..x * 4 + 4];
		assert_eq!(pixel(0), &[0x80, 0x80, 0x00, 0xFF]);
		assert_eq!(pixel(1), &[0x80, 0x80, 0x00, 0xFF]);
		assert_eq!(pixel(2), &[0x00, 0xFF, 0x00, 0xFF]);
	}

	#[test]
	fn rejects_invalid_input() {
		assert!(render_bgra("<svg", 16).is_none());
		assert!(render_bgra("", 16).is_none());
		assert!(render_bgra(LEFT_HALF, 0).is_none());
		assert!(render_png(LEFT_HALF, MAX_SIDE + 1, 1).is_none());
	}
}
