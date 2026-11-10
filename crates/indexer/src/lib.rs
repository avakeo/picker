#![deny(unsafe_code)]

//! Resumable indexing pipeline for Picker.

mod scanner;

pub use scanner::{DiscoveredFile, ScanError, Scanner};

mod identity;
pub use identity::{IdentityError, IdentityReport, IdentityResolver};

use picker_store::{Job, Result, Stage, StageStatus, StageTarget, Store};

#[derive(Debug, Clone, Copy)]
pub struct QueueConfig {
    pub hash: usize,
    pub metadata: usize,
    pub thumbnail: usize,
    pub perceptual_hash: usize,
    pub embedding: usize,
    pub auto_tag: usize,
    pub custom_suggest: usize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            hash: 2,
            metadata: 2,
            thumbnail: 2,
            perceptual_hash: 2,
            embedding: 1,
            auto_tag: 1,
            custom_suggest: 1,
        }
    }
}

impl QueueConfig {
    #[must_use]
    pub const fn for_stage(self, stage: Stage) -> usize {
        match stage {
            Stage::Hash => self.hash,
            Stage::Metadata => self.metadata,
            Stage::Thumbnail => self.thumbnail,
            Stage::PerceptualHash => self.perceptual_hash,
            Stage::Embedding => self.embedding,
            Stage::AutoTag => self.auto_tag,
            Stage::CustomSuggest => self.custom_suggest,
        }
    }
}

/// Durable queue orchestration; worker execution is intentionally outside this boundary.
#[derive(Debug)]
pub struct PersistentQueue<'store> {
    store: &'store Store,
    pub config: QueueConfig,
}

impl<'store> PersistentQueue<'store> {
    #[must_use]
    pub const fn new(store: &'store Store, config: QueueConfig) -> Self {
        Self { store, config }
    }

    pub fn enqueue_stage(
        &self,
        target: StageTarget,
        stage: Stage,
        version: &str,
        priority: i64,
    ) -> Result<Vec<Job>> {
        let ordered = Stage::ordered();
        let start = ordered
            .iter()
            .position(|candidate| *candidate == stage)
            .expect("stage is in ordered list");
        let mut jobs = Vec::new();
        for &downstream in &ordered[start..] {
            self.store.stage_states().upsert(
                target,
                downstream,
                StageStatus::Pending,
                version,
                None,
            )?;
            jobs.push(self.store.jobs().enqueue(target, downstream, priority, 3)?);
        }
        Ok(jobs)
    }

    pub fn dequeue(&self, stage: Option<Stage>) -> Result<Option<Job>> {
        self.store.jobs().dequeue(stage)
    }
    pub fn recover_running(&self) -> Result<usize> {
        self.store.jobs().recover_running()
    }
    pub fn mark_succeeded(&self, id: i64) -> Result<Job> {
        self.store.jobs().mark_succeeded(id)
    }
    pub fn fail_or_retry(&self, id: i64, error: &str) -> Result<Job> {
        self.store.jobs().fail_or_retry(id, error)
    }
    pub fn pause(&self) -> Result<usize> {
        self.store.jobs().pause_pending()
    }
    pub fn resume(&self) -> Result<usize> {
        self.store.jobs().resume_paused()
    }
}

#[derive(Debug, Default)]
pub struct Indexer;

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(store: &Store) -> i64 {
        store
            .assets()
            .insert(&picker_store::NewAsset {
                blake3_hash: Box::leak(Box::new([7; 32])),
                file_size: 1,
                width: Some(1),
                height: Some(1),
                format: Some("jpeg"),
                captured_at: None,
                perceptual_hash: None,
            })
            .expect("asset")
            .id
    }

    #[test]
    fn priority_queue_recovers_running_jobs() -> Result<()> {
        let store = Store::open_in_memory()?;
        let target = StageTarget::Asset(asset(&store));
        let low = store.jobs().enqueue(target, Stage::Hash, 1, 3)?;
        let high = store.jobs().enqueue(target, Stage::Metadata, 10, 3)?;
        let queue = PersistentQueue::new(&store, QueueConfig::default());
        assert_eq!(queue.dequeue(None)?.expect("job").id, high.id);
        assert_eq!(queue.recover_running()?, 1);
        assert_eq!(queue.dequeue(None)?.expect("job").id, high.id);
        assert_eq!(store.jobs().dequeue(None)?.expect("job").id, low.id);
        Ok(())
    }

    #[test]
    fn retries_pause_resume_and_version_cascade_are_persistent() -> Result<()> {
        let store = Store::open_in_memory()?;
        let target = StageTarget::Asset(asset(&store));
        let queue = PersistentQueue::new(&store, QueueConfig::default());
        let jobs = queue.enqueue_stage(target, Stage::Embedding, "embed-v2", 5)?;
        assert_eq!(jobs.len(), 3);
        assert_eq!(
            store
                .stage_states()
                .get(target, Stage::AutoTag)?
                .expect("state")
                .version,
            "embed-v2"
        );
        let first = queue.dequeue(Some(Stage::Embedding))?.expect("embedding");
        assert_eq!(
            queue.fail_or_retry(first.id, "temporary")?.status,
            picker_store::JobStatus::Pending
        );
        assert_eq!(queue.pause()?, 3);
        assert_eq!(queue.resume()?, 3);
        assert_eq!(queue.config.for_stage(Stage::Embedding), 1);
        Ok(())
    }
}
