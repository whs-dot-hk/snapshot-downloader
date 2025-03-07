use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub snapshot_url: String,
    pub binary_url: String,
    pub cosmos: CosmosConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CosmosConfig {
    pub bin: String,
    pub init_command: String,
    pub start_command: String,
    #[serde(default)]
    pub app: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    pub config: HashMap<String, serde_yaml::Value>,
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut file = File::open(path).context("Failed to open config file")?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .context("Failed to read config file")?;

        let config: Config =
            serde_yaml::from_str(&content).context("Failed to parse YAML config")?;

        Ok(config)
    }
}
