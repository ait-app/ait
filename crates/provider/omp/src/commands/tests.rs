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
        {"name":"bare","description":"Later wins"}
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
    assert!(list(&json!({"commands":[{"name":""}]})).is_err());
    assert!(list(&json!({})).is_err());
}
