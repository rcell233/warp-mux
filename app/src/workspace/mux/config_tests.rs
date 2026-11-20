use super::*;

#[test]
fn imports_aliases_and_glob_includes_without_host_patterns() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".ssh/hosts")).unwrap();
    fs::write(
        home.path().join(".ssh/config"),
        "Host=prod alias # comment\nHost * !excluded test?\nInclude hosts/*.conf\n",
    )
    .unwrap();
    fs::write(
        home.path().join(".ssh/hosts/dev.conf"),
        "hOsT \"dev\" prod\nHostName ignored.example\n",
    )
    .unwrap();

    assert_eq!(
        import_hosts(home.path()).unwrap(),
        vec!["alias", "dev", "prod"]
    );
}

#[test]
fn reports_include_cycles() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".ssh")).unwrap();
    fs::write(home.path().join(".ssh/config"), "Include config\n").unwrap();

    assert!(
        import_hosts(home.path())
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
}

#[test]
fn does_not_run_or_follow_match_exec_during_import() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".ssh")).unwrap();
    fs::write(
        home.path().join(".ssh/config"),
        "Match exec \"exit 0\"\nInclude missing.conf\nHost dev\n",
    )
    .unwrap();

    assert_eq!(import_hosts(home.path()).unwrap(), vec!["dev"]);
}
