use std::ffi::{OsStr, OsString};

use super::{is_private, private_environment, private_names};

fn pairs(names: &[&str]) -> Vec<(OsString, OsString)> {
    names
        .iter()
        .map(|name| (OsString::from(*name), OsString::from("value")))
        .collect()
}

#[test]
fn credential_prefixes_are_private() {
    for name in [
        "BONSAI_RUNTIME_TOKEN",
        "BONSAI_RUNTIME_URL",
        "BONSAI_RUNTIME_ID",
        "BONSAI_RUNTIME_MCP",
        "AIT_SERVER_TOKEN",
        "AIT_SERVER_CREDENTIAL_OPENAI",
        "bonsai_runtime_token",
        "Ait_Server_Token",
    ] {
        assert!(is_private(OsStr::new(name)), "{name}");
    }
    for name in [
        "PATH",
        "HOME",
        "BONSAI_TOKEN",
        "XAIT_SERVER_TOKEN",
        "AIT_SERVER",
        "AIT",
        "",
    ] {
        assert!(!is_private(OsStr::new(name)), "{name}");
    }
}

#[test]
fn only_private_names_are_selected_in_order() {
    let selected = private_names(pairs(&[
        "PATH",
        "BONSAI_RUNTIME_TOKEN",
        "HOME",
        "AIT_SERVER_TOKEN",
    ]));
    assert_eq!(
        selected,
        vec![
            OsString::from("BONSAI_RUNTIME_TOKEN"),
            OsString::from("AIT_SERVER_TOKEN")
        ]
    );
}

#[test]
fn the_process_environment_listing_contains_only_private_names() {
    assert!(private_environment().iter().all(|name| is_private(name)));
}

#[cfg(unix)]
#[test]
fn removed_names_never_reach_a_child() {
    let mut command = std::process::Command::new("/usr/bin/env");
    command
        .env("BONSAI_RUNTIME_TOKEN", "runtime-secret")
        .env("AIT_SERVER_TOKEN", "server-secret")
        .env("KEEP_ME", "visible");
    let inherited = pairs(&["BONSAI_RUNTIME_TOKEN", "AIT_SERVER_TOKEN", "KEEP_ME"]);
    for name in private_names(inherited) {
        command.env_remove(name);
    }
    let output = command.output().expect("run env");
    let printed = String::from_utf8_lossy(&output.stdout);
    assert!(printed.contains("KEEP_ME=visible"));
    assert!(!printed.contains("runtime-secret"));
    assert!(!printed.contains("server-secret"));
}
