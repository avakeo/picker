use std::fs;

use image::{DynamicImage, Rgba, RgbaImage};

use super::*;
use picker_imaging::ThumbnailSize;
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
    // scan_root finishes the image work; only the embedding job, which
    // waits for a model, is left to reorder.
    assert_eq!(
        library
            .prioritize_viewport(&[duplicates[0].id])
            .expect("priority"),
        1
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
            all: vec![animal.id],
            ..picker_store::TagFilter::default()
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

#[test]
fn extracts_and_manually_overrides_colors() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(30)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let asset = library.asset_page(0, 10).expect("page").items[0].id;
    let detail = library.asset_detail(asset).expect("detail");
    assert_eq!(detail.colors.len(), 1);
    assert_eq!(detail.colors[0].source, picker_store::ColorSource::Auto);

    library
        .set_asset_colors(asset, &[("#010203".to_owned(), 1.0)])
        .expect("manual");
    let detail = library.asset_detail(asset).expect("detail");
    assert_eq!(detail.colors[0].hex, "#010203");
    assert_eq!(detail.colors[0].source, picker_store::ColorSource::User);

    let reset = library.reset_asset_colors(asset).expect("reset");
    assert_eq!(reset.len(), 1);
    assert_eq!(reset[0].source, picker_store::ColorSource::Auto);
    assert_ne!(reset[0].hex, "#010203");
    assert!(library
        .set_asset_colors(asset, &[("not-hex".to_owned(), 1.0)])
        .is_err());
}

#[test]
fn albums_keep_order_and_survive_root_removal() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(5)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(6)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let page = library.asset_page(0, 10).expect("page");
    let album = library.create_album("旅行").expect("album");
    assert_eq!(
        library
            .add_assets_to_album(album.id, &[page.items[0].id, page.items[1].id])
            .expect("add"),
        2
    );
    let items = library.album_assets(album.id).expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, page.items[0].id);

    library
        .reorder_album(album.id, &[page.items[1].id, page.items[0].id])
        .expect("reorder");
    assert_eq!(
        library.album_asset_ids(album.id).expect("ids"),
        vec![page.items[1].id, page.items[0].id]
    );

    library.remove_root(root.id).expect("remove root");
    assert_eq!(library.album_assets(album.id).expect("items").len(), 2);
    assert!(library.delete_album(album.id).expect("delete"));
    assert!(library.album_assets(album.id).is_err());
    assert_eq!(library.asset_page(0, 10).expect("assets").items.len(), 2);
}

struct MockEmbedder;

impl picker_ml::Embedder for MockEmbedder {
    fn embed_image(
        &self,
        _path: &std::path::Path,
    ) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        Ok(vec![1.0, 0.0])
    }

    fn embed_text(&self, _text: &str) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        Ok(vec![1.0, 0.0])
    }

    fn dimensions(&self) -> usize {
        2
    }

    fn model_id(&self) -> &str {
        "mock"
    }

    fn model_version(&self) -> &str {
        "v1"
    }
}

#[test]
fn embeddings_are_indexed_updated_and_searched() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(7)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(8)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    assert!(library.index_embeddings(10).is_err());

    library
        .set_embedder(Some(std::sync::Arc::new(MockEmbedder)))
        .expect("embedder");
    let indexed = library.index_embeddings(10).expect("index");
    assert_eq!(indexed, 2);
    let results = library.semantic_search("anything", 10).expect("search");
    assert_eq!(results.len(), 2);
    let ids = library.asset_page(0, 10).expect("page").items;
    assert_eq!(
        library.similar_assets(ids[0].id, 10).expect("similar"),
        vec![(ids[1].id, 1.0)]
    );

    library.set_embedder(None).expect("clear");
    assert!(library.semantic_search("anything", 10).is_err());
}

#[test]
#[ignore = "requires the downloaded clip-japanese ONNX model"]
fn clip_model_indexes_and_searches_end_to_end() {
    let root = std::env::var_os("PICKER_CLIP_JAPANESE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/home/agent/models/clip-japanese"));
    if !root.join("clyp_visual.onnx").exists() {
        eprintln!("skipping: no model at {}", root.display());
        return;
    }
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("red.png"), png_bytes(240)).expect("write");
    fs::write(media.path().join("dark.png"), png_bytes(20)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root_id = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root_id.id).expect("scan");
    let embedder = picker_ml::ClipJapanese::open(&root, "clip-japanese", "77a62f8").expect("model");
    library
        .set_embedder(Some(std::sync::Arc::new(embedder)))
        .expect("embedder");
    assert_eq!(library.index_embeddings(10).expect("index"), 2);
    let progress = library
        .embedding_progress()
        .expect("progress")
        .expect("active");
    assert_eq!(progress.dimensions, 512);
    assert_eq!(progress.indexed, 2);
    assert_eq!(progress.missing, 0);
    let results = library.semantic_search("暗い画像", 2).expect("search");
    assert_eq!(results.len(), 2);
    let first = results[0].0;
    assert_eq!(library.similar_assets(first, 1).expect("similar").len(), 1);
}

#[test]
fn tag_name_matches_outrank_semantic_results() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(42)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let items = library.asset_page(0, 10).expect("page").items;
    let tagged = items.iter().max_by_key(|asset| asset.id).expect("asset").id;
    let other = items.iter().min_by_key(|asset| asset.id).expect("asset").id;
    let tag = library.create_tag("海辺", "海辺", None, None).expect("tag");
    library
        .set_tag_on_assets(&[tagged], tag.id, picker_store::TagState::Confirmed)
        .expect("assign");
    library
        .set_embedder(Some(std::sync::Arc::new(MockEmbedder)))
        .expect("embed");
    library.index_embeddings(10).expect("index");
    let results = library.search_text("海辺", 10).expect("search");
    assert_eq!(results.first().expect("first").0, tagged);
    assert_eq!(results[1].0, other);
}

#[test]
fn zero_shot_suggestions_never_replace_rejected_tags() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let asset_id = library.asset_page(0, 10).expect("page").items[0].id;
    library
        .set_embedder(Some(std::sync::Arc::new(MockEmbedder)))
        .expect("embed");
    library.index_embeddings(10).expect("index");
    let suggested = library.auto_tag(10).expect("auto tag");
    assert!(suggested > 0);
    let tags = library.asset_detail(asset_id).expect("detail").tags;
    let suggestion = tags
        .iter()
        .find(|tag| tag.assignment.source == picker_store::AssignmentSource::ZeroShot)
        .expect("suggestion")
        .tag
        .id;
    library
        .set_tag_on_assets(&[asset_id], suggestion, picker_store::TagState::Rejected)
        .expect("reject");
    library
        .store()
        .stage_states()
        .upsert(
            picker_store::StageTarget::Asset(asset_id),
            picker_store::Stage::AutoTag,
            picker_store::StageStatus::Failed,
            "zero-shot-v1",
            Some("retry"),
        )
        .expect("reset stage");
    library.auto_tag(10).expect("retry");
    let state = library
        .store()
        .tag_assignments()
        .get(asset_id, suggestion)
        .expect("assignment");
    assert_eq!(state.state, picker_store::TagState::Rejected);
}

#[test]
fn feedback_events_record_decisions_and_can_be_undone() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let asset_id = library.asset_page(0, 10).expect("page").items[0].id;
    let tag = library.create_tag("cat", "猫", None, None).expect("tag");
    library
        .store()
        .tag_assignments()
        .upsert_suggested(
            asset_id,
            tag.id,
            picker_store::AssignmentSource::ZeroShot,
            0.81,
            "clip",
            "v1",
        )
        .expect("suggest");
    library
        .set_tag_on_assets(&[asset_id], tag.id, picker_store::TagState::Confirmed)
        .expect("confirm");
    let events = library
        .store()
        .feedback()
        .list_for_asset(asset_id)
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, picker_store::FeedbackAction::Approved);
    assert_eq!(events[0].confidence, Some(0.81));
    library.undo_feedback(events[0].id).expect("undo");
    let restored = library
        .store()
        .tag_assignments()
        .get(asset_id, tag.id)
        .expect("restored");
    assert_eq!(restored.state, picker_store::TagState::Suggested);
    assert_eq!(restored.confidence, Some(0.81));

    library
        .set_tag_on_assets(&[asset_id], tag.id, picker_store::TagState::Confirmed)
        .expect("confirm again");
    library
        .remove_tag_from_assets(&[asset_id], tag.id)
        .expect("remove user");
    assert!(library
        .store()
        .tag_assignments()
        .get_optional(asset_id, tag.id)
        .expect("optional")
        .is_none());
}

struct PathEmbedder;

impl picker_ml::Embedder for PathEmbedder {
    fn embed_image(
        &self,
        path: &std::path::Path,
    ) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        Ok(match name {
            "one.png" | "three.png" | "five.png" => vec![1.0, 0.0],
            "blue.png" => vec![0.0, 1.0],
            "cyan.png" => vec![0.1, 0.9],
            _ => vec![0.0, 1.0],
        })
    }

    fn embed_text(&self, _text: &str) -> std::result::Result<Vec<f32>, picker_ml::EmbeddingError> {
        Ok(vec![1.0, 0.0])
    }

    fn dimensions(&self) -> usize {
        2
    }

    fn model_id(&self) -> &str {
        "path"
    }

    fn model_version(&self) -> &str {
        "1"
    }
}

fn asset_id_named(library: &Library, name: &str) -> i64 {
    for asset in library.asset_page(0, 100).expect("page").items {
        if library
            .store()
            .locations()
            .list_for_asset(asset.id)
            .expect("locations")
            .iter()
            .any(|location| location.relative_path.ends_with(name))
        {
            return asset.id;
        }
    }
    panic!("asset {name} not found");
}

#[test]
fn custom_prototype_suggestions_follow_confirmed_examples() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    for (index, name) in ["one.png", "three.png", "five.png", "blue.png", "cyan.png"]
        .into_iter()
        .enumerate()
    {
        fs::write(media.path().join(name), png_bytes(10 + index as u8)).expect("write");
    }
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    library
        .set_embedder(Some(std::sync::Arc::new(PathEmbedder)))
        .expect("embedder");
    library.index_embeddings(10).expect("index");
    let tag = library
        .create_tag("robot", "ロボット", None, None)
        .expect("tag");
    let tag = library.convert_tag_to_custom(tag.id).expect("custom");
    let positives = ["one.png", "three.png", "five.png"].map(|name| asset_id_named(&library, name));
    library
        .set_tag_on_assets(&positives, tag.id, picker_store::TagState::Confirmed)
        .expect("positives");
    library
        .suggest_custom_tag(tag.id, 10)
        .expect("first suggest");
    let cyan = asset_id_named(&library, "cyan.png");
    assert!(library
        .store()
        .tag_assignments()
        .get_optional(cyan, tag.id)
        .expect("assignment")
        .is_none());

    let blue = asset_id_named(&library, "blue.png");
    library
        .set_tag_on_assets(&[blue], tag.id, picker_store::TagState::Confirmed)
        .expect("new positive");
    library
        .suggest_custom_tag(tag.id, 10)
        .expect("second suggest");
    let suggestion = library
        .store()
        .tag_assignments()
        .get(cyan, tag.id)
        .expect("candidate");
    assert_eq!(suggestion.state, picker_store::TagState::Suggested);
    assert_eq!(
        suggestion.source,
        picker_store::AssignmentSource::CustomClassifier
    );
}

#[test]
fn custom_classifier_switches_on_after_ten_examples() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    for index in 0..10 {
        fs::write(
            media.path().join(format!("pos{index}.png")),
            png_bytes(index),
        )
        .expect("write");
        fs::write(
            media.path().join(format!("neg{index}.png")),
            png_bytes(100 + index),
        )
        .expect("write");
    }
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    library
        .set_embedder(Some(std::sync::Arc::new(PathEmbedder)))
        .expect("embedder");
    library.index_embeddings(50).expect("index");
    let tag = library
        .create_tag("robot", "ロボット", None, None)
        .expect("tag");
    let tag = library.convert_tag_to_custom(tag.id).expect("custom");
    let mut positives = Vec::new();
    let mut negatives = Vec::new();
    for index in 0..10 {
        positives.push(asset_id_named(&library, &format!("pos{index}.png")));
        negatives.push(asset_id_named(&library, &format!("neg{index}.png")));
    }
    library
        .set_tag_on_assets(&positives, tag.id, picker_store::TagState::Confirmed)
        .expect("positives");
    library
        .set_tag_on_assets(&negatives, tag.id, picker_store::TagState::Rejected)
        .expect("negatives");
    library.suggest_custom_tag(tag.id, 100).expect("suggest");
    assert!(library
        .store()
        .classifiers()
        .get(tag.id)
        .expect("classifier")
        .is_some());
    for asset_id in positives {
        let probability = library
            .store()
            .tag_assignments()
            .get_optional(asset_id, tag.id)
            .expect("assignment");
        assert_eq!(
            probability.expect("confirmed").state,
            picker_store::TagState::Confirmed
        );
    }
}

#[test]
fn review_confirm_approves_kept_rejects_excluded_and_undoes() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(80)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let assets = library.asset_page(0, 10).expect("page").items;
    let tag = library
        .create_tag("beach", "海辺", None, None)
        .expect("tag");
    for asset in &assets {
        library
            .store()
            .tag_assignments()
            .upsert_suggested(
                asset.id,
                tag.id,
                picker_store::AssignmentSource::ZeroShot,
                0.7,
                "clip",
                "v1",
            )
            .expect("suggest");
    }
    let candidates = library.review_candidates(tag.id, 20, 0.5).expect("review");
    assert_eq!(candidates.len(), 2);
    let keep = candidates[0];
    let reject = candidates[1];
    let events = library
        .confirm_review(tag.id, &[keep], &[reject])
        .expect("confirm");
    assert_eq!(events.len(), 2);
    assert_eq!(
        library
            .store()
            .tag_assignments()
            .get(keep, tag.id)
            .expect("keep")
            .state,
        picker_store::TagState::Confirmed
    );
    assert_eq!(
        library
            .store()
            .tag_assignments()
            .get(reject, tag.id)
            .expect("reject")
            .state,
        picker_store::TagState::Rejected
    );
    for event in events.into_iter().rev() {
        library.undo_feedback(event).expect("undo");
    }
    assert_eq!(
        library
            .store()
            .tag_assignments()
            .get(keep, tag.id)
            .expect("keep")
            .state,
        picker_store::TagState::Suggested
    );
    assert_eq!(
        library
            .store()
            .tag_assignments()
            .get(reject, tag.id)
            .expect("reject")
            .state,
        picker_store::TagState::Suggested
    );
}

#[test]
fn portable_export_restores_tags_after_rescan() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(60)).expect("write");
    let export_dir = tempfile::tempdir().expect("export");
    let export_path = export_dir.path().join("decisions.json");
    let source = Library::open_in_memory(cache.path()).expect("library");
    let root = source.add_root(media.path(), "root").expect("root");
    source.scan_root(root.id).expect("scan");
    let tagged = asset_id_named(&source, "one.png");
    let tag = source.create_tag("beach", "海辺", None, None).expect("tag");
    source
        .set_tag_on_assets(&[tagged], tag.id, picker_store::TagState::Confirmed)
        .expect("assign");
    source.export_portable(&export_path).expect("export");

    let cache2 = tempfile::tempdir().expect("cache2");
    let restored = Library::open_in_memory(cache2.path()).expect("library2");
    restored.import_portable(&export_path).expect("import");
    // The import registers the exported root because the folder exists here.
    let root = restored.list_roots().expect("roots")[0].root.clone();
    restored.scan_root(root.id).expect("scan");
    let restored_asset = asset_id_named(&restored, "one.png");
    let tags = restored.asset_detail(restored_asset).expect("detail").tags;
    assert!(tags.iter().any(|assigned| {
        assigned.tag.canonical_name == "beach"
            && assigned.assignment.state == picker_store::TagState::Confirmed
    }));
}

#[test]
fn portable_reexport_keeps_decisions_waiting_for_assets() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(60)).expect("write");
    let export_dir = tempfile::tempdir().expect("export");
    let first_export = export_dir.path().join("first.json");
    let second_export = export_dir.path().join("second.json");
    let source = Library::open_in_memory(cache.path()).expect("library");
    let root = source.add_root(media.path(), "root").expect("root");
    source.scan_root(root.id).expect("scan");
    let confirmed = asset_id_named(&source, "one.png");
    let rejected = asset_id_named(&source, "two.png");
    let tag = source.create_tag("beach", "海辺", None, None).expect("tag");
    source
        .set_tag_on_assets(&[confirmed], tag.id, picker_store::TagState::Confirmed)
        .expect("confirm");
    source
        .set_tag_on_assets(&[rejected], tag.id, picker_store::TagState::Rejected)
        .expect("reject");
    source.export_portable(&first_export).expect("export");

    // Re-export from the imported library before any image is scanned.
    let cache2 = tempfile::tempdir().expect("cache2");
    let restored = Library::open_in_memory(cache2.path()).expect("library2");
    restored.import_portable(&first_export).expect("import");
    assert_eq!(
        restored
            .store()
            .portable()
            .pending_count()
            .expect("pending"),
        2
    );
    restored.export_portable(&second_export).expect("re-export");

    let first: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&first_export).expect("read")).expect("json");
    let second: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&second_export).expect("read")).expect("json");
    assert_eq!(first["assignments"], second["assignments"]);
    assert_eq!(second["assignments"].as_array().expect("array").len(), 2);
}

fn suggest(library: &Library, asset_id: i64, tag_id: i64) {
    library
        .store()
        .tag_assignments()
        .upsert_suggested(
            asset_id,
            tag_id,
            picker_store::AssignmentSource::ZeroShot,
            0.5,
            "clip",
            "v1",
        )
        .expect("suggest");
}

fn assigned_state(library: &Library, asset_id: i64, tag_id: i64) -> picker_store::TagState {
    library
        .store()
        .tag_assignments()
        .get(asset_id, tag_id)
        .expect("assignment")
        .state
}

#[test]
fn portable_import_replaces_suggestions_but_keeps_existing_decisions() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(60)).expect("write");
    let export_dir = tempfile::tempdir().expect("export");
    let export_path = export_dir.path().join("decisions.json");
    let source = Library::open_in_memory(cache.path()).expect("library");
    let root = source.add_root(media.path(), "root").expect("root");
    source.scan_root(root.id).expect("scan");
    let tag = source.create_tag("beach", "海辺", None, None).expect("tag");
    for name in ["one.png", "two.png"] {
        let asset_id = asset_id_named(&source, name);
        source
            .set_tag_on_assets(&[asset_id], tag.id, picker_store::TagState::Rejected)
            .expect("reject");
    }
    source.export_portable(&export_path).expect("export");

    let cache2 = tempfile::tempdir().expect("cache2");
    let restored = Library::open_in_memory(cache2.path()).expect("library2");
    let root = restored.add_root(media.path(), "root").expect("root");
    restored.scan_root(root.id).expect("scan");
    let tag = restored
        .create_tag("beach", "海辺", None, None)
        .expect("tag");
    let suggested = asset_id_named(&restored, "one.png");
    let confirmed = asset_id_named(&restored, "two.png");
    suggest(&restored, suggested, tag.id);
    restored
        .set_tag_on_assets(&[confirmed], tag.id, picker_store::TagState::Confirmed)
        .expect("confirm");

    restored.import_portable(&export_path).expect("import");

    assert_eq!(
        assigned_state(&restored, suggested, tag.id),
        picker_store::TagState::Rejected
    );
    assert_eq!(
        assigned_state(&restored, confirmed, tag.id),
        picker_store::TagState::Confirmed
    );
    assert_eq!(
        restored
            .store()
            .portable()
            .pending_count()
            .expect("pending"),
        0
    );
}

#[test]
fn rescan_applies_staged_decision_over_suggestion() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let asset_id = asset_id_named(&library, "one.png");
    let tag = library
        .create_tag("beach", "海辺", None, None)
        .expect("tag");
    suggest(&library, asset_id, tag.id);
    let hash = library
        .store()
        .assets()
        .get(asset_id)
        .expect("asset")
        .blake3_hash;
    library
        .store()
        .portable()
        .stage_assignment(
            &hash,
            tag.id,
            picker_store::TagState::Confirmed,
            picker_store::AssignmentSource::User,
            None,
            None,
            None,
        )
        .expect("stage");

    library.scan_root(root.id).expect("rescan");

    assert_eq!(
        assigned_state(&library, asset_id, tag.id),
        picker_store::TagState::Confirmed
    );
    assert_eq!(
        library.store().portable().pending_count().expect("pending"),
        0
    );
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read")).expect("json")
}

/// A library with one root, a manual album, user colors, and feedback history.
fn library_with_extras(media: &std::path::Path, cache: &std::path::Path) -> Library {
    fs::write(media.join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.join("two.png"), png_bytes(60)).expect("write");
    let library = Library::open_in_memory(cache).expect("library");
    let root = library.add_root(media, "写真").expect("root");
    library.scan_root(root.id).expect("scan");
    let one = asset_id_named(&library, "one.png");
    let two = asset_id_named(&library, "two.png");
    let album = library.create_album("旅行").expect("album");
    library
        .add_assets_to_album(album.id, &[two, one])
        .expect("album items");
    library
        .set_asset_colors(one, &[("#112233".to_owned(), 0.7)])
        .expect("colors");
    let tag = library
        .create_tag("beach", "海辺", None, None)
        .expect("tag");
    library
        .set_tag_on_assets(&[one], tag.id, picker_store::TagState::Confirmed)
        .expect("confirm");
    library
}

#[test]
fn portable_export_restores_albums_colors_feedback_and_roots() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    let source = library_with_extras(media.path(), cache.path());
    let export_dir = tempfile::tempdir().expect("export");
    let first = export_dir.path().join("first.json");
    let second = export_dir.path().join("second.json");
    source.export_portable(&first).expect("export");
    let exported = read_json(&first);
    assert_eq!(exported["version"], crate::PORTABLE_EXPORT_VERSION);
    assert!(!exported["feedbackEvents"]
        .as_array()
        .expect("feedback")
        .is_empty());

    let cache2 = tempfile::tempdir().expect("cache2");
    let restored = Library::open_in_memory(cache2.path()).expect("library2");
    restored.import_portable(&first).expect("import");

    // Before the scan, a re-export keeps every staged record.
    restored.export_portable(&second).expect("re-export");
    let reexported = read_json(&second);
    for field in ["albums", "userColors", "feedbackEvents", "roots"] {
        assert_eq!(exported[field], reexported[field], "{field}");
    }

    let roots = restored.list_roots().expect("roots");
    assert_eq!(roots.len(), 1);
    restored.scan_root(roots[0].root.id).expect("scan");
    let one = asset_id_named(&restored, "one.png");
    let two = asset_id_named(&restored, "two.png");
    let album = restored.list_albums().expect("albums")[0].album.clone();
    assert_eq!(album.name, "旅行");
    assert_eq!(
        restored.album_asset_ids(album.id).expect("items"),
        vec![two, one]
    );
    let colors = restored.store().colors().list(one).expect("colors");
    assert_eq!(colors.len(), 1);
    assert_eq!(colors[0].hex, "#112233");
    assert_eq!(colors[0].source, picker_store::ColorSource::User);
    assert_eq!(
        restored
            .store()
            .feedback()
            .list_all()
            .expect("feedback")
            .len(),
        source
            .store()
            .feedback()
            .list_all()
            .expect("feedback")
            .len()
    );
    assert!(restored
        .store()
        .portable()
        .pending_items()
        .expect("pending")
        .is_empty());
}

#[test]
fn portable_import_twice_does_not_duplicate_records() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    let library = library_with_extras(media.path(), cache.path());
    let export_dir = tempfile::tempdir().expect("export");
    let path = export_dir.path().join("decisions.json");
    library.export_portable(&path).expect("export");
    let events = library
        .store()
        .feedback()
        .list_all()
        .expect("feedback")
        .len();
    let album = library.list_albums().expect("albums")[0].album.clone();
    let items = library.album_asset_ids(album.id).expect("items");

    library.import_portable(&path).expect("import");
    library.import_portable(&path).expect("import again");

    assert_eq!(
        library
            .store()
            .feedback()
            .list_all()
            .expect("feedback")
            .len(),
        events
    );
    assert_eq!(library.list_albums().expect("albums").len(), 1);
    assert_eq!(library.album_asset_ids(album.id).expect("items"), items);
    assert_eq!(library.list_roots().expect("roots").len(), 1);
}

#[test]
fn portable_import_accepts_version_one_files() {
    let cache = tempfile::tempdir().expect("cache");
    let export_dir = tempfile::tempdir().expect("export");
    let path = export_dir.path().join("v1.json");
    fs::write(
        &path,
        r#"{"version":1,"tags":[],"aliases":[],"classifiers":[],"assignments":[]}"#,
    )
    .expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    assert_eq!(library.import_portable(&path).expect("import"), 0);
}

#[test]
fn portable_import_rejects_bad_hashes_and_unknown_versions_without_writing() {
    let cache = tempfile::tempdir().expect("cache");
    let export_dir = tempfile::tempdir().expect("export");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let bad_hash = export_dir.path().join("bad-hash.json");
    fs::write(
        &bad_hash,
        r#"{"version":2,"tags":[],"aliases":[],"classifiers":[],"assignments":[],
            "albums":[{"name":"旅行","sortOrder":0,"items":["zz"]}]}"#,
    )
    .expect("write");
    assert!(library.import_portable(&bad_hash).is_err());
    assert!(library.list_albums().expect("albums").is_empty());

    let future = export_dir.path().join("future.json");
    fs::write(
        &future,
        r#"{"version":3,"tags":[],"aliases":[],"classifiers":[],"assignments":[]}"#,
    )
    .expect("write");
    assert!(library.import_portable(&future).is_err());
    assert!(library
        .import_portable(&export_dir.path().join("missing.json"))
        .is_err());
}

#[test]
fn snapshots_are_written_when_due_and_rotated() {
    let cache = tempfile::tempdir().expect("cache");
    let snapshots = tempfile::tempdir().expect("snapshots");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let day = 24 * 60 * 60;
    let first = library
        .snapshot_if_due(snapshots.path(), 1_000, day, 2)
        .expect("first");
    assert!(first.as_ref().is_some_and(|path| path.is_file()));
    assert!(library
        .snapshot_if_due(snapshots.path(), 1_000 + day - 1, day, 2)
        .expect("not due")
        .is_none());
    library
        .snapshot_if_due(snapshots.path(), 1_000 + day, day, 2)
        .expect("second");
    library
        .snapshot_if_due(snapshots.path(), 1_000 + 2 * day, day, 2)
        .expect("third");
    let mut names: Vec<String> = fs::read_dir(snapshots.path())
        .expect("dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("utf8")
        })
        .collect();
    names.sort_by_key(|name| name.len());
    assert_eq!(
        names,
        vec![
            format!("library-{}.sqlite3", 1_000 + day),
            format!("library-{}.sqlite3", 1_000 + 2 * day),
        ]
    );
    let copy = picker_store::Store::open(snapshots.path().join(&names[1])).expect("open copy");
    assert_eq!(
        copy.schema_version().expect("version"),
        picker_store::LATEST_SCHEMA_VERSION
    );
}

/// A PNG whose left `main_percent` columns are `main` and the rest `other`.
fn two_color_png(main: [u8; 3], other: [u8; 3], main_percent: u32) -> Vec<u8> {
    let mut image = RgbaImage::new(100, 40);
    for (x, _, pixel) in image.enumerate_pixels_mut() {
        let [r, g, b] = if x < main_percent { main } else { other };
        *pixel = Rgba([r, g, b, 255]);
    }
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode png");
    bytes.into_inner()
}

fn color_ids(library: &Library, palette_ids: &[&str], scope: crate::ColorScope) -> Vec<i64> {
    let mut filter = picker_store::AssetFilter::default();
    library
        .restrict_to_colors(
            &mut filter,
            &crate::ColorCondition {
                palette_ids: palette_ids.iter().map(|id| (*id).to_owned()).collect(),
                scope,
            },
        )
        .expect("colors");
    let mut ids: Vec<i64> = library
        .filter_page(&filter, 0, 100)
        .expect("filter")
        .items
        .into_iter()
        .map(|item| item.id)
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn color_search_returns_only_images_near_the_selected_color() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    let red = [220, 30, 30];
    let blue = [30, 90, 220];
    let green = [40, 160, 60];
    fs::write(media.path().join("red.png"), two_color_png(red, red, 100)).expect("write");
    fs::write(
        media.path().join("blue.png"),
        two_color_png(blue, blue, 100),
    )
    .expect("write");
    // Mostly blue with a red part: red is not the main color.
    fs::write(media.path().join("mixed.png"), two_color_png(blue, red, 70)).expect("write");
    fs::write(
        media.path().join("green.png"),
        two_color_png(green, green, 100),
    )
    .expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let id = |name| asset_id_named(&library, name);
    let mut red_any = vec![id("red.png"), id("mixed.png")];
    red_any.sort_unstable();

    assert_eq!(
        color_ids(&library, &["red"], crate::ColorScope::Main),
        vec![id("red.png")]
    );
    assert_eq!(
        color_ids(&library, &["red"], crate::ColorScope::AnyOfThree),
        red_any
    );
    let mut blue_main = vec![id("blue.png"), id("mixed.png")];
    blue_main.sort_unstable();
    assert_eq!(
        color_ids(&library, &["blue"], crate::ColorScope::Main),
        blue_main
    );
    // Several palette colors match any of them.
    let mut red_or_green = vec![id("red.png"), id("green.png")];
    red_or_green.sort_unstable();
    assert_eq!(
        color_ids(&library, &["red", "green"], crate::ColorScope::Main),
        red_or_green
    );
    assert!(color_ids(&library, &["yellow"], crate::ColorScope::AnyOfThree).is_empty());

    // The color condition combines with other conditions by AND.
    let tag = library.create_tag("sky", "空", None, None).expect("tag");
    library
        .set_tag_on_assets(
            &[id("mixed.png")],
            tag.id,
            picker_store::TagState::Confirmed,
        )
        .expect("tag");
    let mut filter = picker_store::AssetFilter {
        tags: picker_store::TagFilter {
            all: vec![tag.id],
            ..picker_store::TagFilter::default()
        },
        ..picker_store::AssetFilter::default()
    };
    library
        .restrict_to_colors(
            &mut filter,
            &crate::ColorCondition {
                palette_ids: vec!["red".to_owned()],
                scope: crate::ColorScope::AnyOfThree,
            },
        )
        .expect("colors");
    let items = library.filter_page(&filter, 0, 10).expect("filter").items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, id("mixed.png"));
}

#[test]
fn color_search_rejects_unknown_palette_colors() {
    let cache = tempfile::tempdir().expect("cache");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let mut filter = picker_store::AssetFilter::default();
    let result = library.restrict_to_colors(
        &mut filter,
        &crate::ColorCondition {
            palette_ids: vec!["gray".to_owned()],
            scope: crate::ColorScope::Main,
        },
    );
    assert!(result.is_err());
    assert_eq!(filter.asset_ids, None);
}

#[test]
fn bundled_palette_has_twelve_colors_and_sorts_gray_by_distance() {
    let palette = crate::Palette::bundled().expect("palette");
    assert_eq!(palette.colors().len(), 12);
    for color in palette.colors() {
        assert_eq!(palette.classify(&color.hex), Some(color.id.as_str()));
    }
    assert_eq!(palette.classify("#F2F2F2"), Some("white"));
    assert_eq!(palette.classify("#202020"), Some("black"));
    assert!(matches!(
        palette.classify("#808080"),
        Some("white" | "black")
    ));
    assert_eq!(palette.classify("not a color"), None);
    assert_eq!(palette.classify("#12345"), None);
}

#[test]
fn palette_files_with_bad_values_are_rejected() {
    let file = |threshold: &str, colors: &str| {
        format!(
            r#"{{"status":"x","note":"x","threshold":{threshold},
                "achromatic":{{"maxChroma":12,"lightId":"red","darkId":"red"}},
                "colors":[{colors}]}}"#
        )
    };
    let red = r##"{"id":"red","nameJa":"赤","hex":"#FF0000"}"##;
    assert!(crate::Palette::from_json(&file("10", red)).is_ok());
    assert!(crate::Palette::from_json(&file("0", red)).is_err());
    assert!(crate::Palette::from_json(&file("10", "")).is_err());
    assert!(crate::Palette::from_json(&file("10", &format!("{red},{red}"))).is_err());
    assert!(crate::Palette::from_json(&file(
        "10",
        r##"{"id":"red","nameJa":"赤","hex":"#GG0000"}"##
    ))
    .is_err());
    let unknown_white = r##"{"status":"x","note":"x","threshold":10,
        "achromatic":{"maxChroma":12,"lightId":"white","darkId":"red"},
        "colors":[{"id":"red","nameJa":"赤","hex":"#FF0000"}]}"##;
    assert!(crate::Palette::from_json(unknown_white).is_err());
    assert!(crate::Palette::from_json("{}").is_err());
    assert!(crate::Palette::from_json("").is_err());
}

/// Design chapter 13: a search over 50,000 images answers within 200 ms.
/// Run with `cargo test -p picker-core --release -- --ignored color_search_meets`.
#[test]
#[ignore = "slow: seeds 50,000 assets"]
fn color_search_meets_the_response_target_for_50k_images() {
    let cache = tempfile::tempdir().expect("cache");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let palette = crate::Palette::bundled().expect("palette");
    let hexes: Vec<String> = palette
        .colors()
        .iter()
        .map(|color| color.hex.clone())
        .collect();
    for index in 0_u32..50_000 {
        let mut hash = [0_u8; 32];
        hash[..4].copy_from_slice(&index.to_le_bytes());
        let asset = library
            .store()
            .assets()
            .insert(&picker_store::NewAsset {
                blake3_hash: &hash,
                file_size: 1,
                width: Some(10),
                height: Some(10),
                format: Some("png"),
                captured_at: None,
                perceptual_hash: None,
            })
            .expect("asset");
        let colors: Vec<(String, f64)> = (0..3)
            .map(|offset| (hexes[(index as usize + offset) % hexes.len()].clone(), 0.3))
            .collect();
        library
            .store()
            .colors()
            .set_auto(asset.id, &colors, "test")
            .expect("colors");
    }
    let started = std::time::Instant::now();
    let mut filter = picker_store::AssetFilter::default();
    library
        .restrict_to_colors(
            &mut filter,
            &crate::ColorCondition {
                palette_ids: vec!["red".to_owned()],
                scope: crate::ColorScope::AnyOfThree,
            },
        )
        .expect("colors");
    let page = library.filter_page(&filter, 0, 100).expect("filter");
    let elapsed = started.elapsed();
    eprintln!("color search over 50,000 images: {elapsed:?}");
    assert_eq!(page.items.len(), 100);
    assert!(elapsed.as_millis() < 200, "color search took {elapsed:?}");
}

fn unix_now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs(),
    )
    .expect("time")
}

fn auto_names(library: &Library, album_id: i64, now: i64) -> Vec<String> {
    let mut names: Vec<String> = library
        .auto_album_page(album_id, now, 0, 100)
        .expect("auto album")
        .items
        .into_iter()
        .map(|item| {
            library
                .store()
                .locations()
                .list_for_asset(item.id)
                .expect("locations")[0]
                .relative_path
                .clone()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn default_auto_albums_are_created_once() {
    let cache = tempfile::tempdir().expect("cache");
    let library = Library::open_in_memory(cache.path()).expect("library");
    assert_eq!(library.ensure_default_auto_albums().expect("defaults"), 3);
    let albums = library.list_albums().expect("albums");
    let names: Vec<&str> = albums
        .iter()
        .map(|entry| entry.album.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["最近追加した画像", "タグのない画像", "未確認の自動タグあり"]
    );
    assert!(albums
        .iter()
        .all(|entry| entry.album.kind == picker_store::AlbumKind::Auto));

    library.delete_album(albums[0].album.id).expect("delete");
    assert_eq!(library.ensure_default_auto_albums().expect("again"), 0);
    assert_eq!(library.list_albums().expect("albums").len(), 2);
}

#[test]
fn auto_albums_follow_changes_on_the_next_evaluation() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(60)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    library.ensure_default_auto_albums().expect("defaults");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let one = asset_id_named(&library, "one.png");
    let now = unix_now();
    let album_named = |name: &str| {
        library
            .list_albums()
            .expect("albums")
            .into_iter()
            .find(|entry| entry.album.name == name)
            .expect("album")
            .album
            .id
    };
    let recent = album_named("最近追加した画像");
    let untagged = album_named("タグのない画像");
    let unreviewed = album_named("未確認の自動タグあり");
    let both = vec!["one.png".to_owned(), "two.png".to_owned()];

    assert_eq!(auto_names(&library, recent, now), both);
    assert!(auto_names(&library, recent, now + 31 * 24 * 60 * 60).is_empty());
    assert_eq!(auto_names(&library, untagged, now), both);
    assert!(auto_names(&library, unreviewed, now).is_empty());

    let tag = library
        .create_tag("beach", "海辺", None, None)
        .expect("tag");
    let beach = library
        .create_auto_album(
            "海辺",
            &crate::AlbumQuery {
                tags_all: vec![library.tag_ref(tag.id).expect("ref")],
                ..crate::AlbumQuery::default()
            },
        )
        .expect("auto album");
    assert!(auto_names(&library, beach.id, now).is_empty());

    library
        .set_tag_on_assets(&[one], tag.id, picker_store::TagState::Confirmed)
        .expect("confirm");
    assert_eq!(
        auto_names(&library, beach.id, now),
        vec!["one.png".to_owned()]
    );
    assert_eq!(
        auto_names(&library, untagged, now),
        vec!["two.png".to_owned()]
    );

    library
        .remove_tag_from_assets(&[one], tag.id)
        .expect("remove");
    assert!(auto_names(&library, beach.id, now).is_empty());

    let two = asset_id_named(&library, "two.png");
    library
        .store()
        .tag_assignments()
        .upsert_suggested(
            two,
            tag.id,
            picker_store::AssignmentSource::ZeroShot,
            0.6,
            "clip",
            "v1",
        )
        .expect("suggest");
    assert_eq!(
        auto_names(&library, unreviewed, now),
        vec!["two.png".to_owned()]
    );

    // Editing the condition changes the result on the next evaluation.
    library
        .set_auto_album_query(
            beach.id,
            &crate::AlbumQuery {
                tags_all: vec![library.tag_ref(tag.id).expect("ref")],
                include_suggested: true,
                ..crate::AlbumQuery::default()
            },
        )
        .expect("edit");
    assert_eq!(
        auto_names(&library, beach.id, now),
        vec!["two.png".to_owned()]
    );
}

#[test]
fn exported_auto_albums_give_the_same_result_after_import() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    let red = [220, 30, 30];
    let blue = [30, 90, 220];
    fs::write(media.path().join("red.png"), two_color_png(red, red, 100)).expect("write");
    fs::write(
        media.path().join("blue.png"),
        two_color_png(blue, blue, 100),
    )
    .expect("write");
    fs::write(media.path().join("mixed.png"), two_color_png(blue, red, 70)).expect("write");
    let source = Library::open_in_memory(cache.path()).expect("library");
    let root = source.add_root(media.path(), "root").expect("root");
    source.scan_root(root.id).expect("scan");
    let tag = source
        .create_tag("poster", "ポスター", None, None)
        .expect("tag");
    source
        .set_tag_on_assets(
            &[
                asset_id_named(&source, "red.png"),
                asset_id_named(&source, "mixed.png"),
            ],
            tag.id,
            picker_store::TagState::Confirmed,
        )
        .expect("tag");
    let album = source
        .create_auto_album(
            "赤いポスター",
            &crate::AlbumQuery {
                tags_all: vec![source.tag_ref(tag.id).expect("ref")],
                colors: vec!["red".to_owned()],
                colors_any_of_three: true,
                ..crate::AlbumQuery::default()
            },
        )
        .expect("auto album");
    let now = unix_now();
    let expected = auto_names(&source, album.id, now);
    assert_eq!(expected, vec!["mixed.png".to_owned(), "red.png".to_owned()]);
    let export_dir = tempfile::tempdir().expect("export");
    let path = export_dir.path().join("library.json");
    source.export_portable(&path).expect("export");

    let cache2 = tempfile::tempdir().expect("cache2");
    let restored = Library::open_in_memory(cache2.path()).expect("library2");
    restored.import_portable(&path).expect("import");
    let root = restored.list_roots().expect("roots")[0].root.clone();
    restored.scan_root(root.id).expect("scan");
    let restored_album = restored
        .list_albums()
        .expect("albums")
        .into_iter()
        .find(|entry| entry.album.name == "赤いポスター")
        .expect("album")
        .album;
    assert_eq!(restored_album.kind, picker_store::AlbumKind::Auto);
    assert_eq!(auto_names(&restored, restored_album.id, now), expected);
}

#[test]
fn auto_album_inputs_are_validated() {
    let cache = tempfile::tempdir().expect("cache");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let bad_color = crate::AlbumQuery {
        colors: vec!["gray".to_owned()],
        ..crate::AlbumQuery::default()
    };
    assert!(library.create_auto_album("灰", &bad_color).is_err());
    assert!(library.list_albums().expect("albums").is_empty());

    let manual = library.create_album("手動").expect("album");
    assert!(library
        .set_auto_album_query(manual.id, &crate::AlbumQuery::default())
        .is_err());
    assert!(library.auto_album_query(manual.id).is_err());
    assert!(library.auto_album_page(manual.id, 0, 0, 10).is_err());

    // A tag that no longer exists in `tags_all` matches nothing.
    let missing = library
        .create_auto_album(
            "消えたタグ",
            &crate::AlbumQuery {
                tags_all: vec![crate::TagRef {
                    namespace: "user".to_owned(),
                    canonical_name: "gone".to_owned(),
                }],
                ..crate::AlbumQuery::default()
            },
        )
        .expect("album");
    assert_eq!(
        library
            .auto_album_page(missing.id, 0, 0, 10)
            .expect("page")
            .total,
        0
    );

    assert!(crate::AlbumQuery::from_json("{\"unknown\": 1}").is_err());
    assert!(crate::AlbumQuery::from_json("not json").is_err());
    assert_eq!(
        crate::AlbumQuery::from_json("{}").expect("empty"),
        crate::AlbumQuery::default()
    );
}

#[test]
fn xmp_export_writes_only_when_enabled_and_preserves_existing_fields() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(10)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let asset_id = asset_id_named(&library, "one.png");
    let tag = library
        .create_tag("beach", "海辺", None, None)
        .expect("tag");
    library
        .set_tag_on_assets(&[asset_id], tag.id, picker_store::TagState::Confirmed)
        .expect("tag");
    assert_eq!(library.export_xmp(asset_id).expect("disabled"), 0);
    let sidecar = media.path().join("one.png.xmp");
    fs::write(
        &sidecar,
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"><xmp:Rating>4</xmp:Rating></rdf:Description></rdf:RDF></x:xmpmeta>",
    )
    .expect("sidecar");
    library.set_setting("xmp.enabled", "true").expect("setting");
    assert_eq!(library.export_xmp(asset_id).expect("export"), 1);
    let written = fs::read_to_string(&sidecar).expect("read");
    assert!(written.contains("<xmp:Rating>4</xmp:Rating>"));
    assert!(written.contains("<rdf:li>海辺</rdf:li>"));
}

#[test]
fn clearing_thumbnails_queues_regeneration_and_keeps_user_data() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(30)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(90)).expect("write");
    // A file the app did not create must survive the removal.
    fs::write(cache.path().join("keep.txt"), b"keep").expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    let asset = library.asset_page(0, 10).expect("page").items[0].id;
    library
        .set_asset_colors(asset, &[("#010203".to_owned(), 1.0)])
        .expect("manual color");
    let tag = library.create_tag("keep", "残す", None, None).expect("tag");
    library
        .set_tag_on_assets(&[asset], tag.id, picker_store::TagState::Confirmed)
        .expect("assign");

    let report = library.clear_cache(CacheTarget::Thumbnails).expect("clear");
    assert_eq!(report.image_jobs, 2);
    assert_eq!(report.embedding_jobs, 0);
    assert!(!library.thumbnail_path(asset, ThumbnailSize::Grid).exists());
    assert!(cache.path().join("keep.txt").exists());
    assert_eq!(
        library.asset_summary(asset).expect("summary").thumbnail,
        ThumbnailState::Missing
    );

    assert_eq!(library.run_queued_image_jobs().expect("regenerate"), 2);
    assert!(library.thumbnail_path(asset, ThumbnailSize::Grid).is_file());
    let detail = library.asset_detail(asset).expect("detail");
    assert_eq!(detail.colors[0].hex, "#010203");
    assert!(detail.tags.iter().any(|assigned| assigned.tag.id == tag.id));
}

#[test]
fn clearing_embeddings_empties_the_index_and_queues_jobs() {
    let media = tempfile::tempdir().expect("media");
    let cache = tempfile::tempdir().expect("cache");
    fs::write(media.path().join("one.png"), png_bytes(7)).expect("write");
    fs::write(media.path().join("two.png"), png_bytes(8)).expect("write");
    let library = Library::open_in_memory(cache.path()).expect("library");
    let root = library.add_root(media.path(), "root").expect("root");
    library.scan_root(root.id).expect("scan");
    library
        .set_embedder(Some(std::sync::Arc::new(MockEmbedder)))
        .expect("embedder");
    assert_eq!(library.index_embeddings(10).expect("index"), 2);

    let report = library.clear_cache(CacheTarget::All).expect("clear");
    assert_eq!(report.embedding_jobs, 2);
    assert_eq!(report.image_jobs, 2);
    let progress = library
        .embedding_progress()
        .expect("progress")
        .expect("model");
    assert_eq!((progress.indexed, progress.missing), (0, 2));
    assert!(library
        .semantic_search("anything", 10)
        .expect("search")
        .is_empty());
}

#[test]
fn cache_targets_are_parsed_strictly() {
    assert_eq!(
        CacheTarget::parse("thumbnails").expect("thumbnails"),
        CacheTarget::Thumbnails
    );
    assert_eq!(CacheTarget::parse("all").expect("all"), CacheTarget::All);
    for invalid in ["", "Thumbnails", "../thumbnails", "database"] {
        assert!(CacheTarget::parse(invalid).is_err(), "{invalid}");
    }
}
