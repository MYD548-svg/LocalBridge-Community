// SPDX-License-Identifier: MIT
// Adaptation copyright (c) 2024 Sergey Parfenyuk; LocalBridge modifications 2026.
//! Rust adaptation of mcp-proxy transport/proxy separation (153a96a6, MIT).
//! A raw envelope relay keeps IDs, cancellations, errors and retained outputs
//! intact. Credentials and HTTP session headers remain in the desktop process.
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::codec::{MAX_FRAME_BYTES, validate_message};
use super::pipe::{Pipe, pipe_name};
use super::profile::installation_id;
use crate::credentials::SecretString;

static SUCCESSFUL_CALLS: AtomicUsize = AtomicUsize::new(0);
static BROWSER_CALLS: AtomicUsize = AtomicUsize::new(0);
static BROWSER_CLIENTS: AtomicUsize = AtomicUsize::new(0);
pub fn browser_successful_calls() -> usize {
    BROWSER_CALLS.load(Ordering::Acquire)
}
pub fn browser_connected_clients() -> usize {
    BROWSER_CLIENTS.load(Ordering::Acquire)
}
pub fn successful_calls() -> usize {
    SUCCESSFUL_CALLS
        .load(Ordering::Acquire)
        .saturating_sub(browser_successful_calls())
}
static CLIENTS: AtomicUsize = AtomicUsize::new(0);
pub fn connected_clients() -> usize {
    CLIENTS
        .load(Ordering::Acquire)
        .saturating_sub(browser_connected_clients())
}

pub struct LocalRuntime {
    stopping: Arc<AtomicBool>,
    connections: Arc<Mutex<Vec<Pipe>>>,
    thread: Option<JoinHandle<()>>,
}

impl LocalRuntime {
    pub fn start(adapter: &Path, port: u16, bearer: SecretString) -> io::Result<Self> {
        let adapter = adapter.canonicalize()?;
        let parent = adapter
            .parent()
            .ok_or_else(|| io::Error::other("adapter has no installation directory"))?;
        let name = pipe_name(&installation_id(parent)?)?;
        let listener = Pipe::listen(&name, true)?;
        SUCCESSFUL_CALLS.store(0, Ordering::Release);
        BROWSER_CALLS.store(0, Ordering::Release);
        BROWSER_CLIENTS.store(0, Ordering::Release);
        let stopping = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(Mutex::new(Vec::new()));
        let stop = stopping.clone();
        let peers = connections.clone();
        let bearer = Arc::new(bearer);
        let thread = thread::Builder::new()
            .name("localbridge-local-mcp".into())
            .spawn(move || {
                let mut listener = listener;
                let mut workers: Vec<JoinHandle<()>> = Vec::new();
                while !stop.load(Ordering::Acquire) {
                    workers.retain(|worker| !worker.is_finished());
                    match listener.accept(&adapter, &stop) {
                        Ok(()) => {
                            let pipe = listener.clone();
                            if workers.len() >= 16 {
                                pipe.close();
                            } else {
                                if let Ok(mut peers) = peers.lock() {
                                    peers.push(pipe.clone());
                                }
                                let credential = bearer.clone();
                                let peers = peers.clone();
                                workers.push(thread::spawn(move || {
                                    serve_and_unregister(pipe, port, credential, peers);
                                }));
                            }
                        }
                        Err(_) if stop.load(Ordering::Acquire) => break,
                        Err(_) => listener.close(),
                    }
                    match Pipe::listen(&name, false) {
                        Ok(next) => listener = next,
                        Err(_) => break,
                    }
                }
                listener.close();
                if let Ok(peers) = peers.lock() {
                    for peer in peers.iter() {
                        peer.close();
                    }
                }
                for worker in workers {
                    let _ = worker.join();
                }
            })?;
        Ok(Self {
            stopping,
            connections,
            thread: Some(thread),
        })
    }

    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    #[cfg(test)]
    pub(super) fn registered_connections(&self) -> usize {
        self.connections.lock().unwrap().len()
    }

    pub fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(peers) = self.connections.lock() {
            for peer in peers.iter() {
                peer.close();
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for LocalRuntime {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone, Default)]
struct SocketPool {
    sockets: Arc<Mutex<HashMap<u64, TcpStream>>>,
    closing: Arc<AtomicBool>,
}
static SOCKET_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
struct SocketLease {
    pool: SocketPool,
    id: u64,
}
impl Drop for SocketLease {
    fn drop(&mut self) {
        if let Ok(mut sockets) = self.pool.sockets.lock() {
            sockets.remove(&self.id);
        }
    }
}
impl SocketPool {
    fn register(&self, stream: &TcpStream) -> io::Result<SocketLease> {
        let id = SOCKET_ID.fetch_add(1, Ordering::Relaxed);
        let mut sockets = self
            .sockets
            .lock()
            .map_err(|_| io::Error::other("socket lock failed"))?;
        if self.closing.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "session closed"));
        }
        sockets.insert(id, stream.try_clone()?);
        Ok(SocketLease {
            pool: self.clone(),
            id,
        })
    }
    fn close(&self) {
        self.closing.store(true, Ordering::Release);
        if let Ok(sockets) = self.sockets.lock() {
            for socket in sockets.values() {
                let _ = socket.shutdown(Shutdown::Both);
            }
        }
    }
}
struct HttpReply {
    body: Vec<u8>,
    session: Option<String>,
}
struct BrowserGrantLease(Option<String>);
impl Drop for BrowserGrantLease {
    fn drop(&mut self) {
        if let (Some(token), Some(authority)) =
            (&self.0, crate::browser_connection::authority::global())
        {
            authority.close(token);
        }
    }
}

fn http(
    pool: &SocketPool,
    port: u16,
    bearer: &SecretString,
    method: &str,
    session: Option<&str>,
    protocol: &str,
    body: &[u8],
) -> io::Result<HttpReply> {
    http_scoped(pool, port, bearer, method, session, protocol, body, None)
}

#[allow(clippy::too_many_arguments)]
fn http_scoped(
    pool: &SocketPool,
    port: u16,
    bearer: &SecretString,
    method: &str,
    session: Option<&str>,
    protocol: &str,
    body: &[u8],
    browser: Option<&str>,
) -> io::Result<HttpReply> {
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    let _lease = pool.register(&stream)?;
    let long_call = method == "POST"
        && serde_json::from_slice::<Value>(body)
            .ok()
            .is_some_and(|request| {
                request.get("id").is_some() && request["method"] != "initialize"
            });
    stream.set_read_timeout(Some(Duration::from_secs(if long_call { 610 } else { 5 })))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut headers = format!(
        "{method} /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: {protocol}\r\nConnection: close\r\nContent-Length: {}\r\n",
        bearer.expose_secret(),
        body.len()
    );
    if let Some(session) = session {
        headers.push_str(&format!("Mcp-Session-Id: {session}\r\n"));
    }
    if let Some(token) = browser {
        if token.len() > 128
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(io::Error::other("invalid browser authorization"));
        }
        headers.push_str(&format!("X-LocalBridge-Browser-Grant: {token}\r\n"));
    }
    headers.push_str("\r\n");
    stream.write_all(headers.as_bytes())?;
    stream.write_all(body)?;
    let mut bytes = Vec::new();
    stream
        .take((MAX_FRAME_BYTES + 32769) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FRAME_BYTES + 32768 {
        return Err(io::Error::other("HTTP frame too large"));
    }
    let boundary = bytes
        .windows(4)
        .position(|bytes| bytes == b"\r\n\r\n")
        .ok_or_else(|| io::Error::other("invalid HTTP reply"))?;
    let headers = std::str::from_utf8(&bytes[..boundary])
        .map_err(|_| io::Error::other("invalid HTTP headers"))?;
    let session = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("mcp-session-id"))
        .map(|(_, value)| value.trim().to_owned());
    if session
        .as_ref()
        .is_some_and(|session| session.bytes().any(|byte| !(33..=126).contains(&byte)))
    {
        return Err(io::Error::other("invalid session identity"));
    }
    let mut body = bytes[boundary + 4..].to_vec();
    if body.len() > MAX_FRAME_BYTES {
        return Err(io::Error::other("HTTP payload too large"));
    }
    if headers
        .to_ascii_lowercase()
        .contains("content-type: text/event-stream")
    {
        body = body
            .split(|byte| *byte == b'\n')
            .find_map(|line| line.strip_prefix(b"data: ").map(<[u8]>::to_vec))
            .unwrap_or_default();
    }
    Ok(HttpReply { body, session })
}

fn error_reply(id: Value, reason: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":reason}}))
        .expect("JSON error envelope")
}

fn forward(pipe: &Pipe, reply: io::Result<HttpReply>, request: &Value) {
    let id = request.get("id").cloned();
    match reply {
        Ok(reply) if !reply.body.is_empty() => {
            if let Ok(message) = validate_message(&reply.body) {
                if request["method"] == "tools/call"
                    && message.get("result").is_some()
                    && message.pointer("/result/isError").and_then(Value::as_bool) != Some(true)
                {
                    SUCCESSFUL_CALLS.fetch_add(1, Ordering::AcqRel);
                }
                if id.is_some() || message.get("method").is_some() {
                    let _ = pipe.send(&reply.body);
                }
            } else if let Some(id) = id {
                let data = serde_json::from_slice::<Value>(&reply.body).ok();
                let error = json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"LocalBridge permission service rejected the request","data":data}});
                let _ = pipe.send(&serde_json::to_vec(&error).expect("error envelope"));
            }
        }
        Err(_) => {
            if let Some(id) = id {
                let _ = pipe.send(&error_reply(
                    id,
                    "LocalBridge connection lost; request was not replayed",
                ));
            }
        }
        _ => {}
    }
}

fn serve_and_unregister(
    pipe: Pipe,
    port: u16,
    bearer: Arc<SecretString>,
    peers: Arc<Mutex<Vec<Pipe>>>,
) {
    serve_client(pipe.clone(), port, bearer);
    if let Ok(mut peers) = peers.lock() {
        peers.retain(|peer| !peer.same_connection(&pipe));
    }
    // Release the final server handle after unregistering. DisconnectNamedPipe
    // would discard an error response that the client has not read yet.
}

fn serve_client(mut pipe: Pipe, port: u16, bearer: Arc<SecretString>) {
    let sockets = SocketPool::default();
    let Ok(mut bytes) = pipe.receive_frame(Duration::from_secs(10)) else {
        return;
    };
    let browser = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value["browserGrant"].as_str().map(str::to_owned));
    let _grant_lease = BrowserGrantLease(browser.clone());
    if browser.is_some() {
        bytes = match pipe.receive_frame(Duration::from_secs(10)) {
            Ok(bytes) => bytes,
            Err(_) => return,
        };
    }
    let Ok((request, bytes, _)) = decode_client_frame(&bytes, browser.is_some()) else {
        pipe.close();
        return;
    };
    if request.get("method").and_then(Value::as_str) != Some("initialize")
        || request.get("id").is_none_or(Value::is_null)
    {
        let _ = pipe.send(&error_reply(
            request.get("id").cloned().unwrap_or(Value::Null),
            "initialize is required",
        ));
        return;
    }
    // Keep the protocol header free from client-controlled header injection.
    let requested = request
        .pointer("/params/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-11-25");
    if requested.len() != 10
        || !requested
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
    {
        pipe.close();
        return;
    }
    let reply = match http_scoped(
        &sockets,
        port,
        &bearer,
        "POST",
        None,
        requested,
        &bytes,
        browser.as_deref(),
    ) {
        Ok(reply) => reply,
        Err(_) => {
            forward(&pipe, Err(io::Error::other("initialize failed")), &request);
            return;
        }
    };
    let Some(session) = reply.session.clone() else {
        forward(&pipe, Ok(reply), &request);
        return;
    };
    let protocol = validate_message(&reply.body).ok().and_then(|reply| {
        reply
            .pointer("/result/protocolVersion")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let Some(protocol) = protocol else {
        forward(&pipe, Ok(reply), &request);
        let _ = http(
            &sockets,
            port,
            &bearer,
            "DELETE",
            Some(&session),
            requested,
            &[],
        );
        return;
    };
    forward(&pipe, Ok(reply), &request);
    CLIENTS.fetch_add(1, Ordering::AcqRel);
    if browser.is_some() {
        BROWSER_CLIENTS.fetch_add(1, Ordering::AcqRel);
    }
    let pending = Arc::new(AtomicUsize::new(0));
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let mut notification_clock = Instant::now();
    loop {
        if browser.as_ref().is_some_and(|token| {
            !crate::browser_connection::authority::global()
                .is_some_and(|authority| authority.valid_current(token))
        }) {
            break;
        }
        if notification_clock.elapsed() >= Duration::from_secs(1) {
            if let Ok(reply) = http(
                &sockets,
                port,
                &bearer,
                "GET",
                Some(&session),
                &protocol,
                &[],
            ) {
                if !reply.body.is_empty() {
                    let _ = pipe.send(&reply.body);
                }
            }
            notification_clock = Instant::now();
        }
        let Ok(bytes) = pipe.receive_with_tick(Duration::from_secs(1)) else {
            break;
        };
        let Some(bytes) = bytes else {
            continue;
        };
        let Ok((request, bytes, observation)) = decode_client_frame(&bytes, browser.is_some())
        else {
            break;
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if method == "initialize" {
            forward(
                &pipe,
                Ok(HttpReply {
                    body: error_reply(
                        request.get("id").cloned().unwrap_or(Value::Null),
                        "session already initialized",
                    ),
                    session: None,
                }),
                &request,
            );
            continue;
        }
        // Notifications use a dedicated route; cancellations never queue behind
        // a long-running request. They never acquire another connection's session.
        if !request
            .as_object()
            .is_some_and(|request| request.contains_key("id"))
        {
            forward(
                &pipe,
                http(
                    &sockets,
                    port,
                    &bearer,
                    "POST",
                    Some(&session),
                    &protocol,
                    &bytes,
                ),
                &request,
            );
            continue;
        }
        if pending.load(Ordering::Acquire) >= 32 {
            let _ = pipe.send(&error_reply(
                request["id"].clone(),
                "too many pending requests",
            ));
            continue;
        }
        let journal = if request["method"] == "tools/call" {
            if let Some(token) = &browser {
                let admission = observation
                    .as_ref()
                    .ok_or_else(|| io::Error::other("assistant identity required"))
                    .and_then(|observation| {
                        crate::browser_connection::authority::global()
                            .ok_or_else(|| io::Error::other("browser control unavailable"))?
                            .claim(token, observation, &request)
                    });
                match admission {
                    Ok(crate::browser_connection::authority::Claim::New(key)) => Some(key),
                    Ok(crate::browser_connection::authority::Claim::Existing(previous)) => {
                        let mut reply = previous["result"].clone();
                        if !reply.is_object()
                            || !matches!(previous["state"].as_str(), Some("succeeded" | "failed"))
                        {
                            reply = json!({"jsonrpc":"2.0","error":{"code":-32011,"message":"请求已有记录，状态不明时禁止重放","data":previous}});
                        }
                        reply["id"] = request["id"].clone();
                        let _ = pipe.send(&serde_json::to_vec(&reply).unwrap());
                        continue;
                    }
                    Err(error) => {
                        let _ = pipe.send(&error_reply(request["id"].clone(), &error.to_string()));
                        continue;
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        pending.fetch_add(1, Ordering::AcqRel);
        workers.retain(|worker| !worker.is_finished());
        let sockets = sockets.clone();
        let peer = pipe.clone();
        let credential = bearer.clone();
        let owner = session.clone();
        let version = protocol.clone();
        let pending = pending.clone();
        let browser = browser.clone();
        workers.push(thread::spawn(move || {
            let mut reply = http(
                &sockets,
                port,
                &credential,
                "POST",
                Some(&owner),
                &version,
                &bytes,
            );
            if let (Some(token), Some(key), Some(authority)) = (
                &browser,
                &journal,
                crate::browser_connection::authority::global(),
            ) {
                match reply
                    .as_ref()
                    .ok()
                    .and_then(|reply| validate_message(&reply.body).ok())
                {
                    Some(message) => {
                        if authority.finish(token, key, &message).is_err() {
                            authority.unknown(token, key);
                            reply = Ok(HttpReply { body: serde_json::to_vec(&json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32011,"message":"结果记录未完成，状态未知；禁止自动重放"}})).unwrap(), session: None });
                        }
                        if message.get("result").is_some()
                            && message.pointer("/result/isError").and_then(Value::as_bool)
                                != Some(true)
                        {
                            BROWSER_CALLS.fetch_add(1, Ordering::AcqRel);
                        }
                    }
                    None => {
                        authority.unknown(token, key);
                        reply = Ok(HttpReply { body: serde_json::to_vec(&json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32011,"message":"执行结果未知；禁止自动重放"}})).unwrap(), session: None });
                    }
                }
            }
            forward(&peer, reply, &request);
            pending.fetch_sub(1, Ordering::AcqRel);
        }));
    }
    pipe.close();
    CLIENTS.fetch_sub(1, Ordering::AcqRel);
    if let Some(token) = &browser {
        BROWSER_CLIENTS.fetch_sub(1, Ordering::AcqRel);
        if let Some(authority) = crate::browser_connection::authority::global() {
            authority.close(token);
        }
    }
    sockets.close();
    let _ = http(
        &SocketPool::default(),
        port,
        &bearer,
        "DELETE",
        Some(&session),
        &protocol,
        &[],
    );
    for worker in workers {
        let _ = worker.join();
    }
}

fn decode_client_frame(
    bytes: &[u8],
    browser: bool,
) -> io::Result<(
    Value,
    Vec<u8>,
    Option<crate::browser_connection::authority::Observation>,
)> {
    if !browser {
        return validate_message(bytes).map(|request| (request, bytes.to_vec(), None));
    }
    let envelope: Value = serde_json::from_slice(bytes).map_err(io::Error::other)?;
    let message = serde_json::to_vec(&envelope["mcp"]).map_err(io::Error::other)?;
    let request = validate_message(&message)?;
    if message.len() > crate::browser_connection::protocol::MAX_REQUEST_BYTES {
        return Err(io::Error::other("browser request too large"));
    }
    let observation = envelope
        .get("observation")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value(value.clone()).map_err(io::Error::other))
        .transpose()?;
    Ok((request, message, observation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninitialized_error_survives_server_worker_retirement() {
        let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
        let name = pipe_name(&crate::security::random_hex(32).unwrap()).unwrap();
        let listener = Pipe::listen(&name, true).unwrap();
        let peers = Arc::new(Mutex::new(Vec::new()));
        let server_peers = peers.clone();
        let expected_client = executable.clone();
        let (finished, completion) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            listener
                .accept(&expected_client, &AtomicBool::new(false))
                .unwrap();
            server_peers.lock().unwrap().push(listener.clone());
            let bearer = SecretString::new(crate::security::random_hex(32).unwrap()).unwrap();
            serve_and_unregister(listener, 0, Arc::new(bearer), server_peers);
            // No server Pipe remains when this signal reaches the client.
            finished.send(()).unwrap();
        });
        let mut client = Pipe::connect(&name, &executable).unwrap();
        let request_id = json!("uninitialized-delayed-reader");
        client
            .send(
                &serde_json::to_vec(&json!({
                    "jsonrpc":"2.0", "id":request_id, "method":"tools/list"
                }))
                .unwrap(),
            )
            .unwrap();
        completion.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(peers.lock().unwrap().is_empty());
        let bytes = client.receive_frame(Duration::from_secs(5)).unwrap();
        assert_eq!(
            validate_message(&bytes).unwrap(),
            json!({"jsonrpc":"2.0","id":request_id,
                "error":{"code":-32000,"message":"initialize is required"}})
        );
        assert!(client.receive_with_tick(Duration::from_secs(1)).is_err());
        worker.join().unwrap();
    }
}
