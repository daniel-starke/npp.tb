//! The local time intervals and ticks of d3-time 3.1 and the formatting of d3-time-format 4.1
//! (`en-US`), which the gantt chart's time axis uses.

use super::date::{Calendar, MS_PER_DAY, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND};

/// The most steps a filtered interval takes to find its next date. d3 has no limit; huge
/// intervals of a few units only, so this only stops pathological input.
const MAX_FILTER_STEPS: usize = 100_000;
/// The most ticks [`Interval::range`] returns. d3 has no limit.
pub const MAX_TICKS: usize = 10_000;

const DURATION_WEEK: f64 = MS_PER_DAY * 7.0;
const DURATION_MONTH: f64 = MS_PER_DAY * 30.0;
const DURATION_YEAR: f64 = MS_PER_DAY * 365.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
	Millisecond,
	Second,
	Minute,
	Hour,
	Day,
	/// Weeks starting on the given day, 0 for Sunday.
	Week(u8),
	Month,
	Year,
}

/// A d3 time interval: a unit, every `step` of them. Steps above one are d3's `every`: a filter
/// on the unit's field, or on the count since the epoch for weeks; milliseconds and years have
/// their own arithmetic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
	pub unit: Unit,
	pub step: f64,
}

impl Interval {
	pub fn new(unit: Unit) -> Self {
		Interval { unit, step: 1.0 }
	}

	/// `interval.every(step)`
	pub fn every(unit: Unit, step: f64) -> Option<Self> {
		let step = step.floor();
		(step.is_finite() && step > 0.0).then_some(Interval { unit, step })
	}

	fn is_filtered(&self) -> bool {
		self.step > 1.0 && !matches!(self.unit, Unit::Millisecond | Unit::Year)
	}

	/// The `floori` of the unit itself.
	fn floor_unit(&self, calendar: &Calendar, time: f64) -> f64 {
		match self.unit {
			Unit::Millisecond if self.step > 1.0 => (time / self.step).floor() * self.step,
			Unit::Millisecond => time,
			Unit::Second => time - calendar.fields(time).milliseconds,
			Unit::Minute => {
				let fields = calendar.fields(time);
				time - fields.milliseconds - fields.seconds * MS_PER_SECOND
			}
			Unit::Hour => {
				let fields = calendar.fields(time);
				time - fields.milliseconds
					- fields.seconds * MS_PER_SECOND
					- fields.minutes * MS_PER_MINUTE
			}
			Unit::Day => calendar.start_of_day(time),
			Unit::Week(weekday) => {
				let fields = calendar.fields(time);
				let back = (fields.weekday + 7.0 - f64::from(weekday)) % 7.0;
				let moved = calendar.with_fields(time, |fields| fields.date -= back);
				calendar.start_of_day(moved)
			}
			Unit::Month => {
				let first = calendar.with_fields(time, |fields| fields.date = 1.0);
				calendar.start_of_day(first)
			}
			Unit::Year => {
				let time = if self.step > 1.0 {
					let year = calendar.fields(time).year;
					calendar.set_full_year(time, (year / self.step).floor() * self.step)
				} else {
					time
				};
				let first = calendar.with_fields(time, |fields| {
					fields.month = 0.0;
					fields.date = 1.0;
				});
				calendar.start_of_day(first)
			}
		}
	}

	/// The `offseti` of the unit itself.
	fn offset_unit(&self, calendar: &Calendar, time: f64, step: f64) -> f64 {
		match self.unit {
			Unit::Millisecond if self.step > 1.0 => time + step * self.step,
			Unit::Millisecond => time + step,
			Unit::Second => time + step * MS_PER_SECOND,
			Unit::Minute => time + step * MS_PER_MINUTE,
			Unit::Hour => time + step * MS_PER_HOUR,
			Unit::Day => calendar.with_fields(time, |fields| fields.date += step),
			Unit::Week(_) => calendar.with_fields(time, |fields| fields.date += step * 7.0),
			Unit::Month => calendar.with_fields(time, |fields| fields.month += step),
			Unit::Year => {
				let years = if self.step > 1.0 { step * self.step } else { step };
				let year = calendar.fields(time).year;
				calendar.set_full_year(time, year + years)
			}
		}
	}

	/// `interval.count(start, end)` of the unit itself.
	fn count_unit(&self, calendar: &Calendar, start: f64, end: f64) -> f64 {
		let unit = Interval::new(self.unit);
		let (start, end) = (unit.floor_unit(calendar, start), unit.floor_unit(calendar, end));
		let zone_change = |end: f64, start: f64| {
			(calendar.timezone_offset(end) - calendar.timezone_offset(start)) * MS_PER_MINUTE
		};
		let count = match self.unit {
			Unit::Millisecond => end - start,
			Unit::Second => (end - start) / MS_PER_SECOND,
			Unit::Minute => (end - start) / MS_PER_MINUTE,
			Unit::Hour => (end - start) / MS_PER_HOUR,
			Unit::Day => (end - start - zone_change(end, start)) / MS_PER_DAY,
			Unit::Week(_) => (end - start - zone_change(end, start)) / DURATION_WEEK,
			Unit::Month => {
				let (start, end) = (calendar.fields(start), calendar.fields(end));
				end.month - start.month + (end.year - start.year) * 12.0
			}
			Unit::Year => calendar.fields(end).year - calendar.fields(start).year,
		};
		count.floor()
	}

	/// Whether `time` belongs to the filtered interval.
	fn test(&self, calendar: &Calendar, time: f64) -> bool {
		let fields = calendar.fields(time);
		let field = match self.unit {
			Unit::Second => crate::mermaid::gantt::date::fields_of(time).seconds,
			Unit::Minute => fields.minutes,
			Unit::Hour => fields.hours,
			Unit::Day => fields.date - 1.0,
			Unit::Month => fields.month,
			_ => self.count_unit(calendar, 0.0, time),
		};
		field % self.step == 0.0
	}

	pub fn floor(&self, calendar: &Calendar, time: f64) -> f64 {
		if !self.is_filtered() {
			return self.floor_unit(calendar, time);
		}
		if time.is_nan() {
			return time;
		}
		let mut time = time;
		for _ in 0..MAX_FILTER_STEPS {
			time = self.floor_unit(calendar, time);
			if self.test(calendar, time) {
				break;
			}
			time -= 1.0;
		}
		time
	}

	/// `offseti(date, step)` for positive steps.
	fn offset(&self, calendar: &Calendar, time: f64, step: f64) -> f64 {
		if !self.is_filtered() {
			return self.offset_unit(calendar, time, step);
		}
		if time.is_nan() {
			return time;
		}
		let mut time = time;
		for _ in 0..step.max(0.0) as u64 {
			for _ in 0..MAX_FILTER_STEPS {
				time = self.offset_unit(calendar, time, 1.0);
				if self.test(calendar, time) {
					break;
				}
			}
		}
		time
	}

	pub fn ceil(&self, calendar: &Calendar, time: f64) -> f64 {
		let time = self.floor(calendar, time - 1.0);
		let time = self.offset(calendar, time, 1.0);
		self.floor(calendar, time)
	}

	/// `interval.range(start, stop)`: the dates from `start` before `stop`.
	pub fn range(&self, calendar: &Calendar, start: f64, stop: f64) -> Vec<f64> {
		let mut range = Vec::new();
		let mut start = self.ceil(calendar, start);
		if start.partial_cmp(&stop) != Some(std::cmp::Ordering::Less) {
			return range;
		}
		loop {
			let previous = start;
			range.push(previous);
			start = self.offset(calendar, start, 1.0);
			start = self.floor(calendar, start);
			if !(previous < start && start < stop) || range.len() >= MAX_TICKS {
				return range;
			}
		}
	}
}

/// d3-array's `tickStep`.
fn tick_step(start: f64, stop: f64, count: f64) -> f64 {
	let reverse = stop < start;
	let increment = if reverse {
		tick_increment(stop, start, count)
	} else {
		tick_increment(start, stop, count)
	};
	(if reverse { -1.0 } else { 1.0 }) * if increment < 0.0 { 1.0 / -increment } else { increment }
}

/// d3-array's `tickSpec(...)[2]`.
fn tick_increment(start: f64, stop: f64, count: f64) -> f64 {
	let mut count = count;
	loop {
		let step = (stop - start) / count.max(0.0);
		let power = step.log10().floor();
		let error = step / 10f64.powf(power);
		let factor = if error >= 50f64.sqrt() {
			10.0
		} else if error >= 10f64.sqrt() {
			5.0
		} else if error >= 2f64.sqrt() {
			2.0
		} else {
			1.0
		};
		let (mut first, mut last, increment);
		if power < 0.0 {
			let inverse = 10f64.powf(-power) / factor;
			first = crate::mermaid::color::js_round(start * inverse);
			last = crate::mermaid::color::js_round(stop * inverse);
			if first / inverse < start {
				first += 1.0;
			}
			if last / inverse > stop {
				last -= 1.0;
			}
			increment = -inverse;
		} else {
			increment = 10f64.powf(power) * factor;
			first = crate::mermaid::color::js_round(start / increment);
			last = crate::mermaid::color::js_round(stop / increment);
			if first * increment < start {
				first += 1.0;
			}
			if last * increment > stop {
				last -= 1.0;
			}
		}
		if last < first && (0.5..2.0).contains(&count) {
			count *= 2.0;
			continue;
		}
		return increment;
	}
}

/// d3-time's `timeTickInterval(start, stop, count)`.
fn tick_interval(start: f64, stop: f64, count: f64) -> Option<Interval> {
	const TICK_INTERVALS: [(Unit, f64, f64); 18] = [
		(Unit::Second, 1.0, MS_PER_SECOND),
		(Unit::Second, 5.0, 5.0 * MS_PER_SECOND),
		(Unit::Second, 15.0, 15.0 * MS_PER_SECOND),
		(Unit::Second, 30.0, 30.0 * MS_PER_SECOND),
		(Unit::Minute, 1.0, MS_PER_MINUTE),
		(Unit::Minute, 5.0, 5.0 * MS_PER_MINUTE),
		(Unit::Minute, 15.0, 15.0 * MS_PER_MINUTE),
		(Unit::Minute, 30.0, 30.0 * MS_PER_MINUTE),
		(Unit::Hour, 1.0, MS_PER_HOUR),
		(Unit::Hour, 3.0, 3.0 * MS_PER_HOUR),
		(Unit::Hour, 6.0, 6.0 * MS_PER_HOUR),
		(Unit::Hour, 12.0, 12.0 * MS_PER_HOUR),
		(Unit::Day, 1.0, MS_PER_DAY),
		(Unit::Day, 2.0, 2.0 * MS_PER_DAY),
		(Unit::Week(0), 1.0, DURATION_WEEK),
		(Unit::Month, 1.0, DURATION_MONTH),
		(Unit::Month, 3.0, 3.0 * DURATION_MONTH),
		(Unit::Year, 1.0, DURATION_YEAR),
	];
	let target = (stop - start).abs() / count;
	if target.is_nan() {
		// d3's bisector returns the end for `NaN`.
		return Interval::every(Unit::Year, f64::NAN);
	}
	let index = TICK_INTERVALS.partition_point(|&(_, _, duration)| duration <= target);
	if index == TICK_INTERVALS.len() {
		let step = tick_step(start / DURATION_YEAR, stop / DURATION_YEAR, count);
		return Interval::every(Unit::Year, step);
	}
	if index == 0 {
		return Interval::every(Unit::Millisecond, tick_step(start, stop, count).max(1.0));
	}
	let (before, after) = (TICK_INTERVALS[index - 1], TICK_INTERVALS[index]);
	let (unit, step, _) = if target / before.2 < after.2 / target { before } else { after };
	Interval::every(unit, step)
}

/// The ticks of a time scale: `scale.ticks(interval)`, by default about ten.
pub fn ticks(calendar: &Calendar, start: f64, stop: f64, interval: Option<Interval>) -> Vec<f64> {
	let reverse = stop < start;
	let (start, stop) = if reverse { (stop, start) } else { (start, stop) };
	let Some(interval) = interval.or_else(|| tick_interval(start, stop, 10.0)) else {
		return Vec::new();
	};
	let mut ticks = interval.range(calendar, start, stop + 1.0);
	if reverse {
		ticks.reverse();
	}
	ticks
}

const SHORT_WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAYS: [&str; 7] =
	["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const SHORT_MONTHS: [&str; 12] =
	["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
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

/// d3-time-format's `pad`.
fn pad(value: f64, fill: &str, width: usize) -> String {
	let sign = if value < 0.0 { "-" } else { "" };
	let text = crate::mermaid::color::js_number(if value < 0.0 { -value } else { value });
	let length = text.chars().count();
	if length < width {
		format!("{sign}{}{text}", fill.repeat(width - length))
	} else {
		format!("{sign}{text}")
	}
}

/// A locale name; `Array.join` writes the `undefined` of an invalid date as nothing.
fn name<'a>(names: &[&'a str], index: f64) -> &'a str {
	if index.is_nan() { "" } else { names[index as usize % names.len()] }
}

/// d3's `timeFormat(specifier)(time)` in the `en-US` locale.
pub fn time_format(calendar: &Calendar, specifier: &str, time: f64) -> String {
	let mut output = String::with_capacity(specifier.len() + 8);
	let mut characters = specifier.chars();
	while let Some(character) = characters.next() {
		if character != '%' {
			output.push(character);
			continue;
		}
		let mut directive = characters.next();
		let fill = match directive {
			Some('-') => Some(""),
			Some('_') => Some(" "),
			Some('0') => Some("0"),
			_ => None,
		};
		if fill.is_some() {
			directive = characters.next();
		}
		let Some(directive) = directive else { break };
		let fill = fill.unwrap_or(if directive == 'e' { " " } else { "0" });
		output.push_str(&format_directive(calendar, directive, fill, time));
	}
	output
}

/// One `%` directive; unknown ones are written as their letter.
fn format_directive(calendar: &Calendar, directive: char, fill: &str, time: f64) -> String {
	let fields = calendar.fields(time);
	let year_start = |time: f64| Interval::new(Unit::Year).floor(calendar, time);
	let thursday = Interval::new(Unit::Week(4));
	let iso_thursday = |time: f64| {
		let weekday = calendar.fields(time).weekday;
		if weekday >= 4.0 || weekday == 0.0 {
			thursday.floor(calendar, time)
		} else {
			thursday.ceil(calendar, time)
		}
	};
	let weeks_since_year = |weekday: u8| {
		Interval::new(Unit::Week(weekday)).count_unit(calendar, year_start(time) - 1.0, time)
	};
	match directive {
		'a' => name(&SHORT_WEEKDAYS, fields.weekday).to_owned(),
		'A' => name(&WEEKDAYS, fields.weekday).to_owned(),
		'b' => name(&SHORT_MONTHS, fields.month).to_owned(),
		'B' => name(&MONTHS, fields.month).to_owned(),
		'c' => time_format(calendar, "%x, %X", time),
		'd' | 'e' => pad(fields.date, fill, 2),
		'f' => pad(fields.milliseconds, fill, 3) + "000",
		'g' => pad(calendar.fields(iso_thursday(time)).year % 100.0, fill, 2),
		'G' => pad(calendar.fields(iso_thursday(time)).year % 10000.0, fill, 4),
		'H' => pad(fields.hours, fill, 2),
		'I' => pad(if fields.hours % 12.0 == 0.0 { 12.0 } else { fields.hours % 12.0 }, fill, 2),
		'j' => pad(
			1.0 + Interval::new(Unit::Day).count_unit(calendar, year_start(time), time),
			fill,
			3,
		),
		'L' => pad(fields.milliseconds, fill, 3),
		'm' => pad(fields.month + 1.0, fill, 2),
		'M' => pad(fields.minutes, fill, 2),
		'p' => if fields.hours >= 12.0 { "PM" } else { "AM" }.to_owned(),
		// `~~` makes `NaN` zero.
		'q' => crate::mermaid::color::js_number(1.0 + (fields.month / 3.0).trunc().max(0.0)),
		'Q' => crate::mermaid::color::js_number(time),
		's' => crate::mermaid::color::js_number((time / 1000.0).floor()),
		'S' => pad(fields.seconds, fill, 2),
		'u' => crate::mermaid::color::js_number(if fields.weekday == 0.0 {
			7.0
		} else {
			fields.weekday
		}),
		'U' => pad(weeks_since_year(0), fill, 2),
		'V' => {
			let thursday_time = iso_thursday(time);
			let start = year_start(thursday_time);
			let is_thursday_start = calendar.fields(start).weekday == 4.0;
			let count = thursday.count_unit(calendar, start, thursday_time);
			pad(count + f64::from(u8::from(is_thursday_start)), fill, 2)
		}
		'w' => crate::mermaid::color::js_number(fields.weekday),
		'W' => pad(weeks_since_year(1), fill, 2),
		'x' => time_format(calendar, "%-m/%-d/%Y", time),
		'X' => time_format(calendar, "%-I:%M:%S %p", time),
		'y' => pad(fields.year % 100.0, fill, 2),
		'Y' => pad(fields.year % 10000.0, fill, 4),
		'Z' => {
			let offset = calendar.timezone_offset(time);
			let (sign, minutes) = if offset > 0.0 { ("-", offset) } else { ("+", -offset) };
			format!(
				"{sign}{}{}",
				pad((minutes / 60.0).trunc(), "0", 2),
				pad(minutes % 60.0, "0", 2)
			)
		}
		'%' => "%".to_owned(),
		other => other.to_string(),
	}
}

#[cfg(test)]
mod tests {
	use super::super::date::Fields;
	use super::*;
	use crate::mermaid::FixedClock;

	const CLOCK: FixedClock = FixedClock { now: 0.0, utc_offset: 60.0 };

	fn local(calendar: &Calendar, year: f64, month: f64, date: f64, hours: f64) -> f64 {
		calendar.make(&Fields {
			year,
			month,
			date,
			weekday: 0.0,
			hours,
			minutes: 0.0,
			seconds: 0.0,
			milliseconds: 0.0,
		})
	}

	#[test]
	fn ticks_like_d3() {
		let calendar = Calendar { clock: &CLOCK };
		let format = |ticks: Vec<f64>| {
			ticks
				.iter()
				.map(|&tick| time_format(&calendar, "%Y-%m-%d %H", tick))
				.collect::<Vec<_>>()
		};
		let start = local(&calendar, 2014.0, 0.0, 1.0, 0.0);
		let end = local(&calendar, 2014.0, 1.0, 20.0, 0.0);
		assert_eq!(
			format(ticks(&calendar, start, end, None)),
			[
				"2014-01-05 00",
				"2014-01-12 00",
				"2014-01-19 00",
				"2014-01-26 00",
				"2014-02-02 00",
				"2014-02-09 00",
				"2014-02-16 00"
			]
		);
		let days = Interval::every(Unit::Day, 10.0);
		assert_eq!(
			format(ticks(&calendar, start, end, days)),
			[
				"2014-01-01 00",
				"2014-01-11 00",
				"2014-01-21 00",
				"2014-01-31 00",
				"2014-02-01 00",
				"2014-02-11 00"
			]
		);
		let mondays = Interval::every(Unit::Week(1), 2.0);
		assert!(!ticks(&calendar, start, end, mondays).is_empty());
		let hours = ticks(&calendar, start, start + 10.0 * MS_PER_HOUR, None);
		assert_eq!(format(hours)[..2], ["2014-01-01 00", "2014-01-01 01"]);
		assert!(ticks(&calendar, f64::NAN, f64::NAN, None).is_empty());
		let years = ticks(&calendar, start, local(&calendar, 2060.0, 0.0, 1.0, 0.0), None);
		assert_eq!(format(years)[..2], ["2015-01-01 00", "2020-01-01 00"]);
	}

	#[test]
	fn formats_like_d3() {
		let calendar = Calendar { clock: &CLOCK };
		let time = local(&calendar, 2014.0, 0.0, 5.0, 13.0) + 7.0 * MS_PER_MINUTE + 9_012.0;
		let format = |specifier: &str| time_format(&calendar, specifier, time);
		assert_eq!(format("%a %A %b %B %d %e %-d %_d"), "Sun Sunday Jan January 05  5 5  5");
		assert_eq!(format("%H %I %p %M %S %L %f"), "13 01 PM 07 09 012 012000");
		assert_eq!(format("%j %m %q %u %w %y %Y %Z %%"), "005 01 1 7 0 14 2014 +0100 %");
		assert_eq!(format("%U %W %V %g %G"), "01 00 01 14 2014");
		assert_eq!(format("%c|%x|%X|%k"), "1/5/2014, 1:07:09 PM|1/5/2014|1:07:09 PM|k");
		assert_eq!(format("%"), "");
		assert_eq!(time_format(&calendar, "%Y-%m", f64::NAN), "0NaN-NaN");
	}
}
