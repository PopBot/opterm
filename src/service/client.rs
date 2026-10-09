//! UI-facing cache and one background IPC worker. No socket reads on the UI thread.
use super::{Connection, Info, Reply, Request, Snapshot};
use crate::terminal::Screen;
use alacritty_terminal::term::TermMode;
use anyhow::{Result, bail, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

struct Cached {
    screen: Arc<Screen>,
    selected: Option<String>,
    revision: u64,
}
#[derive(Default)]
struct State {
    infos: BTreeMap<u64, Info>,
    views: BTreeSet<u64>,
    screens: BTreeMap<u64, Cached>,
    active: Option<u64>,
    error: Option<String>,
    disconnected: bool,
}
impl State {
    fn snapshot(&mut self, snapshot: Snapshot) {
        let id = snapshot.info.id;
        self.infos.insert(id, snapshot.info);
        self.screens.insert(
            id,
            Cached {
                screen: Arc::new(snapshot.screen),
                selected: snapshot.selected_text,
                revision: snapshot.revision,
            },
        );
    }
    fn list(&mut self, infos: Vec<Info>) {
        self.infos = infos.into_iter().map(|info| (info.id, info)).collect();
        self.views.retain(|id| self.infos.contains_key(id));
        self.screens.retain(|id, _| self.infos.contains_key(id));
    }
}

pub struct Client {
    sender: SyncSender<(Request, usize)>,
    state: Arc<Mutex<State>>,
    queued: Arc<AtomicUsize>,
}
impl Client {
    pub fn open(
        path: &Path,
        program: &[String],
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        let mut connection = Connection::connect_or_start(path)?;
        let mut state = State::default();
        let Reply::List(infos) = connection.call(Request::List)? else {
            bail!("Invalid session list reply");
        };
        for info in infos {
            let Reply::Snapshot(snapshot) = connection.call(Request::Attach(info.id))? else {
                bail!("Invalid attach reply");
            };
            state.views.insert(info.id);
            state.snapshot(*snapshot);
        }
        if state.views.is_empty() || !program.is_empty() {
            let Reply::Snapshot(snapshot) = connection.call(Request::Create(program.to_vec()))?
            else {
                bail!("Invalid new session reply");
            };
            state.views.insert(snapshot.info.id);
            state.snapshot(*snapshot);
        }
        state.active = state.views.last().copied();
        let state = Arc::new(Mutex::new(state));
        let queued = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel::<(Request, usize)>(1024);
        let client = Arc::new(Self {
            sender,
            state: state.clone(),
            queued: queued.clone(),
        });
        thread::Builder::new().name("opterm-service-client".into()).spawn(move || {
            let mut last_list = Instant::now();
            let mut last_view = Instant::now();
            let outcome = (|| -> Result<()> {
                loop {
                    match receiver.recv_timeout(Duration::from_millis(16)) {
                        Ok((request, weight)) => {
                            queued.fetch_sub(weight, Ordering::Relaxed);
                            // A rejected control operation is recoverable; losing
                            // the transport is not. Never resubmit input on EOF.
                            let reply = connection.call(request.clone());
                            match reply {
                                Ok(reply) => update(&mut state.lock().unwrap(), &request, reply),
                                Err(error) => {
                                    state.lock().unwrap().error = Some(format!("{error:#}"));
                                    // List checks whether the connection remains alive.
                                    let Reply::List(infos) = connection.call(Request::List)? else { bail!("Invalid session list"); };
                                    state.lock().unwrap().list(infos);
                                }
                            }
                            wake();
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    let active = {
                        let state = state.lock().unwrap();
                        state.active.filter(|id| state.views.contains(id)).map(|id| (id, state.screens.get(&id).map(|c| c.revision)))
                    };
                    if let Some((id, since)) = active && last_view.elapsed() >= Duration::from_millis(16) {
                        match connection.call(Request::View { id, since }) {
                            Ok(Reply::Snapshot(snapshot)) => { state.lock().unwrap().snapshot(*snapshot); wake(); }
                            Ok(Reply::Unchanged) => {}
                            Ok(_) => bail!("Invalid screen reply"),
                            Err(error) => {
                                // Another controller may have ended the viewed session.
                                let Reply::List(infos) = connection.call(Request::List)? else { bail!("Invalid session list"); };
                                let mut state = state.lock().unwrap();
                                state.list(infos);
                                if state.views.contains(&id) { state.error = Some(format!("{error:#}")); }
                                wake();
                            }
                        }
                        last_view = Instant::now();
                    }
                    if last_list.elapsed() >= Duration::from_millis(500) {
                        let Reply::List(infos) = connection.call(Request::List)? else { bail!("Invalid session list"); };
                        state.lock().unwrap().list(infos);
                        last_list = Instant::now();
                        wake();
                    }
                }
            })();
            if let Err(error) = outcome {
                let mut state = state.lock().unwrap();
                state.disconnected = true;
                state.error = Some(format!("Session service disconnected: {error:#}. Work was not restarted. Reopen the app to reconnect."));
                drop(state);
                wake();
            }
        })?;
        Ok(client)
    }

    pub fn send(&self, request: Request) -> Result<()> {
        ensure!(
            !self.state.lock().unwrap().disconnected,
            "Session service is disconnected"
        );
        let weight = match &request {
            Request::Write { bytes, .. } => bytes.len() + 128,
            Request::Paste { text, .. } => text.len() + 128,
            _ => 512,
        };
        ensure!(
            self.queued
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| (current
                    + weight
                    <= 1024 * 1024)
                    .then_some(current + weight))
                .is_ok(),
            "Terminal input queue is full. Input was not queued."
        );
        if let Err(error) = self.sender.try_send((request, weight)) {
            self.queued.fetch_sub(weight, Ordering::Relaxed);
            bail!("Terminal request was not queued: {error}");
        }
        Ok(())
    }
    pub fn views(&self) -> Vec<u64> {
        self.state.lock().unwrap().views.iter().copied().collect()
    }
    pub fn infos(&self) -> Vec<Info> {
        self.state.lock().unwrap().infos.values().cloned().collect()
    }
    pub fn active(&self, id: Option<u64>) {
        self.state.lock().unwrap().active = id;
    }
    pub fn take_error(&self) -> Option<String> {
        self.state.lock().unwrap().error.take()
    }
    pub fn session(self: &Arc<Self>, id: u64) -> RemoteSession {
        RemoteSession {
            id,
            client: self.clone(),
            size: Mutex::new(None),
        }
    }
}

fn update(state: &mut State, request: &Request, reply: Reply) {
    if let Reply::Snapshot(snapshot) = reply {
        let id = snapshot.info.id;
        state.snapshot(*snapshot);
        if matches!(request, Request::Create(_) | Request::Attach(_)) {
            state.views.insert(id);
            state.active = Some(id);
        }
    }
    match request {
        Request::Detach(id) | Request::End(id) => {
            state.views.remove(id);
            if state.active == Some(*id) {
                state.active = state.views.last().copied();
            }
            if matches!(request, Request::End(_)) {
                state.infos.remove(id);
                state.screens.remove(id);
            }
        }
        Request::Rename { id, name } => {
            if let Some(info) = state.infos.get_mut(id) {
                info.name = name.clone();
            }
        }
        _ => {}
    }
}

pub struct RemoteSession {
    pub id: u64,
    client: Arc<Client>,
    size: Mutex<Option<(u16, u16, u32, u32)>>,
}
impl RemoteSession {
    pub fn info(&self) -> Option<Info> {
        self.client
            .state
            .lock()
            .unwrap()
            .infos
            .get(&self.id)
            .cloned()
    }
    pub fn controlled(&self) -> bool {
        self.info().is_some_and(|i| i.controlled)
    }
    pub fn title(&self) -> String {
        self.info().map(|i| i.title).unwrap_or_default()
    }
    pub fn name(&self) -> Option<String> {
        self.info().and_then(|i| i.name)
    }
    pub fn screen(&self) -> Arc<Screen> {
        self.client
            .state
            .lock()
            .unwrap()
            .screens
            .get(&self.id)
            .map(|c| c.screen.clone())
            .unwrap_or_else(|| Arc::new(Screen::empty()))
    }
    pub fn mode(&self) -> TermMode {
        self.screen().mode
    }
    pub fn selected_text(&self) -> Option<String> {
        self.client
            .state
            .lock()
            .unwrap()
            .screens
            .get(&self.id)
            .and_then(|c| c.selected.clone())
    }
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.client.send(Request::Write {
            id: self.id,
            bytes: bytes.to_vec(),
        })
    }
    pub fn paste(&self, text: &str) -> Result<()> {
        self.client.send(Request::Paste {
            id: self.id,
            text: text.to_owned(),
        })
    }
    pub fn resize(&self, cols: u16, rows: u16, width: f32, height: f32) -> Result<()> {
        if !self.controlled() {
            *self.size.lock().unwrap() = None;
            return Ok(());
        }
        let size = (cols, rows, width.to_bits(), height.to_bits());
        let mut previous = self.size.lock().unwrap();
        if *previous != Some(size) {
            self.client.send(Request::Resize {
                id: self.id,
                cols,
                rows,
                width,
                height,
            })?;
            *previous = Some(size);
        }
        Ok(())
    }
    pub fn scroll(&self, lines: i32) -> Result<()> {
        self.client.send(Request::Scroll { id: self.id, lines })
    }
    pub fn select(&self, row: usize, col: usize, start: bool) -> Result<()> {
        self.client.send(Request::Select {
            id: self.id,
            row,
            col,
            start,
        })
    }
    pub fn rename(&self, name: Option<String>) -> Result<()> {
        self.client.send(Request::Rename { id: self.id, name })
    }
}
