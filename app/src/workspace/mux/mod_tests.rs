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
