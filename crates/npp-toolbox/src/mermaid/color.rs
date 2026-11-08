//! CSS colors and the color functions of `khroma`, which Mermaid's themes derive their colors
//! with.
//!
//! The functions return the same strings as `khroma`: an unchanged color keeps its text, a color
//! changed in HSL is written as `hsl()`, one changed in RGB as `#rrggbb` or `rgb()`, both with
//! `khroma`'s rounding. The themes chain these strings, so the precision matters for the
//! results. Instead of throwing for an unparsable color, the functions return `None`.

/// Which channels were set last, like `khroma`'s `TYPE`: getters of the other model convert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Model {
	All,
	Rgb,
	Hsl,
}

#[derive(Clone, Debug)]
pub struct Channels {
	red: Option<f64>,
	green: Option<f64>,
	blue: Option<f64>,
	hue: Option<f64>,
	saturation: Option<f64>,
	lightness: Option<f64>,
	alpha: f64,
	model: Model,
	changed: bool,
	/// The parsed text, returned while nothing changed.
	original: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Adjustment {
	pub red: f64,
	pub green: f64,
	pub blue: f64,
	pub hue: f64,
	pub saturation: f64,
	pub lightness: f64,
	pub alpha: f64,
}

impl Channels {
	fn from_rgb(red: f64, green: f64, blue: f64, alpha: f64, original: Option<String>) -> Self {
		Self {
			red: Some(red),
			green: Some(green),
			blue: Some(blue),
			hue: None,
			saturation: None,
			lightness: None,
			alpha,
			model: Model::All,
			changed: false,
			original,
		}
	}

	pub fn parse(text: &str) -> Option<Channels> {
		if let Some(hex) = text.strip_prefix('#') {
			let (red, green, blue, alpha) = parse_hex(hex)?;
			return Some(Self::from_rgb(red, green, blue, alpha, Some(text.to_owned())));
		}
		// Like khroma, keywords are the last resort, also for `red` or `hotpink`.
		let channels = match text.as_bytes().first()? {
			b'r' | b'R' => parse_rgb(text),
			b'h' | b'H' => parse_hsl(text),
			_ => None,
		};
		if channels.is_some() {
			return channels;
		}
		let lower = text.to_ascii_lowercase();
		if lower == "transparent" {
			return Some(Self::from_rgb(0.0, 0.0, 0.0, 0.0, Some(text.to_owned())));
		}
		let index = NAMED_COLORS.binary_search_by(|(name, _)| (*name).cmp(&lower)).ok()?;
		let value = NAMED_COLORS[index].1;
		let channel = |shift: u32| f64::from((value >> shift) & 0xff);
		// `khroma` parses the keyword's hex value, which becomes the original text.
		let hex = format!("#{value:06x}");
		Some(Self::from_rgb(channel(16), channel(8), channel(0), 1.0, Some(hex)))
	}

	fn ensure_hsl(&mut self) {
		let (red, green, blue) = (self.red_raw(), self.green_raw(), self.blue_raw());
		if self.hue.is_none() {
			self.hue = Some(rgb_to_hsl(red, green, blue).0);
		}
		if self.saturation.is_none() {
			self.saturation = Some(rgb_to_hsl(red, green, blue).1);
		}
		if self.lightness.is_none() {
			self.lightness = Some(rgb_to_hsl(red, green, blue).2);
		}
	}

	fn ensure_rgb(&mut self) {
		let hsl = (self.hue_raw(), self.saturation_raw(), self.lightness_raw());
		if self.red.is_none() {
			self.red = Some(hsl_to_rgb(hsl, 0));
		}
		if self.green.is_none() {
			self.green = Some(hsl_to_rgb(hsl, 1));
		}
		if self.blue.is_none() {
			self.blue = Some(hsl_to_rgb(hsl, 2));
		}
	}

	fn red_raw(&self) -> f64 {
		self.red.unwrap_or(f64::NAN)
	}

	fn green_raw(&self) -> f64 {
		self.green.unwrap_or(f64::NAN)
	}

	fn blue_raw(&self) -> f64 {
		self.blue.unwrap_or(f64::NAN)
	}

	fn hue_raw(&self) -> f64 {
		self.hue.unwrap_or(f64::NAN)
	}

	fn saturation_raw(&self) -> f64 {
		self.saturation.unwrap_or(f64::NAN)
	}

	fn lightness_raw(&self) -> f64 {
		self.lightness.unwrap_or(f64::NAN)
	}

	fn rgb_channel(&mut self, index: usize) -> f64 {
		let value = [self.red, self.green, self.blue][index];
		if self.model != Model::Hsl
			&& let Some(value) = value
		{
			return value;
		}
		self.ensure_hsl();
		hsl_to_rgb((self.hue_raw(), self.saturation_raw(), self.lightness_raw()), index)
	}

	fn hsl_channel(&mut self, index: usize) -> f64 {
		let value = [self.hue, self.saturation, self.lightness][index];
		if self.model != Model::Rgb
			&& let Some(value) = value
		{
			return value;
		}
		self.ensure_rgb();
		let hsl = rgb_to_hsl(self.red_raw(), self.green_raw(), self.blue_raw());
		[hsl.0, hsl.1, hsl.2][index]
	}

	pub fn red(&mut self) -> f64 {
		self.rgb_channel(0)
	}

	pub fn green(&mut self) -> f64 {
		self.rgb_channel(1)
	}

	pub fn blue(&mut self) -> f64 {
		self.rgb_channel(2)
	}

	pub fn hue(&mut self) -> f64 {
		self.hsl_channel(0)
	}

	pub fn saturation(&mut self) -> f64 {
		self.hsl_channel(1)
	}

	pub fn lightness(&mut self) -> f64 {
		self.hsl_channel(2)
	}

	pub fn alpha(&self) -> f64 {
		self.alpha
	}

	/// `None` where `khroma` throws for setting RGB and HSL channels on one color.
	fn set_model(&mut self, model: Model) -> Option<()> {
		if self.model != Model::All && self.model != model {
			return None;
		}
		self.model = model;
		self.changed = true;
		Some(())
	}

	fn set_rgb(&mut self, index: usize, value: f64) -> Option<()> {
		self.set_model(Model::Rgb)?;
		*[&mut self.red, &mut self.green, &mut self.blue][index] = Some(value);
		Some(())
	}

	fn set_hsl(&mut self, index: usize, value: f64) -> Option<()> {
		self.set_model(Model::Hsl)?;
		*[&mut self.hue, &mut self.saturation, &mut self.lightness][index] = Some(value);
		Some(())
	}

	fn set_alpha(&mut self, alpha: f64) {
		self.changed = true;
		self.alpha = alpha;
	}

	/// The text `khroma` writes for the channels.
	pub fn css_text(&mut self) -> String {
		if !self.changed
			&& let Some(original) = &self.original
		{
			return original.clone();
		}
		if self.model == Model::Hsl || self.red.is_none() {
			let (hue, saturation, lightness) = (self.hue(), self.saturation(), self.lightness());
			let (hue, saturation, lightness) = (
				js_number(round10(hue)),
				js_number(round10(saturation)),
				js_number(round10(lightness)),
			);
			return match self.alpha < 1.0 {
				true => {
					format!("hsla({hue}, {saturation}%, {lightness}%, {})", js_number(self.alpha))
				}
				false => format!("hsl({hue}, {saturation}%, {lightness}%)"),
			};
		}
		let (red, green, blue) = (self.red(), self.green(), self.blue());
		let is_integer = |value: f64| value.fract() == 0.0;
		if self.alpha < 1.0 || !is_integer(red) || !is_integer(green) || !is_integer(blue) {
			let (r, g, b) =
				(js_number(round10(red)), js_number(round10(green)), js_number(round10(blue)));
			return match self.alpha < 1.0 {
				true => format!("rgba({r}, {g}, {b}, {})", js_number(round10(self.alpha))),
				false => format!("rgb({r}, {g}, {b})"),
			};
		}
		let hex = |value: f64| match value.round() {
			rounded @ 0.0..=255.0 => format!("{:02x}", rounded as u8),
			_ => "undefined".to_owned(),
		};
		match self.alpha < 1.0 {
			true => format!("#{}{}{}{}", hex(red), hex(green), hex(blue), hex(self.alpha * 255.0)),
			false => format!("#{}{}{}", hex(red), hex(green), hex(blue)),
		}
	}

	/// The color for drawing: red, green, blue (0 to 255) and alpha (0 to 1).
	pub fn rgba(&mut self) -> [f64; 4] {
		[self.red(), self.green(), self.blue(), self.alpha]
	}

	/// `khroma`'s luminance, including its threshold on the unscaled channel.
	pub fn luminance(&mut self) -> f64 {
		let linear = |channel: f64| {
			let scaled = channel / 255.0;
			match channel > 0.03928 {
				true => ((scaled + 0.055) / 1.055).powf(2.4),
				false => scaled / 12.92,
			}
		};
		let (red, green, blue) = (self.red(), self.green(), self.blue());
		round10(0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue))
	}
}

/// JavaScript's `Math.round(number * 1e10) / 1e10`.
fn round10(value: f64) -> f64 {
	js_round(value * 1e10) / 1e10
}

/// JavaScript's `Math.round`, which rounds halves up.
pub fn js_round(value: f64) -> f64 {
	(value + 0.5).floor()
}

/// A number as JavaScript's `String(number)` writes it.
pub fn js_number(number: f64) -> String {
	if number.is_nan() {
		return "NaN".to_owned();
	}
	if number.is_infinite() {
		return if number > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
	}
	if number == 0.0 {
		return "0".to_owned();
	}
	let magnitude = number.abs();
	if (1e-7..1e21).contains(&magnitude) {
		return format!("{number}");
	}
	// Exponential notation like JavaScript: `1e-7`, `1.5e+21`.
	let formatted = format!("{number:e}");
	match formatted.split_once('e') {
		Some((mantissa, exponent)) if !exponent.starts_with('-') => {
			format!("{mantissa}e+{exponent}")
		}
		_ => formatted,
	}
}

fn rgb_to_hsl(red: f64, green: f64, blue: f64) -> (f64, f64, f64) {
	let (red, green, blue) = (red / 255.0, green / 255.0, blue / 255.0);
	let max = red.max(green).max(blue);
	let min = red.min(green).min(blue);
	let lightness = (max + min) / 2.0;
	if max == min {
		return (0.0, 0.0, lightness * 100.0);
	}
	let delta = max - min;
	let saturation = match lightness > 0.5 {
		true => delta / (2.0 - max - min),
		false => delta / (max + min),
	};
	let hue = if max == red {
		(green - blue) / delta + if green < blue { 6.0 } else { 0.0 }
	} else if max == green {
		(blue - red) / delta + 2.0
	} else {
		(red - green) / delta + 4.0
	};
	(hue * 60.0, saturation * 100.0, lightness * 100.0)
}

/// Channel `index` (red, green, blue) of `(hue, saturation, lightness)`.
fn hsl_to_rgb((hue, saturation, lightness): (f64, f64, f64), index: usize) -> f64 {
	// JavaScript's `!s` also treats NaN as achromatic.
	if saturation == 0.0 || saturation.is_nan() {
		return lightness * 2.55;
	}
	let (hue, saturation, lightness) = (hue / 360.0, saturation / 100.0, lightness / 100.0);
	let q = match lightness < 0.5 {
		true => lightness * (1.0 + saturation),
		false => lightness + saturation - lightness * saturation,
	};
	let p = 2.0 * lightness - q;
	let offset = [1.0 / 3.0, 0.0, -1.0 / 3.0][index];
	hue_to_rgb(p, q, hue + offset) * 255.0
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
	if t < 0.0 {
		t += 1.0;
	}
	if t > 1.0 {
		t -= 1.0;
	}
	if t < 1.0 / 6.0 {
		return p + (q - p) * 6.0 * t;
	}
	if t < 0.5 {
		return q;
	}
	if t < 2.0 / 3.0 {
		return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
	}
	p
}

fn clamp_rgb(value: f64) -> f64 {
	value.clamp(0.0, 255.0)
}

fn clamp_percent(value: f64) -> f64 {
	value.clamp(0.0, 100.0)
}

fn clamp_alpha(value: f64) -> f64 {
	value.clamp(0.0, 1.0)
}

/// JavaScript's `%`, which keeps the sign of the dividend.
fn clamp_hue(value: f64) -> f64 {
	value % 360.0
}

fn parse_hex(hex: &str) -> Option<(f64, f64, f64, f64)> {
	if !matches!(hex.len(), 3 | 4 | 6 | 8) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
		return None;
	}
	let value = u32::from_str_radix(hex, 16).ok()?;
	let is_full_length = hex.len() > 4;
	let has_alpha = hex.len().is_multiple_of(4);
	let (bits, mask, multiplier) = match is_full_length {
		true => (8, 0xff, 1.0),
		false => (4, 0xf, 17.0),
	};
	let offset = if has_alpha { 0 } else { -1 };
	let channel =
		|position: i32| f64::from((value >> (bits * (offset + position))) & mask) * multiplier;
	let alpha = match has_alpha {
		true => f64::from(value & mask) * multiplier / 255.0,
		false => 1.0,
	};
	Some((channel(3), channel(2), channel(1), alpha))
}

/// A number of `rgb()` or `hsl()` as `khroma`'s regular expressions accept it, with its unit.
fn function_number(text: &str) -> Option<(f64, &str)> {
	let end = text
		.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E')))
		.unwrap_or(text.len());
	let number: f64 = text[..end].parse().ok()?;
	number.is_finite().then_some((number, &text[end..]))
}

/// The three or four arguments of `name(…)`, separated by commas, whitespace or `/`.
fn function_arguments<'a>(text: &'a str, names: [&str; 2]) -> Option<Vec<&'a str>> {
	let lower_start = text.get(..names[1].len().min(text.len()))?.to_ascii_lowercase();
	let rest = match () {
		_ if lower_start == names[1] => &text[names[1].len()..],
		_ if lower_start.starts_with(names[0]) => &text[names[0].len()..],
		_ => return None,
	};
	let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
	let arguments: Vec<&str> = inner
		.split([',', '/', ' ', '\t', '\n'])
		.map(|argument| argument.trim().trim_start_matches('+'))
		.filter(|argument| !argument.is_empty())
		.collect();
	matches!(arguments.len(), 3 | 4).then_some(arguments)
}

fn parse_alpha(argument: Option<&&str>) -> Option<f64> {
	let Some(argument) = argument else { return Some(1.0) };
	let (number, unit) = function_number(argument)?;
	match unit {
		"%" => Some(clamp_alpha(number / 100.0)),
		"" => Some(clamp_alpha(number)),
		_ => None,
	}
}

fn parse_rgb(text: &str) -> Option<Channels> {
	let arguments = function_arguments(text, ["rgb", "rgba"])?;
	let channel = |argument: &str| -> Option<f64> {
		let (number, unit) = function_number(argument)?;
		match unit {
			"%" => Some(clamp_rgb(number * 2.55)),
			"" => Some(clamp_rgb(number)),
			_ => None,
		}
	};
	let (red, green, blue) =
		(channel(arguments[0])?, channel(arguments[1])?, channel(arguments[2])?);
	let alpha = parse_alpha(arguments.get(3))?;
	Some(Channels::from_rgb(red, green, blue, alpha, Some(text.to_owned())))
}

fn parse_hsl(text: &str) -> Option<Channels> {
	let arguments = function_arguments(text, ["hsl", "hsla"])?;
	let (hue, unit) = function_number(arguments[0])?;
	let hue = clamp_hue(match unit.to_ascii_lowercase().as_str() {
		"" | "deg" => hue,
		"grad" => hue * 0.9,
		"rad" => hue * 180.0 / std::f64::consts::PI,
		"turn" => hue * 360.0,
		_ => return None,
	});
	let percent = |argument: &str| -> Option<f64> {
		let (number, unit) = function_number(argument)?;
		(unit == "%").then(|| clamp_percent(number))
	};
	let (saturation, lightness) = (percent(arguments[1])?, percent(arguments[2])?);
	let alpha = parse_alpha(arguments.get(3))?;
	Some(Channels {
		red: None,
		green: None,
		blue: None,
		hue: Some(hue),
		saturation: Some(saturation),
		lightness: Some(lightness),
		alpha,
		model: Model::All,
		changed: false,
		original: Some(text.to_owned()),
	})
}

/// `khroma`'s `change`: sets the given channels, clamped.
fn change(color: &str, adjustment: &dyn Fn(&mut Channels) -> [Option<f64>; 7]) -> Option<String> {
	let mut channels = Channels::parse(color)?;
	let values = adjustment(&mut channels);
	for (index, value) in values.into_iter().enumerate() {
		let Some(value) = value else { continue };
		match index {
			0..=2 => channels.set_rgb(index, clamp_rgb(value))?,
			3 => channels.set_hsl(0, clamp_hue(value))?,
			4 | 5 => channels.set_hsl(index - 3, clamp_percent(value))?,
			_ => channels.set_alpha(clamp_alpha(value)),
		}
	}
	Some(channels.css_text())
}

/// Adds the non-zero channels of `adjustment`, in `khroma`'s channel order.
pub fn adjust(color: &str, adjustment: Adjustment) -> Option<String> {
	change(color, &|channels| {
		let changes = [
			adjustment.red,
			adjustment.green,
			adjustment.blue,
			adjustment.hue,
			adjustment.saturation,
			adjustment.lightness,
			adjustment.alpha,
		];
		// `khroma` skips falsy amounts (0 and NaN).
		let mut values = [None; 7];
		for (index, amount) in changes.into_iter().enumerate() {
			if amount != 0.0 && !amount.is_nan() {
				let current = match index {
					0 => channels.red(),
					1 => channels.green(),
					2 => channels.blue(),
					3 => channels.hue(),
					4 => channels.saturation(),
					5 => channels.lightness(),
					_ => channels.alpha(),
				};
				values[index] = Some(current + amount);
			}
		}
		values
	})
}

/// `khroma`'s `adjustChannel` for lightness: sets it only if it changes.
fn adjust_lightness(color: &str, amount: f64) -> Option<String> {
	let mut channels = Channels::parse(color)?;
	let current = channels.lightness();
	let next = clamp_percent(current + amount);
	// `!==` is true for NaN.
	if current != next || next.is_nan() {
		channels.set_hsl(2, next)?;
	}
	Some(channels.css_text())
}

pub fn lighten(color: &str, amount: f64) -> Option<String> {
	adjust_lightness(color, amount)
}

pub fn darken(color: &str, amount: f64) -> Option<String> {
	adjust_lightness(color, -amount)
}

pub fn transparentize(color: &str, amount: f64) -> Option<String> {
	let mut channels = Channels::parse(color)?;
	let current = channels.alpha();
	let next = clamp_alpha(current - amount);
	if current != next || next.is_nan() {
		channels.set_alpha(next);
	}
	Some(channels.css_text())
}

/// Whether `text` is a CSS color: a keyword, `#hex`, `rgb()` or `hsl()`.
pub fn is_color(text: &str) -> bool {
	Channels::parse(text).is_some()
}

/// `khroma`'s `rgba(r, g, b, a)`.
pub fn rgba_of_numbers(red: f64, green: f64, blue: f64, alpha: f64) -> String {
	let mut channels = Channels::from_rgb(
		clamp_rgb(red),
		clamp_rgb(green),
		clamp_rgb(blue),
		clamp_alpha(alpha),
		None,
	);
	channels.css_text()
}

/// `khroma`'s `rgba(color, alpha)`.
pub fn with_alpha(color: &str, alpha: f64) -> Option<String> {
	change(color, &|_| [None, None, None, None, None, None, Some(alpha)])
}

fn mix_channels(first: [f64; 4], second: [f64; 4], weight: f64) -> String {
	let scale = weight / 100.0;
	let normalized = scale * 2.0 - 1.0;
	let alpha_delta = first[3] - second[3];
	let combined = match normalized * alpha_delta == -1.0 {
		true => normalized,
		false => (normalized + alpha_delta) / (1.0 + normalized * alpha_delta),
	};
	let weight1 = (combined + 1.0) / 2.0;
	let weight2 = 1.0 - weight1;
	rgba_of_numbers(
		first[0] * weight1 + second[0] * weight2,
		first[1] * weight1 + second[1] * weight2,
		first[2] * weight1 + second[2] * weight2,
		first[3] * scale + second[3] * (1.0 - scale),
	)
}

pub fn invert(color: &str, weight: f64) -> Option<String> {
	let original = Channels::parse(color)?.rgba();
	let inverse = [255.0 - original[0], 255.0 - original[1], 255.0 - original[2], original[3]];
	Some(mix_channels(inverse, original, weight))
}

pub fn is_dark(color: &str) -> Option<bool> {
	Some(Channels::parse(color)?.luminance() < 0.5)
}

/// Mermaid's `mkBorder` of `theme-helpers.js`.
pub fn make_border(color: &str, is_dark_mode: bool) -> Option<String> {
	let lightness = if is_dark_mode { 10.0 } else { -10.0 };
	adjust(color, Adjustment { saturation: -40.0, lightness, ..Adjustment::default() })
}

/// The CSS named colors, sorted for binary search.
const NAMED_COLORS: [(&str, u32); 148] = [
	("aliceblue", 0xf0f8ff),
	("antiquewhite", 0xfaebd7),
	("aqua", 0x00ffff),
	("aquamarine", 0x7fffd4),
	("azure", 0xf0ffff),
	("beige", 0xf5f5dc),
	("bisque", 0xffe4c4),
	("black", 0x000000),
	("blanchedalmond", 0xffebcd),
	("blue", 0x0000ff),
	("blueviolet", 0x8a2be2),
	("brown", 0xa52a2a),
	("burlywood", 0xdeb887),
	("cadetblue", 0x5f9ea0),
	("chartreuse", 0x7fff00),
	("chocolate", 0xd2691e),
	("coral", 0xff7f50),
	("cornflowerblue", 0x6495ed),
	("cornsilk", 0xfff8dc),
	("crimson", 0xdc143c),
	("cyan", 0x00ffff),
	("darkblue", 0x00008b),
	("darkcyan", 0x008b8b),
	("darkgoldenrod", 0xb8860b),
	("darkgray", 0xa9a9a9),
	("darkgreen", 0x006400),
	("darkgrey", 0xa9a9a9),
	("darkkhaki", 0xbdb76b),
	("darkmagenta", 0x8b008b),
	("darkolivegreen", 0x556b2f),
	("darkorange", 0xff8c00),
	("darkorchid", 0x9932cc),
	("darkred", 0x8b0000),
	("darksalmon", 0xe9967a),
	("darkseagreen", 0x8fbc8f),
	("darkslateblue", 0x483d8b),
	("darkslategray", 0x2f4f4f),
	("darkslategrey", 0x2f4f4f),
	("darkturquoise", 0x00ced1),
	("darkviolet", 0x9400d3),
	("deeppink", 0xff1493),
	("deepskyblue", 0x00bfff),
	("dimgray", 0x696969),
	("dimgrey", 0x696969),
	("dodgerblue", 0x1e90ff),
	("firebrick", 0xb22222),
	("floralwhite", 0xfffaf0),
	("forestgreen", 0x228b22),
	("fuchsia", 0xff00ff),
	("gainsboro", 0xdcdcdc),
	("ghostwhite", 0xf8f8ff),
	("gold", 0xffd700),
	("goldenrod", 0xdaa520),
	("gray", 0x808080),
	("green", 0x008000),
	("greenyellow", 0xadff2f),
	("grey", 0x808080),
	("honeydew", 0xf0fff0),
	("hotpink", 0xff69b4),
	("indianred", 0xcd5c5c),
	("indigo", 0x4b0082),
	("ivory", 0xfffff0),
	("khaki", 0xf0e68c),
	("lavender", 0xe6e6fa),
	("lavenderblush", 0xfff0f5),
	("lawngreen", 0x7cfc00),
	("lemonchiffon", 0xfffacd),
	("lightblue", 0xadd8e6),
	("lightcoral", 0xf08080),
	("lightcyan", 0xe0ffff),
	("lightgoldenrodyellow", 0xfafad2),
	("lightgray", 0xd3d3d3),
	("lightgreen", 0x90ee90),
	("lightgrey", 0xd3d3d3),
	("lightpink", 0xffb6c1),
	("lightsalmon", 0xffa07a),
	("lightseagreen", 0x20b2aa),
	("lightskyblue", 0x87cefa),
	("lightslategray", 0x778899),
	("lightslategrey", 0x778899),
	("lightsteelblue", 0xb0c4de),
	("lightyellow", 0xffffe0),
	("lime", 0x00ff00),
	("limegreen", 0x32cd32),
	("linen", 0xfaf0e6),
	("magenta", 0xff00ff),
	("maroon", 0x800000),
	("mediumaquamarine", 0x66cdaa),
	("mediumblue", 0x0000cd),
	("mediumorchid", 0xba55d3),
	("mediumpurple", 0x9370db),
	("mediumseagreen", 0x3cb371),
	("mediumslateblue", 0x7b68ee),
	("mediumspringgreen", 0x00fa9a),
	("mediumturquoise", 0x48d1cc),
	("mediumvioletred", 0xc71585),
	("midnightblue", 0x191970),
	("mintcream", 0xf5fffa),
	("mistyrose", 0xffe4e1),
	("moccasin", 0xffe4b5),
	("navajowhite", 0xffdead),
	("navy", 0x000080),
	("oldlace", 0xfdf5e6),
	("olive", 0x808000),
	("olivedrab", 0x6b8e23),
	("orange", 0xffa500),
	("orangered", 0xff4500),
	("orchid", 0xda70d6),
	("palegoldenrod", 0xeee8aa),
	("palegreen", 0x98fb98),
	("paleturquoise", 0xafeeee),
	("palevioletred", 0xdb7093),
	("papayawhip", 0xffefd5),
	("peachpuff", 0xffdab9),
	("peru", 0xcd853f),
	("pink", 0xffc0cb),
	("plum", 0xdda0dd),
	("powderblue", 0xb0e0e6),
	("purple", 0x800080),
	("rebeccapurple", 0x663399),
	("red", 0xff0000),
	("rosybrown", 0xbc8f8f),
	("royalblue", 0x4169e1),
	("saddlebrown", 0x8b4513),
	("salmon", 0xfa8072),
	("sandybrown", 0xf4a460),
	("seagreen", 0x2e8b57),
	("seashell", 0xfff5ee),
	("sienna", 0xa0522d),
	("silver", 0xc0c0c0),
	("skyblue", 0x87ceeb),
	("slateblue", 0x6a5acd),
	("slategray", 0x708090),
	("slategrey", 0x708090),
	("snow", 0xfffafa),
	("springgreen", 0x00ff7f),
	("steelblue", 0x4682b4),
	("tan", 0xd2b48c),
	("teal", 0x008080),
	("thistle", 0xd8bfd8),
	("tomato", 0xff6347),
	("turquoise", 0x40e0d0),
	("violet", 0xee82ee),
	("wheat", 0xf5deb3),
	("white", 0xffffff),
	("whitesmoke", 0xf5f5f5),
	("yellow", 0xffff00),
	("yellowgreen", 0x9acd32),
];

#[cfg(test)]
mod tests {
	use super::*;

	fn parse_rgba(text: &str) -> Option<[f64; 4]> {
		Channels::parse(text).map(|mut channels| channels.rgba())
	}

	#[test]
	fn named_colors_are_sorted() {
		assert!(NAMED_COLORS.windows(2).all(|pair| pair[0].0 < pair[1].0));
	}

	/// Results of `khroma` 2.1.0 under Node.js.
	#[test]
	fn matches_khroma() {
		let hue = |hue| Adjustment { hue, ..Adjustment::default() };
		assert_eq!(
			adjust("#ECECFF", hue(-160.0)).as_deref(),
			Some("hsl(80, 100%, 96.2745098039%)")
		);
		assert_eq!(make_border("#ECECFF", false).as_deref(), Some("hsl(240, 60%, 86.2745098039%)"));
		assert_eq!(
			invert("hsl(80, 100%, 96.2745098039%)", 100.0).as_deref(),
			Some("rgb(6.3333333334, 0, 19.0000000001)")
		);
		assert_eq!(invert("#ECECFF", 100.0).as_deref(), Some("#131300"));
		assert_eq!(adjust("#ECECFF", hue(0.0)).as_deref(), Some("#ECECFF"));
		assert_eq!(lighten("#ECECFF", f64::NAN).as_deref(), Some("hsl(240, 100%, NaN%)"));
		assert_eq!(rgba_of_numbers(102.0, 102.0, 255.0, 0.49), "rgba(102, 102, 255, 0.49)");
		assert_eq!(darken("lightgrey", 0.0).as_deref(), Some("#d3d3d3"));
	}

	#[test]
	fn parses_css_colors() {
		assert_eq!(parse_rgba("#fff"), Some([255.0, 255.0, 255.0, 1.0]));
		assert_eq!(parse_rgba("#00000080").map(|color| color[3]), Some(128.0 / 255.0));
		assert_eq!(parse_rgba("rgba(232,232,232, 0.8)"), Some([232.0, 232.0, 232.0, 0.8]));
		assert_eq!(parse_rgba("rgb(0 0 0 / 50%)").map(|color| color[3]), Some(0.5));
		assert_eq!(parse_rgba("hsl(120, 100%, 25%)").map(|color| color[1].round()), Some(128.0));
		assert_eq!(parse_rgba("LightGrey"), parse_rgba("#d3d3d3"));
		for invalid in ["#12", "rgb(1,2)", "hsl(nan, 1%, 1%)", "url(#x)", "", "r", "rgb(", "#ggg"] {
			assert_eq!(parse_rgba(invalid), None, "{invalid}");
		}
	}

	#[test]
	fn writes_numbers_like_javascript() {
		assert_eq!(js_number(96.2745098039), "96.2745098039");
		assert_eq!(js_number(-0.0), "0");
		assert_eq!(js_number(1e-10), "1e-10");
		assert_eq!(js_number(16.0), "16");
	}
}
