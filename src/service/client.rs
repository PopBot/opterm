//! UI-facing cache and one background IPC worker. No socket reads on the UI thread.
use super::{Connection, Info, Reply, Request, Snapshot};
use crate::terminal::Screen;
use crate::{layouts::SessionRef, profiles::Launch};
use alacritty_terminal::term::TermMode;
use anyhow::{Result, bail, ensure};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
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
    visible: Vec<u64>,
    created: VecDeque<Option<u64>>,
    searched: VecDeque<SearchResult>,
    error: Option<String>,
    disconnected: bool,
}
impl State {
    fn search_result(&mut self, result: SearchResult) {
        if self.searched.len() == 128 {
            self.searched.pop_front();
        }
        self.searched.push_back(result);
    }
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

pub enum Reconnect {
    All,
    Detached,
    Saved(Vec<SessionRef>),
    Fresh,
}

pub struct SearchResult {
    pub id: u64,
    pub token: u64,
    pub result: std::result::Result<bool, String>,
}

pub struct Client {
    sender: SyncSender<(Request, usize)>,
    state: Arc<Mutex<State>>,
    queued: Arc<AtomicUsize>,
    pub profiles: bool,
    pub search_supported: bool,
    pub instance: String,
}
impl Client {
    pub fn open(
        path: &Path,
        launch: Launch,
        force_new: bool,
        restore: bool,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        Self::open_with(
            path,
            launch,
            force_new,
            if restore {
                Reconnect::All
            } else {
                Reconnect::Fresh
            },
            true,
            wake,
        )
    }
    pub fn open_with(
        path: &Path,
        launch: Launch,
        force_new: bool,
        reconnect: Reconnect,
        create_if_empty: bool,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        let mut connection = Connection::connect_or_start(path)?;
        let mut state = State::default();
        let Reply::List(infos) = connection.call(Request::List)? else {
            bail!("Invalid session list reply");
        };
        let selected: Vec<u64> = infos
            .iter()
            .filter(|info| match &reconnect {
                Reconnect::All => true,
                Reconnect::Detached => !info.attached,
                Reconnect::Saved(sessions) => sessions
                    .iter()
                    .any(|saved| saved.matches(&connection.instance, &[info.id])),
                Reconnect::Fresh => false,
            })
            .map(|info| info.id)
            .collect();
        // A fresh window still needs discovery metadata immediately. Waiting
        // for the first periodic List can turn valid transfer references into
        // unavailable panes before their attachments have been requested.
        state.list(infos);
        for id in selected {
            match connection.call(Request::Attach(id)) {
                Ok(Reply::Snapshot(snapshot)) => {
                    state.views.insert(id);
                    state.snapshot(*snapshot);
                }
                Ok(_) => state.error = Some("Invalid attach reply".into()),
                Err(error) => state.error = Some(format!("Restore session: {error:#}")),
            }
        }
        if state.views.is_empty() && create_if_empty || force_new {
            let request = if connection.profiles {
                Ok(Request::CreateProfile(Box::new(launch.clone())))
            } else if launch.legacy_compatible() {
                Ok(Request::Create(launch.program()))
            } else {
                Err(anyhow::anyhow!(
                    "Running service predates launch profiles. Existing work was left running. Finish its sessions before stopping the service, or use an isolated OPTERM_STATE_DIR to try profiles."
                ))
            };
            match request.and_then(|request| connection.call(request)) {
                Ok(Reply::Snapshot(snapshot)) => {
                    state.views.insert(snapshot.info.id);
                    state.snapshot(*snapshot);
                }
                Ok(_) => state.error = Some("Invalid new session reply".into()),
                Err(error) => state.error = Some(format!("{error:#}")),
            }
        }
        let profiles = connection.profiles;
        let search_supported = connection.search_supported;
        let instance = connection.instance.clone();
        state.visible = state.views.iter().copied().collect();
        let state = Arc::new(Mutex::new(state));
        let queued = Arc::new(AtomicUsize::new(0));
        let (sender, receiver) = mpsc::sync_channel::<(Request, usize)>(1024);
        let client = Arc::new(Self {
            sender,
            state: state.clone(),
            queued: queued.clone(),
            profiles,
            search_supported,
            instance,
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
                                    { let mut state = state.lock().unwrap();
                                    if let Request::Search { id, token, .. } = &request {
                                        state.search_result(SearchResult { id: *id, token: *token, result: Err(format!("{error:#}")) });
                                    } else {
                                        state.error = Some(format!("{error:#}"));
                                    }
                                    if matches!(request, Request::Create(_) | Request::CreateProfile(_)) { state.created.push_back(None); }
                                    }
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
                    if last_view.elapsed() >= Duration::from_millis(16) {
                        let visible = {
                            let state = state.lock().unwrap();
                            state.visible.iter().filter(|id| state.views.contains(id))
                                .map(|id| (*id, state.screens.get(id).map(|c| c.revision))).collect::<Vec<_>>()
                        };
                        for (id, since) in visible {
                            match connection.call(Request::View { id, since }) {
                                Ok(Reply::Snapshot(snapshot)) => { state.lock().unwrap().snapshot(*snapshot); wake(); }
                                Ok(Reply::Unchanged) => {}
                                Ok(_) => bail!("Invalid screen reply"),
                                Err(error) => {
                                    let Reply::List(infos) = connection.call(Request::List)? else { bail!("Invalid session list"); };
                                    let mut state = state.lock().unwrap();
                                    state.list(infos);
                                    if state.views.contains(&id) { state.error = Some(format!("{error:#}")); }
                                    wake();
                                }
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
            self.search_supported || !matches!(&request, Request::Search { .. }),
            "Running service predates scrollback search. Existing sessions stay running."
        );
        ensure!(
            self.profiles || !matches!(&request, Request::CreateProfile(_) | Request::Themes(_)),
            "Running service predates profiles/themes. Existing work stays running; finish its sessions before deliberately stopping the service, or try an isolated OPTERM_STATE_DIR."
        );
        ensure!(
            !self.state.lock().unwrap().disconnected,
            "Session service is disconnected"
        );
        let weight = match &request {
            Request::Write { bytes, .. } => bytes.len() + 128,
            Request::Paste { text, .. } => text.len() + 128,
            Request::Create(_) | Request::CreateProfile(_) | Request::Themes(_) => {
                serde_json::to_vec(&request)?.len() + 128
            }
            Request::Search { query, .. } => query.len() + 128,
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
    pub fn search(
        &self,
        id: u64,
        query: &str,
        backwards: bool,
        reset: bool,
        token: u64,
    ) -> Result<()> {
        ensure!(
            self.search_supported,
            "Running service predates scrollback search. Existing sessions stay running; upgrade the service after its work finishes."
        );
        ensure!(query.len() <= 4096, "Search text exceeds 4096 bytes");
        self.send(Request::Search {
            id,
            query: query.to_owned(),
            backwards,
            reset,
            token,
        })
    }
    pub fn take_search_results(&self) -> Vec<SearchResult> {
        self.state.lock().unwrap().searched.drain(..).collect()
    }
    pub fn view_updates(&self) -> (Vec<u64>, Vec<Option<u64>>) {
        let mut state = self.state.lock().unwrap();
        (
            state.views.iter().copied().collect(),
            state.created.drain(..).collect(),
        )
    }
    pub fn infos(&self) -> Vec<Info> {
        self.state.lock().unwrap().infos.values().cloned().collect()
    }
    pub fn visible(&self, ids: Vec<u64>) {
        self.state.lock().unwrap().visible = ids;
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
    if let Reply::Search { id, token, found } = reply {
        state.search_result(SearchResult {
            id,
            token,
            result: Ok(found),
        });
        return;
    }
    if let Reply::Snapshot(snapshot) = reply {
        let id = snapshot.info.id;
        state.snapshot(*snapshot);
        if matches!(
            request,
            Request::Create(_) | Request::CreateProfile(_) | Request::Attach(_)
        ) {
            state.views.insert(id);
            if matches!(request, Request::Create(_) | Request::CreateProfile(_)) {
                state.created.push_back(Some(id));
            }
        }
    }
    match request {
        Request::Detach(id) | Request::End(id) => {
            state.views.remove(id);
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
        self.info()
            .map(|i| {
                if i.title.is_empty() {
                    i.profile.map(|p| p.name).unwrap_or_default()
                } else {
                    i.title
                }
            })
            .unwrap_or_default()
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
