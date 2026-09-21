//! The wire model — the only thing a frontend is allowed to know about, and
//! the reason it is a crate rather than a module: the daemon and the GTK
//! frontend share this definition instead of each carrying their own copy of
//! the JSON shape.
//!
//! Two invariants hold everywhere below:
//!   * a percent is either a real reading or `null` — never a guess;
//!   * every failure is representable as a `status`, so no code path is
//!     tempted to invent a number in order to stay on the happy path.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub schema: u32,
    pub generated_at: DateTime<Utc>,
    pub providers: Vec<Provider>,
}

impl State {
    pub fn new(providers: Vec<Provider>) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            generated_at: Utc::now(),
            providers,
        }
    }

    /// Equality that ignores `generated_at`, so a quiet tick doesn't look like
    /// a change and wake every frontend up.
    pub fn same_content(&self, other: &Self) -> bool {
        self.schema == other.schema && self.providers == other.providers
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub label: String,
    /// 0.0..=1.0, or `null` when there is no reading to show.
    pub headline_percent: Option<f64>,
    pub windows: Vec<Window>,
    pub fidelity: Fidelity,
    pub status: Status,
    pub activity: Activity,
    pub sessions: Vec<Session>,
    /// When the numbers above were actually read from the provider.
    /// `null` together with `headline_percent: null` means "never read".
    pub updated_at: Option<DateTime<Utc>>,
    /// Human-readable detail for a non-`ok` status. Shown, not parsed.
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub name: String,
    /// 0.0..=1.0.
    pub percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub name: String,
    pub pid: i32,
    pub state: SessionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// The provider published this number itself.
    Official,
    /// We computed it from something the provider left lying around.
    Derived,
    /// The user typed it in.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    /// Numbers are real but old — the last poll produced nothing new.
    Stale,
    NeedsAuth,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Idle,
    Busy,
    /// Alive, turn finished, waiting on the human.
    Waiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Working,
    Waiting,
    Idle,
}

/// The numeric half of a reading — the part worth remembering across restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub headline_percent: Option<f64>,
    pub windows: Vec<Window>,
    pub fidelity: Fidelity,
    pub read_at: DateTime<Utc>,
}

impl Usage {
    /// Headline as the most constraining window, which is what the user is
    /// about to hit first.
    pub fn from_windows(windows: Vec<Window>, fidelity: Fidelity) -> Self {
        let mut usage = Self {
            headline_percent: None,
            windows,
            fidelity,
            read_at: Utc::now(),
        };
        usage.recompute_headline();
        usage
    }

    fn recompute_headline(&mut self) {
        let headline = self
            .windows
            .iter()
            .map(|w| w.percent)
            .fold(f64::NAN, f64::max);
        self.headline_percent = (!headline.is_nan()).then_some(headline);
    }

    /// Drop windows whose reset moment has already passed.
    ///
    /// Past its reset a window's percentage is not merely old, it is about a
    /// period that no longer exists — carrying it forward would show a
    /// number nobody measured. Returns `None` if nothing usable is left.
    pub fn without_expired_windows(mut self, now: DateTime<Utc>) -> Option<Self> {
        self.windows
            .retain(|w| w.resets_at.is_none_or(|resets_at| resets_at > now));
        if self.windows.is_empty() {
            return None;
        }
        self.recompute_headline();
        self.headline_percent.is_some().then_some(self)
    }

    pub fn age(&self, now: DateTime<Utc>) -> chrono::Duration {
        now.signed_duration_since(self.read_at)
    }
}

/// Clamp a raw 0..=100 percentage into the 0.0..=1.0 fraction the model uses.
pub fn fraction_from_percent(raw: f64) -> f64 {
    (raw / 100.0).clamp(0.0, 1.0)
}
