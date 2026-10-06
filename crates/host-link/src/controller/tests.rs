use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};

use super::*;
use crate::{Binding, Credential, Machine, Session};

#[derive(Default)]
struct Observed {
    saved: Vec<State>,
    refreshes: Vec<(String, Uuid)>,
    reports: Vec<&'static str>,
    starts: usize,
    stops: usize,
    registrations: Vec<Uuid>,
    renewals: usize,
    renewal_tokens: Vec<String>,
    closes: usize,
    active: bool,
    save_fail_at: Option<usize>,
    refresh_error: Option<Error>,
    register_error: Option<Error>,
    renew_error: Option<Error>,
    mismatch: bool,
    expired_receipt: bool,
}

#[derive(Clone)]
struct Mock(Arc<Mutex<Observed>>, Binding);

fn state() -> State {
    let server = Uuid::new_v4();
    State {
        machine: Machine {
            server_id: server,
            display_name: "build-linux".into(),
            platform: "linux".into(),
            app_version: "test".into(),
        },
        credential: Some(Credential {
            refresh_token: "old-refresh-secret".into(),
            refresh_expires_at: Utc::now() + chrono::Duration::days(30),
            binding: Binding {
                node_id: Uuid::new_v4(),
                host_id: Uuid::new_v4(),
                server_id: server,
                grant_id: Uuid::new_v4(),
            },
        }),
        pending: None,
    }
}

fn fixture() -> (Controller<Mock, Mock, Mock>, Mock) {
    let state = state();
    let mock = Mock(
        Arc::new(Mutex::new(Observed::default())),
        state.credential.as_ref().unwrap().binding.clone(),
    );
    (
        Controller::new(
            mock.clone(),
            mock.clone(),
            mock.clone(),
            state,
            Uuid::new_v4(),
        ),
        mock,
    )
}

impl CredentialStore for Mock {
    fn save(&self, state: &State) -> Result<(), Error> {
        let mut seen = self.0.lock().unwrap();
        if seen.save_fail_at == Some(seen.saved.len()) {
            return Err(Error::Storage);
        }
        seen.saved.push(state.clone());
        Ok(())
    }
}

#[async_trait]
impl Center for Mock {
    async fn refresh(&self, secret: &SecretString, request_id: Uuid) -> Result<Tokens, Error> {
        let mut seen = self.0.lock().unwrap();
        // The exact pending request must already be durable when consumption starts.
        assert!(
            matches!(seen.saved.last().unwrap().pending, Some(Pending::Refresh(id)) if id == request_id)
        );
        seen.refreshes
            .push((secret.expose_secret().to_owned(), request_id));
        if let Some(error) = seen.refresh_error.take() {
            return Err(error);
        }
        let mut binding = self.1.clone();
        if seen.mismatch {
            binding.host_id = Uuid::new_v4();
        }
        let seconds = if seen.expired_receipt { -1 } else { 900 };
        seen.expired_receipt = false;
        Ok(Tokens {
            access_token: format!("device-access-secret-{}", seen.refreshes.len()).into(),
            access_expires_at: Utc::now() + chrono::Duration::seconds(seconds),
            credential: Credential {
                refresh_token: "new-refresh-secret".into(),
                refresh_expires_at: Utc::now() + chrono::Duration::days(30),
                binding,
            },
        })
    }
    async fn register(
        &self,
        _: &Tokens,
        _: &Machine,
        _: Uuid,
        registration: Uuid,
    ) -> Result<Session, Error> {
        let mut seen = self.0.lock().unwrap();
        seen.registrations.push(registration);
        if let Some(error) = seen.register_error.take() {
            return Err(error);
        }
        Ok(Session {
            id: Uuid::new_v4(),
            lease_until: Utc::now() + chrono::Duration::seconds(60),
            renew_after_seconds: 1,
        })
    }
    async fn renew(&self, tokens: &Tokens, _: Uuid) -> Result<DateTime<Utc>, Error> {
        let mut seen = self.0.lock().unwrap();
        seen.renewals += 1;
        seen.renewal_tokens
            .push(tokens.access_token.expose_secret().to_owned());
        if let Some(error) = seen.renew_error.take() {
            return Err(error);
        }
        Ok(Utc::now() + chrono::Duration::seconds(60))
    }
    async fn ticket(&self, _: &Tokens, _: Uuid) -> Result<SecretString, Error> {
        Ok("ticket-secret".into())
    }
    async fn close(&self, _: &Tokens, _: Uuid) -> Result<(), Error> {
        self.0.lock().unwrap().closes += 1;
        Ok(())
    }
}

#[async_trait]
impl ManagedRelay for Mock {
    async fn start(&self, _: Uuid, _: SecretString) -> Result<(), Error> {
        let mut seen = self.0.lock().unwrap();
        seen.starts += 1;
        seen.active = true;
        Ok(())
    }
    async fn active(&self) -> bool {
        self.0.lock().unwrap().active
    }
    async fn stop(&self) {
        let mut seen = self.0.lock().unwrap();
        seen.stops += 1;
        seen.active = false;
    }
    fn report(&self, phase: &'static str, _: Option<&Binding>) {
        self.0.lock().unwrap().reports.push(phase);
    }
}

#[tokio::test]
async fn rotation_commits_pending_before_consumption_and_new_secret_before_use() {
    let (mut controller, mock) = fixture();
    let tokens = controller.refresh().await.unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.saved.len(), 2);
    assert!(seen.saved[0].pending.is_some());
    assert!(seen.saved[1].pending.is_none());
    assert_eq!(
        seen.saved[1]
            .credential
            .as_ref()
            .unwrap()
            .refresh_token
            .expose_secret(),
        tokens.credential.refresh_token.expose_secret()
    );
    assert!(!format!("{tokens:?}").contains("device-access-secret"));
    assert!(!format!("{:?}", seen.saved).contains("old-refresh-secret"));
}

#[tokio::test]
async fn ambiguous_network_failure_reuses_request_id_and_old_token() {
    let (mut controller, mock) = fixture();
    mock.0.lock().unwrap().refresh_error = Some(Error::Unavailable);
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Unavailable);
    controller.refresh().await.unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.refreshes[0], seen.refreshes[1]);
}

#[tokio::test]
async fn save_failures_never_consume_or_advance_an_uncommitted_rotation() {
    for fail_at in [0, 1] {
        let (mut controller, mock) = fixture();
        mock.0.lock().unwrap().save_fail_at = Some(fail_at);
        assert_eq!(controller.refresh().await.unwrap_err(), Error::Storage);
        assert_eq!(mock.0.lock().unwrap().refreshes.len(), fail_at);
        assert_eq!(
            controller
                .state
                .credential
                .as_ref()
                .unwrap()
                .refresh_token
                .expose_secret(),
            "old-refresh-secret"
        );
    }
}

#[tokio::test]
async fn restart_recovers_pending_rotation_without_changing_id() {
    let (mut controller, mock) = fixture();
    let id = Uuid::new_v4();
    controller.state.pending = Some(Pending::Refresh(id));
    controller.refresh().await.unwrap();
    assert_eq!(mock.0.lock().unwrap().refreshes[0].1, id);
}

#[tokio::test]
async fn rejects_missing_expired_mismatched_or_unfinished_login_state() {
    let (mut controller, mock) = fixture();
    mock.0.lock().unwrap().mismatch = true;
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Protocol);
    controller
        .state
        .credential
        .as_mut()
        .unwrap()
        .refresh_expires_at = Utc::now();
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Unauthorized);
    controller.state = state();
    controller.state.pending = Some(Pending::Enrollment {
        token: "enrollment".into(),
        request_id: Uuid::new_v4(),
    });
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Unauthorized);
    controller.state.credential = None;
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Unauthorized);
}

async fn tick() {
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn runs_without_gui_renews_reconnects_and_closes_on_shutdown() {
    let (controller, mock) = fixture();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    assert_eq!(mock.0.lock().unwrap().starts, 1);
    mock.0.lock().unwrap().active = false;
    tick().await;
    assert!(mock.0.lock().unwrap().starts >= 2);
    assert!(mock.0.lock().unwrap().renewals >= 1);
    cancel.cancel();
    task.await.unwrap().unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.closes, 1);
    assert!(!seen.active);
    assert_eq!(seen.reports.last(), Some(&"stopped"));
}

#[tokio::test(start_paused = true)]
async fn register_outage_retries_the_same_identity() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().register_error = Some(Error::Unavailable);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    tick().await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    let seen = mock.0.lock().unwrap();
    assert!(seen.registrations.len() >= 2);
    assert_eq!(seen.registrations[0], seen.registrations[1]);
}

#[tokio::test(start_paused = true)]
async fn expired_session_registers_again_and_revocation_is_terminal() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().renew_error = Some(Error::SessionExpired);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    tick().await;
    assert!(mock.0.lock().unwrap().registrations.len() >= 2);
    mock.0.lock().unwrap().renew_error = Some(Error::Unauthorized);
    tick().await;
    assert_eq!(task.await.unwrap(), Err(Error::Unauthorized));
    let seen = mock.0.lock().unwrap();
    assert!(!seen.active);
    assert_eq!(seen.reports.last(), Some(&"reauthorization_required"));
}

#[tokio::test(start_paused = true)]
async fn initial_outage_recovers_and_old_runtime_conflict_waits_for_lease_release() {
    let (controller, mock) = fixture();
    {
        let mut seen = mock.0.lock().unwrap();
        seen.refresh_error = Some(Error::Unavailable);
        seen.register_error = Some(Error::Conflict);
    }
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    tick().await;
    assert!(!task.is_finished());
    assert_eq!(mock.0.lock().unwrap().registrations.len(), 1);
    tokio::time::advance(Duration::from_secs(31)).await;
    tick().await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.refreshes[0], seen.refreshes[1]);
    assert_eq!(seen.registrations.len(), 2);
    assert_eq!(seen.registrations[0], seen.registrations[1]);
    assert!(seen.reports.contains(&"runtime_conflict"));
}

#[tokio::test(start_paused = true)]
async fn old_receipt_is_saved_then_rotated_before_registration() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().expired_receipt = true;
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    tick().await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.refreshes.len(), 2);
    assert_eq!(seen.refreshes[0].0, "old-refresh-secret");
    assert_eq!(seen.refreshes[1].0, "new-refresh-secret");
    assert_ne!(seen.refreshes[0].1, seen.refreshes[1].1);
    assert_eq!(seen.registrations.len(), 1);
}

#[tokio::test(start_paused = true)]
async fn long_running_runtime_refreshes_and_renews_with_successor_jwt() {
    let (controller, mock) = fixture();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    tokio::time::advance(Duration::from_secs(850)).await;
    tick().await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.refreshes.len(), 2);
    assert_eq!(seen.registrations.len(), 1);
    assert!(
        seen.renewal_tokens
            .iter()
            .any(|s| s == "device-access-secret-2")
    );
}

#[tokio::test(start_paused = true)]
async fn an_ambiguous_renewal_retries_the_existing_session_after_local_expiry() {
    let (controller, mock) = fixture();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    mock.0.lock().unwrap().renew_error = Some(Error::Unavailable);
    tokio::time::advance(Duration::from_secs(61)).await;
    tick().await;
    tick().await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.registrations.len(), 1);
    assert!(seen.stops >= 2);
    assert!(seen.renewals >= 3);
}
