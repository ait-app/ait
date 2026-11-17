use super::*;

#[test]
fn slash_text_splits_as_paseo_parses_it() {
    assert_eq!(
        split("  /compact keep it short "),
        Some(("compact", "keep it short"))
    );
    assert_eq!(split("/handoff"), Some(("handoff", "")));
    assert_eq!(split("/a/b x"), None);
    assert_eq!(split("/"), None);
    assert_eq!(split("compact"), None);
}

#[test]
fn only_the_built_ins_run_out_of_band_and_queued_messages_need_text() {
    assert_eq!(builtin("/Compact"), Some((Builtin::Compact, "")));
    assert_eq!(
        builtin("/autocompact off"),
        Some((Builtin::AutoCompact, "off"))
    );
    assert_eq!(builtin("/handoff next"), Some((Builtin::Handoff, "next")));
    assert_eq!(builtin("/steer go left"), Some((Builtin::Steer, "go left")));
    assert_eq!(
        builtin("/follow-up then this"),
        Some((Builtin::FollowUp, "then this"))
    );
    // Without a message Paseo sends the text as an ordinary prompt.
    assert_eq!(builtin("/steer  "), None);
    assert_eq!(builtin("/follow-up"), None);
    assert_eq!(builtin("/session"), None);
    assert_eq!(builtin("/skill:review x"), None);
}

#[test]
fn autocompact_accepts_paseos_words() {
    for on in ["on", "TRUE", "enable", "enabled"] {
        assert_eq!(auto_compact(on), AutoCompact::Set(true), "{on}");
    }
    for off in ["off", "false", "Disable", "disabled"] {
        assert_eq!(auto_compact(off), AutoCompact::Set(false), "{off}");
    }
    assert_eq!(auto_compact(""), AutoCompact::Toggle);
    assert_eq!(auto_compact("toggle"), AutoCompact::Toggle);
    assert_eq!(auto_compact("sometimes"), AutoCompact::Unknown);
}

#[test]
fn the_list_merges_built_ins_with_omps_commands() {
    let listed = list(&json!({"commands":[
        {"name":"review","description":"Review","source":"skill"},
        {"name":"compact","description":"Compact now","source":"builtin"},
        {"name":"handoff","description":"Hand off","input":{"hint":"[focus]"},"source":"builtin"},
        {"name":"plain","source":"extension"},
        {"name":"bare"},
        {"name":"bare","description":"Later wins"},
        {"name":"advisor","description":"Toggle the advisor","source":"builtin"},
        {"name":"MODEL","description":"Change model","source":"builtin"},
        {"name":"fork","description":"Fork the session","source":"builtin"},
        {"name":"Settings","description":"Open settings","source":"builtin"},
        {"name":"QUIT","description":"Leave","source":"builtin"}
    ]}))
    .unwrap();

    let names: Vec<_> = listed.iter().map(|command| &command["name"]).collect();
    assert_eq!(
        names,
        [
            "compact",
            "autocompact",
            "handoff",
            "steer",
            "follow-up",
            "review",
            "plain",
            "bare"
        ]
    );
    // OMP's entry wins but keeps the built-in hint when it has none.
    assert_eq!(
        listed[0],
        json!({"name":"compact","description":"Compact now","argumentHint":"[instructions]",
            "kind":"command"})
    );
    assert_eq!(listed[1]["argumentHint"], "[on|off|toggle]");
    assert_eq!(listed[2]["argumentHint"], "[focus]");
    assert_eq!(listed[5]["kind"], "skill");
    assert_eq!(listed[6]["description"], "extension");
    assert_eq!(listed[7]["description"], "Later wins");
    // Blocked entries are left out, so completion never offers a refused command.
    assert!(
        listed.iter().all(|command| {
            let name = command["name"].as_str().unwrap_or_default();
            MANAGED
                .iter()
                .all(|(blocked, _)| !blocked.eq_ignore_ascii_case(name))
        }),
        "{listed:?}"
    );
    assert!(list(&json!({"commands":[{"name":""}]})).is_err());
    assert!(list(&json!({})).is_err());
}

#[test]
fn only_managed_commands_are_blocked_with_their_answer() {
    use super::Managed::{NoControl as Uncontrolled, Surface, TerminalOnly, Unavailable};
    assert_eq!(gui_managed("/advisor off"), Some(("advisor", Uncontrolled)));
    assert_eq!(gui_managed("/Advisor OFF"), Some(("advisor", Uncontrolled)));
    assert_eq!(
        gui_managed("  /models "),
        Some(("models", Surface("the model selector")))
    );
    assert_eq!(
        gui_managed("/extended-context on"),
        Some(("extended-context", Uncontrolled))
    );
    assert_eq!(
        gui_managed("/worktree feature/x"),
        Some(("worktree", Unavailable))
    );
    assert_eq!(gui_managed("/Q"), Some(("q", TerminalOnly)));
    // Arguments never change the match, and unrelated text is not blocked.
    assert_eq!(gui_managed("/steering"), None);
    assert_eq!(gui_managed("/compact"), None);
    assert_eq!(gui_managed("/autocompact"), None);
    assert_eq!(gui_managed("/handoff"), None);
    assert_eq!(gui_managed("/follow-up now"), None);
    assert_eq!(gui_managed("/review"), None);
    assert_eq!(gui_managed("/session"), None);
    assert_eq!(gui_managed("/advisorx off"), None);
    assert_eq!(gui_managed("advisor off"), None);
    assert_eq!(gui_managed("/"), None);
}

#[test]
fn every_blocked_row_of_the_split_is_blocked_here() {
    // The split's `commands_tsv` blocks 62 rows; with aliases that is these 69 names.
    let expected: [(&str, Managed); 69] = [
        ("model", Managed::Surface("the model selector")),
        ("models", Managed::Surface("the model selector")),
        ("switch", Managed::Surface("the model selector")),
        (
            "modelpreset",
            Managed::Surface("Settings › Providers › Model presets"),
        ),
        ("effort", Managed::Surface("the Thinking control")),
        ("fast", Managed::Surface("the Speed control")),
        ("settings", Managed::Surface("Settings")),
        ("setup", Managed::Surface("Settings › Providers")),
        ("providers", Managed::Surface("Settings › Providers")),
        ("login", Managed::Surface("the Online Service account page")),
        (
            "logout",
            Managed::Surface("the Online Service account page"),
        ),
        ("hub", Managed::Surface("the Subagents panel")),
        ("subagents", Managed::Surface("the Subagents panel")),
        ("resume", Managed::Surface("History")),
        ("branch", Managed::Surface("the Rewind menu")),
        ("rewind", Managed::Surface("the Rewind menu")),
        ("advisor", Managed::NoControl),
        ("prewalk", Managed::NoControl),
        ("slow", Managed::NoControl),
        ("skillful", Managed::NoControl),
        ("extended-context", Managed::NoControl),
        ("computer", Managed::NoControl),
        ("browser", Managed::NoControl),
        ("agents", Managed::NoControl),
        ("tree", Managed::NoControl),
        ("export", Managed::NoControl),
        ("dump", Managed::NoControl),
        ("todo", Managed::Unavailable),
        ("plan-review", Managed::Unavailable),
        ("new", Managed::Unavailable),
        ("clear", Managed::Unavailable),
        ("delete", Managed::Unavailable),
        ("fresh", Managed::Unavailable),
        ("shake", Managed::Unavailable),
        ("pin", Managed::Unavailable),
        ("unpin", Managed::Unavailable),
        ("rename", Managed::Unavailable),
        ("move", Managed::Unavailable),
        ("wt", Managed::Unavailable),
        ("worktree", Managed::Unavailable),
        ("fork", Managed::Unavailable),
        ("restart", Managed::Unavailable),
        ("mcp", Managed::Unavailable),
        ("skills", Managed::Unavailable),
        ("marketplace", Managed::Unavailable),
        ("plugins", Managed::Unavailable),
        ("plugin", Managed::Unavailable),
        ("reload-plugins", Managed::Unavailable),
        ("ssh", Managed::Unavailable),
        ("forget-approvals", Managed::Unavailable),
        ("add-dir", Managed::Unavailable),
        ("remove-dir", Managed::Unavailable),
        ("copy", Managed::Unavailable),
        ("open", Managed::Unavailable),
        ("local", Managed::Unavailable),
        ("git", Managed::Unavailable),
        ("share", Managed::Unavailable),
        ("collab", Managed::Unavailable),
        ("join", Managed::Unavailable),
        ("leave", Managed::Unavailable),
        ("pause", Managed::Unavailable),
        ("live", Managed::Unavailable),
        ("retry", Managed::Unavailable),
        ("hotkeys", Managed::TerminalOnly),
        ("quit", Managed::TerminalOnly),
        ("q", Managed::TerminalOnly),
        ("exit", Managed::TerminalOnly),
        ("record", Managed::TerminalOnly),
        ("debug", Managed::TerminalOnly),
    ];
    assert_eq!(MANAGED, expected);
    for (name, managed) in expected {
        assert_eq!(gui_managed(&format!("/{name}")), Some((name, managed)));
        assert_eq!(
            gui_managed(&format!("/{} args here", name.to_uppercase())),
            Some((name, managed)),
            "{name}"
        );
    }
}

#[test]
fn managed_notes_say_where_the_setting_lives_or_that_ait_has_none() {
    assert_eq!(
        gui_managed_note("advisor", Managed::NoControl),
        "`/advisor` is managed by Ait, which has no control for it yet."
    );
    assert_eq!(
        gui_managed_note("model", Managed::Surface("the model selector")),
        "`/model` is managed by Ait. Change it in the model selector."
    );
    assert_eq!(
        gui_managed_note("fork", Managed::Unavailable),
        "`/fork` is not available in Ait yet, so it is not sent to OMP."
    );
    assert_eq!(
        gui_managed_note("quit", Managed::TerminalOnly),
        "`/quit` is a terminal-only OMP command, which Ait does not run."
    );
}

#[test]
fn slash_names_label_command_cards() {
    assert_eq!(slash_name("/compact"), Some("compact".to_owned()));
    assert_eq!(slash_name("/Compact now"), Some("compact".to_owned()));
    // OMP's own commands name their cards too, not just the built-ins Ait runs.
    assert_eq!(slash_name("/session"), Some("session".to_owned()));
    assert_eq!(slash_name("  /skillful on "), Some("skillful".to_owned()));
    assert_eq!(slash_name("hello"), None);
    assert_eq!(slash_name("/"), None);
    assert_eq!(slash_name("/a/b"), None);
}
