//! The GitHub look: the embedded style sheet and the same styles as inline `style` attributes.
//!
//! Inline styles are for pasting into Office and e-mail applications, which ignore most of a
//! style sheet. They are hand-copied from `assets/github-markdown-light.css`. Colors with alpha
//! are blended onto the white background because Office does not support `#RRGGBBAA`.

/// `github-markdown-light.css` (minified at build time) and the page layout recommended by it.
pub const STYLE_SHEET: &str = concat!(
	include_str!(concat!(env!("OUT_DIR"), "/github-markdown-light.min.css")),
	"body{margin:0;background-color:#ffffff}",
	".markdown-body{box-sizing:border-box;min-width:200px;max-width:980px;margin:0 auto;padding:45px}",
	"@media (max-width:767px){.markdown-body{padding:15px}}",
);

/// `github-markdown-light.css` without the page layout, for the viewer.
pub const LIGHT_STYLE_SHEET: &str =
	include_str!(concat!(env!("OUT_DIR"), "/github-markdown-light.min.css"));

/// `github-markdown-dark.css`, for the viewer in Notepad++'s dark mode.
pub const DARK_STYLE_SHEET: &str =
	include_str!(concat!(env!("OUT_DIR"), "/github-markdown-dark.min.css"));

/// The kinds of code tokens with a GitHub color ("pretty lights" classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
	Comment,
	Keyword,
	Type,
	Function,
	String,
	Number,
	Tag,
	Attribute,
	Variable,
	DiffInserted,
	DiffDeleted,
	DiffHeader,
	DiffRange,
}

impl TokenKind {
	pub fn class_name(self) -> &'static str {
		match self {
			TokenKind::Comment => "pl-c",
			TokenKind::Keyword => "pl-k",
			TokenKind::Type | TokenKind::Function => "pl-en",
			TokenKind::String => "pl-s",
			TokenKind::Number => "pl-c1",
			TokenKind::Tag => "pl-ent",
			TokenKind::Attribute => "pl-e",
			TokenKind::Variable => "pl-v",
			TokenKind::DiffInserted => "pl-mi1",
			TokenKind::DiffDeleted => "pl-md",
			TokenKind::DiffHeader => "pl-mh",
			TokenKind::DiffRange => "pl-mdr",
		}
	}

	pub fn inline_style(self) -> &'static str {
		match self {
			TokenKind::Comment => "color:#59636e",
			TokenKind::Keyword => "color:#cf222e",
			TokenKind::Type | TokenKind::Function | TokenKind::Attribute => "color:#6639ba",
			TokenKind::String => "color:#0a3069",
			TokenKind::Number | TokenKind::Tag => "color:#0550ae",
			TokenKind::Variable => "color:#953800",
			TokenKind::DiffInserted => "color:#116329;background-color:#dafbe1",
			TokenKind::DiffDeleted => "color:#82071e;background-color:#ffebe9",
			TokenKind::DiffHeader => "font-weight:bold;color:#0550ae",
			TokenKind::DiffRange => "font-weight:bold;color:#8250df",
		}
	}
}

/// GitHub alerts (`> [!NOTE]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
	Note,
	Tip,
	Important,
	Warning,
	Caution,
}

impl AlertKind {
	pub fn title(self) -> &'static str {
		match self {
			AlertKind::Note => "Note",
			AlertKind::Tip => "Tip",
			AlertKind::Important => "Important",
			AlertKind::Warning => "Warning",
			AlertKind::Caution => "Caution",
		}
	}

	pub fn class_name(self) -> &'static str {
		match self {
			AlertKind::Note => "markdown-alert markdown-alert-note",
			AlertKind::Tip => "markdown-alert markdown-alert-tip",
			AlertKind::Important => "markdown-alert markdown-alert-important",
			AlertKind::Warning => "markdown-alert markdown-alert-warning",
			AlertKind::Caution => "markdown-alert markdown-alert-caution",
		}
	}

	pub fn border_color(self) -> &'static str {
		match self {
			AlertKind::Note => "#0969da",
			AlertKind::Tip => "#1a7f37",
			AlertKind::Important => "#8250df",
			AlertKind::Warning => "#9a6700",
			AlertKind::Caution => "#cf222e",
		}
	}

	pub fn title_color(self) -> &'static str {
		match self {
			AlertKind::Caution => "#d1242f",
			_ => self.border_color(),
		}
	}

	/// The title color of `github-markdown-dark.css`.
	pub fn dark_title_color(self) -> &'static str {
		match self {
			AlertKind::Note => "#4493f8",
			AlertKind::Tip => "#3fb950",
			AlertKind::Important => "#ab7df8",
			AlertKind::Warning => "#d29922",
			AlertKind::Caution => "#f85149",
		}
	}

	/// The title icon as a standalone SVG document filled with `color`. Images cannot take the
	/// text color, and Office applications and LibreOffice drop inline `<svg>` elements.
	pub fn icon_svg(self, color: &str) -> String {
		format!(
			"<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" width=\"16\" height=\"16\"><path fill=\"{color}\" d=\"{}\"/></svg>",
			self.icon_path()
		)
	}

	/// Path of the 16 px Octicon GitHub shows in the title (Primer Octicons, MIT License,
	/// Copyright (c) GitHub Inc.).
	pub fn icon_path(self) -> &'static str {
		match self {
			AlertKind::Note => {
				"M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"
			}
			AlertKind::Tip => {
				"M8 1.5c-2.363 0-4 1.69-4 3.75 0 .984.424 1.625.984 2.304l.214.253c.223.264.47.556.673.848.284.411.537.896.621 1.49a.75.75 0 0 1-1.484.211c-.04-.282-.163-.547-.37-.847a8.456 8.456 0 0 0-.542-.68c-.084-.1-.173-.205-.268-.32C3.201 7.75 2.5 6.766 2.5 5.25 2.5 2.31 4.863 0 8 0s5.5 2.31 5.5 5.25c0 1.516-.701 2.5-1.328 3.259-.095.115-.184.22-.268.319-.207.245-.383.453-.541.681-.208.3-.33.565-.37.847a.751.751 0 0 1-1.485-.212c.084-.593.337-1.078.621-1.489.203-.292.45-.584.673-.848.075-.088.147-.173.213-.253.561-.679.985-1.32.985-2.304 0-2.06-1.637-3.75-4-3.75ZM5.75 12h4.5a.75.75 0 0 1 0 1.5h-4.5a.75.75 0 0 1 0-1.5ZM6 15.25a.75.75 0 0 1 .75-.75h2.5a.75.75 0 0 1 0 1.5h-2.5a.75.75 0 0 1-.75-.75Z"
			}
			AlertKind::Important => {
				"M0 1.75C0 .784.784 0 1.75 0h12.5C15.216 0 16 .784 16 1.75v9.5A1.75 1.75 0 0 1 14.25 13H8.06l-2.573 2.573A1.458 1.458 0 0 1 3 14.543V13H1.75A1.75 1.75 0 0 1 0 11.25Zm1.75-.25a.25.25 0 0 0-.25.25v9.5c0 .138.112.25.25.25h2a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h6.5a.25.25 0 0 0 .25-.25v-9.5a.25.25 0 0 0-.25-.25Zm7 2.25v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 9a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"
			}
			AlertKind::Warning => {
				"M6.457 1.047c.659-1.234 2.427-1.234 3.086 0l6.082 11.378A1.75 1.75 0 0 1 14.082 15H1.918a1.75 1.75 0 0 1-1.543-2.575Zm1.763.707a.25.25 0 0 0-.44 0L1.698 13.132a.25.25 0 0 0 .22.368h12.164a.25.25 0 0 0 .22-.368Zm.53 3.996v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 11a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"
			}
			AlertKind::Caution => {
				"M4.47.22A.749.749 0 0 1 5 0h6c.199 0 .389.079.53.22l4.25 4.25c.141.14.22.331.22.53v6a.749.749 0 0 1-.22.53l-4.25 4.25A.749.749 0 0 1 11 16H5a.749.749 0 0 1-.53-.22L.22 11.53A.749.749 0 0 1 0 11V5c0-.199.079-.389.22-.53Zm.84 1.28L1.5 5.31v5.38l3.81 3.81h5.38l3.81-3.81V5.31L10.69 1.5ZM8 4a.75.75 0 0 1 .75.75v3.5a.75.75 0 0 1-1.5 0v-3.5A.75.75 0 0 1 8 4Zm0 8a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"
			}
		}
	}
}

/// Inline styles of the elements the renderer writes, following the style sheet.
///
/// LibreOffice Writer takes only the first name of a font list, so the fonts of Windows come
/// first, and it drops the rest of a style after a number without leading zero. It also turns
/// an inherited background into a character background, which would cover the background of
/// code blocks and table rows, so `BODY` has none.
pub mod inline {
	macro_rules! font {
		() => {
			"font-family:'Segoe UI',-apple-system,BlinkMacSystemFont,'Noto Sans',Helvetica,Arial,sans-serif,'Apple Color Emoji','Segoe UI Emoji'"
		};
	}
	macro_rules! monospace_font {
		() => {
			"font-family:Consolas,ui-monospace,SFMono-Regular,'SF Mono',Menlo,'Liberation Mono',monospace"
		};
	}
	macro_rules! heading {
		($specific:literal) => {
			concat!(
				"margin-top:24px;margin-bottom:16px;font-weight:600;line-height:1.25;",
				$specific
			)
		};
	}

	pub const BODY: &str =
		concat!(font!(), ";font-size:16px;line-height:1.5;color:#1f2328;word-wrap:break-word");
	pub const HEADINGS: [&str; 6] = [
		heading!("padding-bottom:0.3em;font-size:2em;border-bottom:1px solid #dee4e9"),
		heading!("padding-bottom:0.3em;font-size:1.5em;border-bottom:1px solid #dee4e9"),
		heading!("font-size:1.25em"),
		heading!("font-size:1em"),
		heading!("font-size:0.875em"),
		heading!("font-size:0.85em;color:#59636e"),
	];
	pub const PARAGRAPH: &str = "margin-top:0;margin-bottom:16px";
	pub const BLOCK_QUOTE: &str =
		"margin:0 0 16px 0;padding:0 1em;color:#59636e;border-left:0.25em solid #d1d9e0";
	pub const LIST: &str = "margin-top:0;margin-bottom:16px;padding-left:2em";
	pub const NESTED_LIST: &str = "margin-top:0;margin-bottom:0;padding-left:2em";
	pub const TASK_LIST_ITEM: &str = "list-style-type:none";
	pub const INLINE_CODE: &str = concat!(
		"padding:0.2em 0.4em;margin:0;font-size:85%;white-space:break-spaces;background-color:#eff1f3;border-radius:6px;",
		monospace_font!()
	);
	pub const CODE_BLOCK: &str = concat!(
		"padding:16px;overflow:auto;font-size:85%;line-height:1.45;color:#1f2328;background-color:#f6f8fa;border-radius:6px;margin-top:0;margin-bottom:16px;word-wrap:normal;",
		monospace_font!()
	);
	pub const CODE_BLOCK_CODE: &str =
		concat!("padding:0;margin:0;font-size:100%;white-space:pre;border:0;", monospace_font!());
	pub const TABLE: &str = "border-spacing:0;border-collapse:collapse;margin-top:0;margin-bottom:16px;font-variant:tabular-nums";
	pub const TABLE_ROW: &str = "background-color:#ffffff;border-top:1px solid #dee4e9";
	pub const EVEN_TABLE_ROW: &str = "background-color:#f6f8fa;border-top:1px solid #dee4e9";
	pub const TABLE_HEADER_CELL: &str = "padding:6px 13px;border:1px solid #d1d9e0;font-weight:600";
	pub const TABLE_CELL: &str = "padding:6px 13px;border:1px solid #d1d9e0";
	pub const RULE: &str = "box-sizing:content-box;overflow:hidden;height:0.25em;padding:0;margin:24px 0;background-color:#d1d9e0;border:0";
	pub const LINK: &str = "color:#0969da;text-decoration:none";
	pub const IMAGE: &str = "max-width:100%;box-sizing:content-box;border-style:none";
	pub const STRONG: &str = "font-weight:600";
	pub const FOOTNOTES: &str = "font-size:12px;color:#59636e;border-top:1px solid #d1d9e0";
	pub const FOOTNOTE_LIST: &str = "padding-left:16px";
	pub const ALERT: &str =
		"padding:8px 16px;margin-bottom:16px;color:inherit;border-left:0.25em solid ";
	pub const ALERT_TITLE: &str =
		"margin-top:0;margin-bottom:16px;font-weight:500;line-height:1;color:";
}
