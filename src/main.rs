use std::collections::{ HashSet, VecDeque, HashMap };
use std::fs;
use std::path::Path;
use std::sync::{ Arc, Mutex };
use std::thread;
use clap::{ Parser, ArgAction };
use colored::Colorize;

mod package;
mod github_api;
mod npm_api;
mod utils;
mod license_checker;
mod license_urls;
mod archive_handler;
mod license_detection;
mod parsers;
mod lockfile_parser;

use package::Package;
use utils::{ generate_package_hash, get_from_cache, save_to_cache, init_cache_dir };
use license_checker::LicenseChecker;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path(s) to project root directories containing yarn.lock
    #[arg(index = 1, required = true, num_args = 1.., value_name = "PROJECT_PATH")]
    project_paths: Vec<String>,

    /// Comma-separated list of allowed licenses (supports wildcards)
    #[arg(long, value_name = "LICENSES", value_delimiter = ',')]
    allowed: Vec<String>,

    /// Show all packages, not just non-compliant ones
    #[arg(long, short, action = ArgAction::SetTrue)]
    verbose: bool,

    /// Only show packages with unknown licenses (for debugging)
    #[arg(long, action = ArgAction::SetTrue)]
    unknown: bool,

    /// Just output information from the parsed lockfile without license checking
    #[arg(long, action = ArgAction::SetTrue)]
    info: bool,

    /// Retry packages with unknown licenses when paired with --unknown
    #[arg(long, action = ArgAction::SetTrue)]
    retry: bool,

    /// Recursively search directories for supported lock files
    #[arg(short, action = ArgAction::SetTrue)]
    recursive: bool,

    /// Show full debug information including complete API responses
    #[arg(long, action = ArgAction::SetTrue)]
    debug: bool,

    /// Output unique packages as CSV with name, URL, and license
    #[arg(long, action = ArgAction::SetTrue)]
    csv: bool,

    /// Output dependency tree visualization
    #[arg(long, action = ArgAction::SetTrue)]
    tree: bool,

    /// Output file path (for CSV or other formats)
    #[arg(short, value_name = "OUTPUT_FILE")]
    output: Option<String>,
}

// Supported lock file names and their parsing functions
static SUPPORTED_LOCKFILES: &[&str] = &[
    "yarn.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "bun.lock",
    "poetry.lock", // Add poetry.lock to supported files
    "*.csproj", // Added .csproj files for NuGet packages
];

fn main() {
    // Parse command line arguments using clap
    let args = Args::parse();

    // Initialize license checker with allowed license patterns
    let license_checker = Arc::new(LicenseChecker::new(args.allowed.clone()));

    // Initialize cache directory
    match init_cache_dir() {
        Ok(_) => println!("Cache initialized"),
        Err(e) => {
            eprintln!("Warning: Failed to initialize cache: {}", e);
            eprintln!("Continuing without cache...");
        }
    }

    // Create collections to store all packages and results across all projects
    let mut all_initial_packages = Vec::new();
    let mut project_count = 0;
    let mut lockfiles_found = Vec::new();

    // Process each project path
    for project_path in &args.project_paths {
        if args.recursive {
            // Recursively find all supported lock files
            let found_lockfiles = find_lockfiles(project_path);
            if found_lockfiles.is_empty() {
                eprintln!("No supported lock files found in {}", project_path);
                continue;
            }

            lockfiles_found.extend(found_lockfiles);
        } else {
            // Just check for yarn.lock in the specified directory
            let yarn_lock_path = Path::new(project_path).join("yarn.lock");
            if yarn_lock_path.exists() {
                lockfiles_found.push(yarn_lock_path);
            } else {
                eprintln!("yarn.lock not found at {}", yarn_lock_path.display());
            }
        }
    }

    // If no lockfiles were found, exit
    if lockfiles_found.is_empty() {
        eprintln!("No supported lock files found in any of the provided paths.");
        std::process::exit(1);
    }

    // Process each found lockfile
    for lockfile_path in &lockfiles_found {
        project_count += 1;
        println!("Processing lockfile: {}", lockfile_path.display());

        // Parse lockfile using the universal parser
        let initial_packages = match lockfile_parser::parse_lockfile(lockfile_path) {
            Ok(packages) => {
                println!("Found {} packages in {}", packages.len(), lockfile_path.display());
                packages
            }
            Err(e) => {
                eprintln!("Failed to parse {}: {}", lockfile_path.display(), e);
                continue; // Skip this lockfile but continue with others
            }
        };

        // Add to the collection of all packages
        all_initial_packages.extend(initial_packages);
    }

    // If no valid projects were found, exit
    if all_initial_packages.is_empty() {
        eprintln!("No packages found in the provided lock files.");
        std::process::exit(1);
    }

    println!(
        "Processing {} total packages from {} lock files",
        all_initial_packages.len(),
        project_count
    );

    // If --info flag is set, just print the parsed packages and exit
    if args.info {
        println!("\n=== PARSED LOCKFILE INFORMATION ===\n");
        println!("Total packages found: {}", all_initial_packages.len());

        // Clone the initial packages for processing
        let mut info_packages = all_initial_packages.clone();

        // Process each package to get URL and license info when available
        for package in &mut info_packages {
            // Try to get cached package info if available
            let package_hash = generate_package_hash(&package);
            if let Some(cached_package) = get_from_cache(&package_hash) {
                if !cached_package.license.is_empty() {
                    package.license = cached_package.license;
                }
                if let Some(ref license_url) = cached_package.license_url {
                    package.license_url = Some(license_url.clone());
                }
                if !cached_package.url.is_empty() {
                    package.url = cached_package.url;
                }
            }
        }

        for package in &info_packages {
            println!("\nPackage: {}", package.name.bold());
            println!("  Version: {}", package.version);
            println!("  Resolution: {}", if package.resolution.is_empty() {
                "<not specified>".italic().to_string()
            } else {
                package.resolution.clone()
            });
            if let Some(checksum) = &package.checksum {
                println!("  Checksum: {}", checksum);
            }
            println!("  URL: {}", package.url);

            // Show license if we have it from cache
            if !package.license.is_empty() && package.license != "UNKNOWN" {
                println!("  License: {}", package.license);
                if let Some(ref license_url) = package.license_url {
                    println!("  License URL: {}", license_url);
                }
            }
        }

        // Print summary of unique registries found based on resolution URLs
        let mut registry_counts: HashMap<&str, usize> = HashMap::new();

        for package in &all_initial_packages {
            let registry = if package.resolution.contains("github.com") {
                "GitHub"
            } else if
                package.resolution.contains("npmjs.org") ||
                package.resolution.contains("npmjs.com")
            {
                "npm"
            } else if package.resolution.is_empty() {
                "Unknown"
            } else {
                "Other"
            };
            *registry_counts.entry(registry).or_insert(0) += 1;
        }

        println!("\n=== REGISTRY SUMMARY ===");
        for (registry, count) in registry_counts {
            println!("{}: {} packages", registry, count);
        }
        println!("\nTo perform full license analysis, run without the --info flag.");
        return; // Exit after printing info
    }

    // Setup shared data structures
    let queue: Arc<Mutex<VecDeque<Package>>> = Arc::new(Mutex::new(VecDeque::new()));
    let processed: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let results: Arc<Mutex<Vec<Package>>> = Arc::new(Mutex::new(Vec::new()));

    // Store parent-child relationships for tree visualization
    let dependency_tree: Arc<Mutex<HashMap<String, Vec<String>>>> = Arc::new(
        Mutex::new(HashMap::new())
    );

    // Add initial packages to queue
    {
        let mut q = queue.lock().unwrap();
        for package in all_initial_packages {
            q.push_back(package);
        }
    }

    // Create worker threads
    let num_threads = 4;
    let mut handles = Vec::new();

    for _ in 0..num_threads {
        let queue_clone = Arc::clone(&queue);
        let processed_clone = Arc::clone(&processed);
        let results_clone = Arc::clone(&results);
        let dependency_tree_clone = Arc::clone(&dependency_tree);
        let retry_flag = args.retry && args.unknown;
        let verbose_flag = args.verbose;
        let debug_flag = args.debug;
        let tree_flag = args.tree;

        let handle = thread::spawn(move || {
            process_queue(
                queue_clone,
                processed_clone,
                results_clone,
                dependency_tree_clone,
                retry_flag,
                verbose_flag,
                debug_flag,
                tree_flag
            );
        });
        handles.push(handle);
    }

    // Wait for all threads to finish
    for handle in handles {
        handle.join().unwrap();
    }

    // Get final results
    let final_results = results.lock().unwrap();

    // Handle CSV output mode
    if args.csv {
        output_csv(&final_results, args.output.as_deref());
        return;
    }

    // Handle tree visualization mode
    if args.tree {
        let dep_tree = dependency_tree.lock().unwrap();
        output_dependency_tree(&dep_tree, &final_results);
        return;
    }

    // Print results with clear formatting (standard output mode)
    println!("\n=== DEPENDENCY LICENSE SUMMARY ===\n");

    let mut violations_count = 0;
    let mut total_packages = 0;
    let mut unknown_count = 0;
    let mut license_counts: HashMap<String, (usize, Option<String>)> = HashMap::new();

    for package_info in final_results.iter() {
        total_packages += 1;

        if package_info.license == "UNKNOWN" {
            unknown_count += 1;
        }

        // Count each license type and store license URL
        license_counts
            .entry(package_info.license.clone())
            .and_modify(|(count, _)| {
                *count += 1;
            })
            .or_insert((1, package_info.license_url.clone()));

        // Check if license is allowed
        let is_allowed = license_checker.is_allowed(&package_info.license);

        if !is_allowed {
            violations_count += 1;
        }

        print_package_info(package_info, is_allowed, args.unknown, args.verbose, args.debug);
    }

    // Print summary
    println!("\nTotal packages processed: {}", total_packages);

    if unknown_count > 0 {
        println!("Packages with unknown licenses: {}", unknown_count.to_string().yellow());
    }

    if !args.allowed.is_empty() {
        if violations_count > 0 {
            println!("{} with non-compliant licenses", violations_count.to_string().red().bold());
        } else {
            println!("{}", "All licenses are compliant!".green());
        }
        println!("Allowed license patterns: {}", args.allowed.join(", "));
    }

    // If unknown flag is set, specifically highlight we're in debugging mode
    if args.unknown {
        println!(
            "\nRunning in {} mode - showing only packages with unknown licenses",
            "DEBUG".bright_cyan().bold()
        );

        // If retry flag is also set, provide additional information
        if args.retry {
            println!(
                "{}",
                "Retry mode enabled - cached results for unknown licenses will be ignored"
                    .bright_cyan()
                    .bold()
            );
        }
    }

    // Print license usage statistics
    println!("\n=== LICENSE USAGE STATISTICS ===");

    // Sort licenses by frequency (most common first)
    let mut license_vec: Vec<(&String, &(usize, Option<String>))> = license_counts.iter().collect();
    license_vec.sort_by(|a, b| b.1.0.cmp(&a.1.0));

    for (license, (count, license_url)) in license_vec {
        let is_allowed = license_checker.is_allowed(&license);
        let percentage = ((*count as f64) / (total_packages as f64)) * 100.0;

        // First try to use the license URL from the standardized mapping
        // This ensures we use the canonical URL for well-known licenses
        let display_url = crate::license_urls
            ::get_license_url(license)
            .or_else(|| license_url.as_ref().map(|url| url.clone()))
            .unwrap_or_else(|| String::new());

        let license_display = if !display_url.is_empty() {
            format!("{} ({})", license, display_url)
        } else {
            license.to_string()
        };

        if is_allowed {
            println!("{}: {} packages ({:.1}%)", license_display, count, percentage);
        } else {
            println!(
                "{}: {} packages ({:.1}%) {}",
                license_display,
                count,
                percentage,
                "[NOT ALLOWED]".red().bold()
            );
        }
    }
    println!("\nScan complete.");

    // Exit with error code if violations found
    if !args.allowed.is_empty() && violations_count > 0 {
        std::process::exit(1);
    }
}

fn process_queue(
    queue: Arc<Mutex<VecDeque<Package>>>,
    processed: Arc<Mutex<HashSet<String>>>,
    results: Arc<Mutex<Vec<Package>>>,
    dependency_tree: Arc<Mutex<HashMap<String, Vec<String>>>>,
    retry_unknown: bool,
    verbose: bool,
    debug: bool,
    track_deps: bool
) {
    loop {
        // Get a package from the queue
        let package_opt = {
            let mut q = queue.lock().unwrap();
            q.pop_front()
        };

        let package = match package_opt {
            Some(p) => p,
            None => {
                // Check if queue is empty for all threads
                let q = queue.lock().unwrap();
                if q.is_empty() {
                    break;
                }
                // If queue was empty now but might get items from other threads, wait a bit
                thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
        };

        // Skip packages with "0.0.0-use.local" in their version
        if should_ignore_package(&package, verbose) {
            continue;
        }

        // Generate package hash
        let package_hash = generate_package_hash(&package);

        // Check if already processed
        {
            let processed_set = processed.lock().unwrap();
            if processed_set.contains(&package_hash) {
                continue;
            }
        }

        // Try to get from cache first (but skip if retry_unknown is true and this is a retry)
        let skip_cache = retry_unknown && package.retry_for_unknown;
        if !skip_cache {
            if let Some(package_info) = get_from_cache(&package_hash) {
                // Only show cache hit message in verbose mode
                if verbose {
                    println!("CACHE HIT: Using cached data for {}", package.name);
                }

                // If retry_unknown is true and the license is still UNKNOWN, mark for retry
                let needs_retry = retry_unknown && package_info.license == "UNKNOWN";

                if !needs_retry {
                    // Standard cache handling for non-retry or non-UNKNOWN packages

                    // Add to processed set
                    {
                        let mut processed_set = processed.lock().unwrap();
                        processed_set.insert(package_hash.clone());
                    }

                    // Add result
                    {
                        let mut results_vec = results.lock().unwrap();
                        results_vec.push(package_info.clone());
                    }

                    // Add dependencies to queue
                    {
                        let mut q = queue.lock().unwrap();
                        for dep in package_info.dependencies.clone() {
                            // Only add to queue if not processed already
                            let dep_hash = generate_package_hash(&dep);
                            let processed_set = processed.lock().unwrap();
                            if !processed_set.contains(&dep_hash) {
                                q.push_back(dep);
                            }
                        }
                    }
                    continue; // Skip to next package since we already processed this one
                } else {
                    // We need to retry this package because it has an UNKNOWN license
                    // and retry_unknown is true
                    // Only show retry message in verbose mode
                    if verbose {
                        println!(
                            "RETRY: Ignoring cached result with UNKNOWN license for {}",
                            package.name
                        );
                    }

                    // Mark this package for retry
                    let mut retry_package = package.clone();
                    retry_package.retry_for_unknown = true;

                    // Continue with processing this package (skip the continue statement)
                }
            }
        }

        // Process the package if not in cache or if retrying
        match process_package(&package, debug) {
            Ok(package_info) => {
                // Add to processed set
                {
                    let mut processed_set = processed.lock().unwrap();
                    processed_set.insert(package_hash.clone());
                }

                // Save to cache
                if let Err(e) = save_to_cache(&package_hash, &package_info) {
                    eprintln!("Warning: Failed to save to cache: {}", e);
                } else if verbose {
                    // Only show cache save message in verbose mode
                    println!("CACHE: Saved {} to cache", package.name);
                }

                // Add result
                {
                    let mut results_vec = results.lock().unwrap();
                    results_vec.push(package_info.clone());
                }

                // Add dependencies to queue
                {
                    let mut q = queue.lock().unwrap();

                    // If tracking dependencies for tree visualization, record parent-child relationships
                    if track_deps && !package_info.dependencies.is_empty() {
                        let mut dep_tree = dependency_tree.lock().unwrap();
                        let parent_id = format!("{}@{}", package_info.name, package_info.version);

                        for dep in &package_info.dependencies {
                            let child_id = format!("{}@{}", dep.name, dep.version);

                            // Add to dependency tree
                            dep_tree
                                .entry(parent_id.clone())
                                .or_insert_with(Vec::new)
                                .push(child_id);
                        }
                    }

                    for dep in package_info.dependencies.clone() {
                        // Only add to queue if not processed already
                        let dep_hash = generate_package_hash(&dep);
                        let processed_set = processed.lock().unwrap();
                        if !processed_set.contains(&dep_hash) {
                            q.push_back(dep);
                        }
                    }
                }
            }
            Err(e) => {
                // Add to processed to avoid retrying
                {
                    let mut processed_set = processed.lock().unwrap();
                    processed_set.insert(package_hash);
                }

                // Add a minimal result for this package to avoid missing it
                {
                    let mut results_vec = results.lock().unwrap();
                    let registry = if
                        package.name.starts_with("github:") ||
                        package.resolution.contains("github:")
                    {
                        "github"
                    } else {
                        "npm"
                    };
                    let registry_url = if registry == "github" {
                        // Extract GitHub URL if present
                        if let Some(github_url) = extract_github_url(&package.resolution) {
                            github_url
                        } else {
                            format!(
                                "https://github.com/{}",
                                package.name.trim_start_matches("github:")
                            )
                        }
                    } else {
                        format!("https://www.FAILnpmjs.com/package/{}", package.name)
                    };
                    // Use the Package::with_error constructor
                    let package_info = Package::with_error(
                        package.name.clone(),
                        package.version.clone(),
                        registry,
                        registry_url,
                        &format!("Error processing package: {}", e)
                    );
                    results_vec.push(package_info);
                }
                eprintln!("Error processing package {}: {}", package.name, e);
            }
        }
    }
}

/// Output unique packages as CSV with name, URL, and license
fn output_csv(packages: &Vec<Package>, output_file: Option<&str>) {
    // Create a map to store unique packages using an improved normalization approach
    let mut unique_packages: HashMap<String, &Package> = HashMap::new();

    // First pass: collect all packages and prefer those with known licenses
    for package in packages {
        let key = generate_unique_package_key(package);

        match unique_packages.get(&key) {
            Some(existing) => {
                // Replace if the new package has a known license and the existing one doesn't
                if existing.license == "UNKNOWN" && package.license != "UNKNOWN" {
                    unique_packages.insert(key, package);
                }
                // Otherwise keep the existing one
            }
            None => {
                unique_packages.insert(key, package);
            }
        }
    }

    // Sort keys for consistent output
    let mut sorted_keys: Vec<_> = unique_packages.keys().collect();
    sorted_keys.sort();

    // Track which package names we've already output to ensure no duplicate entries
    let mut output_names = HashSet::new();

    // Prepare the CSV content
    let mut csv_content = String::new();
    csv_content.push_str("name,url,license\n");

    for key in sorted_keys {
        let package = unique_packages.get(key).unwrap();

        // Create a simple name key for final deduplication check
        let output_key = format!("{}|{}", package.name, package.url);

        // Skip if we've already output this package (final safety check)
        if output_names.contains(&output_key) {
            continue;
        }

        // Clean fields to ensure proper CSV formatting
        let name = package.name.replace(',', " ").replace('"', "'"); // Replace commas and quotes
        let url = package.url.replace(',', " ").replace('"', "'"); // Replace commas and quotes
        let license = package.license.replace(',', " ").replace('"', "'"); // Replace commas and quotes

        let csv_line = format!("\"{}\",\"{}\",\"{}\"\n", name, url, license);
        csv_content.push_str(&csv_line);

        // Mark this package as output
        output_names.insert(output_key);
    }

    // Output CSV content to file or stdout
    match output_file {
        Some(path) => {
            match fs::write(path, csv_content) {
                Ok(_) => println!("CSV data written to {}", path),
                Err(e) => eprintln!("Error writing to file {}: {}", path, e),
            }
        }
        None => {
            // Print to stdout
            print!("{}", csv_content);
        }
    }
}

/// Generate a consistent unique key for a package by normalizing its name and version
fn generate_unique_package_key(package: &Package) -> String {
    // Normalize package name by:
    // - Converting to lowercase
    // - Removing scope prefixes for comparison (but keeping them for display)
    // - Stripping any registry prefixes (like github: or npm:)
    let normalized_name = if package.name.starts_with("github:") {
        // For GitHub packages, extract the repo name
        package.name.trim_start_matches("github:").to_lowercase()
    } else if package.name.starts_with('@') {
        // Keep scoped packages as-is but lowercase
        package.name.to_lowercase()
    } else {
        // Regular packages, just lowercase
        package.name.to_lowercase()
    };

    // Normalize version by:
    // - Removing leading ^ and ~ which are version range indicators
    // - Keeping only the first segment for comparison if this has a complex version
    let normalized_version = package.version
        .trim_start_matches('^')
        .trim_start_matches('~')
        .split('-') // Handle versions like "1.0.0-beta.1"
        .next()
        .unwrap_or(&package.version)
        .to_string();

    // Make URL part of the key to better distinguish same-named packages from different sources
    let normalized_url = package.url.to_lowercase();

    // Construct a compound key that includes all relevant unique identifiers
    format!("{}|{}|{}", normalized_name, normalized_version, normalized_url)
}

/// Output dependency tree visualization
fn output_dependency_tree(dep_tree: &HashMap<String, Vec<String>>, packages: &Vec<Package>) {
    // Find root packages (those that are not dependencies of any other package)
    let mut all_deps = HashSet::new();
    for deps in dep_tree.values() {
        for dep in deps {
            all_deps.insert(dep.clone());
        }
    }

    // Create a map of package_id to package for quick lookup
    let package_map: HashMap<String, &Package> = packages
        .iter()
        .map(|p| (format!("{}@{}", p.name, p.version), p))
        .collect();

    // Find root packages
    let mut root_packages: Vec<String> = Vec::new();
    for package in packages {
        let package_id = format!("{}@{}", package.name, package.version);
        if !all_deps.contains(&package_id) && dep_tree.contains_key(&package_id) {
            root_packages.push(package_id);
        }
    }

    // Sort root packages for consistent output
    root_packages.sort();

    println!("=== DEPENDENCY TREE ===\n");

    // Print tree for each root package
    for (i, root) in root_packages.iter().enumerate() {
        if i > 0 {
            println!(); // Add empty line between root packages
        }

        if let Some(package) = package_map.get(root) {
            println!("{} ({})", package.name.bold(), package.license);
            print_dependencies(root, dep_tree, &package_map, 1, &mut HashSet::new());
        }
    }
}

/// Helper function to recursively print dependencies
fn print_dependencies(
    package_id: &str,
    dep_tree: &HashMap<String, Vec<String>>,
    package_map: &HashMap<String, &Package>,
    level: usize,
    visited: &mut HashSet<String>
) {
    // Check for circular dependencies
    if visited.contains(package_id) {
        let indent = "  ".repeat(level);
        println!("{}└── {} [circular reference]", indent, package_id);
        return;
    }

    // Mark this package as visited
    visited.insert(package_id.to_string());

    // Get dependencies for this package
    if let Some(deps) = dep_tree.get(package_id) {
        let mut sorted_deps = deps.clone();
        sorted_deps.sort();

        for (i, dep_id) in sorted_deps.iter().enumerate() {
            let is_last = i == sorted_deps.len() - 1;
            let indent = "  ".repeat(level);

            if let Some(package) = package_map.get(dep_id) {
                // Print dependency with its license
                let prefix = if is_last { "└── " } else { "├── " };
                println!("{}{}{} ({})", indent, prefix, package.name, package.license);

                // Recursively print dependencies of this dependency
                let next_level = level + 1;
                let next_visited = &mut visited.clone();

                print_dependencies(dep_id, dep_tree, package_map, next_level, next_visited);
            } else {
                // Package not found in map
                let prefix = if is_last { "└── " } else { "├── " };
                println!("{}{}{} [unknown]", indent, prefix, dep_id);
            }
        }
    }

    // Remove from visited set on way back up
    visited.remove(package_id);
}

// Helper function to extract GitHub URL from resolution string if present
fn extract_github_url(resolution: &str) -> Option<String> {
    if resolution.contains("github:") {
        if let Some(github_part) = resolution.split("github:").nth(1) {
            if let Some(repo_path) = github_part.split('#').next() {
                return Some(format!("https://github.com/{}", repo_path));
            }
        }
    }
    None
}

// Helper function to determine if a package should be ignored
fn should_ignore_package(package: &Package, verbose: bool) -> bool {
    // Check if version contains "0.0.0-use.local"
    let should_ignore = package.version.contains("0.0.0-use.local");

    // Only print the message if verbose mode is enabled
    if should_ignore && verbose {
        eprintln!("INFO: Ignoring local package: {}", package.name);
    }

    should_ignore
}

fn process_package(package: &Package, debug: bool) -> Result<Package, Box<dyn std::error::Error>> {
    // Check registry to determine how to process the package
    if package.registry == "nuget" {
        // For NuGet packages, they're already processed during parsing
        // Just return the package as-is since we got all info from nuget-license
        if cfg!(debug_assertions) {
            println!("DEBUG: Processing nuget package: {}", package.name);
        }
        return Ok(package.clone());
    } else if package.registry == "pypi" {
        // For Python packages, use PyPI API
        if cfg!(debug_assertions) || debug {
            println!("DEBUG: Processing pypi package: {}", package.name);
        }
        parsers::poetry_parser::get_package_info(package, debug)
    } else if
        package.resolution.starts_with("https://github.com") ||
        package.name.starts_with("github:")
    {
        // For GitHub packages, use GitHub API
        if cfg!(debug_assertions) {
            println!("DEBUG: Processing github package: {}", package.name);
        }
        github_api::get_package_info(package)
    } else {
        // For everything else (npm, etc.), use npm API
        if cfg!(debug_assertions) {
            println!("DEBUG: Processing npm package: {}", package.name);
        }
        npm_api::get_package_info(package)
    }
}

/// Recursively find supported lock files in a directory
/// Excludes node_modules and .yarn directories
fn find_lockfiles(root_dir: &str) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    let root_path = Path::new(root_dir);

    if !root_path.exists() || !root_path.is_dir() {
        eprintln!("Path does not exist or is not a directory: {}", root_dir);
        return result;
    }

    // Start recursive search
    find_lockfiles_recursive(root_path, &mut result);
    result
}

fn find_lockfiles_recursive(dir: &Path, result: &mut Vec<std::path::PathBuf>) {
    // Skip node_modules, .yarn directories, and .NET build directories
    let dir_name = dir.file_name().unwrap_or_default().to_string_lossy();
    if dir_name == "node_modules" || dir_name == ".yarn" || dir_name == "bin" || dir_name == "obj" {
        return;
    }

    // Check if this directory contains any of our supported lock files
    for lockfile in SUPPORTED_LOCKFILES {
        // Special handling for csproj files which use wildcard
        if *lockfile == "*.csproj" {
            // Find all .csproj files in this directory
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    if path.is_file() && path.extension().map_or(false, |ext| ext == "csproj") {
                        result.push(path);
                    }
                }
            }
        } else {
            // Standard check for exact filename
            let lockfile_path = dir.join(lockfile);
            if lockfile_path.exists() && lockfile_path.is_file() {
                result.push(lockfile_path);
            }
        }
    }

    // Check package.json files (for future use)
    let package_json_path = dir.join("package.json");
    if package_json_path.exists() && package_json_path.is_file() {
        // We found a package.json - note it for future use
        // Currently we don't do anything with it but we might parse it in the future
    }

    // Recurse into subdirectories
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                find_lockfiles_recursive(&path, result);
            }
        }
    }
}

// Helper function to determine if a package should be displayed
fn should_display_package(
    package: &Package,
    is_allowed: bool,
    args_unknown: bool,
    args_verbose: bool,
    args_debug: bool
) -> bool {
    if args_debug {
        // If --debug flag is set, show everything
        return true;
    } else if args_unknown {
        // If --unknown flag is set, only show unknown licenses
        package.license == "UNKNOWN"
    } else if !is_allowed || args_verbose {
        // Otherwise use the normal display logic
        true
    } else {
        false
    }
}

// Helper function to format and print package information
fn print_package_info(
    package: &Package,
    is_allowed: bool,
    args_unknown: bool,
    args_verbose: bool,
    args_debug: bool
) {
    // First determine if the package should be displayed
    let should_display = should_display_package(
        package,
        is_allowed,
        args_unknown,
        args_verbose,
        args_debug
    );

    if !should_display {
        return;
    }

    // Format the registry and name - ensure NuGet packages show correctly
    let registry_name = if package.registry == "nuget" {
        // For NuGet packages, use a consistent format
        format!("nuget/{}", package.display_name)
    } else if package.registry == "pypi" {
        // For Python packages, use a consistent format
        format!("pypi/{}", package.display_name)
    } else if !package.display_name.is_empty() {
        format!("{}/{}", package.registry, package.display_name)
    } else {
        format!("{}@{}", package.name, package.version)
    };

    // Display differently based on license status and verbosity
    if is_allowed && package.license != "UNKNOWN" {
        if args_verbose || args_debug {
            println!(
                "{} ({}): {}{}",
                registry_name,
                package.url,
                package.license,
                package.license_url.as_ref().map_or(String::new(), |url| format!(" ({})", url))
            );

            // In verbose mode, show debug info for all packages
            if let Some(debug_info) = &package.debug_info {
                println!("    Info: {}", debug_info.yellow());
            }

            // In debug mode, show complete raw API response if available
            if args_debug && package.raw_api_response.is_some() {
                println!("\n=== RAW API RESPONSE ===");
                println!("{}", package.raw_api_response.as_ref().unwrap().cyan());
                println!("=== END API RESPONSE ===\n");
            }
        } else {
            println!("{}: {}", registry_name, package.license);
        }
    } else {
        // Display for non-allowed or unknown licenses
        if args_verbose || args_unknown || args_debug {
            println!(
                "{} ({}): {}{}",
                registry_name,
                package.url,
                package.license.red().bold(),
                package.license_url
                    .as_ref()
                    .map_or(String::new(), |url| format!(" ({})", url).red().bold().to_string())
            );

            // Show debug info for all packages in verbose mode, or UNKNOWN in debug mode
            if let Some(debug_info) = &package.debug_info {
                println!("    Info: {}", debug_info.yellow());
            }

            // In debug mode, show complete raw API response if available
            if args_debug && package.raw_api_response.is_some() {
                println!("\n=== RAW API RESPONSE ===");
                println!("{}", package.raw_api_response.as_ref().unwrap().cyan());
                println!("=== END API RESPONSE ===\n");
            }
        } else {
            println!(
                "{}: {}{}",
                registry_name,
                package.license.red().bold(),
                package.license_url
                    .as_ref()
                    .map_or(String::new(), |url| format!(" ({})", url).red().bold().to_string())
            );

            // Show minimal debug info even in non-verbose mode for UNKNOWN licenses
            if package.license == "UNKNOWN" {
                println!("    Registry URL: {}", package.url.yellow());
            }
        }
    }
}
