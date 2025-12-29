//! Reticle CLI
//!
//! Command-line interface for the Reticle MCP debugging proxy.
//!
//! # Commands
//!
//! - `reticle run [OPTIONS] -- <COMMAND>` - Wrap stdio-based MCP servers
//! - `reticle proxy` - HTTP reverse proxy for remote MCP servers
//! - `reticle daemon` - Start the Reticle daemon (hub for CLI instances)
//! - `reticle ui` - Launch the Reticle GUI dashboard
//!
//! # Architecture: Hub-and-Spoke
//!
//! Reticle uses a distributed sidecar pattern:
//! - **Hub (daemon)**: Listens on `/tmp/reticle.sock` and aggregates telemetry
//! - **Spoke (run/proxy)**: Each instance wraps an MCP server and streams
//!   telemetry to the Hub
//!
//! # Usage
//!
//! ```bash
//! # Start the daemon first (or use the GUI which includes it)
//! reticle daemon
//!
//! # In claude_desktop_config.json:
//! {
//!   "mcpServers": {
//!     "github": {
//!       "command": "reticle",
//!       "args": ["run", "--name", "github", "--", "npx", "-y", "@modelcontextprotocol/server-github"]
//!     },
//!     "remote-api": {
//!       "command": "reticle",
//!       "args": ["proxy", "--name", "api", "--upstream", "http://localhost:8080", "--listen", "3001"]
//!     }
//!   }
//! }
//! ```
//!
//! # Fail-Open Design
//!
//! The CLI wrappers are designed to "fail open" - if the daemon is not running,
//! they continue to proxy traffic normally. Observability is optional;
//! agent functionality is never degraded.

use clap::{Parser, Subcommand};
use reticle_core::events::{InjectReceiver, NoOpEventSink, StdoutEventSink, UnixSocketEventSink};
use std::process::ExitCode;
use tracing_subscriber::EnvFilter;

mod daemon;
mod http_proxy;
mod proxy;

/// Reticle - The Wireshark for the Model Context Protocol
///
/// A high-performance observability proxy for MCP servers.
/// Intercepts JSON-RPC traffic between hosts (Claude, IDEs) and MCP servers.
#[derive(Parser, Debug)]
#[command(name = "reticle")]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Wrap a stdio-based MCP server
    ///
    /// Spawns the given command and proxies stdin/stdout traffic while
    /// streaming telemetry to the Reticle daemon.
    ///
    /// Use this when:
    /// - The MCP server uses stdio transport (most common)
    /// - You want to wrap an existing MCP server command
    /// - Debugging Claude Desktop, Continue, or other stdio-based hosts
    ///
    /// Example:
    ///   reticle run --name github -- npx -y @modelcontextprotocol/server-github
    #[command(name = "run", alias = "wrap")]
    Run {
        /// Server name for identification in the dashboard
        #[arg(short, long)]
        name: Option<String>,

        /// Socket path for daemon connection
        #[arg(short, long, env = "RETICLE_SOCKET")]
        socket: Option<String>,

        /// Disable telemetry (pure proxy mode)
        #[arg(long)]
        no_telemetry: bool,

        /// Output logs to stderr (standalone mode, no daemon needed)
        #[arg(long)]
        log: bool,

        /// Log output format
        #[arg(long, value_enum, default_value = "text")]
        format: LogFormat,

        /// The command and arguments to run
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },

    /// HTTP reverse proxy for remote MCP servers
    ///
    /// Creates an HTTP proxy that intercepts traffic to a remote MCP server
    /// (SSE, Streamable HTTP, WebSocket) and streams telemetry to the daemon.
    ///
    /// Use this when:
    /// - The MCP server uses HTTP/SSE/WebSocket transport
    /// - The MCP server is remote or started separately
    /// - You need to debug HTTP-based MCP traffic
    ///
    /// Example:
    ///   reticle proxy --name api --upstream http://localhost:8080 --listen 3001
    Proxy {
        /// Server name for identification in the dashboard
        #[arg(short, long, required = true)]
        name: String,

        /// Local port to listen on
        #[arg(short, long, default_value = "3001")]
        listen: u16,

        /// Upstream MCP server URL
        #[arg(short, long, required = true)]
        upstream: String,

        /// Socket path for daemon connection
        #[arg(long, env = "RETICLE_SOCKET")]
        socket: Option<String>,

        /// Disable telemetry (pure proxy mode)
        #[arg(long)]
        no_telemetry: bool,
    },

    /// Start the Reticle daemon (telemetry hub)
    ///
    /// The daemon listens on a Unix socket and receives telemetry from
    /// all CLI instances. It can forward events to the GUI or operate standalone.
    ///
    /// Typically you don't need to run this manually - the Reticle GUI
    /// includes the daemon. Use this for headless/server deployments.
    ///
    /// Example:
    ///   reticle daemon                          # Default socket
    ///   reticle daemon --socket /tmp/my.sock    # Custom socket
    Daemon {
        /// Unix socket path to listen on
        #[arg(short, long, default_value = "/tmp/reticle.sock")]
        socket: String,

        /// Optional TCP port for remote connections
        #[arg(short, long)]
        port: Option<u16>,

        /// Output received events to stdout (for debugging)
        #[arg(long)]
        verbose: bool,
    },

    /// Launch the Reticle GUI dashboard
    ///
    /// Opens the graphical dashboard for monitoring MCP traffic.
    /// The GUI includes a built-in daemon, so CLI instances will
    /// automatically connect and display their traffic.
    ///
    /// The GUI binary (reticle-app) must be installed alongside the CLI.
    /// It's typically found in the same directory or a standard location.
    ///
    /// Example:
    ///   reticle ui              # Launch GUI in foreground
    ///   reticle ui --detach     # Launch GUI and return immediately
    ///   reticle ui --dev        # Run in development mode (cargo tauri dev)
    #[command(name = "ui", alias = "gui")]
    Ui {
        /// Detach the GUI process (run in background)
        #[arg(short, long)]
        detach: bool,

        /// Run in development mode using cargo tauri dev
        #[arg(long)]
        dev: bool,
    },
}

#[derive(Debug, Clone, Default, clap::ValueEnum)]
enum LogFormat {
    /// Human-readable text output
    #[default]
    Text,
    /// JSON output (one object per line)
    Json,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            name,
            socket,
            no_telemetry,
            log,
            format,
            command,
        } => run_stdio(name, socket, no_telemetry, log, format, command).await,

        Commands::Proxy {
            name,
            listen,
            upstream,
            socket,
            no_telemetry,
        } => run_proxy(name, listen, upstream, socket, no_telemetry).await,

        Commands::Daemon {
            socket,
            port,
            verbose,
        } => run_daemon(socket, port, verbose).await,

        Commands::Ui { detach, dev } => run_ui(detach, dev).await,
    }
}

/// Run stdio proxy mode
async fn run_stdio(
    name: Option<String>,
    socket: Option<String>,
    no_telemetry: bool,
    log: bool,
    format: LogFormat,
    command: Vec<String>,
) -> ExitCode {
    if command.is_empty() {
        eprintln!("Error: No command specified");
        eprintln!("Usage: reticle --name <NAME> -- <COMMAND> [ARGS...]");
        return ExitCode::FAILURE;
    }

    let cmd = &command[0];
    let args: Vec<&str> = command[1..].iter().map(|s| s.as_str()).collect();
    let server_name = name.unwrap_or_else(|| extract_server_name(cmd));

    // Decide which event sink to use
    if log {
        // Standalone log mode - output to stderr
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();

        let json_output = matches!(format, LogFormat::Json);
        let event_sink = StdoutEventSink::new(json_output);
        tracing::info!("Starting Reticle for '{}' (log mode)", server_name);
        run_proxy_with_sink(cmd, &args, &server_name, event_sink, None).await
    } else if no_telemetry {
        // Pure proxy mode - no telemetry
        run_proxy_with_sink(cmd, &args, &server_name, NoOpEventSink, None).await
    } else {
        // Connect to daemon (fail-open: continues even if daemon unavailable)
        if let Some(path) = socket {
            std::env::set_var("RETICLE_SOCKET", path);
        }

        let (event_sink, inject_rx) = UnixSocketEventSink::new(server_name.clone()).await;
        run_proxy_with_sink(cmd, &args, &server_name, event_sink, Some(inject_rx)).await
    }
}

/// Run HTTP proxy mode
async fn run_proxy(
    name: String,
    listen: u16,
    upstream: String,
    socket: Option<String>,
    no_telemetry: bool,
) -> ExitCode {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    if no_telemetry {
        eprintln!("[reticle proxy] Running in pure proxy mode (no telemetry)");
        let event_sink = http_proxy::HttpEventSink::NoOp(NoOpEventSink);
        match http_proxy::run_http_proxy(upstream, listen, name, event_sink, None).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[reticle proxy] Error: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        // Connect to daemon (fail-open)
        if let Some(path) = socket {
            std::env::set_var("RETICLE_SOCKET", path);
        }

        let (unix_sink, inject_rx) = UnixSocketEventSink::new(name.clone()).await;
        let event_sink = http_proxy::HttpEventSink::UnixSocket(std::sync::Arc::new(unix_sink));

        match http_proxy::run_http_proxy(upstream, listen, name, event_sink, Some(inject_rx)).await
        {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[reticle proxy] Error: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

/// Run daemon mode
async fn run_daemon(socket: String, port: Option<u16>, verbose: bool) -> ExitCode {
    let level = if verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level)),
        )
        .with_target(false)
        .init();

    tracing::info!("Starting Reticle daemon");
    tracing::info!("  Socket: {}", socket);
    if let Some(p) = port {
        tracing::info!("  TCP port: {}", p);
    }

    match daemon::run_daemon(&socket, port, verbose).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[reticle daemon] Error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// GitHub repository for releases
const GITHUB_REPO: &str = "labterminal/reticle";
const GITHUB_RELEASES_URL: &str = "https://github.com/labterminal/reticle/releases";

/// Launch the Reticle GUI
///
/// Precedence for finding/obtaining the GUI binary:
/// 1. If --dev flag, run cargo tauri dev
/// 2. Check local binary locations (find_gui_binary)
/// 3. Check for debug build in target directory
/// 4. If not found, attempt to download latest release from GitHub
/// 5. If download fails, open GitHub releases page in browser
async fn run_ui(detach: bool, dev: bool) -> ExitCode {
    // Dev mode: run cargo tauri dev
    if dev {
        return run_ui_dev().await;
    }

    // Step 1: Check for local GUI binary (release builds)
    if let Some(path) = find_gui_binary() {
        return launch_gui(&path, detach);
    }

    // Step 2: Check for debug build in target directory
    if let Some(path) = find_debug_gui_binary() {
        eprintln!("Found debug build: {}", path.display());
        return launch_gui(&path, detach);
    }

    eprintln!("Reticle GUI (reticle-app) not found locally.");
    eprintln!("Attempting to download latest release from GitHub...");

    // Step 3: Try to download from GitHub releases
    match download_gui_binary().await {
        Ok(path) => {
            eprintln!("Successfully downloaded GUI to: {}", path.display());
            return launch_gui(&path, detach);
        }
        Err(e) => {
            eprintln!("Failed to download GUI: {e}");
        }
    }

    // Step 4: Fallback to opening GitHub releases page
    eprintln!();
    eprintln!("Opening GitHub releases page in your browser...");
    if let Err(e) = open::that(GITHUB_RELEASES_URL) {
        eprintln!("Failed to open browser: {e}");
        eprintln!();
        eprintln!("Please download the GUI manually from:");
        eprintln!("  {GITHUB_RELEASES_URL}");
        eprintln!();
        eprintln!("After downloading, install to one of these locations:");
        eprintln!("  - ~/.local/bin/reticle-app");
        eprintln!("  - /usr/local/bin/reticle-app");
        #[cfg(target_os = "macos")]
        eprintln!("  - /Applications/Reticle.app");
    } else {
        eprintln!();
        eprintln!("Download the appropriate binary for your platform and install to:");
        eprintln!("  - ~/.local/bin/reticle-app");
        #[cfg(target_os = "macos")]
        eprintln!("  - Or drag Reticle.app to /Applications");
    }

    ExitCode::FAILURE
}

/// Run the GUI in development mode using cargo tauri dev
async fn run_ui_dev() -> ExitCode {
    // Find the project root (where src-tauri is located)
    let project_root = find_project_root();

    match project_root {
        Some(root) => {
            eprintln!("Starting Reticle GUI in development mode...");
            eprintln!("Project root: {}", root.display());

            let src_tauri = root.join("src-tauri");
            if !src_tauri.exists() {
                eprintln!(
                    "Error: src-tauri directory not found at {}",
                    src_tauri.display()
                );
                return ExitCode::FAILURE;
            }

            // Run cargo tauri dev
            match std::process::Command::new("cargo")
                .arg("tauri")
                .arg("dev")
                .current_dir(&src_tauri)
                .status()
            {
                Ok(status) => {
                    if status.success() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(status.code().unwrap_or(1) as u8)
                    }
                }
                Err(e) => {
                    eprintln!("Failed to run cargo tauri dev: {e}");
                    eprintln!();
                    eprintln!("Make sure you have tauri-cli installed:");
                    eprintln!("  cargo install tauri-cli");
                    ExitCode::FAILURE
                }
            }
        }
        None => {
            eprintln!("Error: Could not find Reticle project root.");
            eprintln!();
            eprintln!("The --dev flag requires running from within the Reticle source tree,");
            eprintln!("or the RETICLE_PROJECT_ROOT environment variable must be set.");
            ExitCode::FAILURE
        }
    }
}

/// Find the Reticle project root directory
fn find_project_root() -> Option<std::path::PathBuf> {
    // Check environment variable first
    if let Ok(root) = std::env::var("RETICLE_PROJECT_ROOT") {
        let path = std::path::PathBuf::from(root);
        if path.join("src-tauri").exists() {
            return Some(path);
        }
    }

    // Try to find it relative to the CLI binary
    if let Ok(exe) = std::env::current_exe() {
        // If running from target/release or target/debug, go up
        let mut current = exe.parent();
        while let Some(dir) = current {
            if dir.join("src-tauri").exists() {
                return Some(dir.to_path_buf());
            }
            // Check if we're in target/release or target/debug
            if dir
                .file_name()
                .map(|n| n == "release" || n == "debug")
                .unwrap_or(false)
            {
                if let Some(target) = dir.parent() {
                    if target.file_name().map(|n| n == "target").unwrap_or(false) {
                        if let Some(project) = target.parent() {
                            if project.join("src-tauri").exists() {
                                return Some(project.to_path_buf());
                            }
                        }
                    }
                }
            }
            current = dir.parent();
        }
    }

    // Try current working directory and ancestors
    if let Ok(cwd) = std::env::current_dir() {
        let mut current = Some(cwd.as_path());
        while let Some(dir) = current {
            if dir.join("src-tauri").exists() {
                return Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
    }

    None
}

/// Find debug build of the GUI binary
fn find_debug_gui_binary() -> Option<std::path::PathBuf> {
    // Look for debug build in common locations
    let project_root = find_project_root()?;

    let debug_paths = [
        project_root.join("target/debug/reticle-app"),
        project_root.join("src-tauri/target/debug/reticle-app"),
    ];

    debug_paths.into_iter().find(|path| path.exists())
}

/// Launch the GUI binary
fn launch_gui(path: &std::path::Path, detach: bool) -> ExitCode {
    if detach {
        // Spawn detached process
        match std::process::Command::new(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => {
                eprintln!("Reticle GUI launched in background");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Failed to launch GUI: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        // Run in foreground (wait for exit)
        match std::process::Command::new(path).status() {
            Ok(status) => {
                if status.success() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(status.code().unwrap_or(1) as u8)
                }
            }
            Err(e) => {
                eprintln!("Failed to launch GUI: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

/// Download the GUI binary from GitHub releases
async fn download_gui_binary() -> Result<std::path::PathBuf, String> {
    // Determine the asset name based on current platform
    let asset_name = get_platform_asset_name()?;

    // Get the latest release info from GitHub API
    let client = reqwest::Client::builder()
        .user_agent("reticle-cli")
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let api_url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");

    let release: serde_json::Value = client
        .get(&api_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release info: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse release info: {e}"))?;

    // Find the asset matching our platform
    let assets = release["assets"]
        .as_array()
        .ok_or("No assets found in release")?;

    let asset = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .map(|n| n.contains(&asset_name))
                .unwrap_or(false)
        })
        .ok_or_else(|| format!("No asset found for platform: {asset_name}"))?;

    let download_url = asset["browser_download_url"]
        .as_str()
        .ok_or("No download URL in asset")?;

    let asset_filename = asset["name"].as_str().ok_or("No filename in asset")?;

    eprintln!("Downloading: {asset_filename}");

    // Download the asset
    let response = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download asset: {e}"))?;

    let _total_size = response.content_length().unwrap_or(0);
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {e}"))?;

    eprintln!("Downloaded {} bytes", bytes.len());

    // Determine install location
    let install_dir = get_install_directory()?;
    std::fs::create_dir_all(&install_dir)
        .map_err(|e| format!("Failed to create install directory: {e}"))?;

    // Extract or copy the binary
    let binary_path = if asset_filename.ends_with(".tar.gz") {
        extract_tarball(&bytes, &install_dir, &asset_name)?
    } else if asset_filename.ends_with(".zip") {
        extract_zip(&bytes, &install_dir, &asset_name)?
    } else {
        // Assume it's a direct binary download
        let dest = install_dir.join("reticle-app");
        std::fs::write(&dest, &bytes).map_err(|e| format!("Failed to write binary: {e}"))?;
        dest
    };

    // Make the binary executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&binary_path)
            .map_err(|e| format!("Failed to get permissions: {e}"))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&binary_path, perms)
            .map_err(|e| format!("Failed to set permissions: {e}"))?;
    }

    Ok(binary_path)
}

/// Get the asset name pattern for the current platform
fn get_platform_asset_name() -> Result<String, String> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        return Err("Unsupported operating system".to_string());
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return Err("Unsupported architecture".to_string());
    };

    Ok(format!("reticle-app-{os}-{arch}"))
}

/// Get the directory to install the GUI binary
fn get_install_directory() -> Result<std::path::PathBuf, String> {
    // Prefer ~/.local/bin for user-level installation
    if let Some(home) = dirs::home_dir() {
        let local_bin = home.join(".local").join("bin");
        return Ok(local_bin);
    }

    Err("Could not determine home directory".to_string())
}

/// Extract a tarball and return the path to the binary
fn extract_tarball(
    data: &[u8],
    dest_dir: &std::path::Path,
    _asset_name: &str,
) -> Result<std::path::PathBuf, String> {
    let decoder = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);

    let binary_name = if cfg!(target_os = "windows") {
        "reticle-app.exe"
    } else {
        "reticle-app"
    };

    for entry in archive
        .entries()
        .map_err(|e| format!("Failed to read archive: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("Failed to read entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("Failed to get path: {e}"))?;

        // Look for the binary in the archive
        if let Some(name) = path.file_name() {
            if name == binary_name {
                let dest_path = dest_dir.join(binary_name);
                let mut file = std::fs::File::create(&dest_path)
                    .map_err(|e| format!("Failed to create file: {e}"))?;
                std::io::copy(&mut entry, &mut file)
                    .map_err(|e| format!("Failed to extract file: {e}"))?;
                return Ok(dest_path);
            }
        }
    }

    Err(format!("Binary '{binary_name}' not found in archive"))
}

/// Extract a zip file and return the path to the binary
fn extract_zip(
    data: &[u8],
    dest_dir: &std::path::Path,
    _asset_name: &str,
) -> Result<std::path::PathBuf, String> {
    let reader = std::io::Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("Failed to read zip archive: {e}"))?;

    let binary_name = if cfg!(target_os = "windows") {
        "reticle-app.exe"
    } else {
        "reticle-app"
    };

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read zip entry: {e}"))?;

        if let Some(name) = file.name().split('/').next_back() {
            if name == binary_name {
                let dest_path = dest_dir.join(binary_name);
                let mut dest_file = std::fs::File::create(&dest_path)
                    .map_err(|e| format!("Failed to create file: {e}"))?;
                std::io::copy(&mut file, &mut dest_file)
                    .map_err(|e| format!("Failed to extract file: {e}"))?;
                return Ok(dest_path);
            }
        }
    }

    Err(format!("Binary '{binary_name}' not found in archive"))
}

/// Find the GUI binary in standard locations
fn find_gui_binary() -> Option<std::path::PathBuf> {
    // Get our own executable path so we don't accidentally launch ourselves
    let self_exe = std::env::current_exe().ok();

    // Possible names for the GUI binary
    // Note: We look for "reticle-app" first to avoid finding the CLI binary
    let gui_names = if cfg!(target_os = "macos") {
        vec!["reticle-app", "Reticle"]
    } else if cfg!(target_os = "windows") {
        vec!["reticle-app.exe", "Reticle.exe"]
    } else {
        vec!["reticle-app"]
    };

    // Helper to check if a path is ourselves
    let is_self = |path: &std::path::PathBuf| -> bool {
        if let Some(ref self_path) = self_exe {
            // Compare canonical paths to handle symlinks
            match (path.canonicalize(), self_path.canonicalize()) {
                (Ok(a), Ok(b)) => a == b,
                _ => false,
            }
        } else {
            false
        }
    };

    // 1. macOS: Check for .app bundle first (most reliable)
    #[cfg(target_os = "macos")]
    {
        let app_paths = [
            "/Applications/Reticle.app/Contents/MacOS/Reticle",
            "~/Applications/Reticle.app/Contents/MacOS/Reticle",
        ];
        for path in app_paths {
            let expanded = shellexpand::tilde(path);
            let candidate = std::path::PathBuf::from(expanded.as_ref());
            if candidate.exists() && !is_self(&candidate) {
                return Some(candidate);
            }
        }
    }

    // 2. Check same directory as the CLI binary
    if let Some(ref exe_path) = self_exe {
        if let Some(exe_dir) = exe_path.parent() {
            for name in &gui_names {
                let candidate = exe_dir.join(name);
                if candidate.exists() && !is_self(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    // 3. Check common bin directories
    let bin_dirs = ["~/.local/bin", "/usr/local/bin", "/opt/homebrew/bin"];

    for dir in bin_dirs {
        let expanded = shellexpand::tilde(dir);
        let dir_path = std::path::PathBuf::from(expanded.as_ref());
        for name in &gui_names {
            let candidate = dir_path.join(name);
            if candidate.exists() && !is_self(&candidate) {
                return Some(candidate);
            }
        }
    }

    // 4. Check PATH
    for name in &gui_names {
        if let Ok(path) = which::which(name) {
            if !is_self(&path) {
                return Some(path);
            }
        }
    }

    None
}

/// Extract server name from command path
fn extract_server_name(cmd: &str) -> String {
    std::path::Path::new(cmd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "mcp-server".to_string())
}

/// Run the proxy with a given event sink
async fn run_proxy_with_sink<S: reticle_core::events::EventSink + 'static>(
    cmd: &str,
    args: &[&str],
    server_name: &str,
    event_sink: S,
    inject_rx: Option<InjectReceiver>,
) -> ExitCode {
    match proxy::run_stdio_proxy(cmd, args, server_name, event_sink, inject_rx).await {
        Ok(exit_code) => {
            if exit_code == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(exit_code as u8)
            }
        }
        Err(e) => {
            eprintln!("[reticle] Error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_extract_server_name_simple() {
        assert_eq!(extract_server_name("node"), "node");
        assert_eq!(extract_server_name("python3"), "python3");
        assert_eq!(extract_server_name("npx"), "npx");
    }

    #[test]
    fn test_extract_server_name_with_path() {
        assert_eq!(extract_server_name("/usr/bin/node"), "node");
        assert_eq!(
            extract_server_name("/home/user/.local/bin/mcp-server"),
            "mcp-server"
        );
        assert_eq!(extract_server_name("./scripts/server.py"), "server.py");
    }

    #[test]
    fn test_extract_server_name_empty() {
        assert_eq!(extract_server_name(""), "mcp-server");
    }

    // Run subcommand tests

    #[test]
    fn test_cli_run_basic() {
        // Basic run usage: reticle run -- echo hello
        let cli = Cli::parse_from(["reticle", "run", "--", "echo", "hello"]);
        match cli.command {
            Commands::Run {
                name,
                command,
                no_telemetry,
                log,
                ..
            } => {
                assert!(name.is_none());
                assert_eq!(command, vec!["echo", "hello"]);
                assert!(!no_telemetry);
                assert!(!log);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_run_with_name() {
        let cli = Cli::parse_from([
            "reticle",
            "run",
            "--name",
            "github",
            "--",
            "npx",
            "mcp-server",
        ]);
        match cli.command {
            Commands::Run { name, command, .. } => {
                assert_eq!(name, Some("github".to_string()));
                assert_eq!(command, vec!["npx", "mcp-server"]);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_run_with_socket() {
        let cli = Cli::parse_from([
            "reticle",
            "run",
            "--socket",
            "/tmp/custom.sock",
            "--",
            "node",
            "server.js",
        ]);
        match cli.command {
            Commands::Run {
                socket, command, ..
            } => {
                assert_eq!(socket, Some("/tmp/custom.sock".to_string()));
                assert_eq!(command, vec!["node", "server.js"]);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_run_no_telemetry() {
        let cli = Cli::parse_from([
            "reticle",
            "run",
            "--no-telemetry",
            "--",
            "python",
            "server.py",
        ]);
        match cli.command {
            Commands::Run {
                no_telemetry,
                command,
                ..
            } => {
                assert!(no_telemetry);
                assert_eq!(command, vec!["python", "server.py"]);
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_run_log_mode() {
        let cli = Cli::parse_from(["reticle", "run", "--log", "--format", "json", "--", "echo"]);
        match cli.command {
            Commands::Run { log, format, .. } => {
                assert!(log);
                assert!(matches!(format, LogFormat::Json));
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_run_all_options() {
        let cli = Cli::parse_from([
            "reticle",
            "run",
            "--name",
            "test-server",
            "--socket",
            "/tmp/test.sock",
            "--log",
            "--format",
            "json",
            "--",
            "npx",
            "-y",
            "@modelcontextprotocol/server-github",
        ]);
        match cli.command {
            Commands::Run {
                name,
                socket,
                log,
                format,
                command,
                ..
            } => {
                assert_eq!(name, Some("test-server".to_string()));
                assert_eq!(socket, Some("/tmp/test.sock".to_string()));
                assert!(log);
                assert!(matches!(format, LogFormat::Json));
                assert_eq!(
                    command,
                    vec!["npx", "-y", "@modelcontextprotocol/server-github"]
                );
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_run_complex_command_args() {
        let cli = Cli::parse_from([
            "reticle",
            "run",
            "--name",
            "fs",
            "--",
            "npx",
            "-y",
            "@anthropic/mcp-server-filesystem",
            "/home/user",
            "--readonly",
        ]);
        match cli.command {
            Commands::Run { name, command, .. } => {
                assert_eq!(name, Some("fs".to_string()));
                assert_eq!(
                    command,
                    vec![
                        "npx",
                        "-y",
                        "@anthropic/mcp-server-filesystem",
                        "/home/user",
                        "--readonly"
                    ]
                );
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_run_short_flags() {
        let cli = Cli::parse_from([
            "reticle",
            "run",
            "-n",
            "myserver",
            "-s",
            "/tmp/s.sock",
            "--",
            "node",
            "index.js",
        ]);
        match cli.command {
            Commands::Run { name, socket, .. } => {
                assert_eq!(name, Some("myserver".to_string()));
                assert_eq!(socket, Some("/tmp/s.sock".to_string()));
            }
            _ => panic!("Expected Run command"),
        }
    }

    #[test]
    fn test_cli_wrap_alias() {
        // Test "reticle wrap" alias for run
        let cli = Cli::parse_from(["reticle", "wrap", "--", "echo", "hello"]);
        assert!(matches!(cli.command, Commands::Run { .. }));
    }

    // Proxy subcommand tests

    #[test]
    fn test_cli_proxy() {
        let cli = Cli::parse_from([
            "reticle",
            "proxy",
            "--name",
            "api",
            "--upstream",
            "http://localhost:8080",
            "--listen",
            "3001",
        ]);
        match cli.command {
            Commands::Proxy {
                name,
                listen,
                upstream,
                ..
            } => {
                assert_eq!(name, "api");
                assert_eq!(listen, 3001);
                assert_eq!(upstream, "http://localhost:8080");
            }
            _ => panic!("Expected Proxy command"),
        }
    }

    #[test]
    fn test_cli_proxy_no_telemetry() {
        let cli = Cli::parse_from([
            "reticle",
            "proxy",
            "--name",
            "test",
            "--upstream",
            "http://localhost:8080",
            "--no-telemetry",
        ]);
        match cli.command {
            Commands::Proxy { no_telemetry, .. } => {
                assert!(no_telemetry);
            }
            _ => panic!("Expected Proxy command"),
        }
    }

    // Daemon subcommand tests

    #[test]
    fn test_cli_daemon() {
        let cli = Cli::parse_from(["reticle", "daemon", "--socket", "/tmp/test.sock"]);
        match cli.command {
            Commands::Daemon {
                socket,
                port,
                verbose,
            } => {
                assert_eq!(socket, "/tmp/test.sock");
                assert!(port.is_none());
                assert!(!verbose);
            }
            _ => panic!("Expected Daemon command"),
        }
    }

    #[test]
    fn test_cli_daemon_default_socket() {
        let cli = Cli::parse_from(["reticle", "daemon"]);
        match cli.command {
            Commands::Daemon { socket, .. } => {
                assert_eq!(socket, "/tmp/reticle.sock");
            }
            _ => panic!("Expected Daemon command"),
        }
    }

    #[test]
    fn test_cli_daemon_with_port() {
        let cli = Cli::parse_from(["reticle", "daemon", "--port", "9315"]);
        match cli.command {
            Commands::Daemon { port, .. } => {
                assert_eq!(port, Some(9315));
            }
            _ => panic!("Expected Daemon command"),
        }
    }

    #[test]
    fn test_cli_daemon_verbose() {
        let cli = Cli::parse_from(["reticle", "daemon", "--verbose"]);
        match cli.command {
            Commands::Daemon { verbose, .. } => {
                assert!(verbose);
            }
            _ => panic!("Expected Daemon command"),
        }
    }

    // UI subcommand tests

    #[test]
    fn test_cli_ui() {
        let cli = Cli::parse_from(["reticle", "ui"]);
        match cli.command {
            Commands::Ui { detach, dev } => {
                assert!(!detach);
                assert!(!dev);
            }
            _ => panic!("Expected Ui command"),
        }
    }

    #[test]
    fn test_cli_ui_detach() {
        let cli = Cli::parse_from(["reticle", "ui", "--detach"]);
        match cli.command {
            Commands::Ui { detach, dev } => {
                assert!(detach);
                assert!(!dev);
            }
            _ => panic!("Expected Ui command"),
        }
    }

    #[test]
    fn test_cli_ui_dev() {
        let cli = Cli::parse_from(["reticle", "ui", "--dev"]);
        match cli.command {
            Commands::Ui { detach, dev } => {
                assert!(!detach);
                assert!(dev);
            }
            _ => panic!("Expected Ui command"),
        }
    }

    #[test]
    fn test_cli_gui_alias() {
        let cli = Cli::parse_from(["reticle", "gui"]);
        assert!(matches!(cli.command, Commands::Ui { .. }));
    }

    // Utility tests

    #[test]
    fn test_log_format_default() {
        let format = LogFormat::default();
        assert!(matches!(format, LogFormat::Text));
    }
}
