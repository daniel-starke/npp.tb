//! `parsePath`, `absolutize` and `normalize` of path-data-parser 0.1.0 (MIT, copyright (c) Preet
//! Shihn). Path data that the original rejects with an exception gives no segments.

use super::super::number_format::to_fixed;
use super::Xy;

/// A segment of a normalized path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
	Move(Xy),
	Line(Xy),
	Curve([f64; 6]),
	Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
	Command(u8),
	Number(f64),
}

fn parameter_count(command: u8) -> usize {
	match command.to_ascii_uppercase() {
		b'A' => 7,
		b'C' => 6,
		b'Q' | b'S' => 4,
		b'L' | b'M' | b'T' => 2,
		b'H' | b'V' => 1,
		_ => 0,
	}
}

/// The length of `([-+]?[0-9]+(\.[0-9]*)?|[-+]?\.[0-9]+)([eE][-+]?[0-9]+)?` at the start.
fn number_length(bytes: &[u8]) -> Option<usize> {
	let digits = |from: usize| {
		bytes.get(from..).unwrap_or_default().iter().take_while(|b| b.is_ascii_digit()).count()
	};
	let mut position = usize::from(matches!(bytes.first(), Some(b'-' | b'+')));
	let integer_digits = digits(position);
	position += integer_digits;
	if bytes.get(position) == Some(&b'.') {
		let decimals = digits(position + 1);
		if integer_digits == 0 && decimals == 0 {
			return None;
		}
		position += 1 + decimals;
	} else if integer_digits == 0 {
		return None;
	}
	if matches!(bytes.get(position), Some(b'e' | b'E')) {
		let sign = usize::from(matches!(bytes.get(position + 1), Some(b'-' | b'+')));
		let exponent = digits(position + 1 + sign);
		if exponent != 0 {
			position += 1 + sign + exponent;
		}
	}
	Some(position)
}

fn tokenize(d: &str) -> Option<Vec<Token>> {
	let bytes = d.as_bytes();
	let mut tokens = Vec::with_capacity(bytes.len() / 3);
	let mut position = 0;
	while let Some(&byte) = bytes.get(position) {
		if matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b',') {
			position += 1;
		} else if b"aAcChHlLmMqQsStTvVzZ".contains(&byte) {
			tokens.push(Token::Command(byte));
			position += 1;
		} else {
			let length = number_length(&bytes[position..])?;
			let text = d.get(position..position + length)?;
			tokens.push(Token::Number(text.parse().ok()?));
			position += length;
		}
	}
	Some(tokens)
}

type Raw = (u8, [f64; 7]);

/// `parsePath`.
fn parse_path(d: &str) -> Option<Vec<Raw>> {
	let mut tokens = tokenize(d)?;
	if !matches!(tokens.first(), None | Some(Token::Command(b'M' | b'm'))) {
		// `parsePath('M0,0' + d)`.
		let mut prefixed = vec![Token::Command(b'M'), Token::Number(0.0), Token::Number(0.0)];
		prefixed.append(&mut tokens);
		tokens = prefixed;
	}
	let mut segments = Vec::with_capacity(tokens.len() / 2);
	let mut mode = 0;
	let mut index = 0;
	while let Some(token) = tokens.get(index) {
		match *token {
			Token::Command(command) => {
				index += 1;
				mode = command;
			}
			// The original never ends here: a number after `Z` takes no parameters.
			Token::Number(_) if parameter_count(mode) == 0 => return None,
			Token::Number(_) => {}
		}
		let count = parameter_count(mode);
		let mut data = [0.0; 7];
		for (slot, token) in data.iter_mut().zip(tokens.get(index..index + count)?) {
			let Token::Number(number) = *token else {
				return None;
			};
			*slot = number;
		}
		segments.push((mode, data));
		index += count;
		mode = match mode {
			b'M' => b'L',
			b'm' => b'l',
			other => other,
		};
	}
	Some(segments)
}

/// `absolutize`.
fn absolutize(segments: &mut [Raw]) {
	let (mut x, mut y) = (0.0, 0.0);
	let (mut start_x, mut start_y) = (0.0, 0.0);
	for (command, data) in segments {
		let upper = command.to_ascii_uppercase();
		if command.is_ascii_lowercase() {
			match upper {
				b'H' => data[0] += x,
				b'V' => data[0] += y,
				b'A' => {
					data[5] += x;
					data[6] += y;
				}
				_ => {
					for (index, value) in data.iter_mut().enumerate().take(parameter_count(upper)) {
						*value += if index % 2 == 1 { y } else { x };
					}
				}
			}
		}
		*command = upper;
		match upper {
			b'M' => {
				(x, y) = (data[0], data[1]);
				(start_x, start_y) = (x, y);
			}
			b'L' | b'T' => (x, y) = (data[0], data[1]),
			b'C' => (x, y) = (data[4], data[5]),
			b'Q' | b'S' => (x, y) = (data[2], data[3]),
			b'A' => (x, y) = (data[5], data[6]),
			b'H' => x = data[0],
			b'V' => y = data[0],
			b'Z' => (x, y) = (start_x, start_y),
			_ => {}
		}
	}
}

/// `normalize`.
fn normalize(segments: &[Raw]) -> Vec<Segment> {
	let mut out = Vec::with_capacity(segments.len());
	let mut last_type = 0;
	let (mut cx, mut cy) = (0.0, 0.0);
	let (mut start_x, mut start_y) = (0.0, 0.0);
	let (mut lcx, mut lcy) = (0.0, 0.0);
	let quadratic = |cx: f64, cy: f64, x1: f64, y1: f64, x: f64, y: f64| {
		Segment::Curve([
			cx + 2.0 * (x1 - cx) / 3.0,
			cy + 2.0 * (y1 - cy) / 3.0,
			x + 2.0 * (x1 - x) / 3.0,
			y + 2.0 * (y1 - y) / 3.0,
			x,
			y,
		])
	};
	for &(key, data) in segments {
		match key {
			b'M' => {
				out.push(Segment::Move([data[0], data[1]]));
				(cx, cy) = (data[0], data[1]);
				(start_x, start_y) = (cx, cy);
			}
			b'C' => {
				out.push(Segment::Curve([data[0], data[1], data[2], data[3], data[4], data[5]]));
				(cx, cy) = (data[4], data[5]);
				(lcx, lcy) = (data[2], data[3]);
			}
			b'L' => {
				out.push(Segment::Line([data[0], data[1]]));
				(cx, cy) = (data[0], data[1]);
			}
			b'H' => {
				cx = data[0];
				out.push(Segment::Line([cx, cy]));
			}
			b'V' => {
				cy = data[0];
				out.push(Segment::Line([cx, cy]));
			}
			b'S' => {
				let (x1, y1) = match last_type {
					b'C' | b'S' => (cx + (cx - lcx), cy + (cy - lcy)),
					_ => (cx, cy),
				};
				out.push(Segment::Curve([x1, y1, data[0], data[1], data[2], data[3]]));
				(lcx, lcy) = (data[0], data[1]);
				(cx, cy) = (data[2], data[3]);
			}
			b'T' => {
				let (x1, y1) = match last_type {
					b'Q' | b'T' => (cx + (cx - lcx), cy + (cy - lcy)),
					_ => (cx, cy),
				};
				out.push(quadratic(cx, cy, x1, y1, data[0], data[1]));
				(lcx, lcy) = (x1, y1);
				(cx, cy) = (data[0], data[1]);
			}
			b'Q' => {
				out.push(quadratic(cx, cy, data[0], data[1], data[2], data[3]));
				(lcx, lcy) = (data[0], data[1]);
				(cx, cy) = (data[2], data[3]);
			}
			b'A' => {
				let (r1, r2) = (data[0].abs(), data[1].abs());
				let (x, y) = (data[5], data[6]);
				if r1 == 0.0 || r2 == 0.0 {
					out.push(Segment::Curve([cx, cy, x, y, x, y]));
					(cx, cy) = (x, y);
				} else if cx != x || cy != y {
					let arc = Arc { r1, r2, angle: data[2], is_sweep: data[4] != 0.0 };
					arc_to_cubic_curves([cx, cy], [x, y], &arc, data[3] == data[4], &mut out);
					(cx, cy) = (x, y);
				}
			}
			b'Z' => {
				out.push(Segment::Close);
				(cx, cy) = (start_x, start_y);
			}
			_ => {}
		}
		last_type = key;
	}
	out
}

struct Arc {
	r1: f64,
	r2: f64,
	angle: f64,
	is_sweep: bool,
}

fn rotate(x: f64, y: f64, radians: f64) -> Xy {
	[x * radians.cos() - y * radians.sin(), x * radians.sin() + y * radians.cos()]
}

/// `arcToCubicCurves`. The loop runs the recursion of the original over its parts of 120°.
fn arc_to_cubic_curves(from: Xy, to: Xy, arc: &Arc, is_same_flags: bool, out: &mut Vec<Segment>) {
	use std::f64::consts::{PI, TAU};
	let Arc { mut r1, mut r2, angle, is_sweep } = *arc;
	let radians = PI * angle / 180.0;
	let [x1, y1] = rotate(from[0], from[1], -radians);
	let [end_x, end_y] = rotate(to[0], to[1], -radians);
	let half_x = (x1 - end_x) / 2.0;
	let half_y = (y1 - end_y) / 2.0;
	let h = (half_x * half_x) / (r1 * r1) + (half_y * half_y) / (r2 * r2);
	if h > 1.0 {
		let h = h.sqrt();
		r1 *= h;
		r2 *= h;
	}
	let sign = if is_same_flags { -1.0 } else { 1.0 };
	let (r1_squared, r2_squared) = (r1 * r1, r2 * r2);
	let left =
		r1_squared * r2_squared - r1_squared * half_y * half_y - r2_squared * half_x * half_x;
	let right = r1_squared * half_y * half_y + r2_squared * half_x * half_x;
	let k = sign * (left / right).abs().sqrt();
	let cx = k * r1 * half_y / r2 + (x1 + end_x) / 2.0;
	let cy = k * -r2 * half_x / r1 + (y1 + end_y) / 2.0;
	// `parseFloat((…).toFixed(9))`.
	let asin_of = |value: f64| to_fixed(value, 9).parse::<f64>().unwrap_or(f64::NAN).asin();
	let mut f1 = asin_of((y1 - cy) / r2);
	let mut f2 = asin_of((end_y - cy) / r2);
	if x1 < cx {
		f1 = PI - f1;
	}
	if end_x < cx {
		f2 = PI - f2;
	}
	if f1 < 0.0 {
		f1 += TAU;
	}
	if f2 < 0.0 {
		f2 += TAU;
	}
	if is_sweep && f1 > f2 {
		f1 -= TAU;
	}
	if !is_sweep && f2 > f1 {
		f2 -= TAU;
	}
	let (mut from_x, mut from_y) = (x1, y1);
	let max_part = PI * 120.0 / 180.0;
	// An arc turns less than a full circle, so there are at most 4 parts.
	for _ in 0..8 {
		let is_split = (f2 - f1).abs() > max_part;
		let part_end = match is_split {
			true if is_sweep && f2 > f1 => f1 + max_part,
			true => f1 - max_part,
			false => f2,
		};
		let (to_x, to_y) = match is_split {
			true => (cx + r1 * part_end.cos(), cy + r2 * part_end.sin()),
			false => (end_x, end_y),
		};
		let t = ((part_end - f1) / 4.0).tan();
		let (hx, hy) = (4.0 / 3.0 * r1 * t, 4.0 / 3.0 * r2 * t);
		let c1 = rotate(
			2.0 * from_x - (from_x + hx * f1.sin()),
			2.0 * from_y - (from_y - hy * f1.cos()),
			radians,
		);
		let c2 = rotate(to_x + hx * part_end.sin(), to_y - hy * part_end.cos(), radians);
		let end = rotate(to_x, to_y, radians);
		out.push(Segment::Curve([c1[0], c1[1], c2[0], c2[1], end[0], end[1]]));
		if !is_split {
			break;
		}
		(from_x, from_y) = (to_x, to_y);
		f1 = part_end;
	}
}

/// `.replace(/\n/g, ' ').replace(/(-\s)/g, '-')` of `RoughGenerator.path`.
fn cleaned(d: &str) -> String {
	let mut out = String::with_capacity(d.len());
	let mut characters = d.chars().peekable();
	while let Some(character) = characters.next() {
		out.push(if character == '\n' { ' ' } else { character });
		if character == '-' && characters.peek().is_some_and(|next| next.is_whitespace()) {
			characters.next();
		}
	}
	out
}

/// The path data as moves, lines, curves and closes: `normalize(absolutize(parsePath(d)))`.
pub fn normalized_segments(d: &str) -> Vec<Segment> {
	let Some(mut raw) = parse_path(&cleaned(d)) else {
		return Vec::new();
	};
	absolutize(&mut raw);
	normalize(&raw)
}
