use std::os::unix::fs::PermissionsExt;

use super::*;

#[test]
fn refuses_wildcards_and_option_names_before_running_any_command() {
    assert!(kill_command(&Session { name: "*".into() }).is_err());
    assert!(
        kill_command(&Session {
            name: "prod*".into()
        })
        .is_err()
    );
    assert!(
        kill_command(&Session {
            name: "--all".into()
        })
        .is_err()
    );
    assert!(kill_command(&Session { name: "".into() }).is_err());
}

#[test]
fn terminates_only_the_literal_selected_session_using_force_when_supported() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("zmx");
    std::fs::write(
        &executable,
        r#"#!/bin/sh
if [ "$1" = help ]; then printf 'kill <name>... [--force]'; exit; fi
printf '%s\n' "${ZMX_SESSION-unset}" "${ZMX_SESSION_PREFIX-unset}" "$@"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = kill_command(&Session {
        name: "工作 'quoted'; $(printf INJECTED)".into(),
    })
    .unwrap();
    let output = command::blocking::Command::new("/bin/sh")
        .env("PATH", directory.path())
        .env("ZMX_SESSION", "outer")
        .env("ZMX_SESSION_PREFIX", "prod.")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "unset\nunset\nkill\n工作 'quoted'; $(printf INJECTED)\n--force\n"
    );
}

#[test]
fn older_zmx_receives_no_unsupported_force_argument() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("zmx");
    std::fs::write(&executable, "#!/bin/sh\nif [ \"$1\" = help ]; then printf 'kill <name>'; exit; fi\nprintf '%s\\n' \"$@\"\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = kill_command(&Session { name: "api".into() }).unwrap();
    let output = command::blocking::Command::new("/bin/sh")
        .env("PATH", directory.path())
        .args(["-c", &script])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "kill\napi\n");
}
