use serde::{ Serialize, Deserialize };

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    // Basic fields (from lockfile parsing)
    pub name: String,
    pub version: String,
    pub resolution: String,
    pub checksum: Option<String>,

    // Extended fields (filled in after API calls)
    #[serde(default)]
    pub registry: String, // "npm", "github", etc.
    #[serde(default)]
    pub display_name: String, // Formatted name for display (e.g. "package@version")
    #[serde(default)]
    pub license: String, // License type
    #[serde(default)]
    pub license_expiration: Option<String>, // License expiration if available
    #[serde(default)]
    pub url: String, // URL to package or repo
    #[serde(default)]
    pub license_url: Option<String>, // URL to license if available
    #[serde(default)]
    pub debug_info: Option<String>, // Debug information
    #[serde(default)]
    pub dependencies: Vec<Package>, // Dependencies
    #[serde(default)]
    pub processed: bool, // Whether this package has been fully processed
    #[serde(default)]
    pub retry_for_unknown: bool, // Flag to indicate this is a retry for an unknown license
    #[serde(default)]
    pub raw_api_response: Option<String>, // Raw API response (for debug output)
}

impl Package {
    /// Create a new Package from lockfile data
    pub fn new(
        name: String,
        version: String,
        resolution: String,
        checksum: Option<String>
    ) -> Self {
        Package {
            name,
            version: version.clone(),
            resolution,
            checksum,
            registry: String::new(),
            display_name: String::new(),
            license: String::new(),
            license_expiration: None,
            url: String::new(),
            license_url: None,
            debug_info: None,
            dependencies: Vec::new(),
            processed: false,
            retry_for_unknown: false,
            raw_api_response: None,
        }
    }

    /// Create a new Package with minimal information (for error cases)
    pub fn with_error(
        name: String,
        version: String,
        registry: &str,
        url: String,
        error_msg: &str
    ) -> Self {
        let display_name = format!("{}@{}", name, version);

        Package {
            name,
            version,
            resolution: String::new(),
            checksum: None,
            registry: registry.to_string(),
            display_name,
            license: "UNKNOWN".to_string(),
            license_expiration: None,
            url,
            license_url: None,
            debug_info: Some(error_msg.to_string()),
            dependencies: Vec::new(),
            processed: true,
            retry_for_unknown: false,
            raw_api_response: None,
        }
    }

    /// Mark this package as processed
    #[allow(dead_code)] // Added attribute since this method isn't currently used
    pub fn mark_processed(&mut self) {
        self.processed = true;
    }

    /// Check if package has been processed
    #[allow(dead_code)] // Added attribute since this method isn't currently used
    pub fn is_processed(&self) -> bool {
        self.processed
    }

    /// Get formatted display name (now uses the stored display_name if available)
    #[allow(dead_code)]
    pub fn display_name(&self) -> String {
        if !self.display_name.is_empty() {
            self.display_name.clone()
        } else {
            format!("{}@{}", self.name, self.version)
        }
    }
}
