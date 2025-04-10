use reqwest::blocking::Client;
use serde_json::Value;
use std::error::Error;
use urlencoding::encode;

use crate::package::Package;

pub fn get_package_info(package: &Package) -> Result<Package, Box<dyn Error>> {
    let client = Client::new();

    // For scoped packages (starting with @), we need to handle them specially
    let package_name = &package.name;
    let version = &package.version;

    // Custom package sources (GitHub, etc.)
    if package_resolution_is_github(&package.resolution) {
        // Even for GitHub packages, try npm first since many are published there
        match try_npm_registry(package_name, version, &client) {
            Ok(Some(npm_package)) => {
                eprintln!("INFO: GitHub package {} found in npm registry", package_name);
                return Ok(npm_package);
            }
            Ok(None) => {
                eprintln!("INFO: GitHub package {} not found in npm, redirecting to GitHub API", package_name);
                return crate::github_api::get_package_info(package);
            }
            Err(e) => {
                eprintln!(
                    "INFO: Error checking npm registry for GitHub package {}: {}",
                    package_name,
                    e
                );
                return crate::github_api::get_package_info(package);
            }
        }
    }

    // Check if the resolution is an archive that needs to be downloaded and extracted
    if crate::archive_handler::is_archive_url(&package.resolution) {
        // Try npm registry first before downloading and extracting the archive
        match try_npm_registry(package_name, version, &client) {
            Ok(Some(npm_package)) => {
                eprintln!("INFO: Archive package {} found in npm registry", package_name);
                return Ok(npm_package);
            }
            Ok(None) => {
                eprintln!("INFO: Archive package {} not found in npm, downloading and extracting", package_name);
                return extract_info_from_archive(package);
            }
            Err(e) => {
                eprintln!(
                    "INFO: Error checking npm registry for archive package {}: {}",
                    package_name,
                    e
                );
                return extract_info_from_archive(package);
            }
        }
    }

    // Extract actual npm package name from resolution if needed
    let _clean_name = extract_npm_package_name(&package.resolution, package_name);

    // Handle package resolution specially
    if package_name.starts_with("resolution: \"") {
        eprintln!("INFO: Skipping resolution entry: {}", package_name);
        let mut result = Package::new(
            package_name.clone(), // Keep original name
            version.clone(),
            package.resolution.clone(),
            package.checksum.clone()
        );

        result.registry = "npm".to_string();
        result.license = "UNKNOWN".to_string();
        result.debug_info = Some("Entry is a resolution definition, not a package".to_string());
        result.processed = true;

        return Ok(result);
    }

    // Clean up the package name to properly handle scoped packages
    let clean_name = package_name.trim_matches(|c| (c == '"' || c == '\'' || c == ' '));

    // Create package URL
    let package_url = format!("https://www.npmjs.com/package/{}", clean_name);

    // Properly encode the package name for URL usage
    // For scoped packages (@org/name), we need special handling
    let encoded_name = if clean_name.starts_with('@') {
        // The @ symbol must be encoded as %40, and the / as %2F
        clean_name.replace('@', "%40").replace('/', "%2F")
    } else {
        encode(clean_name).to_string()
    };

    // Construct npm registry URL to fetch package metadata
    // Use the npm registry's public API endpoint format
    let registry_url = format!("https://registry.npmjs.org/{}", encoded_name);

    eprintln!("DEBUG: Fetching from npm registry: {}", registry_url);

    // Try to get the package info
    let response = match
        client
            .get(&registry_url)
            .header("Accept", "application/json")
            .header("User-Agent", "Dependency-Scanner/1.0")
            .send()
    {
        Ok(resp) => resp,
        Err(e) => {
            let error_msg = format!("Network error when contacting npm registry: {}", e);
            eprintln!("INFO: npm registry request failed for {}: {}", clean_name, error_msg);

            let mut result = Package::new(
                clean_name.to_string(),
                version.clone(),
                package.resolution.clone(),
                package.checksum.clone()
            );

            result.registry = "npm".to_string();
            result.display_name = format!("{}@{}", clean_name, version);
            result.license = "UNKNOWN".to_string();
            result.url = package_url;
            result.debug_info = Some(error_msg);
            result.processed = true;

            return Ok(result);
        }
    };

    if !response.status().is_success() {
        let status_code = response.status().as_u16();
        let reason = response.status().canonical_reason().unwrap_or("Unknown error");
        let error_msg = format!("npm registry returned status code {}: {}", status_code, reason);

        eprintln!("INFO: {}", error_msg);

        let mut result = Package::new(
            clean_name.to_string(),
            version.clone(),
            package.resolution.clone(),
            package.checksum.clone()
        );

        result.registry = "npm".to_string();
        result.display_name = format!("{}@{}", clean_name, version);
        result.license = "UNKNOWN".to_string();
        result.url = package_url;
        result.debug_info = Some(error_msg);
        result.processed = true;

        return Ok(result);
    }

    // Try to parse the response
    let package_metadata: Value = match response.json() {
        Ok(json) => json,
        Err(e) => {
            let error_msg = format!("Failed to parse JSON from npm registry: {}", e);
            eprintln!("INFO: {}", error_msg);

            let mut result = Package::new(
                clean_name.to_string(),
                version.clone(),
                package.resolution.clone(),
                package.checksum.clone()
            );

            result.registry = "npm".to_string();
            result.display_name = format!("{}@{}", clean_name, version);
            result.license = "UNKNOWN".to_string();
            result.url = package_url;
            result.debug_info = Some(error_msg);
            result.processed = true;

            return Ok(result);
        }
    };

    // Extract license information from the latest version
    // or specific version if available
    let (license, license_debug) = extract_license_info_with_debug(&package_metadata, version);

    // Try to extract license URL if available
    let license_url = extract_license_url(&package_metadata, &license);

    // Extract dependencies
    let dependencies = extract_dependencies(&package_metadata, version);

    // Store license value for comparison
    let is_unknown = license == "UNKNOWN";

    let mut result_package = Package::new(
        clean_name.to_string(),
        version.clone(),
        package.resolution.clone(),
        package.checksum.clone()
    );

    result_package.registry = "npm".to_string();
    result_package.display_name = format!("{}@{}", clean_name, version);
    result_package.license = license.clone();
    result_package.url = package_url;
    result_package.license_url = license_url;
    result_package.debug_info = if is_unknown { Some(license_debug.clone()) } else { None };

    // When license is unknown but we have a license URL, try to download and detect license
    if is_unknown && result_package.license_url.is_some() {
        match try_detect_license_from_url(result_package.license_url.as_ref().unwrap()) {
            Ok(Some(detected_license)) => {
                result_package.license = detected_license;
                result_package.debug_info = Some(
                    format!(
                        "License detected from URL: {}",
                        result_package.license_url.as_ref().unwrap()
                    )
                );
            }
            Ok(None) => {
                // License couldn't be detected, but we attempted
                result_package.debug_info = Some(
                    format!(
                        "{}; Attempted license detection from URL: {}",
                        license_debug,
                        result_package.license_url.as_ref().unwrap()
                    )
                );
            }
            Err(e) => {
                // Error while trying to download license
                result_package.debug_info = Some(
                    format!(
                        "{}; Failed to download license from URL: {} ({})",
                        license_debug,
                        result_package.license_url.as_ref().unwrap(),
                        e
                    )
                );
            }
        }
    }

    result_package.dependencies = dependencies;
    result_package.processed = true;

    Ok(result_package)
}

// Updated to return both license info and debug message
fn extract_license_info_with_debug(
    package_metadata: &Value,
    requested_version: &str
) -> (String, String) {
    let mut debug_info = Vec::new();

    // First check if the specific version has license info
    if let Some(versions) = package_metadata["versions"].as_object() {
        // Try the exact requested version first
        if let Some(version_data) = versions.get(requested_version) {
            if let Some(license) = version_data["license"].as_str() {
                // Use license_detection to normalize license ID
                return (crate::license_detection::normalize_license_id(license), String::new());
            } else {
                debug_info.push(format!("No license field in version {}", requested_version));
            }

            if let Some(licenses) = version_data["licenses"].as_array() {
                if let Some(first_license) = licenses.first() {
                    if let Some(license_type) = first_license["type"].as_str() {
                        // Use license_detection to normalize license ID
                        return (
                            crate::license_detection::normalize_license_id(license_type),
                            String::new(),
                        );
                    }
                } else {
                    debug_info.push("Licenses array is empty in package metadata ".to_string());
                }
            } else {
                debug_info.push("No licenses array in package metadata ".to_string());
            }
        } else {
            debug_info.push(
                format!("Requested version {} not found in package metadata ", requested_version)
            );
        }

        // If requested version not found, try the latest version
        if let Some(latest_version) = package_metadata["dist-tags"]["latest"].as_str() {
            if let Some(latest_data) = versions.get(latest_version) {
                if let Some(license) = latest_data["license"].as_str() {
                    // Use license_detection to normalize license ID
                    return (crate::license_detection::normalize_license_id(license), String::new());
                }

                if let Some(licenses) = latest_data["licenses"].as_array() {
                    if let Some(first_license) = licenses.first() {
                        if let Some(license_type) = first_license["type"].as_str() {
                            // Use license_detection to normalize license ID
                            return (
                                crate::license_detection::normalize_license_id(license_type),
                                String::new(),
                            );
                        }
                    }
                }
            }
            debug_info.push(format!("Could not find license in latest version {}", latest_version));
        } else {
            debug_info.push("No latest version tag found ".to_string());
        }
    } else {
        debug_info.push("No versions field in package metadata ".to_string());
    }

    // As a fallback, check the top-level license field
    if let Some(license) = package_metadata["license"].as_str() {
        // Use license_detection to normalize license ID
        return (crate::license_detection::normalize_license_id(license), String::new());
    } else {
        debug_info.push("No top-level license field in package metadata ".to_string());
    }

    // Check top-level licenses array
    if let Some(licenses) = package_metadata["licenses"].as_array() {
        if let Some(first_license) = licenses.first() {
            if let Some(license_type) = first_license["type"].as_str() {
                // Use license_detection to normalize license ID
                return (
                    crate::license_detection::normalize_license_id(license_type),
                    String::new(),
                );
            }
        }
        debug_info.push("Invalid format in top-level licenses array ".to_string());
    } else {
        debug_info.push("No top-level licenses array in package metadata ".to_string());
    }

    // If no license information found
    ("UNKNOWN".to_string(), debug_info.join("; "))
}

// Extract license URL from package metadata if available
fn extract_license_url(package_metadata: &Value, license: &str) -> Option<String> {
    // First try to get URL from standard license URL mapping
    if let Some(url) = crate::license_urls::get_license_url(license) {
        return Some(url);
    }

    // Try to find a license URL in the package metadata
    if
        let Some(license_url) = package_metadata["license_url"]
            .as_str()
            .or_else(|| package_metadata["licenseUrl"].as_str())
    {
        return Some(license_url.to_string());
    }

    // Check for URLs in package.json's license object (some packages use this format)
    if let Some(license_obj) = package_metadata["license"].as_object() {
        if let Some(url) = license_obj.get("url").and_then(|u| u.as_str()) {
            return Some(url.to_string());
        }
    }

    // Try to get license URL from the metadata
    if let Some(homepage) = package_metadata["homepage"].as_str() {
        if homepage.contains("github.com") {
            if let Some(normalized_url) = crate::utils::normalize_github_url(homepage) {
                // Try to determine the default branch
                let default_branch = "master"; // Normally we would determine this from API
                return crate::utils::get_license_file_url(&normalized_url, default_branch);
            }
        }
    }

    // If repository URL exists and it's GitHub, construct a likely license URL
    if let Some(repo) = package_metadata["repository"].as_object() {
        if let Some(url) = repo["url"].as_str() {
            if url.contains("github.com") {
                if let Some(normalized_url) = crate::utils::normalize_github_url(url) {
                    // Try to determine the default branch
                    let default_branch = "master"; // Normally we would determine this from API
                    return crate::utils::get_license_file_url(&normalized_url, default_branch);
                }
            }
        }
    }

    None
}

fn extract_dependencies(package_metadata: &Value, requested_version: &str) -> Vec<Package> {
    let mut dependencies = Vec::new();

    // Try to find the appropriate version's dependencies
    let version_data = if let Some(versions) = package_metadata["versions"].as_object() {
        if let Some(version) = versions.get(requested_version) {
            version
        } else if let Some(latest_version) = package_metadata["dist-tags"]["latest"].as_str() {
            versions.get(latest_version).unwrap_or(&Value::Null)
        } else {
            &Value::Null
        }
    } else {
        &Value::Null
    };

    // Process regular dependencies
    if let Some(deps) = version_data["dependencies"].as_object() {
        for (name, version_value) in deps {
            if let Some(version_str) = version_value.as_str() {
                let clean_version = version_str.trim_start_matches('^').trim_start_matches('~');

                let dep = Package::new(
                    name.clone(),
                    clean_version.to_string(),
                    if version_str.starts_with("github:") {
                        format!("https://github.com/{}", version_str.trim_start_matches("github:"))
                    } else {
                        format!(
                            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                            name,
                            name.replace('@', "").replace('/', "-"),
                            clean_version
                        )
                    },
                    None
                );

                dependencies.push(dep);
            }
        }
    }

    dependencies
}

// Add this function to handle archives
fn extract_info_from_archive(package: &Package) -> Result<Package, Box<dyn Error>> {
    let package_name = &package.name;
    let version = &package.version;
    let resolution = &package.resolution;

    match crate::archive_handler::extract_info_from_archive(resolution) {
        Ok((license, license_content)) => {
            let mut result = Package::new(
                package_name.clone(),
                version.clone(),
                resolution.clone(),
                package.checksum.clone()
            );

            result.registry = "npm".to_string();
            result.display_name = format!("{}@{}", package_name, version);
            result.license = license.clone();
            result.url = format!("https://www.npmjs.com/package/{}", package_name);
            result.debug_info = if license == "UNKNOWN" {
                Some(format!("License extracted from archive: {}", resolution))
            } else {
                None
            };

            if let Some(content) = license_content {
                if license == "UNKNOWN" {
                    let preview: String = content.chars().take(100).collect();
                    result.debug_info = Some(
                        format!("License file found but type unknown. Preview: {}...", preview)
                    );
                }
            }

            result.processed = true;

            Ok(result)
        }
        Err(e) => {
            Ok(
                Package::with_error(
                    package_name.clone(),
                    version.clone(),
                    "npm",
                    format!("https://www.npmjs.com/package/{}", package_name),
                    &format!("Failed to extract from archive: {}", e)
                )
            )
        }
    }
}

// New function to download license text and detect license
pub fn try_detect_license_from_url(url: &str) -> Result<Option<String>, Box<dyn Error>> {
    let client = reqwest::blocking::Client
        ::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = client.get(url).send()?;

    if !response.status().is_success() {
        return Err(format!("Failed to download license: HTTP status {}", response.status()).into());
    }

    let license_text = response.text()?;

    let detected_license = crate::license_detection::detect_license_from_text(&license_text);

    Ok(detected_license)
}

// Helper function to determine if package uses GitHub as source
fn package_resolution_is_github(resolution: &str) -> bool {
    resolution.contains("github:") ||
        resolution.contains("github.com") ||
        (resolution.contains("__archiveUrl=") && resolution.contains("github.com"))
}

// Helper function to extract npm package name from resolution
fn extract_npm_package_name(resolution: &str, fallback_name: &str) -> String {
    if resolution.contains("@npm:") {
        if let Some(npm_pos) = resolution.find("@npm:") {
            return resolution[..npm_pos].to_string();
        }
    }

    fallback_name.to_string()
}

// Helper function to try getting package info from npm registry first
pub fn try_npm_registry(
    package_name: &str,
    version: &str,
    client: &Client
) -> Result<Option<Package>, Box<dyn Error>> {
    let clean_name = package_name.trim_matches(|c| (c == '"' || c == '\'' || c == ' '));

    let npm_name = if clean_name.starts_with("github:") {
        let parts: Vec<&str> = clean_name.trim_start_matches("github:").split('/').collect();
        if parts.len() >= 2 {
            parts[1].to_string()
        } else {
            clean_name.to_string()
        }
    } else {
        clean_name.to_string()
    };

    let encoded_name = if npm_name.starts_with('@') {
        npm_name.replace('@', "%40").replace('/', "%2F")
    } else {
        encode(&npm_name).to_string()
    };

    let registry_url = format!("https://registry.npmjs.org/{}", encoded_name);

    eprintln!("DEBUG: Trying npm registry for package: {}", npm_name);

    match client.get(&registry_url).header("Accept", "application/json").send() {
        Ok(response) => {
            if !response.status().is_success() {
                return Ok(None);
            }

            match response.json::<Value>() {
                Ok(metadata) => {
                    let (license, license_debug) = extract_license_info_with_debug(
                        &metadata,
                        version
                    );

                    let license_url = extract_license_url(&metadata, &license);
                    let dependencies = extract_dependencies(&metadata, version);

                    let mut result = Package::new(
                        clean_name.to_string(),
                        version.to_string(),
                        format!(
                            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                            npm_name,
                            npm_name.replace('@', "").replace('/', "-"),
                            version
                        ),
                        None
                    );

                    result.registry = "npm".to_string();
                    result.display_name = format!("{}@{}", npm_name, version);
                    result.license = license.clone();
                    result.url = format!("https://www.npmjs.com/package/{}", npm_name);
                    result.license_url = license_url;
                    result.debug_info = if license == "UNKNOWN" {
                        Some(license_debug)
                    } else {
                        None
                    };
                    result.dependencies = dependencies;
                    result.processed = true;

                    Ok(Some(result))
                }
                Err(_) => Ok(None),
            }
        }
        Err(_) => Ok(None),
    }
}
