//! Folder watching and differential scans (design 7.3, T1-9).
//!
//! One background thread owns its own [`Library`]. It scans every enabled
//! root at startup, rescans a root shortly after its files change, and checks
//! regularly whether each root is reachable. A root that disappears (an
//! ejected drive) becomes offline instead of losing its files, and is scanned
//! again when it returns.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use picker_fswatch::{ChangeNotifier, Debouncer, RootWatcher};
use picker_store::RootStatus;

use crate::library::{Library, Result};

#[derive(Debug, Clone)]
pub struct WatchConfig {
    pub database: PathBuf,
    pub cache_root: PathBuf,
    /// A root is rescanned once its files were quiet for this long.
    pub quiet: Duration,
    /// Longest delay between a change and its rescan, for roots that keep
    /// changing.
    pub max_wait: Duration,
    /// How often reachability is checked and polled roots are compared.
    pub health_interval: Duration,
}

impl WatchConfig {
    #[must_use]
    pub fn new(database: impl Into<PathBuf>, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            database: database.into(),
            cache_root: cache_root.into(),
            quiet: Duration::from_secs(2),
            max_wait: Duration::from_secs(10),
            health_interval: Duration::from_secs(15),
        }
    }
}

type ChangeCallback = Arc<dyn Fn() + Send + Sync>;

/// Runs the watching thread until dropped.
pub struct WatchService {
    stop: Arc<AtomicBool>,
    refresh: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl WatchService {
    /// Starts watching. `on_change` is called after each scan that may have
    /// changed the library and after a root goes offline or online.
    pub fn start(config: WatchConfig, on_change: ChangeCallback) -> Result<Self> {
        let library = Library::open(&config.database, &config.cache_root)?;
        let stop = Arc::new(AtomicBool::new(false));
        let refresh = Arc::new(AtomicBool::new(true));
        let handle = {
            let stop = Arc::clone(&stop);
            let refresh = Arc::clone(&refresh);
            thread::spawn(move || {
                let mut watcher = Watch::new(library, config, on_change);
                watcher.run(&stop, &refresh);
            })
        };
        Ok(Self {
            stop,
            refresh,
            handle: Some(handle),
        })
    }

    /// Asks the thread to re-read the roots, after one was added, removed,
    /// enabled, or disabled.
    pub fn refresh(&self) {
        self.refresh.store(true, Ordering::Relaxed);
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WatchService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum Mode {
    /// The OS reports changes below the root.
    Notified,
    /// Notifications could not be registered; the root is compared with a
    /// snapshot at every health check.
    Polled(Box<RootWatcher>),
}

struct WatchedRoot {
    /// Path as stored, used for scanning.
    path: PathBuf,
    /// Canonical path, used to match notifications.
    canonical: PathBuf,
    online: bool,
    mode: Option<Mode>,
}

struct Watch {
    library: Library,
    config: WatchConfig,
    on_change: ChangeCallback,
    notifier: Option<ChangeNotifier>,
    roots: BTreeMap<i64, WatchedRoot>,
    debouncer: Debouncer<i64>,
}

impl Watch {
    fn new(library: Library, config: WatchConfig, on_change: ChangeCallback) -> Self {
        let notifier = ChangeNotifier::new()
            .map_err(|error| {
                log::warn!("change notifications unavailable, polling instead: {error}")
            })
            .ok();
        let debouncer = Debouncer::new(config.quiet, config.max_wait);
        Self {
            library,
            config,
            on_change,
            notifier,
            roots: BTreeMap::new(),
            debouncer,
        }
    }

    fn run(&mut self, stop: &AtomicBool, refresh: &AtomicBool) {
        const STEP: Duration = Duration::from_millis(100);
        let mut next_health = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            if refresh.swap(false, Ordering::Relaxed) || Instant::now() >= next_health {
                if let Err(error) = self.check_roots() {
                    log::error!("root check failed: {error}");
                }
                next_health = Instant::now() + self.config.health_interval;
            }
            let changed = match &self.notifier {
                Some(notifier) => notifier.wait(STEP),
                None => {
                    thread::sleep(STEP);
                    Vec::new()
                }
            };
            let now = Instant::now();
            for path in changed {
                for root_id in self.roots_for(&path) {
                    self.debouncer.record(root_id, now);
                }
            }
            let due = self.debouncer.take_due(now);
            if !due.is_empty() {
                let mut scanned = false;
                for root_id in due {
                    scanned |= self.scan(root_id);
                }
                if scanned {
                    (self.on_change)();
                }
            }
        }
    }

    /// Roots a notification belongs to. An empty path means notifications
    /// may have been lost, so every online root is rescanned.
    fn roots_for(&self, path: &Path) -> Vec<i64> {
        self.roots
            .iter()
            .filter(|(_, root)| {
                root.online && (path.as_os_str().is_empty() || path.starts_with(&root.canonical))
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Re-reads the enabled roots, updates reachability, and scans roots
    /// that are new or came back.
    fn check_roots(&mut self) -> Result<()> {
        let enabled: BTreeMap<i64, PathBuf> = self
            .library
            .store()
            .roots()
            .list()?
            .into_iter()
            .filter(|root| root.enabled)
            .map(|root| (root.id, PathBuf::from(root.absolute_path)))
            .collect();
        let removed: Vec<i64> = self
            .roots
            .keys()
            .filter(|id| !enabled.contains_key(id))
            .copied()
            .collect();
        for root_id in removed {
            if let Some(root) = self.roots.remove(&root_id) {
                self.stop_watching(&root);
            }
        }

        let mut changed = false;
        for (root_id, path) in enabled {
            let reachable = path.is_dir();
            let known = self.roots.get(&root_id).map(|root| root.online);
            match (known, reachable) {
                // New root, or a root that came back: scan, then watch.
                (None | Some(false), true) => {
                    self.library
                        .store()
                        .roots()
                        .set_status(root_id, RootStatus::Online)?;
                    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                    let mut root = WatchedRoot {
                        path,
                        canonical,
                        online: true,
                        mode: None,
                    };
                    self.start_watching(&mut root);
                    self.roots.insert(root_id, root);
                    self.scan(root_id);
                    changed = true;
                }
                // A root that went away: keep its files as offline.
                (None | Some(true), false) => {
                    log::info!("root {root_id} went offline: {}", path.display());
                    self.library
                        .store()
                        .roots()
                        .set_status(root_id, RootStatus::Offline)?;
                    self.library
                        .store()
                        .locations()
                        .mark_root_offline(root_id)?;
                    if let Some(root) = self.roots.remove(&root_id) {
                        self.stop_watching(&root);
                    }
                    self.roots.insert(
                        root_id,
                        WatchedRoot {
                            canonical: path.clone(),
                            path,
                            online: false,
                            mode: None,
                        },
                    );
                    changed = true;
                }
                (Some(true), true) => {
                    let polled = match self
                        .roots
                        .get_mut(&root_id)
                        .and_then(|root| root.mode.as_mut())
                    {
                        Some(Mode::Polled(watcher)) => watcher
                            .poll()
                            .map(|result| !result.events.is_empty())
                            .unwrap_or(true),
                        _ => false,
                    };
                    if polled {
                        self.debouncer.record(root_id, Instant::now());
                    }
                }
                (Some(false), false) => {}
            }
        }
        if changed {
            (self.on_change)();
        }
        Ok(())
    }

    fn start_watching(&mut self, root: &mut WatchedRoot) {
        let notified = self
            .notifier
            .as_mut()
            .is_some_and(|notifier| notifier.watch(&root.canonical).is_ok());
        root.mode = if notified {
            Some(Mode::Notified)
        } else {
            RootWatcher::new(&root.path)
                .ok()
                .map(|watcher| Mode::Polled(Box::new(watcher)))
        };
    }

    fn stop_watching(&mut self, root: &WatchedRoot) {
        if matches!(root.mode, Some(Mode::Notified)) {
            if let Some(notifier) = self.notifier.as_mut() {
                notifier.unwatch(&root.canonical);
            }
        }
    }

    /// Synchronizes one online root. Returns whether a scan ran.
    fn scan(&self, root_id: i64) -> bool {
        let Some(root) = self.roots.get(&root_id) else {
            return false;
        };
        if !root.online || !root.path.is_dir() {
            // The next health check marks it offline.
            return false;
        }
        match self.library.sync_root(root_id) {
            Ok(_) => true,
            Err(error) => {
                log::error!("scan of {:?} failed: {error}", root.path);
                false
            }
        }
    }
}
