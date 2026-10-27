use serde::Deserialize;

use crate::{Embedder, EmbeddingError};

const VOCABULARY: &str = include_str!("../data/zero_shot_tags.json");

/// One entry of the bundled zero-shot vocabulary.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ZeroShotTag {
    pub id: String,
    pub prompt: String,
    pub display_name_ja: String,
    pub display_name_en: String,
    pub category: String,
    pub threshold: f64,
}

/// A scored tag suggestion for one image.
#[derive(Debug, Clone, PartialEq)]
pub struct TagSuggestion {
    pub tag_id: String,
    pub confidence: f64,
}

/// Scores images against a fixed tag vocabulary.
pub trait Tagger: Send + Sync {
    fn suggest(&self, image: &[f32]) -> Result<Vec<TagSuggestion>, EmbeddingError>;
}

/// CLIP-based zero-shot tagger with averaged prompt embeddings.
pub struct ZeroShotTagger {
    tags: Vec<ZeroShotTag>,
    embeddings: Vec<Vec<f32>>,
    dimensions: usize,
}

impl ZeroShotTagger {
    /// Loads the bundled vocabulary.
    pub fn vocabulary() -> Result<Vec<ZeroShotTag>, serde_json::Error> {
        serde_json::from_str(VOCABULARY)
    }

    pub fn new(embedder: &dyn Embedder, tags: Vec<ZeroShotTag>) -> Result<Self, EmbeddingError> {
        let dimensions = embedder.dimensions();
        if tags.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        let mut embeddings = Vec::with_capacity(tags.len());
        for tag in &tags {
            let prompts = [
                tag.prompt.clone(),
                tag.prompt.replace("a photo", "a photograph"),
                format!("a realistic photo showing {}", tag.display_name_en),
            ];
            let mut average = vec![0.0_f32; dimensions];
            for prompt in prompts {
                let mut text = embedder.embed_text(&prompt)?;
                if text.len() != dimensions {
                    return Err(EmbeddingError::InvalidDimensions {
                        expected: dimensions,
                        actual: text.len(),
                    });
                }
                normalize(&mut text);
                for (sum, value) in average.iter_mut().zip(text) {
                    *sum += value;
                }
            }
            normalize(&mut average);
            embeddings.push(average);
        }
        Ok(Self {
            tags,
            embeddings,
            dimensions,
        })
    }

    #[must_use]
    pub fn tags(&self) -> &[ZeroShotTag] {
        &self.tags
    }
}

fn normalize(vector: &mut [f32]) {
    let norm = vector
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        for value in vector {
            *value = (f64::from(*value) / norm) as f32;
        }
    }
}

fn sigmoid(value: f64) -> f64 {
    1.0 / (1.0 + (-value).exp())
}

impl Tagger for ZeroShotTagger {
    fn suggest(&self, image: &[f32]) -> Result<Vec<TagSuggestion>, EmbeddingError> {
        if image.len() != self.dimensions {
            return Err(EmbeddingError::InvalidDimensions {
                expected: self.dimensions,
                actual: image.len(),
            });
        }
        if image.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        let mut image = image.to_vec();
        normalize(&mut image);
        let mut suggestions = Vec::new();
        for (tag, embedding) in self.tags.iter().zip(&self.embeddings) {
            let similarity = image
                .iter()
                .zip(embedding)
                .map(|(left, right)| f64::from(*left) * f64::from(*right))
                .sum::<f64>();
            let confidence = sigmoid(12.0 * (similarity - 0.22));
            if confidence >= tag.threshold {
                suggestions.push(TagSuggestion {
                    tag_id: tag.id.clone(),
                    confidence,
                });
            }
        }
        suggestions.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.tag_id.cmp(&right.tag_id))
        });
        Ok(suggestions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    struct FakeEmbedder;

    impl Embedder for FakeEmbedder {
        fn embed_image(&self, _path: &Path) -> Result<Vec<f32>, EmbeddingError> {
            Ok(vec![1.0, 0.0])
        }

        fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            if text.contains("cat") {
                Ok(vec![1.0, 0.0])
            } else if text.contains("dog") {
                Ok(vec![0.0, 1.0])
            } else {
                Ok(vec![0.7, 0.7])
            }
        }

        fn dimensions(&self) -> usize {
            2
        }

        fn model_id(&self) -> &str {
            "fake"
        }

        fn model_version(&self) -> &str {
            "1"
        }
    }

    fn tag(id: &str, prompt: &str) -> ZeroShotTag {
        ZeroShotTag {
            id: id.to_owned(),
            prompt: prompt.to_owned(),
            display_name_ja: id.to_owned(),
            display_name_en: id.to_owned(),
            category: "test".to_owned(),
            threshold: 0.62,
        }
    }

    #[test]
    fn vocabulary_is_nonempty_and_unique() {
        let vocabulary = ZeroShotTagger::vocabulary().expect("vocabulary");
        assert!(vocabulary.len() >= 200);
        let mut ids: Vec<_> = vocabulary.iter().map(|tag| tag.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), vocabulary.len());
        assert!(vocabulary
            .iter()
            .all(|tag| (0.0..=1.0).contains(&tag.threshold)));
    }

    #[test]
    fn ranks_matching_prompts_and_filters_by_threshold() {
        let tagger = ZeroShotTagger::new(
            &FakeEmbedder,
            vec![
                tag("cat", "a photo of a cat"),
                tag("dog", "a photo of a dog"),
            ],
        )
        .expect("tagger");
        let suggestions = tagger.suggest(&[1.0, 0.0]).expect("suggest");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tag_id, "cat");
        assert!(suggestions[0].confidence > 0.99);
    }

    #[test]
    fn rejects_wrong_dimensions() {
        let tagger = ZeroShotTagger::new(&FakeEmbedder, vec![tag("cat", "a photo of a cat")])
            .expect("tagger");
        assert!(matches!(
            tagger.suggest(&[1.0]),
            Err(EmbeddingError::InvalidDimensions { .. })
        ));
    }
}
