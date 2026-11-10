use crate::{NewAsset, Store, StoreError};

fn store() -> Store {
    Store::open_in_memory().expect("store")
}

#[test]
fn embeddings_round_trip_upsert_and_delete() {
    let store = store();
    let asset = store
        .assets()
        .insert(&NewAsset {
            blake3_hash: &[b'a'; 32],
            file_size: 10,
            width: Some(1),
            height: Some(1),
            format: Some("png"),
            captured_at: None,
            perceptual_hash: None,
        })
        .expect("asset");
    store
        .embeddings()
        .upsert(asset.id, "clip", "v1", &[0.5, -0.25, 1.0])
        .expect("insert");
    let embedding = store
        .embeddings()
        .get(asset.id, "clip", "v1")
        .expect("get")
        .expect("present");
    assert_eq!(embedding.dimensions, 3);
    assert_eq!(embedding.vector, vec![0.5, -0.25, 1.0]);

    store
        .embeddings()
        .upsert(asset.id, "clip", "v1", &[1.0, 2.0, 3.0, 4.0])
        .expect("update");
    assert_eq!(
        store
            .embeddings()
            .get(asset.id, "clip", "v1")
            .expect("get")
            .expect("present")
            .vector,
        vec![1.0, 2.0, 3.0, 4.0]
    );
    assert_eq!(
        store
            .embeddings()
            .count_for_model("clip", "v1")
            .expect("count"),
        1
    );
    assert!(store
        .embeddings()
        .delete(asset.id, "clip", "v1")
        .expect("delete"));
    assert!(store
        .embeddings()
        .get(asset.id, "clip", "v1")
        .expect("get")
        .is_none());
    assert!(!store
        .embeddings()
        .delete(asset.id, "clip", "v1")
        .expect("delete"));
}

#[test]
fn missing_assets_are_listed_per_model_version() {
    let store = store();
    let first = store
        .assets()
        .insert(&NewAsset {
            blake3_hash: &[b'a'; 32],
            file_size: 1,
            width: Some(1),
            height: Some(1),
            format: Some("png"),
            captured_at: None,
            perceptual_hash: None,
        })
        .expect("asset");
    let second = store
        .assets()
        .insert(&NewAsset {
            blake3_hash: &[b'b'; 32],
            file_size: 1,
            width: Some(1),
            height: Some(1),
            format: Some("png"),
            captured_at: None,
            perceptual_hash: None,
        })
        .expect("asset");
    assert_eq!(
        store
            .embeddings()
            .missing_asset_ids("clip", "v1", 10)
            .expect("missing"),
        vec![first.id, second.id]
    );
    store
        .embeddings()
        .upsert(first.id, "clip", "v1", &[1.0])
        .expect("upsert");
    assert_eq!(
        store
            .embeddings()
            .missing_asset_ids("clip", "v1", 10)
            .expect("missing"),
        vec![second.id]
    );
    assert_eq!(
        store
            .embeddings()
            .missing_asset_ids("clip", "v2", 10)
            .expect("missing"),
        vec![first.id, second.id]
    );
}

#[test]
fn invalid_vectors_are_rejected() {
    let store = store();
    let asset = store
        .assets()
        .insert(&NewAsset {
            blake3_hash: &[b'c'; 32],
            file_size: 1,
            width: Some(1),
            height: Some(1),
            format: Some("png"),
            captured_at: None,
            perceptual_hash: None,
        })
        .expect("asset");
    for vector in [vec![], vec![f32::NAN], vec![f32::INFINITY]] {
        let error = store
            .embeddings()
            .upsert(asset.id, "clip", "v1", &vector)
            .expect_err("invalid");
        assert!(matches!(error, StoreError::InvalidInput { .. }));
    }
    assert!(store
        .embeddings()
        .upsert(asset.id, "", "v1", &[1.0])
        .is_err());
    assert!(store
        .embeddings()
        .upsert(asset.id, "clip", "v1", &vec![0.0; 4097])
        .is_err());
}

#[test]
fn embeddings_can_be_listed_by_update_time_and_deleted_together() {
    let store = store();
    let mut ids = Vec::new();
    for byte in *b"ab" {
        ids.push(
            store
                .assets()
                .insert(&NewAsset {
                    blake3_hash: &[byte; 32],
                    file_size: 10,
                    width: Some(1),
                    height: Some(1),
                    format: Some("png"),
                    captured_at: None,
                    perceptual_hash: None,
                })
                .expect("asset")
                .id,
        );
    }
    for id in &ids {
        store
            .embeddings()
            .upsert(*id, "clip", "v1", &[1.0, 0.0])
            .expect("embedding");
    }
    store
        .embeddings()
        .upsert(ids[0], "other", "v1", &[1.0])
        .expect("other model");
    store
        .connection()
        .execute(
            "UPDATE embeddings SET updated_at = 100 WHERE asset_id = ?1",
            [ids[0]],
        )
        .expect("age");

    let recent = store
        .embeddings()
        .list_updated_since("clip", "v1", 101)
        .expect("recent");
    assert_eq!(
        recent.iter().map(|e| e.asset_id).collect::<Vec<_>>(),
        vec![ids[1]]
    );
    assert_eq!(
        store
            .embeddings()
            .list_updated_since("clip", "v1", 100)
            .expect("inclusive")
            .len(),
        2
    );
    assert_eq!(store.embeddings().delete_all().expect("delete"), 3);
    assert_eq!(
        store
            .embeddings()
            .count_for_model("clip", "v1")
            .expect("count"),
        0
    );
}
