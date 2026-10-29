#![deny(unsafe_code)]

//! Image decoding, metadata, thumbnails, and perceptual hashes for Picker.

mod color;
mod decode;
mod error;
mod format;
#[cfg(target_os = "macos")]
mod imageio;
mod limits;
mod metadata;
mod phash;
mod thumbnail;

#[cfg(test)]
mod tests;

pub use color::{delta_e, extract_colors, rgb_to_lab, ExtractedColor};
pub use decode::{
    downscale, fit_within, thumbnail_path, DecodedImage, Decoder, PlatformDecoder,
    RustFallbackDecoder,
};
pub use error::{ImagingError, Result};
pub use format::ImageFormat;
pub use limits::DecodeLimits;
pub use metadata::{extract, extract_exif, parse_exif_datetime, GeoPoint, ImageMetadata};
pub use phash::{dhash, hamming_distance, is_near_duplicate, DEFAULT_NEAR_DUPLICATE_DISTANCE};
pub use thumbnail::{ThumbnailCache, ThumbnailSize};
