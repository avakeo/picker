use std::fs;

use image::{DynamicImage, Rgba, RgbaImage};

use super::*;
use picker_store::Stage;

fn png_bytes(value: u8) -> Vec<u8> {
    let mut image = RgbaImage::new(4, 3);
    for pixel in image.pixels_mut() {
        *pixel = Rgba([value, value / 2, 20, 255]);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode png");
    bytes.into_inner()
}

#[test]
fn scans_roots_and_pages_assets() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::create_dir_all(media.path().join("nested")).expect("nested");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.path().join("nested/two.png"), png_bytes(20)).expect("write");
    fs::write(media.path().join("nested/copy.png"), png_bytes(10)).expect("write");

    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "テスト").expect("add root");
    let report = library.scan_root(root.id).expect("scan");
    assert_eq!(report.added, 3);
    let roots = library.list_roots().expect("roots");
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].image_count, 3);

    let page = library.asset_page(0, 1).expect("page");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.total, 2);
    assert!(page.next_cursor.is_some());
    let second = library
        .asset_page(page.next_cursor.expect("cursor"), 10)
        .expect("second page");
    assert_eq!(second.items.len(), 1);
    assert!(second.next_cursor.is_none());

    let duplicates = library
        .store()
        .assets()
        .exact_duplicates()
        .expect("duplicates");
    assert_eq!(duplicates.len(), 1);
    let detail = library.asset_detail(duplicates[0].id).expect("detail");
    assert_eq!(detail.locations.len(), 2);
    assert_eq!(detail.summary.thumbnail, ThumbnailState::Ready);

    let status = library.index_status().expect("status");
    assert_eq!(status.total_assets, 2);
    assert_eq!(status.stages.len(), Stage::ordered().len());
    assert!(
        library
            .prioritize_viewport(&[duplicates[0].id])
            .expect("priority")
            >= 1
    );
}

#[test]
fn rejects_overlapping_roots_and_non_directories() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("add root");
    assert!(library
        .add_root(media.path().join("child"), "child")
        .is_err());

    let file = media.path().join("file.png");
    fs::write(&file, b"x").expect("write");
    assert!(library.add_root(&file, "file").is_err());
    assert!(library.remove_root(root.id).expect("remove root"));
    assert!(library.list_roots().expect("roots").is_empty());
}

#[test]
fn invalid_asset_ids_return_errors() {
    let cache = tempfile::tempdir().expect("cache");
    let library = Library::open_in_memory(cache.path()).expect("library");
    assert!(library.asset_detail(42).is_err());
    assert_eq!(library.prioritize_viewport(&[42]).expect("priority"), 0);
}
