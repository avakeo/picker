use std::{fmt, fs, path::Path, time::UNIX_EPOCH};

use crate::{DiscoveredFile, ScanError, Scanner};
use picker_store::{NewAsset, NewLocation, Stage, StageTarget, Store};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IdentityReport {
    pub added: usize,
    pub unchanged: usize,
    pub updated: usize,
    pub missing: usize,
    pub jobs: usize,
}

#[derive(Debug)]
pub enum IdentityError {
    Scan(ScanError),
    Store(picker_store::StoreError),
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    InvalidTimestamp,
}
impl fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scan(error) => error.fmt(formatter),
            Self::Store(error) => error.fmt(formatter),
            Self::Io { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::InvalidTimestamp => {
                formatter.write_str("file modification time is before the Unix epoch")
            }
        }
    }
}
impl std::error::Error for IdentityError {}
impl From<ScanError> for IdentityError {
    fn from(value: ScanError) -> Self {
        Self::Scan(value)
    }
}
impl From<picker_store::StoreError> for IdentityError {
    fn from(value: picker_store::StoreError) -> Self {
        Self::Store(value)
    }
}

pub struct IdentityResolver<'store> {
    store: &'store Store,
    scanner: Scanner,
}
impl<'store> IdentityResolver<'store> {
    #[must_use]
    pub const fn new(store: &'store Store) -> Self {
        Self {
            store,
            scanner: Scanner,
        }
    }
    pub fn sync_root(
        &self,
        root_id: i64,
        root_path: impl AsRef<Path>,
    ) -> std::result::Result<IdentityReport, IdentityError> {
        let files = self.scanner.scan(root_path)?;
        let mut report = IdentityReport::default();
        let mut seen = Vec::with_capacity(files.len());
        for file in files {
            seen.push(file.relative_path.clone());
            let modified_at = unix_seconds(&file)?;
            if let Some(existing) = self
                .store
                .locations()
                .find_by_root_and_path(root_id, &file.relative_path)?
            {
                let previous = self.store.assets().get(existing.asset_id)?;
                if existing.modified_at == modified_at
                    && previous.file_size == file.file_size
                    && existing.status == picker_store::LocationStatus::Present
                {
                    report.unchanged += 1;
                    continue;
                }
                let asset_id = self.asset_for(&file)?;
                self.store.locations().upsert(&NewLocation {
                    asset_id,
                    root_id,
                    relative_path: &file.relative_path,
                    modified_at,
                    status: picker_store::LocationStatus::Present,
                })?;
                self.store
                    .jobs()
                    .enqueue(StageTarget::Asset(asset_id), Stage::Metadata, 0, 3)?;
                report.updated += 1;
                report.jobs += 1;
            } else {
                let asset_id = self.asset_for(&file)?;
                self.store.locations().upsert(&NewLocation {
                    asset_id,
                    root_id,
                    relative_path: &file.relative_path,
                    modified_at,
                    status: picker_store::LocationStatus::Present,
                })?;
                self.store
                    .jobs()
                    .enqueue(StageTarget::Asset(asset_id), Stage::Metadata, 0, 3)?;
                report.added += 1;
                report.jobs += 1;
            }
        }
        report.missing = self.store.locations().mark_missing_except(root_id, &seen)?;
        Ok(report)
    }
    fn asset_for(&self, file: &DiscoveredFile) -> std::result::Result<i64, IdentityError> {
        let bytes = fs::read(&file.absolute_path).map_err(|source| IdentityError::Io {
            path: file.absolute_path.clone(),
            source,
        })?;
        let hash = blake3::hash(&bytes);
        if let Some(asset) = self.store.assets().find_by_hash(hash.as_bytes())? {
            return Ok(asset.id);
        }
        Ok(self
            .store
            .assets()
            .insert(&NewAsset {
                blake3_hash: hash.as_bytes(),
                file_size: file.file_size,
                width: None,
                height: None,
                format: file
                    .absolute_path
                    .extension()
                    .and_then(|extension| extension.to_str()),
                captured_at: None,
                perceptual_hash: None,
            })?
            .id)
    }
}

fn unix_seconds(file: &DiscoveredFile) -> std::result::Result<i64, IdentityError> {
    file.modified_at
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IdentityError::InvalidTimestamp)
        .and_then(|duration| {
            i64::try_from(duration.as_secs()).map_err(|_| IdentityError::InvalidTimestamp)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};
    use tempfile::TempDir;
    #[test]
    fn identity_tracks_duplicates_changes_and_missing_files() -> picker_store::Result<()> {
        let root = TempDir::new().expect("root");
        fs::create_dir_all(root.path().join("nested")).expect("nested");
        fs::File::create(root.path().join("nested/one.jpg"))
            .expect("file")
            .write_all(b"same")
            .expect("data");
        fs::File::create(root.path().join("two.jpg"))
            .expect("file")
            .write_all(b"same")
            .expect("data");
        let store = Store::open_in_memory()?;
        let registered = store.roots().insert(&picker_store::NewRoot {
            absolute_path: root.path().to_str().expect("path"),
            volume_id: None,
            display_name: "root",
            enabled: true,
            status: picker_store::RootStatus::Online,
        })?;
        let resolver = IdentityResolver::new(&store);
        let first = resolver
            .sync_root(registered.id, root.path())
            .expect("sync");
        assert_eq!((first.added, first.jobs), (2, 2));
        let second = resolver
            .sync_root(registered.id, root.path())
            .expect("sync");
        assert_eq!(second.unchanged, 2);
        fs::remove_file(root.path().join("two.jpg")).expect("remove");
        let third = resolver
            .sync_root(registered.id, root.path())
            .expect("sync");
        assert_eq!(third.missing, 1);
        Ok(())
    }
}
