//! Application log kept in the platform log folder.
//!
//! Messages from Picker's own crates are written from `info` up, and
//! messages from dependencies from `warn` up. The file is rotated by size so
//! the log never grows without bound. Logs stay on this computer; nothing is
//! sent anywhere.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Name of the current log file inside the log folder.
pub(crate) const LOG_FILE: &str = "picker.log";
/// Size at which the current file is rotated.
const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Rotated files kept next to the current one (`picker.1.log` and so on).
const KEEP: usize = 3;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Starts logging into `dir`. Returns the path of the current log file.
pub(crate) fn init(dir: &Path) -> std::io::Result<PathBuf> {
    let sink = Sink::open(dir, MAX_BYTES, KEEP)?;
    let path = sink.path.clone();
    let logger = FileLogger {
        sink: Mutex::new(sink),
        echo: cfg!(debug_assertions),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(LevelFilter::Info);
        let _ = LOG_PATH.set(path.clone());
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            log::error!("panic: {info}");
            previous(info);
        }));
    }
    Ok(path)
}

/// Path of the current log file, once logging has started.
pub(crate) fn current_path() -> Option<&'static Path> {
    LOG_PATH.get().map(PathBuf::as_path)
}

struct FileLogger {
    sink: Mutex<Sink>,
    /// Also print to the terminal, for `npm run tauri dev`.
    echo: bool,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        accepts(metadata.target(), metadata.level())
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(
            unix_now(),
            record.level(),
            record.target(),
            &record.args().to_string(),
        );
        if self.echo {
            eprint!("{line}");
        }
        if let Ok(mut sink) = self.sink.lock() {
            sink.write(&line);
        }
    }

    fn flush(&self) {
        if let Ok(mut sink) = self.sink.lock() {
            let _ = sink.file.as_mut().map(File::flush);
        }
    }
}

fn accepts(target: &str, level: Level) -> bool {
    level <= Level::Warn || (level <= Level::Info && target.starts_with("picker"))
}

/// The current file plus its size, rotated before a write would pass the
/// limit.
struct Sink {
    path: PathBuf,
    file: Option<File>,
    written: u64,
    max_bytes: u64,
    keep: usize,
}

impl Sink {
    fn open(dir: &Path, max_bytes: u64, keep: usize) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join(LOG_FILE);
        let file = append(&path)?;
        let written = file.metadata().map_or(0, |metadata| metadata.len());
        Ok(Self {
            path,
            file: Some(file),
            written,
            max_bytes,
            keep,
        })
    }

    fn write(&mut self, line: &str) {
        let length = line.len() as u64;
        if self.written > 0 && self.written + length > self.max_bytes {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut() {
            if file.write_all(line.as_bytes()).is_ok() {
                self.written += length;
            }
        }
    }

    /// Shifts `picker.log` to `picker.1.log`, `picker.1.log` to
    /// `picker.2.log`, and so on, dropping the oldest.
    fn rotate(&mut self) {
        self.file = None;
        let rotated = |index: usize| self.path.with_file_name(format!("picker.{index}.log"));
        let _ = fs::remove_file(rotated(self.keep));
        for index in (1..self.keep).rev() {
            let _ = fs::rename(rotated(index), rotated(index + 1));
        }
        let _ = fs::rename(&self.path, rotated(1));
        self.file = append(&self.path).ok();
        self.written = 0;
    }
}

fn append(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn format_line(seconds: u64, level: Level, target: &str, message: &str) -> String {
    // One record per line, so a message with line breaks cannot forge
    // another entry.
    let message = message.replace(['\r', '\n'], " ");
    format!(
        "{} {level:<5} {target}: {message}\n",
        utc_timestamp(seconds)
    )
}

/// Formats seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
fn utc_timestamp(seconds: u64) -> String {
    let days = seconds / 86_400;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3600,
        time % 3600 / 60,
        time % 60
    )
}

/// Converts days since 1970-01-01 to a Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(utc_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_timestamp(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc_timestamp(1_793_318_399), "2026-10-29T23:59:59Z");
    }

    #[test]
    fn lines_cannot_span_several_entries() {
        let line = format_line(0, Level::Warn, "picker_core", "bad\nINFO forged");
        assert_eq!(
            line,
            "1970-01-01T00:00:00Z WARN  picker_core: bad INFO forged\n"
        );
    }

    #[test]
    fn own_info_and_any_warning_are_kept() {
        assert!(accepts("picker_core::worker", Level::Info));
        assert!(!accepts("picker_core::worker", Level::Debug));
        assert!(!accepts("tao::platform", Level::Info));
        assert!(accepts("tao::platform", Level::Warn));
    }

    #[test]
    fn the_file_is_rotated_and_old_files_are_dropped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut sink = Sink::open(dir.path(), 10, 2).expect("sink");
        for line in ["first\n", "second\n", "third\n", "fourth\n"] {
            sink.write(line);
        }
        let read = |name: &str| fs::read_to_string(dir.path().join(name)).ok();
        assert_eq!(read("picker.log").as_deref(), Some("fourth\n"));
        assert_eq!(read("picker.1.log").as_deref(), Some("third\n"));
        assert_eq!(read("picker.2.log").as_deref(), Some("second\n"));
        assert_eq!(read("picker.3.log"), None);

        // Reopening continues the existing file and its size.
        drop(sink);
        let mut sink = Sink::open(dir.path(), 10, 2).expect("reopen");
        assert_eq!(sink.written, 7);
        sink.write("fifth\n");
        assert_eq!(read("picker.log").as_deref(), Some("fifth\n"));
    }
}
