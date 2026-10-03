//! Real Windows child-process and native stdio tests. No installed browser,
//! registry, user configuration or old build is used; fixtures are retained.
#![cfg(windows)]
use localbridge_lib::browser_connection::{EXTENSION_ID, HOST_NAME, protocol};
use serde_json::json;
use std::io;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(process: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = process.try_wait().unwrap() {
            return status.success();
        }
        assert!(Instant::now() < deadline, "native host did not exit");
        thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn native_origin_protocol_errors_and_eof_use_the_new_host() {
    let binary = env!("CARGO_BIN_EXE_localbridge-browser-host");
    let mut rejected = Process(
        Command::new(binary)
            .arg("chrome-extension://wrong/")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(!wait(&mut rejected.0));
    let origin = format!("chrome-extension://{}/", EXTENSION_ID.trim());
    let mut host = Process(
        Command::new(binary)
            .arg(origin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut input = host.0.stdin.take().unwrap();
    let mut output = host.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| -> io::Result<serde_json::Value> {
            let mut assembly = protocol::Reassembler::default();
            loop {
                let frame = protocol::read_native(&mut output)?
                    .ok_or_else(|| io::Error::other("unexpected EOF"))?;
                if let Some(reply) = assembly.push(&frame, protocol::MAX_RESPONSE_BYTES)? {
                    return Ok(reply);
                }
            }
        })();
        let _ = tx.send(result);
    });
    let request = json!({"version":1,"type":"mcp","payload":{"jsonrpc":"2.0","id":"中文-rpc","method":"tools/list"}});
    for frame in protocol::encode(&request, "request").unwrap() {
        protocol::write_native(&mut input, &frame).unwrap();
    }
    let reply = rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    assert_eq!(reply["type"], "error");
    assert_eq!(reply["rpcId"], "中文-rpc");
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("chat not enabled")
    );
    drop(input);
    assert!(wait(&mut host.0));
}
#[test]
fn installed_manifest_is_absolute_fixed_origin_and_template_checked() {
    let directory = std::env::temp_dir().join(format!(
        "localbridge-browser-manifest-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let binary = directory.join("localbridge-browser-host.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_localbridge-browser-host"), &binary).unwrap();
    let template = json!({"name":HOST_NAME,"description":"test","path":"localbridge-browser-host.exe","type":"stdio","allowed_origins":[format!("chrome-extension://{}/", EXTENSION_ID.trim())]});
    std::fs::write(
        directory.join("native-host-template.json"),
        serde_json::to_vec(&template).unwrap(),
    )
    .unwrap();
    let mut process = Process(
        Command::new(&binary)
            .arg("--write-manifest")
            .spawn()
            .unwrap(),
    );
    assert!(wait(&mut process.0));
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.join("localbridge-native-host.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["path"], json!(binary.canonicalize().unwrap()));
    assert_eq!(manifest["allowed_origins"], template["allowed_origins"]);
    let mut malformed = template;
    malformed["allowed_origins"] = json!(["chrome-extension://wrong/"]);
    std::fs::write(
        directory.join("native-host-template.json"),
        serde_json::to_vec(&malformed).unwrap(),
    )
    .unwrap();
    let mut process = Process(
        Command::new(&binary)
            .arg("--write-manifest")
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(!wait(&mut process.0));
    assert_eq!(
        std::fs::read(directory.join("localbridge-native-host.json")).unwrap(),
        serde_json::to_vec(&manifest).unwrap()
    );
}
