use anyhow::{Context, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tracing::info;

pub struct Downloader {
    client: Client,
}

impl Downloader {
    pub fn new() -> Self {
        Downloader {
            client: Client::new(),
        }
    }

    /// Download a file from a URL and save it to the specified directory
    /// Supports resuming downloads if the file already exists
    pub async fn download<P: AsRef<Path>>(&self, url: &str, output_dir: P) -> Result<PathBuf> {
        let file_name = url
            .split('/')
            .next_back()
            .context("Failed to determine file name from URL")?;

        let output_path = output_dir.as_ref().join(file_name);

        // Check if file exists to determine if we're resuming
        let file_exists = output_path.exists();
        let file_size = if file_exists {
            tokio::fs::metadata(&output_path).await?.len()
        } else {
            0
        };

        // Print status message based on whether we're resuming or starting fresh
        info!(
            "{} download of {}",
            if file_exists && file_size > 0 {
                "Resuming"
            } else {
                "Starting"
            },
            file_name
        );

        // Open the file in the appropriate mode (create or append)
        let mut file = if file_exists && file_size > 0 {
            tokio::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&output_path)
                .await
                .context("Failed to open existing file for resuming download")?
        } else {
            tokio::fs::File::create(&output_path)
                .await
                .context("Failed to create output file")?
        };

        // Create the request, adding Range header if resuming
        let mut request = self.client.get(url);

        if file_exists && file_size > 0 {
            info!("Resuming from byte position {}", file_size);
            request = request.header("Range", format!("bytes={}-", file_size));
        }

        // Send the request
        let response = request.send().await.context("Failed to send GET request")?;

        // Log response status and headers for troubleshooting
        info!("Response status: {}", response.status());
        info!("Response headers: {:#?}", response.headers());

        // We always attempt to resume if file_size > 0, regardless of response status

        let total_size = if file_exists && file_size > 0 {
            // For resumed downloads, add existing file size to content length
            info!(
                "Resuming download, adding existing file size {} to content length",
                file_size
            );
            file_size + response.content_length().unwrap_or(0)
        } else {
            // For new downloads, use content length directly
            let content_length = response.content_length().unwrap_or(0);
            info!("New download, content length: {}", content_length);
            content_length
        };

        // Set up progress bar
        let progress_bar = ProgressBar::new(total_size);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );

        // Set initial position if resuming
        let mut downloaded = if file_exists && file_size > 0 {
            // If we're resuming, start from the existing file position
            info!("Continuing download from position: {}", file_size);
            progress_bar.set_position(file_size);
            file_size
        } else {
            0
        };

        // Download the file chunk by chunk
        let mut stream = response.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk = item.context("Error while downloading file")?;
            file.write_all(&chunk)
                .await
                .context("Error while writing to file")?;

            downloaded += chunk.len() as u64;
            progress_bar.set_position(downloaded);

            // Log progress periodically (every 5MB)
            if !chunk.is_empty() && downloaded % (5 * 1024 * 1024) < chunk.len() as u64 {
                info!(
                    "Downloaded: {:.2} MB / {:.2} MB",
                    downloaded as f64 / 1_048_576.0,
                    total_size as f64 / 1_048_576.0
                );
            }
        }

        progress_bar.finish_with_message(format!("Downloaded {} successfully", file_name));
        info!(
            "Completed download of {} ({:.2} MB)",
            file_name,
            downloaded as f64 / 1_048_576.0
        );

        Ok(output_path)
    }
}
