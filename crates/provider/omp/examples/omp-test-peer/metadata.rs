use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde_json::{Value, json};

use super::{env_json, env_set, flag, read_json, refuse, write_stdout};

pub(super) fn version() {
    if env_set("OMP_FIXTURE_VERSION_FAILS") {
        refuse("Error: Invalid OMP profile", 1);
    }
    let version = std::env::var("OMP_FIXTURE_VERSION").unwrap_or_else(|_| "18.18.8".to_owned());
    write_stdout(&format!("omp/{version}\n"));
}

pub(super) fn respond(args: &[String], root: &Path) -> bool {
    match args {
        [config, set, key, value, _]
            if config == "config" && set == "set" && key == "modelPresets" =>
        {
            set_presets(root, value);
        }
        [config, subcommand, ..]
            if config == "config" && (subcommand == "list" || subcommand == "get") =>
        {
            read_config(root, args);
        }
        [models, kind, all, format]
            if models == "models" && kind == "--kind" && all == "all" && format == "--json" =>
        {
            models_catalog();
        }
        _ => return false,
    }
    true
}

fn set_presets(root: &Path, contents: &str) {
    if env_set("OMP_FIXTURE_PRESET_WRITE_FAILS") {
        refuse("Error: config write unavailable", 1);
    }
    let value: Value = serde_json::from_str(contents).expect("modelPresets JSON");
    if !env_set("OMP_FIXTURE_PRESET_WRITE_IGNORED") {
        let mut staging = tempfile::NamedTempFile::new_in(root).expect("stage native presets");
        serde_json::to_writer(&mut staging, &value).expect("write native presets");
        staging
            .persist(root.join("native-presets.json"))
            .expect("save native presets");
    }
    print_json(&json!({ "key": "modelPresets", "value": value }));
}

fn read_config(root: &Path, args: &[String]) {
    if env_set("OMP_FIXTURE_METADATA_FAILS") {
        refuse("Error: config unavailable", 1);
    }
    let native_file = root.join("native-presets.json");
    let mut global_presets = if native_file.exists() {
        read_json(&native_file)
    } else {
        env_json("OMP_FIXTURE_PRESETS", json!({}))
    };
    let mut project_presets = env_json("OMP_FIXTURE_PROJECT_PRESETS", json!({}));
    if args[1] == "list"
        && let Some(scope) = flag(args, "--scope")
    {
        let value = if scope == "global" {
            global_presets
        } else {
            project_presets
        };
        print_json(&json!({ "modelPresets": { "value": value } }));
        return;
    }
    global_presets
        .as_object_mut()
        .expect("global presets object")
        .append(
            project_presets
                .as_object_mut()
                .expect("project presets object"),
        );
    if args[1] == "get" {
        print_json(&json!({ "key": args[2], "value": global_presets }));
        return;
    }
    let overrides = env_json("OMP_FIXTURE_CONFIG_OVERRIDES", json!({}));
    let fallback_chains = env_json("OMP_FIXTURE_FALLBACK_CHAINS", Value::Null);
    print_json(&json!({
        "modelRoles": { "value": overrides.get("modelRoles").cloned().unwrap_or_else(||
            json!({ "default": "test/model", "task": "test/model:high" })) },
        "modelTags": { "value": overrides.get("modelTags").cloned().unwrap_or_else(||
            json!({ "default": { "name": "Director" }, "task": { "name": "Worker" } })) },
        "cycleOrder": { "value": overrides.get("cycleOrder").cloned().unwrap_or_else(||
            json!(["default", "task"])) },
        "modelPresets": { "value": global_presets },
        "retry.fallbackChains": { "value": fallback_chains },
    }));
}

fn models_catalog() {
    if env_set("OMP_FIXTURE_METADATA_FAILS") {
        refuse("Error: catalog unavailable", 1);
    }
    print_json(&json!({ "models": [
        { "selector": "test/model", "name": "Fixture model", "kind": "chat",
          "input": ["text", "image"], "thinking": ["low", "high"] },
        { "selector": "other/model", "name": "Other", "kind": "chat",
          "input": ["text"], "thinking": ["low", "high", "max"] },
        { "selector": "test/image", "name": "Fixture image", "kind": "image",
          "input": ["text"], "thinking": [] },
    ] }));
}

pub(super) fn probe_write_confinement(root: &Path) {
    if let Some(outside) =
        std::env::var_os("OMP_FIXTURE_WRITE_PROBE").filter(|path| !path.is_empty())
    {
        let result = fs::write(&outside, "written").and_then(|()| fs::remove_file(outside));
        let outcome = if result.is_ok() { "written" } else { "denied" };
        log(root, &json!({ "writeProbe": outcome }));
    }
    if env_set("OMP_FIXTURE_STATE_PROBE") {
        let outcomes: serde_json::Map<String, Value> = [
            "sessions/probe",
            "blobs/probe",
            "agent.db",
            "config.yml",
            "models.yml",
            "extensions/probe.ts",
        ]
        .into_iter()
        .map(|name| {
            let file = root.join(name);
            let result = fs::create_dir_all(file.parent().expect("probe parent"))
                .and_then(|()| OpenOptions::new().create(true).append(true).open(file))
                .and_then(|mut output| output.write_all(b"probe\n"));
            let outcome = if result.is_ok() { "written" } else { "denied" };
            (name.to_owned(), json!(outcome))
        })
        .collect();
        log(root, &json!({ "stateProbe": outcomes }));
    }
}

pub(super) fn log(root: &Path, value: &Value) {
    let mut line = value.to_string();
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("calls.jsonl"))
        .expect("open OMP call log")
        .write_all(line.as_bytes())
        .expect("record OMP call");
}

fn print_json(value: &Value) {
    write_stdout(&format!("{value}\n"));
}
