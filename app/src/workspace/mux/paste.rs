use std::io::{Read, Seek, Write};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::files;

pub const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;

pub enum ImageSource {
    Clipboard(Vec<u8>),
    File(PathBuf),
}

impl ImageSource {
    pub fn queued_bytes(&self) -> usize {
        match self {
            Self::Clipboard(bytes) => bytes.len(),
            Self::File(_) => 0,
        }
    }
}

pub async fn upload_image(
    host: &str,
    source: ImageSource,
    progress: impl FnMut(u64, u64),
) -> Result<String> {
    let staged = tokio::task::spawn_blocking(move || stage_image(source)).await??;
    let mut staged = convert_tiff(staged).await?;
    let total = staged.as_file().metadata()?.len();
    let mut header = [0u8; 12];
    let read = staged.read(&mut header)?;
    staged.rewind()?;
    let extension = image_extension(&header[..read])?;
    let arguments = [
        "--clipboard".to_owned(),
        extension.to_owned(),
        total.to_string(),
    ];
    let path = files::upload_stream(Some(host), staged.reopen()?, &arguments, total, progress)
        .await?
        .context("The host did not return the uploaded image path")?;
    validate_remote_path(&path)?;
    Ok(path)
}

fn stage_image(source: ImageSource) -> Result<tempfile::NamedTempFile> {
    let mut staged = tempfile::NamedTempFile::new()?;
    match source {
        ImageSource::Clipboard(bytes) => {
            if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
                bail!("Clipboard images must be between 1 byte and 32 MiB");
            }
            staged.write_all(&bytes)?;
        }
        ImageSource::File(path) => {
            // Reject devices and FIFOs before opening them, as opening can itself block.
            let metadata = std::fs::metadata(&path)?;
            if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES as u64
            {
                bail!("Choose an image file no larger than 32 MiB");
            }
            let mut file = std::fs::File::open(path)?.take(MAX_IMAGE_BYTES as u64 + 1);
            if std::io::copy(&mut file, &mut staged)? > MAX_IMAGE_BYTES as u64 {
                bail!("Image exceeds 32 MiB");
            }
        }
    }
    staged.rewind()?;
    Ok(staged)
}

async fn convert_tiff(mut source: tempfile::NamedTempFile) -> Result<tempfile::NamedTempFile> {
    let mut header = [0u8; 4];
    let read = source.read(&mut header)?;
    source.rewind()?;
    if read == 4 && (&header == b"II\x2a\0" || &header == b"MM\0\x2a") {
        let converted = tempfile::NamedTempFile::new()?;
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            command::r#async::Command::new("/usr/bin/sips")
                .args(["-s", "format", "png"])
                .arg(source.path())
                .arg("--out")
                .arg(converted.path())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("Image conversion timed out")??;
        if !output.status.success()
            || converted.as_file().metadata()?.len() > MAX_IMAGE_BYTES as u64
        {
            bail!("Could not convert clipboard TIFF to a PNG smaller than 32 MiB");
        }
        return Ok(converted);
    }
    Ok(source)
}

fn image_extension(header: &[u8]) -> Result<&'static str> {
    if header.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("png")
    } else if header.starts_with(b"\xff\xd8\xff") {
        Ok("jpg")
    } else if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
        Ok("gif")
    } else if header.starts_with(b"RIFF") && header.get(8..12) == Some(b"WEBP") {
        Ok("webp")
    } else {
        bail!("Use a PNG, JPEG, GIF, WebP, or TIFF image")
    }
}

fn validate_remote_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.chars().any(char::is_control) {
        bail!("The host returned an invalid image path");
    }
    Ok(())
}

pub fn paste_path(path: &str) -> Result<String> {
    validate_remote_path(path)?;
    Ok(super::core::quote(path))
}

#[cfg(test)]
#[path = "paste_tests.rs"]
mod tests;
