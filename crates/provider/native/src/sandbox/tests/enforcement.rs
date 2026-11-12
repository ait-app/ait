//! Run a real shell under the generated Seatbelt profile.

use std::path::{Path, PathBuf};

use super::*;

/// Directories outside the profile's temporary roots, so a sibling write is not allowed by
/// `/private/var/folders`. The test binary's own build directory serves that purpose.
fn build_scratch() -> tempfile::TempDir {
    let executable = std::env::current_exe().expect("test executable path");
    let directory = executable.parent().expect("build directory");
    let real = directory.canonicalize().expect("real build directory");
    assert!(
        !real.starts_with("/private/tmp") && !real.starts_with("/private/var/folders"),
        "the build directory must be outside the sandbox temporary roots: {}",
        real.display()
    );
    tempfile::Builder::new()
        .prefix("ait-sandbox-enforcement-")
        .tempdir_in(directory)
        .expect("scratch directory")
}

async fn shell(confinement: &WriteConfinement, cwd: &Path, script: &str) -> std::process::Output {
    let home = std::env::var("HOME").expect("HOME");
    let lookup = move |name: &str| (name == "HOME").then(|| home.clone());
    let mut command =
        command(Path::new("/bin/sh"), Some(confinement), &[], &lookup).expect("confined command");
    command
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .output()
        .await
        .expect("sandboxed shell runs")
}

fn probe(directory: &Path, name: &str) -> PathBuf {
    directory.join(format!("{name}-{}", std::process::id()))
}

#[tokio::test]
async fn shell_writes_inside_the_worktree_but_not_beside_it_or_in_home() {
    let scratch = build_scratch();
    let worktree = scratch.path().join("wt");
    let sibling = scratch.path().join("sibling");
    fs::create_dir_all(worktree.join(".git")).expect("worktree");
    fs::create_dir_all(&sibling).expect("sibling");
    let home = PathBuf::from(std::env::var("HOME").expect("HOME"));
    let inside = worktree.join("inside.txt");
    let beside = probe(&sibling, "beside");
    let in_home = probe(&home, ".ait-sandbox-probe");
    let git_config = worktree.join(".git/config");
    let script = format!(
        "echo ok > '{}'; echo no > '{}'; echo no > '{}'; echo no > '{}'; exit 0",
        inside.display(),
        beside.display(),
        in_home.display(),
        git_config.display(),
    );

    let output = shell(&confinement(&worktree), &worktree, &script).await;
    let home_written = in_home.exists();
    if home_written {
        fs::remove_file(&in_home).expect("remove leaked home probe");
    }

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(&inside).expect("worktree write"), "ok\n");
    assert!(!beside.exists(), "sibling directory write was allowed");
    assert!(!home_written, "home directory write was allowed");
    assert!(
        !git_config.exists(),
        "protected Git configuration was written"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("Operation not permitted").count(),
        3,
        "{stderr}"
    );
}

#[tokio::test]
async fn temporary_directory_and_null_device_stay_writable() {
    let scratch = build_scratch();
    let worktree = scratch.path().join("wt");
    fs::create_dir_all(worktree.join(".git")).expect("worktree");
    let temporary = tempfile::tempdir().expect("system temporary directory");
    let file = temporary.path().join("scratch.txt");
    let script = format!("echo ok > '{}' && echo ok > /dev/null", file.display());

    let output = shell(&confinement(&worktree), &worktree, &script).await;

    assert!(output.status.success(), "{output:?}");
    assert!(file.exists());
}
