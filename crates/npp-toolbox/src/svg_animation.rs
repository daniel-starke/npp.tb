//! Converts the SMIL animations of SVG images (`<animate>`, `<set>`, `<animateTransform>`) into
//! CSS animations, for EdgeHTML, which runs CSS animations in SVG images but never supported SMIL.
//!
//! Only what CSS expresses the same way is converted: presentation attributes that are animatable
//! CSS properties, transforms, a single begin offset, repetitions, `fill="freeze"` and the
//! calculation modes with `keyTimes` and `keySplines`. Other animations are left out, so their
//! elements keep the static state. The SVG is scanned without a full XML parser; anything
//! unexpected leaves the image unchanged.

use std::collections::HashMap;
use std::fmt::Write;

use crate::image_embedding::EmbeddedImage;

const SVG_MEDIA_TYPE: &str = "image/svg+xml";
/// Marks animated elements without a usable id for the selectors of their animations.
const MARKER_ATTRIBUTE: &str = "data-npp-smil";
const KEYFRAMES_PREFIX: &str = "npp-smil-";

/// Presentation attributes that EdgeHTML animates as CSS properties, and whether they are lengths,
/// which CSS needs with a unit.
const CSS_PROPERTIES: [(&str, bool); 13] = [
	("color", false),
	("fill", false),
	("fill-opacity", false),
	("flood-color", false),
	("flood-opacity", false),
	("opacity", false),
	("stop-color", false),
	("stop-opacity", false),
	("stroke", false),
	("stroke-dashoffset", true),
	("stroke-opacity", false),
	("stroke-width", true),
	("visibility", false),
];

/// `image` with its SMIL animations as CSS animations, `None` if it is no SVG or has nothing to
/// convert.
pub fn css_animated_image(image: &EmbeddedImage) -> Option<EmbeddedImage> {
	if image.media_type != SVG_MEDIA_TYPE {
		return None;
	}
	let svg = std::str::from_utf8(&image.bytes).ok()?;
	let converted = smil_as_css(svg)?;
	Some(EmbeddedImage { media_type: image.media_type, bytes: converted.into_bytes() })
}

/// `svg` with a style sheet that plays its SMIL animations as CSS animations. `None` if no
/// animation could be converted or the markup is not understood.
pub fn smil_as_css(svg: &str) -> Option<String> {
	let mut stack: Vec<OpenElement> = Vec::new();
	let mut style_position = None;
	let mut insertions: Vec<(usize, String)> = Vec::new();
	let mut rules: Vec<(String, Vec<String>)> = Vec::new();
	let mut rule_of_selector: HashMap<String, usize> = HashMap::new();
	let mut keyframes = String::new();
	let mut marker_count = 0;
	let mut animation_count = 0;
	let mut position = 0;
	while let Some(offset) = svg[position..].find('<') {
		let start = position + offset;
		let rest = &svg[start..];
		if rest.starts_with("<!--") {
			position = start + rest.find("-->")? + 3;
		} else if rest.starts_with("<![CDATA[") {
			position = start + rest.find("]]>")? + 3;
		} else if rest.starts_with("<?") {
			position = start + rest.find("?>")? + 2;
		} else if rest.starts_with("<!") {
			// A document type declaration. An internal subset could declare entities.
			let end = rest.find('>')?;
			if rest[..end].contains('[') {
				return None;
			}
			position = start + end + 1;
		} else if let Some(name) = rest.strip_prefix("</") {
			let end = name.find('>')?;
			if stack.pop()?.name != name[..end].trim_end() {
				return None;
			}
			position = start + 2 + end + 1;
		} else {
			let tag = StartTag::parse(rest)?;
			if stack.is_empty() {
				if style_position.is_some() || tag.name != "svg" || tag.is_empty {
					return None;
				}
				style_position = Some(start + tag.length);
			}
			if let Some(parent) = stack.last_mut()
				&& let Some((target, animation)) = Animation::of(&tag, parent)
			{
				let selector = match target {
					Target::Id(id) => format!("[id=\"{id}\"]"),
					Target::Parent => match parent.id.filter(|id| is_safe_id(id)) {
						Some(id) => format!("[id=\"{id}\"]"),
						None => {
							let marker = match parent.marker {
								Some(marker) => marker,
								None => {
									let marker = marker_count;
									marker_count += 1;
									parent.marker = Some(marker);
									let attribute = format!(" {MARKER_ATTRIBUTE}=\"{marker}\"");
									insertions.push((parent.name_end, attribute));
									marker
								}
							};
							format!("[{MARKER_ATTRIBUTE}=\"{marker}\"]")
						}
					},
				};
				let name = format!("{KEYFRAMES_PREFIX}{animation_count}");
				animation_count += 1;
				let _ = writeln!(keyframes, "@keyframes {name}{{{}}}", animation.keyframes);
				let index = *rule_of_selector.entry(selector.clone()).or_insert_with(|| {
					rules.push((selector, Vec::new()));
					rules.len() - 1
				});
				rules[index].1.push(animation.shorthand(&name));
			}
			if !tag.is_empty {
				stack.push(OpenElement {
					name: tag.name,
					name_end: start + 1 + tag.name.len(),
					id: tag.attribute("id"),
					has_transform: tag.attribute("transform").is_some(),
					marker: None,
				});
			}
			position = start + tag.length;
		}
	}
	if rules.is_empty() || !stack.is_empty() {
		return None;
	}
	let mut style = String::from("<style><![CDATA[\n");
	style.push_str(&keyframes);
	for (selector, animations) in &rules {
		let _ = writeln!(style, "{selector}{{animation:{}}}", animations.join(","));
	}
	style.push_str("]]></style>");
	insertions.push((style_position?, style));
	insertions.sort_by_key(|(position, _)| *position);
	let mut converted =
		String::with_capacity(svg.len() + insertions.iter().map(|i| i.1.len()).sum::<usize>());
	let mut copied = 0;
	for (position, text) in &insertions {
		converted.push_str(&svg[copied..*position]);
		converted.push_str(text);
		copied = *position;
	}
	converted.push_str(&svg[copied..]);
	Some(converted)
}

struct OpenElement<'a> {
	name: &'a str,
	/// Where attributes can be added to the start tag.
	name_end: usize,
	id: Option<&'a str>,
	has_transform: bool,
	/// The value of its [`MARKER_ATTRIBUTE`] once added.
	marker: Option<usize>,
}

struct StartTag<'a> {
	name: &'a str,
	attributes: Vec<(&'a str, &'a str)>,
	/// Bytes up to and including `>`.
	length: usize,
	/// `<name/>`
	is_empty: bool,
}

impl<'a> StartTag<'a> {
	/// The start tag at the beginning of `text`, which starts with `<`.
	fn parse(text: &'a str) -> Option<Self> {
		let bytes = text.as_bytes();
		let is_delimiter = |byte: u8| byte.is_ascii_whitespace() || byte == b'>' || byte == b'/';
		let name_length = bytes[1..].iter().position(|&byte| is_delimiter(byte))?;
		let name = &text[1..1 + name_length];
		if name.is_empty() || name.contains(['<', '=', '"', '\'']) {
			return None;
		}
		let mut attributes = Vec::new();
		let mut index = 1 + name_length;
		loop {
			while bytes.get(index)?.is_ascii_whitespace() {
				index += 1;
			}
			match bytes[index] {
				b'>' => {
					return Some(StartTag { name, attributes, length: index + 1, is_empty: false });
				}
				b'/' if bytes.get(index + 1) == Some(&b'>') => {
					return Some(StartTag { name, attributes, length: index + 2, is_empty: true });
				}
				_ => {}
			}
			let name_start = index;
			while !is_delimiter(*bytes.get(index)?) && bytes[index] != b'=' {
				index += 1;
			}
			let attribute_name = &text[name_start..index];
			if attribute_name.is_empty() {
				return None;
			}
			while bytes.get(index)?.is_ascii_whitespace() {
				index += 1;
			}
			if bytes[index] != b'=' {
				return None;
			}
			index += 1;
			while bytes.get(index)?.is_ascii_whitespace() {
				index += 1;
			}
			let quote = bytes[index];
			if quote != b'"' && quote != b'\'' {
				return None;
			}
			let value_start = index + 1;
			let value_length = bytes[value_start..].iter().position(|&byte| byte == quote)?;
			attributes.push((attribute_name, &text[value_start..value_start + value_length]));
			index = value_start + value_length + 1;
		}
	}

	fn attribute(&self, name: &str) -> Option<&'a str> {
		self.attributes
			.iter()
			.find(|(attribute, _)| *attribute == name)
			.map(|(_, value)| value.trim())
	}
}

enum Target<'a> {
	Parent,
	Id(&'a str),
}

struct Animation {
	/// The content of `@keyframes`.
	keyframes: String,
	duration: f64,
	delay: f64,
	/// `None` repeats forever.
	iterations: Option<f64>,
	timing_function: &'static str,
	freezes: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CalculationMode {
	Discrete,
	Linear,
	Spline,
}

impl Animation {
	/// The CSS animation of the SMIL animation element `tag` inside `parent`, if CSS can play it.
	fn of<'a>(tag: &StartTag<'a>, parent: &OpenElement) -> Option<(Target<'a>, Animation)> {
		let target = match tag.attribute("href").or_else(|| tag.attribute("xlink:href")) {
			Some(reference) => Target::Id(reference.strip_prefix('#').filter(|id| is_safe_id(id))?),
			None => Target::Parent,
		};
		let is_set = tag.name == "set";
		let declarations = match tag.name {
			"animate" | "set" => property_values(tag)?,
			"animateTransform" => {
				let is_sum = tag.attribute("additive") == Some("sum");
				// A sum adds to the element's own transform, which a CSS transform replaces.
				if is_sum && (matches!(target, Target::Id(_)) || parent.has_transform) {
					return None;
				}
				transform_values(tag)?
			}
			_ => return None,
		};
		if tag.attribute("end").is_some()
			|| tag.attribute("accumulate") == Some("sum")
			|| (tag.name != "animateTransform" && tag.attribute("additive") == Some("sum"))
		{
			return None;
		}
		let delay = match tag.attribute("begin") {
			Some(begin) => parse_offset(begin)?,
			None => 0.0,
		};
		let simple_duration = match tag.attribute("dur") {
			None | Some("indefinite") => None,
			Some(duration) => Some(parse_clock(duration).filter(|&duration| duration > 0.0)?),
		};
		let fill_freezes = tag.attribute("fill") == Some("freeze");

		if is_set {
			// The value holds for the duration, or for ever without one.
			let keyframes = format!("0%,100%{{{}}}", declarations.values.first()?);
			let (duration, iterations, freezes) = match simple_duration {
				Some(duration) => (duration, Some(iterations(tag, duration)?), fill_freezes),
				None => (1.0, Some(1.0), true),
			};
			let animation = Animation {
				keyframes,
				duration,
				delay,
				iterations: iterations.filter(|count| count.is_finite()),
				timing_function: "step-end",
				freezes,
			};
			return Some((target, animation));
		}

		let duration = simple_duration?;
		let mode = match tag.attribute("calcMode") {
			Some("discrete") => CalculationMode::Discrete,
			Some("spline") => CalculationMode::Spline,
			Some("linear" | "paced") | None => CalculationMode::Linear,
			Some(_) => return None,
		};
		let values = &declarations.values;
		let count = values.len();
		let mut keyframes = String::new();
		if declarations.is_to_animation {
			// Interpolates from the underlying value, which a keyframe at 0% would replace.
			if mode != CalculationMode::Linear || tag.attribute("keyTimes").is_some() {
				return None;
			}
			let _ = write!(keyframes, "100%{{{}}}", values[0]);
		} else {
			let times = match tag.attribute("keyTimes") {
				Some(key_times) => {
					let times = split_list(key_times)
						.map(|time| parse_number(time).filter(|time| (0.0..=1.0).contains(time)))
						.collect::<Option<Vec<f64>>>()?;
					let is_ascending = times.windows(2).all(|pair| pair[0] <= pair[1]);
					let ends_at_one =
						mode == CalculationMode::Discrete || times.last() == Some(&1.0);
					if times.len() != count || times[0] != 0.0 || !is_ascending || !ends_at_one {
						return None;
					}
					times
				}
				None => (0..count)
					.map(|index| match (mode, count) {
						(CalculationMode::Discrete, _) => index as f64 / count as f64,
						(_, 1) => 0.0,
						_ => index as f64 / (count - 1) as f64,
					})
					.collect(),
			};
			let splines = match mode {
				CalculationMode::Spline => {
					let splines = split_list(tag.attribute("keySplines")?)
						.map(parse_spline)
						.collect::<Option<Vec<String>>>()?;
					(splines.len() + 1 == count).then_some(splines)?
				}
				_ => Vec::new(),
			};
			for (index, (time, declaration)) in times.iter().zip(values).enumerate() {
				let _ = write!(keyframes, "{}{{{declaration}", percent(*time));
				if let Some(spline) = splines.get(index) {
					let _ = write!(keyframes, ";animation-timing-function:{spline}");
				}
				keyframes.push('}');
			}
			if times.last() != Some(&1.0) || count == 1 {
				let _ = write!(keyframes, "100%{{{}}}", values[count - 1]);
			}
		}
		let iterations = iterations(tag, duration)?;
		let animation = Animation {
			keyframes,
			duration,
			delay,
			iterations: Some(iterations).filter(|count| count.is_finite()),
			timing_function: match mode {
				CalculationMode::Discrete => "step-end",
				_ => "linear",
			},
			freezes: fill_freezes,
		};
		Some((target, animation))
	}

	/// The value of the CSS `animation` shorthand.
	fn shorthand(&self, name: &str) -> String {
		let iterations = match self.iterations {
			Some(count) => format_number(count),
			None => "infinite".to_owned(),
		};
		let fill_mode = if self.freezes { "forwards" } else { "none" };
		format!(
			"{name} {}s {} {}s {iterations} {fill_mode}",
			format_number(self.duration),
			self.timing_function,
			format_number(self.delay)
		)
	}
}

/// The CSS declarations of the values of an animation, in order.
struct Declarations {
	values: Vec<String>,
	/// Only `to` is given, so the animation starts at the underlying value.
	is_to_animation: bool,
}

/// The values of `values`, `from`/`to` or `to`, each converted by `convert`.
fn animation_values(
	tag: &StartTag,
	convert: impl Fn(&str) -> Option<String>,
) -> Option<Declarations> {
	if tag.attribute("by").is_some() {
		return None;
	}
	let (values, is_to_animation) =
		match (tag.attribute("values"), tag.attribute("from"), tag.attribute("to")) {
			(Some(values), _, _) => (split_list(values).collect::<Vec<_>>(), false),
			(None, Some(from), Some(to)) => (vec![from, to], false),
			(None, None, Some(to)) => (vec![to], tag.name != "set"),
			_ => return None,
		};
	let values = values.into_iter().map(convert).collect::<Option<Vec<String>>>()?;
	(!values.is_empty()).then_some(Declarations { values, is_to_animation })
}

fn property_values(tag: &StartTag) -> Option<Declarations> {
	let property = tag.attribute("attributeName")?;
	let &(property, is_length) = CSS_PROPERTIES.iter().find(|(name, _)| *name == property)?;
	animation_values(tag, |value| {
		let is_safe =
			value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b" .,-+#%()".contains(&byte));
		if value.is_empty() || !is_safe {
			return None;
		}
		let unit = if is_length && parse_number(value).is_some() { "px" } else { "" };
		Some(format!("{property}:{value}{unit}"))
	})
}

/// The transforms of `animateTransform`. A rotation around a center becomes the same three
/// functions for every value, so that CSS interpolates them pairwise.
fn transform_values(tag: &StartTag) -> Option<Declarations> {
	let kind = tag.attribute("type").unwrap_or("translate");
	animation_values(tag, |value| {
		let numbers = value
			.split(|c: char| c.is_ascii_whitespace() || c == ',')
			.filter(|part| !part.is_empty())
			.map(parse_number)
			.collect::<Option<Vec<f64>>>()?;
		let [first, rest @ ..] = numbers.as_slice() else { return None };
		let first = format_number(*first);
		let transform = match (kind, rest) {
			("translate", []) => format!("translate({first}px,0px)"),
			("translate", [y]) => format!("translate({first}px,{}px)", format_number(*y)),
			("scale", []) => format!("scale({first},{first})"),
			("scale", [y]) => format!("scale({first},{})", format_number(*y)),
			("rotate", []) => format!("translate(0px,0px) rotate({first}deg) translate(0px,0px)"),
			("rotate", [x, y]) => format!(
				"translate({}px,{}px) rotate({first}deg) translate({}px,{}px)",
				format_number(*x),
				format_number(*y),
				format_number(-x),
				format_number(-y)
			),
			("skewX", []) => format!("skewX({first}deg)"),
			("skewY", []) => format!("skewY({first}deg)"),
			_ => return None,
		};
		Some(format!("transform:{transform};transform-origin:0px 0px"))
	})
}

/// Number of plays: `repeatCount`, limited by `repeatDur`, infinite for `indefinite`.
fn iterations(tag: &StartTag, duration: f64) -> Option<f64> {
	let count = match tag.attribute("repeatCount") {
		None => None,
		Some("indefinite") => Some(f64::INFINITY),
		Some(count) => Some(parse_number(count).filter(|&count| count > 0.0)?),
	};
	let count_of_duration = match tag.attribute("repeatDur") {
		None => None,
		Some("indefinite") => Some(f64::INFINITY),
		Some(total) => Some(parse_clock(total).filter(|&total| total > 0.0)? / duration),
	};
	Some(match (count, count_of_duration) {
		(Some(count), Some(of_duration)) => count.min(of_duration),
		(Some(count), None) | (None, Some(count)) => count,
		(None, None) => 1.0,
	})
}

/// The items of a `;` separated list. A trailing `;` adds no item.
fn split_list(list: &str) -> impl Iterator<Item = &str> {
	let list = list.trim();
	list.strip_suffix(';').unwrap_or(list).split(';').map(str::trim)
}

/// A `keySplines` item as CSS timing function.
fn parse_spline(spline: &str) -> Option<String> {
	let numbers = spline
		.split(|c: char| c.is_ascii_whitespace() || c == ',')
		.filter(|part| !part.is_empty())
		.map(|number| parse_number(number).filter(|number| (0.0..=1.0).contains(number)))
		.collect::<Option<Vec<f64>>>()?;
	let [x1, y1, x2, y2] = numbers.as_slice() else { return None };
	Some(format!(
		"cubic-bezier({},{},{},{})",
		format_number(*x1),
		format_number(*y1),
		format_number(*x2),
		format_number(*y2)
	))
}

/// A begin offset: a clock value with an optional sign.
fn parse_offset(offset: &str) -> Option<f64> {
	match offset.strip_prefix('-') {
		Some(negative) => parse_clock(negative.trim_start()).map(|seconds| -seconds),
		None => parse_clock(offset.strip_prefix('+').unwrap_or(offset).trim_start()),
	}
}

/// A SMIL clock value in seconds: `02:30.5`, `1:02:30`, `1.5s`, `200ms`, `2min`, `1h` or `1.5`.
fn parse_clock(clock: &str) -> Option<f64> {
	let clock = clock.trim();
	if clock.contains(':') {
		let parts = clock.split(':').collect::<Vec<_>>();
		let is_whole =
			|part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
		let (hours, minutes, seconds) = match parts.as_slice() {
			[hours, minutes, seconds] if is_whole(hours) => {
				(parse_number(hours)?, *minutes, *seconds)
			}
			[minutes, seconds] => (0.0, *minutes, *seconds),
			_ => return None,
		};
		if !is_whole(minutes) {
			return None;
		}
		return Some(hours * 3600.0 + parse_number(minutes)? * 60.0 + parse_number(seconds)?);
	}
	let (number, factor) = if let Some(number) = clock.strip_suffix("ms") {
		(number, 0.001)
	} else if let Some(number) = clock.strip_suffix("min") {
		(number, 60.0)
	} else if let Some(number) = clock.strip_suffix('h') {
		(number, 3600.0)
	} else if let Some(number) = clock.strip_suffix('s') {
		(number, 1.0)
	} else {
		(clock, 1.0)
	};
	Some(parse_number(number)? * factor).filter(|seconds| seconds.is_finite())
}

/// An unsigned decimal number, or a negative one with `-`. No exponents, infinities or NaN.
fn parse_number(text: &str) -> Option<f64> {
	let digits = text.strip_prefix('-').unwrap_or(text);
	let has_digit = digits.bytes().any(|byte| byte.is_ascii_digit());
	let is_decimal = digits.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.');
	if !has_digit || !is_decimal || digits.bytes().filter(|&byte| byte == b'.').count() > 1 {
		return None;
	}
	text.parse::<f64>().ok().filter(|number| number.is_finite())
}

/// Ids that need no escaping in a CSS attribute selector.
fn is_safe_id(id: &str) -> bool {
	!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
}

/// `number` rounded to thousandths, without exponent or trailing zeros.
fn format_number(number: f64) -> String {
	let rounded = (number * 1000.0).round() / 1000.0;
	if rounded == 0.0 { "0".to_owned() } else { rounded.to_string() }
}

fn percent(time: f64) -> String {
	format!("{}%", format_number(time * 100.0))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The style sheet the conversion inserted.
	fn style_of(svg: &str) -> String {
		let converted = smil_as_css(svg).expect("converted");
		let start = converted.find("<style><![CDATA[\n").expect("style") + 17;
		let end = converted.find("]]></style>").expect("style end");
		converted[start..end].to_owned()
	}

	#[test]
	fn converts_a_blinking_circle() {
		let svg = "<?xml version=\"1.0\"?>\n<svg width=\"1em\" xmlns=\"http://www.w3.org/2000/svg\">\n\
			<circle r=\"0.5em\" fill=\"green\">\n<animate attributeName=\"opacity\" dur=\"1s\" \
			begin=\"0s\" values=\"0;1\" calcMode=\"discrete\" repeatCount=\"indefinite\"/>\n</circle>\n</svg>\n";
		let converted = smil_as_css(svg).expect("converted");
		assert!(converted.starts_with(
			"<?xml version=\"1.0\"?>\n<svg width=\"1em\" xmlns=\"http://www.w3.org/2000/svg\"><style>"
		));
		assert!(
			converted.contains("<circle data-npp-smil=\"0\" r=\"0.5em\" fill=\"green\">\n<animate")
		);
		assert_eq!(
			style_of(svg),
			"@keyframes npp-smil-0{0%{opacity:0}50%{opacity:1}100%{opacity:1}}\n\
			[data-npp-smil=\"0\"]{animation:npp-smil-0 1s step-end 0s infinite none}\n"
		);
	}

	#[test]
	fn converts_interpolations_and_timing() {
		let svg = "<svg><rect id=\"r\" width=\"9\"><animate attributeName=\"fill\" from=\"red\" to=\"#00f\" \
			dur=\"500ms\" begin=\"-0.25s\" repeatCount=\"2.5\" fill=\"freeze\"/>\
			<animate attributeName=\"stroke-width\" values=\"1;4;2;\" keyTimes=\"0;0.25;1\" dur=\"0:02\"/>\
			</rect><g><set attributeName=\"visibility\" to=\"hidden\" begin=\"1s\"/></g>\
			<animate href=\"#r\" attributeName=\"opacity\" to=\"0\" dur=\"1min\" repeatDur=\"90s\"/></svg>";
		assert_eq!(
			style_of(svg),
			"@keyframes npp-smil-0{0%{fill:red}100%{fill:#00f}}\n\
			@keyframes npp-smil-1{0%{stroke-width:1px}25%{stroke-width:4px}100%{stroke-width:2px}}\n\
			@keyframes npp-smil-2{0%,100%{visibility:hidden}}\n\
			@keyframes npp-smil-3{100%{opacity:0}}\n\
			[id=\"r\"]{animation:npp-smil-0 0.5s linear -0.25s 2.5 forwards,\
			npp-smil-1 2s linear 0s 1 none,npp-smil-3 60s linear 0s 1.5 none}\n\
			[data-npp-smil=\"0\"]{animation:npp-smil-2 1s step-end 1s 1 forwards}\n"
		);
	}

	#[test]
	fn converts_splines_and_transforms() {
		let svg = "<svg><path d=\"M0 0\"><animateTransform attributeName=\"transform\" type=\"rotate\" \
			values=\"0 8 8;360 8 8\" calcMode=\"spline\" keySplines=\"0.4,0 0.2 1\" dur=\"2s\" \
			repeatCount=\"indefinite\"/></path><g><animateTransform attributeName=\"transform\" \
			type=\"scale\" from=\"1\" to=\"2 3\" dur=\"1s\" additive=\"sum\"/></g></svg>";
		assert_eq!(
			style_of(svg),
			"@keyframes npp-smil-0{0%{transform:translate(8px,8px) rotate(0deg) translate(-8px,-8px);\
			transform-origin:0px 0px;animation-timing-function:cubic-bezier(0.4,0,0.2,1)}\
			100%{transform:translate(8px,8px) rotate(360deg) translate(-8px,-8px);transform-origin:0px 0px}}\n\
			@keyframes npp-smil-1{0%{transform:scale(1,1);transform-origin:0px 0px}\
			100%{transform:scale(2,3);transform-origin:0px 0px}}\n\
			[data-npp-smil=\"0\"]{animation:npp-smil-0 2s linear 0s infinite none}\n\
			[data-npp-smil=\"1\"]{animation:npp-smil-1 1s linear 0s 1 none}\n"
		);
	}

	#[test]
	fn leaves_unsupported_animations_out() {
		let unconverted = [
			"<svg><circle><animate attributeName=\"r\" values=\"1;2\" dur=\"1s\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"opacity\" values=\"0;1\" dur=\"1s\" begin=\"click\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"opacity\" values=\"0;1\" dur=\"indefinite\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"opacity\" values=\"0;1\" dur=\"1s\" end=\"2s\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"opacity\" by=\"1\" dur=\"1s\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"opacity\" values=\"0;1\" keyTimes=\"0;0.5\" dur=\"1s\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"fill\" values=\"url(&quot;x)\" dur=\"1s\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"fill\" values=\"a}b\" dur=\"1s\"/></circle></svg>",
			"<svg><g transform=\"scale(2)\"><animateTransform attributeName=\"transform\" type=\"rotate\" \
			from=\"0\" to=\"90\" dur=\"1s\" additive=\"sum\"/></g></svg>",
			"<svg><circle><animate attributeName=\"opacity\" values=\"0;1\" dur=\"1e3s\"/></circle></svg>",
			"<svg><circle><animate attributeName=\"opacity\" values=\"0;1\" dur=\"inf\"/></circle></svg>",
			"<svg><circle><animate href=\"#a]b\" attributeName=\"opacity\" values=\"0;1\" dur=\"1s\"/></circle></svg>",
			"<svg><rect/></svg>",
		];
		for svg in unconverted {
			assert_eq!(smil_as_css(svg), None, "{svg}");
		}
	}

	#[test]
	fn rejects_markup_it_does_not_understand() {
		let animate = "<animate attributeName=\"opacity\" values=\"0;1\" dur=\"1s\"/>";
		assert!(smil_as_css(&format!("<svg><g>{animate}</g></svg>")).is_some());
		for broken in [
			format!("<svg><g>{animate}</svg>"),
			format!("<svg><g>{animate}</g>"),
			format!("<html><g>{animate}</g></html>"),
			format!("<svg><g a=b>{animate}</g></svg>"),
			format!("<!DOCTYPE svg [<!ENTITY a \"b\">]><svg><g>{animate}</g></svg>"),
			format!("<svg><g>{animate}</g></svg><svg/>"),
			format!("<svg><g>{animate}<!-- </g></svg>"),
		] {
			assert_eq!(smil_as_css(&broken), None, "{broken}");
		}
		let commented = format!(
			"<!-- <x> --><svg><![CDATA[<y>]]><g title=\"a>b\" id=\"bad id\">{animate}</g ></svg>"
		);
		assert!(
			smil_as_css(&commented).expect("converted").contains("<g data-npp-smil=\"0\" title")
		);
	}

	#[test]
	fn converts_only_svg_images() {
		let svg = "<svg><g><set attributeName=\"opacity\" to=\"0\"/></g></svg>";
		let image = |media_type| EmbeddedImage { media_type, bytes: svg.as_bytes().to_vec() };
		assert!(css_animated_image(&image("image/svg+xml")).is_some());
		assert_eq!(css_animated_image(&image("image/png")), None);
	}

	#[test]
	fn parses_clock_values() {
		assert_eq!(parse_clock("02:30.5"), Some(150.5));
		assert_eq!(parse_clock("1:02:30"), Some(3750.0));
		assert_eq!(parse_clock(" 200ms "), Some(0.2));
		assert_eq!(parse_clock("2min"), Some(120.0));
		assert_eq!(parse_clock("1h"), Some(3600.0));
		assert_eq!(parse_clock(".5"), Some(0.5));
		assert_eq!(parse_offset("- 1s"), Some(-1.0));
		for invalid in ["", "s", "1.2.3s", "+1s", "1:2:3:4", "a:01", "NaN", "1e2"] {
			assert_eq!(parse_clock(invalid), None, "{invalid}");
		}
	}

	#[test]
	fn survives_random_markup() {
		const PIECES: [&str; 16] = [
			"<svg>",
			"</svg>",
			"<g",
			">",
			"/>",
			"</g>",
			" values=\"0;1\"",
			" dur=\"1s\"",
			"<animate",
			" attributeName=\"opacity\"",
			"<set",
			" to='0'",
			"<!--",
			"-->",
			"ä",
			"\"",
		];
		let mut state = 0x2545_f491_u32;
		for _ in 0..20_000 {
			let mut svg = String::new();
			for _ in 0..(state % 24) {
				state ^= state << 13;
				state ^= state >> 17;
				state ^= state << 5;
				svg.push_str(PIECES[(state % 16) as usize]);
			}
			let _ = smil_as_css(&svg);
		}
	}
}
