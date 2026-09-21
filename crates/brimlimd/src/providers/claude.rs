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
use crate::model::{
    Fidelity, Reading, Session, SessionState, Status, Usage, Window, fraction_from_percent,
};
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
                // Sampled either way: the sampler needs two readings of a
                // pid before it can report a rate, so skipping it on the
                // first-hand path would leave the fallback blind whenever a
                // session outlives an upgrade.
                let burn = cpu.as_mut().and_then(|c| c.sample(entry.pid));
                let last_write = transcripts.mtime_of(&entry.session_id);
                Some(Session {
                    name: entry.display_name(),
                    pid: entry.pid,
                    state: state_from_status(entry.status.as_deref())
                        .unwrap_or_else(|| session_state(burn, last_write)),
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

/// Why the usage endpoint cannot be asked, in the words the card will show.
///
/// The card draws this on one line at 11px inside a 280px slab — about
/// forty-five characters. A longer message is not a more helpful one, it is
/// an invisible one, and [`the budget below`](tests) keeps it that way.
///
/// Each says what is wrong *and* what fixes it, because "needs auth" on its
/// own leaves the user to guess which of several Claude clients they are
/// supposed to do something to.
const MSG_NOT_SIGNED_IN: &str = "Not signed in — run `claude` in a terminal";
const MSG_EXPIRED: &str = "Token expired — run `claude` in a terminal";
const MSG_UNREADABLE: &str = "Credentials file is unreadable";
#[cfg(test)]
const MESSAGE_BUDGET: usize = 45;

/// `~/.claude/.credentials.json`, written by whichever Claude client last
/// signed in from a terminal.
///
/// Notably *not* by the desktop app, which keeps its own tokens encrypted in
/// `~/.config/Claude/config.json` under `oauth:tokenCacheV2`, behind an
/// Electron safeStorage key held in the system keyring. For a desktop-only
/// user this file is a leftover from their last terminal login and stops
/// working eight hours later. See docs/design.md — the short version is that
/// brimlim will not reach into another application's encrypted store, and
/// will not refresh this file either, so all it can do is say so precisely.
/// Deliberately not `Debug`: a struct holding an access token should not be
/// one that a stray `{:?}` can put in a log file.
struct Credentials {
    access_token: String,
}

impl Credentials {
    fn load() -> Result<Self, String> {
        let path = paths::claude_dir().join(".credentials.json");
        // A missing file and an unreadable one are different problems with
        // different answers, and neither of them is "expired".
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Err(MSG_NOT_SIGNED_IN.to_owned());
        };
        Self::parse(&raw, Utc::now().timestamp_millis())
    }

    /// Split out from [`Self::load`] so every branch can be tested without a
    /// home directory to stand in.
    fn parse(raw: &str, now_ms: i64) -> Result<Self, String> {
        let file: CredentialsFile =
            serde_json::from_str(raw).map_err(|_| MSG_UNREADABLE.to_owned())?;
        let oauth = file.oauth.ok_or_else(|| MSG_NOT_SIGNED_IN.to_owned())?;

        if oauth.expires_at.is_some_and(|at| at <= now_ms) {
            return Err(MSG_EXPIRED.to_owned());
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
    /// Claude Code's own verdict on what this session is doing: `busy` while
    /// it is working a turn, `idle` when it is not. It is written by the CLI
    /// itself, which makes it the only first-hand account of agent activity
    /// available on this machine — everything else is inference from the
    /// outside. Absent on CLI versions that predate the field, which is the
    /// one case where we fall back to guessing.
    status: Option<String>,
}

/// What Claude Code's own registry says a session is doing, or `None` when
/// the CLI is too old to have written it down and we have to fall back to
/// watching it from the outside.
fn state_from_status(status: Option<&str>) -> Option<SessionState> {
    match status? {
        "busy" => Some(SessionState::Working),
        // Anything the CLI does not call `busy`, it is not doing. In
        // particular it is not "waiting on you": Claude Code reports whether
        // it is working, never whether it has asked you something, and the
        // difference is the whole reason this function exists.
        _ => Some(SessionState::Idle),
    }
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
            status: None,
        };
        assert_eq!(entry.display_name(), "brimlim");
    }

    /// A credentials file with obviously fake values. Shaped like the real
    /// one; nothing in here is a secret, in this repository or anywhere.
    fn credentials_json(expires_at: i64) -> String {
        format!(
            r#"{{"claudeAiOauth":{{"accessToken":"not-a-real-token",
               "refreshToken":"also-not-real","expiresAt":{expires_at},
               "scopes":["user:inference"],"subscriptionType":"max"}}}}"#
        )
    }

    /// `Credentials` is not `Debug`, on purpose, so assertions go through
    /// the one field a test is allowed to look at.
    fn token_of(result: Result<Credentials, String>) -> Result<String, String> {
        result.map(|c| c.access_token)
    }

    #[test]
    fn a_live_token_is_accepted() {
        assert_eq!(
            token_of(Credentials::parse(&credentials_json(2_000), 1_000)),
            Ok("not-a-real-token".to_owned())
        );
    }

    #[test]
    fn each_way_of_having_no_token_says_something_different() {
        // "needs auth" on its own leaves the user guessing which of several
        // Claude clients they are meant to do something to. Every branch
        // names the remedy, and no two branches say the same thing.
        let expired = token_of(Credentials::parse(&credentials_json(1_000), 2_000)).unwrap_err();
        let signed_out = token_of(Credentials::parse(r#"{"mcpOAuth":{}}"#, 0)).unwrap_err();
        let unreadable = token_of(Credentials::parse("{ this is not json", 0)).unwrap_err();

        assert_eq!(expired, MSG_EXPIRED);
        assert_eq!(signed_out, MSG_NOT_SIGNED_IN);
        assert_eq!(unreadable, MSG_UNREADABLE);
        assert_ne!(expired, signed_out);
    }

    #[test]
    fn a_token_with_no_expiry_is_not_assumed_to_be_expired() {
        // Absent is not zero. A file that never carried an expiry is not a
        // file whose token expired in 1970.
        let raw = r#"{"claudeAiOauth":{"accessToken":"not-a-real-token"}}"#;
        assert!(token_of(Credentials::parse(raw, i64::MAX)).is_ok());
    }

    #[test]
    fn every_auth_message_fits_on_the_card() {
        // The card draws one line at 11px inside a 280px slab. A message
        // that overflows it is not a worse message, it is an invisible one.
        for message in [MSG_NOT_SIGNED_IN, MSG_EXPIRED, MSG_UNREADABLE] {
            let width = message.chars().count();
            assert!(
                width <= MESSAGE_BUDGET,
                "{message:?} is {width} characters, budget is {MESSAGE_BUDGET}"
            );
        }
    }

    #[test]
    fn the_cli_gets_the_last_word_on_what_it_is_doing() {
        assert_eq!(state_from_status(Some("busy")), Some(SessionState::Working));
        assert_eq!(state_from_status(Some("idle")), Some(SessionState::Idle));
    }

    #[test]
    fn an_unrecognised_status_is_not_working_and_is_never_waiting() {
        // A future CLI could add a status we have not seen. Whatever it
        // means, it does not mean "this agent has asked you something" —
        // only a signal that says so may produce Waiting.
        for status in ["compacting", "paused", ""] {
            assert_eq!(state_from_status(Some(status)), Some(SessionState::Idle));
        }
    }

    #[test]
    fn a_registry_without_a_status_falls_back_rather_than_guessing_idle() {
        // Older Claude Code wrote no status at all. Reporting those sessions
        // as idle would be a claim; falling back to watching the process is
        // what the heuristic is for.
        assert_eq!(state_from_status(None), None);
    }
}
