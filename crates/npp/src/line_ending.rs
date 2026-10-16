//! Document line ending handling independent of Scintilla.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
	CrLf,
	Cr,
	Lf,
}

impl LineEnding {
	pub fn as_str(self) -> &'static str {
		match self {
			LineEnding::CrLf => "\r\n",
			LineEnding::Cr => "\r",
			LineEnding::Lf => "\n",
		}
	}
}

/// Splits on `\r\n`, `\r` and `\n`. Unlike [`str::lines`] a lone `\r` also ends a line and a
/// trailing line ending yields a final empty line, matching Scintilla's line model.
pub fn split_lines(text: &str) -> Vec<&str> {
	let mut lines = Vec::new();
	let mut line_start = 0;
	let bytes = text.as_bytes();
	let mut index = 0;
	while index < bytes.len() {
		match bytes[index] {
			b'\r' => {
				lines.push(&text[line_start..index]);
				index += if bytes.get(index + 1) == Some(&b'\n') { 2 } else { 1 };
				line_start = index;
			}
			b'\n' => {
				lines.push(&text[line_start..index]);
				index += 1;
				line_start = index;
			}
			_ => index += 1,
		}
	}
	lines.push(&text[line_start..]);
	lines
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn splits_mixed_line_endings_like_scintilla() {
		assert_eq!(split_lines("a\r\nb\rc\nd"), ["a", "b", "c", "d"]);
		assert_eq!(split_lines("a\n"), ["a", ""]);
		assert_eq!(split_lines(""), [""]);
	}
}
