use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{env, fs};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams, TruncationStrategy};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 && args.len() != 4 {
        bail!("usage: t0-3-tokenizer-parity TOKENIZER_JSON PARITY_JSON [OUTPUT_JSON]");
    }
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(&args[2]).with_context(|| format!("read {}", args[2]))?,
    )?;
    let texts: Vec<String> = serde_json::from_value(fixture["fixed_texts"].clone())?;
    let expected: Vec<Vec<u32>> = serde_json::from_value(fixture["python_token_ids"].clone())?;
    let sequence_length = expected.first().context("empty Python token fixture")?.len();

    let mut tokenizer = Tokenizer::from_file(&args[1]).map_err(anyhow::Error::msg)?;
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: sequence_length,
            strategy: TruncationStrategy::LongestFirst,
            ..Default::default()
        }))
        .map_err(anyhow::Error::msg)?;
    tokenizer.with_padding(Some(PaddingParams {
        strategy: PaddingStrategy::Fixed(sequence_length),
        pad_id: 1,
        pad_token: "<pad>".into(),
        ..Default::default()
    }));

    let encodings = tokenizer.encode_batch(texts, true).map_err(anyhow::Error::msg)?;
    let actual: Vec<Vec<u32>> = encodings.iter().map(|encoding| encoding.get_ids().to_vec()).collect();
    let passed = actual == expected;
    let report = serde_json::to_string_pretty(&json!({
        "passed": passed,
        "sequence_length": sequence_length,
        "expected": expected,
        "actual": actual
    }))?;
    println!("{report}");
    if let Some(path) = args.get(3) {
        fs::write(path, format!("{report}\n")).with_context(|| format!("write {path}"))?;
    }
    if !passed {
        bail!("Rust token IDs differ from the Python reference");
    }
    Ok(())
}
