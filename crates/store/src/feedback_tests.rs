use crate::{AssignmentSource, FeedbackAction, NewAsset, NewTag, Store, TagOrigin, TagState};

fn store_with_asset_and_tag() -> (Store, i64, i64) {
    let store = Store::open_in_memory().expect("store");
    let asset = store
        .assets()
        .insert(&NewAsset {
            blake3_hash: &[b'f'; 32],
            file_size: 10,
            width: Some(1),
            height: Some(1),
            format: Some("png"),
            captured_at: None,
            perceptual_hash: None,
        })
        .expect("asset");
    let tag = store
        .tags()
        .create(&NewTag {
            namespace: "custom",
            canonical_name: "robot",
            display_name_ja: "ロボット",
            display_name_en: Some("robot"),
            parent_id: None,
            color: None,
            origin: TagOrigin::User,
        })
        .expect("tag");
    (store, asset.id, tag.id)
}

#[test]
fn records_decision_with_previous_confidence() {
    let (store, asset_id, tag_id) = store_with_asset_and_tag();
    store
        .tag_assignments()
        .upsert_suggested(
            asset_id,
            tag_id,
            AssignmentSource::ZeroShot,
            0.72,
            "clip",
            "v1",
        )
        .expect("suggest");
    let previous = store
        .tag_assignments()
        .get(asset_id, tag_id)
        .expect("assignment");
    store
        .tag_assignments()
        .set_user(asset_id, tag_id, TagState::Confirmed)
        .expect("confirm");
    let event = store
        .feedback()
        .record(
            asset_id,
            tag_id,
            FeedbackAction::Approved,
            previous.confidence,
            Some(&previous),
        )
        .expect("record");
    assert_eq!(event.action, FeedbackAction::Approved);
    assert_eq!(event.previous_state, Some(TagState::Suggested));
    assert_eq!(event.previous_source, Some(AssignmentSource::ZeroShot));
    assert_eq!(event.previous_confidence, Some(0.72));
    assert_eq!(event.previous_model_id.as_deref(), Some("clip"));
    assert_eq!(
        store
            .feedback()
            .list_for_asset(asset_id)
            .expect("list")
            .len(),
        1
    );
    assert_eq!(store.feedback().list_since(0, 10).expect("since").len(), 1);
}

#[test]
fn rejects_invalid_feedback_confidence() {
    let (store, asset_id, tag_id) = store_with_asset_and_tag();
    let result =
        store
            .feedback()
            .record(asset_id, tag_id, FeedbackAction::Rejected, Some(1.5), None);
    assert!(result.is_err());
}
