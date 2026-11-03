use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use terminal::ports::{Launch, Observation, Process, Runtime};
use terminal::protocol::{Input, Restore};
use terminal::service::Terminals;

use super::*;

#[derive(Debug, Default)]
struct Gate {
    started: tokio::sync::Notify,
    released: Mutex<bool>,
    wake: Condvar,
    killed: Arc<AtomicBool>,
}

impl Gate {
    fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
}

struct Release(Arc<Gate>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[derive(Debug)]
struct MockRuntime(Arc<Gate>);

impl Runtime for MockRuntime {
    fn directory(&self, path: &str) -> Result<String, terminal::Error> {
        std::fs::canonicalize(path)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|_| terminal::Error::Invalid)
    }

    fn spawn(&self, _: &Launch) -> Result<Box<dyn Process>, terminal::Error> {
        self.0.started.notify_one();
        let mut released = self.0.released.lock().unwrap();
        while !*released {
            released = self.0.wake.wait(released).unwrap();
        }
        Ok(Box::new(MockProcess(self.0.killed.clone())))
    }
}

#[derive(Debug)]
struct MockProcess(Arc<AtomicBool>);

impl Process for MockProcess {
    fn title(&self) -> Option<String> {
        None
    }
    fn exited(&mut self) -> Result<bool, terminal::Error> {
        Ok(self.0.load(Ordering::SeqCst))
    }
    fn send(&mut self, _: &Input) -> Result<(), terminal::Error> {
        Ok(())
    }
    fn observe(
        &mut self,
        _: Option<u64>,
        _: Option<&Restore>,
    ) -> Result<Observation, terminal::Error> {
        Err(terminal::Error::NotFound)
    }
    fn capture(&self) -> Result<Vec<String>, terminal::Error> {
        Ok(Vec::new())
    }
    fn kill(&mut self) -> Result<(), terminal::Error> {
        self.0.store(true, Ordering::SeqCst);
        Ok(())
    }
}

async fn terminal_guest(gate: Arc<Gate>) -> Guest {
    let mut fixture = Workspaces::new(address());
    let registry = fixture.root.path().join("registry");
    let terminals = Terminals::new(
        Box::new(FileBackedWorkspaceRegistry::new(
            registry.join("workspaces.json"),
        )),
        Box::new(FileBackedProjectRegistry::new(
            registry.join("projects.json"),
        )),
        Box::new(MockRuntime(gate)),
    );
    let state = Arc::get_mut(&mut fixture.api.shared).unwrap();
    state.terminal = Arc::new(terminal::dispatch::State {
        runtime: state.runtime.clone(),
        terminals: Some(crate::shared_service(terminals)),
    });
    state.shares = Arc::new(crate::shares::Shares::new(&state.terminal, &state.provider));
    let visitor = fixture.accept_share().await;
    let session = admit(&fixture.api.shared, &visitor.encode()).await.unwrap();
    Guest {
        fixture,
        session,
        visitor,
    }
}

#[tokio::test]
async fn terminal_creation_keeps_tracking_after_the_request_is_dropped() {
    for revoke_during_spawn in [false, true] {
        let gate = Arc::new(Gate::default());
        let release = Release(gate.clone());
        let guest = terminal_guest(gate.clone()).await;
        let state = &guest.fixture.api.shared;
        {
            let creation = private::execute(
                &guest.session,
                state,
                "terminal.create.request",
                json!({"cwd": guest.root(), "workspaceId": guest.fixture.shared,
                    "size": {"rows": 24, "cols": 80}}),
                Private::TerminalCreate,
            );
            tokio::pin!(creation);
            tokio::select! {
                result = &mut creation => panic!("spawn should be blocked: {result:?}"),
                () = gate.started.notified() => {}
            }
            // Dropping this response future emulates the single-connection worker exiting.
        }
        if revoke_during_spawn {
            state
                .shares
                .remove(
                    ResourceKind::Workspace,
                    &guest.fixture.shared,
                    guest.fixture.group,
                )
                .await
                .unwrap();
        }
        drop(release);
        state.tasks.close();
        tokio::time::timeout(Duration::from_secs(5), state.tasks.wait())
            .await
            .unwrap();
        if !revoke_during_spawn {
            let ids = state.shares.terminal_ids();
            assert_eq!(ids.len(), 1);
            assert!(guest.session.owns_terminal(&ids[0]));
            state
                .shares
                .remove(
                    ResourceKind::Workspace,
                    &guest.fixture.shared,
                    guest.fixture.group,
                )
                .await
                .unwrap();
        }
        assert!(gate.killed.load(Ordering::SeqCst));
        assert!(state.shares.terminal_ids().is_empty());
    }
}

#[tokio::test]
async fn already_revoked_creation_never_starts_a_process() {
    let gate = Arc::new(Gate::default());
    let _release = Release(gate.clone());
    let guest = terminal_guest(gate.clone()).await;
    let state = &guest.fixture.api.shared;
    state
        .shares
        .remove(
            ResourceKind::Workspace,
            &guest.fixture.shared,
            guest.fixture.group,
        )
        .await
        .unwrap();
    assert_eq!(
        private::execute(
            &guest.session,
            state,
            "terminal.create.request",
            json!({}),
            Private::TerminalCreate
        )
        .await,
        Err(ErrorCode::Forbidden)
    );
    assert!(!gate.killed.load(Ordering::SeqCst));
}
