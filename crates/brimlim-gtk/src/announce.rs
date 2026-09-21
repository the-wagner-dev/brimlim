//! When the notch is allowed to interrupt. Transliterated from
//! `lib/announce.js`, and kept pure for the same reason.

use std::collections::HashMap;

use brimlim_model::{Provider, SessionState, State};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Finished,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub provider_id: String,
    pub session: String,
    pub kind: Kind,
}

fn sessions_by_pid(provider: &Provider) -> HashMap<i32, &brimlim_model::Session> {
    provider.sessions.iter().map(|s| (s.pid, s)).collect()
}

/// Moments worth a reveal: a session that was working has stopped.
///
/// There used to be a second rule — any session entering `Waiting` — and it
/// was the reason the notch interrupted people who had not been asked
/// anything: the providers reached `Waiting` by noticing a session was not
/// computing, which is not the same thing at all. A reveal is an
/// interruption, so it is only spent on a transition a provider actually
/// witnessed.
///
/// `previous` is `None` for the first state after startup, which deliberately
/// announces nothing — otherwise every launch would chime once per session
/// that happened to be open.
pub fn transitions(previous: Option<&State>, next: &State) -> Vec<Event> {
    let Some(previous) = previous else {
        return Vec::new();
    };

    let before: HashMap<&str, &Provider> = previous
        .providers
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let mut events = Vec::new();

    for provider in &next.providers {
        let Some(old) = before.get(provider.id.as_str()) else {
            continue;
        };
        let old_sessions = sessions_by_pid(old);
        let new_sessions = sessions_by_pid(provider);

        for (pid, session) in &new_sessions {
            let Some(was) = old_sessions.get(pid) else {
                continue;
            };
            if was.state == SessionState::Working && session.state != SessionState::Working {
                events.push(Event {
                    provider_id: provider.id.clone(),
                    session: session.name.clone(),
                    kind: if session.state == SessionState::Waiting {
                        Kind::Waiting
                    } else {
                        Kind::Finished
                    },
                });
            }
        }

        // A session that vanished while working finished by leaving.
        for (pid, was) in &old_sessions {
            if was.state == SessionState::Working && !new_sessions.contains_key(pid) {
                events.push(Event {
                    provider_id: provider.id.clone(),
                    session: was.name.clone(),
                    kind: Kind::Finished,
                });
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use brimlim_model::{Activity, Fidelity, Session, Status};

    fn session(pid: i32, state: SessionState) -> Session {
        Session {
            name: "repo".to_owned(),
            pid,
            state,
        }
    }

    fn state(sessions: Vec<Session>) -> State {
        State::new(vec![Provider {
            id: "claude".to_owned(),
            label: "Claude".to_owned(),
            headline_percent: None,
            windows: Vec::new(),
            fidelity: Fidelity::Official,
            status: Status::Ok,
            activity: Activity::Idle,
            sessions,
            updated_at: None,
            message: None,
        }])
    }

    #[test]
    fn the_first_reading_after_startup_announces_nothing() {
        let now = state(vec![session(1, SessionState::Working)]);
        assert!(transitions(None, &now).is_empty());
    }

    #[test]
    fn a_session_that_stops_working_is_a_finish() {
        let before = state(vec![session(1, SessionState::Working)]);
        let after = state(vec![session(1, SessionState::Idle)]);
        let events = transitions(Some(&before), &after);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Finished);
    }

    #[test]
    fn a_session_that_starts_waiting_is_announced_once_not_twice() {
        let before = state(vec![session(1, SessionState::Working)]);
        let after = state(vec![session(1, SessionState::Waiting)]);
        let events = transitions(Some(&before), &after);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, Kind::Waiting);
    }

    #[test]
    fn a_session_that_was_never_working_buys_no_interruption() {
        // The regression this exists for: every session that simply was not
        // computing used to be called "waiting on you" and got itself a
        // reveal. Nothing about a session that was not working is an event,
        // whatever state it lands in.
        let before = state(vec![session(1, SessionState::Idle)]);
        for landed in [SessionState::Idle, SessionState::Waiting] {
            let after = state(vec![session(1, landed)]);
            assert!(
                transitions(Some(&before), &after).is_empty(),
                "idle -> {landed:?} should not interrupt anyone"
            );
        }
    }

    #[test]
    fn a_session_that_vanishes_mid_turn_counts_as_finished() {
        let before = state(vec![session(1, SessionState::Working)]);
        let after = state(Vec::new());
        assert_eq!(transitions(Some(&before), &after)[0].kind, Kind::Finished);
    }

    #[test]
    fn a_quiet_tick_announces_nothing() {
        let now = state(vec![session(1, SessionState::Idle)]);
        assert!(transitions(Some(&now), &now).is_empty());
    }

    #[test]
    fn a_session_appearing_already_busy_is_not_an_event() {
        let before = state(Vec::new());
        let after = state(vec![session(1, SessionState::Working)]);
        assert!(transitions(Some(&before), &after).is_empty());
    }
}
