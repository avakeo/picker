#![deny(unsafe_code)]

//! Tauri application bootstrap library.

mod commands;
pub mod contract;
mod thumbnail_protocol;

use std::fs;

use contract::AppInfo;

/// Returns the application and model license information defined by the UI contract.
#[must_use]
#[tauri::command]
fn get_app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        dependency_licenses: Vec::new(),
        model_licenses: Vec::new(),
    }
}

/// Starts the Tauri desktop application.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_app_info,
            commands::roots_list,
            commands::root_add,
            commands::root_remove,
            commands::root_set_enabled,
            commands::rescan_root,
            commands::asset_search,
            commands::asset_detail,
            commands::finder_reveal,
            commands::viewport_update,
            commands::index_status,
            commands::pause_index,
            commands::resume_index,
            commands::tags_list,
            commands::tag_create,
            commands::tag_rename,
            commands::tag_delete,
            commands::tag_set_parent,
            commands::tag_merge,
            commands::tag_aliases,
            commands::tag_add_alias,
            commands::tag_remove_alias,
            commands::tag_assign,
            commands::tag_reject,
            commands::auto_tag,
            commands::tag_remove,
            commands::asset_filter,
            commands::duplicates_list,
            commands::color_set,
            commands::color_reset,
            commands::albums_list,
            commands::album_create,
            commands::album_rename,
            commands::album_delete,
            commands::album_add_assets,
            commands::album_remove_assets,
            commands::album_reorder,
            commands::album_assets,
            commands::models_list,
            commands::model_install,
            commands::model_activate,
            commands::model_remove,
            commands::embedding_status,
            commands::embedding_index,
            commands::semantic_search,
            commands::similar_assets,
        ])
        .register_uri_scheme_protocol("asset", |_context, request| {
            thumbnail_protocol::handle(request)
        })
        .setup(|app| {
            use tauri::Manager;

            let data_dir = app.path().app_data_dir()?;
            fs::create_dir_all(&data_dir)?;
            let database = commands::default_database(&data_dir);
            let cache = commands::default_cache(&data_dir);
            let mut library = picker_core::Library::open(&database, &cache)?;
            thumbnail_protocol::set_cache_root(library.cache_root().to_path_buf());
            let models = picker_ml::ModelRegistry::open(data_dir.join("models"));
            if let Err(error) = commands::configure_embedder(&mut library, &models) {
                eprintln!("embedding model unavailable: {error}");
            }
            app.manage(commands::AppState::new(library, models));
            let _core = create_core();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Picker application");
}

/// Returns the application core used by the desktop bootstrap.
#[must_use]
pub const fn create_core() -> picker_core::Core {
    picker_core::Core::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{CommandContract, GetAppInfo, GetAppInfoRequest};

    #[test]
    fn ids_and_paths_are_bounded() {
        assert!(commands::parse_id("1").is_ok());
        assert!(commands::parse_id("0").is_err());
        assert!(commands::parse_id("-1").is_err());
        assert!(commands::parse_id("../../etc/passwd").is_err());
        let data = std::path::Path::new("/tmp/picker");
        assert_eq!(
            commands::default_database(data),
            std::path::PathBuf::from("/tmp/picker/library.sqlite3")
        );
        assert_eq!(
            commands::default_cache(data),
            std::path::PathBuf::from("/tmp/picker/thumbnails")
        );
    }

    #[test]
    fn sample_app_info_command_matches_the_wire_contract() {
        let request =
            serde_json::to_value(GetAppInfoRequest::default()).expect("request should serialize");
        assert_eq!(GetAppInfo::NAME, "get_app_info");
        assert_eq!(request, serde_json::json!({}));

        let response = serde_json::to_value(get_app_info()).expect("response should serialize");
        assert_eq!(response["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(response["dependencyLicenses"], serde_json::json!([]));
        assert_eq!(response["modelLicenses"], serde_json::json!([]));
    }
}
