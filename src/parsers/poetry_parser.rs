use toml::Value;
use crate::package::Package;
use reqwest::blocking::Client;
use std::error::Error;

/// Parse a poetry.lock file into a vector of packages
pub fn parse_poetry_lock(content: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut package_map = std::collections::HashMap::new();

    // Parse the TOML content
    match content.parse::<Value>() {
        Ok(toml_value) => {
            // In poetry.lock, packages are stored in an array of tables called "package"
            if let Some(package_array) = toml_value.get("package").and_then(|p| p.as_array()) {
                // First pass: create all package objects
                for package_table in package_array {
                    if let Some(table) = package_table.as_table() {
                        // Extract package information
                        let name = table
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let version = table
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0.0.0")
                            .to_string();

                        // Get optional source information (source type and URL)
                        let mut source_type = "pypi"; // Default to PyPI
                        let mut source_url = String::new();

                        if let Some(source) = table.get("source").and_then(|s| s.as_table()) {
                            // Extract source type if available
                            if let Some(src_type) = source.get("type").and_then(|t| t.as_str()) {
                                source_type = src_type;
                            }

                            // Extract source URL if available
                            if let Some(url) = source.get("url").and_then(|u| u.as_str()) {
                                source_url = url.to_string();
                            }

                            // For git sources, also try to extract reference
                            if source_type == "git" && source_url.contains("github.com") {
                                if
                                    let Some(reference) = source
                                        .get("reference")
                                        .and_then(|r| r.as_str())
                                {
                                    if !source_url.contains("#") {
                                        source_url = format!("{}#{}", source_url, reference);
                                    }
                                }
                            }
                        }

                        // Create a resolution URL based on the source type and URL
                        let resolution = if source_url.is_empty() {
                            format!("https://pypi.org/project/{}/{}/", name, version)
                        } else {
                            source_url.clone()
                        };

                        // Create the package object
                        let mut package = Package::new(
                            name.clone(),
                            version.clone(),
                            resolution,
                            None // Python packages don't typically have checksums in poetry.lock
                        );

                        // Set basic metadata
                        package.registry = if
                            source_type == "git" &&
                            source_url.contains("github.com")
                        {
                            "github".to_string()
                        } else if source_type != "pypi" {
                            source_type.to_string()
                        } else {
                            "pypi".to_string()
                        };
                        package.display_name = format!("{}@{}", name, version);

                        // Set URL based on source
                        if source_type == "git" && source_url.contains("github.com") {
                            package.url = source_url.clone();
                        } else {
                            package.url = format!("https://pypi.org/project/{}/", name);
                        }

                        // Add source info to debug_info if it's not standard PyPI
                        if source_type != "pypi" {
                            package.debug_info = Some(
                                format!("Source type: {}, URL: {}", source_type, source_url)
                            );
                        }

                        // Store in our package map for the second pass
                        package_map.insert(name.clone(), package);
                    }
                }

                // Second pass: extract dependencies from each package
                for package_table in package_array {
                    if let Some(table) = package_table.as_table() {
                        let name = table
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        // Try to get dependencies
                        if
                            let Some(deps_table) = table
                                .get("dependencies")
                                .and_then(|d| d.as_table())
                        {
                            let package = package_map.get_mut(&name).unwrap();

                            for (dep_name, dep_constraint) in deps_table {
                                // Extract version constraint - this might be a string or a table with more info
                                let version_req = match dep_constraint {
                                    Value::String(s) => s.clone(),
                                    Value::Table(t) => {
                                        if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                                            v.to_string()
                                        } else {
                                            "*".to_string() // Wildcard if no version specified
                                        }
                                    }
                                    _ => "*".to_string(), // Wildcard for any other type
                                };

                                // Create a new dependency package
                                let dep_package = Package::new(
                                    dep_name.clone(),
                                    version_req.clone(),
                                    format!("https://pypi.org/project/{}/", dep_name),
                                    None
                                );

                                // Add dependency to the package
                                package.dependencies.push(dep_package);
                            }
                        }
                    }
                }

                // Add all packages to the result vector
                packages.extend(package_map.into_values());
            } else {
                eprintln!("Warning: No package array found in poetry.lock");
            }

            // Try to parse metadata section to get dev dependencies
            if let Some(metadata) = toml_value.get("metadata").and_then(|m| m.as_table()) {
                if let Some(dev_deps) = metadata.get("dev-dependencies").and_then(|d| d.as_table()) {
                    for (dep_name, dep_constraint) in dev_deps {
                        // Extract version constraint
                        let version_req = match dep_constraint {
                            Value::String(s) => s.clone(),
                            Value::Table(t) => {
                                if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                                    v.to_string()
                                } else {
                                    "*".to_string()
                                }
                            }
                            _ => "*".to_string(),
                        };

                        // Create a new dev dependency package (marked in the display name)
                        let mut dep_package = Package::new(
                            dep_name.clone(),
                            version_req.clone(),
                            format!("https://pypi.org/project/{}/", dep_name),
                            None
                        );

                        dep_package.registry = "pypi".to_string();
                        dep_package.display_name = format!("{}@{} (dev)", dep_name, version_req);
                        dep_package.url = format!("https://pypi.org/project/{}/", dep_name);

                        // Add to the packages list
                        packages.push(dep_package);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error parsing poetry.lock: {}", e);
        }
    }

    packages
}

/// Parse a pyproject.toml file to extract additional dependencies
pub fn parse_pyproject_toml(content: &str) -> Result<Vec<Package>, Box<dyn Error>> {
    let mut packages = Vec::new();

    // Parse the TOML content
    let toml_value: Value = content.parse()?;

    // Try to get dependencies from the tool.poetry section
    if let Some(poetry) = toml_value.get("tool").and_then(|t| t.get("poetry")) {
        // Extract regular dependencies
        if let Some(deps) = poetry.get("dependencies").and_then(|d| d.as_table()) {
            for (name, constraint) in deps {
                if name == "python" {
                    // Skip python dependency
                    continue;
                }

                let version_req = extract_version_constraint(constraint);

                let mut package = Package::new(
                    name.clone(),
                    version_req.clone(),
                    format!("https://pypi.org/project/{}/", name),
                    None
                );

                package.registry = "pypi".to_string();
                package.display_name = format!("{}@{}", name, version_req);
                package.url = format!("https://pypi.org/project/{}/", name);

                packages.push(package);
            }
        }

        // Extract dev dependencies
        if let Some(dev_deps) = poetry.get("dev-dependencies").and_then(|d| d.as_table()) {
            for (name, constraint) in dev_deps {
                let version_req = extract_version_constraint(constraint);

                let mut package = Package::new(
                    name.clone(),
                    version_req.clone(),
                    format!("https://pypi.org/project/{}/", name),
                    None
                );

                package.registry = "pypi".to_string();
                package.display_name = format!("{}@{} (dev)", name, version_req);
                package.url = format!("https://pypi.org/project/{}/", name);

                packages.push(package);
            }
        }
    }

    Ok(packages)
}

// Helper function to extract version constraint from TOML value
fn extract_version_constraint(constraint: &Value) -> String {
    match constraint {
        Value::String(s) => s.clone(),
        Value::Table(t) => {
            if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                v.to_string()
            } else {
                "*".to_string()
            }
        }
        _ => "*".to_string(),
    }
}

/// Get package info from PyPI API
pub fn get_package_info(package: &Package, debug: bool) -> Result<Package, Box<dyn Error>> {
    let client = Client::new();
    let package_name = &package.name;
    let version = &package.version;

    // Check if this is a GitHub source - if so, use GitHub API
    if package.registry == "github" || package.resolution.contains("github.com") {
        // Log that we're using GitHub API
        if cfg!(debug_assertions) || debug {
            println!("DEBUG: Using GitHub API for package from git source: {}", package_name);
        }

        // Create a temporary package for GitHub API with correct formatting
        let mut github_package = package.clone();

        // If not already prefixed, mark as GitHub package
        if !github_package.name.starts_with("github:") {
            github_package.name = format!("github:{}", package.name);
        }

        // Try to get license information from GitHub
        match crate::github_api::get_package_info(&github_package) {
            Ok(mut result) => {
                // If GitHub API couldn't determine the license, try to find a license file
                if result.license == "UNKNOWN" && result.url.contains("github.com") {
                    // Extract repo URL and branch/ref
                    let repo_url = result.url.clone();

                    // Try to extract a reference from the resolution URL
                    let reference = if github_package.resolution.contains('#') {
                        if let Some(ref_part) = github_package.resolution.split('#').nth(1) {
                            ref_part.to_string()
                        } else {
                            "main".to_string() // Default to main if not specified
                        }
                    } else {
                        "main".to_string() // Default to main branch
                    };

                    // Try to find a license file in the repository
                    if
                        let Some(license_url) = crate::utils::get_license_file_url(
                            &repo_url,
                            &reference
                        )
                    {
                        // Try to download and detect license from the license file
                        match crate::npm_api::try_detect_license_from_url(&license_url) {
                            Ok(Some(detected_license)) => {
                                result.license = detected_license.clone(); // Clone before moving
                                result.license_url = Some(license_url.clone()); // Clone before moving
                                result.debug_info = Some(
                                    format!("License detected from GitHub repository license file: {}", license_url)
                                );
                            }
                            Ok(None) => {
                                // License file exists but couldn't detect type
                                result.license_url = Some(license_url.clone()); // Clone before moving
                                result.debug_info = Some(
                                    format!("License file found at {} but type could not be detected", license_url)
                                );
                            }
                            Err(e) => {
                                // Error downloading license file
                                result.debug_info = Some(
                                    format!("Found GitHub repo but error fetching license file: {}", e)
                                );
                            }
                        }
                    } else {
                        result.debug_info = Some(
                            format!("No license file found in GitHub repo: {}", repo_url)
                        );
                    }
                }

                // Preserve original source information
                if package.debug_info.is_some() {
                    let orig_debug = package.debug_info.as_ref().unwrap();
                    if let Some(ref mut debug_info) = result.debug_info {
                        *debug_info = format!("{}; {}", orig_debug, debug_info);
                    } else {
                        result.debug_info = package.debug_info.clone();
                    }
                }

                return Ok(result);
            }
            Err(e) => {
                // Log error and fall back to PyPI
                eprintln!(
                    "INFO: GitHub API error for {}, falling back to PyPI: {}",
                    package_name,
                    e
                );
                // Continue with PyPI processing
            }
        }
    }

    // If the source type is not GitHub or PyPI, add an error in debug logs
    if package.registry != "pypi" && package.registry != "github" && !package.registry.is_empty() {
        let mut result = package.clone();
        let error_msg = format!(
            "Unsupported source type: {}. Source URL: {}. Currently only PyPI and GitHub sources are fully supported",
            package.registry,
            package.resolution
        );

        result.license = "UNKNOWN".to_string();
        result.debug_info = Some(error_msg);
        result.processed = true;
        return Ok(result);
    }

    // Create PyPI API URL
    let api_url = format!("https://pypi.org/pypi/{}/{}/json", package_name, version);

    // Add verbose debug output
    if cfg!(debug_assertions) || debug {
        println!("DEBUG: Fetching PyPI package info for {}@{}", package_name, version);
        println!("DEBUG: PyPI API URL: {}", api_url);
    }

    // Try to get the package info from PyPI
    let response = match client.get(&api_url).send() {
        Ok(resp) => resp,
        Err(e) => {
            let error_msg = format!("Network error when contacting PyPI API: {}", e);
            eprintln!("INFO: PyPI API request failed for {}: {}", package_name, error_msg);

            let mut result = package.clone();
            result.license = "UNKNOWN".to_string();
            result.debug_info = Some(error_msg);
            result.processed = true;
            return Ok(result);
        }
    };

    if !response.status().is_success() {
        let status_code = response.status().as_u16();
        let error_msg = format!("PyPI API returned status code {}", status_code);
        eprintln!("INFO: {}", error_msg);

        // Try without version to get info from the latest version
        return get_latest_package_info(package, debug);
    }

    // Get the response text for debug output
    let response_text = response.text()?;

    // Store the full response text if in debug mode
    let mut raw_response = None;
    if debug {
        raw_response = Some(response_text.clone());
    }

    // Parse the JSON response
    let pypi_data: serde_json::Value = match serde_json::from_str(&response_text) {
        Ok(json) => json,
        Err(e) => {
            let error_msg = format!("Failed to parse JSON from PyPI API: {}", e);
            eprintln!("INFO: {}", error_msg);

            let mut result = package.clone();
            result.license = "UNKNOWN".to_string();
            result.debug_info = Some(error_msg);
            result.processed = true;
            return Ok(result);
        }
    };

    // Extract license information
    let mut result = package.clone();

    // Store raw API response for debug mode
    result.raw_api_response = raw_response;

    // Keep original source information
    if package.debug_info.is_some() {
        result.debug_info = package.debug_info.clone();
    }

    if let Some(info) = pypi_data.get("info") {
        // First try to get license from the license field
        let mut license = "UNKNOWN".to_string();

        if let Some(license_str) = info.get("license").and_then(|l| l.as_str()) {
            let license_str = license_str.trim();
            if !license_str.is_empty() && license_str != "UNKNOWN" {
                license = crate::license_detection::normalize_license_id(license_str);
            }
        }

        // If license is still unknown, try to extract from classifiers
        if license == "UNKNOWN" {
            if let Some(classifiers) = info.get("classifiers").and_then(|c| c.as_array()) {
                if let Some(detected_license) = extract_license_from_classifiers(classifiers) {
                    license = detected_license;
                }
            }
        }

        result.license = license;

        // Collect additional PyPI metadata for verbose output
        let mut metadata = Vec::new();

        // Add summary if available
        if let Some(summary) = info.get("summary").and_then(|s| s.as_str()) {
            if !summary.is_empty() {
                metadata.push(format!("Summary: {}", summary));
            }
        }

        // Add author information
        if let Some(author) = info.get("author").and_then(|a| a.as_str()) {
            if !author.is_empty() {
                metadata.push(format!("Author: {}", author));
            }
        }

        // Add author email if available
        if let Some(author_email) = info.get("author_email").and_then(|a| a.as_str()) {
            if !author_email.is_empty() {
                metadata.push(format!("Author Email: {}", author_email));
            }
        }

        // Add maintainer if different from author
        if let Some(maintainer) = info.get("maintainer").and_then(|m| m.as_str()) {
            if
                !maintainer.is_empty() &&
                Some(maintainer) != info.get("author").and_then(|a| a.as_str())
            {
                metadata.push(format!("Maintainer: {}", maintainer));
            }
        }

        // Get project URL
        if let Some(project_url) = info.get("project_url").and_then(|u| u.as_str()) {
            result.url = project_url.to_string();
            metadata.push(format!("Project URL: {}", project_url));
        } else if let Some(home_page) = info.get("home_page").and_then(|h| h.as_str()) {
            if !home_page.is_empty() && home_page != "UNKNOWN" {
                result.url = home_page.to_string();
                metadata.push(format!("Homepage: {}", home_page));
            }
        }

        // Add project URLs if available
        if let Some(project_urls) = info.get("project_urls").and_then(|p| p.as_object()) {
            let mut url_info = Vec::new();
            for (name, url_value) in project_urls {
                if let Some(url) = url_value.as_str() {
                    url_info.push(format!("{}: {}", name, url));
                }
            }
            if !url_info.is_empty() {
                metadata.push(format!("URLs: [{}]", url_info.join(", ")));
            }
        }

        // Add version info
        metadata.push(
            format!(
                "Latest Version: {}",
                info
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
            )
        );

        // Update debug info to include original source info if it existed
        if let Some(existing_debug_info) = &result.debug_info {
            if !metadata.is_empty() {
                result.debug_info = Some(
                    format!("{}; {}", existing_debug_info, metadata.join("; "))
                );
            }
        } else if !metadata.is_empty() {
            result.debug_info = Some(metadata.join("; "));
        }

        // Get author information for debug info
        let mut debug_info = Vec::new();
        if let Some(author) = info.get("author").and_then(|a| a.as_str()) {
            if !author.is_empty() {
                debug_info.push(format!("Author: {}", author));
            }
        }

        // If license is unknown, try to find from GitHub repo
        if result.license == "UNKNOWN" {
            // Check if there's a GitHub URL
            let mut github_url = None;

            // Look in project URLs dictionary
            if let Some(project_urls) = info.get("project_urls").and_then(|p| p.as_object()) {
                for (name, url_value) in project_urls {
                    if let Some(url) = url_value.as_str() {
                        if url.contains("github.com") {
                            github_url = Some(url.to_string());
                            debug_info.push(
                                format!("Found GitHub URL in project_urls[{}]: {}", name, url)
                            );
                            break;
                        }
                    }
                }
            }

            // Also check home_page
            if github_url.is_none() {
                if let Some(home_page) = info.get("home_page").and_then(|h| h.as_str()) {
                    if home_page.contains("github.com") {
                        github_url = Some(home_page.to_string());
                        debug_info.push(format!("Found GitHub URL in home_page: {}", home_page));
                    }
                }
            }

            // If we found a GitHub URL, use the GitHub API to get license info
            if let Some(github_url) = github_url {
                // Create a temporary package for GitHub API
                let mut github_package = Package::new(
                    format!("github:{}", package_name), // Mark as GitHub package
                    version.clone(),
                    github_url.clone(),
                    None
                );
                github_package.registry = "github".to_string();
                github_package.url = github_url.clone();

                // Use GitHub API to get license info
                match crate::github_api::get_package_info(&github_package) {
                    Ok(github_result) => {
                        if github_result.license != "UNKNOWN" {
                            result.license = github_result.license;
                            result.license_url = github_result.license_url;
                            debug_info.push("License found via GitHub API".to_string());
                        } else {
                            // Try to find license file directly
                            if
                                let Some(license_url) = crate::utils::get_license_file_url(
                                    &github_url,
                                    "main"
                                )
                            {
                                debug_info.push(format!("Found license file at: {}", license_url));
                                result.license_url = Some(license_url.clone());

                                // Try to detect license from the file content
                                match crate::npm_api::try_detect_license_from_url(&license_url) {
                                    Ok(Some(detected_license)) => {
                                        result.license = detected_license.clone(); // Clone before moving
                                        debug_info.push(
                                            format!("Detected license from file: {}", detected_license)
                                        );
                                    }
                                    Ok(None) => {
                                        debug_info.push(
                                            "License file found but could not determine type".to_string()
                                        );
                                    }
                                    Err(e) => {
                                        debug_info.push(
                                            format!("Error downloading license file: {}", e)
                                        );
                                    }
                                }
                            } else {
                                debug_info.push("No license file found in GitHub repo".to_string());
                            }
                        }
                    }
                    Err(e) => {
                        debug_info.push(format!("GitHub API error: {}", e));
                    }
                }
            }
        }

        if !debug_info.is_empty() {
            result.debug_info = Some(debug_info.join("; "));
        }
    } else {
        result.license = "UNKNOWN".to_string();
        result.debug_info = Some("No info object found in PyPI API response".to_string());
    }

    result.processed = true;
    Ok(result)
}

/// Extract license information from Python classifiers
fn extract_license_from_classifiers(classifiers: &[serde_json::Value]) -> Option<String> {
    // Common license patterns in Python classifiers
    let license_patterns = [
        ("License :: OSI Approved :: MIT License", "MIT"),
        ("License :: OSI Approved :: Apache Software License", "Apache-2.0"),
        ("License :: OSI Approved :: BSD License", "BSD-3-Clause"),
        ("License :: OSI Approved :: GNU General Public License v3 (GPLv3)", "GPL-3.0"),
        ("License :: OSI Approved :: GNU General Public License v2 (GPLv2)", "GPL-2.0"),
        ("License :: OSI Approved :: GNU Lesser General Public License v3 (LGPLv3)", "LGPL-3.0"),
        (
            "License :: OSI Approved :: GNU Lesser General Public License v2.1 (LGPLv2.1)",
            "LGPL-2.1",
        ),
        ("License :: OSI Approved :: Mozilla Public License 2.0 (MPL 2.0)", "MPL-2.0"),
        ("License :: OSI Approved :: ISC License (ISCL)", "ISC"),
        ("License :: CC0 1.0 Universal (CC0 1.0) Public Domain Dedication", "CC0-1.0"),
        ("License :: Public Domain", "Unlicense"),
        ("License :: OSI Approved :: Python Software Foundation License", "PSF"),
        ("License :: OSI Approved :: zlib/libpng License", "Zlib"),
        // BSD variants
        ("License :: OSI Approved :: BSD License", "BSD-3-Clause"),
        ("License :: OSI Approved :: BSD 3-Clause License", "BSD-3-Clause"),
        ("License :: OSI Approved :: BSD 2-Clause License", "BSD-2-Clause"),
    ];

    for classifier in classifiers.iter().filter_map(|c| c.as_str()) {
        for (pattern, license) in license_patterns.iter() {
            if classifier.contains(pattern) {
                return Some(license.to_string());
            }
        }

        // For any classifier containing "License :: OSI Approved ::" that we don't specifically match,
        // try to extract the license name
        if classifier.contains("License :: OSI Approved :: ") {
            let parts: Vec<&str> = classifier.split("License :: OSI Approved :: ").collect();
            if parts.len() > 1 {
                let license_name = parts[1].trim();
                // Map common patterns to SPDX
                if license_name.contains("MIT") {
                    return Some("MIT".to_string());
                } else if license_name.contains("Apache") {
                    return Some("Apache-2.0".to_string());
                } else if license_name.contains("BSD") {
                    if license_name.contains("2") {
                        return Some("BSD-2-Clause".to_string());
                    } else {
                        return Some("BSD-3-Clause".to_string());
                    }
                } else {
                    // Return as is, will be normalized later
                    return Some(crate::license_detection::normalize_license_id(license_name));
                }
            }
        }
    }

    None
}

/// Fallback to get the latest version info when specific version fails
fn get_latest_package_info(package: &Package, debug: bool) -> Result<Package, Box<dyn Error>> {
    let client = Client::new();
    let package_name = &package.name;

    // Create PyPI API URL without version to get the latest
    let api_url = format!("https://pypi.org/pypi/{}/json", package_name);

    // Try to get the package info
    let response = match client.get(&api_url).send() {
        Ok(resp) => resp,
        Err(e) => {
            let error_msg = format!("Network error when contacting PyPI API: {}", e);
            eprintln!("INFO: PyPI API request failed for {}: {}", package_name, error_msg);

            let mut result = package.clone();
            result.license = "UNKNOWN".to_string();
            result.debug_info = Some(error_msg);
            result.processed = true;
            return Ok(result);
        }
    };

    if !response.status().is_success() {
        let status_code = response.status().as_u16();
        let error_msg = format!("PyPI API returned status code {} for latest version", status_code);
        eprintln!("INFO: {}", error_msg);

        let mut result = package.clone();
        result.license = "UNKNOWN".to_string();
        result.debug_info = Some(error_msg);
        result.processed = true;
        return Ok(result);
    }

    // Get the response text for debug output
    let response_text = response.text()?;

    // Store the full response text if in debug mode
    let mut raw_response = None;
    if debug {
        raw_response = Some(response_text.clone());
    }

    // Parse response and extract info using same logic as above
    let pypi_data: serde_json::Value = match serde_json::from_str(&response_text) {
        Ok(json) => json,
        Err(e) => {
            let error_msg = format!("Failed to parse JSON from PyPI API: {}", e);
            let mut result = package.clone();
            result.license = "UNKNOWN".to_string();
            result.debug_info = Some(error_msg);
            result.processed = true;
            return Ok(result);
        }
    };

    // Use same extraction logic as in get_package_info
    let mut result = package.clone();

    // Store raw API response for debug mode
    result.raw_api_response = raw_response;

    if let Some(info) = pypi_data.get("info") {
        // First try to get license from the license field
        let mut license = "UNKNOWN".to_string();

        if let Some(license_str) = info.get("license").and_then(|l| l.as_str()) {
            let license_str = license_str.trim();
            if !license_str.is_empty() && license_str != "UNKNOWN" {
                license = crate::license_detection::normalize_license_id(license_str);
            }
        }

        // If license is still unknown, try to extract from classifiers
        if license == "UNKNOWN" {
            if let Some(classifiers) = info.get("classifiers").and_then(|c| c.as_array()) {
                if let Some(detected_license) = extract_license_from_classifiers(classifiers) {
                    license = detected_license;
                }
            }
        }

        result.license = license;

        if let Some(project_url) = info.get("project_url").and_then(|u| u.as_str()) {
            result.url = project_url.to_string();
        } else if let Some(home_page) = info.get("home_page").and_then(|h| h.as_str()) {
            if !home_page.is_empty() && home_page != "UNKNOWN" {
                result.url = home_page.to_string();
            }
        }

        result.debug_info = Some(
            format!(
                "Used data from latest version instead of requested version {}",
                package.version
            )
        );
    } else {
        result.license = "UNKNOWN".to_string();
        result.debug_info = Some(
            "No info object found in PyPI API response for latest version".to_string()
        );
    }

    result.processed = true;
    Ok(result)
}
