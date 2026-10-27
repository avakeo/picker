use std::path::Path;

/// Image formats supported by the MVP decoder boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Heic,
    Webp,
}

impl ImageFormat {
    /// Detects the format from the file extension.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "heic" | "heif" => Some(Self::Heic),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }

    /// Returns the format detected from a standard file signature.
    #[must_use]
    pub fn from_magic(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(Self::Jpeg);
        }
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(Self::Png);
        }
        if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            return Some(Self::Webp);
        }
        if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
            let brand = &bytes[8..12];
            if brand == b"heic" || brand == b"heix" || brand == b"hevc" || brand == b"hevx" {
                return Some(Self::Heic);
            }
        }
        None
    }

    /// Returns the stable identifier used by the database and UI contract.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Heic => "heic",
            Self::Webp => "webp",
        }
    }

    pub(crate) fn image_crate_format(self) -> Option<image::ImageFormat> {
        match self {
            Self::Jpeg => Some(image::ImageFormat::Jpeg),
            Self::Png => Some(image::ImageFormat::Png),
            Self::Webp => Some(image::ImageFormat::WebP),
            Self::Heic => None,
        }
    }
}
