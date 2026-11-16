use std::collections::BTreeMap;

use domain::agent::{AgentSnapshot, AgentTarget, Revision};
use domain::{AgentId, OperationId};
use provider::ports::agent::{
    AgentCatalog, AgentError, AgentReceipt, ConfigureAgent, DefaultReceipt, DefaultSelection,
    SelectDefault,
};

/// Independent adapter: this file can use only the provider crate's public contract.
#[derive(Debug)]
pub(crate) struct MemoryCatalog {
    records: BTreeMap<AgentId, Vec<AgentSnapshot>>,
    selection: DefaultSelection,
    configured: BTreeMap<String, (AgentTarget, domain::agent::AgentConfig, AgentReceipt)>,
    selected: BTreeMap<String, (Option<AgentId>, u64, DefaultReceipt)>,
}

impl MemoryCatalog {
    /// Start an empty catalog with no selected default.
    pub(crate) fn new() -> Self {
        Self {
            records: BTreeMap::new(),
            selection: DefaultSelection::new(None, 0).unwrap(),
            configured: BTreeMap::new(),
            selected: BTreeMap::new(),
        }
    }
}

impl AgentCatalog for MemoryCatalog {
    fn configure(&mut self, command: &ConfigureAgent) -> Result<AgentReceipt, AgentError> {
        if let Some((target, config, receipt)) = self.configured.get(command.key()) {
            return if *target == command.target() && config == command.config() {
                Ok(*receipt)
            } else {
                Err(AgentError::IdempotencyConflict)
            };
        }
        let (id, revision) = match command.target() {
            AgentTarget::Create => (AgentId::generate(), Revision::new(1)?),
            AgentTarget::Update { id, expected } => {
                let current = self.get_agent(id, None)?;
                if current.revision() != expected {
                    return Err(AgentError::RevisionConflict);
                }
                (id, expected.next()?)
            }
        };
        if !command.config().enabled() && self.selection.agent_id() == Some(id) {
            return Err(AgentError::IsDefault);
        }
        let snapshot = AgentSnapshot::new(
            id,
            revision,
            command.config().clone(),
            command.recorded_at(),
        )?;
        let receipt = AgentReceipt::new(OperationId::generate(), id, revision);
        self.records.entry(id).or_default().push(snapshot);
        self.configured.insert(
            command.key().to_owned(),
            (command.target(), command.config().clone(), receipt),
        );
        Ok(receipt)
    }

    fn get_agent(
        &mut self,
        id: AgentId,
        revision: Option<Revision>,
    ) -> Result<AgentSnapshot, AgentError> {
        let history = self.records.get(&id).ok_or(AgentError::NotFound)?;
        history
            .iter()
            .rev()
            .find(|snapshot| revision.is_none_or(|revision| snapshot.revision() == revision))
            .cloned()
            .ok_or(AgentError::RevisionNotFound)
    }

    fn list_agents(
        &mut self,
        after: Option<AgentId>,
        limit: usize,
    ) -> Result<Vec<AgentSnapshot>, AgentError> {
        if !(1..=50).contains(&limit) {
            return Err(AgentError::Invalid);
        }
        Ok(self
            .records
            .iter()
            .filter(|(id, _)| after.is_none_or(|after| **id > after))
            .filter_map(|(_, history)| history.last().cloned())
            .take(limit)
            .collect())
    }

    fn get_default(&mut self) -> Result<DefaultSelection, AgentError> {
        Ok(self.selection)
    }

    fn set_default(&mut self, command: &SelectDefault) -> Result<DefaultReceipt, AgentError> {
        if let Some((id, version, receipt)) = self.selected.get(command.key()) {
            return if *id == command.agent_id() && *version == command.expected_version() {
                Ok(*receipt)
            } else {
                Err(AgentError::IdempotencyConflict)
            };
        }
        if command.expected_version() != self.selection.version() {
            return Err(AgentError::DefaultConflict);
        }
        if let Some(id) = command.agent_id()
            && !self.get_agent(id, None)?.config().enabled()
        {
            return Err(AgentError::Disabled);
        }
        self.selection = DefaultSelection::new(
            command.agent_id(),
            self.selection
                .version()
                .checked_add(1)
                .ok_or(AgentError::Invalid)?,
        )?;
        let receipt = DefaultReceipt::new(OperationId::generate(), self.selection);
        self.selected.insert(
            command.key().to_owned(),
            (command.agent_id(), command.expected_version(), receipt),
        );
        Ok(receipt)
    }
}
