//! Removing regenerable caches (design 6.3, ui-contract 3.7, T3-8).
//!
//! Only data that the workers can produce again is removed: thumbnails and
//! embeddings. Tags, albums, colors set by the user, and feedback stay.

use std::fs;

use picker_store::{LocationStatus, Stage, StageTarget};

use crate::library::{Library, LibraryError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheTarget {
    Thumbnails,
    Embeddings,
    All,
}

impl CacheTarget {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "thumbnails" => Ok(Self::Thumbnails),
            "embeddings" => Ok(Self::Embeddings),
            "all" => Ok(Self::All),
            _ => Err(LibraryError::InvalidInput(
                "cache target must be thumbnails, embeddings, or all".to_owned(),
            )),
        }
    }

    const fn includes_thumbnails(self) -> bool {
        matches!(self, Self::Thumbnails | Self::All)
    }

    const fn includes_embeddings(self) -> bool {
        matches!(self, Self::Embeddings | Self::All)
    }
}

/// What a cache removal queued for regeneration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheClearReport {
    pub image_jobs: usize,
    pub embedding_jobs: usize,
}

impl Library {
    /// Removes the chosen caches and queues their regeneration.
    pub fn clear_cache(&self, target: CacheTarget) -> Result<CacheClearReport> {
        let mut report = CacheClearReport::default();
        if target.includes_thumbnails() {
            self.remove_thumbnail_files()?;
            for root in self.store().roots().list()? {
                report.image_jobs += self.enqueue_unprocessed(root.id)?;
            }
        }
        if target.includes_embeddings() {
            self.store().embeddings().delete_all()?;
            self.store().stage_states().clear_stage(Stage::Embedding)?;
            self.clear_vector_index();
            report.embedding_jobs = self.enqueue_all_embeddings()?;
        }
        Ok(report)
    }

    /// Deletes the size directories under the cache root. Anything else in
    /// the directory is left alone.
    fn remove_thumbnail_files(&self) -> Result<()> {
        let entries = match fs::read_dir(self.cache_root()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            let is_size_dir = entry.file_type()?.is_dir()
                && entry.file_name().to_str().is_some_and(|name| {
                    !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit())
                });
            if is_size_dir {
                fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    }

    /// Queues an embedding job for every asset with a present file.
    pub(crate) fn enqueue_all_embeddings(&self) -> Result<usize> {
        let mut queued = std::collections::HashSet::new();
        for root in self.store().roots().list()? {
            for location in self.store().locations().list_for_root(root.id)? {
                if location.status == LocationStatus::Present && queued.insert(location.asset_id) {
                    self.store().jobs().enqueue(
                        StageTarget::Asset(location.asset_id),
                        Stage::Embedding,
                        0,
                        3,
                    )?;
                }
            }
        }
        Ok(queued.len())
    }
}
