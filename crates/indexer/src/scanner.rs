use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "heic", "heif", "webp"];

/// A file discovered by a root scan. Hashing and database identity are kept
/// out of this type so a scan never invents an Asset identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFile {
    pub absolute_path: PathBuf,
    pub relative_path: String,
    pub file_size: u64,
    pub modified_at: SystemTime,
}

#[derive(Debug)]
pub enum ScanError {
    RootNotDirectory(PathBuf),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootNotDirectory(path) => write!(
                formatter,
                "scan root is not a directory: {}",
                path.display()
            ),
            Self::Io { path, source } => {
                write!(formatter, "cannot scan {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ScanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::RootNotDirectory(_) => None,
        }
    }
}

/// Recursively discovers supported image files below a registered root.
/// Hidden entries and symlinks are ignored before their targets are opened.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scanner;

impl Scanner {
    pub fn scan(&self, root: impl AsRef<Path>) -> Result<Vec<DiscoveredFile>, ScanError> {
        let root = root.as_ref();
        let metadata = fs::metadata(root).map_err(|source| ScanError::Io {
            path: root.to_owned(),
            source,
        })?;
        if !metadata.is_dir() {
            return Err(ScanError::RootNotDirectory(root.to_owned()));
        }

        let mut discovered = Vec::new();
        self.visit(root, root, &mut discovered)?;
        discovered.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(discovered)
    }

    fn visit(
        &self,
        root: &Path,
        directory: &Path,
        output: &mut Vec<DiscoveredFile>,
    ) -> Result<(), ScanError> {
        let entries = fs::read_dir(directory).map_err(|source| ScanError::Io {
            path: directory.to_owned(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ScanError::Io {
                path: directory.to_owned(),
                source,
            })?;
            let path = entry.path();
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let file_type = entry.file_type().map_err(|source| ScanError::Io {
                path: path.clone(),
                source,
            })?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                self.visit(root, &path, output)?;
                continue;
            }
            if !file_type.is_file() || !is_supported(&path) {
                continue;
            }

            let metadata = entry.metadata().map_err(|source| ScanError::Io {
                path: path.clone(),
                source,
            })?;
            let modified_at = metadata.modified().map_err(|source| ScanError::Io {
                path: path.clone(),
                source,
            })?;
            let relative_path = path
                .strip_prefix(root)
                .expect("visited path is below root")
                .to_string_lossy()
                .replace('\\', "/");
            output.push(DiscoveredFile {
                absolute_path: path,
                relative_path,
                file_size: metadata.len(),
                modified_at,
            });
        }
        Ok(())
    }
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        io::Write,
    };
    use tempfile::TempDir;

    fn file(root: &Path, relative: &str, contents: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        File::create(&path)
            .expect("file")
            .write_all(contents)
            .expect("contents");
    }

    #[test]
    fn recursively_finds_supported_non_hidden_files_in_stable_order() {
        let root = TempDir::new().expect("temp root");
        file(root.path(), "z/last.WEBP", b"z");
        file(root.path(), "a/first.JPG", b"a");
        file(root.path(), "a/not-image.txt", b"x");
        file(root.path(), ".hidden/secret.jpg", b"x");
        file(root.path(), "a/.hidden.jpg", b"x");

        let files = Scanner.scan(root.path()).expect("scan");
        assert_eq!(
            files
                .iter()
                .map(|file| file.relative_path.as_str())
                .collect::<Vec<_>>(),
            ["a/first.JPG", "z/last.WEBP"]
        );
        assert_eq!(files[0].file_size, 1);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks_or_symlinked_directories() {
        use std::os::unix::fs::symlink;
        let root = TempDir::new().expect("temp root");
        file(root.path(), "real/photo.jpg", b"photo");
        symlink(
            root.path().join("real/photo.jpg"),
            root.path().join("linked.jpg"),
        )
        .expect("file link");
        symlink(root.path().join("real"), root.path().join("linked-dir")).expect("directory link");

        let files = Scanner.scan(root.path()).expect("scan");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "real/photo.jpg");
    }

    #[test]
    fn rejects_a_file_as_the_scan_root() {
        let root = TempDir::new().expect("temp root");
        file(root.path(), "photo.jpg", b"photo");
        assert!(matches!(
            Scanner.scan(root.path().join("photo.jpg")),
            Err(ScanError::RootNotDirectory(_))
        ));
    }
}
