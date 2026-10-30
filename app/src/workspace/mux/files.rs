use std::io::{Seek, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use command::r#async::Command;
use serde::Deserialize;

use super::core::{quote, valid_host};

const LIST_SCRIPT: &str = include_str!("list_files.py");

#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Listing {
    pub base: String,
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

pub async fn list(host: Option<&str>, base: &str, path: &str) -> Result<Listing> {
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
                "RemoteCommand=none",
                "-o",
                "RequestTTY=no",
                "--",
                host,
            ]);
            command.arg(format!(
                "python3 -c {} {} {}",
                quote(LIST_SCRIPT),
                quote(base),
                quote(path)
            ));
            command
        }
        None => {
            let mut command = Command::new("python3");
            command.args(["-c", LIST_SCRIPT, base, path]);
            command
        }
    };
    let output = tokio::time::timeout(Duration::from_secs(30), command.kill_on_drop(true).output())
        .await
        .context("File listing timed out")??;
    if !output.status.success() {
        bail!(
            "Could not browse files (SSH authentication and python3 are required on remote hosts): {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8(output.stdout)?;
    let (_, json) = text
        .split_once("__WARP_MUX_FILES__\n")
        .context("Missing file listing response")?;
    Ok(serde_json::from_str(json)?)
}

fn sftp_path(path: &str) -> Result<String> {
    if path.is_empty() || path.chars().any(char::is_control) {
        bail!("Unsupported file path");
    }
    let mut escaped = String::from("\"");
    for character in path.chars() {
        if "\\\"*?[]".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('"');
    Ok(escaped)
}

pub async fn download(host: Option<&str>, source: &str, destination: &Path) -> Result<()> {
    let parent = destination.parent().context("Missing download directory")?;
    let staged = tempfile::NamedTempFile::new_in(parent)?;
    match host {
        Some(host) => {
            if !valid_host(host) {
                bail!("Invalid SSH host alias");
            }
            let mut batch = tempfile::tempfile()?;
            writeln!(
                batch,
                "get {} {}",
                sftp_path(source)?,
                sftp_path(staged.path().to_str().context("Invalid destination path")?)?
            )?;
            batch.rewind()?;
            let output = tokio::time::timeout(
                Duration::from_secs(3600),
                Command::new("/usr/bin/sftp")
                    .args([
                        "-b",
                        "-",
                        "-o",
                        "BatchMode=yes",
                        "-o",
                        "ConnectTimeout=8",
                        "-o",
                        "RemoteCommand=none",
                        "--",
                        host,
                    ])
                    .stdin(batch)
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .context("Download timed out")??;
            if !output.status.success() {
                bail!(
                    "Download failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
        }
        None => {
            let mut command = Command::new("/bin/cp");
            let output = command
                .arg("--")
                .arg(source)
                .arg(staged.path())
                .kill_on_drop(true)
                .output()
                .await?;
            if !output.status.success() {
                bail!(
                    "Copy failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
        }
    }
    staged.as_file().sync_all()?;
    staged.persist(destination).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
