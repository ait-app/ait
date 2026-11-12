use std::ffi::OsString;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

pub(super) struct NativeFixture {
    pub root: tempfile::TempDir,
    pub cwd: PathBuf,
    pub path: OsString,
}

impl NativeFixture {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let native = root.path().join("codex");
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/provider/codex/tests/fixtures/codex_app_server.py");
        symlink(source, &native).unwrap();
        let cwd = root.path().join("work");
        std::fs::create_dir(&cwd).unwrap();
        let mut paths = vec![root.path().to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let path = std::env::join_paths(paths).unwrap();
        Self { root, cwd, path }
    }
}

/// Native `thread/start` and `thread/resume` requests in `cwd` whose injected Ait MCP server
/// identifies Agent `id`, in the order the offline Codex received them.
pub(super) fn launches(cwd: &std::path::Path, id: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(cwd.join("native-requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|request| {
            request["params"]["config"]["mcp_servers"]["ait"]["http_headers"]["X-Ait-Agent-Id"]
                == id
        })
        .collect()
}
