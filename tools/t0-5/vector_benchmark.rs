use std::cmp::Ordering;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::process::Command;
use std::time::Instant;

#[derive(Clone, Copy)]
struct Rng(u64);

impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 7;
        self.0 ^= self.0 >> 9;
        self.0 ^= self.0 << 8;
        (self.0 as u32) as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

#[derive(Clone, Copy)]
enum Precision {
    F32,
    F16,
}

impl Precision {
    fn name(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::F16 => "f16",
        }
    }
}

struct Options {
    output: String,
    iterations: usize,
    warmups: usize,
    top_k: usize,
    dimensions: Vec<usize>,
    sizes: Vec<usize>,
}

fn parse_list(value: &str) -> Result<Vec<usize>, String> {
    value
        .split(',')
        .map(|part| part.parse::<usize>().map_err(|_| format!("invalid number: {part}")))
        .collect()
}

fn option_value(args: &[String], name: &str, default: &str) -> Result<String, String> {
    match args.iter().position(|arg| arg == name) {
        Some(index) if index + 1 < args.len() => Ok(args[index + 1].clone()),
        Some(_) => Err(format!("missing value for {name}")),
        None => Ok(default.to_owned()),
    }
}

fn parse_options() -> Result<Options, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let dimensions = parse_list(&option_value(&args, "--dimensions", "512,768,1152")?)?;
    let sizes = parse_list(&option_value(&args, "--sizes", "10000,50000,100000")?)?;
    let options = Options {
        output: option_value(&args, "--output", "vector-benchmark.csv")?,
        iterations: option_value(&args, "--iterations", "3")?
            .parse()
            .map_err(|_| "invalid --iterations".to_owned())?,
        warmups: option_value(&args, "--warmups", "1")?
            .parse()
            .map_err(|_| "invalid --warmups".to_owned())?,
        top_k: option_value(&args, "--top-k", "10")?
            .parse()
            .map_err(|_| "invalid --top-k".to_owned())?,
        dimensions,
        sizes,
    };
    if options.iterations == 0 || options.top_k == 0 || options.dimensions.is_empty() || options.sizes.is_empty() {
        return Err("iterations, top-k, dimensions, and sizes must be non-empty".to_owned());
    }
    Ok(options)
}

fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        let shifted = (mantissa | 0x80_0000) >> (1 - exponent);
        return sign | ((shifted + 0x1000) >> 13) as u16;
    }
    if exponent >= 31 {
        return sign | 0x7c00 | if mantissa == 0 { 0 } else { 0x0200 };
    }
    sign | ((exponent as u16) << 10) | (((mantissa + 0x1000) >> 13) as u16 & 0x03ff)
}

fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = (bits & 0x03ff) as u32;
    let value = if exponent == 0 {
        if mantissa == 0 {
            sign
        } else {
            let mut mantissa = mantissa;
            let mut exponent = -14i32;
            while mantissa & 0x0400 == 0 {
                mantissa <<= 1;
                exponent -= 1;
            }
            let exponent = (exponent + 127) as u32;
            sign | (exponent << 23) | ((mantissa & 0x03ff) << 13)
        }
    } else if exponent == 0x1f {
        sign | 0x7f80_0000 | (mantissa << 13)
    } else {
        let exponent = (exponent as u32) + 112;
        sign | (exponent << 23) | (mantissa << 13)
    };
    f32::from_bits(value)
}

fn unit_vector(rng: &mut Rng, dimensions: usize) -> Vec<f32> {
    let mut vector: Vec<f32> = (0..dimensions).map(|_| rng.next_f32()).collect();
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    for value in &mut vector {
        *value /= norm;
    }
    vector
}

fn generate_vectors(count: usize, dimensions: usize, rng: &mut Rng) -> Vec<f32> {
    let mut vectors = Vec::with_capacity(count * dimensions);
    for _ in 0..count {
        vectors.extend(unit_vector(rng, dimensions));
    }
    vectors
}

fn encode_f16(vectors: &[f32]) -> Vec<u16> {
    vectors.iter().copied().map(f32_to_f16).collect()
}

fn dot_f32(vector: &[f32], query: &[f32]) -> f32 {
    vector.iter().zip(query).map(|(left, right)| left * right).sum()
}

fn dot_f16(vector: &[u16], query: &[u16]) -> f32 {
    vector
        .iter()
        .zip(query)
        .map(|(left, right)| f16_to_f32(*left) * f16_to_f32(*right))
        .sum()
}

fn scan_f32(vectors: &[f32], query: &[f32], count: usize, dimensions: usize) -> Vec<f32> {
    (0..count)
        .map(|index| dot_f32(&vectors[index * dimensions..(index + 1) * dimensions], query))
        .collect()
}

fn scan_f16(vectors: &[u16], query: &[u16], count: usize, dimensions: usize) -> Vec<f32> {
    (0..count)
        .map(|index| dot_f16(&vectors[index * dimensions..(index + 1) * dimensions], query))
        .collect()
}

fn top_k(scores: &[f32], k: usize) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..scores.len()).collect();
    indices.sort_unstable_by(|left, right| {
        scores[*right]
            .partial_cmp(&scores[*left])
            .unwrap_or(Ordering::Equal)
    });
    indices.truncate(k.min(indices.len()));
    indices
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let index = ((sorted.len() - 1) as f64 * percentile) as usize;
    sorted[index]
}

fn resident_bytes() -> Option<u64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    text.trim().parse::<u64>().ok().map(|kilobytes| kilobytes * 1024)
}

fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

fn write_row(file: &mut File, row: &[String]) -> io::Result<()> {
    let line = row.iter().map(|value| csv_escape(value)).collect::<Vec<_>>().join(",");
    writeln!(file, "{line}")
}

fn run_case(
    file: &mut File,
    dimensions: usize,
    count: usize,
    precision: Precision,
    vectors: &[f32],
    f16_vectors: &[u16],
    query: &[f32],
    f16_query: &[u16],
    options: &Options,
    baseline_top_k: &[usize],
) -> io::Result<()> {
    let mut durations = Vec::with_capacity(options.iterations);
    let mut last_scores = Vec::new();
    let mut peak_resident = resident_bytes();
    for _ in 0..options.warmups {
        match precision {
            Precision::F32 => {
                let _ = scan_f32(vectors, query, count, dimensions);
            }
            Precision::F16 => {
                let _ = scan_f16(f16_vectors, f16_query, count, dimensions);
            }
        }
    }
    for _ in 0..options.iterations {
        let start = Instant::now();
        last_scores = match precision {
            Precision::F32 => scan_f32(vectors, query, count, dimensions),
            Precision::F16 => scan_f16(f16_vectors, f16_query, count, dimensions),
        };
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
        if let Some(current) = resident_bytes() {
            peak_resident = Some(peak_resident.unwrap_or(current).max(current));
        }
    }
    let current_top_k = top_k(&last_scores, options.top_k);
    let recall = if baseline_top_k.is_empty() {
        1.0
    } else {
        current_top_k.iter().filter(|index| baseline_top_k.contains(index)).count() as f64
            / baseline_top_k.len() as f64
    };
    let storage_bytes = match precision {
        Precision::F32 => vectors.len() * std::mem::size_of::<f32>(),
        Precision::F16 => f16_vectors.len() * std::mem::size_of::<u16>(),
    };
    write_row(
        file,
        &[
            "self-full-scan".to_owned(),
            dimensions.to_string(),
            count.to_string(),
            precision.name().to_owned(),
            format!("{:.3}", percentile(&durations, 0.50)),
            format!("{:.3}", percentile(&durations, 0.95)),
            format!("{recall:.4}"),
            storage_bytes.to_string(),
            peak_resident.map_or_else(|| "".to_owned(), |value| value.to_string()),
            "measured".to_owned(),
        ],
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_options().map_err(io::Error::other)?;
    let mut file = File::create(&options.output)?;
    write_row(
        &mut file,
        &[
            "method".to_owned(),
            "dimensions".to_owned(),
            "count".to_owned(),
            "precision".to_owned(),
            "p50_ms".to_owned(),
            "p95_ms".to_owned(),
            "top10_recall_vs_f32".to_owned(),
            "vector_storage_bytes".to_owned(),
            "resident_bytes".to_owned(),
            "status".to_owned(),
        ],
    )?;

    for &dimensions in &options.dimensions {
        for &count in &options.sizes {
            eprintln!("benchmarking dimensions={dimensions} count={count}");
            let mut rng = Rng(0x5049_434b_4552_0001 ^ dimensions as u64 ^ count as u64);
            let vectors = generate_vectors(count, dimensions, &mut rng);
            let query = unit_vector(&mut rng, dimensions);
            let f16_vectors = encode_f16(&vectors);
            let f16_query = encode_f16(&query);
            let baseline_scores = scan_f32(&vectors, &query, count, dimensions);
            let baseline_top_k = top_k(&baseline_scores, options.top_k);
            run_case(
                &mut file,
                dimensions,
                count,
                Precision::F32,
                &vectors,
                &f16_vectors,
                &query,
                &f16_query,
                &options,
                &baseline_top_k,
            )?;
            run_case(
                &mut file,
                dimensions,
                count,
                Precision::F16,
                &vectors,
                &f16_vectors,
                &query,
                &f16_query,
                &options,
                &baseline_top_k,
            )?;
        }
    }
    eprintln!("wrote {}", options.output);
    Ok(())
}
