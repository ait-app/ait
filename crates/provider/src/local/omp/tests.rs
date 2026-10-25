use std::collections::BTreeMap;
use std::time::Duration;

use super::*;
use crate::ports::agent_session::AgentTurnEvent;
use crate::ports::environment::AgentEnvironment;

mod installed;
mod real;

pub(super) async fn next(
    session: &mut dyn AgentSession,
) -> Result<AgentTurnEvent, AgentSessionError> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = session.poll_turn()? {
                return Ok(event);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap()
}

pub(super) async fn complete(session: &mut dyn AgentSession) -> Vec<AgentTurnEvent> {
    let mut events = Vec::new();
    loop {
        let event = next(session).await.unwrap();
        let done = matches!(
            event,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
        );
        events.push(event);
        if done {
            return events;
        }
    }
}
