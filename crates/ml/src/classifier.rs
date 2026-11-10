/// L2-regularized binary logistic regression trained with full-batch gradient descent.
#[derive(Debug, Clone, PartialEq)]
pub struct LogisticRegression {
    weights: Vec<f64>,
    bias: f64,
}

impl LogisticRegression {
    #[must_use]
    pub fn new(dimensions: usize) -> Self {
        Self {
            weights: vec![0.0; dimensions],
            bias: 0.0,
        }
    }

    #[must_use]
    pub fn from_parts(weights: Vec<f64>, bias: f64) -> Self {
        Self { weights, bias }
    }

    #[must_use]
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    #[must_use]
    pub fn bias(&self) -> f64 {
        self.bias
    }

    /// Fits the model. `l2` is the regularization strength and `epochs` the
    /// number of full-batch steps.
    pub fn fit(&mut self, samples: &[(Vec<f32>, bool)], l2: f64, learning_rate: f64, epochs: u32) {
        if samples.is_empty() {
            return;
        }
        let count = samples.len() as f64;
        for _ in 0..epochs {
            let mut gradient = vec![0.0_f64; self.weights.len()];
            let mut bias_gradient = 0.0_f64;
            for (features, label) in samples {
                let probability = self.predict(features);
                let error = probability - f64::from(u8::from(*label));
                for (index, value) in features.iter().enumerate() {
                    gradient[index] += error * f64::from(*value);
                }
                bias_gradient += error;
            }
            for (weight, gradient) in self.weights.iter_mut().zip(&gradient) {
                *weight -= learning_rate * (gradient / count + l2 * *weight);
            }
            self.bias -= learning_rate * bias_gradient / count;
        }
    }

    /// Returns the probability of the positive class.
    #[must_use]
    pub fn predict(&self, features: &[f32]) -> f64 {
        if features.len() != self.weights.len() {
            return 0.5;
        }
        let logit = self
            .weights
            .iter()
            .zip(features)
            .map(|(weight, value)| weight * f64::from(*value))
            .sum::<f64>()
            + self.bias;
        1.0 / (1.0 + (-logit.clamp(-60.0, 60.0)).exp())
    }
}

/// Picks review candidates by mixing borderline and high-confidence scores.
///
/// `total` candidates are chosen deterministically. Approximately
/// `high_ratio` of them come from the highest scores; the rest are the
/// closest to the decision boundary.
#[must_use]
pub fn select_review_candidates(
    scored: &[(i64, f64)],
    total: usize,
    high_ratio: f64,
) -> Vec<(i64, f64)> {
    if total == 0 || scored.is_empty() {
        return Vec::new();
    }
    let high_count = ((total as f64 * high_ratio.clamp(0.0, 1.0)).round() as usize).min(total);
    let mut by_score = scored.to_vec();
    by_score.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    let mut selected: Vec<(i64, f64)> = by_score.iter().take(high_count).copied().collect();
    let mut near_boundary = scored.to_vec();
    near_boundary.sort_by(|left, right| {
        (left.1 - 0.5)
            .abs()
            .total_cmp(&(right.1 - 0.5).abs())
            .then_with(|| left.0.cmp(&right.0))
    });
    for candidate in near_boundary {
        if selected.len() == total {
            break;
        }
        if !selected.iter().any(|(id, _)| *id == candidate.0) {
            selected.push(candidate);
        }
    }
    if selected.len() < total {
        for candidate in by_score {
            if selected.len() == total {
                break;
            }
            if !selected.iter().any(|(id, _)| *id == candidate.0) {
                selected.push(candidate);
            }
        }
    }
    selected.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_a_known_linear_boundary() {
        let mut model = LogisticRegression::new(2);
        let mut samples = Vec::new();
        for x in -20..=20 {
            for y in -20..=20 {
                let value = (x as f32 / 10.0, y as f32 / 10.0);
                samples.push((vec![value.0, value.1], value.0 + value.1 > 0.0));
                samples.push((vec![-value.0, -value.1], -value.0 - value.1 > 0.0));
            }
        }
        model.fit(&samples, 0.001, 0.5, 400);
        assert!(model.predict(&[1.0, 1.0]) > 0.9);
        assert!(model.predict(&[-1.0, -1.0]) < 0.1);
        assert!((model.predict(&[0.0, 0.0]) - 0.5).abs() < 0.1);
    }

    #[test]
    fn review_selection_mixes_high_and_borderline_candidates() {
        let scored: Vec<(i64, f64)> = (0..100)
            .map(|id| (id, if id == 0 { 0.5 } else { id as f64 / 100.0 }))
            .collect();
        let selected = select_review_candidates(&scored, 10, 0.5);
        assert_eq!(selected.len(), 10);
        assert!(selected.iter().any(|(id, _)| *id == 0));
        assert!(selected.iter().any(|(id, _)| *id == 99));
        let unique: std::collections::BTreeSet<_> = selected.iter().map(|(id, _)| *id).collect();
        assert_eq!(unique.len(), 10);
    }

    #[test]
    fn handles_empty_and_mismatched_inputs() {
        assert!(select_review_candidates(&[], 10, 0.3).is_empty());
        assert!(select_review_candidates(&[(1, 0.9)], 0, 0.3).is_empty());
        let model = LogisticRegression::new(2);
        assert!((model.predict(&[1.0]) - 0.5).abs() < f64::EPSILON);
    }
}

#[cfg(test)]
mod benchmark {
    use super::LogisticRegression;

    #[test]
    #[ignore = "50k x 512 training benchmark from T3-3"]
    fn fifty_thousand_samples_retrain_and_score_within_target() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut samples = Vec::with_capacity(50_000);
        for _ in 0..50_000 {
            let mut vector = Vec::with_capacity(512);
            let mut norm = 0.0_f32;
            for _ in 0..512 {
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                let unit = (state.wrapping_mul(2_685_821_657_736_338_717) >> 11) as f64
                    / (1_u64 << 53) as f64;
                let value = (unit * 2.0 - 1.0) as f32;
                norm += value * value;
                vector.push(value);
            }
            let norm = norm.sqrt();
            for value in &mut vector {
                *value /= norm;
            }
            let label = vector[0] > 0.0;
            samples.push((vector, label));
        }
        let started = std::time::Instant::now();
        let mut model = LogisticRegression::new(512);
        model.fit(&samples, 0.001, 5.0, 20);
        let mut positive = 0;
        for (features, _) in &samples {
            if model.predict(features) >= 0.5 {
                positive += 1;
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        let true_count = samples.iter().filter(|(_, label)| *label).count();
        eprintln!(
            "50k x 512 train+score: {elapsed:.3}s, positive {positive}, true {true_count}, bias {:.4}, w0 {:.4}, w1 {:.4}",
            model.bias(), model.weights()[0], model.weights()[1]
        );
        assert!(elapsed < 1.0, "training took {elapsed}s");
    }
}
