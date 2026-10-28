//! Queued model work for one asset: embedding, zero-shot auto-tagging, and
//! custom-tag suggestions (design 7.4). Workers run these on their own
//! library; the UI library picks up new vectors with
//! [`Library::sync_vectors`].

use picker_ml::{Embedder, LogisticRegression};
use picker_store::{AssignmentSource, JobStatus, Stage, StageStatus, StageTarget, TagState};

use crate::library::{Library, LibraryError, Result};

/// Stages that need the embedding model.
pub const ML_STAGES: &[Stage] = &[Stage::Embedding, Stage::AutoTag, Stage::CustomSuggest];

/// Version recorded for per-asset custom suggestions.
pub(crate) const CUSTOM_SUGGEST_STAGE_VERSION: &str = "custom-asset-v1";
/// Cosine similarity needed when the tag has at least three examples.
const PROTOTYPE_THRESHOLD: f32 = 0.35;
/// Cosine similarity needed when only the tag name is known.
const TAG_NAME_THRESHOLD: f32 = 0.30;
const CLASSIFIER_THRESHOLD: f64 = 0.5;
const MIN_PROTOTYPE_EXAMPLES: usize = 3;

impl Library {
    /// Claims and runs the most urgent model job. Returns `None` when no
    /// model job is waiting or no embedding model is set.
    pub fn run_next_ml_job(&self) -> Result<Option<crate::JobRun>> {
        let Some(embedder) = self.current_embedder() else {
            return Ok(None);
        };
        let Some(job) = self.store().jobs().dequeue_any(ML_STAGES)? else {
            return Ok(None);
        };
        let StageTarget::Asset(asset_id) = job.target else {
            let job = self.store().jobs().mark_succeeded(job.id)?;
            return Ok(Some(crate::JobRun {
                job_id: job.id,
                asset_id: None,
                status: job.status,
            }));
        };
        let outcome = match job.stage {
            Stage::Embedding => self.embed_asset(embedder.as_ref(), asset_id),
            Stage::AutoTag => self.auto_tag_asset(asset_id).map(|_| ()),
            Stage::CustomSuggest => self
                .suggest_custom_for_asset(embedder.as_ref(), asset_id)
                .map(|_| ()),
            other => Err(LibraryError::InvalidInput(format!(
                "stage {} is not a model stage",
                other.as_str()
            ))),
        };
        let finished = match &outcome {
            Ok(()) => {
                if job.stage == Stage::Embedding {
                    // Downstream stages read the new embedding.
                    for stage in [Stage::AutoTag, Stage::CustomSuggest] {
                        self.store().jobs().enqueue(
                            StageTarget::Asset(asset_id),
                            stage,
                            job.priority,
                            3,
                        )?;
                    }
                }
                self.store().jobs().mark_succeeded(job.id)
            }
            Err(error) => {
                let message = error.to_string();
                let finished = self.store().jobs().fail_or_retry(job.id, &message);
                if matches!(&finished, Ok(failed) if failed.status == JobStatus::Failed) {
                    self.store().stage_states().upsert(
                        StageTarget::Asset(asset_id),
                        job.stage,
                        StageStatus::Failed,
                        &stage_version(embedder.as_ref(), job.stage),
                        Some(&message),
                    )?;
                }
                finished
            }
        };
        let status = finished.map_or(JobStatus::Succeeded, |job| job.status);
        Ok(Some(crate::JobRun {
            job_id: job.id,
            asset_id: Some(asset_id),
            status,
        }))
    }

    /// Runs model jobs on this thread until none is waiting.
    pub fn run_queued_ml_jobs(&self) -> Result<usize> {
        let mut processed = 0;
        while self.run_next_ml_job()?.is_some() {
            processed += 1;
        }
        Ok(processed)
    }

    /// Queues embedding jobs for assets that lack an embedding of the
    /// current model.
    pub fn enqueue_missing_embeddings(&self) -> Result<usize> {
        let Some(embedder) = self.current_embedder() else {
            return Ok(0);
        };
        Ok(self
            .store()
            .jobs()
            .enqueue_missing_embeddings(embedder.model_id(), embedder.model_version())?)
    }

    fn embed_asset(&self, embedder: &dyn Embedder, asset_id: i64) -> Result<()> {
        let (model_id, model_version) = (embedder.model_id(), embedder.model_version());
        if self
            .store()
            .embeddings()
            .get(asset_id, model_id, model_version)?
            .is_none()
        {
            let Some(source) = self.present_source(asset_id)? else {
                // The file went missing; a later scan queues it again.
                return Ok(());
            };
            let vector = embedder
                .embed_image(&source)
                .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
            self.store()
                .embeddings()
                .upsert(asset_id, model_id, model_version, &vector)?;
        }
        self.store().stage_states().upsert(
            StageTarget::Asset(asset_id),
            Stage::Embedding,
            StageStatus::Succeeded,
            &stage_version(embedder, Stage::Embedding),
            None,
        )?;
        Ok(())
    }

    /// Scores one asset against every custom tag and keeps a suggestion
    /// where the score passes the tag's threshold. Returns the number of
    /// suggestions. Tags with a trained classifier use it; otherwise the
    /// average of confirmed examples, or the tag name, is compared.
    pub(crate) fn suggest_custom_for_asset(
        &self,
        embedder: &dyn Embedder,
        asset_id: i64,
    ) -> Result<usize> {
        let (model_id, model_version) = (embedder.model_id(), embedder.model_version());
        let Some(embedding) = self
            .store()
            .embeddings()
            .get(asset_id, model_id, model_version)?
        else {
            return Ok(0);
        };
        let mut suggested = 0;
        for tag in self.store().tags().list()? {
            if tag.namespace != "custom" {
                continue;
            }
            let score = match self.store().classifiers().get(tag.id)? {
                Some(classifier) if classifier.weights.len() == embedding.vector.len() => {
                    let classifier =
                        LogisticRegression::from_parts(classifier.weights, classifier.bias);
                    let probability = classifier.predict(&embedding.vector);
                    (probability >= CLASSIFIER_THRESHOLD).then_some(probability)
                }
                _ => {
                    let (query, threshold) = self.custom_query(embedder, &tag)?;
                    let similarity = dot(&query, &embedding.vector);
                    (similarity >= threshold).then_some(f64::from(similarity.min(1.0)))
                }
            };
            self.store().tag_assignments().clear_suggested(
                asset_id,
                tag.id,
                AssignmentSource::CustomClassifier,
            )?;
            if let Some(score) = score {
                if self
                    .store()
                    .tag_assignments()
                    .upsert_suggested(
                        asset_id,
                        tag.id,
                        AssignmentSource::CustomClassifier,
                        score,
                        model_id,
                        model_version,
                    )?
                    .is_some_and(|assignment| assignment.state == TagState::Suggested)
                {
                    suggested += 1;
                }
            }
        }
        self.store().stage_states().upsert(
            StageTarget::Asset(asset_id),
            Stage::CustomSuggest,
            StageStatus::Succeeded,
            CUSTOM_SUGGEST_STAGE_VERSION,
            None,
        )?;
        Ok(suggested)
    }

    /// The vector a custom tag is compared with, and its threshold.
    fn custom_query(
        &self,
        embedder: &dyn Embedder,
        tag: &picker_store::Tag,
    ) -> Result<(Vec<f32>, f32)> {
        let mut examples = Vec::new();
        for assignment in self
            .store()
            .tag_assignments()
            .list_for_tag(tag.id, TagState::Confirmed)?
        {
            if let Some(embedding) = self.store().embeddings().get(
                assignment.asset_id,
                embedder.model_id(),
                embedder.model_version(),
            )? {
                examples.push(embedding.vector);
            }
        }
        if examples.len() >= MIN_PROTOTYPE_EXAMPLES {
            return Ok((normalized_mean(&examples)?, PROTOTYPE_THRESHOLD));
        }
        let query = embedder
            .embed_text(&format!("{} {}", tag.display_name_ja, tag.canonical_name))
            .map_err(|error| LibraryError::InvalidInput(error.to_string()))?;
        Ok((query, TAG_NAME_THRESHOLD))
    }
}

fn stage_version(embedder: &dyn Embedder, stage: Stage) -> String {
    match stage {
        Stage::AutoTag => crate::library::AUTO_TAG_STAGE_VERSION.to_owned(),
        Stage::CustomSuggest => CUSTOM_SUGGEST_STAGE_VERSION.to_owned(),
        _ => format!("{}@{}", embedder.model_id(), embedder.model_version()),
    }
}

fn dot(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() {
        return f32::NEG_INFINITY;
    }
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

/// Mean of unit vectors, scaled back to unit length.
pub(crate) fn normalized_mean(vectors: &[Vec<f32>]) -> Result<Vec<f32>> {
    let Some(first) = vectors.first() else {
        return Err(LibraryError::InvalidInput(
            "at least one vector is required".to_owned(),
        ));
    };
    let mut mean = vec![0.0_f64; first.len()];
    for vector in vectors {
        if vector.len() != mean.len() {
            return Err(LibraryError::InvalidInput(
                "embedding dimensions do not match the model".to_owned(),
            ));
        }
        for (sum, value) in mean.iter_mut().zip(vector) {
            *sum += f64::from(*value);
        }
    }
    let norm = mean.iter().map(|value| value * value).sum::<f64>().sqrt();
    #[allow(clippy::cast_possible_truncation)]
    Ok(mean
        .into_iter()
        .map(|value| {
            if norm > 0.0 {
                (value / norm) as f32
            } else {
                0.0
            }
        })
        .collect())
}
