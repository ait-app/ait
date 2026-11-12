use std::sync::Arc;

use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use crate::ports::usage_ledger::{NoopUsageLedger, UsageLedger};
use crate::service::agent_execution::AgentExecution;
use provider_codex::testing::Fixture;

pub(crate) mod usage_ledger;

/// Start a worker with `fixture` storage, shared `creations`, and the injected native `client`.
/// Returns the execution handle and its durable Agent registry; panics on fixture setup errors.
pub(crate) fn worker_with_client(
    fixture: &Fixture,
    creations: model::creation::Creations,
    client: Box<dyn provider_core::ports::agent_session::AgentClient>,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    worker_with_ledger(fixture, creations, client, Arc::new(NoopUsageLedger))
}

/// [`worker_with_client`] whose Agent manager records provider calls in `ledger`.
pub(crate) fn worker_with_ledger(
    fixture: &Fixture,
    creations: model::creation::Creations,
    client: Box<dyn provider_core::ports::agent_session::AgentClient>,
    ledger: Arc<dyn UsageLedger>,
) -> (AgentExecution, FileBackedAgentRuntimeRegistry) {
    crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        creations,
        client,
        |manager| manager.with_ledger(ledger),
    )
}
