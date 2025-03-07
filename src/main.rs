use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

mod config;
mod downloader;
mod extractor;
mod setup;

use config::Config;
use downloader::Downloader;
use extractor::Extractor;
use setup::CosmosSetup;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// Path to store downloaded files and extracted data
    #[arg(short, long, default_value = ".")]
    output_dir: PathBuf,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize tracing
    let log_level = if args.verbose {
        Level::INFO
    } else {
        Level::WARN
    };

    // Create a subscriber with formatted output
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env().add_directive(log_level.into()))
        .with_target(false)
        .finish();

    // Set the subscriber as the default
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;

    // Create necessary directories
    let snapshots_dir = args.output_dir.join("snapshots");
    std::fs::create_dir_all(&snapshots_dir).context("Failed to create snapshots directory")?;

    let data_dir = args.output_dir.join("data");
    std::fs::create_dir_all(&data_dir).context("Failed to create data directory")?;

    // Parse configuration
    info!("Loading configuration from: {:?}", args.config);
    let config = Config::from_file(&args.config).context("Failed to parse configuration file")?;

    // Download snapshot
    info!("Downloading snapshot from: {}", config.snapshot_url);
    let downloader = Downloader::new();
    let snapshot_path = downloader
        .download(&config.snapshot_url, &snapshots_dir)
        .await
        .context("Failed to download snapshot")?;

    // Download binary
    info!("Downloading binary from: {}", config.binary_url);
    let binary_path = downloader
        .download(&config.binary_url, &snapshots_dir)
        .await
        .context("Failed to download binary")?;

    // Extract files
    let extractor = Extractor::new();

    info!("Extracting binary tarball");
    let binary_extract_path = args.output_dir.join("bin_extract");
    std::fs::create_dir_all(&binary_extract_path)?;
    extractor
        .extract(&binary_path, &binary_extract_path)
        .context("Failed to extract binary tarball")?;

    info!("Extracting snapshot");
    extractor
        .extract(&snapshot_path, &snapshots_dir)
        .context("Failed to extract snapshot")?;

    // Move snapshot to data directory
    info!("Moving snapshot to data directory");
    setup::move_snapshot(&snapshots_dir, &data_dir)
        .context("Failed to move snapshot to data directory")?;

    // Setup Cosmos node
    let cosmos_setup = CosmosSetup::new(&config.cosmos, &binary_extract_path, &data_dir);

    info!("Initializing Cosmos node");
    cosmos_setup.init().context("Failed to initialize node")?;

    info!("Setup complete! You can now start your node.");
    Ok(())
}
