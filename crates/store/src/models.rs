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
