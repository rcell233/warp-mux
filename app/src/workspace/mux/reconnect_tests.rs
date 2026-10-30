use super::*;

#[test]
fn connection_loss_retries_same_command_then_stops_on_clean_exit() {
    let directory = tempfile::tempdir().unwrap();
    let counter = directory.path().join("attempts");
    let command = format!(
        "if [ -f {} ]; then printf 'restored'; exit 0; else touch {}; (exit 255); fi",
        quote(counter.to_str().unwrap()),
        quote(counter.to_str().unwrap())
    );
    let output = command::blocking::Command::new("/bin/sh")
        .args(["-c", &retry_transport(&command)])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Retrying in 1s"));
    assert!(stdout.ends_with("restored"));
}

#[test]
fn remote_errors_do_not_restart_or_create_another_session() {
    let output = command::blocking::Command::new("/bin/sh")
        .args(["-c", &retry_transport("(exit 7)")])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(output.stdout.is_empty());
}

#[test]
fn session_command_preserves_shell_arguments() {
    let session = Session {
        name: "project '雪' ; $(bad)".into(),
    };
    let script = ssh_session_script("prod", &session);
    let command = script.replace("/usr/bin/ssh", "printf '%s\\n'");
    let output = command::blocking::Command::new("/bin/sh")
        .args(["-c", &command])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&remote_command(&attach_command(&session))));
    assert!(stdout.contains("ServerAliveInterval=15"));
}
