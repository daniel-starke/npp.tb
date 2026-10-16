//! GitHub emoji shortcodes such as `:smile:`.

use crate::emoji_table::SHORTCODES;

/// The emoji for a shortcode without its colons.
pub fn emoji_for_shortcode(shortcode: &str) -> Option<&'static str> {
	// Binary search over the sorted lines of the table without an index: probe the line around
	// the middle byte of the remaining range.
	let table = SHORTCODES.as_bytes();
	let (mut low, mut high) = (0, table.len());
	while low < high {
		let middle = low + (high - low) / 2;
		let line_start = table[low..middle]
			.iter()
			.rposition(|&byte| byte == b'\n')
			.map_or(low, |newline| low + newline + 1);
		let line_end = table[middle..high]
			.iter()
			.position(|&byte| byte == b'\n')
			.map_or(high, |end| middle + end);
		let line = &SHORTCODES[line_start..line_end];
		let (name, emoji) = line.split_once('\t')?;
		match name.cmp(shortcode) {
			std::cmp::Ordering::Equal => return Some(emoji),
			std::cmp::Ordering::Less => low = line_end + 1,
			std::cmp::Ordering::Greater => high = line_start,
		}
	}
	None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextPart<'a> {
	Text(&'a str),
	/// The emoji of a known `:shortcode:`.
	Emoji(&'static str),
}

/// Splits `text` into plain text and emoji shortcodes, in order.
pub fn for_each_part<'a>(text: &'a str, mut on_part: impl FnMut(TextPart<'a>)) {
	let mut written = 0;
	let mut search_from = 0;
	while let Some(offset) = text[search_from..].find(':') {
		let open = search_from + offset;
		let name_start = open + 1;
		let name_length =
			text[name_start..].bytes().take_while(|&byte| is_shortcode_byte(byte)).count();
		let close = name_start + name_length;
		let emoji = (name_length > 0 && text[close..].starts_with(':'))
			.then(|| emoji_for_shortcode(&text[name_start..close]))
			.flatten();
		match emoji {
			Some(emoji) => {
				on_part(TextPart::Text(&text[written..open]));
				on_part(TextPart::Emoji(emoji));
				written = close + 1;
				search_from = written;
			}
			// The closing colon may open the next shortcode, e.g. in `:x:smile:`.
			None => search_from = if name_length > 0 { close } else { name_start },
		}
	}
	on_part(TextPart::Text(&text[written..]));
}

fn is_shortcode_byte(byte: u8) -> bool {
	byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_+-".contains(&byte)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn replaced(text: &str) -> String {
		let mut output = String::new();
		for_each_part(text, |part| match part {
			TextPart::Text(text) => output.push_str(text),
			TextPart::Emoji(emoji) => output.push_str(&format!("[{emoji}]")),
		});
		output
	}

	#[test]
	fn finds_every_table_entry() {
		for line in SHORTCODES.lines() {
			let (name, emoji) = line.split_once('\t').expect("tab separated");
			assert_eq!(emoji_for_shortcode(name), Some(emoji), "{name}");
		}
		assert_eq!(emoji_for_shortcode("smile"), Some("😄"));
		assert_eq!(emoji_for_shortcode("+1"), Some("👍"));
		assert_eq!(emoji_for_shortcode("no_such_emoji"), None);
		assert_eq!(emoji_for_shortcode(""), None);
	}

	#[test]
	fn table_is_sorted() {
		let names: Vec<&str> =
			SHORTCODES.lines().filter_map(|line| line.split('\t').next()).collect();
		assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
	}

	#[test]
	fn replaces_known_shortcodes_only() {
		assert_eq!(replaced("Hi :smile: and :tada:!"), "Hi [😄] and [🎉]!");
		assert_eq!(replaced(":unknown: :smile"), ":unknown: :smile");
		assert_eq!(replaced("10:30:smile:"), "10:30[😄]");
		assert_eq!(replaced(":Smile: :: :"), ":Smile: :: :");
		assert_eq!(replaced("a:+1::-1:b"), "a[👍][👎]b");
	}
}
