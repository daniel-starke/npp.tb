//! JavaScript `Date`s in the local time zone, which Mermaid's gantt charts, dayjs and d3-time
//! compute with, and V8's `Date.parse`. Times are milliseconds since the Unix epoch as `f64`,
//! `NaN` for an invalid date, like JavaScript's time values.

use super::super::Clock;

pub const MS_PER_SECOND: f64 = 1000.0;
pub const MS_PER_MINUTE: f64 = 60_000.0;
pub const MS_PER_HOUR: f64 = 3_600_000.0;
pub const MS_PER_DAY: f64 = 86_400_000.0;
/// The largest time value: ±100,000,000 days.
const MAX_TIME: f64 = 8.64e15;

/// The calendar fields of a time; all `NaN` for an invalid date.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fields {
	pub year: f64,
	/// 0 for January.
	pub month: f64,
	/// The day of the month, from 1.
	pub date: f64,
	/// 0 for Sunday.
	pub weekday: f64,
	pub hours: f64,
	pub minutes: f64,
	pub seconds: f64,
	pub milliseconds: f64,
}

/// Days since 1970-01-01 of a proleptic Gregorian date (month from 0).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
	let year = if month <= 1 { year - 1 } else { year };
	let era = year.div_euclid(400);
	let year_of_era = year - era * 400;
	let month_from_march = (month + 10) % 12;
	let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
	let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
	era * 146_097 + day_of_era - 719_468
}

/// Year, month from 0 and day of the month of days since 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
	let days = days + 719_468;
	let era = days.div_euclid(146_097);
	let day_of_era = days - era * 146_097;
	let year_of_era =
		(day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let month_from_march = (5 * day_of_year + 2) / 153;
	let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
	let month = (month_from_march + 2) % 12;
	let year = year_of_era + era * 400 + i64::from(month <= 1);
	(year, month, day)
}

/// ECMAScript's `MakeDay`: days since the epoch, months and days may overflow.
pub fn make_day(year: f64, month: f64, date: f64) -> f64 {
	if !(year.is_finite() && month.is_finite() && date.is_finite()) {
		return f64::NAN;
	}
	let (year, month, date) = (year.trunc(), month.trunc(), date.trunc());
	let whole_year = year + (month / 12.0).floor();
	// Beyond this no time value is valid anyway, and the integer arithmetic stays exact.
	if whole_year.abs() > 400_000.0 {
		return f64::NAN;
	}
	let month_of_year = month.rem_euclid(12.0);
	days_from_civil(whole_year as i64, month_of_year as i64, 1) as f64 + date - 1.0
}

/// ECMAScript's `MakeTime`.
pub fn make_time(hours: f64, minutes: f64, seconds: f64, milliseconds: f64) -> f64 {
	if !(hours.is_finite() && minutes.is_finite() && seconds.is_finite())
		|| !milliseconds.is_finite()
	{
		return f64::NAN;
	}
	hours.trunc() * MS_PER_HOUR
		+ minutes.trunc() * MS_PER_MINUTE
		+ seconds.trunc() * MS_PER_SECOND
		+ milliseconds.trunc()
}

/// ECMAScript's `MakeDate`.
pub fn make_date(day: f64, time: f64) -> f64 {
	let date = day * MS_PER_DAY + time;
	if date.is_finite() { date } else { f64::NAN }
}

/// ECMAScript's `TimeClip`.
pub fn time_clip(time: f64) -> f64 {
	if !time.is_finite() || time.abs() > MAX_TIME { f64::NAN } else { time.trunc() + 0.0 }
}

/// The calendar fields of `time` without time zone.
pub fn fields_of(time: f64) -> Fields {
	if !time.is_finite() {
		let nan = f64::NAN;
		return Fields {
			year: nan,
			month: nan,
			date: nan,
			weekday: nan,
			hours: nan,
			minutes: nan,
			seconds: nan,
			milliseconds: nan,
		};
	}
	let days = (time / MS_PER_DAY).floor();
	let within_day = time - days * MS_PER_DAY;
	let (year, month, date) = civil_from_days(days as i64);
	Fields {
		year: year as f64,
		month: month as f64,
		date: date as f64,
		weekday: (days + 4.0).rem_euclid(7.0),
		hours: (within_day / MS_PER_HOUR).floor(),
		minutes: (within_day / MS_PER_MINUTE).floor() % 60.0,
		seconds: (within_day / MS_PER_SECOND).floor() % 60.0,
		milliseconds: within_day % MS_PER_SECOND,
	}
}

/// `Date.UTC`
pub fn utc(fields: &Fields) -> f64 {
	time_clip(make_date(
		make_day(fields.year, fields.month, fields.date),
		make_time(fields.hours, fields.minutes, fields.seconds, fields.milliseconds),
	))
}

/// Dates in the local time zone of a [`Clock`].
#[derive(Clone, Copy)]
pub struct Calendar<'a> {
	pub clock: &'a dyn Clock,
}

impl Calendar<'_> {
	/// Milliseconds the local time is ahead of UTC at `time`.
	fn offset(&self, time: f64) -> f64 {
		let minutes = self.clock.utc_offset(time);
		if minutes.is_finite() { minutes * MS_PER_MINUTE } else { 0.0 }
	}

	/// `getTimezoneOffset`: minutes UTC is ahead of the local time.
	pub fn timezone_offset(&self, time: f64) -> f64 {
		if time.is_nan() { f64::NAN } else { -self.offset(time) / MS_PER_MINUTE }
	}

	/// The local calendar fields of `time`.
	pub fn fields(&self, time: f64) -> Fields {
		if time.is_nan() {
			return fields_of(time);
		}
		fields_of(time + self.offset(time))
	}

	/// The UTC time of a local time. Like V8 with ICU, a local time that occurs twice takes the
	/// offset before the transition, and one skipped by a transition moves forward.
	fn utc_of_local(&self, local: f64) -> f64 {
		if !local.is_finite() {
			return f64::NAN;
		}
		let before = self.offset(local - MS_PER_DAY);
		let after = self.offset(local + MS_PER_DAY);
		let with_before = local - before;
		if before == after {
			return with_before;
		}
		let with_after = local - after;
		let is_before_valid = self.offset(with_before) == before;
		let is_after_valid = self.offset(with_after) == after;
		if !is_before_valid && is_after_valid { with_after } else { with_before }
	}

	/// `new Date(year, month, date, hours, minutes, seconds, milliseconds)`
	pub fn make(&self, fields: &Fields) -> f64 {
		let local = make_date(
			make_day(fields.year, fields.month, fields.date),
			make_time(fields.hours, fields.minutes, fields.seconds, fields.milliseconds),
		);
		time_clip(self.utc_of_local(local))
	}

	/// The local fields of `time` changed by `change`, as a time again: the `set…` methods of
	/// `Date`. An invalid date stays invalid.
	pub fn with_fields(&self, time: f64, change: impl FnOnce(&mut Fields)) -> f64 {
		if time.is_nan() {
			return f64::NAN;
		}
		let mut fields = self.fields(time);
		change(&mut fields);
		self.make(&fields)
	}

	/// `setFullYear`, which starts from the epoch for an invalid date.
	pub fn set_full_year(&self, time: f64, year: f64) -> f64 {
		let time = if time.is_nan() { 0.0 } else { time };
		self.with_fields(time, |fields| fields.year = year)
	}

	/// `Date.parse` of V8.
	pub fn parse(&self, text: &str) -> f64 {
		let Some(parsed) = parse_date_time(text) else { return f64::NAN };
		let day = make_day(parsed.year, parsed.month, parsed.day);
		let time = make_time(parsed.hour, parsed.minute, parsed.second, parsed.millisecond);
		let date = make_date(day, time);
		match parsed.utc_offset {
			// V8 rejects local times this far out before converting them.
			None if date.abs() > MAX_TIME + 864_000_000.0 => f64::NAN,
			None => time_clip(self.utc_of_local(date)),
			Some(seconds) => time_clip(date - seconds * MS_PER_SECOND),
		}
	}
}

/// The result of V8's `DateParser`.
#[derive(Debug, PartialEq)]
struct ParsedDate {
	year: f64,
	month: f64,
	day: f64,
	hour: f64,
	minute: f64,
	second: f64,
	millisecond: f64,
	/// Seconds ahead of UTC; local time without.
	utc_offset: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeywordKind {
	MonthName,
	AmPm,
	TimeZoneName,
	TimeSeparator,
	Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateToken {
	Invalid,
	Unknown,
	Number { value: i64, length: usize },
	Symbol(char),
	WhiteSpace,
	Keyword { kind: KeywordKind, value: i64 },
	EndOfInput,
}

impl DateToken {
	fn is_number(self) -> bool {
		matches!(self, DateToken::Number { .. })
	}

	fn is_fixed_length_number(self, wanted: usize) -> bool {
		matches!(self, DateToken::Number { length, .. } if length == wanted)
	}

	fn number(self) -> i64 {
		match self {
			DateToken::Number { value, .. } => value,
			_ => 0,
		}
	}

	fn is_symbol(self, symbol: char) -> bool {
		self == DateToken::Symbol(symbol)
	}

	fn is_ascii_sign(self) -> bool {
		self.is_symbol('+') || self.is_symbol('-')
	}

	fn is_keyword_z(self) -> bool {
		// Only the one-letter word `z` is this keyword.
		self == DateToken::Keyword { kind: KeywordKind::TimeZoneName, value: 0 }
	}
}

/// V8's `KeywordTable`: the first three letters of a word, then its kind and value. Longer
/// words match only month names.
const KEYWORDS: [(&str, KeywordKind, i64); 25] = [
	("jan", KeywordKind::MonthName, 1),
	("feb", KeywordKind::MonthName, 2),
	("mar", KeywordKind::MonthName, 3),
	("apr", KeywordKind::MonthName, 4),
	("may", KeywordKind::MonthName, 5),
	("jun", KeywordKind::MonthName, 6),
	("jul", KeywordKind::MonthName, 7),
	("aug", KeywordKind::MonthName, 8),
	("sep", KeywordKind::MonthName, 9),
	("oct", KeywordKind::MonthName, 10),
	("nov", KeywordKind::MonthName, 11),
	("dec", KeywordKind::MonthName, 12),
	("am", KeywordKind::AmPm, 0),
	("pm", KeywordKind::AmPm, 12),
	("ut", KeywordKind::TimeZoneName, 0),
	("utc", KeywordKind::TimeZoneName, 0),
	("z", KeywordKind::TimeZoneName, 0),
	("gmt", KeywordKind::TimeZoneName, 0),
	("cdt", KeywordKind::TimeZoneName, -5),
	("cst", KeywordKind::TimeZoneName, -6),
	("edt", KeywordKind::TimeZoneName, -4),
	("est", KeywordKind::TimeZoneName, -5),
	("mdt", KeywordKind::TimeZoneName, -6),
	("mst", KeywordKind::TimeZoneName, -7),
	("pdt", KeywordKind::TimeZoneName, -7),
];

/// The keyword `t` and `pst`, which do not fit [`KEYWORDS`]' size.
fn keyword_of(prefix: &str, length: usize) -> (KeywordKind, i64) {
	let extra = [("pst", KeywordKind::TimeZoneName, -8), ("t", KeywordKind::TimeSeparator, 0)];
	for &(word, kind, value) in KEYWORDS.iter().chain(extra.iter()) {
		if word == prefix && (length <= 3 || kind == KeywordKind::MonthName) {
			return (kind, value);
		}
	}
	(KeywordKind::Invalid, 0)
}

/// V8's `DateStringTokenizer`.
struct Tokenizer<'a> {
	characters: std::iter::Peekable<std::str::Chars<'a>>,
	next: DateToken,
}

impl<'a> Tokenizer<'a> {
	fn new(text: &'a str) -> Self {
		let mut tokenizer =
			Tokenizer { characters: text.chars().peekable(), next: DateToken::Unknown };
		tokenizer.next = tokenizer.scan();
		tokenizer
	}

	fn scan(&mut self) -> DateToken {
		let Some(&first) = self.characters.peek() else { return DateToken::EndOfInput };
		if first.is_ascii_digit() {
			let mut value = 0i64;
			let mut length = 0;
			while let Some(digit) = self.characters.peek().and_then(|c| c.to_digit(10)) {
				// V8 stops accumulating near `i32::MAX`.
				if value < i64::from(i32::MAX) / 10 - 10 {
					value = value * 10 + i64::from(digit);
				}
				length += 1;
				self.characters.next();
			}
			return DateToken::Number { value, length };
		}
		if matches!(first, ':' | '-' | '+' | '.' | ')') {
			self.characters.next();
			return DateToken::Symbol(first);
		}
		if first >= 'A' && !first.is_whitespace() {
			let mut prefix = String::new();
			let mut length = 0;
			while let Some(&character) = self.characters.peek() {
				if character < 'A' || character.is_whitespace() {
					break;
				}
				if length < 3 {
					prefix.extend(character.to_lowercase());
				}
				length += 1;
				self.characters.next();
			}
			let (kind, value) = keyword_of(&prefix, length);
			return DateToken::Keyword { kind, value };
		}
		if first.is_whitespace() {
			while self.characters.peek().is_some_and(|character| character.is_whitespace()) {
				self.characters.next();
			}
			return DateToken::WhiteSpace;
		}
		if first == '(' {
			let mut depth = 0;
			for character in self.characters.by_ref() {
				match character {
					'(' => depth += 1,
					')' => depth -= 1,
					_ => {}
				}
				if depth == 0 {
					break;
				}
			}
			return DateToken::Unknown;
		}
		self.characters.next();
		DateToken::Unknown
	}

	fn next(&mut self) -> DateToken {
		let token = self.next;
		self.next = self.scan();
		token
	}

	fn peek(&self) -> DateToken {
		self.next
	}

	fn skip_symbol(&mut self, symbol: char) -> bool {
		if self.next.is_symbol(symbol) {
			self.next();
			true
		} else {
			false
		}
	}
}

/// V8's `DayComposer`.
#[derive(Default)]
struct DayComposer {
	components: Vec<i64>,
	named_month: Option<i64>,
	is_iso_date: bool,
}

impl DayComposer {
	fn add(&mut self, value: i64) -> bool {
		if self.components.len() < 3 {
			self.components.push(value);
			true
		} else {
			false
		}
	}

	fn write(&mut self) -> Option<(f64, f64, f64)> {
		if self.components.is_empty() {
			return None;
		}
		while self.components.len() < 3 {
			self.components.push(1);
		}
		let components = &self.components;
		let is_day = |value: i64| (1..=31).contains(&value);
		let (mut year, month, day) = match self.named_month {
			None if self.is_iso_date || !is_day(components[0]) => {
				(components[0], components[1], components[2])
			}
			None => (components[2], components[0], components[1]),
			Some(month) if !is_day(components[0]) => (components[0], month, components[1]),
			Some(month) => (components[1], month, components[0]),
		};
		if !self.is_iso_date {
			if (0..=49).contains(&year) {
				year += 2000;
			} else if (50..=99).contains(&year) {
				year += 1900;
			}
		}
		if !(1..=12).contains(&month) || !is_day(day) {
			return None;
		}
		Some((year as f64, (month - 1) as f64, day as f64))
	}
}

/// V8's `TimeComposer`.
#[derive(Default)]
struct TimeComposer {
	components: Vec<i64>,
	hour_offset: Option<i64>,
}

impl TimeComposer {
	fn is_empty(&self) -> bool {
		self.components.is_empty()
	}

	fn is_expecting(&self, value: i64) -> bool {
		match self.components.len() {
			1 | 2 => (0..=59).contains(&value),
			3 => (0..=999).contains(&value),
			_ => false,
		}
	}

	fn add(&mut self, value: i64) -> bool {
		if self.components.len() < 4 {
			self.components.push(value);
			true
		} else {
			false
		}
	}

	fn add_final(&mut self, value: i64) -> bool {
		if !self.add(value) {
			return false;
		}
		while self.components.len() < 4 {
			self.components.push(0);
		}
		true
	}

	fn write(&mut self) -> Option<[f64; 4]> {
		while self.components.len() < 4 {
			self.components.push(0);
		}
		let (mut hour, minute, second, millisecond) =
			(self.components[0], self.components[1], self.components[2], self.components[3]);
		if let Some(offset) = self.hour_offset {
			if !(0..=12).contains(&hour) {
				return None;
			}
			hour = hour % 12 + offset;
		}
		let is_valid = (0..=23).contains(&hour)
			&& (0..=59).contains(&minute)
			&& (0..=59).contains(&second)
			&& (0..=999).contains(&millisecond);
		if !is_valid && (hour != 24 || minute != 0 || second != 0 || millisecond != 0) {
			return None;
		}
		Some([hour as f64, minute as f64, second as f64, millisecond as f64])
	}
}

/// V8's `TimeZoneComposer`.
#[derive(Default)]
struct TimeZoneComposer {
	sign: Option<i64>,
	hour: Option<i64>,
	minute: Option<i64>,
}

impl TimeZoneComposer {
	fn set(&mut self, offset_in_hours: i64) {
		let sign = if offset_in_hours < 0 { -1 } else { 1 };
		self.sign = Some(sign);
		self.hour = Some(offset_in_hours * sign);
		self.minute = Some(0);
	}

	fn is_expecting(&self, value: i64) -> bool {
		self.hour.is_some() && self.minute.is_none() && (0..=59).contains(&value)
	}

	fn is_utc(&self) -> bool {
		self.hour == Some(0) && self.minute == Some(0)
	}

	fn write(&self) -> Option<Option<f64>> {
		let Some(sign) = self.sign else { return Some(None) };
		let seconds = self.hour.unwrap_or(0) * 3600 + self.minute.unwrap_or(0) * 60;
		if !(0..=i64::from(i32::MAX)).contains(&seconds) {
			return None;
		}
		Some(Some((sign * seconds) as f64))
	}
}

/// V8's `ReadMilliseconds`: the first three significant digits of a numeral.
fn read_milliseconds(token: DateToken) -> i64 {
	let DateToken::Number { mut value, length } = token else { return 0 };
	match length {
		1 => value * 100,
		2 => value * 10,
		3 => value,
		_ => {
			for _ in 3..length.min(9) {
				value /= 10;
			}
			value
		}
	}
}

/// V8's `ParseES5DateTime`: the first token it does not handle, `Invalid` for an invalid date.
fn parse_es5_date_time(
	scanner: &mut Tokenizer,
	day: &mut DayComposer,
	time: &mut TimeComposer,
	time_zone: &mut TimeZoneComposer,
) -> DateToken {
	if scanner.peek().is_ascii_sign() {
		let sign_token = scanner.next();
		if !scanner.peek().is_fixed_length_number(6) {
			return sign_token;
		}
		let sign = if sign_token.is_symbol('-') { -1 } else { 1 };
		let year = scanner.next().number();
		if sign < 0 && year == 0 {
			return sign_token;
		}
		day.add(sign * year);
	} else if scanner.peek().is_fixed_length_number(4) {
		day.add(scanner.next().number());
	} else {
		return scanner.next();
	}
	if scanner.skip_symbol('-') {
		if !scanner.peek().is_fixed_length_number(2) || !(1..=12).contains(&scanner.peek().number())
		{
			return scanner.next();
		}
		day.add(scanner.next().number());
		if scanner.skip_symbol('-') {
			if !scanner.peek().is_fixed_length_number(2)
				|| !(1..=31).contains(&scanner.peek().number())
			{
				return scanner.next();
			}
			day.add(scanner.next().number());
		}
	}
	let is_time_separator =
		matches!(scanner.peek(), DateToken::Keyword { kind: KeywordKind::TimeSeparator, .. });
	if !is_time_separator {
		if scanner.peek() != DateToken::EndOfInput {
			return scanner.next();
		}
	} else {
		scanner.next();
		if !scanner.peek().is_fixed_length_number(2) || !(0..=24).contains(&scanner.peek().number())
		{
			return DateToken::Invalid;
		}
		let is_hour_24 = scanner.peek().number() == 24;
		time.add(scanner.next().number());
		if !scanner.skip_symbol(':') {
			return DateToken::Invalid;
		}
		let is_valid_part = |token: DateToken| {
			token.is_fixed_length_number(2)
				&& (0..=59).contains(&token.number())
				&& !(is_hour_24 && token.number() > 0)
		};
		if !is_valid_part(scanner.peek()) {
			return DateToken::Invalid;
		}
		time.add(scanner.next().number());
		if scanner.skip_symbol(':') {
			if !is_valid_part(scanner.peek()) {
				return DateToken::Invalid;
			}
			time.add(scanner.next().number());
			if scanner.skip_symbol('.') {
				if !scanner.peek().is_number() || (is_hour_24 && scanner.peek().number() > 0) {
					return DateToken::Invalid;
				}
				time.add(read_milliseconds(scanner.next()));
			}
		}
		if scanner.peek().is_keyword_z() {
			scanner.next();
			time_zone.set(0);
		} else if scanner.peek().is_ascii_sign() {
			time_zone.sign = Some(if scanner.next().is_symbol('+') { 1 } else { -1 });
			if scanner.peek().is_fixed_length_number(4) {
				let hours_and_minutes = scanner.next().number();
				let (hour, minute) = (hours_and_minutes / 100, hours_and_minutes % 100);
				if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
					return DateToken::Invalid;
				}
				time_zone.hour = Some(hour);
				time_zone.minute = Some(minute);
			} else {
				if !scanner.peek().is_fixed_length_number(2)
					|| !(0..=23).contains(&scanner.peek().number())
				{
					return DateToken::Invalid;
				}
				time_zone.hour = Some(scanner.next().number());
				if !scanner.skip_symbol(':') {
					return DateToken::Invalid;
				}
				if !scanner.peek().is_fixed_length_number(2)
					|| !(0..=59).contains(&scanner.peek().number())
				{
					return DateToken::Invalid;
				}
				time_zone.minute = Some(scanner.next().number());
			}
		}
		if scanner.peek() != DateToken::EndOfInput {
			return DateToken::Invalid;
		}
	}
	// Date-only forms without offset are UTC, date-time forms local.
	if time_zone.hour.is_none() && time.is_empty() {
		time_zone.set(0);
	}
	day.is_iso_date = true;
	DateToken::EndOfInput
}

/// V8's `DateParser::Parse`: ES5 ISO date-time strings, then the legacy formats.
fn parse_date_time(text: &str) -> Option<ParsedDate> {
	let mut scanner = Tokenizer::new(text);
	let mut day = DayComposer::default();
	let mut time = TimeComposer::default();
	let mut time_zone = TimeZoneComposer::default();
	let mut token = parse_es5_date_time(&mut scanner, &mut day, &mut time, &mut time_zone);
	if token == DateToken::Invalid {
		return None;
	}
	let mut has_read_number = !day.components.is_empty();
	while token != DateToken::EndOfInput {
		match token {
			DateToken::Number { value, .. } => {
				has_read_number = true;
				if scanner.skip_symbol(':') {
					if scanner.skip_symbol(':') {
						if !time.is_empty() {
							return None;
						}
						time.add(value);
						time.add(0);
					} else {
						if !time.add(value) {
							return None;
						}
						if scanner.peek().is_symbol('.') {
							scanner.next();
						}
					}
				} else if scanner.skip_symbol('.') && time.is_expecting(value) {
					// V8 skips the `.` before it checks the time, so other numbers lose it too.
					time.add(value);
					if !scanner.peek().is_number() {
						return None;
					}
					let milliseconds = read_milliseconds(scanner.next());
					time.add_final(milliseconds);
				} else if time_zone.is_expecting(value) {
					time_zone.minute = Some(value);
				} else if time.is_expecting(value) {
					time.add_final(value);
					let peek = scanner.peek();
					if peek != DateToken::EndOfInput
						&& peek != DateToken::WhiteSpace
						&& !peek.is_keyword_z()
						&& !peek.is_ascii_sign()
					{
						return None;
					}
				} else {
					if !day.add(value) {
						return None;
					}
					scanner.skip_symbol('-');
				}
			}
			DateToken::Keyword { kind, value } => {
				if kind == KeywordKind::AmPm && !time.is_empty() {
					time.hour_offset = Some(value);
				} else if kind == KeywordKind::MonthName {
					day.named_month = Some(value);
					scanner.skip_symbol('-');
				} else if kind == KeywordKind::TimeZoneName && has_read_number {
					time_zone.set(value);
				} else {
					if has_read_number || scanner.peek().is_number() {
						return None;
					}
				}
			}
			_ if token.is_ascii_sign() && (time_zone.is_utc() || !time.is_empty()) => {
				time_zone.sign = Some(if token.is_symbol('+') { 1 } else { -1 });
				let (mut number, mut length) = (0, 0);
				if let DateToken::Number { value, length: digits } = scanner.peek() {
					scanner.next();
					(number, length) = (value, digits);
				}
				has_read_number = true;
				if scanner.peek().is_symbol(':') {
					time_zone.hour = Some(number);
					time_zone.minute = None;
				} else if length == 1 || length == 2 {
					time_zone.hour = Some(number);
					time_zone.minute = Some(0);
				} else if length == 3 || length == 4 {
					time_zone.hour = Some(number / 100);
					time_zone.minute = Some(number % 100);
				} else {
					return None;
				}
			}
			_ if (token.is_ascii_sign() || token.is_symbol(')')) && has_read_number => {
				return None;
			}
			_ => {}
		}
		token = scanner.next();
	}
	let (year, month, day) = day.write()?;
	let [hour, minute, second, millisecond] = time.write()?;
	let utc_offset = time_zone.write()?;
	Some(ParsedDate { year, month, day, hour, minute, second, millisecond, utc_offset })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mermaid::FixedClock;

	const CLOCK: FixedClock = FixedClock { now: 0.0, utc_offset: 0.0 };

	fn parse(text: &str) -> f64 {
		Calendar { clock: &CLOCK }.parse(text)
	}

	#[test]
	fn converts_between_days_and_dates() {
		for days in [-800_000, -719_468, -1, 0, 1, 59, 365, 11_016, 2_932_896] {
			let (year, month, day) = civil_from_days(days);
			assert_eq!(days_from_civil(year, month, day), days);
		}
		assert_eq!(civil_from_days(0), (1970, 0, 1));
		assert_eq!(make_day(2014.0, 0.0, 1.0), 16_071.0);
		assert_eq!(make_day(2013.0, 12.0, 1.0), 16_071.0);
		assert_eq!(make_day(2014.0, -1.0, 32.0), 16_071.0);
		assert!(make_day(f64::NAN, 0.0, 1.0).is_nan());
		let fields = fields_of(1_388_534_400_000.0 + 3_723_004.0);
		assert_eq!(
			(fields.year, fields.month, fields.date, fields.weekday),
			(2014.0, 0.0, 1.0, 3.0)
		);
		assert_eq!(
			(fields.hours, fields.minutes, fields.seconds, fields.milliseconds),
			(1.0, 2.0, 3.0, 4.0)
		);
		assert_eq!(fields_of(-1.0).year, 1969.0);
	}

	#[test]
	fn parses_dates_like_v8() {
		let day = 1_388_534_400_000.0;
		assert_eq!(parse("2014-01-01"), day);
		assert_eq!(parse("2014-01"), day);
		assert_eq!(parse("2014"), day);
		assert_eq!(parse("2014-01-01T01:00"), day + MS_PER_HOUR);
		assert_eq!(parse("2014-01-01T01:00Z"), day + MS_PER_HOUR);
		assert_eq!(parse("2014-01-01T01:00+01:00"), day);
		assert_eq!(parse("2014-01-01T01:00:00.5"), day + MS_PER_HOUR + 500.0);
		assert_eq!(parse("2014-01-01 01:00"), day + MS_PER_HOUR);
		assert_eq!(parse("2014/01/01"), day);
		assert_eq!(parse("1/1/2014"), day);
		assert_eq!(parse("2014-1-1"), day);
		assert_eq!(parse("Jan 1 2014"), day);
		assert_eq!(parse("1 January 2014"), day);
		assert_eq!(parse("Wed, 01 Jan 2014 01:00:00 GMT"), day + MS_PER_HOUR);
		assert_eq!(parse("Jan 1 2014 1:00 pm"), day + 13.0 * MS_PER_HOUR);
		assert_eq!(parse("Jan 1 2014 10:00 GMT+0200"), day + 8.0 * MS_PER_HOUR);
		assert_eq!(parse("Jan 5"), 978_652_800_000.0);
		assert_eq!(parse("+002014-01-01"), day);
		for invalid in
			["", "x", "2014-13-01", "2014-01-01T25:00", "1 2 3 4", "Jan 1 2014 x", "-000000"]
		{
			assert!(parse(invalid).is_nan(), "{invalid}");
		}
	}

	#[test]
	fn converts_local_times_around_transitions() {
		struct Europe;
		impl Clock for Europe {
			fn now(&self) -> f64 {
				0.0
			}
			// Central European time with summer time in 2024: 31 March 01:00 UTC to
			// 27 October 01:00 UTC.
			fn utc_offset(&self, time: f64) -> f64 {
				if (1_711_846_800_000.0..1_729_990_800_000.0).contains(&time) {
					120.0
				} else {
					60.0
				}
			}
		}
		let calendar = Calendar { clock: &Europe };
		let local = |month, date, hours, minutes| {
			calendar.make(&Fields {
				year: 2024.0,
				month,
				date,
				weekday: 0.0,
				hours,
				minutes,
				seconds: 0.0,
				milliseconds: 0.0,
			})
		};
		// 02:30 does not exist and moves forward, 02:30 in October takes summer time.
		assert_eq!(local(2.0, 31.0, 2.0, 30.0), 1_711_848_600_000.0);
		assert_eq!(calendar.fields(local(2.0, 31.0, 2.0, 30.0)).hours, 3.0);
		assert_eq!(local(9.0, 27.0, 2.0, 30.0), 1_729_989_000_000.0);
		assert_eq!(local(2.0, 31.0, 0.0, 0.0), 1_711_839_600_000.0);
		assert_eq!(calendar.timezone_offset(local(5.0, 1.0, 0.0, 0.0)), -120.0);
		let next_day = calendar.with_fields(local(2.0, 30.0, 12.0, 0.0), |f| f.date += 1.0);
		assert_eq!(next_day - local(2.0, 30.0, 12.0, 0.0), 23.0 * MS_PER_HOUR);
	}
}
