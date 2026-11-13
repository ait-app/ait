//! Recovery reads native ownership facts; it never replays a side-question send.

use model::ErrorCode;

use super::AgentManager;
use crate::storage::side::now_millis;
use provider_core::protocol::side_questions::{SideAskResponse, SideReceiptState, SideTurnStatus};

impl AgentManager {
    /// Reconcile only unresolved or accepted-running receipts through their chosen runner.
    /// # Errors
    /// Returns storage failures; unknown native ownership leaves admission blocked.
    pub(super) async fn reconcile_side_requests(&mut self, agent: &str) -> Result<(), ErrorCode> {
        let store = self.side_store.clone().ok_or(ErrorCode::AgentIo)?;
        let pending = store.pending(agent)?;
        let had_pending = !pending.is_empty();
        for (request, snapshot) in pending {
            let Some(snapshot) = snapshot else {
                // No snapshot/strategy was committed, so the send boundary was never crossed.
                store.settle_ask(
                    agent,
                    &request,
                    &SideAskResponse {
                        receipt: SideReceiptState::Rejected,
                        record_id: None,
                        strategy: None,
                        reason: Some("interrupted_before_send".to_owned()),
                    },
                    now_millis(),
                )?;
                continue;
            };
            if !self.side_runners.contains_key(agent) {
                self.prepare_side_runner(agent, &snapshot).await?;
            }
            let runner = self.side_runners.get_mut(agent).ok_or(ErrorCode::AgentIo)?;
            match runner.reconcile_side(&request).await {
                Ok(Some(topic)) => {
                    let id = topic.record.id.clone();
                    let strategy = topic.strategy;
                    let terminal = topic.latest_turn().status.is_terminal();
                    if store.topic(agent, &id)?.is_some() {
                        store.update_topic(agent, topic)?;
                    } else {
                        store.insert_topic(agent, topic, &snapshot)?;
                    }
                    store.settle_ask(
                        agent,
                        &request,
                        &SideAskResponse {
                            receipt: SideReceiptState::Accepted,
                            record_id: Some(id),
                            strategy: Some(strategy),
                            reason: None,
                        },
                        now_millis(),
                    )?;
                    if terminal {
                        store.release_uncertain(agent, &request)?;
                    }
                }
                Ok(None) => {
                    // The adapter confirmed no execution remains. Do not invent an accepted
                    // record for a lost first ack, or change the immutable replay receipt.
                    if let Some(receipt) = store.ask(agent, &request)?
                        && let Some(id) = receipt.topic_id
                        && let Some((mut topic, _)) = store.topic(agent, &id)?
                        && topic.latest_turn().status == SideTurnStatus::Running
                    {
                        if let Some(turn) = topic.record.follow_ups.last_mut() {
                            turn.status = SideTurnStatus::Interrupted;
                            turn.updated_at = turn.updated_at.max(now_millis());
                        } else {
                            topic.record.status = SideTurnStatus::Interrupted;
                            topic.record.updated_at = topic.record.updated_at.max(now_millis());
                        }
                        store.update_topic(agent, topic)?;
                    }
                    store.release_uncertain(agent, &request)?;
                }
                Err(_) => {}
            }
        }
        if !store.uncertain_active(agent, "")?
            && self
                .side_projection(agent)
                .is_none_or(|state| state.running_id.is_none())
        {
            self.side_permits.remove(agent);
        }
        if had_pending && let Some(owner) = &self.owner {
            owner.side_changed();
        }
        Ok(())
    }
}
