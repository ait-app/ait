use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;

const NAME: &str = "omp-test-peer";
static PROGRAM: OnceLock<PathBuf> = OnceLock::new();

/// Locate the example built by Cargo, or build it for a library-only test run.
pub(super) fn program() -> &'static Path {
    PROGRAM.get_or_init(|| {
        let current = std::env::current_exe().expect("locate OMP test executable");
        let executable = format!("{NAME}{}", std::env::consts::EXE_SUFFIX);
        if let Some(program) = current
            .ancestors()
            .map(|directory| directory.join("examples").join(&executable))
            .find(|program| current_example(program))
        {
            return program;
        }

        // Cargo builds examples for a normal test run, but skips them with `--lib`.
        let build = Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "build",
                "--locked",
                "--example",
                NAME,
                "--message-format=json",
            ])
            .output()
            .expect("start Cargo to build the OMP test peer");
        assert!(
            build.status.success(),
            "build OMP test peer: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        let messages = String::from_utf8_lossy(&build.stdout);
        messages
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find_map(|message| {
                if message["reason"] == "compiler-artifact" && message["target"]["name"] == NAME {
                    message["executable"].as_str().map(PathBuf::from)
                } else {
                    None
                }
            })
            .expect("find the compiled OMP test peer")
    })
}

fn current_example(program: &Path) -> bool {
    let Ok(built) = fs::metadata(program).and_then(|file| file.modified()) else {
        return false;
    };
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(NAME);
    fs::read_dir(source).is_ok_and(|entries| {
        entries.into_iter().all(|entry| {
            entry
                .and_then(|entry| entry.metadata())
                .and_then(|file| file.modified())
                .is_ok_and(|edited| edited <= built)
        })
    })
}
