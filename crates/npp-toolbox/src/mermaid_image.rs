//! The natural size of images in Mermaid diagrams, read by the Windows Imaging Component.

use crate::mermaid::ImageDecoder;

#[cfg(not(feature = "pure-rust"))]
struct WicImages;

#[cfg(not(feature = "pure-rust"))]
impl ImageDecoder for WicImages {
	fn natural_size(&self, bytes: &[u8]) -> Option<(u32, u32)> {
		npp::wic::image_size(bytes)
	}
}

/// The image decoder for [`crate::mermaid::DiagramRenderer`].
pub fn decoder() -> &'static dyn ImageDecoder {
	#[cfg(not(feature = "pure-rust"))]
	return &WicImages;
	#[cfg(feature = "pure-rust")]
	&crate::mermaid::NoImageDecoder
}
