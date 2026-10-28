//! Background workers for the persistent job queue (design 7.5, T1-5, T3-8).
//!
//! Each worker thread opens its own [`Library`] on the same database, so a
//! slow decode never holds the connection the UI uses. The number of active
//! workers follows the settings and the power source, and is re-read while
//! the pool runs.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use picker_imaging::ImageFormat;
use picker_store::{JobStatus, Stage, StageStatus, StageTarget};

use crate::library::{Library, LibraryError, Result, IMAGE_STAGE_VERSION};

/// Stages handled by one image job. A single decode produces the metadata,
/// thumbnails, perceptual hash, and colors of an asset.
pub const IMAGE_STAGES: &[Stage] = &[
    Stage::Hash,
    Stage::Metadata,
    Stage::Thumbnail,
    Stage::PerceptualHash,
];

/// Upper bound for image workers; each one holds a decoded image in memory.
pub const MAX_IMAGE_WORKERS: u32 = 8;
pub const DEFAULT_IMAGE_WORKERS: u32 = 2;

pub const IMAGE_WORKERS_KEY: &str = "index.image_workers";
pub const BATTERY_POLICY_KEY: &str = "index.battery_policy";

/// What the workers do while the computer runs on battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BatteryPolicy {
    /// Keep the configured number of workers.
    Normal,
    /// Run a single worker.
    #[default]
    Reduce,
    /// Stop processing until the computer is plugged in.
    Pause,
}

impl BatteryPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Reduce => "reduce",
            Self::Pause => "pause",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "normal" => Ok(Self::Normal),
            "reduce" => Ok(Self::Reduce),
            "pause" => Ok(Self::Pause),
            _ => Err(LibraryError::InvalidInput(format!(
                "{BATTERY_POLICY_KEY} must be normal, reduce, or pause"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerSettings {
    pub image_workers: u32,
    pub battery: BatteryPolicy,
}

impl Default for WorkerSettings {
    fn default() -> Self {
        Self {
            image_workers: DEFAULT_IMAGE_WORKERS,
            battery: BatteryPolicy::default(),
        }
    }
}

impl WorkerSettings {
    /// Number of image workers that should run now.
    #[must_use]
    pub fn active_image_workers(self, on_battery: bool) -> u32 {
        let configured = self.image_workers.clamp(1, MAX_IMAGE_WORKERS);
        match (on_battery, self.battery) {
            (false, _) | (true, BatteryPolicy::Normal) => configured,
            (true, BatteryPolicy::Reduce) => 1,
            (true, BatteryPolicy::Pause) => 0,
        }
    }
}

pub(crate) fn parse_image_workers(value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .ok()
        .filter(|count| (1..=MAX_IMAGE_WORKERS).contains(count))
        .ok_or_else(|| {
            LibraryError::InvalidInput(format!(
                "{IMAGE_WORKERS_KEY} must be an integer from 1 to {MAX_IMAGE_WORKERS}"
            ))
        })
}

/// Validates the settings that the workers read. Other keys are accepted as is.
pub(crate) fn validate_setting(key: &str, value: &str) -> Result<()> {
    match key {
        IMAGE_WORKERS_KEY => parse_image_workers(value).map(|_| ()),
        BATTERY_POLICY_KEY => BatteryPolicy::parse(value).map(|_| ()),
        _ => Ok(()),
    }
}

/// Result of running one queued job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobRun {
    pub job_id: i64,
    pub asset_id: Option<i64>,
    pub status: JobStatus,
}

impl Library {
    /// Reads the worker settings, falling back to defaults for missing keys.
    pub fn worker_settings(&self) -> Result<WorkerSettings> {
        let defaults = WorkerSettings::default();
        Ok(WorkerSettings {
            image_workers: self
                .get_setting(IMAGE_WORKERS_KEY)?
                .map_or(Ok(defaults.image_workers), |value| {
                    parse_image_workers(&value)
                })?,
            battery: self
                .get_setting(BATTERY_POLICY_KEY)?
                .map_or(Ok(defaults.battery), |value| BatteryPolicy::parse(&value))?,
        })
    }

    /// Claims and runs the most urgent image job. Returns `None` when no
    /// image job is waiting.
    pub fn run_next_image_job(&self) -> Result<Option<JobRun>> {
        let Some(job) = self.store().jobs().dequeue_any(IMAGE_STAGES)? else {
            return Ok(None);
        };
        let StageTarget::Asset(asset_id) = job.target else {
            // Location jobs carry no image work in this version.
            let job = self.store().jobs().mark_succeeded(job.id)?;
            return Ok(Some(JobRun {
                job_id: job.id,
                asset_id: None,
                status: job.status,
            }));
        };
        let outcome = self.process_queued_asset(asset_id);
        let target = StageTarget::Asset(asset_id);
        let finished = match &outcome {
            Ok(()) => {
                self.store().stage_states().upsert(
                    target,
                    Stage::Thumbnail,
                    StageStatus::Succeeded,
                    IMAGE_STAGE_VERSION,
                    None,
                )?;
                self.store().jobs().mark_succeeded(job.id)
            }
            Err(error) => {
                let message = error.to_string();
                let finished = self.store().jobs().fail_or_retry(job.id, &message);
                if matches!(&finished, Ok(job) if job.status == JobStatus::Failed) {
                    self.store().stage_states().upsert(
                        target,
                        Stage::Thumbnail,
                        StageStatus::Failed,
                        IMAGE_STAGE_VERSION,
                        Some(&message),
                    )?;
                }
                finished
            }
        };
        // The job disappears when its asset is deleted while it runs.
        let status = finished.map_or(JobStatus::Succeeded, |job| job.status);
        Ok(Some(JobRun {
            job_id: job.id,
            asset_id: Some(asset_id),
            status,
        }))
    }

    /// Runs image jobs on this thread until none is waiting.
    pub fn run_queued_image_jobs(&self) -> Result<usize> {
        let mut processed = 0;
        while self.run_next_image_job()?.is_some() {
            processed += 1;
        }
        Ok(processed)
    }

    fn process_queued_asset(&self, asset_id: i64) -> Result<()> {
        if self.is_image_processed(asset_id)? {
            return Ok(());
        }
        let Some(source) = self.present_source(asset_id)? else {
            // The file went missing after the job was queued; a later scan
            // queues it again when the file returns.
            return Ok(());
        };
        let format = ImageFormat::from_path(&source)
            .ok_or_else(|| LibraryError::InvalidInput("unsupported image format".to_owned()))?;
        self.process_asset(asset_id, &source, format)
    }
}

/// Reports whether the computer currently runs on battery.
pub trait PowerSource: Send + Sync {
    fn on_battery(&self) -> bool;
}

/// Reads the power source from the operating system.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemPower;

impl PowerSource for SystemPower {
    #[cfg(target_os = "macos")]
    fn on_battery(&self) -> bool {
        // `pmset` ships with macOS and avoids calling IOKit through FFI.
        std::process::Command::new("/usr/bin/pmset")
            .args(["-g", "batt"])
            .output()
            .ok()
            .and_then(|output| parse_pmset(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "macos"))]
    fn on_battery(&self) -> bool {
        false
    }
}

/// Parses the first line of `pmset -g batt`.
#[must_use]
pub fn parse_pmset(output: &str) -> Option<bool> {
    let line = output.lines().next()?;
    if line.contains("'Battery Power'") {
        Some(true)
    } else if line.contains("'AC Power'") || line.contains("'UPS Power'") {
        Some(false)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub database: PathBuf,
    pub cache_root: PathBuf,
    /// How long an idle worker waits before looking at the queue again.
    pub idle_wait: Duration,
    /// How often the settings and the power source are read.
    pub control_interval: Duration,
    /// Shortest time between two progress notifications.
    pub progress_interval: Duration,
}

impl PoolConfig {
    #[must_use]
    pub fn new(database: impl Into<PathBuf>, cache_root: impl Into<PathBuf>) -> Self {
        Self {
            database: database.into(),
            cache_root: cache_root.into(),
            idle_wait: Duration::from_millis(500),
            control_interval: Duration::from_secs(30),
            progress_interval: Duration::from_millis(500),
        }
    }
}

type ProgressCallback = Arc<dyn Fn() + Send + Sync>;

struct Control {
    stop: AtomicBool,
    active: AtomicU32,
    on_battery: AtomicBool,
    /// Milliseconds since `started` of the last progress notification.
    last_progress: AtomicU64,
    started: Instant,
}

/// Runs image jobs on background threads until dropped.
pub struct WorkerPool {
    control: Arc<Control>,
    handles: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    /// Starts the pool. Jobs left running by a previous process are queued
    /// again first.
    pub fn start(
        config: PoolConfig,
        power: Arc<dyn PowerSource>,
        on_progress: ProgressCallback,
    ) -> Result<Self> {
        let supervisor = Library::open(&config.database, &config.cache_root)?;
        supervisor.store().jobs().recover_running()?;
        let control = Arc::new(Control {
            stop: AtomicBool::new(false),
            active: AtomicU32::new(0),
            on_battery: AtomicBool::new(false),
            last_progress: AtomicU64::new(0),
            started: Instant::now(),
        });
        refresh_control(&supervisor, power.as_ref(), &control);

        let mut handles = Vec::with_capacity(MAX_IMAGE_WORKERS as usize + 1);
        {
            let control = Arc::clone(&control);
            let interval = config.control_interval;
            handles.push(thread::spawn(move || {
                while wait_or_stop(&control, interval) {
                    refresh_control(&supervisor, power.as_ref(), &control);
                }
            }));
        }
        for index in 0..MAX_IMAGE_WORKERS {
            let control = Arc::clone(&control);
            let config = config.clone();
            let on_progress = Arc::clone(&on_progress);
            handles.push(thread::spawn(move || {
                image_worker(index, &config, &control, &on_progress);
            }));
        }
        Ok(Self { control, handles })
    }

    /// Applies changed settings now instead of at the next periodic read.
    pub fn apply_settings(&self, settings: WorkerSettings) {
        let on_battery = self.control.on_battery.load(Ordering::Relaxed);
        self.control
            .active
            .store(settings.active_image_workers(on_battery), Ordering::Relaxed);
    }

    /// Number of image workers allowed to run now.
    #[must_use]
    pub fn active_workers(&self) -> u32 {
        self.control.active.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn on_battery(&self) -> bool {
        self.control.on_battery.load(Ordering::Relaxed)
    }

    /// Stops all threads and waits for running jobs to finish.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.control.stop.store(true, Ordering::Relaxed);
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn refresh_control(library: &Library, power: &dyn PowerSource, control: &Control) {
    let settings = library.worker_settings().unwrap_or_else(|error| {
        eprintln!("worker settings unavailable: {error}");
        WorkerSettings::default()
    });
    let on_battery = power.on_battery();
    control.on_battery.store(on_battery, Ordering::Relaxed);
    control
        .active
        .store(settings.active_image_workers(on_battery), Ordering::Relaxed);
}

/// Sleeps in short steps so that a stop request is seen quickly. Returns
/// `false` once the pool is stopping.
fn wait_or_stop(control: &Control, duration: Duration) -> bool {
    const STEP: Duration = Duration::from_millis(50);
    let deadline = Instant::now() + duration;
    while !control.stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep(STEP.min(deadline - now));
    }
    false
}

fn image_worker(
    index: u32,
    config: &PoolConfig,
    control: &Control,
    on_progress: &ProgressCallback,
) {
    let mut library = None;
    // Set after a job runs, so the last jobs of a batch are always reported
    // even when they finish inside the throttling interval.
    let mut unreported = false;
    loop {
        if control.stop.load(Ordering::Relaxed) {
            return;
        }
        if index >= control.active.load(Ordering::Relaxed) {
            if !wait_or_stop(control, config.idle_wait) {
                return;
            }
            continue;
        }
        if library.is_none() {
            match Library::open(&config.database, &config.cache_root) {
                Ok(opened) => library = Some(opened),
                Err(error) => {
                    eprintln!("image worker {index} could not open the library: {error}");
                    if !wait_or_stop(control, config.idle_wait) {
                        return;
                    }
                    continue;
                }
            }
        }
        let Some(worker_library) = library.as_ref() else {
            continue;
        };
        match worker_library.run_next_image_job() {
            Ok(Some(_)) => {
                unreported = !notify_progress(control, config.progress_interval, on_progress);
            }
            Ok(None) => {
                if std::mem::take(&mut unreported) {
                    on_progress();
                }
                if !wait_or_stop(control, config.idle_wait) {
                    return;
                }
            }
            Err(error) => {
                eprintln!("image worker {index} failed: {error}");
                if !wait_or_stop(control, config.idle_wait) {
                    return;
                }
            }
        }
    }
}

/// Calls `on_progress` unless another call happened within `interval`.
/// Returns whether it was called.
fn notify_progress(control: &Control, interval: Duration, on_progress: &ProgressCallback) -> bool {
    let now = u64::try_from(control.started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let interval = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
    let last = control.last_progress.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= interval
        && control
            .last_progress
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        on_progress();
        return true;
    }
    false
}
