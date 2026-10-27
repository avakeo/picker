#![deny(unsafe_code)]

//! Tauri application bootstrap library.

pub mod contract;

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
        .invoke_handler(tauri::generate_handler![get_app_info])
        .setup(|_app| {
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
