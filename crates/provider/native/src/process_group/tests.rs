use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

use super::*;

fn spawn_group() -> std::process::Child {
    use std::os::unix::process::CommandExt;
    Command::new("/bin/sh")
        .args(["-c", "sleep 30 & wait"])
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap()
}

#[test]
fn signals_reach_the_whole_owned_group() {
    for (kind, number) in [(Signal::Terminate, 15), (Signal::Kill, 9)] {
        let mut child = spawn_group();

        signal(child.id().into(), kind);

        assert_eq!(child.wait().unwrap().signal(), Some(number));
    }
}

#[test]
fn exited_or_missing_groups_are_ignored() {
    let mut child = spawn_group();
    let group = child.id();
    signal(Some(group), Signal::Kill);
    child.wait().unwrap();

    signal(Some(group), Signal::Kill);
    signal(None, Signal::Terminate);
    signal(Some(0), Signal::Terminate);
}
