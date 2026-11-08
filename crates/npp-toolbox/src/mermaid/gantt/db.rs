//! The gantt database (`ganttDb.js`): sections, tasks and their dates, resolved like Mermaid's
//! `getTasks` with dayjs.

use std::collections::HashMap;

use super::date::Calendar;
use super::dayjs::Exception;
use crate::mermaid::common::{acc_description, sanitize_text, sanitize_url};

const TAGS: [&str; 5] = ["active", "done", "crit", "milestone", "vert"];
/// The most days `fixTaskDates` steps through for all tasks together. Mermaid has no limit,
/// only one of 10,000 excluded days in a row per task.
const MAX_EXCLUDE_STEPS: usize = 2_000_000;

/// Where a task starts, from its data.
#[derive(Clone, Debug, PartialEq)]
pub enum Start {
	/// At the end of the task before it.
	PreviousTaskEnd,
	/// A date, `after <ids>` or a timestamp.
	Data(String),
	/// More than three data parts, which Mermaid cannot read.
	Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
	pub section: String,
	/// The name as written, with its trailing whitespace.
	pub name: String,
	/// `None` is JavaScript's `undefined` of tasks with more than three data parts.
	pub id: Option<String>,
	previous_id: Option<String>,
	start: Start,
	end_data: String,
	pub is_active: bool,
	pub is_done: bool,
	pub is_critical: bool,
	pub is_milestone: bool,
	pub is_vertical: bool,
	/// The row, -1 for vertical markers.
	pub order: f64,
	pub classes: Vec<&'static str>,
	/// `None` is `undefined`, `NaN` an invalid date.
	pub start_time: Option<f64>,
	pub end_time: Option<f64>,
	pub render_end_time: Option<f64>,
	manual_end_time: bool,
	processed: bool,
	/// The line of the task in the code block.
	pub line: usize,
}

impl Task {
	/// The id as JavaScript writes it into the element ids.
	pub fn id_text(&self) -> &str {
		self.id.as_deref().unwrap_or("undefined")
	}
}

#[derive(Debug)]
pub struct Database {
	pub date_format: String,
	pub axis_format: String,
	pub tick_interval: Option<String>,
	pub today_marker: String,
	pub includes: Vec<String>,
	pub excludes: Vec<String>,
	pub inclusive_end_dates: bool,
	pub weekday: String,
	pub weekend: String,
	pub title: String,
	pub acc_title: String,
	pub acc_description: String,
	pub links: HashMap<String, String>,
	pub tasks: Vec<Task>,
	/// Task positions by id, like the object `taskDb`.
	positions: HashMap<String, usize>,
	current_section: String,
	task_count: usize,
	last_order: f64,
}

/// `>` of two JavaScript dates, either `undefined`.
fn is_later(first: Option<f64>, second: Option<f64>) -> bool {
	matches!((first, second), (Some(first), Some(second)) if first > second)
}

/// `mergeTokens`: lower case words, without duplicates.
fn merge_tokens(existing: &mut Vec<String>, text: &str) {
	let lower = text.to_lowercase();
	for token in lower.split(|character: char| character.is_whitespace() || character == ',') {
		if !token.is_empty() && !existing.iter().any(|known| known == token) {
			existing.push(token.to_owned());
		}
	}
}

impl Database {
	pub fn new(title: Option<&str>) -> Self {
		Database {
			date_format: String::new(),
			axis_format: String::new(),
			tick_interval: None,
			today_marker: String::new(),
			includes: Vec::new(),
			excludes: Vec::new(),
			inclusive_end_dates: false,
			weekday: "sunday".to_owned(),
			weekend: "saturday".to_owned(),
			title: title.map(sanitize_text).unwrap_or_default(),
			acc_title: String::new(),
			acc_description: String::new(),
			links: HashMap::new(),
			tasks: Vec::new(),
			positions: HashMap::new(),
			current_section: String::new(),
			task_count: 0,
			last_order: 0.0,
		}
	}

	pub fn set_title(&mut self, title: &str) {
		self.title = sanitize_text(title);
	}

	pub fn set_acc_title(&mut self, title: &str) {
		self.acc_title = sanitize_text(title.trim()).trim_start().to_owned();
	}

	pub fn set_acc_description(&mut self, description: &str) {
		self.acc_description = acc_description(description);
	}

	pub fn add_excludes(&mut self, text: &str) {
		merge_tokens(&mut self.excludes, text);
	}

	pub fn add_includes(&mut self, text: &str) {
		merge_tokens(&mut self.includes, text);
	}

	pub fn add_section(&mut self, name: &str) {
		self.current_section = name.to_owned();
	}

	/// `findTaskById`, where JavaScript's object keys make `__proto__` unfindable.
	fn position_of(&self, id: Option<&str>) -> Option<usize> {
		let key = id.unwrap_or("undefined");
		if key == "__proto__" {
			return None;
		}
		self.positions.get(key).copied()
	}

	fn find_task(&self, id: Option<&str>) -> Option<&Task> {
		self.position_of(id).and_then(|position| self.tasks.get(position))
	}

	/// `addTask`; an error is the exception of `getTaskTags` when only tags are left.
	pub fn add_task(&mut self, name: &str, data: &str, line: usize) -> Result<(), String> {
		let data = data.strip_prefix(':').unwrap_or(data);
		let mut parts: Vec<&str> = data.split(',').collect();
		let mut tags = [false; 5];
		let mut has_matched = true;
		while has_matched {
			has_matched = false;
			for (index, tag) in TAGS.iter().enumerate() {
				let Some(first) = parts.first() else {
					return Err("Cannot read properties of undefined (reading 'match')".to_owned());
				};
				if first.trim() == *tag {
					tags[index] = true;
					parts.remove(0);
					has_matched = true;
				}
			}
		}
		let parts: Vec<&str> = parts.iter().map(|part| part.trim()).collect();
		let (id, start, end_data) = match parts.as_slice() {
			[end] => (None, Start::PreviousTaskEnd, *end),
			[start, end] => (None, Start::Data((*start).to_owned()), *end),
			[id, start, end] => (Some((*id).to_owned()), Start::Data((*start).to_owned()), *end),
			_ => (None, Start::Missing, ""),
		};
		let has_id = id.is_some() || start != Start::Missing;
		let id = match id {
			Some(id) => Some(id),
			None if has_id => {
				self.task_count += 1;
				Some(format!("task{}", self.task_count))
			}
			None => None,
		};
		let [is_active, is_done, is_critical, is_milestone, is_vertical] = tags;
		let order = if is_vertical {
			-1.0
		} else {
			self.last_order += 1.0;
			self.last_order - 1.0
		};
		let previous_id = self.tasks.last().map(|task| task.id.clone()).unwrap_or(None);
		let task = Task {
			section: self.current_section.clone(),
			name: name.to_owned(),
			id,
			previous_id,
			start,
			end_data: end_data.to_owned(),
			is_active,
			is_done,
			is_critical,
			is_milestone,
			is_vertical,
			order,
			classes: Vec::new(),
			start_time: None,
			end_time: None,
			render_end_time: None,
			manual_end_time: false,
			processed: false,
			line,
		};
		let key = task.id_text().to_owned();
		self.tasks.push(task);
		if key != "__proto__" {
			self.positions.insert(key, self.tasks.len() - 1);
		}
		Ok(())
	}

	/// `setClass(ids, 'clickable')`
	pub fn set_clickable(&mut self, ids: &str) {
		for id in ids.split(',') {
			if let Some(position) = self.position_of(Some(id)) {
				self.tasks[position].classes.push("clickable");
			}
		}
	}

	/// `setLink`: the sanitized link for the existing tasks.
	pub fn set_link(&mut self, ids: &str, link: &str) {
		let link = sanitize_url(link);
		for id in ids.split(',') {
			if self.position_of(Some(id)).is_some() {
				self.links.insert(id.to_owned(), link.clone());
			}
		}
		self.set_clickable(ids);
	}

	/// `isInvalidDate`: whether `time` is excluded.
	pub fn is_excluded(&self, calendar: &Calendar, time: f64) -> Result<bool, Exception> {
		let formatted = calendar.format(time, self.date_format.trim())?;
		let date_only = calendar.format(time, "YYYY-MM-DD")?;
		let contains = |list: &[String], text: &str| list.iter().any(|item| item == text);
		if contains(&self.includes, &formatted) || contains(&self.includes, &date_only) {
			return Ok(false);
		}
		let weekend_start = if self.weekend == "friday" { 5.0 } else { 6.0 };
		let weekday = calendar.iso_weekday(time);
		if contains(&self.excludes, "weekends")
			&& (weekday == weekend_start || weekday == weekend_start + 1.0)
		{
			return Ok(true);
		}
		let day_name = calendar.format(time, "dddd")?.to_lowercase();
		Ok(contains(&self.excludes, &day_name)
			|| contains(&self.excludes, &formatted)
			|| contains(&self.excludes, &date_only))
	}

	/// `getStartDate`
	fn start_date(&self, calendar: &Calendar, text: &str) -> Result<Option<f64>, String> {
		let text = text.trim();
		let format = self.date_format.trim();
		if (format == "x" || format == "X")
			&& !text.is_empty()
			&& text.bytes().all(|byte| byte.is_ascii_digit())
		{
			return Ok(Some(super::date::time_clip(crate::mermaid::common::js_number_of(text))));
		}
		if let Some(ids) = referenced_ids(text, "after") {
			let mut latest: Option<&Task> = None;
			for id in ids {
				let Some(task) = self.find_task(Some(id)) else { continue };
				if latest.is_none_or(|latest| is_later(task.end_time, latest.end_time)) {
					latest = Some(task);
				}
			}
			return Ok(match latest {
				Some(task) => task.end_time,
				None => Some(calendar.start_of_day(calendar.clock.now())),
			});
		}
		let parsed = calendar.parse_strict(text, format).map_err(|exception| exception.0)?;
		if !parsed.is_nan() {
			return Ok(Some(parsed));
		}
		let fallback = calendar.parse(text);
		let year = calendar.fields(fallback).year;
		if fallback.is_nan() || !(-10000.0..=10000.0).contains(&year) {
			return Err(format!("Invalid date:{text}"));
		}
		Ok(Some(fallback))
	}

	/// `getEndDate`
	fn end_date(
		&self,
		calendar: &Calendar,
		previous: f64,
		text: &str,
	) -> Result<Option<f64>, String> {
		let text = text.trim();
		if let Some(ids) = referenced_ids(text, "until") {
			let mut earliest: Option<&Task> = None;
			for id in ids {
				let Some(task) = self.find_task(Some(id)) else { continue };
				if earliest.is_none_or(|earliest| is_later(earliest.start_time, task.start_time)) {
					earliest = Some(task);
				}
			}
			return Ok(match earliest {
				Some(task) => task.start_time,
				None => Some(calendar.start_of_day(calendar.clock.now())),
			});
		}
		let parsed = calendar
			.parse_strict(text, self.date_format.trim())
			.map_err(|exception| exception.0)?;
		if !parsed.is_nan() {
			let end =
				if self.inclusive_end_dates { calendar.add(parsed, 1.0, "d") } else { parsed };
			return Ok(Some(end));
		}
		let Some((amount, unit)) = parse_duration(text) else { return Ok(Some(previous)) };
		let moved = calendar.add(previous, amount, unit);
		Ok(Some(if moved.is_nan() { previous } else { moved }))
	}

	/// `checkTaskDates`: moves the end of the task at `index` past excluded days.
	fn check_task_dates(
		&mut self,
		calendar: &Calendar,
		index: usize,
		steps: &mut usize,
	) -> Result<(), String> {
		let task = &self.tasks[index];
		if self.excludes.is_empty() || task.manual_end_time {
			return Ok(());
		}
		let mut start = calendar.add(task.start_time.unwrap_or(f64::NAN), 1.0, "d");
		let mut end = task.end_time.unwrap_or(f64::NAN);
		let max_end = calendar.add(end, 10000.0, "d");
		let mut is_invalid = false;
		let mut render_end = None;
		while start <= end {
			if !is_invalid {
				render_end = Some(end);
			}
			is_invalid = self.is_excluded(calendar, start).map_err(|exception| exception.0)?;
			if is_invalid {
				end = calendar.add(end, 1.0, "d");
				if end > max_end {
					return Err("Failed to find a valid date that was not excluded by `excludes` \
						after 10,000 iterations."
						.to_owned());
				}
			}
			start = calendar.add(start, 1.0, "d");
			*steps += 1;
			if *steps > MAX_EXCLUDE_STEPS {
				return Err("The tasks span too many days to skip the excluded ones".to_owned());
			}
		}
		let task = &mut self.tasks[index];
		task.end_time = Some(end);
		task.render_end_time = render_end;
		Ok(())
	}

	/// One pass of `compileTasks`: whether all tasks have their dates. An error names the line
	/// of the task.
	fn compile_tasks(
		&mut self,
		calendar: &Calendar,
		steps: &mut usize,
	) -> Result<bool, (usize, String)> {
		let mut all_processed = true;
		for index in 0..self.tasks.len() {
			let line = self.tasks[index].line;
			let fail = |message: String| (line, message);
			match self.tasks[index].start.clone() {
				Start::PreviousTaskEnd => {
					let previous = self.tasks[index].previous_id.clone();
					let Some(previous) = self.find_task(previous.as_deref()) else {
						return Err(fail(
							"Cannot read properties of undefined (reading 'endTime')".to_owned(),
						));
					};
					self.tasks[index].start_time = previous.end_time;
				}
				Start::Data(text) => {
					if let Some(start) = self.start_date(calendar, &text).map_err(fail)? {
						self.tasks[index].start_time = Some(start);
					}
				}
				Start::Missing => {
					return Err(fail(
						"Cannot read properties of undefined (reading 'type')".to_owned(),
					));
				}
			}
			if let Some(start) = self.tasks[index].start_time {
				let end_data = self.tasks[index].end_data.clone();
				let end = self.end_date(calendar, start, &end_data).map_err(fail)?;
				self.tasks[index].end_time = end;
				if end.is_some() {
					let manual = !calendar
						.parse_strict(&end_data, "YYYY-MM-DD")
						.map_err(|exception| fail(exception.0))?
						.is_nan();
					let task = &mut self.tasks[index];
					task.processed = true;
					task.manual_end_time = manual;
					self.check_task_dates(calendar, index, steps).map_err(fail)?;
				}
			}
			all_processed &= self.tasks[index].processed;
		}
		Ok(all_processed)
	}

	/// `getTasks`: the dates of all tasks, resolved in up to eleven passes.
	pub fn compile(&mut self, calendar: &Calendar) -> Result<(), (usize, String)> {
		let mut steps = 0;
		let mut all_processed = self.compile_tasks(calendar, &mut steps)?;
		let mut passes = 0;
		while !all_processed && passes < 10 {
			all_processed = self.compile_tasks(calendar, &mut steps)?;
			passes += 1;
		}
		Ok(())
	}
}

/// The ids of `^after\s+(?<ids>[\d\w- ]+)`, or of `until`.
fn referenced_ids<'a>(text: &'a str, keyword: &str) -> Option<impl Iterator<Item = &'a str>> {
	let rest = text.strip_prefix(keyword)?;
	let spaces = rest.len() - rest.trim_start().len();
	if spaces == 0 {
		return None;
	}
	let rest = &rest[spaces..];
	let length = rest
		.bytes()
		.take_while(|&byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b' '))
		.count();
	if length == 0 {
		return None;
	}
	Some(rest[..length].split(' ').filter(|id| !id.is_empty()))
}

/// `parseDuration`: `^(\d+(?:\.\d+)?)([Mdhmswy]|ms)$`.
fn parse_duration(text: &str) -> Option<(f64, &'static str)> {
	let digits = text.bytes().take_while(u8::is_ascii_digit).count();
	if digits == 0 {
		return None;
	}
	let mut length = digits;
	if text[length..].starts_with('.') {
		let decimals = text[length + 1..].bytes().take_while(u8::is_ascii_digit).count();
		if decimals > 0 {
			length += 1 + decimals;
		}
	}
	let unit = match &text[length..] {
		"ms" => "ms",
		"M" => "M",
		"d" => "d",
		"h" => "h",
		"m" => "m",
		"s" => "s",
		"w" => "w",
		"y" => "y",
		_ => return None,
	};
	Some((text[..length].parse().ok()?, unit))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mermaid::FixedClock;

	const CLOCK: FixedClock = FixedClock { now: 1_700_000_000_000.0, utc_offset: 0.0 };
	const DAY: f64 = super::super::date::MS_PER_DAY;
	const JAN_1: f64 = 1_388_534_400_000.0;

	fn compile(lines: &[(&str, &str)], change: impl FnOnce(&mut Database)) -> Database {
		let mut database = Database::new(None);
		database.date_format = "YYYY-MM-DD".to_owned();
		change(&mut database);
		for (index, (name, data)) in lines.iter().enumerate() {
			database.add_task(name, data, index + 1).unwrap();
		}
		database.compile(&Calendar { clock: &CLOCK }).unwrap();
		database
	}

	fn span(task: &Task) -> (f64, f64) {
		((task.start_time.unwrap() - JAN_1) / DAY, (task.end_time.unwrap() - JAN_1) / DAY)
	}

	#[test]
	fn compiles_tasks_like_mermaid() {
		let database = compile(
			&[
				("A", ":a1, 2014-01-01, 30d"),
				("B", ":after a1, 20d"),
				("C", ":done, crit, c, after b x a1, 2014-02-01"),
				("D", ":12h"),
				("E", ":milestone, m, 2014-01-01, until a1"),
				("F", ":vert, 2014-01-03, 0d"),
			],
			|_| {},
		);
		let tasks = &database.tasks;
		assert_eq!(span(&tasks[0]), (0.0, 30.0));
		assert_eq!(span(&tasks[1]), (30.0, 50.0));
		assert_eq!(tasks[1].id.as_deref(), Some("task1"));
		assert_eq!(span(&tasks[2]), (30.0, 31.0));
		assert!(tasks[2].is_done && tasks[2].is_critical);
		assert_eq!(span(&tasks[3]), (31.0, 31.5));
		assert_eq!(span(&tasks[4]), (0.0, 0.0));
		assert!(tasks[4].is_milestone);
		assert_eq!((tasks[5].order, tasks[3].order), (-1.0, 3.0));
	}

	#[test]
	fn skips_excluded_days() {
		let database =
			compile(&[("A", ":2014-01-03, 3d"), ("B", ":2014-01-03, 2014-01-07")], |database| {
				database.add_excludes("Weekends, 2014-01-07");
			});
		assert_eq!(database.excludes, ["weekends", "2014-01-07"]);
		// Friday plus three days without the weekend and the 7th ends on the 9th.
		assert_eq!(span(&database.tasks[0]), (2.0, 8.0));
		assert_eq!(database.tasks[0].render_end_time, Some(JAN_1 + 8.0 * DAY));
		assert_eq!(span(&database.tasks[1]), (2.0, 6.0));
	}

	#[test]
	fn reports_mermaids_exceptions() {
		let mut database = Database::new(None);
		assert!(database.add_task("A", ":done", 1).is_err());
		database.add_task("A", ":3d", 2).unwrap();
		let calendar = Calendar { clock: &CLOCK };
		assert_eq!(database.compile(&calendar).unwrap_err().0, 2);
		let mut database = Database::new(None);
		database.add_task("A", ":a, b, c, d", 1).unwrap();
		assert!(database.compile(&calendar).is_err());
		let mut database = Database::new(None);
		database.add_task("A", ":nonsense, 1d", 1).unwrap();
		assert_eq!(database.compile(&calendar).unwrap_err().1, "Invalid date:nonsense");
		assert_eq!(parse_duration("1.5d"), Some((1.5, "d")));
		assert_eq!(parse_duration("2ms"), Some((2.0, "ms")));
		assert_eq!(parse_duration("1.d"), None);
	}
}
