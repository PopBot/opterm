//! Small, temporary tab offers between desktop windows. No commands are stored.
use crate::{
    layouts::{SavedTab, Window},
    profiles::fresh_id,
    settings::{commit, prepare, private_dir},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: usize = 256 * 1024;
const TIMEOUT_MS: u64 = 15_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub id: String,
    pub source: String,
    pub target: Option<String>,
    pub tab: SavedTab,
    expires: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    window: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    Pending,
    Accepted { window: String },
    Expired,
}
pub struct Pending {
    state: PathBuf,
    offer: Offer,
    finished: bool,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn valid_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix)
        .is_some_and(|suffix| suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn directory(state: &Path) -> Result<PathBuf> {
    let path = state.join("transfers");
    private_dir(&path)?;
    Ok(path)
}
fn file(directory: &Path, id: &str, suffix: &str) -> Result<PathBuf> {
    ensure!(valid_id(id, "transfer-"), "Invalid tab transfer ID");
    Ok(directory.join(format!("{id}.{suffix}")))
}
fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "Tab transfer must be a regular file"
    );
    let mut bytes = Vec::new();
    File::open(path)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_BYTES, "Tab transfer exceeds 256 KiB");
    Ok(serde_json::from_slice(&bytes)?)
}
fn lock(path: &Path) -> Result<File> {
    if path.exists() {
        ensure!(
            fs::symlink_metadata(path)?.is_file(),
            "Tab transfer lock must be a regular file"
        );
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let handle = options.open(path)?;
    handle.lock()?;
    Ok(handle)
}
fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(bytes.len() <= MAX_BYTES, "Tab transfer exceeds 256 KiB");
    commit(prepare(path, &bytes)?, path)
}
fn live_lock(state: &Path, id: &str) -> bool {
    if !valid_id(id, "window-") {
        return false;
    }
    let path = state.join("layouts").join(format!("{id}.lock"));
    if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .is_ok_and(|handle| matches!(handle.try_lock(), Err(std::fs::TryLockError::WouldBlock)))
}
fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
impl Offer {
    fn validate(&self) -> Result<()> {
        ensure!(
            valid_id(&self.id, "transfer-") && valid_id(&self.source, "window-"),
            "Invalid tab transfer identity"
        );
        if let Some(target) = &self.target {
            ensure!(
                valid_id(target, "window-") && target != &self.source,
                "Invalid transfer destination"
            );
        }
        let mut window = Window::empty(self.source.clone());
        window.tabs.push(self.tab.clone());
        window.validate()
    }
    pub fn send(
        state: &Path,
        source: &str,
        target: Option<&str>,
        tab: SavedTab,
    ) -> Result<Pending> {
        let offer = Self {
            id: fresh_id("transfer")?,
            source: source.into(),
            target: target.map(str::to_owned),
            tab,
            expires: now().saturating_add(TIMEOUT_MS),
        };
        offer.validate()?;
        ensure!(live_lock(state, source), "Source window is no longer open");
        if let Some(target) = target {
            ensure!(
                live_lock(state, target),
                "Destination window is no longer open"
            );
        }
        let dir = directory(state)?;
        // A desktop crash must not leave expired offers consuming the bounded mailbox.
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if !path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                continue;
            }
            let Ok(stale) = read::<Self>(&path) else {
                continue;
            };
            if stale.validate().is_err() || stale.expires > now() || live_lock(state, &stale.source)
            {
                continue;
            }
            if path
                .file_stem()
                .is_none_or(|stem| stem != stale.id.as_str())
            {
                continue;
            }
            let guard = lock(&file(&dir, &stale.id, "lock")?)?;
            remove(&path)?;
            remove(&file(&dir, &stale.id, "ack")?)?;
            drop(guard);
            remove(&file(&dir, &stale.id, "lock")?)?;
        }
        ensure!(
            fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json"))
                .count()
                < 64,
            "Too many pending tab transfers"
        );
        let _guard = lock(&file(&dir, &offer.id, "lock")?)?;
        write(&file(&dir, &offer.id, "json")?, &offer)?;
        Ok(Pending {
            state: state.into(),
            offer,
            finished: false,
        })
    }
    /// Call only after attaching every available reference and saving the target layout.
    /// Failure means the target must roll back its imported view; the source retains it.
    pub fn acknowledge(&self, state: &Path, receiver: &str) -> Result<()> {
        ensure!(
            valid_id(receiver, "window-") && receiver != self.source,
            "Invalid receiving window"
        );
        ensure!(
            live_lock(state, &self.source) && live_lock(state, receiver),
            "Source or destination window is no longer open"
        );
        let dir = directory(state)?;
        let _guard = lock(&file(&dir, &self.id, "lock")?)?;
        let current: Self = read(&file(&dir, &self.id, "json")?)?;
        current.validate()?;
        ensure!(
            current.expires > now(),
            "Tab transfer expired; source view retained"
        );
        ensure!(
            current
                .target
                .as_deref()
                .is_none_or(|target| target == receiver),
            "Tab transfer belongs to another window"
        );
        ensure!(current.source == self.source, "Tab transfer source changed");
        let ack = file(&dir, &self.id, "ack")?;
        ensure!(!ack.exists(), "Tab transfer has already been accepted");
        write(
            &ack,
            &Ack {
                window: receiver.into(),
            },
        )
    }
}
impl Pending {
    pub fn id(&self) -> &str {
        &self.offer.id
    }
    pub fn poll(&mut self) -> Result<Status> {
        if self.finished {
            return Ok(Status::Expired);
        }
        let dir = directory(&self.state)?;
        let _guard = lock(&file(&dir, &self.offer.id, "lock")?)?;
        let ack = file(&dir, &self.offer.id, "ack")?;
        if ack.exists() {
            let accepted: Ack = read(&ack)?;
            ensure!(
                valid_id(&accepted.window, "window-")
                    && accepted.window != self.offer.source
                    && self
                        .offer
                        .target
                        .as_deref()
                        .is_none_or(|target| target == accepted.window),
                "Invalid tab transfer acknowledgment"
            );
            remove(&file(&dir, &self.offer.id, "json")?)?;
            remove(&ack)?;
            self.finished = true;
            drop(_guard);
            remove(&file(&dir, &self.offer.id, "lock")?)?;
            return Ok(Status::Accepted {
                window: accepted.window,
            });
        }
        if now() >= self.offer.expires || !file(&dir, &self.offer.id, "json")?.exists() {
            remove(&file(&dir, &self.offer.id, "json")?)?;
            self.finished = true;
            drop(_guard);
            remove(&file(&dir, &self.offer.id, "lock")?)?;
            return Ok(Status::Expired);
        }
        Ok(Status::Pending)
    }
    pub fn cancel(&mut self) -> Result<()> {
        if !self.finished {
            let dir = directory(&self.state)?;
            let _guard = lock(&file(&dir, &self.offer.id, "lock")?)?;
            remove(&file(&dir, &self.offer.id, "json")?)?;
            remove(&file(&dir, &self.offer.id, "ack")?)?;
            self.finished = true;
            drop(_guard);
            remove(&file(&dir, &self.offer.id, "lock")?)?;
        }
        Ok(())
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}
/// Existing windows only discover offers explicitly addressed to their own ID.
pub fn incoming(state: &Path, receiver: &str) -> Result<Vec<Offer>> {
    ensure!(valid_id(receiver, "window-"), "Invalid receiving window");
    let dir = directory(state)?;
    let mut offers = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let Ok(offer) = read::<Offer>(&path) else {
                continue;
            };
            if offer.validate().is_ok()
                && path
                    .file_stem()
                    .is_some_and(|stem| stem == offer.id.as_str())
                && offer.target.as_deref() == Some(receiver)
                && offer.expires > now()
                && !path.with_extension("ack").exists()
            {
                offers.push(offer);
            }
        }
    }
    Ok(offers)
}
/// New-window offers must be selected explicitly through the internal receive argument.
pub fn receive(state: &Path, id: &str, receiver: &str) -> Result<Offer> {
    ensure!(valid_id(receiver, "window-"), "Invalid receiving window");
    let dir = directory(state)?;
    let _guard = lock(&file(&dir, id, "lock")?)?;
    let offer: Offer = read(&file(&dir, id, "json")?)?;
    offer.validate()?;
    ensure!(
        offer.id == id
            && offer.source != receiver
            && offer
                .target
                .as_deref()
                .is_none_or(|target| target == receiver),
        "Tab transfer belongs to another window"
    );
    ensure!(
        offer.expires > now() && !file(&dir, id, "ack")?.exists(),
        "Tab transfer expired or already accepted"
    );
    Ok(offer)
}
/// Only windows whose layout lock is currently held are transfer destinations.
pub fn live_windows(state: &Path) -> Result<Vec<Window>> {
    let dir = state.join("layouts");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut windows = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if !path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            continue;
        }
        let Ok(window) = read::<Window>(&path) else {
            continue;
        };
        if window.validate().is_err()
            || path
                .file_stem()
                .is_none_or(|stem| stem != window.id.as_str())
        {
            continue;
        }
        let path = path.with_extension("lock");
        if !fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        let Ok(handle) = OpenOptions::new().read(true).write(true).open(path) else {
            continue;
        };
        if matches!(handle.try_lock(), Err(std::fs::TryLockError::WouldBlock)) {
            windows.push(window);
        }
    }
    windows.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timeout_preserves_source_and_rejects_late_ack() {
        let state = tempfile::tempdir().unwrap();
        let source = crate::layouts::Store::open(state.path(), None, false)
            .unwrap()
            .unwrap()
            .store;
        let target = crate::layouts::Store::open(state.path(), None, false)
            .unwrap()
            .unwrap()
            .store;
        let tab = SavedTab {
            name: None,
            focused: 0,
            maximized: false,
            layout: crate::layouts::Node::Pane {
                session: crate::layouts::SessionRef {
                    instance: "service".into(),
                    id: 1,
                },
                title: "Shell".into(),
            },
        };
        let mut pending = Offer::send(
            state.path(),
            &source.window.id,
            Some(&target.window.id),
            tab,
        )
        .unwrap();
        let offer = receive(state.path(), pending.id(), &target.window.id).unwrap();
        pending.offer.expires = 0;
        assert_eq!(pending.poll().unwrap(), Status::Expired);
        assert!(offer.acknowledge(state.path(), &target.window.id).is_err());
        assert!(source.window.tabs.is_empty());
    }
}
