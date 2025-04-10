use std::collections::HashMap;
use once_cell::sync::Lazy;

// Map of common license identifiers to their URLs
pub static LICENSE_URLS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // SPDX License IDs and URLs
    map.insert("MIT", "https://opensource.org/licenses/MIT");
    map.insert("Apache-2.0", "https://opensource.org/licenses/Apache-2.0");
    map.insert("BSD-2-Clause", "https://opensource.org/licenses/BSD-2-Clause");
    map.insert("BSD-3-Clause", "https://opensource.org/licenses/BSD-3-Clause");
    map.insert("GPL-2.0", "https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html");
    map.insert("GPL-3.0", "https://www.gnu.org/licenses/gpl-3.0.en.html");
    map.insert("LGPL-2.1", "https://www.gnu.org/licenses/old-licenses/lgpl-2.1.en.html");
    map.insert("LGPL-3.0", "https://www.gnu.org/licenses/lgpl-3.0.en.html");
    map.insert("ISC", "https://opensource.org/licenses/ISC");
    map.insert("MPL-2.0", "https://opensource.org/licenses/MPL-2.0");
    map.insert("CDDL-1.0", "https://opensource.org/licenses/CDDL-1.0");
    map.insert("EPL-2.0", "https://opensource.org/licenses/EPL-2.0");
    map.insert("CC0-1.0", "https://creativecommons.org/publicdomain/zero/1.0/");
    map.insert("Unlicense", "https://unlicense.org/");
    map.insert("Zlib", "https://opensource.org/licenses/Zlib");
    map.insert("WTFPL", "http://www.wtfpl.net/");
    map.insert("0BSD", "https://opensource.org/licenses/0BSD");

    // Aliases and common variations
    map.insert("Apache 2.0", "https://opensource.org/licenses/Apache-2.0");
    map.insert("Apache License 2.0", "https://opensource.org/licenses/Apache-2.0");
    map.insert("GPL-2.0-only", "https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html");
    map.insert("GPL-2.0-or-later", "https://www.gnu.org/licenses/old-licenses/gpl-2.0.en.html");
    map.insert("GPL-3.0-only", "https://www.gnu.org/licenses/gpl-3.0.en.html");
    map.insert("GPL-3.0-or-later", "https://www.gnu.org/licenses/gpl-3.0.en.html");

    map
});

pub fn get_license_url(license: &str) -> Option<String> {
    LICENSE_URLS.get(license).map(|&url| url.to_string())
}
