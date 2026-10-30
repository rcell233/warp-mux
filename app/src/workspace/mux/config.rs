use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::core::valid_host;

pub fn import_hosts(home: &Path) -> Result<Vec<String>> {
    let mut hosts = BTreeSet::new();
    let mut stack = Vec::new();
    read_config(&home.join(".ssh/config"), home, &mut stack, &mut hosts)?;
    Ok(hosts.into_iter().collect())
}

fn read_config(
    path: &Path,
    home: &Path,
    stack: &mut Vec<PathBuf>,
    hosts: &mut BTreeSet<String>,
) -> Result<()> {
    let path = fs::canonicalize(path).with_context(|| format!("Cannot read {}", path.display()))?;
    if stack.contains(&path) || stack.len() >= 16 {
        bail!(
            "SSH config Include cycle or nesting limit at {}",
            path.display()
        );
    }
    let text = fs::read_to_string(&path)?;
    stack.push(path);
    let mut conditional = false;
    for line in text.lines() {
        let line = line.trim_start();
        let end = line
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(line.len());
        let keyword = &line[..end];
        let args = line[end..]
            .trim_start()
            .trim_start_matches('=')
            .trim_start();
        let fields = shlex::split(args).context("Invalid quoting in SSH config")?;
        if keyword.eq_ignore_ascii_case("host") {
            conditional = false;
            hosts.extend(fields.into_iter().filter(|host| valid_host(host)));
        } else if keyword.eq_ignore_ascii_case("match") {
            conditional = !fields.iter().any(|field| field.eq_ignore_ascii_case("all"));
        } else if keyword.eq_ignore_ascii_case("include") && !conditional {
            for pattern in fields {
                let pattern = if let Some(rest) = pattern.strip_prefix("~/") {
                    home.join(rest)
                } else if Path::new(&pattern).is_absolute() {
                    PathBuf::from(pattern)
                } else {
                    home.join(".ssh").join(pattern)
                };
                for included in glob::glob(&pattern.to_string_lossy())? {
                    read_config(&included?, home, stack, hosts)?;
                }
            }
        }
    }
    stack.pop();
    Ok(())
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
