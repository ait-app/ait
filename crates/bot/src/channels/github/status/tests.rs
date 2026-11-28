use domain::ChannelId;
use domain::bot::review_status::{ReviewStatus, ReviewStatusKey, StatusEvent};
use domain::bot::{GithubReviewTrigger, StatusLanguage};

use super::{COMMENT_BYTES, catalog, pages, render};

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
    assert_eq!(
        view(&status, StatusLanguage::ZhCn),
        "<!-- ait-review-status:v1 channel=d4119547-46ff-4a0c-82e1-5b921ca2d25e page=0 -->
### 🤖 ait-bot review 状态

**提交 `74408c9`**
- ~~📥 `08:45 UTC` `@oowl` 请求 review（评论召唤）~~
- ~~🔁 `08:46 UTC` `@oowl` 再次请求（指派），由本次 review 覆盖~~
- ~~📦 `08:47 UTC` 正在准备只读检出~~
- ~~🔍 `08:48 UTC` 正在用 Codex 审阅~~
- ~~✅ `08:52 UTC` Review 已发布：7 条发现（7 条 medium） · [查看 review](https://github.com/owner/repository/pull/32#pullrequestreview-1)~~
- ⏭️ `09:01 UTC` 已过期：有新提交（`c22db06`）

**提交 `c22db06`**
- 📥 `09:01 UTC` `@oowl` 请求 review（评论召唤）
"
    );
}

#[test]
fn free_text_cannot_mention_or_format_and_foreign_links_are_dropped() {
    let mut status = board();
    let head = status.heads[0].head_sha.clone();
    status.append(
        &head,
        StatusEvent::Failed {
            reason: "ping @everyone `{provider}`".to_owned(),
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
    assert!(text.contains("Review could not run: `ping @everyone '{provider}'`"));
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

#[test]
fn a_repeated_request_preserves_the_published_review_as_the_current_state() {
    let mut status = board();
    let head = status.heads[0].head_sha.clone();
    status.append(
        &head,
        StatusEvent::RequestedAgain {
            user: "oowl".to_owned(),
            trigger: GithubReviewTrigger::Mention,
            comment_id: Some(42),
        },
        "2026-11-28T08:53:00Z",
    );
    let body = view(&status, StatusLanguage::En);
    assert!(body.contains("\n- ✅ `08:52 UTC` Review published"));
    assert!(body.contains("- ~~🔁 `08:53 UTC` Requested again"));
}

#[test]
fn both_catalogs_render_queue_retry_unknown_and_severity_counts() {
    let events = [
        StatusEvent::Requested {
            user: "oowl".to_owned(),
            trigger: GithubReviewTrigger::ReviewerAssignment,
        },
        StatusEvent::Queued,
        StatusEvent::RateLimited { wait_seconds: 12 },
        StatusEvent::Unknown,
        StatusEvent::Published {
            high: 1,
            medium: 0,
            low: 2,
            url: None,
        },
    ];
    let (english, chinese) = (catalog(StatusLanguage::En), catalog(StatusLanguage::ZhCn));
    assert_eq!(
        events.each_ref().map(|event| english.event(event)),
        [
            "Review requested by `@oowl` (reviewer request)",
            "Review queued",
            "Publication delayed; retrying in 12 s",
            "Outcome could not be confirmed; push a new commit to request another review",
            "Review published: 3 findings (1 high, 2 low)",
        ]
    );
    assert_eq!(
        events.each_ref().map(|event| chinese.event(event)),
        [
            "`@oowl` 请求 review（请求 reviewer）",
            "已排队",
            "发布暂缓，12 秒后重试",
            "结果无法确认；推送新提交后可再次请求",
            "Review 已发布：3 条发现（1 条 high，2 条 low）",
        ]
    );
}
