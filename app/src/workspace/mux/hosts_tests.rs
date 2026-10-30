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
