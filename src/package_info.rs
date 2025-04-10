use crate::lockfile_parser::Package;
use serde::{ Serialize, Deserialize };

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PackageInfo {
    pub registry: String, // "npm", "github", etc.
    pub name: String, // Full package name including version
    pub license: String, // License type
    pub license_expiration: Option<String>, // License expiration if available
    pub url: String, // URL to package or repo
    pub license_url: Option<String>, // URL to license if available
    pub debug_info: Option<String>, // Debug information for UNKNOWN licenses
    pub dependencies: Vec<Package>, // Dependencies to be processed
}
