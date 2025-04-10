use yarn_lock_parser::parse_str;
use crate::package::Package;
use crate::utils;

/// Parse a yarn.lock file into a vector of packages using yarn-lock-parser
pub fn parse_yarn_lock(content: &str) -> Vec<Package> {
    let mut packages = Vec::new();

    // Use the yarn-lock-parser crate to parse the yarn.lock content
    match parse_str(content) {
        Ok(entries) => {
            // The parser returns a vector of entries directly
            for entry in entries {
                // Extract the package name
                let package_name = extract_package_name(&entry.name);

                // Convert version from &str to String
                let version = entry.version.to_string();

                // Skip local packages (0.0.0-use.local)
                if version.contains("0.0.0-use.local") {
                    continue;
                }

                // Extract resolution URL from the entry's descriptors
                let resolution = if
                    let Some(descriptor) = entry.descriptors
                        .iter()
                        .find(|(key, _)| *key == "resolution")
                {
                    // The resolution value contains package specifier, not URL
                    descriptor.1.to_string()
                } else {
                    // If no resolution, use the entry name as fallback
                    entry.name.to_string()
                };

                // Extract checksum from the entry's descriptors
                let checksum = entry.descriptors
                    .iter()
                    .find(|(key, _)| *key == "checksum")
                    .map(|(_, value)| value.to_string());

                // Create package object directly using Package::new
                let mut package = Package::new(package_name.clone(), version, resolution, checksum);

                // Set the package URL based on its source/resolution
                package.url = determine_package_url(&package_name, &package.resolution);

                packages.push(package);
            }
        }
        Err(e) => {
            eprintln!("Error parsing yarn.lock: {}", e);
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

/// Determine the appropriate URL for a package based on its name and resolution
fn determine_package_url(name: &str, resolution: &str) -> String {
    if name.starts_with("github:") {
        // GitHub package referenced by shorthand
        format!("https://github.com/{}", name.trim_start_matches("github:"))
    } else if resolution.contains("github:") || resolution.contains("github.com") {
        // GitHub resolution
        if let Some(github_part) = resolution.split("github:").nth(1) {
            if let Some(repo_path) = github_part.split('#').next() {
                return format!("https://github.com/{}", repo_path);
            }
        } else if let Some(index) = resolution.find("github.com") {
            let substr = &resolution[index..];
            if let Some(end) = substr.find(".git") {
                return substr[0..end].to_string();
            }
            return substr.to_string();
        }
        format!("https://github.com/{}", name) // Fallback
    } else {
        // Default to npm registry URL
        format!("https://www.npmjs.com/package/{}", name)
    }
}

/// Extract the base package name from an identifier (e.g., "lodash@^4.17.21" -> "lodash")
pub fn extract_package_name(identifier: &str) -> String {
    // Handle complex cases with commas (grouped dependencies)
    if identifier.contains(',') {
        if let Some(first_part) = identifier.split(',').next() {
            return extract_package_name(first_part.trim());
        }
    }

    // Handle scoped packages (@org/name)
    if identifier.starts_with('@') {
        // Split by @ but be careful with the format @org/name@version
        let parts: Vec<&str> = identifier.split('@').collect();
        if parts.len() >= 3 {
            // Format is like @org/name@version, parts[0] is empty
            let scope = parts[1];
            // Get the name part before the next @
            let name_version_part = parts[2];
            let name_parts: Vec<&str> = name_version_part.split('/').collect();
            if !name_parts.is_empty() {
                // Extract the version part after the name if it exists
                let name_and_version: Vec<&str> = name_parts[0].split('^').collect();
                if !name_and_version.is_empty() {
                    let package_name = format!("@{}/{}", scope, name_and_version[0]);
                    return package_name.trim_end_matches('/').to_string();
                }
            }
        }
        // If we can't parse it properly, return as is
        return identifier.to_string();
    }

    // Handle normal case (package@version)
    if let Some(at_pos) = identifier.find('@') {
        identifier[0..at_pos].to_string()
    } else {
        identifier.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name() {
        assert_eq!(extract_package_name("lodash@^4.17.21"), "lodash");
        assert_eq!(extract_package_name("@babel/core@^7.0.0"), "@babel/core");
        assert_eq!(extract_package_name("get-intrinsic@npm:^1.2.4"), "get-intrinsic");
        assert_eq!(
            extract_package_name("get-intrinsic@npm:^1.2.4, get-intrinsic@npm:^1.2.5"),
            "get-intrinsic"
        );
    }
}
