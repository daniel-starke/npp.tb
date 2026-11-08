//! Packet diagrams (`packet.langium`, `packet/parser.ts`, `db.ts`, `renderer.ts`, `styles.ts`).

use super::color::js_number;
use super::common::CommonFields;
use super::langium::Scanner;
use super::svg::{Anchor, Baseline, Frame, Svg, TextRun, document};
use super::text::{Font, parse_font_size};
use super::value::Value;
use super::{Context, Diagram, Result};

/// Mermaid's `maxPacketSize`: rows beyond it are dropped.
const MAX_ROWS: usize = 10_000;

#[derive(Clone, Debug, PartialEq)]
struct Block {
	start: f64,
	end: f64,
	label: String,
}

struct Packet {
	common: CommonFields,
	rows: Vec<Vec<Block>>,
}

/// A block of the grammar: `start[-end]: "label"` or `+bits: "label"`.
struct ParsedBlock {
	start: Option<f64>,
	end: Option<f64>,
	bits: Option<f64>,
	label: String,
	line: usize,
}

fn parse(code: &str, context: &Context) -> Result<(CommonFields, Vec<ParsedBlock>)> {
	let mut common = CommonFields::new(context);
	let mut blocks = Vec::new();
	let mut scanner = Scanner::new(code);
	scanner.skip_newlines();
	if !scanner.keyword("packet-beta", false) && !scanner.keyword("packet", true) {
		return Err(scanner.error(context, "`packet`"));
	}
	loop {
		if scanner.is_at_end() {
			break;
		}
		if scanner.newline() {
			continue;
		}
		if scanner.common_field(&mut common) {
			if !scanner.end_of_line() {
				return Err(scanner.error(context, "a new line"));
			}
			continue;
		}
		let line = scanner.line();
		let (start, end, bits) = if scanner.punctuation("+") {
			let bits = scanner.int().ok_or_else(|| scanner.error(context, "a number of bits"))?;
			(None, None, Some(bits))
		} else if let Some(start) = scanner.int() {
			let end = match scanner.punctuation("-") {
				true => Some(scanner.int().ok_or_else(|| scanner.error(context, "an end bit"))?),
				false => None,
			};
			(Some(start), end, None)
		} else {
			return Err(scanner.error(context, "a block like `0-15: \"label\"`"));
		};
		if !scanner.punctuation(":") {
			return Err(scanner.error(context, "`:`"));
		}
		let label = scanner.string().ok_or_else(|| scanner.error(context, "a quoted label"))?;
		if !scanner.end_of_line() {
			return Err(scanner.error(context, "a new line"));
		}
		blocks.push(ParsedBlock { start, end, bits, label, line });
	}
	Ok((common, blocks))
}

/// `populate`: checks the blocks and splits them into rows of `bits_per_row`.
fn populate(
	blocks: Vec<ParsedBlock>,
	bits_per_row: f64,
	context: &Context,
) -> Result<Vec<Vec<Block>>> {
	if bits_per_row.is_nan() || bits_per_row < 1.0 {
		return Err(context.error_at(1, "packet.bitsPerRow must be at least 1"));
	}
	let mut rows: Vec<Vec<Block>> = Vec::new();
	let push_row = |rows: &mut Vec<Vec<Block>>, row: Vec<Block>| {
		if !row.is_empty() {
			rows.push(row);
		}
	};
	let mut last_bit = -1.0;
	let mut word: Vec<Block> = Vec::new();
	let mut row = 1.0;
	for parsed in blocks {
		let line = parsed.line;
		if let (Some(start), Some(end)) = (parsed.start, parsed.end)
			&& end < start
		{
			return Err(context.error_at(
				line,
				format!(
					"Packet block {} - {} is invalid. End must be greater than start.",
					js_number(start),
					js_number(end)
				),
			));
		}
		let start = parsed.start.unwrap_or(last_bit + 1.0);
		if start != last_bit + 1.0 {
			return Err(context.error_at(
				line,
				format!(
					"Packet block {} - {} is not contiguous. It should start from {}.",
					js_number(start),
					js_number(parsed.end.unwrap_or(start)),
					js_number(last_bit + 1.0)
				),
			));
		}
		if parsed.bits == Some(0.0) {
			return Err(context.error_at(
				line,
				format!(
					"Packet block {} is invalid. Cannot have a zero bit field.",
					js_number(start)
				),
			));
		}
		let end = parsed.end.unwrap_or(start + parsed.bits.unwrap_or(1.0) - 1.0);
		last_bit = end;
		let mut block = Block { start, end, label: parsed.label };
		// Mermaid's loop, whose word length limit only matters for a fractional `bitsPerRow`,
		// where a row may never end on a block boundary.
		while word.len() as f64 <= bits_per_row + 1.0 && rows.len() < MAX_ROWS {
			if block.start > block.end {
				return Err(context.error_at(
					line,
					format!(
						"Block start {} is greater than block end {}.",
						js_number(block.start),
						js_number(block.end)
					),
				));
			}
			let row_start = row * bits_per_row;
			let next = (block.end + 1.0 > row_start).then(|| {
				let next = Block { start: row_start, end: block.end, label: block.label.clone() };
				block.end = row_start - 1.0;
				next
			});
			let closes_row = block.end + 1.0 == row_start;
			word.push(block);
			if closes_row {
				push_row(&mut rows, std::mem::take(&mut word));
				row += 1.0;
			}
			match next {
				Some(next) => block = next,
				None => break,
			}
		}
	}
	push_row(&mut rows, word);
	Ok(rows)
}

/// The packet style options of the theme (`styles.ts`) over Mermaid's defaults.
fn style_option(context: &Context, key: &str, default: &str) -> String {
	context
		.theme
		.get("packet")
		.and_then(|packet| packet.get(key))
		.and_then(Value::to_text)
		.unwrap_or_else(|| default.to_owned())
}

pub fn render(code: &str, context: &mut Context) -> Result<Diagram> {
	let (common, blocks) = parse(code, context)?;
	let config = |key: &str, default: f64| context.config_number("packet", key, default);
	let row_height = config("rowHeight", 32.0);
	let bit_width = config("bitWidth", 32.0);
	let bits_per_row = config("bitsPerRow", 32.0);
	let padding_x = config("paddingX", 5.0);
	let show_bits = context.config_bool("packet", "showBits", true);
	let padding_y = config("paddingY", 5.0) + if show_bits { 10.0 } else { 0.0 };
	let is_descending =
		context.config_text("packet", "bitOrder").is_some_and(|order| order == "descending");
	let use_max_width = context.config_bool("packet", "useMaxWidth", true);
	let rows = populate(blocks, bits_per_row, context)?;
	let packet = Packet { common, rows };

	let title = packet.common.title.clone().unwrap_or_default();
	let total_row_height = row_height + padding_y;
	let height = total_row_height * (packet.rows.len() as f64 + 1.0)
		- if title.is_empty() { row_height } else { 0.0 };
	let width = bit_width * bits_per_row + 2.0;

	let font_family = context.font_family();
	let font = |key: &str, default: &str| {
		let size = parse_font_size(&style_option(context, key, default), 16.0).unwrap_or(16.0);
		Font::new(font_family.clone(), size)
	};
	let byte_font = font("byteFontSize", "10px");
	let label_font = font("labelFontSize", "12px");
	let title_font = font("titleFontSize", "14px");
	let start_byte_color = style_option(context, "startByteColor", "black");
	let end_byte_color = style_option(context, "endByteColor", "black");
	let label_color = style_option(context, "labelColor", "black");
	let title_color = style_option(context, "titleColor", "black");
	let block_stroke_color = style_option(context, "blockStrokeColor", "black");
	let block_stroke_width = style_option(context, "blockStrokeWidth", "1");
	let block_fill_color = style_option(context, "blockFillColor", "#efefef");

	let mut body = Svg::new();
	for (row_number, row) in packet.rows.iter().enumerate() {
		body.open("g").end();
		let word_y = row_number as f64 * (row_height + padding_y) + padding_y;
		for block in row {
			let bits = block.end - block.start + 1.0;
			let first_column = block.start % bits_per_row;
			let column = match is_descending {
				true => bits_per_row - first_column - bits,
				false => first_column,
			};
			let block_x = column * bit_width + 1.0;
			let block_width = bits * bit_width - padding_x;
			body.open("rect")
				.number_attr("x", block_x)
				.number_attr("y", word_y)
				.number_attr("width", block_width)
				.number_attr("height", row_height)
				.attr("class", "packetBlock")
				.attr_if("stroke", &block_stroke_color)
				.attr_if("stroke-width", &block_stroke_width)
				.attr_if("fill", &block_fill_color)
				.close();
			let label = TextRun {
				x: block_x + block_width / 2.0,
				y: word_y + row_height / 2.0,
				anchor: Anchor::Middle,
				baseline: Baseline::Middle,
				font: &label_font,
				content: &block.label,
			};
			body.text(label, |svg| {
				svg.attr("class", "packetLabel").attr_if("fill", &label_color);
			});
			if !show_bits {
				continue;
			}
			let (leading_bit, trailing_bit) = match is_descending {
				true => (block.end, block.start),
				false => (block.start, block.end),
			};
			let is_single_bit = bits == 1.0;
			let bit_number_y = word_y - 2.0;
			let leading = js_number(leading_bit);
			let start = TextRun {
				x: block_x + if is_single_bit { block_width / 2.0 } else { 0.0 },
				y: bit_number_y,
				anchor: if is_single_bit { Anchor::Middle } else { Anchor::Start },
				baseline: Baseline::Alphabetic,
				font: &byte_font,
				content: &leading,
			};
			body.text(start, |svg| {
				svg.attr("class", "packetByte start").attr_if("fill", &start_byte_color);
			});
			if !is_single_bit {
				let trailing = js_number(trailing_bit);
				let end = TextRun {
					x: block_x + block_width,
					y: bit_number_y,
					anchor: Anchor::End,
					baseline: Baseline::Alphabetic,
					font: &byte_font,
					content: &trailing,
				};
				body.text(end, |svg| {
					svg.attr("class", "packetByte end").attr_if("fill", &end_byte_color);
				});
			}
		}
		body.close_tag("g");
	}
	if !title.is_empty() {
		let run = TextRun {
			x: width / 2.0,
			y: height - total_row_height / 2.0,
			anchor: Anchor::Middle,
			baseline: Baseline::Middle,
			font: &title_font,
			content: &title,
		};
		body.text(run, |svg| {
			svg.attr("class", "packetTitle").attr_if("fill", &title_color);
		});
	}
	let frame = Frame { x: 0.0, y: 0.0, width, height, use_max_width };
	Ok(document(context, "packet", &frame, &packet.common.accessibility, body))
}

#[cfg(test)]
mod tests {
	use super::super::text::testing::FixedMeasure;
	use super::*;

	fn rows(code: &str, bits_per_row: f64) -> Result<Vec<Vec<Block>>> {
		let mut measure = FixedMeasure;
		let context = Context::for_tests(&mut measure);
		let (_, blocks) = parse(code, &context)?;
		populate(blocks, bits_per_row, &context)
	}

	fn bounds(rows: &[Vec<Block>]) -> Vec<Vec<(f64, f64)>> {
		rows.iter().map(|row| row.iter().map(|block| (block.start, block.end)).collect()).collect()
	}

	#[test]
	fn splits_blocks_into_rows() {
		let parsed =
			rows("packet\n0-10: \"test\"\n11-90: \"multiple\"\n91-94: \"x\"", 32.0).unwrap();
		assert_eq!(
			bounds(&parsed),
			[vec![(0.0, 10.0), (11.0, 31.0)], vec![(32.0, 63.0)], vec![(64.0, 90.0), (91.0, 94.0)]]
		);
		assert_eq!(parsed[1][0].label, "multiple");
		let parsed = rows("packet-beta\n+8: \"byte\"\n+16: 'word'\n24: \"bit\"", 32.0).unwrap();
		assert_eq!(bounds(&parsed), [vec![(0.0, 7.0), (8.0, 23.0), (24.0, 24.0)]]);
	}

	#[test]
	fn reports_invalid_blocks() {
		let error = rows("packet\n0-10: \"a\"\n12-14: \"b\"", 32.0).unwrap_err();
		assert_eq!(error.line, Some(3));
		assert!(error.message.contains("not contiguous. It should start from 11"));
		assert!(rows("packet\n5-1: \"a\"", 32.0).unwrap_err().message.contains("End must be"));
		assert!(rows("packet\n+0: \"a\"", 32.0).unwrap_err().message.contains("zero bit"));
		assert!(rows("packet\n0-1: \"a\" 2-3: \"b\"", 32.0).is_err());
		assert!(rows("packet\n0-1: a", 32.0).is_err());
		assert!(rows("packetx\n0-1: \"a\"", 32.0).is_err());
		assert!(rows("packet\n0-1 \"a\"", 32.0).is_err());
		assert_eq!(rows("packet\n+1000000000: \"a\"", 1.0).unwrap().len(), MAX_ROWS);
	}
}
