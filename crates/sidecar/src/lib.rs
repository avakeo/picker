#![deny(unsafe_code)]

//! JSON export/import and XMP sidecar boundary for Picker.

mod xmp;
pub use xmp::{merge, XmpError};

/// Placeholder for portable metadata handling introduced in M3.
#[derive(Debug, Default)]
pub struct Sidecar;
