use anyhow::{Context, Result};
use fs_extra::dir::{copy, CopyOptions};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, instrument, warn};

use crate::config::CosmosConfig;

pub struct CosmosSetup {
    config: CosmosConfig,
    binary_path: PathBuf,
    data_dir: PathBuf,
}

impl CosmosSetup {
    pub fn new(config: &CosmosConfig, binary_extract_path: &Path, data_dir: &Path) -> Self {
        CosmosSetup {
            config: config.clone(),
            binary_path: binary_extract_path.join(&config.bin),
            data_dir: data_dir.to_path_buf(),
        }
    }

    /// Initialize the Cosmos node with the provided configuration
    #[instrument(skip(self), fields(bin_path = %self.binary_path.display(), data_dir = %self.data_dir.display()))]
    pub fn init(&self) -> Result<()> {
        // Run init command
        info!(
            "Running initialization command: {}",
            self.config.init_command
        );
        let output = Command::new(&self.binary_path)
            .args(self.config.init_command.split_whitespace())
            .current_dir(&self.data_dir)
            .output()
            .context("Failed to execute init command")?;

        if !output.status.success() {
            warn!(
                "Init command failed with output: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        } else {
            info!("Init command successful");
        }

        // Configure app.toml
        self.configure_app_toml()?;

        // Configure config.toml
        self.configure_config_toml()?;

        Ok(())
    }

    /// Configure app.toml with the provided settings
    #[instrument(skip(self), fields(app_toml_path = %self.data_dir.join("config/app.toml").display()))]
    fn configure_app_toml(&self) -> Result<()> {
        let app_toml_path = self.data_dir.join("config/app.toml");

        if self.config.app.is_empty() {
            info!("No app.toml configurations specified, skipping");
            return Ok(());
        }

        if !app_toml_path.exists() {
            warn!("app.toml not found at specified path, skipping configuration");
            return Ok(());
        }

        let content = fs::read_to_string(&app_toml_path).context("Failed to read app.toml")?;

        let mut updated_content = content.clone();

        for (key, value) in &self.config.app {
            info!(key = %key, value = ?value, "Setting app.toml value");
            let value_str = format!("{:?}", value);
            // Simple replacement - in a real application, proper TOML parsing would be better
            let pattern = format!("{} = ", key);
            if updated_content.contains(&pattern) {
                let re = regex::Regex::new(&format!(r"(?m)^{}\s*=.*$", regex::escape(key)))
                    .context("Failed to create regex")?;
                updated_content = re
                    .replace(&updated_content, &format!("{} = {}", key, value_str))
                    .to_string();
            } else {
                updated_content.push_str(&format!("\n{} = {}", key, value_str));
            }
        }

        if content != updated_content {
            fs::write(&app_toml_path, updated_content)
                .context("Failed to write updated app.toml")?;
            info!("Updated app.toml configuration");
        } else {
            info!("No changes needed for app.toml");
        }

        Ok(())
    }

    /// Configure config.toml with the provided settings
    #[instrument(skip(self), fields(config_toml_path = %self.data_dir.join("config/config.toml").display()))]
    fn configure_config_toml(&self) -> Result<()> {
        let config_toml_path = self.data_dir.join("config/config.toml");

        if self.config.config.is_empty() {
            info!("No config.toml configurations specified, skipping");
            return Ok(());
        }

        if !config_toml_path.exists() {
            warn!("config.toml not found at specified path, skipping configuration");
            return Ok(());
        }

        let content =
            fs::read_to_string(&config_toml_path).context("Failed to read config.toml")?;

        let mut updated_content = content.clone();

        for (key, value) in &self.config.config {
            info!(key = %key, value = ?value, "Setting config.toml value");
            let value_str = format!("{:?}", value);
            // Simple replacement - in a real application, proper TOML parsing would be better
            let pattern = format!("{} = ", key);
            if updated_content.contains(&pattern) {
                let re = regex::Regex::new(&format!(r"(?m)^{}\s*=.*$", regex::escape(key)))
                    .context("Failed to create regex")?;
                updated_content = re
                    .replace(&updated_content, &format!("{} = {}", key, value_str))
                    .to_string();
            } else {
                updated_content.push_str(&format!("\n{} = {}", key, value_str));
            }
        }

        if content != updated_content {
            fs::write(&config_toml_path, updated_content)
                .context("Failed to write updated config.toml")?;
            info!("Updated config.toml configuration");
        } else {
            info!("No changes needed for config.toml");
        }

        Ok(())
    }
}

/// Move snapshot data to the data directory
#[instrument(skip(snapshot_dir, data_dir), fields(from = %snapshot_dir.as_ref().display(), to = %data_dir.as_ref().display()))]
pub fn move_snapshot<P: AsRef<Path>, Q: AsRef<Path>>(snapshot_dir: P, data_dir: Q) -> Result<()> {
    let snapshot_dir = snapshot_dir.as_ref();
    let data_dir = data_dir.as_ref();

    info!("Moving snapshot data");

    // Find the extracted snapshot directory (typically the only directory in the snapshots folder)
    let entries = fs::read_dir(snapshot_dir)
        .context("Failed to read snapshot directory")?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();

    if entries.is_empty() {
        return Err(anyhow::anyhow!("No extracted snapshot directory found"));
    }

    // Copy the found directory to the data directory
    let snapshot_src = entries[0].path();
    info!(source = %snapshot_src.display(), "Found snapshot directory");

    let mut options = CopyOptions::new();
    options.overwrite = true;
    options.copy_inside = true;

    copy(&snapshot_src, data_dir, &options)
        .context("Failed to copy snapshot data to data directory")?;

    info!("Successfully moved snapshot data to data directory");

    Ok(())
}
