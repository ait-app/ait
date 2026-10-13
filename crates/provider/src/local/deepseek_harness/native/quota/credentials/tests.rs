use super::*;

#[test]
fn yaml_credentials_preserve_quoted_values_and_reject_unknown_versions_or_malformed_input() {
    let raw = SecretString::from(
        "version: 1\nrefs:\n  DEEPSEEK_API_KEY: 'fixture: # key'\nrecords: {}\n",
    );
    assert_eq!(
        stored(&raw, "DEEPSEEK_API_KEY")
            .unwrap()
            .unwrap()
            .expose_secret(),
        "fixture: # key"
    );
    assert!(stored(&raw, "ABSENT").unwrap().is_none());
    for raw in [
        "version: 2\nrefs: {}",
        "version: 1\nrefs: [",
        "version: 1\nrefs:\n  KEY: 42",
        "KEY: secret",
    ] {
        assert!(stored(&raw.into(), "KEY").is_err());
    }
}

#[test]
fn node_dotenv_values_are_not_shell_expanded_and_the_last_duplicate_wins() {
    let raw = SecretString::from(
        "export KEY = first # comment\nKEY=\"last$NOT_EXPANDED\"\nSINGLE='with # hash'\nMULTI=\"one\\ntwo\"\nBACK=`value`\nEMPTY=\n",
    );
    let values = dotenv(&raw);
    assert_eq!(values["KEY"].expose_secret(), "last$NOT_EXPANDED");
    assert_eq!(values["SINGLE"].expose_secret(), "with # hash");
    assert_eq!(values["MULTI"].expose_secret(), "one\ntwo");
    assert_eq!(values["BACK"].expose_secret(), "value");
    assert_eq!(values["EMPTY"].expose_secret(), "");
}

#[test]
fn only_official_https_routes_and_valid_references_can_supply_deepseek_keys() {
    for base in [
        "https://api.deepseek.com",
        "https://api.deepseek.com/anthropic",
        "https://api.deepseek.com:443/v1",
    ] {
        assert!(official(base));
    }
    for base in [
        "http://api.deepseek.com",
        "https://api.deepseek.com.evil.test",
        "https://api.deepseek.com:8443",
        "https://user@api.deepseek.com",
        "https://api.deepseek.com?key=secret",
        "https://api.deepseek.com#fragment",
    ] {
        assert!(!official(base));
    }
    for name in ["DEEPSEEK_API_KEY", "CUSTOM_KEY_2", "_KEY"] {
        assert!(valid_reference(name));
    }
    for name in ["", "2KEY", "KEY\n", "../KEY", "KEY=bad"] {
        assert!(!valid_reference(name));
    }
    let entries = plugins(&SecretString::from(
        "- id: credentials\n  config:\n    path: !!js secretPath()\n",
    ))
    .unwrap();
    assert!(optional_path(&entries[0].config, "path").is_err());
}

#[tokio::test]
async fn launch_environment_is_private_and_dotenv_layers_remain_frozen() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join("home");
    tokio::fs::create_dir(&home).await.unwrap();
    tokio::fs::write(
        directory.path().join(".env"),
        "PRIVATE_DSH_KEY=project\nDEEPSEEK_BASE_URL=https://project.test\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        home.join(".env"),
        "PRIVATE_DSH_KEY=user\nUSER_DSH_KEY=user\n",
    )
    .await
    .unwrap();
    let mut client = DeepSeekHarnessClient::new("unused".into());
    client.environment = BTreeMap::from([
        ("DSH_HOME".into(), home.to_str().unwrap().into()),
        ("PRIVATE_DSH_KEY".into(), "environment".into()),
    ])
    .try_into()
    .unwrap();
    let context = Context::capture(&client, directory.path().to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(
        context.environment_value("PRIVATE_DSH_KEY"),
        Some("environment")
    );
    assert_eq!(context.environment_value("USER_DSH_KEY"), Some("user"));
    tokio::fs::write(directory.path().join(".env"), "PRIVATE_DSH_KEY=changed\n")
        .await
        .unwrap();
    assert_eq!(
        context.project["PRIVATE_DSH_KEY"].expose_secret(),
        "project"
    );
    assert!(!format!("{context:?}").contains("https://project.test"));
    tokio::fs::write(
        home.join("large"),
        vec![b'a'; usize::try_from(MAX_FILE).unwrap() + 1],
    )
    .await
    .unwrap();
    assert!(read(&home.join("large")).await.is_err());
}
