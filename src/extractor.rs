use anyhow::{anyhow, Context, Result};
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tar::Archive;
use tracing::{info, instrument};

pub struct Extractor {}

impl Extractor {
    pub fn new() -> Self {
        Extractor {}
    }

    /// Extract an archive file to the specified directory
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

        info!("Extracting archive");

        if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
            self.extract_tar_gz(path, output_dir.as_ref())
        } else if file_name.ends_with(".tar.lz4") {
            self.extract_tar_lz4(path, output_dir.as_ref())
        } else {
            Err(anyhow!("Unsupported archive format: {}", file_name))
        }
    }

    /// Extract a .tar.gz archive
    #[instrument(skip(self, archive_path, output_dir))]
    fn extract_tar_gz<P: AsRef<Path>, Q: AsRef<Path>>(
        &self,
        archive_path: P,
        output_dir: Q,
    ) -> Result<()> {
        info!("Opening tar.gz archive");
        let file = File::open(archive_path).context("Failed to open .tar.gz archive")?;

        info!("Creating gzip decoder");
        let gz = GzDecoder::new(file);
        let mut archive = Archive::new(gz);

        info!("Unpacking tar archive");
        archive
            .unpack(output_dir)
            .context("Failed to extract .tar.gz archive")?;

        info!("Extraction completed successfully");
        Ok(())
    }

    /// Extract a .tar.lz4 archive
    #[instrument(skip(self, archive_path, output_dir))]
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
        let decoder = lz4::Decoder::new(buf_reader).context("Failed to create LZ4 decoder")?;

        info!("Decompressing LZ4 data (this may take a while for large snapshots)");
        // Instead of loading the entire decompressed data into memory, we can
        // directly pipe the LZ4 decoder to the tar extractor
        let mut archive = Archive::new(decoder);

        info!("Extracting tar archive to directory");
        archive
            .unpack(output_dir)
            .context("Failed to extract tar archive")?;

        info!("Extraction completed successfully");
        Ok(())
    }
}
