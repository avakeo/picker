use std::io::Cursor;

use image::{DynamicImage, ImageFormat as CrateFormat, Rgba, RgbaImage};

use super::*;

fn test_image(width: u32, height: u32) -> DynamicImage {
    let mut image = RgbaImage::new(width, height);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = Rgba([(x * 40) as u8, (y * 60) as u8, ((x + y) * 20) as u8, 255]);
    }
    DynamicImage::ImageRgba8(image)
}

fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let crate_format = match format {
        ImageFormat::Jpeg => CrateFormat::Jpeg,
        ImageFormat::Png => CrateFormat::Png,
        ImageFormat::Webp => CrateFormat::WebP,
        ImageFormat::Heic => panic!("HEIC cannot be encoded in the fallback test"),
    };
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, crate_format)
        .expect("encode test image");
    bytes.into_inner()
}

fn jpeg_with_orientation(image: &DynamicImage, orientation: u16) -> Vec<u8> {
    let mut jpeg = encode(image, ImageFormat::Jpeg);
    assert_eq!(&jpeg[0..2], &[0xFF, 0xD8]);
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2A\x00\x08\x00\x00\x00");
    tiff.extend_from_slice(&1_u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112_u16.to_le_bytes());
    tiff.extend_from_slice(&3_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&u32::from(orientation).to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    let mut payload = b"Exif\x00\x00".to_vec();
    payload.extend_from_slice(&tiff);
    let mut segment = vec![0xFF, 0xE1];
    let length = u16::try_from(payload.len() + 2).expect("small fixture");
    segment.extend_from_slice(&length.to_be_bytes());
    segment.extend_from_slice(&payload);
    jpeg.splice(2..2, segment);
    jpeg
}

#[test]
fn decodes_jpeg_png_and_webp() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let decoder = RustFallbackDecoder;
    let limits = DecodeLimits::default();
    for format in [ImageFormat::Jpeg, ImageFormat::Png, ImageFormat::Webp] {
        let path = temporary.path().join(format!("sample.{}", format.as_str()));
        std::fs::write(&path, encode(&test_image(8, 5), format)).expect("write fixture");
        let decoded = decoder
            .decode_file(&path, format, &limits)
            .unwrap_or_else(|error| panic!("{format:?}: {error}"));
        assert_eq!((decoded.width, decoded.height), (8, 5));
        assert_eq!(decoded.rgba.len(), 8 * 5 * 4);
    }
}

#[test]
fn applies_exif_orientation_to_dimensions() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let path = temporary.path().join("rotated.jpg");
    std::fs::write(&path, jpeg_with_orientation(&test_image(6, 3), 6)).expect("write fixture");
    let decoded = RustFallbackDecoder
        .decode_file(&path, ImageFormat::Jpeg, &DecodeLimits::default())
        .expect("decode rotated jpeg");
    assert_eq!((decoded.width, decoded.height), (3, 6));
}

#[test]
fn rejects_corrupt_oversized_and_unsupported_inputs() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let decoder = RustFallbackDecoder;
    let corrupt = temporary.path().join("corrupt.png");
    std::fs::write(&corrupt, b"not a png").expect("write fixture");
    assert!(matches!(
        decoder.decode_file(&corrupt, ImageFormat::Png, &DecodeLimits::default()),
        Err(ImagingError::Decode(_))
    ));

    let valid = temporary.path().join("valid.png");
    std::fs::write(&valid, encode(&test_image(4, 4), ImageFormat::Png)).expect("write fixture");
    let tiny_limit = DecodeLimits {
        max_file_bytes: 1,
        ..DecodeLimits::default()
    };
    assert!(matches!(
        decoder.decode_file(&valid, ImageFormat::Png, &tiny_limit),
        Err(ImagingError::TooLarge { .. })
    ));

    let pixel_limit = DecodeLimits {
        max_pixels: 1,
        ..DecodeLimits::default()
    };
    assert!(matches!(
        decoder.decode_file(&valid, ImageFormat::Png, &pixel_limit),
        Err(ImagingError::TooLarge { .. })
    ));

    if cfg!(not(target_os = "macos")) {
        assert!(matches!(
            decoder.decode_file(&valid, ImageFormat::Heic, &DecodeLimits::default()),
            Err(ImagingError::UnsupportedFormat(_))
        ));
    }
}

#[test]
fn generates_two_sizes_and_reuses_the_cache() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let source = temporary.path().join("source.webp");
    std::fs::write(&source, encode(&test_image(1200, 800), ImageFormat::Webp)).expect("fixture");
    let cache = ThumbnailCache::new(temporary.path().join("cache"));
    let paths = cache
        .generate(
            &RustFallbackDecoder,
            42,
            &source,
            ImageFormat::Webp,
            &DecodeLimits::default(),
        )
        .expect("generate thumbnails");
    assert_eq!(paths.len(), 2);
    for (path, size) in paths
        .iter()
        .zip([ThumbnailSize::Grid, ThumbnailSize::Detail])
    {
        assert!(path.is_file());
        let thumbnail = image::open(path).expect("read thumbnail");
        assert!(thumbnail.width() <= size.max_edge());
        assert!(thumbnail.height() <= size.max_edge());
    }

    std::fs::remove_file(&source).expect("remove source");
    let cached = cache
        .get_or_generate(
            &RustFallbackDecoder,
            42,
            ThumbnailSize::Grid,
            &source,
            ImageFormat::Webp,
            &DecodeLimits::default(),
        )
        .expect("cache hit must not read the source");
    assert_eq!(cached, paths[0]);
}

#[test]
fn missing_cache_is_regenerated_but_broken_source_fails() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let source = temporary.path().join("source.png");
    std::fs::write(&source, encode(&test_image(10, 10), ImageFormat::Png)).expect("fixture");
    let cache = ThumbnailCache::new(temporary.path().join("cache"));
    let path = cache
        .get_or_generate(
            &RustFallbackDecoder,
            7,
            ThumbnailSize::Grid,
            &source,
            ImageFormat::Png,
            &DecodeLimits::default(),
        )
        .expect("generate");
    std::fs::remove_file(&path).expect("remove cached file");
    assert!(cache
        .get_or_generate(
            &RustFallbackDecoder,
            7,
            ThumbnailSize::Grid,
            &source,
            ImageFormat::Png,
            &DecodeLimits::default(),
        )
        .is_ok());

    std::fs::write(&source, b"broken").expect("overwrite source");
    assert!(cache
        .get_or_generate(
            &RustFallbackDecoder,
            8,
            ThumbnailSize::Grid,
            &source,
            ImageFormat::Png,
            &DecodeLimits::default(),
        )
        .is_err());
}

#[test]
fn detects_formats_from_magic_and_path() {
    assert_eq!(
        ImageFormat::from_magic(&[0xFF, 0xD8, 0xFF, 0xE0]),
        Some(ImageFormat::Jpeg)
    );
    assert_eq!(
        ImageFormat::from_magic(b"\x89PNG\r\n\x1a\nrest"),
        Some(ImageFormat::Png)
    );
    assert_eq!(
        ImageFormat::from_path(std::path::Path::new("photo.HEIC")),
        Some(ImageFormat::Heic)
    );
    assert_eq!(
        ImageFormat::from_path(std::path::Path::new("photo.gif")),
        None
    );
}
