use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ort::session::Session;
use picker_imaging::{DecodeLimits, Decoder, ImageFormat, PlatformDecoder};
use sentencepiece_rs::SentencePieceProcessor;

const IMAGE_SIZE: u32 = 224;
const MAX_TOKENS: usize = 77;
const CLS_TOKEN_ID: i64 = 4;
const PAD_TOKEN_ID: i64 = 3;

/// Text or image embedding boundary shared by search and tagging.
pub trait Embedder: Send + Sync {
    /// Embeds one image as a unit vector.
    fn embed_image(&self, path: &Path) -> Result<Vec<f32>, EmbeddingError>;
    /// Embeds one text as a unit vector.
    fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    /// Dimensionality of the returned vectors.
    fn dimensions(&self) -> usize;
    /// Stable model identifier.
    fn model_id(&self) -> &str;
    /// Stable model version.
    fn model_version(&self) -> &str;
}

#[derive(Debug)]
pub enum EmbeddingError {
    Io(String),
    Image(String),
    Tokenizer(String),
    Inference(String),
    EmptyInput,
    InvalidDimensions { expected: usize, actual: usize },
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmbeddingError::Io(message) => write!(formatter, "embedding I/O failed: {message}"),
            EmbeddingError::Image(message) => {
                write!(formatter, "image preprocessing failed: {message}")
            }
            EmbeddingError::Tokenizer(message) => write!(formatter, "tokenizer failed: {message}"),
            EmbeddingError::Inference(message) => write!(formatter, "inference failed: {message}"),
            EmbeddingError::EmptyInput => write!(formatter, "embedding input is empty"),
            EmbeddingError::InvalidDimensions { expected, actual } => {
                write!(
                    formatter,
                    "embedding dimension mismatch: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl std::error::Error for EmbeddingError {}

/// ONNX implementation of the clip-japanese image/text embedder.
pub struct ClipJapanese {
    visual: Mutex<Session>,
    textual: Mutex<Session>,
    tokenizer: SentencePieceProcessor,
    dimensions: usize,
    model_id: String,
    model_version: String,
}

impl fmt::Debug for ClipJapanese {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClipJapanese")
            .field("dimensions", &self.dimensions)
            .field("model_id", &self.model_id)
            .field("model_version", &self.model_version)
            .finish_non_exhaustive()
    }
}

impl ClipJapanese {
    /// Loads the visual ONNX, textual ONNX, and SentencePiece model from a directory.
    pub fn open(
        directory: impl AsRef<Path>,
        model_id: impl Into<String>,
        model_version: impl Into<String>,
    ) -> Result<Self, EmbeddingError> {
        let directory = directory.as_ref();
        let visual = load_session(&directory.join("clyp_visual.onnx"))?;
        let textual = load_session(&directory.join("clyp_textual.onnx"))?;
        let tokenizer = SentencePieceProcessor::open(directory.join("spiece.model"))
            .map_err(|error| EmbeddingError::Tokenizer(error.to_string()))?;
        Ok(Self {
            visual: Mutex::new(visual),
            textual: Mutex::new(textual),
            tokenizer,
            dimensions: 512,
            model_id: model_id.into(),
            model_version: model_version.into(),
        })
    }

    /// Loads from file paths previously returned by the model registry.
    pub fn from_paths(
        files: &[(String, PathBuf)],
        model_id: impl Into<String>,
        model_version: impl Into<String>,
    ) -> Result<Self, EmbeddingError> {
        let find = |name: &str| {
            files
                .iter()
                .find(|(file, _)| file == name)
                .map(|(_, path)| path.clone())
                .ok_or_else(|| EmbeddingError::Io(format!("missing model file {name}")))
        };
        let visual = load_session(&find("clyp_visual.onnx")?)?;
        let textual = load_session(&find("clyp_textual.onnx")?)?;
        let tokenizer = SentencePieceProcessor::open(find("spiece.model")?)
            .map_err(|error| EmbeddingError::Tokenizer(error.to_string()))?;
        Ok(Self {
            visual: Mutex::new(visual),
            textual: Mutex::new(textual),
            tokenizer,
            dimensions: 512,
            model_id: model_id.into(),
            model_version: model_version.into(),
        })
    }

    fn token_ids(&self, text: &str) -> Result<Vec<i64>, EmbeddingError> {
        let lowered = text.trim().to_lowercase();
        if lowered.is_empty() {
            return Err(EmbeddingError::EmptyInput);
        }
        let ids = self
            .tokenizer
            .encode_to_ids(&lowered)
            .map_err(|error| EmbeddingError::Tokenizer(error.to_string()))?;
        let mut tokens = Vec::with_capacity(MAX_TOKENS);
        tokens.push(CLS_TOKEN_ID);
        tokens.extend(ids.into_iter().take(MAX_TOKENS - 1).map(|id| id as i64));
        tokens.resize(MAX_TOKENS, PAD_TOKEN_ID);
        Ok(tokens)
    }
}

impl Embedder for ClipJapanese {
    fn embed_image(&self, path: &Path) -> Result<Vec<f32>, EmbeddingError> {
        let format = ImageFormat::from_path(path)
            .ok_or_else(|| EmbeddingError::Image("unsupported image format".to_owned()))?;
        let decoded = PlatformDecoder
            .decode_file(path, format, &DecodeLimits::default())
            .map_err(|error| EmbeddingError::Image(error.to_string()))?;
        let input = preprocess_image(decoded.width, decoded.height, &decoded.rgba);
        let tensor = ort::value::Tensor::from_array(([1_i64, 3, 224, 224], input))
            .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        let mut session = self
            .visual
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let outputs = session
            .run(ort::inputs!["input" => tensor])
            .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        let (_, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        normalize(data, self.dimensions)
    }

    fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let tokens = self.token_ids(text)?;
        let attention = tokens
            .iter()
            .map(|id| i64::from(*id != PAD_TOKEN_ID))
            .collect::<Vec<_>>();
        let positions = (0..MAX_TOKENS as i64).collect::<Vec<_>>();
        let input_ids = ort::value::Tensor::from_array(([1_i64, MAX_TOKENS as i64], tokens))
            .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        let attention_mask =
            ort::value::Tensor::from_array(([1_i64, MAX_TOKENS as i64], attention))
                .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        let position_ids = ort::value::Tensor::from_array(([1_i64, MAX_TOKENS as i64], positions))
            .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        let mut session = self
            .textual
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let outputs = session
            .run(ort::inputs![
                "input0" => input_ids,
                "input1" => attention_mask,
                "input2" => position_ids
            ])
            .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        let (_, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
        normalize(data, self.dimensions)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_version(&self) -> &str {
        &self.model_version
    }
}

fn environment() -> Result<ort::environment::Environment, EmbeddingError> {
    static ENVIRONMENT: std::sync::OnceLock<ort::environment::Environment> =
        std::sync::OnceLock::new();
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment.clone());
    }
    #[cfg(not(target_os = "macos"))]
    let builder = match std::env::var("PICKER_ORT_DYLIB") {
        Ok(path) if !path.is_empty() => {
            ort::init_from(path).map_err(|error| EmbeddingError::Inference(error.to_string()))?
        }
        _ => ort::init(),
    };
    #[cfg(target_os = "macos")]
    let builder = ort::init();
    let environment = builder
        .with_name("picker")
        .build()
        .map_err(|error| EmbeddingError::Inference(error.to_string()))?;
    let _ = ENVIRONMENT.set(environment.clone());
    Ok(ENVIRONMENT.get().cloned().unwrap_or(environment))
}

fn load_session(path: &Path) -> Result<Session, EmbeddingError> {
    let env = environment()?;
    #[cfg(target_os = "macos")]
    {
        // Core ML が使えない環境では CPU 実行へ切り替えるため、失敗は捨てる
        let coreml = [ort::ep::CoreML::default().build()];
        let session = Session::builder(&env)
            .ok()
            .and_then(|builder| builder.with_execution_providers(coreml).ok())
            .and_then(|mut builder| builder.commit_from_file(path).ok());
        if let Some(session) = session {
            return Ok(session);
        }
    }
    Session::builder(&env)
        .and_then(|mut builder| builder.commit_from_file(path))
        .map_err(|error| EmbeddingError::Inference(error.to_string()))
}

fn normalize(data: &[f32], expected: usize) -> Result<Vec<f32>, EmbeddingError> {
    if data.len() != expected {
        return Err(EmbeddingError::InvalidDimensions {
            expected,
            actual: data.len(),
        });
    }
    let norm = data.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return Err(EmbeddingError::Inference(
            "model returned a zero vector".to_owned(),
        ));
    }
    Ok(data.iter().map(|value| value / norm).collect())
}

fn preprocess_image(width: u32, height: u32, rgba: &[u8]) -> Vec<f32> {
    let scale = f64::from(IMAGE_SIZE) / f64::from(height.max(width));
    let (new_width, new_height) = if (scale - 1.0).abs() < f64::EPSILON {
        (width, height)
    } else {
        (
            ((f64::from(width) * scale).round() as u32).max(1),
            ((f64::from(height) * scale).round() as u32).max(1),
        )
    };
    let resized = if (new_width, new_height) == (width, height) {
        rgba.to_vec()
    } else {
        let source = image::RgbaImage::from_raw(width, height, rgba.to_vec())
            .expect("decoded image has width * height * 4 bytes");
        image::imageops::resize(
            &source,
            new_width,
            new_height,
            image::imageops::FilterType::CatmullRom,
        )
        .into_raw()
    };
    let pad_x = (IMAGE_SIZE - new_width) / 2;
    let pad_y = (IMAGE_SIZE - new_height) / 2;
    let mean = [0.485_f32, 0.456, 0.406];
    let std = [0.229_f32, 0.224, 0.225];
    let plane = (IMAGE_SIZE * IMAGE_SIZE) as usize;
    let mut output = Vec::with_capacity(3 * plane);
    for channel in 0..3 {
        output.extend(std::iter::repeat_n(-mean[channel] / std[channel], plane));
    }
    for y in 0..new_height {
        for x in 0..new_width {
            let source = ((y * new_width + x) * 4) as usize;
            let target = (y + pad_y) as usize * IMAGE_SIZE as usize + (x + pad_x) as usize;
            for channel in 0..3 {
                let value = f32::from(resized[source + channel]) / 255.0;
                output[channel * plane + target] = (value - mean[channel]) / std[channel];
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use picker_imaging::{DecodeLimits, Decoder, ImageFormat, PlatformDecoder};

    #[test]
    fn preprocesses_into_chw_with_padding() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gradient.png");
        let decoded = PlatformDecoder
            .decode_file(&path, ImageFormat::Png, &DecodeLimits::default())
            .expect("decode");
        let input = preprocess_image(decoded.width, decoded.height, &decoded.rgba);
        assert_eq!(input.len(), 3 * 224 * 224);
        let padding = -0.485_f32 / 0.229;
        assert!((input[0] - padding).abs() < 1e-6);
    }
}
