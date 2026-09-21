//! Polling, merging and the one rule that matters: a provider that fails
//! degrades into a visible status, never into a plausible-looking number.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;

use crate::config::Config;
use crate::model::{Provider, Reading, State, Status, Usage};
use crate::providers::{PollCtx, UsageProvider, claude::ClaudeProvider, codex::CodexProvider};
use crate::store::Store;

/// A provider that wedges must not wedge the daemon.
const POLL_TIMEOUT: Duration = Duration::from_secs(8);

pub struct Engine {
    providers: Vec<Arc<dyn UsageProvider>>,
    config: Config,
    store: Mutex<Store>,
    state: RwLock<State>,
    pending_refresh: Mutex<HashSet<String>>,
}

impl Engine {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let mut providers: Vec<Arc<dyn UsageProvider>> = Vec::new();
        if config.providers.claude {
            providers.push(Arc::new(ClaudeProvider::new(config.api_interval())?));
        }
        if config.providers.codex {
            providers.push(Arc::new(CodexProvider::new()));
        }
        if providers.is_empty() {
            tracing::warn!("every provider is disabled in the config");
        }

        Ok(Self {
            providers,
            config,
            store: Mutex::new(Store::load()),
            state: RwLock::new(State::new(Vec::new())),
            pending_refresh: Mutex::new(HashSet::new()),
        })
    }

    /// Mark a provider (or, for an empty id, all of them) as needing real
    /// work on the next tick rather than a cached answer.
    pub fn request_refresh(&self, provider_id: &str) {
        let Ok(mut pending) = self.pending_refresh.lock() else {
            return;
        };
        if provider_id.is_empty() {
            pending.extend(self.providers.iter().map(|p| p.id().to_owned()));
        } else {
            pending.insert(provider_id.to_owned());
        }
    }

    fn take_refresh(&self, provider_id: &str) -> bool {
        self.pending_refresh
            .lock()
            .map(|mut p| p.remove(provider_id))
            .unwrap_or(false)
    }

    pub fn snapshot(&self) -> State {
        self.state
            .read()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn snapshot_json(&self) -> String {
        serde_json::to_string(&self.snapshot())
            .unwrap_or_else(|_| r#"{"schema":1,"providers":[]}"#.to_owned())
    }

    /// Poll every provider once and fold the results into the published
    /// state. Returns the new state and whether anything a frontend cares
    /// about actually changed.
    pub async fn tick(&self) -> (State, bool) {
        // Providers are filtered per tick rather than at startup, so
        // installing an assistant while the daemon runs makes it appear.
        let polls = self
            .providers
            .iter()
            .filter(|p| p.is_installed())
            .map(|provider| {
                let provider = Arc::clone(provider);
                let ctx = PollCtx {
                    force: self.take_refresh(provider.id()),
                };
                async move {
                    let reading =
                        match tokio::time::timeout(POLL_TIMEOUT, provider.poll(&ctx)).await {
                            Ok(reading) => reading,
                            Err(_) => Reading::empty(Status::Error, "provider timed out"),
                        };
                    (provider, reading)
                }
            });

        let results = futures_util::future::join_all(polls).await;
        let providers: Vec<Provider> = results
            .into_iter()
            .map(|(provider, reading)| self.merge(&*provider, reading))
            .collect();

        if let Ok(store) = self.store.lock()
            && let Err(error) = store.save()
        {
            tracing::warn!(%error, "could not persist readings");
        }

        let next = State::new(providers);
        let changed = {
            let mut state = self.state.write().unwrap_or_else(|e| e.into_inner());
            let changed = !state.same_content(&next);
            *state = next.clone();
            changed
        };
        (next, changed)
    }

    fn merge(&self, provider: &dyn UsageProvider, reading: Reading) -> Provider {
        let id = provider.id();
        let now = Utc::now();
        let (usage, status, message) = match reading.usage {
            Some(usage) => {
                // Remember what the provider actually said, expired windows
                // and all; pruning is a presentation decision and the next
                // poll may well be able to re-read the same log.
                if let Ok(mut store) = self.store.lock() {
                    store.put(id, usage.clone());
                }
                match usage.without_expired_windows(now) {
                    Some(usage) => {
                        let stale = usage.age(now).num_minutes() >= self.config.stale_after_minutes;
                        let status = match (reading.status, stale) {
                            (Status::Ok, true) => Status::Stale,
                            (other, _) => other,
                        };
                        (Some(usage), status, reading.message)
                    }
                    None => (
                        None,
                        Status::Stale,
                        Some(format!("{id} reading outlived its reset window")),
                    ),
                }
            }
            // No fresh numbers. Fall back to the last good reading if it is
            // still young enough to mean anything, and say so in the status.
            None => {
                let remembered = self
                    .remembered(id)
                    .and_then(|u| u.without_expired_windows(now));
                let status = match reading.status {
                    Status::Ok => Status::Stale,
                    other => other,
                };
                let message = reading.message.or_else(|| {
                    remembered
                        .is_none()
                        .then(|| format!("no usage reading for {id} yet"))
                });
                (remembered, status, message)
            }
        };

        Provider {
            id: id.to_owned(),
            label: provider.label().to_owned(),
            headline_percent: usage.as_ref().and_then(|u| u.headline_percent),
            windows: usage
                .as_ref()
                .map(|u| u.windows.clone())
                .unwrap_or_default(),
            fidelity: usage.as_ref().map_or(provider.fidelity(), |u| u.fidelity),
            status,
            activity: reading.activity,
            sessions: reading.sessions,
            updated_at: usage.as_ref().map(|u| u.read_at),
            message,
        }
    }

    fn remembered(&self, id: &str) -> Option<Usage> {
        let store = self.store.lock().ok()?;
        let usage = store.get(id)?;
        let age = usage.age(Utc::now());
        (age.num_hours() < self.config.max_reading_age_hours).then(|| usage.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Activity, Fidelity, Session, SessionState, Window};
    use async_trait::async_trait;

    struct Fake {
        reading: Mutex<Option<Reading>>,
    }

    #[async_trait]
    impl UsageProvider for Fake {
        fn id(&self) -> &str {
            "fake"
        }
        fn label(&self) -> &str {
            "Fake"
        }
        async fn poll(&self, _ctx: &PollCtx) -> Reading {
            self.reading.lock().unwrap().take().unwrap()
        }
    }

    fn engine_with(reading: Reading) -> (Engine, Arc<Fake>) {
        let fake = Arc::new(Fake {
            reading: Mutex::new(Some(reading)),
        });
        let engine = Engine {
            providers: vec![fake.clone()],
            config: Config::default(),
            store: Mutex::new(Store::default()),
            state: RwLock::new(State::new(Vec::new())),
            pending_refresh: Mutex::new(HashSet::new()),
        };
        (engine, fake)
    }

    fn usage(percent: f64) -> Usage {
        Usage::from_windows(
            vec![Window {
                name: "Session".into(),
                percent,
                resets_at: None,
            }],
            Fidelity::Official,
        )
    }

    #[tokio::test]
    async fn a_good_reading_is_published_as_is() {
        let (engine, _) = engine_with(Reading {
            usage: Some(usage(0.62)),
            status: Status::Ok,
            message: None,
            activity: Activity::Busy,
            sessions: vec![Session {
                name: "repo-x".into(),
                pid: 1,
                state: SessionState::Working,
            }],
        });

        let (state, changed) = engine.tick().await;
        assert!(changed);
        let provider = &state.providers[0];
        assert_eq!(provider.status, Status::Ok);
        assert_eq!(provider.headline_percent, Some(0.62));
        assert_eq!(provider.activity, Activity::Busy);
    }

    #[tokio::test]
    async fn a_failed_poll_with_no_history_shows_no_number() {
        let (engine, _) = engine_with(Reading::empty(Status::NeedsAuth, "not signed in"));

        let (state, _) = engine.tick().await;
        let provider = &state.providers[0];
        assert_eq!(provider.status, Status::NeedsAuth);
        assert_eq!(provider.headline_percent, None);
        assert!(provider.windows.is_empty());
        assert_eq!(provider.updated_at, None);
        assert_eq!(provider.message.as_deref(), Some("not signed in"));
    }

    #[tokio::test]
    async fn a_failed_poll_keeps_the_last_good_number_but_flags_it() {
        let (engine, fake) = engine_with(Reading {
            usage: Some(usage(0.4)),
            status: Status::Ok,
            message: None,
            activity: Activity::Idle,
            sessions: Vec::new(),
        });
        engine.tick().await;

        *fake.reading.lock().unwrap() =
            Some(Reading::empty(Status::Error, "usage endpoint returned 500"));
        let (state, _) = engine.tick().await;

        let provider = &state.providers[0];
        assert_eq!(provider.status, Status::Error);
        assert_eq!(provider.headline_percent, Some(0.4));
        assert!(
            provider.updated_at.is_some(),
            "the number must carry its own age"
        );
    }

    #[tokio::test]
    async fn nothing_new_degrades_to_stale_not_to_ok() {
        let (engine, fake) = engine_with(Reading {
            usage: Some(usage(0.4)),
            status: Status::Ok,
            message: None,
            activity: Activity::Idle,
            sessions: Vec::new(),
        });
        engine.tick().await;

        *fake.reading.lock().unwrap() =
            Some(crate::providers::nothing_new(Activity::Idle, Vec::new()));
        let (state, _) = engine.tick().await;

        assert_eq!(state.providers[0].status, Status::Stale);
        assert_eq!(state.providers[0].headline_percent, Some(0.4));
    }

    #[tokio::test]
    async fn an_ancient_reading_is_dropped_rather_than_shown() {
        let (engine, fake) = engine_with(Reading::empty(Status::Error, "boom"));
        {
            let mut store = engine.store.lock().unwrap();
            let mut old = usage(0.9);
            old.read_at = Utc::now() - chrono::Duration::days(3);
            store.put("fake", old);
        }
        *fake.reading.lock().unwrap() = Some(Reading::empty(Status::Error, "boom"));

        let (state, _) = engine.tick().await;
        assert_eq!(state.providers[0].headline_percent, None);
    }

    #[tokio::test]
    async fn a_reading_that_outlived_its_window_shows_no_number() {
        let mut expired = Usage::from_windows(
            vec![Window {
                name: "Weekly".into(),
                percent: 0.76,
                resets_at: Some(Utc::now() - chrono::Duration::hours(2)),
            }],
            Fidelity::Official,
        );
        expired.read_at = Utc::now() - chrono::Duration::days(1);

        let (engine, _) = engine_with(Reading {
            usage: Some(expired),
            status: Status::Ok,
            message: None,
            activity: Activity::Idle,
            sessions: Vec::new(),
        });

        let (state, _) = engine.tick().await;
        let provider = &state.providers[0];
        assert_eq!(provider.headline_percent, None);
        assert_eq!(provider.status, Status::Stale);
        assert!(provider.windows.is_empty());
    }

    #[tokio::test]
    async fn a_live_window_survives_while_its_expired_sibling_is_dropped() {
        let usage = Usage::from_windows(
            vec![
                Window {
                    name: "Session".into(),
                    percent: 0.9,
                    resets_at: Some(Utc::now() - chrono::Duration::minutes(5)),
                },
                Window {
                    name: "Weekly".into(),
                    percent: 0.31,
                    resets_at: Some(Utc::now() + chrono::Duration::days(2)),
                },
            ],
            Fidelity::Official,
        );

        let (engine, _) = engine_with(Reading {
            usage: Some(usage),
            status: Status::Ok,
            message: None,
            activity: Activity::Idle,
            sessions: Vec::new(),
        });

        let (state, _) = engine.tick().await;
        let provider = &state.providers[0];
        assert_eq!(provider.windows.len(), 1);
        assert_eq!(provider.windows[0].name, "Weekly");
        assert_eq!(
            provider.headline_percent,
            Some(0.31),
            "the headline must follow the surviving windows, not the dropped 0.9"
        );
    }

    #[tokio::test]
    async fn an_old_but_valid_reading_is_marked_stale() {
        let mut old = Usage::from_windows(
            vec![Window {
                name: "Weekly".into(),
                percent: 0.76,
                resets_at: Some(Utc::now() + chrono::Duration::days(2)),
            }],
            Fidelity::Official,
        );
        old.read_at = Utc::now() - chrono::Duration::hours(4);

        let (engine, _) = engine_with(Reading {
            usage: Some(old),
            status: Status::Ok,
            message: None,
            activity: Activity::Idle,
            sessions: Vec::new(),
        });

        let (state, _) = engine.tick().await;
        assert_eq!(state.providers[0].status, Status::Stale);
        assert_eq!(state.providers[0].headline_percent, Some(0.76));
    }

    #[tokio::test]
    async fn an_unchanged_state_does_not_report_a_change() {
        let (engine, fake) = engine_with(Reading {
            usage: Some(usage(0.4)),
            status: Status::Ok,
            message: None,
            activity: Activity::Idle,
            sessions: Vec::new(),
        });
        let (first, changed) = engine.tick().await;
        assert!(changed);

        *fake.reading.lock().unwrap() = Some(Reading {
            usage: Some(Usage {
                read_at: first.providers[0].updated_at.unwrap(),
                ..usage(0.4)
            }),
            status: Status::Ok,
            message: None,
            activity: Activity::Idle,
            sessions: Vec::new(),
        });
        let (_, changed) = engine.tick().await;
        assert!(!changed, "a quiet tick must not wake every frontend up");
    }
}
