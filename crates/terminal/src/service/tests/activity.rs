use crate::activity::ReportState;
use crate::protocol::{Input, ListRequest};
use crate::tests::support::{fixture, request};

#[test]
fn reports_require_a_live_terminal_and_its_own_secret_and_interrupts_clear_running() {
    let (mut service, _, calls) = fixture();
    service.set_activity_url("http://127.0.0.1:6767/api/terminal-activity".into());
    let first = service.create(&request()).unwrap();
    let second = service.create(&request()).unwrap();
    let token = calls.lock().unwrap().launches[0].env["PASEO_ACTIVITY_TOKEN"].clone();
    let launch = calls.lock().unwrap().launches[0].clone();
    assert_eq!(launch.env["PASEO_TERMINAL_ID"], first.id);
    assert_eq!(
        launch.env["PASEO_TERMINAL_ACTIVITY_URL"],
        "http://127.0.0.1:6767/api/terminal-activity"
    );
    assert!(!format!("{service:?} {launch:?}").contains(&token));
    for id in ["unknown", &second.id] {
        assert!(
            !service
                .report_activity(id, &token, ReportState::Running)
                .unwrap()
        );
    }
    assert!(
        !service
            .report_activity(&first.id, "wrong", ReportState::Running)
            .unwrap()
    );
    assert!(
        service
            .report_activity(&first.id, &token, ReportState::Running)
            .unwrap()
    );
    let listed = service.list(&ListRequest::default()).unwrap();
    assert!(
        listed
            .iter()
            .find(|info| info.id == first.id)
            .unwrap()
            .activity
            .is_some()
    );
    for input in ["\u{3}", "\u{1b}"] {
        service
            .report_activity(&first.id, &token, ReportState::Running)
            .unwrap();
        service
            .input(&first.id, "owner", &Input::Input { data: input.into() })
            .unwrap();
        assert!(service.activity_source().get(&first.id).is_none());
    }
    service
        .report_activity(&first.id, &token, ReportState::Running)
        .unwrap();
    service
        .input(
            &first.id,
            "owner",
            &Input::Input {
                data: "pasted\u{3}text".into(),
            },
        )
        .unwrap();
    assert!(service.activity_source().get(&first.id).is_some());
    service.kill(&first.id).unwrap();
    assert!(
        !service
            .report_activity(&first.id, &token, ReportState::Running)
            .unwrap()
    );
    assert!(service.activity_source().get(&first.id).is_none());
}

#[test]
fn failed_input_keeps_activity_and_natural_exit_clears_the_workspace_projection() {
    let (mut service, _, calls) = fixture();
    let terminal = service.create(&request()).unwrap();
    let token = calls.lock().unwrap().launches[0].env["PASEO_ACTIVITY_TOKEN"].clone();
    service
        .report_activity(&terminal.id, &token, ReportState::Running)
        .unwrap();
    calls.lock().unwrap().send_failure = true;
    assert!(
        service
            .input(
                &terminal.id,
                "owner",
                &Input::Input {
                    data: "\u{3}".into()
                }
            )
            .is_err()
    );
    assert!(service.activity_source().get(&terminal.id).is_some());
    calls.lock().unwrap().exited = true;
    service.reconcile().unwrap();
    assert!(service.activity_source().get(&terminal.id).is_none());
    assert!(
        !service
            .report_activity(&terminal.id, &token, ReportState::Idle)
            .unwrap()
    );
}

#[test]
fn attention_events_are_owned_by_the_terminal_and_repeat_reports_do_not_notify_twice() {
    use model::session::SessionEvents;
    use model::session::protocol::EventsRequest;
    use std::sync::{Arc, Mutex};
    let events = SessionEvents::default();
    let connection = events.connect();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    let subscription = connection
        .subscribe(
            EventsRequest {
                events: vec!["terminal_attention_required".into()],
                notifications: true,
            },
            Arc::new(move |_, value| {
                sink.lock().unwrap().push(value);
                Ok(())
            }),
        )
        .unwrap();
    subscription.activate().unwrap();
    let (mut service, _, calls) = fixture();
    service.set_session_events(events, "server".into());
    let terminal = service.create(&request()).unwrap();
    let token = calls.lock().unwrap().launches[0].env["PASEO_ACTIVITY_TOKEN"].clone();
    for state in [
        ReportState::Running,
        ReportState::Idle,
        ReportState::Idle,
        ReportState::NeedsInput,
        ReportState::NeedsInput,
    ] {
        service
            .report_activity(&terminal.id, &token, state)
            .unwrap();
    }
    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0]["terminalId"], terminal.id);
    assert_eq!(captured[0]["workspaceId"], terminal.workspace_id);
    assert_eq!(captured[0]["serverId"], "server");
    assert_eq!(captured[0]["title"], "Terminal finished");
    assert_eq!(captured[1]["title"], "Terminal needs input");
    assert_eq!(captured[1]["body"], terminal.name);
    assert_eq!(captured[0]["shouldNotify"], false);
    assert!(!serde_json::to_string(&*captured).unwrap().contains(&token));
}
