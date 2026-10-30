use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::core::valid_host;

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedHosts {
    pub aliases: Vec<String>,
    #[serde(default)]
    pub base_paths: std::collections::BTreeMap<String, String>,
}

impl SavedHosts {
    pub fn load(path: &Path) -> Result<Self> {
        let data = match fs::read(path) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut saved: Self =
            serde_json::from_slice(&data).context("Could not read saved SSH hosts")?;
        if saved.aliases.iter().any(|alias| !valid_host(alias)) {
            bail!("Saved hosts contain an invalid SSH alias");
        }
        saved.aliases.sort();
        saved.aliases.dedup();
        Ok(saved)
    }
    pub fn add(&mut self, aliases: impl IntoIterator<Item = String>) {
        self.aliases
            .extend(aliases.into_iter().filter(|alias| valid_host(alias)));
        self.aliases.sort();
        self.aliases.dedup();
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().context("Missing host-list directory")?;
        fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(staged.as_file_mut(), self)?;
        staged.as_file_mut().sync_all()?;
        staged.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "hosts_tests.rs"]
mod tests;
