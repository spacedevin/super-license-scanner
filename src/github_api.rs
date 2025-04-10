use reqwest::blocking::Client;
use serde_json::Value;
use std::error::Error;

use crate::package::Package;
use crate::utils;

pub fn get_package_info(package: &Package) -> Result<Package, Box<dyn Error>> {
    let client = Client::new();

    // First try to find the package on npm registry, since many GitHub packages are published there
    match crate::npm_api::try_npm_registry(&package.name, &package.version, &client) {
        Ok(Some(npm_package)) => {
            eprintln!("INFO: GitHub package {} found in npm registry", package.name);
            return Ok(npm_package);
        }
        Ok(None) => {
            eprintln!("INFO: GitHub package {} not found in npm, using GitHub API", package.name);
        }
        Err(e) => {
            eprintln!(
                "INFO: Error checking npm registry for GitHub package {}: {}",
                package.name,
                e
            );
        }
    }

    // If not found in npm, continue with GitHub API

    // Determine the GitHub repository URL from package info
    let repo_url = if package.resolution.contains("github:") {
        // Extract GitHub repo from resolution
        extract_github_url_from_resolution(&package.resolution)?
    } else if package.name.starts_with("github:") {
        // Extract GitHub repo from name
        format!("https://github.com/{}", package.name.trim_start_matches("github:"))
    } else if
        package.resolution.contains("__archiveUrl=") &&
        package.resolution.contains("github.com")
    {
        // Try to extract from archive URL
        let start_idx = package.resolution.find("github.com").unwrap_or(0);
        let end_idx = package.resolution[start_idx..]
            .find(".git")
            .unwrap_or(package.resolution.len() - start_idx);
        package.resolution[start_idx..start_idx + end_idx].to_string()
    } else {
        return Err(
            format!("Could not determine GitHub repository from package: {}", package.name).into()
        );
    };

    // Extract owner and repo from GitHub URL
    let (owner, repo, ref_or_commit) = match extract_github_details(&repo_url) {
        Ok(details) => details,
        Err(e) => {
            // Log the error
            let error_msg = format!("Invalid GitHub URL format: {}", e);
            eprintln!("INFO: {}", error_msg);

            // If we can't extract GitHub details, return minimal info using Package::with_error
            return Ok(
                Package::with_error(
                    package.name.clone(),
                    package.version.clone(),
                    "github",
                    package.resolution.clone(),
                    &error_msg
                )
            );
        }
    };

    // Create repository URL
    let repo_url = format!("https://github.com/{}/{}", owner, repo);

    // Find appropriate license file using the utility function
    let license_url = utils::get_license_file_url(&repo_url, &ref_or_commit);

    // Construct GitHub API URL to fetch package.json
    let api_url = format!(
        "https://api.github.com/repos/{}/{}/contents/package.json?ref={}",
        owner,
        repo,
        ref_or_commit
    );

    // Try to get the package info
    let response = match client.get(&api_url).header("User-Agent", "Dependency-Scanner").send() {
        Ok(resp) => resp,
        Err(e) => {
            let error_msg = format!("GitHub API network error: {}", e);
            eprintln!("INFO: {}", error_msg);

            // Return minimal info if request fails
            return Ok(
                Package::with_error(
                    package.name.clone(),
                    package.version.clone(),
                    "github",
                    repo_url.clone(),
                    &error_msg
                )
            );
        }
    };

    if !response.status().is_success() {
        // Log status code issues
        let status_code = response.status().as_u16();
        let reason = response.status().canonical_reason().unwrap_or("Unknown error");
        let error_msg = format!("GitHub API returned status code {}: {}", status_code, reason);

        eprintln!("INFO: {}", error_msg);

        // Return minimal info if response indicates failure
        return Ok(
            Package::with_error(
                package.name.clone(),
                package.version.clone(),
                "github",
                repo_url.clone(),
                &error_msg
            )
        );
    }

    // Try to parse the response as JSON
    let content: Value = match response.json() {
        Ok(json) => json,
        Err(e) => {
            let error_msg = format!("Failed to parse GitHub API response: {}", e);

            // Return minimal info if can't parse JSON
            return Ok(
                Package::with_error(
                    package.name.clone(),
                    package.version.clone(),
                    "github",
                    repo_url.clone(),
                    &error_msg
                )
            );
        }
    };

    // GitHub API returns content as base64-encoded
    let content_str = match content["content"].as_str() {
        Some(str) => str,
        None => {
            let error_msg = "No content field in GitHub API response";

            // Return minimal info if content field not found
            return Ok(
                Package::with_error(
                    package.name.clone(),
                    package.version.clone(),
                    "github",
                    repo_url.clone(),
                    &error_msg.to_string()
                )
            );
        }
    };

    // Try to decode base64 content
    let decoded_content = match base64::decode(&content_str.replace("\n", "")) {
        Ok(bytes) => bytes,
        Err(e) => {
            let error_msg = format!("Failed to decode base64 content: {}", e);

            // Return minimal info if can't decode base64
            return Ok(
                Package::with_error(
                    package.name.clone(),
                    package.version.clone(),
                    "github",
                    repo_url.clone(),
                    &error_msg
                )
            );
        }
    };

    // Try to parse decoded content as JSON
    let package_json: Value = match serde_json::from_slice(&decoded_content) {
        Ok(json) => json,
        Err(e) => {
            let error_msg = format!("Failed to parse package.json: {}", e);

            // Return minimal info if can't parse package.json
            return Ok(
                Package::with_error(
                    package.name.clone(),
                    package.version.clone(),
                    "github",
                    repo_url.clone(),
                    &error_msg
                )
            );
        }
    };

    // Extract license information
    let license_field = package_json["license"].as_str();
    let license = if let Some(lic) = license_field {
        lic.to_string()
    } else {
        "UNKNOWN".to_string()
    };

    let debug_info = if license == "UNKNOWN" {
        Some(
            format!(
                "No license field in package.json; manual check needed at {}",
                license_url.clone().unwrap_or_else(|| repo_url.clone())
            )
        )
    } else {
        None
    };

    // Get standard license URL if available, otherwise use repo license URL
    let final_license_url = crate::license_urls::get_license_url(&license).or_else(|| license_url);

    // Extract dependencies
    let mut dependencies = Vec::new();

    // Process regular dependencies
    if let Some(deps) = package_json["dependencies"].as_object() {
        for (name, version_value) in deps {
            let version_str = version_value.as_str().unwrap_or("").to_string();

            // Create a package entry for each dependency
            let dep = Package::new(
                name.clone(),
                version_str.clone(),
                if version_str.starts_with("github:") {
                    format!("https://github.com/{}", version_str.trim_start_matches("github:"))
                } else {
                    format!(
                        "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                        name,
                        name.replace('@', "").replace('/', "-"),
                        version_str.trim_start_matches("^").trim_start_matches("~")
                    )
                },
                None
            );

            dependencies.push(dep);
        }
    }

    // Process dev dependencies (optional)
    if let Some(dev_deps) = package_json["devDependencies"].as_object() {
        for (name, version_value) in dev_deps {
            let version_str = version_value.as_str().unwrap_or("").to_string();

            let dep = Package::new(
                name.clone(),
                version_str.clone(),
                if version_str.starts_with("github:") {
                    format!("https://github.com/{}", version_str.trim_start_matches("github:"))
                } else {
                    format!(
                        "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                        name,
                        name.replace('@', "").replace('/', "-"),
                        version_str.trim_start_matches("^").trim_start_matches("~")
                    )
                },
                None
            );

            dependencies.push(dep);
        }
    }

    // At the end, create a new Package with all the information:
    let mut result_package = Package::new(
        package.name.clone(), // Keep original package name
        package.version.clone(),
        package.resolution.clone(),
        package.checksum.clone()
    );

    // Fix: Keep the original name and set a display name in the registry field
    result_package.name = package.name.clone(); // Keep original package name
    result_package.registry = format!("github:{}/{}", owner, repo); // Store GitHub info in registry field
    result_package.license = license.clone(); // FIX: Clone license to avoid move
    result_package.license_expiration = None;
    result_package.url = repo_url;
    result_package.license_url = final_license_url.clone();
    result_package.debug_info = debug_info.clone(); // FIX: Clone if needed

    // When license is unknown but we have a license URL, try to download and detect license
    if license == "UNKNOWN" && final_license_url.is_some() {
        match crate::npm_api::try_detect_license_from_url(final_license_url.as_ref().unwrap()) {
            Ok(Some(detected_license)) => {
                result_package.license = detected_license;
                result_package.debug_info = Some(
                    format!("License detected from URL: {}", final_license_url.as_ref().unwrap())
                );
            }
            Ok(None) => {
                // License URL didn't help determine the license
                result_package.debug_info = Some(
                    format!(
                        "{}; No license detected from URL: {}",
                        result_package.debug_info.unwrap_or_else(|| "Unknown license".to_string()),
                        final_license_url.as_ref().unwrap()
                    )
                );
            }
            Err(e) => {
                // Error while trying to download license
                result_package.debug_info = Some(
                    format!(
                        "{}; Failed to download license from URL: {} ({})",
                        result_package.debug_info.unwrap_or_else(|| "Unknown license".to_string()),
                        final_license_url.as_ref().unwrap(),
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

fn extract_github_details(url: &str) -> Result<(String, String, String), Box<dyn Error>> {
    // Handle different GitHub URL formats
    if url.starts_with("https://github.com/") {
        // Format: https://github.com/owner/repo/...
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() >= 5 {
            let owner = parts[3].to_string();
            let repo = parts[4].to_string();

            // Determine ref or commit
            let ref_or_commit = if parts.len() > 6 && (parts[5] == "tree" || parts[5] == "commit") {
                parts[6].to_string()
            } else {
                "main".to_string() // Default to main if not specified
            };

            return Ok((owner, repo, ref_or_commit));
        }
    } else if url.starts_with("github:") {
        // Format: github:owner/repo#ref
        let url = url.trim_start_matches("github:");
        let parts: Vec<&str> = url.split('#').collect();

        let repo_parts: Vec<&str> = parts[0].split('/').collect();
        if repo_parts.len() >= 2 {
            let owner = repo_parts[0].to_string();
            let repo = repo_parts[1].to_string();

            // Get ref if specified, otherwise use main
            let ref_or_commit = if parts.len() > 1 {
                parts[1].to_string()
            } else {
                "main".to_string()
            };

            return Ok((owner, repo, ref_or_commit));
        }
    }

    Err(format!("Could not extract GitHub details from URL: {}", url).into())
}

fn extract_github_url_from_resolution(resolution: &str) -> Result<String, Box<dyn Error>> {
    if resolution.contains("github:") {
        if let Some(github_part) = resolution.split("github:").nth(1) {
            if let Some(repo_path) = github_part.split('#').next() {
                return Ok(format!("https://github.com/{}", repo_path));
            }
        }
    }

    Err(format!("Could not extract GitHub URL from resolution: {}", resolution).into())
}
