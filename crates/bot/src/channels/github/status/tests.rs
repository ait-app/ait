use domain::ChannelId;
use domain::bot::review_status::{ReviewStatus, ReviewStatusKey, StatusEvent};
use domain::bot::{GithubReviewTrigger, StatusLanguage};

use super::{COMMENT_BYTES, pages, render};

fn board() -> ReviewStatus {
    let channel: ChannelId = "d4119547-46ff-4a0c-82e1-5b921ca2d25e".parse().unwrap();
    let mut status = ReviewStatus::new(ReviewStatusKey {
        channel_id: channel,
        repository: "owner/repository".to_owned(),
        pull_number: 32,
    });
    let head = "74408c96fc0633c27594b648fb84b2314a0764d7";
    let at = |minute: u32| format!("2026-11-28T08:{minute:02}:10Z");
    status.append(
        head,
        StatusEvent::Requested {
            user: "oowl".to_owned(),
            trigger: GithubReviewTrigger::Mention,
        },
        &at(45),
    );
    status.append(
        head,
        StatusEvent::RequestedAgain {
            user: "oowl".to_owned(),
            trigger: GithubReviewTrigger::AssigneeAssignment,
            comment_id: None,
        },
        &at(46),
    );
    status.append(head, StatusEvent::Preparing, &at(47));
    status.append(
        head,
        StatusEvent::Reviewing {
            provider: "codex".to_owned(),
        },
        &at(48),
    );
    status.append(
        head,
        StatusEvent::Published {
            high: 0,
            medium: 7,
            low: 0,
            url: Some("https://github.com/owner/repository/pull/32#pullrequestreview-1".to_owned()),
        },
        &at(52),
    );
    status
}

fn view(status: &ReviewStatus, language: StatusLanguage) -> String {
    render(
        status,
        (&pages(status)[0], 0),
        ("ait-bot", language),
        (None, None),
    )
}

#[test]
fn a_finished_review_strikes_every_line_but_its_result() {
    assert_eq!(
        view(&board(), StatusLanguage::En),
        "<!-- ait-review-status:v1 channel=d4119547-46ff-4a0c-82e1-5b921ca2d25e page=0 -->
### 🤖 ait-bot review status

**Head `74408c9`**
- ~~📥 `08:45 UTC` Review requested by `@oowl` (comment)~~
- ~~🔁 `08:46 UTC` Requested again by `@oowl` (assignment); this review covers it~~
- ~~📦 `08:47 UTC` Preparing a read-only checkout~~
- ~~🔍 `08:48 UTC` Reviewing with Codex~~
- ✅ `08:52 UTC` Review published: 7 findings (7 medium) · [view review](https://github.com/owner/repository/pull/32#pullrequestreview-1)
"
    );
}

#[test]
fn a_newer_head_strikes_the_earlier_result_and_chinese_reads_naturally() {
    let mut status = board();
    status.append(
        "c22db065b439aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        StatusEvent::Requested {
            user: "oowl".to_owned(),
            trigger: GithubReviewTrigger::Mention,
        },
        "2026-11-28T09:01:00Z",
    );
    let text = view(&status, StatusLanguage::ZhCn);
    assert!(text.contains("### 🤖 ait-bot review 状态"));
    assert!(
        text.contains("- ~~✅ `08:52 UTC` Review 已发布：7 条发现（7 条 medium） · [查看 review](")
    );
    assert!(text.contains("- ⏭️ `09:01 UTC` 已过期：有新提交（`c22db06`）"));
    assert!(
        text.ends_with("**提交 `c22db06`**\n- 📥 `09:01 UTC` `@oowl` 请求 review（评论召唤）\n")
    );
}

#[test]
fn free_text_cannot_mention_or_format_and_foreign_links_are_dropped() {
    let mut status = board();
    let head = status.heads[0].head_sha.clone();
    status.append(
        &head,
        StatusEvent::Failed {
            reason: "ping @everyone `now`".to_owned(),
        },
        "2026-11-28T09:00:00Z",
    );
    status.append(
        &head,
        StatusEvent::Published {
            high: 0,
            medium: 0,
            low: 0,
            url: Some("https://evil.example/x".to_owned()),
        },
        "2026-11-28T09:01:00Z",
    );
    let text = view(&status, StatusLanguage::En);
    assert!(text.contains("Review could not run: `ping @everyone 'now'`"));
    assert!(text.ends_with("- ✅ `09:01 UTC` Review published: no findings\n"));
}

#[test]
fn many_heads_fold_and_a_continuation_points_both_ways() {
    let mut status = board();
    for index in 0..64 {
        for _ in 0..3 {
            status.append(
                &format!("{index:040x}"),
                StatusEvent::Failed {
                    reason: "x".repeat(500),
                },
                "2026-11-28T09:00:00Z",
            );
        }
    }
    let original = pages(&status);
    assert!(original.len() > 1);
    let head = status.heads[0].head_sha.clone();
    status.append(&head, StatusEvent::Cancelled, "2026-11-28T09:01:00Z");
    let updated = pages(&status);
    let previous = render(
        &status,
        (&updated[0], 0),
        ("ait-bot", StatusLanguage::En),
        (None, Some(100)),
    );
    let last = updated.len() - 1;
    let continuation = render(
        &status,
        (&updated[last], last),
        ("ait-bot", StatusLanguage::En),
        (Some(99), None),
    );
    assert!(previous.contains("[the next status comment](#issuecomment-100)"));
    assert!(continuation.contains("[the previous status comment](#issuecomment-99)"));
    assert!(continuation.contains("- ~~🚫 `09:01 UTC` Review cancelled~~"));
    assert!(previous.len() < COMMENT_BYTES && continuation.len() < COMMENT_BYTES);
    let responses: Vec<_> = updated
        .iter()
        .enumerate()
        .map(|(index, page)| {
            render(
                &status,
                (page, index),
                ("ait-bot", StatusLanguage::En),
                (None, None),
            )
        })
        .collect();
    assert!(responses.iter().all(|body| body.len() < COMMENT_BYTES));
    assert_eq!(
        responses
            .iter()
            .map(|body| body.matches("Review could not run:").count())
            .sum::<usize>(),
        192
    );
    assert!(
        responses
            .iter()
            .any(|body| body.contains("<details><summary>"))
    );
}
