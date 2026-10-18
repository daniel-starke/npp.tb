//! GFM extended autolinks: `www.` domains, `http://`, `https://` and `ftp://` URLs and e-mail
//! addresses in plain text. The rules follow cmark-gfm's `autolink.c`, the reference of the GFM
//! specification.

use std::ops::Range;

pub struct Autolink {
	/// Byte range of the link text in the searched text.
	pub range: Range<usize>,
	pub href: String,
}

/// All autolinks in `text`, in order and without overlaps.
pub fn find_autolinks(text: &str) -> Vec<Autolink> {
	let bytes = text.as_bytes();
	let mut links = Vec::new();
	let mut position = 0;
	while position < bytes.len() {
		let found = match bytes[position] {
			b'w' | b'W' => www_link(text, position),
			b':' => url_link(text, position),
			b'@' => {
				email_link(text, position, links.last().map_or(0, |link: &Autolink| link.range.end))
			}
			_ => None,
		};
		match found {
			Some(link) => {
				position = link.range.end;
				links.push(link);
			}
			None => position += 1,
		}
	}
	links
}

/// `www.example.com/path` at the start, after whitespace or after `*`, `_`, `~` or `(`.
fn www_link(text: &str, start: usize) -> Option<Autolink> {
	let bytes = text.as_bytes();
	if !bytes[start..].starts_with(b"www.") {
		return None;
	}
	if let Some(&previous) = start.checked_sub(1).map(|index| &bytes[index])
		&& !previous.is_ascii_whitespace()
		&& !b"*_~(".contains(&previous)
	{
		return None;
	}
	let domain_length = domain_length(&bytes[start..], false)?;
	let end = trim_link_end(bytes, start, extend_to_path_end(bytes, start + domain_length))?;
	Some(Autolink { range: start..end, href: format!("http://{}", &text[start..end]) })
}

/// `scheme://domain/path` with `colon` at the `:` after the scheme.
fn url_link(text: &str, colon: usize) -> Option<Autolink> {
	let bytes = text.as_bytes();
	let scheme_start =
		colon - bytes[..colon].iter().rev().take_while(|b| b.is_ascii_alphabetic()).count();
	let scheme = &text[scheme_start..colon];
	if !["http", "https", "ftp"].iter().any(|known| scheme.eq_ignore_ascii_case(known)) {
		return None;
	}
	let domain_start = colon + "://".len();
	if !bytes[colon..].starts_with(b"://") {
		return None;
	}
	let domain_length = domain_length(&bytes[domain_start..], true)?;
	let end = trim_link_end(
		bytes,
		scheme_start,
		extend_to_path_end(bytes, domain_start + domain_length),
	)?;
	Some(Autolink { range: scheme_start..end, href: text[scheme_start..end].to_owned() })
}

/// `user.name+tag@example.com` with `at` at the `@`, also after a `mailto:` prefix. The local
/// part may not reach back before `earliest_start`, the end of the previous link.
fn email_link(text: &str, at: usize, earliest_start: usize) -> Option<Autolink> {
	let bytes = text.as_bytes();
	let mut start = at;
	let mut has_mailto_prefix = false;
	while start > earliest_start {
		let previous = bytes[start - 1];
		if previous.is_ascii_alphanumeric() || b".+-_".contains(&previous) {
			start -= 1;
			continue;
		}
		if previous == b':' && bytes[earliest_start..start].ends_with(b"mailto:") {
			let prefix_start = start - "mailto:".len();
			let is_word_start =
				prefix_start == 0 || !bytes[prefix_start - 1].is_ascii_alphanumeric();
			if is_word_start {
				start = prefix_start;
				has_mailto_prefix = true;
			}
		}
		break;
	}
	let local_part_start = if has_mailto_prefix { start + "mailto:".len() } else { start };
	if local_part_start == at {
		return None;
	}

	let mut end = at + 1;
	let mut periods = 0;
	while end < bytes.len() {
		let byte = bytes[end];
		if byte == b'.' && bytes.get(end + 1).is_some_and(u8::is_ascii_alphanumeric) {
			periods += 1;
		} else if !byte.is_ascii_alphanumeric() && byte != b'-' && byte != b'_' {
			break;
		}
		end += 1;
	}
	let last = bytes[end - 1];
	if end - at < 2 || periods == 0 || !(last.is_ascii_alphabetic() || last == b'.') {
		return None;
	}
	let end = trim_link_end(bytes, start, end)?;
	let address = &text[start..end];
	let href = if has_mailto_prefix { address.to_owned() } else { format!("mailto:{address}") };
	Some(Autolink { range: start..end, href })
}

/// Length of a valid domain at the start of `bytes`: segments of alphanumeric characters, `_`
/// and `-` separated by periods, without underscores in the last two segments. Unless
/// `allows_single_segment`, at least one period is required.
fn domain_length(bytes: &[u8], allows_single_segment: bool) -> Option<usize> {
	let mut length = 0;
	let mut periods = 0;
	let (mut underscores_in_previous_segment, mut underscores_in_segment) = (0, 0);
	while let Some(&byte) = bytes.get(length) {
		if byte == b'\\' && length + 2 < bytes.len() {
			length += 2;
			continue;
		}
		match byte {
			b'_' => underscores_in_segment += 1,
			b'.' => {
				underscores_in_previous_segment = underscores_in_segment;
				underscores_in_segment = 0;
				periods += 1;
			}
			b'-' => {}
			_ if is_host_byte(byte) => {}
			_ => break,
		}
		length += 1;
	}
	// A trailing period ends the sentence, not the domain.
	while length > 0 && bytes[length - 1] == b'.' {
		length -= 1;
		periods -= 1;
	}
	let is_valid = length > 0
		&& underscores_in_previous_segment == 0
		&& underscores_in_segment == 0
		&& (allows_single_segment || periods > 0);
	is_valid.then_some(length)
}

/// Letters, digits and any non-ASCII byte (part of a Unicode character).
fn is_host_byte(byte: u8) -> bool {
	!byte.is_ascii() || byte.is_ascii_alphanumeric()
}

/// The path extends to the next whitespace or `<`.
fn extend_to_path_end(bytes: &[u8], from: usize) -> usize {
	from + bytes[from..]
		.iter()
		.take_while(|&&byte| !byte.is_ascii_whitespace() && byte != b'<')
		.count()
}

/// Removes trailing punctuation, unbalanced closing parentheses and entity references from
/// the link `start..end`. `None` if nothing is left.
fn trim_link_end(bytes: &[u8], start: usize, mut end: usize) -> Option<usize> {
	while end > start {
		match bytes[end - 1] {
			b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~' | b'\'' | b'"' => end -= 1,
			b';' => {
				let letters = bytes[start..end - 1]
					.iter()
					.rev()
					.take_while(|b| b.is_ascii_alphabetic())
					.count();
				let ampersand = end - 1 - letters;
				end = if letters > 0 && ampersand > start && bytes[ampersand - 1] == b'&' {
					ampersand - 1
				} else {
					end - 1
				};
			}
			b')' => {
				let link = &bytes[start..end];
				let opening = link.iter().filter(|&&byte| byte == b'(').count();
				let closing = link.iter().filter(|&&byte| byte == b')').count();
				if closing <= opening {
					break;
				}
				end -= 1;
			}
			_ => break,
		}
	}
	(end > start).then_some(end)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn links(text: &str) -> Vec<(&str, String)> {
		find_autolinks(text).into_iter().map(|link| (&text[link.range], link.href)).collect()
	}

	fn link(text: &str, href: &str) -> (String, String) {
		(text.to_owned(), href.to_owned())
	}

	fn owned(found: Vec<(&str, String)>) -> Vec<(String, String)> {
		found.into_iter().map(|(text, href)| (text.to_owned(), href)).collect()
	}

	#[test]
	fn links_www_domains_at_word_starts() {
		assert_eq!(
			owned(links("Visit www.commonmark.org/help for more.")),
			[link("www.commonmark.org/help", "http://www.commonmark.org/help")]
		);
		assert_eq!(
			owned(links("(www.google.com/search?q=Markup+(business))")),
			[link(
				"www.google.com/search?q=Markup+(business)",
				"http://www.google.com/search?q=Markup+(business)"
			)]
		);
		assert!(links("xwww.example.com").is_empty());
		assert!(links("www.").is_empty());
	}

	#[test]
	fn applies_gfm_trailing_punctuation_rules() {
		assert_eq!(
			links("www.google.com/search?q=(business))+ok")[0].0,
			"www.google.com/search?q=(business))+ok"
		);
		assert_eq!(
			links("www.google.com/search?q=Markup+(business)))")[0].0,
			"www.google.com/search?q=Markup+(business)"
		);
		assert_eq!(
			links("www.google.com/search?q=commonmark&hl=en")[0].0,
			"www.google.com/search?q=commonmark&hl=en"
		);
		assert_eq!(
			links("www.google.com/search?q=commonmark&hl;")[0].0,
			"www.google.com/search?q=commonmark"
		);
		assert_eq!(links("www.commonmark.org/he<lp")[0].0, "www.commonmark.org/he");
		assert_eq!(links("Visit www.commonmark.org.")[0].0, "www.commonmark.org");
	}

	#[test]
	fn rejects_underscores_in_the_last_two_domain_segments() {
		assert!(links("www.xxx.yyy._zzz").is_empty());
		assert!(links("www.xxx._yyy.zzz").is_empty());
		assert_eq!(links("www._xxx.yyy.zzz").len(), 1);
	}

	#[test]
	fn links_urls_with_known_schemes() {
		assert_eq!(
			owned(links("See http://commonmark.org, https://x.org/a_b_ and ftp://localhost.")),
			[
				link("http://commonmark.org", "http://commonmark.org"),
				link("https://x.org/a_b", "https://x.org/a_b"),
				link("ftp://localhost", "ftp://localhost"),
			]
		);
		assert!(links("mailto://example.com xhttp:// file://x").is_empty());
	}

	#[test]
	fn links_email_addresses() {
		assert_eq!(
			owned(links("foo@bar.baz and hello+xyz@mail.example, mailto:a.b-c_d@a.b.")),
			[
				link("foo@bar.baz", "mailto:foo@bar.baz"),
				link("hello+xyz@mail.example", "mailto:hello+xyz@mail.example"),
				link("mailto:a.b-c_d@a.b", "mailto:a.b-c_d@a.b"),
			]
		);
		assert_eq!(owned(links("xmailto:a@b.c")), [link("a@b.c", "mailto:a@b.c")]);
		for text in ["a.b-c_d@a.b-", "a.b-c_d@a.b_", "@a.b", "a@b", "a@b.c1"] {
			assert!(links(text).is_empty(), "{text}");
		}
	}

	#[test]
	fn handles_non_ascii_text() {
		assert_eq!(links("Grüße www.bücher.de/ä ok")[0].0, "www.bücher.de/ä");
		assert!(links("ä:ö@ü").is_empty());
	}
}
