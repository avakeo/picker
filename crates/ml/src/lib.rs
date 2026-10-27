#![deny(unsafe_code)]

//! Model boundaries and model registry for Picker.

mod registry;

pub use registry::{
    catalog, HttpDownloader, ModelDownloader, ModelError, ModelFile, ModelInfo, ModelRegistry,
    ModelState, ModelVersion, Progress,
};
