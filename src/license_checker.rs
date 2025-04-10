use regex::Regex;

pub struct LicenseChecker {
    allowed_patterns: Vec<String>,
}

impl LicenseChecker {
    pub fn new(allowed_licenses: Vec<String>) -> Self {
        LicenseChecker {
            allowed_patterns: allowed_licenses,
        }
    }

    pub fn is_allowed(&self, license: &str) -> bool {
        // If no patterns specified, all licenses are allowed
        if self.allowed_patterns.is_empty() {
            return true;
        }

        for pattern in &self.allowed_patterns {
            if Self::matches_pattern(license, pattern) {
                return true;
            }
        }

        false
    }

    // Match license string against a pattern, supporting wildcards
    fn matches_pattern(license: &str, pattern: &str) -> bool {
        // Convert wildcard pattern to regex
        // * matches any sequence of characters
        let regex_pattern = pattern.replace(".", "\\.").replace("*", ".*");

        // Ensure the pattern matches the entire string
        let regex_str = format!("^{}$", regex_pattern);

        if let Ok(re) = Regex::new(&regex_str) {
            return re.is_match(license);
        }

        // Fallback to exact match if regex creation fails
        license == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let checker = LicenseChecker::new(vec!["MIT".to_string()]);
        assert!(checker.is_allowed("MIT"));
        assert!(!checker.is_allowed("Apache-2.0"));
    }

    #[test]
    fn test_wildcard_match() {
        let checker = LicenseChecker::new(vec!["Apache*".to_string()]);
        assert!(checker.is_allowed("Apache-2.0"));
        assert!(checker.is_allowed("Apache"));
        assert!(!checker.is_allowed("MIT"));
    }

    #[test]
    fn test_multiple_patterns() {
        let checker = LicenseChecker::new(vec!["MIT".to_string(), "ISC".to_string()]);
        assert!(checker.is_allowed("MIT"));
        assert!(checker.is_allowed("ISC"));
        assert!(!checker.is_allowed("GPL-3.0"));
    }

    #[test]
    fn test_empty_patterns() {
        let checker = LicenseChecker::new(vec![]);
        assert!(checker.is_allowed("MIT"));
        assert!(checker.is_allowed("Any-License"));
    }
}
