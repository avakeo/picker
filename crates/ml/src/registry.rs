use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const CHUNK_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelVersion {
    pub id: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    pub files: Vec<ModelFile>,
}

impl ModelVersion {
    fn total_size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

pub fn catalog() -> Vec<ModelVersion> {
    vec![ModelVersion {
        id: "clip-japanese".to_owned(),
        version: "77a62f8".to_owned(),
        license: "Apache-2.0".to_owned(),
        license_url: "https://huggingface.co/line-corporation/clip-japanese-base/blob/77a62f8af977acd73ec2f927fb73fdaeb13af7d1/LICENSE".to_owned(),
        files: vec![
            ModelFile {
                name: "clyp_textual.onnx".to_owned(),
                url: "https://huggingface.co/line-corporation/clip-japanese-base/resolve/77a62f8af977acd73ec2f927fb73fdaeb13af7d1/onnx/clyp_textual.onnx".to_owned(),
                sha256: "8104b764c88099e5395e93300d52b17bdf34a4f43a6fd681eb83a34aa4db31f8".to_owned(),
                size: 441_988_972,
            },
            ModelFile {
                name: "clyp_visual.onnx".to_owned(),
                url: "https://huggingface.co/line-corporation/clip-japanese-base/resolve/77a62f8af977acd73ec2f927fb73fdaeb13af7d1/onnx/clyp_visual.onnx".to_owned(),
                sha256: "a91c5686cf47ff91c09736a8ce0f8a518001f52f63cb5ad123a0f6183e3e4ccf".to_owned(),
                size: 347_813_462,
            },
            ModelFile {
                name: "spiece.model".to_owned(),
                url: "https://huggingface.co/line-corporation/clip-japanese-base/resolve/77a62f8af977acd73ec2f927fb73fdaeb13af7d1/onnx/spiece.model".to_owned(),
                sha256: "b5cbdfa8aa7c54c8c5af85b78c309c54a5f2749a20468bf6f60eee007fe6fec1".to_owned(),
                size: 805_634,
            },
        ],
    }]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelState {
    Missing,
    Partial,
    Ready,
}

impl ModelState {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelState::Missing => "missing",
            ModelState::Partial => "partial",
            ModelState::Ready => "ready",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub version: String,
    pub license: String,
    pub license_url: String,
    pub size: u64,
    pub state: ModelState,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub model_id: String,
    pub version: String,
    pub file: String,
    pub file_index: usize,
    pub file_count: usize,
    pub file_downloaded: u64,
    pub file_total: u64,
    pub total_downloaded: u64,
    pub total_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    Io(String),
    Http(String),
    InsecureUrl(String),
    UnknownModel(String),
    NotInstalled(String),
    SizeMismatch { expected: u64, actual: u64 },
    HashMismatch { expected: String, actual: String },
    InvalidName(String),
}

impl fmt::Display for ModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::Io(message) => write!(formatter, "model I/O failed: {message}"),
            ModelError::Http(message) => write!(formatter, "model download failed: {message}"),
            ModelError::InsecureUrl(url) => write!(formatter, "model URL is not HTTPS: {url}"),
            ModelError::UnknownModel(model) => write!(formatter, "unknown model: {model}"),
            ModelError::NotInstalled(model) => write!(formatter, "model is not installed: {model}"),
            ModelError::SizeMismatch { expected, actual } => {
                write!(
                    formatter,
                    "model size mismatch: expected {expected}, got {actual}"
                )
            }
            ModelError::HashMismatch { expected, actual } => {
                write!(
                    formatter,
                    "model hash mismatch: expected {expected}, got {actual}"
                )
            }
            ModelError::InvalidName(name) => {
                write!(formatter, "invalid model path component: {name}")
            }
        }
    }
}

impl std::error::Error for ModelError {}

impl From<io::Error> for ModelError {
    fn from(error: io::Error) -> Self {
        ModelError::Io(error.to_string())
    }
}

pub trait ModelDownloader: Send + Sync {
    fn download(
        &self,
        url: &str,
        part: &Path,
        offset: u64,
        on_chunk: &mut dyn FnMut(u64),
    ) -> Result<(), ModelError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HttpDownloader;

impl ModelDownloader for HttpDownloader {
    fn download(
        &self,
        url: &str,
        part: &Path,
        offset: u64,
        on_chunk: &mut dyn FnMut(u64),
    ) -> Result<(), ModelError> {
        if !url.starts_with("https://") {
            return Err(ModelError::InsecureUrl(url.to_owned()));
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(30))
            .build();
        let mut request = agent.get(url);
        if offset > 0 {
            request = request.set("Range", &format!("bytes={offset}-"));
        }
        let response = request
            .call()
            .map_err(|error| ModelError::Http(error.to_string()))?;
        let status = response.status();
        let mut reader = response.into_reader();
        let mut file = if offset > 0 && status == 206 {
            OpenOptions::new().append(true).open(part)?
        } else {
            File::create(part)?
        };
        let mut buffer = vec![0u8; CHUNK_SIZE];
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])?;
            on_chunk(read as u64);
        }
        file.flush()?;
        Ok(())
    }
}

pub struct ModelRegistry {
    root: PathBuf,
    models: Vec<ModelVersion>,
    downloader: Box<dyn ModelDownloader>,
}

impl fmt::Debug for ModelRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ModelRegistry")
            .field("root", &self.root)
            .field("models", &self.models)
            .finish_non_exhaustive()
    }
}

impl ModelRegistry {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            models: catalog(),
            downloader: Box::new(HttpDownloader),
        }
    }

    pub fn with_models(
        root: impl Into<PathBuf>,
        models: Vec<ModelVersion>,
        downloader: Box<dyn ModelDownloader>,
    ) -> Self {
        Self {
            root: root.into(),
            models,
            downloader,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list(&self) -> Vec<ModelInfo> {
        let active = self.active_map();
        self.models
            .iter()
            .map(|model| ModelInfo {
                id: model.id.clone(),
                version: model.version.clone(),
                license: model.license.clone(),
                license_url: model.license_url.clone(),
                size: model.total_size(),
                state: self.state(model),
                active: active.get(&model.id).is_some_and(|v| v == &model.version),
            })
            .collect()
    }

    pub fn state(&self, model: &ModelVersion) -> ModelState {
        let directory = self.model_dir(model);
        if !directory.exists() {
            return ModelState::Missing;
        }
        let mut any = false;
        let mut complete = true;
        for file in &model.files {
            let path = directory.join(&file.name);
            let part = part_path(&path);
            if path.exists() {
                any = true;
                if fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) != file.size {
                    complete = false;
                }
            } else {
                complete = false;
                if part.exists() {
                    any = true;
                }
            }
        }
        if complete {
            ModelState::Ready
        } else if any {
            ModelState::Partial
        } else {
            ModelState::Missing
        }
    }

    pub fn install(
        &self,
        model_id: &str,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<PathBuf, ModelError> {
        let model = self
            .find(model_id)
            .ok_or_else(|| ModelError::UnknownModel(model_id.to_owned()))?;
        validate_component(&model.id)?;
        validate_component(&model.version)?;
        let directory = self.model_dir(model);
        fs::create_dir_all(&directory)?;
        let total_size = model.total_size();
        let mut total_downloaded = 0;
        for (index, file) in model.files.iter().enumerate() {
            let destination = directory.join(&file.name);
            let part = part_path(&destination);
            if destination.exists()
                && fs::metadata(&destination)?.len() == file.size
                && hash_file(&destination)? == file.sha256
            {
                total_downloaded += file.size;
                on_progress(Progress {
                    model_id: model.id.clone(),
                    version: model.version.clone(),
                    file: file.name.clone(),
                    file_index: index,
                    file_count: model.files.len(),
                    file_downloaded: file.size,
                    file_total: file.size,
                    total_downloaded,
                    total_size,
                });
                continue;
            }
            let mut offset = fs::metadata(&part).map(|meta| meta.len()).unwrap_or(0);
            if offset > file.size {
                fs::remove_file(&part)?;
                offset = 0;
            }
            let base = total_downloaded;
            let mut received = offset;
            self.downloader
                .download(&file.url, &part, offset, &mut |chunk| {
                    received += chunk;
                    let file_downloaded = received;
                    on_progress(Progress {
                        model_id: model.id.clone(),
                        version: model.version.clone(),
                        file: file.name.clone(),
                        file_index: index,
                        file_count: model.files.len(),
                        file_downloaded,
                        file_total: file.size,
                        total_downloaded: base + file_downloaded,
                        total_size,
                    });
                })?;
            let actual_size = fs::metadata(&part)?.len();
            if actual_size != file.size {
                return Err(ModelError::SizeMismatch {
                    expected: file.size,
                    actual: actual_size,
                });
            }
            let actual_hash = hash_file(&part)?;
            if actual_hash != file.sha256 {
                fs::remove_file(&part)?;
                return Err(ModelError::HashMismatch {
                    expected: file.sha256.clone(),
                    actual: actual_hash,
                });
            }
            fs::rename(&part, &destination)?;
            total_downloaded += file.size;
        }
        Ok(directory)
    }

    pub fn verify(&self, model: &ModelVersion) -> Result<(), ModelError> {
        let directory = self.model_dir(model);
        for file in &model.files {
            let path = directory.join(&file.name);
            if !path.exists() {
                return Err(ModelError::NotInstalled(format!(
                    "{} {} ({})",
                    model.id, model.version, file.name
                )));
            }
            let size = fs::metadata(&path)?.len();
            if size != file.size {
                return Err(ModelError::SizeMismatch {
                    expected: file.size,
                    actual: size,
                });
            }
            let actual = hash_file(&path)?;
            if actual != file.sha256 {
                return Err(ModelError::HashMismatch {
                    expected: file.sha256.clone(),
                    actual,
                });
            }
        }
        Ok(())
    }

    pub fn load(&self, model_id: &str) -> Result<Vec<(String, PathBuf)>, ModelError> {
        let model = self
            .find(model_id)
            .ok_or_else(|| ModelError::UnknownModel(model_id.to_owned()))?;
        self.verify(model)?;
        let directory = self.model_dir(model);
        Ok(model
            .files
            .iter()
            .map(|file| (file.name.clone(), directory.join(&file.name)))
            .collect())
    }

    pub fn set_active(&self, model_id: &str) -> Result<(), ModelError> {
        let model = self
            .find(model_id)
            .ok_or_else(|| ModelError::UnknownModel(model_id.to_owned()))?;
        self.verify(model)?;
        fs::create_dir_all(&self.root)?;
        fs::write(
            self.root.join(format!("active-{}", model.id)),
            &model.version,
        )?;
        Ok(())
    }

    pub fn active_version(&self, model_id: &str) -> Option<String> {
        fs::read_to_string(self.root.join(format!("active-{model_id}")))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    pub fn remove(&self, model_id: &str, version: &str) -> Result<(), ModelError> {
        validate_component(model_id)?;
        validate_component(version)?;
        let directory = self.root.join(model_id).join(version);
        if directory.exists() {
            fs::remove_dir_all(&directory)?;
        }
        if self.active_version(model_id).as_deref() == Some(version) {
            let _ = fs::remove_file(self.root.join(format!("active-{model_id}")));
        }
        Ok(())
    }

    fn find(&self, model_id: &str) -> Option<&ModelVersion> {
        self.models.iter().find(|model| {
            model.id == model_id || format!("{}@{}", model.id, model.version) == model_id
        })
    }

    fn model_dir(&self, model: &ModelVersion) -> PathBuf {
        self.root.join(&model.id).join(&model.version)
    }

    fn active_map(&self) -> std::collections::HashMap<String, String> {
        let mut active = std::collections::HashMap::new();
        for model in &self.models {
            if let Some(version) = self.active_version(&model.id) {
                active.insert(model.id.clone(), version);
            }
        }
        active
    }
}

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

fn validate_component(value: &str) -> Result<(), ModelError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(ModelError::InvalidName(value.to_owned()));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, ModelError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK_SIZE];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone)]
    struct FakeDownloader {
        body: Vec<u8>,
        fail_after: Option<usize>,
        offsets: Arc<Mutex<Vec<u64>>>,
        calls: Arc<AtomicUsize>,
    }

    impl FakeDownloader {
        fn new(body: &[u8], fail_after: Option<usize>) -> Self {
            Self {
                body: body.to_vec(),
                fail_after,
                offsets: Arc::new(Mutex::new(Vec::new())),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn offsets(&self) -> Vec<u64> {
            self.offsets.lock().unwrap().clone()
        }
    }

    impl ModelDownloader for Arc<FakeDownloader> {
        fn download(
            &self,
            _url: &str,
            part: &Path,
            offset: u64,
            on_chunk: &mut dyn FnMut(u64),
        ) -> Result<(), ModelError> {
            self.offsets.lock().unwrap().push(offset);
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let limit = if call == 0 {
                self.fail_after.unwrap_or(self.body.len())
            } else {
                self.body.len()
            };
            let end = limit.min(self.body.len());
            let mut file = OpenOptions::new().create(true).append(true).open(part)?;
            if offset as usize > end {
                return Err(ModelError::Http("offset beyond body".to_owned()));
            }
            for chunk in self.body[offset as usize..end].chunks(3) {
                file.write_all(chunk)?;
                on_chunk(chunk.len() as u64);
            }
            if end < self.body.len() {
                return Err(ModelError::Http("interrupted".to_owned()));
            }
            Ok(())
        }
    }

    fn model(body: &[u8], hash: &str) -> ModelVersion {
        ModelVersion {
            id: "test-model".to_owned(),
            version: "v1".to_owned(),
            license: "MIT".to_owned(),
            license_url: "https://example.test/license".to_owned(),
            files: vec![ModelFile {
                name: "weights.onnx".to_owned(),
                url: "https://example.test/weights.onnx".to_owned(),
                sha256: hash.to_owned(),
                size: body.len() as u64,
            }],
        }
    }

    fn sha(body: &[u8]) -> String {
        use sha2::Digest;
        format!("{:x}", Sha256::digest(body))
    }

    #[test]
    fn install_verifies_hash_and_resumes_partial_downloads() {
        let body = b"0123456789abcdef";
        let directory = tempfile::tempdir().expect("tempdir");
        let downloader = Arc::new(FakeDownloader::new(body, Some(5)));
        let probe = Arc::clone(&downloader);
        let registry = ModelRegistry::with_models(
            directory.path(),
            vec![model(body, &sha(body))],
            Box::new(Arc::clone(&downloader)),
        );
        let info = &registry.list()[0];
        assert_eq!(info.state, ModelState::Missing);

        let mut progress = Vec::new();
        let first = registry.install("test-model", |event| progress.push(event));
        assert!(first.is_err());
        assert_eq!(registry.list()[0].state, ModelState::Partial);
        assert!(progress.iter().any(|event| event.total_downloaded == 5));

        let path = registry.install("test-model", |_| {}).expect("resume");
        assert_eq!(fs::read(path.join("weights.onnx")).expect("file"), body);
        assert_eq!(registry.list()[0].state, ModelState::Ready);
        assert_eq!(probe.offsets(), vec![0, 5]);
        assert_eq!(fs::read_dir(path).expect("dir").count(), 1);
    }

    #[test]
    fn hash_mismatch_is_never_ready() {
        let body = b"bad weights";
        let directory = tempfile::tempdir().expect("tempdir");
        let registry = ModelRegistry::with_models(
            directory.path(),
            vec![model(body, &"0".repeat(64))],
            Box::new(Arc::new(FakeDownloader::new(body, None))),
        );
        let error = registry
            .install("test-model", |_| {})
            .expect_err("mismatch");
        assert!(matches!(error, ModelError::HashMismatch { .. }));
        assert_ne!(registry.list()[0].state, ModelState::Ready);
        assert!(registry.load("test-model").is_err());
    }

    #[test]
    fn tampered_installed_file_fails_verification_and_can_be_reinstalled() {
        let body = b"good weights";
        let directory = tempfile::tempdir().expect("tempdir");
        let registry = ModelRegistry::with_models(
            directory.path(),
            vec![model(body, &sha(body))],
            Box::new(Arc::new(FakeDownloader::new(body, None))),
        );
        let path = registry.install("test-model", |_| {}).expect("install");
        fs::write(path.join("weights.onnx"), b"evil weights").expect("tamper");
        assert!(matches!(
            registry.load("test-model"),
            Err(ModelError::HashMismatch { .. })
        ));
        registry.install("test-model", |_| {}).expect("reinstall");
        assert!(registry.load("test-model").is_ok());
    }

    #[test]
    fn active_version_requires_a_verified_install() {
        let body = b"weights";
        let directory = tempfile::tempdir().expect("tempdir");
        let registry = ModelRegistry::with_models(
            directory.path(),
            vec![model(body, &sha(body))],
            Box::new(Arc::new(FakeDownloader::new(body, None))),
        );
        assert!(registry.set_active("test-model").is_err());
        registry.install("test-model", |_| {}).expect("install");
        registry.set_active("test-model").expect("activate");
        assert_eq!(registry.active_version("test-model").as_deref(), Some("v1"));
        assert!(registry.list()[0].active);
        registry.remove("test-model", "v1").expect("remove");
        assert_eq!(registry.list()[0].state, ModelState::Missing);
        assert_eq!(registry.active_version("test-model"), None);
    }

    #[test]
    fn rejects_unknown_models_and_insecure_urls() {
        let directory = tempfile::tempdir().expect("tempdir");
        let registry = ModelRegistry::open(directory.path());
        assert!(matches!(
            registry.install("missing", |_| {}),
            Err(ModelError::UnknownModel(_))
        ));
        let error = HttpDownloader
            .download(
                "http://example.test/model.onnx",
                &directory.path().join("part"),
                0,
                &mut |_| {},
            )
            .expect_err("insecure");
        assert!(matches!(error, ModelError::InsecureUrl(_)));
    }

    #[test]
    fn bundled_catalog_is_complete_and_https() {
        for model in catalog() {
            assert!(!model.files.is_empty());
            assert_eq!(model.license, "Apache-2.0");
            assert!(model.license_url.starts_with("https://"));
            for file in model.files {
                assert!(file.url.starts_with("https://"));
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.chars().all(|ch| ch.is_ascii_hexdigit()));
            }
        }
    }
}
