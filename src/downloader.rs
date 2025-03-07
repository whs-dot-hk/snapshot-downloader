use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use reqwest::StatusCode;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

pub struct Downloader {
    client: Client,
}

impl Downloader {
    pub fn new() -> Self {
        Downloader {
            client: Client::new(),
        }
    }

    /// Get metadata about a file before downloading
    /// Returns the content length and whether the server supports range requests
    async fn get_file_metadata(&self, url: &str) -> Result<(Option<u64>, bool)> {
        let response = self
            .client
            .head(url)
            .send()
            .await
            .context("Failed to send HEAD request")?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "HEAD request failed with status: {}",
                response.status()
            ));
        }

        // Check if server supports range requests
        let supports_range = response
            .headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "bytes")
            .unwrap_or(false);

        // Get content length if available
        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        info!(
            "File metadata - Content length: {:?}, Supports range: {}",
            content_length, supports_range
        );

        Ok((content_length, supports_range))
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

        // Get metadata about the file
        let (remote_size, supports_range) = self.get_file_metadata(url).await?;

        info!(
            "{} download of {} to {}",
            if file_exists && file_size > 0 {
                "Resuming"
            } else {
                "Starting"
            },
            file_name,
            output_path.display()
        );

        // If local file is already complete, no need to download again
        if let Some(remote_size) = remote_size {
            if file_exists && file_size == remote_size {
                info!("File is already complete, skipping download");
                return Ok(output_path);
            }
        }

        // Open the file in the appropriate mode (create or append)
        let mut file = if file_exists && file_size > 0 && supports_range {
            tokio::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&output_path)
                .await
                .context("Failed to open existing file for resuming download")?
        } else {
            // If the server doesn't support range requests or file doesn't exist,
            // start a new download
            tokio::fs::File::create(&output_path)
                .await
                .context("Failed to create output file")?
        };

        // Create the request, adding Range header if resuming and server supports it
        let mut request = self.client.get(url);

        if file_exists && file_size > 0 && supports_range {
            info!("Resuming from byte position {}", file_size);
            request = request.header("Range", format!("bytes={}-", file_size));
        }

        // Send the request
        let response = request.send().await.context("Failed to send GET request")?;

        // Log response status and headers for troubleshooting
        info!("Response status: {}", response.status());
        info!("Response headers: {:#?}", response.headers());

        // Handle different status codes for range requests
        let status = response.status();
        match status {
            StatusCode::PARTIAL_CONTENT => {
                // 206 Partial Content: Server accepted the range request
                info!("Server accepted range request with 206 Partial Content");
                // Process the response with the knowledge that we're resuming
                // We need to add the existing file size to the progress
                self.process_download_response(response, file, output_path, remote_size)
                    .await
                    .map(|path| {
                        // For 206 responses, we've already accounted for the existing file size in process_download_response
                        info!("Resumed download completed successfully");
                        path
                    })
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                // 416 Range Not Satisfiable: Requested range is invalid
                warn!("Range request rejected with 416 Range Not Satisfiable");
                // This should be rare now that we check metadata first, but handle it just in case
                // Start a new download from the beginning
                file = tokio::fs::File::create(&output_path)
                    .await
                    .context("Failed to create new output file after range rejection")?;
                // Get a new response for the full file
                let new_response = self
                    .client
                    .get(url)
                    .send()
                    .await
                    .context("Failed to send new GET request after range rejection")?;
                // Check if the new request was successful
                if !new_response.status().is_success() {
                    return Err(anyhow!(
                        "Failed to download file after range rejection: {}",
                        new_response.status()
                    ));
                }
                // Use the new response for the rest of the function
                self.process_download_response(new_response, file, output_path, remote_size)
                    .await
            }
            StatusCode::OK => {
                // 200 OK: Server doesn't support range requests or ignored our range header
                // This should be rare now that we check for range support, but handle it anyway
                if file_exists && file_size > 0 && supports_range {
                    warn!("Server returned 200 OK instead of 206 Partial Content despite reporting range support.");
                    // We need to start the download from the beginning
                    file = tokio::fs::File::create(&output_path)
                        .await
                        .context("Failed to create new output file for complete download")?;
                }
                // Continue with the download from the beginning
                self.process_download_response(response, file, output_path, remote_size)
                    .await
            }
            _ => {
                // Any other status code is an error
                Err(anyhow!("Unexpected response status: {}", status))
            }
        }
    }

    /// Process a download response stream and save it to a file
    /// This is used when we need to restart a download with a fresh request
    async fn process_download_response(
        &self,
        response: reqwest::Response,
        mut file: tokio::fs::File,
        output_path: PathBuf,
        known_content_length: Option<u64>,
    ) -> Result<PathBuf> {
        // Log response status and headers
        info!("Processing new response with status: {}", response.status());
        info!("Response headers: {:#?}", response.headers());

        if !response.status().is_success() && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!("Unexpected response status: {}", response.status()));
        }

        // Get the file name for progress reporting
        let file_name = output_path
            .file_name()
            .context("Failed to get filename from path")?
            .to_string_lossy();

        // Check if we're resuming (status is 206 Partial Content)
        let is_resuming = response.status() == StatusCode::PARTIAL_CONTENT;

        // Get file size if we're resuming
        let file_size = if is_resuming {
            tokio::fs::metadata(&output_path).await?.len()
        } else {
            0
        };

        // Get content length from the response
        let content_length = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        // Calculate total size
        let total_size = if is_resuming && file_size > 0 {
            // For resumed downloads, add existing file size to content length
            let cl = content_length.context("Missing Content-Length in 206 response")?;
            info!(
                "Resuming download, adding existing file size {} to content length {}",
                file_size, cl
            );
            file_size + cl
        } else {
            // For new downloads, use the content length from the response
            // or fall back to the known content length from HEAD request
            content_length.or(known_content_length).unwrap_or(0)
        };

        // Set up progress bar
        let progress_bar = ProgressBar::new(total_size);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );

        // Set initial position if resuming
        let mut downloaded = if is_resuming && file_size > 0 {
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
