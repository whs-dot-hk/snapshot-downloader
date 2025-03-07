use anyhow::{Context, Result};
use fs_extra::dir::{copy, CopyOptions};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, instrument, warn};

use crate::config::CosmosConfig;

/// Handles Cosmos blockchain node setup and configuration
pub struct CosmosSetup {
    /// Node configuration
    config: CosmosConfig,

    /// Path to the node binary
    binary_path: PathBuf,

    /// Path to the data directory
    data_dir: PathBuf,
}

impl CosmosSetup {
    /// Creates a new CosmosSetup instance
    ///
    /// # Arguments
    /// * `config` - Configuration for the Cosmos node
    /// * `binary_extract_path` - Path where the node binary is located
    /// * `data_dir` - Path to the data directory for the node
    pub fn new(config: &CosmosConfig, binary_extract_path: &Path, data_dir: &Path) -> Self {
        CosmosSetup {
            config: config.clone(),
            binary_path: binary_extract_path.join(&config.bin),
            data_dir: data_dir.to_path_buf(),
        }
    }

    /// Initializes the Cosmos node with the provided configuration
    ///
    /// This will:
    /// 1. Run the initialization command
    /// 2. Configure app.toml with custom settings
    /// 3. Configure config.toml with custom settings
    #[instrument(skip(self), fields(bin_path = %self.binary_path.display(), data_dir = %self.data_dir.display()))]
    pub fn init(&self) -> Result<()> {
        // Run initialization command
        self.run_init_command()?;

        // Apply configurations
        self.configure_app_toml()?;
        self.configure_config_toml()?;

        info!("Node initialization completed successfully");
        Ok(())
    }

    /// Runs the node initialization command
    #[instrument(skip(self))]
    fn run_init_command(&self) -> Result<()> {
        info!(
            "Running initialization command: {}",
            self.config.init_command
        );

        let output = Command::new(&self.binary_path)
            .args(self.config.init_command.split_whitespace())
            .current_dir(&self.data_dir)
            .output()
            .context("Failed to execute initialization command")?;

        if !output.status.success() {
            let error_message = String::from_utf8_lossy(&output.stderr);
            warn!(
                "Initialization command failed with output: {}",
                error_message
            );
        } else {
            info!("Initialization command executed successfully");
        }

        Ok(())
    }

    /// Configures app.toml with the provided settings
    #[instrument(skip(self), fields(app_toml_path = %self.data_dir.join("config/app.toml").display()))]
    fn configure_app_toml(&self) -> Result<()> {
        let app_toml_path = self.data_dir.join("config/app.toml");

        // Skip if no app.toml configurations specified
        if self.config.app.is_empty() {
            info!("No app.toml configurations specified, skipping");
            return Ok(());
        }

        // Check if app.toml exists
        if !app_toml_path.exists() {
            warn!(
                "app.toml not found at path: {}, skipping configuration",
                app_toml_path.display()
            );
            return Ok(());
        }

        // Apply configuration changes
        self.apply_toml_changes(app_toml_path, &self.config.app, "app.toml")
    }

    /// Configures config.toml with the provided settings
    #[instrument(skip(self), fields(config_toml_path = %self.data_dir.join("config/config.toml").display()))]
    fn configure_config_toml(&self) -> Result<()> {
        let config_toml_path = self.data_dir.join("config/config.toml");

        // Skip if no config.toml configurations specified
        if self.config.config.is_empty() {
            info!("No config.toml configurations specified, skipping");
            return Ok(());
        }

        // Check if config.toml exists
        if !config_toml_path.exists() {
            warn!(
                "config.toml not found at path: {}, skipping configuration",
                config_toml_path.display()
            );
            return Ok(());
        }

        // Apply configuration changes
        self.apply_toml_changes(config_toml_path, &self.config.config, "config.toml")
    }

    /// Applies configuration changes to a TOML file
    fn apply_toml_changes(
        &self,
        file_path: PathBuf,
        settings: &HashMap<String, serde_yaml::Value>,
        file_type: &str,
    ) -> Result<()> {
        // Read existing file content
        let content =
            fs::read_to_string(&file_path).context(format!("Failed to read {}", file_type))?;

        let mut updated_content = content.clone();

        // Apply each setting
        for (key, value) in settings {
            info!(key = %key, value = ?value, "Setting {} value", file_type);
            let value_str = format!("{:?}", value);

            // Create regex for finding the key
            let pattern = format!("{} = ", key);
            if updated_content.contains(&pattern) {
                // Update existing key
                let re = regex::Regex::new(&format!(r"(?m)^{}\s*=.*$", regex::escape(key)))
                    .context("Failed to create regex")?;
                updated_content = re
                    .replace(&updated_content, &format!("{} = {}", key, value_str))
                    .to_string();
            } else {
                // Add new key
                updated_content.push_str(&format!("\n{} = {}", key, value_str));
            }
        }

        // Write changes if content was modified
        if content != updated_content {
            fs::write(&file_path, updated_content)
                .context(format!("Failed to write updated {}", file_type))?;
            info!("Updated {} configuration", file_type);
        } else {
            info!("No changes needed for {}", file_type);
        }

        Ok(())
    }
}

/// Moves extracted snapshot data to the node's data directory
///
/// This function finds the extracted snapshot directory and
/// copies its contents to the specified data directory.
#[instrument(skip(snapshot_dir, data_dir), fields(from = %snapshot_dir.as_ref().display(), to = %data_dir.as_ref().display()))]
pub fn move_snapshot<P: AsRef<Path>, Q: AsRef<Path>>(snapshot_dir: P, data_dir: Q) -> Result<()> {
    let snapshot_dir = snapshot_dir.as_ref();
    let data_dir = data_dir.as_ref();

    info!("Moving snapshot data to data directory");

    // Find the extracted snapshot directory
    let snapshot_dirs = find_snapshot_directories(snapshot_dir)?;

    if snapshot_dirs.is_empty() {
        return Err(anyhow::anyhow!("No extracted snapshot directory found"));
    }

    // Use the first found directory
    let snapshot_src = &snapshot_dirs[0];
    info!(source = %snapshot_src.display(), "Found snapshot directory");

    // Copy with overwrite options
    let options = create_copy_options();

    copy(snapshot_src, data_dir, &options)
        .context("Failed to copy snapshot data to data directory")?;

    info!("Successfully moved snapshot data to data directory");
    Ok(())
}

/// Finds snapshot directories in the specified path
fn find_snapshot_directories(dir: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(dir)
        .context("Failed to read snapshot directory")?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    Ok(entries)
}

/// Creates copy options for directory copying
fn create_copy_options() -> CopyOptions {
    let mut options = CopyOptions::new();
    options.overwrite = true;
    options.copy_inside = true;
    options
}
