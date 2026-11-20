use super::*;

#[test]
#[cfg(unix)]
fn remote_shell_keeps_session_metacharacters_literal() {
    let name = "工作 'quoted' ; $(printf INJECTED)";
    let script = remote_command(&format!("printf '%s' {}", quote(name)));
    let output = command::blocking::Command::new("/bin/sh")
        .args(["-c", &script])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), name);
}

#[test]
#[cfg(unix)]
fn opening_window_does_not_switch_an_inherited_zmx_session() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let fake = directory.path().join("zmx");
    std::fs::write(
        &fake,
        "#!/bin/sh\nprintf '%s|%s' \"${ZMX_SESSION-unset}\" \"$2\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let session = core::Session {
        name: "api window".into(),
    };
    let output = command::blocking::Command::new("/bin/sh")
        .env("PATH", directory.path())
        .env("ZMX_SESSION", "outer")
        .args(["-c", &core::attach_command(&session)])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "unset|api window"
    );
}
