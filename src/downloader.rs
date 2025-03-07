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
    pub async fn download<P: AsRef<Path>>(&self, url: &str, output_dir: P) -> Result<PathBuf> {
        let file_name = url
            .split('/')
            .last()
            .context("Failed to determine file name from URL")?;

        let output_path = output_dir.as_ref().join(file_name);

        // Create the file
        let mut file = tokio::fs::File::create(&output_path)
            .await
            .context("Failed to create output file")?;

        // Send the GET request
        info!("Starting download of {}", file_name);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to send GET request")?;

        let total_size = response.content_length().unwrap_or(0);

        // Set up progress bar
        let progress_bar = ProgressBar::new(total_size);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );

        // Download the file chunk by chunk
        let mut stream = response.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(item) = stream.next().await {
            let chunk = item.context("Error while downloading file")?;
            file.write_all(&chunk)
                .await
                .context("Error while writing to file")?;

            downloaded += chunk.len() as u64;
            progress_bar.set_position(downloaded);
        }

        progress_bar.finish_with_message(format!("Downloaded {} successfully", file_name));

        Ok(output_path)
    }
}
