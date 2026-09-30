// SPDX-License-Identifier: MIT
// Adaptation copyright (c) 2024 Sergey Parfenyuk; LocalBridge modifications 2026.
// mcp-proxy transport separation adaptation; see docs/licenses/mcp-proxy-MIT.txt.
use localbridge_lib::local_connection::{codec, pipe, profile};

use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--install-id" {
        return Err(io::Error::other("invalid adapter arguments"));
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let root = executable
        .parent()
        .ok_or_else(|| io::Error::other("installation unavailable"))?;
    if profile::installation_id(root)? != args[1] {
        return Err(io::Error::other("installation identity mismatch"));
    }
    let mut connection =
        pipe::Pipe::connect(&pipe::pipe_name(&args[1])?, &root.join("localbridge.exe"))?;
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
            Err(error) => return Err(error),
        };
        let message = codec::validate_message(&bytes)?;
        serde_json::to_writer(&mut output, &message).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

fn main() {
    if run().is_err() {
        eprintln!(
            "LocalBridge 本地连接不可用，请启动 LocalBridge 并检查安装完整性；请求不会自动重放。"
        );
        std::process::exit(1);
    }
}
