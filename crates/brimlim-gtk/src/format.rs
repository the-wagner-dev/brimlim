//! Times, as a person reads them. Transliterated from `lib/format.js`, and
//! kept pure so the two frontends can be checked against each other.

use chrono::{DateTime, Local, Utc};

/// "in 51 min" while the reset is close enough to feel, "Thu 14:00" once it
/// is far enough away that a countdown stops meaning anything, and "due" once
/// it has passed.
pub fn format_reset(at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Option<String> {
    let at = at?;
    let seconds = at.signed_duration_since(now).num_seconds();
    if seconds <= 0 {
        return Some("due".to_owned());
    }
    if seconds >= 24 * 3600 {
        return Some(at.with_timezone(&Local).format("%a %H:%M").to_string());
    }

    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;

    Some(if hours > 0 {
        format!("in {hours}h {minutes}m")
    } else if minutes > 0 {
        format!("in {minutes} min")
    } else {
        format!("in {seconds}s")
    })
}

pub fn format_age(at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(at) = at else {
        return "never read".to_owned();
    };
    let seconds = now.signed_duration_since(at).num_seconds().max(0);

    if seconds < 60 {
        return "just now".to_owned();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", hours / 24)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-20T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn a_future_reset_reads_as_a_countdown() {
        let at = now() + Duration::minutes(3 * 60 + 25);
        assert_eq!(format_reset(Some(at), now()).as_deref(), Some("in 3h 25m"));
        assert_eq!(
            format_reset(Some(now() + Duration::minutes(51)), now()).as_deref(),
            Some("in 51 min")
        );
    }

    #[test]
    fn a_reset_more_than_a_day_out_is_a_date_not_a_countdown() {
        let text = format_reset(Some(now() + Duration::hours(30)), now()).unwrap();
        assert!(
            !text.starts_with("in "),
            "a day-away countdown means nothing: {text}"
        );
        assert!(text.contains(':'), "it should name a time of day: {text}");
    }

    #[test]
    fn a_reset_in_the_past_reads_as_due_never_as_a_negative_time() {
        let at = now() - Duration::seconds(1);
        assert_eq!(format_reset(Some(at), now()).as_deref(), Some("due"));
    }

    #[test]
    fn a_missing_reset_time_produces_nothing_rather_than_a_guess() {
        assert_eq!(format_reset(None, now()), None);
    }

    #[test]
    fn an_unread_provider_says_so_instead_of_showing_an_age() {
        assert_eq!(format_age(None, now()), "never read");
        assert_eq!(format_age(Some(now()), now()), "just now");
        assert_eq!(
            format_age(Some(now() - Duration::hours(30)), now()),
            "1d ago"
        );
    }
}
