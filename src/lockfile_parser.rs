use serde::{ Serialize, Deserialize };
use crate::package::Package;
use std::fs;
use std::path::Path;
use crate::parsers;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockfilePackage {
    pub name: String,
    pub version: String,
    pub resolution: String,
    pub checksum: Option<String>,
}

impl LockfilePackage {
    // Convert LockfilePackage to the unified Package type
    #[allow(dead_code)] // Added attribute since this method isn't currently used
    pub fn to_package(&self) -> Package {
        Package::new(
            self.name.clone(),
            self.version.clone(),
            self.resolution.clone(),
            self.checksum.clone()
        )
    }
}

pub fn parse_lockfile(path: &Path) -> Result<Vec<Package>, String> {
    // Check if file exists
    if !path.exists() || !path.is_file() {
        return Err(format!("File not found: {}", path.display()));
    }

    // Read the file content
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return Err(format!("Failed to read file: {}", e));
        }
    };

    // Determine file type by extension and parse accordingly
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();

    let packages: Vec<Package>;

    if file_name == "yarn.lock" {
        packages = parsers::yarn_parser::parse_yarn_lock(&content);
    } else if file_name == "package-lock.json" {
        packages = parsers::npm_parser::parse_package_lock(&content);
    } else if file_name == "poetry.lock" {
        packages = parsers::poetry_parser::parse_poetry_lock(&content);

        // Also try to parse pyproject.toml if it exists in the same directory
        let pyproject_path = path.parent().unwrap().join("pyproject.toml");
        if pyproject_path.exists() && pyproject_path.is_file() {
            if let Ok(pyproject_content) = fs::read_to_string(&pyproject_path) {
                if
                    let Ok(pyproject_packages) = parsers::poetry_parser::parse_pyproject_toml(
                        &pyproject_content
                    )
                {
                    // Add pyproject packages to the list if they're not already there
                    let mut combined_packages = packages.clone();
                    for pkg in pyproject_packages {
                        if
                            !combined_packages
                                .iter()
                                .any(|p| p.name == pkg.name && p.version == pkg.version)
                        {
                            combined_packages.push(pkg);
                        }
                    }
                    return Ok(combined_packages);
                }
            }
        }
    } else if file_name == "pnpm-lock.yaml" {
        return Err("pnpm-lock.yaml support is coming soon!".to_string());
    } else if file_name == "bun.lock" {
        return Err("bun.lock support is coming soon!".to_string());
    } else if extension == "csproj" {
        // For .csproj files, we pass the path directly to the nuget parser
        packages = parsers::nuget_parser::parse_csproj(path)?;
    } else {
        return Err(format!("Unsupported lock file format: {}", file_name));
    }

    Ok(packages)
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
