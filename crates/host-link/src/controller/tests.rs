use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};

use super::*;
use crate::{Binding, Credential, Machine, Pending, Session};

#[derive(Default)]
struct Observed {
    saved: Vec<State>,
    refreshes: Vec<(String, Uuid)>,
    enrollment_requests: Vec<Uuid>,
    enrollment_times: Vec<tokio::time::Instant>,
    enrollment_error: Option<Error>,
    reports: Vec<&'static str>,
    starts: usize,
    stops: usize,
    hub: HubObserved,
    registrations: Vec<Uuid>,
    renewals: usize,
    renewal_tokens: Vec<String>,
    closes: usize,
    revocations: Vec<String>,
    revoke_error: Option<Error>,
    revoke_delay: Duration,
    relay_grants: Vec<(Binding, Uuid)>,
    hub_grants: Vec<(Binding, Uuid)>,
    active: bool,
    save_fail_at: Option<usize>,
    refresh_error: Option<Error>,
    register_error: Option<Error>,
    renew_error: Option<Error>,
    renew_errors: std::collections::VecDeque<Error>,
    mismatch: bool,
    expired_receipt: bool,
    ticket_error: Option<Error>,
    ticket_delay: Duration,
    ticket_requests: usize,
    renew_delay: Duration,
    lease_seconds: Option<i64>,
    access_seconds: Option<i64>,
}

#[derive(Default)]
struct HubObserved {
    connecting: bool,
    starts: usize,
    tickets: usize,
    active: bool,
    ticket_delay: Duration,
    error: Option<Error>,
}

#[derive(Clone)]
struct Mock(Arc<Mutex<Observed>>, Binding);

fn state() -> State {
    let server = Uuid::new_v4();
    State {
        authorization_id: None,
        disconnected: false,
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

#[expect(
    clippy::unused_async_trait_impl,
    reason = "Keep mock side effects lazy until polled, like the production adapters."
)]
impl Center for Mock {
    async fn revoke(&self, token: &SecretString) -> Result<(), Error> {
        let (delay, error) = {
            let mut seen = self.0.lock().unwrap();
            assert!(!seen.active && !seen.hub.active);
            seen.revocations.push(token.expose_secret().to_owned());
            (seen.revoke_delay, seen.revoke_error)
        };
        tokio::time::sleep(delay).await;
        error.map_or(Ok(()), Err)
    }

    async fn enroll(
        &self,
        _: &SecretString,
        request_id: Uuid,
        _: &Machine,
    ) -> Result<Tokens, Error> {
        let mut seen = self.0.lock().unwrap();
        assert!(
            matches!(seen.saved.last().unwrap().pending, Some(Pending::Enrollment { request_id: saved, .. }) if saved == request_id)
        );
        seen.enrollment_requests.push(request_id);
        seen.enrollment_times.push(tokio::time::Instant::now());
        if let Some(error) = seen.enrollment_error {
            return Err(error);
        }
        Ok(Tokens {
            access_token: "enrolled-access".into(),
            access_expires_at: Utc::now() + chrono::Duration::minutes(15),
            credential: Credential {
                refresh_token: "enrolled-refresh".into(),
                refresh_expires_at: Utc::now() + chrono::Duration::days(30),
                binding: self.1.clone(),
            },
        })
    }
    async fn poll(&self, _: &SecretString, _: Uuid) -> Result<Tokens, Error> {
        Err(Error::Denied)
    }
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
        let seconds = if seen.expired_receipt {
            -1
        } else {
            seen.access_seconds.unwrap_or(900)
        };
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
        let delay = self.0.lock().unwrap().renew_delay;
        tokio::time::sleep(delay).await;
        let mut seen = self.0.lock().unwrap();
        seen.renewals += 1;
        seen.renewal_tokens
            .push(tokens.access_token.expose_secret().to_owned());
        if let Some(error) = seen.renew_errors.pop_front() {
            return Err(error);
        }
        if let Some(error) = seen.renew_error.take() {
            return Err(error);
        }
        Ok(Utc::now() + chrono::Duration::seconds(seen.lease_seconds.unwrap_or(60)))
    }
    async fn ticket(&self, tokens: &Tokens, session: Uuid) -> Result<SecretString, Error> {
        let (error, delay) = {
            let mut seen = self.0.lock().unwrap();
            seen.relay_grants
                .push((tokens.credential.binding.clone(), session));
            seen.ticket_requests += 1;
            (seen.ticket_error, seen.ticket_delay)
        };
        tokio::time::sleep(delay).await;
        if let Some(error) = error {
            return Err(error);
        }
        Ok("ticket-secret".into())
    }
    async fn hub_ticket(&self, tokens: &Tokens, session: Uuid) -> Result<SecretString, Error> {
        let delay = {
            let mut seen = self.0.lock().unwrap();
            seen.hub_grants
                .push((tokens.credential.binding.clone(), session));
            seen.hub.tickets += 1;
            seen.hub.ticket_delay
        };
        tokio::time::sleep(delay).await;
        let seen = self.0.lock().unwrap();
        if let Some(error) = seen.hub.error {
            return Err(error);
        }
        Ok("hub-ticket-secret".into())
    }
    async fn close(&self, _: &Tokens, _: Uuid) -> Result<(), Error> {
        self.0.lock().unwrap().closes += 1;
        Ok(())
    }
}

mod links;
mod recovery;
mod service;

#[expect(
    clippy::unused_async_trait_impl,
    reason = "Keep mock side effects lazy until polled, like the production adapters."
)]
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
    async fn online(&self) -> bool {
        self.0.lock().unwrap().active
    }
    async fn hub_online(&self) -> bool {
        let seen = self.0.lock().unwrap();
        seen.hub.active && !seen.hub.connecting
    }
    async fn stop(&self) {
        let mut seen = self.0.lock().unwrap();
        seen.stops += 1;
        seen.active = false;
        seen.hub.active = false;
    }
    async fn start_hub(&self, _: Uuid, ticket: SecretString) -> Result<(), Error> {
        assert_eq!(ticket.expose_secret(), "hub-ticket-secret");
        let mut seen = self.0.lock().unwrap();
        seen.hub.starts += 1;
        seen.hub.active = true;
        Ok(())
    }
    async fn hub_active(&self) -> bool {
        let seen = self.0.lock().unwrap();
        seen.hub.active
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
async fn expired_pending_rotation_replays_saved_secret_and_id_before_adopting_receipt() {
    let (mut controller, mock) = fixture();
    let request_id = Uuid::new_v4();
    controller.state.pending = Some(Pending::Refresh(request_id));
    controller
        .state
        .credential
        .as_mut()
        .unwrap()
        .refresh_expires_at = Utc::now() - chrono::Duration::seconds(1);
    let tokens = controller.refresh().await.unwrap();
    let seen = mock.0.lock().unwrap();
    assert_eq!(
        seen.refreshes,
        vec![("old-refresh-secret".to_owned(), request_id)]
    );
    assert_eq!(seen.saved.len(), 2);
    assert!(matches!(seen.saved[0].pending, Some(Pending::Refresh(id)) if id == request_id));
    assert!(seen.saved[1].pending.is_none());
    assert_eq!(
        controller
            .state
            .credential
            .as_ref()
            .unwrap()
            .refresh_expires_at,
        tokens.credential.refresh_expires_at
    );
    assert!(tokens.credential.refresh_expires_at > Utc::now());
}

#[tokio::test]
async fn expired_pending_rotation_defers_rejection_to_center_and_preserves_recovery_state() {
    let (mut controller, mock) = fixture();
    let request_id = Uuid::new_v4();
    controller.state.pending = Some(Pending::Refresh(request_id));
    controller
        .state
        .credential
        .as_mut()
        .unwrap()
        .refresh_expires_at = Utc::now() - chrono::Duration::seconds(1);
    mock.0.lock().unwrap().refresh_error = Some(Error::Unauthorized);
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Unauthorized);
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.refreshes.len(), 1);
    assert_eq!(seen.refreshes[0].1, request_id);
    assert_eq!(seen.saved.len(), 1);
    assert!(matches!(controller.state.pending, Some(Pending::Refresh(id)) if id == request_id));
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

#[tokio::test]
async fn rejects_missing_expired_mismatched_or_unfinished_login_state() {
    let (mut controller, mock) = fixture();
    mock.0.lock().unwrap().mismatch = true;
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Protocol);
    controller.state.pending = None;
    controller
        .state
        .credential
        .as_mut()
        .unwrap()
        .refresh_expires_at = Utc::now();
    assert_eq!(controller.refresh().await.unwrap_err(), Error::Unauthorized);
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 1);
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
async fn optional_hub_link_starts_beside_relay_and_backs_off_on_failure() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().hub.error = Some(Error::Unavailable);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!((seen.starts, seen.hub.tickets, seen.hub.starts), (1, 1, 0));
        assert_eq!(seen.reports.last(), Some(&"running"));
    }
    mock.0.lock().unwrap().hub.error = None;
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(
        mock.0.lock().unwrap().hub.tickets,
        1,
        "waits before retrying"
    );
    for _ in 0..3 {
        tick().await;
    }
    let seen = mock.0.lock().unwrap().hub.starts;
    assert_eq!(seen, 1);
    tick().await;
    assert_eq!(
        mock.0.lock().unwrap().hub.starts,
        1,
        "an active link is kept"
    );
    cancel.cancel();
    task.await.unwrap().unwrap();
    assert!(!mock.0.lock().unwrap().hub.active);
}

#[tokio::test(start_paused = true)]
async fn relay_ticket_outage_does_not_block_hub_or_renewal_and_recovers() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().ticket_error = Some(Error::Unavailable);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    assert!(mock.0.lock().unwrap().hub.active);
    for _ in 0..10 {
        tick().await;
    }
    assert!(mock.0.lock().unwrap().renewals >= 10);
    mock.0.lock().unwrap().ticket_error = None;
    for _ in 0..31 {
        tick().await;
    }
    assert!(mock.0.lock().unwrap().active);
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn slow_tickets_do_not_block_the_other_link_or_renewal() {
    for slow_hub in [false, true] {
        let (controller, mock) = fixture();
        {
            let mut seen = mock.0.lock().unwrap();
            if slow_hub {
                seen.hub.ticket_delay = Duration::from_secs(5);
            } else {
                seen.ticket_delay = Duration::from_secs(5);
            }
        }
        let cancel = CancellationToken::new();
        let task = tokio::spawn(controller.run(cancel.clone()));
        tick().await;
        {
            let seen = mock.0.lock().unwrap();
            assert!(if slow_hub {
                seen.active
            } else {
                seen.hub.active
            });
        }
        for _ in 0..6 {
            tick().await;
        }
        {
            let seen = mock.0.lock().unwrap();
            assert!(seen.renewals >= 6);
            assert!(seen.active && seen.hub.active);
        }
        cancel.cancel();
        task.await.unwrap().unwrap();
    }
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

#[tokio::test]
async fn initial_terminal_failures_close_relay_and_report_the_recovery_action() {
    for (error, phase) in [
        (Error::Storage, "storage_error"),
        (Error::Conflict, "runtime_conflict"),
        (Error::Protocol, "protocol_error"),
        (Error::Denied, "reauthorization_required"),
    ] {
        let (controller, mock) = fixture();
        mock.0.lock().unwrap().refresh_error = Some(error);

        assert_eq!(controller.run(CancellationToken::new()).await, Err(error));

        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.reports, ["connecting", phase]);
        assert_eq!(seen.stops, 1);
        assert_eq!(seen.starts, 0);
        assert!(seen.registrations.is_empty());
        assert_eq!(seen.saved.len(), 1);
        assert!(matches!(seen.saved[0].pending, Some(Pending::Refresh(_))));
    }
}
