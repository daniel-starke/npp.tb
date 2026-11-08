//! The subset of the Windows Imaging Component (`wincodec.h`) and of `IStream` (`objidl.h`) used
//! by [`super`]. `windows-sys` has neither the COM interfaces nor the WIC GUIDs.
//!
//! Interfaces that are only called declare their methods up to the last one used. Methods that are
//! never called keep their vtable slot with a placeholder signature. The method names are those of
//! the SDK header.

// The signatures are those of the SDK, including their argument counts.
#![allow(
	non_snake_case,
	non_camel_case_types,
	clippy::upper_case_acronyms,
	clippy::too_many_arguments
)]

use core::ffi::c_void;

use windows_core::{GUID, HRESULT, IUnknown, interface};

/// `CLSID_WICImagingFactory`
pub const CLSID_WIC_IMAGING_FACTORY: GUID = GUID::from_u128(0xcacaf262_9370_4615_a13b_9f5539da4c0a);
/// `GUID_ContainerFormatPng`
pub const CONTAINER_FORMAT_PNG: GUID = GUID::from_u128(0x1b7cfaf4_713f_473c_bbcd_6137425faeaf);
/// `GUID_WICPixelFormat32bppBGRA`: straight alpha.
pub const PIXEL_FORMAT_32BPP_BGRA: GUID = GUID::from_u128(0x6fddc324_4e03_4bfe_b185_3d77768dc90f);
/// `GUID_WICPixelFormat32bppPRGBA`: premultiplied alpha, the pixel layout of tiny-skia.
pub const PIXEL_FORMAT_32BPP_PRGBA: GUID = GUID::from_u128(0x3cc4a650_a527_4d37_a916_3142c7ebedba);

/// `WICDecodeMetadataCacheOnDemand`
pub const DECODE_METADATA_CACHE_ON_DEMAND: i32 = 0;
/// `WICBitmapEncoderNoCache`
pub const BITMAP_ENCODER_NO_CACHE: i32 = 2;
/// `WICBitmapDitherTypeNone`
pub const BITMAP_DITHER_TYPE_NONE: i32 = 0;
/// `WICBitmapPaletteTypeCustom`
pub const BITMAP_PALETTE_TYPE_CUSTOM: i32 = 0;

#[interface("0c733a30-2a1c-11ce-ade5-00aa0044773d")]
pub unsafe trait ISequentialStream: IUnknown {
	pub fn Read(&self, buffer: *mut c_void, size: u32, read: *mut u32) -> HRESULT;
	pub fn Write(&self) -> HRESULT;
}

#[interface("0000000c-0000-0000-c000-000000000046")]
pub unsafe trait IStream: ISequentialStream {
	/// `offset` is a `LARGE_INTEGER`, `position` an `ULARGE_INTEGER`.
	pub fn Seek(&self, offset: i64, origin: u32, position: *mut u64) -> HRESULT;
	pub fn SetSize(&self) -> HRESULT;
	pub fn CopyTo(&self) -> HRESULT;
	pub fn Commit(&self) -> HRESULT;
	pub fn Revert(&self) -> HRESULT;
	pub fn LockRegion(&self) -> HRESULT;
	pub fn UnlockRegion(&self) -> HRESULT;
	pub fn Stat(&self) -> HRESULT;
	pub fn Clone(&self) -> HRESULT;
}

#[interface("135ff860-22b7-4ddf-b0f6-218f4f299a43")]
pub unsafe trait IWICStream: IStream {
	pub fn InitializeFromIStream(&self) -> HRESULT;
	pub fn InitializeFromFilename(&self) -> HRESULT;
	pub fn InitializeFromMemory(&self, buffer: *mut u8, size: u32) -> HRESULT;
}

#[interface("ec5ec8a9-c395-4314-9c77-54d7a935ff70")]
pub unsafe trait IWICImagingFactory: IUnknown {
	pub fn CreateDecoderFromFilename(&self) -> HRESULT;
	/// `stream`: an `IStream`. `decoder`: receives an `IWICBitmapDecoder`.
	pub fn CreateDecoderFromStream(
		&self,
		stream: *mut c_void,
		vendor: *const GUID,
		options: i32,
		decoder: *mut *mut c_void,
	) -> HRESULT;
	pub fn CreateDecoderFromFileHandle(&self) -> HRESULT;
	pub fn CreateComponentInfo(&self) -> HRESULT;
	pub fn CreateDecoder(&self) -> HRESULT;
	/// `encoder`: receives an `IWICBitmapEncoder`.
	pub fn CreateEncoder(
		&self,
		container_format: *const GUID,
		vendor: *const GUID,
		encoder: *mut *mut c_void,
	) -> HRESULT;
	pub fn CreatePalette(&self) -> HRESULT;
	/// `converter`: receives an `IWICFormatConverter`.
	pub fn CreateFormatConverter(&self, converter: *mut *mut c_void) -> HRESULT;
	pub fn CreateBitmapScaler(&self) -> HRESULT;
	pub fn CreateBitmapClipper(&self) -> HRESULT;
	pub fn CreateBitmapFlipRotator(&self) -> HRESULT;
	/// `stream`: receives an `IWICStream`.
	pub fn CreateStream(&self, stream: *mut *mut c_void) -> HRESULT;
}

#[interface("9edde9e7-8dee-47ea-99df-e6faf2ed44bf")]
pub unsafe trait IWICBitmapDecoder: IUnknown {
	pub fn QueryCapability(&self) -> HRESULT;
	pub fn Initialize(&self) -> HRESULT;
	pub fn GetContainerFormat(&self) -> HRESULT;
	pub fn GetDecoderInfo(&self) -> HRESULT;
	pub fn CopyPalette(&self) -> HRESULT;
	pub fn GetMetadataQueryReader(&self) -> HRESULT;
	pub fn GetPreview(&self) -> HRESULT;
	pub fn GetColorContexts(&self) -> HRESULT;
	pub fn GetThumbnail(&self) -> HRESULT;
	pub fn GetFrameCount(&self) -> HRESULT;
	/// `frame`: receives an `IWICBitmapFrameDecode`, which is an `IWICBitmapSource`.
	pub fn GetFrame(&self, index: u32, frame: *mut *mut c_void) -> HRESULT;
}

#[interface("00000120-a8f2-4877-ba0a-fd2b6645fb94")]
pub unsafe trait IWICBitmapSource: IUnknown {
	pub fn GetSize(&self, width: *mut u32, height: *mut u32) -> HRESULT;
	pub fn GetPixelFormat(&self) -> HRESULT;
	pub fn GetResolution(&self) -> HRESULT;
	pub fn CopyPalette(&self) -> HRESULT;
	/// `rectangle`: a `WICRect`, null for the whole bitmap.
	pub fn CopyPixels(
		&self,
		rectangle: *const c_void,
		stride: u32,
		buffer_size: u32,
		buffer: *mut u8,
	) -> HRESULT;
}

#[interface("00000301-a8f2-4877-ba0a-fd2b6645fb94")]
pub unsafe trait IWICFormatConverter: IWICBitmapSource {
	/// `source`: an `IWICBitmapSource`. `palette`: an `IWICPalette`.
	pub fn Initialize(
		&self,
		source: *mut c_void,
		format: *const GUID,
		dither: i32,
		palette: *mut c_void,
		alpha_threshold_percent: f64,
		palette_translate: i32,
	) -> HRESULT;
}

#[interface("00000103-a8f2-4877-ba0a-fd2b6645fb94")]
pub unsafe trait IWICBitmapEncoder: IUnknown {
	/// `stream`: an `IStream`.
	pub fn Initialize(&self, stream: *mut c_void, cache_option: i32) -> HRESULT;
	pub fn GetContainerFormat(&self) -> HRESULT;
	pub fn GetEncoderInfo(&self) -> HRESULT;
	pub fn SetColorContexts(&self) -> HRESULT;
	pub fn SetPalette(&self) -> HRESULT;
	pub fn SetThumbnail(&self) -> HRESULT;
	pub fn SetPreview(&self) -> HRESULT;
	/// `frame`: receives an `IWICBitmapFrameEncode`. `options`: receives an `IPropertyBag2`, may
	/// be null.
	pub fn CreateNewFrame(&self, frame: *mut *mut c_void, options: *mut *mut c_void) -> HRESULT;
	pub fn Commit(&self) -> HRESULT;
}

#[interface("00000105-a8f2-4877-ba0a-fd2b6645fb94")]
pub unsafe trait IWICBitmapFrameEncode: IUnknown {
	/// `options`: an `IPropertyBag2`, null for the defaults.
	pub fn Initialize(&self, options: *mut c_void) -> HRESULT;
	pub fn SetSize(&self, width: u32, height: u32) -> HRESULT;
	pub fn SetResolution(&self) -> HRESULT;
	/// The encoder replaces `format` with the closest format it supports.
	pub fn SetPixelFormat(&self, format: *mut GUID) -> HRESULT;
	pub fn SetColorContexts(&self) -> HRESULT;
	pub fn SetPalette(&self) -> HRESULT;
	pub fn SetThumbnail(&self) -> HRESULT;
	pub fn WritePixels(
		&self,
		line_count: u32,
		stride: u32,
		buffer_size: u32,
		pixels: *const u8,
	) -> HRESULT;
	pub fn WriteSource(&self) -> HRESULT;
	pub fn Commit(&self) -> HRESULT;
}
