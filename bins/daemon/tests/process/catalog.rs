use std::collections::{BTreeMap, BTreeSet};

use super::{TOKEN, ready, start};

#[tokio::test]
async fn production_installs_every_in_scope_method_without_placeholders() {
    let root = tempfile::tempdir().unwrap();
    let log = root.path().join("server.log");
    let mut server = start(&root.path().join("state"), &log);
    let address = ready(&mut server, &log).await;
    let info: model::server::ServerInfo = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("http://{address}/v1/server/info"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let published = info.capabilities.into_iter().collect::<BTreeSet<_>>();
    let implemented = info
        .implemented_capabilities
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(published.len(), 180);
    assert_eq!(implemented.len(), 179);
    assert!(implemented.is_subset(&published));
    assert!(!published.iter().any(|name| {
        ["hub.", "chat.", "loop.", "plugin."]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    }));
    for retired in [
        "project.open",
        "project.list",
        "project.get",
        "project.close",
    ] {
        assert!(!published.contains(retired));
        assert!(!implemented.contains(retired));
    }
    let catalog = schedule::capabilities::implemented_methods()
        .chain(browser::capabilities::implemented_methods())
        .chain(voice::capabilities::implemented_methods())
        .chain(metadata::capabilities::implemented_methods())
        .chain(filesystem::capabilities::implemented_methods())
        .chain(provider::capabilities::implemented_methods())
        .chain(terminal::capabilities::implemented_methods())
        .map(|spec| (spec.name, spec.kind))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(catalog.len(), 167);
    for method in [
        "server.info",
        "connection.ping",
        "subscription.release.request",
        "session.heartbeat",
        "creation.subscribe.request",
    ] {
        assert!(implemented.contains(method));
        assert!(
            !catalog.contains_key(method),
            "API connection method: {method}"
        );
    }
    assert!(catalog.keys().all(|method| published.contains(*method)));
    let placeholders = catalog
        .into_iter()
        .filter(|(method, _)| !implemented.contains(*method))
        .collect::<Vec<_>>();
    assert_eq!(placeholders.len(), 0);
    assert_eq!(
        published
            .difference(&implemented)
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["connection.single.v1"]
    );
}
