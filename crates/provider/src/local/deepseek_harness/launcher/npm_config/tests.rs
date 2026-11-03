use std::time::Instant;

use super::super::{resolve, tests::file};
use super::*;
use crate::ports::agent_session::AgentClient;

fn npm(root: &Path, output: &str) -> PathBuf {
    let program = root.join("node tools/npm");
    file(
        &program,
        &format!(
            "#!/bin/sh\n[ \"$*\" = '--global config get prefix cache' ] || exit 1\nprintf '%s' '{output}'\n"
        ),
        true,
    );
    program
}

#[test]
fn npm_reports_arbitrary_prefix_and_cache_paths() {
    let root = tempfile::tempdir().unwrap();
    let prefix = root.path().join("applications/custom prefix=version");
    let cache = root.path().join("cache outside home");
    let program = npm(
        root.path(),
        &format!("prefix={}\ncache={}\n", prefix.display(), cache.display()),
    );

    let configuration = read(&[program.parent().unwrap().into()]);

    assert_eq!(configuration, Some(Configuration { prefix, cache }));
    assert_eq!(read(&[]), None);
}

#[test]
fn invalid_or_incomplete_npm_output_is_rejected() {
    for output in [
        "",
        "prefix=/install\n",
        "prefix=relative\ncache=/cache\n",
        "prefix=/install\ncache=undefined\n",
        "prefix=/install\ncache=/cache\nprefix=/other\n",
        "prefix=/install\ncache=/cache\nunexpected=/value\n",
        "prefix=/install\ncache=/cache\nextra output\n",
        &format!("prefix=/{}\ncache=/cache\n", "x".repeat(64 * 1024)),
    ] {
        assert_eq!(parse(output), None);
    }
}

#[tokio::test]
async fn synchronous_discovery_can_run_inside_an_existing_runtime() {
    let root = tempfile::tempdir().unwrap();
    let program = npm(
        root.path(),
        "prefix=/effective/install\ncache=/effective/cache\n",
    );

    assert_eq!(
        read(&[program.parent().unwrap().into()]),
        Some(Configuration {
            prefix: "/effective/install".into(),
            cache: "/effective/cache".into(),
        })
    );
}

#[tokio::test]
async fn failed_or_unavailable_npm_does_not_produce_guessed_paths() {
    let root = tempfile::tempdir().unwrap();
    let program = npm(root.path(), "malformed");
    assert_eq!(query(&program, Duration::from_secs(1)).await, None);
    file(&program, "#!/bin/sh\nexit 1\n", true);
    assert_eq!(query(&program, Duration::from_secs(1)).await, None);
    std::fs::remove_file(&program).unwrap();
    assert_eq!(query(&program, Duration::from_secs(1)).await, None);
}

#[tokio::test]
async fn stalled_npm_is_terminated_within_the_query_budget() {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("npm");
    file(&program, "#!/bin/sh\nexec /bin/sleep 10\n", true);
    let start = Instant::now();

    assert_eq!(query(&program, Duration::from_millis(50)).await, None);

    assert!(start.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn reported_prefix_runs_a_cli_outside_path() {
    let root = tempfile::tempdir().unwrap();
    let prefix = root.path().join("unusual installation location");
    let cache = root.path().join("different cache location");
    let program = npm(
        root.path(),
        &format!("prefix={}\ncache={}\n", prefix.display(), cache.display()),
    );
    let cli = prefix.join("bin/dsh");
    file(
        &cli,
        "#!/bin/sh\nprintf 'discovered-cli-fixture\\n'\n",
        true,
    );
    let configuration = read(&[program.parent().unwrap().into()]).unwrap();

    let client = resolve(
        &[configuration.prefix.join("bin")],
        &[],
        Some(&configuration.cache),
    );

    assert_eq!(client.program, cli);
    assert!(client.is_available().await.unwrap());
    let output = client.command().arg("--version").output().await.unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"discovered-cli-fixture\n");
}
