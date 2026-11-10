use serde::Deserialize;

use crate::{Embedder, EmbeddingError};

const VOCABULARY: &str = include_str!("../data/zero_shot_tags.json");

/// The most suggestions one image receives.
pub const MAX_SUGGESTIONS: usize = 5;

/// Japanese prompt templates; `{}` is replaced with the Japanese tag name.
/// The model was trained on Japanese text, and these separate the right tag
/// from the rest more clearly than the English prompts (ADR-010).
const PROMPT_TEMPLATES: [&str; 3] = ["{}の写真", "{}が写っている写真", "{}の画像"];

/// One entry of the bundled zero-shot vocabulary.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ZeroShotTag {
    pub id: String,
    pub prompt: String,
    pub display_name_ja: String,
    pub display_name_en: String,
    pub category: String,
    /// The z-score, relative to the same image's other tags, at or above
    /// which the tag is suggested.
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
///
/// Images differ more in their similarity to every tag than tags differ
/// within one image, so a fixed similarity cut-off tags some images with
/// dozens of tags and others with none. Each tag is instead scored by how far
/// it stands out from the same image's other tags (ADR-010).
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
            let prompts =
                PROMPT_TEMPLATES.map(|template| template.replace("{}", &tag.display_name_ja));
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
        let similarities: Vec<f64> = self
            .embeddings
            .iter()
            .map(|embedding| {
                image
                    .iter()
                    .zip(embedding)
                    .map(|(left, right)| f64::from(*left) * f64::from(*right))
                    .sum::<f64>()
            })
            .collect();
        let count = similarities.len() as f64;
        let mean = similarities.iter().sum::<f64>() / count;
        let deviation = (similarities
            .iter()
            .map(|similarity| (similarity - mean).powi(2))
            .sum::<f64>()
            / count)
            .sqrt();
        // With no spread no tag stands out, and the z-scores are undefined.
        if !deviation.is_finite() || deviation <= f64::EPSILON {
            return Ok(Vec::new());
        }
        let mut suggestions = Vec::new();
        for (tag, similarity) in self.tags.iter().zip(similarities) {
            let z = (similarity - mean) / deviation;
            if z >= tag.threshold {
                suggestions.push(TagSuggestion {
                    tag_id: tag.id.clone(),
                    // A tag right at its threshold gets 0.5, so the review
                    // boundary (design.md 8.5) sits at the threshold.
                    confidence: sigmoid(2.0 * (z - tag.threshold)),
                });
            }
        }
        suggestions.sort_by(|left, right| {
            right
                .confidence
                .total_cmp(&left.confidence)
                .then_with(|| left.tag_id.cmp(&right.tag_id))
        });
        suggestions.truncate(MAX_SUGGESTIONS);
        Ok(suggestions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::Path, sync::Mutex};

    /// Maps prompts mentioning "cat" to one axis and everything else to the
    /// other, and records every prompt it is asked to embed.
    #[derive(Default)]
    struct FakeEmbedder {
        prompts: Mutex<Vec<String>>,
    }

    impl Embedder for FakeEmbedder {
        fn embed_image(&self, _path: &Path) -> Result<Vec<f32>, EmbeddingError> {
            Ok(vec![1.0, 0.0])
        }

        fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            self.prompts.lock().expect("prompts").push(text.to_owned());
            if text.contains("cat") {
                Ok(vec![1.0, 0.0])
            } else {
                Ok(vec![0.0, 1.0])
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

    fn tag(id: &str, threshold: f64) -> ZeroShotTag {
        ZeroShotTag {
            id: id.to_owned(),
            prompt: format!("a photo of {id}"),
            display_name_ja: id.to_owned(),
            display_name_en: id.to_owned(),
            category: "test".to_owned(),
            threshold,
        }
    }

    fn tagger(tags: Vec<ZeroShotTag>) -> ZeroShotTagger {
        ZeroShotTagger::new(&FakeEmbedder::default(), tags).expect("tagger")
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
            .all(|tag| tag.threshold.is_finite() && tag.threshold > 0.0));
        assert!(vocabulary
            .iter()
            .all(|tag| !tag.display_name_ja.trim().is_empty()));
    }

    #[test]
    fn prompts_are_built_from_the_japanese_name() {
        let embedder = FakeEmbedder::default();
        let mut cat = tag("cat", 1.0);
        cat.display_name_ja = "猫cat".to_owned();
        ZeroShotTagger::new(&embedder, vec![cat]).expect("tagger");
        assert_eq!(
            *embedder.prompts.lock().expect("prompts"),
            ["猫catの写真", "猫catが写っている写真", "猫catの画像"]
        );
    }

    #[test]
    fn suggests_only_the_tag_that_stands_out_within_the_image() {
        // Similarities are [1, 0, 0, 0, 0]: the mean is 0.2 and the standard
        // deviation 0.4, so "cat" has z = 2 and the others z = -0.5.
        let tagger = tagger(vec![
            tag("cat", 1.5),
            tag("dog", 1.5),
            tag("car", 1.5),
            tag("tree", 1.5),
            tag("sky", 1.5),
        ]);
        let suggestions = tagger.suggest(&[1.0, 0.0]).expect("suggest");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].tag_id, "cat");
        assert!((suggestions[0].confidence - sigmoid(1.0)).abs() < 1e-6);
    }

    #[test]
    fn the_score_does_not_depend_on_the_image_baseline() {
        // [0.8, 0.6] gives similarities of 0.2 * s + 0.6 for the s of
        // [1, 0]: every tag is far more similar, yet the z-scores match.
        let tags = || {
            vec![
                tag("cat", 1.5),
                tag("dog", 1.5),
                tag("car", 1.5),
                tag("tree", 1.5),
                tag("sky", 1.5),
            ]
        };
        let low = tagger(tags()).suggest(&[1.0, 0.0]).expect("suggest");
        let high = tagger(tags()).suggest(&[0.8, 0.6]).expect("suggest");
        assert_eq!(low.len(), 1);
        assert_eq!(high.len(), 1);
        assert_eq!(low[0].tag_id, high[0].tag_id);
        assert!((low[0].confidence - high[0].confidence).abs() < 1e-6);
    }

    #[test]
    fn a_tag_right_at_its_threshold_gets_half_confidence() {
        // "cat" has z = 2 here; the threshold sits just below it so that
        // rounding cannot drop the tag.
        let threshold = 2.0 - 1e-9;
        let tagger = tagger(vec![
            tag("cat", threshold),
            tag("dog", threshold),
            tag("car", threshold),
            tag("tree", threshold),
            tag("sky", threshold),
        ]);
        let suggestions = tagger.suggest(&[1.0, 0.0]).expect("suggest");
        assert_eq!(suggestions.len(), 1);
        assert!((suggestions[0].confidence - 0.5).abs() < 1e-6);
    }

    #[test]
    fn caps_the_suggestions_per_image() {
        let mut tags: Vec<_> = (0..6)
            .map(|index| tag(&format!("cat{index}"), 0.5))
            .collect();
        tags.extend((0..6).map(|index| tag(&format!("other{index}"), 0.5)));
        let suggestions = tagger(tags).suggest(&[1.0, 0.0]).expect("suggest");
        assert_eq!(suggestions.len(), MAX_SUGGESTIONS);
        assert!(suggestions
            .iter()
            .all(|suggestion| suggestion.tag_id.starts_with("cat")));
    }

    #[test]
    fn suggests_nothing_when_no_tag_stands_out() {
        let tagger = tagger(vec![tag("dog", 0.0), tag("car", 0.0), tag("tree", 0.0)]);
        assert!(tagger.suggest(&[1.0, 0.0]).expect("suggest").is_empty());
        let single = super::tests::tagger(vec![tag("cat", 0.0)]);
        assert!(single.suggest(&[1.0, 0.0]).expect("suggest").is_empty());
    }

    #[test]
    fn rejects_wrong_dimensions() {
        let tagger = tagger(vec![tag("cat", 1.0)]);
        assert!(matches!(
            tagger.suggest(&[1.0]),
            Err(EmbeddingError::InvalidDimensions { .. })
        ));
    }
}
