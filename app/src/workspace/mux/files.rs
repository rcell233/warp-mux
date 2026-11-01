use std::io::{Seek, Write};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use command::r#async::Command;
use futures_lite::io::{AsyncBufReadExt, BufReader};
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

pub fn download_destination(directory: &Path, name: &str) -> Result<std::path::PathBuf> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.chars().any(char::is_control)
    {
        bail!("Invalid download filename");
    }
    Ok(directory.join(name))
}

pub async fn download(host: Option<&str>, source: &str, destination: &Path) -> Result<()> {
    download_with_progress(host, source, destination, |_| {}).await
}

pub async fn download_with_progress(
    host: Option<&str>,
    source: &str,
    destination: &Path,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let parent = destination.parent().context("Missing download directory")?;
    async_fs::create_dir_all(parent).await?;
    let staged = tempfile::NamedTempFile::new_in(parent)?;
    let mut command;
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
            command = Command::new("/usr/bin/sftp");
            command
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
                .stdin(batch);
        }
        None => {
            command = Command::new("/bin/cp");
            command.arg("--").arg(source).arg(staged.path());
        }
    }
    let mut output = Box::pin(command.kill_on_drop(true).output());
    let result = tokio::time::timeout(Duration::from_secs(3600), async {
        loop {
            let next = futures_lite::future::or(async { Some(output.as_mut().await) }, async {
                tokio::time::sleep(Duration::from_millis(250)).await;
                None
            })
            .await;
            progress(async_fs::metadata(staged.path()).await?.len());
            if let Some(result) = next {
                return result;
            }
        }
    })
    .await
    .context("Download timed out")??;
    if !result.status.success() {
        bail!(
            "Download failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    let destination = destination.to_owned();
    tokio::task::spawn_blocking(move || -> Result<()> {
        staged.as_file().sync_all()?;
        staged.persist(destination).map_err(|error| error.error)?;
        Ok(())
    })
    .await??;
    Ok(())
}

pub async fn upload(
    host: Option<&str>,
    source: &Path,
    base: &str,
    directory: &str,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    let metadata = async_fs::metadata(source).await?;
    if !metadata.is_file() {
        bail!("Select files to upload; folders are not supported");
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .context("Invalid upload filename")?;
    download_destination(Path::new(directory), name)?;
    let total = metadata.len();
    progress(0, total);
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
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "RemoteCommand=none",
                "--",
                host,
            ]);
            command.arg(format!(
                "python3 -c {} {} {} {} {}",
                quote(include_str!("upload_file.py")),
                quote(base),
                quote(directory),
                quote(name),
                total
            ));
            command
        }
        None => {
            let mut command = Command::new("python3");
            command.args([
                "-c",
                include_str!("upload_file.py"),
                base,
                directory,
                name,
                &total.to_string(),
            ]);
            command
        }
    };
    let source = source.to_owned();
    let input = tokio::task::spawn_blocking(move || std::fs::File::open(source)).await??;
    let errors = tempfile::NamedTempFile::new()?;
    let mut child = command
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(errors.reopen()?)
        .kill_on_drop(true)
        .spawn()?;
    tokio::time::timeout(Duration::from_secs(3600), async {
        let mut reader = BufReader::new(
            child
                .stdout
                .take()
                .context("Missing upload progress stream")?,
        );
        let mut line = String::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).await? == 0 {
                break;
            }
            if let Some(bytes) = line
                .trim()
                .strip_prefix("__WARP_MUX_TRANSFER__")
                .and_then(|value| value.parse::<u64>().ok())
            {
                progress(bytes.min(total), total);
            }
        }
        if !child.status().await?.success() {
            bail!(
                "Upload failed: {}",
                async_fs::read_to_string(errors.path()).await?.trim()
            );
        }
        Ok(())
    })
    .await
    .context("Upload timed out")?
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
