// SPDX-License-Identifier: MIT
// Adaptation copyright (c) 2024 Sergey Parfenyuk; LocalBridge modifications 2026.
// mcp-proxy transport separation adaptation; see docs/licenses/mcp-proxy-MIT.txt.
use localbridge_lib::local_connection::{codec, pipe, profile};

use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct AdapterError {
    phase: &'static str,
    code: &'static str,
    win32: Option<i32>,
}

fn failure(phase: &'static str, error: io::Error) -> AdapterError {
    let win32 = error.raw_os_error().or_else(|| {
        error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<io::Error>())
            .and_then(io::Error::raw_os_error)
    });
    let code = match (phase, error.kind()) {
        ("arguments", _) => "invalid_arguments",
        ("installation_identity", _) => "installation_identity_mismatch",
        ("connect", io::ErrorKind::TimedOut) => "pipe_busy_timeout",
        (_, io::ErrorKind::NotFound) => "service_or_installation_missing",
        (_, io::ErrorKind::PermissionDenied) if win32.is_some() => "access_denied",
        (_, io::ErrorKind::PermissionDenied) => "peer_identity_rejected",
        (_, io::ErrorKind::BrokenPipe) => "pipe_closed",
        _ => "operation_failed",
    };
    AdapterError { phase, code, win32 }
}

fn run() -> Result<(), AdapterError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--install-id" {
        return Err(failure(
            "arguments",
            io::Error::other("invalid adapter arguments"),
        ));
    }
    let executable = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| failure("installation", error))?;
    let root = executable
        .parent()
        .ok_or_else(|| failure("installation", io::Error::other("installation unavailable")))?;
    if profile::installation_id(root).map_err(|error| failure("installation", error))? != args[1] {
        return Err(failure(
            "installation_identity",
            io::Error::other("installation identity mismatch"),
        ));
    }
    let mut connection = pipe::Pipe::connect(
        &pipe::pipe_name(&args[1]).map_err(|error| failure("arguments", error))?,
        &root.join("localbridge.exe"),
    )
    .map_err(|error| failure("connect", error))?;
    let input_peer = connection.clone();
    let input_ended = Arc::new(AtomicBool::new(false));
    let ended = input_ended.clone();
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        loop {
            match codec::read_stdio(&mut input) {
                Ok(Some(bytes)) => {
                    if input_peer.send(&bytes).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    ended.store(true, Ordering::Release);
                    break;
                }
                Err(_) => break,
            }
        }
        input_peer.close();
    });
    let mut output = io::stdout().lock();
    loop {
        let bytes = match connection.receive_with_tick(std::time::Duration::from_secs(1)) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => continue,
            Err(_) if input_ended.load(Ordering::Acquire) => return Ok(()),
            Err(error) => return Err(failure("pipe_read", error)),
        };
        let message = codec::validate_message(&bytes).map_err(|error| failure("frame", error))?;
        serde_json::to_writer(&mut output, &message)
            .map_err(|error| failure("stdout", io::Error::other(error)))?;
        output
            .write_all(b"\n")
            .map_err(|error| failure("stdout", error))?;
        output.flush().map_err(|error| failure("stdout", error))?;
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!(
            "LocalBridge 本地连接不可用，请启动 LocalBridge 并检查安装完整性；请求不会自动重放。"
        );
        // Only fixed classifications/numeric OS codes reach stderr, never the
        // source error text, paths, frames, credentials or configuration.
        eprintln!(
            "LocalBridge diagnostic: phase={} code={} win32={}",
            error.phase,
            error.code,
            error
                .win32
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unavailable".into())
        );
        std::process::exit(1);
    }
}
