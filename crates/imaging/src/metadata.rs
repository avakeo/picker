use std::{fs::File, io::BufReader, path::Path};

use exif::{Tag, Value};

use crate::{decode::Decoder, format::ImageFormat, limits::DecodeLimits, Result};

/// EXIF information extracted from an image without making parse failures fatal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub format: Option<String>,
    pub captured_at: Option<i64>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub iso: Option<u32>,
    pub focal_length_mm: Option<f64>,
    pub gps: Option<GeoPoint>,
}

/// WGS84 coordinates stored with the image metadata.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
}

/// Extracts dimensions and EXIF fields from an image.
///
/// A missing or malformed EXIF block yields `None` for the affected fields and
/// never fails the whole extraction. Decode failures are still returned as
/// typed errors because the image itself is unusable.
pub fn extract<D: Decoder>(
    decoder: &D,
    path: &Path,
    format: ImageFormat,
    limits: &DecodeLimits,
) -> Result<ImageMetadata> {
    let decoded = decoder.decode_file(path, format, limits)?;
    Ok(ImageMetadata {
        width: decoded.width,
        height: decoded.height,
        ..extract_exif(path, format)
    })
}

/// Reads the EXIF fields of an image without decoding its pixels. The
/// dimensions are left at zero.
///
/// A missing or malformed EXIF block yields `None` for the affected fields.
#[must_use]
pub fn extract_exif(path: &Path, format: ImageFormat) -> ImageMetadata {
    let mut metadata = ImageMetadata {
        format: Some(format.as_str().to_owned()),
        ..ImageMetadata::default()
    };

    let Ok(file) = File::open(path) else {
        return metadata;
    };
    let mut reader = BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return metadata;
    };

    metadata.captured_at = exif_datetime(&exif);
    metadata.camera_make = ascii_field(&exif, Tag::Make);
    metadata.camera_model = ascii_field(&exif, Tag::Model);
    metadata.lens = ascii_field(&exif, Tag::LensModel);
    metadata.exposure_time =
        find_field(&exif, Tag::ExposureTime).map(|field| field.display_value().to_string());
    metadata.f_number = rational_field(&exif, Tag::FNumber);
    metadata.iso =
        find_field(&exif, Tag::PhotographicSensitivity).and_then(|field| field.value.get_uint(0));
    metadata.focal_length_mm = rational_field(&exif, Tag::FocalLength);
    metadata.gps = gps_coordinates(&exif);
    metadata
}

fn find_field(exif: &exif::Exif, tag: Tag) -> Option<&exif::Field> {
    // Some tags appear in the Exif sub-IFD in real files but in IFD0 in
    // synthetic or non-conforming files. Match on the numeric tag so both are
    // read without weakening validation of the value itself.
    exif.fields().find(|field| field.tag.1 == tag.1)
}

fn ascii_field(exif: &exif::Exif, tag: Tag) -> Option<String> {
    find_field(exif, tag)
        .map(|field| field.display_value().to_string())
        .map(|value| value.trim_matches('"').trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn rational_field(exif: &exif::Exif, tag: Tag) -> Option<f64> {
    match &find_field(exif, tag)?.value {
        Value::Rational(values) => values.first().map(|value| value.to_f64()),
        _ => None,
    }
}

fn exif_datetime(exif: &exif::Exif) -> Option<i64> {
    for tag in [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime] {
        let Some(field) = find_field(exif, tag) else {
            continue;
        };
        let value = field.display_value().to_string();
        if let Some(timestamp) = parse_exif_datetime(value.trim_matches('"')) {
            return Some(timestamp);
        }
    }
    None
}

/// Parses the EXIF `YYYY:MM:DD HH:MM:SS` convention into a Unix timestamp.
#[must_use]
pub fn parse_exif_datetime(value: &str) -> Option<i64> {
    let mut parts = value.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let mut date_parts = date.split([':', '-']);
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
}

// Howard Hinnant's civil-days algorithm, relative to 1970-01-01.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = i64::from(year_of_era) * 365 + i64::from(year_of_era / 4)
        - i64::from(year_of_era / 100)
        + day_of_year;
    i64::from(era) * 146_097 + day_of_era - 719_468
}

fn gps_coordinates(exif: &exif::Exif) -> Option<GeoPoint> {
    let latitude = gps_coordinate(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, "S")?;
    let longitude = gps_coordinate(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, "W")?;
    Some(GeoPoint {
        latitude,
        longitude,
    })
}

fn gps_coordinate(exif: &exif::Exif, value_tag: Tag, ref_tag: Tag, negative: &str) -> Option<f64> {
    let Value::Rational(parts) = &find_field(exif, value_tag)?.value else {
        return None;
    };
    if parts.len() < 3 {
        return None;
    }
    let mut value = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
    if find_field(exif, ref_tag)
        .map(|field| field.display_value().to_string())
        .is_some_and(|reference| reference.contains(negative))
    {
        value = -value;
    }
    Some(value)
}
