use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use picker_ml::{ClipJapanese, Embedder, Tagger, ZeroShotTagger};

fn image_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(directory) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(image_files(&path));
        } else if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("jpg" | "jpeg" | "png" | "webp")
        ) {
            files.push(path);
        }
    }
    files
}

/// Per-tag counts: true positives, images where the tag was predicted, and
/// false negatives.
type Stats = BTreeMap<String, (u32, u32, u32)>;

/// Adds one image whose true label is `expected`. Every predicted tag counts
/// toward its own "predicted" total, so a tag's precision is TP divided by
/// the images it was predicted for; TP and FN count toward the true label.
fn record(stats: &mut Stats, expected: &str, predicted: &BTreeSet<String>) {
    let truth = stats.entry(expected.to_owned()).or_insert((0, 0, 0));
    if predicted.contains(expected) {
        truth.0 += 1;
    } else {
        truth.2 += 1;
    }
    for tag in predicted {
        stats.entry(tag.clone()).or_insert((0, 0, 0)).1 += 1;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut directory = std::env::args().nth(1).map(PathBuf::from);
    let model = std::env::args().nth(2).map(PathBuf::from);
    if directory.is_none() || model.is_none() {
        directory = std::env::var_os("PICKER_EVAL_DIR").map(PathBuf::from);
    }
    let directory = directory.ok_or("usage: eval_zero_shot <labeled-dir> <model-dir>")?;
    let model = model
        .or_else(|| std::env::var_os("PICKER_CLIP_JAPANESE_DIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/home/agent/models/clip-japanese"));
    let embedder = ClipJapanese::open(&model, "clip-japanese", "77a62f8")?;
    let tagger = ZeroShotTagger::new(&embedder, ZeroShotTagger::vocabulary()?)?;

    let mut stats = Stats::new();
    let mut images = 0;
    let mut root_entries: Vec<_> = fs::read_dir(&directory)?.flatten().collect();
    root_entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in root_entries {
        if !entry.path().is_dir() {
            continue;
        }
        let label = entry.file_name().to_string_lossy().into_owned();
        for file in image_files(&entry.path()) {
            images += 1;
            let image = embedder.embed_image(&file)?;
            let predicted: BTreeSet<_> = tagger
                .suggest(&image)?
                .into_iter()
                .map(|suggestion| suggestion.tag_id)
                .collect();
            record(&mut stats, &label, &predicted);
        }
    }

    println!("| tag | tp | predicted | fn | precision | recall |");
    println!("|---|---:|---:|---:|---:|---:|");
    let (mut tp, mut predicted, mut false_negative) = (0, 0, 0);
    for (tag, (hit, count, missed)) in &stats {
        tp += hit;
        predicted += count;
        false_negative += missed;
        let precision = if *count == 0 {
            0.0
        } else {
            f64::from(*hit) / f64::from(*count)
        };
        let recall = if hit + missed == 0 {
            0.0
        } else {
            f64::from(*hit) / f64::from(hit + missed)
        };
        println!("| {tag} | {hit} | {count} | {missed} | {precision:.3} | {recall:.3} |");
    }
    let precision = if predicted == 0 {
        0.0
    } else {
        f64::from(tp) / f64::from(predicted)
    };
    let recall = if tp + false_negative == 0 {
        0.0
    } else {
        f64::from(tp) / f64::from(tp + false_negative)
    };
    println!("\nimages: {images}, micro precision: {precision:.3}, micro recall: {recall:.3}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn each_predicted_tag_counts_toward_its_own_precision() {
        let mut stats = Stats::new();
        record(&mut stats, "red", &tags(&["red", "blue"]));
        record(&mut stats, "blue", &tags(&[]));
        // red: one hit out of one prediction.
        assert_eq!(stats["red"], (1, 1, 0));
        // blue: predicted once (a false positive on the red image), missed once.
        assert_eq!(stats["blue"], (0, 1, 1));
    }

    #[test]
    fn tags_predicted_without_their_own_images_are_listed() {
        let mut stats = Stats::new();
        record(&mut stats, "sea", &tags(&["sky"]));
        assert_eq!(stats["sea"], (0, 0, 1));
        assert_eq!(stats["sky"], (0, 1, 0));
    }
}
