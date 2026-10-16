//! In-memory `DLGTEMPLATE` builder, so dialogs need no resource compiler.
//!
//! Layout contract (see "DLGTEMPLATE structure" in the Win32 docs): a header, menu, class, title
//! and, with `DS_SETFONT`, font size and name, then one DWORD aligned `DLGITEMTEMPLATE` per
//! control followed by its class, title and creation data size. Coordinates are dialog units.

use windows_sys::Win32::UI::WindowsAndMessaging::DS_SETFONT;

/// Predefined window class atoms usable instead of a class name.
#[derive(Clone, Copy)]
#[repr(u16)]
pub(crate) enum ControlClass {
	Button = 0x0080,
	Edit = 0x0081,
	Static = 0x0082,
}

#[derive(Clone, Copy)]
pub(crate) struct DialogRect {
	pub x: i16,
	pub y: i16,
	pub width: i16,
	pub height: i16,
}

pub(crate) struct DialogTemplate {
	words: Vec<u16>,
	control_count: u16,
}

/// Word index of `DLGTEMPLATE::cdit`.
const CONTROL_COUNT_WORD_INDEX: usize = 4;

impl DialogTemplate {
	pub fn new(
		style: u32,
		title: &str,
		rect: DialogRect,
		font_name: &str,
		font_point_size: u16,
	) -> Self {
		let mut template = Self { words: Vec::new(), control_count: 0 };
		template.push_u32(style | DS_SETFONT as u32);
		template.push_u32(0); // extended style
		template.words.push(0); // control count, patched in `add_control`
		template.push_rect(rect);
		template.words.push(0); // no menu
		template.words.push(0); // default dialog class
		template.push_string(title);
		template.words.push(font_point_size);
		template.push_string(font_name);
		template
	}

	pub fn add_control(
		&mut self,
		class: ControlClass,
		id: u16,
		style: u32,
		rect: DialogRect,
		text: &str,
	) {
		self.align_to_dword();
		self.push_u32(style);
		self.push_u32(0); // extended style
		self.push_rect(rect);
		self.words.push(id);
		self.words.extend([0xFFFF, class as u16]);
		self.push_string(text);
		self.words.push(0); // no creation data
		self.control_count += 1;
		self.words[CONTROL_COUNT_WORD_INDEX] = self.control_count;
	}

	/// The template must start on a DWORD boundary, hence the `u32` backing store.
	pub fn into_dword_buffer(mut self) -> Vec<u32> {
		self.align_to_dword();
		self.words
			.chunks_exact(2)
			.map(|pair| u32::from(pair[0]) | (u32::from(pair[1]) << 16))
			.collect()
	}

	fn align_to_dword(&mut self) {
		if self.words.len() % 2 != 0 {
			self.words.push(0);
		}
	}

	fn push_u32(&mut self, value: u32) {
		self.words.extend([value as u16, (value >> 16) as u16]);
	}

	fn push_rect(&mut self, rect: DialogRect) {
		self.words.extend([rect.x, rect.y, rect.width, rect.height].map(|value| value as u16));
	}

	fn push_string(&mut self, text: &str) {
		self.words.extend(text.encode_utf16().chain(Some(0)));
	}
}
