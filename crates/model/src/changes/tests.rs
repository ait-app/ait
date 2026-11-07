use super::Changes;

#[test]
fn subscribers_observe_committed_notifications() {
    let changes = Changes::default();
    let mut receiver = changes.subscribe();
    assert!(!receiver.has_changed().expect("sender remains available"));

    changes.notify();
    assert!(receiver.has_changed().expect("sender remains available"));
    receiver.borrow_and_update();
    assert!(!receiver.has_changed().expect("sender remains available"));
}

#[test]
fn extensions_observe_their_parent_but_not_the_reverse() {
    let parent = Changes::default();
    let extension = parent.extend();
    let parent_observer = parent.subscribe();
    let mut extension_observer = extension.subscribe();

    extension.notify();
    assert!(
        extension_observer
            .has_changed()
            .expect("sender remains available")
    );
    assert!(
        !parent_observer
            .has_changed()
            .expect("sender remains available")
    );
    extension_observer.borrow_and_update();

    parent.clone().notify();
    assert!(
        parent_observer
            .has_changed()
            .expect("sender remains available")
    );
    assert!(
        extension_observer
            .has_changed()
            .expect("sender remains available")
    );
}
