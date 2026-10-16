//! `icon-preview`: renders all toolbar icons as Notepad++ would show them, for design reviews.
//!
//! One row per icon. For each theme: the icon at 16, 24 and 32 px on the toolbar background,
//! followed by the 16 px rendering magnified 4x to judge pixel alignment.

use std::path::Path;

use npp_build::{IconStyle, Theme, render_pixmap, svg_files_in, validate_icon};
use resvg::tiny_skia::{Color, FilterQuality, Pixmap, PixmapPaint, Rect, Transform};

const ACTUAL_SIZES: [u32; 3] = [16, 24, 32];
const MAGNIFIED_SIZE: u32 = 16;
const MAGNIFICATION: u32 = 4;
const CELL_SIZE: u32 = MAGNIFIED_SIZE * MAGNIFICATION + 16;
/// Toolbar backgrounds of Notepad++ in the light and the dark theme.
const LIGHT_BACKGROUND: (u8, u8, u8) = (0xF3, 0xF3, 0xF3);
const DARK_BACKGROUND: (u8, u8, u8) = (0x20, 0x20, 0x20);

pub fn write_preview(
	icon_directory: &Path,
	output: &Path,
	style: &IconStyle,
) -> Result<(), String> {
	let svg_files = svg_files_in(icon_directory).map_err(|error| error.to_string())?;
	if svg_files.is_empty() {
		return Err(format!("no SVG files in {}", icon_directory.display()));
	}
	let columns_per_theme = ACTUAL_SIZES.len() as u32 + 1;
	let mut sheet = Pixmap::new(
		CELL_SIZE * columns_per_theme * Theme::ALL.len() as u32,
		CELL_SIZE * svg_files.len() as u32,
	)
	.ok_or("preview too large")?;

	for (row, svg_file) in svg_files.iter().enumerate() {
		let source = std::fs::read_to_string(svg_file).map_err(|error| error.to_string())?;
		if let Err(violations) = validate_icon(&source, style) {
			for violation in violations {
				eprintln!("{}: {violation}", svg_file.display());
			}
		}
		let top = row as u32 * CELL_SIZE;
		for (theme_index, theme) in Theme::ALL.into_iter().enumerate() {
			let left = theme_index as u32 * columns_per_theme * CELL_SIZE;
			let (red, green, blue) = match theme {
				Theme::Light => LIGHT_BACKGROUND,
				Theme::Dark => DARK_BACKGROUND,
			};
			let background = Rect::from_xywh(
				left as f32,
				top as f32,
				(columns_per_theme * CELL_SIZE) as f32,
				CELL_SIZE as f32,
			)
			.ok_or("invalid cell")?;
			sheet.fill_rect(
				background,
				&solid_paint(Color::from_rgba8(red, green, blue, 255)),
				Transform::identity(),
				None,
			);
			for (column, size) in ACTUAL_SIZES.into_iter().enumerate() {
				let icon = render_pixmap(&source, style, theme, size)
					.map_err(|error| error.to_string())?;
				let x = left + column as u32 * CELL_SIZE + (CELL_SIZE - size) / 2;
				let y = top + (CELL_SIZE - size) / 2;
				draw(&mut sheet, &icon, x, y, 1);
			}
			let icon = render_pixmap(&source, style, theme, MAGNIFIED_SIZE)
				.map_err(|error| error.to_string())?;
			let x = left + ACTUAL_SIZES.len() as u32 * CELL_SIZE + 8;
			draw(&mut sheet, &icon, x, top + 8, MAGNIFICATION);
		}
	}
	sheet.save_png(output).map_err(|error| error.to_string())?;
	println!("Wrote {} ({} icons, style \"{}\").", output.display(), svg_files.len(), style.name);
	Ok(())
}

fn draw(sheet: &mut Pixmap, icon: &Pixmap, x: u32, y: u32, magnification: u32) {
	let paint = PixmapPaint { quality: FilterQuality::Nearest, ..PixmapPaint::default() };
	let scale = magnification as f32;
	let transform = Transform::from_row(scale, 0.0, 0.0, scale, x as f32, y as f32);
	sheet.draw_pixmap(0, 0, icon.as_ref(), &paint, transform, None);
}

fn solid_paint(color: Color) -> resvg::tiny_skia::Paint<'static> {
	let mut paint = resvg::tiny_skia::Paint::default();
	paint.set_color(color);
	paint
}
