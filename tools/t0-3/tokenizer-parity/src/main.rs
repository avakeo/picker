use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{env, fs};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams, TruncationStrategy};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if !(3..=5).contains(&args.len()) {
        bail!("usage: t0-3-tokenizer-parity TOKENIZER_JSON PARITY_JSON [OUTPUT_JSON] [siglip|siglip2|line]");
    }
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(&args[2]).with_context(|| format!("read {}", args[2]))?,
    )?;
    let texts: Vec<String> = serde_json::from_value(fixture["fixed_texts"].clone())?;
    let expected: Vec<Vec<u32>> = serde_json::from_value(fixture["python_token_ids"].clone())?;
    let sequence_length = expected.first().context("empty Python token fixture")?.len();

    let mode = args.get(4).map(String::as_str).unwrap_or("siglip");
    let mut tokenizer = Tokenizer::from_file(&args[1]).map_err(anyhow::Error::msg)?;
    let actual: Vec<Vec<u32>> = if mode == "line" {
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: sequence_length - 1,
                strategy: TruncationStrategy::LongestFirst,
                ..Default::default()
            }))
            .map_err(anyhow::Error::msg)?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(sequence_length - 1),
            pad_id: 3,
            pad_token: "[PAD]".into(),
            ..Default::default()
        }));
        tokenizer
            .encode_batch(texts, false)
            .map_err(anyhow::Error::msg)?
            .iter()
            .map(|encoding| {
                let mut ids = Vec::with_capacity(sequence_length);
                ids.push(4);
                ids.extend_from_slice(encoding.get_ids());
                ids
            })
            .collect()
    } else if mode == "siglip" || mode == "siglip2" {
        let pad_id = if mode == "siglip2" { 0 } else { 1 };
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: sequence_length,
                strategy: TruncationStrategy::LongestFirst,
                ..Default::default()
            }))
            .map_err(anyhow::Error::msg)?;
        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(sequence_length),
            pad_id,
            pad_token: "<pad>".into(),
            ..Default::default()
        }));
        tokenizer
            .encode_batch(texts, true)
            .map_err(anyhow::Error::msg)?
            .iter()
            .map(|encoding| encoding.get_ids().to_vec())
            .collect()
    } else {
        bail!("unknown tokenizer mode: {mode}");
    };
    let passed = actual == expected;
    let report = serde_json::to_string_pretty(&json!({
        "passed": passed,
        "mode": mode,
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
