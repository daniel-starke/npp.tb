//! The operator dictionary of MathML Core as Blink keeps it (`mathml_operator_dictionary.cc`,
//! `mathml_operator_element.cc`), the operators that stretch along the inline axis
//! (`character.cc`) and the `text-transform: math-auto` of identifiers.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OperatorProperties {
	/// Spaces in math units, 1/18 em.
	pub leading_space: u8,
	pub trailing_space: u8,
	pub is_stretchy: bool,
	pub is_symmetric: bool,
	pub is_large_op: bool,
	pub has_movable_limits: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
	Infix,
	Prefix,
	Postfix,
}

const fn properties(
	leading_space: u8,
	trailing_space: u8,
	(is_stretchy, is_symmetric): (bool, bool),
	(is_large_op, has_movable_limits): (bool, bool),
) -> OperatorProperties {
	OperatorProperties {
		leading_space,
		trailing_space,
		is_stretchy,
		is_symmetric,
		is_large_op,
		has_movable_limits,
	}
}

const NO_FLAGS: (bool, bool) = (false, false);
const DEFAULT: OperatorProperties = properties(5, 5, NO_FLAGS, NO_FLAGS);
const CATEGORY_A: OperatorProperties = properties(5, 5, (true, false), NO_FLAGS);
const CATEGORY_B: OperatorProperties = properties(4, 4, NO_FLAGS, NO_FLAGS);
const CATEGORY_C: OperatorProperties = properties(3, 3, NO_FLAGS, NO_FLAGS);
const CATEGORY_D_E_K: OperatorProperties = properties(0, 0, NO_FLAGS, NO_FLAGS);
const CATEGORY_F_G: OperatorProperties = properties(0, 0, (true, true), NO_FLAGS);
const CATEGORY_H: OperatorProperties = properties(3, 3, (false, true), (true, false));
const CATEGORY_I: OperatorProperties = properties(0, 0, (true, false), NO_FLAGS);
const CATEGORY_J: OperatorProperties = properties(3, 3, (false, true), (true, true));
const CATEGORY_L: OperatorProperties = properties(3, 0, NO_FLAGS, NO_FLAGS);
const CATEGORY_M: OperatorProperties = properties(0, 3, NO_FLAGS, NO_FLAGS);

/// Operators of two ASCII characters, mapped to keys from `COMBINING_MINUS_SIGN_BELOW` on.
const TWO_ASCII_CHARACTERS: [&str; 18] = [
	"!!", "!=", "&&", "**", "*=", "++", "+=", "--", "-=", "->", "//", "/=", ":=", "<=", "<>", "==",
	">=", "||",
];
const COMBINING_MINUS_SIGN_BELOW: u32 = 0x320;
const GREEK_CAPITAL_REVERSED_DOTTED_LUNATE_SIGMA_SYMBOL: u32 = 0x3FF;

/// Blink's `kCompactDictionary`: the key in the lower 12 bits with the form above it (infix 0,
/// prefix 1, postfix 2), the category in the upper 4 bits, and how many keys follow the first.
const COMPACT_DICTIONARY: [(u16, u8); 236] = [
	(0x8025, 0),
	(0x802A, 0),
	(0x402B, 0),
	(0x402D, 0),
	(0x802E, 0),
	(0x402F, 0),
	(0x803F, 1),
	(0xC05C, 0),
	(0x805E, 0),
	(0xC05F, 0),
	(0x40B1, 0),
	(0x80B7, 0),
	(0x80D7, 0),
	(0x40F7, 0),
	(0x4322, 0),
	(0x8323, 0),
	(0x832E, 0),
	(0x8422, 0),
	(0x8443, 0),
	(0x4444, 0),
	(0xC461, 3),
	(0x0590, 5),
	(0x059A, 15),
	(0x05AA, 4),
	(0x05B0, 5),
	(0x05B9, 0),
	(0x05BC, 15),
	(0x05CC, 9),
	(0x05DA, 15),
	(0x05EA, 6),
	(0x05F3, 12),
	(0xC606, 0),
	(0x4612, 4),
	(0x8617, 2),
	(0x4627, 3),
	(0x4636, 0),
	(0x4638, 0),
	(0x8640, 0),
	(0x468C, 2),
	(0x4693, 3),
	(0x8697, 0),
	(0x4698, 0),
	(0x8699, 2),
	(0x469D, 2),
	(0x86A0, 1),
	(0x86BA, 0),
	(0x46BB, 2),
	(0x86C4, 3),
	(0x86C9, 3),
	(0x46CE, 1),
	(0x46D2, 1),
	(0x8705, 1),
	(0x0B94, 0),
	(0x4B95, 2),
	(0x0B99, 0),
	(0x0B9B, 6),
	(0x0BA5, 1),
	(0x0BA8, 7),
	(0x0BB1, 0),
	(0x0BB3, 0),
	(0x0BB5, 0),
	(0x0BB8, 0),
	(0x0BBA, 4),
	(0x8BCB, 0),
	(0x8BCD, 0),
	(0x0BF0, 1),
	(0x0BF4, 11),
	(0x0D00, 15),
	(0x0D10, 15),
	(0x0D20, 0),
	(0x0D34, 3),
	(0x0D42, 15),
	(0x0D52, 15),
	(0x0D62, 15),
	(0x0D72, 3),
	(0x0D7C, 3),
	(0x4DB8, 0),
	(0x4DBC, 0),
	(0x4DC4, 1),
	(0x8DC6, 2),
	(0x8DD4, 3),
	(0x8DE2, 0),
	(0x4DF5, 6),
	(0x8E1D, 1),
	(0x4E1F, 15),
	(0x8E2F, 8),
	(0x4E38, 2),
	(0x8E3B, 2),
	(0x4E3E, 0),
	(0x8E3F, 0),
	(0x4E40, 15),
	(0x8E50, 0),
	(0x4E51, 15),
	(0x4E61, 2),
	(0x8E64, 1),
	(0x4EDB, 0),
	(0x8EDC, 1),
	(0x4EF6, 0),
	(0x4EFB, 0),
	(0x4EFD, 0),
	(0x8EFE, 0),
	(0x0F04, 3),
	(0x0F0C, 5),
	(0x0F30, 14),
	(0x0F40, 12),
	(0x0F60, 5),
	(0x0F6A, 3),
	(0x0F70, 3),
	(0x0F7A, 3),
	(0x0F80, 7),
	(0x0F95, 0),
	(0x0FA0, 15),
	(0x0FB8, 0),
	(0x1021, 0),
	(0x5028, 0),
	(0x102B, 0),
	(0x102D, 0),
	(0x505B, 0),
	(0x507B, 1),
	(0x10AC, 0),
	(0x10B1, 0),
	(0x1331, 0),
	(0x5416, 0),
	(0x1418, 0),
	(0x141C, 0),
	(0x1600, 1),
	(0x1603, 1),
	(0x1607, 0),
	(0xD60F, 2),
	(0x1612, 1),
	(0x161F, 3),
	(0x962B, 8),
	(0x1634, 1),
	(0x163C, 0),
	(0x16BE, 1),
	(0xD6C0, 3),
	(0x5708, 0),
	(0x570A, 0),
	(0x1710, 0),
	(0x1719, 0),
	(0x5729, 0),
	(0x5B72, 0),
	(0x1B95, 1),
	(0x1BC0, 0),
	(0x5BE6, 0),
	(0x5BE8, 0),
	(0x5BEA, 0),
	(0x5BEC, 0),
	(0x5BEE, 0),
	(0x5D80, 0),
	(0x5D83, 0),
	(0x5D85, 0),
	(0x5D87, 0),
	(0x5D89, 0),
	(0x5D8B, 0),
	(0x5D8D, 0),
	(0x5D8F, 0),
	(0x5D91, 0),
	(0x5D93, 0),
	(0x5D95, 0),
	(0x5D97, 0),
	(0x5D99, 0),
	(0x1D9B, 15),
	(0x1DAB, 4),
	(0x5DD8, 0),
	(0x5DDA, 0),
	(0x5DFC, 0),
	(0xDE00, 10),
	(0x9E0B, 15),
	(0x9E1B, 1),
	(0xDE1D, 1),
	(0x1EEC, 1),
	(0xDEFC, 0),
	(0xDEFF, 0),
	(0x2021, 1),
	(0x2025, 2),
	(0x6029, 0),
	(0x605D, 0),
	(0xA05E, 1),
	(0x2060, 0),
	(0x607C, 1),
	(0xA07E, 0),
	(0x20A8, 0),
	(0xA0AF, 0),
	(0x20B0, 0),
	(0x20B2, 2),
	(0x20B8, 1),
	(0xA2C6, 1),
	(0xA2C9, 0),
	(0x22CA, 1),
	(0xA2CD, 0),
	(0x22D8, 2),
	(0xA2DC, 0),
	(0x22DD, 0),
	(0xA2F7, 0),
	(0xA302, 0),
	(0x2311, 0),
	(0x2320, 0),
	(0x2325, 0),
	(0x2327, 0),
	(0x2331, 0),
	(0x6416, 0),
	(0x2419, 2),
	(0x241D, 2),
	(0x2432, 5),
	(0xA43E, 0),
	(0x2457, 0),
	(0x24DB, 1),
	(0x6709, 0),
	(0x670B, 0),
	(0xA722, 1),
	(0x672A, 0),
	(0xA7B4, 1),
	(0x27CD, 0),
	(0xA7DC, 5),
	(0x6B73, 0),
	(0x6BE7, 0),
	(0x6BE9, 0),
	(0x6BEB, 0),
	(0x6BED, 0),
	(0x6BEF, 0),
	(0x6D80, 0),
	(0x6D84, 0),
	(0x6D86, 0),
	(0x6D88, 0),
	(0x6D8A, 0),
	(0x6D8C, 0),
	(0x6D8E, 0),
	(0x6D90, 0),
	(0x6D92, 0),
	(0x6D94, 0),
	(0x6D96, 0),
	(0x6D98, 1),
	(0x6DD9, 0),
	(0x6DDB, 0),
	(0x6DFD, 0),
];

/// Blink's `kStretchyOperatorWithInlineAxis`.
const INLINE_AXIS_OPERATORS: [u16; 108] = [
	0x003D, 0x005E, 0x005F, 0x007E, 0x00AF, 0x02C6, 0x02C7, 0x02C9, 0x02CD, 0x02DC, 0x02F7, 0x0302,
	0x0332, 0x203E, 0x20D0, 0x20D1, 0x20D6, 0x20D7, 0x20E1, 0x2190, 0x2192, 0x2194, 0x2198, 0x2199,
	0x219C, 0x219D, 0x219E, 0x21A0, 0x21A2, 0x21A3, 0x21A4, 0x21A6, 0x21A9, 0x21AA, 0x21AB, 0x21AC,
	0x21AD, 0x21B4, 0x21B9, 0x21BC, 0x21BD, 0x21C0, 0x21C1, 0x21C4, 0x21C6, 0x21C7, 0x21C9, 0x21CB,
	0x21CC, 0x21D0, 0x21D2, 0x21D4, 0x21DA, 0x21DB, 0x21DC, 0x21DD, 0x21E0, 0x21E2, 0x21E4, 0x21E5,
	0x21E6, 0x21E8, 0x21F0, 0x21F6, 0x21FD, 0x21FE, 0x21FF, 0x23B4, 0x23B5, 0x23DC, 0x23DD, 0x23DE,
	0x23DF, 0x23E0, 0x23E1, 0x2500, 0x27F5, 0x27F6, 0x27F7, 0x27F8, 0x27F9, 0x27FA, 0x27FB, 0x27FC,
	0x27FD, 0x27FE, 0x27FF, 0x290C, 0x290D, 0x290E, 0x290F, 0x2910, 0x294E, 0x2950, 0x2952, 0x2953,
	0x2956, 0x2957, 0x295A, 0x295B, 0x295E, 0x295F, 0x2B45, 0x2B46, 0xFE35, 0xFE36, 0xFE37, 0xFE38,
];

/// Whether `character` stretches along the block axis (`IsVerticalMathCharacter`).
pub fn is_vertical(character: char) -> bool {
	!u16::try_from(u32::from(character))
		.is_ok_and(|code| INLINE_AXIS_OPERATORS.binary_search(&code).is_ok())
}

/// Blink's `FindCategory`: `None` if the dictionary does not know `content` in `form`.
fn find(content: &str, form: Form) -> Option<OperatorProperties> {
	let mut characters = content.chars();
	let first = characters.next()?;
	let key = match (characters.next(), characters.next()) {
		(None, _) => {
			let code = u32::from(first);
			let is_mapped_range = (COMBINING_MINUS_SIGN_BELOW
				..=GREEK_CAPITAL_REVERSED_DOTTED_LUNATE_SIGMA_SYMBOL)
				.contains(&code);
			(!is_mapped_range).then_some(code)?
		}
		// Combining overlays take the properties of the first character.
		(Some('\u{338}' | '\u{20D2}'), None) => u32::from(first),
		(Some(_), None) => {
			let index = TWO_ASCII_CHARACTERS.iter().position(|operator| *operator == content)?;
			COMBINING_MINUS_SIGN_BELOW + index as u32
		}
		_ => return None,
	};
	if form == Form::Infix && matches!(key, 0x7C | 0x223C) {
		return Some(DEFAULT);
	}
	let is_prefix_space = matches!(key, 0x2145..=0x2146 | 0x2202 | 0x221A..=0x221C);
	if form == Form::Prefix && is_prefix_space {
		return Some(CATEGORY_L);
	}
	if form == Form::Infix && matches!(key, 0x2C | 0x3A | 0x3B) {
		return Some(CATEGORY_M);
	}
	let key = match key {
		0x2000..=0x2BFF => key - 0x1C00,
		0x400.. => return None,
		_ => key,
	};
	let key = key
		| match form {
			Form::Infix => 0,
			Form::Prefix => 0x1000,
			Form::Postfix => 0x2000,
		};
	let index = COMPACT_DICTIONARY.partition_point(|&(entry, _)| u32::from(entry & 0x3FFF) <= key);
	let &(entry, range) = COMPACT_DICTIONARY.get(index.checked_sub(1)?)?;
	if key > u32::from(entry & 0x3FFF) + u32::from(range) {
		return None;
	}
	match entry >> 12 {
		0x0 => Some(CATEGORY_A),
		0x4 => Some(CATEGORY_B),
		0x8 => Some(CATEGORY_C),
		0x1 | 0x2 | 0xC => Some(CATEGORY_D_E_K),
		0x5 | 0x6 => Some(CATEGORY_F_G),
		0x9 => Some(CATEGORY_H),
		0xA => Some(CATEGORY_I),
		0xD => Some(CATEGORY_J),
		_ => None,
	}
}

/// The properties of an operator without an explicit `form`, whose form follows from its siblings
/// (`ComputeDictionaryCategory`).
pub fn operator_properties(
	content: &str,
	has_previous_sibling: bool,
	has_next_sibling: bool,
) -> OperatorProperties {
	let form = match (has_previous_sibling, has_next_sibling) {
		(false, true) => Form::Prefix,
		(true, false) => Form::Postfix,
		_ => Form::Infix,
	};
	find(content, form)
		.or_else(|| {
			[Form::Infix, Form::Postfix, Form::Prefix]
				.into_iter()
				.filter(|&other| other != form)
				.find_map(|other| find(content, other))
		})
		.unwrap_or(DEFAULT)
}

/// `text-transform: math-auto`: the italic form of a single character identifier.
pub fn math_italic(character: char) -> char {
	let code = match character {
		'h' => 0x210E,
		'A'..='Z' => 0x1D434 + (character as u32 - 'A' as u32),
		'a'..='z' => 0x1D44E + (character as u32 - 'a' as u32),
		'\u{131}' => 0x1D6A4,
		'\u{237}' => 0x1D6A5,
		'\u{391}'..='\u{3A1}' => 0x1D6E2 + (character as u32 - 0x391),
		'\u{3F4}' => 0x1D6F3,
		'\u{3A3}'..='\u{3A9}' => 0x1D6F4 + (character as u32 - 0x3A3),
		'\u{2207}' => 0x1D6FB,
		'\u{3B1}'..='\u{3C9}' => 0x1D6FC + (character as u32 - 0x3B1),
		'\u{2202}' => 0x1D715,
		'\u{3F5}' => 0x1D716,
		'\u{3D1}' => 0x1D717,
		'\u{3F0}' => 0x1D718,
		'\u{3D5}' => 0x1D719,
		'\u{3F1}' => 0x1D71A,
		'\u{3D6}' => 0x1D71B,
		_ => return character,
	};
	char::from_u32(code).unwrap_or(character)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_tables_are_sorted() {
		assert!(INLINE_AXIS_OPERATORS.is_sorted());
		assert!(COMPACT_DICTIONARY.is_sorted_by_key(|(entry, _)| entry & 0x3FFF));
	}

	#[test]
	fn finds_operator_properties() {
		// Relations and binary operators.
		let plus = operator_properties("+", true, true);
		assert_eq!((plus.leading_space, plus.trailing_space), (4, 4));
		let equals = operator_properties("=", true, true);
		assert_eq!(
			(equals.leading_space, equals.trailing_space, equals.is_stretchy),
			(5, 5, false)
		);
		// A sign at the start of a row has no spaces.
		let minus = operator_properties("\u{2212}", false, true);
		assert_eq!((minus.leading_space, minus.trailing_space), (0, 0));
		// Fences.
		let open = operator_properties("(", false, true);
		assert!(open.is_stretchy && open.is_symmetric && open.leading_space == 0);
		// Large operators.
		let sum = operator_properties("\u{2211}", false, true);
		assert!(sum.is_large_op && sum.has_movable_limits && sum.is_symmetric);
		let integral = operator_properties("\u{222B}", false, true);
		assert!(integral.is_large_op && !integral.has_movable_limits);
		// Separators.
		let comma = operator_properties(",", true, true);
		assert_eq!((comma.leading_space, comma.trailing_space), (0, 3));
		// The function application.
		let apply = operator_properties("\u{2061}", true, true);
		assert_eq!((apply.leading_space, apply.trailing_space), (0, 0));
		// Unknown operators have the default spaces.
		assert_eq!(operator_properties("x", true, true), DEFAULT);
		assert_eq!(operator_properties("mod", true, true), DEFAULT);
		assert_eq!(operator_properties("", true, true), DEFAULT);
		assert_eq!(operator_properties("\u{1F600}", true, true), DEFAULT);
		// Two ASCII characters.
		assert_eq!(operator_properties("<=", true, true).leading_space, 5);
	}

	#[test]
	fn knows_the_stretch_axis() {
		assert!(is_vertical('('));
		assert!(is_vertical('\u{221A}'));
		assert!(!is_vertical('\u{2192}'));
		assert!(!is_vertical('\u{203E}'));
		assert!(is_vertical('\u{1F600}'));
	}

	#[test]
	fn maps_identifiers_to_italics() {
		assert_eq!(math_italic('x'), '\u{1D465}');
		assert_eq!(math_italic('h'), '\u{210E}');
		assert_eq!(math_italic('A'), '\u{1D434}');
		assert_eq!(math_italic('\u{3B1}'), '\u{1D6FC}');
		assert_eq!(math_italic('1'), '1');
	}
}
