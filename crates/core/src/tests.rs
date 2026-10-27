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

#[test]
fn applies_tags_and_filters_by_parent() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(1)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(2)).expect("write");
    fs::write(media.path().join("three.png"), png_bytes(3)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("add root");
    library.scan_root(root.id).expect("scan");
    let page = library.asset_page(0, 10).expect("page");
    assert_eq!(page.items.len(), 3);

    let animal = library
        .create_tag("animal", "動物", None, None)
        .expect("parent tag");
    let dog = library
        .create_tag("dog", "犬", Some(animal.id), Some("#112233"))
        .expect("child tag");
    let updated = library
        .set_tag_on_assets(
            &[page.items[0].id, page.items[1].id],
            dog.id,
            picker_store::TagState::Confirmed,
        )
        .expect("assign");
    assert_eq!(updated, 2);

    let filter = picker_store::AssetFilter {
        tags: picker_store::TagFilter {
            tag_ids: vec![animal.id],
            mode: picker_store::TagMatchMode::All,
            include_suggested: false,
        },
        ..picker_store::AssetFilter::default()
    };
    let filtered = library.filter_page(&filter, 0, 10).expect("filter");
    assert_eq!(filtered.items.len(), 2);
    assert_eq!(filtered.total, 2);

    let detail = library.asset_detail(page.items[0].id).expect("detail");
    assert_eq!(detail.tags.len(), 1);
    assert_eq!(detail.tags[0].tag.display_name_ja, "犬");

    let tags = library.list_tags().expect("tags");
    assert_eq!(tags.len(), 2);
    assert_eq!(
        tags.iter()
            .find(|tag| tag.tag.id == dog.id)
            .expect("dog count")
            .asset_count,
        2
    );

    assert_eq!(
        library
            .remove_tag_from_assets(&[page.items[0].id], dog.id)
            .expect("remove"),
        1
    );
    assert!(library.delete_tag(dog.id).expect("delete"));
    assert!(library.list_tags().expect("tags").len() == 1);

    let duplicate_filter = picker_store::AssetFilter::default();
    assert_eq!(
        library
            .filter_page(&duplicate_filter, 0, 10)
            .expect("all")
            .items
            .len(),
        3
    );
    assert!(library.exact_duplicate_assets().expect("dupes").is_empty());
}

#[test]
fn tag_hierarchy_and_alias_errors_are_typed() {
    let cache = tempfile::tempdir().expect("cache");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.create_tag("root", "根", None, None).expect("tag");
    let child = library
        .create_tag("child", "子", Some(root.id), None)
        .expect("child");
    assert!(library.set_tag_parent(root.id, Some(child.id)).is_err());
    library
        .add_tag_alias(child.id, "こども")
        .expect("add alias");
    assert!(library.add_tag_alias(root.id, "こども").is_err());
    assert_eq!(library.tag_aliases(child.id).expect("aliases").len(), 1);
    assert!(library
        .set_tag_on_assets(&[999], child.id, picker_store::TagState::Confirmed)
        .is_err());
}
