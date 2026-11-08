//! Icons shared by the diagrams (`rendering-util/icons.ts`, `stringToIcon` of `@iconify/utils`).
//!
//! Mermaid has no icon data of its own: a page registers icon packs. The plugin behaves as if the
//! Font Awesome Free packs were registered under the names `fa`, `fas`, `far`, `fab` and
//! `fa6-solid`, `fa6-regular`, `fa6-brands`, with the data of the generated `icon_table.rs`. Other
//! packs, and builds without the feature `font-awesome`, draw the unknown icon.

use super::svg::{Anchor, Baseline, Svg, TextRun, number};
use super::text::Font;

/// The height of the view box of all Font Awesome icons.
const VIEW_HEIGHT: f64 = 512.0;
/// The private use characters that stand for icons in the text of labels.
const ICON_CHARACTERS_START: u32 = 0x10_0000;
/// The path commands in the order of their code (`cargo xtask update-fontawesome`).
const COMMANDS: &[u8; 20] = b"MmLlHhVvCcSsQqTtAaZz";
/// The codes after the commands: a shared sub-path, and the end of an icon or a sub-path.
const SHARED_CODE: u32 = 20;
const END_CODE: u32 = 21;
/// The bits of a code, of the width of an icon, of the decimals of a command and of the bit count
/// of its numbers.
const CODE_BITS: u32 = 5;
const WIDTH_BITS: u32 = 10;
const SCALE_BITS: u32 = 2;
const NUMBER_WIDTH_BITS: u32 = 5;

#[cfg(feature = "font-awesome")]
use super::icon_table::{PATH_STARTS, PATHS, SHARED_INDEX_BITS, SHARED_STARTS};
#[cfg(not(feature = "font-awesome"))]
const PATH_STARTS: [u32; 0] = [];
#[cfg(not(feature = "font-awesome"))]
const PATHS: &[u8] = &[];
#[cfg(not(feature = "font-awesome"))]
const SHARED_STARTS: [u32; 0] = [];
#[cfg(not(feature = "font-awesome"))]
const SHARED_INDEX_BITS: u32 = 0;

#[derive(Clone, Copy)]
enum Style {
	Solid,
	Regular,
	Brands,
}

/// The style of a pack and whether the pack also finds the names of Font Awesome 4.
fn style_of_pack(prefix: &str) -> Option<(Style, bool)> {
	Some(match prefix {
		"fa" | "fas" => (Style::Solid, true),
		"far" => (Style::Regular, true),
		"fab" => (Style::Brands, true),
		"fa6-solid" => (Style::Solid, false),
		"fa6-regular" => (Style::Regular, false),
		"fa6-brands" => (Style::Brands, false),
		_ => return None,
	})
}

/// The index of the icon `name` in the table of `style`, else of its name of Font Awesome 4.
#[cfg(feature = "font-awesome")]
fn icon_index(style: Style, name: &str, uses_shims: bool) -> Option<usize> {
	use super::icon_table::{BRANDS, NAMES, REGULAR, SHIMS, SOLID};
	let find = |entries: &[(u16, u16)]| {
		entries
			.binary_search_by(|&(start, _)| {
				let rest = NAMES.get(usize::from(start)..).unwrap_or_default();
				rest.split('\n').next().unwrap_or_default().cmp(name)
			})
			.ok()
			.map(|position| usize::from(entries[position].1))
	};
	let entries: &[(u16, u16)] = match style {
		Style::Solid => &SOLID,
		Style::Regular => &REGULAR,
		Style::Brands => &BRANDS,
	};
	find(entries).or_else(|| uses_shims.then(|| find(&SHIMS)).flatten())
}

#[cfg(not(feature = "font-awesome"))]
fn icon_index(_style: Style, _name: &str, _uses_shims: bool) -> Option<usize> {
	None
}

/// The bit position of an icon of the table.
fn icon_start(index: usize) -> Option<usize> {
	PATH_STARTS.get(index).map(|&start| start as usize)
}

/// `stringToIcon`: the prefix (which may be empty) and the name of an icon reference.
fn split_reference(value: &str, allows_simple_name: bool) -> Option<(&str, &str)> {
	let mut parts: Vec<&str> = value.split(':').collect();
	let mut has_provider = false;
	if value.starts_with('@') {
		if parts.len() < 2 || parts.len() > 3 {
			return None;
		}
		parts.remove(0);
		has_provider = true;
	}
	if parts.len() > 3 {
		return None;
	}
	let (prefix, name) = match *parts.as_slice() {
		[.., prefix, name] => (prefix, name),
		[name] => match name.split_once('-') {
			Some((prefix, name)) => (prefix, name),
			None if allows_simple_name && !has_provider => ("", name),
			None => return None,
		},
		[] => return None,
	};
	let is_valid =
		!name.is_empty() && (!prefix.is_empty() || (allows_simple_name && !has_provider));
	is_valid.then_some((prefix, name))
}

/// `getRegisteredIconData`: the pack and the name that `reference` asks for; `fallback_pack` is
/// the pack of references without one.
pub fn resolve_reference<'a>(
	reference: &'a str,
	fallback_pack: Option<&'a str>,
) -> Option<(&'a str, &'a str)> {
	let (prefix, name) = split_reference(reference, fallback_pack.is_some())?;
	let pack = if prefix.is_empty() { fallback_pack? } else { prefix };
	(!pack.is_empty()).then_some((pack, name))
}

/// The icon `name` of the Font Awesome pack `pack`.
pub fn font_awesome(pack: &str, name: &str) -> Option<IconGlyph> {
	let (style, uses_shims) = style_of_pack(pack)?;
	icon_index(style, name, uses_shims).and_then(icon_start).map(IconGlyph::from_start)
}

/// The icon that `reference` asks for, `None` for the unknown icon.
pub fn registered_glyph(reference: &str, fallback_pack: Option<&str>) -> Option<IconGlyph> {
	let (pack, name) = resolve_reference(reference, fallback_pack)?;
	font_awesome(pack, name)
}

/// The character that stands for an icon in the text of a label.
pub fn icon_character(pack: &str, name: &str) -> Option<char> {
	let (style, uses_shims) = style_of_pack(pack)?;
	let index = icon_index(style, name, uses_shims)?;
	char::from_u32(ICON_CHARACTERS_START + u32::try_from(index).ok()?)
}

/// The icon that `character` stands for; `None` for all other characters.
pub fn glyph_of_character(character: char) -> Option<IconGlyph> {
	let index = (character as u32).checked_sub(ICON_CHARACTERS_START)?;
	icon_start(index as usize).map(IconGlyph::from_start)
}

/// A box of an icon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconBox {
	pub x: f64,
	pub y: f64,
	pub width: f64,
	pub height: f64,
}

/// An icon of the table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconGlyph {
	/// The width of the view box, whose height is always 512.
	pub width: f64,
	/// The bit position of the commands in the table.
	commands: usize,
}

impl IconGlyph {
	fn from_start(start: usize) -> Self {
		let mut reader = BitReader { position: start };
		let width = f64::from(reader.bits(WIDTH_BITS));
		Self { width, commands: reader.position }
	}

	fn commands(&self) -> CommandReader {
		CommandReader { reader: BitReader { position: self.commands }, return_position: None }
	}

	/// The SVG path data.
	pub fn path_data(&self) -> String {
		let mut data = String::with_capacity(512);
		for command in self.commands() {
			data.push(command.letter as char);
			for (index, number) in command.numbers().iter().enumerate() {
				if command.is_arc() && index == 3 {
					data.push_str(if command.is_large_arc { " 1" } else { " 0" });
					data.push_str(if command.is_sweep { " 1" } else { " 0" });
				}
				if index > 0 && number.digits >= 0 {
					data.push(' ');
				}
				number.write(&mut data);
			}
		}
		data
	}

	/// The tight box of the path in the view box, like `getBBox`.
	pub fn path_box(&self) -> IconBox {
		let mut extent = Extent::default();
		let mut current = (0.0, 0.0);
		let mut start = current;
		// The second control point of the last curve, which smooth curves reflect.
		let mut last_control: Option<(u8, (f64, f64))> = None;
		for command in self.commands() {
			let relative = command.letter.is_ascii_lowercase();
			let upper = command.letter.to_ascii_uppercase();
			let values = command.values();
			let at = |x_index: usize, y_index: usize| match relative {
				true => (current.0 + values[x_index], current.1 + values[y_index]),
				false => (values[x_index], values[y_index]),
			};
			let reflected = |kind: u8| match last_control {
				Some((last, control)) if last == kind => {
					(2.0 * current.0 - control.0, 2.0 * current.1 - control.1)
				}
				_ => current,
			};
			let mut control = None;
			let end = match upper {
				b'M' => {
					let end = at(0, 1);
					start = end;
					extent.include(end);
					end
				}
				b'L' => {
					let end = at(0, 1);
					extent.include(end);
					end
				}
				b'H' => {
					let end = (if relative { current.0 + values[0] } else { values[0] }, current.1);
					extent.include(end);
					end
				}
				b'V' => {
					let end = (current.0, if relative { current.1 + values[0] } else { values[0] });
					extent.include(end);
					end
				}
				b'C' | b'S' => {
					let (first, second, end) = match upper {
						b'C' => (at(0, 1), at(2, 3), at(4, 5)),
						_ => (reflected(b'C'), at(0, 1), at(2, 3)),
					};
					extent.include_cubic(current, first, second, end);
					control = Some((b'C', second));
					end
				}
				b'Q' | b'T' => {
					let (first, end) = match upper {
						b'Q' => (at(0, 1), at(2, 3)),
						_ => (reflected(b'Q'), at(0, 1)),
					};
					extent.include_quadratic(current, first, end);
					control = Some((b'Q', first));
					end
				}
				b'A' => {
					let end = at(5, 6);
					let arc = (values[0], values[1], values[2]);
					extent.include_arc(current, end, arc, command.is_large_arc, command.is_sweep);
					end
				}
				_ => start,
			};
			current = end;
			last_control = control;
		}
		extent.to_box()
	}

	/// The scale and position of the view box in a nested `<svg>` of `width` × `height`
	/// (`preserveAspectRatio` meet and centered).
	fn placement(&self, width: f64, height: f64) -> (f64, f64, f64) {
		let scale = f64::min(width / self.width, height / VIEW_HEIGHT);
		(scale, (width - self.width * scale) / 2.0, (height - VIEW_HEIGHT * scale) / 2.0)
	}

	/// The box of the path in a square `<svg>` of `size`.
	pub fn box_in_square(&self, size: f64) -> IconBox {
		let (scale, x, y) = self.placement(size, size);
		let path = self.path_box();
		IconBox {
			x: x + path.x * scale,
			y: y + path.y * scale,
			width: path.width * scale,
			height: path.height * scale,
		}
	}

	/// The width of the icon in a label, in `em`: `calculateSize` rounds it up to two decimals.
	pub fn width_in_em(&self) -> f64 {
		(self.width / VIEW_HEIGHT * 100.0).ceil() / 100.0
	}

	/// Writes the icon as the inline box of a label: 1 em high, with its left side at `x` and
	/// the bottom of the box at `bottom`.
	pub fn write_inline(
		&self,
		svg: &mut Svg,
		(x, bottom): (f64, f64),
		font_size: f64,
		attributes: &dyn Fn(&mut Svg),
	) {
		let (scale, offset_x, offset_y) = self.placement(self.width_in_em() * font_size, font_size);
		// The scale in full: three decimals are 2 % of a typical icon.
		let transform = format!(
			"translate({},{}) scale({scale})",
			number(x + offset_x),
			number(bottom - font_size + offset_y),
		);
		svg.open("path").attr("class", "label-icon").attr("transform", &transform);
		attributes(svg);
		svg.attr("d", &self.path_data()).close();
	}
}

/// `getIconSVG` of a registered icon with both sizes given: the nested `<svg>` of `size` with the
/// path in `fill`.
pub fn write_glyph_svg(svg: &mut Svg, glyph: &IconGlyph, size: f64, attributes: &dyn Fn(&mut Svg)) {
	svg.open("svg")
		.attr("xmlns", "http://www.w3.org/2000/svg")
		.number_attr("width", size)
		.number_attr("height", size)
		.attr("viewBox", &format!("0 0 {} {}", number(glyph.width), number(VIEW_HEIGHT)))
		.end();
	svg.open("path");
	attributes(svg);
	svg.attr("d", &glyph.path_data()).close();
	svg.close_tag("svg");
}

/// `getIconSVG` of the unknown icon: the nested `<svg>` of `size`.
pub fn write_unknown_svg(svg: &mut Svg, size: f64) {
	svg.open("svg")
		.attr("xmlns", "http://www.w3.org/2000/svg")
		.number_attr("width", size)
		.number_attr("height", size)
		.attr("viewBox", "0 0 80 80")
		.end()
		.open("g")
		.end();
	write_unknown_icon(svg);
	svg.close_tag("g").close_tag("svg");
}

/// Reads the bits of [`PATHS`], each value with its lowest bit first.
struct BitReader {
	position: usize,
}

impl BitReader {
	/// The next `count` bits, at most 32. The table ends with 7 bytes of padding, so that the 8
	/// bytes around any bit of it can be loaded at once; beyond them, the bits are zeros.
	fn bits(&mut self, count: u32) -> u32 {
		let byte = self.position / 8;
		let window = PATHS
			.get(byte..byte + 8)
			.and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
			.unwrap_or_default();
		let value = u64::from_le_bytes(window) >> (self.position % 8);
		self.position += count as usize;
		(value & ((1 << count) - 1)) as u32
	}

	fn is_at_end(&self) -> bool {
		self.position >= PATHS.len() * 8
	}
}

/// A number as digits and a count of decimals.
#[derive(Clone, Copy, Default)]
struct Number {
	digits: i32,
	decimals: u8,
}

impl Number {
	/// The digits with the fewest decimals, as the paths of Font Awesome write them.
	fn new(mut digits: i32, mut decimals: u8) -> Self {
		while decimals > 0 && digits % 10 == 0 {
			digits /= 10;
			decimals -= 1;
		}
		Self { digits, decimals }
	}

	fn value(self) -> f64 {
		f64::from(self.digits) / [1.0, 10.0, 100.0, 1000.0][usize::from(self.decimals & 3)]
	}

	fn write(self, output: &mut String) {
		use std::fmt::Write as _;
		if self.digits < 0 {
			output.push('-');
		}
		let digits = self.digits.unsigned_abs();
		let unit = [1, 10, 100, 1000][usize::from(self.decimals & 3)];
		let _ = write!(output, "{}", digits / unit);
		if self.decimals > 0 {
			let _ =
				write!(output, ".{:0width$}", digits % unit, width = usize::from(self.decimals));
		}
	}
}

/// A path command with its numbers, without the flags of an arc.
struct Command {
	letter: u8,
	is_large_arc: bool,
	is_sweep: bool,
	numbers: [Number; 6],
	count: usize,
}

impl Command {
	fn is_arc(&self) -> bool {
		self.letter.eq_ignore_ascii_case(&b'a')
	}

	fn numbers(&self) -> &[Number] {
		&self.numbers[..self.count]
	}

	fn values(&self) -> [f64; 7] {
		let mut values = [0.0; 7];
		match self.is_arc() {
			// The flags are not among the numbers: the rotation stays at 2, the end moves by 2.
			true => {
				for (index, number) in self.numbers().iter().enumerate() {
					values[if index < 3 { index } else { index + 2 }] = number.value();
				}
			}
			false => {
				for (index, number) in self.numbers().iter().enumerate() {
					values[index] = number.value();
				}
			}
		}
		values
	}
}

struct CommandReader {
	reader: BitReader,
	/// Where the icon goes on after the shared sub-path that is being read.
	return_position: Option<usize>,
}

impl CommandReader {
	fn command(&mut self, code: u32) -> Option<Command> {
		let letter = *COMMANDS.get(code as usize)?;
		let count = match letter.to_ascii_uppercase() {
			b'H' | b'V' => 1,
			b'M' | b'L' | b'T' => 2,
			b'S' | b'Q' => 4,
			b'C' => 6,
			b'A' => 5,
			_ => 0,
		};
		let (mut is_large_arc, mut is_sweep) = (false, false);
		if letter.eq_ignore_ascii_case(&b'a') {
			is_large_arc = self.reader.bits(1) != 0;
			is_sweep = self.reader.bits(1) != 0;
		}
		let mut numbers = [Number::default(); 6];
		if count > 0 {
			let scale = self.reader.bits(SCALE_BITS) as u8;
			let width = self.reader.bits(NUMBER_WIDTH_BITS);
			for number in numbers.iter_mut().take(count) {
				let zigzag = self.reader.bits(width);
				*number = Number::new((zigzag >> 1) as i32 ^ -((zigzag & 1) as i32), scale);
			}
		}
		Some(Command { letter, is_large_arc, is_sweep, numbers, count })
	}
}

impl Iterator for CommandReader {
	type Item = Command;

	fn next(&mut self) -> Option<Command> {
		loop {
			if self.reader.is_at_end() {
				return None;
			}
			match self.reader.bits(CODE_BITS) {
				// Shared sub-paths refer to no others.
				SHARED_CODE if self.return_position.is_none() => {
					let index = self.reader.bits(SHARED_INDEX_BITS) as usize;
					let start = *SHARED_STARTS.get(index)? as usize;
					self.return_position = Some(self.reader.position);
					self.reader.position = start;
				}
				END_CODE => self.reader.position = self.return_position.take()?,
				code => return self.command(code),
			}
		}
	}
}

/// The extent of a path.
struct Extent {
	min: (f64, f64),
	max: (f64, f64),
}

impl Default for Extent {
	fn default() -> Self {
		Self { min: (f64::INFINITY, f64::INFINITY), max: (f64::NEG_INFINITY, f64::NEG_INFINITY) }
	}
}

impl Extent {
	fn include(&mut self, (x, y): (f64, f64)) {
		self.min = (self.min.0.min(x), self.min.1.min(y));
		self.max = (self.max.0.max(x), self.max.1.max(y));
	}

	fn include_cubic(
		&mut self,
		from: (f64, f64),
		first: (f64, f64),
		second: (f64, f64),
		to: (f64, f64),
	) {
		self.include(to);
		let axis = |p0: f64, p1: f64, p2: f64, p3: f64| {
			let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
			let b = 2.0 * (p0 - 2.0 * p1 + p2);
			let c = p1 - p0;
			let mut roots = [None, None];
			if a.abs() < 1e-12 {
				if b.abs() >= 1e-12 {
					roots[0] = Some(-c / b);
				}
			} else {
				let discriminant = b * b - 4.0 * a * c;
				if discriminant >= 0.0 {
					let root = discriminant.sqrt();
					roots = [Some((-b + root) / (2.0 * a)), Some((-b - root) / (2.0 * a))];
				}
			}
			roots
		};
		let point = |t: f64| {
			let u = 1.0 - t;
			let at = |p0: f64, p1: f64, p2: f64, p3: f64| {
				u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
			};
			(at(from.0, first.0, second.0, to.0), at(from.1, first.1, second.1, to.1))
		};
		let roots = axis(from.0, first.0, second.0, to.0)
			.into_iter()
			.chain(axis(from.1, first.1, second.1, to.1))
			.flatten();
		for t in roots {
			if t > 0.0 && t < 1.0 {
				self.include(point(t));
			}
		}
	}

	fn include_quadratic(&mut self, from: (f64, f64), control: (f64, f64), to: (f64, f64)) {
		let toward = |a: (f64, f64), b: (f64, f64)| {
			(a.0 + (b.0 - a.0) * 2.0 / 3.0, a.1 + (b.1 - a.1) * 2.0 / 3.0)
		};
		self.include_cubic(from, toward(from, control), toward(to, control), to);
	}

	/// An elliptical arc from `from` to `to` (SVG 1.1 F.6.5 and F.6.6).
	fn include_arc(
		&mut self,
		from: (f64, f64),
		to: (f64, f64),
		(rx, ry, rotation): (f64, f64, f64),
		is_large_arc: bool,
		is_sweep: bool,
	) {
		use std::f64::consts::{PI, TAU};
		self.include(to);
		let (mut rx, mut ry) = (rx.abs(), ry.abs());
		if rx == 0.0 || ry == 0.0 || from == to {
			return;
		}
		let (sin, cos) = rotation.to_radians().sin_cos();
		let (half_x, half_y) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
		let (x1, y1) = (cos * half_x + sin * half_y, -sin * half_x + cos * half_y);
		let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
		if lambda > 1.0 {
			rx *= lambda.sqrt();
			ry *= lambda.sqrt();
		}
		let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
		let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
		let mut factor = (numerator / denominator).max(0.0).sqrt();
		if is_large_arc == is_sweep {
			factor = -factor;
		}
		let (center_x1, center_y1) = (factor * rx * y1 / ry, -factor * ry * x1 / rx);
		let center_x = cos * center_x1 - sin * center_y1 + (from.0 + to.0) / 2.0;
		let center_y = sin * center_x1 + cos * center_y1 + (from.1 + to.1) / 2.0;
		let start = ((y1 - center_y1) / ry).atan2((x1 - center_x1) / rx);
		let end = ((-y1 - center_y1) / ry).atan2((-x1 - center_x1) / rx);
		let mut sweep = end - start;
		if is_sweep && sweep < 0.0 {
			sweep += TAU;
		} else if !is_sweep && sweep > 0.0 {
			sweep -= TAU;
		}
		let extremes = [(-ry * sin).atan2(rx * cos), (ry * cos).atan2(rx * sin)];
		for base in extremes {
			for angle in [base, base + PI] {
				// The offset of `angle` from the start in the direction of the arc.
				let offset = match is_sweep {
					true => (angle - start).rem_euclid(TAU),
					false => (start - angle).rem_euclid(TAU),
				};
				if offset <= sweep.abs() {
					let (sin_t, cos_t) = angle.sin_cos();
					self.include((
						center_x + rx * cos_t * cos - ry * sin_t * sin,
						center_y + rx * cos_t * sin + ry * sin_t * cos,
					));
				}
			}
		}
	}

	fn to_box(&self) -> IconBox {
		match self.min.0 <= self.max.0 {
			true => IconBox {
				x: self.min.0,
				y: self.min.1,
				width: self.max.0 - self.min.0,
				height: self.max.1 - self.min.1,
			},
			false => IconBox { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
		}
	}
}

/// The blue 80 × 80 tile of the architecture icons and of `unknownIcon`.
pub fn write_icon_tile(svg: &mut Svg) {
	write_icon_tile_with(svg, &|_| {});
}

/// The tile with further attributes, which style sheet rules of the diagram give it.
fn write_icon_tile_with(svg: &mut Svg, attributes: &dyn Fn(&mut Svg)) {
	svg.open("rect")
		.attr("width", "80")
		.attr("height", "80")
		.attr("fill", "#087ebf")
		.attr("stroke-width", "0");
	attributes(svg);
	svg.close();
}

/// The body of `unknownIcon`, which Mermaid draws for icons of packs that are not registered:
/// a white question mark on the tile, in a view box of 80 × 80.
pub fn write_unknown_icon(svg: &mut Svg) {
	write_unknown_icon_with(svg, &|_| {});
}

/// [`write_unknown_icon`] with further attributes of its tile.
pub fn write_unknown_icon_with(svg: &mut Svg, tile_attributes: &dyn Fn(&mut Svg)) {
	write_icon_tile_with(svg, tile_attributes);
	let font = Font::new("ArialMT, Arial", 67.75);
	let run = TextRun {
		x: 21.16,
		y: 64.67,
		anchor: Anchor::Start,
		baseline: Baseline::Alphabetic,
		font: &font,
		content: "?",
	};
	svg.text(run, |svg| _ = svg.attr("fill", "#fff"));
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn splits_references_like_iconify() {
		let resolved = |reference, fallback| resolve_reference(reference, fallback);
		assert_eq!(resolved("fa:user", Some("")), Some(("fa", "user")));
		assert_eq!(resolved("fa:fa-car", Some("")), Some(("fa", "fa-car")));
		// A dash separates the pack as well.
		assert_eq!(resolved("fa-user", Some("")), Some(("fa", "user")));
		assert_eq!(resolved("fa-solid-user", None), Some(("fa", "solid-user")));
		assert_eq!(resolved("user", Some("")), None);
		assert_eq!(resolved("user", Some("fa")), Some(("fa", "user")));
		assert_eq!(resolved("user", None), None);
		assert_eq!(resolved("a:fa:user", None), Some(("fa", "user")));
		assert_eq!(resolved("@a:fa:user", None), Some(("fa", "user")));
		assert_eq!(resolved("@a:user", Some("fa")), None);
		assert_eq!(resolved("a:b:c:d", None), None);
		assert_eq!(resolved("fa:", None), None);
		assert_eq!(resolved(":user", None), None);
		assert_eq!(resolved(":user", Some("fa")), Some(("fa", "user")));
		assert_eq!(resolved("", Some("fa")), None);
		assert_eq!(resolved("@", Some("fa")), None);
	}

	#[cfg(feature = "font-awesome")]
	mod with_table {
		use super::super::*;

		fn glyph(pack: &str, name: &str) -> IconGlyph {
			font_awesome(pack, name).expect("a known icon")
		}

		#[test]
		fn resolves_packs_aliases_and_shims() {
			assert!(font_awesome("fa", "car").is_some());
			assert_eq!(font_awesome("fas", "car"), font_awesome("fa6-solid", "car"));
			// An alias of the same style.
			assert_eq!(font_awesome("fa", "automobile"), font_awesome("fa", "car"));
			// The regular style has its own clock.
			assert_ne!(font_awesome("far", "clock"), font_awesome("fas", "clock"));
			assert_eq!(font_awesome("far", "car"), None);
			// Names of Font Awesome 4 may change the style, but only the short packs know them.
			assert_eq!(font_awesome("fa", "twitter"), font_awesome("fab", "twitter"));
			assert_eq!(font_awesome("fa", "clock-o"), font_awesome("far", "clock"));
			assert_eq!(font_awesome("fa6-solid", "twitter"), None);
			assert_eq!(font_awesome("fa6-regular", "clock-o"), None);
			for (pack, name) in [
				("fal", "car"),
				("fak", "car"),
				("fad", "car"),
				("logos", "aws"),
				("fa", "bogus"),
				("fa", "Car"),
				("fa", "fa-car"),
				("fa", ""),
				("fa", "car\n"),
				("fa", "zzzzzzzz"),
			] {
				assert_eq!(font_awesome(pack, name), None, "{pack}:{name}");
			}
		}

		#[test]
		fn finds_every_name_of_the_tables() {
			use super::super::super::icon_table::{BRANDS, NAMES, REGULAR, SHIMS, SOLID};
			for (pack, entries) in [
				("fa6-solid", &SOLID[..]),
				("fa6-regular", &REGULAR[..]),
				("fa6-brands", &BRANDS[..]),
			] {
				for &(start, icon) in entries {
					let name = NAMES[usize::from(start)..].split('\n').next().unwrap();
					let expected = icon_start(usize::from(icon)).map(IconGlyph::from_start);
					assert_eq!(font_awesome(pack, name), expected, "{pack}:{name}");
				}
			}
			assert_eq!(SHIMS.len(), 464);
		}

		#[test]
		fn decodes_paths() {
			let car = glyph("fa", "car");
			assert_eq!(car.width, 512.0);
			assert!(car.path_data().starts_with(
				"M135.2 117.4L109.1 192l293.8 0l-26.1-74.6C372.3 104.6 360.2 96 346.6 96L165.4 96"
			));
			let circle = glyph("fa", "user").path_data();
			assert!(
				circle.starts_with("M224 256A128 128 0 1 0 224 0a128 128 0 1 0 0 256"),
				"{circle}"
			);
			// Every icon has a closed outline in its view box, up to the shapes that poke out of it.
			for name in ["car", "user", "i", "ruler-horizontal", "truck-medical"] {
				let icon = glyph("fa", name);
				let bounds = icon.path_box();
				assert!(bounds.width > 0.0 && bounds.height > 0.0, "{name}");
				assert!(bounds.x >= 0.0 && bounds.x + bounds.width <= icon.width, "{name}");
				assert!(bounds.y >= 0.0 && bounds.y + bounds.height <= VIEW_HEIGHT, "{name}");
			}
		}

		#[test]
		fn decodes_every_icon() {
			let mut count = 0;
			while let Some(glyph) = icon_start(count).map(IconGlyph::from_start) {
				let data = glyph.path_data();
				assert!(data.starts_with('M'), "{count}: {data}");
				assert!(glyph.width > 0.0 && glyph.path_box().width > 0.0, "{count}");
				count += 1;
			}
			assert_eq!(count, 2060);
		}

		#[test]
		fn bounds_paths_tightly() {
			let close = |actual: f64, expected: f64| {
				assert!((actual - expected).abs() < 1e-3, "{actual} != {expected}")
			};
			let user = glyph("fa", "user").path_box();
			close(user.x, 0.0);
			close(user.y, 0.0);
			close(user.width, 448.0);
			close(user.height, 512.0);
			// Placed centered in a square, like the nested `<svg>`.
			let placed = glyph("fa", "user").box_in_square(48.0);
			close(placed.x, 3.0);
			close(placed.y, 0.0);
			close(placed.width, 42.0);
			close(placed.height, 48.0);
			let wide = glyph("fa", "ruler-horizontal");
			assert_eq!(wide.width, 640.0);
			let placed = wide.box_in_square(64.0);
			close(placed.x, wide.path_box().x / 10.0);
			close(placed.y, (64.0 - 51.2) / 2.0 + wide.path_box().y / 10.0);
		}

		#[test]
		fn rounds_widths_in_em_up() {
			assert_eq!(glyph("fa", "car").width_in_em(), 1.0);
			assert_eq!(glyph("fa", "user").width_in_em(), 0.88);
			assert_eq!(glyph("fa", "ruler-horizontal").width_in_em(), 1.25);
			assert_eq!(glyph("fa", "i").width_in_em(), 0.63);
		}

		#[test]
		fn maps_label_characters() {
			let character = icon_character("fa", "car").unwrap();
			assert_eq!(glyph_of_character(character), font_awesome("fa", "car"));
			assert_eq!(glyph_of_character('a'), None);
			assert_eq!(glyph_of_character('\u{10fffd}'), None);
			assert_eq!(icon_character("fa", "bogus"), None);
			assert_eq!(icon_character("fal", "car"), None);
		}

		#[test]
		fn resolves_registered_references() {
			assert_eq!(registered_glyph("fa:user", Some("")), font_awesome("fa", "user"));
			assert_eq!(registered_glyph("fa-user", Some("")), font_awesome("fa", "user"));
			assert_eq!(registered_glyph("user", Some("fa")), font_awesome("fa", "user"));
			assert_eq!(registered_glyph("user", Some("")), None);
			assert_eq!(registered_glyph("fa:fa-car", Some("")), None);
			assert_eq!(registered_glyph("logos:aws", Some("")), None);
		}
	}

	#[cfg(not(feature = "font-awesome"))]
	#[test]
	fn knows_no_icons_without_the_table() {
		assert_eq!(font_awesome("fa", "car"), None);
		assert_eq!(registered_glyph("fa:user", Some("")), None);
		assert_eq!(icon_character("fa", "car"), None);
		assert_eq!(glyph_of_character(char::from_u32(ICON_CHARACTERS_START).unwrap()), None);
	}
}
