use crate::{NewTag, Store, TagOrigin};

#[test]
fn classifier_round_trip_and_delete() {
    let store = Store::open_in_memory().expect("store");
    let tag = store
        .tags()
        .create(&NewTag {
            namespace: "custom",
            canonical_name: "robot",
            display_name_ja: "ロボット",
            display_name_en: None,
            parent_id: None,
            color: None,
            origin: TagOrigin::User,
        })
        .expect("tag");
    store
        .classifiers()
        .upsert(tag.id, 2, &[0.25, -1.5], 0.125)
        .expect("upsert");
    let classifier = store.classifiers().get(tag.id).expect("get").expect("row");
    assert_eq!(classifier.weights, vec![0.25, -1.5]);
    assert_eq!(classifier.bias, 0.125);
    assert!(store.classifiers().upsert(tag.id, 2, &[1.0], 0.0).is_err());
    assert!(store.classifiers().delete(tag.id).expect("delete"));
    assert!(store.classifiers().get(tag.id).expect("get").is_none());
}
