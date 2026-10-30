pub mod config;
pub mod core;
pub mod hosts;
pub mod navigation;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use command::r#async::Command;

use self::core::{Session, parse_sessions, quote, valid_host};

const REMOTE_PATH: &str =
    "export PATH=\"$HOME/.local/bin:$HOME/bin:/opt/homebrew/bin:/usr/local/bin:$PATH\"; ";

#[derive(Debug)]
pub struct Discovery {
    pub sessions: Vec<Session>,
}

pub fn remote_command(command: &str) -> String {
    format!("sh -lc {}", quote(&format!("{REMOTE_PATH}{command}")))
}

pub async fn discover(host: Option<&str>) -> Result<Discovery> {
    let script = "if command -v zmx >/dev/null 2>&1; then printf '__WARP_ZMX_LIST__\\n'; zmx list; else exit 127; fi";
    let remote = remote_command(script);
    let mut command = match host {
        Some(host) => {
            if !valid_host(host) {
                bail!("Invalid SSH host alias");
            }
            let mut command = Command::new("/usr/bin/ssh");
            command.args([
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
                &remote,
            ]);
            command
        }
        None => {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", &remote]);
            command
        }
    };
    let output = tokio::time::timeout(Duration::from_secs(20), command.kill_on_drop(true).output())
        .await
        .context("Session discovery timed out after 20 seconds")?
        .context("Could not start session discovery")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    match output.status.code() {
        Some(255) => bail!(
            "SSH connection failed: {}. Open an SSH terminal to authenticate, then retry.",
            stderr.trim()
        ),
        Some(127) => bail!("zmx was not found on this host. Install zmx, then refresh."),
        _ => {}
    }
    if !output.status.success() && !stderr.to_ascii_lowercase().contains("no sessions") {
        bail!("Could not list zmx windows: {}", stderr.trim());
    }
    let (_, data) = stdout
        .split_once("__WARP_ZMX_LIST__\n")
        .context("The host did not return a zmx session list")?;
    let sessions = parse_sessions(data);
    if sessions.is_empty()
        && !data.trim().is_empty()
        && !data.to_ascii_lowercase().contains("no sessions")
    {
        bail!("Unrecognized zmx list output. Check the installed zmx version.");
    }
    Ok(Discovery { sessions })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
