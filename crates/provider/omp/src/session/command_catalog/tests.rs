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
    assert_eq!(commands.route("/advisor"), Some(CommandRoute::Unavailable));
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
