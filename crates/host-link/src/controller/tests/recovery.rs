use super::*;
use crate::service::Service;

#[tokio::test(start_paused = true)]
async fn access_refusal_recovers_with_valid_refresh() {
    let (controller, mock) = fixture();
    let service = Service::new(
        mock.clone(),
        mock.clone(),
        mock.clone(),
        controller.state,
        Uuid::new_v4(),
    );
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    mock.0.lock().unwrap().renew_error = Some(Error::AccessRejected);
    for _ in 0..10 {
        tick().await;
    }
    let observed = {
        let seen = mock.0.lock().unwrap();
        (seen.active, seen.hub.active, seen.refreshes.len())
    };
    cancel.cancel();
    task.await.unwrap();
    assert_eq!(
        observed,
        (true, true, 2),
        "an expired access JWT must probe the still-valid refresh credential before abandoning publication"
    );
}

#[tokio::test(start_paused = true)]
async fn persistent_access_refusal_probes_refresh_once_then_stops() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().ticket_error = Some(Error::AccessRejected);
    let task = tokio::spawn(controller.run(CancellationToken::new()));
    for _ in 0..10 {
        tick().await;
    }
    assert!(task.is_finished());
    assert_eq!(task.await.unwrap(), Err(Error::Unauthorized));
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.refreshes.len(), 2);
    assert!(!seen.active && !seen.hub.active);
}

#[tokio::test(start_paused = true)]
async fn persistent_renewal_refusal_does_not_loop_on_refresh() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().renew_errors = [Error::AccessRejected; 2].into();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..10 {
        tick().await;
    }
    let finished = task.is_finished();
    cancel.cancel();
    let result = task.await.unwrap();
    assert!(
        finished,
        "repeated 401 must terminate after one refresh probe"
    );
    assert_eq!(result, Err(Error::Unauthorized));
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 2);
}

#[tokio::test(start_paused = true)]
async fn rejected_refresh_after_access_refusal_requires_login() {
    let (controller, mock) = fixture();
    let task = tokio::spawn(controller.run(CancellationToken::new()));
    tick().await;
    {
        let mut seen = mock.0.lock().unwrap();
        seen.renew_error = Some(Error::AccessRejected);
        seen.refresh_error = Some(Error::Unauthorized);
    }
    for _ in 0..5 {
        tick().await;
    }
    assert!(task.is_finished());
    assert_eq!(task.await.unwrap(), Err(Error::Unauthorized));
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 2);
}
