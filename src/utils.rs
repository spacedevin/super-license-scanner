use sha2::{ Sha256, Digest };
use std::fs;
use std::path::{ Path, PathBuf };
use std::io::{ Read, Write };
use crate::package::Package; // Updated import
use std::error::Error;

// List of common license file patterns
pub const LICENSE_FILE_PATTERNS: [&str; 9] = [
    "LICENSE",
    "LICENSE.txt",
    "LICENSE.md",
    "License",
    "License.txt",
    "License.md",
    "license",
    "COPYING",
    "COPYING.txt",
];

pub fn generate_package_hash(package: &Package) -> String {
    let mut hasher = Sha256::new();

    // Create a string that uniquely identifies a package
    let package_id = if
        package.name.starts_with("github:") ||
        package.resolution.contains("github:")
    {
        // For GitHub packages, use the name and resolution
        format!("github:{}/{}", package.name, package.resolution)
    } else if package.resolution.contains("__archiveUrl=") {
        // For packages with archive URLs, extract the URL
        if let Some(archive_url_index) = package.resolution.find("__archiveUrl=") {
            let archive_url = &package.resolution[archive_url_index + 13..];
            format!("url:{}", archive_url)
        } else {
            format!("npm:{}@{}", package.name, package.version)
        }
    } else {
        // For npm packages, use name + version
        format!("npm:{}@{}", package.name, package.version)
    };

    hasher.update(package_id.as_bytes());
    let result = hasher.finalize();

    format!("{:x}", result)
}

/// Generate a fallback checksum for a package when none is provided
pub fn generate_fallback_checksum(package: &Package) -> String {
    let mut hasher = Sha256::new();

    // Build a string containing registry, organization, repo, and version
    // Start with the name (which might include org/repo)
    let mut id_parts = Vec::new();

    // Add registry info
    let registry = if package.registry.is_empty() {
        if package.name.starts_with("github:") || package.resolution.contains("github:") {
            "github"
        } else {
            "npm"
        }
    } else {
        &package.registry
    };
    id_parts.push(registry);

    // Add name parts (split by / to get org and repo if available)
    let name_parts: Vec<&str> = package.name.split('/').collect();
    for part in name_parts {
        id_parts.push(part);
    }

    // Add version
    id_parts.push(&package.version);

    // Join all parts and hash
    let id_string = id_parts.join(":");
    hasher.update(id_string.as_bytes());

    // Format the result as a base64-like string similar to yarn's format
    let hash = hasher.finalize();
    format!("fallback:{:x}", hash)
}

// Initialize cache directory
pub fn init_cache_dir() -> Result<PathBuf, Box<dyn Error>> {
    let cache_dir = Path::new(".").join(".cache");

    // Create cache directory if it doesn't exist
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir)?;
        println!("Created cache directory at: {}", cache_dir.display());
    }

    Ok(cache_dir)
}

// Save package info to cache
pub fn save_to_cache(package_hash: &str, package_info: &Package) -> Result<(), Box<dyn Error>> {
    let cache_dir = init_cache_dir()?;
    let cache_file = cache_dir.join(format!("{}.json", package_hash));

    // Serialize the package info to JSON
    let json_content = serde_json::to_string(package_info)?;

    // Write to cache file
    let mut file = fs::File::create(&cache_file)?;
    file.write_all(json_content.as_bytes())?;

    Ok(())
}

// Try to get package info from cache
pub fn get_from_cache(package_hash: &str) -> Option<Package> {
    let cache_dir = match init_cache_dir() {
        Ok(dir) => dir,
        Err(_) => {
            return None;
        }
    };

    let cache_file = cache_dir.join(format!("{}.json", package_hash));

    if !cache_file.exists() {
        return None;
    }

    // Read cache file
    let mut file = match fs::File::open(&cache_file) {
        Ok(file) => file,
        Err(_) => {
            return None;
        }
    };

    let mut content = String::new();
    if let Err(_) = file.read_to_string(&mut content) {
        return None;
    }

    // Deserialize the package info from JSON - Fix: Add type annotation for Package
    match serde_json::from_str::<Package>(&content) {
        Ok(mut package_info) => {
            // Always reset the retry_for_unknown flag when loading from cache
            // It will be set again if needed by the caller
            package_info.retry_for_unknown = false;
            Some(package_info)
        }
        Err(_) => None,
    }
}

// Format repo URL with appropriate license file if it exists
pub fn get_license_file_url(repo_url: &str, branch_or_commit: &str) -> Option<String> {
    // This function makes HTTP requests to check if license files exist
    let client = reqwest::blocking::Client
        ::builder()
        .timeout(std::time::Duration::from_secs(5)) // Add timeout to avoid long waits
        .build()
        .unwrap_or_default();

    // For GitHub repositories, try the API
    if repo_url.contains("github.com") {
        // Extract owner and repo from URL
        let parts: Vec<&str> = repo_url.split('/').collect();
        if parts.len() >= 5 {
            let owner = parts[3];
            let repo = parts[4];

            // Try to get the repository contents for each license pattern
            for pattern in LICENSE_FILE_PATTERNS.iter() {
                let api_path = format!(
                    "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
                    owner,
                    repo,
                    pattern,
                    branch_or_commit
                );

                match client.get(&api_path).header("User-Agent", "Dependency-Scanner").send() {
                    Ok(response) => {
                        if response.status().is_success() {
                            return Some(
                                format!("{}/blob/{}/{}", repo_url, branch_or_commit, pattern)
                            );
                        }
                    }
                    Err(_) => {
                        // If we hit rate limits or network errors, don't keep trying
                        break;
                    }
                }
            }
        }
    }

    // If we couldn't verify any license files, return a generic LICENSE link
    // as a fallback, since it's the most common name
    Some(format!("{}/blob/{}/LICENSE", repo_url, branch_or_commit))
}

// Normalize GitHub URL to a standard format
pub fn normalize_github_url(url: &str) -> Option<String> {
    if url.contains("github.com") {
        let url = url.replace("git+", "").replace("git://", "https://").replace(".git", "");

        // Extract owner and repo
        let parts: Vec<&str> = url.split('/').collect();
        if url.starts_with("https://github.com/") && parts.len() >= 5 {
            return Some(format!("https://github.com/{}/{}", parts[3], parts[4]));
        }
    }
    None
}
