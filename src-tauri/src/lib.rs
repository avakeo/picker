#![deny(unsafe_code)]

//! Tauri application bootstrap library.

mod thumbnail_protocol;

/// Starts the Tauri desktop application.
pub fn run() {
    tauri::Builder::default()
        .register_uri_scheme_protocol("asset", |_context, request| {
            thumbnail_protocol::handle(request)
        })
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
