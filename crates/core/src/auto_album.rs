//! Automatic albums (design 9.5, ADR-006): saved search conditions evaluated
//! each time the album is shown. Membership is never stored.
//!
//! Tags in a saved condition are referenced by namespace and name rather than
//! by id, so the condition keeps its meaning after an export and import.

use picker_store::{Album, AlbumKind, AssetFilter, TagFilter};
use serde::{Deserialize, Serialize};

use crate::library::{AssetPage, Library, LibraryError, Result};
use crate::palette::{ColorCondition, ColorScope};

/// Setting that records that the default albums were created once, so albums
/// the user deleted are not created again.
const DEFAULTS_CREATED_KEY: &str = "auto_albums.defaults_created";
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TagRef {
    pub namespace: String,
    pub canonical_name: String,
}

/// A saved search condition. Every field is optional in JSON so that later
/// versions can add conditions without breaking saved albums.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct AlbumQuery {
    pub tags_all: Vec<TagRef>,
    pub tags_any: Vec<TagRef>,
    pub tags_none: Vec<TagRef>,
    pub include_suggested: bool,
    pub colors: Vec<String>,
    /// Looks at up to three colors instead of the main color only.
    pub colors_any_of_three: bool,
    pub formats: Vec<String>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub captured_from: Option<i64>,
    pub captured_to: Option<i64>,
    pub added_within_days: Option<u32>,
    pub untagged: bool,
    pub has_suggested: bool,
}

impl AlbumQuery {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|error| LibraryError::InvalidInput(error.to_string()))
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|error| LibraryError::InvalidInput(format!("album query: {error}")))
    }
}

/// The default automatic albums decided in ADR-006.
#[must_use]
pub fn default_auto_albums() -> Vec<(&'static str, AlbumQuery)> {
    vec![
        (
            "最近追加した画像",
            AlbumQuery {
                added_within_days: Some(30),
                ..AlbumQuery::default()
            },
        ),
        (
            "タグのない画像",
            AlbumQuery {
                untagged: true,
                ..AlbumQuery::default()
            },
        ),
        (
            "未確認の自動タグあり",
            AlbumQuery {
                has_suggested: true,
                ..AlbumQuery::default()
            },
        ),
    ]
}

impl Library {
    pub fn create_auto_album(&self, name: &str, query: &AlbumQuery) -> Result<Album> {
        self.validate_query(query)?;
        Ok(self
            .store()
            .albums()
            .create_auto(name, &query.to_json()?, 0)?)
    }

    pub fn set_auto_album_query(&self, album_id: i64, query: &AlbumQuery) -> Result<Album> {
        self.validate_query(query)?;
        Ok(self
            .store()
            .albums()
            .set_query(album_id, &query.to_json()?)?)
    }

    /// Returns the saved condition of an automatic album.
    pub fn auto_album_query(&self, album_id: i64) -> Result<AlbumQuery> {
        let album = self.store().albums().get(album_id)?;
        match (album.kind, album.query) {
            (AlbumKind::Auto, Some(query)) => AlbumQuery::from_json(&query),
            _ => Err(LibraryError::InvalidInput(
                "album is not automatic".to_owned(),
            )),
        }
    }

    /// Evaluates an automatic album at `now` (Unix seconds).
    pub fn auto_album_page(
        &self,
        album_id: i64,
        now: i64,
        after_id: i64,
        limit: u32,
    ) -> Result<AssetPage> {
        let filter = self.query_filter(&self.auto_album_query(album_id)?, now)?;
        self.filter_page(&filter, after_id, limit)
    }

    /// Creates the default automatic albums the first time only.
    pub fn ensure_default_auto_albums(&self) -> Result<usize> {
        if self.get_setting(DEFAULTS_CREATED_KEY)?.is_some() {
            return Ok(0);
        }
        let defaults = default_auto_albums();
        for (name, query) in &defaults {
            if self.store().albums().find_auto_by_name(name)?.is_none() {
                self.create_auto_album(name, query)?;
            }
        }
        self.set_setting(DEFAULTS_CREATED_KEY, "1")?;
        Ok(defaults.len())
    }

    /// Builds the store filter for a saved condition. A missing tag in
    /// `tags_all` matches nothing; missing tags elsewhere are ignored.
    pub fn query_filter(&self, query: &AlbumQuery, now: i64) -> Result<AssetFilter> {
        let resolve = |refs: &[TagRef]| -> Result<(Vec<i64>, bool)> {
            let mut ids = Vec::with_capacity(refs.len());
            let mut missing = false;
            for tag in refs {
                match self
                    .store()
                    .tags()
                    .find(&tag.namespace, &tag.canonical_name)?
                {
                    Some(found) => ids.push(found.id),
                    None => missing = true,
                }
            }
            Ok((ids, missing))
        };
        let (all, all_missing) = resolve(&query.tags_all)?;
        let (any, _) = resolve(&query.tags_any)?;
        let (none, _) = resolve(&query.tags_none)?;
        let mut filter = AssetFilter {
            tags: TagFilter {
                all,
                any,
                none,
                include_suggested: query.include_suggested,
            },
            formats: query.formats.clone(),
            min_width: query.min_width,
            min_height: query.min_height,
            captured_from: query.captured_from,
            captured_to: query.captured_to,
            asset_ids: all_missing.then(Vec::new),
            added_since: query
                .added_within_days
                .map(|days| now - i64::from(days) * SECONDS_PER_DAY),
            untagged: query.untagged,
            has_suggested: query.has_suggested,
        };
        self.restrict_to_colors(
            &mut filter,
            &ColorCondition {
                palette_ids: query.colors.clone(),
                scope: if query.colors_any_of_three {
                    ColorScope::AnyOfThree
                } else {
                    ColorScope::Main
                },
            },
        )?;
        Ok(filter)
    }

    fn validate_query(&self, query: &AlbumQuery) -> Result<()> {
        // Resolving the filter checks palette ids without writing anything.
        self.query_filter(query, 0).map(|_| ())
    }

    /// Returns the tag reference of a tag id, for building saved conditions.
    pub fn tag_ref(&self, tag_id: i64) -> Result<TagRef> {
        let tag = self.store().tags().get(tag_id)?;
        Ok(TagRef {
            namespace: tag.namespace,
            canonical_name: tag.canonical_name,
        })
    }
}
