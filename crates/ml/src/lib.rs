#![deny(unsafe_code)]

//! Model boundaries and model registry for Picker.

mod classifier;
mod embedder;
mod registry;
mod tagger;
mod vector;

pub use classifier::{select_review_candidates, LogisticRegression};
pub use embedder::{ClipJapanese, Embedder, EmbeddingError};
pub use tagger::{TagSuggestion, Tagger, ZeroShotTag, ZeroShotTagger};

pub use vector::{BruteForceIndex, VectorError, VectorIndex};

pub use registry::{
    catalog, HttpDownloader, ModelDownloader, ModelError, ModelFile, ModelInfo, ModelRegistry,
    ModelState, ModelVersion, Progress,
};
