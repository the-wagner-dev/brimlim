//! Reading the end of an append-only log without reading the whole thing.
//!
//! Codex rollouts and Claude transcripts both reach tens of megabytes; the
//! interesting record is always near the end.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Last `max_bytes` of the file, decoded lossily and split into whole lines.
/// A partial first line (we almost certainly landed mid-record) is dropped.
pub fn tail_lines(path: &Path, max_bytes: u64) -> std::io::Result<Vec<String>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let from = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(from))?;

    let mut buf = Vec::with_capacity(max_bytes.min(len) as usize);
    file.take(max_bytes).read_to_end(&mut buf)?;

    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    Ok(lines)
}

/// Walk the tail backwards and hand back the first line that both contains
/// `needle` and parses as JSON. Cheap pre-filter, then the real parse.
pub fn last_json_line_containing(
    path: &Path,
    needle: &str,
    max_bytes: u64,
) -> std::io::Result<Option<serde_json::Value>> {
    let lines = tail_lines(path, max_bytes)?;
    for line in lines.iter().rev() {
        if !line.contains(needle) {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
