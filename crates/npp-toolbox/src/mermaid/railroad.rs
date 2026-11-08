//! Railroad diagrams in four syntaxes (`railroadDb.ts`, `railroadRenderer.ts`, `styles.ts`; the
//! grammars in [`parser`]).
//!
//! Colors and the font come from the theme unless the `railroad` section sets them, falling
//! back to the default theme's variables and then to `DEFAULT_RAILROAD_CONFIG`
//! (`buildRailroadStyleOptions`). Text is measured with `getBBox` in the regular weight, also
//! for the bold rule names.

mod parser;

use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document, number};
use super::text::Font;
use super::theme::ThemeVariables;
use super::value::{Value, parse_leading_number};
use super::{Context, Diagram, DiagramKind, Result};
use parser::Syntax;

const SECTION: &str = "railroad";

#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
	Terminal(String),
	NonTerminal(String),
	Special(String),
	Sequence(Vec<Expression>),
	Choice(Vec<Expression>),
	Optional(Box<Expression>),
	/// One or more, with a bypass above it for zero or more.
	Repetition {
		element: Box<Expression>,
		has_bypass: bool,
	},
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
	pub name: String,
	pub definition: Expression,
}

/// The resolved `RailroadStyleOptions` that the drawing uses.
struct Style {
	padding: f64,
	vertical_separation: f64,
	horizontal_separation: f64,
	arc_radius: f64,
	font: Font,
	stroke_width: f64,
	marker_radius: f64,
	terminal_fill: String,
	terminal_stroke: String,
	terminal_text: String,
	non_terminal_fill: String,
	non_terminal_stroke: String,
	non_terminal_text: String,
	line_color: String,
	marker_fill: String,
	special_fill: String,
	special_stroke: String,
	rule_name_color: String,
}

impl Style {
	fn new(context: &Context) -> Self {
		let default_theme = ThemeVariables::new("default", None);
		// `{...getThemeVariables(), ...config.themeVariables}`: the theme's value, else the
		// default theme's; `null` counts as missing for `??`.
		let theme = |key: &str| -> Option<Value> {
			match context.theme.get(key) {
				Some(Value::Null) => None,
				Some(value) => Some(value.clone()),
				None => {
					default_theme.get(key).filter(|value| !matches!(value, Value::Null)).cloned()
				}
			}
		};
		let themed = |keys: &[&str], fallback: &str| -> String {
			let value = keys.iter().find_map(|key| theme(key));
			sanitize_color(value.as_ref(), fallback)
		};
		let option = |key: &str| context.config_value(SECTION, key);
		let color = |key: &str, theme_default: String| sanitize_color(option(key), &theme_default);
		let number = |key: &str, default: f64| sanitize_number(option(key), default);

		let theme_family = sanitize_font_family(theme("fontFamily").as_ref(), "monospace");
		let theme_size = theme("fontSize")
			.and_then(|size| match size {
				Value::Number(size) => Some(size),
				Value::String(text) => parse_leading_number(&text),
				_ => None,
			})
			.filter(|size| size.is_finite() && *size > 0.0)
			.unwrap_or(14.0);
		let font = Font::new(
			sanitize_font_family(option("fontFamily"), &theme_family),
			number("fontSize", theme_size),
		);
		let line_color = themed(&["lineColor"], "#000000");
		Self {
			padding: number("padding", 10.0),
			// Mermaid's `sanitizeDirective` drops the keys that its default configuration has
			// nowhere (`configKeys`), so documents cannot set these.
			vertical_separation: 8.0,
			horizontal_separation: 10.0,
			arc_radius: 10.0,
			font,
			stroke_width: number("strokeWidth", 2.0),
			marker_radius: 5.0,
			terminal_fill: color(
				"terminalFill",
				themed(&["secondBkg", "secondaryColor"], "#FFFFC0"),
			),
			terminal_stroke: color(
				"terminalStroke",
				themed(&["secondaryBorderColor", "lineColor"], "#000000"),
			),
			terminal_text: color(
				"terminalTextColor",
				themed(&["secondaryTextColor", "textColor"], "#000000"),
			),
			non_terminal_fill: color(
				"nonTerminalFill",
				themed(&["mainBkg", "background"], "#FFFFFF"),
			),
			non_terminal_stroke: color(
				"nonTerminalStroke",
				themed(&["primaryBorderColor", "lineColor"], "#000000"),
			),
			non_terminal_text: color(
				"nonTerminalTextColor",
				themed(&["primaryTextColor", "textColor"], "#000000"),
			),
			line_color: color("lineColor", line_color.clone()),
			marker_fill: color("markerFill", themed(&["lineColor"], "#000000")),
			special_fill: color(
				"specialFill",
				themed(&["tertiaryColor", "secondaryColor"], "#F0E0FF"),
			),
			special_stroke: color(
				"specialStroke",
				themed(&["tertiaryBorderColor", "secondaryBorderColor"], "#8800CC"),
			),
			rule_name_color: color(
				"ruleNameColor",
				themed(&["titleColor", "textColor"], "#000066"),
			),
		}
	}
}

/// `sanitizeColorValue`: hex colors, CSS color functions of plain numbers, and color names.
fn sanitize_color(value: Option<&Value>, fallback: &str) -> String {
	let Some(Value::String(text)) = value else { return fallback.to_owned() };
	let text = text.trim();
	let is_hex = text.strip_prefix('#').is_some_and(|digits| {
		matches!(digits.len(), 3 | 4 | 6 | 8) && digits.chars().all(|c| c.is_ascii_hexdigit())
	});
	let is_function = text.split_once('(').is_some_and(|(name, arguments)| {
		const FUNCTIONS: [&str; 9] =
			["rgb", "rgba", "hsl", "hsla", "hwb", "lab", "lch", "oklab", "oklch"];
		FUNCTIONS.iter().any(|function| function.eq_ignore_ascii_case(name))
			&& arguments.strip_suffix(')').is_some_and(|arguments| {
				!arguments.is_empty()
					&& arguments
						.chars()
						.all(|c| c.is_ascii_digit() || c.is_whitespace() || "%+,./-".contains(c))
			})
	});
	let is_name = !text.is_empty() && text.chars().all(|c| c.is_ascii_alphabetic());
	match is_hex || is_function || is_name {
		true => text.to_owned(),
		false => fallback.to_owned(),
	}
}

/// `sanitizeFontFamilyValue`: `^[\w "',.-]+$`.
fn sanitize_font_family(value: Option<&Value>, fallback: &str) -> String {
	let Some(Value::String(text)) = value else { return fallback.to_owned() };
	let text = text.trim();
	let is_valid = !text.is_empty()
		&& text.chars().all(|c| c.is_ascii_alphanumeric() || "_ \"',.-".contains(c));
	match is_valid {
		true => text.to_owned(),
		false => fallback.to_owned(),
	}
}

/// `sanitizeNumberValue`: numbers and `parseFloat` of strings, finite and not negative.
fn sanitize_number(value: Option<&Value>, fallback: f64) -> f64 {
	let parsed = match value {
		Some(Value::Number(number)) => Some(*number),
		Some(Value::String(text)) => parse_leading_number(text),
		_ => None,
	};
	parsed.filter(|number| number.is_finite() && *number >= 0.0).unwrap_or(fallback)
}

/// A drawn expression: the content of its group and its size around the line through it.
struct Drawn {
	class: &'static str,
	body: Svg,
	width: f64,
	height: f64,
	up: f64,
}

impl Drawn {
	fn down(&self) -> f64 {
		self.height - self.up
	}
}

/// The `M`/`L`/`A` path data that `PathBuilder` writes.
#[derive(Default)]
struct Path(String);

impl Path {
	fn command(mut self, command: &str, values: &[f64]) -> Self {
		if !self.0.is_empty() {
			self.0.push(' ');
		}
		self.0.push_str(command);
		for value in values {
			self.0.push(' ');
			self.0.push_str(&number(*value));
		}
		self
	}

	fn move_to(self, x: f64, y: f64) -> Self {
		self.command("M", &[x, y])
	}

	fn line_to(self, x: f64, y: f64) -> Self {
		self.command("L", &[x, y])
	}

	fn arc_to(self, radius: f64, is_sweep: bool, x: f64, y: f64) -> Self {
		let sweep = if is_sweep { 1.0 } else { 0.0 };
		self.command("A", &[radius, radius, 0.0, 0.0, sweep, x, y])
	}
}

struct Writer<'a, 'c> {
	context: &'a mut Context<'c>,
	style: Style,
}

impl Writer<'_, '_> {
	/// `getBBox` of a `<text>`, which is empty without glyphs or at a font size of 0.
	fn measure(&mut self, text: &str) -> (f64, f64) {
		if self.style.font.size <= 0.0 || text.chars().all(char::is_whitespace) {
			return (0.0, 0.0);
		}
		self.context.bounding_box(text, &self.style.font)
	}

	fn line(&self, svg: &mut Svg, path: Path) {
		svg.open("path")
			.attr("class", "railroad-line")
			.attr("d", &path.0)
			.attr("fill", "none")
			.attr("stroke", &self.style.line_color)
			.number_attr("stroke-width", self.style.stroke_width)
			.close();
	}

	/// Places `drawn` in `parent` at `x`, `y`.
	fn place(parent: &mut Svg, drawn: Drawn, x: f64, y: f64) {
		parent
			.open("g")
			.attr("class", drawn.class)
			.attr("transform", &format!("translate({}, {})", number(x), number(y)))
			.end();
		parent.append(drawn.body);
		parent.close_tag("g");
	}

	/// A terminal, non-terminal or special sequence: a box around the text.
	fn boxed(&mut self, class: &'static str, text: &str) -> Drawn {
		let (text_width, text_height) = self.measure(text);
		let padding = self.style.padding;
		let width = text_width + padding * 2.0;
		let height = text_height + padding * 2.0;
		let style = &self.style;
		let (fill, stroke, text_color) = match class {
			"railroad-terminal" => {
				(&style.terminal_fill, &style.terminal_stroke, &style.terminal_text)
			}
			"railroad-nonterminal" => {
				(&style.non_terminal_fill, &style.non_terminal_stroke, &style.non_terminal_text)
			}
			_ => (&style.special_fill, &style.special_stroke, &style.non_terminal_text),
		};
		let mut body = Svg::new();
		body.open("rect")
			.attr("x", "0")
			.attr("y", "0")
			.number_attr("width", width)
			.number_attr("height", height);
		if class == "railroad-terminal" {
			body.attr("rx", "10").attr("ry", "10");
		}
		body.attr("fill", fill)
			.attr("stroke", stroke)
			.number_attr("stroke-width", style.stroke_width);
		if class == "railroad-special" {
			body.attr("stroke-dasharray", "5,3");
		}
		body.close();
		let run = TextRun {
			x: width / 2.0,
			y: height / 2.0,
			anchor: Anchor::Middle,
			baseline: Baseline::Middle,
			font: &style.font,
			content: text,
		};
		body.text(run, |svg| _ = svg.attr("fill", text_color));
		Drawn { class, body, width, height, up: height / 2.0 }
	}

	fn draw(&mut self, expression: &Expression) -> Drawn {
		match expression {
			Expression::Terminal(value) => self.boxed("railroad-terminal", value),
			Expression::NonTerminal(name) => self.boxed("railroad-nonterminal", name),
			Expression::Special(text) => self.boxed("railroad-special", &format!("? {text} ?")),
			Expression::Sequence(elements) => self.sequence(elements),
			Expression::Choice(alternatives) => self.choice(alternatives),
			Expression::Optional(element) => self.optional(element),
			Expression::Repetition { element, has_bypass } => self.repetition(element, *has_bypass),
		}
	}

	fn sequence(&mut self, elements: &[Expression]) -> Drawn {
		let drawn: Vec<Drawn> = elements.iter().map(|element| self.draw(element)).collect();
		let separation = self.style.horizontal_separation;
		let mut total_width = 0.0;
		let mut max_up: f64 = 0.0;
		let mut max_down: f64 = 0.0;
		for element in &drawn {
			total_width += element.width;
			max_up = max_up.max(element.up);
			max_down = max_down.max(element.down());
		}
		total_width += (drawn.len() as f64 - 1.0) * separation;
		let mut body = Svg::new();
		let mut x = 0.0;
		let count = drawn.len();
		for (index, element) in drawn.into_iter().enumerate() {
			let width = element.width;
			let y = max_up - element.up;
			Self::place(&mut body, element, x, y);
			if index + 1 < count {
				let start = x + width;
				self.line(
					&mut body,
					Path::default().move_to(start, max_up).line_to(start + separation, max_up),
				);
			}
			x += width + separation;
		}
		Drawn {
			class: "railroad-sequence",
			body,
			width: total_width,
			height: max_up + max_down,
			up: max_up,
		}
	}

	fn choice(&mut self, alternatives: &[Expression]) -> Drawn {
		let drawn: Vec<Drawn> = alternatives.iter().map(|element| self.draw(element)).collect();
		let mut max_width: f64 = 0.0;
		let mut total_height = 0.0;
		for alternative in &drawn {
			max_width = max_width.max(alternative.width);
			total_height += alternative.height;
		}
		total_height += (drawn.len() as f64 - 1.0) * self.style.vertical_separation;
		let radius = self.style.arc_radius;
		let total_width = max_width + radius * 4.0;
		let center_y = total_height / 2.0;
		let mut body = Svg::new();
		let mut y = 0.0;
		for alternative in drawn {
			let (width, height) = (alternative.width, alternative.height);
			let element_center = y + alternative.up;
			let element_x = radius * 2.0 + (max_width - width) / 2.0;
			Self::place(&mut body, alternative, element_x, y);
			let is_below = element_center > center_y;
			let toward = if is_below { radius } else { -radius };
			let left = if element_center == center_y {
				Path::default().move_to(0.0, center_y).line_to(element_x, element_center)
			} else {
				Path::default()
					.move_to(0.0, center_y)
					.arc_to(radius, is_below, radius, center_y + toward)
					.line_to(radius, element_center - toward)
					.arc_to(radius, !is_below, radius * 2.0, element_center)
					.line_to(element_x, element_center)
			};
			self.line(&mut body, left);
			let right_start = element_x + width;
			let right = if element_center == center_y {
				Path::default().move_to(right_start, element_center).line_to(total_width, center_y)
			} else {
				Path::default()
					.move_to(right_start, element_center)
					.line_to(total_width - radius * 2.0, element_center)
					.arc_to(radius, !is_below, total_width - radius, element_center - toward)
					.line_to(total_width - radius, center_y + toward)
					.arc_to(radius, is_below, total_width, center_y)
			};
			self.line(&mut body, right);
			y += height + self.style.vertical_separation;
		}
		Drawn {
			class: "railroad-choice",
			body,
			width: total_width,
			height: total_height,
			up: center_y,
		}
	}

	/// The path over an element from the line at `center_y` back down to it.
	fn bypass(&self, total_width: f64, center_y: f64) -> Path {
		let radius = self.style.arc_radius;
		Path::default()
			.move_to(0.0, center_y)
			.arc_to(radius, false, radius, center_y - radius)
			.line_to(radius, radius)
			.arc_to(radius, true, radius * 2.0, 0.0)
			.line_to(total_width - radius * 2.0, 0.0)
			.arc_to(radius, true, total_width - radius, radius)
			.line_to(total_width - radius, center_y - radius)
			.arc_to(radius, false, total_width, center_y)
	}

	fn optional(&mut self, element: &Expression) -> Drawn {
		let inner = self.draw(element);
		let radius = self.style.arc_radius;
		let total_width = inner.width + radius * 4.0;
		let total_height = inner.height + radius * 2.0;
		let element_x = radius * 2.0;
		let element_y = radius * 2.0;
		let center_y = element_y + inner.up;
		let inner_width = inner.width;
		let mut body = Svg::new();
		Self::place(&mut body, inner, element_x, element_y);
		self.line(
			&mut body,
			Path::default().move_to(0.0, center_y).line_to(radius * 2.0, center_y),
		);
		let after = element_x + inner_width;
		self.line(
			&mut body,
			Path::default().move_to(after, center_y).line_to(total_width, center_y),
		);
		self.line(&mut body, self.bypass(total_width, center_y));
		Drawn {
			class: "railroad-optional",
			body,
			width: total_width,
			height: total_height,
			up: center_y,
		}
	}

	fn repetition(&mut self, element: &Expression, has_bypass: bool) -> Drawn {
		let inner = self.draw(element);
		let radius = self.style.arc_radius;
		let arc_height = radius * 2.0;
		let total_width = inner.width + radius * 4.0;
		let total_height = inner.height + arc_height + if has_bypass { arc_height } else { 0.0 };
		let element_x = radius * 2.0;
		let element_y = if has_bypass { arc_height } else { 0.0 };
		let center_y = element_y + inner.up;
		let (inner_width, inner_height) = (inner.width, inner.height);
		let mut body = Svg::new();
		Self::place(&mut body, inner, element_x, element_y);
		self.line(
			&mut body,
			Path::default().move_to(0.0, center_y).line_to(radius * 2.0, center_y),
		);
		let after = element_x + inner_width;
		self.line(
			&mut body,
			Path::default().move_to(after, center_y).line_to(total_width, center_y),
		);
		let loop_y = element_y + inner_height + radius;
		let loop_back = Path::default()
			.move_to(after, center_y)
			.arc_to(radius, true, after + radius, center_y + radius)
			.line_to(after + radius, loop_y)
			.arc_to(radius, true, after, loop_y + radius)
			.line_to(radius * 2.0, loop_y + radius)
			.arc_to(radius, true, radius, loop_y)
			.line_to(radius, center_y + radius)
			.arc_to(radius, true, radius * 2.0, center_y);
		self.line(&mut body, loop_back);
		if has_bypass {
			self.line(&mut body, self.bypass(total_width, center_y));
		}
		Drawn {
			class: "railroad-repetition",
			body,
			width: total_width,
			height: total_height,
			up: center_y,
		}
	}

	/// `renderRule`: the rule at `y`, returning its width and height.
	fn rule(&mut self, svg: &mut Svg, rule: &Rule, y: f64) -> (f64, f64) {
		let name = format!("{} =", rule.name);
		let name_width = self.measure(&name).0 + 20.0;
		let definition_x = name_width + 20.0;
		let result = self.draw(&rule.definition);
		let (width, height, up) = (result.width, result.height, result.up);
		let baseline = up.max(20.0);
		let definition_y = baseline - up;
		let marker_radius = self.style.marker_radius;
		svg.open("g")
			.attr("class", "railroad-rule")
			.attr("transform", &format!("translate(0, {})", number(y)))
			.end();
		svg.open("g")
			.attr(
				"transform",
				&format!("translate({}, {})", number(definition_x), number(definition_y)),
			)
			.end();
		svg.open("g").attr("class", result.class).end();
		svg.append(result.body);
		svg.close_tag("g").close_tag("g");
		svg.open("g").attr("class", "railroad-rule-name-group").end();
		let mut font = self.style.font.clone();
		font.weight = 700;
		let run = TextRun {
			x: 0.0,
			y: baseline,
			anchor: Anchor::Start,
			baseline: Baseline::Alphabetic,
			font: &font,
			content: &name,
		};
		let color = &self.style.rule_name_color;
		svg.text(run, |svg| _ = svg.attr("class", "railroad-rule-name").attr("fill", color));
		svg.close_tag("g");
		let end_x = definition_x + width + 10.0;
		for (class, x) in [("railroad-start", name_width), ("railroad-end", end_x)] {
			svg.open("g").attr("class", class).end();
			svg.open("circle")
				.number_attr("cx", x)
				.number_attr("cy", baseline)
				.number_attr("r", marker_radius)
				.attr("fill", &self.style.marker_fill)
				.close();
			svg.close_tag("g");
		}
		let start_line = Path::default()
			.move_to(name_width + marker_radius, baseline)
			.line_to(definition_x, baseline);
		self.line(svg, start_line);
		let end_line = Path::default()
			.move_to(definition_x + width, baseline)
			.line_to(end_x - marker_radius, baseline);
		self.line(svg, end_line);
		svg.close_tag("g");
		let rule_height = (definition_y + height + self.style.padding * 2.0).max(40.0);
		(end_x + marker_radius, rule_height)
	}
}

pub fn render(code: &str, kind: DiagramKind, context: &mut Context) -> Result<Diagram> {
	let syntax = match kind {
		DiagramKind::RailroadEbnf => Syntax::Ebnf,
		DiagramKind::RailroadAbnf => Syntax::Abnf,
		DiagramKind::RailroadPeg => Syntax::Peg,
		_ => Syntax::Functions,
	};
	let (fields, rules) = parser::parse(code, syntax, context)?;
	let use_max_width = context.config_bool(SECTION, "useMaxWidth", true);
	let mut body = Svg::new();
	let (width, height) = if rules.is_empty() {
		(200.0, 100.0)
	} else {
		let style = Style::new(context);
		let padding = style.padding;
		let vertical_separation = style.vertical_separation;
		let mut writer = Writer { context, style };
		let mut y = padding;
		let mut max_width: f64 = 0.0;
		for rule in &rules {
			let (width, height) = writer.rule(&mut body, rule, y);
			y += height + vertical_separation;
			max_width = max_width.max(width);
		}
		(max_width + padding * 2.0, y + padding)
	};
	let frame = Frame { x: 0.0, y: 0.0, width, height, use_max_width };
	Ok(document(context, kind.config_key(), &frame, &fields.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn rendered(code: &str, kind: DiagramKind) -> String {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		render(code, kind, &mut context)
			.map(|diagram| diagram.svg)
			.unwrap_or_else(|error| error.message)
	}

	#[test]
	fn draws_a_rule_like_mermaid() {
		let svg = rendered("railroad-beta\ndigit = terminal(\"0\") ;", DiagramKind::Railroad);
		// FixedMeasure: "0" is 9.6 x 19.2, "digit =" 67.2 wide.
		assert!(svg.contains("viewBox=\"0 0 171.8 87.6\""), "{svg}");
		assert!(svg.contains("<g class=\"railroad-terminal\""), "{svg}");
		assert!(svg.contains("width=\"29.6\" height=\"39.2\" rx=\"10\" ry=\"10\""), "{svg}");
		assert!(svg.contains("d=\"M 92.2 20 L 107.2 20\""), "{svg}");
		assert!(svg.contains("font-weight=\"700\""), "{svg}");
	}

	#[test]
	fn draws_every_expression() {
		let svg = rendered(
			"railroad-ebnf-beta\na = b , { c | \"d\" } , [ e ] , f+ , ? x ? ;",
			DiagramKind::RailroadEbnf,
		);
		for class in [
			"railroad-sequence",
			"railroad-repetition",
			"railroad-choice",
			"railroad-optional",
			"railroad-special",
		] {
			assert!(svg.contains(class), "{class}: {svg}");
		}
		assert!(svg.contains("stroke-dasharray=\"5,3\""));
		assert!(svg.contains("? x ?"));
	}

	#[test]
	fn sanitizes_style_values() {
		let text = |text: &str| Value::String(text.to_owned());
		assert_eq!(sanitize_color(Some(&text(" #abc ")), "x"), "#abc");
		assert_eq!(sanitize_color(Some(&text("hsl(1, 2%, 3%)")), "x"), "hsl(1, 2%, 3%)");
		assert_eq!(sanitize_color(Some(&text("red;x")), "x"), "x");
		assert_eq!(sanitize_color(Some(&text("url(#a)")), "x"), "x");
		assert_eq!(sanitize_color(Some(&Value::Number(1.0)), "x"), "x");
		assert_eq!(sanitize_font_family(Some(&text("\"a b\", c")), "x"), "\"a b\", c");
		assert_eq!(sanitize_font_family(Some(&text("a;b")), "x"), "x");
		assert_eq!(sanitize_number(Some(&text("12px")), 1.0), 12.0);
		assert_eq!(sanitize_number(Some(&Value::Number(-1.0)), 1.0), 1.0);
	}

	#[test]
	fn draws_an_empty_diagram() {
		let svg = rendered("railroad-peg-beta\n", DiagramKind::RailroadPeg);
		assert!(svg.contains("viewBox=\"0 0 200 100\""), "{svg}");
	}
}
