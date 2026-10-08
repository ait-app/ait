//! Workspace-owned sort values for the shared Paseo keyset pager.

use domain::pagination::{Direction, Entry, Sort, SortValue};
use domain::workspace::activity::WorkspaceStateBucket;
use domain::workspace::protocol::directory::{
    SortDirection, WorkspacePage, WorkspacePageInfo, WorkspaceSort, WorkspaceSortKey,
};
use domain::workspace::protocol::workspace::WorkspaceDescriptorPayload;
use model::pagination;

use crate::rpc::ErrorCode;

pub(super) fn paginate(
    entries: Vec<WorkspaceDescriptorPayload>,
    sort: Option<&[WorkspaceSort]>,
    page: Option<&WorkspacePage>,
) -> Result<(Vec<WorkspaceDescriptorPayload>, WorkspacePageInfo), ErrorCode> {
    let sort = normalize_sort(sort);
    let entries = entries
        .into_iter()
        .map(|workspace| Entry {
            id: workspace.id.clone(),
            values: sort
                .iter()
                .map(|sort| {
                    (
                        key_name(sort.key).to_owned(),
                        sort_value(&workspace, sort.key),
                    )
                })
                .collect(),
            value: workspace,
        })
        .collect();
    let sort = sort
        .iter()
        .map(|sort| Sort {
            key: key_name(sort.key).to_owned(),
            direction: match sort.direction {
                SortDirection::Asc => Direction::Asc,
                SortDirection::Desc => Direction::Desc,
            },
        })
        .collect::<Vec<_>>();
    let page = pagination::paginate(
        entries,
        &sort,
        page.map_or(200, |page| page.limit),
        page.and_then(|page| page.cursor.as_deref()),
    )?;
    Ok((
        page.entries,
        WorkspacePageInfo {
            next_cursor: page.next_cursor,
            prev_cursor: page.prev_cursor,
            has_more: page.has_more,
        },
    ))
}

fn normalize_sort(sort: Option<&[WorkspaceSort]>) -> Vec<WorkspaceSort> {
    let mut normalized = Vec::with_capacity(4);
    for clause in sort.unwrap_or_default() {
        if !normalized
            .iter()
            .any(|entry: &WorkspaceSort| entry.key == clause.key)
        {
            normalized.push(*clause);
        }
    }
    if normalized.is_empty() {
        normalized.push(WorkspaceSort {
            key: WorkspaceSortKey::ActivityAt,
            direction: SortDirection::Desc,
        });
    }
    normalized
}

const fn key_name(key: WorkspaceSortKey) -> &'static str {
    match key {
        WorkspaceSortKey::StatusPriority => "status_priority",
        WorkspaceSortKey::ActivityAt => "activity_at",
        WorkspaceSortKey::Name => "name",
        WorkspaceSortKey::ProjectId => "project_id",
    }
}

fn sort_value(workspace: &WorkspaceDescriptorPayload, key: WorkspaceSortKey) -> SortValue {
    match key {
        WorkspaceSortKey::StatusPriority => SortValue::Number(match workspace.status {
            WorkspaceStateBucket::NeedsInput => 0,
            WorkspaceStateBucket::Failed => 1,
            WorkspaceStateBucket::Running => 2,
            WorkspaceStateBucket::Attention => 3,
            WorkspaceStateBucket::Done => 4,
        }),
        WorkspaceSortKey::ActivityAt => workspace
            .activity_at
            .as_deref()
            .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok())
            .map_or(SortValue::Null, |timestamp| {
                SortValue::Number(timestamp.timestamp_millis())
            }),
        WorkspaceSortKey::Name => SortValue::Text(workspace.name.to_lowercase()),
        WorkspaceSortKey::ProjectId => SortValue::Text(workspace.project_id.to_lowercase()),
    }
}

#[cfg(test)]
mod tests;
