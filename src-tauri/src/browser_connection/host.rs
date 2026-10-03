use super::{control_pipe_name, protocol};
use crate::local_connection::{codec, pipe::Pipe, profile};
use serde_json::{Value, json};
use std::io::{self, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x08000000;
fn send(output: &Arc<Mutex<io::Stdout>>, value: &Value) -> io::Result<()> {
    let frames = protocol::encode(value, &crate::security::random_prefixed_id("frame-"))?;
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("host output lock"))?;
    for bytes in frames {
        protocol::write_native(&mut *output, &bytes)?;
    }
    Ok(())
}
fn control(root: &Path, request: &Value) -> io::Result<Value> {
    let mut pipe = Pipe::connect(
        &control_pipe_name(&profile::installation_id(root)?)?,
        &root.join("localbridge.exe"),
    )?;
    pipe.send(&serde_json::to_vec(request).map_err(io::Error::other)?)?;
    let response: Value = serde_json::from_slice(&pipe.receive_frame(Duration::from_secs(5))?)
        .map_err(io::Error::other)?;
    if response["ok"] != true {
        return Err(io::Error::other(
            response["error"]
                .as_str()
                .unwrap_or("browser control rejected"),
        ));
    }
    Ok(response["value"].clone())
}
struct Adapter {
    process: Child,
    input: std::process::ChildStdin,
    reader: Option<JoinHandle<()>>,
    root: std::path::PathBuf,
    grant: String,
}
impl Adapter {
    fn start(root: &Path, grant: String, output: Arc<Mutex<io::Stdout>>) -> io::Result<Self> {
        let mut process = Command::new(root.join("localbridge-mcp.exe"))
            .args([
                "--install-id",
                &profile::installation_id(root)?,
                "--browser",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let mut input = process
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("adapter input"))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("adapter output"))?;
        writeln!(input, "{}", json!({"browserGrant":grant}))?;
        input.flush()?;
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(bytes)) = codec::read_stdio(&mut reader) {
                if let Ok(payload) = serde_json::from_slice::<Value>(&bytes) {
                    if send(
                        &output,
                        &json!({"version":1,"type":"mcp","payload":payload}),
                    )
                    .is_err()
                    {
                        break;
                    }
                } else {
                    break;
                }
            }
            let _ = send(
                &output,
                &json!({"version":1,"type":"disconnected","error":"本机连接已关闭；状态不明的请求不会自动重放"}),
            );
        });
        Ok(Self {
            process,
            input,
            reader: Some(reader),
            root: root.into(),
            grant,
        })
    }
    fn forward(&mut self, payload: &Value, observation: Option<&Value>) -> io::Result<()> {
        let envelope = json!({"mcp":payload,"observation":observation});
        let bytes = serde_json::to_vec(&envelope).map_err(io::Error::other)?;
        if bytes.len() > protocol::MAX_REQUEST_BYTES {
            return Err(io::Error::other("browser request too large"));
        }
        self.input.write_all(&bytes)?;
        self.input.write_all(b"\n")?;
        self.input.flush()
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        // Closing the authenticated grant causes the runtime to cancel the session.
        let _ = control(&self.root, &json!({"operation":"close","grant":self.grant}));
        let _ = self.process.kill();
        let _ = self.process.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
pub fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.as_slice() == ["--write-manifest"] {
        return write_manifest();
    }
    if args.as_slice() == ["--prepare-update"] {
        return ensure_unoccupied();
    }
    let origin = args
        .first()
        .ok_or_else(|| io::Error::other("missing extension origin"))?;
    if !super::allowed_origin(origin)
        || args.iter().skip(1).any(|arg| {
            !arg.strip_prefix("--parent-window=")
                .is_some_and(|value| value.bytes().all(|b| b.is_ascii_digit()))
        })
    {
        return Err(io::Error::other("extension origin rejected"));
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let root = executable
        .parent()
        .ok_or_else(|| io::Error::other("installation directory"))?;
    let output = Arc::new(Mutex::new(io::stdout()));
    // Native input can stay open after an adapter crashes. A bounded channel
    // lets the owner reclaim its child without waiting for another message.
    let (tx, rx) = mpsc::sync_channel(4);
    thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            let frame = protocol::read_native(&mut input);
            let finished = !matches!(&frame, Ok(Some(_)));
            if tx.send(frame).is_err() || finished {
                break;
            }
        }
    });
    let mut assembly = protocol::Reassembler::default();
    let mut adapter: Option<Adapter> = None;
    let mut presence: Option<Pipe> = None;
    loop {
        if let Some(child) = adapter.as_mut() {
            if child.process.try_wait()?.is_some() {
                break;
            }
        }
        let bytes = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(Some(bytes))) => bytes,
            Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Err(error)) => return Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        };
        let Some(request) = assembly.push(&bytes, protocol::MAX_REQUEST_BYTES)? else {
            continue;
        };
        let result: io::Result<()> = (|| {
            if request["version"] != super::PROTOCOL_VERSION {
                return Err(io::Error::other("browser protocol incompatible"));
            }
            match request["type"].as_str().unwrap_or("") {
                "pair" | "enable" => {
                    if adapter.is_some() && request["type"] == "enable" {
                        return Err(io::Error::other("chat already enabled"));
                    }
                    if request["startApp"] == true {
                        // Only an explicit extension UI action may request this once.
                        Command::new(root.join("localbridge.exe"))
                            .creation_flags(CREATE_NO_WINDOW)
                            .spawn()?;
                        for _ in 0..40 {
                            if control(root, &json!({"operation":"pair","instance":request["instance"],"secret":request["secret"],"origin":origin})).is_ok() { break; }
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                    let paired = control(
                        root,
                        &json!({"operation":"pair","instance":request["instance"],"secret":request["secret"],"origin":origin}),
                    )?;
                    if presence.is_none() {
                        let mut connection = Pipe::connect(
                            &control_pipe_name(&profile::installation_id(root)?)?,
                            &root.join("localbridge.exe"),
                        )?;
                        connection.send(&serde_json::to_vec(&json!({"operation":"presence","instance":request["instance"],"secret":request["secret"],"origin":origin})).map_err(io::Error::other)?)?;
                        let reply: Value = serde_json::from_slice(
                            &connection.receive_frame(Duration::from_secs(5))?,
                        )
                        .map_err(io::Error::other)?;
                        if reply["ok"] != true {
                            return Err(io::Error::other("browser presence rejected"));
                        }
                        presence = Some(connection);
                    }
                    if request["type"] == "enable" && paired["approved"] == true {
                        let value = control(
                            root,
                            &json!({"operation":"grant","instance":request["instance"],"secret":request["secret"],"origin":origin,"chat":request["chat"]}),
                        )?;
                        let grant = value["grant"]
                            .as_str()
                            .ok_or_else(|| io::Error::other("grant response missing"))?
                            .to_owned();
                        adapter = Some(Adapter::start(root, grant, output.clone())?);
                    }
                    send(
                        &output,
                        &json!({"version":1,"type":"state","requestId":request["requestId"],"payload":paired}),
                    )
                }
                "mcp" => {
                    let payload = &request["payload"];
                    let bytes = serde_json::to_vec(payload).map_err(io::Error::other)?;
                    codec::validate_message(&bytes)?;
                    if !matches!(
                        payload["method"].as_str(),
                        Some(
                            "initialize"
                                | "notifications/initialized"
                                | "tools/list"
                                | "tools/call"
                                | "ping"
                                | "notifications/cancelled"
                        )
                    ) {
                        return Err(io::Error::other("browser MCP method rejected"));
                    }
                    adapter
                        .as_mut()
                        .ok_or_else(|| io::Error::other("chat not enabled"))?
                        .forward(payload, request.get("observation"))
                }
                "close" => {
                    adapter = None;
                    Ok(())
                }
                _ => Err(io::Error::other("unknown browser operation")),
            }
        })();
        if let Err(error) = result {
            send(
                &output,
                &json!({"version":1,"type":"error","requestId":request["requestId"],"rpcId":request.pointer("/payload/id"),"error":error.to_string()}),
            )?;
        }
    }
    drop(adapter);
    drop(presence);
    Ok(())
}

fn write_manifest() -> io::Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let root = executable
        .parent()
        .ok_or_else(|| io::Error::other("installation directory"))?;
    let mut template: Value =
        serde_json::from_slice(&std::fs::read(root.join("native-host-template.json"))?)
            .map_err(io::Error::other)?;
    if template["name"] != super::HOST_NAME
        || template["path"] != "localbridge-browser-host.exe"
        || template["type"] != "stdio"
        || template["allowed_origins"]
            != json!([format!(
                "chrome-extension://{}/",
                super::EXTENSION_ID.trim()
            )])
    {
        return Err(io::Error::other("native host template incompatible"));
    }
    template["path"] = json!(executable);
    let staged = root
        .join(crate::security::random_prefixed_id("native-host-"))
        .with_extension("tmp");
    std::fs::write(
        &staged,
        serde_json::to_vec(&template).map_err(io::Error::other)?,
    )?;
    profile::atomic_replace(&staged, &root.join("localbridge-native-host.json"))
}

fn ensure_unoccupied() -> io::Result<()> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    let executable = std::env::current_exe()?.canonicalize()?;
    let root = executable
        .parent()
        .ok_or_else(|| io::Error::other("installation directory"))?;
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut next = Process32FirstW(snapshot, &mut entry);
        while next != 0 {
            if entry.th32ProcessID != std::process::id() {
                let process =
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !process.is_null() {
                    let mut path = vec![0_u16; 32768];
                    let mut length = path.len() as u32;
                    if QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut length) != 0 {
                        let path = std::path::PathBuf::from(String::from_utf16_lossy(
                            &path[..length as usize],
                        ));
                        if let Ok(path) = path.canonicalize() {
                            found |= path == executable
                                || path == root.join("localbridge-mcp.exe")
                                || path == root.join("localbridge.exe");
                        }
                    }
                    CloseHandle(process);
                }
            }
            next = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        if found {
            return Err(io::Error::other(
                "请关闭浏览器扩展和 LocalBridge 后重新运行安装程序",
            ));
        }
    }
    Ok(())
}
