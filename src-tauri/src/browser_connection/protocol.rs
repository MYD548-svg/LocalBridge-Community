use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

pub const MAX_NATIVE_FRAME: usize = 256 * 1024;
pub const MAX_REQUEST_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const CHUNK_BYTES: usize = 180 * 1024;
const MAX_BUFFER_BYTES: usize = 32 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fragment {
    pub version: u32,
    pub transfer_id: String,
    pub index: usize,
    pub count: usize,
    pub total_bytes: usize,
    pub data: String,
}

pub fn encode(value: &Value, transfer_id: &str) -> io::Result<Vec<Vec<u8>>> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() > MAX_RESPONSE_BYTES || transfer_id.len() > 80 {
        return Err(io::Error::other("browser message exceeds limit"));
    }
    bytes
        .chunks(CHUNK_BYTES)
        .enumerate()
        .map(|(index, chunk)| {
            let frame = serde_json::to_vec(&Fragment {
                version: super::PROTOCOL_VERSION,
                transfer_id: transfer_id.to_owned(),
                index,
                count: bytes.len().div_ceil(CHUNK_BYTES),
                total_bytes: bytes.len(),
                data: STANDARD.encode(chunk),
            })
            .map_err(io::Error::other)?;
            if frame.len() > MAX_NATIVE_FRAME {
                return Err(io::Error::other("native frame exceeds limit"));
            }
            Ok(frame)
        })
        .collect()
}

struct Assembly {
    started: Instant,
    total: usize,
    parts: Vec<Vec<u8>>,
    bytes: usize,
}

#[derive(Default)]
pub struct Reassembler {
    pending: HashMap<String, Assembly>,
}

impl Reassembler {
    pub fn push(&mut self, bytes: &[u8], limit: usize) -> io::Result<Option<Value>> {
        self.pending
            .retain(|_, assembly| assembly.started.elapsed() < Duration::from_secs(15));
        let frame: Fragment = serde_json::from_slice(bytes).map_err(io::Error::other)?;
        if frame.version != super::PROTOCOL_VERSION
            || frame.transfer_id.is_empty()
            || frame.transfer_id.len() > 80
            || frame.total_bytes == 0
            || frame.total_bytes > limit
            || frame.count != frame.total_bytes.div_ceil(CHUNK_BYTES)
            || frame.index >= frame.count
            || bytes.len() > MAX_NATIVE_FRAME
        {
            return Err(io::Error::other("invalid browser fragment"));
        }
        if !self.pending.contains_key(&frame.transfer_id)
            && (frame.index != 0 || self.pending.len() >= 4)
        {
            return Err(io::Error::other("fragment order or capacity"));
        }
        let chunk = STANDARD.decode(&frame.data).map_err(io::Error::other)?;
        let expected = if frame.index + 1 == frame.count {
            frame.total_bytes - frame.index * CHUNK_BYTES
        } else {
            CHUNK_BYTES
        };
        if chunk.len() != expected {
            return Err(io::Error::other("fragment byte length mismatch"));
        }
        let buffered: usize = self.pending.values().map(|entry| entry.bytes).sum();
        if buffered + chunk.len() > MAX_BUFFER_BYTES {
            return Err(io::Error::other("fragment memory budget"));
        }
        let entry = self
            .pending
            .entry(frame.transfer_id.clone())
            .or_insert_with(|| Assembly {
                started: Instant::now(),
                total: frame.total_bytes,
                parts: Vec::new(),
                bytes: 0,
            });
        if entry.total != frame.total_bytes || entry.parts.len() != frame.index {
            return Err(io::Error::other("fragment identity or order mismatch"));
        }
        entry.bytes += chunk.len();
        entry.parts.push(chunk);
        if entry.parts.len() != frame.count {
            return Ok(None);
        }
        let complete = self
            .pending
            .remove(&frame.transfer_id)
            .expect("completed assembly");
        let joined: Vec<u8> = complete.parts.into_iter().flatten().collect();
        if joined.len() != complete.total {
            return Err(io::Error::other("fragment total mismatch"));
        }
        serde_json::from_slice(&joined)
            .map(Some)
            .map_err(io::Error::other)
    }
}

pub fn read_native(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0; 4];
    match input.read(&mut header[..1])? {
        0 => return Ok(None),
        1 => input.read_exact(&mut header[1..])?,
        _ => unreachable!(),
    }
    let size = u32::from_le_bytes(header) as usize;
    if size == 0 || size > MAX_NATIVE_FRAME {
        return Err(io::Error::other("native frame size"));
    }
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

pub fn write_native(output: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > MAX_NATIVE_FRAME {
        return Err(io::Error::other("native frame size"));
    }
    output.write_all(&(bytes.len() as u32).to_le_bytes())?;
    output.write_all(bytes)?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn utf8_large_message_roundtrips_and_frames_are_bounded() {
        let value = json!({"text":"中文".repeat(250_000),"id":"rpc","structuredContent":{"output_refs":{"stdout":"x"}}});
        let frames = encode(&value, "transfer").unwrap();
        assert!(frames.len() > 1);
        let mut reader = Reassembler::default();
        let mut result = None;
        for frame in frames {
            assert!(frame.len() <= MAX_NATIVE_FRAME);
            result = reader.push(&frame, MAX_RESPONSE_BYTES).unwrap();
        }
        assert_eq!(result, Some(value));
    }
    #[test]
    fn oversized_headers_incomplete_and_out_of_order_are_rejected() {
        assert!(read_native(&mut &((MAX_NATIVE_FRAME + 1) as u32).to_le_bytes()[..]).is_err());
        assert!(read_native(&mut &[1_u8, 0][..]).is_err());
        let frames = encode(&json!({"text":"x".repeat(300_000)}), "transfer").unwrap();
        assert!(
            Reassembler::default()
                .push(&frames[1], MAX_REQUEST_BYTES)
                .is_err()
        );
        assert!(Reassembler::default().push(&frames[0], 10).is_err());
    }
}
