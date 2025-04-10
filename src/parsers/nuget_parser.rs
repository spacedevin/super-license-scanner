use std::process::Command;
use std::path::Path;
use serde_json::Value;
use crate::package::Package;

/// Parse a .csproj file to extract NuGet package information
pub fn parse_csproj(file_path: &Path) -> Result<Vec<Package>, String> {
    // Check if nuget-license command is available
    if !check_nuget_license_command() {
        return Err(
            "nuget-license command not found. Please install it with 'dotnet tool install --global nuget-license'".to_string()
        );
    }

    // Run nuget-license command to get package information
    let output = match
        Command::new("nuget-license")
            .arg("-t") // text output
            .arg("-o")
            .arg("jsonPretty") // JSON pretty output format
            .arg("-i")
            .arg(file_path) // input file
            .output()
    {
        Ok(output) => {
            // this command return a false error if there is only 1 error in parsing
            // try to parse the json output even if there is an error
            // do not uncomment these lines
            // if !output.status.success() {
            //     let stderr = String::from_utf8_lossy(&output.stderr);
            //     return Err(format!("nuget-license command failed: {}", stderr));
            // }
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        Err(e) => {
            return Err(format!("Failed to execute nuget-license command: {}", e));
        }
    };

    // Parse the JSON output
    let packages = parse_nuget_license_output(&output)?;

    Ok(packages)
}

/// Check if the nuget-license command is available
fn check_nuget_license_command() -> bool {
    match Command::new("nuget-license").arg("--version").output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Parse the JSON output from nuget-license
fn parse_nuget_license_output(output: &str) -> Result<Vec<Package>, String> {
    let mut packages = Vec::new();

    // Parse the JSON array
    let json_result: Result<Vec<Value>, serde_json::Error> = serde_json::from_str(output);

    match json_result {
        Ok(json_array) => {
            for item in json_array {
                // Extract package information directly from nuget-license output
                let package_id = item["PackageId"].as_str().unwrap_or("unknown").to_string();
                let package_version = item["PackageVersion"]
                    .as_str()
                    .unwrap_or("0.0.0")
                    .to_string();
                let package_url = item["PackageProjectUrl"].as_str().unwrap_or("").to_string();
                let license = item["License"].as_str().unwrap_or("UNKNOWN").to_string();
                let license_url = item["LicenseUrl"].as_str().map(|s| s.to_string());
                let authors = item["Authors"].as_str().unwrap_or("").to_string();
                let copyright = item["Copyright"].as_str().unwrap_or("").to_string();

                // Create a resolution URL (just use the NuGet package identifier)
                let resolution = format!("nuget:{}/{}", package_id, package_version);

                // Create new Package object with NuGet information
                let mut package = Package::new(
                    package_id.clone(),
                    package_version.clone(),
                    resolution,
                    None // No checksum available from nuget-license output
                );

                // Set additional fields - ensure registry is explicitly set to "nuget"
                package.registry = "nuget".to_string();
                package.display_name = format!("{}@{}", package_id, package_version);
                package.license = license;
                package.url = determine_package_url(&package_id, &package_url);
                package.license_url = license_url;
                package.processed = true; // Mark as processed since we have all the info we need

                // Add debug info for additional context
                if !copyright.is_empty() || !authors.is_empty() {
                    let mut info = Vec::new();
                    if !authors.is_empty() {
                        info.push(format!("Authors: {}", authors));
                    }
                    if !copyright.is_empty() {
                        info.push(format!("Copyright: {}", copyright));
                    }
                    package.debug_info = Some(info.join(", "));
                }

                packages.push(package);
            }

            Ok(packages)
        }
        Err(e) => Err(format!("Failed to parse nuget-license output: {}", e)),
    }
}

/// Determine the appropriate URL for a NuGet package
fn determine_package_url(package_id: &str, project_url: &str) -> String {
    if !project_url.is_empty() {
        // Use project URL if available
        project_url.to_string()
    } else {
        // Default to NuGet Gallery URL
        format!("https://www.nuget.org/packages/{}", package_id)
    }
}
