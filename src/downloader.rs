use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use reqwest::StatusCode;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

/// A robust file downloader that supports resumable downloads
pub struct Downloader {
    client: Client,
}

impl Downloader {
    /// Creates a new downloader instance
    pub fn new() -> Self {
        Downloader {
            client: Client::new(),
        }
    }

    /// Fetches metadata about a remote file before downloading
    ///
    /// Returns:
    /// - The total file size (if available)
    /// - Whether the server supports range requests for resumable downloads
    async fn fetch_remote_file_metadata(&self, url: &str) -> Result<(Option<u64>, bool)> {
        // Use a GET request with a minimal range instead of HEAD request
        // This has better compatibility with servers that reject HEAD requests
        let response = self
            .client
            .get(url)
            .header("Range", "bytes=0-0") // Request just the first byte
            .send()
            .await
            .context("Failed to send GET request for metadata")?;

        let status = response.status();

        // 206 Partial Content indicates the server supports range requests
        let supports_range = status == StatusCode::PARTIAL_CONTENT;

        // Extract content length based on response type
        let content_length = if supports_range {
            self.extract_size_from_content_range(&response)
        } else if status.is_success() {
            self.extract_size_from_content_length(&response)
        } else {
            return Err(anyhow!("Request failed with status: {}", status));
        };

        info!(
            "File metadata - Content length: {:?}, Supports range: {}",
            content_length, supports_range
        );

        Ok((content_length, supports_range))
    }

    /// Extracts total file size from Content-Range header
    /// Format is typically "bytes 0-0/1234" where 1234 is the total size
    fn extract_size_from_content_range(&self, response: &reqwest::Response) -> Option<u64> {
        response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.split('/')
                    .nth(1)
                    .and_then(|size| size.parse::<u64>().ok())
            })
    }

    /// Extracts file size from Content-Length header
    fn extract_size_from_content_length(&self, response: &reqwest::Response) -> Option<u64> {
        response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    }

    /// Downloads a file from a URL and saves it to the specified directory
    ///
    /// Features:
    /// - Automatic resume of partial downloads when possible
    /// - Progress tracking with ETA
    /// - Handles server quirks and edge cases
    pub async fn download<P: AsRef<Path>>(&self, url: &str, output_dir: P) -> Result<PathBuf> {
        // Extract filename from URL and create full output path
        let (file_name, output_path) = self.prepare_output_path(url, output_dir)?;

        // Check if file exists to determine if we're resuming
        let (file_exists, file_size) = self.check_existing_file(&output_path).await?;

        // Get metadata about the remote file
        let (remote_size, supports_range) = self.fetch_remote_file_metadata(url).await?;

        // Log download start/resume status
        self.log_download_start(&file_name, &output_path, file_exists, file_size);

        // Check if file is already complete
        if self.is_download_complete(file_exists, file_size, remote_size) {
            info!("File is already complete or larger, skipping download");
            return Ok(output_path);
        }

        // Open file for writing (either new or append mode)
        let file = self
            .open_output_file(&output_path, file_exists, file_size, supports_range)
            .await?;

        // Create and send the HTTP request
        let request = self.build_download_request(url, file_exists, file_size, supports_range);
        let response = request.send().await.context("Failed to send GET request")?;

        // Log response details for troubleshooting
        self.log_response_details(&response);

        // Process the download based on the response status
        self.handle_download_response(response, file, output_path, remote_size, file_size)
            .await
    }

    /// Prepares the output path for the downloaded file
    fn prepare_output_path<P: AsRef<Path>>(
        &self,
        url: &str,
        output_dir: P,
    ) -> Result<(String, PathBuf)> {
        let file_name = url
            .split('/')
            .next_back()
            .context("Failed to determine file name from URL")?
            .to_string();

        let output_path = output_dir.as_ref().join(&file_name);

        Ok((file_name, output_path))
    }

    /// Checks if a file already exists and returns its size
    async fn check_existing_file(&self, path: &Path) -> Result<(bool, u64)> {
        let file_exists = path.exists();
        let file_size = if file_exists {
            tokio::fs::metadata(path).await?.len()
        } else {
            0
        };

        Ok((file_exists, file_size))
    }

    /// Logs information about the download start/resume
    fn log_download_start(
        &self,
        file_name: &str,
        output_path: &Path,
        file_exists: bool,
        file_size: u64,
    ) {
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
    }

    /// Checks if the download is already complete
    fn is_download_complete(
        &self,
        file_exists: bool,
        file_size: u64,
        remote_size: Option<u64>,
    ) -> bool {
        if let Some(remote_size) = remote_size {
            file_exists && file_size == remote_size
        } else {
            false
        }
    }

    /// Opens the output file in the appropriate mode (create or append)
    async fn open_output_file(
        &self,
        output_path: &Path,
        file_exists: bool,
        file_size: u64,
        supports_range: bool,
    ) -> Result<tokio::fs::File> {
        if file_exists && file_size > 0 && supports_range {
            tokio::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(output_path)
                .await
                .context("Failed to open existing file for resuming download")
        } else {
            tokio::fs::File::create(output_path)
                .await
                .context("Failed to create output file")
        }
    }

    /// Builds the HTTP request for downloading, adding Range header if resuming
    fn build_download_request(
        &self,
        url: &str,
        file_exists: bool,
        file_size: u64,
        supports_range: bool,
    ) -> reqwest::RequestBuilder {
        let mut request = self.client.get(url);

        if file_exists && file_size > 0 && supports_range {
            info!("Resuming from byte position {}", file_size);
            request = request.header("Range", format!("bytes={}-", file_size));
        }

        request
    }

    /// Logs details about the HTTP response
    fn log_response_details(&self, response: &reqwest::Response) {
        info!("Response status: {}", response.status());
        info!("Response headers: {:#?}", response.headers());
    }

    /// Handles the download response based on the status code
    async fn handle_download_response(
        &self,
        response: reqwest::Response,
        file: tokio::fs::File,
        output_path: PathBuf,
        remote_size: Option<u64>,
        file_size: u64,
    ) -> Result<PathBuf> {
        let status = response.status();
        let url = response.url().to_string();

        match status {
            StatusCode::PARTIAL_CONTENT => {
                // 206 Partial Content: Server accepted the range request
                info!("Server accepted range request with 206 Partial Content");
                self.process_download_stream(
                    response,
                    file,
                    output_path,
                    remote_size,
                    true,
                    file_size,
                )
                .await
                .inspect(|_| {
                    info!("Resumed download completed successfully");
                })
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                // 416 Range Not Satisfiable: Range is invalid
                warn!("Range request rejected with 416 Range Not Satisfiable");
                self.restart_download(&url, &output_path, remote_size).await
            }
            StatusCode::OK => {
                // 200 OK: Server doesn't support range or ignored range header
                if file_size > 0 {
                    warn!("Server returned 200 OK instead of 206 Partial Content despite reporting range support");
                    // Start from beginning since server ignored our range request
                    self.restart_download(&url, &output_path, remote_size).await
                } else {
                    // Normal download from beginning
                    self.process_download_stream(response, file, output_path, remote_size, false, 0)
                        .await
                }
            }
            _ => {
                // Any other status code is an error
                Err(anyhow!("Unexpected response status: {}", status))
            }
        }
    }

    /// Restarts a download from the beginning
    async fn restart_download(
        &self,
        url: &str,
        output_path: &Path,
        remote_size: Option<u64>,
    ) -> Result<PathBuf> {
        // Create a new file from scratch
        let file = tokio::fs::File::create(output_path)
            .await
            .context("Failed to create new output file for restart")?;

        // Get a new response without range header
        let new_response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to send new GET request after restart")?;

        // Check if successful
        if !new_response.status().is_success() {
            return Err(anyhow!(
                "Failed to download file after restart: {}",
                new_response.status()
            ));
        }

        // Process the new download from beginning
        self.process_download_stream(
            new_response,
            file,
            output_path.to_path_buf(),
            remote_size,
            false,
            0,
        )
        .await
    }

    /// Processes the download response stream and saves it to a file
    async fn process_download_stream(
        &self,
        response: reqwest::Response,
        mut file: tokio::fs::File,
        output_path: PathBuf,
        known_content_length: Option<u64>,
        is_resuming: bool,
        existing_file_size: u64,
    ) -> Result<PathBuf> {
        // Get file name for progress reporting
        let file_name = output_path
            .file_name()
            .context("Failed to get filename from path")?
            .to_string_lossy();

        // Get content length from response
        let content_length = self.extract_size_from_content_length(&response);

        // Calculate total size
        let total_size = self.calculate_total_download_size(
            is_resuming,
            existing_file_size,
            content_length,
            known_content_length,
        );

        // Set up progress tracking
        let progress_bar = self.create_progress_bar(total_size)?;

        // Set initial position if resuming
        let mut downloaded = if is_resuming && existing_file_size > 0 {
            info!("Continuing download from position: {}", existing_file_size);
            progress_bar.set_position(existing_file_size);
            existing_file_size
        } else {
            0
        };

        // Stream the file contents and save to disk
        downloaded = self
            .stream_file_contents(response, &mut file, progress_bar, downloaded)
            .await?;

        // Log completion
        info!(
            "Completed download of {} ({:.2} MB)",
            file_name,
            downloaded as f64 / 1_048_576.0
        );

        Ok(output_path)
    }

    /// Calculates the total download size including already downloaded bytes
    fn calculate_total_download_size(
        &self,
        is_resuming: bool,
        file_size: u64,
        content_length: Option<u64>,
        known_content_length: Option<u64>,
    ) -> u64 {
        if is_resuming && file_size > 0 {
            // For resumed downloads, add existing file size to content length
            if let Some(cl) = content_length {
                info!(
                    "Resuming download, adding existing file size {} to content length {}",
                    file_size, cl
                );
                file_size + cl
            } else {
                file_size + known_content_length.unwrap_or(0)
            }
        } else {
            // For new downloads, use content length or fallback
            content_length.or(known_content_length).unwrap_or(0)
        }
    }

    /// Creates a progress bar for tracking download progress
    fn create_progress_bar(&self, total_size: u64) -> Result<ProgressBar> {
        let progress_bar = ProgressBar::new(total_size);
        progress_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
                .progress_chars("#>-"),
        );
        Ok(progress_bar)
    }

    /// Streams file contents from the HTTP response to the local file
    async fn stream_file_contents(
        &self,
        response: reqwest::Response,
        file: &mut tokio::fs::File,
        progress_bar: ProgressBar,
        initial_position: u64,
    ) -> Result<u64> {
        let mut downloaded = initial_position;
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
                    progress_bar.length().unwrap_or(0) as f64 / 1_048_576.0
                );
            }
        }

        // Get filename from progress bar message or use "file" as fallback
        let message = progress_bar.message();
        let file_name = if message.is_empty() { "file" } else { &message };

        progress_bar.finish_with_message(format!("Downloaded {} successfully", file_name));
        Ok(downloaded)
    }
}
