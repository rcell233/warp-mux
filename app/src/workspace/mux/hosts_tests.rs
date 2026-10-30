use super::*;
#[test]
fn added_hosts_persist_separately_from_config_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hosts.json");
    let mut hosts = SavedHosts::load(&path).unwrap();
    hosts.add(vec!["prod".into(), "prod".into(), "*.example".into()]);
    hosts.save(&path).unwrap();
    assert_eq!(SavedHosts::load(&path).unwrap().aliases, vec!["prod"]);
}
#[test]
fn removed_hosts_stay_removed_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hosts.json");
    let mut hosts = SavedHosts {
        aliases: vec!["dev".into(), "prod".into()],
        ..Default::default()
    };
    hosts.aliases.retain(|name| name != "dev");
    hosts.save(&path).unwrap();
    assert_eq!(SavedHosts::load(&path).unwrap().aliases, vec!["prod"]);
}
#[test]
fn malformed_registry_is_reported_instead_of_silently_reset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hosts.json");
    fs::write(&path, "{broken").unwrap();
    assert!(SavedHosts::load(&path).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "{broken");
}

#[test]
fn base_paths_migrate_and_survive_saved_host_changes() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hosts.json");
    fs::write(&path, r#"{"aliases":["prod"]}"#).unwrap();
    let mut saved = SavedHosts::load(&path).unwrap();
    assert!(saved.base_paths.is_empty());
    saved
        .base_paths
        .insert("prod".into(), "/srv/projects".into());
    saved.save(&path).unwrap();
    let mut reloaded = SavedHosts::load(&path).unwrap();
    reloaded.add(vec!["dev".into()]);
    reloaded.save(&path).unwrap();
    assert_eq!(
        SavedHosts::load(&path).unwrap().base_paths["prod"],
        "/srv/projects"
    );
}

#[test]
fn browser_location_survives_restart_and_is_isolated_by_host() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("hosts.json");
    let mut saved = SavedHosts::default();
    saved
        .last_paths
        .insert("prod".into(), "/srv/projects/api".into());
    saved
        .last_paths
        .insert("dev".into(), "/home/dev/code".into());
    saved.save(&path).unwrap();
    let restored = SavedHosts::load(&path).unwrap();
    assert_eq!(restored.last_paths["prod"], "/srv/projects/api");
    assert_eq!(restored.last_paths["dev"], "/home/dev/code");
}
