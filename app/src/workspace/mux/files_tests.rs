use super::*;

#[test]
fn listing_resolves_base_and_confines_navigation() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let canonical = temp.path().canonicalize().unwrap();
            let base = canonical.to_str().unwrap();
            std::fs::create_dir(temp.path().join("folder")).unwrap();
            std::fs::write(temp.path().join("quote' [*].txt"), "hello").unwrap();
            let listing = list(None, base, "").await.unwrap();
            assert!(listing.parent.is_none());
            assert!(listing.entries[0].directory);
            assert_eq!(listing.entries[1].name, "quote' [*].txt");
            let folder = temp.path().join("folder");
            assert_eq!(
                list(None, base, folder.to_str().unwrap())
                    .await
                    .unwrap()
                    .parent
                    .as_deref(),
                Some(base)
            );
            assert!(list(None, base, "/").await.is_err());
        });
}

#[test]
fn sftp_paths_cannot_add_commands_or_expand_globs() {
    assert_eq!(
        sftp_path("/a b/[x]*?\"\\").unwrap(),
        "\"/a b/\\[x\\]\\*\\?\\\"\\\\\""
    );
    assert!(sftp_path("/a\n!touch /tmp/bad").is_err());
}

#[test]
fn failed_download_preserves_existing_destination() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let destination = temp.path().join("saved");
            std::fs::write(&destination, "original").unwrap();
            assert!(
                download(None, "/missing-warp-mux-file", &destination)
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read_to_string(destination).unwrap(), "original");
        });
}

#[test]
fn completed_download_replaces_destination_with_exact_bytes() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("a 'quoted' [*].bin");
            let destination = directory.path().join("download.bin");
            let bytes = [0, 255, 10, 13, 128];
            std::fs::write(&source, bytes).unwrap();
            std::fs::write(&destination, "previous").unwrap();
            download(None, source.to_str().unwrap(), &destination)
                .await
                .unwrap();
            assert_eq!(std::fs::read(destination).unwrap(), bytes);
        });
}
