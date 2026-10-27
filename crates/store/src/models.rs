use std::{fmt, str::FromStr};

use crate::{Result, StoreError};

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl $name {
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $value),+ }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = StoreError;

            fn from_str(value: &str) -> Result<Self> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(StoreError::CorruptData {
                        field: stringify!($name),
                        value: value.to_owned(),
                    }),
                }
            }
        }
    };
}

string_enum!(RootStatus { Online => "online", Offline => "offline" });
string_enum!(LocationStatus {
    Present => "present",
    Missing => "missing",
    Offline => "offline",
});
string_enum!(Stage {
    Hash => "hash",
    Metadata => "metadata",
    Thumbnail => "thumbnail",
    PerceptualHash => "phash",
    Embedding => "embedding",
    AutoTag => "auto_tag",
    CustomSuggest => "custom_suggest",
});

impl Stage {
    #[must_use]
    pub const fn ordered() -> &'static [Self] {
        &[
            Self::Hash,
            Self::Metadata,
            Self::Thumbnail,
            Self::PerceptualHash,
            Self::Embedding,
            Self::AutoTag,
            Self::CustomSuggest,
        ]
    }
}
string_enum!(StageStatus {
    Pending => "pending",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
});
string_enum!(JobStatus {
    Pending => "pending",
    Running => "running",
    Paused => "paused",
    Succeeded => "succeeded",
    Failed => "failed",
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub id: i64,
    pub absolute_path: String,
    pub volume_id: Option<String>,
    pub display_name: String,
    pub enabled: bool,
    pub status: RootStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRoot<'a> {
    pub absolute_path: &'a str,
    pub volume_id: Option<&'a str>,
    pub display_name: &'a str,
    pub enabled: bool,
    pub status: RootStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub id: i64,
    pub blake3_hash: [u8; 32],
    pub file_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
    pub captured_at: Option<i64>,
    pub perceptual_hash: Option<[u8; 8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAsset<'a> {
    pub blake3_hash: &'a [u8; 32],
    pub file_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<&'a str>,
    pub captured_at: Option<i64>,
    pub perceptual_hash: Option<&'a [u8; 8]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub id: i64,
    pub asset_id: i64,
    pub root_id: i64,
    pub relative_path: String,
    pub modified_at: i64,
    pub status: LocationStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewLocation<'a> {
    pub asset_id: i64,
    pub root_id: i64,
    pub relative_path: &'a str,
    pub modified_at: i64,
    pub status: LocationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageTarget {
    Asset(i64),
    Location(i64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageState {
    pub id: i64,
    pub target: StageTarget,
    pub stage: Stage,
    pub status: StageStatus,
    pub version: String,
    pub error: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: i64,
    pub target: StageTarget,
    pub stage: Stage,
    pub priority: i64,
    pub attempts: u32,
    pub max_attempts: u32,
    pub status: JobStatus,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

/// EXIF-derived metadata cached alongside an asset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AssetMetadata {
    pub asset_id: i64,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub exposure_time: Option<String>,
    pub f_number: Option<f64>,
    pub iso: Option<u32>,
    pub focal_length_mm: Option<f64>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
}

/// A pair of assets whose perceptual hashes are within a threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NearDuplicate {
    pub left_asset_id: i64,
    pub right_asset_id: i64,
    pub distance: u32,
}

string_enum!(TagOrigin {
    User => "user",
    ZeroShot => "zero_shot",
    CustomClassifier => "custom_classifier",
    Vision => "vision",
    Danbooru => "danbooru",
    Rating => "rating",
    Laya => "laya",
});
string_enum!(TagState {
    Suggested => "suggested",
    Confirmed => "confirmed",
    Rejected => "rejected",
});
string_enum!(AssignmentSource {
    ZeroShot => "zero_shot",
    CustomClassifier => "custom_classifier",
    User => "user",
    Vision => "vision",
    Danbooru => "danbooru",
    Rating => "rating",
    Laya => "laya",
});
string_enum!(TagMatchMode {
    All => "all",
    Any => "any",
    None => "none",
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub id: i64,
    pub namespace: String,
    pub canonical_name: String,
    pub display_name_ja: String,
    pub display_name_en: Option<String>,
    pub parent_id: Option<i64>,
    pub color: Option<String>,
    pub origin: TagOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTag<'a> {
    pub namespace: &'a str,
    pub canonical_name: &'a str,
    pub display_name_ja: &'a str,
    pub display_name_en: Option<&'a str>,
    pub parent_id: Option<i64>,
    pub color: Option<&'a str>,
    pub origin: TagOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagAlias {
    pub id: i64,
    pub tag_id: i64,
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TagAssignment {
    pub asset_id: i64,
    pub tag_id: i64,
    pub state: TagState,
    pub source: AssignmentSource,
    pub confidence: Option<f64>,
    pub model_id: Option<String>,
    pub model_version: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagWithCount {
    pub tag: Tag,
    pub asset_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagFilter {
    pub tag_ids: Vec<i64>,
    pub mode: TagMatchMode,
    pub include_suggested: bool,
}

impl Default for TagFilter {
    fn default() -> Self {
        Self {
            tag_ids: Vec::new(),
            mode: TagMatchMode::All,
            include_suggested: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssetFilter {
    pub tags: TagFilter,
    pub formats: Vec<String>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub captured_from: Option<i64>,
    pub captured_to: Option<i64>,
}

string_enum!(ColorSource {
    Auto => "auto",
    User => "user",
});

#[derive(Debug, Clone, PartialEq)]
pub struct AssetColor {
    pub asset_id: i64,
    pub rank: u8,
    pub hex: String,
    pub ratio: f64,
    pub source: ColorSource,
    pub algorithm_version: String,
}
