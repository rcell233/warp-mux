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

#[test]
fn quick_download_uses_literal_basename_and_rejects_path_traversal() {
    let directory = Path::new("/Users/test/Downloads");
    assert_eq!(
        download_destination(directory, "report '雪'.txt").unwrap(),
        directory.join("report '雪'.txt")
    );
    assert!(download_destination(directory, "../outside").is_err());
    assert!(download_destination(directory, "/absolute").is_err());
    assert!(download_destination(directory, "..").is_err());
}

#[test]
fn upload_reports_received_bytes_and_replaces_file_after_success() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("report '雪' [*] ; $(bad).bin");
            let target = directory.path().join("remote");
            std::fs::create_dir(&target).unwrap();
            let destination = target.join(source.file_name().unwrap());
            std::fs::write(&source, vec![0xa5; 2 * 1024 * 1024]).unwrap();
            std::fs::write(&destination, "previous").unwrap();
            let mut updates = Vec::new();

            upload(
                None,
                &source,
                target.to_str().unwrap(),
                target.to_str().unwrap(),
                |bytes, total| updates.push((bytes, total)),
            )
            .await
            .unwrap();

            assert_eq!(
                std::fs::read(&destination).unwrap(),
                std::fs::read(source).unwrap()
            );
            assert_eq!(updates.first(), Some(&(0, 2097152)));
            assert!(updates.contains(&(1048576, 2097152)));
            assert_eq!(updates.last(), Some(&(2097152, 2097152)));
            assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
        });
}

#[test]
fn upload_rejects_destination_outside_base_and_directories() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let base = directory.path().join("base");
            std::fs::create_dir(&base).unwrap();
            let source = base.join("data.txt");
            let destination = directory.path().join("data.txt");
            std::fs::write(&source, "new").unwrap();
            std::fs::write(&destination, "keep").unwrap();

            assert!(
                upload(
                    None,
                    &source,
                    base.to_str().unwrap(),
                    directory.path().to_str().unwrap(),
                    |_, _| {}
                )
                .await
                .is_err()
            );
            assert!(
                upload(
                    None,
                    &base,
                    base.to_str().unwrap(),
                    base.to_str().unwrap(),
                    |_, _| {}
                )
                .await
                .is_err()
            );
            assert_eq!(std::fs::read_to_string(destination).unwrap(), "keep");
        });
}

#[test]
fn interrupted_upload_preserves_existing_file_and_removes_staging() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("data.txt");
    std::fs::write(&destination, "keep").unwrap();
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(b"partial").unwrap();
    input.rewind().unwrap();

    let output = command::blocking::Command::new("python3")
        .args([
            "-c",
            include_str!("upload_file.py"),
            directory.path().to_str().unwrap(),
            directory.path().to_str().unwrap(),
            "data.txt",
            "100",
        ])
        .stdin(input)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(destination).unwrap(), "keep");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_downloads_finish_independently_with_byte_progress() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("source");
            let first = directory.path().join("first");
            let second = directory.path().join("second");
            std::fs::write(&source, b"independent transfers").unwrap();
            let mut first_progress = Vec::new();
            let mut second_progress = Vec::new();

            let (a, b) = futures_lite::future::zip(
                download_with_progress(None, source.to_str().unwrap(), &first, |bytes| {
                    first_progress.push(bytes)
                }),
                download_with_progress(None, source.to_str().unwrap(), &second, |bytes| {
                    second_progress.push(bytes)
                }),
            )
            .await;

            a.unwrap();
            b.unwrap();
            assert_eq!(std::fs::read(first).unwrap(), b"independent transfers");
            assert_eq!(std::fs::read(second).unwrap(), b"independent transfers");
            assert_eq!(first_progress.last(), Some(&21));
            assert_eq!(second_progress.last(), Some(&21));
        });
}
