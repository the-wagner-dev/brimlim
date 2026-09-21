//! Codex CLI.
//!
//! Codex writes the server's own rate-limit payload into every rollout log as
//! part of its `token_count` events, so the numbers here are `official` —
//! read off disk, with no second network call to anyone's API.
//!
//! Sessions are found by scanning /proc, because Codex keeps no registry of
//! running CLIs the way Claude Code does.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;

use super::{
    PollCtx, UsageProvider, mtime, roll_up_activity, session_state, window_label, workspace_name,
};
use crate::model::{Fidelity, Reading, Session, Status, Usage, Window, fraction_from_percent};
use crate::util::{paths, proc, tail};

/// How far back in the sessions/YYYY/MM/DD tree to look before giving up.
const SCAN_YEARS: usize = 2;
const SCAN_MONTHS: usize = 2;
const SCAN_DAYS: usize = 4;
/// Rollouts that could plausibly hold the newest reading.
const CANDIDATE_FILES: usize = 5;
/// The rate-limit record sits at the very end of an active rollout.
const TAIL_BYTES: u64 = 512 * 1024;

pub struct CodexProvider {
    cpu: Mutex<CpuState>,
    cache: Mutex<Option<CachedRollout>>,
}

#[derive(Default)]
struct CpuState {
    sampler: proc::CpuSampler,
}

/// Remembers which file we parsed and how old it was, so an unchanged rollout
/// costs one stat() instead of half a megabyte of reads every tick.
struct CachedRollout {
    path: PathBuf,
    mtime: SystemTime,
    usage: Usage,
}

impl CodexProvider {
    pub fn new() -> Self {
        Self {
            cpu: Mutex::new(CpuState::default()),
            cache: Mutex::new(None),
        }
    }

    fn sessions(&self) -> Vec<Session> {
        let newest_write = self.newest_rollout_mtime();
        let mut guard = self.cpu.lock().ok();
        if let Some(state) = guard.as_mut() {
            state.sampler.retain_alive();
        }

        let mut sessions: Vec<Session> = proc::pids()
            .into_iter()
            .filter(|pid| is_codex_process(*pid))
            .map(|pid| {
                let burn = guard.as_mut().and_then(|s| s.sampler.sample(pid));
                let name = workspace_name(proc::cwd(pid).as_deref(), pid);
                Session {
                    name,
                    pid,
                    state: session_state(burn, newest_write),
                }
            })
            .collect();

        sessions.sort_by(|a, b| a.name.cmp(&b.name).then(a.pid.cmp(&b.pid)));
        sessions
    }

    fn newest_rollout_mtime(&self) -> Option<SystemTime> {
        recent_rollouts().first().map(|(time, _)| *time)
    }

    /// `None` means "no new reading this tick" — either nothing changed or
    /// nothing on disk carries limits yet. The engine turns that into a
    /// status; it never becomes a number here.
    fn read_usage(&self) -> Option<Usage> {
        let candidates = recent_rollouts();
        let (newest_mtime, newest_path) = candidates.first()?;

        if let Ok(cache) = self.cache.lock()
            && let Some(cached) = cache.as_ref()
            && cached.path == *newest_path
            && cached.mtime == *newest_mtime
        {
            return Some(cached.usage.clone());
        }

        let mut best: Option<(DateTime<Utc>, Usage)> = None;
        for (_, path) in candidates.iter().take(CANDIDATE_FILES) {
            let Some((at, usage)) = read_rate_limits(path) else {
                continue;
            };
            if best.as_ref().is_none_or(|(best_at, _)| at > *best_at) {
                best = Some((at, usage));
            }
        }

        let (_, usage) = best?;
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(CachedRollout {
                path: newest_path.clone(),
                mtime: *newest_mtime,
                usage: usage.clone(),
            });
        }
        Some(usage)
    }
}

impl Default for CodexProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UsageProvider for CodexProvider {
    fn id(&self) -> &str {
        "codex"
    }

    fn label(&self) -> &str {
        "Codex"
    }

    fn is_installed(&self) -> bool {
        paths::codex_dir().is_dir()
    }

    async fn poll(&self, _ctx: &PollCtx) -> Reading {
        let root = paths::codex_dir();
        if !root.is_dir() {
            return Reading::empty(
                Status::Error,
                format!("no Codex home at {}", root.display()),
            );
        }

        let sessions = self.sessions();
        let activity = roll_up_activity(&sessions);

        match self.read_usage() {
            Some(usage) => Reading {
                usage: Some(usage),
                status: Status::Ok,
                message: None,
                activity,
                sessions,
            },
            None => super::nothing_new(activity, sessions),
        }
    }
}

fn is_codex_process(pid: i32) -> bool {
    // comm is truncated at 15 bytes, and a wrapper script can leave it as the
    // interpreter name, so check the resolved binary too.
    proc::comm(pid).is_some_and(|c| c == "codex")
        || proc::exe_name(pid).is_some_and(|e| e == "codex")
}

/// Rollout files under `~/.codex/sessions/YYYY/MM/DD`, newest first.
///
/// The tree is walked depth-bounded rather than recursively: a year of daily
/// directories is a lot of readdir() for a poll that runs every few seconds.
fn recent_rollouts() -> Vec<(SystemTime, PathBuf)> {
    let root = paths::codex_dir().join("sessions");
    let mut files = Vec::new();

    for year in newest_children(&root, SCAN_YEARS) {
        for month in newest_children(&year, SCAN_MONTHS) {
            for day in newest_children(&month, SCAN_DAYS) {
                let Ok(entries) = std::fs::read_dir(&day) else {
                    continue;
                };
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    let is_rollout = path.extension().is_some_and(|x| x == "jsonl")
                        && path
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("rollout-"));
                    if !is_rollout {
                        continue;
                    }
                    if let Some(time) = mtime(&path) {
                        files.push((time, path));
                    }
                }
            }
        }
    }

    files.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    files
}

/// The `limit` lexicographically-largest subdirectories, which for a
/// YYYY/MM/DD tree means the most recent ones.
fn newest_children(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    children.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    children.truncate(limit);
    children
}

/// Pull the last `rate_limits` payload out of one rollout, with the timestamp
/// of the event that carried it.
fn read_rate_limits(path: &Path) -> Option<(DateTime<Utc>, Usage)> {
    let value = tail::last_json_line_containing(path, "\"rate_limits\"", TAIL_BYTES).ok()??;
    let event: RolloutEvent = serde_json::from_value(value).ok()?;
    let limits = event.payload.rate_limits?;

    let windows: Vec<Window> = [limits.primary.as_ref(), limits.secondary.as_ref()]
        .into_iter()
        .flatten()
        .map(RateWindow::to_model)
        .collect();

    if windows.is_empty() {
        return None;
    }
    let mut usage = Usage::from_windows(windows, Fidelity::Official);
    // The reading is as old as the event, not as old as our parse of it.
    usage.read_at = event.timestamp;
    Some((event.timestamp, usage))
}

#[derive(Deserialize)]
struct RolloutEvent {
    timestamp: DateTime<Utc>,
    payload: RolloutPayload,
}

#[derive(Deserialize)]
struct RolloutPayload {
    rate_limits: Option<RateLimits>,
}

#[derive(Deserialize)]
struct RateLimits {
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
}

#[derive(Deserialize)]
struct RateWindow {
    used_percent: f64,
    window_minutes: u64,
    /// Unix seconds.
    resets_at: Option<i64>,
}

impl RateWindow {
    fn to_model(&self) -> Window {
        Window {
            name: window_label(self.window_minutes),
            percent: fraction_from_percent(self.used_percent),
            resets_at: self
                .resets_at
                .and_then(|s| Utc.timestamp_opt(s, 0).single()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn rollout_with(body: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new()
            .suffix(".jsonl")
            .tempfile()
            .unwrap();
        writeln!(
            file,
            r#"{{"type":"event_msg","payload":{{"type":"agent_message"}}}}"#
        )
        .unwrap();
        writeln!(file, "{body}").unwrap();
        file.flush().unwrap();
        file
    }

    /// A real rollout tail, trimmed to the records under test. Codex writes
    /// the server's own payload here, so this file is the provider contract.
    fn recorded_rollout() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/codex-rollout.jsonl")
    }

    #[test]
    fn reads_the_official_window_out_of_a_recorded_rollout() {
        let (at, usage) = read_rate_limits(&recorded_rollout()).unwrap();

        assert_eq!(
            at.to_rfc3339(),
            "2026-09-19T16:46:48.200+00:00",
            "the last rate-limit record wins, not the first"
        );
        assert_eq!(usage.fidelity, Fidelity::Official);
        assert_eq!(usage.headline_percent, Some(0.76));
        assert_eq!(
            usage.windows.len(),
            1,
            "a null secondary is not a zeroed window"
        );
        assert_eq!(usage.windows[0].name, "Weekly");
        assert_eq!(
            usage.read_at, at,
            "the reading is as old as the event, not as old as our parse"
        );
        assert_eq!(
            usage.windows[0].resets_at.unwrap().to_rfc3339(),
            "2026-09-22T12:54:08+00:00"
        );
    }

    #[test]
    fn both_windows_are_published_when_the_server_sends_both() {
        let file = rollout_with(
            r#"{"timestamp":"2026-09-19T16:46:48.200Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":76.0,"window_minutes":10080,"resets_at":1790081648},"secondary":{"used_percent":12.5,"window_minutes":300,"resets_at":1790000000}}}}"#,
        );
        let (_, usage) = read_rate_limits(file.path()).unwrap();

        assert_eq!(usage.windows[0].name, "Weekly");
        assert_eq!(usage.windows[1].name, "5h");
        assert!((usage.windows[1].percent - 0.125).abs() < 1e-9);
        assert_eq!(usage.headline_percent, Some(0.76));
    }

    #[test]
    fn a_rollout_without_limits_yields_nothing_rather_than_zero() {
        let file = rollout_with(
            r#"{"timestamp":"2026-09-19T16:46:48.200Z","type":"event_msg","payload":{"type":"token_count","rate_limits":null}}"#,
        );
        assert!(read_rate_limits(file.path()).is_none());
    }

    #[test]
    fn a_window_without_a_reset_time_is_still_usable() {
        let file = rollout_with(
            r#"{"timestamp":"2026-09-19T16:46:48.200Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":76.0,"window_minutes":10080,"resets_at":null},"secondary":null}}}"#,
        );
        let (_, usage) = read_rate_limits(file.path()).unwrap();
        assert_eq!(usage.windows.len(), 1);
        assert!(usage.windows[0].resets_at.is_none());
    }
}
