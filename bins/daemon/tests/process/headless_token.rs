use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;

#[tokio::test]
async fn headless_process_restarts_with_saved_local_token_without_environment() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("state");
    std::fs::create_dir_all(data.join("device")).unwrap();
    std::fs::set_permissions(data.join("device"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let id = uuid::Uuid::new_v4();
    std::fs::write(data.join("server-id"), id.to_string()).unwrap();
    // An unbound login snapshot keeps this process test entirely offline.
    let state = json!({"version":1,"machine":{"server_id":id,"display_name":"test",
        "platform":"linux","app_version":"test"},"credential":null,"pending":null});
    let state_path = data.join("device/.env");
    std::fs::write(
        &state_path,
        format!(
            "AIT_DEVICE_STATE={}\n",
            STANDARD.encode(serde_json::to_vec(&state).unwrap())
        ),
    )
    .unwrap();
    std::fs::set_permissions(state_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut first_token = None;
    for iteration in 0..2 {
        let log_path = root.path().join(format!("run-{iteration}.log"));
        let child = Command::new(env!("CARGO_BIN_EXE_daemon"))
            .args([
                "--data-dir",
                data.to_str().unwrap(),
                "--listen",
                "127.0.0.1:0",
                "--log-level",
                "info",
                "run",
                "--headless",
            ])
            .env_remove("AIT_SERVER_TOKEN")
            .env("HOME", root.path())
            .env("AIT_SPEECH_PROVIDER", "disabled")
            .env_remove("AIT_SPEECH_STT_PROVIDER")
            .env_remove("AIT_SPEECH_TTS_PROVIDER")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log_path).unwrap())
            .spawn()
            .unwrap();
        let mut process = Process(child);
        let deadline = Instant::now() + Duration::from_secs(30);
        let address = loop {
            let log = std::fs::read_to_string(&log_path).unwrap();
            if let Some(line) = log.lines().find(|line| line.contains("daemon ready")) {
                break line
                    .split("listen=")
                    .nth(1)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .to_owned();
            }
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "daemon exited: {log}"
            );
            assert!(Instant::now() < deadline, "daemon startup timed out: {log}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let token = std::fs::read_to_string(data.join("local-api.token")).unwrap();
        if let Some(saved) = &first_token {
            assert_eq!(saved, &token);
        } else {
            first_token = Some(token.clone());
        }
        let response = client
            .get(format!("http://{address}/v1/server/info"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert!(!std::fs::read_to_string(&log_path).unwrap().contains(&token));
        drop(process);
    }
}

struct Process(std::process::Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
