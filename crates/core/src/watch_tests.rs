use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use image::{DynamicImage, Rgba, RgbaImage};
use picker_store::{LocationStatus, RootStatus};

use super::*;

fn png_bytes(value: u8) -> Vec<u8> {
    let mut image = RgbaImage::new(5, 5);
    for pixel in image.pixels_mut() {
        *pixel = Rgba([value, value / 2, 255 - value, 255]);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode png");
    bytes.into_inner()
}

struct Fixture {
    base: tempfile::TempDir,
    library: Library,
    changes: Arc<AtomicUsize>,
}

impl Fixture {
    fn new() -> Self {
        let base = tempfile::tempdir().expect("base");
        let library = Library::open(
            base.path().join("library.sqlite3"),
            base.path().join("cache"),
        )
        .expect("library");
        Self {
            base,
            library,
            changes: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn media(&self, name: &str) -> PathBuf {
        let path = self.base.path().join(name);
        fs::create_dir_all(&path).expect("media");
        path
    }

    fn start(&self) -> WatchService {
        let changes = Arc::clone(&self.changes);
        WatchService::start(
            WatchConfig {
                quiet: Duration::from_millis(200),
                max_wait: Duration::from_secs(1),
                health_interval: Duration::from_millis(300),
                ..WatchConfig::new(
                    self.base.path().join("library.sqlite3"),
                    self.base.path().join("cache"),
                )
            },
            Arc::new(move || {
                changes.fetch_add(1, Ordering::Relaxed);
            }),
        )
        .expect("watch service")
    }

    fn statuses(&self, root_id: i64) -> Vec<(String, LocationStatus)> {
        let mut statuses: Vec<_> = self
            .library
            .store()
            .locations()
            .list_for_root(root_id)
            .expect("locations")
            .into_iter()
            .map(|location| (location.relative_path, location.status))
            .collect();
        statuses.sort_by(|left, right| left.0.cmp(&right.0));
        statuses
    }

    fn root_status(&self, root_id: i64) -> RootStatus {
        self.library
            .store()
            .roots()
            .get(root_id)
            .expect("root")
            .status
    }
}

fn wait_until(condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    condition()
}

fn write_image(dir: &Path, name: &str, value: u8) {
    fs::write(dir.join(name), png_bytes(value)).expect("write image");
}

#[test]
fn startup_scans_roots_that_changed_while_the_app_was_closed() {
    let fixture = Fixture::new();
    let media = fixture.media("photos");
    write_image(&media, "one.png", 10);
    let root = fixture.library.add_root(&media, "写真").expect("root");
    fixture.library.sync_root(root.id).expect("first scan");
    // Changes made while nothing watched.
    write_image(&media, "two.png", 20);
    fs::remove_file(media.join("one.png")).expect("remove");

    let service = fixture.start();
    assert!(wait_until(|| fixture.statuses(root.id)
        == vec![
            ("one.png".to_owned(), LocationStatus::Missing),
            ("two.png".to_owned(), LocationStatus::Present),
        ]));
    assert!(fixture.changes.load(Ordering::Relaxed) > 0);
    service.stop();
}

#[test]
fn changes_while_running_are_reflected_after_the_debounce() {
    let fixture = Fixture::new();
    let media = fixture.media("photos");
    let root = fixture.library.add_root(&media, "写真").expect("root");
    let service = fixture.start();
    assert!(wait_until(
        || fixture.root_status(root.id) == RootStatus::Online
    ));
    std::thread::sleep(Duration::from_millis(400));

    fs::create_dir_all(media.join("trip")).expect("subdir");
    write_image(&media, "trip/a.png", 30);
    assert!(wait_until(
        || fixture.statuses(root.id) == vec![("trip/a.png".to_owned(), LocationStatus::Present)]
    ));

    // A rename keeps the asset and moves its location.
    fs::rename(media.join("trip/a.png"), media.join("trip/b.png")).expect("rename");
    assert!(wait_until(|| fixture.statuses(root.id)
        == vec![
            ("trip/a.png".to_owned(), LocationStatus::Missing),
            ("trip/b.png".to_owned(), LocationStatus::Present),
        ]));
    assert_eq!(fixture.library.store().assets().count().expect("count"), 1);

    fs::remove_file(media.join("trip/b.png")).expect("delete");
    assert!(wait_until(|| fixture
        .statuses(root.id)
        .iter()
        .all(|(_, status)| *status == LocationStatus::Missing)));
    service.stop();
}

#[test]
fn a_disconnected_root_goes_offline_and_recovers() {
    let fixture = Fixture::new();
    let media = fixture.media("volume");
    write_image(&media, "one.png", 40);
    let root = fixture.library.add_root(&media, "外付け").expect("root");
    let service = fixture.start();
    assert!(wait_until(
        || fixture.statuses(root.id) == vec![("one.png".to_owned(), LocationStatus::Present)]
    ));

    // Moving the directory away stands in for ejecting the volume.
    let parked = fixture.base.path().join("parked");
    fs::rename(&media, &parked).expect("eject");
    assert!(wait_until(
        || fixture.root_status(root.id) == RootStatus::Offline
    ));
    assert!(wait_until(
        || fixture.statuses(root.id) == vec![("one.png".to_owned(), LocationStatus::Offline)]
    ));
    assert_eq!(fixture.library.store().assets().count().expect("assets"), 1);

    fs::rename(&parked, &media).expect("reconnect");
    assert!(wait_until(
        || fixture.root_status(root.id) == RootStatus::Online
    ));
    assert!(wait_until(
        || fixture.statuses(root.id) == vec![("one.png".to_owned(), LocationStatus::Present)]
    ));
    service.stop();
}

#[test]
fn roots_added_later_are_watched_after_a_refresh_and_disabled_roots_are_not() {
    let fixture = Fixture::new();
    let service = fixture.start();
    let disabled_media = fixture.media("disabled");
    write_image(&disabled_media, "skip.png", 50);
    let disabled = fixture
        .library
        .add_root(&disabled_media, "無効")
        .expect("disabled root");
    fixture
        .library
        .set_root_enabled(disabled.id, false)
        .expect("disable");
    let media = fixture.media("later");
    write_image(&media, "late.png", 60);
    let root = fixture.library.add_root(&media, "あとから").expect("root");
    service.refresh();

    assert!(wait_until(
        || fixture.statuses(root.id) == vec![("late.png".to_owned(), LocationStatus::Present)]
    ));
    std::thread::sleep(Duration::from_millis(500));
    assert!(fixture.statuses(disabled.id).is_empty());
    drop(service);
}
