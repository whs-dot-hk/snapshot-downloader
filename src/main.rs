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

/// Command-line arguments for the snapshot downloader
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    /// Path to store downloaded files and extracted data
    #[arg(short, long, default_value = ".")]
    output_dir: PathBuf,

    /// Enable verbose output for detailed logs
    #[arg(short, long)]
    verbose: bool,
}

/// Main entry point for the snapshot downloader application
///
/// This application:
/// 1. Downloads a blockchain snapshot and node binary
/// 2. Extracts them to the specified directories
/// 3. Sets up a Cosmos node with the snapshot data
#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize logging
    setup_logging(args.verbose)?;

    // Create necessary directories
    let (snapshots_dir, data_dir) = create_directories(&args.output_dir)?;

    // Load and parse configuration
    info!("Loading configuration from: {}", args.config.display());
    let config = Config::from_file(&args.config).context("Failed to parse configuration file")?;

    // Download and extract files
    let (snapshot_path, binary_path) = download_required_files(&config, &snapshots_dir).await?;
    extract_files(
        &snapshot_path,
        &binary_path,
        &snapshots_dir,
        &args.output_dir,
    )
    .await?;

    // Move snapshot to data directory
    info!("Moving snapshot to data directory");
    setup::move_snapshot(&snapshots_dir, &data_dir)
        .context("Failed to move snapshot to data directory")?;

    // Setup and initialize Cosmos node
    setup_cosmos_node(&config, &args.output_dir, &data_dir)?;

    info!("Setup complete! You can now start your node.");
    Ok(())
}

/// Sets up the logging system with appropriate verbosity
fn setup_logging(verbose: bool) -> Result<()> {
    let log_level = if verbose { Level::INFO } else { Level::WARN };

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env().add_directive(log_level.into()))
        .with_target(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;

    Ok(())
}

/// Creates necessary directories for downloads and data
fn create_directories(base_dir: &PathBuf) -> Result<(PathBuf, PathBuf)> {
    let snapshots_dir = base_dir.join("snapshots");
    std::fs::create_dir_all(&snapshots_dir).context("Failed to create snapshots directory")?;

    let data_dir = base_dir.join("data");
    std::fs::create_dir_all(&data_dir).context("Failed to create data directory")?;

    Ok((snapshots_dir, data_dir))
}

/// Downloads the snapshot and binary files
async fn download_required_files(
    config: &Config,
    snapshots_dir: &PathBuf,
) -> Result<(PathBuf, PathBuf)> {
    let downloader = Downloader::new();

    // Download snapshot
    info!("Downloading snapshot from: {}", config.snapshot_url);
    let snapshot_path = downloader
        .download(&config.snapshot_url, snapshots_dir)
        .await
        .context("Failed to download snapshot")?;

    // Download binary
    info!("Downloading binary from: {}", config.binary_url);
    let binary_path = downloader
        .download(&config.binary_url, snapshots_dir)
        .await
        .context("Failed to download binary")?;

    Ok((snapshot_path, binary_path))
}

/// Extracts the snapshot and binary files
async fn extract_files(
    snapshot_path: &PathBuf,
    binary_path: &PathBuf,
    snapshots_dir: &PathBuf,
    output_dir: &PathBuf,
) -> Result<()> {
    let extractor = Extractor::new();

    // Extract binary
    info!("Extracting binary package");
    let binary_extract_path = output_dir.join("bin_extract");
    std::fs::create_dir_all(&binary_extract_path)?;
    extractor
        .extract(binary_path, &binary_extract_path)
        .context("Failed to extract binary package")?;

    // Extract snapshot
    info!("Extracting blockchain snapshot");
    extractor
        .extract(snapshot_path, snapshots_dir)
        .context("Failed to extract snapshot")?;

    Ok(())
}

/// Sets up the Cosmos node with the downloaded data
fn setup_cosmos_node(config: &Config, output_dir: &PathBuf, data_dir: &PathBuf) -> Result<()> {
    let binary_extract_path = output_dir.join("bin_extract");
    let cosmos_setup = CosmosSetup::new(&config.cosmos, &binary_extract_path, data_dir);

    info!("Initializing Cosmos node");
    cosmos_setup.init().context("Failed to initialize node")?;

    Ok(())
}
