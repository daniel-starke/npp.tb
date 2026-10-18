use super::*;

fn render_with(markdown: &str, styling: Styling) -> String {
	render_with_lines(markdown, styling, false)
}

fn render_with_lines(markdown: &str, styling: Styling, annotates_source_lines: bool) -> String {
	let mut replace_image_source = |syntax: ImageSyntax, source: &str| {
		source.ends_with(".png").then(|| format!("data:image/png;{syntax:?}"))
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
	let mut options = RenderOptions {
		annotates_source_lines,
		is_dark: false,
		styling,
		replace_image_source: &mut replace_image_source,
		highlight_code: &mut highlight_code,
	};
	render_markdown(markdown, &mut options)
}

fn render(markdown: &str) -> String {
	render_with(markdown, Styling::StyleSheet)
}

#[test]
fn renders_commonmark_basics() {
	assert_eq!(
		render("# Title\r\n\r\nSome *emphasis*, **strong** and `code`.\r\n\r\n---\r\n"),
		"<h1 id=\"title\">Title</h1>\n<p>Some <em>emphasis</em>, <strong>strong</strong> and <code>code</code>.</p>\n<hr>\n"
	);
	assert_eq!(render("a  \nb\nc"), "<p>a<br>\nb\nc</p>\n");
	assert_eq!(render("> quote"), "<blockquote>\n<p>quote</p>\n</blockquote>\n");
}

#[test]
fn renders_gfm_tables_with_alignment() {
	assert_eq!(
		render("| a | b |\n|:--|--:|\n| 1 | 2 |\n| 3 | 4 |"),
		"<table>\n<thead>\n<tr>\n<th align=\"left\">a</th>\n<th align=\"right\">b</th>\n</tr>\n</thead>\n<tbody>\n<tr>\n<td align=\"left\">1</td>\n<td align=\"right\">2</td>\n</tr>\n<tr>\n<td align=\"left\">3</td>\n<td align=\"right\">4</td>\n</tr>\n</tbody>\n</table>\n"
	);
}

#[test]
fn renders_task_lists_strikethrough_and_ordered_lists() {
	assert_eq!(
		render("- [x] done\n- [ ] ~~open~~\n\n3. three"),
		"<ul class=\"contains-task-list\">\n<li class=\"task-list-item\"><input type=\"checkbox\" class=\"task-list-item-checkbox\" disabled checked> done</li>\n<li class=\"task-list-item\"><input type=\"checkbox\" class=\"task-list-item-checkbox\" disabled> <del>open</del></li>\n</ul>\n<ol start=\"3\">\n<li>three</li>\n</ol>\n"
	);
	assert_eq!(
		render("- a\n  - [ ] b"),
		"<ul>\n<li>a\n<ul class=\"contains-task-list\">\n<li class=\"task-list-item\"><input type=\"checkbox\" class=\"task-list-item-checkbox\" disabled> b</li>\n</ul>\n</li>\n</ul>\n"
	);
}

#[test]
fn links_bare_urls_but_not_inside_links_or_code() {
	assert_eq!(
		render("See www.example.com, [https://a.b](https://a.b) and `https://c.d`."),
		"<p>See <a href=\"http://www.example.com\">www.example.com</a>, <a href=\"https://a.b\">https://a.b</a> and <code>https://c.d</code>.</p>\n"
	);
	assert_eq!(
		render("<mail@example.com>"),
		"<p><a href=\"mailto:mail@example.com\">mail@example.com</a></p>\n"
	);
}

#[test]
fn drops_unsafe_urls() {
	assert_eq!(render("[x](javascript:alert(1))"), "<p><a>x</a></p>\n");
}

#[test]
fn filters_disallowed_raw_html() {
	assert_eq!(
		render("<div>\n<script>alert(1)</script>\n</div>\n\nText <style>x</style>"),
		"<div>\n&lt;script>alert(1)&lt;/script>\n</div>\n<p>Text &lt;style>x&lt;/style></p>\n"
	);
}

#[test]
fn replaces_markdown_and_html_image_sources() {
	assert_eq!(
		render("![An *image*](a.png \"Title\") ![b](b.gif)\n\n<img src=\"c.png\">"),
		"<p><img src=\"data:image/png;Markdown\" alt=\"An image\" title=\"Title\"> <img src=\"b.gif\" alt=\"b\"></p>\n<img src=\"data:image/png;Html\">"
	);
}

#[test]
fn adds_unique_heading_anchors() {
	assert_eq!(
		render("## A `b` c\n## A b c"),
		"<h2 id=\"a-b-c\">A <code>b</code> c</h2>\n<h2 id=\"a-b-c-1\">A b c</h2>\n"
	);
}

#[test]
fn replaces_emoji_shortcodes_outside_code() {
	assert_eq!(
		render("Done :tada: `:tada:`"),
		"<p>Done <g-emoji>🎉</g-emoji> <code>:tada:</code></p>\n"
	);
	assert_eq!(
		render_with(":tada:", Styling::Inline),
		"<p style=\"margin-top:0;margin-bottom:16px\">🎉</p>\n"
	);
}

#[test]
fn renders_alerts_like_github() {
	let html = render("> [!WARNING]\n> Careful");
	assert!(
		html.starts_with(
			"<div class=\"markdown-alert markdown-alert-warning\">\n<p class=\"markdown-alert-title\"><img class=\"octicon mr-2\" src=\"data:image/svg+xml;base64,"
		),
		"{html}"
	);
	assert!(
		html.ends_with(
			" width=\"16\" height=\"16\" alt=\"\" aria-hidden=\"true\">Warning</p>\n<p>Careful</p>\n</div>\n"
		),
		"{html}"
	);
}

#[test]
fn colors_alert_icons_for_the_theme() {
	let icon_uri = |color: &str| {
		let svg = AlertKind::Note.icon_svg(color).into_bytes();
		EmbeddedImage { media_type: "image/svg+xml", bytes: svg }.data_uri()
	};
	let render_note = |styling: Styling, is_dark: bool| {
		let mut replace_image_source = |_: ImageSyntax, _: &str| None;
		let mut highlight_code = |_: &str, _: &str| Vec::new();
		render_markdown(
			"> [!NOTE]\n> n",
			&mut RenderOptions {
				styling,
				replace_image_source: &mut replace_image_source,
				highlight_code: &mut highlight_code,
				annotates_source_lines: false,
				is_dark,
			},
		)
	};
	assert!(render_note(Styling::StyleSheet, false).contains(&icon_uri("#0969da")));
	assert!(render_note(Styling::StyleSheet, true).contains(&icon_uri("#4493f8")));
	let inline = render_note(Styling::Inline, true);
	let inline_icon = format!(
		"<img style=\"vertical-align:text-bottom;margin-right:8px\" src=\"{}\"",
		icon_uri("#0969da")
	);
	assert!(inline.contains(&inline_icon), "{inline}");
}

#[test]
fn highlights_code_blocks() {
	assert_eq!(
		render("```rust ignore\nfn main() {}\n```\n\n    plain <b>\n"),
		"<pre><code><span class=\"pl-k\">fn</span> main() {}</code></pre>\n<pre><code>plain &lt;b&gt;</code></pre>\n"
	);
}

#[test]
fn renders_math_as_mathml() {
	let html = render("Inline $a^2$ and $`b_1`$.\n\n$$\nx\n$$\n\n```math\ny\n```");
	assert!(html.starts_with("<p>Inline <math"), "{html}");
	assert_eq!(html.matches("<math").count(), 4, "{html}");
	assert_eq!(html.matches("display=\"block\"").count(), 2, "{html}");
	assert!(!html.contains('$'), "{html}");
	assert!(html.contains("<msup><mi>a</mi><mn>2</mn></msup>"), "{html}");
}

#[test]
fn shows_invalid_math_without_failing() {
	let html = render("$\\frac{$ and $\\unknowncommand x$");
	assert!(html.starts_with("<p>"), "{html}");
}

#[test]
fn collects_footnotes_at_the_end() {
	assert_eq!(
		render("A[^b] B[^a] C[^B]\n\n[^a]: First.\n[^b]: Second.\n[^unused]: Never."),
		"<p>A<sup><a href=\"#fn-1\" id=\"fnref-1\" data-footnote-ref>1</a></sup> B<sup><a href=\"#fn-2\" id=\"fnref-2\" data-footnote-ref>2</a></sup> C<sup><a href=\"#fn-1\" id=\"fnref-1-2\" data-footnote-ref>1</a></sup></p>\n<section class=\"footnotes\" data-footnotes>\n<ol>\n<li id=\"fn-1\">\n<p>Second. <a href=\"#fnref-1\" class=\"data-footnote-backref\" aria-label=\"Back to reference 1\">↩</a> <a href=\"#fnref-1-2\" class=\"data-footnote-backref\" aria-label=\"Back to reference 1\">↩<sup>2</sup></a></p>\n</li>\n<li id=\"fn-2\">\n<p>First. <a href=\"#fnref-2\" class=\"data-footnote-backref\" aria-label=\"Back to reference 2\">↩</a></p>\n</li>\n</ol>\n</section>\n"
	);
}

#[test]
fn renders_front_matter_as_table_or_code() {
	assert_eq!(
		render("---\ntitle: \"Hello\"\nversion: 2\n---\nText"),
		"<table>\n<thead>\n<tr><th>title</th><th>version</th></tr>\n</thead>\n<tbody>\n<tr><td>Hello</td><td>2</td></tr>\n</tbody>\n</table>\n<p>Text</p>\n"
	);
	assert_eq!(render("---\nlist:\n  - a\n---\n"), "<pre><code>list:\n  - a</code></pre>\n");
}

#[test]
fn writes_inline_styles_for_office() {
	let html = render_with("# T\n\n- [x] a\n\n`c`", Styling::Inline);
	assert!(html.starts_with("<h1 style=\"margin-top:24px;"), "{html}");
	assert!(
		html.contains("<ul style=\"margin-top:0;margin-bottom:16px;padding-left:2em\">"),
		"{html}"
	);
	assert!(html.contains("<li style=\"list-style-type:none\">\u{2611} a</li>"), "{html}");
	assert!(html.contains("<code style=\"padding:0.2em 0.4em;"), "{html}");
	assert!(!html.contains("class="), "{html}");
}

/// LibreOffice Writer ignores the rest of a `style` attribute after a number without leading
/// zero, takes only the first name of a `font-family` list, and paints an inherited background
/// behind the text of code blocks.
#[test]
fn writes_inline_styles_libreoffice_understands() {
	let markdown = "# H1\n\n## H2\n\n#### H4\n\n###### H6\n\n> q\n\n`c`\n\n```sh\nb\n```\n\n---\n\n\
		> [!NOTE]\n> n\n\n|a|\n|-|\n|b|\n";
	let html = inline_styled_fragment(&render_with(markdown, Styling::Inline));
	let styles: Vec<&str> = html
		.split(" style=\"")
		.skip(1)
		.filter_map(|rest| rest.split_once('"').map(|(style, _)| style))
		.collect();
	assert!(styles.len() > 10, "{html}");
	assert!(!html.contains(" lang="), "{html}");
	assert!(!styles[0].contains("background"), "{}", styles[0]);
	for style in styles {
		let bytes = style.as_bytes();
		let number_without_leading_zero = (1..bytes.len().saturating_sub(1)).any(|index| {
			bytes[index] == b'.'
				&& matches!(bytes[index - 1], b':' | b' ')
				&& bytes[index + 1].is_ascii_digit()
		});
		assert!(!number_without_leading_zero, "{style}");
		if let Some((_, fonts)) = style.split_once("font-family:") {
			assert!(!fonts.starts_with("ui-") && !fonts.starts_with("-apple"), "{style}");
		}
	}
}

#[test]
fn wraps_pages_and_fragments() {
	let page = html_page("A & B", "<p>x</p>\n");
	assert!(page.contains("<title>A &amp; B</title>"));
	assert!(page.contains(".markdown-body{"));
	assert!(page.contains("<article class=\"markdown-body\">\n<p>x</p>\n</article>"));
	assert!(inline_styled_fragment("<p>x</p>\n").starts_with("<div style=\"font-family:"));
}

/// A panic would abort Notepad++, so random Markdown and LaTeX must render without one.
#[test]
fn renders_random_input_without_panicking() {
	const FRAGMENTS: [&str; 48] = [
		"# ",
		"> ",
		"> [!NOTE]\n",
		"- ",
		"1. ",
		"- [ ] ",
		"* ",
		"|",
		"|---|",
		":--",
		"\n",
		"\n\n",
		"  ",
		"\t",
		"`",
		"```",
		"```rust\n",
		"```math\n",
		"~~",
		"*",
		"_",
		"**",
		"[",
		"]",
		"(",
		")",
		"![",
		"<",
		">",
		"<img src=a.png>",
		"<script>",
		"&amp;",
		"\\",
		"$",
		"$$",
		"\\frac{",
		"{",
		"}",
		"^",
		"_{",
		"\\begin{matrix}",
		"&",
		"\\\\",
		"[^1]",
		"[^1]: ",
		"www.a.b",
		":smile:",
		"ä😀",
	];
	let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for _ in 0..iterations {
		let length = next_random() % 40;
		let markdown: String = (0..length)
			.map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize])
			.collect();
		render(&markdown);
		render_with(&markdown, Styling::Inline);
		render_with_lines(&markdown, Styling::StyleSheet, true);
		crate::markdown_rtf::tests::render(&markdown);
	}
}

#[test]
fn renders_random_math_without_panicking() {
	const FRAGMENTS: [&str; 60] = [
		"\\",
		"\\\\",
		"{",
		"}",
		"[",
		"]",
		"(",
		")",
		"^",
		"_",
		"&",
		"%",
		"\n",
		" ",
		"#1",
		"'",
		"`",
		"\"",
		"-",
		"+",
		".",
		",",
		"0",
		"12",
		"x",
		"ä",
		"α",
		"\u{a0}",
		"😀",
		"\\frac",
		"\\sqrt",
		"\\text",
		"\\mathrm",
		"\\mathbb",
		"\\kern",
		"\\hspace",
		"\\color",
		"\\operatorname",
		"\\left",
		"\\right",
		"\\middle",
		"\\begin{matrix}",
		"\\end{matrix}",
		"\\begin{array}{",
		"\\end{array}",
		"\\begin{",
		"\\end",
		"\\over",
		"\\not",
		"\\sum",
		"\\lim",
		"\\hat",
		"\\overbrace",
		"\\limits",
		"\\big",
		"\\displaystyle",
		"\\rm",
		"\\xrightarrow",
		"\\binom",
		"em",
	];
	let mut state: u64 = 0x2545_F491_4F6C_DD1D;
	let mut next_random = move || {
		state ^= state << 13;
		state ^= state >> 7;
		state ^= state << 17;
		state
	};
	let iterations = std::env::var("RANDOM_RENDER_ITERATIONS")
		.ok()
		.and_then(|value| value.parse().ok())
		.unwrap_or(2000);
	for _ in 0..iterations {
		let length = next_random() % 32;
		let latex: String = (0..length)
			.map(|_| FRAGMENTS[(next_random() % FRAGMENTS.len() as u64) as usize])
			.collect();
		let mut output = String::new();
		math::push_math(&mut output, &latex, next_random() % 2 == 0);
		assert!(output.ends_with("</math>"));
	}
}

#[test]
fn annotates_blocks_with_source_lines() {
	let html = render_with_lines(
		"# Title\n\npara\ngraph\n\n- a\n- b\n\n|x|\n|-|\n|1|\n\n```\ncode\n```\n",
		Styling::StyleSheet,
		true,
	);
	for expected in [
		"<h1 id=\"title\" data-line=\"0\">",
		"<p data-line=\"2\">para",
		"<ul data-line=\"5\">\n<li data-line=\"5\">a</li>\n<li data-line=\"6\">b",
		"<table data-line=\"8\">",
		"<tr data-line=\"10\">",
		"<pre data-line=\"12\">",
	] {
		assert!(html.contains(expected), "{expected} missing in {html}");
	}
	assert!(!render("para").contains("data-line"));
}
