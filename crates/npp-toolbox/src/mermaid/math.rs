//! Math in labels: Mermaid's `$$…$$` (`diagrams/common/common.ts`, KaTeX with MathML output)
//! typeset as SVG, because EdgeHTML has no MathML and neither it nor resvg shows
//! `foreignObject`. `markdown_html::math` converts the LaTeX to MathML, and [`layout`] lays that
//! out like Blink's MathML Core with the OpenType `MATH` table of the math font.

mod layout;
mod mathml;
mod open_type;
mod operators;

use std::rc::Rc;

use super::Context;
use super::svg::{Anchor, Baseline, Svg, TextRun, number};
use super::text::Font;
use crate::markdown_html::latex_to_mathml;
pub use open_type::MathTable;

/// The math font of Chromium on Windows, then what other systems install.
pub const FONT_FAMILY: &str = "\"Cambria Math\", \"STIX Two Math\", \"Latin Modern Math\", serif";

/// A formula laid out, with its origin at the left end of its baseline.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MathBox {
	pub width: f64,
	pub ascent: f64,
	pub descent: f64,
	items: Vec<Item>,
}

/// Something drawn, relative to the origin of its box.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
	x: f64,
	y: f64,
	/// `mathcolor` instead of the color of the label.
	color: Option<Rc<str>>,
	/// The target of `\href`.
	link: Option<Rc<str>>,
	kind: ItemKind,
}

#[derive(Clone, Debug, PartialEq)]
enum ItemKind {
	Text {
		text: String,
		font: Font,
	},
	/// A size variant or part of an assembly, which have no characters.
	Glyph {
		glyph: u16,
		size: f64,
	},
	Bar {
		width: f64,
		height: f64,
	},
	/// A straight line as a filled shape, whose fill is that of the text.
	Line {
		end_x: f64,
		end_y: f64,
		thickness: f64,
	},
}

impl MathBox {
	pub fn height(&self) -> f64 {
		self.ascent + self.descent
	}

	/// Draws the formula with the left end of its baseline at `(x, baseline)`; `attributes` writes
	/// the fill of everything without a color of its own.
	pub fn draw(
		&self,
		svg: &mut Svg,
		context: &mut Context,
		(x, baseline): (f64, f64),
		attributes: &dyn Fn(&mut Svg),
	) {
		let mut open_link: Option<&Rc<str>> = None;
		for item in &self.items {
			if item.link.as_ref() != open_link {
				if open_link.is_some() {
					svg.close_tag("a");
				}
				if let Some(link) = &item.link {
					svg.open("a").attr("xlink:href", link).end();
				}
				open_link = item.link.as_ref();
			}
			let (item_x, item_y) = (x + item.x, baseline + item.y);
			let paint = |svg: &mut Svg| match &item.color {
				Some(color) => _ = svg.attr("fill", color),
				None => attributes(svg),
			};
			match &item.kind {
				ItemKind::Text { text, font } => {
					let run = TextRun {
						x: item_x,
						y: item_y,
						anchor: Anchor::Start,
						baseline: Baseline::Alphabetic,
						font,
						content: text,
					};
					svg.text(run, paint);
				}
				ItemKind::Glyph { glyph, size } => {
					let font = Font::new(FONT_FAMILY, *size);
					let Some(outline) = context.glyph_outline(&font, *glyph) else { continue };
					if outline.is_empty() {
						continue;
					}
					svg.open("path")
						.attr(
							"transform",
							&format!("translate({},{})", number(item_x), number(item_y)),
						)
						.attr("d", &outline);
					paint(svg);
					svg.close();
				}
				ItemKind::Bar { width, height } => {
					svg.open("rect")
						.number_attr("x", item_x)
						.number_attr("y", item_y)
						.number_attr("width", *width)
						.number_attr("height", *height);
					paint(svg);
					svg.close();
				}
				ItemKind::Line { end_x, end_y, thickness } => {
					let (delta_x, delta_y) = (*end_x, *end_y);
					let length = delta_x.hypot(delta_y).max(f64::MIN_POSITIVE);
					let normal_x = -delta_y / length * thickness / 2.0;
					let normal_y = delta_x / length * thickness / 2.0;
					let corner = |along: f64, side: f64| {
						format!(
							"{},{}",
							number(item_x + along * delta_x + side * normal_x),
							number(item_y + along * delta_y + side * normal_y)
						)
					};
					let path = format!(
						"M{}L{}L{}L{}Z",
						corner(0.0, 1.0),
						corner(1.0, 1.0),
						corner(1.0, -1.0),
						corner(0.0, -1.0)
					);
					svg.open("path").attr("d", &path);
					paint(svg);
					svg.close();
				}
			}
		}
		if open_link.is_some() {
			svg.close_tag("a");
		}
	}
}

/// A piece of a label with math.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part<'a> {
	Text(&'a str),
	Formula(&'a str),
}

/// Whether `/\$\$(.*?)\$\$/` matches in `text`: Mermaid's `hasKatex`.
pub fn has_math(text: &str) -> bool {
	next_formula(text).is_some()
}

/// The start of the next match in `text` and the range of its formula.
fn next_formula(text: &str) -> Option<(usize, std::ops::Range<usize>)> {
	let is_line_end = |character| matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}');
	let mut search = 0;
	while let Some(found) = text[search..].find("$$") {
		let start = search + found;
		let formula_start = start + 2;
		let line_end =
			text[formula_start..].find(is_line_end).map_or(text.len(), |end| formula_start + end);
		if let Some(end) = text[formula_start..line_end].find("$$") {
			return Some((start, formula_start..formula_start + end));
		}
		search = start + 1;
	}
	None
}

/// `text` split at the matches of `katexRegex`.
pub fn split(text: &str) -> Vec<Part<'_>> {
	let mut parts = Vec::new();
	let mut rest = text;
	while let Some((start, formula)) = next_formula(rest) {
		if start > 0 {
			parts.push(Part::Text(&rest[..start]));
		}
		parts.push(Part::Formula(&rest[formula.clone()]));
		rest = &rest[formula.end + 2..];
	}
	if !rest.is_empty() {
		parts.push(Part::Text(rest));
	}
	parts
}

/// How many `<mrow>` Mermaid finds in the HTML of `text`, which counts as lines of a class member:
/// KaTeX's row around each formula and its groups of more than one element.
pub fn row_count(text: &str) -> usize {
	let formulas = split(text).into_iter().filter_map(|part| match part {
		Part::Formula(latex) => Some(latex),
		Part::Text(_) => None,
	});
	formulas
		.map(|latex| {
			let document = latex_to_mathml(latex).ok().and_then(|mathml| mathml::parse(&mathml));
			let rows = document.map_or(0, |document| {
				let is_row = |element: &&mathml::Element| {
					element.tag == mathml::Tag::Mrow && element.children.len() > 1
				};
				document.elements.iter().filter(is_row).count()
			});
			1 + rows
		})
		.sum()
}

/// Lays out a formula in the font `size`. A formula KaTeX would reject makes the diagram fail
/// with Mermaid's message when it is finished, and is empty until then.
pub fn layout(context: &mut Context, latex: &str, size: f64) -> MathBox {
	match try_layout(context, latex, size) {
		Ok(math_box) => math_box,
		Err(message) => {
			context.fail(format!("KaTeX parse error: {message}"));
			MathBox::default()
		}
	}
}

fn try_layout(
	context: &mut Context,
	latex: &str,
	size: f64,
) -> std::result::Result<MathBox, String> {
	let latex = latex.replace('\n', " ");
	let mathml = latex_to_mathml(&latex)?;
	let document = mathml::parse(&mathml).ok_or("Too many nested groups")?;
	let font = Font::new(FONT_FAMILY, 1.0);
	let table = context.math_table(&font);
	let design_unit = context.design_unit(&font);
	let mut engine =
		layout::Engine { context, document: &document, table, design_unit, layouts: 0 };
	let laid = engine.layout_root(size);
	if engine.is_exhausted() {
		return Err("Too many nested groups".to_owned());
	}
	Ok(MathBox { width: laid.width, ascent: laid.ascent, descent: laid.descent, items: laid.items })
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn math_box(latex: &str) -> MathBox {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let result = layout(&mut context, latex, 10.0);
		assert_eq!(context.failure(), None, "{latex}");
		result
	}

	#[test]
	fn finds_formulas_like_the_regex() {
		assert!(has_math("a $$x$$ b"));
		assert!(has_math("$$ $$"));
		assert!(has_math("$$$x$$"));
		assert!(!has_math("$$x"));
		assert!(!has_math("$$a\nb$$"));
		assert!(has_math("$$a\nb$$c$$"));
		assert_eq!(
			split("a $$x$$ b$$y$$"),
			[Part::Text("a "), Part::Formula("x"), Part::Text(" b"), Part::Formula("y")]
		);
		assert_eq!(split("$$$x$$"), [Part::Formula("$x")]);
		assert_eq!(split("$$x"), [Part::Text("$$x")]);
	}

	#[test]
	fn counts_rows_like_the_html() {
		assert_eq!(row_count("a"), 0);
		assert_eq!(row_count("$$x$$"), 1);
		assert_eq!(row_count("$$x$$ a $$y$$"), 2);
		assert_eq!(row_count("$$\\frac{1}{2}$$"), 1);
		assert_eq!(row_count("$$\\frac{a+b}{2}$$"), 2);
	}

	#[test]
	fn lays_out_tokens_in_a_row() {
		// 6 pixels per character at size 10, 7 above and 2 below the baseline.
		let token = math_box("x");
		assert_eq!((token.width, token.ascent, token.descent), (6.0, 7.0, 2.0));
		// `+` has spaces of 4/18 em on both sides of the row.
		let sum = math_box("a+b");
		let space = (4.0f64 * 10.0 / 18.0 * 64.0).trunc() / 64.0;
		assert_eq!(sum.width, 18.0 + 2.0 * space);
		// The function name stays upright, a single letter does not.
		assert!(matches!(&math_box("x").items[0].kind,
			ItemKind::Text { text, .. } if text == "\u{1D465}"));
		assert!(matches!(&math_box("\\sin").items[0].kind,
			ItemKind::Text { text, .. } if text == "sin"));
	}

	#[test]
	fn lays_out_scripts_and_fractions_with_fallback_constants() {
		let base = math_box("x");
		let power = math_box("x^2");
		assert!(power.width > base.width && power.ascent > base.ascent);
		let index = math_box("x_2");
		assert!(index.descent > base.descent);
		let both = math_box("x_1^2");
		assert!(both.ascent > base.ascent && both.descent > base.descent);
		let fraction = math_box("\\frac{a}{b}");
		// `padding-inline: 1px` around the wider part.
		assert_eq!(fraction.width, 8.0);
		assert!(fraction.height() > 2.0 * base.height());
		assert!(fraction.items.iter().any(|item| matches!(item.kind, ItemKind::Bar { .. })));
		let without_bar = math_box("\\binom{a}{b}");
		assert!(without_bar.items.iter().all(|item| !matches!(item.kind, ItemKind::Bar { .. })));
	}

	#[test]
	fn scales_scripts_and_keeps_accents_in_size() {
		let font_size = |math_box: &MathBox, text: &str| {
			math_box.items.iter().find_map(|item| match &item.kind {
				ItemKind::Text { text: own, font } if own == text => Some(font.size),
				_ => None,
			})
		};
		assert!(font_size(&math_box("x^2"), "2").unwrap() < 8.0);
		assert_eq!(font_size(&math_box("\\frac{1}{2}"), "2"), Some(10.0));
		assert!(font_size(&math_box("\\frac{\\frac{1}{2}}{3}"), "2").unwrap() < 8.0);
		assert_eq!(font_size(&math_box("\\hat{x}"), "^"), Some(10.0));
	}

	#[test]
	fn lays_out_roots_tables_and_limits() {
		let root = math_box("\\sqrt{x}");
		assert!(root.width >= 6.0);
		assert!(root.items.iter().any(|item| matches!(item.kind, ItemKind::Bar { .. })));
		assert!(math_box("\\sqrt[3]{x}").width > root.width);
		let matrix = math_box("\\begin{matrix}a&b\\\\c&d\\end{matrix}");
		// Two columns with 0.4 em on both sides of each cell.
		let padding = (0.4f64 * 10.0 * 64.0).trunc() / 64.0;
		assert_eq!(matrix.width, 12.0 + 4.0 * padding);
		assert!(matrix.height() > 2.0 * math_box("a").height());
		let limits = math_box("\\sum_{i}^{n}");
		assert!(limits.height() > math_box("\\sum").height());
	}

	#[test]
	fn uses_the_color_of_formulas() {
		let colored = math_box("\\textcolor{red}{x}");
		assert_eq!(colored.items[0].color.as_deref(), Some("red"));
		assert_eq!(math_box("x").items[0].color, None);
		assert!(math_box("\\phantom{x}").items.is_empty());
		assert_eq!(math_box("\\phantom{x}").width, 6.0);
	}

	#[test]
	fn draws_text_bars_and_colors() {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let formula = layout(&mut context, "\\frac{\\textcolor{red}{a}}{b}", 10.0);
		let mut svg = Svg::new();
		formula.draw(&mut svg, &mut context, (10.0, 20.0), &|svg: &mut Svg| {
			svg.attr("fill", "blue");
		});
		let out = &svg.out;
		assert!(out.contains("<rect x=\"11\""), "{out}");
		assert!(out.contains("fill=\"red\""), "{out}");
		assert!(out.contains("fill=\"blue\""), "{out}");
		assert_eq!(out.matches("<text").count(), 2);
	}

	#[test]
	fn lays_out_padded_boxes_like_blink() {
		let plain = math_box("x");
		// `height`, `depth` and `width` replace the sizes of the box, the content stays.
		let smash = math_box("\\smash{x}");
		assert_eq!((smash.width, smash.ascent, smash.descent), (6.0, 0.0, 0.0));
		assert_eq!(smash.items.len(), 1);
		let top = math_box("\\smash[t]{x}");
		assert_eq!((top.ascent, top.descent), (0.0, plain.descent));
		let hidden = math_box("\\hphantom{x}");
		assert_eq!((hidden.width, hidden.ascent, hidden.items.len()), (6.0, 0.0, 0));
		let upright = math_box("\\vphantom{x}");
		assert_eq!((upright.width, upright.ascent), (0.0, plain.ascent));
		// `voffset` moves the content only, and the unit `width` of KaTeX's `lspace` is invalid.
		let raised = math_box("\\raisebox{1em}{x}");
		assert_eq!((raised.ascent, raised.items[0].y), (plain.ascent, -10.0));
		let left = math_box("\\llap{x}");
		assert_eq!((left.width, left.items[0].x), (0.0, 0.0));
		// The rule overflows a box of the height of the raise.
		let rule = math_box("\\rule{1em}{0.5em}");
		assert_eq!((rule.width, rule.ascent), (10.0, 0.0));
		assert_eq!((rule.items[0].x, rule.items[0].y), (0.0, -5.0));
		assert_eq!(rule.items[0].color.as_deref(), Some("black"));
		assert!(matches!(rule.items[0].kind, ItemKind::Bar { width: 10.0, height: 5.0 }));
	}

	#[test]
	fn draws_boxes_and_lines() {
		let plain = math_box("x");
		let boxed = math_box("\\boxed{x}");
		// `border: 1px` and `padding: 0.2em` on all sides.
		assert_eq!(boxed.width, plain.width + 2.0 * 3.0);
		assert_eq!(boxed.ascent, plain.ascent + 3.0);
		assert_eq!(boxed.descent, plain.descent + 3.0);
		let bars = boxed.items.iter().filter(|item| matches!(item.kind, ItemKind::Bar { .. }));
		assert_eq!(bars.count(), 4);
		let colored = math_box("\\colorbox{red}{x}");
		assert_eq!(colored.items[0].color.as_deref(), Some("red"));
		assert!(colored.width > plain.width);
		assert_eq!(colored.items.len(), 2);
		let framed = math_box("\\fcolorbox{green}{red}{x}");
		assert_eq!(framed.items.len(), 6);
		assert_eq!(framed.items[1].color.as_deref(), Some("green"));
		for (latex, lines) in
			[("\\cancel{x}", 1), ("\\bcancel{x}", 1), ("\\xcancel{x}", 2), ("\\sout{x}", 1)]
		{
			let struck = math_box(latex);
			assert_eq!(struck.width, plain.width, "{latex}");
			let drawn =
				struck.items.iter().filter(|item| matches!(item.kind, ItemKind::Line { .. }));
			assert_eq!(drawn.count(), lines, "{latex}");
		}
	}

	#[test]
	fn shadows_poor_bold_without_changing_the_box() {
		let bold = math_box("\\pmb{x}");
		assert_eq!(bold.width, math_box("x").width);
		assert_eq!(bold.items.len(), 2);
	}

	#[test]
	fn writes_links_around_their_items() {
		let mut measure = FixedMeasure;
		let mut context = Context::for_tests(&mut measure);
		let formula = layout(&mut context, "a\\href{https://x.org}{bc}d", 10.0);
		let mut svg = Svg::new();
		formula.draw(&mut svg, &mut context, (0.0, 0.0), &|_: &mut Svg| {});
		let out = &svg.out;
		assert_eq!(out.matches("<a xlink:href=\"https://x.org\">").count(), 1, "{out}");
		assert_eq!(out.matches("</a>").count(), 1, "{out}");
		assert!(out.find("<a ").unwrap() > out.find("<text").unwrap(), "{out}");
		assert!(out.ends_with("</text>"), "{out}");
	}

	#[test]
	fn lays_out_tags_as_cells() {
		let tagged = math_box("x\\tag{1}");
		// Four cells with 0.4 em on both sides, of which two are empty.
		let padding = (0.4f64 * 10.0 * 64.0).trunc() / 64.0;
		assert_eq!(tagged.width, 6.0 + 18.0 + 8.0 * padding);
	}

	#[test]
	fn reports_what_katex_rejects() {
		for (latex, message) in [
			(
				"\\foo",
				"Undefined control sequence: \\foo at position 1: \\\u{332}f\u{332}o\u{332}o\u{332}",
			),
			("x^", "Expected group after '^' at position 2: x^\u{332}"),
			("{x", "Expected '}', got 'EOF' at end of input: {x"),
			("x}", "Expected 'EOF', got '}' at position 2: x}\u{332}"),
			("x^1^2", "Double superscript at position 4: x^1^\u{332}2"),
			("\\left( x", "Expected '\\right', got 'EOF' at end of input: \\left( x"),
			("x&y", "Expected 'EOF', got '&' at position 2: x&\u{332}y"),
			(
				"x\\label{a}",
				"Undefined control sequence: \\label at position 2: x\\\u{332}l\u{332}a\u{332}b\u{332}e\u{332}l\u{332}{a}",
			),
			(
				"\\def\\a{\\a}\\a",
				"Too many expansions: infinite loop or need to increase maxExpand setting",
			),
		] {
			let mut measure = FixedMeasure;
			let mut context = Context::for_tests(&mut measure);
			layout(&mut context, latex, 10.0);
			assert_eq!(
				context.failure().map(|error| error.message.clone()),
				Some(format!("KaTeX parse error: {message}")),
				"{latex}"
			);
		}
	}

	/// Development aid: the boxes of the formulas of the file `MATH_PROBE` at 14 pixels, to compare
	/// with what Edge lays out for them.
	#[cfg(windows)]
	#[test]
	#[ignore]
	fn dump_math_boxes() {
		let file = std::env::var("MATH_PROBE").unwrap();
		let mut text = npp::direct_write::TextEngine::new().unwrap();
		for line in std::fs::read_to_string(file).unwrap().lines() {
			let mut context = Context::for_tests(&mut text);
			let latex = line.replace("\\\\\\\\", "\\\\");
			let size = std::env::var("MATH_PROBE_SIZE").map_or(14.0, |size| size.parse().unwrap());
			let formula = layout(&mut context, &latex, size);
			println!("=== {line}");
			println!("math {} {}", formula.width, formula.height());
			if let Some(failure) = context.failure() {
				println!("error {}", failure.message);
			}
			for item in &formula.items {
				let x = item.x;
				let y = item.y + formula.ascent;
				match &item.kind {
					ItemKind::Text { text, font } => {
						println!("text {x} {y} {} {text}", font.size)
					}
					ItemKind::Glyph { glyph, size } => {
						let font = Font::new(FONT_FAMILY, *size);
						let bounds = context.glyph_bounds(&font, *glyph);
						let advance = context.glyph_advance(&font, *glyph);
						println!("glyph {x} {y} {size} {glyph} {bounds:?} {advance}");
					}
					ItemKind::Bar { width, height } => println!("bar {x} {y} {width} {height}"),
					ItemKind::Line { end_x, end_y, .. } => println!("line {x} {y} {end_x} {end_y}"),
				}
			}
		}
	}

	#[test]
	fn survives_hostile_formulas() {
		let hostile = [
			"".to_owned(),
			" ".to_owned(),
			"{".repeat(10_000),
			"}".repeat(10_000),
			"\\frac".repeat(10_000),
			"\\sqrt".repeat(10_000),
			"x^{".repeat(10_000),
			"\\left(".repeat(10_000),
			"\\overrightarrow{".repeat(200) + "x" + &"}".repeat(200),
			"\\begin{matrix}".repeat(1000),
			"&".repeat(1000),
			"\\\\".repeat(1000),
			"\\text{".to_owned(),
			"\\mathbb".to_owned(),
			"x".repeat(5000),
			"\\frac{\\frac{\\frac{1}{2}}{3}}{4}".repeat(100),
			"\\def\\a#1{#1#1}".to_owned() + &"\\a{".repeat(40) + "x" + &"}".repeat(40),
			"\\smash{".repeat(2000),
			"\\boxed{".repeat(2000),
			"\\href{x}{".repeat(2000),
			"\\begin{CD}@>".repeat(1000),
			"\\pmb{".repeat(40) + "x" + &"}".repeat(40),
		];
		for latex in &hostile {
			let mut measure = FixedMeasure;
			let mut context = Context::for_tests(&mut measure);
			let _ = layout(&mut context, latex, 16.0);
		}
	}
}
