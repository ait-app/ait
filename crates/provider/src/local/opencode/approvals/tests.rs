use super::*;
use crate::local::opencode::types::{ApprovalSink, ApprovalTarget};
use async_trait::async_trait;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Approval {
    withdraw: bool,
    entered: tokio::sync::Semaphore,
    expired: AtomicUsize,
}
#[async_trait]
impl ApprovalSink for Approval {
    async fn decide(&self, _: ApprovalRequest) -> Result<Decision, ProtocolError> {
        self.entered.add_permits(1);
        if self.withdraw {
            std::future::pending().await
        } else {
            Ok(Decision::Approved)
        }
    }
    async fn expire(&self, _: &ApprovalRequest) -> Result<(), ProtocolError> {
        self.expired.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn approved_requests_reply_once_and_withdrawn_permissions_expire() {
    for withdraw in [false, true] {
        let fixture = super::super::tests::fixture::Fixture::start(Version::V2).await;
        let runtime = super::super::runtime::Runtime::spawn(
            &fixture.binary,
            &fixture.cwd,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        let approval = Arc::new(Approval {
            withdraw,
            entered: tokio::sync::Semaphore::new(0),
            expired: AtomicUsize::new(0),
        });
        let mut request = super::super::tests::invocation(fixture.cwd.clone());
        request.approvals = approval.clone();
        fixture
            .state
            .lock()
            .unwrap()
            .pending_permissions
            .push(json!({"id":"perm1","sessionID":"ses_one","action":"shell","resources":["pwd"]}));
        let mut pending = Pending::new();
        pending
            .reconcile(&runtime.api, &request, "ses_one")
            .await
            .unwrap();
        assert!(!pending.can_settle());
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            approval.entered.acquire(),
        )
        .await
        .unwrap()
        .unwrap()
        .forget();
        if withdraw {
            fixture.state.lock().unwrap().pending_permissions.clear();
            pending
                .reconcile(&runtime.api, &request, "ses_one")
                .await
                .unwrap();
            assert_eq!(approval.expired.load(Ordering::SeqCst), 1);
            assert!(fixture.state.lock().unwrap().replies.is_empty());
            assert!(pending.can_settle());
        } else {
            let resolution =
                tokio::time::timeout(std::time::Duration::from_secs(3), pending.receiver.recv())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
            assert_eq!(resolution, Resolution::Resolved);
            assert_eq!(
                fixture.state.lock().unwrap().replies,
                vec![json!({"decision":"once"})]
            );
        }
        let mut runtime = runtime;
        let _ = runtime.close().await;
    }
}

#[tokio::test]
async fn native_withdrawal_does_not_abort_an_in_flight_decision() {
    let fixture = super::super::tests::fixture::Fixture::start(Version::V2).await;
    let mut runtime = super::super::runtime::Runtime::spawn(
        &fixture.binary,
        &fixture.cwd,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let request = super::super::tests::invocation(fixture.cwd.clone());
    let mut pending = Pending::new();
    let (release, ready) = tokio::sync::oneshot::channel();
    let sender = pending.sender.clone();
    let handle = tokio::spawn(async move {
        ready.await.unwrap();
        sender.send(Ok(Resolution::Declined)).await.unwrap();
    });
    pending.tasks.insert(
        "perm1".into(),
        Task {
            handle: AbortOnDropHandle::new(handle),
            approval: None,
            waiting: Arc::new(AtomicBool::new(false)),
        },
    );

    pending
        .reconcile(&runtime.api, &request, "ses_one")
        .await
        .unwrap();
    assert_eq!(pending.tasks.len(), 1);
    assert!(!pending.can_settle());
    release.send(()).unwrap();
    let resolution =
        tokio::time::timeout(std::time::Duration::from_secs(3), pending.receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    assert_eq!(resolution, Resolution::Declined);
    pending
        .reconcile(&runtime.api, &request, "ses_one")
        .await
        .unwrap();
    assert!(pending.tasks.is_empty());
    assert!(pending.can_settle());
    runtime.close().await.unwrap();
}

#[test]
fn native_permissions_preserve_commands_patterns_and_other_actions() {
    let request = super::super::tests::invocation("/tmp/project".into());
    let mut data =
        json!({"id":"perm1","sessionID":"ses_one","action":"shell","resources":["git *"]});
    assert!(normalize(Version::V2, &request, "ses_one", &data).is_some());
    data["metadata"] = json!({"command":"git status"});
    let approval = normalize(Version::V2, &request, "ses_one", &data).unwrap();
    assert_eq!(approval.id, "perm1");
    assert!(
        matches!(approval.target, ApprovalTarget::Command {command,..} if command=="git status")
    );
    data["metadata"] = json!({"command":"echo bearer secret"});
    assert!(normalize(Version::V2, &request, "ses_one", &data).is_some());
    data = json!({"id":"perm2","permission":"edit","patterns":["src/main.rs"]});
    assert!(
        matches!(normalize(Version::V1, &request, "ses_one", &data).unwrap().target,
        ApprovalTarget::Files {paths} if paths[0]=="/tmp/project/src/main.rs")
    );
    data["patterns"] = json!(["src/*"]);
    assert!(normalize(Version::V1, &request, "ses_one", &data).is_some());
    data = json!({"id":"perm3","action":"network","resources":["*"]});
    assert!(matches!(
        normalize(Version::V2, &request, "ses_one", &data)
            .unwrap()
            .target,
        ApprovalTarget::Native { .. }
    ));
}
