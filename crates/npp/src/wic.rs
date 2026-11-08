//! Images with the Windows Imaging Component: decodes every format a WIC codec is installed for
//! (PNG, JPEG, GIF, BMP, ICO, TIFF, JPEG XR and DDS with Windows, WebP, HEIF and AVIF with the
//! extensions of the Microsoft Store, more with third-party codecs) and encodes PNG.

mod interfaces;

use core::ffi::c_void;
use windows_core::{GUID, HRESULT, Interface};
use windows_sys::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows_sys::Win32::System::Com::{
	CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
	CoUninitialize, STREAM_SEEK_END, STREAM_SEEK_SET,
};

use interfaces::*;

/// Larger images are not decoded, against files that ask for huge allocations.
const MAX_PIXELS: u64 = 1 << 24;

/// Decoded pixels, rows top-down without padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bitmap {
	pub width: u32,
	pub height: u32,
	pub pixels: Vec<u8>,
}

/// Width and height in pixels of the first frame of the image file `bytes`.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
	let _com = ComApartment::enter();
	let factory = factory()?;
	let frame = first_frame(&factory, bytes)?;
	frame_size(&frame)
}

/// The first frame of the image file `bytes` in 32-bit RGBA with premultiplied alpha, the pixel
/// layout of tiny-skia.
pub fn decode_premultiplied_rgba(bytes: &[u8]) -> Option<Bitmap> {
	decode(bytes, &PIXEL_FORMAT_32BPP_PRGBA)
}

/// The first frame of the image file `bytes` as PNG file, for consumers that only read PNG.
pub fn transcode_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
	let bitmap = decode(bytes, &PIXEL_FORMAT_32BPP_BGRA)?;
	encode_png(bitmap.width, bitmap.height, &bitmap.pixels)
}

/// A PNG file of `width` x `height` pixels in 32-bit BGRA with straight alpha, rows top-down.
pub fn encode_png(width: u32, height: u32, bgra: &[u8]) -> Option<Vec<u8>> {
	let stride = width.checked_mul(4)?;
	let buffer_size = stride.checked_mul(height)?;
	if width == 0 || height == 0 || bgra.len() != buffer_size as usize {
		return None;
	}
	let _com = ComApartment::enter();
	let factory = factory()?;
	let mut stream = core::ptr::null_mut();
	// SAFETY: Valid output pointer; a null `HGLOBAL` lets the stream allocate and free its memory.
	let result = unsafe { CreateStreamOnHGlobal(core::ptr::null_mut(), 1, &mut stream) };
	if result < 0 || stream.is_null() {
		return None;
	}
	// SAFETY: `CreateStreamOnHGlobal` returned an owned `IStream`.
	let stream = unsafe { IStream::from_raw(stream) };
	let encoder: IWICBitmapEncoder = created(|encoder| {
		// SAFETY: Valid GUID and output pointer; the vendor is optional.
		unsafe { factory.CreateEncoder(&CONTAINER_FORMAT_PNG, core::ptr::null(), encoder) }
	})?;
	// SAFETY: `stream` is a valid `IStream`.
	unsafe { encoder.Initialize(stream.as_raw(), BITMAP_ENCODER_NO_CACHE) }.ok().ok()?;
	let frame: IWICBitmapFrameEncode = created(|frame| {
		// SAFETY: Valid output pointer; the encoder options are optional.
		unsafe { encoder.CreateNewFrame(frame, core::ptr::null_mut()) }
	})?;
	// SAFETY: Null selects the default encoder options.
	unsafe { frame.Initialize(core::ptr::null_mut()) }.ok().ok()?;
	// SAFETY: Plain values.
	unsafe { frame.SetSize(width, height) }.ok().ok()?;
	let mut format = PIXEL_FORMAT_32BPP_BGRA;
	// SAFETY: Valid in/out GUID.
	unsafe { frame.SetPixelFormat(&mut format) }.ok().ok()?;
	if format != PIXEL_FORMAT_32BPP_BGRA {
		return None;
	}
	// SAFETY: `bgra` holds `height` rows of `stride` bytes, checked above.
	unsafe { frame.WritePixels(height, stride, buffer_size, bgra.as_ptr()) }.ok().ok()?;
	// SAFETY: No arguments.
	unsafe { frame.Commit() }.ok().ok()?;
	// SAFETY: No arguments.
	unsafe { encoder.Commit() }.ok().ok()?;
	read_stream(&stream)
}

/// Keeps COM initialized on the calling thread while WIC is used, for threads that did not
/// initialize it themselves. Notepad++'s UI thread has initialized OLE already.
struct ComApartment {
	initialized: bool,
}

impl ComApartment {
	fn enter() -> Self {
		// SAFETY: No reserved pointer. A thread in another apartment fails with
		// `RPC_E_CHANGED_MODE` and keeps it, which WIC's free-threaded objects work in as well.
		let result = unsafe { CoInitializeEx(core::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
		Self { initialized: result >= 0 }
	}
}

impl Drop for ComApartment {
	fn drop(&mut self) {
		if self.initialized {
			// SAFETY: Balances the successful `CoInitializeEx` of `enter`.
			unsafe { CoUninitialize() };
		}
	}
}

fn factory() -> Option<IWICImagingFactory> {
	created(|factory| {
		// SAFETY: Valid class and interface ids and output pointer. `windows-core` and `windows-sys`
		// GUIDs have the same layout.
		HRESULT(unsafe {
			CoCreateInstance(
				(&CLSID_WIC_IMAGING_FACTORY as *const GUID).cast(),
				core::ptr::null_mut(),
				CLSCTX_INPROC_SERVER,
				(&IWICImagingFactory::IID as *const GUID).cast(),
				factory,
			)
		})
	})
}

/// The interface that `create` returned an owned pointer to.
fn created<T: Interface>(create: impl FnOnce(*mut *mut c_void) -> HRESULT) -> Option<T> {
	let mut object = core::ptr::null_mut();
	if create(&mut object).is_err() || object.is_null() {
		return None;
	}
	// SAFETY: The creating call returned an owned pointer of the interface `T`.
	Some(unsafe { T::from_raw(object) })
}

/// The first frame of the image file `bytes`. It reads `bytes` while it is alive, so it must not
/// outlive them.
fn first_frame(factory: &IWICImagingFactory, bytes: &[u8]) -> Option<IWICBitmapSource> {
	let size = u32::try_from(bytes.len()).ok()?;
	let stream: IWICStream = created(|stream| {
		// SAFETY: Valid output pointer.
		unsafe { factory.CreateStream(stream) }
	})?;
	// SAFETY: The stream only reads `bytes`, which the callers keep alive while using the frame.
	unsafe { stream.InitializeFromMemory(bytes.as_ptr().cast_mut(), size) }.ok().ok()?;
	let decoder: IWICBitmapDecoder = created(|decoder| {
		// SAFETY: `stream` is a valid `IStream`; the vendor is optional.
		unsafe {
			factory.CreateDecoderFromStream(
				stream.as_raw(),
				core::ptr::null(),
				DECODE_METADATA_CACHE_ON_DEMAND,
				decoder,
			)
		}
	})?;
	created(|frame| {
		// SAFETY: Valid output pointer.
		unsafe { decoder.GetFrame(0, frame) }
	})
}

fn frame_size(frame: &IWICBitmapSource) -> Option<(u32, u32)> {
	let (mut width, mut height) = (0, 0);
	// SAFETY: Valid output pointers.
	unsafe { frame.GetSize(&mut width, &mut height) }.ok().ok()?;
	Some((width, height))
}

/// The first frame of `bytes` in a 32-bit pixel `format`.
fn decode(bytes: &[u8], format: &GUID) -> Option<Bitmap> {
	let _com = ComApartment::enter();
	let factory = factory()?;
	let frame = first_frame(&factory, bytes)?;
	let (width, height) = frame_size(&frame)?;
	if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
		return None;
	}
	let converter: IWICFormatConverter = created(|converter| {
		// SAFETY: Valid output pointer.
		unsafe { factory.CreateFormatConverter(converter) }
	})?;
	// SAFETY: `frame` is a valid `IWICBitmapSource`; no palette is needed for 32-bit formats.
	unsafe {
		converter.Initialize(
			frame.as_raw(),
			format,
			BITMAP_DITHER_TYPE_NONE,
			core::ptr::null_mut(),
			0.0,
			BITMAP_PALETTE_TYPE_CUSTOM,
		)
	}
	.ok()
	.ok()?;
	let stride = width * 4;
	let buffer_size = stride.checked_mul(height)?;
	let mut pixels = vec![0; buffer_size as usize];
	// SAFETY: `pixels` holds `buffer_size` bytes; null copies the whole bitmap.
	unsafe { converter.CopyPixels(core::ptr::null(), stride, buffer_size, pixels.as_mut_ptr()) }
		.ok()
		.ok()?;
	Some(Bitmap { width, height, pixels })
}

/// All bytes written to `stream`.
fn read_stream(stream: &IStream) -> Option<Vec<u8>> {
	let mut size = 0u64;
	// SAFETY: Valid output pointer.
	unsafe { stream.Seek(0, STREAM_SEEK_END, &mut size) }.ok().ok()?;
	// SAFETY: The new position is not needed.
	unsafe { stream.Seek(0, STREAM_SEEK_SET, core::ptr::null_mut()) }.ok().ok()?;
	let size = u32::try_from(size).ok()?;
	let mut bytes = vec![0u8; size as usize];
	let mut read = 0;
	// SAFETY: `bytes` holds `size` bytes.
	unsafe { stream.Read(bytes.as_mut_ptr().cast(), size, &mut read) }.ok().ok()?;
	bytes.truncate(read as usize);
	Some(bytes)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A red and a half transparent blue pixel.
	const PIXELS: [u8; 8] = [0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x80];

	#[test]
	fn encodes_and_decodes_png() {
		let png = encode_png(2, 1, &PIXELS).unwrap();
		assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
		assert_eq!(&png[16..24], &[0, 0, 0, 2, 0, 0, 0, 1]);
		assert_eq!(image_size(&png), Some((2, 1)));
		let bitmap = decode_premultiplied_rgba(&png).unwrap();
		assert_eq!((bitmap.width, bitmap.height), (2, 1));
		assert_eq!(bitmap.pixels, [0xFF, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x80, 0x80]);
	}

	#[test]
	fn transcodes_bmp_to_png() {
		// A 1 x 1 24-bit BMP with a green pixel.
		let mut bmp = b"BM".to_vec();
		bmp.extend_from_slice(&58u32.to_le_bytes());
		bmp.extend_from_slice(&[0, 0, 0, 0]);
		bmp.extend_from_slice(&54u32.to_le_bytes());
		bmp.extend_from_slice(&40u32.to_le_bytes());
		bmp.extend_from_slice(&1i32.to_le_bytes());
		bmp.extend_from_slice(&1i32.to_le_bytes());
		bmp.extend_from_slice(&1u16.to_le_bytes());
		bmp.extend_from_slice(&24u16.to_le_bytes());
		bmp.extend_from_slice(&[0; 24]);
		bmp.extend_from_slice(&[0x00, 0xFF, 0x00, 0x00]);
		let png = transcode_to_png(&bmp).unwrap();
		let bitmap = decode_premultiplied_rgba(&png).unwrap();
		assert_eq!(bitmap.pixels, [0x00, 0xFF, 0x00, 0xFF]);
	}

	#[test]
	fn rejects_invalid_input() {
		assert_eq!(image_size(b""), None);
		assert_eq!(image_size(b"no image"), None);
		assert_eq!(decode_premultiplied_rgba(b"\x89PNG\r\n\x1a\n"), None);
		assert_eq!(encode_png(2, 1, &PIXELS[..4]), None);
		assert_eq!(encode_png(0, 0, &[]), None);
	}
}
