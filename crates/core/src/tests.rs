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
    let root = restored.add_root(media.path(), "root").expect("root");
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
