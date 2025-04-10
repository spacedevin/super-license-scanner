# Dependency Scanner

A Rust-based tool for recursively analyzing dependency trees in JavaScript/TypeScript projects. The scanner currently supports yarn.lock files with plans to add support for package-lock.json, bun.lock, and pnpm-lock.yaml in the future.

## Features

- Recursively analyzes dependencies from lockfiles
- Uses GitHub and npm APIs to retrieve package information without downloading packages
- Extracts license information for each dependency
- Outputs detailed dependency information in a consistent format
- Prevents duplicate processing with hash-based tracking
- Multi-threaded processing for improved performance
- Supports analyzing multiple projects at once

## Requirements

- Rust 1.58 or later
- Cargo package manager

## Installation

1. Clone the repository:
   ```
   git clone <repository-url>
   cd super-license-scanner
   ```

2. Build the project:
   ```
   cargo build --release
   ```

## Usage

Run the dependency scanner with one or more paths to project root directories containing yarn.lock files:

```
cargo run --release -- /path/to/your/project1 /path/to/your/project2
```

Or use the binary directly after building:

```
./target/release/super-license-scanner /path/to/your/project1 /path/to/your/project2
```

recursive
```
cargo run /path/to/your/project1 -r
```

csv
```
cargo run /path/to/your/project1 --csv -o FILENAME.csv
```




## Output Format

The scanner outputs dependency information in the following format:

```
<registry>:<name>@<version>,<license>[,<expiration>]
```

Examples:
- `npm:@user/repo@1.0.0,MIT`
- `github:@user/repo@somegithash,BSD`

## How It Works

1. The scanner parses the yarn.lock file to extract initial dependencies
2. Each dependency is added to a processing queue
3. Worker threads process the queue concurrently:
   - For GitHub dependencies, only the package.json is fetched using the GitHub API
   - For npm dependencies, package metadata is fetched from the npm Registry
4. License information is extracted from each package
5. New dependencies found are added to the queue for processing
6. A hash table prevents processing the same dependency multiple times
7. Results are collected and output when processing completes

## Planned Features

- Support for additional lockfile formats:
  - npm's package-lock.json
  - pnpm-lock.yaml
  - bun.lock
- Improved error handling and retry logic
- Authentication support for GitHub API to increase rate limits
- Configurable thread count for parallel processing
- Output formats (JSON, CSV, etc.)
- License compliance analysis
- Dependency graph visualization
