//! Bounded, asynchronous raw terminal output. Files contain output bytes only.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, SystemTime},
};

const MIB: u64 = 1024 * 1024;
/// History cells exclude the live screen, which is separately capped at 500×200.
pub const MAX_HISTORY_CELLS: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub enabled: bool,
    pub retention_days: u16,
    pub total_mib: u32,
    pub stream_mib: u32,
    pub scrollback_lines: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 30,
            total_mib: 5120,
            stream_mib: 256,
            scrollback_lines: 10_000,
        }
    }
}
impl Options {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=3650).contains(&self.retention_days),
            "Saved output retention must be 1–3650 days"
        );
        ensure!(
            (1..=102_400).contains(&self.total_mib),
            "Saved output total limit must be 1–102400 MiB"
        );
        ensure!(
            (1..=self.total_mib).contains(&self.stream_mib),
            "Saved output stream limit must be 1 MiB up to the total limit"
        );
        ensure!(
            self.scrollback_lines <= 100_000,
            "Scrollback must be 0–100000 lines"
        );
        Ok(())
    }
    pub fn lines(&self, columns: usize) -> usize {
        self.scrollback_lines
            .min(MAX_HISTORY_CELLS / columns.max(1))
    }
}

fn active() -> &'static Mutex<BTreeSet<PathBuf>> {
    static ACTIVE: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    ACTIVE.get_or_init(Mutex::default)
}
fn disk_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Mutex::default)
}
struct Entry {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}
fn entries(directory: &Path) -> Result<Vec<Entry>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("output-") || !name.ends_with(".ansi") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() {
            continue;
        }
        ensure!(files.len() < 10_000, "Too many saved output files");
        files.push(Entry {
            path: entry.path(),
            bytes: metadata.len(),
            modified: metadata.modified()?,
        });
    }
    files.sort_by_key(|entry| entry.modified);
    Ok(files)
}
/// Remove expired/oldest closed streams; active files are never unlinked.
fn prune(directory: &Path, options: &Options, incoming: u64) -> Result<()> {
    let protected = active().lock().unwrap().clone();
    let age = Duration::from_secs(u64::from(options.retention_days) * 86400);
    let mut files = Vec::new();
    for entry in entries(directory)? {
        if !protected.contains(&entry.path)
            && entry.modified.elapsed().is_ok_and(|elapsed| elapsed > age)
        {
            fs::remove_file(entry.path)?;
        } else {
            files.push(entry);
        }
    }
    let mut total: u64 = files.iter().map(|entry| entry.bytes).sum();
    let limit = u64::from(options.total_mib) * MIB;
    for entry in files {
        if total.saturating_add(incoming) <= limit {
            break;
        }
        if !protected.contains(&entry.path) {
            fs::remove_file(entry.path)?;
            total = total.saturating_sub(entry.bytes);
        }
    }
    ensure!(
        total.saturating_add(incoming) <= limit,
        "Saved output total limit reached; active output files were preserved"
    );
    Ok(())
}
pub fn cleanup(directory: &Path, options: &Options) -> Result<()> {
    options.validate()?;
    let _guard = disk_lock().lock().unwrap();
    crate::settings::private_dir(directory)?;
    prune(directory, options, 0)
}

struct Shared {
    enabled: AtomicBool,
    finished: AtomicBool,
    error: Mutex<Option<String>>,
    dropped: AtomicU64,
    options: Mutex<Options>,
}
#[derive(Clone)]
pub struct Recorder {
    sender: Option<SyncSender<Vec<u8>>>,
    shared: Arc<Shared>,
}
impl Recorder {
    /// Recorder failures are isolated from the PTY and never prevent shell launch.
    pub fn new(directory: PathBuf, options: Options) -> Self {
        let shared = Arc::new(Shared {
            enabled: AtomicBool::new(options.enabled),
            finished: AtomicBool::new(false),
            error: Mutex::new(None),
            dropped: AtomicU64::new(0),
            options: Mutex::new(options),
        });
        let mut recorder = Self {
            sender: None,
            shared: shared.clone(),
        };
        let path = match crate::profiles::fresh_id("output") {
            Ok(id) => directory.join(format!("{id}.ansi")),
            Err(error) => {
                recorder.fail(format!("Create saved output ID: {error}"));
                return recorder;
            }
        };
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(128);
        let spawned = thread::Builder::new()
            .name("opterm-output-write".into())
            .spawn(move || {
                active().lock().unwrap().insert(path.clone());
                let mut maintenance = SystemTime::now();
                loop {
                    let bytes = match receiver.recv_timeout(Duration::from_secs(1)) {
                        Ok(mut bytes) => {
                            for _ in 0..63 {
                                let Ok(next) = receiver.try_recv() else {
                                    break;
                                };
                                bytes.extend_from_slice(&next);
                            }
                            Some(bytes)
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    if bytes.is_none() && shared.finished.load(Ordering::Relaxed) {
                        break;
                    }
                    let options = shared.options.lock().unwrap().clone();
                    if bytes.is_none()
                        && maintenance
                            .elapsed()
                            .is_ok_and(|age| age < Duration::from_secs(60))
                    {
                        continue;
                    }
                    let result = (|| -> Result<()> {
                        let _guard = disk_lock().lock().unwrap();
                        crate::settings::private_dir(&directory)?;
                        let incoming = bytes.as_ref().map_or(0, |bytes| bytes.len() as u64);
                        let written = fs::metadata(&path).map_or(0, |metadata| metadata.len());
                        ensure!(
                            written.saturating_add(incoming) <= u64::from(options.stream_mib) * MIB,
                            "Saved output stream limit reached"
                        );
                        prune(&directory, &options, incoming)?;
                        if let Some(bytes) = &bytes {
                            let mut open = OpenOptions::new();
                            open.append(true).create(true);
                            #[cfg(unix)]
                            {
                                use std::os::unix::fs::OpenOptionsExt;
                                open.mode(0o600).custom_flags(libc::O_NOFOLLOW);
                            }
                            let mut file = open.open(&path)?;
                            ensure!(
                                file.metadata()?.is_file(),
                                "Saved output path is not a regular file"
                            );
                            file.write_all(bytes)?;
                        }
                        Ok(())
                    })();
                    maintenance = SystemTime::now();
                    if let Err(error) = result {
                        shared.enabled.store(false, Ordering::Relaxed);
                        *shared.error.lock().unwrap() = Some(format!(
                            "Saved output stopped: {error}. Live work continues."
                        ));
                        if let Some(bytes) = bytes {
                            shared
                                .dropped
                                .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        }
                    }
                }
                active().lock().unwrap().remove(&path);
            });
        match spawned {
            Ok(_) => recorder.sender = Some(sender),
            Err(error) => recorder.fail(format!("Start saved output writer: {error}")),
        }
        recorder
    }
    fn fail(&mut self, error: String) {
        self.shared.enabled.store(false, Ordering::Relaxed);
        *self.shared.error.lock().unwrap() = Some(error);
    }
    pub fn finish(&self) {
        self.shared.finished.store(true, Ordering::Relaxed);
        self.shared.enabled.store(false, Ordering::Relaxed);
    }
    pub fn enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Relaxed)
    }
    pub fn set_enabled(&self, enabled: bool) {
        let globally_enabled = self.shared.options.lock().unwrap().enabled;
        self.shared.enabled.store(
            enabled
                && globally_enabled
                && self.sender.is_some()
                && !self.shared.finished.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }
    pub fn configure(&self, options: Options) {
        let changed = {
            let mut previous = self.shared.options.lock().unwrap();
            let changed = previous.enabled != options.enabled;
            *previous = options.clone();
            changed
        };
        if changed {
            self.set_enabled(options.enabled);
        }
    }
    pub fn error(&self) -> Option<String> {
        let dropped = self.shared.dropped.load(Ordering::Relaxed);
        let error = self.shared.error.lock().unwrap().clone();
        match (error, dropped) {
            (None, 0) => None,
            (error, count) if count > 0 => Some(format!(
                "{} Saved output gap: {count} bytes omitted.",
                error.unwrap_or_else(|| "Saved output writer could not keep up.".into())
            )),
            (error, _) => error,
        }
    }
    pub fn record(&self, bytes: &[u8]) {
        if !self.enabled() {
            return;
        }
        if let Some(sender) = &self.sender {
            // At most 1 MiB queued per session, regardless of the caller's chunk size.
            for chunk in bytes.chunks(8192) {
                if sender.try_send(chunk.to_vec()).is_err() {
                    self.shared
                        .dropped
                        .fetch_add(chunk.len() as u64, Ordering::Relaxed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_disk_cannot_block_recording_and_reports_dropped_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let guard = disk_lock().lock().unwrap();
        let recorder = Recorder::new(directory.path().join("history"), Options::default());
        let started = std::time::Instant::now();
        recorder.record(&vec![b'x'; 2 * 1024 * 1024]);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(recorder.error().unwrap().contains("gap"));
        drop(guard);
    }
    #[test]
    fn finished_streams_cannot_be_resumed() {
        let directory = tempfile::tempdir().unwrap();
        let recorder = Recorder::new(directory.path().join("history"), Options::default());
        recorder.finish();
        recorder.set_enabled(true);
        assert!(!recorder.enabled());
    }
}
