//! Small, local session service. Live state stays in this process, not in a DB.
use crate::profiles::{Launch, ProfileTag, Theme};
use crate::terminal::{Screen, Session};
use anyhow::{Context, Result, bail, ensure};
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Name, prelude::*};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;

pub mod client;
pub const VERSION: u32 = 1;
const MAX_FRAME: usize = 32 * 1024 * 1024;
const MAX_SESSIONS: usize = 128;

#[derive(Clone, Serialize, Deserialize)]
pub struct Info {
    pub id: u64,
    pub title: String,
    pub name: Option<String>,
    pub exited: bool,
    pub attached: bool,
    pub controlled: bool,
    #[serde(default)]
    pub profile: Option<ProfileTag>,
    #[serde(default)]
    pub recording: bool,
    #[serde(default)]
    pub history_error: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub info: Info,
    pub revision: u64,
    pub screen: Screen,
    pub selected_text: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub enum Request {
    Hello(u32),
    List,
    Create(Vec<String>),
    CreateProfile(Box<Launch>),
    Themes(Vec<Theme>),
    ConfigHistory(crate::history::Options),
    Recording {
        id: u64,
        enabled: bool,
    },
    Attach(u64),
    Detach(u64),
    TakeControl(u64),
    End(u64),
    Rename {
        id: u64,
        name: Option<String>,
    },
    View {
        id: u64,
        since: Option<u64>,
    },
    Write {
        id: u64,
        bytes: Vec<u8>,
    },
    Paste {
        id: u64,
        text: String,
    },
    Resize {
        id: u64,
        cols: u16,
        rows: u16,
        width: f32,
        height: f32,
    },
    Scroll {
        id: u64,
        lines: i32,
    },
    Select {
        id: u64,
        row: usize,
        col: usize,
        start: bool,
    },
    Search {
        id: u64,
        query: String,
        backwards: bool,
        reset: bool,
        token: u64,
    },
    Shutdown,
}
#[derive(Serialize, Deserialize)]
pub enum Reply {
    Hello {
        version: u32,
        pid: u32,
        #[serde(default)]
        profiles: bool,
        #[serde(default)]
        search: bool,
        #[serde(default)]
        history: bool,
        #[serde(default)]
        instance: String,
    },
    List(Vec<Info>),
    Snapshot(Box<Snapshot>),
    Unchanged,
    Done,
    Error(String),
    Search {
        id: u64,
        token: u64,
        found: bool,
    },
}

fn write_frame<T: Serialize>(stream: &mut impl Write, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    ensure!(
        bytes.len() <= MAX_FRAME,
        "Local message exceeds the 32 MiB limit"
    );
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}
fn read_frame<T: DeserializeOwned>(stream: &mut impl Read) -> Result<T> {
    let mut header = [0; 4];
    stream.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    ensure!(
        length > 0 && length <= MAX_FRAME,
        "Invalid local message length"
    );
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn state_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OPTERM_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }
    Ok(directories::ProjectDirs::from("", "", "opterm")
        .context("Cannot locate the user's application data directory")?
        .data_local_dir()
        .join("runtime"))
}

fn private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
        let meta = fs::symlink_metadata(path)?;
        // SAFETY: geteuid has no arguments and does not dereference memory.
        ensure!(
            meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o077 == 0,
            "Service directory must be a private, user-owned directory: {}",
            path.display()
        );
    }
    #[cfg(windows)]
    fs::create_dir_all(path)?;
    Ok(())
}

fn endpoint(path: &Path) -> Result<Name<'static>> {
    #[cfg(unix)]
    {
        Ok(path
            .join("service.sock")
            .to_fs_name::<GenericFilePath>()?
            .into_owned())
    }
    #[cfg(windows)]
    {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hash);
        Ok(format!("opterm-{:016x}", hash.finish())
            .to_ns_name::<GenericNamespaced>()?
            .into_owned())
    }
}

pub struct Connection {
    stream: LocalSocketStream,
    pub profiles: bool,
    pub search_supported: bool,
    pub history_supported: bool,
    pub instance: String,
}
impl Connection {
    pub fn connect(path: &Path) -> Result<Self> {
        let path = fs::canonicalize(path)?;
        let mut connection = Self {
            stream: LocalSocketStream::connect(endpoint(&path)?)?,
            profiles: false,
            search_supported: false,
            history_supported: false,
            instance: String::new(),
        };
        match connection.call(Request::Hello(VERSION))? {
            Reply::Hello {
                version: VERSION,
                profiles,
                search,
                history,
                instance,
                ..
            } => {
                connection.profiles = profiles;
                connection.search_supported = search;
                connection.history_supported = history;
                connection.instance = instance;
                Ok(connection)
            }
            _ => bail!("Unsupported session service version; existing work was left running"),
        }
    }
    pub fn call(&mut self, request: Request) -> Result<Reply> {
        write_frame(&mut self.stream, &request)?;
        match read_frame(&mut self.stream)? {
            Reply::Error(error) => bail!("{error}"),
            reply => Ok(reply),
        }
    }
    pub fn connect_or_start(path: &Path) -> Result<Self> {
        private_dir(path)?;
        let path = fs::canonicalize(path)?;
        if let Ok(connection) = Self::connect(&path) {
            return Ok(connection);
        }
        let mut log_options = OpenOptions::new();
        log_options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            log_options.mode(0o600);
        }
        let log = log_options.open(path.join("service.log"))?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("--session-service")
            .env("OPTERM_STATE_DIR", &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: setsid is async-signal-safe; the closure only changes the
            // newly forked service child's process-session membership.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0000_0008 | 0x0000_0200); // Detached process, new process group.
        }
        let mut child = command
            .spawn()
            .context("Start background session service")?;
        // Reap this process if it exits while the desktop is still alive.
        thread::spawn(move || {
            let _ = child.wait();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(connection) = Self::connect(&path) {
                return Ok(connection);
            }
            ensure!(
                Instant::now() < deadline,
                "Session service did not become available. Check {}",
                path.join("service.log").display()
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

struct Managed {
    session: Session,
    name: Option<String>,
    owner: Option<u64>,
    viewers: BTreeSet<u64>,
    revision: u64,
    profile: Option<ProfileTag>,
}
impl Managed {
    fn info(&self, id: u64, client: u64) -> Info {
        Info {
            id,
            title: self.session.title(),
            name: self.name.clone(),
            exited: self.session.exited(),
            attached: !self.viewers.is_empty(),
            controlled: self.owner == Some(client),
            profile: self.profile.clone(),
            recording: self.session.recording(),
            history_error: self.session.history_error(),
        }
    }
    fn revision(&self) -> u64 {
        self.revision
            .wrapping_add(self.session.generation.load(Ordering::Relaxed))
    }
    fn snapshot(&self, id: u64, client: u64) -> Reply {
        // Read revision first. If output arrives during capture, the next poll
        // must refresh again rather than incorrectly marking stale cells current.
        let revision = self.revision();
        let mut screen = self.session.screen();
        let background = screen.background;
        screen.cells.retain(|cell| {
            cell.text != " " || cell.bg != background || cell.selected || cell.underline
        });
        Reply::Snapshot(Box::new(Snapshot {
            info: self.info(id, client),
            revision,
            screen,
            selected_text: self.session.selected_text(),
        }))
    }
    fn require_control(&self, client: u64) -> Result<()> {
        ensure!(
            self.owner == Some(client),
            "Session is controlled in another window. Use Take control to send input."
        );
        Ok(())
    }
}
#[derive(Default)]
struct Hub {
    history_directory: PathBuf,
    history_options: crate::history::Options,
    sessions: BTreeMap<u64, Managed>,
    next_id: u64,
    instance: String,
}

fn create(hub: &mut Hub, client: u64, launch: Launch, configured: bool) -> Result<Reply> {
    ensure!(
        hub.sessions.len() < MAX_SESSIONS,
        "128-session prototype limit reached. End unused sessions."
    );
    let session = Session::spawn_with_history(
        &launch,
        hub.history_directory.clone(),
        hub.history_options.clone(),
        || {},
    )
    .with_context(|| format!("Launch profile {}", launch.profile.name))?;
    hub.next_id += 1;
    let id = hub.next_id;
    let managed = Managed {
        session,
        name: None,
        owner: Some(client),
        viewers: BTreeSet::from([client]),
        revision: 0,
        profile: configured.then(|| launch.profile.tag()),
    };
    let reply = managed.snapshot(id, client);
    hub.sessions.insert(id, managed);
    Ok(reply)
}

fn handle(hub: &mut Hub, client: u64, request: Request) -> Result<Reply> {
    match request {
        Request::Hello(version) => {
            ensure!(version == VERSION, "Unsupported protocol version");
            return Ok(Reply::Hello {
                version: VERSION,
                pid: std::process::id(),
                profiles: true,
                search: true,
                history: true,
                instance: hub.instance.clone(),
            });
        }
        Request::List => {
            return Ok(Reply::List(
                hub.sessions
                    .iter()
                    .map(|(&id, m)| m.info(id, client))
                    .collect(),
            ));
        }
        Request::Create(program) => return create(hub, client, Launch::legacy(&program), false),
        Request::CreateProfile(launch) => return create(hub, client, *launch, true),
        Request::ConfigHistory(options) => {
            options.validate()?;
            for managed in hub.sessions.values() {
                managed.session.configure_history(options.clone());
            }
            hub.history_options = options;
            return Ok(Reply::Done);
        }
        Request::Themes(themes) => {
            ensure!(themes.len() <= 66, "Too many terminal themes");
            for theme in &themes {
                theme.validate()?;
            }
            for managed in hub.sessions.values() {
                let id = managed
                    .profile
                    .as_ref()
                    .map(|p| p.theme.as_str())
                    .unwrap_or(crate::profiles::DARK);
                managed.session.set_theme(
                    themes
                        .iter()
                        .find(|t| t.id == id)
                        .cloned()
                        .unwrap_or_else(Theme::dark),
                );
            }
            return Ok(Reply::Done);
        }
        Request::End(id) => {
            let managed = hub.sessions.get(&id).context("Session no longer exists")?;
            managed.require_control(client)?;
            hub.sessions.remove(&id);
            return Ok(Reply::Done);
        }
        Request::Shutdown => {
            hub.sessions.clear();
            return Ok(Reply::Done);
        }
        _ => {}
    }
    let id = match &request {
        Request::Attach(id) | Request::Detach(id) | Request::TakeControl(id) => *id,
        Request::Rename { id, .. }
        | Request::View { id, .. }
        | Request::Write { id, .. }
        | Request::Paste { id, .. }
        | Request::Resize { id, .. }
        | Request::Scroll { id, .. }
        | Request::Select { id, .. }
        | Request::Search { id, .. }
        | Request::Recording { id, .. } => *id,
        _ => unreachable!(),
    };
    let managed = hub
        .sessions
        .get_mut(&id)
        .context("Session no longer exists")?;
    match request {
        Request::Attach(_) => {
            managed.viewers.insert(client);
            if managed.owner.is_none() {
                managed.owner = Some(client);
            }
            managed.revision += 1;
            return Ok(managed.snapshot(id, client));
        }
        Request::TakeControl(_) => {
            ensure!(
                managed.viewers.contains(&client),
                "Attach before taking control"
            );
            managed.owner = Some(client);
            managed.revision += 1;
            return Ok(managed.snapshot(id, client));
        }
        Request::Detach(_) => {
            managed.viewers.remove(&client);
            if managed.owner == Some(client) {
                managed.owner = None;
            }
            managed.revision += 1;
        }
        Request::View { since, .. } => {
            ensure!(managed.viewers.contains(&client), "Attach before viewing");
            if since == Some(managed.revision()) {
                return Ok(Reply::Unchanged);
            }
            return Ok(managed.snapshot(id, client));
        }
        Request::Write { bytes, .. } => {
            managed.require_control(client)?;
            managed.session.write(&bytes)?;
        }
        Request::Paste { text, .. } => {
            managed.require_control(client)?;
            managed.session.paste(&text)?;
        }
        Request::Resize {
            cols,
            rows,
            width,
            height,
            ..
        } => {
            managed.require_control(client)?;
            ensure!(
                width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0,
                "Invalid cell dimensions"
            );
            managed.session.resize(cols, rows, width, height)?;
        }
        Request::Rename { name, .. } => {
            managed.require_control(client)?;
            ensure!(
                name.as_ref().is_none_or(|s| s.len() <= 256),
                "Tab name exceeds 256 bytes"
            );
            managed.name = name;
            managed.revision += 1;
        }
        Request::Scroll { lines, .. } => {
            managed.require_control(client)?;
            managed.session.scroll(lines);
        }
        Request::Recording { enabled, .. } => {
            managed.require_control(client)?;
            managed.session.set_recording(enabled);
        }
        Request::Search {
            id,
            query,
            backwards,
            reset,
            token,
        } => {
            managed.require_control(client)?;
            let found = managed.session.search(&query, backwards, reset)?;
            return Ok(Reply::Search { id, token, found });
        }
        Request::Select {
            row, col, start, ..
        } => {
            managed.require_control(client)?;
            managed.session.select(row, col, start);
        }
        _ => unreachable!(),
    }
    Ok(Reply::Done)
}

pub fn serve(path: &Path) -> Result<()> {
    private_dir(path)?;
    let path = fs::canonicalize(path)?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock: File = options.open(path.join("service.lock"))?;
    if let Err(error) = lock.try_lock() {
        return match error {
            std::fs::TryLockError::WouldBlock => Ok(()),
            error => Err(error.into()),
        };
    }
    let options = ListenerOptions::new()
        .name(endpoint(&path)?)
        .try_overwrite(true)
        .nonblocking(ListenerNonblockingMode::Accept);
    #[cfg(target_os = "linux")]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sddl = widestring::U16CString::from_str("D:P(A;;GA;;;OW)")?;
        options.security_descriptor(SecurityDescriptor::deserialize(&sddl)?)
    };
    let listener = options.create_sync()?;
    let hub = Arc::new(Mutex::new(Hub {
        instance: crate::profiles::fresh_id("service")?,
        history_directory: path.join("history"),
        ..Hub::default()
    }));
    let next_client = AtomicU64::new(1);
    let running = Arc::new(AtomicBool::new(true));
    while running.load(Ordering::Relaxed) {
        let mut stream = match listener.accept() {
            Ok(stream) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let client = next_client.fetch_add(1, Ordering::Relaxed);
        let hub = hub.clone();
        let running = running.clone();
        let instance = hub.lock().unwrap().instance.clone();
        thread::spawn(move || {
            // A version handshake precedes operations on every connection.
            let hello = read_frame::<Request>(&mut stream);
            if !matches!(hello, Ok(Request::Hello(VERSION))) {
                return;
            }
            if write_frame(
                &mut stream,
                &Reply::Hello {
                    version: VERSION,
                    pid: std::process::id(),
                    profiles: true,
                    search: true,
                    history: true,
                    instance,
                },
            )
            .is_err()
            {
                return;
            }
            while let Ok(request) = read_frame::<Request>(&mut stream) {
                let shutdown = matches!(request, Request::Shutdown);
                let reply = handle(&mut hub.lock().unwrap(), client, request)
                    .unwrap_or_else(|error| Reply::Error(format!("{error:#}")));
                let delivered = write_frame(&mut stream, &reply);
                if shutdown {
                    running.store(false, Ordering::Relaxed);
                }
                if delivered.is_err() || shutdown {
                    break;
                }
            }
            for managed in hub.lock().unwrap().sessions.values_mut() {
                managed.viewers.remove(&client);
                if managed.owner == Some(client) {
                    managed.owner = None;
                }
                managed.revision += 1;
            }
        });
    }
    drop(listener);
    drop(lock);
    Ok(())
}
