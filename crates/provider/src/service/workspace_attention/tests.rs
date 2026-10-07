use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod activity;

#[derive(Debug, Clone, Default)]
struct Agents(
    Arc<Mutex<Vec<PersistedAgentRuntimeRecord>>>,
    Arc<AtomicUsize>,
    Arc<Mutex<Faults>>,
);

#[derive(Debug, Default)]
struct Faults {
    list: bool,
}

impl AgentRuntimeRegistry for Agents {
    fn initialize(&self) -> Result<(), AgentRuntimeRegistryError> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        self.1.fetch_add(1, Ordering::SeqCst);
        if self.2.lock().unwrap().list {
            return Err(AgentRuntimeRegistryError::Io);
        }
        Ok(self.0.lock().expect("agents").clone())
    }
    fn get(
        &self,
        agent_id: &str,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        Ok(self
            .0
            .lock()
            .expect("agents")
            .iter()
            .find(|record| record.id == agent_id)
            .cloned())
    }
    fn upsert(
        &self,
        record: &PersistedAgentRuntimeRecord,
    ) -> Result<(), AgentRuntimeRegistryError> {
        let mut records = self.0.lock().expect("agents");
        if let Some(current) = records.iter_mut().find(|current| current.id == record.id) {
            current.clone_from(record);
        } else {
            records.push(record.clone());
        }
        Ok(())
    }
    fn update(
        &self,
        agent_id: &str,
        update: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        let mut records = self.0.lock().expect("agents");
        let Some(record) = records.iter_mut().find(|record| record.id == agent_id) else {
            return Ok(None);
        };
        *record = update(record);
        Ok(Some(record.clone()))
    }
    fn remove(&self, agent_id: &str) -> Result<bool, AgentRuntimeRegistryError> {
        let mut records = self.0.lock().expect("agents");
        let before = records.len();
        records.retain(|record| record.id != agent_id);
        Ok(records.len() != before)
    }
}

fn agent(id: &str, workspace_id: &str, updated_at: &str) -> PersistedAgentRuntimeRecord {
    PersistedAgentRuntimeRecord {
        id: id.to_owned(),
        provider: "codex".to_owned(),
        cwd: "/repo".to_owned(),
        workspace_id: Some(workspace_id.to_owned()),
        created_at: "2026-09-22T09:00:00.000Z".to_owned(),
        updated_at: updated_at.to_owned(),
        last_activity_at: None,
        last_user_message_at: None,
        title: None,
        title_origin: None,
        labels: BTreeMap::new(),
        last_status: AgentRuntimeStatus::Closed,
        last_mode_id: None,
        config: None,
        runtime_info: None,
        features: Vec::new(),
        persistence: None,
        last_error: None,
        requires_attention: true,
        attention_reason: Some(AgentAttentionReason::Finished),
        attention_timestamp: Some(updated_at.to_owned()),
        internal: false,
        archived_at: None,
        owner: None,
    }
}
