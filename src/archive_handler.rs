use flate2::read::GzDecoder;
use std::fs::{ self, File };
use std::io::{ self };
use std::path::{ Path, PathBuf };
use tar::Archive;
use tempfile::TempDir;
use zip::ZipArchive;

pub struct ArchiveHandler {
    temp_dir: TempDir,
}

impl ArchiveHandler {
    /// Create a new archive handler with a temporary directory
    pub fn new() -> Result<Self, io::Error> {
        let temp_dir = TempDir::new()?;
        Ok(ArchiveHandler { temp_dir })
    }

    /// Return the path to the temporary directory
    #[allow(dead_code)]
    pub fn temp_dir_path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Download with retry logic
    fn download_with_retry(
        &self,
        url: &str,
        max_retries: usize
    ) -> Result<Vec<u8>, reqwest::Error> {
        let client = reqwest::blocking::Client
            ::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let mut retries = 0;
        let mut last_error = None;

        while retries < max_retries {
            match client.get(url).send() {
                Ok(response) => {
                    if response.status().is_success() {
                        return response.bytes().map(|b| b.to_vec());
                    }

                    // If we got a 429 Too Many Requests, wait longer before retrying
                    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        std::thread::sleep(
                            std::time::Duration::from_secs(5 * ((retries + 1) as u64))
                        );
                    }
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }

            retries += 1;
            std::thread::sleep(std::time::Duration::from_secs(1 * (retries as u64)));
        }

        // The issue being fixed: If all retries fail but none returned an actual error,
        // we still need to return some kind of error.
        // For example, if all responses were 404 or 500 status codes.
        match last_error {
            Some(e) => Err(e),
            None => {
                // Create a simple request that will fail and use that error
                // This ensures we always return a reqwest::Error
                let err = client
                    .get("invalid://example.com")
                    .send()
                    .expect_err("Expected error request to fail");

                Err(err)
            }
        }
    }

    /// Download and extract an archive based on its URL
    pub fn download_and_extract(&self, url: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        println!("Downloading archive from: {}", url);

        // Download with retry logic
        let content = self.download_with_retry(url, 3)?;

        // Determine archive type from URL and extract accordingly
        if url.ends_with(".zip") {
            self.extract_zip(&content)
        } else if url.ends_with(".tar.gz") || url.ends_with(".tgz") {
            self.extract_tar_gz(&content)
        } else {
            Err("Unsupported archive format".into())
        }
    }

    // Extract a zip archive
    fn extract_zip(&self, content: &[u8]) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Create a temporary file to hold the zip data
        let temp_file_path = self.temp_dir.path().join("archive.zip");
        let mut temp_file = File::create(&temp_file_path)?;

        // Fix: Add explicit type annotation for content
        std::io::copy(&mut std::io::Cursor::new(content), &mut temp_file)?;

        // Open the zip file
        let file = File::open(&temp_file_path)?;
        let mut archive = ZipArchive::new(file)?;

        // Directory to extract to
        let extract_dir = self.temp_dir.path().join("extracted");
        fs::create_dir_all(&extract_dir)?;

        // Extract all files
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let outpath = match file.enclosed_name() {
                Some(path) => extract_dir.join(path),
                None => {
                    continue;
                }
            };

            if file.name().ends_with('/') {
                fs::create_dir_all(&outpath)?;
            } else {
                if let Some(p) = outpath.parent() {
                    if !p.exists() {
                        fs::create_dir_all(p)?;
                    }
                }
                let mut outfile = File::create(&outpath)?;
                std::io::copy(&mut file, &mut outfile)?;
            }
        }

        Ok(extract_dir)
    }

    // Extract a tar.gz archive
    fn extract_tar_gz(&self, content: &[u8]) -> Result<PathBuf, Box<dyn std::error::Error>> {
        // Directory to extract to
        let extract_dir = self.temp_dir.path().join("extracted");
        fs::create_dir_all(&extract_dir)?;

        // Decompress the gzip data
        let gz = GzDecoder::new(content);
        let mut archive = Archive::new(gz);

        // Extract all files
        archive.unpack(&extract_dir)?;

        Ok(extract_dir)
    }

    /// Locate package.json in the extracted directory (might be nested)
    pub fn find_package_json(&self, extract_dir: &Path) -> Option<PathBuf> {
        self.find_file(extract_dir, "package.json")
    }

    /// Locate license file in the extracted directory
    pub fn find_license_file(&self, extract_dir: &Path) -> Option<PathBuf> {
        // Try each license pattern in order
        for pattern in &crate::utils::LICENSE_FILE_PATTERNS {
            if let Some(path) = self.find_file(extract_dir, pattern) {
                return Some(path);
            }
        }
        None
    }

    /// Find a file by name in the directory and its subdirectories
    fn find_file(&self, dir: &Path, filename: &str) -> Option<PathBuf> {
        // First check if the file exists in the root directory
        let file_path = dir.join(filename);
        if file_path.exists() {
            return Some(file_path);
        }

        // If not, check in the "package" directory (common in npm tarballs)
        let package_dir = dir.join("package");
        let package_file_path = package_dir.join(filename);
        if package_file_path.exists() {
            return Some(package_file_path);
        }

        // Otherwise, try to find the first package.json in subdirectories
        // Sort entries to make the search deterministic
        if let Ok(entries) = fs::read_dir(dir) {
            let mut subdirs: Vec<_> = entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .collect();

            // Sort directories for deterministic search
            subdirs.sort_by_key(|entry| entry.path());

            // Search in each subdirectory
            for entry in subdirs {
                if let Some(found_path) = self.find_file(&entry.path(), filename) {
                    return Some(found_path);
                }
            }
        }

        // File not found
        None
    }

    /// Read content of a file as string
    pub fn read_file_content(&self, path: &Path) -> Result<String, io::Error> {
        fs::read_to_string(path)
    }
}

/// Extract license info from an archive URL
/// Note: This should be used as a fallback after trying to get info from npm registry
pub fn extract_info_from_archive(
    url: &str
) -> Result<(String, Option<String>), Box<dyn std::error::Error>> {
    // Create a new archive handler
    let handler = ArchiveHandler::new()?;

    // Download and extract the archive
    let extract_dir = handler.download_and_extract(url)?;

    // Try to find package.json
    let mut license = "UNKNOWN".to_string();
    if let Some(package_json_path) = handler.find_package_json(&extract_dir) {
        // Read and parse package.json
        let content = handler.read_file_content(&package_json_path)?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            // Extract license information
            if let Some(lic) = json["license"].as_str() {
                license = crate::license_detection::normalize_license_id(lic);
            }
        }
    }

    // Try to find license file content
    let license_content = if let Some(license_path) = handler.find_license_file(&extract_dir) {
        if let Ok(content) = handler.read_file_content(&license_path) {
            // If license is still unknown, try to detect it from the license file content
            if license == "UNKNOWN" {
                if
                    let Some(detected_license) = crate::license_detection::detect_license_from_text(
                        &content
                    )
                {
                    license = detected_license;
                }
            }
            Some(content)
        } else {
            None
        }
    } else {
        None
    };

    Ok((license, license_content))
}

/// Check if a URL points to an archive that needs special handling
pub fn is_archive_url(url: &str) -> bool {
    url.ends_with(".zip") || url.ends_with(".tar.gz") || url.ends_with(".tgz")
}
