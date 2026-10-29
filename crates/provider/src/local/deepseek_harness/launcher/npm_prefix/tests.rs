use std::collections::BTreeMap;
use std::path::Path;

use super::super::{resolve, tests::file};
use super::*;
use crate::ports::agent_session::AgentClient;

fn environment(home: &Path) -> BTreeMap<&'static str, OsString> {
    BTreeMap::from([("HOME", home.as_os_str().to_os_string())])
}

#[test]
fn conventional_prefix_is_found_without_npm_configuration() {
    let root = tempfile::tempdir().unwrap();
    let values = environment(root.path());

    assert_eq!(
        bins(|key| values.get(key).cloned()),
        vec![root.path().join(".npm-global/bin")]
    );
    assert_eq!(bins(|_| None), [] as [PathBuf; 0]);
    assert_eq!(bins(|_| Some("relative".into())), [] as [PathBuf; 0]);
}

#[test]
fn environment_prefix_overrides_user_configuration() {
    let root = tempfile::tempdir().unwrap();
    file(
        &root.path().join(".npmrc"),
        "prefix=${HOME}/configured\n",
        false,
    );
    let mut values = environment(root.path());
    values.insert(
        "NPM_CONFIG_PREFIX",
        root.path().join("uppercase").into_os_string(),
    );
    values.insert(
        "npm_config_prefix",
        root.path().join("lowercase").into_os_string(),
    );
    for (expected, removed) in [
        ("lowercase", "npm_config_prefix"),
        ("uppercase", "NPM_CONFIG_PREFIX"),
        ("configured", "HOME"),
    ] {
        assert_eq!(
            bins(|key| values.get(key).cloned()),
            vec![
                root.path().join(expected).join("bin"),
                root.path().join(".npm-global/bin"),
            ]
        );
        values.remove(removed);
    }
}

#[test]
fn userconfig_override_and_last_root_prefix_are_respected() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("custom npmrc");
    file(&root.path().join(".npmrc"), "prefix=/ignored", false);
    file(
        &config,
        "# prefix=/ignored\nprefix=/old\nprefix='${HOME}/custom prefix'\n[other]\nprefix=/wrong-section\n",
        false,
    );
    let mut values = environment(root.path());
    values.insert("NPM_CONFIG_USERCONFIG", config.into_os_string());

    assert_eq!(
        bins(|key| values.get(key).cloned()),
        vec![
            root.path().join("custom prefix/bin"),
            root.path().join(".npm-global/bin"),
        ]
    );
    values.insert(
        "npm_config_userconfig",
        root.path().join(".npmrc").into_os_string(),
    );
    assert_eq!(
        bins(|key| values.get(key).cloned())[0],
        Path::new("/ignored/bin")
    );
}

#[test]
fn invalid_or_oversized_configuration_is_ignored() {
    let root = tempfile::tempdir().unwrap();
    let values = environment(root.path());
    let config = root.path().join(".npmrc");
    for contents in [
        "prefix=relative\n",
        "prefix=${MISSING}/bin\n",
        "prefix=\"unterminated\n",
        "[section]\nprefix=/ignored\n",
        &format!("prefix=/oversized\n{}", " ".repeat(64 * 1024)),
    ] {
        file(&config, contents, false);
        assert_eq!(
            bins(|key| values.get(key).cloned()),
            vec![root.path().join(".npm-global/bin")]
        );
    }
    std::fs::write(config, [0xff]).unwrap();
    assert_eq!(
        bins(|key| values.get(key).cloned()),
        vec![root.path().join(".npm-global/bin")]
    );
}

#[test]
fn ini_path_values_preserve_quotes_and_escaped_punctuation() {
    for (input, expected) in [
        (" /plain/prefix ", Some("/plain/prefix")),
        ("\"/quoted prefix\"", Some("/quoted prefix")),
        ("'/single quoted'", Some("/single quoted")),
        ("/prefix # comment", Some("/prefix")),
        ("/prefix ; comment", Some("/prefix")),
        (r"/hash\#semi\;slash\\", Some("/hash#semi;slash\\")),
        (r"/literal\x", Some(r"/literal\x")),
        ("/trailing\\", Some("/trailing\\")),
        ("'unterminated", None),
    ] {
        assert_eq!(decode(input).as_deref(), expected);
    }
}

#[test]
fn home_and_environment_expansion_requires_absolute_paths() {
    let values = BTreeMap::from([("HOME", OsString::from("/home/fixture"))]);
    for (value, expected) in [
        ("~/.npm-global", Some("/home/fixture/.npm-global")),
        ("${HOME}/.npm-global", Some("/home/fixture/.npm-global")),
        ("${MISSING?}/absolute", Some("/absolute")),
        ("${MISSING}/prefix", None),
        ("${HOME/prefix", None),
        ("relative/prefix", None),
    ] {
        assert_eq!(
            expand(value, &|key| values.get(key).cloned()),
            expected.map(PathBuf::from)
        );
    }
    assert_eq!(expand("~/prefix", &|_| None), None);
    assert_eq!(expand("~/prefix", &|_| Some(OsString::new())), None);
    assert_eq!(expand("~/prefix", &|_| Some("relative-home".into())), None);
}

#[tokio::test]
async fn npm_global_cli_runs_when_prefix_is_absent_from_path() {
    let root = tempfile::tempdir().unwrap();
    let values = environment(root.path());
    let program = root.path().join(".npm-global/bin/dsh");
    file(
        &program,
        "#!/bin/sh\nprintf 'global-cli-fixture\\n'\n",
        true,
    );
    file(
        &root.path().join(".npmrc"),
        "prefix=${HOME}/.npm-global\n",
        false,
    );
    let paths = bins(|key| values.get(key).cloned());

    let client = resolve(&paths, &[], None);

    assert_eq!(client.program, program);
    assert!(client.is_available().await.unwrap());
    let output = client.command().arg("--version").output().await.unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"global-cli-fixture\n");
    let explicit = root.path().join("path/bin/dsh");
    file(&explicit, "#!/bin/sh\nexit 0\n", true);
    let paths: Vec<_> = std::iter::once(explicit.parent().unwrap().to_path_buf())
        .chain(paths)
        .collect();
    assert_eq!(resolve(&paths, &[], None).program, explicit);
}
