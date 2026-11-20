use super::*;

#[test]
fn searches_path_then_official_installer_and_homebrew_without_ide_aliases() {
    let search = Search {
        path: Some(std::env::join_paths(["/test/path", "/another/bin"]).unwrap()),
        home: Some("/test/home".into()),
        local_app_data: Some("/test/local".into()),
        homebrew_prefix: Some("/test/brew".into()),
    };
    let programs = candidates(&search);
    let executable = if cfg!(windows) { "agy.exe" } else { "agy" };
    assert_eq!(programs[0], Path::new("/test/path").join(executable));
    assert_eq!(
        programs[2],
        Path::new("/test/home/.local/bin").join(executable)
    );
    assert!(programs.contains(&PathBuf::from("/test/local/agy/bin/agy.exe")));
    assert!(programs.contains(&Path::new("/test/brew/bin").join(executable)));
    assert!(
        programs
            .iter()
            .all(|program| program.file_name() != Some(OsStr::new("antigravity")))
    );
    if !cfg!(windows) {
        assert!(programs.contains(&PathBuf::from("/opt/homebrew/bin/agy")));
        assert!(programs.contains(&PathBuf::from("/usr/local/bin/agy")));
        assert!(programs.contains(&PathBuf::from("/home/linuxbrew/.linuxbrew/bin/agy")));
    }
}

/// Listing order of AGY 1.4.7: efforts are separate records, not sorted by strength.
const LISTING: &str = concat!(
    "Fetching available models...\n",
    "gemini-test-high\tGemini Test (High)\n",
    "gemini-test-xhigh\tGemini Test (XHigh)\n",
    "gemini-test-low\tGemini Test (Low)\n",
    "claude-test-high\tClaude Test (High)\n",
    "plain-test\tPlain Test\n",
    "odd-test-low\tOdd Test (High)\n",
);

fn selection(model: Option<&str>, effort: Option<&str>) -> Result<Selection, AgentSessionError> {
    parse_catalog(LISTING).unwrap().select(&StoredAgentConfig {
        model: model.map(str::to_owned),
        thinking_option_id: effort.map(str::to_owned),
        ..StoredAgentConfig::default()
    })
}

#[test]
fn groups_native_effort_variants_into_one_model_with_thinking_options() {
    let catalog = parse_catalog(LISTING).unwrap();
    let models = catalog.models();
    assert_eq!(catalog.len(), 4);
    assert_eq!(
        models[0],
        json!({"provider":"antigravity","id":"gemini-test","label":"Gemini Test",
            "isSelectable":true,"defaultThinkingOptionId":"high","thinkingOptions":[
                {"id":"low","label":"Low"},{"id":"high","label":"High"},
                {"id":"xhigh","label":"XHigh"}]})
    );
    assert_eq!(models[1]["id"], "claude-test");
    assert_eq!(
        models[1]["thinkingOptions"],
        json!([{"id":"high","label":"High"}])
    );
    assert!(models.iter().all(|model| model.get("isDefault").is_none()));
}

#[test]
fn keeps_records_without_a_matching_effort_suffix_whole() {
    let models = parse_catalog(LISTING).unwrap().models();
    for (model, id, label) in [
        (&models[2], "plain-test", "Plain Test"),
        (&models[3], "odd-test-low", "Odd Test (High)"),
    ] {
        assert_eq!(model["id"], id);
        assert_eq!(model["label"], label);
        assert_eq!(model["thinkingOptions"], json!([]));
        assert!(model.get("defaultThinkingOptionId").is_none());
    }
}

#[test]
fn selects_model_and_effort_as_separate_native_arguments() {
    for (model, effort, expected) in [
        (None, None, (None, None)),
        (None, Some("max"), (None, Some("max"))),
        (Some("gemini-test"), None, (Some("gemini-test"), None)),
        (
            Some("gemini-test"),
            Some("xhigh"),
            (Some("gemini-test"), Some("xhigh")),
        ),
        (Some("plain-test"), None, (Some("plain-test"), None)),
        (Some("odd-test-low"), None, (Some("odd-test-low"), None)),
    ] {
        let expected = Selection {
            model: expected.0.map(str::to_owned),
            effort: expected.1.map(str::to_owned),
        };
        assert_eq!(
            selection(model, effort),
            Ok(expected),
            "{model:?} {effort:?}"
        );
    }
}

#[test]
fn resolves_variant_slugs_saved_before_grouping() {
    let resolved = |effort: &str| {
        Ok(Selection {
            model: Some("gemini-test".to_owned()),
            effort: Some(effort.to_owned()),
        })
    };
    assert_eq!(selection(Some("gemini-test-low"), None), resolved("low"));
    assert_eq!(
        selection(Some("gemini-test-low"), Some("high")),
        resolved("high")
    );
}

#[test]
fn rejects_models_and_efforts_the_cli_does_not_offer() {
    for (model, effort) in [
        ("missing-test", None),
        ("gemini-test", Some("medium")),
        ("gemini-test-medium", None),
        ("gemini-test-low", Some("max")),
        ("plain-test", Some("low")),
        ("plain-test-low", None),
    ] {
        assert_eq!(
            selection(Some(model), effort),
            Err(AgentSessionError::Rejected),
            "{model} {effort:?}"
        );
    }
}

#[test]
fn rejects_empty_duplicate_and_malformed_model_records() {
    for output in [
        "notice",
        "\tlabel",
        "id\t",
        "id\tlabel\nid\tlabel",
        "id\tbad\tlabel",
    ] {
        assert!(parse_catalog(output).is_err(), "{output}");
    }
}

#[cfg(unix)]
#[test]
fn resolves_official_and_homebrew_installs_when_path_omits_both() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    let brew = directory.path().join("brew");
    let official = home.join(".local/bin/agy");
    let homebrew = brew.join("bin/agy");
    for program in [&official, &homebrew] {
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(program, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(program, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut search = Search {
        path: None,
        home: Some(home),
        local_app_data: None,
        homebrew_prefix: Some(brew),
    };
    assert_eq!(resolve(&search), official);
    std::fs::remove_file(&official).unwrap();
    assert_eq!(resolve(&search), homebrew);
    let chosen = directory.path().join("chosen");
    std::fs::create_dir_all(&chosen).unwrap();
    std::fs::copy(&homebrew, chosen.join("agy")).unwrap();
    search.path = Some(std::env::join_paths([chosen.clone()]).unwrap());
    assert_eq!(resolve(&search), chosen.join("agy"));
}
