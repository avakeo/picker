#![deny(unsafe_code)]

//! Image decoding, metadata, thumbnails, and perceptual hashes for Picker.

mod decode;
mod error;
mod format;
mod limits;
mod metadata;
mod thumbnail;

#[cfg(test)]
mod tests;

pub use decode::{thumbnail_path, DecodedImage, Decoder, PlatformDecoder, RustFallbackDecoder};
pub use error::{ImagingError, Result};
pub use format::ImageFormat;
pub use limits::DecodeLimits;
pub use metadata::{extract, parse_exif_datetime, GeoPoint, ImageMetadata};
pub use thumbnail::{ThumbnailCache, ThumbnailSize};
