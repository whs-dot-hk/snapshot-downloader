use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tar::Archive;
use tracing::{info, instrument};

/// Handles extraction of compressed archive files
pub struct Extractor {}

impl Extractor {
    /// Creates a new extractor instance
    pub fn new() -> Self {
        Extractor {}
    }

    /// Extracts an archive file to the specified directory
    ///
    /// Supports multiple archive formats:
    /// - .tar.gz / .tgz (gzip compressed tar)
    /// - .tar.lz4 (LZ4 compressed tar)
    ///
    /// # Arguments
    /// * `archive_path` - Path to the archive file
    /// * `output_dir` - Directory where contents should be extracted
    #[instrument(skip(self, archive_path, output_dir), fields(file_name = archive_path.as_ref().file_name().and_then(|n| n.to_str())))]
    pub fn extract<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        archive_path: P,
        output_dir: Q,
    ) -> Result<()> {
        let path = archive_path.as_ref();
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("Failed to get archive filename")?;

        info!("Extracting archive: {}", file_name);

        // Determine extraction method based on file extension
        match file_name {
            name if name.ends_with(".tar.gz") || name.ends_with(".tgz") => {
                self.extract_tar_gz(path, output_dir.as_ref())
            }
            name if name.ends_with(".tar.lz4") => self.extract_tar_lz4(path, output_dir.as_ref()),
            _ => Err(anyhow!("Unsupported archive format: {}", file_name)),
        }
    }

    /// Extracts a tar.gz compressed archive
    ///
    /// Uses a streaming approach to minimize memory usage during extraction
    #[instrument(skip(self, archive_path, output_dir), fields(path = %archive_path.as_ref().display()))]
    fn extract_tar_gz<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        archive_path: P,
        output_dir: Q,
    ) -> Result<()> {
        info!("Opening tar.gz archive");
        let file = File::open(archive_path).context("Failed to open .tar.gz archive")?;

        info!("Creating gzip decoder");
        let gz_decoder = GzDecoder::new(file);
        let mut archive = Archive::new(gz_decoder);

        info!("Unpacking tar archive to {}", output_dir.as_ref().display());
        archive
            .unpack(output_dir)
            .context("Failed to extract .tar.gz archive")?;

        info!("Extraction completed successfully");
        Ok(())
    }

    /// Extracts a tar.lz4 compressed archive
    ///
    /// Uses buffered reading and streaming extraction to handle large files efficiently
    #[instrument(skip(self, archive_path, output_dir), fields(path = %archive_path.as_ref().display()))]
    fn extract_tar_lz4<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        archive_path: P,
        output_dir: Q,
    ) -> Result<()> {
        info!("Opening LZ4 compressed file");
        let file = File::open(archive_path).context("Failed to open .tar.lz4 archive")?;

        // Use a BufReader to improve performance with large files
        let buf_reader = BufReader::new(file);

        info!("Creating LZ4 decoder");
        let lz4_decoder = lz4::Decoder::new(buf_reader).context("Failed to create LZ4 decoder")?;

        info!("Decompressing LZ4 data (this may take a while for large archives)");
        // Pipe the LZ4 decoder directly to the tar extractor for memory efficiency
        let mut archive = Archive::new(lz4_decoder);

        info!(
            "Extracting tar archive to {}",
            output_dir.as_ref().display()
        );
        archive
            .unpack(output_dir)
            .context("Failed to extract tar archive")?;

        info!("Extraction completed successfully");
        Ok(())
    }
}
