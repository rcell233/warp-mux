use super::*;

fn run<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn image_path_paste_preserves_spaces_quotes_and_shell_metacharacters() {
    let path = "/home/雪 user/cache/a'b $(touch bad).png";
    let pasted = paste_path(path).unwrap();

    assert_eq!(shlex::split(&pasted).unwrap(), vec![path]);
    assert!(!pasted.contains('\n'));
}

#[test]
fn image_path_cannot_inject_terminal_controls() {
    assert!(paste_path("/cache/image\x1b[201~\n.png").is_err());
    assert!(paste_path("relative.png").is_err());
}

#[test]
fn empty_clipboard_is_rejected_before_connecting() {
    let error = run(upload_image(
        "unused",
        ImageSource::Clipboard(Vec::new()),
        |_, _| {},
    ))
    .unwrap_err();
    assert!(error.to_string().contains("between 1 byte and 32 MiB"));
}

#[test]
fn disguised_non_image_file_is_rejected_before_connecting() {
    let mut file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
    file.write_all(b"this is not a PNG").unwrap();

    let error = run(upload_image(
        "unused",
        ImageSource::File(file.path().into()),
        |_, _| {},
    ))
    .unwrap_err();

    assert!(error.to_string().contains("Use a PNG"));
}

#[test]
fn oversized_image_file_is_rejected_before_reading() {
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(33 * 1024 * 1024).unwrap();

    let error = run(upload_image(
        "unused",
        ImageSource::File(file.path().into()),
        |_, _| {},
    ))
    .unwrap_err();

    assert!(error.to_string().contains("no larger than 32 MiB"));
}

#[test]
fn directories_are_not_uploaded_as_images() {
    let directory = tempfile::tempdir().unwrap();

    let error = run(upload_image(
        "unused",
        ImageSource::File(directory.path().into()),
        |_, _| {},
    ))
    .unwrap_err();

    assert!(error.to_string().contains("Choose an image file"));
}
