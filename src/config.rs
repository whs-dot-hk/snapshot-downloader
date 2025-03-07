use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Main application configuration
///
/// Contains URLs for downloading required files and
/// configuration for the Cosmos node setup
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// URL to download the blockchain snapshot
    pub snapshot_url: String,

    /// URL to download the node binary
    pub binary_url: String,

    /// Cosmos-specific configuration
    pub cosmos: CosmosConfig,
}

/// Cosmos node configuration
///
/// Contains settings for initializing and running a Cosmos blockchain node
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CosmosConfig {
    /// Path to the binary executable
    pub bin: String,

    /// Command to initialize the node
    pub init_command: String,

    /// Command to start the node
    pub start_command: String,

    /// Custom settings for app.toml configuration file
    #[serde(default)]
    pub app: HashMap<String, serde_yaml::Value>,

    /// Custom settings for config.toml configuration file
    #[serde(default)]
    pub config: HashMap<String, serde_yaml::Value>,
}

impl Config {
    /// Loads configuration from a YAML file
    ///
    /// # Arguments
    /// * `path` - Path to the YAML configuration file
    ///
    /// # Returns
    /// * `Result<Config>` - The parsed configuration or an error
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        // Open and read the file
        let mut file = File::open(path).context("Failed to open config file")?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .context("Failed to read config file")?;

        // Parse YAML into Config struct
        let config: Config =
            serde_yaml::from_str(&content).context("Failed to parse YAML config")?;

        Ok(config)
    }
}
