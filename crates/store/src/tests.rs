use std::collections::BTreeSet;

use rusqlite::OptionalExtension;
use tempfile::TempDir;

use super::*;

fn root<'a>(path: &'a str, name: &'a str) -> NewRoot<'a> {
    NewRoot {
        absolute_path: path,
        volume_id: Some("volume-1"),
        display_name: name,
        enabled: true,
        status: RootStatus::Online,
    }
}

fn asset(hash_byte: u8) -> NewAsset<'static> {
    let hash = Box::leak(Box::new([hash_byte; 32]));
    NewAsset {
        blake3_hash: hash,
        file_size: 1_024,
        width: Some(640),
        height: Some(480),
        format: Some("jpeg"),
        captured_at: Some(1_700_000_000),
        perceptual_hash: None,
    }
}

#[test]
fn empty_database_migrates_to_latest_schema_in_wal_mode() -> Result<()> {
    let temporary = TempDir::new().expect("create temporary directory");
    let store = Store::open(temporary.path().join("library.sqlite3"))?;

    assert_eq!(store.schema_version()?, LATEST_SCHEMA_VERSION);
    let journal_mode: String = store
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
    assert_eq!(journal_mode, "wal");
    let foreign_keys: bool = store
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    assert!(foreign_keys);

    let mut statement = store.connection().prepare(
        "SELECT name FROM sqlite_schema
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    let expected = BTreeSet::from([
        "assets".to_owned(),
        "jobs".to_owned(),
        "locations".to_owned(),
        "roots".to_owned(),
        "schema_migrations".to_owned(),
        "settings".to_owned(),
        "stage_states".to_owned(),
    ]);
    assert_eq!(tables, expected);
    Ok(())
}

#[test]
fn in_memory_paths_never_create_files() -> Result<()> {
    let temporary = TempDir::new().expect("create temporary directory");
    let store = Store::open(temporary.path().join(":memory:"))?;

    assert_eq!(store.schema_version()?, LATEST_SCHEMA_VERSION);
    assert_eq!(
        std::fs::read_dir(temporary.path())
            .expect("read temporary directory")
            .count(),
        0
    );
    assert!(!temporary.path().join(":memory:.ses").exists());
    Ok(())
}

#[test]
fn repositories_round_trip_all_v1_entities() -> Result<()> {
    let store = Store::open_in_memory()?;
    let created_root = store
        .roots()
        .insert(&root("/Users/example/Pictures", "写真"))?;
    let created_asset = store.assets().insert(&asset(7))?;
    let created_location = store.locations().insert(&NewLocation {
        asset_id: created_asset.id,
        root_id: created_root.id,
        relative_path: "trips/sea.jpg",
        modified_at: 1_700_000_001,
        status: LocationStatus::Present,
    })?;

    assert_eq!(store.roots().get(created_root.id)?, created_root);
    assert_eq!(
        store.assets().find_by_hash(&[7; 32])?,
        Some(created_asset.clone())
    );
    assert_eq!(
        store.locations().get(created_location.id)?,
        created_location
    );

    let state = store.stage_states().upsert(
        StageTarget::Asset(created_asset.id),
        Stage::Metadata,
        StageStatus::Succeeded,
        "metadata-v1",
        None,
    )?;
    assert_eq!(state.target, StageTarget::Asset(created_asset.id));
    assert_eq!(
        store
            .stage_states()
            .get(StageTarget::Asset(created_asset.id), Stage::Metadata)?,
        Some(state)
    );

    let job = store.jobs().enqueue(
        StageTarget::Location(created_location.id),
        Stage::Hash,
        100,
        3,
    )?;
    assert_eq!(store.jobs().get(job.id)?, job);

    assert_eq!(store.settings().set("ui.language", "ja")?.value, "ja");
    assert_eq!(
        store.settings().get("ui.language")?.expect("setting").value,
        "ja"
    );
    Ok(())
}

#[test]
fn database_constraints_reject_duplicates_invalid_targets_and_foreign_keys() -> Result<()> {
    let store = Store::open_in_memory()?;
    let root = store.roots().insert(&root("/one", "one"))?;
    let asset = store.assets().insert(&asset(1))?;
    store.locations().insert(&NewLocation {
        asset_id: asset.id,
        root_id: root.id,
        relative_path: "image.jpg",
        modified_at: 1,
        status: LocationStatus::Present,
    })?;

    assert!(matches!(
        store
            .roots()
            .insert(&super::tests::root("/one", "duplicate")),
        Err(StoreError::Sqlite(_))
    ));
    assert!(matches!(
        store.assets().insert(&super::tests::asset(1)),
        Err(StoreError::Sqlite(_))
    ));
    assert!(matches!(
        store.locations().insert(&NewLocation {
            asset_id: 99_999,
            root_id: root.id,
            relative_path: "unknown.jpg",
            modified_at: 1,
            status: LocationStatus::Present,
        }),
        Err(StoreError::Sqlite(_))
    ));
    assert!(store
        .connection()
        .execute(
            "INSERT INTO jobs (asset_id, location_id, stage) VALUES (?1, ?2, ?3)",
            rusqlite::params![asset.id, 1, "hash"],
        )
        .is_err());
    Ok(())
}

#[test]
fn root_registration_rejects_parent_child_overlap_and_toggles_enabled() -> Result<()> {
    let store = Store::open_in_memory()?;
    let first = store.roots().register(&root("/library/photos", "Photos"))?;

    assert!(matches!(
        store
            .roots()
            .register(&root("/library/photos/2026", "Nested")),
        Err(StoreError::RootOverlap { .. })
    ));
    assert!(store
        .roots()
        .register(&root("/library/photos-old", "Not nested"))
        .is_ok());

    let disabled = store.roots().set_enabled(first.id, false)?;
    assert!(!disabled.enabled);
    assert!(store.roots().set_enabled(first.id, true)?.enabled);
    assert!(matches!(
        store.roots().set_enabled(99_999, true),
        Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows))
    ));
    Ok(())
}

#[test]
fn invalid_external_inputs_return_typed_errors_without_panicking() -> Result<()> {
    let store = Store::open_in_memory()?;
    assert!(matches!(
        store.roots().insert(&root("relative/path", "name")),
        Err(StoreError::InvalidInput {
            field: "absolute_path",
            ..
        })
    ));
    assert!(matches!(
        store.roots().insert(&root("/valid", "   ")),
        Err(StoreError::InvalidInput {
            field: "display_name",
            ..
        })
    ));

    let root = store.roots().insert(&root("/valid", "valid"))?;
    let asset = store.assets().insert(&asset(2))?;
    assert!(matches!(
        store.locations().insert(&NewLocation {
            asset_id: asset.id,
            root_id: root.id,
            relative_path: "../escape.jpg",
            modified_at: 1,
            status: LocationStatus::Present,
        }),
        Err(StoreError::InvalidInput {
            field: "relative_path",
            ..
        })
    ));
    assert!(matches!(
        store
            .jobs()
            .enqueue(StageTarget::Asset(asset.id), Stage::Hash, 0, 0),
        Err(StoreError::InvalidInput {
            field: "max_attempts",
            ..
        })
    ));
    assert!(matches!(
        store.stage_states().upsert(
            StageTarget::Asset(asset.id),
            Stage::Hash,
            StageStatus::Succeeded,
            "v1",
            Some("not allowed"),
        ),
        Err(StoreError::InvalidInput { field: "error", .. })
    ));
    Ok(())
}

#[test]
fn parameterized_queries_store_sql_metacharacters_as_data() -> Result<()> {
    let store = Store::open_in_memory()?;
    let hostile = "x'); DROP TABLE roots; --";
    store.settings().set(hostile, hostile)?;
    store.roots().insert(&root("/safe", hostile))?;

    assert_eq!(
        store.settings().get(hostile)?.expect("setting").value,
        hostile
    );
    assert_eq!(store.roots().list()?.len(), 1);
    Ok(())
}

#[test]
fn deleting_root_cascades_locations_but_not_shared_asset() -> Result<()> {
    let store = Store::open_in_memory()?;
    let root_one = store.roots().insert(&root("/one", "one"))?;
    let root_two = store.roots().insert(&root("/two", "two"))?;
    let asset = store.assets().insert(&asset(3))?;
    for (root_id, path) in [(root_one.id, "a.jpg"), (root_two.id, "b.jpg")] {
        store.locations().insert(&NewLocation {
            asset_id: asset.id,
            root_id,
            relative_path: path,
            modified_at: 1,
            status: LocationStatus::Present,
        })?;
    }

    assert!(store.roots().delete(root_one.id)?);
    assert_eq!(store.assets().get(asset.id)?.id, asset.id);
    let count: i64 = store.connection().query_row(
        "SELECT COUNT(*) FROM locations WHERE asset_id = ?1",
        [asset.id],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn independent_new_databases_do_not_share_state() -> Result<()> {
    let temporary = TempDir::new().expect("create temporary directory");
    let first = Store::open(temporary.path().join("first.sqlite3"))?;
    let second = Store::open(temporary.path().join("second.sqlite3"))?;
    first.settings().set("only-first", "value")?;

    assert!(second.settings().get("only-first")?.is_none());
    assert_eq!(first.schema_version()?, LATEST_SCHEMA_VERSION);
    assert_eq!(second.schema_version()?, LATEST_SCHEMA_VERSION);
    Ok(())
}

#[test]
fn changed_or_unknown_applied_migrations_are_rejected() -> Result<()> {
    let temporary = TempDir::new().expect("create temporary directory");
    let changed_path = temporary.path().join("changed.sqlite3");
    {
        let store = Store::open(&changed_path)?;
        store.connection().execute(
            "UPDATE schema_migrations SET checksum = ?1 WHERE version = ?2",
            ("tampered", 1),
        )?;
    }
    assert!(matches!(
        Store::open(&changed_path),
        Err(StoreError::MigrationModified { version: 1, .. })
    ));

    let unknown_path = temporary.path().join("unknown.sqlite3");
    {
        let store = Store::open(&unknown_path)?;
        store.connection().execute(
            "INSERT INTO schema_migrations (version, name, checksum) VALUES (?1, ?2, ?3)",
            (99, "future", "unknown"),
        )?;
    }
    assert!(matches!(
        Store::open(&unknown_path),
        Err(StoreError::UnknownMigration(99))
    ));
    Ok(())
}

#[test]
fn missing_rows_are_reported_as_sqlite_query_errors() -> Result<()> {
    let store = Store::open_in_memory()?;
    assert!(matches!(store.roots().get(42), Err(StoreError::Sqlite(_))));
    assert!(store.settings().get("missing")?.is_none());
    let raw: Option<i64> = store
        .connection()
        .query_row("SELECT id FROM roots WHERE id = ?1", [42], |row| row.get(0))
        .optional()?;
    assert!(raw.is_none());
    Ok(())
}

#[test]
fn asset_metadata_round_trips_and_rejects_unknown_assets() -> Result<()> {
    let store = Store::open_in_memory()?;
    let created = store.assets().insert(&asset(9))?;
    assert_eq!(
        store.assets().metadata(created.id)?,
        Some(AssetMetadata {
            asset_id: created.id,
            ..AssetMetadata::default()
        })
    );

    let metadata = AssetMetadata {
        asset_id: created.id,
        camera_make: Some("Picker".into()),
        camera_model: Some("TestCam".into()),
        lens: Some("35mm".into()),
        exposure_time: Some("1/125".into()),
        f_number: Some(2.8),
        iso: Some(200),
        focal_length_mm: Some(35.0),
        gps_latitude: Some(35.675),
        gps_longitude: Some(139.65),
    };
    store.assets().set_metadata(&metadata)?;
    assert_eq!(store.assets().metadata(created.id)?, Some(metadata));

    let missing = AssetMetadata {
        asset_id: created.id + 100,
        ..AssetMetadata::default()
    };
    assert!(store.assets().set_metadata(&missing).is_err());
    Ok(())
}

#[test]
fn duplicate_queries_find_exact_and_near_matches() -> Result<()> {
    let temporary = TempDir::new().expect("create temporary directory");
    let store = Store::open(temporary.path().join("library.sqlite3"))?;
    let created_root = store.roots().insert(&root("/library/photos", "写真"))?;
    let first = store.assets().insert(&asset(1))?;
    let second = store.assets().insert(&asset(2))?;
    let third = store.assets().insert(&asset(3))?;
    for (asset_id, path) in [
        (first.id, "one.jpg"),
        (first.id, "copy/one.jpg"),
        (second.id, "two.jpg"),
        (third.id, "three.jpg"),
    ] {
        store.locations().insert(&NewLocation {
            asset_id,
            root_id: created_root.id,
            relative_path: path,
            modified_at: 1,
            status: LocationStatus::Present,
        })?;
    }
    let exact = store.assets().exact_duplicates()?;
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].id, first.id);

    store
        .assets()
        .set_perceptual_hash(first.id, &[0b1010_1010; 8])?;
    let mut second_hash = [0b1010_1010; 8];
    second_hash[0] = 0b1010_1011;
    store
        .assets()
        .set_perceptual_hash(second.id, &second_hash)?;
    store.assets().set_perceptual_hash(third.id, &[0; 8])?;
    let near = store.assets().near_duplicates(2, 10)?;
    assert_eq!(near.len(), 1);
    assert_eq!(near[0].left_asset_id, first.id);
    assert_eq!(near[0].right_asset_id, second.id);
    assert_eq!(near[0].distance, 1);

    assert!(store.assets().near_duplicates(1, 1)?.len() == 1);
    Ok(())
}
