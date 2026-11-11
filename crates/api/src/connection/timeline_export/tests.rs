use std::io::{Read, Seek, Write};
use std::sync::Arc;

use filesystem::files::local::LocalFiles;
use filesystem::files::service::files::Files;
use model::download::GeneratedArtifact;

use super::*;
use crate::{Api, Services};

fn api(root: &std::path::Path) -> Api {
    let mut api = Api::new(
        "127.0.0.1:19766".parse().unwrap(),
        "export-test".to_owned(),
        "instance".to_owned(),
        crate::test_support::TOKEN.into(),
        Services::default(),
    )
    .unwrap();
    let state = Arc::get_mut(&mut api.shared).unwrap();
    Arc::get_mut(&mut state.filesystem).unwrap().files = Some(crate::shared_service(Files::new(
        Box::new(LocalFiles::new(root.to_owned(), root)),
    )));
    api
}

fn export() -> ExportedTimeline {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"# Saved\n").unwrap();
    file.rewind().unwrap();
    ExportedTimeline {
        artifact: GeneratedArtifact {
            agent_id: "agent".to_owned(),
            file_name: "timeline.md".to_owned(),
            mime_type: "text/markdown".to_owned(),
            size: 8,
            file,
        },
        format: Format::Markdown,
        scope: Scope::Timeline,
        entries: 1,
    }
}

#[tokio::test]
async fn registered_export_is_a_real_single_use_existing_download_and_cancel_is_scoped() {
    let root = tempfile::tempdir().unwrap();
    let api = api(root.path());
    let receipt = register(&api.shared, export()).await.unwrap();
    assert_eq!(receipt["source"], "persisted_timeline");
    assert_eq!(receipt["size"], 8);
    let token = receipt["token"].as_str().unwrap();
    let files = api.shared.filesystem.files.as_ref().unwrap();
    assert!(files.lock().unwrap().bind_download(token, "visitor"));
    assert!(files.lock().unwrap().consume_download(token, None).is_err());
    let (mut reader, name) = files
        .lock()
        .unwrap()
        .consume_download(token, Some("visitor"))
        .unwrap();
    let mut bytes = String::new();
    reader.read_to_string(&mut bytes).unwrap();
    assert_eq!(bytes, "# Saved\n");
    assert_eq!(name, "timeline.md");
    assert!(
        files
            .lock()
            .unwrap()
            .consume_download(token, Some("visitor"))
            .is_err()
    );
    let receipt = register(&api.shared, export()).await.unwrap();
    let token = receipt["token"].as_str().unwrap().to_owned();
    assert_eq!(
        release(&api.shared, "other".to_owned(), token.clone())
            .await
            .unwrap()["released"],
        false
    );
    assert_eq!(
        release(&api.shared, "agent".to_owned(), token.clone())
            .await
            .unwrap()["released"],
        true
    );
    assert_eq!(
        release(&api.shared, "agent".to_owned(), token)
            .await
            .unwrap()["released"],
        false
    );
    api.shared.cancellation.cancel();
    api.shared.tasks.close();
    api.shared.tasks.wait().await;
}

#[tokio::test(start_paused = true)]
async fn idle_expiry_removes_artifact_without_a_later_request_or_provider_reexecution() {
    let root = tempfile::tempdir().unwrap();
    let api = api(root.path());
    let receipt = register(&api.shared, export()).await.unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(61)).await;
    api.shared.tasks.close();
    api.shared.tasks.wait().await;
    let token = receipt["token"].as_str().unwrap();
    assert!(
        !api.shared
            .filesystem
            .files
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .release_artifact("agent", token)
    );
    assert!(api.shared.provider.agent_execution.is_none());
}

#[tokio::test]
async fn native_html_receipt_preserves_bytes_and_reuses_the_single_use_grant() {
    let root = tempfile::tempdir().unwrap();
    let api = api(root.path());
    let mut file = tempfile::tempfile().unwrap();
    let native = "<!DOCTYPE html><html><script>var v=1;</script>NATIVE_BYTES</html>";
    file.write_all(native.as_bytes()).unwrap();
    file.rewind().unwrap();
    let export = provider::ports::native_html_export::ExportedNativeHtml {
        artifact: GeneratedArtifact {
            agent_id: "agent".to_owned(),
            file_name: "ait-native-test.html".to_owned(),
            mime_type: "text/html; charset=utf-8".to_owned(),
            size: native.len() as u64,
            file,
        },
    };
    let receipt = register_native(&api.shared, export).await.unwrap();
    assert_eq!(receipt["source"], "omp_native");
    assert_eq!(receipt["size"], native.len() as u64);
    assert_eq!(receipt["fileName"], "ait-native-test.html");
    let token = receipt["token"].as_str().unwrap();
    let files = api.shared.filesystem.files.as_ref().unwrap();
    assert!(files.lock().unwrap().bind_download(token, "visitor"));
    let (mut reader, name) = files
        .lock()
        .unwrap()
        .consume_download(token, Some("visitor"))
        .unwrap();
    let mut bytes = String::new();
    reader.read_to_string(&mut bytes).unwrap();
    // Native bytes are preserved verbatim, including active scripts.
    assert_eq!(bytes, native);
    assert_eq!(name, "ait-native-test.html");
    assert!(
        files
            .lock()
            .unwrap()
            .consume_download(token, Some("visitor"))
            .is_err()
    );
    api.shared.cancellation.cancel();
    api.shared.tasks.close();
    api.shared.tasks.wait().await;
}
