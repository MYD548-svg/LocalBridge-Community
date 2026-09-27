use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::server::CURRENT_PROTOCOL_VERSION;
use super::{
    CodingToolsPermissionMode, CodingToolsRuntime, CodingToolsRuntimeConfig, InternalBearer,
    PolicyEnforcementRuntime,
};
use crate::execution::policy::CapabilityPolicy;
use crate::state::PermissionMode;

const TEST_BEARER: &str = "LOCALBRIDGE_TEST_RUNTIME_BEARER_DO_NOT_LEAK";

#[derive(Debug)]
pub(crate) struct ClientResponse {
    pub(crate) status: u16,
    pub(crate) session: Option<String>,
    pub(crate) body: Value,
}

pub(crate) struct RawHttpResponse {
    pub(crate) status: u16,
    pub(crate) session: Option<String>,
    pub(crate) content_type: Option<String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn assert_tool_error(response: &ClientResponse, expected_code: &str) {
    assert_eq!(response.status, 200, "{:#?}", response.body);
    assert!(response.body.get("error").is_none(), "{:#?}", response.body);
    assert_eq!(
        response.body["result"]["isError"], true,
        "{:#?}",
        response.body
    );
    assert_eq!(
        response.body["result"]["structuredContent"]["error"]["code"], expected_code,
        "{:#?}",
        response.body
    );
}

pub(crate) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has repository parent")
        .to_path_buf()
}

pub(crate) fn temp_workspace() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "localbridge-mcp-test-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    // This content is part of the shared black-box fixture contract. Keep it
    // stable so behavior tests assert the filesystem route, not fixture drift.
    fs::write(path.join("probe.txt"), b"LB009 PEP\n").unwrap();
    path
}

pub(crate) fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.local_addr().unwrap().port()
}

pub(crate) fn cleanup_test_directory(path: &Path) {
    eprintln!("TEST_WORKSPACE_RETAINED path={}", path.display());
}

pub(crate) fn assert_eventually(
    description: &str,
    timeout: Duration,
    mut condition: impl FnMut() -> bool,
) {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return;
        }
        assert!(Instant::now() < deadline, "{description}");
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn policy(root: &Path) -> CapabilityPolicy {
    CapabilityPolicy::load(&root.join("runtime-policy.toml")).unwrap()
}

pub(crate) struct PublicRuntimeFixture {
    workspace: PathBuf,
    runtime: Option<PolicyEnforcementRuntime>,
    cleaned: bool,
}

impl PublicRuntimeFixture {
    pub(crate) fn start(permission: PermissionMode) -> Self {
        Self::start_in(temp_workspace(), permission)
    }

    pub(crate) fn start_in(workspace: PathBuf, permission: PermissionMode) -> Self {
        let root = repo_root();
        let coding = CodingToolsRuntime::start(
            CodingToolsRuntimeConfig::new(
                &root,
                &workspace,
                free_port(),
                CodingToolsPermissionMode::Trusted,
            ),
            InternalBearer::new(TEST_BEARER).unwrap(),
            Duration::from_secs(10),
        )
        .expect("bundled MCP test runtime ready");
        let runtime = PolicyEnforcementRuntime::start(coding, policy(&root), permission)
            .expect("public MCP test runtime ready");
        Self {
            workspace,
            runtime: Some(runtime),
            cleaned: false,
        }
    }

    pub(crate) fn runtime(&self) -> &PolicyEnforcementRuntime {
        self.runtime.as_ref().expect("test runtime is active")
    }

    pub(crate) fn shutdown(mut self) {
        let runtime = self.runtime.take().expect("test runtime is active");
        let mut coding = runtime.stop().expect("public MCP test runtime stops");
        coding.stop().expect("bundled MCP test runtime stops");
        assert_eq!(
            coding.active_processes().unwrap(),
            0,
            "test fixture leaked a managed process"
        );
        cleanup_test_directory(&self.workspace);
        self.cleaned = true;
    }

    fn stop_best_effort(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            if let Ok(mut coding) = runtime.stop() {
                let _ = coding.stop();
            }
        }
    }
}

impl Drop for PublicRuntimeFixture {
    fn drop(&mut self) {
        self.stop_best_effort();
        if !self.cleaned {
            cleanup_test_directory(&self.workspace);
        }
    }
}

pub(crate) fn post(port: u16, session: Option<&str>, payload: &Value) -> ClientResponse {
    // Process-backed requests may include cold process creation and antivirus
    // scanning. The socket budget is intentionally larger than command budgets;
    // lifecycle deadlines belong to the command driver below.
    post_with_read_timeout(port, session, payload, Duration::from_secs(30))
}

pub(crate) fn post_with_read_timeout(
    port: u16,
    session: Option<&str>,
    payload: &Value,
    read_timeout: Duration,
) -> ClientResponse {
    try_post_with_read_timeout(port, session, payload, read_timeout, || {})
        .unwrap_or_else(|error| panic!("test HTTP request: {error:?}"))
}

#[derive(Debug)]
pub(crate) struct TestHttpError {
    pub(crate) stage: &'static str,
    pub(crate) kind: std::io::ErrorKind,
    pub(crate) received_bytes: usize,
}

pub(crate) fn try_post_with_read_timeout(
    port: u16,
    session: Option<&str>,
    payload: &Value,
    read_timeout: Duration,
    sent: impl FnOnce(),
) -> Result<ClientResponse, TestHttpError> {
    let io_error = |stage, error: std::io::Error| TestHttpError {
        stage,
        kind: error.kind(),
        received_bytes: 0,
    };
    let body = serde_json::to_vec(payload).expect("JSON value serializes");
    let address = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| io_error("connect", error))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|error| io_error("configure", error))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| io_error("configure", error))?;
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json, text/event-stream\r\nContent-Type: application/json\r\nMCP-Protocol-Version: {CURRENT_PROTOCOL_VERSION}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if let Some(session) = session {
        request.push_str("Mcp-Session-Id: ");
        request.push_str(session);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| io_error("write", error))?;
    stream
        .write_all(&body)
        .map_err(|error| io_error("write", error))?;
    stream.flush().map_err(|error| io_error("write", error))?;
    sent();
    let mut bytes = Vec::new();
    let deadline = Instant::now() + read_timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(TestHttpError {
                stage: "read",
                kind: std::io::ErrorKind::TimedOut,
                received_bytes: bytes.len(),
            });
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| io_error("configure", error))?;
        let mut chunk = [0_u8; 8192];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
            Err(error)
                if error.kind() == std::io::ErrorKind::ConnectionReset
                    && response_content_length_is_complete(&bytes) =>
            {
                break;
            }
            Err(error) => {
                return Err(TestHttpError {
                    stage: "read",
                    kind: error.kind(),
                    received_bytes: bytes.len(),
                });
            }
        }
    }
    parse_client_bytes(&bytes)
}

pub(crate) fn delete(port: u16, session: &str) -> u16 {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let request = format!(
        "DELETE /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nMcp-Session-Id: {session}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    parse_client_response(stream).status
}

pub(crate) fn get_sse(port: u16, session: &str) -> RawHttpResponse {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let request = format!(
        "GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: text/event-stream\r\nMCP-Protocol-Version: {CURRENT_PROTOCOL_VERSION}\r\nMcp-Session-Id: {session}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    parse_raw_http_response(stream)
}

fn parse_raw_http_response(mut stream: TcpStream) -> RawHttpResponse {
    let bytes = read_complete_http_response(&mut stream);
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let headers = std::str::from_utf8(&bytes[..split]).unwrap();
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let mut session = None;
    let mut content_type = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Mcp-Session-Id") {
                session = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("Content-Type") {
                content_type = Some(value.trim().to_string());
            }
        }
    }
    RawHttpResponse {
        status,
        session,
        content_type,
        body: bytes[(split + 4)..].to_vec(),
    }
}

pub(crate) fn parse_client_response(mut stream: TcpStream) -> ClientResponse {
    let bytes = read_complete_http_response(&mut stream);
    parse_client_bytes(&bytes).unwrap_or_else(|error| panic!("test HTTP response: {error:?}"))
}

fn parse_client_bytes(bytes: &[u8]) -> Result<ClientResponse, TestHttpError> {
    let invalid = || TestHttpError {
        stage: "parse",
        kind: std::io::ErrorKind::InvalidData,
        received_bytes: bytes.len(),
    };
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(invalid)?;
    let headers = std::str::from_utf8(&bytes[..split]).map_err(|_| invalid())?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .ok_or_else(invalid)?
        .split_whitespace()
        .nth(1)
        .ok_or_else(invalid)?
        .parse::<u16>()
        .map_err(|_| invalid())?;
    let session = lines.find_map(|line| {
        line.split_once(':').and_then(|(name, value)| {
            name.eq_ignore_ascii_case("Mcp-Session-Id")
                .then(|| value.trim().to_string())
        })
    });
    let body_bytes = &bytes[(split + 4)..];
    let body = if body_bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(body_bytes).map_err(|_| invalid())?
    };
    Ok(ClientResponse {
        status,
        session,
        body,
    })
}

fn read_complete_http_response(stream: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    match stream.read_to_end(&mut bytes) {
        Ok(_) => bytes,
        Err(error)
            if error.kind() == std::io::ErrorKind::ConnectionReset
                && response_content_length_is_complete(&bytes) =>
        {
            bytes
        }
        Err(error) => panic!("read complete test HTTP response: {error}"),
    }
}

fn response_content_length_is_complete(bytes: &[u8]) -> bool {
    let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let Ok(headers) = std::str::from_utf8(&bytes[..header_end]) else {
        return false;
    };
    let content_length = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("Content-Length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    content_length.is_some_and(|length| bytes.len() >= header_end + 4 + length)
}

#[test]
fn structured_http_errors_distinguish_parse_failures_from_timeouts() {
    let malformed = b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n{";
    let error = parse_client_bytes(malformed).unwrap_err();
    assert_eq!(error.stage, "parse");
    assert_eq!(error.kind, std::io::ErrorKind::InvalidData);
    assert_eq!(error.received_bytes, malformed.len());
    let empty = parse_client_bytes(b"").unwrap_err();
    assert_eq!(empty.stage, "parse");
    assert_eq!(empty.received_bytes, 0);
    let valid = parse_client_bytes(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
    assert_eq!(valid.status, 200);
    assert_eq!(valid.body, json!({}));
}

#[test]
fn reset_tolerance_requires_a_complete_content_length_body() {
    assert!(response_content_length_is_complete(
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"
    ));
    assert!(!response_content_length_is_complete(
        b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\n{}"
    ));
    assert!(!response_content_length_is_complete(
        b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}"
    ));
}

pub(crate) fn initialize(port: u16, id: u64) -> ClientResponse {
    post(
        port,
        None,
        &json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"initialize",
            "params":{
                "protocolVersion":CURRENT_PROTOCOL_VERSION,
                "capabilities":{},
                "clientInfo":{"name":"localbridge-test-client","version":"1"}
            }
        }),
    )
}

pub(crate) fn public_tool_call(
    port: u16,
    session: &str,
    id: u64,
    name: &str,
    arguments: Value,
) -> ClientResponse {
    post(
        port,
        Some(session),
        &json!({
            "jsonrpc":"2.0",
            "id":id,
            "method":"tools/call",
            "params":{"name":name,"arguments":arguments}
        }),
    )
}

#[derive(Debug)]
pub(crate) struct PublicMcpClient {
    port: u16,
    session: String,
    next_request_id: AtomicU64,
}

impl PublicMcpClient {
    pub(crate) fn connect(port: u16, first_request_id: u64) -> (Self, ClientResponse) {
        let initialized = initialize(port, first_request_id);
        let session = initialized
            .session
            .clone()
            .expect("initialize returns an MCP-session-scoped identity");
        (
            Self {
                port,
                session,
                next_request_id: AtomicU64::new(first_request_id.saturating_add(1)),
            },
            initialized,
        )
    }

    pub(crate) fn call_tool(&self, name: &str, arguments: Value) -> ClientResponse {
        public_tool_call(
            self.port,
            &self.session,
            self.next_request_id.fetch_add(1, Ordering::Relaxed),
            name,
            arguments,
        )
    }

    pub(crate) fn start_detached_command(&self, arguments: Value) -> DetachedCommand<'_> {
        let deadline = Instant::now() + Duration::from_secs(60);
        let response = loop {
            let response = self.call_tool("exec_command", arguments.clone());
            let content = &response.body["result"]["structuredContent"];
            if content["data"]["status"].as_str() == Some("running") {
                break response;
            }
            if content["error"]["code"] == "OperationTimedOut" {
                assert!(
                    Instant::now() < deadline,
                    "detached command submission kept timing out: {:#?}",
                    response.body
                );
                continue;
            }
            // Anything else (a delivered terminal status or a typed error) is
            // a real fact about this submission; from_response asserts running
            // and dumps the response for diagnosis.
            break response;
        };
        DetachedCommand::from_response(self, response)
    }
}

pub(crate) struct DetachedCommand<'a> {
    client: &'a PublicMcpClient,
    session_id: String,
    output: String,
    last_response: ClientResponse,
}

impl<'a> DetachedCommand<'a> {
    fn from_response(client: &'a PublicMcpClient, response: ClientResponse) -> Self {
        let data = &response.body["result"]["structuredContent"]["data"];
        assert_eq!(data["status"], "running", "{:#?}", response.body);
        let session_id = data["session_id"]
            .as_str()
            .expect("running command has PublicSessionId")
            .to_string();
        let output = data["output"].as_str().unwrap_or_default().to_string();
        Self {
            client,
            session_id,
            output,
            last_response: response,
        }
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn output(&self) -> &str {
        &self.output
    }

    pub(crate) fn poll(&mut self, wait_ms: u64) -> &ClientResponse {
        let response = self.client.call_tool(
            "command_control",
            json!({"action":"poll","session_id":self.session_id,"wait_ms":wait_ms}),
        );
        self.observe(response)
    }

    pub(crate) fn wait_for_output(&mut self, marker: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.output.contains(marker) {
            assert!(
                Instant::now() < deadline,
                "detached command did not emit {marker:?}; output={:?}; response={:#?}",
                self.output,
                self.last_response.body
            );
            self.poll(1_000);
            if self.output.contains(marker) {
                break;
            }
            match classify_command_poll_response(&self.last_response) {
                CommandPollObservation::Running | CommandPollObservation::BoundedWaitExpired => {}
                CommandPollObservation::Terminal => panic!(
                    "command terminated before emitting {marker:?}: {:#?}",
                    self.last_response.body
                ),
                CommandPollObservation::Invalid => panic!(
                    "detached command poll response is neither lifecycle status nor bounded timeout: {:#?}",
                    self.last_response.body
                ),
            }
        }
    }

    pub(crate) fn assert_next_poll_empty(&mut self) {
        let before = self.output.len();
        self.poll(0);
        assert_eq!(
            self.output.len(),
            before,
            "poll replayed previously observed output: {:#?}",
            self.last_response.body
        );
    }

    pub(crate) fn write(&mut self, chars: &str, wait_ms: u64) -> &ClientResponse {
        let response = self.client.call_tool(
            "command_control",
            json!({
                "action":"write",
                "session_id":self.session_id,
                "chars":chars,
                "wait_ms":wait_ms
            }),
        );
        self.observe(response)
    }

    pub(crate) fn kill(&mut self, signal: &str, wait_ms: u64) -> &ClientResponse {
        let response = self.client.call_tool(
            "command_control",
            json!({
                "action":"kill",
                "session_id":self.session_id,
                "signal":signal,
                "wait_ms":wait_ms
            }),
        );
        self.observe(response)
    }

    fn observe(&mut self, response: ClientResponse) -> &ClientResponse {
        self.output.push_str(
            response.body["result"]["structuredContent"]["data"]["output"]
                .as_str()
                .unwrap_or_default(),
        );
        self.last_response = response;
        &self.last_response
    }
}

/// A command_control response carries either a lifecycle observation in
/// `data.status` or a typed facade error. The facade keeps the Execution
/// non-terminal when a poll/write hits its wait budget and answers
/// `OperationTimedOut` (retryable, transport phase), so that response is a
/// "keep polling the same session" signal, never a terminal fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandPollObservation {
    Running,
    BoundedWaitExpired,
    Terminal,
    Invalid,
}

pub(crate) fn classify_command_poll_response(response: &ClientResponse) -> CommandPollObservation {
    let content = &response.body["result"]["structuredContent"];
    if let Some(status) = content["data"]["status"].as_str() {
        return if status == "running" {
            CommandPollObservation::Running
        } else {
            CommandPollObservation::Terminal
        };
    }
    if content["error"]["code"] == "OperationTimedOut" {
        return CommandPollObservation::BoundedWaitExpired;
    }
    CommandPollObservation::Invalid
}

#[test]
fn command_poll_classification_matches_the_facade_response_contract() {
    let observation = |structured: Value| ClientResponse {
        status: 200,
        session: None,
        body: json!({"result": {"structuredContent": structured}}),
    };
    assert_eq!(
        classify_command_poll_response(&observation(json!({
            "ok": true,
            "data": {"status": "running", "session_id": "lb-session-x", "output": "chunk"}
        }))),
        CommandPollObservation::Running
    );
    assert_eq!(
        classify_command_poll_response(&observation(json!({
            "ok": true,
            "data": {"status": "completed", "output": "done"}
        }))),
        CommandPollObservation::Terminal
    );
    // A failed terminal still carries data.status; the error object does not
    // turn it into a pending observation.
    assert_eq!(
        classify_command_poll_response(&observation(json!({
            "ok": false,
            "data": {"status": "failed"},
            "error": {"code": "ProcessFailed"}
        }))),
        CommandPollObservation::Terminal
    );
    // The bounded-wait response has no data.status at all.
    assert_eq!(
        classify_command_poll_response(&observation(json!({
            "ok": false,
            "data": null,
            "error": {
                "code": "OperationTimedOut",
                "cause": "operation_timed_out",
                "phase": "transport",
                "retryable": true
            }
        }))),
        CommandPollObservation::BoundedWaitExpired
    );
    assert_eq!(
        classify_command_poll_response(&observation(json!({
            "ok": false,
            "data": null,
            "error": {"code": "SessionUnavailable"}
        }))),
        CommandPollObservation::Invalid
    );
    assert_eq!(
        classify_command_poll_response(&observation(json!({}))),
        CommandPollObservation::Invalid
    );
}

/// Submit a detached exec_command and return the first response that carries a
/// lifecycle observation.
///
/// Resubmission safety scope: a submission whose wait budget expires does NOT
/// prove the command was not executed — the facade's private call timeout does
/// not cancel the upstream request, so the first attempt may still run to its
/// natural end as an orphaned execution whose public session was terminalized
/// with the error. Resubmitting is therefore only justified for
/// observation-only scenario commands (sleep/echo) whose duplicate cannot
/// corrupt the workspace or the assertions; a command with persistent side
/// effects must not use this helper.
pub(crate) fn submit_side_effect_free_public_command(
    port: u16,
    session: &str,
    request_id: u64,
    arguments: Value,
) -> ClientResponse {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let response =
            public_tool_call(port, session, request_id, "exec_command", arguments.clone());
        let content = &response.body["result"]["structuredContent"];
        if content["data"]["status"].is_string() {
            return response;
        }
        if content["error"]["code"] == "OperationTimedOut" {
            assert!(
                Instant::now() < deadline,
                "exec_command submission kept timing out: {:#?}",
                response.body
            );
            continue;
        }
        panic!(
            "exec_command submission returned neither lifecycle status nor bounded timeout: {:#?}",
            response.body
        );
    }
}

pub(crate) fn settle_public_command(
    port: u16,
    session: &str,
    mut poll_id: u64,
    mut response: ClientResponse,
) -> (ClientResponse, String) {
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut output = String::new();
    let mut public_session = String::new();
    loop {
        let data = &response.body["result"]["structuredContent"]["data"];
        output.push_str(data["output"].as_str().unwrap_or_default());
        match classify_command_poll_response(&response) {
            CommandPollObservation::Running => {
                public_session = data["session_id"]
                    .as_str()
                    .expect("running command has PublicSessionId")
                    .to_string();
            }
            CommandPollObservation::Terminal => return (response, output),
            CommandPollObservation::BoundedWaitExpired if public_session.is_empty() => panic!(
                "public command transport timeout before the session identity was delivered: {:#?}",
                response.body
            ),
            CommandPollObservation::BoundedWaitExpired => {}
            CommandPollObservation::Invalid => panic!(
                "public command response is neither lifecycle status nor bounded timeout: {:#?}",
                response.body
            ),
        }
        assert!(
            Instant::now() < deadline,
            "public command did not converge: {:#?}",
            response.body
        );
        response = public_tool_call(
            port,
            session,
            poll_id,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":1000}),
        );
        poll_id = poll_id.saturating_add(1);
    }
}

pub(crate) fn poll_public_command_to_terminal(
    port: u16,
    session: &str,
    mut poll_id: u64,
    public_session: &str,
    timeout: Duration,
) -> ClientResponse {
    let deadline = Instant::now() + timeout;
    loop {
        let response = public_tool_call(
            port,
            session,
            poll_id,
            "command_control",
            json!({"action":"poll","session_id":public_session,"wait_ms":1_000}),
        );
        poll_id = poll_id.saturating_add(1);
        match classify_command_poll_response(&response) {
            CommandPollObservation::Running | CommandPollObservation::BoundedWaitExpired => {}
            CommandPollObservation::Terminal => return response,
            // No transient SessionUnavailable tolerance here: the production
            // poll path replays the durable terminal when a concurrent control
            // call finalizes the same Execution, and a genuinely unavailable
            // or unknown session must surface as the contract error.
            CommandPollObservation::Invalid => panic!(
                "public command returned neither lifecycle status nor bounded timeout: {:#?}",
                response.body
            ),
        }
        assert!(
            Instant::now() < deadline,
            "public command did not reach terminal state: {:#?}",
            response.body
        );
    }
}
