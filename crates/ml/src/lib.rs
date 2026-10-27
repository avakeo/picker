#![deny(unsafe_code)]

//! Model boundaries and model registry for Picker.

mod embedder;
mod registry;
mod vector;

pub use embedder::{ClipJapanese, Embedder, EmbeddingError};

pub use vector::{BruteForceIndex, VectorError, VectorIndex};

pub use registry::{
    catalog, HttpDownloader, ModelDownloader, ModelError, ModelFile, ModelInfo, ModelRegistry,
    ModelState, ModelVersion, Progress,
};
