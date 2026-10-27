use std::{fmt, io};

/// Errors returned by image decoding and thumbnail generation.
#[derive(Debug)]
pub enum ImagingError {
    /// The file could not be read or written.
    Io(io::Error),
    /// The file extension or content is not a supported image format.
    UnsupportedFormat(String),
    /// The file or decoded image exceeds the configured safety limits.
    TooLarge { limit: &'static str, actual: u64 },
    /// The image data could not be decoded.
    Decode(String),
    /// A thumbnail could not be generated.
    Thumbnail(String),
}

impl fmt::Display for ImagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::UnsupportedFormat(format) => {
                write!(formatter, "unsupported image format: {format}")
            }
            Self::TooLarge { limit, actual } => {
                write!(formatter, "{limit} limit exceeded: {actual}")
            }
            Self::Decode(message) => write!(formatter, "image decode failed: {message}"),
            Self::Thumbnail(message) => write!(formatter, "thumbnail generation failed: {message}"),
        }
    }
}

impl std::error::Error for ImagingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ImagingError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<image::ImageError> for ImagingError {
    fn from(error: image::ImageError) -> Self {
        Self::Decode(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ImagingError>;
