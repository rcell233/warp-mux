use super::core::{Session, attach_command, quote};
use super::remote_command;

pub fn ssh_session_script(host: &str, session: &Session) -> String {
    let command = format!(
        "/usr/bin/ssh -tt -o RemoteCommand=none -o ConnectTimeout=8 -o ConnectionAttempts=1 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o TCPKeepAlive=yes -o BatchMode=\"$batch\" -- {} {}",
        quote(host),
        quote(&remote_command(&attach_command(session)))
    );
    retry_transport(&command)
}

fn retry_transport(command: &str) -> String {
    format!(
        concat!(
            "delay=0; batch=no; ",
            "trap 'exit 130' INT; trap 'exit 143' TERM HUP; ",
            "while :; do {}; status=$?; ",
            "[ \"$status\" -eq 255 ] || exit \"$status\"; batch=yes; ",
            "printf '\\r\\nSSH disconnected. Retrying in %ss (Ctrl-C to stop).\\r\\n' \"$delay\"; ",
            "sleep \"$delay\" || exit 130; ",
            "delay=2; done"
        ),
        command
    )
}

#[cfg(test)]
#[path = "reconnect_tests.rs"]
mod tests;
