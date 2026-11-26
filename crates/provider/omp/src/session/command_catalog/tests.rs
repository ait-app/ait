use super::*;

fn pending(commands: &mut Commands, id: &str) -> oneshot::Sender<OptionalWrite> {
    let (sender, receiver) = oneshot::channel();
    commands.pending = Some(Pending {
        id: id.to_owned(),
        generation: commands.generation,
        cancel: CancellationToken::new(),
        written: Some(receiver),
        deadline: Instant::now() + CATALOG_TIMEOUT,
    });
    sender
}

#[test]
fn catalog_routes_builtin_aliases_and_native_collisions_without_guessing() {
    let mut commands = Commands::default();
    assert_eq!(commands.route("/model"), Some(CommandRoute::Unavailable));
    commands.update(&json!({"commands":[
        {"name":"model","source":"builtin","aliases":["m"]},
        {"name":"compact","source":"builtin","aliases":["c"]},
        {"name":"record","source":"extension"},
        {"name":"fork","aliases":["branch-me"]},
        {"name":"mystery","source":"future-source"},
        {"name":"shared","source":"builtin","aliases":["dupe"]},
        {"name":"other","source":"builtin","aliases":["dupe"]}
    ]}));
    assert_eq!(commands.route("/m x"), Some(CommandRoute::OutOfBand));
    assert_eq!(commands.canonical_text("/m x").as_deref(), Some("/model x"));
    assert_eq!(commands.route("/c x"), Some(CommandRoute::OutOfBand));
    assert_eq!(
        commands.canonical_text("/c x").as_deref(),
        Some("/compact x")
    );
    assert_eq!(commands.route("/record"), Some(CommandRoute::Foreground));
    assert_eq!(commands.route("/fork"), Some(CommandRoute::Unavailable));
    assert_eq!(
        commands.route("/branch-me"),
        Some(CommandRoute::Unavailable)
    );
    assert_eq!(commands.route("/mystery"), Some(CommandRoute::Foreground));
    assert_eq!(commands.route("/dupe"), Some(CommandRoute::Unavailable));
    assert_eq!(commands.route("/advisor"), Some(CommandRoute::OutOfBand));
    assert_eq!(commands.route("/handoff"), Some(CommandRoute::OutOfBand));
    assert_eq!(commands.route("hello"), None);
    commands.update(&json!({"commands":[
        {"name":"model","source":"builtin","aliases":["m"]},
        {"name":"plugin","source":"extension","aliases":["m"]}
    ]}));
    assert_eq!(commands.route("/m x"), Some(CommandRoute::Unavailable));
}

#[test]
fn late_or_failed_queries_never_override_a_newer_catalog() {
    let mut commands = Commands::default();
    let writer = pending(&mut commands, "query");
    writer.send(OptionalWrite::Sent).unwrap();
    commands.poll();
    commands.poll();
    assert!(
        commands.pending.is_some(),
        "a completed write is not a completed query"
    );
    commands.update(&json!({"commands":[{"name":"record","source":"extension"}]}));
    assert_eq!(
        commands.response(&json!({"id":"query","command":"get_available_commands",
        "success":true,"data":{"commands":[{"name":"record","source":"builtin"}]}})),
        Some(false)
    );
    assert_eq!(commands.route("/record"), Some(CommandRoute::Foreground));
    commands.update(&json!({"commands":"invalid"}));
    assert!(commands.catalog().is_none());
    assert_eq!(commands.route("/record"), Some(CommandRoute::Unavailable));
    let _writer = pending(&mut commands, "failed");
    assert_eq!(
        commands
            .response(&json!({"id":"failed","command":"get_available_commands","success":false})),
        Some(false)
    );
    assert!(commands.catalog().is_none());
}

#[test]
fn pending_catalog_is_bounded_and_late_responses_do_not_restore_it() {
    let mut commands = Commands::default();
    let writer = pending(&mut commands, "first");
    assert_eq!(commands.route("/record"), Some(CommandRoute::Pending));
    writer.send(OptionalWrite::Sent).unwrap();
    commands.poll();
    assert_eq!(commands.route("/record"), Some(CommandRoute::Pending));
    assert_eq!(
        commands.response(&json!({"id":"first","command":"get_available_commands",
        "success":true,"data":{"commands":[{"name":"record","source":"extension"}]}})),
        Some(true)
    );
    assert_eq!(commands.route("/record"), Some(CommandRoute::Foreground));

    commands.catalog = None;
    let writer = pending(&mut commands, "expired");
    commands.pending.as_mut().unwrap().deadline = Instant::now() - Duration::from_secs(1);
    assert_eq!(commands.route("/record"), Some(CommandRoute::Unavailable));
    commands.poll();
    assert!(commands.pending.is_none());
    assert!(writer.is_closed());
    assert_eq!(
        commands.response(&json!({"id":"expired","command":"get_available_commands",
        "success":true,"data":{"commands":[{"name":"record","source":"extension"}]}})),
        None
    );
    assert_eq!(commands.route("/record"), Some(CommandRoute::Unavailable));
}

#[test]
fn failed_or_malformed_catalogs_are_unavailable_instead_of_pending() {
    let mut commands = Commands::default();
    let writer = pending(&mut commands, "unwritten");
    writer.send(OptionalWrite::Unsent).unwrap();
    commands.poll();
    assert_eq!(commands.route("/record"), Some(CommandRoute::Unavailable));
    let _writer = pending(&mut commands, "malformed");
    assert_eq!(commands.route("/record"), Some(CommandRoute::Pending));
    assert_eq!(
        commands.response(&json!({"id":"malformed","command":"get_available_commands",
        "success":true,"data":{"commands":"invalid"}})),
        Some(false)
    );
    assert_eq!(commands.route("/record"), Some(CommandRoute::Unavailable));
    let _writer = pending(&mut commands, "old");
    commands.update(&json!({"commands":"invalid"}));
    assert_eq!(commands.route("/record"), Some(CommandRoute::Unavailable));
}

#[test]
fn goal_rpc_support_is_independent_of_completion_but_respects_native_claims() {
    let mut commands = Commands::default();
    commands.observe_state(&json!({"goal":null}));
    assert!(!commands.handles_goal("/goal show"));
    commands.update(&json!({"commands":[]}));
    assert!(commands.handles_goal("/goal show"));
    assert!(
        commands
            .catalog()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "goal")
    );

    for source in ["extension", "skill", "future-source"] {
        commands.update(&json!({"commands":[{"name":"goal","source":source}]}));
        assert!(!commands.handles_goal("/goal show"), "{source}");
        let catalog = commands.catalog().unwrap();
        assert_eq!(
            catalog
                .iter()
                .filter(|entry| entry["name"] == "goal")
                .count(),
            1
        );
    }
    commands.update(&json!({"commands":[
        {"name":"custom","source":"extension","aliases":["goal"]}
    ]}));
    assert!(!commands.handles_goal("/goal show"));
    assert!(
        !commands
            .catalog()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "goal")
    );

    commands.update(&json!({"commands":[
        {"name":"goal","source":"builtin","aliases":["g"]}
    ]}));
    assert!(commands.handles_goal("/g show"));
    commands.update(&json!({"commands":[
        {"name":"goal","source":"builtin"},
        {"name":"goal","source":"extension"}
    ]}));
    assert!(!commands.handles_goal("/goal show"));
}

#[test]
fn goal_capability_is_reset_by_a_process_without_valid_native_state() {
    let mut commands = Commands::default();
    commands.update(&json!({"commands":[]}));
    for state in [json!({}), json!({"goal":{}}), json!({"goal":"invalid"})] {
        commands.observe_state(&json!({"goal":null}));
        assert!(commands.handles_goal("/goal show"));
        commands.observe_state(&state);
        assert_eq!(
            commands.route("/goal show"),
            Some(CommandRoute::Unavailable)
        );
        assert!(
            !commands
                .catalog()
                .unwrap()
                .iter()
                .any(|entry| entry["name"] == "goal")
        );
    }
    commands.observe_state(&json!({"goal":{"enabled":false,"goal":{
        "objective":"Saved goal","status":"paused"
    }}}));
    assert!(commands.handles_goal("/goal resume"));
    commands.update(&json!({"commands":"invalid"}));
    assert!(!commands.handles_goal("/goal resume"));
}

#[test]
fn advisor_and_vibe_respect_current_command_ownership() {
    let mut commands = Commands::default();
    commands.update(&json!({"commands":[]}));
    assert!(commands.handles_advisor("/advisor on"));
    assert_eq!(
        commands.route("/vibe task"),
        Some(CommandRoute::Unavailable)
    );
    commands.update(&json!({"commands":[
        {"name":"advisor","source":"builtin","aliases":["adv"]},
        {"name":"ait-vibe-mode-v1","source":"extension"}
    ]}));
    assert!(commands.handles_advisor("/adv off"));
    assert!(commands.handles_vibe("/vibe on"));
    assert_eq!(commands.route("/vibe task"), Some(CommandRoute::Foreground));
    assert_eq!(commands.route("/vibe off"), Some(CommandRoute::OutOfBand));
    assert!(
        !commands
            .catalog()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "ait-vibe-mode-v1")
    );
    commands.update(&json!({"commands":[
        {"name":"advisor","source":"extension"},
        {"name":"vibe","source":"extension"},
        {"name":"ait-vibe-mode-v1","source":"extension"}
    ]}));
    assert!(!commands.handles_advisor("/advisor on"));
    assert!(!commands.handles_vibe("/vibe on"));
    assert_eq!(
        commands.route("/advisor on"),
        Some(CommandRoute::Foreground)
    );
    assert_eq!(commands.route("/vibe on"), Some(CommandRoute::Foreground));
}
