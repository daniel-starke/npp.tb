//! Cynefin frameworks (`cynefin.langium`, `cynefinParser.ts`, `cynefinDb.ts`,
//! `cynefinRenderer.ts`, `cynefinBoundaries.ts`, `styles.ts`).
//!
//! With `seed: 0` Mermaid seeds the wavy boundaries from the id of the SVG element, which
//! changes with every render; here the seed comes from the diagram's own id, a hash of its
//! source, so a diagram keeps its boundaries.

use super::common::{CommonFields, js_number_of};
use super::langium::Scanner;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::theme::ThemeVariables;
use super::value::Value;
use super::{Context, Diagram, Result};

const SECTION: &str = "cynefin";
const DOMAINS: [&str; 5] = ["complex", "complicated", "clear", "chaotic", "confusion"];
const QUADRANTS: [Domain; 4] =
	[Domain::Complex, Domain::Complicated, Domain::Chaotic, Domain::Clear];
const MAX_CONFUSION_ITEMS: usize = 3;
const ITEM_HEIGHT: f64 = 26.0;
const ITEM_PADDING_X: f64 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Domain {
	Complex,
	Complicated,
	Clear,
	Chaotic,
	Confusion,
}

impl Domain {
	fn name(self) -> &'static str {
		DOMAINS[self as usize]
	}

	fn label(self) -> &'static str {
		["Complex", "Complicated", "Clear", "Chaotic", "Confusion"][self as usize]
	}

	/// `DOMAIN_META`: the decision model and the practice.
	fn meta(self) -> (&'static str, &'static str) {
		match self {
			Domain::Complex => ("Probe \u{2192} Sense \u{2192} Respond", "Emergent Practices"),
			Domain::Complicated => ("Sense \u{2192} Analyse \u{2192} Respond", "Good Practices"),
			Domain::Clear => ("Sense \u{2192} Categorise \u{2192} Respond", "Best Practices"),
			Domain::Chaotic => ("Act \u{2192} Sense \u{2192} Respond", "Novel Practices"),
			Domain::Confusion => ("", "Disorder"),
		}
	}
}

/// `DOMAIN_NAME`: `complex|complicated|clear|chaotic|confusion`, the first that matches.
fn domain(scanner: &mut Scanner) -> Option<Domain> {
	let all =
		[Domain::Complex, Domain::Complicated, Domain::Clear, Domain::Chaotic, Domain::Confusion];
	all.into_iter().find(|domain| scanner.punctuation(domain.name()))
}

struct Transition {
	from: Domain,
	to: Domain,
	label: Option<String>,
}

struct Cynefin {
	fields: CommonFields,
	/// The items of each domain, by `Domain as usize`; a later block replaces an earlier one.
	items: [Option<Vec<String>>; 5],
	transitions: Vec<Transition>,
}

fn parse(code: &str, context: &Context) -> Result<Cynefin> {
	let mut cynefin = Cynefin {
		fields: CommonFields::new(context),
		items: Default::default(),
		transitions: Vec::new(),
	};
	let mut scanner = Scanner::new(code);
	scanner.skip_newlines();
	if !(scanner.keyword("cynefin-beta:", false) || scanner.keyword("cynefin-beta", true)) {
		return Err(scanner.error(context, "`cynefin-beta`"));
	}
	loop {
		if scanner.is_at_end() {
			break;
		}
		if scanner.newline() {
			continue;
		}
		if scanner.common_field(&mut cynefin.fields) {
			if !scanner.end_of_line() {
				return Err(scanner.error(context, "a new line"));
			}
			continue;
		}
		let Some(from) = domain(&mut scanner) else {
			return Err(scanner.error(context, "a domain, a transition or a title"));
		};
		if scanner.punctuation("-->") {
			let to = domain(&mut scanner).ok_or_else(|| scanner.error(context, "a domain"))?;
			let mut label = None;
			if scanner.punctuation(":") {
				label = Some(scanner.string().ok_or_else(|| scanner.error(context, "a string"))?);
			}
			if !scanner.end_of_line() {
				return Err(scanner.error(context, "a new line"));
			}
			// Self-loops are skipped.
			if from != to {
				cynefin.transitions.push(Transition {
					from,
					to,
					label: label.filter(|label| !label.is_empty()),
				});
			}
			continue;
		}
		let mut items = Vec::new();
		scanner.skip_newlines();
		while let Some(item) = scanner.string() {
			items.push(item);
			scanner.skip_newlines();
		}
		cynefin.items[from as usize] = Some(items);
	}
	Ok(cynefin)
}

/// JavaScript's `ToInt32`.
fn to_int32(value: f64) -> u32 {
	match value.is_finite() {
		true => value.trunc().rem_euclid(4_294_967_296.0) as u32,
		false => 0,
	}
}

/// `seededRandom`: one value of mulberry32 for `seed`.
fn seeded_random(seed: f64) -> f64 {
	let mut t = to_int32(seed + f64::from(0x6d2b_79f5u32));
	t = (t ^ (t >> 15)).wrapping_mul(t | 1);
	t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
	f64::from(t ^ (t >> 14)) / 4_294_967_296.0
}

/// `hashString`: `hash * 31 + code unit` in 32 bits.
fn hash_string(text: &str) -> f64 {
	let hash = text
		.encode_utf16()
		.fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(unit.into()));
	f64::from(hash)
}

/// `generateFoldPath` (`is_vertical`) and `generateHorizontalBoundary`.
fn boundary_path(width: f64, height: f64, seed: f64, amplitude: f64, is_vertical: bool) -> String {
	let segments = 7;
	let (jitter_step, offset_step, offset_base) = match is_vertical {
		true => (17.0, 31.0, 7.0),
		false => (23.0, 37.0, 11.0),
	};
	let points: Vec<(f64, f64)> = (0..=segments)
		.map(|index| {
			let jitter =
				seeded_random(seed + f64::from(index) * jitter_step) * amplitude * 2.0 - amplitude;
			match is_vertical {
				true => (width / 2.0 + jitter, f64::from(index) * (height / f64::from(segments))),
				false => (f64::from(index) * (width / f64::from(segments)), height / 2.0 + jitter),
			}
		})
		.collect();
	let mut path = format!("M{},{}", number(points[0].0), number(points[0].1));
	for index in 0..segments as usize {
		let (p0, p1) = (points[index], points[index + 1]);
		let direction = if index % 2 == 0 { 1.0 } else { -1.0 };
		let offset = amplitude
			* 1.5 * direction
			* seeded_random(seed + index as f64 * offset_step + offset_base);
		let (first, second) = match is_vertical {
			true => {
				let middle = (p0.1 + p1.1) / 2.0;
				((p0.0 + offset, middle), (p1.0 - offset, middle))
			}
			false => {
				let middle = (p0.0 + p1.0) / 2.0;
				((middle, p0.1 + offset), (middle, p1.1 - offset))
			}
		};
		path.push_str(&format!(
			" C{},{} {},{} {},{}",
			number(first.0),
			number(first.1),
			number(second.0),
			number(second.1),
			number(p1.0),
			number(p1.1)
		));
	}
	path
}

/// `generateCliffPath`: the steep S-curve between Clear and Chaotic.
fn cliff_path(width: f64, height: f64) -> String {
	let center = width / 2.0;
	let (top, bottom) = (height * 0.5, height);
	let amplitude = width * 0.03;
	let y = |fraction: f64| number(top + (bottom - top) * fraction);
	format!(
		"M{},{} C{},{} {},{} {},{} C{},{} {},{} {},{}",
		number(center),
		number(top),
		number(center + amplitude),
		y(0.2),
		number(center - amplitude * 1.5),
		y(0.55),
		number(center + amplitude * 0.5),
		y(0.75),
		number(center - amplitude),
		y(0.85),
		number(center + amplitude * 0.3),
		y(0.95),
		number(center),
		number(bottom)
	)
}

/// The layout of a domain: its center and box (`getDomainLayouts`).
fn domain_layout(domain: Domain, width: f64, height: f64) -> [f64; 6] {
	let (half_width, half_height) = (width / 2.0, height / 2.0);
	match domain {
		Domain::Complex => [half_width / 2.0, half_height / 2.0, 0.0, 0.0, half_width, half_height],
		Domain::Complicated => [
			half_width + half_width / 2.0,
			half_height / 2.0,
			half_width,
			0.0,
			half_width,
			half_height,
		],
		Domain::Chaotic => [
			half_width / 2.0,
			half_height + half_height / 2.0,
			0.0,
			half_height,
			half_width,
			half_height,
		],
		Domain::Clear => [
			half_width + half_width / 2.0,
			half_height + half_height / 2.0,
			half_width,
			half_height,
			half_width,
			half_height,
		],
		Domain::Confusion => [
			half_width,
			half_height,
			half_width * 0.7,
			half_height * 0.7,
			half_width * 0.6,
			half_height * 0.6,
		],
	}
}

/// The `cynefin` theme block: the theme's values over those of the default theme, which also
/// serve the themes without the block.
struct ThemeBlock {
	current: Option<Value>,
	default: Option<Value>,
}

impl ThemeBlock {
	fn new(context: &Context) -> Self {
		Self {
			current: context.theme.get(SECTION).cloned(),
			default: ThemeVariables::new("default", None).get(SECTION).cloned(),
		}
	}

	fn value(&self, key: &str) -> Option<&Value> {
		[&self.current, &self.default]
			.into_iter()
			.flatten()
			.find_map(|block| block.get(key))
			.filter(|value| !matches!(value, Value::Null))
	}

	fn text(&self, key: &str) -> String {
		self.value(key).and_then(Value::to_text).unwrap_or_default()
	}

	/// A font size written as `${value}px`, `None` where that is no valid CSS length.
	fn font_size(&self, key: &str, added: f64) -> Option<f64> {
		let size = match self.value(key)? {
			Value::Number(size) => size + added,
			// `-` converts to a number, `+` appends to the string.
			Value::String(text) if added < 0.0 => js_number_of(text) + added,
			Value::String(text) if added > 0.0 => {
				format!("{}{}", text, number(added)).trim().parse().ok()?
			}
			Value::String(text) => text.trim().parse().ok()?,
			_ => return None,
		};
		(size.is_finite() && size >= 0.0).then_some(size)
	}
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let cynefin = parse(code, context)?;
	let config = |key: &str, default: f64| context.config_number(SECTION, key, default);
	let (width, height, padding) =
		(config("width", 800.0), config("height", 600.0), config("padding", 40.0));
	let amplitude = config("boundaryAmplitude", 8.0);
	let shows_descriptions = context.config_bool(SECTION, "showDomainDescriptions", true);
	let seed = match context.config_value(SECTION, "seed").and_then(Value::as_number) {
		Some(seed) if seed.is_finite() && seed != 0.0 => seed,
		_ => hash_string(&context.id),
	};
	let theme = ThemeBlock::new(context);
	let background = |domain: Domain| theme.text(&format!("{}Bg", domain.name()));
	let root_font = Font::new(context.font_family(), context.font_size());
	let font_of =
		|size: Option<f64>| Font::new(root_font.family.clone(), size.unwrap_or(root_font.size));
	let mut label_font = font_of(theme.font_size("domainFontSize", 0.0));
	label_font.weight = 700;
	let mut subtitle_font = font_of(theme.font_size("itemFontSize", -1.0));
	subtitle_font.is_italic = true;
	let item_font = font_of(theme.font_size("itemFontSize", 0.0));
	let arrow_label_font = font_of(theme.font_size("itemFontSize", -1.0));
	let mut title_font = font_of(theme.font_size("domainFontSize", 2.0));
	title_font.weight = 700;
	let (text_color, label_color) = (theme.text("textColor"), theme.text("labelColor"));
	let boundary_color = theme.text("boundaryColor");
	let arrow_color = theme.text("arrowColor");

	let mut body = Svg::new();
	let translate = format!("translate({}, {})", number(padding), number(padding));
	body.open("g").attr("transform", &translate).end();
	body.open("g").attr("class", "cynefin-backgrounds").end();
	for domain in QUADRANTS {
		let [_, _, x, y, w, h] = domain_layout(domain, width, height);
		body.open("rect")
			.attr("class", "cynefinDomain")
			.number_attr("x", x)
			.number_attr("y", y)
			.number_attr("width", w)
			.number_attr("height", h)
			.attr_if("fill", &background(domain))
			.attr("fill-opacity", "0.4")
			.attr("stroke", "none")
			.close();
	}
	body.close_tag("g");
	body.open("g").attr("class", "cynefin-boundaries").end();
	let boundary_width = theme.text("boundaryWidth");
	for (seed, is_vertical) in [(seed, true), (seed + 100.0, false)] {
		body.open("path")
			.attr("class", "cynefinBoundary")
			.attr("d", &boundary_path(width, height, seed, amplitude, is_vertical))
			.attr("fill", "none")
			.attr_if("stroke", &boundary_color)
			.attr_if("stroke-width", &boundary_width)
			.attr("stroke-dasharray", "6 3")
			.close();
	}
	body.open("path")
		.attr("class", "cynefinCliff")
		.attr("d", &cliff_path(width, height))
		.attr("fill", "none")
		.attr_if("stroke", &theme.text("cliffColor"))
		.attr_if("stroke-width", &theme.text("cliffWidth"))
		.close();
	body.close_tag("g");
	let (center_x, center_y) = (width / 2.0, height / 2.0);
	let (radius_x, radius_y) = (width * 0.15, height * 0.15);
	let ellipse = format!(
		"M{},{} A{},{} 0 1,1 {},{} A{},{} 0 1,1 {},{} Z",
		number(center_x - radius_x),
		number(center_y),
		number(radius_x),
		number(radius_y),
		number(center_x + radius_x),
		number(center_y),
		number(radius_x),
		number(radius_y),
		number(center_x - radius_x),
		number(center_y)
	);
	body.open("path")
		.attr("class", "cynefinConfusion")
		.attr("d", &ellipse)
		.attr_if("fill", &background(Domain::Confusion))
		.attr("fill-opacity", "0.5")
		.attr_if("stroke", &boundary_color)
		.attr("stroke-width", "1.5")
		.attr("stroke-dasharray", "4 2")
		.close();

	let centered_text =
		|body: &mut Svg, x: f64, y: f64, font: &Font, text: &str, class: &str, fill: &str| {
			let run = TextRun {
				x,
				y,
				anchor: Anchor::Middle,
				baseline: Baseline::Middle,
				font,
				content: text,
			};
			body.text(run, |svg| {
				svg.attr("class", class).attr_if("fill", fill);
			});
		};
	body.open("g").attr("class", "cynefin-labels").end();
	for domain in QUADRANTS {
		let [cx, cy, ..] = domain_layout(domain, width, height);
		let y = if shows_descriptions { cy - 30.0 } else { cy };
		centered_text(
			&mut body,
			cx,
			y,
			&label_font,
			domain.label(),
			"cynefinDomainLabel",
			&label_color,
		);
	}
	let y = if shows_descriptions { center_y - 10.0 } else { center_y };
	centered_text(
		&mut body,
		center_x,
		y,
		&label_font,
		"Confusion",
		"cynefinDomainLabel",
		&label_color,
	);
	body.close_tag("g");
	if shows_descriptions {
		body.open("g").attr("class", "cynefin-subtitles").end();
		for domain in QUADRANTS {
			let [cx, cy, ..] = domain_layout(domain, width, height);
			let (model, practice) = domain.meta();
			centered_text(
				&mut body,
				cx,
				cy - 10.0,
				&subtitle_font,
				model,
				"cynefinSubtitle",
				&text_color,
			);
			centered_text(
				&mut body,
				cx,
				cy + 5.0,
				&subtitle_font,
				practice,
				"cynefinSubtitle",
				&text_color,
			);
		}
		let practice = Domain::Confusion.meta().1;
		centered_text(
			&mut body,
			center_x,
			center_y + 8.0,
			&subtitle_font,
			practice,
			"cynefinSubtitle",
			&text_color,
		);
		body.close_tag("g");
	}

	body.open("g").attr("class", "cynefin-items").end();
	let order =
		[Domain::Complex, Domain::Complicated, Domain::Chaotic, Domain::Clear, Domain::Confusion];
	for domain in order {
		let Some(items) = cynefin.items[domain as usize].as_ref().filter(|items| !items.is_empty())
		else {
			continue;
		};
		let [cx, cy, ..] = domain_layout(domain, width, height);
		let is_confusion = domain == Domain::Confusion;
		let shown = if is_confusion { items.len().min(MAX_CONFUSION_ITEMS) } else { items.len() };
		let start_y = cy
			+ match (is_confusion, shows_descriptions) {
				(true, true) => 22.0,
				(true, false) => 14.0,
				(false, true) => 25.0,
				(false, false) => 15.0,
			};
		let overflow = items.len() - shown;
		let overflow_label = format!("+{overflow} more");
		let badges = items[..shown]
			.iter()
			.map(|item| (item.as_str(), "cynefinItem"))
			.chain((overflow > 0).then_some((overflow_label.as_str(), "cynefinItemOverflow")));
		for (index, (label, class)) in badges.enumerate() {
			let y = start_y + index as f64 * (ITEM_HEIGHT + 4.0);
			let measured = context.bounding_box(label, &item_font).0;
			let measured =
				if measured > 0.0 { measured } else { label.encode_utf16().count() as f64 * 7.0 };
			let badge_width = measured + ITEM_PADDING_X * 2.0;
			let transform = format!("translate({}, {})", number(cx - badge_width / 2.0), number(y));
			body.open("g").attr("transform", &transform).end();
			let is_overflow = class == "cynefinItemOverflow";
			body.open("rect")
				.attr("class", class)
				.attr("x", "0")
				.attr("y", "0")
				.number_attr("width", badge_width)
				.number_attr("height", ITEM_HEIGHT)
				.attr("rx", "4")
				.attr("ry", "4")
				.attr_if("fill", &background(domain))
				.attr("fill-opacity", if is_overflow { "0.6" } else { "0.95" })
				.attr_if("stroke", &boundary_color)
				.attr("stroke-width", "1")
				.attr_if("stroke-dasharray", if is_overflow { "3 2" } else { "" })
				.close();
			let run = TextRun {
				x: badge_width / 2.0,
				y: ITEM_HEIGHT / 2.0,
				anchor: Anchor::Middle,
				baseline: Baseline::Central,
				font: &item_font,
				content: label,
			};
			body.text(run, |svg| {
				svg.attr("class", "cynefinItemText").attr_if("fill", &text_color);
			});
			body.close_tag("g");
		}
	}
	body.close_tag("g");

	let marker_id = format!("{}-cynefin-arrow", context.id);
	if !cynefin.transitions.is_empty() {
		body.open("g").attr("class", "cynefin-arrows").end();
		let arrow_width = theme.text("arrowWidth");
		for transition in &cynefin.transitions {
			let [x1, y1, ..] = domain_layout(transition.from, width, height);
			let [x2, y2, ..] = domain_layout(transition.to, width, height);
			let (dx, dy) = (x2 - x1, y2 - y1);
			let length = (dx * dx + dy * dy).sqrt();
			let offset = length * 0.15;
			let control_x = (x1 + x2) / 2.0 + -dy / length * offset;
			let control_y = (y1 + y2) / 2.0 + dx / length * offset;
			let path = format!(
				"M{},{} Q{},{} {},{}",
				number(x1),
				number(y1),
				number(control_x),
				number(control_y),
				number(x2),
				number(y2)
			);
			body.open("path")
				.attr("class", "cynefinArrowLine")
				.attr("d", &path)
				.attr("fill", "none")
				.attr("marker-end", &format!("url(#{marker_id})"))
				.attr_if("stroke", &arrow_color)
				.attr_if("stroke-width", &arrow_width)
				.close();
			if let Some(label) = &transition.label {
				let run = TextRun {
					x: control_x,
					y: control_y - 6.0,
					anchor: Anchor::Middle,
					baseline: Baseline::Alphabetic,
					font: &arrow_label_font,
					content: label,
				};
				body.text(run, |svg| {
					svg.attr("class", "cynefinArrowLabel").attr_if("fill", &text_color);
				});
			}
		}
		body.close_tag("g");
	}
	if let Some(title) = cynefin.fields.title.as_deref().filter(|title| !title.is_empty()) {
		centered_text(
			&mut body,
			width / 2.0,
			-padding / 2.0,
			&title_font,
			title,
			"cynefinTitle",
			&label_color,
		);
	}
	body.close_tag("g");
	if !cynefin.transitions.is_empty() {
		// `orient="auto"`: EdgeHTML ignores `auto-start-reverse`, and only ends have markers.
		body.open("defs")
			.end()
			.open("marker")
			.attr("id", &marker_id)
			.attr("viewBox", "0 0 10 10")
			.attr("refX", "9")
			.attr("refY", "5")
			.attr("markerWidth", "6")
			.attr("markerHeight", "6")
			.attr("orient", "auto")
			.end()
			.open("path")
			.attr("d", "M 0 0 L 10 5 L 0 10 z")
			.attr("class", "cynefinArrowHead")
			.attr_if("fill", &arrow_color)
			.attr("stroke", "none")
			.close()
			.close_tag("marker")
			.close_tag("defs");
	}
	let frame = Frame {
		x: 0.0,
		y: 0.0,
		width: width + padding * 2.0,
		height: height + padding * 2.0,
		use_max_width: context.config_bool(SECTION, "useMaxWidth", true),
	};
	Ok(document(context, "cynefin", &frame, &cynefin.fields.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn parsed(code: &str) -> Result<Cynefin> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		parse(code, &context)
	}

	#[test]
	fn parses_domains_and_transitions() {
		let cynefin = parsed(
			"cynefin-beta:\n  title T\n  complex\n    \"a\"\n    \"b\"\n  clear \"c\"\n  complex\n    \"d\"\n  chaotic --> complex : \"probe\"\n  clear --> clear\n  clear --> chaotic : \"\"\n",
		)
		.unwrap();
		assert_eq!(cynefin.fields.title.as_deref(), Some("T"));
		assert_eq!(cynefin.items[Domain::Complex as usize], Some(vec!["d".to_owned()]));
		assert_eq!(cynefin.items[Domain::Clear as usize], Some(vec!["c".to_owned()]));
		assert_eq!(cynefin.transitions.len(), 2);
		assert_eq!(cynefin.transitions[0].label.as_deref(), Some("probe"));
		assert_eq!(cynefin.transitions[1].label, None);
		let line = |code: &str| parsed(code).err().and_then(|error| error.line);
		assert_eq!(line("cynefin-beta\n  complex\n  word\n"), Some(3));
		assert_eq!(line("cynefin-beta\n  \"a\"\n"), Some(2));
		assert_eq!(line("cynefin-beta\n  clear --> chaotic \"x\"\n"), Some(2));
		assert_eq!(line("cynefin-beta x\n"), Some(1));
	}

	#[test]
	fn seeds_like_mermaid() {
		// Values of `seededRandom` and `hashString` in Node.
		assert_eq!(seeded_random(0.0), 0.26642920868471265);
		assert_eq!(seeded_random(42.0), 0.6011037519201636);
		assert_eq!(seeded_random(-7.5), 0.7800034740939736);
		assert_eq!(seeded_random(1e300), 0.0);
		assert_eq!(seeded_random(4_294_967_296.0 * 3.0 + 5.0), 0.6897749109193683);
		assert_eq!(hash_string("mermaid-1"), 1_795_732_973.0);
		assert_eq!(hash_string("\u{e4}\u{20ac}\u{1f600}x"), 514_694_701.0);
	}
}
