//! Images on the clipboard as HTML `<img>` tags with Base64 `data:` URIs, for Paste Embedded
//! Image.
//!
//! The clipboard data is given as bytes; reading it is up to the caller. Images are taken in
//! this order of preference: image files (copied in Explorer), SVG, PNG/JPEG/GIF/WebP in the
//! order the clipboard lists them, device independent bitmaps (converted to PNG) and at last
//! text that is an SVG document.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::image_embedding::{EmbeddedImage, media_type_of};
use crate::markdown_html::escape::push_escaped_attribute;

/// A clipboard format as far as images are concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardFormat<'a> {
	/// `CF_HDROP`: `DROPFILES` followed by the file names.
	FileDrop,
	/// `CF_DIB` or `CF_DIBV5`: `BITMAPINFO` followed by the pixels.
	Bitmap,
	/// `CF_UNICODETEXT`: NUL terminated UTF-16 text.
	UnicodeText,
	/// A registered format by its name, such as `PNG` or `image/svg+xml`.
	Registered(&'a str),
	Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ImageSource {
	Files,
	Svg,
	ImageFile,
	Bitmap,
	SvgText,
}

impl ImageSource {
	fn of(format: ClipboardFormat) -> Option<Self> {
		Some(match format {
			ClipboardFormat::FileDrop => Self::Files,
			ClipboardFormat::Bitmap => Self::Bitmap,
			ClipboardFormat::UnicodeText => Self::SvgText,
			ClipboardFormat::Registered(name) => {
				const IMAGE_FILE_FORMATS: [&str; 8] = [
					"PNG",
					"image/png",
					"JFIF",
					"image/jpeg",
					"GIF",
					"image/gif",
					"image/webp",
					"image/bmp",
				];
				if name.eq_ignore_ascii_case("image/svg+xml") {
					Self::Svg
				} else if IMAGE_FILE_FORMATS.iter().any(|known| known.eq_ignore_ascii_case(name)) {
					Self::ImageFile
				} else {
					return None;
				}
			}
			ClipboardFormat::Other => return None,
		})
	}
}

/// Indices of the `formats` that may hold an image, most preferred first. Formats of the same
/// kind keep the clipboard's order, which is the order of preference of the copying application.
pub fn formats_by_preference(formats: &[ClipboardFormat]) -> Vec<usize> {
	let mut candidates: Vec<(ImageSource, usize)> = formats
		.iter()
		.enumerate()
		.filter_map(|(index, &format)| Some((ImageSource::of(format)?, index)))
		.collect();
	candidates.sort_unstable();
	candidates.into_iter().map(|(_, index)| index).collect()
}

/// Uncompressed pixels, in 32-bit BGRA with straight alpha, rows top-down.
#[derive(Debug, PartialEq, Eq)]
pub struct Pixels {
	pub width: u32,
	pub height: u32,
	pub bgra: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClipboardImage {
	Image(EmbeddedImage),
	/// Still to be encoded as PNG.
	Pixels(Pixels),
	/// Local image files, still to be read.
	Files(Vec<PathBuf>),
}

/// The image in `bytes`, the clipboard content in `format`. `None` if it holds no image.
pub fn clipboard_image(format: ClipboardFormat, bytes: &[u8]) -> Option<ClipboardImage> {
	match ImageSource::of(format)? {
		ImageSource::Files => {
			let files: Vec<PathBuf> = dropped_files(bytes)
				.into_iter()
				.filter(|path| media_type_of(path).is_some())
				.collect();
			(!files.is_empty()).then_some(ClipboardImage::Files(files))
		}
		ImageSource::Svg => svg_image(&text_of_bytes(bytes)?),
		ImageSource::ImageFile => image_file(bytes).map(ClipboardImage::Image),
		ImageSource::Bitmap => decode_dib(bytes).map(ClipboardImage::Pixels),
		ImageSource::SvgText => svg_image(&utf16_text(bytes)?),
	}
}

/// NUL terminated UTF-16 text in little endian byte order.
fn utf16_text(bytes: &[u8]) -> Option<String> {
	let units: Vec<u16> = utf16_units(bytes).take_while(|&unit| unit != 0).collect();
	String::from_utf16(&units).ok()
}

fn utf16_units(bytes: &[u8]) -> impl Iterator<Item = u16> {
	bytes.as_chunks::<2>().0.iter().map(|&unit| u16::from_le_bytes(unit))
}

fn svg_image(text: &str) -> Option<ClipboardImage> {
	let svg = svg_document(text)?;
	let bytes = svg.as_bytes().to_vec();
	Some(ClipboardImage::Image(EmbeddedImage { media_type: "image/svg+xml", bytes }))
}

/// Text in UTF-8, or in UTF-16 with byte order mark, without the padding of global memory.
fn text_of_bytes(bytes: &[u8]) -> Option<String> {
	if let Some(utf16) = bytes.strip_prefix(&[0xFF, 0xFE]) {
		return utf16_text(utf16);
	}
	let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
	let length = bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len());
	String::from_utf8(bytes[..length].to_vec()).ok()
}

/// `text` without surrounding white space if it is an SVG document: an `<svg>` root element,
/// optionally after an XML declaration, comments and a document type declaration.
pub fn svg_document(text: &str) -> Option<&str> {
	let text = text.trim_start_matches('\u{FEFF}').trim();
	let mut rest = text;
	loop {
		rest = rest.trim_start();
		if let Some(declaration) = rest.strip_prefix("<?") {
			rest = &declaration[declaration.find("?>")? + 2..];
		} else if let Some(comment) = rest.strip_prefix("<!--") {
			rest = &comment[comment.find("-->")? + 3..];
		} else if let Some(document_type) = rest.strip_prefix("<!") {
			let end = match (document_type.find('['), document_type.find('>')) {
				(Some(subset), Some(end)) if subset < end => document_type.find("]>")? + 2,
				(_, end) => end? + 1,
			};
			rest = &document_type[end..];
		} else {
			break;
		}
	}
	let after_name = rest.strip_prefix("<svg")?;
	let is_svg_element = after_name.starts_with(|character: char| {
		character.is_ascii_whitespace() || character == '>' || character == '/'
	});
	(is_svg_element && text.ends_with('>')).then_some(text)
}

/// An image file of a format browsers display, recognized by its signature.
fn image_file(bytes: &[u8]) -> Option<EmbeddedImage> {
	const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
	let (media_type, length) = if bytes.starts_with(PNG_SIGNATURE) {
		("image/png", png_length(bytes).unwrap_or(bytes.len()))
	} else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
		("image/jpeg", bytes.len())
	} else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
		("image/gif", bytes.len())
	} else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
		("image/webp", bytes.len())
	} else if bytes.starts_with(b"BM") {
		("image/bmp", bytes.len())
	} else {
		return None;
	};
	Some(EmbeddedImage { media_type, bytes: bytes[..length].to_vec() })
}

/// The length of the PNG file at the start of `bytes`, up to the end of its `IEND` chunk.
fn png_length(bytes: &[u8]) -> Option<usize> {
	let mut offset = 8;
	loop {
		let length = u32::from_be_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize;
		let chunk_type = bytes.get(offset + 4..offset + 8)?;
		// Length, type, data and CRC.
		offset = offset.checked_add(length)?.checked_add(12)?;
		if chunk_type == b"IEND" {
			return (offset <= bytes.len()).then_some(offset);
		}
	}
}

/// The file names of a `DROPFILES` structure, followed by NUL terminated names and an empty one.
fn dropped_files(bytes: &[u8]) -> Vec<PathBuf> {
	let read_u32 = |offset: usize| -> Option<u32> {
		Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
	};
	let (Some(names_offset), Some(is_wide)) = (read_u32(0), read_u32(16)) else {
		return Vec::new();
	};
	let Some(names) = bytes.get(names_offset as usize..) else {
		return Vec::new();
	};
	let names: Vec<String> = if is_wide != 0 {
		let units: Vec<u16> = utf16_units(names).collect();
		units
			.split(|&unit| unit == 0)
			.take_while(|name| !name.is_empty())
			.map(String::from_utf16_lossy)
			.collect()
	} else {
		names
			.split(|&byte| byte == 0)
			.take_while(|name| !name.is_empty())
			.map(|name| String::from_utf8_lossy(name).into_owned())
			.collect()
	};
	names.into_iter().map(PathBuf::from).collect()
}

/// Larger bitmaps are refused, against headers that ask for huge allocations.
const MAX_PIXELS: u64 = 1 << 26;

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

/// The pixels of a packed device independent bitmap (`CF_DIB`, `CF_DIBV5`) with 1, 2, 4, 8, 16,
/// 24 or 32 bits per pixel, uncompressed or with bit fields. Per the format, 32-bit pixels
/// without an alpha mask are opaque. So are pixels whose alpha is zero throughout, as written by
/// applications that do not set it.
fn decode_dib(bytes: &[u8]) -> Option<Pixels> {
	let read_u32 = |offset: usize| -> Option<u32> {
		Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
	};
	let header_size = read_u32(0)? as usize;
	if header_size < 40 {
		return None;
	}
	let width = read_u32(4)? as i32;
	let height = read_u32(8)? as i32;
	let bit_count = u16::from_le_bytes(bytes.get(14..16)?.try_into().ok()?);
	let compression = read_u32(16)?;
	let colors_used = read_u32(32)?;
	if width <= 0 || height == 0 {
		return None;
	}
	let is_top_down = height < 0;
	let (width, height) = (width.unsigned_abs(), height.unsigned_abs());
	if u64::from(width) * u64::from(height) > MAX_PIXELS {
		return None;
	}

	let mask_count = match compression {
		BI_RGB => 0,
		BI_BITFIELDS => 3,
		BI_ALPHABITFIELDS => 4,
		_ => return None,
	};
	// Masks follow a `BITMAPINFOHEADER`, later headers contain them.
	let masks_after_header = if header_size == 40 { mask_count } else { 0 };
	let masks: Option<[u32; 4]> = match mask_count {
		0 => None,
		_ => Some([
			read_u32(40)?,
			read_u32(44)?,
			read_u32(48)?,
			match mask_count == 4 || header_size >= 56 {
				true => read_u32(52)?,
				false => 0,
			},
		]),
	};
	let palette_size = match (bit_count, colors_used) {
		(1 | 2 | 4 | 8, 0) => 1usize << bit_count,
		(1 | 2 | 4 | 8, used) => (used as usize).min(1 << bit_count),
		(16 | 24 | 32, used) => used as usize,
		_ => return None,
	};
	let palette_offset = header_size.checked_add(masks_after_header * 4)?;
	let pixels_offset = palette_offset.checked_add(palette_size.checked_mul(4)?)?;
	let palette = bytes.get(palette_offset..pixels_offset)?;
	let stride = (u64::from(width) * u64::from(bit_count)).div_ceil(32) * 4;
	let pixels_end = (pixels_offset as u64).checked_add(stride * u64::from(height))?;
	if pixels_end > bytes.len() as u64 {
		return None;
	}
	let stride = stride as usize;

	let masks = match (masks, bit_count) {
		(Some(masks), 16 | 32) => Some(masks),
		(Some(_), _) => return None,
		(None, 16) => Some([0x7C00, 0x03E0, 0x001F, 0]),
		(None, _) => None,
	};
	let mut bgra = Vec::with_capacity(width as usize * height as usize * 4);
	for row in 0..height as usize {
		let source_row = if is_top_down { row } else { height as usize - 1 - row };
		let line = &bytes[pixels_offset + source_row * stride..][..stride];
		for column in 0..width as usize {
			let pixel = match (bit_count, masks) {
				(16 | 32, Some(masks)) => {
					let value = match bit_count {
						16 => {
							u32::from(u16::from_le_bytes([line[column * 2], line[column * 2 + 1]]))
						}
						_ => u32::from_le_bytes(line[column * 4..][..4].try_into().ok()?),
					};
					let channel = |mask: u32| scaled_channel(value, mask);
					let alpha = if masks[3] == 0 { 255 } else { channel(masks[3]) };
					[channel(masks[2]), channel(masks[1]), channel(masks[0]), alpha]
				}
				(24 | 32, None) => {
					let pixel = &line[column * usize::from(bit_count / 8)..];
					[pixel[0], pixel[1], pixel[2], 255]
				}
				_ => {
					let bits = usize::from(bit_count);
					let bit_offset = column * bits;
					let byte = line[bit_offset / 8];
					let shift = 8 - bits - bit_offset % 8;
					let index = usize::from(byte >> shift) & ((1 << bits) - 1);
					match palette.get(index * 4..index * 4 + 3) {
						Some(color) => [color[0], color[1], color[2], 255],
						None => [0, 0, 0, 255],
					}
				}
			};
			bgra.extend_from_slice(&pixel);
		}
	}
	if bgra.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
		bgra.as_chunks_mut::<4>().0.iter_mut().for_each(|pixel| pixel[3] = 255);
	}
	Some(Pixels { width, height, bgra })
}

/// The channel of `value` selected by `mask`, scaled to 8 bits.
fn scaled_channel(value: u32, mask: u32) -> u8 {
	if mask == 0 {
		return 0;
	}
	let channel = u64::from((value & mask) >> mask.trailing_zeros());
	let maximum = u64::from(mask >> mask.trailing_zeros());
	(channel * 255 / maximum) as u8
}

/// Why images cannot be pasted.
#[derive(Debug)]
pub enum PasteImageError {
	TooLarge { file: Option<PathBuf>, size: u64, max_size: u64 },
	Unreadable { file: PathBuf, error: std::io::Error },
	NotConvertible,
}

impl fmt::Display for PasteImageError {
	fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Self::TooLarge { file, size, max_size } => {
				match file {
					Some(file) => write!(formatter, "The image {}", file.display())?,
					None => write!(formatter, "The image on the clipboard")?,
				}
				write!(
					formatter,
					" has {size} bytes, more than the {max_size} bytes of the setting [Html] MaxEmbeddedImageBytes."
				)
			}
			Self::Unreadable { file, error } => {
				write!(formatter, "Cannot read {}:\n{error}", file.display())
			}
			Self::NotConvertible => {
				write!(formatter, "The image on the clipboard cannot be converted to PNG.")
			}
		}
	}
}

/// An image to paste, with the alternative text taken from its file name.
#[derive(Debug, PartialEq, Eq)]
pub struct PastedImage {
	pub image: EmbeddedImage,
	pub alt: Option<String>,
}

/// The image files of `image` within `max_size` bytes each. `encode_png` converts pixels.
pub fn embedded_images(
	image: ClipboardImage,
	max_size: u64,
	encode_png: impl FnOnce(&Pixels) -> Option<Vec<u8>>,
) -> Result<Vec<PastedImage>, PasteImageError> {
	let too_large = |file: Option<&Path>, size: u64| PasteImageError::TooLarge {
		file: file.map(Path::to_path_buf),
		size,
		max_size,
	};
	let image = match image {
		ClipboardImage::Image(image) => image,
		ClipboardImage::Pixels(pixels) => {
			let bytes = encode_png(&pixels).ok_or(PasteImageError::NotConvertible)?;
			EmbeddedImage { media_type: "image/png", bytes }
		}
		ClipboardImage::Files(files) => {
			let mut images = Vec::with_capacity(files.len());
			for file in files {
				let unreadable = |error| PasteImageError::Unreadable { file: file.clone(), error };
				let size = std::fs::metadata(&file).map_err(unreadable)?.len();
				if size > max_size {
					return Err(too_large(Some(&file), size));
				}
				let bytes = std::fs::read(&file).map_err(unreadable)?;
				let media_type = media_type_of(&file).unwrap_or("application/octet-stream");
				let alt = file.file_stem().map(|stem| stem.to_string_lossy().into_owned());
				images.push(PastedImage { image: EmbeddedImage { media_type, bytes }, alt });
			}
			return Ok(images);
		}
	};
	match image.bytes.len() as u64 {
		size if size > max_size => Err(too_large(None, size)),
		_ => Ok(vec![PastedImage { image, alt: None }]),
	}
}

/// An HTML image element showing `pasted`, with an `alt` attribute only for a known file name.
pub fn img_tag(pasted: &PastedImage) -> String {
	let mut tag = format!("<img src=\"{}\"", pasted.image.data_uri());
	if let Some(alt) = &pasted.alt {
		tag.push_str(" alt=\"");
		push_escaped_attribute(&mut tag, alt);
		tag.push('"');
	}
	tag.push('>');
	tag
}

#[cfg(test)]
mod tests {
	use super::*;

	fn image(media_type: &'static str, bytes: &[u8]) -> Option<ClipboardImage> {
		Some(ClipboardImage::Image(EmbeddedImage { media_type, bytes: bytes.to_vec() }))
	}

	#[test]
	fn prefers_files_then_svg_then_image_files_then_bitmaps_then_text() {
		let formats = [
			ClipboardFormat::UnicodeText,
			ClipboardFormat::Bitmap,
			ClipboardFormat::Registered("Rich Text Format"),
			ClipboardFormat::Registered("JFIF"),
			ClipboardFormat::Registered("PNG"),
			ClipboardFormat::Other,
			ClipboardFormat::Registered("image/svg+xml"),
			ClipboardFormat::FileDrop,
		];
		assert_eq!(formats_by_preference(&formats), [7, 6, 3, 4, 1, 0]);
		assert!(formats_by_preference(&[ClipboardFormat::Other]).is_empty());
	}

	#[test]
	fn recognizes_svg_documents() {
		let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect/></svg>";
		assert_eq!(svg_document(&format!("\n {svg}\n")), Some(svg));
		let prolog = "\u{FEFF}<?xml version=\"1.0\"?>\n<!-- Generator: x -->\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"x.dtd\" [<!ENTITY a \"b\">]>\n";
		assert!(svg_document(&format!("{prolog}{svg}")).is_some());
		assert_eq!(svg_document("<svg/>"), Some("<svg/>"));
		for text in ["<svgx/>", "<html><svg></svg></html>", "<svg", "<?xml", "text", ""] {
			assert_eq!(svg_document(text), None, "{text}");
		}
	}

	#[test]
	fn reads_svg_formats_and_svg_text() {
		let svg = b"<svg></svg>";
		let padded = [&b"\xEF\xBB\xBF"[..], svg, &[0, 0, 0]].concat();
		let expected = image("image/svg+xml", svg);
		assert_eq!(
			clipboard_image(ClipboardFormat::Registered("image/svg+xml"), &padded),
			expected
		);
		let utf16: Vec<u8> =
			"<svg></svg>\0junk".encode_utf16().flat_map(u16::to_le_bytes).collect();
		assert_eq!(clipboard_image(ClipboardFormat::UnicodeText, &utf16), expected);
		let with_bom = [&[0xFF, 0xFE][..], &utf16].concat();
		assert_eq!(
			clipboard_image(ClipboardFormat::Registered("image/svg+xml"), &with_bom),
			expected
		);
		let text: Vec<u8> = "plain text".encode_utf16().flat_map(u16::to_le_bytes).collect();
		assert_eq!(clipboard_image(ClipboardFormat::UnicodeText, &text), None);
	}

	#[test]
	fn recognizes_image_files_by_signature() {
		let png = [
			&b"\x89PNG\r\n\x1a\n"[..],
			&[0, 0, 0, 1],
			b"IHDR",
			&[7],
			&[1, 2, 3, 4],
			&[0, 0, 0, 0],
			b"IEND",
			&[5, 6, 7, 8],
		]
		.concat();
		let padded = [&png[..], &[0; 7]].concat();
		assert_eq!(
			clipboard_image(ClipboardFormat::Registered("PNG"), &padded),
			image("image/png", &png)
		);
		let jpeg = [0xFF, 0xD8, 0xFF, 0xE0];
		assert_eq!(
			clipboard_image(ClipboardFormat::Registered("JFIF"), &jpeg),
			image("image/jpeg", &jpeg)
		);
		assert_eq!(
			clipboard_image(ClipboardFormat::Registered("GIF"), b"GIF89a.."),
			image("image/gif", b"GIF89a..")
		);
		assert_eq!(clipboard_image(ClipboardFormat::Registered("PNG"), b"no image"), None);
		assert_eq!(clipboard_image(ClipboardFormat::Registered("Text"), &jpeg), None);
	}

	fn drop_files(names: &[&str], is_wide: bool) -> Vec<u8> {
		let mut bytes = [20u32, 0, 0, 0, u32::from(is_wide)]
			.iter()
			.flat_map(|value| value.to_le_bytes())
			.collect::<Vec<u8>>();
		for name in names.iter().chain(&[""]) {
			match is_wide {
				true => bytes.extend(name.encode_utf16().chain([0]).flat_map(u16::to_le_bytes)),
				false => bytes.extend(name.bytes().chain([0])),
			}
		}
		bytes
	}

	#[test]
	fn takes_the_image_files_of_copied_files() {
		let bytes = drop_files(&["C:\\images\\a.png", "C:\\notes.txt", "D:\\b.SVG"], true);
		let expected = ClipboardImage::Files(vec!["C:\\images\\a.png".into(), "D:\\b.SVG".into()]);
		assert_eq!(clipboard_image(ClipboardFormat::FileDrop, &bytes), Some(expected));
		let bytes = drop_files(&["C:\\a.jpg"], false);
		let expected = ClipboardImage::Files(vec!["C:\\a.jpg".into()]);
		assert_eq!(clipboard_image(ClipboardFormat::FileDrop, &bytes), Some(expected));
		let bytes = drop_files(&["C:\\notes.txt"], true);
		assert_eq!(clipboard_image(ClipboardFormat::FileDrop, &bytes), None);
		assert_eq!(clipboard_image(ClipboardFormat::FileDrop, &[1, 2, 3]), None);
	}

	/// A `BITMAPINFOHEADER` (or a larger header) followed by `rest`.
	fn dib(
		header_size: u32,
		(width, height): (i32, i32),
		bit_count: u16,
		compression: u32,
		colors_used: u32,
		extra_header: &[u8],
		rest: &[u8],
	) -> Vec<u8> {
		let mut bytes = Vec::new();
		bytes.extend(header_size.to_le_bytes());
		bytes.extend(width.to_le_bytes());
		bytes.extend(height.to_le_bytes());
		bytes.extend(1u16.to_le_bytes());
		bytes.extend(bit_count.to_le_bytes());
		bytes.extend(compression.to_le_bytes());
		bytes.extend([0; 12]);
		bytes.extend(colors_used.to_le_bytes());
		bytes.extend([0; 4]);
		bytes.extend(extra_header);
		bytes.resize(header_size as usize, 0);
		bytes.extend(rest);
		bytes
	}

	fn pixels(bytes: &[u8]) -> Option<Pixels> {
		match clipboard_image(ClipboardFormat::Bitmap, bytes)? {
			ClipboardImage::Pixels(pixels) => Some(pixels),
			_ => None,
		}
	}

	#[test]
	fn decodes_bottom_up_24_bit_bitmaps_with_row_padding() {
		// Two rows of two pixels, padded to four bytes, the bottom row first.
		let rows = [1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10, 11, 12, 0, 0];
		let decoded = pixels(&dib(40, (2, 2), 24, BI_RGB, 0, &[], &rows)).unwrap();
		assert_eq!((decoded.width, decoded.height), (2, 2));
		assert_eq!(decoded.bgra, [7, 8, 9, 255, 10, 11, 12, 255, 1, 2, 3, 255, 4, 5, 6, 255]);
		assert_eq!(pixels(&dib(40, (2, 2), 24, BI_RGB, 0, &[], &rows[..15])), None);
	}

	#[test]
	fn decodes_32_bit_bitmaps_with_and_without_alpha() {
		let rows = [1, 2, 3, 4, 5, 6, 7, 8];
		let opaque = pixels(&dib(40, (1, -2), 32, BI_RGB, 0, &[], &rows)).unwrap();
		assert_eq!(opaque.bgra, [1, 2, 3, 255, 5, 6, 7, 255]);

		let masks: Vec<u8> = [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000]
			.iter()
			.flat_map(|mask| mask.to_le_bytes())
			.collect();
		let with_alpha = pixels(&dib(124, (1, -2), 32, BI_BITFIELDS, 0, &masks, &rows)).unwrap();
		assert_eq!(with_alpha.bgra, rows);
		let transparent = [1, 2, 3, 0, 5, 6, 7, 0];
		let unset_alpha =
			pixels(&dib(124, (1, -2), 32, BI_BITFIELDS, 0, &masks, &transparent)).unwrap();
		assert_eq!(unset_alpha.bgra, opaque.bgra);
		// A `BITMAPINFOHEADER` followed by three masks.
		let after_header = [&masks[..12], &rows[..]].concat();
		let decoded = pixels(&dib(40, (1, -2), 32, BI_BITFIELDS, 0, &[], &after_header)).unwrap();
		assert_eq!(decoded.bgra, opaque.bgra);
	}

	#[test]
	fn decodes_16_bit_and_palette_bitmaps() {
		// 5-5-5: white and pure red.
		let decoded =
			pixels(&dib(40, (2, 1), 16, BI_RGB, 0, &[], &[0xFF, 0x7F, 0x00, 0x7C])).unwrap();
		assert_eq!(decoded.bgra, [255, 255, 255, 255, 0, 0, 255, 255]);
		// Two palette colors, eight 1-bit pixels.
		let palette = [10, 20, 30, 0, 40, 50, 60, 0];
		let rest = [&palette[..], &[0b1000_0001, 0, 0, 0]].concat();
		let decoded = pixels(&dib(40, (8, 1), 1, BI_RGB, 2, &[], &rest)).unwrap();
		assert_eq!(&decoded.bgra[..8], [40, 50, 60, 255, 10, 20, 30, 255]);
		assert_eq!(&decoded.bgra[28..], [40, 50, 60, 255]);
	}

	#[test]
	fn refuses_compressed_and_malformed_bitmaps() {
		assert_eq!(pixels(&dib(40, (1, 1), 8, 1, 0, &[], &[0; 1028])), None);
		assert_eq!(pixels(&dib(40, (0, 1), 24, BI_RGB, 0, &[], &[0; 4])), None);
		assert_eq!(pixels(&dib(40, (1, 1), 24, BI_RGB, 0, &[], &[])), None);
		assert_eq!(pixels(&dib(40, (1, 1), 7, BI_RGB, 0, &[], &[0; 4])), None);
		assert_eq!(pixels(&dib(40, (65536, 65536), 1, BI_RGB, 0, &[], &[0; 8])), None);
		assert_eq!(pixels(&[40, 0, 0]), None);
	}

	#[test]
	fn accepts_random_clipboard_data_without_panicking() {
		let mut state = 0x2545_F491_4F6C_DD1Du64;
		let mut random = || {
			state ^= state << 13;
			state ^= state >> 7;
			state ^= state << 17;
			state
		};
		let formats = [
			ClipboardFormat::FileDrop,
			ClipboardFormat::Bitmap,
			ClipboardFormat::UnicodeText,
			ClipboardFormat::Registered("PNG"),
			ClipboardFormat::Registered("image/svg+xml"),
		];
		for _ in 0..20_000 {
			let length = (random() % 200) as usize;
			let mut bytes: Vec<u8> = (0..length).map(|_| random() as u8).collect();
			if random() % 2 == 0 && bytes.len() >= 40 {
				// A plausible bitmap header with random fields.
				bytes[..4].copy_from_slice(&[40, 0, 0, 0]);
				bytes[4..8].copy_from_slice(&((random() % 8) as u32).to_le_bytes());
				bytes[14] = [1, 4, 8, 16, 24, 32][(random() % 6) as usize];
				bytes[15] = 0;
			}
			for format in formats {
				let _ = clipboard_image(format, &bytes);
			}
		}
	}

	#[test]
	fn limits_the_size_of_embedded_images() {
		let png = |_: &Pixels| Some(vec![1, 2, 3]);
		let pixels = Pixels { width: 1, height: 1, bgra: vec![0; 4] };
		let images = embedded_images(ClipboardImage::Pixels(pixels), 3, png).unwrap();
		let image = EmbeddedImage { media_type: "image/png", bytes: vec![1, 2, 3] };
		assert_eq!(images, [PastedImage { image, alt: None }]);
		let svg = EmbeddedImage { media_type: "image/svg+xml", bytes: vec![0; 4] };
		let error = embedded_images(ClipboardImage::Image(svg), 3, png).unwrap_err();
		assert!(matches!(error, PasteImageError::TooLarge { file: None, size: 4, max_size: 3 }));
		assert!(error.to_string().contains("MaxEmbeddedImageBytes"));
		let pixels = Pixels { width: 1, height: 1, bgra: vec![0; 4] };
		let error = embedded_images(ClipboardImage::Pixels(pixels), 3, |_| None).unwrap_err();
		assert!(matches!(error, PasteImageError::NotConvertible));
	}

	#[test]
	fn reads_image_files() {
		let directory = std::env::temp_dir()
			.join(format!("npp-toolbox-clipboard-image-{}", std::process::id()));
		std::fs::create_dir_all(&directory).unwrap();
		let small = directory.join("small.gif");
		let large = directory.join("large.png");
		std::fs::write(&small, b"GIF").unwrap();
		std::fs::write(&large, [0; 10]).unwrap();
		let unused = |_: &Pixels| None;

		let images = embedded_images(ClipboardImage::Files(vec![small.clone()]), 5, unused);
		assert_eq!(
			images.unwrap(),
			[PastedImage {
				image: EmbeddedImage { media_type: "image/gif", bytes: b"GIF".to_vec() },
				alt: Some("small".to_owned()),
			}]
		);
		let error = embedded_images(ClipboardImage::Files(vec![small, large.clone()]), 5, unused);
		assert!(
			matches!(error, Err(PasteImageError::TooLarge { file: Some(file), size: 10, .. }) if file == large)
		);
		let missing = ClipboardImage::Files(vec![directory.join("missing.png")]);
		assert!(matches!(
			embedded_images(missing, 5, unused),
			Err(PasteImageError::Unreadable { .. })
		));
		let _ = std::fs::remove_dir_all(&directory);
	}

	#[test]
	fn writes_img_tags() {
		let image = || EmbeddedImage { media_type: "image/png", bytes: b"foo".to_vec() };
		let pasted = PastedImage { image: image(), alt: None };
		assert_eq!(img_tag(&pasted), "<img src=\"data:image/png;base64,Zm9v\">");
		let pasted = PastedImage { image: image(), alt: Some("a \"b\" & c".to_owned()) };
		assert_eq!(
			img_tag(&pasted),
			"<img src=\"data:image/png;base64,Zm9v\" alt=\"a &quot;b&quot; &amp; c\">"
		);
	}
}
