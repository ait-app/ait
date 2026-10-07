use super::*;
use crate::service::workspace_collaboration::SharedWorkspaceSetup;
use model::workspace::lifecycle::WorkspaceSetup;

#[test]
fn setup_port_retains_the_shared_service_and_trust_checks() {
    let (automation, _, runtime) = service();
    let shared = Arc::new(Mutex::new(automation));
    let port = SharedWorkspaceSetup::new(shared.clone());
    assert!(port.start_created_setup("trusted").unwrap());
    assert!(!port.start_created_setup("blocked").unwrap());
    assert!(port.start_created_setup("missing").is_err());
    assert_eq!(runtime.setups.lock().unwrap().len(), 1);
    shared
        .lock()
        .unwrap()
        .start_created_setup("trusted")
        .unwrap();
    assert_eq!(runtime.setups.lock().unwrap().len(), 2);
}

#[test]
fn setup_port_reports_a_poisoned_owner_lock() {
    let (automation, _, _) = service();
    let shared = Arc::new(Mutex::new(automation));
    let owner = shared.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = owner.lock().unwrap();
            panic!("inject poisoned setup owner");
        })
        .join()
        .is_err()
    );
    assert!(
        SharedWorkspaceSetup::new(shared)
            .start_created_setup("trusted")
            .unwrap_err()
            .message
            .contains("poisoned")
    );
}
