use super::*;

#[test]
fn explicit_executable_wins_without_reading_environment() {
    let cli = Cli::try_parse_from(["ait-omp-acp", "--omp-bin", "/installed OMP/omp"]).unwrap();
    assert_eq!(
        cli.program(|| panic!("explicit flag must bypass environment")),
        PathBuf::from("/installed OMP/omp")
    );
}

#[test]
fn environment_precedes_path_lookup() {
    let cli = Cli::try_parse_from(["ait-omp-acp"]).unwrap();
    assert_eq!(
        cli.program(|| Some("/environment OMP/omp".into())),
        PathBuf::from("/environment OMP/omp")
    );
    let cli = Cli::try_parse_from(["ait-omp-acp"]).unwrap();
    assert_eq!(cli.program(|| None), PathBuf::from("omp"));
}

#[test]
fn rejects_unknown_flags_and_missing_executable() {
    for arguments in [
        vec!["ait-omp-acp", "--unknown"],
        vec!["ait-omp-acp", "--omp-bin"],
        vec!["ait-omp-acp", "omp-acp"],
    ] {
        assert!(Cli::try_parse_from(arguments).is_err());
    }
}

#[test]
fn version_uses_shared_build_identity() {
    let error = Cli::try_parse_from(["ait-omp-acp", "--version"]).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayVersion);
    assert_eq!(
        error.to_string(),
        format!("ait-omp-acp {}\n", model::BUILD_VERSION)
    );
}

#[test]
fn help_documents_installed_runtime_selection() {
    let error = Cli::try_parse_from(["ait-omp-acp", "--help"]).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    let help = error.to_string();
    assert!(help.contains("--omp-bin"));
    assert!(help.contains("AIT_SERVER_OMP_BIN"));
    assert!(help.contains("PATH"));
}

#[cfg(unix)]
#[test]
fn executable_paths_preserve_non_utf8_bytes() {
    use std::os::unix::ffi::OsStringExt;

    let program = OsString::from_vec(b"/installed OMP/omp-\xff".to_vec());
    let cli = Cli::try_parse_from([
        OsString::from("ait-omp-acp"),
        OsString::from("--omp-bin"),
        program.clone(),
    ])
    .unwrap();
    assert_eq!(cli.program(|| None), PathBuf::from(program.clone()));
    let cli = Cli::try_parse_from(["ait-omp-acp"]).unwrap();
    assert_eq!(
        cli.program(|| Some(program.clone())),
        PathBuf::from(program)
    );
}
