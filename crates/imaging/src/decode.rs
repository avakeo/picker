use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use exif::{In, Tag};

use crate::{format::ImageFormat, limits::DecodeLimits, ImagingError, Result};

/// A decoded image in RGBA8 byte order (not premultiplied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl DecodedImage {
    /// Returns the number of pixels.
    #[must_use]
    pub const fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    fn into_image(self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.rgba)
            .expect("DecodedImage always holds width * height * 4 bytes")
    }
}

/// Platform-independent decoder boundary.
pub trait Decoder: Send + Sync {
    /// Decodes a file after applying EXIF orientation.
    fn decode_file(
        &self,
        path: &Path,
        format: ImageFormat,
        limits: &DecodeLimits,
    ) -> Result<DecodedImage>;

    /// Decodes a file after applying EXIF orientation, with the longer edge
    /// reduced to at most `max_edge`. Decoders that can decode at a reduced
    /// size override this to skip the full resolution image.
    fn decode_scaled(
        &self,
        path: &Path,
        format: ImageFormat,
        limits: &DecodeLimits,
        max_edge: u32,
    ) -> Result<DecodedImage> {
        let decoded = self.decode_file(path, format, limits)?;
        Ok(downscale(decoded, max_edge))
    }
}

/// Portable Rust decoder used on every platform as the fallback.
#[derive(Debug, Default, Clone, Copy)]
pub struct RustFallbackDecoder;

impl Decoder for RustFallbackDecoder {
    fn decode_file(
        &self,
        path: &Path,
        format: ImageFormat,
        limits: &DecodeLimits,
    ) -> Result<DecodedImage> {
        let metadata = fs::metadata(path)?;
        if metadata.len() > limits.max_file_bytes {
            return Err(ImagingError::TooLarge {
                limit: "file size",
                actual: metadata.len(),
            });
        }
        let bytes = fs::read(path)?;
        let mut decoded = self.decode_bytes(format, &bytes, limits)?;
        let orientation = read_orientation(path).unwrap_or(1);
        decoded = apply_orientation(decoded, orientation);
        Ok(decoded)
    }
}

impl RustFallbackDecoder {
    /// Decodes an in-memory image, applying the given safety limits.
    pub fn decode_bytes(
        &self,
        format: ImageFormat,
        bytes: &[u8],
        limits: &DecodeLimits,
    ) -> Result<DecodedImage> {
        let image_format = format
            .image_crate_format()
            .ok_or_else(|| ImagingError::UnsupportedFormat(format.as_str().to_owned()))?;
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image_format);
        let mut image_limits = image::Limits::default();
        image_limits.max_image_width = Some(limits.max_edge);
        image_limits.max_image_height = Some(limits.max_edge);
        image_limits.max_alloc = Some(limits.max_alloc_bytes);
        reader.limits(image_limits);
        let decoded = reader.decode()?;
        let (width, height) = (decoded.width(), decoded.height());
        if width == 0 || height == 0 {
            return Err(ImagingError::Decode("image has zero dimensions".to_owned()));
        }
        if u64::from(width) * u64::from(height) > limits.max_pixels {
            return Err(ImagingError::TooLarge {
                limit: "pixel count",
                actual: u64::from(width) * u64::from(height),
            });
        }
        let rgba = decoded.to_rgba8();
        Ok(DecodedImage {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        })
    }
}

/// Decoder selected for the current platform.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlatformDecoder;

impl Decoder for PlatformDecoder {
    fn decode_file(
        &self,
        path: &Path,
        format: ImageFormat,
        limits: &DecodeLimits,
    ) -> Result<DecodedImage> {
        #[cfg(target_os = "macos")]
        if format == ImageFormat::Heic {
            return crate::imageio::decode(path, limits, None);
        }
        RustFallbackDecoder.decode_file(path, format, limits)
    }

    fn decode_scaled(
        &self,
        path: &Path,
        format: ImageFormat,
        limits: &DecodeLimits,
        max_edge: u32,
    ) -> Result<DecodedImage> {
        #[cfg(target_os = "macos")]
        if format == ImageFormat::Heic {
            return crate::imageio::decode(path, limits, Some(max_edge));
        }
        RustFallbackDecoder.decode_scaled(path, format, limits, max_edge)
    }
}

/// Size that fits `width` x `height` within `max_edge` on both sides while
/// keeping the aspect ratio. Images that already fit keep their size.
#[must_use]
pub fn fit_within(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max_edge || max_edge == 0 {
        return (width, height);
    }
    let shrink = |side: u32| {
        let scaled =
            (u64::from(side) * u64::from(max_edge) + u64::from(longest) / 2) / u64::from(longest);
        u32::try_from(scaled).unwrap_or(max_edge).max(1)
    };
    (shrink(width), shrink(height))
}

/// Shrinks an image so its longer edge is at most `max_edge`. Smaller images
/// are returned unchanged.
#[must_use]
pub fn downscale(decoded: DecodedImage, max_edge: u32) -> DecodedImage {
    let (width, height) = fit_within(decoded.width, decoded.height, max_edge);
    if (width, height) == (decoded.width, decoded.height) {
        return decoded;
    }
    let image = image::imageops::thumbnail(&decoded.into_image(), width, height);
    DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

fn read_orientation(path: &Path) -> Option<u32> {
    let file = fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;
    exif.get_field(Tag::Orientation, In::PRIMARY)?
        .value
        .get_uint(0)
}

fn apply_orientation(decoded: DecodedImage, orientation: u32) -> DecodedImage {
    let image = decoded.into_image();
    let image = match orientation {
        2 => image::imageops::flip_horizontal(&image),
        3 => image::imageops::rotate180(&image),
        4 => image::imageops::flip_vertical(&image),
        5 => image::imageops::flip_horizontal(&image::imageops::rotate90(&image)),
        6 => image::imageops::rotate90(&image),
        7 => image::imageops::flip_horizontal(&image::imageops::rotate270(&image)),
        8 => image::imageops::rotate270(&image),
        _ => image,
    };
    DecodedImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

/// Returns the canonical path used to cache a thumbnail.
#[must_use]
pub fn thumbnail_path(cache_root: &Path, asset_id: i64, max_edge: u32) -> PathBuf {
    cache_root
        .join(max_edge.to_string())
        .join(format!("{asset_id}.png"))
}
