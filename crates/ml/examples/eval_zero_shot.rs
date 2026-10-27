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

    let mut stats: BTreeMap<String, (u32, u32, u32)> = BTreeMap::new();
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
            let expected = label.clone();
            let entry = stats.entry(expected.clone()).or_insert((0, 0, 0));
            if predicted.contains(&expected) {
                entry.0 += 1;
            } else {
                entry.2 += 1;
            }
            entry.1 += predicted.len() as u32;
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
