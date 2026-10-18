//! Toolbar icon sources as produced by `npp-build`.

/// A toolbar icon as SVG, recolored for the light and the dark theme. It is rendered at the size
/// Notepad++ asks for (see [`crate::svg`]).
#[derive(Debug)]
pub struct ToolbarIcon {
	pub light: &'static str,
	pub dark: &'static str,
}
