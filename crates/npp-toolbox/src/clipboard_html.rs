//! The Windows "HTML Format" clipboard payload (CF_HTML).
//!
//! It is UTF-8 HTML preceded by an ASCII header with byte offsets of the whole document and of
//! the fragment between the `StartFragment`/`EndFragment` comments, which is what applications
//! paste. See "HTML Clipboard Format" in the Win32 documentation.

const DOCUMENT_START: &str =
	"<!DOCTYPE html>\r\n<html>\r\n<head>\r\n<meta charset=\"utf-8\">\r\n</head>\r\n<body>\r\n";
const START_FRAGMENT_COMMENT: &str = "<!--StartFragment-->";
const END_FRAGMENT_COMMENT: &str = "<!--EndFragment-->";
const DOCUMENT_END: &str = "\r\n</body>\r\n</html>\r\n";
/// Offsets are written zero padded so that the header length does not depend on them.
const OFFSET_DIGITS: usize = 10;

pub fn clipboard_html(fragment: &str) -> String {
	let start_html = header(0, 0, 0, 0).len();
	let start_fragment = start_html + DOCUMENT_START.len() + START_FRAGMENT_COMMENT.len();
	let end_fragment = start_fragment + fragment.len();
	let end_html = end_fragment + END_FRAGMENT_COMMENT.len() + DOCUMENT_END.len();

	let mut payload = header(start_html, end_html, start_fragment, end_fragment);
	payload.reserve(end_html - start_html);
	for part in
		[DOCUMENT_START, START_FRAGMENT_COMMENT, fragment, END_FRAGMENT_COMMENT, DOCUMENT_END]
	{
		payload.push_str(part);
	}
	payload
}

fn header(
	start_html: usize,
	end_html: usize,
	start_fragment: usize,
	end_fragment: usize,
) -> String {
	format!(
		"Version:0.9\r\nStartHTML:{start_html:0OFFSET_DIGITS$}\r\nEndHTML:{end_html:0OFFSET_DIGITS$}\r\nStartFragment:{start_fragment:0OFFSET_DIGITS$}\r\nEndFragment:{end_fragment:0OFFSET_DIGITS$}\r\n"
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn offset(payload: &str, name: &str) -> usize {
		let line = payload.lines().find_map(|line| line.strip_prefix(name)).expect("header line");
		line.trim_start_matches(':').parse().expect("offset")
	}

	#[test]
	fn header_offsets_point_at_document_and_fragment() {
		let fragment = "<p>Grüße 😀</p>";
		let payload = clipboard_html(fragment);
		let start_html = offset(&payload, "StartHTML");
		let end_html = offset(&payload, "EndHTML");
		let start_fragment = offset(&payload, "StartFragment");
		let end_fragment = offset(&payload, "EndFragment");

		assert!(payload[start_html..].starts_with("<!DOCTYPE html>"));
		assert_eq!(end_html, payload.len());
		assert_eq!(&payload[start_fragment..end_fragment], fragment);
		assert!(payload[..start_fragment].ends_with(START_FRAGMENT_COMMENT));
		assert!(payload[end_fragment..].starts_with(END_FRAGMENT_COMMENT));
	}
}
