use std::sync::Arc;

use persistence::storage::project_config::LocalProjectConfigStore;

use super::*;

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
