//! The parts of dayjs 1.11 that Mermaid's gantt charts use: strict parsing with the
//! `customParseFormat` plugin, formatting with `advancedFormat` and `isoWeek`, `add`, `startOf`
//! and `endOf` a day and `diff` in years, all in the English locale.

use super::date::{Calendar, Fields, MS_PER_DAY, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND};
use crate::mermaid::color::js_round;
use crate::mermaid::common::js_number_of;

const MONTHS: [&str; 12] = [
	"January",
	"February",
	"March",
	"April",
	"May",
	"June",
	"July",
	"August",
	"September",
	"October",
	"November",
	"December",
];
const WEEKDAYS: [&str; 7] =
	["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// A JavaScript exception thrown by dayjs, for formats that need plugins Mermaid does not load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exception(pub String);

/// dayjs' `padStart`, which leaves longer strings alone and pads before a minus sign.
fn pad_start(text: &str, length: usize) -> String {
	let count = text.chars().count();
	if text.is_empty() || count >= length {
		return text.to_owned();
	}
	"0".repeat(length - count) + text
}

/// A JavaScript number as `String()` writes it, for the integers dates consist of.
fn js_integer(value: f64) -> String {
	crate::mermaid::color::js_number(value)
}

/// The English ordinal of a day without its brackets: `1st`, `22nd`.
fn ordinal(day: f64) -> String {
	let value = day % 100.0;
	let suffixes = ["th", "st", "nd", "rd"];
	let index = (value - 20.0) % 10.0;
	let suffix = if (0.0..4.0).contains(&index) && index.fract() == 0.0 && index != 0.0 {
		suffixes[index as usize]
	} else if (0.0..4.0).contains(&value) && value != 0.0 {
		suffixes[value as usize]
	} else {
		"th"
	};
	format!("{}{suffix}", js_integer(day))
}

impl Calendar<'_> {
	/// dayjs' `utcOffset`: minutes ahead of UTC, rounded to quarter hours.
	fn dayjs_utc_offset(&self, time: f64) -> f64 {
		-js_round(self.timezone_offset(time) / 15.0) * 15.0 + 0.0
	}

	/// dayjs' `padZoneStr`: `+01:00`.
	fn zone_text(&self, time: f64) -> String {
		let negative_minutes = -self.dayjs_utc_offset(time);
		let minutes = negative_minutes.abs();
		let sign = if negative_minutes <= 0.0 { '+' } else { '-' };
		format!(
			"{sign}{}:{}",
			pad_start(&js_integer((minutes / 60.0).floor()), 2),
			pad_start(&js_integer(minutes % 60.0), 2)
		)
	}

	/// `isoWeekday`: 1 for Monday to 7 for Sunday.
	pub fn iso_weekday(&self, time: f64) -> f64 {
		let weekday = self.fields(time).weekday;
		if weekday == 0.0 { 7.0 } else { weekday }
	}

	/// `add` of dayjs with a unit of `parseDuration`: `y`, `M`, `w`, `d`, `h`, `m`, `s` or `ms`.
	pub fn add(&self, time: f64, amount: f64, unit: &str) -> f64 {
		match unit {
			"M" | "y" => {
				if time.is_nan() {
					return f64::NAN;
				}
				let fields = self.fields(time);
				// dayjs sets the first of the month, changes the month or year and clamps the
				// day to the length of the new month.
				let first = self.with_fields(time, |changed| changed.date = 1.0);
				let moved = if unit == "M" {
					self.with_fields(first, |changed| changed.month = fields.month + amount)
				} else {
					self.set_full_year(first, fields.year + amount)
				};
				let days_in_month = self.days_in_month(moved);
				let date =
					if days_in_month.is_nan() { f64::NAN } else { fields.date.min(days_in_month) };
				self.with_fields(moved, |changed| changed.date = date)
			}
			"d" | "w" => {
				let days = if unit == "d" { 1.0 } else { 7.0 };
				self.with_fields(time, |fields| fields.date += js_round(days * amount))
			}
			_ => {
				let step = match unit {
					"m" => MS_PER_MINUTE,
					"h" => MS_PER_HOUR,
					"s" => MS_PER_SECOND,
					_ => 1.0,
				};
				super::date::time_clip(time + amount * step)
			}
		}
	}

	fn days_in_month(&self, time: f64) -> f64 {
		let fields = self.fields(time);
		let last = self.make(&Fields { month: fields.month + 1.0, date: 0.0, ..fields });
		self.fields(last).date
	}

	/// `startOf('day')`
	pub fn start_of_day(&self, time: f64) -> f64 {
		self.with_fields(time, |fields| {
			(fields.hours, fields.minutes, fields.seconds, fields.milliseconds) =
				(0.0, 0.0, 0.0, 0.0)
		})
	}

	/// `endOf('day')`
	pub fn end_of_day(&self, time: f64) -> f64 {
		self.with_fields(time, |fields| {
			(fields.hours, fields.minutes, fields.seconds, fields.milliseconds) =
				(23.0, 59.0, 59.0, 999.0)
		})
	}

	/// dayjs' `monthDiff`, from moment.js.
	fn month_diff(&self, first: f64, second: f64, depth: u8) -> f64 {
		let (a, b) = (self.fields(first), self.fields(second));
		if a.date < b.date && depth == 0 {
			return -self.month_diff(second, first, 1);
		}
		let whole = (b.year - a.year) * 12.0 + (b.month - a.month);
		let anchor = self.add(first, whole, "M");
		let is_before = second - anchor < 0.0;
		let anchor2 = self.add(first, whole + if is_before { -1.0 } else { 1.0 }, "M");
		let fraction =
			(second - anchor) / if is_before { anchor - anchor2 } else { anchor2 - anchor };
		let result = -(whole + fraction);
		if result.is_nan() { 0.0 } else { result + 0.0 }
	}

	/// `diff(other, 'year')`: whole years, towards zero.
	pub fn diff_years(&self, time: f64, other: f64) -> f64 {
		let years = self.month_diff(time, other, 0) / 12.0;
		if years < 0.0 { years.ceil() + 0.0 } else { years.floor() }
	}

	/// `format` with the `advancedFormat` and `isoWeek` plugins.
	pub fn format(&self, time: f64, format: &str) -> Result<String, Exception> {
		if time.is_nan() {
			return Ok("Invalid Date".to_owned());
		}
		let format = if format.is_empty() { "YYYY-MM-DDTHH:mm:ssZ" } else { format };
		let advanced = self.format_advanced(time, format)?;
		Ok(self.format_core(time, &advanced))
	}

	/// The replacements of `advancedFormat`, whose output the core format then reads again.
	fn format_advanced(&self, time: f64, format: &str) -> Result<String, Exception> {
		let fields = self.fields(time);
		let mut output = String::with_capacity(format.len());
		let mut rest = format;
		while let Some(character) = rest.chars().next() {
			if character == '['
				&& let Some(end) = rest[1..].find(']')
				&& end > 0
			{
				output.push_str(&rest[..end + 2]);
				rest = &rest[end + 2..];
				continue;
			}
			let tokens = [
				"Q", "wo", "ww", "w", "WW", "W", "zzz", "z", "gggg", "GGGG", "Do", "X", "x", "kk",
				"k", "S",
			];
			let Some(token) = tokens.iter().find(|token| rest.starts_with(**token)) else {
				output.push(character);
				rest = &rest[character.len_utf8()..];
				continue;
			};
			rest = &rest[token.len()..];
			match *token {
				"Q" => output.push_str(&js_integer(((fields.month + 1.0) / 3.0).ceil())),
				"Do" => {
					output.push('[');
					output.push_str(&ordinal(fields.date));
					output.push(']');
				}
				"GGGG" => output.push_str(&js_integer(self.iso_week_year(time))),
				"W" | "WW" => output.push_str(&pad_start(
					&js_integer(self.iso_week(time)),
					if *token == "W" { 1 } else { 2 },
				)),
				"k" | "kk" => {
					let hours = if fields.hours == 0.0 { 24.0 } else { fields.hours };
					output.push_str(&pad_start(&js_integer(hours), token.len()));
				}
				"X" => output.push_str(&js_integer((time / 1000.0).floor())),
				"x" => output.push_str(&js_integer(time)),
				"S" => output.push('S'),
				// `week`, `weekYear` and `offsetName` come from plugins Mermaid does not load.
				other => return Err(Exception(format!("dayjs cannot format \"{other}\""))),
			}
		}
		Ok(output)
	}

	fn format_core(&self, time: f64, format: &str) -> String {
		let fields = self.fields(time);
		let zone = self.zone_text(time);
		let hours12 = {
			let hours = fields.hours % 12.0;
			if hours == 0.0 { 12.0 } else { hours }
		};
		let mut output = String::with_capacity(format.len() + 8);
		let mut rest = format;
		while let Some(character) = rest.chars().next() {
			if character == '['
				&& let Some(end) = rest[1..].find(']')
				&& end > 0
			{
				output.push_str(&rest[1..end + 1]);
				rest = &rest[end + 2..];
				continue;
			}
			let run = rest.chars().take_while(|&next| next == character).count();
			let (length, text) = match character {
				'Y' if run >= 4 => (4, pad_start(&js_integer(fields.year), 4)),
				'Y' if run >= 2 => {
					let year = js_integer(fields.year);
					let start = year.len().saturating_sub(2);
					(2, year[start..].to_owned())
				}
				'M' => {
					let length = run.min(4);
					let text = match length {
						1 => js_integer(fields.month + 1.0),
						2 => pad_start(&js_integer(fields.month + 1.0), 2),
						3 => MONTHS[fields.month as usize % 12][..3].to_owned(),
						_ => MONTHS[fields.month as usize % 12].to_owned(),
					};
					(length, text)
				}
				'D' => {
					let length = run.min(2);
					let date = js_integer(fields.date);
					(length, if length == 2 { pad_start(&date, 2) } else { date })
				}
				'd' => {
					let weekday = WEEKDAYS[fields.weekday as usize % 7];
					let length = run.min(4);
					let text = match length {
						1 => js_integer(fields.weekday),
						2 => weekday[..2].to_owned(),
						3 => weekday[..3].to_owned(),
						_ => weekday.to_owned(),
					};
					(length, text)
				}
				'H' | 'h' | 'm' | 's' => {
					let value = match character {
						'H' => fields.hours,
						'h' => hours12,
						'm' => fields.minutes,
						_ => fields.seconds,
					};
					let length = run.min(2);
					let text = js_integer(value);
					(length, if length == 2 { pad_start(&text, 2) } else { text })
				}
				'a' => (1, if fields.hours < 12.0 { "am" } else { "pm" }.to_owned()),
				'A' => (1, if fields.hours < 12.0 { "AM" } else { "PM" }.to_owned()),
				'Z' if run >= 2 => (2, zone.replacen(':', "", 1)),
				'Z' => (1, zone.clone()),
				'S' if run >= 3 => (3, pad_start(&js_integer(fields.milliseconds), 3)),
				_ => {
					output.push(character);
					rest = &rest[character.len_utf8()..];
					continue;
				}
			};
			output.push_str(&text);
			rest = &rest[length..];
		}
		output
	}

	/// The Thursday of the ISO week of `time`.
	fn week_thursday(&self, time: f64) -> f64 {
		self.add(time, 4.0 - self.iso_weekday(time), "d")
	}

	fn iso_week_year(&self, time: f64) -> f64 {
		self.fields(self.week_thursday(time)).year
	}

	/// `isoWeek` of the `isoWeek` plugin.
	fn iso_week(&self, time: f64) -> f64 {
		let thursday = self.week_thursday(time);
		let year = self.iso_week_year(time);
		// `dayjs().year(year).startOf('year')`
		let year_start = self.make(&Fields {
			year,
			month: 0.0,
			date: 1.0,
			weekday: 0.0,
			hours: 0.0,
			minutes: 0.0,
			seconds: 0.0,
			milliseconds: 0.0,
		});
		let weekday = self.iso_weekday(year_start);
		let days = 4.0 - weekday + if weekday > 4.0 { 7.0 } else { 0.0 };
		let first_thursday = self.add(year_start, days, "d");
		// `diff(…, 'week')` corrects the difference by the change of the time zone offset.
		let zone_delta = (self.dayjs_utc_offset(first_thursday) - self.dayjs_utc_offset(thursday))
			* MS_PER_MINUTE;
		let weeks = (thursday - first_thursday - zone_delta) / (7.0 * MS_PER_DAY);
		(if weeks < 0.0 { weeks.ceil() } else { weeks.floor() }) + 1.0
	}

	/// `dayjs(input, format, true)`: the time, `NaN` where the input does not match the format
	/// exactly.
	pub fn parse_strict(&self, input: &str, format: &str) -> Result<f64, Exception> {
		let time = if format == "x" || format == "X" {
			let factor = if format == "X" { 1000.0 } else { 1.0 };
			super::date::time_clip(factor * js_number_of(input))
		} else {
			self.parse_format(input, format).unwrap_or(f64::NAN)
		};
		let formatted = self.format(time, format)?;
		Ok(if formatted == input { time } else { f64::NAN })
	}

	/// `parseFormattedInput` without its exceptions: `None` where dayjs throws and returns an
	/// invalid date.
	fn parse_format(&self, input: &str, format: &str) -> Option<f64> {
		let tokens = format_tokens(format)?;
		let mut parsed = ParsedInput::default();
		let mut input = input.to_owned();
		let mut start = 0;
		for token in &tokens {
			match token {
				FormatToken::Literal(text) => start += text.encode_utf16().count(),
				FormatToken::Value(kind) => {
					let part = utf16_slice(&input, start);
					let value = kind.find(part)?.to_owned();
					parsed.apply(*kind, &value)?;
					input = input.replacen(&value, "", 1);
				}
			}
		}
		if let Some(is_afternoon) = parsed.afternoon {
			let hours = parsed.hours.unwrap_or(f64::NAN);
			if is_afternoon {
				if hours < 12.0 {
					parsed.hours = Some(hours + 12.0);
				}
			} else if hours == 12.0 {
				parsed.hours = Some(0.0);
			}
		}
		if parsed.week.is_some_and(|week| week != 0.0 && !week.is_nan()) {
			// `dayjs(date).week(week)` needs the `weekOfYear` plugin.
			return None;
		}
		let now = self.fields(self.clock.now());
		let truthy = |value: Option<f64>| value.filter(|value| *value != 0.0 && !value.is_nan());
		let year = truthy(parsed.year);
		let month = truthy(parsed.month);
		let date = truthy(parsed.day).unwrap_or(if year.is_none() && month.is_none() {
			now.date
		} else {
			1.0
		});
		let full_year = year.unwrap_or(now.year);
		let month_index = if year.is_some() && month.is_none() {
			0.0
		} else {
			match parsed.month {
				Some(month) if month > 0.0 => month - 1.0,
				_ => now.month,
			}
		};
		let or_zero = |value: Option<f64>| truthy(value).unwrap_or(0.0);
		let fields = Fields {
			year: full_year,
			month: month_index,
			date,
			weekday: 0.0,
			hours: or_zero(parsed.hours),
			minutes: or_zero(parsed.minutes),
			seconds: or_zero(parsed.seconds),
			milliseconds: or_zero(parsed.milliseconds),
		};
		Some(match parsed.zone_offset {
			Some(offset) => super::date::utc(&Fields {
				milliseconds: fields.milliseconds + offset * 60.0 * 1000.0,
				..fields
			}),
			None => self.make(&fields),
		})
	}
}

/// The text of `text` from the UTF-16 index `start`, like JavaScript's `slice`.
fn utf16_slice(text: &str, start: usize) -> &str {
	let mut units = 0;
	for (index, character) in text.char_indices() {
		if units >= start {
			return &text[index..];
		}
		units += character.len_utf16();
	}
	""
}

/// The value tokens of `customParseFormat` and the regular expressions they match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ValueKind {
	UpperMeridiem,
	LowerMeridiem,
	Quarter,
	Tenths,
	Hundredths,
	Milliseconds,
	Seconds,
	Minutes,
	Hours,
	Day,
	TwoDigitDay,
	OrdinalDay,
	Week,
	TwoDigitWeek,
	Month,
	TwoDigitMonth,
	ShortMonthName,
	MonthName,
	SignedYear,
	TwoDigitYear,
	FourDigitYear,
	Zone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FormatToken {
	Literal(String),
	Value(ValueKind),
}

/// `[-_:/,()\s\d]`: the characters `matchWord` does not take.
fn is_word_break(character: char) -> bool {
	matches!(character, '-' | '_' | ':' | '/' | ',' | '(' | ')')
		|| character.is_whitespace()
		|| character.is_ascii_digit()
}

impl ValueKind {
	/// The first match of the token's unanchored regular expression in `text`.
	fn find(self, text: &str) -> Option<&str> {
		let digits_at = |index: usize| text[index..].bytes().take_while(u8::is_ascii_digit).count();
		let fixed_digits = |count: usize| {
			(0..text.len())
				.filter(|&index| text.is_char_boundary(index))
				.find(|&index| digits_at(index) >= count)
				.map(|index| &text[index..index + count])
		};
		match self {
			ValueKind::Quarter | ValueKind::Tenths => fixed_digits(1),
			ValueKind::Hundredths | ValueKind::TwoDigitDay | ValueKind::TwoDigitWeek => {
				fixed_digits(2)
			}
			ValueKind::TwoDigitMonth | ValueKind::TwoDigitYear => fixed_digits(2),
			ValueKind::Milliseconds => fixed_digits(3),
			ValueKind::FourDigitYear => fixed_digits(4),
			ValueKind::Seconds
			| ValueKind::Minutes
			| ValueKind::Hours
			| ValueKind::Day
			| ValueKind::Week
			| ValueKind::Month => {
				let index = text.find(|character: char| character.is_ascii_digit())?;
				let length = digits_at(index).min(2);
				Some(&text[index..index + length])
			}
			ValueKind::SignedYear => {
				let index = text.char_indices().find_map(|(index, character)| {
					let digits = if matches!(character, '+' | '-') { index + 1 } else { index };
					(text.is_char_boundary(digits) && digits_at(digits) > 0).then_some(index)
				})?;
				let sign = usize::from(matches!(text.as_bytes()[index], b'+' | b'-'));
				Some(&text[index..index + sign + digits_at(index + sign)])
			}
			ValueKind::Zone => {
				for (index, character) in text.char_indices() {
					if character == 'Z' {
						return Some(&text[index..index + 1]);
					}
					if matches!(character, '+' | '-') && digits_at(index + 1) >= 2 {
						let mut end = index + 3;
						if text[end..].starts_with(':') {
							end += 1;
						}
						if digits_at(end) >= 2 {
							end += 2;
						}
						return Some(&text[index..end]);
					}
				}
				None
			}
			ValueKind::UpperMeridiem
			| ValueKind::LowerMeridiem
			| ValueKind::OrdinalDay
			| ValueKind::ShortMonthName
			| ValueKind::MonthName => {
				// `\d*[^-_:/,()\s\d]+`
				for (index, _) in text.char_indices() {
					let after_digits = index + digits_at(index);
					let word = text[after_digits..]
						.char_indices()
						.find(|&(_, character)| is_word_break(character))
						.map_or(text.len() - after_digits, |(length, _)| length);
					if word > 0 {
						return Some(&text[index..after_digits + word]);
					}
				}
				None
			}
		}
	}
}

/// The fields `customParseFormat` collects.
#[derive(Default)]
struct ParsedInput {
	year: Option<f64>,
	month: Option<f64>,
	day: Option<f64>,
	hours: Option<f64>,
	minutes: Option<f64>,
	seconds: Option<f64>,
	milliseconds: Option<f64>,
	week: Option<f64>,
	afternoon: Option<bool>,
	/// Minutes to add to UTC.
	zone_offset: Option<f64>,
}

impl ParsedInput {
	/// The token's parser; `None` where it throws.
	fn apply(&mut self, kind: ValueKind, value: &str) -> Option<()> {
		let number = js_number_of(value);
		match kind {
			ValueKind::UpperMeridiem => self.afternoon = Some(value == "PM"),
			ValueKind::LowerMeridiem => self.afternoon = Some(value == "pm"),
			ValueKind::Quarter => self.month = Some((number - 1.0) * 3.0 + 1.0),
			ValueKind::Tenths => self.milliseconds = Some(number * 100.0),
			ValueKind::Hundredths => self.milliseconds = Some(number * 10.0),
			ValueKind::Milliseconds => self.milliseconds = Some(number),
			ValueKind::Seconds => self.seconds = Some(number),
			ValueKind::Minutes => self.minutes = Some(number),
			ValueKind::Hours => self.hours = Some(number),
			ValueKind::Day | ValueKind::TwoDigitDay => self.day = Some(number),
			ValueKind::OrdinalDay => {
				let start = value.find(|character: char| character.is_ascii_digit())?;
				let digits = value[start..].bytes().take_while(u8::is_ascii_digit).count();
				self.day = Some(js_number_of(&value[start..start + digits]));
				for day in 1..=31 {
					if ordinal(f64::from(day)) == value {
						self.day = Some(f64::from(day));
					}
				}
			}
			ValueKind::Week | ValueKind::TwoDigitWeek => self.week = Some(number),
			ValueKind::Month | ValueKind::TwoDigitMonth => self.month = Some(number),
			ValueKind::ShortMonthName | ValueKind::MonthName => {
				let index = MONTHS.iter().position(|month| match kind {
					ValueKind::ShortMonthName => &month[..3] == value,
					_ => *month == value,
				})?;
				self.month = Some(index as f64 + 1.0);
			}
			ValueKind::SignedYear | ValueKind::FourDigitYear => self.year = Some(number),
			ValueKind::TwoDigitYear => {
				self.year = Some(number + if number > 68.0 { 1900.0 } else { 2000.0 })
			}
			ValueKind::Zone => self.zone_offset = Some(zone_offset_of(value)),
		}
		Some(())
	}
}

/// `offsetFromString`: minutes to add to the local reading for UTC.
fn zone_offset_of(text: &str) -> f64 {
	if text.is_empty() || text == "Z" {
		return 0.0;
	}
	let sign = &text[..1];
	let digits: Vec<&str> = text[1..]
		.split(':')
		.flat_map(|part| (0..part.len() / 2).map(move |index| &part[index * 2..index * 2 + 2]))
		.collect();
	let hours = digits.first().map_or(f64::NAN, |hours| js_number_of(hours));
	let minutes = digits.get(1).map_or(0.0, |minutes| js_number_of(minutes));
	let total = hours * 60.0 + minutes;
	if total == 0.0 {
		0.0
	} else if sign == "+" {
		-total
	} else {
		total
	}
}

/// The tokens of `makeParser`'s format, `None` where dayjs throws: for localized formats (`L`,
/// `LT`, …), which the English locale's missing `formats` cannot expand.
fn format_tokens(format: &str) -> Option<Vec<FormatToken>> {
	let mut tokens = Vec::new();
	let mut rest = format;
	// `localizedFormat`'s `u` runs over the format first and throws on these.
	{
		let mut scan = format;
		while let Some(character) = scan.chars().next() {
			if character == '['
				&& let Some(end) = scan[1..].find(']')
				&& end > 0
			{
				scan = &scan[end + 2..];
				continue;
			}
			if matches!(character, 'L' | 'l') {
				return None;
			}
			scan = &scan[character.len_utf8()..];
		}
	}
	while let Some(character) = rest.chars().next() {
		// `\[[^[]*\]`: to the last `]` before the next `[`.
		if character == '[' {
			let until_bracket = rest[1..].find('[').map_or(rest.len(), |index| index + 1);
			if let Some(end) = rest[..until_bracket].rfind(']').filter(|&end| end > 0) {
				let inner = &rest[..end + 1];
				let text = inner.strip_prefix('[').unwrap_or(inner);
				let text = text.strip_suffix(']').unwrap_or(text);
				tokens.push(FormatToken::Literal(text.to_owned()));
				rest = &rest[end + 1..];
				continue;
			}
		}
		let separators = rest
			.char_indices()
			.find(|&(_, character)| {
				!(matches!(character, '-' | '_' | ':' | '/' | '.' | ',' | '(' | ')')
					|| character.is_whitespace())
			})
			.map_or(rest.len(), |(index, _)| index);
		if separators > 0 {
			tokens.push(FormatToken::Literal(rest[..separators].to_owned()));
			rest = &rest[separators..];
			continue;
		}
		let run = rest.chars().take_while(|&next| next == character).count();
		let (length, token) = match character {
			'A' => (1, Some(ValueKind::UpperMeridiem)),
			'a' => (1, Some(ValueKind::LowerMeridiem)),
			'Q' => (1, Some(ValueKind::Quarter)),
			'Y' if run >= 4 => (4, Some(ValueKind::FourDigitYear)),
			'Y' if run >= 2 => (2, Some(ValueKind::TwoDigitYear)),
			'Y' => (1, Some(ValueKind::SignedYear)),
			'w' if run >= 2 => (2, Some(ValueKind::TwoDigitWeek)),
			'w' => (1, Some(ValueKind::Week)),
			'M' => {
				let length = run.min(4);
				let kind = match length {
					1 => ValueKind::Month,
					2 => ValueKind::TwoDigitMonth,
					3 => ValueKind::ShortMonthName,
					_ => ValueKind::MonthName,
				};
				(length, Some(kind))
			}
			'D' if rest.starts_with("Do") => (2, Some(ValueKind::OrdinalDay)),
			'D' if run >= 2 => (2, Some(ValueKind::TwoDigitDay)),
			'D' => (1, Some(ValueKind::Day)),
			'h' | 'H' | 'm' | 's' => {
				let kind = match character {
					'h' | 'H' => ValueKind::Hours,
					'm' => ValueKind::Minutes,
					_ => ValueKind::Seconds,
				};
				(run.min(2), Some(kind))
			}
			'S' => {
				let length = run.min(3);
				let kind = match length {
					1 => ValueKind::Tenths,
					2 => ValueKind::Hundredths,
					_ => ValueKind::Milliseconds,
				};
				(length, Some(kind))
			}
			// A token without parser, which counts as text.
			'z' => {
				tokens.push(FormatToken::Literal("z".to_owned()));
				rest = &rest[1..];
				continue;
			}
			'Z' => (run.min(2), Some(ValueKind::Zone)),
			_ => (0, None),
		};
		match token {
			Some(kind) => {
				tokens.push(FormatToken::Value(kind));
				rest = &rest[length..];
			}
			// Text that no token matches is dropped and does not count.
			None => rest = &rest[character.len_utf8()..],
		}
	}
	// `format.match` is `null` without any token, and its `length` throws.
	(!tokens.is_empty()).then_some(tokens)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mermaid::FixedClock;

	const CLOCK: FixedClock = FixedClock { now: 1_700_000_000_000.0, utc_offset: 60.0 };

	fn calendar() -> Calendar<'static> {
		Calendar { clock: &CLOCK }
	}

	fn local(year: f64, month: f64, date: f64, hours: f64, minutes: f64) -> f64 {
		calendar().make(&Fields {
			year,
			month,
			date,
			weekday: 0.0,
			hours,
			minutes,
			seconds: 0.0,
			milliseconds: 0.0,
		})
	}

	#[test]
	fn parses_strictly() {
		let calendar = calendar();
		let parse = |input: &str, format: &str| calendar.parse_strict(input, format).unwrap();
		assert_eq!(parse("2014-01-06", "YYYY-MM-DD"), local(2014.0, 0.0, 6.0, 0.0, 0.0));
		assert!(parse("2014-1-6", "YYYY-MM-DD").is_nan());
		assert!(parse("2014-01-06 ", "YYYY-MM-DD").is_nan());
		assert_eq!(
			parse("2014-01-06 10:30", "YYYY-MM-DD HH:mm"),
			local(2014.0, 0.0, 6.0, 10.0, 30.0)
		);
		assert_eq!(parse("06.01.14", "DD.MM.YY"), local(2014.0, 0.0, 6.0, 0.0, 0.0));
		assert_eq!(parse("1:05 pm", "h:mm a"), {
			let now = calendar.fields(CLOCK.now);
			local(now.year, now.month, now.date, 13.0, 5.0)
		});
		assert_eq!(parse("Jan 6th 2014", "MMM Do YYYY"), local(2014.0, 0.0, 6.0, 0.0, 0.0));
		assert_eq!(parse("January 2014", "MMMM YYYY"), local(2014.0, 0.0, 1.0, 0.0, 0.0));
		assert!(parse("2014-01-06T10:00+02:00", "YYYY-MM-DDTHH:mmZ").is_nan());
		assert_eq!(parse("1389000000", "X"), 1_389_000_000_000.0);
		assert_eq!(parse("1389000000123", "x"), 1_389_000_000_123.0);
		assert!(parse("2014-01-06", "").is_nan());
		assert!(parse("01/06/2014", "L").is_nan());
		assert!(parse("2014-13-06", "YYYY-MM-DD").is_nan());
		assert!(calendar.parse_strict("2014-01-06", "YYYY-MM-DD gggg").is_err());
	}

	#[test]
	fn formats_like_dayjs() {
		let calendar = calendar();
		let time = local(2014.0, 0.0, 6.0, 0.0, 5.0);
		let format = |format: &str| calendar.format(time, format).unwrap();
		assert_eq!(format("YYYY-MM-DD HH:mm:ss.SSS"), "2014-01-06 00:05:00.000");
		assert_eq!(format("YY M D H h hh m s A a"), "14 1 6 0 12 12 5 0 AM am");
		assert_eq!(format("MMM MMMM d dd ddd dddd"), "Jan January 1 Mo Mon Monday");
		assert_eq!(format("Q Do W WW GGGG k kk X"), "1 6th 2 02 2014 24 24 1388963100");
		assert_eq!(format("Z ZZ [YYYY] Y"), "+01:00 +0100 YYYY Y");
		assert_eq!(format(""), "2014-01-06T00:05:00+01:00");
		assert!(calendar.format(time, "gggg").is_err());
		assert_eq!(calendar.format(f64::NAN, "gggg").unwrap(), "Invalid Date");
		assert_eq!(
			calendar.format(local(2020.0, 11.0, 31.0, 0.0, 0.0), "W GGGG").unwrap(),
			"53 2020"
		);
	}

	#[test]
	fn adds_and_compares_like_dayjs() {
		let calendar = calendar();
		let start = local(2014.0, 0.0, 31.0, 10.0, 0.0);
		assert_eq!(calendar.add(start, 1.0, "M"), local(2014.0, 1.0, 28.0, 10.0, 0.0));
		assert_eq!(calendar.add(start, 2.0, "y"), local(2016.0, 0.0, 31.0, 10.0, 0.0));
		assert_eq!(calendar.add(start, 1.5, "d"), local(2014.0, 1.0, 2.0, 10.0, 0.0));
		assert_eq!(calendar.add(start, 1.0, "w"), local(2014.0, 1.0, 7.0, 10.0, 0.0));
		assert_eq!(calendar.add(start, 1.5, "h"), local(2014.0, 0.0, 31.0, 11.0, 30.0));
		assert_eq!(calendar.start_of_day(start), local(2014.0, 0.0, 31.0, 0.0, 0.0));
		assert_eq!(calendar.end_of_day(start) - calendar.start_of_day(start), MS_PER_DAY - 1.0);
		assert_eq!(calendar.diff_years(local(2020.0, 0.0, 30.0, 0.0, 0.0), start), 5.0);
		assert_eq!(calendar.diff_years(local(2020.0, 0.0, 31.0, 10.0, 0.0), start), 6.0);
		assert_eq!(calendar.iso_weekday(local(2014.0, 0.0, 5.0, 0.0, 0.0)), 7.0);
	}
}
