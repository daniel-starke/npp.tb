//! The subset of DirectWrite (`dwrite.h`) and the Direct2D geometry sink (`d2d1.h`) used by
//! [`super::TextEngine`]. `windows-sys` has no COM interfaces.
//!
//! Interfaces that are only called declare their methods up to the last one used. Methods that are
//! never called keep their vtable slot with a placeholder signature. The method names are those of
//! the SDK header; methods of `IDWriteTextLayout` that repeat a name of `IDWriteTextFormat` get the
//! suffix `Range`, after their text range parameter.

// The signatures are those of the SDK, including their argument counts.
#![allow(
	non_snake_case,
	non_camel_case_types,
	clippy::upper_case_acronyms,
	clippy::too_many_arguments
)]

use core::ffi::c_void;

use windows_core::{GUID, HRESULT, IUnknown, interface};

pub type BOOL = i32;

/// `DWRITE_FACTORY_TYPE_SHARED`
pub const FACTORY_TYPE_SHARED: i32 = 0;
/// `DWRITE_FONT_STRETCH_NORMAL`
pub const FONT_STRETCH_NORMAL: i32 = 5;
/// `DWRITE_FONT_STYLE_NORMAL`, `DWRITE_FONT_STYLE_ITALIC`
pub const FONT_STYLE_NORMAL: i32 = 0;
pub const FONT_STYLE_ITALIC: i32 = 2;
/// `DWRITE_WORD_WRAPPING_NO_WRAP`
pub const WORD_WRAPPING_NO_WRAP: i32 = 1;
/// `D2D1_FIGURE_END_CLOSED`
pub const FIGURE_END_CLOSED: i32 = 1;

pub const IID_IDWRITE_FACTORY: GUID = GUID::from_u128(0xb859ee5a_d838_4b5b_a2e8_1adc7d93db48);

#[link(name = "dwrite")]
unsafe extern "system" {
	pub fn DWriteCreateFactory(
		factory_type: i32,
		iid: *const GUID,
		factory: *mut *mut c_void,
	) -> HRESULT;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DWRITE_FONT_METRICS {
	pub designUnitsPerEm: u16,
	pub ascent: u16,
	pub descent: u16,
	pub lineGap: i16,
	pub capHeight: u16,
	pub xHeight: u16,
	pub underlinePosition: i16,
	pub underlineThickness: u16,
	pub strikethroughPosition: i16,
	pub strikethroughThickness: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DWRITE_TEXT_METRICS {
	pub left: f32,
	pub top: f32,
	pub width: f32,
	pub widthIncludingTrailingWhitespace: f32,
	pub height: f32,
	pub layoutWidth: f32,
	pub layoutHeight: f32,
	pub maxBidiReorderingDepth: u32,
	pub lineCount: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DWRITE_GLYPH_METRICS {
	pub leftSideBearing: i32,
	pub advanceWidth: u32,
	pub rightSideBearing: i32,
	pub topSideBearing: i32,
	pub advanceHeight: u32,
	pub bottomSideBearing: i32,
	pub verticalOriginY: i32,
}

#[repr(C)]
pub struct DWRITE_GLYPH_OFFSET {
	pub advanceOffset: f32,
	pub ascenderOffset: f32,
}

#[repr(C)]
pub struct DWRITE_GLYPH_RUN {
	/// `IDWriteFontFace`, borrowed.
	pub fontFace: *mut c_void,
	pub fontEmSize: f32,
	pub glyphCount: u32,
	pub glyphIndices: *const u16,
	pub glyphAdvances: *const f32,
	pub glyphOffsets: *const DWRITE_GLYPH_OFFSET,
	pub isSideways: BOOL,
	pub bidiLevel: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DWRITE_MATRIX {
	pub m11: f32,
	pub m12: f32,
	pub m21: f32,
	pub m22: f32,
	pub dx: f32,
	pub dy: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct D2D1_POINT_2F {
	pub x: f32,
	pub y: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct D2D1_BEZIER_SEGMENT {
	pub point1: D2D1_POINT_2F,
	pub point2: D2D1_POINT_2F,
	pub point3: D2D1_POINT_2F,
}

#[interface("b859ee5a-d838-4b5b-a2e8-1adc7d93db48")]
pub unsafe trait IDWriteFactory: IUnknown {
	/// `collection`: receives an `IDWriteFontCollection`.
	pub fn GetSystemFontCollection(
		&self,
		collection: *mut *mut c_void,
		check_for_updates: BOOL,
	) -> HRESULT;
	pub fn CreateCustomFontCollection(&self) -> HRESULT;
	pub fn RegisterFontCollectionLoader(&self) -> HRESULT;
	pub fn UnregisterFontCollectionLoader(&self) -> HRESULT;
	pub fn CreateFontFileReference(&self) -> HRESULT;
	pub fn CreateCustomFontFileReference(&self) -> HRESULT;
	pub fn CreateFontFace(&self) -> HRESULT;
	pub fn CreateRenderingParams(&self) -> HRESULT;
	pub fn CreateMonitorRenderingParams(&self) -> HRESULT;
	pub fn CreateCustomRenderingParams(&self) -> HRESULT;
	pub fn RegisterFontFileLoader(&self) -> HRESULT;
	pub fn UnregisterFontFileLoader(&self) -> HRESULT;
	/// `collection`: `IDWriteFontCollection` or null; `format`: receives an `IDWriteTextFormat`.
	pub fn CreateTextFormat(
		&self,
		family: *const u16,
		collection: *mut c_void,
		weight: i32,
		style: i32,
		stretch: i32,
		size: f32,
		locale: *const u16,
		format: *mut *mut c_void,
	) -> HRESULT;
	pub fn CreateTypography(&self) -> HRESULT;
	pub fn GetGdiInterop(&self) -> HRESULT;
	/// `format`: `IDWriteTextFormat`; `layout`: receives an `IDWriteTextLayout`.
	pub fn CreateTextLayout(
		&self,
		text: *const u16,
		length: u32,
		format: *mut c_void,
		max_width: f32,
		max_height: f32,
		layout: *mut *mut c_void,
	) -> HRESULT;
	/// `format`: `IDWriteTextFormat`; `transform`: may be null; `layout`: receives an
	/// `IDWriteTextLayout`.
	pub fn CreateGdiCompatibleTextLayout(
		&self,
		text: *const u16,
		length: u32,
		format: *mut c_void,
		max_width: f32,
		max_height: f32,
		pixels_per_dip: f32,
		transform: *const DWRITE_MATRIX,
		use_gdi_natural: BOOL,
		layout: *mut *mut c_void,
	) -> HRESULT;
	pub fn CreateEllipsisTrimmingSign(&self) -> HRESULT;
	pub fn CreateTextAnalyzer(&self) -> HRESULT;
	pub fn CreateNumberSubstitution(&self) -> HRESULT;
	/// `analysis`: receives an `IDWriteGlyphRunAnalysis`.
	pub fn CreateGlyphRunAnalysis(
		&self,
		glyph_run: *const DWRITE_GLYPH_RUN,
		pixels_per_dip: f32,
		transform: *const DWRITE_MATRIX,
		rendering_mode: i32,
		measuring_mode: i32,
		baseline_x: f32,
		baseline_y: f32,
		analysis: *mut *mut c_void,
	) -> HRESULT;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RECT {
	pub left: i32,
	pub top: i32,
	pub right: i32,
	pub bottom: i32,
}

#[interface("7d97dbf7-e085-42d4-81e3-6a883bded118")]
pub unsafe trait IDWriteGlyphRunAnalysis: IUnknown {
	pub fn GetAlphaTextureBounds(&self, texture_type: i32, bounds: *mut RECT) -> HRESULT;
}

#[interface("a84cee02-3eea-4eee-a827-87c1a02a0fcc")]
pub unsafe trait IDWriteFontCollection: IUnknown {
	pub fn GetFontFamilyCount(&self) -> u32;
	/// `family`: receives an `IDWriteFontFamily`.
	pub fn GetFontFamily(&self, index: u32, family: *mut *mut c_void) -> HRESULT;
	pub fn FindFamilyName(&self, name: *const u16, index: *mut u32, exists: *mut BOOL) -> HRESULT;
}

#[interface("1a0d8438-1d97-4ec1-aef9-a2fb86ed6acb")]
pub unsafe trait IDWriteFontList: IUnknown {
	pub fn GetFontCollection(&self) -> HRESULT;
	pub fn GetFontCount(&self) -> u32;
	pub fn GetFont(&self) -> HRESULT;
}

#[interface("da20d8ef-812a-4c43-9802-62ec4abd7add")]
pub unsafe trait IDWriteFontFamily: IDWriteFontList {
	pub fn GetFamilyNames(&self) -> HRESULT;
	/// `font`: receives an `IDWriteFont`.
	pub fn GetFirstMatchingFont(
		&self,
		weight: i32,
		stretch: i32,
		style: i32,
		font: *mut *mut c_void,
	) -> HRESULT;
}

#[interface("acd16696-8c14-4f5d-877e-fe3fc1d32737")]
pub unsafe trait IDWriteFont: IUnknown {
	pub fn GetFontFamily(&self) -> HRESULT;
	pub fn GetWeight(&self) -> i32;
	pub fn GetStretch(&self) -> i32;
	pub fn GetStyle(&self) -> i32;
	pub fn IsSymbolFont(&self) -> BOOL;
	pub fn GetFaceNames(&self) -> HRESULT;
	pub fn GetInformationalStrings(&self) -> HRESULT;
	pub fn GetSimulations(&self) -> i32;
	pub fn GetMetrics(&self, metrics: *mut DWRITE_FONT_METRICS);
	pub fn HasCharacter(&self) -> HRESULT;
	/// `face`: receives an `IDWriteFontFace`.
	pub fn CreateFontFace(&self, face: *mut *mut c_void) -> HRESULT;
}

#[interface("5f49804d-7024-4d43-bfa9-d25984f53849")]
pub unsafe trait IDWriteFontFace: IUnknown {
	pub fn GetType(&self) -> i32;
	pub fn GetFiles(&self) -> HRESULT;
	pub fn GetIndex(&self) -> u32;
	pub fn GetSimulations(&self) -> i32;
	pub fn IsSymbolFont(&self) -> BOOL;
	pub fn GetMetrics(&self, metrics: *mut DWRITE_FONT_METRICS);
	pub fn GetGlyphCount(&self) -> u16;
	pub fn GetDesignGlyphMetrics(
		&self,
		glyph_indices: *const u16,
		glyph_count: u32,
		metrics: *mut DWRITE_GLYPH_METRICS,
		is_sideways: BOOL,
	) -> HRESULT;
	pub fn GetGlyphIndices(
		&self,
		code_points: *const u32,
		count: u32,
		glyph_indices: *mut u16,
	) -> HRESULT;
	pub fn TryGetFontTable(
		&self,
		tag: u32,
		data: *mut *const c_void,
		size: *mut u32,
		context: *mut *mut c_void,
		exists: *mut BOOL,
	) -> HRESULT;
	pub fn ReleaseFontTable(&self, context: *mut c_void);
	/// `sink`: an `ID2D1SimplifiedGeometrySink`.
	pub fn GetGlyphRunOutline(
		&self,
		em_size: f32,
		glyph_indices: *const u16,
		glyph_advances: *const f32,
		glyph_offsets: *const DWRITE_GLYPH_OFFSET,
		glyph_count: u32,
		is_sideways: BOOL,
		is_right_to_left: BOOL,
		sink: *mut c_void,
	) -> HRESULT;
	pub fn GetRecommendedRenderingMode(&self) -> HRESULT;
	/// `transform`: may be null.
	pub fn GetGdiCompatibleMetrics(
		&self,
		em_size: f32,
		pixels_per_dip: f32,
		transform: *const DWRITE_MATRIX,
		metrics: *mut DWRITE_FONT_METRICS,
	) -> HRESULT;
	/// `transform`: may be null.
	pub fn GetGdiCompatibleGlyphMetrics(
		&self,
		em_size: f32,
		pixels_per_dip: f32,
		transform: *const DWRITE_MATRIX,
		use_gdi_natural: BOOL,
		glyph_indices: *const u16,
		glyph_count: u32,
		metrics: *mut DWRITE_GLYPH_METRICS,
		is_sideways: BOOL,
	) -> HRESULT;
}

#[interface("9c906818-31d7-4fd3-a151-7c5e225db55a")]
pub unsafe trait IDWriteTextFormat: IUnknown {
	pub fn SetTextAlignment(&self) -> HRESULT;
	pub fn SetParagraphAlignment(&self) -> HRESULT;
	pub fn SetWordWrapping(&self, wrapping: i32) -> HRESULT;
	pub fn SetReadingDirection(&self) -> HRESULT;
	pub fn SetFlowDirection(&self) -> HRESULT;
	pub fn SetIncrementalTabStop(&self) -> HRESULT;
	pub fn SetTrimming(&self) -> HRESULT;
	pub fn SetLineSpacing(&self) -> HRESULT;
	pub fn GetTextAlignment(&self) -> i32;
	pub fn GetParagraphAlignment(&self) -> i32;
	pub fn GetWordWrapping(&self) -> i32;
	pub fn GetReadingDirection(&self) -> i32;
	pub fn GetFlowDirection(&self) -> i32;
	pub fn GetIncrementalTabStop(&self) -> f32;
	pub fn GetTrimming(&self) -> HRESULT;
	pub fn GetLineSpacing(&self) -> HRESULT;
	pub fn GetFontCollection(&self) -> HRESULT;
	pub fn GetFontFamilyNameLength(&self) -> u32;
	pub fn GetFontFamilyName(&self) -> HRESULT;
	pub fn GetFontWeight(&self) -> i32;
	pub fn GetFontStyle(&self) -> i32;
	pub fn GetFontStretch(&self) -> i32;
	pub fn GetFontSize(&self) -> f32;
	pub fn GetLocaleNameLength(&self) -> u32;
	pub fn GetLocaleName(&self) -> HRESULT;
}

#[interface("53737037-6d14-410b-9bfe-0b182bb70961")]
pub unsafe trait IDWriteTextLayout: IDWriteTextFormat {
	pub fn SetMaxWidth(&self) -> HRESULT;
	pub fn SetMaxHeight(&self) -> HRESULT;
	pub fn SetFontCollection(&self) -> HRESULT;
	pub fn SetFontFamilyName(&self) -> HRESULT;
	pub fn SetFontWeight(&self) -> HRESULT;
	pub fn SetFontStyle(&self) -> HRESULT;
	pub fn SetFontStretch(&self) -> HRESULT;
	pub fn SetFontSize(&self) -> HRESULT;
	pub fn SetUnderline(&self) -> HRESULT;
	pub fn SetStrikethrough(&self) -> HRESULT;
	pub fn SetDrawingEffect(&self) -> HRESULT;
	pub fn SetInlineObject(&self) -> HRESULT;
	pub fn SetTypography(&self) -> HRESULT;
	pub fn SetLocaleName(&self) -> HRESULT;
	pub fn GetMaxWidth(&self) -> f32;
	pub fn GetMaxHeight(&self) -> f32;
	pub fn GetFontCollectionRange(&self) -> HRESULT;
	pub fn GetFontFamilyNameLengthRange(&self) -> HRESULT;
	pub fn GetFontFamilyNameRange(&self) -> HRESULT;
	pub fn GetFontWeightRange(&self) -> HRESULT;
	pub fn GetFontStyleRange(&self) -> HRESULT;
	pub fn GetFontStretchRange(&self) -> HRESULT;
	pub fn GetFontSizeRange(&self) -> HRESULT;
	pub fn GetUnderline(&self) -> HRESULT;
	pub fn GetStrikethrough(&self) -> HRESULT;
	pub fn GetDrawingEffect(&self) -> HRESULT;
	pub fn GetInlineObject(&self) -> HRESULT;
	pub fn GetTypography(&self) -> HRESULT;
	pub fn GetLocaleNameLengthRange(&self) -> HRESULT;
	pub fn GetLocaleNameRange(&self) -> HRESULT;
	/// `renderer`: an `IDWriteTextRenderer`.
	pub fn Draw(&self, context: *mut c_void, renderer: *mut c_void, x: f32, y: f32) -> HRESULT;
	pub fn GetLineMetrics(&self) -> HRESULT;
	pub fn GetMetrics(&self, metrics: *mut DWRITE_TEXT_METRICS) -> HRESULT;
}

// Interfaces implemented by the plugin. Interface arguments are raw borrowed pointers.

#[interface("eaf3a2da-ecf4-4d24-b644-b34f6842024b")]
pub unsafe trait IDWritePixelSnapping: IUnknown {
	pub fn IsPixelSnappingDisabled(&self, context: *mut c_void, is_disabled: *mut BOOL) -> HRESULT;
	pub fn GetCurrentTransform(
		&self,
		context: *mut c_void,
		transform: *mut DWRITE_MATRIX,
	) -> HRESULT;
	pub fn GetPixelsPerDip(&self, context: *mut c_void, pixels_per_dip: *mut f32) -> HRESULT;
}

#[interface("ef8a8135-5cc6-45fe-8825-c5a0724eb819")]
pub unsafe trait IDWriteTextRenderer: IDWritePixelSnapping {
	pub fn DrawGlyphRun(
		&self,
		context: *mut c_void,
		baseline_x: f32,
		baseline_y: f32,
		measuring_mode: i32,
		glyph_run: *const DWRITE_GLYPH_RUN,
		description: *const c_void,
		effect: *mut c_void,
	) -> HRESULT;
	pub fn DrawUnderline(
		&self,
		context: *mut c_void,
		x: f32,
		y: f32,
		underline: *const c_void,
		effect: *mut c_void,
	) -> HRESULT;
	pub fn DrawStrikethrough(
		&self,
		context: *mut c_void,
		x: f32,
		y: f32,
		strikethrough: *const c_void,
		effect: *mut c_void,
	) -> HRESULT;
	pub fn DrawInlineObject(
		&self,
		context: *mut c_void,
		x: f32,
		y: f32,
		object: *mut c_void,
		is_sideways: BOOL,
		is_right_to_left: BOOL,
		effect: *mut c_void,
	) -> HRESULT;
}

/// `ID2D1SimplifiedGeometrySink`, which `dwrite.h` calls `IDWriteGeometrySink`.
#[interface("2cd9069e-12e2-11dc-9fed-001143a055f9")]
pub unsafe trait ID2D1SimplifiedGeometrySink: IUnknown {
	pub fn SetFillMode(&self, mode: i32);
	pub fn SetSegmentFlags(&self, flags: i32);
	pub fn BeginFigure(&self, start: D2D1_POINT_2F, begin: i32);
	pub fn AddLines(&self, points: *const D2D1_POINT_2F, count: u32);
	pub fn AddBeziers(&self, beziers: *const D2D1_BEZIER_SEGMENT, count: u32);
	pub fn EndFigure(&self, end: i32);
	pub fn Close(&self) -> HRESULT;
}
