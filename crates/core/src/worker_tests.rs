use std::{
    fs,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use image::{DynamicImage, Rgba, RgbaImage};
use picker_imaging::ThumbnailSize;
use picker_store::{JobStatus, Stage, StageStatus, StageTarget};

use super::*;

fn png_bytes(value: u8) -> Vec<u8> {
    let mut image = RgbaImage::new(6, 4);
    for pixel in image.pixels_mut() {
        *pixel = Rgba([value, 255 - value, value / 3, 255]);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode png");
    bytes.into_inner()
}

struct FakePower(AtomicBool);

impl FakePower {
    fn new(on_battery: bool) -> Arc<Self> {
        Arc::new(Self(AtomicBool::new(on_battery)))
    }
}

impl PowerSource for FakePower {
    fn on_battery(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

struct Fixture {
    _media: tempfile::TempDir,
    data: tempfile::TempDir,
    library: Library,
}

impl Fixture {
    fn with_images(count: u8) -> Self {
        let media = tempfile::tempdir().expect("media");
        for index in 0..count {
            fs::write(
                media.path().join(format!("image-{index}.png")),
                png_bytes(index.wrapping_mul(29)),
            )
            .expect("write image");
        }
        let data = tempfile::tempdir().expect("data");
        let library = Library::open(
            data.path().join("library.sqlite3"),
            data.path().join("cache"),
        )
        .expect("library");
        let root = library.add_root(media.path(), "テスト").expect("root");
        library.sync_root(root.id).expect("sync");
        Self {
            _media: media,
            data,
            library,
        }
    }

    fn config(&self) -> PoolConfig {
        PoolConfig {
            idle_wait: Duration::from_millis(20),
            control_interval: Duration::from_millis(20),
            progress_interval: Duration::from_millis(1),
            ..PoolConfig::new(
                self.data.path().join("library.sqlite3"),
                self.data.path().join("cache"),
            )
        }
    }

    fn asset_ids(&self) -> Vec<i64> {
        self.library
            .asset_page(0, 500)
            .expect("page")
            .items
            .into_iter()
            .map(|item| item.id)
            .collect()
    }

    fn processed(&self) -> usize {
        self.asset_ids()
            .into_iter()
            .filter(|id| self.library.is_image_processed(*id).expect("processed"))
            .count()
    }
}

fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

#[test]
fn sync_root_queues_work_without_processing_it() {
    let fixture = Fixture::with_images(3);
    assert_eq!(fixture.processed(), 0);
    let status = fixture.library.index_status().expect("status");
    let thumbnail = status
        .stages
        .iter()
        .find(|stage| stage.stage == Stage::Thumbnail)
        .expect("thumbnail stage");
    assert_eq!(thumbnail.pending, 3);

    assert_eq!(fixture.library.run_queued_image_jobs().expect("run"), 3);
    assert_eq!(fixture.processed(), 3);
    let status = fixture.library.index_status().expect("status");
    let thumbnail = status
        .stages
        .iter()
        .find(|stage| stage.stage == Stage::Thumbnail)
        .expect("thumbnail stage");
    assert_eq!((thumbnail.completed, thumbnail.pending), (3, 0));
}

#[test]
fn rescan_queues_assets_whose_thumbnails_were_removed() {
    let fixture = Fixture::with_images(2);
    fixture.library.run_queued_image_jobs().expect("run");
    let asset_id = fixture.asset_ids()[0];
    fs::remove_file(
        fixture
            .library
            .thumbnail_path(asset_id, ThumbnailSize::Grid),
    )
    .expect("remove thumbnail");

    let root_id = fixture.library.list_roots().expect("roots")[0].root.id;
    fixture.library.sync_root(root_id).expect("sync");
    assert_eq!(fixture.library.run_queued_image_jobs().expect("run"), 1);
    assert!(fixture
        .library
        .is_image_processed(asset_id)
        .expect("processed"));
}

#[test]
fn viewport_assets_are_processed_first() {
    let fixture = Fixture::with_images(4);
    let last = *fixture.asset_ids().iter().max().expect("asset");
    fixture
        .library
        .prioritize_viewport(&[last])
        .expect("priority");
    let run = fixture
        .library
        .run_next_image_job()
        .expect("run")
        .expect("job");
    assert_eq!(run.asset_id, Some(last));
    assert_eq!(run.status, JobStatus::Succeeded);
}

#[test]
fn broken_images_fail_after_retries_without_blocking_others() {
    let fixture = Fixture::with_images(2);
    let media = &fixture.library.list_roots().expect("roots")[0]
        .root
        .absolute_path;
    fs::write(std::path::Path::new(media).join("broken.png"), b"not a png").expect("write");
    let root_id = fixture.library.list_roots().expect("roots")[0].root.id;
    fixture.library.sync_root(root_id).expect("sync");

    // Three attempts for the broken file plus one run for each good file.
    assert_eq!(fixture.library.run_queued_image_jobs().expect("run"), 5);
    assert_eq!(fixture.processed(), 2);
    let broken = fixture
        .asset_ids()
        .into_iter()
        .find(|id| !fixture.library.is_image_processed(*id).expect("processed"))
        .expect("broken asset");
    let state = fixture
        .library
        .store()
        .stage_states()
        .get(StageTarget::Asset(broken), Stage::Thumbnail)
        .expect("state")
        .expect("thumbnail state");
    assert_eq!(state.status, StageStatus::Failed);
    assert!(state.error.is_some());
}

#[test]
fn paused_jobs_are_not_run() {
    let fixture = Fixture::with_images(2);
    fixture.library.pause_index().expect("pause");
    assert!(fixture.library.run_next_image_job().expect("run").is_none());
    fixture.library.resume_index().expect("resume");
    assert_eq!(fixture.library.run_queued_image_jobs().expect("run"), 2);
}

#[test]
fn worker_settings_default_validate_and_follow_the_battery_policy() {
    let fixture = Fixture::with_images(0);
    let library = &fixture.library;
    assert_eq!(
        library.worker_settings().expect("settings"),
        WorkerSettings::default()
    );
    for (key, value) in [
        (IMAGE_WORKERS_KEY, "0"),
        (IMAGE_WORKERS_KEY, "9"),
        (IMAGE_WORKERS_KEY, "two"),
        (IMAGE_WORKERS_KEY, ""),
        (BATTERY_POLICY_KEY, "eco"),
    ] {
        assert!(
            matches!(
                library.set_setting(key, value),
                Err(LibraryError::InvalidInput(_))
            ),
            "{key}={value}"
        );
    }
    library
        .set_setting(IMAGE_WORKERS_KEY, "4")
        .expect("workers");
    library
        .set_setting(BATTERY_POLICY_KEY, "pause")
        .expect("policy");
    let settings = library.worker_settings().expect("settings");
    assert_eq!(settings.image_workers, 4);
    assert_eq!(settings.battery, BatteryPolicy::Pause);

    let policy = |battery| WorkerSettings {
        image_workers: 4,
        battery,
    };
    assert_eq!(policy(BatteryPolicy::Normal).active_image_workers(true), 4);
    assert_eq!(policy(BatteryPolicy::Reduce).active_image_workers(true), 1);
    assert_eq!(policy(BatteryPolicy::Pause).active_image_workers(true), 0);
    assert_eq!(policy(BatteryPolicy::Pause).active_image_workers(false), 4);
}

#[test]
fn pmset_output_is_parsed() {
    assert_eq!(
        parse_pmset("Now drawing from 'Battery Power'\n -InternalBattery-0 80%"),
        Some(true)
    );
    assert_eq!(parse_pmset("Now drawing from 'AC Power'\n"), Some(false));
    assert_eq!(parse_pmset(""), None);
    assert_eq!(parse_pmset("unexpected"), None);
}

#[test]
fn pool_processes_the_queue_in_parallel_and_reports_progress() {
    let fixture = Fixture::with_images(12);
    fixture
        .library
        .set_setting(IMAGE_WORKERS_KEY, "3")
        .expect("workers");
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let pool = WorkerPool::start(
        fixture.config(),
        FakePower::new(false),
        Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        }),
    )
    .expect("pool");
    assert_eq!(pool.active_workers(), 3);
    pool.apply_settings(WorkerSettings {
        image_workers: 5,
        battery: BatteryPolicy::Pause,
    });
    assert_eq!(pool.active_workers(), 5);
    assert!(wait_until(Duration::from_secs(20), || fixture.processed() == 12));
    assert!(wait_until(Duration::from_secs(5), || calls
        .load(Ordering::Relaxed)
        > 0));
    pool.stop();
}

#[test]
fn pool_pauses_on_battery_and_resumes_on_ac_power() {
    let fixture = Fixture::with_images(3);
    fixture
        .library
        .set_setting(BATTERY_POLICY_KEY, "pause")
        .expect("policy");
    let power = FakePower::new(true);
    let pool = WorkerPool::start(fixture.config(), power.clone(), Arc::new(|| {})).expect("pool");
    assert!(pool.on_battery());
    assert_eq!(pool.active_workers(), 0);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(fixture.processed(), 0);

    power.0.store(false, Ordering::Relaxed);
    assert!(wait_until(Duration::from_secs(20), || fixture.processed() == 3));
    assert_eq!(pool.active_workers(), DEFAULT_IMAGE_WORKERS);
    drop(pool);
}

#[test]
fn pool_requeues_jobs_left_running_by_a_crash() {
    let fixture = Fixture::with_images(1);
    let claimed = fixture
        .library
        .store()
        .jobs()
        .dequeue_any(IMAGE_STAGES)
        .expect("dequeue")
        .expect("job");
    assert_eq!(claimed.status, JobStatus::Running);
    let pool =
        WorkerPool::start(fixture.config(), FakePower::new(false), Arc::new(|| {})).expect("pool");
    assert!(wait_until(Duration::from_secs(20), || fixture.processed() == 1));
    pool.stop();
}
