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
        (ML_WORKERS_KEY, "0"),
        (ML_WORKERS_KEY, "3"),
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
        ml_workers: 2,
        battery,
    };
    assert_eq!(policy(BatteryPolicy::Normal).active_image_workers(true), 4);
    assert_eq!(policy(BatteryPolicy::Reduce).active_image_workers(true), 1);
    assert_eq!(policy(BatteryPolicy::Pause).active_image_workers(true), 0);
    assert_eq!(policy(BatteryPolicy::Pause).active_image_workers(false), 4);
    assert_eq!(policy(BatteryPolicy::Normal).active_ml_workers(true), 2);
    assert_eq!(policy(BatteryPolicy::Reduce).active_ml_workers(true), 1);
    assert_eq!(policy(BatteryPolicy::Pause).active_ml_workers(true), 0);
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
        ml_workers: 2,
        battery: BatteryPolicy::Pause,
    });
    assert_eq!(pool.active_workers(), 5);
    assert_eq!(pool.active_ml_workers(), 2);
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

/// Returns the same unit vector for every input, or fails for files whose
/// name contains "fail".
struct ConstantEmbedder;

impl picker_ml::Embedder for ConstantEmbedder {
    fn embed_image(
        &self,
        path: &std::path::Path,
    ) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        if path.to_string_lossy().contains("fail") {
            return Err(picker_ml::EmbeddingError::Image("broken".to_owned()));
        }
        Ok(vec![1.0, 0.0])
    }

    fn embed_text(&self, _text: &str) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        Ok(vec![1.0, 0.0])
    }

    fn dimensions(&self) -> usize {
        2
    }

    fn model_id(&self) -> &str {
        "constant"
    }

    fn model_version(&self) -> &str {
        "v1"
    }
}

fn embedder() -> Arc<dyn picker_ml::Embedder> {
    Arc::new(ConstantEmbedder)
}

fn stage_status(library: &Library, asset_id: i64, stage: Stage) -> Option<StageStatus> {
    library
        .store()
        .stage_states()
        .get(StageTarget::Asset(asset_id), stage)
        .expect("state")
        .map(|state| state.status)
}

#[test]
fn model_stages_follow_image_jobs_once_a_model_is_set() {
    let fixture = Fixture::with_images(3);
    let library = &fixture.library;
    library.run_queued_image_jobs().expect("images");
    // Without a model the embedding jobs wait.
    assert!(library.run_next_ml_job().expect("ml").is_none());

    library.set_worker_embedder(Some(embedder()));
    // Three embeddings, then three auto-tag and three custom jobs.
    assert_eq!(library.run_queued_ml_jobs().expect("ml"), 9);
    for asset_id in fixture.asset_ids() {
        assert!(library
            .store()
            .embeddings()
            .get(asset_id, "constant", "v1")
            .expect("embedding")
            .is_some());
        for stage in [Stage::Embedding, Stage::AutoTag, Stage::CustomSuggest] {
            assert_eq!(
                stage_status(library, asset_id, stage),
                Some(StageStatus::Succeeded),
                "{stage:?}"
            );
        }
    }
}

#[test]
fn ui_library_picks_up_vectors_written_by_workers() {
    let fixture = Fixture::with_images(2);
    fixture.library.run_queued_image_jobs().expect("images");
    let ui = Library::open(
        fixture.data.path().join("library.sqlite3"),
        fixture.data.path().join("cache"),
    )
    .expect("ui library");
    ui.set_embedder(Some(embedder())).expect("ui embedder");
    assert!(ui
        .semantic_search("anything", 10)
        .expect("search")
        .is_empty());

    fixture.library.set_worker_embedder(Some(embedder()));
    fixture.library.run_queued_ml_jobs().expect("ml");
    assert_eq!(ui.sync_vectors().expect("sync"), 2);
    assert_eq!(ui.semantic_search("anything", 10).expect("search").len(), 2);
    // Later syncs only reread the current second at most.
    assert!(ui.sync_vectors().expect("sync again") <= 2);
}

#[test]
fn custom_tags_are_suggested_per_asset_and_keep_user_decisions() {
    let fixture = Fixture::with_images(5);
    let library = &fixture.library;
    library.run_queued_image_jobs().expect("images");
    library.set_worker_embedder(Some(embedder()));
    library.run_queued_ml_jobs().expect("ml");
    let ids = fixture.asset_ids();
    let tag = library.create_tag("cat", "ねこ", None, None).expect("tag");
    let tag = library.convert_tag_to_custom(tag.id).expect("custom");
    library
        .set_tag_on_assets(&ids[..3], tag.id, picker_store::TagState::Confirmed)
        .expect("examples");
    library
        .set_tag_on_assets(&ids[3..4], tag.id, picker_store::TagState::Rejected)
        .expect("rejected");

    let model = ConstantEmbedder;
    assert_eq!(
        library
            .suggest_custom_for_asset(&model, ids[4])
            .expect("suggest"),
        1
    );
    assert_eq!(
        library
            .suggest_custom_for_asset(&model, ids[3])
            .expect("rejected stays"),
        0
    );
    let state_of = |asset_id: i64| {
        library
            .asset_detail(asset_id)
            .expect("detail")
            .tags
            .into_iter()
            .find(|assigned| assigned.tag.id == tag.id)
            .map(|assigned| assigned.assignment.state)
    };
    assert_eq!(state_of(ids[4]), Some(picker_store::TagState::Suggested));
    assert_eq!(state_of(ids[3]), Some(picker_store::TagState::Rejected));
    assert_eq!(state_of(ids[0]), Some(picker_store::TagState::Confirmed));
}

#[test]
fn failing_embeddings_are_recorded_after_retries() {
    let fixture = Fixture::with_images(1);
    let media = &fixture.library.list_roots().expect("roots")[0]
        .root
        .absolute_path;
    fs::write(std::path::Path::new(media).join("fail.png"), png_bytes(3)).expect("write");
    let root_id = fixture.library.list_roots().expect("roots")[0].root.id;
    fixture.library.sync_root(root_id).expect("sync");
    fixture.library.run_queued_image_jobs().expect("images");
    fixture.library.set_worker_embedder(Some(embedder()));
    // One good image (3 jobs) and three attempts for the failing one.
    assert_eq!(fixture.library.run_queued_ml_jobs().expect("ml"), 6);
    let failed = fixture
        .asset_ids()
        .into_iter()
        .filter(|id| {
            stage_status(&fixture.library, *id, Stage::Embedding) == Some(StageStatus::Failed)
        })
        .count();
    assert_eq!(failed, 1);
}

#[test]
fn missing_embeddings_are_queued_for_a_new_model() {
    let fixture = Fixture::with_images(2);
    let library = &fixture.library;
    library.run_queued_image_jobs().expect("images");
    library.set_worker_embedder(Some(embedder()));
    library.run_queued_ml_jobs().expect("ml");
    assert_eq!(
        library.enqueue_missing_embeddings().expect("none missing"),
        0
    );
    library.store().embeddings().delete_all().expect("delete");
    assert_eq!(library.enqueue_missing_embeddings().expect("queued"), 2);
    library.set_worker_embedder(None);
    assert_eq!(library.enqueue_missing_embeddings().expect("no model"), 0);
}

#[test]
fn pool_runs_model_stages_after_the_embedder_arrives() {
    let fixture = Fixture::with_images(4);
    let pool =
        WorkerPool::start(fixture.config(), FakePower::new(false), Arc::new(|| {})).expect("pool");
    assert!(wait_until(Duration::from_secs(20), || fixture.processed() == 4));
    let embedded = || {
        fixture
            .library
            .store()
            .embeddings()
            .count_for_model("constant", "v1")
            .expect("count")
    };
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(embedded(), 0);
    pool.set_embedder(Some(embedder()));
    assert!(wait_until(Duration::from_secs(20), || embedded() == 4));
    pool.stop();
}

/// A constant embedder whose version can be chosen, to simulate a model
/// update.
struct VersionedEmbedder(&'static str);

impl picker_ml::Embedder for VersionedEmbedder {
    fn embed_image(
        &self,
        _path: &std::path::Path,
    ) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        Ok(vec![0.0, 1.0])
    }

    fn embed_text(&self, _text: &str) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        Ok(vec![0.0, 1.0])
    }

    fn dimensions(&self) -> usize {
        2
    }

    fn model_id(&self) -> &str {
        "constant"
    }

    fn model_version(&self) -> &str {
        self.0
    }
}

fn fully_processed(count: u8) -> Fixture {
    let fixture = Fixture::with_images(count);
    fixture.library.run_queued_image_jobs().expect("images");
    fixture.library.set_worker_embedder(Some(embedder()));
    fixture.library.run_queued_ml_jobs().expect("ml");
    fixture
}

fn set_version(library: &Library, asset_id: i64, stage: Stage, version: &str) {
    library
        .store()
        .stage_states()
        .upsert(
            StageTarget::Asset(asset_id),
            stage,
            StageStatus::Succeeded,
            version,
            None,
        )
        .expect("state");
}

#[test]
fn an_outdated_image_stage_is_redone_without_its_independent_stages() {
    let fixture = fully_processed(2);
    let library = &fixture.library;
    let outdated = fixture.asset_ids()[0];
    set_version(library, outdated, Stage::Thumbnail, "thumbnail-v0");

    assert_eq!(library.requeue_outdated_stages().expect("requeue"), 1);
    assert_eq!(library.run_queued_image_jobs().expect("images"), 1);
    assert_eq!(
        library
            .store()
            .stage_states()
            .get(StageTarget::Asset(outdated), Stage::Thumbnail)
            .expect("state")
            .expect("thumbnail")
            .version,
        crate::library::IMAGE_STAGE_VERSION
    );
    // The embedding reads the original file, so nothing downstream runs.
    assert_eq!(library.run_queued_ml_jobs().expect("ml"), 0);
}

#[test]
fn a_new_model_version_redoes_embeddings_and_their_downstream_stages_only() {
    let fixture = fully_processed(2);
    let library = &fixture.library;
    library.set_worker_embedder(Some(Arc::new(VersionedEmbedder("v2"))));

    assert_eq!(library.requeue_outdated_stages().expect("requeue"), 2);
    assert_eq!(library.run_queued_image_jobs().expect("images"), 0);
    // Two embeddings, then auto-tag and custom suggestions for each.
    assert_eq!(library.run_queued_ml_jobs().expect("ml"), 6);
    for asset_id in fixture.asset_ids() {
        assert!(library
            .store()
            .embeddings()
            .get(asset_id, "constant", "v2")
            .expect("embedding")
            .is_some());
    }
}

#[test]
fn an_outdated_auto_tag_stage_is_redone_alone() {
    let fixture = fully_processed(2);
    let library = &fixture.library;
    for asset_id in fixture.asset_ids() {
        set_version(library, asset_id, Stage::AutoTag, "zero-shot-v0");
    }
    assert_eq!(library.requeue_outdated_stages().expect("requeue"), 2);
    assert_eq!(library.run_queued_image_jobs().expect("images"), 0);
    assert_eq!(library.run_queued_ml_jobs().expect("ml"), 2);
    assert_eq!(library.requeue_outdated_stages().expect("current"), 0);
}

#[test]
fn current_results_queue_nothing_at_startup() {
    let fixture = fully_processed(3);
    assert_eq!(
        fixture.library.requeue_outdated_stages().expect("requeue"),
        0
    );
}
