use serde_json::Value;
use crate::package::Package;
use crate::utils;

/// Parse an npm package-lock.json file into a vector of packages
pub fn parse_package_lock(content: &str) -> Vec<Package> {
    let mut packages = Vec::new();

    // Instead of using the package-lock-json-parser crate's structured types,
    // parse the JSON directly to avoid private field access issues
    match serde_json::from_str::<Value>(content) {
        Ok(json) => {
            // Process the root dependencies
            if let Some(dependencies) = json.get("dependencies").and_then(|d| d.as_object()) {
                for (name, dependency) in dependencies {
                    if let Some(version) = dependency.get("version").and_then(|v| v.as_str()) {
                        // Create a resolution URL (npm registry URL pattern)
                        let resolution = format!(
                            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                            name,
                            name.replace('@', "").replace('/', "-"),
                            version
                        );

                        // Extract integrity hash if available as checksum
                        let checksum = dependency
                            .get("integrity")
                            .and_then(|i| i.as_str())
                            .map(|s| s.to_string());

                        // Create package object
                        let mut package = Package::new(
                            name.clone(),
                            version.to_string(),
                            resolution.clone(),
                            checksum
                        );

                        // Set the URL based on the package source
                        package.url = determine_package_url(&name, &resolution, dependency);

                        packages.push(package);
                    }
                }
            }

            // Process packages from the packages field (npm v7+)
            if let Some(packages_map) = json.get("packages").and_then(|p| p.as_object()) {
                for (path, pkg_data) in packages_map {
                    // Skip the root package
                    if path == "" {
                        continue;
                    }

                    // Extract name and version from path
                    let mut name = path.clone();
                    let mut version = String::new();

                    // Handle node_modules/ prefix
                    if name.starts_with("node_modules/") {
                        name = name.trim_start_matches("node_modules/").to_string();
                    }

                    // Extract version from pkg_data if available
                    if let Some(v) = pkg_data.get("version").and_then(|v| v.as_str()) {
                        version = v.to_string();
                    } else {
                        // Try to extract version from path (e.g., node_modules/lodash@4.17.21)
                        if let Some(at_pos) = name.rfind('@') {
                            // Ensure this '@' isn't part of a scoped package name
                            if !(name.starts_with('@') && name[1..at_pos].contains('/')) {
                                version = name[at_pos + 1..].to_string();
                                name = name[..at_pos].to_string();
                            }
                        }
                    }

                    // Skip if we couldn't determine version
                    if version.is_empty() {
                        continue;
                    }

                    // Create a resolution URL
                    let resolution = format!(
                        "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                        name,
                        name.replace('@', "").replace('/', "-"),
                        version
                    );

                    // Extract integrity hash if available
                    let checksum = pkg_data
                        .get("integrity")
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string());

                    // Create package object
                    let package = Package::new(name, version, resolution, checksum);

                    // Only add if not already added (avoid duplicates)
                    if
                        !packages
                            .iter()
                            .any(|p| p.name == package.name && p.version == package.version)
                    {
                        packages.push(package);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error parsing package-lock.json: {}", e);
        }
    }

    // Generate fallback checksums for packages that don't have one
    for package in &mut packages {
        if package.checksum.is_none() {
            let fallback = utils::generate_fallback_checksum(&package);
            package.checksum = Some(fallback);
        }
    }

    packages
}

/// Determine the appropriate URL for a package based on its source
fn determine_package_url(name: &str, resolution: &str, dependency: &Value) -> String {
    // First check if there's a resolved URL in the package-lock.json
    if let Some(resolved) = dependency.get("resolved").and_then(|r| r.as_str()) {
        if resolved.contains("github.com") {
            return format!("https://github.com/{}", resolved.trim_start_matches("github:"));
        }
    }

    // Handle GitHub packages
    if resolution.contains("github.com") || name.starts_with("github:") {
        if name.starts_with("github:") {
            format!("https://github.com/{}", name.trim_start_matches("github:"))
        } else {
            // Try to extract GitHub URL from resolution
            if let Some(index) = resolution.find("github.com") {
                let substr = &resolution[index..];
                if let Some(end) = substr.find(".git") {
                    return substr[0..end].to_string();
                }
                return substr.to_string();
            }
            format!("https://github.com/{}", name) // Fallback
        }
    } else {
        // Default to npm registry URL
        format!("https://www.npmjs.com/package/{}", name)
    }
}
