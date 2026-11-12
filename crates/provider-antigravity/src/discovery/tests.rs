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

#[test]
fn parses_actual_tab_separated_models_without_guessing_defaults() {
    let models = parse_models(concat!(
        "Fetching available models...\n",
        "gemini-test-low\tGemini Test (Low)\n",
        "claude-test-high\tClaude Test (High)\n",
    ))
    .unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["id"], "gemini-test-low");
    assert_eq!(models[1]["label"], "Claude Test (High)");
    assert!(models.iter().all(|model| model.get("isDefault").is_none()));
    assert!(
        models
            .iter()
            .all(|model| model["thinkingOptions"] == json!([]))
    );
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
        assert!(parse_models(output).is_err(), "{output}");
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
