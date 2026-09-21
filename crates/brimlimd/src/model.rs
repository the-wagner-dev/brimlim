//! Daemon-side view of the model: the shared wire types, plus the one type
//! that never leaves this process.

pub use brimlim_model::*;

/// What a provider hands back from one poll.
///
/// `usage: None` is a normal, non-alarming outcome: it means "nothing new to
/// report this tick". The engine decides what that does to the displayed
/// status, because only it knows whether a cached reading exists.
#[derive(Debug, Clone)]
pub struct Reading {
    pub usage: Option<Usage>,
    pub status: Status,
    pub message: Option<String>,
    pub activity: Activity,
    pub sessions: Vec<Session>,
}

impl Reading {
    pub fn empty(status: Status, message: impl Into<String>) -> Self {
        Self {
            usage: None,
            status,
            message: Some(message.into()),
            activity: Activity::Idle,
            sessions: Vec::new(),
        }
    }
}
