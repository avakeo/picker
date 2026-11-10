use std::collections::HashMap;

/// Nearest-neighbour index over normalized embedding vectors.
pub trait VectorIndex: Send + Sync {
    /// Inserts or replaces one vector.
    fn upsert(&mut self, asset_id: i64, vector: &[f32]) -> Result<(), VectorError>;
    /// Removes one vector and returns whether it existed.
    fn remove(&mut self, asset_id: i64) -> bool;
    /// Returns the highest-scoring neighbours, ties ordered by asset id.
    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(i64, f32)>, VectorError>;
    /// Number of indexed vectors.
    fn len(&self) -> usize;
    /// Returns whether the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Vector dimensionality.
    fn dimensions(&self) -> usize;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorError {
    DimensionMismatch { expected: usize, actual: usize },
    EmptyDimension,
}

impl std::fmt::Display for VectorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VectorError::DimensionMismatch { expected, actual } => {
                write!(
                    formatter,
                    "vector dimension mismatch: expected {expected}, got {actual}"
                )
            }
            VectorError::EmptyDimension => formatter.write_str("vector dimension must be positive"),
        }
    }
}

impl std::error::Error for VectorError {}

/// Exact cosine search using normalized vectors, matching ADR-003's full scan.
#[derive(Debug, Clone)]
pub struct BruteForceIndex {
    dimensions: usize,
    scores: HashMap<i64, Vec<f32>>,
}

impl BruteForceIndex {
    #[must_use]
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions,
            scores: HashMap::new(),
        }
    }

    pub fn from_records(
        dimensions: usize,
        records: impl IntoIterator<Item = (i64, Vec<f32>)>,
    ) -> Result<Self, VectorError> {
        let mut index = Self::new(dimensions);
        for (asset_id, vector) in records {
            index.upsert(asset_id, &vector)?;
        }
        Ok(index)
    }
}

impl VectorIndex for BruteForceIndex {
    fn upsert(&mut self, asset_id: i64, vector: &[f32]) -> Result<(), VectorError> {
        if self.dimensions == 0 {
            return Err(VectorError::EmptyDimension);
        }
        if vector.len() != self.dimensions {
            return Err(VectorError::DimensionMismatch {
                expected: self.dimensions,
                actual: vector.len(),
            });
        }
        if vector.iter().any(|value| !value.is_finite()) {
            return Err(VectorError::DimensionMismatch {
                expected: self.dimensions,
                actual: vector.len(),
            });
        }
        self.scores.insert(asset_id, vector.to_vec());
        Ok(())
    }

    fn remove(&mut self, asset_id: i64) -> bool {
        self.scores.remove(&asset_id).is_some()
    }

    fn search(&self, query: &[f32], limit: usize) -> Result<Vec<(i64, f32)>, VectorError> {
        if query.len() != self.dimensions {
            return Err(VectorError::DimensionMismatch {
                expected: self.dimensions,
                actual: query.len(),
            });
        }
        let query_norm = query.iter().map(|value| value * value).sum::<f32>().sqrt();
        if !query_norm.is_finite() || query_norm <= f32::EPSILON {
            return Ok(Vec::new());
        }
        let mut scored = self
            .scores
            .iter()
            .map(|(asset_id, vector)| {
                let dot = vector
                    .iter()
                    .zip(query)
                    .map(|(left, right)| left * right)
                    .sum::<f32>();
                let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
                let score = if norm > f32::EPSILON {
                    dot / (norm * query_norm)
                } else {
                    f32::NEG_INFINITY
                };
                (*asset_id, score)
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        scored.truncate(limit);
        Ok(scored)
    }

    fn len(&self) -> usize {
        self.scores.len()
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_nearest_neighbours_in_score_then_id_order() {
        let mut index = BruteForceIndex::new(3);
        index.upsert(10, &[1.0, 0.0, 0.0]).expect("insert");
        index.upsert(5, &[1.0, 0.0, 0.0]).expect("insert");
        index.upsert(7, &[0.0, 1.0, 0.0]).expect("insert");
        index.upsert(8, &[0.8, 0.2, 0.0]).expect("insert");
        assert_eq!(index.len(), 4);
        let results = index.search(&[1.0, 0.0, 0.0], 3).expect("search");
        assert_eq!(
            results.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![5, 10, 8]
        );
        assert!((results[0].1 - 1.0).abs() < 1e-6);
        assert_eq!(index.search(&[1.0, 0.0, 0.0], 10).expect("search").len(), 4);
    }

    #[test]
    fn update_remove_and_dimension_validation() {
        let mut index = BruteForceIndex::new(2);
        assert!(index.upsert(1, &[1.0]).is_err());
        assert!(index.upsert(1, &[f32::NAN, 0.0]).is_err());
        index.upsert(1, &[1.0, 0.0]).expect("insert");
        index.upsert(1, &[0.0, 1.0]).expect("update");
        assert_eq!(index.len(), 1);
        assert_eq!(index.search(&[1.0, 0.0], 1).expect("search")[0].0, 1);
        assert!((index.search(&[1.0, 0.0], 1).expect("search")[0].1).abs() < 1e-6);
        assert!(index.remove(1));
        assert!(!index.remove(1));
        assert!(index.is_empty());
        assert!(index.search(&[1.0], 1).is_err());
        assert!(index.search(&[0.0, 0.0], 1).expect("search").is_empty());
        assert!(BruteForceIndex::new(0).upsert(1, &[]).is_err());
    }

    #[test]
    #[ignore = "50k vector timing benchmark from T2-3"]
    fn fifty_thousand_vectors_meet_response_target() {
        let mut index = BruteForceIndex::new(512);
        let mut state = 0x1234_5678_9abc_def0_u64;
        for asset_id in 0..50_000 {
            let mut vector = Vec::with_capacity(512);
            let mut norm = 0.0_f32;
            for _ in 0..512 {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let value = (state >> 33) as f32 / (u32::MAX >> 1) as f32 - 1.0;
                norm += value * value;
                vector.push(value);
            }
            let norm = norm.sqrt();
            for value in &mut vector {
                *value /= norm;
            }
            index.upsert(asset_id, &vector).expect("insert");
        }
        let mut times = Vec::new();
        for _ in 0..20 {
            let mut query = Vec::with_capacity(512);
            let mut norm = 0.0_f32;
            for _ in 0..512 {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let value = (state >> 33) as f32 / (u32::MAX >> 1) as f32 - 1.0;
                norm += value * value;
                query.push(value);
            }
            let norm = norm.sqrt();
            for value in &mut query {
                *value /= norm;
            }
            let started = std::time::Instant::now();
            let results = index.search(&query, 10).expect("search");
            times.push(started.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(results.len(), 10);
        }
        times.sort_by(f64::total_cmp);
        let p95 = times[18];
        eprintln!("50k x 512 full scan p95: {p95:.2} ms");
        assert!(p95 < 200.0, "p95 {p95} ms");
    }
}
