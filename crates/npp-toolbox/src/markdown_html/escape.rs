//! HTML escaping and URL checks.

pub fn push_escaped_text(output: &mut String, text: &str) {
	push_escaped(output, text, |character| match character {
		'&' => Some("&amp;"),
		'<' => Some("&lt;"),
		'>' => Some("&gt;"),
		_ => None,
	});
}

pub fn push_escaped_attribute(output: &mut String, value: &str) {
	push_escaped(output, value, |character| match character {
		'&' => Some("&amp;"),
		'<' => Some("&lt;"),
		'>' => Some("&gt;"),
		'"' => Some("&quot;"),
		'\'' => Some("&#39;"),
		_ => None,
	});
}

fn push_escaped(
	output: &mut String,
	text: &str,
	replacement: impl Fn(char) -> Option<&'static str>,
) {
	let mut written = 0;
	for (index, character) in text.char_indices() {
		if let Some(replacement) = replacement(character) {
			output.push_str(&text[written..index]);
			output.push_str(replacement);
			written = index + character.len_utf8();
		}
	}
	output.push_str(&text[written..]);
}

/// Whether a link or image URL may be written. Like GitHub, scripts and other active content
/// are dropped. Relative URLs and the usual document schemes are kept.
pub fn is_safe_url(url: &str, is_image: bool) -> bool {
	let url = url.trim_start();
	let Some(colon) = url.find(':') else { return true };
	let scheme = &url[..colon];
	if scheme.contains(['/', '?', '#']) {
		return true;
	}
	let scheme = scheme.to_ascii_lowercase();
	match scheme.as_str() {
		"http" | "https" | "mailto" | "ftp" | "file" | "tel" | "xmpp" | "irc" | "ircs" => true,
		"data" => {
			is_image && url[colon + 1..].trim_start().to_ascii_lowercase().starts_with("image/")
		}
		// A Windows drive letter such as `C:\image.png`.
		_ => scheme.len() == 1 && scheme.chars().all(|letter| letter.is_ascii_alphabetic()),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn escapes_markup_characters() {
		let mut output = String::new();
		push_escaped_text(&mut output, "a < b && \"c\" > 'd' ä");
		assert_eq!(output, "a &lt; b &amp;&amp; \"c\" &gt; 'd' ä");
		output.clear();
		push_escaped_attribute(&mut output, "a<\"b\"&'c'");
		assert_eq!(output, "a&lt;&quot;b&quot;&amp;&#39;c&#39;");
	}

	#[test]
	fn rejects_active_url_schemes() {
		for url in
			["https://x", "#top", "a/b:c", "mailto:a@b.c", "image.png", "C:\\a.png", "?q=a:b"]
		{
			assert!(is_safe_url(url, false), "{url}");
		}
		for url in ["javascript:alert(1)", " JavaScript:x", "vbscript:x", "data:text/html,x"] {
			assert!(!is_safe_url(url, true), "{url}");
		}
		assert!(is_safe_url("data:image/png;base64,AAAA", true));
		assert!(!is_safe_url("data:image/png;base64,AAAA", false));
	}
}
