//! OS change notifications for registered roots, and debouncing.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::{Result, WatchError};

/// Receives change notifications for the roots it watches.
///
/// Notifications only say that something below a path changed. The caller
/// rescans the root, so lost or merged notifications only delay a rescan.
pub struct ChangeNotifier {
    watcher: RecommendedWatcher,
    receiver: Receiver<PathBuf>,
    watched: BTreeSet<PathBuf>,
}

impl std::fmt::Debug for ChangeNotifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChangeNotifier")
            .field("watched", &self.watched)
            .finish_non_exhaustive()
    }
}

impl ChangeNotifier {
    pub fn new() -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            match event {
                Ok(event) => {
                    for path in event.paths {
                        // The receiver is gone only while shutting down.
                        let _ = sender.send(path);
                    }
                }
                // A notification error means changes may have been missed;
                // an empty path asks the caller to rescan every root.
                Err(_) => {
                    let _ = sender.send(PathBuf::new());
                }
            }
        })
        .map_err(notify_error)?;
        Ok(Self {
            watcher,
            receiver,
            watched: BTreeSet::new(),
        })
    }

    /// Starts watching a directory tree. Watching it again does nothing.
    pub fn watch(&mut self, root: &Path) -> Result<()> {
        if self.watched.contains(root) {
            return Ok(());
        }
        self.watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(notify_error)?;
        self.watched.insert(root.to_path_buf());
        Ok(())
    }

    /// Stops watching a directory tree. Unknown paths are ignored.
    pub fn unwatch(&mut self, root: &Path) {
        if self.watched.remove(root) {
            // The path may already be gone with its volume.
            let _ = self.watcher.unwatch(root);
        }
    }

    #[must_use]
    pub fn is_watching(&self, root: &Path) -> bool {
        self.watched.contains(root)
    }

    /// Waits up to `timeout` for the first notification, then returns it with
    /// every other notification already waiting. An empty path in the result
    /// means changes may have been missed.
    pub fn wait(&self, timeout: Duration) -> Vec<PathBuf> {
        let mut paths = match self.receiver.recv_timeout(timeout) {
            Ok(path) => vec![path],
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return Vec::new(),
        };
        paths.extend(self.receiver.try_iter());
        paths
    }
}

fn notify_error(error: notify::Error) -> WatchError {
    WatchError::Notify(error.to_string())
}

/// Groups repeated changes per key. A key becomes due once no change arrived
/// for `quiet`, or `max_wait` after its first change, whichever comes first.
#[derive(Debug, Clone)]
pub struct Debouncer<K> {
    quiet: Duration,
    max_wait: Duration,
    pending: BTreeMap<K, (Instant, Instant)>,
}

impl<K: Ord + Clone> Debouncer<K> {
    #[must_use]
    pub const fn new(quiet: Duration, max_wait: Duration) -> Self {
        Self {
            quiet,
            max_wait,
            pending: BTreeMap::new(),
        }
    }

    pub fn record(&mut self, key: K, now: Instant) {
        self.pending
            .entry(key)
            .and_modify(|(_, last)| *last = now)
            .or_insert((now, now));
    }

    /// Removes and returns the keys that are due at `now`.
    pub fn take_due(&mut self, now: Instant) -> Vec<K> {
        let due: Vec<K> = self
            .pending
            .iter()
            .filter(|(_, (first, last))| {
                now.duration_since(*last) >= self.quiet
                    || now.duration_since(*first) >= self.max_wait
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in &due {
            self.pending.remove(key);
        }
        due
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
