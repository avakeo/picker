#![deny(unsafe_code)]

//! Filesystem watching boundary for Picker.
//!
//! [`ChangeNotifier`] receives OS change notifications (FSEvents on macOS) for
//! registered roots, and [`Debouncer`] groups a burst of them into one rescan
//! per root. [`RootWatcher`] polls a root and computes a content-free snapshot
//! diff, for volumes where notifications cannot be registered.

mod notifier;

pub use notifier::{ChangeNotifier, Debouncer};

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

/// A typed filesystem boundary error.
#[derive(Debug)]
pub enum WatchError {
    Io(std::io::Error),
    Notify(String),
}

impl std::fmt::Display for WatchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Notify(error) => write!(formatter, "change notification error: {error}"),
        }
    }
}

impl std::error::Error for WatchError {}

impl From<std::io::Error> for WatchError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub type Result<T> = std::result::Result<T, WatchError>;

/// State of a watched root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootState {
    Online,
    Offline,
}

/// Kind of change detected for one relative path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Removed,
    Offline,
    Online,
}

/// One changed path relative to the watched root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEvent {
    pub relative_path: PathBuf,
    pub kind: ChangeKind,
}

/// Result of one poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollResult {
    pub state: RootState,
    pub state_changed: bool,
    pub events: Vec<ChangeEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified_at: u128,
}

type Snapshot = BTreeMap<PathBuf, FileStamp>;

/// Polling watcher for one root.
#[derive(Debug)]
pub struct RootWatcher {
    root: PathBuf,
    state: RootState,
    snapshot: Snapshot,
}

impl RootWatcher {
    /// Creates a watcher and takes the initial snapshot.
    ///
    /// If the root is missing at startup, the watcher starts offline and emits
    /// no removals. This is the expected state for an ejected external volume.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let (state, snapshot) = match scan(&root) {
            Ok(snapshot) => (RootState::Online, snapshot),
            Err(error) if is_missing(&error) => (RootState::Offline, Snapshot::new()),
            Err(error) => return Err(error),
        };
        Ok(Self {
            root,
            state,
            snapshot,
        })
    }

    /// Returns the current root state without polling.
    #[must_use]
    pub const fn state(&self) -> RootState {
        self.state
    }

    /// Polls the root and returns coalesced changes since the previous poll.
    pub fn poll(&mut self) -> Result<PollResult> {
        let (state, snapshot) = match scan(&self.root) {
            Ok(snapshot) => (RootState::Online, snapshot),
            Err(error) if is_missing(&error) => (RootState::Offline, Snapshot::new()),
            Err(error) => return Err(error),
        };
        let state_changed = state != self.state;
        let mut events = Vec::new();
        if state_changed {
            events.push(ChangeEvent {
                relative_path: PathBuf::new(),
                kind: if state == RootState::Online {
                    ChangeKind::Online
                } else {
                    ChangeKind::Offline
                },
            });
        }
        if state == RootState::Online {
            events.extend(diff(&self.snapshot, &snapshot));
            self.snapshot = snapshot;
        }
        self.state = state;
        Ok(PollResult {
            state,
            state_changed,
            events,
        })
    }
}

fn is_missing(error: &WatchError) -> bool {
    matches!(error, WatchError::Io(error) if error.kind() == std::io::ErrorKind::NotFound)
}

fn scan(root: &Path) -> Result<Snapshot> {
    let mut snapshot = Snapshot::new();
    let metadata = fs::metadata(root)?;
    if !metadata.is_dir() {
        return Err(WatchError::Io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "watched root is not a directory",
        )));
    }
    scan_dir(root, Path::new(""), &mut snapshot)?;
    Ok(snapshot)
}

fn scan_dir(root: &Path, relative: &Path, snapshot: &mut Snapshot) -> Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let child = relative.join(name);
        if file_type.is_dir() {
            scan_dir(root, &child, snapshot)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            snapshot.insert(child, stamp(&metadata));
        }
    }
    Ok(())
}

fn stamp(metadata: &fs::Metadata) -> FileStamp {
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    FileStamp {
        size: metadata.len(),
        modified_at,
    }
}

fn diff(previous: &Snapshot, current: &Snapshot) -> Vec<ChangeEvent> {
    let mut events = Vec::new();
    for (path, stamp) in current {
        match previous.get(path) {
            None => events.push(ChangeEvent {
                relative_path: path.clone(),
                kind: ChangeKind::Added,
            }),
            Some(old) if old != stamp => events.push(ChangeEvent {
                relative_path: path.clone(),
                kind: ChangeKind::Modified,
            }),
            Some(_) => {}
        }
    }
    for path in previous.keys() {
        if !current.contains_key(path) {
            events.push(ChangeEvent {
                relative_path: path.clone(),
                kind: ChangeKind::Removed,
            });
        }
    }
    events.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    events
}

/// Returns true when a relative path is a supported, visible file candidate.
///
/// The scanner uses this to keep symlinks and hidden entries out of the index.
#[must_use]
pub fn is_visible_relative_path(path: &Path) -> bool {
    path.components().all(|component| match component {
        Component::Normal(name) => !name.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

#[cfg(test)]
mod tests;
