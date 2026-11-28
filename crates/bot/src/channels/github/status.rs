//! Markdown of a pull request's review status comment (ADR-321).
//!
//! Every line except the last of a head is struck through; nothing written earlier is removed.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use domain::bot::GithubReviewTrigger;
use domain::bot::StatusLanguage;
use domain::bot::review_status::{ReviewHead, ReviewStatus, StatusEvent, StatusLine};

/// Hidden marker that identifies the board's comment among a pull request's comments.
pub(super) const MARKER: &str = "<!-- ait-review-status:v1";
/// Above this size, all but the newest heads fold into a details block.
const FOLD_BYTES: usize = 48 * 1024;
/// Leave room for the heading, fold markup and navigation links.
pub(super) const COMMENT_BYTES: usize = 60 * 1024;
const PAGE_BUDGET: usize = COMMENT_BYTES - 1024;
/// Maximum overhead for one rendered line and a head heading, including strikethrough.
const LINE_OVERHEAD: usize = 96;
/// Heads left unfolded in a large comment.
const OPEN_HEADS: usize = 2;

/// Lines grouped by head and ordered as they appear in the comment.
type Page = BTreeMap<usize, Vec<usize>>;

/// Allocate successive lines to pages. A line always stays on its original page, even
/// when a late result arrives for an obsolete head or the channel language changes.
pub(super) fn pages(status: &ReviewStatus) -> Vec<Page> {
    let mut lines: Vec<_> = status
        .heads
        .iter()
        .enumerate()
        .flat_map(|(head, section)| {
            section
                .lines
                .iter()
                .enumerate()
                .map(move |(index, line)| (head, index, line))
        })
        .collect();
    lines.sort_unstable_by_key(|(_, _, line)| line.sequence);
    let mut pages = Vec::new();
    let mut page = Page::new();
    let mut bytes = 0;
    for (head, index, line) in lines {
        let cost = Text(StatusLanguage::En)
            .event(line)
            .len()
            .max(Text(StatusLanguage::ZhCn).event(line).len())
            + LINE_OVERHEAD;
        if bytes + cost > PAGE_BUDGET && !page.is_empty() {
            pages.push(std::mem::take(&mut page));
            bytes = 0;
        }
        page.entry(head).or_default().push(index);
        bytes += cost;
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}

/// The marker for one page. It can recover a successful POST whose reply was lost.
pub(super) fn marker(status: &ReviewStatus, page: usize) -> String {
    format!("{MARKER} channel={} page={page} -->", status.key.channel_id)
}

/// Render one page for `status`, with links to its adjacent comments when known.
pub(super) fn render(
    status: &ReviewStatus,
    page: (&Page, usize),
    (mention, language): (&str, StatusLanguage),
    (previous, next): (Option<u64>, Option<u64>),
) -> String {
    let text = Text(language);
    let (heads, number) = page;
    let mut body = format!(
        "{}\n### \u{1f916} {mention} {}\n",
        marker(status, number),
        text.title(),
    );
    if let Some(previous) = previous {
        let _ = writeln!(body, "\n{}", text.continued_from(previous));
    }
    let sections: Vec<_> = heads
        .iter()
        .map(|(&head, lines)| section(&status.heads[head], lines, text))
        .collect();
    let total: usize = sections.iter().map(String::len).sum();
    let folded = if total > FOLD_BYTES {
        sections.len().saturating_sub(OPEN_HEADS)
    } else {
        0
    };
    if folded > 0 {
        let _ = write!(
            body,
            "\n<details><summary>{}</summary>\n",
            text.earlier(folded)
        );
        for section in &sections[..folded] {
            body.push_str(section);
        }
        body.push_str("\n</details>\n");
    }
    for section in &sections[folded..] {
        body.push_str(section);
    }
    if let Some(next) = next {
        let _ = writeln!(body, "\n{}", text.continued_to(next));
    }
    body
}

fn section(head: &ReviewHead, lines: &[usize], text: Text) -> String {
    let mut section = format!("\n**{} `{}`**\n", text.head(), short(&head.head_sha));
    let last = head.lines.len().saturating_sub(1);
    let mut lines = lines.to_vec();
    lines.sort_unstable();
    for index in lines {
        let line = &head.lines[index];
        let content = format!(
            "{} `{} UTC` {}",
            emoji(&line.event),
            time(line),
            text.event(line)
        );
        if index < last {
            let _ = writeln!(section, "- ~~{content}~~");
        } else {
            let _ = writeln!(section, "- {content}");
        }
    }
    section
}

const fn emoji(event: &StatusEvent) -> &'static str {
    match event {
        StatusEvent::Requested { .. } => "\u{1f4e5}",
        StatusEvent::RequestedAgain { .. } => "\u{1f501}",
        StatusEvent::Queued => "⏳",
        StatusEvent::Preparing => "\u{1f4e6}",
        StatusEvent::Reviewing { .. } => "\u{1f50d}",
        StatusEvent::RateLimited { .. } => "\u{23f8}\u{fe0f}",
        StatusEvent::Published { .. } => "\u{2705}",
        StatusEvent::Failed { .. } => "\u{274c}",
        StatusEvent::Unknown => "\u{26a0}\u{fe0f}",
        StatusEvent::Cancelled => "\u{1f6ab}",
        StatusEvent::Outdated { .. } => "\u{23ed}\u{fe0f}",
    }
}

fn time(line: &StatusLine) -> String {
    chrono::DateTime::parse_from_rfc3339(&line.at).map_or_else(
        |_| "--:--".to_owned(),
        |at| at.with_timezone(&chrono::Utc).format("%H:%M").to_string(),
    )
}

fn short(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// A GitHub login, kept from notifying or formatting anything.
fn login(user: &str) -> String {
    format!(
        "`@{}`",
        user.chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '[' | ']'))
            .collect::<String>()
    )
}

/// Free text shown as code, so it cannot mention anyone or break the list.
fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "'"))
}

fn provider(name: &str) -> String {
    match name {
        "codex" => "Codex".to_owned(),
        "claude" => "Claude".to_owned(),
        other => code(&other[..other.floor_char_boundary(other.len().min(512))]),
    }
}

#[derive(Clone, Copy)]
struct Text(StatusLanguage);

impl Text {
    const fn zh(self) -> bool {
        matches!(self.0, StatusLanguage::ZhCn)
    }

    const fn title(self) -> &'static str {
        if self.zh() {
            "review 状态"
        } else {
            "review status"
        }
    }

    const fn head(self) -> &'static str {
        if self.zh() { "提交" } else { "Head" }
    }

    fn earlier(self, count: usize) -> String {
        if self.zh() {
            format!("更早的 {count} 个提交")
        } else {
            format!("{count} earlier heads")
        }
    }

    fn continued_from(self, comment: u64) -> String {
        if self.zh() {
            format!("\u{2b05}\u{fe0f} 更早的记录见[上一条状态评论](#issuecomment-{comment})。")
        } else {
            format!(
                "\u{2b05}\u{fe0f} Earlier heads are in [the previous status comment](#issuecomment-{comment})."
            )
        }
    }

    fn continued_to(self, comment: u64) -> String {
        if self.zh() {
            format!("\u{27a1}\u{fe0f} 后续记录见[下一条状态评论](#issuecomment-{comment})。")
        } else {
            format!(
                "\u{27a1}\u{fe0f} Continued in [the next status comment](#issuecomment-{comment})."
            )
        }
    }

    const fn trigger(self, trigger: GithubReviewTrigger) -> &'static str {
        match (trigger, self.zh()) {
            (GithubReviewTrigger::Mention, false) => "comment",
            (GithubReviewTrigger::Mention, true) => "评论召唤",
            (GithubReviewTrigger::ReviewerAssignment, false) => "reviewer request",
            (GithubReviewTrigger::ReviewerAssignment, true) => "请求 reviewer",
            (GithubReviewTrigger::AssigneeAssignment, false) => "assignment",
            (GithubReviewTrigger::AssigneeAssignment, true) => "指派",
        }
    }

    fn event(self, line: &StatusLine) -> String {
        let zh = self.zh();
        match &line.event {
            StatusEvent::Requested { user, trigger } if zh => {
                format!("{} 请求 review（{}）", login(user), self.trigger(*trigger))
            }
            StatusEvent::Requested { user, trigger } => {
                format!(
                    "Review requested by {} ({})",
                    login(user),
                    self.trigger(*trigger)
                )
            }
            StatusEvent::RequestedAgain { user, trigger, .. } if zh => format!(
                "{} 再次请求（{}），由本次 review 覆盖",
                login(user),
                self.trigger(*trigger)
            ),
            StatusEvent::RequestedAgain { user, trigger, .. } => format!(
                "Requested again by {} ({}); this review covers it",
                login(user),
                self.trigger(*trigger)
            ),
            StatusEvent::Queued if zh => "已排队".to_owned(),
            StatusEvent::Queued => "Review queued".to_owned(),
            StatusEvent::Preparing if zh => "正在准备只读检出".to_owned(),
            StatusEvent::Preparing => "Preparing a read-only checkout".to_owned(),
            StatusEvent::Reviewing { provider: name } if zh => {
                format!("正在用 {} 审阅", provider(name))
            }
            StatusEvent::Reviewing { provider: name } => {
                format!("Reviewing with {}", provider(name))
            }
            StatusEvent::RateLimited { wait_seconds } if zh => {
                format!("发布暂缓，{wait_seconds} 秒后重试")
            }
            StatusEvent::RateLimited { wait_seconds } => {
                format!("Publication delayed; retrying in {wait_seconds} s")
            }
            StatusEvent::Published {
                high,
                medium,
                low,
                url,
            } => published(zh, [*high, *medium, *low], url.as_deref()),
            StatusEvent::Failed { reason } if zh => format!("Review 未能执行：{}", code(reason)),
            StatusEvent::Failed { reason } => format!("Review could not run: {}", code(reason)),
            StatusEvent::Unknown if zh => "结果无法确认；推送新提交后可再次请求".to_owned(),
            StatusEvent::Unknown => {
                "Outcome could not be confirmed; push a new commit to request another review"
                    .to_owned()
            }
            StatusEvent::Cancelled if zh => "Review 已取消".to_owned(),
            StatusEvent::Cancelled => "Review cancelled".to_owned(),
            StatusEvent::Outdated { head_sha } if zh => {
                format!("已过期：有新提交（`{}`）", short(head_sha))
            }
            StatusEvent::Outdated { head_sha } => {
                format!("Outdated: new commits pushed (`{}`)", short(head_sha))
            }
        }
    }
}

fn published(zh: bool, [high, medium, low]: [u64; 3], url: Option<&str>) -> String {
    let total = high.saturating_add(medium).saturating_add(low);
    let counts: Vec<String> = [(high, "high"), (medium, "medium"), (low, "low")]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, name)| {
            if zh {
                format!("{count} 条 {name}")
            } else {
                format!("{count} {name}")
            }
        })
        .collect();
    let mut text = match (zh, total) {
        (true, 0) => "Review 已发布：没有发现问题".to_owned(),
        (false, 0) => "Review published: no findings".to_owned(),
        (true, _) => format!("Review 已发布：{total} 条发现（{}）", counts.join("，")),
        (false, _) => format!("Review published: {total} findings ({})", counts.join(", ")),
    };
    if let Some(url) = url.filter(|url| {
        url.len() <= 512
            && url.starts_with("https://github.com/")
            && !url.contains(|c: char| {
                c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '\\' | '`')
            })
    }) {
        let label = if zh { "查看 review" } else { "view review" };
        let _ = write!(text, " · [{label}]({url})");
    }
    text
}

#[cfg(test)]
mod tests;
