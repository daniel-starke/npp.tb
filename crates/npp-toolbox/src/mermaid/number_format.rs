//! d3's `format` (d3-format 3) in its default locale, with JavaScript's exact `toFixed`,
//! `toExponential` and `toPrecision`.

use super::color::{js_number, js_round};
use super::common::js_number_of;

/// The decimal digits of a positive finite number and the exponent of its first digit: the
/// exact value, without trailing zeros.
fn exact_digits(value: f64) -> (Vec<u8>, i32) {
	// A double has at most 767 significant decimal digits.
	let text = format!("{value:.800e}");
	let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
	let mut digits: Vec<u8> =
		mantissa.bytes().filter(u8::is_ascii_digit).map(|digit| digit - b'0').collect();
	while digits.len() > 1 && digits.last() == Some(&0) {
		digits.pop();
	}
	(digits, exponent.parse().unwrap_or(0))
}

/// The digits rounded to `count` significant digits, halves up like JavaScript, and the exponent
/// of the first digit, which a carry raises.
fn round_digits(value: f64, count: usize) -> (Vec<u8>, i32) {
	let (mut digits, mut exponent) = exact_digits(value);
	if digits.len() > count {
		let round_up = digits[count] >= 5;
		digits.truncate(count);
		if round_up {
			let mut index = count;
			loop {
				if index == 0 {
					digits.insert(0, 1);
					digits.truncate(count.max(1));
					exponent += 1;
					break;
				}
				index -= 1;
				if digits[index] == 9 {
					digits[index] = 0;
				} else {
					digits[index] += 1;
					break;
				}
			}
		}
	}
	digits.resize(count, 0);
	(digits, exponent)
}

fn digit_text(digits: &[u8]) -> String {
	digits.iter().map(|digit| char::from(b'0' + digit)).collect()
}

/// `number.toFixed(digits)` with `digits` from 0 to 100.
pub fn to_fixed(value: f64, digits: usize) -> String {
	if !value.is_finite() || value.abs() >= 1e21 {
		return js_number(value);
	}
	let sign = if value < 0.0 { "-" } else { "" };
	let magnitude = value.abs();
	// The digits of the integer closest to `magnitude * 10^digits`, halves up.
	let mut integer = match magnitude {
		0.0 => vec![0],
		_ => {
			let (exact, exponent) = exact_digits(magnitude);
			let count = exponent + 1 + digits as i32;
			match count {
				..0 => vec![0],
				0 => vec![u8::from(exact[0] >= 5)],
				_ => {
					let (mut rounded, rounded_exponent) = round_digits(magnitude, count as usize);
					if rounded_exponent > exponent {
						rounded.push(0);
					}
					rounded
				}
			}
		}
	};
	if integer.len() < digits + 1 {
		integer.splice(0..0, std::iter::repeat_n(0, digits + 1 - integer.len()));
	}
	let point = integer.len() - digits;
	let mut text = format!("{sign}{}", digit_text(&integer[..point]));
	if digits > 0 {
		text.push('.');
		text.push_str(&digit_text(&integer[point..]));
	}
	text
}

fn exponent_suffix(exponent: i32) -> String {
	match exponent < 0 {
		true => format!("e-{}", -exponent),
		false => format!("e+{exponent}"),
	}
}

/// `number.toExponential(fraction)`, or `number.toExponential()` with `None`.
pub fn to_exponential(value: f64, fraction: Option<usize>) -> String {
	if !value.is_finite() {
		return js_number(value);
	}
	let sign = if value < 0.0 { "-" } else { "" };
	let magnitude = value.abs();
	let (digits, exponent) = match (magnitude, fraction) {
		(0.0, fraction) => (vec![0; fraction.unwrap_or(0) + 1], 0),
		(_, Some(fraction)) => round_digits(magnitude, fraction + 1),
		(_, None) => {
			// The shortest digits that read back as the same number.
			let shortest = format!("{magnitude:e}");
			let (mantissa, exponent) = shortest.split_once('e').unwrap_or((&shortest, "0"));
			let digits =
				mantissa.bytes().filter(u8::is_ascii_digit).map(|digit| digit - b'0').collect();
			(digits, exponent.parse().unwrap_or(0))
		}
	};
	let mut text = format!("{sign}{}", digits[0]);
	if digits.len() > 1 {
		text.push('.');
		text.push_str(&digit_text(&digits[1..]));
	}
	text + &exponent_suffix(exponent)
}

/// `number.toPrecision(precision)` with `precision` from 1 to 100.
pub fn to_precision(value: f64, precision: usize) -> String {
	if !value.is_finite() {
		return js_number(value);
	}
	let sign = if value < 0.0 { "-" } else { "" };
	let magnitude = value.abs();
	let (digits, exponent) = match magnitude {
		0.0 => (vec![0; precision], 0),
		_ => round_digits(magnitude, precision),
	};
	if exponent < -6 || exponent >= precision as i32 {
		let mut text = format!("{sign}{}", digits[0]);
		if precision > 1 {
			text.push('.');
			text.push_str(&digit_text(&digits[1..]));
		}
		return text + &exponent_suffix(exponent);
	}
	if exponent >= 0 {
		let point = exponent as usize + 1;
		let mut text = format!("{sign}{}", digit_text(&digits[..point]));
		if precision > point {
			text.push('.');
			text.push_str(&digit_text(&digits[point..]));
		}
		return text;
	}
	format!("{sign}0.{}{}", "0".repeat((-exponent - 1) as usize), digit_text(&digits))
}

/// `Math.round(value).toString(radix)` for the radixes 2, 8 and 16.
fn integer_in_radix(value: f64, radix: u32) -> String {
	let rounded = js_round(value);
	if !rounded.is_finite() {
		return js_number(rounded);
	}
	let sign = if rounded < 0.0 { "-" } else { "" };
	let magnitude = rounded.abs();
	if magnitude < 2f64.powi(64) {
		let integer = magnitude as u64;
		return match radix {
			2 => format!("{sign}{integer:b}"),
			8 => format!("{sign}{integer:o}"),
			_ => format!("{sign}{integer:x}"),
		};
	}
	// Larger doubles are a 53-bit integer shifted left: the digits are exact.
	let bits = magnitude.to_bits();
	let exponent = ((bits >> 52) & 0x7ff) as i32 - 1075;
	let mantissa = (bits & ((1 << 52) - 1)) | (1 << 52);
	let mut binary = format!("{mantissa:b}");
	binary.push_str(&"0".repeat(exponent.max(0) as usize));
	let width = match radix {
		2 => 1,
		8 => 3,
		_ => 4,
	};
	let padding = (width - binary.len() % width) % width;
	let binary = "0".repeat(padding) + &binary;
	let digits: String = binary
		.as_bytes()
		.chunks(width)
		.map(|chunk| {
			let digit = chunk.iter().fold(0u32, |digit, bit| digit * 2 + u32::from(bit - b'0'));
			char::from_digit(digit, radix).unwrap_or('0')
		})
		.collect();
	format!("{sign}{}", digits.trim_start_matches('0'))
}

/// `formatDecimalParts`: the significant digits and the exponent of `toExponential`.
fn decimal_parts(value: f64, precision: Option<usize>) -> Option<(String, i32)> {
	if !value.is_finite() {
		return None;
	}
	let text = to_exponential(value, precision.map(|precision| precision.saturating_sub(1)));
	let (coefficient, exponent) = text.split_once('e')?;
	Some((coefficient.replace('.', ""), exponent.parse().ok()?))
}

/// `formatRounded`
fn format_rounded(value: f64, precision: usize) -> String {
	let Some((coefficient, exponent)) = decimal_parts(value, Some(precision)) else {
		return js_number(value);
	};
	let length = coefficient.len() as i32;
	if exponent < 0 {
		format!("0.{}{coefficient}", "0".repeat((-exponent - 1) as usize))
	} else if length > exponent + 1 {
		let point = exponent as usize + 1;
		format!("{}.{}", &coefficient[..point], &coefficient[point..])
	} else {
		coefficient + &"0".repeat((exponent - length + 1) as usize)
	}
}

const SI_PREFIXES: [&str; 17] =
	["y", "z", "a", "f", "p", "n", "µ", "m", "", "k", "M", "G", "T", "P", "E", "Z", "Y"];

/// `formatPrefixAuto`, which also sets the exponent of the SI prefix.
fn format_prefix_auto(value: f64, precision: usize, prefix_exponent: &mut Option<i32>) -> String {
	let Some((coefficient, exponent)) = decimal_parts(value, Some(precision)) else {
		return js_number(value);
	};
	let prefix = (exponent as f64 / 3.0).floor().clamp(-8.0, 8.0) as i32 * 3;
	*prefix_exponent = Some(prefix);
	let index = exponent - prefix + 1;
	let length = coefficient.len() as i32;
	if index == length {
		coefficient
	} else if index > length {
		coefficient + &"0".repeat((index - length) as usize)
	} else if index > 0 {
		let point = index as usize;
		format!("{}.{}", &coefficient[..point], &coefficient[point..])
	} else {
		let precision = (precision as i32 + index - 1).max(0) as usize;
		let digits =
			decimal_parts(value, (precision > 0).then_some(precision)).map(|(digits, _)| digits);
		format!("0.{}{}", "0".repeat((-index) as usize), digits.unwrap_or_default())
	}
}

/// `formatTrim`: removes insignificant zeros, `1.2000k` becomes `1.2k`.
fn trim_zeros(text: &str) -> String {
	let characters: Vec<char> = text.chars().collect();
	let (mut start, mut end) = (-1i64, 0usize);
	for (index, &character) in characters.iter().enumerate().skip(1) {
		match character {
			'.' => {
				start = index as i64;
				end = index;
			}
			'0' => {
				if start == 0 {
					start = index as i64;
				}
				end = index;
			}
			_ => {
				if !character.is_ascii_digit() || character == '0' {
					break;
				}
				if start > 0 {
					start = 0;
				}
			}
		}
	}
	match start > 0 {
		true => {
			let start = start as usize;
			characters[..start].iter().chain(&characters[end + 1..]).collect()
		}
		false => text.to_owned(),
	}
}

/// `formatGroup` with the default locale: groups of three digits separated by commas.
fn group(value: &[char], width: usize) -> String {
	let mut parts: Vec<String> = Vec::new();
	let mut index = value.len();
	let mut length = 0;
	let mut size = 3;
	while index > 0 && size > 0 {
		if length + size + 1 > width {
			size = width.saturating_sub(length).max(1);
		}
		let start = index.saturating_sub(size);
		parts.push(value[start..index].iter().collect());
		index = start;
		length += size + 1;
		if length > width {
			break;
		}
		size = 3;
	}
	parts.reverse();
	parts.join(",")
}

const MAX_WIDTH: usize = 10_000;

/// The next character of `rest` if it matches.
fn take(rest: &mut &[char], predicate: impl Fn(char) -> bool) -> Option<char> {
	let (&first, after) = rest.split_first()?;
	predicate(first).then(|| {
		*rest = after;
		first
	})
}

/// A parsed format specifier: `[[fill]align][sign][symbol][0][width][,][.precision][~][type]`.
#[derive(Clone, Debug, PartialEq)]
pub struct NumberFormat {
	fill: char,
	align: char,
	sign: char,
	symbol: Option<char>,
	zero: bool,
	width: usize,
	comma: bool,
	precision: usize,
	trim: bool,
	kind: char,
	prefix: String,
	suffix: String,
}

impl NumberFormat {
	/// `d3.format(specifier)`, `None` where d3 throws "invalid format".
	pub fn new(specifier: &str) -> Option<NumberFormat> {
		let characters: Vec<char> = specifier.chars().collect();
		let is_align = |character: Option<&char>| character.is_some_and(|c| "<>=^".contains(*c));
		let mut candidates = Vec::new();
		if characters.len() >= 2
			&& is_align(characters.get(1))
			&& !matches!(characters[0], '\n' | '\r')
		{
			candidates.push((Some(characters[0]), Some(characters[1]), 2));
		}
		if is_align(characters.first()) {
			candidates.push((None, Some(characters[0]), 1));
		}
		candidates.push((None, None, 0));
		candidates
			.into_iter()
			.find_map(|(fill, align, start)| Self::with_rest(fill, align, &characters[start..]))
	}

	/// The specifier after fill and alignment, as d3's regular expression reads it.
	fn with_rest(fill: Option<char>, align: Option<char>, rest: &[char]) -> Option<NumberFormat> {
		let mut rest = rest;
		let sign = take(&mut rest, |c| "+-( ".contains(c));
		let symbol = take(&mut rest, |c| c == '$' || c == '#');
		let zero = take(&mut rest, |c| c == '0').is_some();
		let mut width_digits = String::new();
		while let Some(digit) = take(&mut rest, |c| c.is_ascii_digit()) {
			width_digits.push(digit);
		}
		let comma = take(&mut rest, |c| c == ',').is_some();
		let mut precision_digits = None;
		if rest.first() == Some(&'.') && rest.get(1).is_some_and(char::is_ascii_digit) {
			rest = &rest[1..];
			let mut digits = String::new();
			while let Some(digit) = take(&mut rest, |c| c.is_ascii_digit()) {
				digits.push(digit);
			}
			precision_digits = Some(digits);
		}
		let trim = take(&mut rest, |c| c == '~').is_some();
		let kind = take(&mut rest, |c| c.is_ascii_alphabetic() || c == '%');
		if !rest.is_empty() {
			return None;
		}
		let mut format = NumberFormat {
			fill: fill.unwrap_or(' '),
			align: align.unwrap_or('>'),
			sign: sign.unwrap_or('-'),
			symbol,
			zero,
			// Wider padding than this is no use and only costs memory.
			width: match width_digits.is_empty() {
				true => 0,
				false => width_digits.parse::<usize>().unwrap_or(usize::MAX).min(MAX_WIDTH),
			},
			comma,
			precision: 0,
			trim,
			kind: kind.unwrap_or('\0'),
			prefix: String::new(),
			suffix: String::new(),
		};
		let mut precision: Option<usize> =
			precision_digits.map(|digits| digits.parse().unwrap_or(usize::MAX));
		const TYPES: &str = "%bcdefgoprsXx";
		if format.kind == 'n' {
			format.comma = true;
			format.kind = 'g';
		} else if !TYPES.contains(format.kind) || format.kind == '\0' {
			precision.get_or_insert(12);
			format.trim = true;
			format.kind = 'g';
		}
		if format.zero || (format.fill == '0' && format.align == '=') {
			(format.zero, format.fill, format.align) = (true, '0', '=');
		}
		format.prefix = match format.symbol {
			Some('$') => "$".to_owned(),
			Some('#') if "boxX".contains(format.kind) => {
				format!("0{}", format.kind.to_ascii_lowercase())
			}
			_ => String::new(),
		};
		format.suffix = match format.symbol {
			Some('$') => String::new(),
			_ if "%p".contains(format.kind) => "%".to_owned(),
			_ => String::new(),
		};
		format.precision = match precision {
			None => 6,
			Some(precision) if "gprs".contains(format.kind) => precision.clamp(1, 21),
			Some(precision) => precision.min(20),
		};
		Some(format)
	}

	fn format_type(&self, value: f64, prefix_exponent: &mut Option<i32>) -> String {
		let precision = self.precision;
		match self.kind {
			'%' => to_fixed(value * 100.0, precision),
			'b' => integer_in_radix(value, 2),
			'c' => js_number(value),
			'd' => {
				let rounded = js_round(value);
				match rounded.abs() >= 1e21 {
					true if rounded.is_infinite() => "∞".to_owned(),
					true => format!("{rounded:.0}"),
					false => js_number(rounded),
				}
			}
			'e' => to_exponential(value, Some(precision)),
			'f' => to_fixed(value, precision),
			'g' => to_precision(value, precision),
			'o' => integer_in_radix(value, 8),
			'p' => format_rounded(value * 100.0, precision),
			'r' => format_rounded(value, precision),
			's' => format_prefix_auto(value, precision, prefix_exponent),
			'X' => integer_in_radix(value, 16).to_uppercase(),
			_ => integer_in_radix(value, 16),
		}
	}

	/// The formatted value. `prefix_exponent` is d3's module-wide SI exponent, which a value
	/// that is no finite number leaves as it was.
	pub fn format(&self, value: f64, prefix_exponent: &mut Option<i32>) -> String {
		let mut value_prefix = self.prefix.clone();
		let mut value_suffix = self.suffix.clone();
		let mut text;
		if self.kind == 'c' {
			value_suffix = js_number(value) + &value_suffix;
			text = String::new();
		} else {
			let mut is_negative = value < 0.0 || (value == 0.0 && value.is_sign_negative());
			text = match value.is_nan() {
				true => "NaN".to_owned(),
				false => self.format_type(value.abs(), prefix_exponent),
			};
			if self.trim {
				text = trim_zeros(&text);
			}
			if is_negative && js_number_of(&text) == 0.0 && self.sign != '+' {
				is_negative = false;
			}
			let sign = match (is_negative, self.sign) {
				(true, '(') => "(".to_owned(),
				(true, _) => "−".to_owned(),
				(false, '-' | '(') => String::new(),
				(false, sign) => sign.to_string(),
			};
			value_prefix = sign + &value_prefix;
			let si = match self.kind {
				's' => match *prefix_exponent {
					Some(exponent) => {
						SI_PREFIXES[(8 + exponent / 3).clamp(0, 16) as usize].to_owned()
					}
					None => "undefined".to_owned(),
				},
				_ => String::new(),
			};
			value_suffix =
				si + &value_suffix + if is_negative && self.sign == '(' { ")" } else { "" };
			if "defgprs%".contains(self.kind)
				&& let Some(split) = text.find(|character: char| !character.is_ascii_digit())
			{
				value_suffix = text[split..].to_owned() + &value_suffix;
				text.truncate(split);
			}
		}
		let mut digits: Vec<char> = text.chars().collect();
		if self.comma && !self.zero {
			digits = group(&digits, usize::MAX).chars().collect();
		}
		let length = value_prefix.chars().count() + digits.len() + value_suffix.chars().count();
		let mut padding: String = match length < self.width {
			true => std::iter::repeat_n(self.fill, self.width - length).collect(),
			false => String::new(),
		};
		if self.comma && self.zero {
			let padded: Vec<char> = padding.chars().chain(digits.iter().copied()).collect();
			let width = match padding.is_empty() {
				true => usize::MAX,
				false => self.width.saturating_sub(value_suffix.chars().count()),
			};
			digits = group(&padded, width).chars().collect();
			padding.clear();
		}
		let value: String = digits.into_iter().collect();
		match self.align {
			'<' => value_prefix + &value + &value_suffix + &padding,
			'=' => value_prefix + &padding + &value + &value_suffix,
			'^' => {
				let half = padding.chars().count() / 2;
				let before: String = padding.chars().take(half).collect();
				let after: String = padding.chars().skip(half).collect();
				before + &value_prefix + &value + &value_suffix + &after
			}
			_ => padding + &value_prefix + &value + &value_suffix,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn converts_numbers_like_javascript() {
		assert_eq!(to_fixed(1.005, 2), "1.00");
		assert_eq!(to_fixed(2.5, 0), "3");
		assert_eq!(to_fixed(0.5, 0), "1");
		assert_eq!(to_fixed(0.04, 1), "0.0");
		assert_eq!(to_fixed(0.05, 1), "0.1");
		assert_eq!(to_fixed(0.0001, 2), "0.00");
		assert_eq!(to_fixed(999.996, 2), "1000.00");
		assert_eq!(to_fixed(1234.5678, 2), "1234.57");
		assert_eq!(to_fixed(1e21, 2), "1e+21");
		assert_eq!(to_exponential(1234.5, Some(2)), "1.23e+3");
		assert_eq!(to_exponential(0.00015, None), "1.5e-4");
		assert_eq!(to_exponential(9.99, Some(1)), "1.0e+1");
		assert_eq!(to_exponential(0.0, Some(2)), "0.00e+0");
		assert_eq!(to_precision(1234567.891, 12), "1234567.89100");
		assert_eq!(to_precision(0.0123, 3), "0.0123");
		assert_eq!(to_precision(1e-7, 2), "1.0e-7");
		assert_eq!(to_precision(123456.0, 2), "1.2e+5");
		assert_eq!(integer_in_radix(255.4, 16), "ff");
		assert_eq!(integer_in_radix(2f64.powi(70), 16), "400000000000000000");
	}

	fn formatted(specifier: &str, value: f64) -> String {
		NumberFormat::new(specifier).unwrap().format(value, &mut None)
	}

	#[test]
	fn formats_like_d3() {
		assert_eq!(formatted(",", 1234567.891), "1,234,567.891");
		assert_eq!(formatted(",", 42.0), "42");
		assert_eq!(formatted(",.2f", 1234.5), "1,234.50");
		assert_eq!(formatted(".1%", 0.0123), "1.2%");
		assert_eq!(formatted("s", 1234567.891), "1.23457M");
		assert_eq!(formatted("~s", 1500.0), "1.5k");
		assert_eq!(formatted("e", 42.0), "4.200000e+1");
		assert_eq!(formatted("x", 255.0), "ff");
		assert_eq!(formatted("#x", 255.0), "0xff");
		assert_eq!(formatted("+,", 1234.0), "+1,234");
		assert_eq!(formatted("08.2f", 4.567), "00004.57");
		assert_eq!(formatted("^12,", 1234.0), "   1,234    ");
		assert_eq!(formatted(".3r", 0.0123), "0.0123");
		assert_eq!(formatted("$,.2f", 3.5), "$3.50");
		assert_eq!(formatted("(.1f", -2.25), "(2.3)");
		assert_eq!(formatted(".1f", -0.01), "0.0");
		assert_eq!(formatted("d", 2.5), "3");
		assert_eq!(formatted("010,", 1234567.0), "01,234,567");
		assert!(NumberFormat::new("bad!!").is_none());
		assert!(NumberFormat::new(".f").is_none());
	}

	#[test]
	fn trims_insignificant_zeros() {
		assert_eq!(trim_zeros("1.2000"), "1.2");
		assert_eq!(trim_zeros("1.000"), "1");
		assert_eq!(trim_zeros("100"), "100");
		assert_eq!(trim_zeros("1.50e+3"), "1.5e+3");
	}
}
