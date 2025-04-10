use regex::Regex;
use once_cell::sync::Lazy;
use std::collections::HashMap;

// Common license text patterns to match against license files when license identifier is unknown
static LICENSE_PATTERNS: Lazy<HashMap<&'static str, Regex>> = Lazy::new(|| {
    let mut patterns = HashMap::new();

    // MIT License pattern - enhance with more variations
    patterns.insert(
        "MIT",
        Regex::new(
            r"(?i)(Permission is hereby granted, free of charge,.*MIT License|The MIT License \(MIT\)|MIT License Copyright|Permission is hereby granted, free of charge,.*subject to the following conditions)"
        ).unwrap()
    );

    // Apache 2.0 pattern - make more robust
    patterns.insert(
        "Apache-2.0",
        Regex::new(
            r"(?i)(Apache License.*Version 2\.0|Licensed under the Apache License, Version 2\.0)"
        ).unwrap()
    );

    // GPL patterns
    patterns.insert("GPL-3.0", Regex::new(r"(?i)GNU General Public License.*Version 3").unwrap());
    patterns.insert("GPL-2.0", Regex::new(r"(?i)GNU General Public License.*Version 2").unwrap());

    // BSD patterns - improve matching
    patterns.insert(
        "BSD-3-Clause",
        Regex::new(
            r"(?i)(redistribution and use.*permitted provided that.*conditions are met.*neither the name.*nor the names of|The 3-Clause BSD License|3-Clause BSD License|3-clause BSD license)"
        ).unwrap()
    );
    patterns.insert(
        "BSD-2-Clause",
        Regex::new(
            r"(?i)redistribution and use.*permitted provided that.*conditions are met.*binary form must"
        ).unwrap()
    );

    // ISC
    patterns.insert(
        "ISC",
        Regex::new(r"(?i)ISC License.*Permission to use, copy, modify, and/or distribute").unwrap()
    );

    // Unlicense
    patterns.insert(
        "Unlicense",
        Regex::new(
            r"(?i)This is free and unencumbered software released into the public domain"
        ).unwrap()
    );

    // Add more patterns for common licenses
    patterns.insert(
        "MPL-2.0",
        Regex::new(r"(?i)(Mozilla Public License.*Version 2\.0|MPL 2\.0)").unwrap()
    );

    patterns.insert(
        "LGPL-2.1",
        Regex::new(r"(?i)(GNU Lesser General Public License.*Version 2\.1)").unwrap()
    );

    patterns.insert(
        "LGPL-3.0",
        Regex::new(r"(?i)(GNU Lesser General Public License.*Version 3)").unwrap()
    );

    patterns.insert(
        "CC0-1.0",
        Regex::new(
            r"(?i)(Creative Commons Legal Code.*CC0 1\.0|CC0 1\.0 Universal|The person.*waives all of his or her rights)"
        ).unwrap()
    );

    patterns.insert("EPL-2.0", Regex::new(r"(?i)(Eclipse Public License.*2\.0|EPL-2\.0)").unwrap());

    patterns
});

/// Attempt to detect license type from license text
pub fn detect_license_from_text(text: &str) -> Option<String> {
    for (license_type, pattern) in LICENSE_PATTERNS.iter() {
        if pattern.is_match(text) {
            return Some(license_type.to_string());
        }
    }
    None
}

/// Clean up commonly found license variations
pub fn normalize_license_id(license: &str) -> String {
    match license.trim().to_lowercase().as_str() {
        "mit" => "MIT".to_string(),
        "apache2" | "apache 2" | "apache2.0" | "apache 2.0" => "Apache-2.0".to_string(),
        "bsd" => "BSD-3-Clause".to_string(), // Default to 3-clause when unspecified
        "bsd-3" => "BSD-3-Clause".to_string(),
        "bsd-2" => "BSD-2-Clause".to_string(),
        "gpl" | "gpl3" | "gplv3" | "gpl-3" => "GPL-3.0".to_string(),
        "gpl2" | "gplv2" | "gpl-2" => "GPL-2.0".to_string(),
        "isc license" => "ISC".to_string(),
        "public domain" => "Unlicense".to_string(),
        _ => license.to_string(),
    }
}
