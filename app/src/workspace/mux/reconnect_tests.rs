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
    let output = run_in_terminal(&retry_transport(&command));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Retrying in 0s"));
    assert!(stdout.ends_with("restored"));
}

#[test]
fn retries_immediately_then_every_two_seconds_without_backoff() {
    let directory = tempfile::tempdir().unwrap();
    let counter = quote(directory.path().join("attempts").to_str().unwrap());
    let delays = directory.path().join("delays");
    let command = format!(
        "(attempt=$(cat {counter} 2>/dev/null || printf 0); attempt=$((attempt + 1)); \
         printf '%s' \"$attempt\" > {counter}; [ \"$attempt\" -ge 4 ] || exit 255; printf restored)"
    );
    let script = format!(
        "sleep() {{ printf '%s\\n' \"$1\" >> {}; }}; {}",
        quote(delays.to_str().unwrap()),
        retry_transport(&command)
    );

    let output = run_in_terminal(&script);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(std::fs::read_to_string(delays).unwrap(), "0\n2\n2\n");
    assert!(output.stdout.ends_with(b"restored"));
}

#[test]
fn remote_errors_do_not_restart_or_create_another_session() {
    let output = run_in_terminal(&retry_transport("(exit 7)"));
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
    let output = run_in_terminal(&command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&remote_command(&attach_command(&session))));
    assert!(stdout.contains("ServerAliveInterval=15"));
}

fn run_in_terminal(script: &str) -> std::process::Output {
    let command = format!(
        "command /bin/sh -c {}",
        warp_util::path::ShellFamily::Posix.shell_escape(script)
    );
    command::blocking::Command::new("/bin/sh")
        .args(["-c", &command])
        .output()
        .unwrap()
}
