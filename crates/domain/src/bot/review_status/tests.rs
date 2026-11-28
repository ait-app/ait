use super::{ReviewStatus, ReviewStatusKey, StatusEvent, StatusLine};
use crate::ChannelId;
use crate::bot::GithubReviewTrigger;

fn board() -> ReviewStatus {
    ReviewStatus::new(ReviewStatusKey {
        channel_id: ChannelId::generate(),
        repository: "owner/repository".to_owned(),
        pull_number: 7,
    })
}

fn kinds(lines: &[StatusLine]) -> Vec<&'static str> {
    lines
        .iter()
        .map(|line| match line.event {
            StatusEvent::Requested { .. } => "requested",
            StatusEvent::Preparing => "preparing",
            StatusEvent::Published { .. } => "published",
            StatusEvent::Outdated { .. } => "outdated",
            _ => "other",
        })
        .collect()
}

#[test]
fn a_newer_head_outdates_earlier_heads_and_later_lines_stay_above_the_mark() {
    let mut status = board();
    let requested = StatusEvent::Requested {
        user: "maintainer".to_owned(),
        trigger: GithubReviewTrigger::Mention,
    };
    status.append("aaa", requested.clone(), "2026-11-28T08:45:00Z");
    status.append("aaa", StatusEvent::Preparing, "2026-11-28T08:46:00Z");
    status.append("bbb", requested, "2026-11-28T08:47:00Z");
    let published = StatusEvent::Published {
        high: 0,
        medium: 1,
        low: 0,
        url: None,
    };
    status.append("aaa", published, "2026-11-28T08:48:00Z");
    assert_eq!(
        kinds(&status.heads[0].lines),
        ["requested", "preparing", "published", "outdated"]
    );
    assert_eq!(kinds(&status.heads[1].lines), ["requested"]);
    assert_eq!(status.revision, 5);
    assert!(status.dirty());
    // A third head does not outdate the first head twice.
    status.append("ccc", StatusEvent::Preparing, "2026-11-28T08:49:00Z");
    assert_eq!(kinds(&status.heads[0].lines).last(), Some(&"outdated"));
    assert_eq!(status.heads[0].lines.len(), 4);
    assert_eq!(kinds(&status.heads[1].lines), ["requested", "outdated"]);
}

#[test]
fn failure_reasons_are_single_line_and_bounded() {
    let mut status = board();
    let reason = format!("first\nsecond{}", "x".repeat(600));
    status.append("aaa", StatusEvent::Failed { reason }, "t");
    let StatusEvent::Failed { reason } = &status.heads[0].lines[0].event else {
        panic!("a failure line");
    };
    assert!(reason.starts_with("first second") && reason.len() <= 512);
}

#[test]
fn reactions_are_requested_once_and_records_round_trip() {
    let mut status = board();
    status.react(10);
    status.react(10);
    assert_eq!(status.pending_reactions, [10]);
    status.published_revision = status.revision;
    assert!(status.dirty());
    status.append("aaa", StatusEvent::Unknown, "t");
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["heads"][0]["lines"][0]["event"]["kind"], "unknown");
    let back: ReviewStatus = serde_json::from_value(json).unwrap();
    assert_eq!(back, status);
}
