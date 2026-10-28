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

struct ExifFixture<'a> {
    make: &'a str,
    model: &'a str,
    datetime: &'a str,
    iso: u32,
    gps: bool,
}

fn jpeg_with_exif(image: &DynamicImage, fixture: &ExifFixture<'_>) -> Vec<u8> {
    let mut jpeg = encode(image, ImageFormat::Jpeg);
    let make = format!("{}\0", fixture.make).into_bytes();
    let model = format!("{}\0", fixture.model).into_bytes();
    let datetime = format!("{}\0", fixture.datetime).into_bytes();

    let mut data = Vec::<u8>::new();
    let data_start = 8 + 2 + 9 * 12 + 4;
    let mut cursor = data_start;
    let make_offset = push_data(&mut data, &mut cursor, &make);
    let model_offset = push_data(&mut data, &mut cursor, &model);
    let datetime_offset = push_data(&mut data, &mut cursor, &datetime);
    let mut rational = Vec::new();
    rational.extend_from_slice(&1_u32.to_le_bytes());
    rational.extend_from_slice(&125_u32.to_le_bytes());
    let exposure_offset = push_data(&mut data, &mut cursor, &rational);
    rational.clear();
    rational.extend_from_slice(&28_u32.to_le_bytes());
    rational.extend_from_slice(&10_u32.to_le_bytes());
    let f_number_offset = push_data(&mut data, &mut cursor, &rational);
    rational.clear();
    rational.extend_from_slice(&35_u32.to_le_bytes());
    rational.extend_from_slice(&1_u32.to_le_bytes());
    let focal_offset = push_data(&mut data, &mut cursor, &rational);

    let gps_ifd_offset = if fixture.gps {
        let offset = cursor;
        let gps_data_start = cursor + 2 + 4 * 12 + 4;
        let lat_offset = gps_data_start;
        let lon_offset = lat_offset + 24;
        let gps_entry = |tag: u16, kind: u16, count: u32, value: [u8; 4]| {
            let mut entry = Vec::with_capacity(12);
            entry.extend_from_slice(&tag.to_le_bytes());
            entry.extend_from_slice(&kind.to_le_bytes());
            entry.extend_from_slice(&count.to_le_bytes());
            entry.extend_from_slice(&value);
            entry
        };
        let inline = |bytes: [u8; 2]| [bytes[0], bytes[1], 0, 0];
        let mut gps = Vec::new();
        gps.extend_from_slice(&4_u16.to_le_bytes());
        gps.extend_from_slice(&gps_entry(0x0001, 2, 2, inline(*b"N\0")));
        gps.extend_from_slice(&gps_entry(0x0002, 5, 3, lat_offset.to_le_bytes()));
        gps.extend_from_slice(&gps_entry(0x0003, 2, 2, inline(*b"E\0")));
        gps.extend_from_slice(&gps_entry(0x0004, 5, 3, lon_offset.to_le_bytes()));
        gps.extend_from_slice(&0_u32.to_le_bytes());
        for (numerator, denominator) in [(35_u32, 1_u32), (40, 1), (30, 1)] {
            gps.extend_from_slice(&numerator.to_le_bytes());
            gps.extend_from_slice(&denominator.to_le_bytes());
        }
        for (numerator, denominator) in [(139_u32, 1_u32), (39, 1), (0, 1)] {
            gps.extend_from_slice(&numerator.to_le_bytes());
            gps.extend_from_slice(&denominator.to_le_bytes());
        }
        data.extend_from_slice(&gps);
        cursor += u32::try_from(gps.len()).expect("small fixture");
        Some(offset)
    } else {
        None
    };
    let _ = cursor;

    let mut entries = vec![
        exif_entry(
            0x010F,
            2,
            u32::try_from(make.len()).expect("len"),
            make_offset,
        ),
        exif_entry(
            0x0110,
            2,
            u32::try_from(model.len()).expect("len"),
            model_offset,
        ),
        exif_entry(0x0112, 3, 1, 1),
        exif_entry(
            0x0132,
            2,
            u32::try_from(datetime.len()).expect("len"),
            datetime_offset,
        ),
        exif_entry(0x829A, 5, 1, exposure_offset),
        exif_entry(0x829D, 5, 1, f_number_offset),
        exif_entry(0x8825, 4, 1, gps_ifd_offset.unwrap_or(0)),
        exif_entry(0x8827, 3, 1, fixture.iso),
        exif_entry(0x920A, 5, 1, focal_offset),
    ];
    entries.sort_by_key(|entry| u16::from_le_bytes([entry[0], entry[1]]));
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II\x2A\x00\x08\x00\x00\x00");
    tiff.extend_from_slice(&u16::try_from(entries.len()).expect("count").to_le_bytes());
    for entry in entries {
        tiff.extend_from_slice(&entry);
    }
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    tiff.extend_from_slice(&data);

    let mut payload = b"Exif\x00\x00".to_vec();
    payload.extend_from_slice(&tiff);
    let mut segment = vec![0xFF, 0xE1];
    let length = u16::try_from(payload.len() + 2).expect("small fixture");
    segment.extend_from_slice(&length.to_be_bytes());
    segment.extend_from_slice(&payload);
    jpeg.splice(2..2, segment);
    jpeg
}

fn push_data(data: &mut Vec<u8>, cursor: &mut u32, bytes: &[u8]) -> u32 {
    let offset = *cursor;
    data.extend_from_slice(bytes);
    *cursor += u32::try_from(bytes.len()).expect("small fixture");
    offset
}

fn exif_entry(tag: u16, kind: u16, count: u32, value: u32) -> Vec<u8> {
    let mut entry = Vec::with_capacity(12);
    entry.extend_from_slice(&tag.to_le_bytes());
    entry.extend_from_slice(&kind.to_le_bytes());
    entry.extend_from_slice(&count.to_le_bytes());
    entry.extend_from_slice(&value.to_le_bytes());
    entry
}

#[test]
fn extracts_exif_fields_and_gps() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let path = temporary.path().join("exif.jpg");
    let fixture = ExifFixture {
        make: "Picker",
        model: "TestCam",
        datetime: "2024:05:06 07:08:09",
        iso: 200,
        gps: true,
    };
    std::fs::write(&path, jpeg_with_exif(&test_image(9, 4), &fixture)).expect("write fixture");
    let metadata = extract(
        &RustFallbackDecoder,
        &path,
        ImageFormat::Jpeg,
        &DecodeLimits::default(),
    )
    .expect("extract metadata");
    assert_eq!((metadata.width, metadata.height), (9, 4));
    assert_eq!(metadata.format.as_deref(), Some("jpeg"));
    assert_eq!(metadata.camera_make.as_deref(), Some("Picker"));
    assert_eq!(metadata.camera_model.as_deref(), Some("TestCam"));
    assert_eq!(metadata.captured_at, Some(1_714_979_289));
    assert_eq!(metadata.iso, Some(200));
    assert_eq!(metadata.f_number, Some(2.8));
    assert_eq!(metadata.focal_length_mm, Some(35.0));
    assert_eq!(metadata.exposure_time.as_deref(), Some("1/125"));
    let gps = metadata.gps.expect("gps coordinates");
    assert!((gps.latitude - 35.675).abs() < 1e-9);
    assert!((gps.longitude - 139.65).abs() < 1e-9);
}

#[test]
fn missing_or_broken_exif_is_not_fatal() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let plain = temporary.path().join("plain.png");
    std::fs::write(&plain, encode(&test_image(5, 3), ImageFormat::Png)).expect("fixture");
    let metadata = extract(
        &RustFallbackDecoder,
        &plain,
        ImageFormat::Png,
        &DecodeLimits::default(),
    )
    .expect("metadata without exif");
    assert_eq!((metadata.width, metadata.height), (5, 3));
    assert!(metadata.captured_at.is_none());
    assert!(metadata.camera_make.is_none());

    let broken = temporary.path().join("broken-exif.jpg");
    let mut bytes = encode(&test_image(5, 3), ImageFormat::Jpeg);
    let payload = b"Exif\x00\x00not-a-tiff";
    let mut segment = vec![0xFF, 0xE1];
    segment.extend_from_slice(
        &u16::try_from(payload.len() + 2)
            .expect("small fixture")
            .to_be_bytes(),
    );
    segment.extend_from_slice(payload);
    bytes.splice(2..2, segment);
    std::fs::write(&broken, bytes).expect("fixture");
    let metadata = extract(
        &RustFallbackDecoder,
        &broken,
        ImageFormat::Jpeg,
        &DecodeLimits::default(),
    )
    .expect("broken exif must still decode");
    assert!(metadata.camera_make.is_none());
}

#[test]
fn parses_and_validates_exif_datetime() {
    assert_eq!(
        parse_exif_datetime("2024:05:06 07:08:09"),
        Some(1_714_979_289)
    );
    assert_eq!(parse_exif_datetime("not a date"), None);
    assert_eq!(parse_exif_datetime("2024:13:40 25:00:00"), None);
}

#[test]
fn perceptual_hash_matches_resized_and_recompressed_images() {
    let source = test_image(320, 240);
    let baseline = dhash(
        &RustFallbackDecoder
            .decode_bytes(
                ImageFormat::Png,
                &encode(&source, ImageFormat::Png),
                &DecodeLimits::default(),
            )
            .expect("decode baseline"),
    );

    let resized = source.resize_exact(160, 120, image::imageops::FilterType::Triangle);
    let transformed = dhash(
        &RustFallbackDecoder
            .decode_bytes(
                ImageFormat::Jpeg,
                &encode(&resized, ImageFormat::Jpeg),
                &DecodeLimits::default(),
            )
            .expect("decode transformed"),
    );
    assert!(
        is_near_duplicate(&baseline, &transformed, DEFAULT_NEAR_DUPLICATE_DISTANCE),
        "resized/recompressed distance was {}",
        hamming_distance(&baseline, &transformed)
    );

    let mut other = RgbaImage::new(320, 240);
    for (x, _y, pixel) in other.enumerate_pixels_mut() {
        let value = if (x / 40) % 2 == 0 { 250 } else { 5 };
        *pixel = Rgba([value, value, value, 255]);
    }
    let unrelated = dhash(
        &RustFallbackDecoder
            .decode_bytes(
                ImageFormat::Png,
                &encode(&DynamicImage::ImageRgba8(other), ImageFormat::Png),
                &DecodeLimits::default(),
            )
            .expect("decode unrelated"),
    );
    assert!(
        !is_near_duplicate(&baseline, &unrelated, DEFAULT_NEAR_DUPLICATE_DISTANCE),
        "unrelated image distance was {}",
        hamming_distance(&baseline, &unrelated)
    );
}

fn solid(width: u32, height: u32, pixel: [u8; 4]) -> DecodedImage {
    DecodedImage {
        width,
        height,
        rgba: pixel.repeat((width * height) as usize),
    }
}

fn rgb(hex: &str) -> [i32; 3] {
    let value = u32::from_str_radix(&hex[1..], 16).expect("hex");
    [
        ((value >> 16) & 0xFF) as i32,
        ((value >> 8) & 0xFF) as i32,
        (value & 0xFF) as i32,
    ]
}

fn close(actual: &str, expected: &str) -> bool {
    let actual = rgb(actual);
    let expected = rgb(expected);
    (0..3).all(|channel| (actual[channel] - expected[channel]).abs() <= 6)
}

#[test]
fn extracts_one_color_from_solid_images() {
    let colors = extract_colors(&solid(64, 64, [220, 30, 40, 255]));
    assert_eq!(colors.len(), 1);
    assert!(close(&colors[0].hex, "#DC1E28"), "{:?}", colors);
    assert!((colors[0].ratio - 1.0).abs() < f64::EPSILON);
}

#[test]
fn extracts_two_and_three_colors_in_area_order() {
    let mut image = DecodedImage {
        width: 60,
        height: 40,
        rgba: vec![0; 60 * 40 * 4],
    };
    for y in 0..40 {
        for x in 0..60 {
            let offset = ((y * 60 + x) * 4) as usize;
            let color = if x < 30 {
                [220, 30, 40, 255]
            } else {
                [30, 60, 220, 255]
            };
            image.rgba[offset..offset + 4].copy_from_slice(&color);
        }
    }
    let colors = extract_colors(&image);
    assert_eq!(colors.len(), 2);
    assert!(
        colors.iter().any(|color| close(&color.hex, "#DC1E28")),
        "{colors:?}"
    );
    assert!(
        colors.iter().any(|color| close(&color.hex, "#1E3CDC")),
        "{colors:?}"
    );
    assert!((colors[0].ratio - 0.5).abs() < 0.05);

    for y in 0..30 {
        for x in 0..60 {
            let offset = ((y * 60 + x) * 4) as usize;
            let color = match x / 20 {
                0 => [220, 30, 40, 255],
                1 => [30, 200, 60, 255],
                _ => [30, 60, 220, 255],
            };
            image.rgba[offset..offset + 4].copy_from_slice(&color);
        }
    }
    let colors = extract_colors(&image);
    assert_eq!(colors.len(), 3, "{colors:?}");
}

#[test]
fn transparent_tiny_and_low_contrast_images_are_safe() {
    assert!(extract_colors(&solid(8, 8, [255, 0, 0, 0])).is_empty());
    assert!(extract_colors(&solid(1, 1, [0, 0, 0, 0])).is_empty());
    let tiny = extract_colors(&solid(1, 1, [12, 200, 90, 255]));
    assert_eq!(tiny.len(), 1);
    assert!(close(&tiny[0].hex, "#0CC85A"), "{tiny:?}");

    let mut image = DecodedImage {
        width: 32,
        height: 32,
        rgba: Vec::new(),
    };
    for index in 0..32 * 32 {
        let value = if index % 2 == 0 { 192 } else { 200 };
        image.rgba.extend_from_slice(&[value, 10, 10, 255]);
    }
    let merged = extract_colors(&image);
    assert!(!merged.is_empty() && merged.len() <= 2, "{merged:?}");
}

/// T1-6 on macOS: HEIC files decode through ImageIO, also when several with
/// the same file name are decoded at once by parallel workers.
#[cfg(target_os = "macos")]
#[test]
fn heic_decodes_through_imageio_in_parallel() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let mut sources = Vec::new();
    for (index, (width, height)) in [(64, 32), (40, 80), (50, 50), (90, 30)]
        .into_iter()
        .enumerate()
    {
        let folder = temporary.path().join(format!("folder-{index}"));
        std::fs::create_dir_all(&folder).expect("folder");
        let png = folder.join("source.png");
        std::fs::write(&png, encode(&test_image(width, height), ImageFormat::Png)).expect("png");
        // Every HEIC has the same file name, as photos from two cameras can.
        let heic = folder.join("IMG_0001.HEIC");
        let status = std::process::Command::new("sips")
            .args(["-s", "format", "heic"])
            .arg(&png)
            .arg("--out")
            .arg(&heic)
            .output()
            .expect("run sips");
        assert!(status.status.success(), "sips could not write HEIC");
        sources.push((heic, width, height));
    }
    let handles: Vec<_> = sources
        .into_iter()
        .map(|(heic, width, height)| {
            std::thread::spawn(move || {
                let decoded = PlatformDecoder
                    .decode_file(&heic, ImageFormat::Heic, &DecodeLimits::default())
                    .expect("decode heic");
                assert_eq!((decoded.width, decoded.height), (width, height));
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("decoder thread");
    }

    let broken = temporary.path().join("broken.heic");
    std::fs::write(&broken, b"not a heic file").expect("broken");
    assert!(PlatformDecoder
        .decode_file(&broken, ImageFormat::Heic, &DecodeLimits::default())
        .is_err());
    let tiny = DecodeLimits {
        max_file_bytes: 1,
        ..DecodeLimits::default()
    };
    let first = temporary.path().join("folder-0/IMG_0001.HEIC");
    assert!(matches!(
        PlatformDecoder.decode_file(&first, ImageFormat::Heic, &tiny),
        Err(ImagingError::TooLarge { .. })
    ));
}
