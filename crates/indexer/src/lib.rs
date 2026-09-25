#![deny(unsafe_code)]

//! Resumable indexing pipeline for Picker.

mod scanner;

pub use scanner::{DiscoveredFile, ScanError, Scanner};

/// Placeholder for the persistent job queue introduced in T1-5.
#[derive(Debug, Default)]
pub struct Indexer;
