#![deny(unsafe_code)]

//! Model boundaries and model registry for Picker.

mod embedder;
mod registry;

pub use embedder::{ClipJapanese, Embedder, EmbeddingError};

pub use registry::{
    catalog, HttpDownloader, ModelDownloader, ModelError, ModelFile, ModelInfo, ModelRegistry,
    ModelState, ModelVersion, Progress,
};
