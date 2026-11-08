//! The natural size of images in node shapes. Browsers load the image for it; here only `data:`
//! URIs can be read, because the renderer fetches nothing.

/// Reads the size of the image formats that [`natural_size`] does not know itself.
pub trait ImageDecoder {
	/// Width and height in pixels of the image file `bytes`.
	fn natural_size(&self, bytes: &[u8]) -> Option<(u32, u32)>;
}

/// Knows no further formats.
#[cfg(any(test, feature = "pure-rust"))]
pub struct NoImageDecoder;

#[cfg(any(test, feature = "pure-rust"))]
impl ImageDecoder for NoImageDecoder {
	fn natural_size(&self, _bytes: &[u8]) -> Option<(u32, u32)> {
		None
	}
}

/// The width and height in pixels of the image in the `data:` URI `href`: PNG, GIF, JPEG, SVG and
/// the formats of `decoder`.
pub fn natural_size(href: &str, decoder: &dyn ImageDecoder) -> Option<(f64, f64)> {
	let rest = href.trim().strip_prefix("data:")?;
	let (header, data) = rest.split_once(',')?;
	let bytes = match header.to_ascii_lowercase().ends_with(";base64") {
		true => decode_base64(data)?,
		false => percent_decode(data),
	};
	png_size(&bytes)
		.or_else(|| gif_size(&bytes))
		.or_else(|| jpeg_size(&bytes))
		.or_else(|| svg_size(&bytes))
		.or_else(|| {
			let (width, height) = decoder.natural_size(&bytes)?;
			Some((f64::from(width), f64::from(height)))
		})
		.filter(|&(width, height)| width > 0.0 && height > 0.0)
}

fn decode_base64(text: &str) -> Option<Vec<u8>> {
	let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
	let mut buffer = 0u32;
	let mut bit_count = 0;
	for byte in text.bytes() {
		let value = match byte {
			b'A'..=b'Z' => byte - b'A',
			b'a'..=b'z' => byte - b'a' + 26,
			b'0'..=b'9' => byte - b'0' + 52,
			b'+' | b'-' => 62,
			b'/' | b'_' => 63,
			b'=' => break,
			byte if byte.is_ascii_whitespace() => continue,
			_ => return None,
		};
		buffer = buffer << 6 | u32::from(value);
		bit_count += 6;
		if bit_count >= 8 {
			bit_count -= 8;
			bytes.push((buffer >> bit_count) as u8);
		}
	}
	Some(bytes)
}

fn percent_decode(text: &str) -> Vec<u8> {
	let bytes = text.as_bytes();
	let mut decoded = Vec::with_capacity(bytes.len());
	let mut index = 0;
	while index < bytes.len() {
		let hex = |offset: usize| bytes.get(index + offset).and_then(|&b| (b as char).to_digit(16));
		match (bytes[index], hex(1), hex(2)) {
			(b'%', Some(high), Some(low)) => {
				decoded.push((high * 16 + low) as u8);
				index += 3;
			}
			(byte, _, _) => {
				decoded.push(byte);
				index += 1;
			}
		}
	}
	decoded
}

fn png_size(bytes: &[u8]) -> Option<(f64, f64)> {
	if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16)? != b"IHDR" {
		return None;
	}
	let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
	let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
	Some((f64::from(width), f64::from(height)))
}

fn gif_size(bytes: &[u8]) -> Option<(f64, f64)> {
	if !bytes.starts_with(b"GIF8") {
		return None;
	}
	let width = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
	let height = u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
	Some((f64::from(width), f64::from(height)))
}

/// The size in the first start of frame segment.
fn jpeg_size(bytes: &[u8]) -> Option<(f64, f64)> {
	if !bytes.starts_with(&[0xFF, 0xD8]) {
		return None;
	}
	let mut index = 2;
	loop {
		if *bytes.get(index)? != 0xFF {
			return None;
		}
		let marker = *bytes.get(index + 1)?;
		match marker {
			// Fill bytes before a marker.
			0xFF => index += 1,
			// Markers without a length.
			0x01 | 0xD0..=0xD7 => index += 2,
			0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
				let height = u16::from_be_bytes(bytes.get(index + 5..index + 7)?.try_into().ok()?);
				let width = u16::from_be_bytes(bytes.get(index + 7..index + 9)?.try_into().ok()?);
				return Some((f64::from(width), f64::from(height)));
			}
			_ => {
				let length = u16::from_be_bytes(bytes.get(index + 2..index + 4)?.try_into().ok()?);
				index += 2 + usize::from(length);
			}
		}
	}
}

/// The `width` and `height` of the root element, or the size of its `viewBox`.
fn svg_size(bytes: &[u8]) -> Option<(f64, f64)> {
	let text = std::str::from_utf8(bytes).ok()?;
	let start = text.find("<svg")?;
	let tag = &text[start..start + text[start..].find('>')?];
	let attribute = |name: &str| -> Option<&str> {
		let mut rest = tag;
		while let Some(position) = rest.find(name) {
			let before = rest[..position].chars().next_back();
			let after = rest[position + name.len()..].trim_start();
			rest = &rest[position + name.len()..];
			if !before.is_some_and(char::is_whitespace) {
				continue;
			}
			let Some(value) = after.strip_prefix('=') else { continue };
			let value = value.trim_start();
			let quote = value.chars().next().filter(|&quote| quote == '"' || quote == '\'')?;
			let value = &value[1..];
			return Some(&value[..value.find(quote)?]);
		}
		None
	};
	// Lengths in pixels; percentages and other units are not sizes of their own.
	let length = |name: &str| {
		attribute(name)
			.map(|value| value.trim().trim_end_matches("px"))
			.and_then(|value| value.parse::<f64>().ok())
	};
	match (length("width"), length("height")) {
		(Some(width), Some(height)) => Some((width, height)),
		_ => {
			let view_box: Vec<f64> = attribute("viewBox")?
				.split(|c: char| c.is_whitespace() || c == ',')
				.filter(|part| !part.is_empty())
				.filter_map(|part| part.parse().ok())
				.collect();
			match view_box[..] {
				[_, _, width, height] => Some((width, height)),
				_ => None,
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn reads_sizes_of_data_uris() {
		let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACgAAAAUCAIAAABwJOjsAAAAJElEQVR4nO3NMQ0AAAwEofdvupVxCwk7uy3RrGKxWCwWi8WJB336HQ594lo5AAAAAElFTkSuQmCC";
		assert_eq!(natural_size(png, &NoImageDecoder), Some((40.0, 20.0)));
		let gif = "data:image/gif;base64,R0lGODlhAwAFAIAAAP///wAAACwAAAAAAwAFAAACAoQBADs=";
		assert_eq!(natural_size(gif, &NoImageDecoder), Some((3.0, 5.0)));
		let jpeg = [
			0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00,
			0x07, 0x00, 0x09,
		];
		assert_eq!(jpeg_size(&jpeg), Some((9.0, 7.0)));
		let svg =
			"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 30 10'/%3E";
		assert_eq!(natural_size(svg, &NoImageDecoder), Some((30.0, 10.0)));
		let sized =
			"data:image/svg+xml;utf8,<svg stroke-width='2' width=\"12px\" height='8'></svg>";
		assert_eq!(natural_size(sized, &NoImageDecoder), Some((12.0, 8.0)));
		assert_eq!(natural_size("https://example.com/a.png", &NoImageDecoder), None);
		assert_eq!(natural_size("data:image/png;base64,!!", &NoImageDecoder), None);
		assert_eq!(natural_size("data:image/png;base64,", &NoImageDecoder), None);
		assert_eq!(jpeg_size(&[0xFF, 0xD8, 0xFF, 0xE0, 0xFF]), None);
	}

	#[test]
	fn asks_the_decoder_for_other_formats() {
		struct Decoder;
		impl ImageDecoder for Decoder {
			fn natural_size(&self, bytes: &[u8]) -> Option<(u32, u32)> {
				(bytes == b"BM").then_some((7, 3))
			}
		}
		assert_eq!(natural_size("data:image/bmp,BM", &Decoder), Some((7.0, 3.0)));
		assert_eq!(natural_size("data:image/bmp,BM", &NoImageDecoder), None);
	}
}
