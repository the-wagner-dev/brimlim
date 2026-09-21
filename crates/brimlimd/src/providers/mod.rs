//! Provider plumbing. One trait, one contract: a poll never fails loudly and
//! never invents a number — it returns a [`Reading`] describing what it could
//! and could not learn.

pub mod claude;
pub mod codex;

use std::time::{Duration, SystemTime};

use async_trait::async_trait;

use crate::model::{Activity, Fidelity, Reading, Session, SessionState};

/// Burning this fraction of a core counts as "thinking".
const BUSY_CPU_FRACTION: f64 = 0.12;
/// A log that grew this recently means the agent is mid-turn.
const BUSY_WRITE_WINDOW: Duration = Duration::from_secs(6);

pub struct PollCtx {
    /// Set when the frontend asked for this provider explicitly; bypasses
    /// rate-limit throttles on network calls.
    pub force: bool,
}

#[async_trait]
pub trait UsageProvider: Send + Sync {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    /// Whether this assistant is on the machine at all.
    ///
    /// An assistant that was never installed is not a failure to report — it
    /// is simply absent, and an overlay should not carry a red badge for a
    /// tool the user does not use.
    fn is_installed(&self) -> bool {
        true
    }

    /// The best fidelity this provider can ever offer. Used to render a
    /// provider that currently has no reading at all with the right visual
    /// treatment, so "official but unknown" never looks like a guess.
    fn fidelity(&self) -> Fidelity {
        Fidelity::Official
    }
    /// Must not panic and must not block for long; the engine polls providers
    /// concurrently and applies its own timeout on top.
    async fn poll(&self, ctx: &PollCtx) -> Reading;
}

/// Provider-level activity from its sessions: the busiest one wins.
pub fn roll_up_activity(sessions: &[Session]) -> Activity {
    if sessions.iter().any(|s| s.state == SessionState::Working) {
        Activity::Busy
    } else if sessions.iter().any(|s| s.state == SessionState::Waiting) {
        Activity::Waiting
    } else {
        Activity::Idle
    }
}

/// The shared verdict for one session, from the two signals we trust:
/// CPU burn and log growth. `cpu` is `None` on the first sample of a pid.
///
/// It answers exactly one question — is this session computing right now —
/// and deliberately cannot return [`SessionState::Waiting`].
///
/// It used to: "alive, used in the last half hour, not computing" was
/// reported as waiting on the human. That describes someone reading their
/// screen, not an agent that has asked them something, and the notch spent
/// its time revealing itself over questions nobody had been asked. Not
/// computing is not the same as wanting you, and there is no signal here for
/// the second, so this returns nothing about it.
pub fn session_state(cpu: Option<f64>, last_write: Option<SystemTime>) -> SessionState {
    if cpu.is_some_and(|c| c >= BUSY_CPU_FRACTION) {
        return SessionState::Working;
    }
    match last_write.and_then(|t| SystemTime::now().duration_since(t).ok()) {
        Some(age) if age <= BUSY_WRITE_WINDOW => SessionState::Working,
        _ => SessionState::Idle,
    }
}

/// A short, recognisable label for a session, from the directory it runs in.
/// The home directory gets `~` rather than the username, which tells the user
/// nothing about which session they are looking at.
pub fn workspace_name(cwd: Option<&std::path::Path>, pid: i32) -> String {
    let Some(cwd) = cwd else {
        return format!("pid {pid}");
    };
    if cwd == crate::util::paths::home() {
        return "~".to_owned();
    }
    cwd.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("pid {pid}"))
}

pub fn mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// "5h", "Weekly", "3d" — a label for a rolling window given its length.
pub fn window_label(minutes: u64) -> String {
    match minutes {
        0 => "Window".to_owned(),
        m if m % (60 * 24 * 7) == 0 => {
            let weeks = m / (60 * 24 * 7);
            if weeks == 1 {
                "Weekly".to_owned()
            } else {
                format!("{weeks}w")
            }
        }
        m if m % (60 * 24) == 0 => format!("{}d", m / (60 * 24)),
        m if m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

/// Convenience for the empty-but-fine case: nothing new, nothing wrong.
pub fn nothing_new(activity: Activity, sessions: Vec<Session>) -> Reading {
    Reading {
        usage: None,
        status: crate::model::Status::Ok,
        message: None,
        activity,
        sessions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_labels_read_like_a_human_wrote_them() {
        assert_eq!(window_label(300), "5h");
        assert_eq!(window_label(10080), "Weekly");
        assert_eq!(window_label(20160), "2w");
        assert_eq!(window_label(4320), "3d");
        assert_eq!(window_label(90), "90m");
    }

    #[test]
    fn the_home_directory_is_labelled_tilde_not_by_username() {
        let home = crate::util::paths::home();
        assert_eq!(workspace_name(Some(&home), 7), "~");
        assert_eq!(
            workspace_name(Some(std::path::Path::new("/srv/repo-x")), 7),
            "repo-x"
        );
        assert_eq!(workspace_name(None, 7), "pid 7");
    }

    #[test]
    fn cpu_burn_beats_a_quiet_log() {
        let long_ago = SystemTime::now() - Duration::from_secs(3600);
        assert_eq!(
            session_state(Some(0.9), Some(long_ago)),
            SessionState::Working
        );
        assert_eq!(session_state(Some(0.0), Some(long_ago)), SessionState::Idle);
    }

    #[test]
    fn a_session_that_stopped_computing_is_idle_not_waiting_on_you() {
        // A minute ago it wrote to its log; right now it is burning nothing.
        // That is a session sitting there while you read the screen. The
        // heuristic has no way to tell it from one that asked you a
        // question, so it must not claim the difference.
        let recent = SystemTime::now() - Duration::from_secs(60);
        assert_eq!(session_state(Some(0.0), Some(recent)), SessionState::Idle);
    }

    #[test]
    fn unknown_cpu_does_not_promote_to_working() {
        assert_eq!(session_state(None, None), SessionState::Idle);
    }
}
