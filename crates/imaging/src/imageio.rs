//! Decoding through ImageIO on macOS (ADR-002).
//!
//! ImageIO decodes HEIC with the system codec, applies the EXIF orientation,
//! and can decode at a reduced size, so a thumbnail never needs the full
//! resolution image in memory.
#![allow(unsafe_code)]

use std::{fs, path::Path};

use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo,
};
use objc2_image_io::{
    kCGImagePropertyOrientation, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
    kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceShouldCache, kCGImageSourceThumbnailMaxPixelSize, CGImageSource,
};

use crate::{decode::DecodedImage, limits::DecodeLimits, ImagingError, Result};

/// Decodes the first image of a file, oriented upright and converted to
/// sRGB. With `max_edge`, the longer edge is reduced to at most that length.
pub(crate) fn decode(
    path: &Path,
    limits: &DecodeLimits,
    max_edge: Option<u32>,
) -> Result<DecodedImage> {
    let (source, stored) = open(path, limits)?;
    let longest = stored.width.max(stored.height);
    let target = max_edge.map_or(longest, |edge| edge.clamp(1, longest));
    let target = CFNumber::new_i64(i64::from(target));
    let thumbnail_options = options(&[
        (
            unsafe { kCGImageSourceCreateThumbnailFromImageAlways },
            boolean(true),
        ),
        (
            unsafe { kCGImageSourceCreateThumbnailWithTransform },
            boolean(true),
        ),
        (
            unsafe { kCGImageSourceThumbnailMaxPixelSize },
            target.as_ref(),
        ),
        (unsafe { kCGImageSourceShouldCache }, boolean(false)),
    ]);
    // SAFETY: `source` is valid and index 0 exists (checked by `open`).
    let image = unsafe { source.thumbnail_at_index(0, Some(thumbnail_options.as_ref())) }
        .ok_or_else(|| decode_error("ImageIO could not decode the image"))?;
    render(&image, limits)
}

/// Reads the upright size of the first image without decoding its pixels.
pub(crate) fn dimensions(path: &Path, limits: &DecodeLimits) -> Result<(u32, u32)> {
    let (_, stored) = open(path, limits)?;
    Ok(if (5..=8).contains(&stored.orientation) {
        (stored.height, stored.width)
    } else {
        (stored.width, stored.height)
    })
}

/// Opens an image source and checks the stored size against the limits.
fn open(path: &Path, limits: &DecodeLimits) -> Result<(CFRetained<CGImageSource>, Stored)> {
    let size = fs::metadata(path)?.len();
    if size > limits.max_file_bytes {
        return Err(ImagingError::TooLarge {
            limit: "file size",
            actual: size,
        });
    }
    let url = objc2_core_foundation::CFURL::from_file_path(path)
        .ok_or_else(|| decode_error("the path cannot be converted to a URL"))?;
    let no_cache = options(&[(unsafe { kCGImageSourceShouldCache }, boolean(false))]);
    // SAFETY: `url` and the options dictionary are valid CF objects for the
    // duration of the call; ImageIO returns a retained source or null.
    let source = unsafe { CGImageSource::with_url(&url, Some(no_cache.as_ref())) }
        .ok_or_else(|| decode_error("ImageIO could not open the file"))?;
    // SAFETY: `source` is a valid image source.
    if unsafe { source.count() } == 0 {
        return Err(decode_error("ImageIO found no image in the file"));
    }

    let stored = stored_properties(&source)?;
    if stored.width > limits.max_edge || stored.height > limits.max_edge {
        return Err(ImagingError::TooLarge {
            limit: "edge length",
            actual: u64::from(stored.width.max(stored.height)),
        });
    }
    let pixels = u64::from(stored.width) * u64::from(stored.height);
    if pixels > limits.max_pixels {
        return Err(ImagingError::TooLarge {
            limit: "pixel count",
            actual: pixels,
        });
    }
    Ok((source, stored))
}

/// Pixel size as stored in the file, before the orientation is applied.
struct Stored {
    width: u32,
    height: u32,
    orientation: u32,
}

/// Reads the stored pixel size and orientation without decoding the image.
fn stored_properties(source: &CGImageSource) -> Result<Stored> {
    // SAFETY: `source` is valid and index 0 exists.
    let properties = unsafe { source.properties_at_index(0, None) }
        .ok_or_else(|| decode_error("ImageIO returned no image properties"))?;
    // SAFETY: image properties are a dictionary keyed by CFString.
    let properties: CFRetained<CFDictionary<CFString, CFType>> =
        unsafe { CFRetained::cast_unchecked(properties) };
    let read = |key: &CFString| {
        properties
            .get(key)
            .and_then(|value| value.downcast::<CFNumber>().ok())
            .and_then(|number| number.as_i64())
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
    };
    // SAFETY: the keys are constant CFStrings exported by ImageIO.
    let width = read(unsafe { kCGImagePropertyPixelWidth });
    let height = read(unsafe { kCGImagePropertyPixelHeight });
    let orientation = read(unsafe { kCGImagePropertyOrientation }).unwrap_or(1);
    let (width, height) = width
        .zip(height)
        .ok_or_else(|| decode_error("image has zero or unknown dimensions"))?;
    Ok(Stored {
        width,
        height,
        orientation,
    })
}

/// Draws a CGImage into an sRGB RGBA8 buffer and removes the
/// premultiplication that Core Graphics bitmap contexts require.
fn render(image: &CGImage, limits: &DecodeLimits) -> Result<DecodedImage> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    if width == 0 || height == 0 {
        return Err(decode_error("image has zero dimensions"));
    }
    let bytes_per_row = width
        .checked_mul(4)
        .ok_or_else(|| decode_error("image is too wide"))?;
    let length = bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| decode_error("image is too large"))?;
    if length as u64 > limits.max_alloc_bytes {
        return Err(ImagingError::TooLarge {
            limit: "decoded allocation",
            actual: length as u64,
        });
    }
    let mut rgba = vec![0_u8; length];
    // SAFETY: the name is a constant CFString exported by Core Graphics.
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or_else(|| decode_error("sRGB color space is unavailable"))?;
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    {
        // SAFETY: `rgba` holds `bytes_per_row * height` bytes and outlives
        // the context, which is dropped at the end of this block before the
        // buffer is read.
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                width,
                height,
                8,
                bytes_per_row,
                Some(&srgb),
                info,
            )
        }
        .ok_or_else(|| decode_error("Core Graphics could not create a bitmap"))?;
        let bounds = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        );
        CGContext::draw_image(Some(&context), bounds, Some(image));
    }
    unpremultiply(&mut rgba);
    let width = u32::try_from(width).map_err(|_| decode_error("image is too wide"))?;
    let height = u32::try_from(height).map_err(|_| decode_error("image is too tall"))?;
    Ok(DecodedImage {
        width,
        height,
        rgba,
    })
}

fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            let value = (u16::from(*channel) * 255 + alpha / 2) / alpha;
            *channel = value.min(255) as u8;
        }
    }
}

fn options(entries: &[(&CFString, &CFType)]) -> CFRetained<CFDictionary<CFString, CFType>> {
    let keys: Vec<&CFString> = entries.iter().map(|(key, _)| *key).collect();
    let values: Vec<&CFType> = entries.iter().map(|(_, value)| *value).collect();
    CFDictionary::from_slices(&keys, &values)
}

fn boolean(value: bool) -> &'static CFType {
    CFBoolean::new(value)
}

fn decode_error(message: &str) -> ImagingError {
    ImagingError::Decode(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::unpremultiply;

    #[test]
    fn unpremultiply_restores_straight_alpha() {
        let mut pixels = [64, 32, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0];
        unpremultiply(&mut pixels);
        assert_eq!(pixels, [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }
}
