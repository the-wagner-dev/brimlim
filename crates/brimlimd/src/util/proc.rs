//! Just enough /proc to answer "is this agent actually doing anything".
//!
//! There is no portable API for "is the assistant thinking right now", so we
//! measure two things that are true regardless of which CLI we are watching:
//! the process is burning CPU, and its log grew recently. Both are honest
//! signals; neither is ever turned into a usage percentage.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub fn is_alive(pid: i32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).is_dir()
}

pub fn cwd(pid: i32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// `/proc/<pid>/comm` is truncated to 15 bytes, so callers that care about
/// longer names should check [`exe_name`] too.
pub fn comm(pid: i32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_owned())
}

pub fn exe_name(pid: i32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()?
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
}

/// utime + stime in clock ticks.
fn cpu_ticks(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // comm can contain spaces and parentheses; fields are counted after the
    // last ')'.
    let rest = &stat[stat.rfind(')')? + 1..];
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // After the ')' the first field is state, so utime/stime (14th/15th of
    // the whole line) land at indices 11 and 12 here.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some(utime + stime)
}

fn clock_ticks_per_sec() -> f64 {
    // _SC_CLK_TCK is 100 on every Linux target we support; reading it via
    // libc would mean taking the dependency for one constant.
    100.0
}

pub fn pids() -> Vec<i32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .collect()
}

/// Remembers the previous CPU counter per pid so a poll can report a rate
/// rather than a lifetime total.
#[derive(Default)]
pub struct CpuSampler {
    last: HashMap<i32, (u64, Instant)>,
}

impl CpuSampler {
    /// Fraction of one core used since the previous sample for this pid.
    /// The first sample for a pid returns `None` — we have nothing to
    /// subtract from yet, and guessing would be exactly the sin we avoid.
    pub fn sample(&mut self, pid: i32) -> Option<f64> {
        let ticks = cpu_ticks(pid)?;
        let now = Instant::now();
        let previous = self.last.insert(pid, (ticks, now));
        let (prev_ticks, prev_at) = previous?;
        let elapsed = now.duration_since(prev_at).as_secs_f64();
        if elapsed <= 0.0 {
            return None;
        }
        let delta = ticks.saturating_sub(prev_ticks) as f64;
        Some(delta / clock_ticks_per_sec() / elapsed)
    }

    /// Drop pids that are gone, so a long-running daemon doesn't accumulate
    /// one entry per session ever started.
    pub fn retain_alive(&mut self) {
        self.last.retain(|pid, _| is_alive(*pid));
    }
}
