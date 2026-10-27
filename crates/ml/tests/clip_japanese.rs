use std::path::{Path, PathBuf};

use picker_ml::{ClipJapanese, Embedder};
use serde_json::Value;

fn model_dir() -> PathBuf {
    std::env::var_os("PICKER_CLIP_JAPANESE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/agent/models/clip-japanese"))
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[test]
#[ignore = "requires the downloaded clip-japanese ONNX model"]
fn matches_reference_vectors_and_is_reproducible() {
    let root = model_dir();
    if !root.join("clyp_visual.onnx").exists() {
        eprintln!("skipping: no model at {}", root.display());
        return;
    }
    let golden: Value =
        serde_json::from_str(include_str!("golden/clip-japanese.json")).expect("golden json");
    let embedder = ClipJapanese::open(&root, "clip-japanese@77a62f8").expect("load model");
    assert_eq!(embedder.dimensions(), 512);

    for (name, expected) in golden["images"].as_object().expect("images") {
        let vector = embedder
            .embed_image(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(name),
            )
            .expect("image embedding");
        let again = embedder
            .embed_image(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/fixtures")
                    .join(name),
            )
            .expect("image embedding");
        assert_eq!(vector.len(), 512);
        assert!((cosine(&vector, &again) - 1.0).abs() < 1e-6);
        let expected = expected["vector"]
            .as_array()
            .expect("vector")
            .iter()
            .map(|value| value.as_f64().expect("number") as f32)
            .collect::<Vec<_>>();
        let score = cosine(&vector, &expected);
        eprintln!("{name} cosine {score}");
        eprintln!("{name} cosine {score}");
        assert!(score > 0.999, "{name} cosine {score}");
    }

    for (text, expected) in golden["texts"].as_object().expect("texts") {
        let vector = embedder.embed_text(text).expect("text embedding");
        let expected = expected["vector"]
            .as_array()
            .expect("vector")
            .iter()
            .map(|value| value.as_f64().expect("number") as f32)
            .collect::<Vec<_>>();
        let score = cosine(&vector, &expected);
        eprintln!("{text} cosine {score}");
        assert!(score > 0.999, "{text} cosine {score}");
    }

    assert!(embedder.embed_text("   ").is_err());
    assert!(embedder
        .embed_image(&root.join("does-not-exist.png"))
        .is_err());
}
