//! Shared incremental delivery for facade, observer and direct control calls.
//! Only short buffer operations hold this lock; a collection guard owns no lock.
use std::sync::{Arc, Mutex};

pub(crate) const INCOMPLETE_COMMAND_OUTPUT: &str =
    "Output incomplete: pipe readers did not finish within 5 seconds after process exit.";
const MAX_PENDING_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_STDERR_PROTOCOL_BYTES: usize = 256 * 1024;

#[derive(Debug, Default)]
struct OutputState {
    pending: String,
    pending_truncated: bool,
    truncated: bool,
    incomplete: bool,
    stderr_protocol: String,
    collecting: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CommandOutput(Arc<Mutex<OutputState>>);

#[derive(Debug, Default)]
pub(crate) struct OutputDelivery {
    pub output: String,
    pub truncated: bool,
    pub incomplete: bool,
    pub collecting: bool,
}

pub(crate) struct OutputCollection(CommandOutput);
impl Drop for OutputCollection {
    fn drop(&mut self) {
        let mut state = self
            .0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.collecting -= 1;
    }
}

impl CommandOutput {
    pub(crate) fn begin(&self) -> OutputCollection {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .collecting += 1;
        OutputCollection(self.clone())
    }
    pub(crate) fn append(&self, output: &str) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.pending.push_str(output);
        let trimmed = trim_utf8_front(&mut state.pending, MAX_PENDING_OUTPUT_BYTES);
        state.pending_truncated |= trimmed;
        state.truncated |= trimmed;
    }
    pub(crate) fn filter_stderr(&self, stderr: &str) -> String {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stderr_protocol.push_str(stderr);
        if trim_utf8_front(&mut state.stderr_protocol, MAX_STDERR_PROTOCOL_BYTES) {
            state.truncated = true;
            return format!(
                "[stderr protocol fragment truncated]\n{}",
                public_command_stderr(&std::mem::take(&mut state.stderr_protocol))
            );
        }
        drain_public_stderr_protocol_buffer(&mut state.stderr_protocol)
    }
    pub(crate) fn annotate(&self, truncated: bool, incomplete: bool) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.truncated |= truncated || incomplete;
        state.incomplete |= incomplete;
    }
    pub(crate) fn incomplete(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .incomplete
    }
    pub(crate) fn take(&self) -> OutputDelivery {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.collecting != 0 {
            return OutputDelivery {
                truncated: state.truncated,
                incomplete: state.incomplete,
                collecting: true,
                ..OutputDelivery::default()
            };
        }
        let mut output = std::mem::take(&mut state.pending);
        if std::mem::take(&mut state.pending_truncated) {
            output.insert_str(0, "[earlier command output truncated]\n");
        }
        OutputDelivery {
            output,
            truncated: state.truncated,
            incomplete: state.incomplete,
            collecting: state.collecting != 0,
        }
    }
    pub(crate) fn has_pending(&self) -> bool {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.collecting == 0 && !state.pending.is_empty()
    }
    #[cfg(test)]
    pub(crate) fn pending(&self) -> String {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn paused_collector_prevents_delivery_until_its_bytes_are_staged() {
        let output = CommandOutput::default();
        let other = output.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _collection = other.begin();
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            other.append("DELAYED");
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        output.append("EARLIER");
        let held = output.take();
        assert!(held.collecting);
        assert!(held.output.is_empty());
        release_tx.send(()).unwrap();
        worker.join().unwrap();
        let settled = output.take();
        assert!(!settled.collecting);
        assert_eq!(settled.output, "EARLIERDELAYED");
        assert!(output.take().output.is_empty());
    }

    #[test]
    fn concurrent_deliveries_consume_one_shared_cursor() {
        let output = CommandOutput::default();
        output.append("ONCE");
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let output = output.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    output.take().output
                })
            })
            .collect();
        barrier.wait();
        let delivered: String = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(delivered, "ONCE");
    }

    #[test]
    fn shared_stderr_fragments_and_sticky_truncation_survive_replay() {
        let output = CommandOutput::default();
        let other = output.clone();
        assert!(
            output
                .filter_stderr("#< CLIXML\r\n<Objs><S S=\"Error\">hel")
                .is_empty()
        );
        let visible = other.filter_stderr("lo</S></Objs>");
        assert!(visible.contains("hello"), "{visible:?}");
        output.append(&"x".repeat(MAX_PENDING_OUTPUT_BYTES + 16));
        output.annotate(false, true);
        let first = other.take();
        assert!(
            first
                .output
                .starts_with("[earlier command output truncated]")
        );
        assert!(first.truncated && first.incomplete);
        let replay = output.take();
        assert!(replay.output.is_empty());
        assert!(replay.truncated && replay.incomplete);
    }
}

pub(crate) fn public_command_stderr(stderr: &str) -> String {
    if !looks_like_clixml_protocol(stderr) {
        return stderr.to_string();
    }
    let lower = stderr.to_ascii_lowercase();
    if !lower.contains("s=\"error\"") {
        return String::new();
    }
    strip_private_powershell_prologue(&extract_clixml_error_strings(stderr))
}

fn strip_private_powershell_prologue(value: &str) -> String {
    let contains_private_prologue = value.contains("PSModuleAutoLoadingPreference")
        || value.contains("Microsoft.PowerShell.Management.psd1")
        || value.contains("System.Text.UTF8Encoding")
        || value.contains("[Console]::OutputEncoding");
    if !contains_private_prologue {
        return value.to_string();
    }
    const END: &str = "$OutputEncoding=[Console]::OutputEncoding;";
    let Some(end) = find_ignoring_line_breaks(value, END) else {
        return String::new();
    };
    value[end..].trim_start_matches(['\r', '\n']).to_string()
}

fn find_ignoring_line_breaks(value: &str, needle: &str) -> Option<usize> {
    let expected = needle.as_bytes();
    let mut matched = 0usize;
    for (index, ch) in value.char_indices() {
        if matches!(ch, '\r' | '\n') {
            continue;
        }
        if ch.is_ascii() && expected.get(matched).copied() == Some(ch as u8) {
            matched += 1;
            if matched == expected.len() {
                return Some(index + ch.len_utf8());
            }
        } else {
            matched = usize::from(ch.is_ascii() && expected.first().copied() == Some(ch as u8));
        }
    }
    None
}

pub(crate) fn drain_public_stderr_protocol_buffer(buffer: &mut String) -> String {
    let mut visible = String::new();
    loop {
        if buffer.is_empty() {
            break;
        }

        if let Some(start) = clixml_envelope_start(buffer) {
            if start > 0 {
                visible.push_str(&buffer[..start]);
                buffer.drain(..start);
                continue;
            }
            let lower = buffer.to_ascii_lowercase();
            let Some(end_start) = lower.find("</objs>") else {
                break;
            };
            let end = end_start + "</objs>".len();
            let envelope = buffer[..end].to_string();
            visible.push_str(&public_command_stderr(&envelope));
            buffer.drain(..end);
            continue;
        }

        if looks_like_clixml_protocol(buffer) {
            let fragment = std::mem::take(buffer);
            visible.push_str(&public_command_stderr(&fragment));
            break;
        }

        let hold = clixml_marker_prefix_suffix_len(buffer);
        if hold > 0 {
            let emit = buffer.len() - hold;
            visible.push_str(&buffer[..emit]);
            buffer.drain(..emit);
            break;
        }

        visible.push_str(buffer);
        buffer.clear();
        break;
    }
    visible
}

pub(crate) fn trim_utf8_front(value: &mut String, max_bytes: usize) -> bool {
    if value.len() <= max_bytes {
        return false;
    }
    let mut start = value.len().saturating_sub(max_bytes);
    while !value.is_char_boundary(start) {
        start += 1;
    }
    value.drain(..start);
    true
}

fn clixml_envelope_start(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    [lower.find("#< clixml"), lower.find("<objs")]
        .into_iter()
        .flatten()
        .min()
}

fn clixml_marker_prefix_suffix_len(value: &str) -> usize {
    let lower = value.to_ascii_lowercase();
    ["#< clixml", "<objs"]
        .into_iter()
        .map(|marker| {
            (1..marker.len())
                .rev()
                .find(|length| lower.ends_with(&marker[..*length]))
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0)
}

fn looks_like_clixml_protocol(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("#< clixml")
        || lower.contains("<objs")
        || lower.contains("</objs>")
        || lower.contains("<obj")
        || lower.contains("</obj>")
        || lower.contains("<ms")
        || lower.contains("</ms>")
        || lower.contains("s=\"progress\"")
        || lower.contains("s=\"error\"")
}

fn extract_clixml_error_strings(stderr: &str) -> String {
    let mut values = Vec::new();
    let lower = stderr.to_ascii_lowercase();
    let mut cursor = 0usize;
    while cursor < lower.len() {
        let Some(relative_start) = lower[cursor..].find("<s") else {
            break;
        };
        let start = cursor + relative_start;
        let Some(relative_open_end) = lower[start..].find('>') else {
            break;
        };
        let open_end = start + relative_open_end;
        let open = &lower[start..=open_end];
        let Some(relative_close) = lower[open_end + 1..].find("</s>") else {
            break;
        };
        let close = open_end + 1 + relative_close;
        if open == "<s>" || open.contains("s=\"error\"") {
            let decoded = decode_clixml_text(&stderr[open_end + 1..close]);
            if !decoded.trim().is_empty() {
                values.push(decoded);
            }
        }
        cursor = close + "</s>".len();
    }
    values.concat()
}

fn decode_clixml_text(value: &str) -> String {
    let xml = value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    decode_clixml_utf16_escapes(&xml)
}

fn decode_clixml_utf16_escapes(value: &str) -> String {
    fn escaped_unit(value: &str, index: usize) -> Option<u16> {
        let token = value.get(index..index + 7)?;
        (token.starts_with("_x") && token.ends_with('_'))
            .then(|| u16::from_str_radix(&token[2..6], 16).ok())
            .flatten()
    }

    let mut decoded = String::with_capacity(value.len());
    let mut index = 0usize;
    while index < value.len() {
        if let Some(unit) = escaped_unit(value, index) {
            if (0xD800..=0xDBFF).contains(&unit) {
                if let Some(low) = escaped_unit(value, index + 7) {
                    if (0xDC00..=0xDFFF).contains(&low) {
                        let scalar =
                            0x10000 + (((unit as u32 - 0xD800) << 10) | (low as u32 - 0xDC00));
                        if let Some(ch) = char::from_u32(scalar) {
                            decoded.push(ch);
                            index += 14;
                            continue;
                        }
                    }
                }
            } else if !(0xDC00..=0xDFFF).contains(&unit) {
                if let Some(ch) = char::from_u32(unit as u32) {
                    decoded.push(ch);
                    index += 7;
                    continue;
                }
            }
        }
        let ch = value[index..].chars().next().expect("valid UTF-8 boundary");
        decoded.push(ch);
        index += ch.len_utf8();
    }
    decoded
}
