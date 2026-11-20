use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::core::{Session, quote, valid_session_name};
use super::{discover, host_command};

pub fn kill_command(session: &Session) -> Result<String> {
    // zmx interprets a trailing '*' itself, even when shell-quoted.
    if !valid_session_name(&session.name) || session.name.ends_with('*') {
        bail!("Cannot safely terminate this window: its name is empty, an option, or a wildcard.");
    }
    Ok(format!(
        concat!(
            "unset ZMX_SESSION ZMX_SESSION_PREFIX; ",
            "case \"$(zmx help 2>&1)\" in ",
            "*--force*) exec zmx kill {} --force ;; ",
            "*) exec zmx kill {} ;; esac"
        ),
        quote(&session.name),
        quote(&session.name),
    ))
}

pub async fn terminate(host: Option<&str>, session: &Session) -> Result<()> {
    let mut command = host_command(host, &kill_command(session)?)?;
    let output = tokio::time::timeout(Duration::from_secs(20), command.kill_on_drop(true).output())
        .await
        .context("Terminating the zmx window timed out. Refresh to check its status.")?
        .context("Could not start zmx termination")?;
    if !output.status.success() {
        bail!(
            "Could not terminate the zmx window: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    // Some zmx versions return success even when a busy daemon refused the kill.
    for _ in 0..5 {
        if !discover(host).await?.sessions.contains(session) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    bail!("The zmx window is still running. Refresh and retry; updating zmx may be necessary.")
}

#[cfg(test)]
#[path = "terminate_tests.rs"]
mod tests;
