use super::*;

pub(crate) fn render(markdown: &str) -> RtfDocument {
	render_at_dpi(markdown, 96)
}

fn render_at_dpi(markdown: &str, pixels_per_inch: u32) -> RtfDocument {
	let mut load_image = |source: &str| {
		source.ends_with(".png").then(|| {
			let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
			bytes.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 3]);
			Rc::new(EmbeddedImage { media_type: "image/png", bytes })
		})
	};
	let mut highlight_code = |language: &str, code: &str| match language {
		"rust" => code
			.find("fn")
			.map(|start| {
				vec![HighlightedSpan { range: start..start + 2, kind: TokenKind::Keyword }]
			})
			.unwrap_or_default(),
		_ => Vec::new(),
	};
	render_rtf(
		markdown,
		&mut RtfOptions {
			theme: &LIGHT_THEME,
			page_width: 6000,
			pixels_per_inch,
			load_image: &mut load_image,
			highlight_code: &mut highlight_code,
		},
	)
}

/// The text RichEdit keeps for the body, following the character model of the module
/// documentation, to check the recorded positions against.
fn body(document: &RtfDocument) -> &str {
	let start = document.rtf.find("}\n\\pard").map_or(0, |end| end + 2);
	&document.rtf[start..]
}

#[test]
fn records_block_and_link_positions_like_rich_edit() {
	// RichEdit's text of this document is
	// "Para one\rTwo HYPERLINK \"https://x.y/\"link end\r".
	let document = render("Para one\n\nTwo [link](https://x.y/) end\n");
	assert_eq!(
		document.blocks,
		vec![SourceBlock { line: 0, position: 0 }, SourceBlock { line: 2, position: 9 }]
	);
	assert_eq!(
		document.links,
		vec![RtfLink { range: 37..41, target_range: 24..36, target: "https://x.y/".to_owned() }]
	);
	assert!(body(&document).contains("{\\field{\\*\\fldinst{HYPERLINK \"https://x.y/\"}}"));
}

#[test]
fn counts_table_rows_and_cells() {
	// "\u{fff9}\rA\u{7}B\u{7}\u{fffb}\r\u{fff9}\rC\u{7}D\u{7}\u{fffb}\rAfter\r"
	let document = render("|A|B|\n|-|-:|\n|C|D|\n\nAfter\n");
	assert_eq!(
		document.blocks,
		vec![
			SourceBlock { line: 0, position: 0 },
			SourceBlock { line: 2, position: 8 },
			SourceBlock { line: 4, position: 16 },
		]
	);
	let rtf = body(&document);
	assert_eq!(rtf.matches("\\trowd").count(), 2);
	assert_eq!(rtf.matches("\\cell ").count(), 4);
	assert!(rtf.contains("\\qr D\\cell"));
	assert!(rtf.contains("{\\b A}\\cell"));
}

#[test]
fn escapes_text_and_counts_utf16_units() {
	let document = render("{a}\\\\ ä😀\n\nnext");
	assert!(body(&document).contains("\\{a\\}\\\\ \\u228?\\u-10179?\\u-8704?\\par"));
	// "{a}\ " is 5, "ä" 1, the emoji 2 units, plus the paragraph end.
	assert_eq!(document.blocks[1].position, 9);
}

#[test]
fn writes_lists_code_and_headings() {
	let document = render("# Head\n\n- a\n  1. b\n\n```rust\nfn x\n```\n\n## Head");
	let rtf = body(&document);
	assert!(rtf.contains("\\b\\fs40"));
	assert!(rtf.contains("\\u8226?\\tab a\\par"));
	assert!(rtf.contains("1.\\tab b\\par"));
	assert!(rtf.contains("\\f1\\fs19\\highlight"));
	assert!(rtf.contains("{\\cf"));
	assert_eq!(
		document.anchors.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
		["head", "head-1"]
	);
	// "Head\r", "•\ta\r", "1.\tb\r", "fn x\r": the second heading starts at 19.
	assert_eq!(document.anchors[1].1, 19);
}

#[test]
fn embeds_png_pictures_and_shows_other_images_as_text() {
	let document = render("![alt](a.png) ![other](b.gif)");
	let rtf = body(&document);
	// 2 x 3 pixels are 52 x 79 HIMETRIC and 30 x 45 twips.
	assert!(rtf.contains("{\\pict\\pngblip\\picw52\\pich79\\picwgoal30\\pichgoal45\n89504e47"));
	assert!(rtf.contains("[other]"));
}

#[test]
fn puts_alert_icons_rendered_for_the_dpi_before_the_title() {
	// "\u{fffc} Note\r", the picture counting as one character, then "n".
	let document = render_at_dpi("> [!NOTE]\n> n", 144);
	assert_eq!(document.blocks[1].position, 7);
	let rtf = body(&document);
	// 16 pixels at 96 DPI are 423 HIMETRIC and 240 twips, rendered with 24 pixels at 144 DPI.
	let picture = "{\\pict\\pngblip\\picw423\\pich423\\picwgoal240\\pichgoal240\n89504e470d0a1a0a0000000d494844520000001800000018";
	assert!(rtf.contains(picture), "{rtf}");
	assert!(rtf.contains("} Note\\par"), "{rtf}");
}

#[test]
fn moves_the_space_before_pictures_on_a_first_line_to_the_previous_paragraph() {
	let rtf = body(&render("a\n\n> [!NOTE]\n> n")).to_owned();
	assert!(rtf.contains("a\\sa160\\par\n\\pard\\plain\\f0\\fs22\\cf1\\sb0\\li360\\b"), "{rtf}");
	let rtf = body(&render("a\n\n![x](a.png)")).to_owned();
	assert!(rtf.contains("a\\sa160\\par\n\\pard\\plain\\f0\\fs22\\cf1\\sb0\\li0 {\\pict"), "{rtf}");
	// The picture is on the second line.
	let rtf = body(&render("a\n\nb\\\n![x](a.png)")).to_owned();
	assert!(rtf.contains("a\\par\n\\pard\\plain\\f0\\fs22\\cf1\\sb160"), "{rtf}");
}

#[test]
fn keeps_text_of_raw_html() {
	let document = render("<p align=\"center\">\n  <b>Bold</b> text<br>more\n</p>\n");
	let rtf = body(&document);
	assert!(rtf.contains("Bold text\\line more\\par"), "{rtf}");
}

#[test]
fn reads_png_and_jpeg_sizes() {
	let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
	png.extend_from_slice(&[0, 0, 1, 0, 0, 0, 0, 32]);
	assert_eq!(image_size(&png), Some((256, 32)));
	let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0, 20, 0, 40, 1, 0, 0, 0];
	assert_eq!(image_size(&jpeg), Some((40, 20)));
	assert_eq!(image_size(&jpeg[..12]), None);
	assert_eq!(image_size(b"GIF89a"), None);
}

#[test]
fn draws_rules_as_bordered_cells() {
	// "a\r", the rule's "\u{fff9}\r\u{7}\u{fffb}\r", then "b".
	let document = render("a\n\n---\n\nb");
	assert_eq!(
		document.blocks,
		vec![
			SourceBlock { line: 0, position: 0 },
			SourceBlock { line: 2, position: 2 },
			SourceBlock { line: 4, position: 7 },
		]
	);
	assert!(body(&document).contains(r"\clbrdrt\brdrs\brdrw30"));
}

#[test]
fn sizes_table_columns_by_their_longest_cell() {
	// 140 twips per character of "Name" and "Value" plus 240 padding.
	let document = render("|Name|Value|\n|-|-|\n|a|1|\n|b|22|\n");
	assert!(body(&document).contains(r"\cellx800"));
	assert!(body(&document).contains(r"\cellx1740"));
}

#[test]
fn shares_narrow_table_widths_by_minimum_and_need() {
	assert_eq!(column_widths(&[100, 200], &[50, 50], 400), [100, 200]);
	// 100 beyond the minimums, the second column needs three times as much.
	assert_eq!(column_widths(&[100, 200], &[50, 50], 200), [75, 125]);
	assert_eq!(column_widths(&[100, 200], &[100, 300], 200), [50, 150]);
	assert!(column_widths(&[], &[], 0).is_empty());
}
