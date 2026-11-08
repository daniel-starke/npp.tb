//! The `info` diagram (`info.langium`, `infoRenderer.ts`): the Mermaid version.

use super::common::{CommonFields, Lines, starts_with_word, strip_comment};
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document};
use super::text::Font;
use super::{Context, Diagram, Result};

/// The Mermaid release whose syntax and rendering this implementation follows.
pub const MERMAID_VERSION: &str = "12.2.0";

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let mut common = CommonFields::new(context);
	let mut lines = Lines::new(code);
	let mut has_header = false;
	let mut has_show_info = false;
	while let Some((line_number, line)) = lines.next_line() {
		let statement = strip_comment(line).trim();
		if statement.is_empty() {
			continue;
		}
		if !has_header {
			if statement != "info" {
				return Err(context.error_at(line_number, "expected `info`"));
			}
			has_header = true;
		} else if !has_show_info
			&& starts_with_word(statement, "showInfo")
			&& statement == "showInfo"
		{
			has_show_info = true;
		} else if let Some(result) = common.parse(statement, &mut lines) {
			result.map_err(|error| {
				context.error_at(error.line.unwrap_or(line_number), error.message)
			})?;
		} else {
			return Err(context.error_at(line_number, format!("unexpected {statement:?}")));
		}
	}
	let mut body = Svg::new();
	body.open("g").end();
	let font = Font::new(context.font_family(), 32.0);
	let version = format!("v{MERMAID_VERSION}");
	let run = TextRun {
		x: 100.0,
		y: 40.0,
		anchor: Anchor::Middle,
		baseline: Baseline::Alphabetic,
		font: &font,
		content: &version,
	};
	body.text(run, |svg| _ = svg.attr("class", "version"));
	body.close_tag("g");
	let frame = Frame { x: 0.0, y: 0.0, width: 400.0, height: 100.0, use_max_width: true };
	Ok(document(context, "info", &frame, &common.accessibility, body))
}
