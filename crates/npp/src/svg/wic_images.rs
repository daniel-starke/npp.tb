//! Raster images of SVGs, decoded by the Windows Imaging Component. resvg is built without its
//! raster image decoders and skips image nodes, so the tree is walked here: image nodes are drawn
//! like resvg draws them, everything else is rendered by resvg.
//!
//! usvg only keeps PNG, JPEG, GIF and WebP images and reads their size from the file header. The
//! resolvers therefore decode every image WIC can read and pass the premultiplied RGBA pixels as
//! [`usvg::ImageKind::PNG`] behind a PNG signature and `IHDR` chunk with the size.

use std::path::Path;
use std::sync::Arc;

use resvg::{tiny_skia, usvg};

/// PNG signature, `IHDR` chunk length and type, then width and height in big endian.
const HEADER_SIZE: usize = 24;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR";

/// Options that decode the images of the SVG with WIC.
pub fn options() -> usvg::Options<'static> {
	let resolve_svg_data = usvg::ImageHrefResolver::default_data_resolver();
	let resolve_svg_file = usvg::ImageHrefResolver::default_string_resolver();
	usvg::Options {
		image_href_resolver: usvg::ImageHrefResolver {
			resolve_data: Box::new(move |mime, data, options| {
				decoded_image(&data).or_else(|| resolve_svg_data(mime, data, options))
			}),
			resolve_string: Box::new(move |href, options| {
				let path = options.get_abs_path(Path::new(href));
				let data = std::fs::read(path).ok();
				data.and_then(|data| decoded_image(&data))
					.or_else(|| resolve_svg_file(href, options))
			}),
		},
		..usvg::Options::default()
	}
}

/// `tree` with its raster images, like `resvg::render`.
pub fn render(
	tree: &usvg::Tree,
	transform: tiny_skia::Transform,
	pixmap: &mut tiny_skia::PixmapMut,
) {
	match contains_raster_image(tree.root()) {
		true => render_children(tree.root(), transform, pixmap),
		false => resvg::render(tree, transform, pixmap),
	}
}

fn decoded_image(file: &[u8]) -> Option<usvg::ImageKind> {
	let bitmap = crate::wic::decode_premultiplied_rgba(file)?;
	let mut data = Vec::with_capacity(HEADER_SIZE + bitmap.pixels.len());
	data.extend_from_slice(PNG_SIGNATURE);
	data.extend_from_slice(&bitmap.width.to_be_bytes());
	data.extend_from_slice(&bitmap.height.to_be_bytes());
	data.extend_from_slice(&bitmap.pixels);
	Some(usvg::ImageKind::PNG(Arc::new(data)))
}

/// The pixels of an image of [`decoded_image`]. PNG files that WIC could not decode, which the
/// default resolvers pass on, have other sizes.
fn decoded_pixels(kind: &usvg::ImageKind) -> Option<tiny_skia::PixmapRef<'_>> {
	let usvg::ImageKind::PNG(data) = kind else { return None };
	let (header, pixels) = data.split_at_checked(HEADER_SIZE)?;
	let size =
		|offset: usize| Some(u32::from_be_bytes(header[offset..offset + 4].try_into().ok()?));
	let (width, height) = (size(16)?, size(20)?);
	let pixel_bytes = u64::from(width) * u64::from(height) * 4;
	if !header.starts_with(PNG_SIGNATURE) || pixels.len() as u64 != pixel_bytes {
		return None;
	}
	tiny_skia::PixmapRef::from_bytes(pixels, width, height)
}

fn contains_raster_image(group: &usvg::Group) -> bool {
	group.children().iter().any(|node| match node {
		usvg::Node::Image(image) => decoded_pixels(image.kind()).is_some(),
		usvg::Node::Group(group) => contains_raster_image(group),
		_ => false,
	})
}

/// `transform` is that of the parent group, as in resvg.
fn render_children(
	group: &usvg::Group,
	transform: tiny_skia::Transform,
	pixmap: &mut tiny_skia::PixmapMut,
) {
	for node in group.children() {
		match node {
			usvg::Node::Image(image) if decoded_pixels(image.kind()).is_some() => {
				draw_image(image, transform, pixmap);
			}
			usvg::Node::Group(group)
				if contains_raster_image(group) && can_draw_raster_images_in(group) =>
			{
				render_group(group, transform, pixmap);
			}
			node => {
				// `resvg::render_node` renders relative to the node's layer bounding box.
				if let Some(bounds) = node.abs_layer_bounding_box() {
					let transform = transform.pre_translate(bounds.x(), bounds.y());
					resvg::render_node(node, transform, pixmap);
				}
			}
		}
	}
}

/// Masks and filters are applied by resvg only, which then renders the group without its raster
/// images.
fn can_draw_raster_images_in(group: &usvg::Group) -> bool {
	group.mask().is_none()
		&& group.filters().is_empty()
		&& group
			.clip_path()
			.is_none_or(|clip| is_simple_clip(clip.root()) && clip.clip_path().is_none())
}

/// Clip paths of plain paths, such as the clip of `preserveAspectRatio="... slice"`.
fn is_simple_clip(group: &usvg::Group) -> bool {
	group.children().iter().all(|node| match node {
		usvg::Node::Path(_) => true,
		usvg::Node::Group(group) => group.clip_path().is_none() && is_simple_clip(group),
		_ => false,
	})
}

/// resvg's `render_group` for groups of [`can_draw_raster_images_in`].
fn render_group(
	group: &usvg::Group,
	transform: tiny_skia::Transform,
	pixmap: &mut tiny_skia::PixmapMut,
) {
	let transform = transform.pre_concat(group.transform());
	if !group.should_isolate() {
		render_children(group, transform, pixmap);
		return;
	}
	let Some(mut layer) = tiny_skia::Pixmap::new(pixmap.width(), pixmap.height()) else { return };
	render_children(group, transform, &mut layer.as_mut());
	if let Some(clip) = group.clip_path() {
		let Some(mut coverage) = tiny_skia::Pixmap::new(pixmap.width(), pixmap.height()) else {
			return;
		};
		fill_clip(clip.root(), transform.pre_concat(clip.transform()), &mut coverage.as_mut());
		layer.apply_mask(&tiny_skia::Mask::from_pixmap(
			coverage.as_ref(),
			tiny_skia::MaskType::Alpha,
		));
	}
	let paint = tiny_skia::PixmapPaint {
		opacity: group.opacity().get(),
		blend_mode: blend_mode(group.blend_mode()),
		quality: tiny_skia::FilterQuality::Nearest,
	};
	pixmap.draw_pixmap(0, 0, layer.as_ref(), &paint, tiny_skia::Transform::identity(), None);
}

/// The area of the clip path `group`, opaque.
fn fill_clip(
	group: &usvg::Group,
	transform: tiny_skia::Transform,
	coverage: &mut tiny_skia::PixmapMut,
) {
	for node in group.children() {
		match node {
			usvg::Node::Path(path) if path.is_visible() => {
				let rule = match path.fill().map(usvg::Fill::rule) {
					Some(usvg::FillRule::EvenOdd) => tiny_skia::FillRule::EvenOdd,
					_ => tiny_skia::FillRule::Winding,
				};
				let paint = tiny_skia::Paint {
					anti_alias: path.rendering_mode().use_shape_antialiasing(),
					..tiny_skia::Paint::default()
				};
				coverage.fill_path(path.data(), &paint, rule, transform, None);
			}
			usvg::Node::Group(group) => {
				fill_clip(group, transform.pre_concat(group.transform()), coverage);
			}
			_ => {}
		}
	}
}

/// resvg's `render_raster`.
fn draw_image(
	image: &usvg::Image,
	transform: tiny_skia::Transform,
	pixmap: &mut tiny_skia::PixmapMut,
) {
	let Some(pixels) = decoded_pixels(image.kind()) else { return };
	if !image.is_visible() {
		return;
	}
	let Some(rect) =
		tiny_skia::Rect::from_xywh(0.0, 0.0, pixels.width() as f32, pixels.height() as f32)
	else {
		return;
	};
	let quality = match image.rendering_mode() {
		usvg::ImageRendering::OptimizeQuality | usvg::ImageRendering::HighQuality => {
			tiny_skia::FilterQuality::Bicubic
		}
		usvg::ImageRendering::Smooth => tiny_skia::FilterQuality::Bilinear,
		usvg::ImageRendering::OptimizeSpeed
		| usvg::ImageRendering::CrispEdges
		| usvg::ImageRendering::Pixelated => tiny_skia::FilterQuality::Nearest,
	};
	let paint = tiny_skia::Paint {
		shader: tiny_skia::Pattern::new(
			pixels,
			tiny_skia::SpreadMode::Pad,
			quality,
			1.0,
			tiny_skia::Transform::default(),
		),
		..tiny_skia::Paint::default()
	};
	pixmap.fill_rect(rect, &paint, transform, None);
}

/// resvg's `convert_blend_mode`.
fn blend_mode(mode: usvg::BlendMode) -> tiny_skia::BlendMode {
	match mode {
		usvg::BlendMode::Normal => tiny_skia::BlendMode::SourceOver,
		usvg::BlendMode::Multiply => tiny_skia::BlendMode::Multiply,
		usvg::BlendMode::Screen => tiny_skia::BlendMode::Screen,
		usvg::BlendMode::Overlay => tiny_skia::BlendMode::Overlay,
		usvg::BlendMode::Darken => tiny_skia::BlendMode::Darken,
		usvg::BlendMode::Lighten => tiny_skia::BlendMode::Lighten,
		usvg::BlendMode::ColorDodge => tiny_skia::BlendMode::ColorDodge,
		usvg::BlendMode::ColorBurn => tiny_skia::BlendMode::ColorBurn,
		usvg::BlendMode::HardLight => tiny_skia::BlendMode::HardLight,
		usvg::BlendMode::SoftLight => tiny_skia::BlendMode::SoftLight,
		usvg::BlendMode::Difference => tiny_skia::BlendMode::Difference,
		usvg::BlendMode::Exclusion => tiny_skia::BlendMode::Exclusion,
		usvg::BlendMode::Hue => tiny_skia::BlendMode::Hue,
		usvg::BlendMode::Saturation => tiny_skia::BlendMode::Saturation,
		usvg::BlendMode::Color => tiny_skia::BlendMode::Color,
		usvg::BlendMode::Luminosity => tiny_skia::BlendMode::Luminosity,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ignores_png_files_that_were_not_decoded() {
		let png =
			b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x02\0\0\0\x01\x08\x02\0\0\0\x7b\x40\xe8\xdd";
		assert!(decoded_pixels(&usvg::ImageKind::PNG(Arc::new(png.to_vec()))).is_none());
		let decoded = decoded_image(&crate::wic::encode_png(1, 1, &[1, 2, 3, 4]).unwrap()).unwrap();
		assert_eq!(decoded_pixels(&decoded).map(|pixels| pixels.data().len()), Some(4));
	}
}
