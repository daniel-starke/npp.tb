//! Creates the GDI handles Notepad++ expects for a toolbar button.

use core::ffi::c_void;

use windows_sys::Win32::Graphics::Gdi::{
	BI_RGB, BITMAPINFO, BITMAPINFOHEADER, COLOR_BTNFACE, CreateBitmap, CreateDIBSection,
	DIB_RGB_COLORS, DeleteObject, GetSysColor, HBITMAP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
	CreateIconIndirect, DestroyIcon, HICON, ICONINFO,
};

use crate::ffi::ToolbarIconsWithDarkMode;
use crate::icon::ToolbarIcon;
use crate::svg::render_bgra;

/// A square image in 32-bit BGRA order with straight alpha, rows top-down.
struct IconImage {
	size: u32,
	bgra: Vec<u8>,
}

impl IconImage {
	fn render(svg: &str, size: u32) -> Option<Self> {
		Some(Self { size, bgra: render_bgra(svg, size)? })
	}
}

/// `bitmap_size` is used for the "Standard icons" toolbar set, `icon_size` for the Fluent sets.
/// Returns `None` if any handle could not be created. Nothing is leaked in that case.
pub(crate) fn create_toolbar_icon_handles(
	icon: &ToolbarIcon,
	bitmap_size: u32,
	icon_size: u32,
) -> Option<ToolbarIconsWithDarkMode> {
	let light_bitmap_image = IconImage::render(icon.light, bitmap_size)?;
	let light_icon_image = IconImage::render(icon.light, icon_size)?;
	let dark_icon_image = IconImage::render(icon.dark, icon_size)?;

	let toolbar_bitmap = create_opaque_bitmap(&light_bitmap_image)?;
	let Some(toolbar_icon) = create_icon(&light_icon_image) else {
		delete_bitmap(toolbar_bitmap);
		return None;
	};
	let Some(toolbar_icon_dark_mode) = create_icon(&dark_icon_image) else {
		delete_bitmap(toolbar_bitmap);
		// SAFETY: The icon was created above and never handed out.
		unsafe { DestroyIcon(toolbar_icon) };
		return None;
	};
	Some(ToolbarIconsWithDarkMode { toolbar_bitmap, toolbar_icon, toolbar_icon_dark_mode })
}

/// The "Standard icons" toolbar draws plugin bitmaps without honoring alpha. They are therefore
/// pre-composited onto the button face color, which is what `LR_LOADMAP3DCOLORS` would achieve
/// for bitmaps loaded from resources.
fn create_opaque_bitmap(image: &IconImage) -> Option<HBITMAP> {
	// SAFETY: `GetSysColor` has no preconditions.
	let background = unsafe { GetSysColor(COLOR_BTNFACE) };
	let background_bgr = [(background >> 16) as u8, (background >> 8) as u8, background as u8];
	let pixels: Vec<u8> = image
		.bgra
		.as_chunks::<4>()
		.0
		.iter()
		.flat_map(|pixel| {
			let alpha = u32::from(pixel[3]);
			let blend = |foreground: u8, background: u8| {
				((u32::from(foreground) * alpha + u32::from(background) * (255 - alpha) + 127)
					/ 255) as u8
			};
			[
				blend(pixel[0], background_bgr[0]),
				blend(pixel[1], background_bgr[1]),
				blend(pixel[2], background_bgr[2]),
				0xFF,
			]
		})
		.collect();
	create_dib_section(image.size, &pixels)
}

fn create_icon(image: &IconImage) -> Option<HICON> {
	let color = create_dib_section(image.size, &image.bgra)?;
	// With a 32-bit color bitmap the alpha channel defines transparency and the AND mask must be
	// all zeros. Monochrome rows are padded to 16 bits.
	let mask_bytes = vec![0u8; image.size.div_ceil(16) as usize * 2 * image.size as usize];
	// SAFETY: `mask_bytes` holds a full monochrome bitmap of the requested size.
	let mask = unsafe {
		CreateBitmap(image.size as i32, image.size as i32, 1, 1, mask_bytes.as_ptr().cast())
	};
	if mask.is_null() {
		delete_bitmap(color);
		return None;
	}
	let icon_info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
	// SAFETY: Both bitmaps are valid. `CreateIconIndirect` copies them, so they are freed below.
	let icon = unsafe { CreateIconIndirect(&icon_info) };
	delete_bitmap(mask);
	delete_bitmap(color);
	(!icon.is_null()).then_some(icon)
}

fn create_dib_section(size: u32, bgra: &[u8]) -> Option<HBITMAP> {
	let expected_length = size as usize * size as usize * 4;
	if bgra.len() != expected_length {
		return None;
	}
	let info = BITMAPINFO {
		bmiHeader: BITMAPINFOHEADER {
			biSize: size_of::<BITMAPINFOHEADER>() as u32,
			biWidth: size as i32,
			// Negative height selects a top-down bitmap, matching `IconImage`.
			biHeight: -(size as i32),
			biPlanes: 1,
			biBitCount: 32,
			biCompression: BI_RGB,
			..Default::default()
		},
		..Default::default()
	};
	let mut bits: *mut c_void = core::ptr::null_mut();
	// SAFETY: `info` describes a 32-bit DIB. On success `bits` points to `expected_length` bytes.
	unsafe {
		let bitmap = CreateDIBSection(
			core::ptr::null_mut(),
			&info,
			DIB_RGB_COLORS,
			&mut bits,
			core::ptr::null_mut(),
			0,
		);
		if bitmap.is_null() || bits.is_null() {
			return None;
		}
		core::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), expected_length);
		Some(bitmap)
	}
}

fn delete_bitmap(bitmap: HBITMAP) {
	// SAFETY: Only called with bitmaps created and owned by this module.
	unsafe { DeleteObject(bitmap) };
}
