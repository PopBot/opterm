//! Small per-window files. They contain references to live work, never commands.
use crate::{
    layout::Axis,
    profiles::fresh_id,
    settings::{commit, prepare, private_dir},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 256 * 1024;
const MAX_WINDOWS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRef {
    pub instance: String,
    pub id: u64,
}
impl SessionRef {
    pub fn matches(&self, instance: &str, ids: &[u64]) -> bool {
        !instance.is_empty() && self.instance == instance && ids.contains(&self.id)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Node {
    Pane {
        session: SessionRef,
        title: String,
    },
    Split {
        axis: Axis,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}
impl Node {
    pub fn sessions(&self) -> Vec<SessionRef> {
        match self {
            Self::Pane { session, .. } => vec![session.clone()],
            Self::Split { first, second, .. } => {
                let mut sessions = first.sessions();
                sessions.extend(second.sessions());
                sessions
            }
        }
    }
    fn validate(&self, depth: usize, sessions: &mut BTreeSet<SessionRef>) -> Result<()> {
        ensure!(depth <= 32, "Saved split tree is too deep");
        match self {
            Self::Pane { session, title } => {
                ensure!(
                    session.id > 0 && session.instance.len() <= 128,
                    "Invalid saved session reference"
                );
                ensure!(title.len() <= 512, "Saved terminal title is too long");
                ensure!(
                    sessions.insert(session.clone()),
                    "Duplicate session in saved window"
                );
                ensure!(sessions.len() <= 128, "Too many saved panes");
            }
            Self::Split {
                ratio,
                first,
                second,
                ..
            } => {
                ensure!(
                    ratio.is_finite() && *ratio > 0.0 && *ratio < 1.0,
                    "Invalid saved split ratio"
                );
                first.validate(depth + 1, sessions)?;
                second.validate(depth + 1, sessions)?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedTab {
    pub name: Option<String>,
    pub focused: usize,
    pub maximized: bool,
    pub layout: Node,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub version: u32,
    pub id: String,
    pub size: [f32; 2],
    pub position: Option<[f32; 2]>,
    pub maximized: bool,
    pub active: usize,
    pub tabs: Vec<SavedTab>,
}
impl Window {
    pub fn empty(id: String) -> Self {
        Self {
            version: 1,
            id,
            size: [1000.0, 680.0],
            position: None,
            maximized: false,
            active: 0,
            tabs: Vec::new(),
        }
    }
    pub fn sessions(&self) -> Vec<SessionRef> {
        self.tabs
            .iter()
            .flat_map(|tab| tab.layout.sessions())
            .collect()
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && valid_id(&self.id),
            "Unsupported or invalid saved window"
        );
        ensure!(
            self.size[0].is_finite()
                && (320.0..=16384.0).contains(&self.size[0])
                && self.size[1].is_finite()
                && (160.0..=16384.0).contains(&self.size[1]),
            "Invalid saved window size"
        );
        ensure!(
            self.position
                .is_none_or(|p| p.iter().all(|v| v.is_finite() && v.abs() <= 65536.0)),
            "Invalid saved window position"
        );
        ensure!(
            self.tabs.len() <= 128
                && (self.active < self.tabs.len() || self.tabs.is_empty() && self.active == 0),
            "Invalid saved tab selection"
        );
        let mut sessions = BTreeSet::new();
        for tab in &self.tabs {
            ensure!(
                tab.name.as_ref().is_none_or(|name| name.len() <= 512),
                "Saved tab name is too long"
            );
            tab.layout.validate(0, &mut sessions)?;
            ensure!(
                tab.focused < tab.layout.sessions().len(),
                "Invalid saved pane selection"
            );
        }
        Ok(())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_BYTES, "Saved layout exceeds 256 KiB");
        let window: Self = serde_json::from_slice(bytes)?;
        window.validate()?;
        Ok(window)
    }
}
fn valid_id(id: &str) -> bool {
    id.strip_prefix("window-")
        .is_some_and(|suffix| suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn read(path: &Path) -> Result<Window> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "Saved layout is not a regular file"
    );
    let mut bytes = Vec::new();
    File::open(path)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Window::decode(&bytes).with_context(|| format!("Read saved layout {}", path.display()))
}
fn claim(directory: &Path, id: &str) -> Result<Option<File>> {
    ensure!(valid_id(id), "Invalid saved window ID");
    let path = directory.join(format!("{id}.lock"));
    if path.exists() {
        ensure!(
            fs::symlink_metadata(&path)?.is_file(),
            "Invalid window lock"
        );
    }
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error.into()),
    }
}
/// Holding the lock gives this desktop exclusive ownership of its saved window.
pub struct Store {
    path: PathBuf,
    _lock: File,
    pub window: Window,
    saved: Vec<u8>,
}
pub struct Boot {
    pub store: Store,
    pub restored: bool,
    pub others: Vec<String>,
    pub notice: Option<String>,
}
impl Store {
    /// None means an explicitly requested window is already open elsewhere.
    pub fn open(state: &Path, requested: Option<&str>, restore: bool) -> Result<Option<Boot>> {
        let directory = state.join("layouts");
        private_dir(&directory)?;
        if let Some(id) = requested {
            ensure!(valid_id(id), "Invalid saved window ID");
            let window = read(&directory.join(format!("{id}.json")))?;
            ensure!(
                window.id == id && valid_id(id),
                "Saved window ID does not match its filename"
            );
            let Some(lock) = claim(&directory, id)? else {
                return Ok(None);
            };
            return Ok(Some(Boot {
                store: Self::loaded(&directory, lock, window),
                restored: true,
                others: Vec::new(),
                notice: None,
            }));
        }
        let mut entries = fs::read_dir(&directory)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect::<Vec<_>>();
        ensure!(
            entries.len() <= MAX_WINDOWS,
            "Saved window limit reached (64); remove unused layout files from {}",
            directory.display()
        );
        entries.sort();
        let mut chosen = None;
        let mut others = Vec::new();
        let mut notices = Vec::new();
        if restore {
            for path in &entries {
                let outcome = (|| -> Result<()> {
                    let window = read(path)?;
                    ensure!(
                        path.file_stem()
                            .is_some_and(|stem| stem == window.id.as_str()),
                        "Saved window ID does not match its filename"
                    );
                    if let Some(lock) = claim(&directory, &window.id)? {
                        if chosen.is_none() {
                            chosen = Some(Self::loaded(&directory, lock, window));
                        } else {
                            others.push(window.id);
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = outcome {
                    notices.push(format!("{error:#}. The saved file was left untouched."));
                }
            }
        }
        let restored = chosen.is_some();
        let store = match chosen {
            Some(store) => store,
            None => {
                ensure!(
                    entries.len() < MAX_WINDOWS,
                    "Saved window limit reached (64)"
                );
                let id = fresh_id("window")?;
                let lock = claim(&directory, &id)?.context("New window ID is already in use")?;
                Self::loaded(&directory, lock, Window::empty(id))
            }
        };
        Ok(Some(Boot {
            store,
            restored,
            others,
            notice: (!notices.is_empty()).then(|| notices.join(" ")),
        }))
    }
    fn loaded(directory: &Path, lock: File, window: Window) -> Self {
        Self {
            path: directory.join(format!("{}.json", window.id)),
            _lock: lock,
            window,
            saved: Vec::new(),
        }
    }
    pub fn save(&mut self, window: Window) -> Result<()> {
        window.validate()?;
        ensure!(
            window.id == self.window.id,
            "Cannot replace a different window's layout"
        );
        let bytes = serde_json::to_vec_pretty(&window)?;
        ensure!(bytes.len() <= MAX_BYTES, "Saved layout exceeds 256 KiB");
        if self.saved != bytes {
            commit(prepare(&self.path, &bytes)?, &self.path)?;
            self.saved = bytes;
        }
        self.window = window;
        Ok(())
    }
}
