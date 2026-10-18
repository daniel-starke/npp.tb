//! Turns local image references into Base64 `data:` URIs. Internet resources are never fetched.

use std::path::{Path, PathBuf};

use crate::base64;

pub struct ImageEmbedding<'a> {
	/// Directory of the Markdown document. Relative paths cannot be resolved without one.
	pub base_directory: Option<&'a Path>,
	/// Larger images stay linked.
	pub max_image_bytes: u64,
}

/// The content of a local image file.
#[derive(Debug, PartialEq, Eq)]
pub struct EmbeddedImage {
	pub media_type: &'static str,
	pub bytes: Vec<u8>,
}

impl EmbeddedImage {
	pub fn data_uri(&self) -> String {
		let mut uri = String::with_capacity(
			"data:;base64,".len()
				+ self.media_type.len()
				+ base64::encoded_length(self.bytes.len()),
		);
		uri.push_str("data:");
		uri.push_str(self.media_type);
		uri.push_str(";base64,");
		base64::encode_into(&self.bytes, &mut uri);
		uri
	}
}

impl ImageEmbedding<'_> {
	/// The `data:` URI for `source`, or `None` if it is not a readable local image within the
	/// size limit. Then the caller keeps `source` as written.
	pub fn data_uri(&self, source: &str) -> Option<String> {
		self.load(source).map(|image| image.data_uri())
	}

	/// The local image `source` refers to, or `None` if it is not a readable local image within
	/// the size limit.
	pub fn load(&self, source: &str) -> Option<EmbeddedImage> {
		let path = self.local_path(source)?;
		let media_type = media_type_of(&path)?;
		if std::fs::metadata(&path).ok()?.len() > self.max_image_bytes {
			return None;
		}
		let bytes = std::fs::read(&path).ok()?;
		Some(EmbeddedImage { media_type, bytes })
	}

	fn local_path(&self, source: &str) -> Option<PathBuf> {
		local_file_path(source, self.base_directory)
	}
}

/// The local file a link or image `source` names, relative ones resolved against `base_directory`.
/// `None` for other schemes, network locations and relative paths without a base directory.
pub fn local_file_path(source: &str, base_directory: Option<&Path>) -> Option<PathBuf> {
	let source = source.trim();
	// The query and fragment are not part of the file name.
	let source = &source[..source.find(['?', '#']).unwrap_or(source.len())];
	if source.is_empty() || source.starts_with("//") {
		return None;
	}
	let path = match uri_scheme(source) {
		Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
			let path = &source[scheme.len() + 1..];
			let path = match path.strip_prefix("//") {
				Some(authority_and_path) => {
					let authority_end =
						authority_and_path.find('/').unwrap_or(authority_and_path.len());
					let authority = &authority_and_path[..authority_end];
					// Only local files. A host name would be a network resource.
					if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
						return None;
					}
					&authority_and_path[authority_end..]
				}
				None => path,
			};
			// `/C:/dir/file.png` names `C:/dir/file.png`.
			let path = match path.as_bytes() {
				[b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => &path[1..],
				_ => path,
			};
			PathBuf::from(percent_decode(path)?)
		}
		// A Windows drive letter looks like a one letter scheme.
		Some(scheme) if scheme.len() > 1 => return None,
		_ => PathBuf::from(percent_decode(source)?),
	};
	if path.is_absolute() || is_windows_absolute(&path) {
		Some(path)
	} else {
		Some(base_directory?.join(path))
	}
}

/// The scheme of an absolute URI (`https`, `data`, `file`, ...), if `reference` has one.
pub fn uri_scheme(reference: &str) -> Option<&str> {
	let colon = reference.find(':')?;
	let scheme = &reference[..colon];
	let mut characters = scheme.chars();
	let starts_with_letter = characters.next().is_some_and(|first| first.is_ascii_alphabetic());
	let is_scheme = starts_with_letter
		&& characters
			.all(|character| character.is_ascii_alphanumeric() || "+-.".contains(character));
	is_scheme.then_some(scheme)
}

/// Drive letter or UNC paths, which `Path::is_absolute` only knows on Windows hosts.
fn is_windows_absolute(path: &Path) -> bool {
	let text = path.to_string_lossy();
	let bytes = text.as_bytes();
	matches!(bytes, [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic())
		|| text.starts_with("\\\\")
}

/// Decodes `%XX` escapes. `None` for malformed escapes or non UTF-8 results.
fn percent_decode(text: &str) -> Option<String> {
	if !text.contains('%') {
		return Some(text.to_owned());
	}
	let mut bytes = Vec::with_capacity(text.len());
	let mut rest = text.as_bytes();
	while let Some((&byte, tail)) = rest.split_first() {
		if byte == b'%' {
			let hex = tail.get(..2)?;
			bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
			rest = &tail[2..];
		} else {
			bytes.push(byte);
			rest = tail;
		}
	}
	String::from_utf8(bytes).ok()
}

/// Media type by file extension, only for formats browsers and Office display.
fn media_type_of(path: &Path) -> Option<&'static str> {
	let extension = path.extension()?.to_str()?.to_ascii_lowercase();
	Some(match extension.as_str() {
		"png" => "image/png",
		"jpg" | "jpeg" | "jfif" => "image/jpeg",
		"gif" => "image/gif",
		"webp" => "image/webp",
		"svg" => "image/svg+xml",
		"bmp" => "image/bmp",
		"ico" => "image/x-icon",
		"avif" => "image/avif",
		_ => return None,
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	struct TemporaryDirectory(PathBuf);

	impl TemporaryDirectory {
		fn new(name: &str) -> Self {
			let path = std::env::temp_dir()
				.join(format!("npp-toolbox-image-embedding-{name}-{}", std::process::id()));
			std::fs::create_dir_all(&path).expect("temporary directory");
			Self(path)
		}
	}

	impl Drop for TemporaryDirectory {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}

	fn embedding(directory: &Path) -> ImageEmbedding<'_> {
		ImageEmbedding { base_directory: Some(directory), max_image_bytes: 100 }
	}

	#[test]
	fn embeds_relative_and_absolute_local_images() {
		let directory = TemporaryDirectory::new("local");
		std::fs::create_dir_all(directory.0.join("my images")).expect("subdirectory");
		std::fs::write(directory.0.join("my images/dot.png"), b"foo").expect("image");
		let embedding = embedding(&directory.0);

		let expected = Some("data:image/png;base64,Zm9v".to_owned());
		assert_eq!(embedding.data_uri("my images/dot.png"), expected);
		assert_eq!(embedding.data_uri("./my%20images/dot.png?raw=true#top"), expected);
		let absolute = directory.0.join("my images/dot.png");
		assert_eq!(embedding.data_uri(absolute.to_str().expect("UTF-8 path")), expected);
		// `file:///C:/dir/...` on Windows, `file:///tmp/...` elsewhere.
		let path = absolute.to_str().expect("UTF-8 path").replace('\\', "/");
		let file_uri = format!("file:///{}", path.trim_start_matches('/'));
		assert_eq!(embedding.data_uri(&file_uri.replace(' ', "%20")), expected);
	}

	#[test]
	fn keeps_internet_unknown_missing_and_large_images() {
		let directory = TemporaryDirectory::new("rejected");
		std::fs::write(directory.0.join("large.png"), [0u8; 101]).expect("image");
		std::fs::write(directory.0.join("notes.txt"), b"text").expect("text");
		let embedding = embedding(&directory.0);

		for source in [
			"https://example.com/a.png",
			"http://example.com/a.png",
			"//example.com/a.png",
			"data:image/png;base64,AAAA",
			"file://server/share/a.png",
			"large.png",
			"notes.txt",
			"missing.png",
			"bad%zzescape.png",
			"",
		] {
			assert_eq!(embedding.data_uri(source), None, "{source}");
		}
	}

	#[test]
	fn needs_a_base_directory_for_relative_paths() {
		let embedding = ImageEmbedding { base_directory: None, max_image_bytes: 100 };
		assert_eq!(embedding.data_uri("image.png"), None);
	}

	#[test]
	fn recognizes_uri_schemes_and_windows_paths() {
		assert_eq!(uri_scheme("https://a"), Some("https"));
		assert_eq!(uri_scheme("C:/a.png"), Some("C"));
		assert_eq!(uri_scheme("a/b:c"), None);
		assert!(is_windows_absolute(Path::new("C:\\images\\a.png")));
		assert!(is_windows_absolute(Path::new("\\\\server\\share\\a.png")));
		assert!(!is_windows_absolute(Path::new("images\\a.png")));
	}
}
