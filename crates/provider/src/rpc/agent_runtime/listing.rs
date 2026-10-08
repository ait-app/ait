//! Coherent Agent directory bootstrap for connection-owned observers.

use domain::directory_sync::Cursor;
use serde_json::{Value, json};

use super::{
    AgentListRequest, AgentRuntimeDirectory, ErrorCode, QueryScope, encode, entry, map_error, page,
    query, synchronize,
};

pub(crate) struct Listing {
    pub(crate) response: Value,
    pub(crate) entries: Vec<Value>,
    cursor: Option<Cursor>,
}

pub(crate) fn prepare(
    directory: &AgentRuntimeDirectory,
    request: AgentListRequest,
) -> Result<Listing, ErrorCode> {
    if request
        .scope
        .as_deref()
        .is_some_and(|scope| scope != "active")
        || request
            .subscribe
            .as_ref()
            .is_some_and(|subscribe| subscribe.subscription_id.is_some())
        || request.sync.is_some()
            && (request.scope.as_deref() != Some("active") || request.filter.is_some())
    {
        return Err(ErrorCode::InvalidMessage);
    }
    let scope = if request.scope.as_deref() == Some("active") {
        QueryScope::Active
    } else {
        QueryScope::All
    };
    let query = query(request.filter, request.sort, request.page, scope, None)?;
    let entries = directory
        .matching_entries(&query)
        .map_err(|error| map_error(&error))?;
    let response = if request.sync.is_some() {
        json!({"entries":entries.iter().map(entry).collect::<Vec<_>>(),
            "pageInfo":{"nextCursor":null,"prevCursor":null,"hasMore":false}})
    } else {
        encode(page(
            &AgentRuntimeDirectory::paginate(entries.clone(), &query)
                .map_err(|error| map_error(&error))?,
        ))?
    };
    let entries = entries
        .iter()
        .map(|row| encode(entry(row)))
        .collect::<Result<_, _>>()?;
    Ok(Listing {
        response,
        entries,
        cursor: request.sync,
    })
}

impl Listing {
    pub(crate) fn finish(self, directory: &AgentRuntimeDirectory) -> Result<Value, ErrorCode> {
        let response = if let Some(cursor) = self.cursor {
            synchronize(directory, self.response, &cursor)?
        } else {
            self.response
        };
        Ok(json!({"response":response,"entries":self.entries}))
    }
}
