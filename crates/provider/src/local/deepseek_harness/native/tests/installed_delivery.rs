//! Real CLI checks compose an ordinary test plugin; no model or personal profile is used.

use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, path::Path};

use domain::agent_runtime::StoredAgentConfig;
use serde_json::json;

use super::fixture::next;
use crate::{
    local::deepseek_harness::DeepSeekHarnessClient,
    ports::agent_session::{AgentClient, AgentSessionSpec, AgentTurnEvent},
    protocol::prompt::{AgentPrompt, PromptImage},
};

const PLUGIN: &str = r"
export const name = 'ait-delivery-check';
export const inject = ['commands', 'attachments'];
export function apply(ctx) {
  ctx.effect(() => ctx.commands.register({
    name: 'ait_attachment_check',
    description: 'Check native durable attachment admission without inference',
    input: {hint: '<attachments>', attachments: true},
    async handler(invocation) {
      const blocks = invocation.attachments;
      if(blocks.length !== 2 || blocks[0].type !== 'image' || blocks[1].type !== 'file')
        return {kind: 'error', text: 'Expected native image and file blocks'};
      const image = await ctx.attachments.readImage(blocks[0].attachment);
      if(image.data.length === 0) return {kind: 'error', text: 'Missing image bytes'};
      const chunks = [];
      for await(const chunk of ctx.attachments.readFileStream(blocks[1].attachment)) chunks.push(chunk);
      if(Buffer.concat(chunks).toString('utf8') !== 'native receipt\n')
        return {kind: 'error', text: 'File bytes changed'};
      return {kind: 'success', text: 'DSH_NATIVE_ATTACHMENT_OK'};
    }
  }), 'test native attachment checker');
}
";

fn quoted(path: &Path) -> String {
    format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}

#[tokio::test]
#[ignore = "requires AIT_TEST_DSH_BIN and unzip; isolated official CLI/plugin, no inference"]
async fn installed_host_admits_native_image_and_file_receipts_and_delivers_real_zip() {
    let binary = std::env::var_os("AIT_TEST_DSH_BIN").expect("set AIT_TEST_DSH_BIN");
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path();
    let plugin = cwd.join("attachment-check.mjs");
    std::fs::write(&plugin, PLUGIN).unwrap();
    let plugin_config = cwd.join("test.patch.json");
    std::fs::write(
        &plugin_config,
        json!([{"insert":[{"id":"ait-delivery-check","name":plugin}]}]).to_string(),
    )
    .unwrap();
    let wrapper = cwd.join("dsh-test");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexec {} --patch {} \"$@\"\n",
            quoted(Path::new(&binary)),
            quoted(&plugin_config)
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut client =
        DeepSeekHarnessClient::new(wrapper).with_image_directory(cwd.join("artifacts"));
    client.environment =
        BTreeMap::from([("DSH_HOME".into(), cwd.join("home").to_str().unwrap().into())])
            .try_into()
            .unwrap();
    let spec = AgentSessionSpec {
        confinement: None,
        provider: "deepseek-harness".into(),
        cwd: cwd.to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
    };
    let mut session = client.create_session(&spec).await.unwrap();
    assert!(
        session
            .commands()
            .unwrap()
            .iter()
            .any(|command| command["name"] == "ait_attachment_check")
    );
    let path = cwd.join("reference.txt");
    std::fs::write(&path, b"native receipt\n").unwrap();
    let prompt = AgentPrompt {text:"/ait_attachment_check".into(),
        images:vec![PromptImage {mime_type:"image/png".into(),data:"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC".into()}],
        attachments:vec![json!({"type":"uploaded_file","id":"upload_native","fileName":"reference.txt",
            "path":path,"mimeType":"text/plain","size":15})],..AgentPrompt::default()};
    session.out_of_band(&prompt).await.unwrap();
    session.take_command_completion().unwrap().await.unwrap();
    loop {
        if let AgentTurnEvent::Timeline(item) = next(session.as_mut()).await
            && item.item["type"] == "provider_command"
            && item.item["command"] == "ait_attachment_check"
        {
            assert_eq!(item.item["status"], "success");
            assert_eq!(item.item["text"], "DSH_NATIVE_ATTACHMENT_OK");
            break;
        }
    }
    session
        .out_of_band(&AgentPrompt::text("/export"))
        .await
        .unwrap();
    session.take_command_completion().unwrap().await.unwrap();
    let archive = loop {
        if let AgentTurnEvent::Timeline(item) = next(session.as_mut()).await
            && item.item.get("file").is_some()
        {
            break item;
        }
    };
    let descriptor = &archive.item["file"];
    let path = Path::new(descriptor["directory"].as_str().unwrap())
        .join(descriptor["fileName"].as_str().unwrap());
    let unzipped = std::process::Command::new("unzip")
        .arg("-p")
        .arg(path)
        .output()
        .unwrap();
    assert!(unzipped.status.success());
    assert!(
        String::from_utf8(unzipped.stdout)
            .unwrap()
            .contains("DSH_NATIVE_ATTACHMENT_OK")
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    assert!(
        history
            .iter()
            .any(|entry| entry.key == archive.key && entry.item["file"] == archive.item["file"])
    );
}
