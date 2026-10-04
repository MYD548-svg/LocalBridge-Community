//! Tests enter the actual named pipe and existing permission service. Fixtures
//! are retained; no installed client configuration or old binaries are changed.
use super::{
    codec,
    pipe::{Pipe, pipe_name},
    profile::installation_id,
    runtime::LocalRuntime,
};
use crate::mcp::test_support::{
    CommandPollObservation, PublicRuntimeFixture, classify_command_poll_body,
};
use crate::state::PermissionMode;
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

fn client(executable: &Path) -> Pipe {
    let name = pipe_name(&installation_id(executable.parent().unwrap()).unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match Pipe::connect(&name, executable) {
            Ok(pipe) => return pipe,
            Err(error) => {
                assert!(Instant::now() < deadline, "connect: {error}");
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
fn send(pipe: &Pipe, value: &Value) {
    pipe.send(&serde_json::to_vec(value).unwrap()).unwrap();
}
fn response(pipe: &mut Pipe, id: Value) -> Value {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(Instant::now() < deadline, "request {id} did not settle");
        if let Some(bytes) = pipe
            .receive_with_tick(Duration::from_millis(100))
            .unwrap_or_else(|error| panic!("response stage=request {id}: {error}"))
        {
            let value = codec::validate_message(&bytes).unwrap();
            if value.get("id") == Some(&id) {
                return value;
            }
            assert!(value.get("method").is_some(), "unmatched response: {value}");
        }
    }
}
fn initialize(pipe: &mut Pipe) {
    send(
        pipe,
        &json!({"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"local-pipe-test","version":"1"}}}),
    );
    assert_eq!(
        response(pipe, json!("init"))["result"]["protocolVersion"],
        "2025-11-25"
    );
    send(
        pipe,
        &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
}
fn tool(pipe: &mut Pipe, id: &str, name: &str, arguments: Value) -> Value {
    send(
        pipe,
        &json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
    );
    response(pipe, json!(id))
}

fn settle_command(pipe: &mut Pipe, iteration: usize, mut current: Value) -> Value {
    let session = current["result"]["structuredContent"]["data"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("stage=start iteration={iteration}: {current}"))
        .to_owned();
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let observed_session = &current["result"]["structuredContent"]["data"]["session_id"];
        if !observed_session.is_null() {
            assert_eq!(
                observed_session.as_str(),
                Some(session.as_str()),
                "stage=poll iteration={iteration} session changed: {current}"
            );
        }
        match classify_command_poll_body(&current) {
            CommandPollObservation::Terminal => return current,
            CommandPollObservation::Running | CommandPollObservation::BoundedWaitExpired => {}
            CommandPollObservation::Invalid => {
                panic!("stage=poll iteration={iteration} unknown command response: {current}")
            }
        }
        assert!(
            Instant::now() < deadline,
            "stage=poll iteration={iteration} terminal observation expired: {current}"
        );
        current = tool(
            pipe,
            &format!("poll-{iteration}"),
            "command_control",
            json!({"action":"poll","session_id":session,"wait_ms":1000}),
        );
    }
}

fn read_stderr(pipe: &mut Pipe, iteration: usize, reference: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let read = tool(
            pipe,
            &format!("read-{iteration}"),
            "command_control",
            json!({"action":"read","output_ref":reference,"stream":"stderr","limit":1048576}),
        );
        if read["result"]["structuredContent"]["data"]["content"].is_string()
            && read["result"]["isError"] == false
        {
            return read;
        }
        let error = &read["result"]["structuredContent"]["error"];
        // Match the shared retained-stderr read regression's typed busy errors.
        // Only retry this read, keeping the same reference and owner connection.
        assert!(
            classify_command_poll_body(&read) == CommandPollObservation::BoundedWaitExpired
                || (error["code"] == "RuntimeUnavailable" && error["retryable"] == true),
            "stage=read iteration={iteration} unexpected output response: {read}"
        );
        assert!(
            Instant::now() < deadline,
            "stage=read iteration={iteration} bounded read retries expired: {read}"
        );
    }
}

#[test]
fn simultaneous_clients_initialize_and_call_without_retrying_requests() {
    let fixture = PublicRuntimeFixture::start_authenticated(PermissionMode::Edit);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut runtime = LocalRuntime::start(
        &executable,
        fixture.runtime().port(),
        fixture
            .runtime()
            .local_connector_bearer()
            .expect("authenticated fixture provides a connector bearer"),
    )
    .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let barrier = barrier.clone();
        let expected = executable.clone();
        workers.push(thread::spawn(move || {
            let name = pipe_name(&installation_id(expected.parent().unwrap()).unwrap()).unwrap();
            barrier.wait();
            // Unlike client(), no test wrapper retries connection failures.
            let mut pipe = Pipe::connect(&name, &expected).unwrap();
            initialize(&mut pipe);
            send(
                &pipe,
                &json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}),
            );
            let reply = response(&mut pipe, json!(0));
            assert!(reply["result"]["tools"].is_array(), "{reply}");
            pipe.close();
            reply
        }));
    }
    barrier.wait();
    let replies: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(replies[0], replies[1]);
    runtime.stop();
    fixture.shutdown();
}

#[test]
fn output_read_local_pipe_preserves_owned_output_and_errors_with_the_facade_locked() {
    let fixture = PublicRuntimeFixture::start_authenticated(PermissionMode::Full);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut runtime = LocalRuntime::start(
        &executable,
        fixture.runtime().port(),
        fixture.runtime().local_connector_bearer().unwrap(),
    )
    .unwrap();
    let mut owner = client(&executable);
    initialize(&mut owner);
    let mut other = client(&executable);
    initialize(&mut other);
    let initial = tool(
        &mut owner,
        "output-read-start",
        "exec_command",
        json!({
            "command":"Write-Output LB_PIPE_LOCKED_STDOUT; Write-Error LB_PIPE_LOCKED_STDERR",
            "shell":"windows_powershell","yield_time_ms":0,"timeout_ms":120000
        }),
    );
    let terminal = settle_command(&mut owner, 71_000, initial);
    assert_eq!(
        terminal["result"]["structuredContent"]["error"]["code"],
        "ProcessFailed"
    );
    let data = &terminal["result"]["structuredContent"]["data"];
    let public = data["session_id"].as_str().unwrap();
    let stdout = data["output_refs"]["stdout"].as_str().unwrap().to_owned();
    let stderr = data["output_refs"]["stderr"].as_str().unwrap().to_owned();
    fixture.runtime().with_output_read_guard_held(|executions| {
        let owner_id = executions
            .execution_for_public_session(&crate::domain::PublicSessionId::new(public))
            .unwrap()
            .owner_session
            .unwrap();
        let outputs = executions.output_handles();
        let expired = outputs.retain_local(owner_id.clone(), "stdout", "old".into());
        for _ in 0..crate::execution::output_handles::MAX_LOCAL_RETAINED_OUTPUT_HANDLES {
            outputs.retain_local(owner_id.clone(), "stdout", "filler".into());
        }
        let local = outputs.retain_local(owner_id, "stdout", "甲🙂尾".into());
        for reference in [&stdout, &stderr, &local] {
            let denied = tool(
                &mut other,
                "output-read-same-id",
                "command_control",
                json!({"action":"read","output_ref":reference,"stream":"stderr"}),
            );
            assert_eq!(denied["id"], "output-read-same-id");
            assert_eq!(
                denied["result"]["structuredContent"]["error"]["code"], "OutputNotFound",
                "{denied}"
            );
        }
        for reference in ["lb-output-unknown", expired.as_str()] {
            let denied = tool(
                &mut owner,
                "output-read-same-id",
                "command_control",
                json!({"action":"read","output_ref":reference}),
            );
            assert_eq!(
                denied["result"]["structuredContent"]["error"]["code"],
                "OutputNotFound"
            );
        }
        let mismatch = tool(
            &mut owner,
            "output-read-mismatch",
            "command_control",
            json!({"action":"read","output_ref":stdout,"stream":"stderr"}),
        );
        let error = &mismatch["result"]["structuredContent"]["error"];
        assert_eq!(error["code"], "InvalidArgument");
        assert_eq!(error["details"]["expected"], "stdout");
        assert_eq!(error["details"]["actual"], "stderr");
        for (stream, reference, marker) in [
            ("stdout", &stdout, "LB_PIPE_LOCKED_STDOUT"),
            ("stderr", &stderr, "LB_PIPE_LOCKED_STDERR"),
        ] {
            let first = tool(
                &mut owner,
                "output-read-same-id",
                "command_control",
                json!({"action":"read","output_ref":reference,"stream":stream,"limit":1048576}),
            );
            assert_eq!(first["id"], "output-read-same-id");
            assert_eq!(first["result"]["isError"], false, "{first}");
            let page = &first["result"]["structuredContent"]["data"];
            assert_eq!(page["output_ref"], reference.as_str());
            assert!(page["content"].as_str().unwrap().contains(marker));
            let second = tool(
                &mut owner,
                "output-read-again",
                "command_control",
                json!({"action":"read","output_ref":reference,"stream":stream,"limit":1048576}),
            );
            assert_eq!(
                second["result"]["structuredContent"],
                first["result"]["structuredContent"]
            );
        }
        let page = tool(
            &mut owner,
            "output-read-unicode",
            "command_control",
            json!({"action":"read","output_ref":local,"offset":3,"limit":1}),
        );
        assert_eq!(page["result"]["structuredContent"]["data"]["content"], "🙂");
        assert_eq!(
            page["result"]["structuredContent"]["data"]["next_offset"],
            7
        );
        let invalid = tool(
            &mut owner,
            "output-read-unaligned",
            "command_control",
            json!({"action":"read","output_ref":local,"offset":1}),
        );
        assert_eq!(
            invalid["result"]["structuredContent"]["error"]["code"],
            "InvalidArgument"
        );
    });
    owner.close();
    other.close();
    runtime.stop();
    assert_eq!(super::runtime::connected_clients(), 0);
    fixture.shutdown();
}

#[test]
fn local_route_retains_failed_stderr_and_enforces_session_ownership_ten_times() {
    let fixture = PublicRuntimeFixture::start_authenticated(PermissionMode::Full);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut runtime = LocalRuntime::start(
        &executable,
        fixture.runtime().port(),
        fixture
            .runtime()
            .local_connector_bearer()
            .expect("authenticated local transport fixture provides a connector bearer"),
    )
    .unwrap();
    let mut owner = client(&executable);
    initialize(&mut owner);
    let mut other = client(&executable);
    initialize(&mut other);
    for iteration in 0..10 {
        eprintln!("LOCAL_PIPE stage=start iteration={iteration}");
        let marker = format!("LOCAL_RETAINED_STDERR_{iteration}");
        let started = tool(
            &mut owner,
            "same-id",
            "exec_command",
            json!({"command":format!("Write-Error {marker}"),"shell":"windows_powershell","yield_time_ms":0,"timeout_ms":120000}),
        );
        let session = started["result"]["structuredContent"]["data"]["session_id"]
            .as_str()
            .unwrap_or_else(|| panic!("stage=start iteration={iteration}: {started}"))
            .to_owned();
        let terminal = settle_command(&mut owner, iteration, started);
        eprintln!("LOCAL_PIPE stage=terminal iteration={iteration} response={terminal}");
        assert_eq!(terminal["result"]["isError"], true, "{terminal}");
        assert_eq!(
            terminal["result"]["structuredContent"]["error"]["code"], "ProcessFailed",
            "stage=terminal iteration={iteration}: {terminal}"
        );
        let stderr = terminal["result"]["structuredContent"]["data"]["output_refs"]["stderr"]
            .as_str()
            .unwrap_or_else(|| panic!("stage=stderr-ref iteration={iteration}: {terminal}"));
        let replay = tool(
            &mut owner,
            "replay",
            "command_control",
            json!({"action":"poll","session_id":session,"wait_ms":0}),
        );
        assert_eq!(
            replay["result"]["structuredContent"]["data"]["output_refs"]["stderr"], stderr,
            "stage=replay iteration={iteration} terminal={terminal} replay={replay}"
        );
        assert_eq!(
            replay["result"]["structuredContent"]["error"]["code"], "ProcessFailed",
            "stage=replay iteration={iteration}: {replay}"
        );
        let read = read_stderr(&mut owner, iteration, stderr);
        assert!(
            read["result"]["structuredContent"]["data"]["content"]
                .as_str()
                .unwrap()
                .contains(&marker),
            "stage=marker iteration={iteration} terminal={terminal} read={read}"
        );
        let denied = tool(
            &mut other,
            "same-id",
            "command_control",
            json!({"action":"read","output_ref":stderr,"stream":"stderr"}),
        );
        assert_eq!(denied["result"]["isError"], true, "{denied}");
        assert_eq!(
            denied["result"]["structuredContent"]["error"]["code"],
            "OutputNotFound"
        );
    }
    owner.close();
    other.close();
    runtime.stop();
    assert!(!runtime.is_running());
    fixture.shutdown();
}

#[test]
fn initialization_concurrency_errors_and_manual_stop_use_real_local_transport() {
    let fixture = PublicRuntimeFixture::start_authenticated(PermissionMode::Edit);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut runtime = LocalRuntime::start(
        &executable,
        fixture.runtime().port(),
        fixture
            .runtime()
            .local_connector_bearer()
            .expect("authenticated local transport fixture provides a connector bearer"),
    )
    .unwrap();
    let mut premature = client(&executable);
    send(
        &premature,
        &json!({"jsonrpc":"2.0","id":0,"method":"tools/list"}),
    );
    assert!(response(&mut premature, json!(0)).get("error").is_some());
    premature.close();
    let mut first = client(&executable);
    initialize(&mut first);
    let mut second = client(&executable);
    initialize(&mut second);
    let request = json!({"jsonrpc":"2.0","id":0,"method":"tools/list"});
    send(&first, &request);
    send(&second, &request);
    assert_eq!(
        response(&mut first, json!(0)),
        response(&mut second, json!(0))
    );
    send(
        &first,
        &json!({"jsonrpc":"2.0","id":"unknown","method":"unknown"}),
    );
    assert_eq!(
        response(&mut first, json!("unknown"))["error"]["code"],
        -32601
    );
    send(
        &first,
        &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"missing"}}),
    );
    send(&first, &request);
    assert!(response(&mut first, json!(0))["result"]["tools"].is_array());
    let denied = tool(
        &mut first,
        "permission",
        "exec_command",
        json!({"command":"echo must-not-run","shell":"cmd"}),
    );
    assert_eq!(
        denied["result"]["structuredContent"]["error"]["code"],
        "PolicyDenied"
    );
    let outside = tool(
        &mut first,
        "outside",
        "agent_workflow",
        json!({"action":"document","directory_changes":[{"action":"create_directory","path":"../escape"}]}),
    );
    assert_eq!(
        outside["result"]["structuredContent"]["error"]["code"],
        "WorkspaceDenied"
    );
    let mut incomplete = client(&executable);
    incomplete.write_all(&[10, 0, 0, 0, 1]).unwrap();
    assert!(
        incomplete
            .receive_with_tick(Duration::from_secs(15))
            .is_err()
    );
    let mut malformed = client(&executable);
    malformed
        .write_all(&(codec::MAX_FRAME_BYTES as u32 + 1).to_le_bytes())
        .unwrap();
    assert!(malformed.receive_with_tick(Duration::from_secs(5)).is_err());
    runtime.stop();
    assert!(first.receive_with_tick(Duration::from_secs(2)).is_err());
    let name = pipe_name(&installation_id(executable.parent().unwrap()).unwrap()).unwrap();
    assert!(
        Pipe::connect(&name, &executable).is_err(),
        "stopped service must not restart"
    );
    fixture.shutdown();
}

#[test]
fn cancellation_timeout_and_disconnect_never_replay_a_side_effect() {
    let workspace = crate::mcp::test_support::temp_workspace();
    let fixture =
        PublicRuntimeFixture::start_authenticated_in(workspace.clone(), PermissionMode::Full);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut runtime = LocalRuntime::start(
        &executable,
        fixture.runtime().port(),
        fixture
            .runtime()
            .local_connector_bearer()
            .expect("authenticated local transport fixture provides a connector bearer"),
    )
    .unwrap();
    let mut owner = client(&executable);
    initialize(&mut owner);
    let mut other = client(&executable);
    initialize(&mut other);
    send(
        &owner,
        &json!({"jsonrpc":"2.0","id":"cancel-owner","method":"tools/call","params":{"name":"exec_command","arguments":{"command":"Start-Sleep -Seconds 120","shell":"windows_powershell","yield_time_ms":10000,"timeout_ms":180000}}}),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while fixture.runtime().active_task_summaries().is_empty() {
        assert!(
            Instant::now() < deadline,
            "stage=cancel-owner activation expired"
        );
        thread::sleep(Duration::from_millis(20));
    }
    send(
        &other,
        &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"cancel-owner"}}),
    );
    // Notifications are forwarded synchronously on this connection. A later
    // tools/list response is a barrier for processing the foreign cancellation.
    send(
        &other,
        &json!({"jsonrpc":"2.0","id":"cancel-barrier","method":"tools/list"}),
    );
    let barrier = response(&mut other, json!("cancel-barrier"));
    assert!(
        barrier["result"]["tools"].is_array(),
        "stage=cancel-barrier: {barrier}"
    );
    assert!(
        !fixture.runtime().active_task_summaries().is_empty(),
        "another session cancelled the owner request"
    );
    send(
        &owner,
        &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"cancel-owner"}}),
    );
    let cancelled = response(&mut owner, json!("cancel-owner"));
    assert_eq!(
        cancelled["result"]["structuredContent"]["error"]["code"], "ProcessCancelled",
        "{cancelled}"
    );
    let timed = tool(
        &mut owner,
        "timeout",
        "exec_command",
        json!({"command":"Start-Sleep -Seconds 10","shell":"windows_powershell","yield_time_ms":10000,"timeout_ms":300}),
    );
    assert_eq!(
        timed["result"]["structuredContent"]["error"]["code"], "ProcessTimedOut",
        "{timed}"
    );
    send(
        &owner,
        &json!({"jsonrpc":"2.0","id":"abandon","method":"tools/call","params":{"name":"exec_command","arguments":{"command":"Add-Content -LiteralPath 'executions.txt' -Value 'once'; Start-Sleep -Seconds 120","shell":"windows_powershell","yield_time_ms":10000,"timeout_ms":180000}}}),
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut started = Value::Null;
    let side_effect = workspace.join("executions.txt");
    loop {
        if let Some(bytes) = owner
            .receive_with_tick(Duration::from_millis(20))
            .unwrap_or_else(|error| {
                panic!("stage=side-effect response: {error}; started={started}")
            })
        {
            let received = codec::validate_message(&bytes).unwrap();
            if received.get("id") == Some(&json!("abandon")) {
                assert_eq!(
                    classify_command_poll_body(&received),
                    CommandPollObservation::Running,
                    "stage=side-effect unexpected start status: {received}"
                );
                assert_eq!(
                    received["result"]["isError"], false,
                    "stage=side-effect start failed: {received}"
                );
                started = received;
            } else {
                assert!(
                    received.get("method").is_some(),
                    "stage=side-effect unmatched response: {received}"
                );
            }
        }
        let contents = std::fs::read_to_string(&side_effect);
        if let Ok(contents) = &contents {
            assert!(
                contents.lines().filter(|line| *line == "once").count() <= 1,
                "stage=side-effect duplicate execution; started={started}; file={contents:?}"
            );
            if contents.lines().collect::<Vec<_>>() == ["once"] && !started.is_null() {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "stage=side-effect file deadline; started={started}; file={contents:?}; active={:?}",
            fixture.runtime().active_task_summaries()
        );
    }
    owner.close();
    other.close();
    drop(owner);
    drop(other);
    let deadline = Instant::now() + Duration::from_secs(60);
    while runtime.registered_connections() != 0 {
        assert!(
            Instant::now() < deadline,
            "stage=disconnect connections={}; started={started}; active={:?}; file={:?}",
            runtime.registered_connections(),
            fixture.runtime().active_task_summaries(),
            std::fs::read_to_string(&side_effect)
        );
        thread::sleep(Duration::from_millis(20));
    }
    runtime.stop();
    let contents = std::fs::read_to_string(&side_effect).unwrap();
    assert_eq!(
        contents.lines().filter(|line| *line == "once").count(),
        1,
        "stage=after-stop started={started}; file={contents:?}"
    );
    let name = pipe_name(&installation_id(executable.parent().unwrap()).unwrap()).unwrap();
    assert!(Pipe::connect(&name, &workspace.join("untrusted.exe")).is_err());
    fixture.shutdown();
}

#[test]
fn both_sides_reject_a_process_from_another_installation() {
    let fixture = PublicRuntimeFixture::start_authenticated(PermissionMode::Edit);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let workspace = crate::mcp::test_support::temp_workspace();
    let wrong_installation = workspace.join("probe.txt");
    let mut runtime = LocalRuntime::start(
        &wrong_installation,
        fixture.runtime().port(),
        fixture
            .runtime()
            .local_connector_bearer()
            .expect("authenticated local transport fixture provides a connector bearer"),
    )
    .unwrap();
    let name = pipe_name(&installation_id(&workspace).unwrap()).unwrap();
    if let Ok(mut unauthorized) = Pipe::connect(&name, &executable) {
        assert!(
            unauthorized
                .receive_with_tick(Duration::from_secs(5))
                .is_err(),
            "server accepted a client outside its installation"
        );
    }
    runtime.stop();
    let mut runtime = LocalRuntime::start(
        &executable,
        fixture.runtime().port(),
        fixture
            .runtime()
            .local_connector_bearer()
            .expect("authenticated local transport fixture provides a connector bearer"),
    )
    .unwrap();
    let name = pipe_name(&installation_id(executable.parent().unwrap()).unwrap()).unwrap();
    assert!(
        Pipe::connect(&name, &wrong_installation).is_err(),
        "client accepted a server outside its installation"
    );
    runtime.stop();
    fixture.shutdown();
}

#[test]
fn browser_authorization_actual_pipe_durable_dedup_and_revocation() {
    use crate::browser_connection::authority::{Authority, WorkspaceStamp};
    use std::sync::Arc;
    let candidate = crate::mcp::test_support::temp_workspace();
    let validated = crate::workspace::WorkspaceValidator
        .validate(&candidate)
        .unwrap();
    // Win32 canonicalize produces an identity-only \\?\ alias. Use the
    // validator's ordinary, identity-checked execution path for both sides.
    let workspace = validated.execution_path().to_path_buf();
    let stamp = WorkspaceStamp {
        path: workspace.to_string_lossy().into_owned(),
        identity: validated.identity().as_str().into(),
        permission: serde_json::to_string(&PermissionMode::Full).unwrap(),
    };
    let source = stamp.clone();
    let authority = Arc::new(
        Authority::open(
            workspace.join("browser-test-state.json"),
            Arc::new(move || Some(source.clone())),
        )
        .unwrap(),
    );
    crate::browser_connection::authority::install(authority.clone()).unwrap();
    let instance = "11111111-1111-1111-1111-111111111111";
    let secret = "a".repeat(64);
    let origin = format!(
        "chrome-extension://{}/",
        crate::browser_connection::EXTENSION_ID.trim()
    );
    authority.pair(instance, &secret, &origin).unwrap();
    authority
        .approve(instance, &authority.summaries().unwrap()[0].context)
        .unwrap();
    let fixture =
        PublicRuntimeFixture::start_authenticated_in(workspace.clone(), PermissionMode::Full);
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let mut runtime = LocalRuntime::start(
        &executable,
        fixture.runtime().port(),
        fixture.runtime().local_connector_bearer().unwrap(),
    )
    .unwrap();
    let token = authority
        .grant(instance, &secret, &origin, "chat-one")
        .unwrap();
    let mut pipe = client(&executable);
    send(&pipe, &json!({"browserGrant":token}));
    send(
        &pipe,
        &json!({"mcp":{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"arbitrary untrusted field","version":"1"}}}}),
    );
    assert_eq!(
        response(&mut pipe, json!("init"))["result"]["protocolVersion"],
        "2025-11-25"
    );
    send(
        &pipe,
        &json!({"mcp":{"jsonrpc":"2.0","method":"notifications/initialized"}}),
    );
    let request = json!({"jsonrpc":"2.0","id":"write","method":"tools/call","params":{"name":"filesystem","arguments":{"action":"write","path":"browser-once.txt","content":"once","encoding":"utf8"}}});
    let mut envelope = json!({"mcp":request,"observation":{"message":"assistant-one","branch":"user-one","index":0}});
    send(&pipe, &envelope);
    let initial = response(&mut pipe, json!("write"));
    assert_eq!(initial["result"]["isError"], false, "{initial}");
    std::fs::write(
        workspace.join("browser-once.txt"),
        "user-changed-after-call",
    )
    .unwrap();
    envelope["mcp"]["id"] = json!("duplicate");
    send(&pipe, &envelope);
    let duplicate = response(&mut pipe, json!("duplicate"));
    assert_eq!(duplicate["result"], initial["result"]);
    assert_eq!(
        std::fs::read_to_string(workspace.join("browser-once.txt")).unwrap(),
        "user-changed-after-call"
    );
    assert_eq!(super::runtime::browser_successful_calls(), 1);
    assert_eq!(super::runtime::successful_calls(), 0);
    send(
        &pipe,
        &json!({"mcp":{"jsonrpc":"2.0","id":"browser-running","method":"tools/call","params":{"name":"exec_command","arguments":{"command":"Add-Content -LiteralPath browser-running.txt -Value once; Start-Sleep -Seconds 120","shell":"windows_powershell","workdir":".","yield_time_ms":1000,"timeout_ms":180000,"max_output_bytes":65536}}},"observation":{"message":"assistant-running","branch":"user-two","index":0}}),
    );
    let running = response(&mut pipe, json!("browser-running"));
    assert_eq!(running["result"]["isError"], false, "{running}");
    assert!(
        running["result"]["structuredContent"]["data"]["session_id"].is_string(),
        "{running}"
    );
    // A yielded command may still be starting PowerShell. Observe its actual
    // write before revocation; do not rerun a side effect to make the test pass.
    let side_effect = workspace.join("browser-running.txt");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let contents = std::fs::read_to_string(&side_effect);
        if contents
            .as_ref()
            .is_ok_and(|text| text.lines().collect::<Vec<_>>() == ["once"])
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "browser command did not reach its write: response={running}; file={contents:?}; active={:?}",
            fixture.runtime().active_task_summaries()
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !fixture.runtime().active_task_summaries().is_empty(),
        "browser revocation must cancel an actually running command"
    );
    authority.revoke(instance).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while super::runtime::browser_connected_clients() != 0 {
        assert!(
            Instant::now() < deadline,
            "revoked browser session did not close"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        authority
            .grant(instance, &secret, &origin, "another-chat")
            .is_err()
    );
    runtime.stop();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fixture.runtime().active_task_summaries().is_empty() {
        assert!(
            Instant::now() < deadline,
            "browser cancellation did not settle: {:?}",
            fixture.runtime().active_task_summaries()
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        std::fs::read_to_string(workspace.join("browser-running.txt"))
            .unwrap()
            .lines()
            .filter(|line| *line == "once")
            .count(),
        1
    );
    fixture.shutdown();
}
