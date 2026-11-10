use super::*;

fn seed_assets(store: &Store, count: u8) -> Vec<Asset> {
    (1..=count)
        .map(|byte| {
            store
                .assets()
                .insert(&NewAsset {
                    blake3_hash: &[byte; 32],
                    file_size: 100,
                    width: Some(10),
                    height: Some(10),
                    format: Some("jpeg"),
                    captured_at: None,
                    perceptual_hash: None,
                })
                .expect("asset")
        })
        .collect()
}

#[test]
fn albums_append_remove_and_compact_in_order() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 3);
    let album = store.albums().create("旅行", 0)?;
    assert_eq!(
        store
            .albums()
            .add_assets(album.id, &[assets[0].id, assets[1].id])?,
        2
    );
    assert_eq!(
        store
            .albums()
            .add_assets(album.id, &[assets[1].id, assets[2].id])?,
        1
    );
    let items = store.albums().list_items(album.id)?;
    assert_eq!(
        items.iter().map(|item| item.asset_id).collect::<Vec<_>>(),
        vec![assets[0].id, assets[1].id, assets[2].id]
    );

    assert_eq!(store.albums().remove_assets(album.id, &[assets[1].id])?, 1);
    let items = store.albums().list_items(album.id)?;
    assert_eq!(
        items.iter().map(|item| item.position).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(store.albums().list()?[0].asset_count, 2);

    store
        .albums()
        .reorder(album.id, &[assets[2].id, assets[0].id])?;
    assert_eq!(
        store
            .albums()
            .list_items(album.id)?
            .iter()
            .map(|item| item.asset_id)
            .collect::<Vec<_>>(),
        vec![assets[2].id, assets[0].id]
    );
    assert!(store
        .albums()
        .reorder(album.id, &[assets[2].id, assets[2].id])
        .is_err());
    assert!(store
        .albums()
        .reorder(album.id, &[assets[2].id, assets[1].id])
        .is_err());
    Ok(())
}

#[test]
fn albums_share_assets_and_deletion_does_not_touch_files_or_other_albums() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 2);
    let first = store.albums().create("A", 0)?;
    let second = store.albums().create("B", 1)?;
    store
        .albums()
        .add_assets(first.id, &[assets[0].id, assets[1].id])?;
    store.albums().add_assets(second.id, &[assets[0].id])?;

    let root = store.roots().register(&NewRoot {
        absolute_path: "/photos",
        volume_id: None,
        display_name: "photos",
        enabled: true,
        status: RootStatus::Online,
    })?;
    store.locations().insert(&NewLocation {
        asset_id: assets[0].id,
        root_id: root.id,
        relative_path: "a.jpg",
        modified_at: 1,
        status: LocationStatus::Present,
    })?;

    assert!(store.albums().delete(first.id)?);
    assert_eq!(store.albums().list_items(second.id)?.len(), 1);
    assert!(store.assets().get(assets[0].id).is_ok());
    assert_eq!(store.locations().list_for_asset(assets[0].id)?.len(), 1);
    assert!(store.albums().list_items(first.id).is_err());
    Ok(())
}

#[test]
fn moving_a_file_keeps_the_album_membership() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 1);
    let album = store.albums().create("A", 0)?;
    store.albums().add_assets(album.id, &[assets[0].id])?;
    let root = store.roots().register(&NewRoot {
        absolute_path: "/photos",
        volume_id: None,
        display_name: "photos",
        enabled: true,
        status: RootStatus::Online,
    })?;
    let location = store.locations().insert(&NewLocation {
        asset_id: assets[0].id,
        root_id: root.id,
        relative_path: "old.jpg",
        modified_at: 1,
        status: LocationStatus::Present,
    })?;
    store
        .connection()
        .execute("DELETE FROM locations WHERE id = ?1", [location.id])?;
    store.locations().insert(&NewLocation {
        asset_id: assets[0].id,
        root_id: root.id,
        relative_path: "new.jpg",
        modified_at: 2,
        status: LocationStatus::Present,
    })?;
    assert_eq!(store.albums().list_items(album.id)?.len(), 1);
    Ok(())
}

#[test]
fn album_validation_rejects_empty_names_and_automatic_edits() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 1);
    assert!(store.albums().create("  ", 0).is_err());
    let album = store.albums().create("A", 0)?;
    assert_eq!(store.albums().rename(album.id, "B")?.name, "B");
    assert!(store.albums().rename(album.id, "").is_err());

    store.connection().execute(
        "INSERT INTO albums (name, kind, query, sort_order) VALUES ('auto', 'auto', '{}', 1)",
        [],
    )?;
    let auto = store.albums().get(2)?;
    assert!(store.albums().add_assets(auto.id, &[assets[0].id]).is_err());
    assert!(store
        .albums()
        .remove_assets(auto.id, &[assets[0].id])
        .is_err());
    assert!(store.albums().rename(auto.id, "ok").is_ok());
    Ok(())
}

#[test]
fn insert_at_keeps_explicit_order_and_rejects_bad_input() -> Result<()> {
    let store = Store::open_in_memory()?;
    let assets = seed_assets(&store, 3);
    let album = store.albums().create("旅行", 0)?;
    assert!(store.albums().insert_at(album.id, assets[2].id, 0)?);
    assert!(store.albums().insert_at(album.id, assets[0].id, 2)?);
    assert!(store.albums().insert_at(album.id, assets[1].id, 1)?);
    assert!(!store.albums().insert_at(album.id, assets[1].id, 5)?);
    let order: Vec<i64> = store
        .albums()
        .list_items(album.id)?
        .into_iter()
        .map(|item| item.asset_id)
        .collect();
    assert_eq!(order, vec![assets[2].id, assets[1].id, assets[0].id]);
    assert!(store
        .albums()
        .insert_at(album.id, assets[0].id, -1)
        .is_err());
    assert!(store
        .albums()
        .insert_at(album.id + 99, assets[0].id, 0)
        .is_err());
    assert_eq!(
        store
            .albums()
            .find_manual_by_name(" 旅行 ")?
            .map(|found| found.id),
        Some(album.id)
    );
    assert!(store.albums().find_manual_by_name("ない")?.is_none());
    Ok(())
}

#[test]
fn pending_items_are_taken_once_per_hash() -> Result<()> {
    let store = Store::open_in_memory()?;
    let hash = [7_u8; 32];
    let other = [8_u8; 32];
    store
        .portable()
        .stage_item(&hash, PendingItemKind::Color, r#"{"a":1}"#)?;
    store
        .portable()
        .stage_item(&other, PendingItemKind::Feedback, r#"{"b":2}"#)?;
    assert!(store
        .portable()
        .stage_item(&[1_u8; 4], PendingItemKind::Color, "{}")
        .is_err());
    assert!(store
        .portable()
        .stage_item(&hash, PendingItemKind::Color, "not json")
        .is_err());
    assert_eq!(store.portable().pending_items()?.len(), 2);
    let taken = store.portable().take_items(&hash)?;
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].kind, PendingItemKind::Color);
    assert!(store.portable().take_items(&hash)?.is_empty());
    assert_eq!(store.portable().pending_items()?.len(), 1);
    Ok(())
}
