#![deny(unsafe_code)]

//! Image decoding, metadata, thumbnails, and perceptual hashes for Picker.

mod color;
mod decode;
mod error;
mod format;
mod limits;
mod metadata;
mod phash;
mod thumbnail;

#[cfg(test)]
mod tests;

pub use color::{extract_colors, ExtractedColor};
pub use decode::{thumbnail_path, DecodedImage, Decoder, PlatformDecoder, RustFallbackDecoder};
pub use error::{ImagingError, Result};
pub use format::ImageFormat;
pub use limits::DecodeLimits;
pub use metadata::{extract, parse_exif_datetime, GeoPoint, ImageMetadata};
pub use phash::{dhash, hamming_distance, is_near_duplicate, DEFAULT_NEAR_DUPLICATE_DISTANCE};
pub use thumbnail::{ThumbnailCache, ThumbnailSize};
