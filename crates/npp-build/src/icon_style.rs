//! The common look of all toolbar icons: canvas, palette and stroke widths.

use crate::Theme;

/// A color with one value per Notepad++ theme.
#[derive(Clone, Copy, Debug)]
pub struct ThemedColor {
	/// What the color is used for, e.g. "accent". Documentation only.
	pub role: &'static str,
	/// `#RRGGBB`. Also the value written in the SVG masters (except for the foreground).
	pub light: &'static str,
	pub dark: &'static str,
}

impl ThemedColor {
	pub fn for_theme(&self, theme: Theme) -> &'static str {
		match theme {
			Theme::Light => self.light,
			Theme::Dark => self.dark,
		}
	}
}

#[derive(Clone, Copy, Debug)]
pub struct IconStyle {
	pub name: &'static str,
	/// Master icons use `viewBox="0 0 canvas_size canvas_size"`.
	pub canvas_size: u32,
	/// Written as `currentColor` in the masters.
	pub foreground: ThemedColor,
	/// Written as their light theme value in the masters. Replaced for the dark theme.
	pub accents: &'static [ThemedColor],
	/// Allowed effective stroke widths in canvas units.
	pub stroke_widths: &'static [f32],
	/// `false` for outline styles: the foreground may only be stroked, fills are for accents.
	pub allows_foreground_fills: bool,
}

impl IconStyle {
	pub fn colors_for_theme(&self, theme: Theme) -> impl Iterator<Item = &'static str> + '_ {
		std::iter::once(self.foreground.for_theme(theme))
			.chain(self.accents.iter().map(move |accent| accent.for_theme(theme)))
	}
}

/// Outline icons in the colors of Notepad++'s own Fluent UI toolbar icons (measured from
/// Notepad++ 8.9.8): 1 px foreground outlines on half-pixel coordinates, one accent element.
pub const NOTEPAD_PLUS_PLUS_FLUENT_OUTLINE: IconStyle = IconStyle {
	name: "Notepad++ Fluent outline",
	canvas_size: 16,
	foreground: ThemedColor { role: "foreground", light: "#212121", dark: "#DEDEDE" },
	accents: &[ThemedColor { role: "accent", light: "#0078D4", dark: "#4CC2FF" }],
	stroke_widths: &[1.0],
	allows_foreground_fills: false,
};

pub(crate) fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
	let digits = hex.strip_prefix('#').filter(|digits| digits.len() == 6)?;
	let channel = |range| u8::from_str_radix(&digits[range], 16).ok();
	Some((channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

/// The common style all toolbar icons of plugins built on this framework follow.
pub const DEFAULT: IconStyle = NOTEPAD_PLUS_PLUS_FLUENT_OUTLINE;
