// SPDX-License-Identifier: MIT
// Adaptation copyright (c) 2024 Sergey Parfenyuk; LocalBridge modifications 2026.
//! Transport-independent relay framing. Adapted from mcp-proxy's separation of
//! streamablehttp_client.py and proxy_server.py (153a96a6, MIT, Sergey Parfenyuk).
//! Unlike the SDK proxy, forward entire JSON-RPC envelopes without regenerating IDs.
use std::io::{self, BufRead, Read, Write};

use serde_json::Value;

pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

pub fn read_frame(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame size",
        ));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub fn write_frame(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame size",
        ));
    }
    writer.write_all(&(bytes.len() as u32).to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}

pub fn read_stdio(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let count = reader
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut line)?;
    if count == 0 {
        return Ok(None);
    }
    if count > MAX_FRAME_BYTES || !line.ends_with(b"\n") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid stdio frame",
        ));
    }
    validate_message(&line)?;
    Ok(Some(line))
}

pub fn validate_message(bytes: &[u8]) -> io::Result<Value> {
    let message: Value = serde_json::from_slice(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid JSON"))?;
    let Some(object) = message.as_object() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSON-RPC object required",
        ));
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSON-RPC 2.0 required",
        ));
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_preserve_ids_errors_structured_content_and_unicode() {
        for id in [serde_json::json!(0), serde_json::json!("请求-0")] {
            let original = serde_json::json!({"jsonrpc":"2.0","id":id,"result":{
                "structuredContent":{"output_refs":{"stderr":"retained-错误"}},
                "content":[{"type":"text","text":"中文\nsecond line"}],"isError":true}});
            let bytes = serde_json::to_vec(&original).unwrap();
            let mut stream = Vec::new();
            write_frame(&mut stream, &bytes).unwrap();
            assert_eq!(
                validate_message(&read_frame(&mut Cursor::new(stream)).unwrap()).unwrap(),
                original
            );
        }
    }

    #[test]
    fn rejects_empty_oversized_partial_and_non_rpc_frames() {
        for size in [0, MAX_FRAME_BYTES as u32 + 1] {
            assert!(read_frame(&mut Cursor::new(size.to_le_bytes())).is_err());
        }
        assert!(read_frame(&mut Cursor::new([5, 0, 0, 0, 1])).is_err());
        assert!(read_stdio(&mut Cursor::new(b"{}\n")).is_err());
        assert!(read_stdio(&mut Cursor::new(b"{\"jsonrpc\":\"2.0\"}")).is_err());
        assert!(validate_message(b"[]").is_err());
    }

    #[test]
    fn consecutive_notifications_are_not_combined() {
        let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":\"x\"}}\n";
        let mut reader = Cursor::new(input);
        assert!(read_stdio(&mut reader).unwrap().is_some());
        assert!(read_stdio(&mut reader).unwrap().is_some());
        assert!(read_stdio(&mut reader).unwrap().is_none());
    }
}
