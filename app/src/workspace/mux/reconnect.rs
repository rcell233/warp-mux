use std::path::Path;

use super::core::{Session, attach_command, quote};
use super::remote_command;

pub fn ssh_session_script(host: &str, session: &Session, lease: &Path) -> String {
    let command = format!(
        "/usr/bin/ssh -tt -o RemoteCommand=none -o ConnectTimeout=8 -o ConnectionAttempts=1 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o TCPKeepAlive=yes -o BatchMode=\"$batch\" -- {} {}",
        quote(host),
        quote(&remote_command(&attach_command(session)))
    );
    retry_transport(&command, Some(lease))
}

fn retry_transport(command: &str, lease: Option<&Path>) -> String {
    let check = lease.map_or(String::new(), |path| {
        format!("[ -f {} ] || exit 0; ", quote(&path.to_string_lossy()))
    });
    format!(
        concat!(
            "delay=0; batch=no; ",
            "trap 'exit 130' INT; trap 'exit 143' TERM HUP; ",
            "while :; do {check}{command}; status=$?; {check}",
            "[ \"$status\" -eq 255 ] || exit \"$status\"; batch=yes; ",
            "printf '\\r\\nSSH disconnected. Retrying in %ss (Ctrl-C to stop).\\r\\n' \"$delay\"; ",
            "sleep \"$delay\" || exit 130; ",
            "delay=2; done"
        ),
        check = check,
        command = command,
    )
}

#[cfg(test)]
#[path = "reconnect_tests.rs"]
mod tests;
