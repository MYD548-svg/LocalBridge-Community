use super::authority::{Authority, WorkspaceStamp};
use crate::app::DesktopLifecycle;
use crate::local_connection::{pipe::Pipe, profile::installation_id};
use serde_json::{Value, json};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
static CONNECTED_BROWSERS: AtomicUsize = AtomicUsize::new(0);
pub fn connected_browsers() -> usize {
    CONNECTED_BROWSERS.load(Ordering::Acquire)
}
struct Presence(bool);
impl Drop for Presence {
    fn drop(&mut self) {
        if self.0 {
            CONNECTED_BROWSERS.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub struct BrowserService {
    stopping: Arc<AtomicBool>,
    peers: Arc<Mutex<Vec<Pipe>>>,
    worker: Option<JoinHandle<()>>,
    observer: Option<JoinHandle<()>>,
}

pub fn workspace(app: &AppHandle) -> Option<WorkspaceStamp> {
    let lifecycle = app.try_state::<DesktopLifecycle>()?;
    let snapshot = lifecycle.control_plane_snapshot();
    if lifecycle
        .desired_state()
        .snapshot()
        .state
        .connection
        .as_ref()
        .is_none_or(|connection| {
            connection.mode != crate::local_connection::profile::ConnectionMode::Local
        })
    {
        return None;
    }
    if !snapshot
        .runtime
        .ready_value()
        .is_some_and(|state| state.state == crate::state::RuntimeState::Ready)
    {
        return None;
    }
    let projection = snapshot.workspace.ready_value()?;
    if projection.desired_path != projection.observed_path {
        return None;
    }
    let path = PathBuf::from(projection.observed_path.as_ref()?);
    let validated = crate::workspace::WorkspaceValidator.validate(&path).ok()?;
    let permission = serde_json::to_string(&snapshot.authority.ready_value()?.effective).ok()?;
    Some(WorkspaceStamp {
        path: path.to_string_lossy().into_owned(),
        identity: validated.identity().as_str().to_owned(),
        permission,
    })
}

impl BrowserService {
    pub fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(peers) = self.peers.lock() {
            for peer in peers.iter() {
                peer.close();
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Some(observer) = self.observer.take() {
            let _ = observer.join();
        }
    }
    pub fn start(app: &AppHandle) -> io::Result<Self> {
        let executable = std::env::current_exe()?.canonicalize()?;
        let root = executable
            .parent()
            .ok_or_else(|| io::Error::other("installation directory"))?;
        let expected = root.join("localbridge-browser-host.exe");
        // Development builds live together in target/debug; release is installed together.
        let name = super::control_pipe_name(&installation_id(root)?)?;
        let listener = Pipe::listen(&name, true)?;
        let authority_app = app.clone();
        let authority = Arc::new(Authority::open(
            app.path()
                .app_data_dir()
                .map_err(io::Error::other)?
                .join("browser-connections.json"),
            Arc::new(move || workspace(&authority_app)),
        )?);
        super::authority::install(authority.clone())?;
        let stopping = Arc::new(AtomicBool::new(false));
        let observer_stop = stopping.clone();
        let observer_authority = authority.clone();
        let peers = Arc::new(Mutex::new(Vec::new()));
        let stop = stopping.clone();
        let connections = peers.clone();
        let app = app.clone();
        let worker = thread::Builder::new()
            .name("localbridge-browser-control".into())
            .spawn(move || {
                let mut listener = listener;
                let mut workers: Vec<JoinHandle<()>> = Vec::new();
                while !stop.load(Ordering::Acquire) {
                    workers.retain(|worker| !worker.is_finished());
                    if listener.accept(&expected, &stop).is_ok() {
                        if workers.len() < 8 {
                            let mut pipe = listener.clone();
                            let authority = authority.clone();
                            let app = app.clone();
                            let stop = stop.clone();
                            let peers = connections.clone();
                            if let Ok(mut connections) = peers.lock() {
                                connections.push(pipe.clone());
                            }
                            workers.push(thread::spawn(move || {
                                let mut presence = Presence(false);
                                while !stop.load(Ordering::Acquire) {
                                    let bytes = match pipe.receive_with_tick(Duration::from_secs(1))
                                    {
                                        Ok(Some(bytes)) => bytes,
                                        Ok(None) => continue,
                                        Err(_) => break,
                                    };
                                    let request = serde_json::from_slice::<Value>(&bytes);
                                    let response = match request {
                                        Ok(request) => {
                                            let response = dispatch(&authority, &request, &app);
                                            if response.is_ok()
                                                && request["operation"] == "presence"
                                                && !presence.0
                                            {
                                                presence.0 = true;
                                                CONNECTED_BROWSERS.fetch_add(1, Ordering::AcqRel);
                                            }
                                            response
                                        }
                                        Err(_) => Err(io::Error::other("invalid control request")),
                                    };
                                    let response = match response {
                                        Ok(value) => json!({"ok":true,"value":value}),
                                        Err(error) => json!({"ok":false,"error":error.to_string()}),
                                    };
                                    if pipe.send(&serde_json::to_vec(&response).unwrap()).is_err() {
                                        break;
                                    }
                                }
                                pipe.close();
                                if let Ok(mut connections) = peers.lock() {
                                    connections.retain(|peer| !peer.same_connection(&pipe));
                                }
                            }));
                        } else {
                            listener.close();
                        }
                    } else {
                        listener.close();
                    }
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    match Pipe::listen(&name, false) {
                        Ok(next) => listener = next,
                        Err(_) => break,
                    }
                }
                listener.close();
                if let Ok(peers) = connections.lock() {
                    for peer in peers.iter() {
                        peer.close();
                    }
                }
                for worker in workers {
                    let _ = worker.join();
                }
            })?;
        let observer = thread::spawn(move || {
            while !observer_stop.load(Ordering::Acquire) {
                let _ = observer_authority.refresh_workspace();
                thread::sleep(Duration::from_millis(250));
            }
        });
        Ok(Self {
            stopping,
            peers,
            worker: Some(worker),
            observer: Some(observer),
        })
    }
}
impl Drop for BrowserService {
    fn drop(&mut self) {
        self.stop();
    }
}
fn required<'a>(request: &'a Value, name: &str) -> io::Result<&'a str> {
    request
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("missing browser control field"))
}
fn dispatch(authority: &Authority, request: &Value, app: &AppHandle) -> io::Result<Value> {
    match required(request, "operation")? {
        "pair" | "presence" => {
            let approved = authority.pair(
                required(request, "instance")?,
                required(request, "secret")?,
                required(request, "origin")?,
            )?;
            if !approved {
                let _ = app.emit(
                    "browser-pairing-requested",
                    json!({"instance":required(request,"instance")?}),
                );
            }
            Ok(json!({"approved":approved,"workspace":workspace(app)}))
        }
        "grant" => Ok(
            json!({"grant":authority.grant(required(request,"instance")?, required(request,"secret")?, required(request,"origin")?, required(request,"chat")?)?}),
        ),
        "close" => {
            authority.close(required(request, "grant")?);
            Ok(Value::Null)
        }
        _ => Err(io::Error::other("unsupported browser control operation")),
    }
}
