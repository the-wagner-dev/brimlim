//! Claude Code.
//!
//! Limits come from the same OAuth endpoint the CLI's own `/usage` uses, with
//! the token Claude Code already refreshed onto disk — so the numbers are
//! `official` or they are absent. We never refresh the token ourselves: that
//! would race the CLI for the same file. An expired token is `needs_auth`.
//!
//! Sessions come from `~/.claude/sessions/<pid>.json`, which Claude Code
//! maintains as a registry of live CLIs.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{PollCtx, UsageProvider, mtime, roll_up_activity, session_state, workspace_name};
use crate::model::{Fidelity, Reading, Session, Status, Usage, Window, fraction_from_percent};
use crate::util::{paths, proc::CpuSampler};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";

/// After a 429, wait at least this long before asking again, doubling on
/// each further refusal up to [`MAX_BACKOFF`].
const MIN_BACKOFF: Duration = Duration::from_secs(120);
const MAX_BACKOFF: Duration = Duration::from_secs(20 * 60);

pub struct ClaudeProvider {
    http: reqwest::Client,
    min_api_interval: Duration,
    cpu: Mutex<CpuSampler>,
    cache: Mutex<Option<(Instant, Usage)>>,
    /// When the endpoint has told us to slow down, and by how much.
    backoff: Mutex<Option<(Instant, Duration)>>,
}

impl ClaudeProvider {
    pub fn new(min_api_interval: Duration) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("brimlimd/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            min_api_interval,
            cpu: Mutex::new(CpuSampler::default()),
            cache: Mutex::new(None),
            backoff: Mutex::new(None),
        })
    }

    fn cached(&self) -> Option<Usage> {
        let cache = self.cache.lock().ok()?;
        let (at, usage) = cache.as_ref()?;
        (at.elapsed() < self.interval()).then(|| usage.clone())
    }

    /// The configured interval, or the backoff the endpoint asked for.
    fn interval(&self) -> Duration {
        let Ok(backoff) = self.backoff.lock() else {
            return self.min_api_interval;
        };
        match backoff.as_ref() {
            Some((at, wait)) if at.elapsed() < *wait => (*wait).max(self.min_api_interval),
            _ => self.min_api_interval,
        }
    }

    /// Any answer at all means we are talking to the endpoint again.
    fn clear_backoff(&self) {
        if let Ok(mut backoff) = self.backoff.lock() {
            *backoff = None;
        }
    }

    /// Honour `Retry-After` when the endpoint sends one, and otherwise
    /// double what we waited last time.
    fn note_rate_limit(&self, retry_after: Option<Duration>) -> Duration {
        let Ok(mut backoff) = self.backoff.lock() else {
            return MIN_BACKOFF;
        };
        let previous = backoff.map(|(_, wait)| wait).unwrap_or(MIN_BACKOFF / 2);
        let wait = retry_after
            .unwrap_or(previous * 2)
            .clamp(MIN_BACKOFF, MAX_BACKOFF);
        *backoff = Some((Instant::now(), wait));
        wait
    }

    fn remember(&self, usage: &Usage) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some((Instant::now(), usage.clone()));
        }
    }

    async fn fetch_usage(&self, token: &str) -> Result<Usage, (Status, String)> {
        let response = self
            .http
            .get(USAGE_URL)
            .bearer_auth(token)
            .header("anthropic-beta", OAUTH_BETA)
            .send()
            .await
            .map_err(|e| (Status::Error, format!("usage request failed: {e}")))?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err((
                Status::NeedsAuth,
                "Claude rejected the stored token".to_owned(),
            ));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // Being rate-limited says nothing about the numbers we already
            // have, so this is staleness, not an error: back off, keep the
            // last reading, and let the UI age it.
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);
            let wait = self.note_rate_limit(retry_after);
            return Err((
                Status::Stale,
                format!(
                    "rate-limited; asking again in {}m",
                    wait.as_secs().div_ceil(60)
                ),
            ));
        }
        self.clear_backoff();
        if !status.is_success() {
            return Err((Status::Error, format!("usage endpoint returned {status}")));
        }

        let body: UsageResponse = response
            .json()
            .await
            .map_err(|e| (Status::Error, format!("unreadable usage response: {e}")))?;

        let windows = body.windows();
        if windows.is_empty() {
            return Err((
                Status::Error,
                "usage response carried no windows".to_owned(),
            ));
        }
        Ok(Usage::from_windows(windows, Fidelity::Official))
    }

    fn sessions(&self) -> Vec<Session> {
        let dir = paths::claude_dir().join("sessions");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let transcripts = TranscriptIndex::build();
        let mut cpu = self.cpu.lock().ok();
        if let Some(cpu) = cpu.as_mut() {
            cpu.retain_alive();
        }

        let mut sessions: Vec<Session> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let raw = std::fs::read_to_string(e.path()).ok()?;
                let entry: SessionFile = serde_json::from_str(&raw).ok()?;
                if !crate::util::proc::is_alive(entry.pid) {
                    return None;
                }
                let burn = cpu.as_mut().and_then(|c| c.sample(entry.pid));
                let last_write = transcripts.mtime_of(&entry.session_id);
                Some(Session {
                    name: entry.display_name(),
                    pid: entry.pid,
                    state: session_state(burn, last_write),
                })
            })
            .collect();

        sessions.sort_by(|a, b| a.name.cmp(&b.name).then(a.pid.cmp(&b.pid)));
        sessions
    }
}

#[async_trait]
impl UsageProvider for ClaudeProvider {
    fn id(&self) -> &str {
        "claude"
    }

    fn label(&self) -> &str {
        "Claude"
    }

    fn is_installed(&self) -> bool {
        paths::claude_dir().is_dir()
    }

    async fn poll(&self, ctx: &PollCtx) -> Reading {
        let sessions = self.sessions();
        let activity = roll_up_activity(&sessions);

        // Serving the in-memory cache keeps a 3-second UI tick from turning
        // into a 3-second poll of someone else's API.
        if !ctx.force
            && let Some(usage) = self.cached()
        {
            return Reading {
                usage: Some(usage),
                status: Status::Ok,
                message: None,
                activity,
                sessions,
            };
        }

        let credentials = match Credentials::load() {
            Ok(credentials) => credentials,
            Err(message) => {
                return Reading {
                    activity,
                    sessions,
                    ..Reading::empty(Status::NeedsAuth, message)
                };
            }
        };

        match self.fetch_usage(&credentials.access_token).await {
            Ok(usage) => {
                self.remember(&usage);
                Reading {
                    usage: Some(usage),
                    status: Status::Ok,
                    message: None,
                    activity,
                    sessions,
                }
            }
            Err((status, message)) => Reading {
                activity,
                sessions,
                ..Reading::empty(status, message)
            },
        }
    }
}

/// `~/.claude/.credentials.json`, written and refreshed by Claude Code.
struct Credentials {
    access_token: String,
}

impl Credentials {
    fn load() -> Result<Self, String> {
        let path = paths::claude_dir().join(".credentials.json");
        let raw = std::fs::read_to_string(&path)
            .map_err(|_| format!("no Claude credentials at {}", path.display()))?;
        let file: CredentialsFile =
            serde_json::from_str(&raw).map_err(|e| format!("unreadable credentials: {e}"))?;
        let oauth = file
            .oauth
            .ok_or_else(|| "not signed in to Claude".to_owned())?;

        // Claude Code refreshes this file itself; if it has gone stale the
        // honest answer is "go run the CLI", not a stale number.
        if let Some(expires_at) = oauth.expires_at
            && expires_at <= Utc::now().timestamp_millis()
        {
            return Err("Claude token expired — run `claude` to refresh".to_owned());
        }
        Ok(Self {
            access_token: oauth.access_token,
        })
    }
}

#[derive(Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OauthEntry>,
}

#[derive(Deserialize)]
struct OauthEntry {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    five_hour: Option<UsageWindow>,
    seven_day: Option<UsageWindow>,
    seven_day_opus: Option<UsageWindow>,
}

impl UsageResponse {
    fn windows(&self) -> Vec<Window> {
        [
            ("Session", self.five_hour.as_ref()),
            ("Weekly", self.seven_day.as_ref()),
            ("Weekly Opus", self.seven_day_opus.as_ref()),
        ]
        .into_iter()
        .filter_map(|(name, window)| window?.to_model(name))
        .collect()
    }
}

#[derive(Debug, Deserialize)]
struct UsageWindow {
    /// 0..=100, absent while a window is not in force for this account.
    utilization: Option<f64>,
    resets_at: Option<DateTime<Utc>>,
}

impl UsageWindow {
    fn to_model(&self, name: &str) -> Option<Window> {
        Some(Window {
            name: name.to_owned(),
            percent: fraction_from_percent(self.utilization?),
            resets_at: self.resets_at,
        })
    }
}

#[derive(Deserialize)]
struct SessionFile {
    pid: i32,
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: Option<String>,
    name: Option<String>,
}

impl SessionFile {
    fn display_name(&self) -> String {
        if let Some(name) = self.name.as_ref().filter(|n| !n.is_empty()) {
            return name.clone();
        }
        workspace_name(self.cwd.as_deref().map(Path::new), self.pid)
    }
}

/// Maps a session id to its transcript, so "did this session write anything
/// recently" is a stat() rather than a search.
struct TranscriptIndex {
    root: PathBuf,
}

impl TranscriptIndex {
    fn build() -> Self {
        Self {
            root: paths::claude_dir().join("projects"),
        }
    }

    fn mtime_of(&self, session_id: &str) -> Option<std::time::SystemTime> {
        let file = format!("{session_id}.jsonl");
        // Transcripts live under a per-project directory whose name is the
        // mangled cwd; matching the file name is cheaper and more robust than
        // reproducing the mangling.
        std::fs::read_dir(&self.root)
            .ok()?
            .filter_map(Result::ok)
            .find_map(|project| {
                let candidate = project.path().join(&file);
                candidate.is_file().then(|| mtime(&candidate))?
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded verbatim from api.anthropic.com/api/oauth/usage. This test
    /// is the contract: if the response shape drifts, it fails here rather
    /// than silently producing a wrong overlay.
    const RECORDED_USAGE: &str = include_str!("../../fixtures/claude-usage.json");

    #[test]
    fn the_recorded_response_maps_to_the_windows_we_publish() {
        let parsed: UsageResponse = serde_json::from_str(RECORDED_USAGE).unwrap();
        let windows = parsed.windows();

        assert_eq!(
            windows.len(),
            2,
            "only the windows in force for the account"
        );
        assert_eq!(windows[0].name, "Session");
        assert!((windows[0].percent - 0.02).abs() < 1e-9);
        assert_eq!(windows[1].name, "Weekly");
        assert!((windows[1].percent - 0.33).abs() < 1e-9);
        assert_eq!(
            windows[0].resets_at.unwrap().to_rfc3339(),
            "2026-09-20T21:50:00.466212+00:00"
        );

        let usage = Usage::from_windows(windows, Fidelity::Official);
        assert_eq!(
            usage.headline_percent,
            Some(0.33),
            "the tightest window leads"
        );
        assert_eq!(usage.fidelity, Fidelity::Official);
    }

    #[test]
    fn unknown_fields_in_a_future_response_do_not_break_the_parse() {
        let raw = r#"{"five_hour":{"utilization":5.0,"resets_at":null},
                      "nimbus_quill":{"utilization":0.0},"something_new":42}"#;
        let parsed: UsageResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.windows().len(), 1);
    }

    #[test]
    fn a_window_without_a_number_is_dropped_not_zeroed() {
        let raw = r#"{"five_hour": {"utilization": null, "resets_at": null}}"#;
        let parsed: UsageResponse = serde_json::from_str(raw).unwrap();
        assert!(parsed.windows().is_empty());
    }

    fn provider() -> ClaudeProvider {
        ClaudeProvider::new(Duration::from_secs(300)).unwrap()
    }

    #[test]
    fn a_rate_limit_backs_off_and_then_doubles() {
        let claude = provider();
        assert_eq!(claude.interval(), Duration::from_secs(300));

        let first = claude.note_rate_limit(None);
        assert_eq!(first, MIN_BACKOFF);
        let second = claude.note_rate_limit(None);
        assert_eq!(second, MIN_BACKOFF * 2);
    }

    #[test]
    fn a_retry_after_header_wins_over_our_own_guess() {
        let claude = provider();
        let wait = claude.note_rate_limit(Some(Duration::from_secs(9 * 60)));
        assert_eq!(wait, Duration::from_secs(9 * 60));
    }

    #[test]
    fn the_backoff_is_clamped_at_both_ends() {
        let claude = provider();
        assert_eq!(
            claude.note_rate_limit(Some(Duration::from_secs(1))),
            MIN_BACKOFF
        );
        assert_eq!(
            claude.note_rate_limit(Some(Duration::from_secs(9999))),
            MAX_BACKOFF
        );
    }

    #[test]
    fn a_successful_answer_clears_the_backoff() {
        let claude = provider();
        claude.note_rate_limit(Some(Duration::from_secs(600)));
        assert_eq!(claude.interval(), Duration::from_secs(600));

        claude.clear_backoff();
        assert_eq!(claude.interval(), Duration::from_secs(300));
    }

    #[test]
    fn session_name_falls_back_to_the_directory() {
        let entry = SessionFile {
            pid: 42,
            session_id: "s".to_owned(),
            cwd: Some("/mnt/data/Dev/git/prototypes/brimlim".to_owned()),
            name: None,
        };
        assert_eq!(entry.display_name(), "brimlim");
    }
}
