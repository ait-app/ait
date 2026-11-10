use std::sync::Arc;

use persistence::storage::project_config::LocalProjectConfigStore;

use super::super::ScriptProcess;
use super::*;
use crate::automation::ports::{ScriptType, SetupLifecycle, SetupSnapshot};

#[test]
fn cancellation_timeout_retains_ownership_and_the_same_workspace_can_be_retried() {
    let inner = Inner::new(Arc::new(LocalProjectConfigStore));
    lock(&inner.state).setup_running.insert("pending".into());
    let ids = vec!["pending".into()];
    assert!(close_until(&inner, &ids, Instant::now()).is_err());
    assert!(lock(&inner.state).setup_cancelled.contains("pending"));
    lock(&inner.state).setup_running.remove("pending");
    close(&inner, &ids).unwrap();
    assert!(!lock(&inner.state).setup_cancelled.contains("pending"));
}

#[test]
fn closing_releases_setup_and_script_state_but_keeps_the_workspace_port() {
    let inner = Inner::new(Arc::new(LocalProjectConfigStore));
    {
        let mut state = lock(&inner.state);
        for id in ["archived", "kept"] {
            state.store_setup(
                id.to_owned(),
                SetupSnapshot {
                    revision: 0,
                    lifecycle: SetupLifecycle::Completed,
                    worktree_path: format!("/tmp/{id}"),
                    branch_name: "main".to_owned(),
                    log: String::new(),
                    commands: Vec::new(),
                    truncated: false,
                    error: None,
                },
            );
            state.scripts.insert(
                (id.to_owned(), "web".to_owned()),
                ScriptProcess {
                    kind: ScriptType::Service,
                    hostname: "localhost".to_owned(),
                    port: None,
                    terminal_id: format!("{id}-terminal"),
                    child: None,
                    accepts_args: false,
                    run: super::super::ScriptRun::start(
                        "web",
                        &[],
                        super::super::runs::CheckoutEvidence::default(),
                        Arc::default(),
                    ),
                },
            );
            state.workspace_ports.insert(id.to_owned(), 4000);
        }
    }

    close(&inner, &["archived".to_owned()]).unwrap();

    let state = lock(&inner.state);
    assert!(!state.setups.contains_key("archived"));
    assert!(state.setups.contains_key("kept"));
    assert!(
        !state
            .scripts
            .contains_key(&("archived".to_owned(), "web".to_owned()))
    );
    assert!(
        state
            .scripts
            .contains_key(&("kept".to_owned(), "web".to_owned()))
    );
    assert_eq!(state.workspace_ports.get("archived"), Some(&4000));
}
