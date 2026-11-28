//! Markdown of a pull request's review status comment (ADR-321).
//!
//! The current state of each head stays visible while superseded entries are struck through.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::LazyLock;

use domain::bot::GithubReviewTrigger;
use domain::bot::StatusLanguage;
use domain::bot::review_status::{ReviewHead, ReviewStatus, StatusEvent, StatusLine};
use serde::Deserialize;

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
        let cost = CATALOGS
            .iter()
            .map(|(_, catalog)| catalog.event(&line.event).len())
            .max()
            .unwrap_or_default()
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
    let text = catalog(language);
    let (heads, number) = page;
    let mut body = format!(
        "{}\n### \u{1f916} {mention} {}\n",
        marker(status, number),
        text.title,
    );
    if let Some(previous) = previous {
        let message = continuation(&text.continued_from, &text.previous_comment, previous);
        let _ = writeln!(body, "\n\u{2b05}\u{fe0f} {message}");
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
            format_message(&text.earlier, &[("count", &folded.to_string())])
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
        let message = continuation(&text.continued_to, &text.next_comment, next);
        let _ = writeln!(body, "\n\u{27a1}\u{fe0f} {message}");
    }
    body
}

fn section(head: &ReviewHead, lines: &[usize], text: &Catalog) -> String {
    let mut section = format!("\n**{} `{}`**\n", text.head, short(&head.head_sha));
    let last = head
        .lines
        .iter()
        .rposition(|line| !matches!(line.event, StatusEvent::RequestedAgain { .. }))
        .unwrap_or_else(|| head.lines.len().saturating_sub(1));
    let mut lines = lines.to_vec();
    lines.sort_unstable();
    for index in lines {
        let line = &head.lines[index];
        let content = format!(
            "{} `{} UTC` {}",
            emoji(&line.event),
            time(line),
            text.event(&line.event)
        );
        if index == last {
            let _ = writeln!(section, "- {content}");
        } else {
            let _ = writeln!(section, "- ~~{content}~~");
        }
    }
    section
}

const fn emoji(event: &StatusEvent) -> &'static str {
    match event {
        StatusEvent::Requested { .. } => "\u{1f4e5}",
        StatusEvent::RequestedAgain { .. } => "\u{1f501}",
        StatusEvent::Queued => "\u{23f3}",
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

/// Embedded message catalogs share this schema and are parsed once.
static CATALOGS: LazyLock<[(StatusLanguage, Catalog); 2]> = LazyLock::new(|| {
    [
        (
            StatusLanguage::En,
            serde_json::from_str(include_str!("status/locales/en.json"))
                .expect("valid embedded English review status catalog"),
        ),
        (
            StatusLanguage::ZhCn,
            serde_json::from_str(include_str!("status/locales/zh-cn.json"))
                .expect("valid embedded Simplified Chinese review status catalog"),
        ),
    ]
});

fn catalog(language: StatusLanguage) -> &'static Catalog {
    CATALOGS
        .iter()
        .find_map(|(locale, catalog)| (*locale == language).then_some(catalog))
        .expect("every supported review status language has a catalog")
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    title: String,
    head: String,
    earlier: String,
    continued_from: String,
    previous_comment: String,
    continued_to: String,
    next_comment: String,
    mention: String,
    reviewer_assignment: String,
    assignee_assignment: String,
    requested: String,
    requested_again: String,
    queued: String,
    preparing: String,
    reviewing: String,
    rate_limited: String,
    published_empty: String,
    published: String,
    finding_count: String,
    finding_separator: String,
    view_review: String,
    failed: String,
    unknown: String,
    cancelled: String,
    outdated: String,
}

impl Catalog {
    fn trigger(&self, trigger: GithubReviewTrigger) -> &str {
        match trigger {
            GithubReviewTrigger::Mention => &self.mention,
            GithubReviewTrigger::ReviewerAssignment => &self.reviewer_assignment,
            GithubReviewTrigger::AssigneeAssignment => &self.assignee_assignment,
        }
    }

    fn event(&self, event: &StatusEvent) -> Cow<'_, str> {
        match event {
            StatusEvent::Requested { user, trigger } => format_message(
                &self.requested,
                &[("user", &login(user)), ("trigger", self.trigger(*trigger))],
            )
            .into(),
            StatusEvent::RequestedAgain { user, trigger, .. } => format_message(
                &self.requested_again,
                &[("user", &login(user)), ("trigger", self.trigger(*trigger))],
            )
            .into(),
            StatusEvent::Queued => self.queued.as_str().into(),
            StatusEvent::Preparing => self.preparing.as_str().into(),
            StatusEvent::Reviewing { provider: name } => {
                format_message(&self.reviewing, &[("provider", &provider(name))]).into()
            }
            StatusEvent::RateLimited { wait_seconds } => format_message(
                &self.rate_limited,
                &[("seconds", &wait_seconds.to_string())],
            )
            .into(),
            StatusEvent::Published {
                high,
                medium,
                low,
                url,
            } => self
                .published([*high, *medium, *low], url.as_deref())
                .into(),
            StatusEvent::Failed { reason } => {
                format_message(&self.failed, &[("reason", &code(reason))]).into()
            }
            StatusEvent::Unknown => self.unknown.as_str().into(),
            StatusEvent::Cancelled => self.cancelled.as_str().into(),
            StatusEvent::Outdated { head_sha } => {
                format_message(&self.outdated, &[("head", &code(short(head_sha)))]).into()
            }
        }
    }

    fn published(&self, [high, medium, low]: [u64; 3], url: Option<&str>) -> String {
        let total = high.saturating_add(medium).saturating_add(low);
        let mut text = if total == 0 {
            self.published_empty.clone()
        } else {
            let counts: Vec<_> = [(high, "high"), (medium, "medium"), (low, "low")]
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, severity)| {
                    format_message(
                        &self.finding_count,
                        &[("count", &count.to_string()), ("severity", severity)],
                    )
                })
                .collect();
            format_message(
                &self.published,
                &[
                    ("total", &total.to_string()),
                    ("counts", &counts.join(&self.finding_separator)),
                ],
            )
        };
        if let Some(url) = url.filter(|url| {
            url.len() <= 512
                && url.starts_with("https://github.com/")
                && !url.contains(|c: char| {
                    c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '\\' | '`')
                })
        }) {
            let _ = write!(text, " · [{}]({url})", self.view_review);
        }
        text
    }
}

fn continuation(template: &str, label: &str, comment: u64) -> String {
    format_message(
        template,
        &[("link", &format!("[{label}](#issuecomment-{comment})"))],
    )
}

/// Substitute named placeholders in an embedded template, treating each value as literal text.
fn format_message(template: &str, values: &[(&str, &str)]) -> String {
    let mut message = String::with_capacity(template.len());
    let mut remaining = template;
    while let Some((literal, slot)) = remaining.split_once('{') {
        let (name, tail) = slot
            .split_once('}')
            .expect("embedded message placeholder closes");
        let value = values
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(*value))
            .expect("embedded message placeholder has a supplied value");
        message.push_str(literal);
        message.push_str(value);
        remaining = tail;
    }
    message.push_str(remaining);
    message
}

#[cfg(test)]
mod tests;
