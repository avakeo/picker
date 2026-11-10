use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    decode::{fit_within, thumbnail_path, DecodedImage, Decoder},
    format::ImageFormat,
    limits::DecodeLimits,
    ImagingError, Result,
};

/// Thumbnail size classes exposed by the UI contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailSize {
    Grid,
    Detail,
}

impl ThumbnailSize {
    /// Maximum edge length for the size class.
    #[must_use]
    pub const fn max_edge(self) -> u32 {
        match self {
            Self::Grid => 512,
            Self::Detail => 2048,
        }
    }

    /// Stable identifier used in cache paths.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Detail => "detail",
        }
    }
}

/// Persistent thumbnail cache rooted in the application data directory.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    root: PathBuf,
}

impl ThumbnailCache {
    /// Creates a cache rooted at the given directory.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the cache path for an asset without checking whether it exists.
    #[must_use]
    pub fn path(&self, asset_id: i64, size: ThumbnailSize) -> PathBuf {
        thumbnail_path(&self.root, asset_id, size.max_edge())
    }

    /// Generates both thumbnail sizes for an asset and returns their paths.
    pub fn generate<D: Decoder>(
        &self,
        decoder: &D,
        asset_id: i64,
        source: &Path,
        format: ImageFormat,
        limits: &DecodeLimits,
    ) -> Result<Vec<PathBuf>> {
        let decoded =
            decoder.decode_scaled(source, format, limits, ThumbnailSize::Detail.max_edge())?;
        self.store(asset_id, &decoded)
    }

    /// Writes both thumbnail sizes from an already decoded image, which
    /// should be at least as large as the detail size.
    pub fn store(&self, asset_id: i64, decoded: &DecodedImage) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::with_capacity(2);
        for size in [ThumbnailSize::Grid, ThumbnailSize::Detail] {
            paths.push(self.write(asset_id, size, decoded)?);
        }
        Ok(paths)
    }

    /// Returns an existing thumbnail or generates it from the source file.
    pub fn get_or_generate<D: Decoder>(
        &self,
        decoder: &D,
        asset_id: i64,
        size: ThumbnailSize,
        source: &Path,
        format: ImageFormat,
        limits: &DecodeLimits,
    ) -> Result<PathBuf> {
        let path = self.path(asset_id, size);
        if path.is_file() {
            return Ok(path);
        }
        let decoded = decoder.decode_scaled(source, format, limits, size.max_edge())?;
        self.write(asset_id, size, &decoded)
    }

    fn write(&self, asset_id: i64, size: ThumbnailSize, decoded: &DecodedImage) -> Result<PathBuf> {
        let path = self.path(asset_id, size);
        let parent = path
            .parent()
            .ok_or_else(|| ImagingError::Thumbnail("cache path has no parent".to_owned()))?;
        fs::create_dir_all(parent)?;
        let thumbnail = scale(decoded, size.max_edge())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| ImagingError::Thumbnail(error.to_string()))?
            .as_nanos();
        let temporary = parent.join(format!(".{asset_id}-{stamp}.tmp.png"));
        thumbnail
            .save_with_format(&temporary, image::ImageFormat::Png)
            .map_err(|error| ImagingError::Thumbnail(error.to_string()))?;
        fs::rename(&temporary, &path)?;
        Ok(path)
    }
}

fn scale(decoded: &DecodedImage, max_edge: u32) -> Result<image::RgbaImage> {
    let source = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba.clone())
        .ok_or_else(|| ImagingError::Thumbnail("invalid RGBA buffer length".to_owned()))?;
    let (width, height) = fit_within(decoded.width, decoded.height, max_edge);
    if (width, height) == (decoded.width, decoded.height) {
        return Ok(source);
    }
    Ok(image::imageops::thumbnail(&source, width, height))
}
