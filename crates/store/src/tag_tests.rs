use super::*;

fn new_tag<'a>(canonical: &'a str, display: &'a str, parent: Option<i64>) -> NewTag<'a> {
    NewTag {
        namespace: "user",
        canonical_name: canonical,
        display_name_ja: display,
        display_name_en: None,
        parent_id: parent,
        color: None,
        origin: TagOrigin::User,
    }
}

fn seed_assets(store: &Store, count: u8) -> Vec<Asset> {
    (1..=count)
        .map(|byte| {
            store
                .assets()
                .insert(&NewAsset {
                    blake3_hash: &[byte; 32],
                    file_size: 100,
                    width: Some(300),
                    height: Some(200),
                    format: Some(if byte % 2 == 0 { "png" } else { "jpeg" }),
                    captured_at: Some(1_700_000_000 + i64::from(byte)),
                    perceptual_hash: None,
                })
                .expect("insert asset")
        })
        .collect()
}

#[test]
fn confirmed_and_rejected_assignments_are_never_overwritten() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 2);
    let tag = store.tags().create(&new_tag("sunset", "夕日", None))?;

    store
        .tag_assignments()
        .set_user(assets[0].id, tag.id, TagState::Confirmed)?;
    let result = store.tag_assignments().upsert_suggested(
        assets[0].id,
        tag.id,
        AssignmentSource::ZeroShot,
        0.99,
        "clip",
        "1",
    )?;
    let result = result.expect("assignment");
    assert_eq!(result.state, TagState::Confirmed);
    assert_eq!(result.source, AssignmentSource::User);

    store
        .tag_assignments()
        .set_user(assets[1].id, tag.id, TagState::Rejected)?;
    store.tag_assignments().upsert_suggested(
        assets[1].id,
        tag.id,
        AssignmentSource::ZeroShot,
        0.99,
        "clip",
        "1",
    )?;
    let rejected = store.tag_assignments().get(assets[1].id, tag.id)?;
    assert_eq!(rejected.state, TagState::Rejected);

    assert!(store
        .tag_assignments()
        .upsert_suggested(
            assets[0].id,
            tag.id,
            AssignmentSource::User,
            0.5,
            "clip",
            "1"
        )
        .is_err());
    assert!(store
        .tag_assignments()
        .set_user(assets[0].id, tag.id, TagState::Suggested)
        .is_err());
    Ok(())
}

#[test]
fn suggestions_replace_only_the_same_source() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 1);
    let tag = store.tags().create(&new_tag("sea", "海", None))?;

    store.tag_assignments().upsert_suggested(
        assets[0].id,
        tag.id,
        AssignmentSource::ZeroShot,
        0.4,
        "clip",
        "1",
    )?;
    store.tag_assignments().upsert_suggested(
        assets[0].id,
        tag.id,
        AssignmentSource::CustomClassifier,
        0.8,
        "custom",
        "1",
    )?;
    assert_eq!(
        store.tag_assignments().get(assets[0].id, tag.id)?.source,
        AssignmentSource::ZeroShot
    );

    store.tag_assignments().upsert_suggested(
        assets[0].id,
        tag.id,
        AssignmentSource::ZeroShot,
        0.7,
        "clip",
        "2",
    )?;
    let updated = store.tag_assignments().get(assets[0].id, tag.id)?;
    assert_eq!(updated.source, AssignmentSource::ZeroShot);
    assert_eq!(updated.confidence, Some(0.7));

    store
        .tag_assignments()
        .set_user(assets[0].id, tag.id, TagState::Confirmed)?;
    store.tag_assignments().replace_suggestions(
        assets[0].id,
        AssignmentSource::ZeroShot,
        "clip-japanese",
        "v1",
        &[(tag.id, 0.9)],
    )?;
    assert_eq!(
        store.tag_assignments().get(assets[0].id, tag.id)?.state,
        TagState::Confirmed
    );

    assert!(store.tag_assignments().remove_user(assets[0].id, tag.id)?);
    assert!(store
        .tag_assignments()
        .get_optional(assets[0].id, tag.id)?
        .is_none());
    Ok(())
}

#[test]
fn parent_filters_include_descendants_and_support_all_any_none() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 3);
    let animal = store.tags().create(&new_tag("animal", "動物", None))?;
    let dog = store
        .tags()
        .create(&new_tag("dog", "犬", Some(animal.id)))?;
    let cat = store
        .tags()
        .create(&new_tag("cat", "猫", Some(animal.id)))?;

    store
        .tag_assignments()
        .set_user(assets[0].id, dog.id, TagState::Confirmed)?;
    store
        .tag_assignments()
        .set_user(assets[1].id, cat.id, TagState::Confirmed)?;
    store
        .tag_assignments()
        .set_user(assets[2].id, dog.id, TagState::Confirmed)?;
    store
        .tag_assignments()
        .set_user(assets[2].id, cat.id, TagState::Confirmed)?;

    let filter = |mode, tags: Vec<i64>| AssetFilter {
        tags: TagFilter {
            tag_ids: tags,
            mode,
            include_suggested: false,
        },
        ..AssetFilter::default()
    };
    let ids = |filter: &AssetFilter| {
        store
            .asset_filters()
            .page(filter, 0, 100)
            .expect("filter")
            .into_iter()
            .map(|asset| asset.id)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        ids(&filter(TagMatchMode::All, vec![animal.id])),
        vec![assets[0].id, assets[1].id, assets[2].id]
    );
    assert_eq!(
        ids(&filter(TagMatchMode::All, vec![animal.id, dog.id])),
        vec![assets[0].id, assets[2].id]
    );
    assert_eq!(
        ids(&filter(TagMatchMode::Any, vec![dog.id, cat.id])),
        vec![assets[0].id, assets[1].id, assets[2].id]
    );
    assert_eq!(
        ids(&filter(TagMatchMode::None, vec![dog.id])),
        vec![assets[1].id]
    );
    assert_eq!(
        store
            .asset_filters()
            .count(&filter(TagMatchMode::All, vec![cat.id]))?,
        2
    );
    Ok(())
}

#[test]
fn attribute_filters_combine_with_tag_filters() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 3);
    let tag = store.tags().create(&new_tag("night", "夜", None))?;
    store
        .tag_assignments()
        .set_user(assets[1].id, tag.id, TagState::Confirmed)?;

    let filter = AssetFilter {
        tags: TagFilter {
            tag_ids: vec![tag.id],
            mode: TagMatchMode::All,
            include_suggested: false,
        },
        formats: vec!["PNG".to_owned()],
        min_width: Some(300),
        min_height: Some(200),
        captured_from: Some(1_700_000_000),
        captured_to: Some(1_800_000_000),
    };
    let page = store.asset_filters().page(&filter, 0, 10)?;
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, assets[1].id);

    let empty = AssetFilter {
        formats: vec!["gif".to_owned()],
        ..AssetFilter::default()
    };
    assert!(store.asset_filters().page(&empty, 0, 10)?.is_empty());
    Ok(())
}

#[test]
fn hierarchy_rejects_cycles_and_alias_lookup_is_stable() -> Result<()> {
    let store = Store::open_in_memory()?;
    let root = store.tags().create(&new_tag("root", "根", None))?;
    let child = store
        .tags()
        .create(&new_tag("child", "子", Some(root.id)))?;
    let grandchild = store
        .tags()
        .create(&new_tag("grand", "孫", Some(child.id)))?;
    assert!(store
        .tags()
        .set_parent(root.id, Some(grandchild.id))
        .is_err());
    assert!(store.tags().set_parent(root.id, Some(root.id)).is_err());
    assert_eq!(
        store.tags().descendants(root.id)?,
        vec![root.id, child.id, grandchild.id]
    );
    assert!(store.tags().set_parent(grandchild.id, None).is_ok());

    let alias = store.tags().add_alias(child.id, "こども")?;
    assert_eq!(
        store.tags().find_by_alias("こども")?.map(|tag| tag.id),
        Some(child.id)
    );
    assert!(store.tags().add_alias(root.id, "こども").is_err());
    assert!(store.tags().remove_alias(alias.id)?);
    assert!(store.tags().find_by_alias("こども")?.is_none());
    Ok(())
}

#[test]
fn tags_can_be_renamed_merged_and_deleted() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 2);
    let source = store.tags().create(&new_tag("old", "旧", None))?;
    let target = store.tags().create(&new_tag("new", "新", None))?;
    store.tags().add_alias(source.id, "legacy")?;
    store.tag_assignments().upsert_suggested(
        assets[0].id,
        source.id,
        AssignmentSource::ZeroShot,
        0.4,
        "clip",
        "1",
    )?;
    store
        .tag_assignments()
        .set_user(assets[0].id, target.id, TagState::Rejected)?;
    store
        .tag_assignments()
        .set_user(assets[1].id, source.id, TagState::Confirmed)?;

    let merged = store.tags().merge(source.id, target.id)?;
    assert_eq!(merged.canonical_name, "new");
    assert_eq!(
        store.tags().find_by_alias("legacy")?.map(|tag| tag.id),
        Some(target.id)
    );
    assert_eq!(
        store.tags().find_by_alias("old")?.map(|tag| tag.id),
        Some(target.id)
    );
    assert_eq!(
        store.tag_assignments().get(assets[0].id, target.id)?.state,
        TagState::Rejected
    );
    assert_eq!(
        store.tag_assignments().get(assets[1].id, target.id)?.state,
        TagState::Confirmed
    );
    assert!(store.tags().get(source.id).is_err());
    assert!(store.tags().merge(source.id, target.id).is_err());

    let renamed = store.tags().rename(target.id, "renamed", "改名")?;
    assert_eq!(renamed.canonical_name, "renamed");
    assert!(store.tags().rename(target.id, "  ", "空").is_err());
    assert!(store.tags().delete(target.id)?);
    assert!(store
        .tag_assignments()
        .list_for_asset(assets[1].id)?
        .is_empty());
    Ok(())
}

#[test]
fn invalid_tags_are_rejected_and_asset_deletion_cascades() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 1);
    assert!(store.tags().create(&new_tag("", "空", None)).is_err());
    assert!(store
        .tags()
        .create(&NewTag {
            color: Some("red"),
            ..new_tag("color", "色", None)
        })
        .is_err());
    let tag = store.tags().create(&NewTag {
        color: Some("#AABBCC"),
        ..new_tag("color", "色", None)
    })?;
    assert_eq!(tag.color.as_deref(), Some("#AABBCC"));
    store
        .tag_assignments()
        .set_user(assets[0].id, tag.id, TagState::Confirmed)?;
    store
        .connection()
        .execute("DELETE FROM assets WHERE id = ?1", [assets[0].id])?;
    assert!(store
        .tag_assignments()
        .get_optional(assets[0].id, tag.id)?
        .is_none());
    Ok(())
}
