pub mod config;
pub mod core;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use command::r#async::Command;

use self::core::{Manager, Session, no_sessions_error, parse_sessions, quote, valid_host};

const REMOTE_PATH: &str =
    "export PATH=\"$HOME/.local/bin:$HOME/bin:/opt/homebrew/bin:/usr/local/bin:$PATH\"; ";

#[derive(Debug)]
pub struct Discovery {
    pub sessions: Vec<Session>,
    pub notices: Vec<String>,
}

pub fn remote_command(command: &str) -> String {
    format!("sh -lc {}", quote(&format!("{REMOTE_PATH}{command}")))
}

pub async fn discover(host: &str) -> Result<Discovery> {
    if !valid_host(host) {
        bail!("Invalid SSH host alias");
    }
    let mut sessions = Vec::new();
    let mut notices = Vec::new();
    for manager in [Manager::Zmx, Manager::Tmux] {
        let label = manager.label();
        let list = match manager {
            Manager::Zmx => "zmx list",
            Manager::Tmux => "tmux -u list-sessions -F '#{session_id}|#{session_name}'",
        };
        let script = format!(
            "if command -v {label} >/dev/null 2>&1; then printf '__WARP_MUX_LIST__\\n'; {list}; else exit 127; fi"
        );
        let output = tokio::time::timeout(
            Duration::from_secs(20),
            Command::new("/usr/bin/ssh")
                .args([
                    "-T",
                    "-o",
                    "BatchMode=yes",
                    "-o",
                    "ConnectTimeout=8",
                    "-o",
                    "ConnectionAttempts=1",
                    "-o",
                    "RemoteCommand=none",
                    "-o",
                    "RequestTTY=no",
                    "--",
                    host,
                    &remote_command(&script),
                ])
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("SSH discovery timed out (20 seconds)")?
        .context("Could not start SSH")?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.code() == Some(255) {
            bail!(
                "SSH failed: {}. Use Open SSH to authenticate or accept the host key, then refresh.",
                stderr.trim()
            );
        }
        if output.status.code() == Some(127) {
            notices.push(format!("{label}: not installed or not on PATH"));
            continue;
        }
        let Some((_, data)) = stdout.split_once("__WARP_MUX_LIST__\n") else {
            bail!("SSH did not return a session list: {}", stderr.trim());
        };
        if output.status.success() {
            let found = parse_sessions(manager, data);
            if found.is_empty() && !data.trim().is_empty() && !no_sessions_error(data) {
                notices.push(format!("{label}: unrecognized list output"));
            }
            sessions.extend(found);
        } else if !no_sessions_error(&stderr) && !no_sessions_error(data) {
            notices.push(format!("{label}: {}", stderr.trim()));
        }
    }
    sessions.sort_by(|a, b| (a.manager.label(), &a.name).cmp(&(b.manager.label(), &b.name)));
    sessions.dedup();
    Ok(Discovery { sessions, notices })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
