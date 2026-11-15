use chatclient::connected::{MessageRow, RowKind};
use chrono::{DateTime, Local, NaiveDate};

pub fn local_time(message: &MessageRow) -> Option<DateTime<Local>> {
    DateTime::from_timestamp_millis(message.timestamp_ms?).map(|time| time.with_timezone(&Local))
}
pub fn date(message: &MessageRow) -> Option<NaiveDate> {
    local_time(message).map(|time| time.date_naive())
}
pub fn grouped(previous: Option<&MessageRow>, current: &MessageRow) -> bool {
    previous.is_some_and(|previous| {
        previous.kind == RowKind::Message
            && current.kind == RowKind::Message
            && previous.sender == current.sender
            && previous.display_name == current.display_name
            && date(previous).is_some()
            && date(previous) == date(current)
            && previous
                .timestamp_ms
                .zip(current.timestamp_ms)
                .and_then(|(a, b)| b.checked_sub(a))
                .is_some_and(|gap| (0..=5 * 60_000).contains(&gap))
    })
}
pub fn time_label(message: &MessageRow) -> String {
    local_time(message)
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default()
}
pub fn time_detail(message: &MessageRow) -> String {
    local_time(message)
        .map(|time| time.format("%a, %d %b %Y · %H:%M:%S %:z").to_string())
        .unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn message(sender: &str, timestamp_ms: Option<i64>) -> MessageRow {
        MessageRow {
            key: "fixture".into(),
            kind: RowKind::Message,
            event_id: None,
            sender: sender.into(),
            display_name: "Same name".into(),
            avatar_url: None,
            image: None,
            timestamp_ms,
            thread_replies: 0,
            thread_root: None,
            is_own: false,
            is_live: false,
            body: String::new(),
            send_state: String::new(),
            retry: None,
        }
    }
    #[test]
    fn grouping_requires_same_identity_and_nearby_ordered_known_times() {
        let now = Local::now()
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp_millis();
        let a = message("@a:example.org", Some(now));
        assert!(grouped(Some(&a), &message(&a.sender, Some(now + 60_000))));
        assert!(!grouped(Some(&a), &message("@b:example.org", Some(now))));
        assert!(!grouped(Some(&a), &message(&a.sender, Some(now + 300_001))));
        assert!(!grouped(Some(&a), &message(&a.sender, Some(now - 1))));
        assert!(!grouped(Some(&a), &message(&a.sender, None)));
        assert!(!grouped(None, &a));
        let mut activity = a.clone();
        activity.kind = RowKind::Activity;
        assert!(!grouped(Some(&a), &activity));
        assert!(!grouped(Some(&activity), &a));
        assert!(!grouped(Some(&activity), &activity));
    }
}
