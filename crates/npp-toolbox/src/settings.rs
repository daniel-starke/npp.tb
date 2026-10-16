//! The persistent settings in `<plugins config dir>/NppToolBox.ini`.

use std::ops::RangeInclusive;

pub const SETTINGS_FILE_NAME: &str = "NppToolBox.ini";

pub struct Setting {
	pub section: &'static str,
	pub key: &'static str,
	pub default: u32,
	pub allowed: RangeInclusive<u32>,
	/// Shown above the key in the settings file.
	pub description: &'static str,
}

impl Setting {
	/// The default for missing or invalid values.
	pub fn valid_or_default(&self, value: u32) -> u32 {
		if self.allowed.contains(&value) { value } else { self.default }
	}
}

pub const MAX_LINE_WIDTH: Setting = Setting {
	section: "Rewrap",
	key: "MaxLineWidth",
	default: 80,
	allowed: 8..=1000,
	description: "Maximum number of characters per line (8 to 1000). The Rewrap dialog updates it.",
};

pub const EMBED_MARKDOWN_IMAGES: Setting = Setting {
	section: "Html",
	key: "EmbedMarkdownImages",
	default: 1,
	allowed: 0..=1,
	description: "1: embed local images of ![alt](path) as Base64 data URIs, 0: keep the path.",
};

pub const EMBED_HTML_IMAGES: Setting = Setting {
	section: "Html",
	key: "EmbedHtmlImages",
	default: 1,
	allowed: 0..=1,
	description: "1: embed local images of <img src=\"path\"> as Base64 data URIs, 0: keep the path.",
};

pub const MAX_EMBEDDED_IMAGE_BYTES: Setting = Setting {
	section: "Html",
	key: "MaxEmbeddedImageBytes",
	default: 10 * 1024 * 1024,
	allowed: 0..=u32::MAX,
	description: "Larger images stay linked. Internet images are never embedded.",
};

pub const HIGHLIGHT_CODE: Setting = Setting {
	section: "Html",
	key: "HighlightCode",
	default: 1,
	allowed: 0..=1,
	description: "1: color code blocks with the Notepad++ lexer of their language, 0: plain code.",
};

/// All settings in the order of the settings file.
pub const ALL: [&Setting; 5] = [
	&MAX_LINE_WIDTH,
	&EMBED_MARKDOWN_IMAGES,
	&EMBED_HTML_IMAGES,
	&MAX_EMBEDDED_IMAGE_BYTES,
	&HIGHLIGHT_CODE,
];

/// A new settings file with every setting at its default value and described.
pub fn settings_file_template(line_ending: &str) -> String {
	let mut lines = vec![
		"; NppToolBox settings. Changes apply the next time a command runs.".to_owned(),
		"; Missing or invalid values fall back to the defaults.".to_owned(),
	];
	let mut section = "";
	for setting in ALL {
		if setting.section != section {
			section = setting.section;
			lines.push(String::new());
			lines.push(format!("[{section}]"));
		}
		lines.push(format!("; {}", setting.description));
		lines.push(format!("{}={}", setting.key, setting.default));
	}
	lines.push(String::new());
	lines.join(line_ending)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn template_lists_every_setting_by_section() {
		let template = settings_file_template("\n");
		assert!(template.starts_with("; NppToolBox settings."));
		assert!(template.contains("\n[Rewrap]\n; Maximum number"));
		assert!(template.contains("\nMaxLineWidth=80\n\n[Html]\n"));
		assert!(template.contains("\nMaxEmbeddedImageBytes=10485760\n"));
		assert_eq!(template.matches("\n[").count(), 2);
		for setting in ALL {
			assert!(setting.allowed.contains(&setting.default), "{}", setting.key);
			assert!(template.contains(&format!("\n{}={}\n", setting.key, setting.default)));
		}
	}

	#[test]
	fn falls_back_to_defaults() {
		assert_eq!(MAX_LINE_WIDTH.valid_or_default(5), 80);
		assert_eq!(MAX_LINE_WIDTH.valid_or_default(120), 120);
		assert_eq!(EMBED_HTML_IMAGES.valid_or_default(2), 1);
	}
}
