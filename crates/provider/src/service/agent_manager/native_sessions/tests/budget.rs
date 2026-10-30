use std::future::pending;
use std::task::Poll;

use super::*;
use crate::ports::agent_session::{AgentResumePurpose, AgentSession, AgentSessionSpec};

#[derive(Debug)]
struct PendingClient {
    pending_probe: bool,
}

#[async_trait::async_trait]
impl AgentClient for PendingClient {
    fn provider(&self) -> &'static str {
        "listing"
    }

    fn supports_session_import(&self) -> bool {
        true
    }

    async fn is_available(&self) -> Result<bool, AgentSessionError> {
        if self.pending_probe {
            pending().await
        } else {
            Ok(true)
        }
    }

    async fn list_sessions(
        &self,
        _options: &ListOptions,
    ) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
        pending().await
    }

    async fn create_session(
        &self,
        _spec: &AgentSessionSpec,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        unreachable!("listing must not create a session")
    }

    async fn resume_session(
        &self,
        _handle: &AgentPersistenceHandle,
        _spec: &AgentSessionSpec,
        _purpose: AgentResumePurpose,
    ) -> Result<Box<dyn AgentSession>, AgentSessionError> {
        unreachable!("listing must not resume a session")
    }
}

#[tokio::test]
async fn unsupported_providers_skip_the_saturated_history_budget_without_probing() {
    let mut manager = AgentManager::new(Box::new(
        super::super::super::tests::MemoryRegistry::default(),
    ));
    manager
        .register_client(Box::new(ImportClient {
            provider: "unsupported",
            supported: false,
            available: Ok(true),
            sessions: Err(AgentSessionError::Unavailable),
            expected_cwd: None,
            remove_cwd: None,
        }))
        .unwrap();
    let _held = manager.history_budget.acquire_many(8).await.unwrap();
    let listing = manager.recent_sessions(serde_json::from_value(json!({})).unwrap());
    tokio::pin!(listing);

    let Poll::Ready(Ok(result)) = futures_util::poll!(&mut listing) else {
        panic!("unsupported listing must finish without waiting for history capacity");
    };
    assert!(result["entries"].as_array().unwrap().is_empty());
    assert!(result["providerErrors"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn cancelling_listing_releases_capacity_for_another_providers_inspection() {
    for pending_probe in [true, false] {
        let mut manager = AgentManager::new(Box::new(
            super::super::super::tests::MemoryRegistry::default(),
        ));
        manager
            .register_client(Box::new(PendingClient { pending_probe }))
            .unwrap();
        manager
            .register_client(Box::new(ImportClient {
                provider: "inspecting",
                supported: true,
                available: Ok(true),
                sessions: Ok(Vec::new()),
                expected_cwd: None,
                remove_cwd: None,
            }))
            .unwrap();
        let reader = manager.fork(None);
        let _held = manager.history_budget.acquire_many(7).await.unwrap();
        let mut listing = Box::pin(
            manager
                .recent_sessions(serde_json::from_value(json!({"providers":["listing"]})).unwrap()),
        );
        assert!(futures_util::poll!(&mut listing).is_pending());
        assert_eq!(manager.history_budget.available_permits(), 0);
        let handle = AgentPersistenceHandle {
            provider: "inspecting".into(),
            session_id: "native-session".into(),
            native_handle: None,
            metadata: None,
        };
        let mut inspection = Box::pin(reader.inspect_native(&handle, "/project"));
        assert!(futures_util::poll!(&mut inspection).is_pending());

        drop(listing);

        // The other adapter's default inspection fails immediately once it is admitted.
        assert!(matches!(
            futures_util::poll!(&mut inspection),
            Poll::Ready(Err(ErrorCode::AgentIo))
        ));
        assert_eq!(manager.history_budget.available_permits(), 1);
    }
}

#[tokio::test]
async fn listing_releases_history_capacity_after_absence_errors_and_success() {
    let mut manager = AgentManager::new(Box::new(
        super::super::super::tests::MemoryRegistry::default(),
    ));
    for (provider, available, sessions, errors) in [
        ("absent", Ok(false), Err(AgentSessionError::Unavailable), 0),
        (
            "probe-failed",
            Err(AgentSessionError::Failed),
            Ok(Vec::new()),
            1,
        ),
        ("read-failed", Ok(true), Err(AgentSessionError::Failed), 1),
        ("working", Ok(true), Ok(Vec::new()), 0),
    ] {
        manager
            .register_client(Box::new(ImportClient {
                provider,
                supported: true,
                available,
                sessions,
                expected_cwd: None,
                remove_cwd: None,
            }))
            .unwrap();
        let _held = manager.history_budget.acquire_many(7).await.unwrap();

        let result = manager
            .recent_sessions(serde_json::from_value(json!({"providers":[provider]})).unwrap())
            .await
            .unwrap();

        assert_eq!(result["providerErrors"].as_array().unwrap().len(), errors);
        assert_eq!(manager.history_budget.available_permits(), 1);
    }
}
