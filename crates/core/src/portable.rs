//! Portable export of the non-regenerable data other than tags (design 6.3):
//! roots, manual albums, user-set colors, and feedback history.
//!
//! Records keyed by an asset hash that is not in the library yet are staged
//! and applied when a scan registers the asset, like tag assignments.

use std::collections::BTreeMap;
use std::path::Path;

use picker_store::{FeedbackEvent, PendingItem, PendingItemKind};
use serde::{Deserialize, Serialize};

use crate::library::{hash_hex, parse_hash, Library, LibraryError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableRoot {
    pub absolute_path: String,
    pub display_name: String,
    pub enabled: bool,
}

/// A manual album with its items as asset hashes in display order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableAlbum {
    pub name: String,
    pub sort_order: i64,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableColor {
    pub hex: String,
    pub ratio: f64,
}

/// Colors the user set for one asset. An empty list means "no colors".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableColors {
    pub blake3_hash: String,
    pub colors: Vec<PortableColor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PortableFeedback {
    pub blake3_hash: String,
    pub namespace: Option<String>,
    pub canonical_name: Option<String>,
    pub action: String,
    pub confidence: Option<f64>,
    pub previous_state: Option<String>,
    pub previous_source: Option<String>,
    pub previous_confidence: Option<f64>,
    pub previous_model_id: Option<String>,
    pub previous_model_version: Option<String>,
    pub created_at: i64,
}

/// Staged form of one album item whose asset is not registered yet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingAlbumItem {
    album_name: String,
    position: i64,
}

/// The parts of an export written by this module.
#[derive(Debug, Default)]
pub(crate) struct PortableExtras {
    pub roots: Vec<PortableRoot>,
    pub albums: Vec<PortableAlbum>,
    pub colors: Vec<PortableColors>,
    pub feedback: Vec<PortableFeedback>,
}

fn invalid(message: &str) -> LibraryError {
    LibraryError::InvalidInput(message.to_owned())
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|error| LibraryError::InvalidInput(error.to_string()))
}

fn from_json<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|error| LibraryError::InvalidInput(error.to_string()))
}

impl Library {
    pub(crate) fn export_extras(&self) -> Result<PortableExtras> {
        let store = self.store();
        let roots = store
            .roots()
            .list()?
            .into_iter()
            .map(|root| PortableRoot {
                absolute_path: root.absolute_path,
                display_name: root.display_name,
                enabled: root.enabled,
            })
            .collect();

        let mut albums = Vec::new();
        for entry in store.albums().list()? {
            let album = entry.album;
            if album.kind != picker_store::AlbumKind::Manual {
                continue;
            }
            let mut items = Vec::new();
            for item in store.albums().list_items(album.id)? {
                items.push(hash_hex(&store.assets().get(item.asset_id)?.blake3_hash));
            }
            albums.push(PortableAlbum {
                name: album.name,
                sort_order: album.sort_order,
                items,
            });
        }

        let mut colors = Vec::new();
        for asset_id in store.colors().manual_assets()? {
            colors.push(PortableColors {
                blake3_hash: hash_hex(&store.assets().get(asset_id)?.blake3_hash),
                colors: store
                    .colors()
                    .list(asset_id)?
                    .into_iter()
                    .map(|color| PortableColor {
                        hex: color.hex,
                        ratio: color.ratio,
                    })
                    .collect(),
            });
        }

        let mut feedback = Vec::new();
        for event in store.feedback().list_all()? {
            let hash = store.assets().get(event.asset_id)?.blake3_hash;
            feedback.push(self.portable_feedback(&hash, &event)?);
        }

        let mut extras = PortableExtras {
            roots,
            albums,
            colors,
            feedback,
        };
        self.append_pending(&mut extras)?;
        Ok(extras)
    }

    /// Copies staged records into the export so a re-export does not lose them.
    fn append_pending(&self, extras: &mut PortableExtras) -> Result<()> {
        let mut album_items: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
        for item in self.store().portable().pending_items()? {
            let hash = hash_hex(&item.blake3_hash);
            match item.kind {
                PendingItemKind::Color => extras.colors.push(from_json(&item.payload)?),
                PendingItemKind::Feedback => extras.feedback.push(from_json(&item.payload)?),
                PendingItemKind::AlbumItem => {
                    let pending: PendingAlbumItem = from_json(&item.payload)?;
                    album_items
                        .entry(pending.album_name)
                        .or_default()
                        .push((pending.position, hash));
                }
            }
        }
        for (name, pending) in album_items {
            let index = match extras.albums.iter().position(|album| album.name == name) {
                Some(index) => index,
                None => {
                    extras.albums.push(PortableAlbum {
                        name,
                        sort_order: 0,
                        items: Vec::new(),
                    });
                    extras.albums.len() - 1
                }
            };
            let album = &mut extras.albums[index];
            // Pending items are in insertion order, which follows the exported order.
            for (position, hash) in pending {
                let at = usize::try_from(position)
                    .unwrap_or(usize::MAX)
                    .min(album.items.len());
                album.items.insert(at, hash);
            }
        }
        Ok(())
    }

    fn portable_feedback(&self, hash: &[u8], event: &FeedbackEvent) -> Result<PortableFeedback> {
        let tag = event
            .tag_id
            .map(|tag_id| self.store().tags().get(tag_id))
            .transpose()?;
        Ok(PortableFeedback {
            blake3_hash: hash_hex(hash),
            namespace: tag.as_ref().map(|tag| tag.namespace.clone()),
            canonical_name: tag.map(|tag| tag.canonical_name),
            action: event.action.as_str().to_owned(),
            confidence: event.confidence,
            previous_state: event.previous_state.map(|state| state.as_str().to_owned()),
            previous_source: event
                .previous_source
                .map(|source| source.as_str().to_owned()),
            previous_confidence: event.previous_confidence,
            previous_model_id: event.previous_model_id.clone(),
            previous_model_version: event.previous_model_version.clone(),
            created_at: event.created_at,
        })
    }

    /// Imports the records of this module. Tags must already be imported.
    pub(crate) fn import_extras(
        &self,
        roots: &[PortableRoot],
        albums: &[PortableAlbum],
        colors: &[PortableColors],
        feedback: &[PortableFeedback],
    ) -> Result<()> {
        // Validate every hash before writing anything.
        for hash in albums
            .iter()
            .flat_map(|album| album.items.iter())
            .chain(colors.iter().map(|entry| &entry.blake3_hash))
            .chain(feedback.iter().map(|event| &event.blake3_hash))
        {
            parse_hash(hash)?;
        }
        self.import_roots(roots)?;
        for album in albums {
            self.import_album(album)?;
        }
        for entry in colors {
            let hash = parse_hash(&entry.blake3_hash)?;
            match self.asset_id_for(&hash)? {
                Some(asset_id) => self.apply_colors(asset_id, entry)?,
                None => self.store().portable().stage_item(
                    &hash,
                    PendingItemKind::Color,
                    &to_json(entry)?,
                )?,
            }
        }
        for event in feedback {
            let hash = parse_hash(&event.blake3_hash)?;
            match self.asset_id_for(&hash)? {
                Some(asset_id) => self.apply_feedback(asset_id, event)?,
                None => self.store().portable().stage_item(
                    &hash,
                    PendingItemKind::Feedback,
                    &to_json(event)?,
                )?,
            }
        }
        Ok(())
    }

    /// Registers exported roots that exist on this machine and are not registered.
    /// Roots that are missing or overlap a registered root are skipped.
    fn import_roots(&self, roots: &[PortableRoot]) -> Result<()> {
        let registered = self.store().roots().list()?;
        for root in roots {
            if registered
                .iter()
                .any(|existing| existing.absolute_path == root.absolute_path)
            {
                continue;
            }
            if !Path::new(&root.absolute_path).is_dir() {
                continue;
            }
            let Ok(added) = self.add_root(&root.absolute_path, &root.display_name) else {
                continue;
            };
            if !root.enabled {
                self.set_root_enabled(added.id, false)?;
            }
        }
        Ok(())
    }

    fn import_album(&self, album: &PortableAlbum) -> Result<()> {
        let albums = self.store().albums();
        let target = match albums.find_manual_by_name(&album.name)? {
            Some(existing) => existing,
            None => albums.create(&album.name, album.sort_order)?,
        };
        for (index, hash) in album.items.iter().enumerate() {
            let hash = parse_hash(hash)?;
            let position = i64::try_from(index).map_err(|_| invalid("album is too large"))?;
            match self.asset_id_for(&hash)? {
                Some(asset_id) => {
                    albums.insert_at(target.id, asset_id, position)?;
                }
                None => self.store().portable().stage_item(
                    &hash,
                    PendingItemKind::AlbumItem,
                    &to_json(&PendingAlbumItem {
                        album_name: target.name.clone(),
                        position,
                    })?,
                )?,
            }
        }
        Ok(())
    }

    fn apply_colors(&self, asset_id: i64, entry: &PortableColors) -> Result<()> {
        let colors: Vec<(String, f64)> = entry
            .colors
            .iter()
            .map(|color| (color.hex.clone(), color.ratio))
            .collect();
        self.store().colors().set_user(asset_id, &colors)?;
        Ok(())
    }

    fn apply_feedback(&self, asset_id: i64, event: &PortableFeedback) -> Result<()> {
        let tag_id = match (&event.namespace, &event.canonical_name) {
            (Some(namespace), Some(canonical)) => self
                .store()
                .tags()
                .find(namespace, canonical)?
                .map(|tag| tag.id),
            _ => None,
        };
        let parse_state = |value: &Option<String>| {
            value
                .as_deref()
                .map(|value| value.parse().map_err(|_| invalid("invalid feedback state")))
                .transpose()
        };
        let parse_source = |value: &Option<String>| {
            value
                .as_deref()
                .map(|value| {
                    value
                        .parse()
                        .map_err(|_| invalid("invalid feedback source"))
                })
                .transpose()
        };
        self.store().feedback().restore(&FeedbackEvent {
            id: 0,
            asset_id,
            tag_id,
            action: event
                .action
                .parse()
                .map_err(|_| invalid("invalid feedback action"))?,
            confidence: event.confidence,
            previous_state: parse_state(&event.previous_state)?,
            previous_source: parse_source(&event.previous_source)?,
            previous_confidence: event.previous_confidence,
            previous_model_id: event.previous_model_id.clone(),
            previous_model_version: event.previous_model_version.clone(),
            created_at: event.created_at,
        })?;
        Ok(())
    }

    /// Applies staged colors, album items, and feedback for a newly scanned asset.
    pub(crate) fn apply_pending_items(&self, asset_id: i64, hash: &[u8]) -> Result<()> {
        for PendingItem { kind, payload, .. } in self.store().portable().take_items(hash)? {
            match kind {
                PendingItemKind::Color => self.apply_colors(asset_id, &from_json(&payload)?)?,
                PendingItemKind::Feedback => {
                    self.apply_feedback(asset_id, &from_json(&payload)?)?;
                }
                PendingItemKind::AlbumItem => {
                    let pending: PendingAlbumItem = from_json(&payload)?;
                    let albums = self.store().albums();
                    let album = match albums.find_manual_by_name(&pending.album_name)? {
                        Some(album) => album,
                        None => albums.create(&pending.album_name, 0)?,
                    };
                    albums.insert_at(album.id, asset_id, pending.position)?;
                }
            }
        }
        Ok(())
    }

    fn asset_id_for(&self, hash: &[u8]) -> Result<Option<i64>> {
        let hash: [u8; 32] = hash
            .try_into()
            .map_err(|_| invalid("invalid BLAKE3 hash"))?;
        Ok(self
            .store()
            .assets()
            .find_by_hash(&hash)?
            .map(|asset| asset.id))
    }
}

const SNAPSHOT_PREFIX: &str = "library-";
const SNAPSHOT_SUFFIX: &str = ".sqlite3";

fn snapshot_time(name: &str) -> Option<i64> {
    name.strip_prefix(SNAPSHOT_PREFIX)?
        .strip_suffix(SNAPSHOT_SUFFIX)?
        .parse()
        .ok()
}

impl Library {
    /// Writes `library-<unix seconds>.sqlite3` into `dir` when the newest
    /// snapshot there is at least `interval_secs` old, then keeps only the
    /// newest `keep` snapshots. Returns the written path, if any.
    pub fn snapshot_if_due(
        &self,
        dir: &Path,
        now: i64,
        interval_secs: i64,
        keep: usize,
    ) -> Result<Option<std::path::PathBuf>> {
        std::fs::create_dir_all(dir)?;
        let mut snapshots: Vec<(i64, std::path::PathBuf)> = std::fs::read_dir(dir)?
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| {
                let time = snapshot_time(entry.file_name().to_str()?)?;
                Some((time, entry.path()))
            })
            .collect();
        snapshots.sort();
        if snapshots
            .last()
            .is_some_and(|(time, _)| now - time < interval_secs)
        {
            return Ok(None);
        }
        let path = dir.join(format!("{SNAPSHOT_PREFIX}{now}{SNAPSHOT_SUFFIX}"));
        self.store().snapshot_to(&path)?;
        snapshots.push((now, path.clone()));
        let excess = snapshots.len().saturating_sub(keep.max(1));
        for (_, old) in snapshots.drain(..excess) {
            std::fs::remove_file(old)?;
        }
        Ok(Some(path))
    }
}
