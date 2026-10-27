use super::*;

fn seed_asset(store: &Store) -> Asset {
    store
        .assets()
        .insert(&NewAsset {
            blake3_hash: &[7; 32],
            file_size: 100,
            width: Some(10),
            height: Some(10),
            format: Some("jpeg"),
            captured_at: None,
            perceptual_hash: None,
        })
        .expect("asset")
}

fn colors(values: &[(&str, f64)]) -> Vec<(String, f64)> {
    values
        .iter()
        .map(|(hex, ratio)| ((*hex).to_owned(), *ratio))
        .collect()
}

#[test]
fn automatic_colors_are_replaced_but_user_colors_are_protected() -> Result<()> {
    let store = Store::open_in_memory()?;
    let asset = seed_asset(&store);
    assert!(store.colors().set_auto(
        asset.id,
        &colors(&[("#DD1122", 0.7), ("#1122DD", 0.3)]),
        "color-v1"
    )?);
    let auto = store.colors().list(asset.id)?;
    assert_eq!(auto.len(), 2);
    assert_eq!(auto[0].hex, "#DD1122");
    assert_eq!(auto[0].source, ColorSource::Auto);
    assert_eq!(auto[0].rank, 1);

    assert!(store
        .colors()
        .set_auto(asset.id, &colors(&[("#22CC33", 1.0)]), "color-v2")?);
    assert_eq!(store.colors().list(asset.id)?.len(), 1);

    store
        .colors()
        .set_user(asset.id, &colors(&[("#000000", 1.0)]))?;
    assert!(store.colors().is_manual(asset.id)?);
    assert!(!store
        .colors()
        .set_auto(asset.id, &colors(&[("#FFFFFF", 1.0)]), "color-v3")?);
    let user = store.colors().list(asset.id)?;
    assert_eq!(user.len(), 1);
    assert_eq!(user[0].hex, "#000000");
    assert_eq!(user[0].source, ColorSource::User);
    assert_eq!(user[0].algorithm_version, "manual");
    Ok(())
}

#[test]
fn an_empty_user_edit_is_still_an_override() -> Result<()> {
    let store = Store::open_in_memory()?;
    let asset = seed_asset(&store);
    store.colors().set_user(asset.id, &[])?;
    assert!(store.colors().is_manual(asset.id)?);
    assert!(store.colors().list(asset.id)?.is_empty());
    assert!(!store
        .colors()
        .set_auto(asset.id, &colors(&[("#123456", 1.0)]), "v1")?);
    assert!(store.colors().list(asset.id)?.is_empty());

    store.colors().clear_manual(asset.id)?;
    assert!(!store.colors().is_manual(asset.id)?);
    assert!(store
        .colors()
        .set_auto(asset.id, &colors(&[("#123456", 1.0)]), "v1")?);
    assert_eq!(store.colors().list(asset.id)?.len(), 1);
    Ok(())
}

#[test]
fn invalid_colors_are_rejected() -> Result<()> {
    let store = Store::open_in_memory()?;
    let asset = seed_asset(&store);
    for invalid in [
        vec![("#GGGGGG".to_owned(), 1.0)],
        vec![("#123456".to_owned(), -0.1)],
        vec![("#123456".to_owned(), 1.1)],
        vec![("#123456".to_owned(), f64::NAN)],
        vec![
            ("#111111".to_owned(), 0.2),
            ("#222222".to_owned(), 0.2),
            ("#333333".to_owned(), 0.2),
            ("#444444".to_owned(), 0.2),
        ],
    ] {
        assert!(
            store.colors().set_user(asset.id, &invalid).is_err(),
            "{invalid:?}"
        );
        assert!(
            store.colors().set_auto(asset.id, &invalid, "v1").is_err(),
            "{invalid:?}"
        );
    }
    assert!(store
        .colors()
        .set_auto(asset.id, &colors(&[("#123456", 1.0)]), " ")
        .is_err());
    Ok(())
}

#[test]
fn deleting_an_asset_removes_its_colors() -> Result<()> {
    let store = Store::open_in_memory()?;
    let asset = seed_asset(&store);
    store
        .colors()
        .set_user(asset.id, &colors(&[("#123456", 1.0)]))?;
    store
        .connection()
        .execute("DELETE FROM assets WHERE id = ?1", [asset.id])?;
    assert!(store.colors().list(asset.id)?.is_empty());
    assert!(!store.colors().is_manual(asset.id)?);
    Ok(())
}
