// use anyhow::Context; // For legacy contexts if any remain, though mostly removed
use zero_kelvin_stazis::error::ZksError;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use rand::Rng;
use zero_kelvin_stazis::constants::{ALLOWED_ROOT_CMDS, DEFAULT_ZSTD_COMPRESSION, LUKS_HEADER_SIZE, LUKS_SAFETY_BUFFER};
use zero_kelvin_stazis::executor::{CommandExecutor, RealSystem};

/// Global path for cleanup on interrupt (SIGINT/SIGTERM)
/// Used by ctrlc handler to remove incomplete output files
static CLEANUP_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static CLEANUP_MAPPER: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(serde::Deserialize)]
struct RootCmdConfig {
    #[serde(default)]
    default: String,
    #[serde(default)]
    allowed: Vec<String>,
}

/// Validate that a command name contains only safe characters: [a-zA-Z0-9_-]
fn is_valid_cmd_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Load optional config from ~/.config/stazis/allowed_root_cmds.yaml
/// Returns None if file doesn't exist or fails validation.
fn load_root_cmd_config() -> Option<RootCmdConfig> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    let config_dir = env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = env::var("HOME").unwrap_or_default();
            format!("{}/.config", home)
        });
    let config_path = PathBuf::from(config_dir).join("stazis/allowed_root_cmds.yaml");

    if !config_path.exists() {
        return None;
    }

    // Security: verify file is not a symlink, owned by us, and not world-readable
    let meta = match fs::symlink_metadata(&config_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Warning: cannot stat config {:?}: {}",
                config_path, e
            );
            return None;
        }
    };

    if meta.file_type().is_symlink() {
        eprintln!(
            "Warning: config {:?} is a symlink, ignoring for security.",
            config_path
        );
        return None;
    }

    let uid = unsafe { libc::getuid() };
    if meta.uid() != uid {
        eprintln!(
            "Warning: config {:?} is owned by uid {} (expected {}), ignoring.",
            config_path,
            meta.uid(),
            uid
        );
        return None;
    }

    let mode = meta.permissions().mode();
    if mode & 0o077 != 0 {
        eprintln!(
            "Warning: config {:?} has insecure permissions {:04o} (expected 0600), ignoring.",
            config_path,
            mode & 0o777
        );
        return None;
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: cannot read config {:?}: {}", config_path, e);
            return None;
        }
    };

    let config: RootCmdConfig = match serde_yaml::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: invalid YAML in {:?}: {}", config_path, e);
            return None;
        }
    };

    // Validate allowed list entries
    for cmd in &config.allowed {
        if !is_valid_cmd_name(cmd) {
            eprintln!(
                "Warning: invalid command name '{}' in config {:?}, ignoring entire config.",
                cmd, config_path
            );
            return None;
        }
    }

    // Validate default is in allowed list (if set)
    if !config.default.is_empty() && !config.allowed.iter().any(|c| c == &config.default) {
        eprintln!(
            "Warning: default '{}' is not in allowed list in {:?}, ignoring entire config.",
            config.default, config_path
        );
        return None;
    }

    Some(config)
}

fn get_effective_root_cmd() -> Vec<String> {
    // Check if we are root via 'id -u'
    if let Ok(output) = std::process::Command::new("id").arg("-u").output() {
        if let Ok(uid_str) = String::from_utf8(output.stdout) {
            if let Ok(uid) = uid_str.trim().parse::<u32>() {
                if uid == 0 {
                    return vec![];
                }
            }
        }
    }

    // Load config (if present) to get whitelist and preferred default
    let config = load_root_cmd_config();
    let whitelist: Vec<&str> = match &config {
        Some(cfg) if !cfg.allowed.is_empty() => cfg.allowed.iter().map(|s| s.as_str()).collect(),
        _ => ALLOWED_ROOT_CMDS.to_vec(),
    };
    let preferred = config.as_ref().map(|c| c.default.as_str()).unwrap_or("");

    // Check ROOT_CMD env var — validate against whitelist.
    // SECURITY: only the first word (command name) is accepted; any extra
    // arguments are stripped to prevent argument injection (e.g.
    // ROOT_CMD="sudo -S /tmp/malicious" would smuggle flags).
    if let Ok(cmd) = std::env::var("ROOT_CMD") {
        let cmd = cmd.trim().to_string();
        if !cmd.is_empty() {
            let first_word = cmd.split_whitespace().next().unwrap_or("");
            if whitelist.contains(&first_word) {
                if cmd.split_whitespace().count() > 1 {
                    eprintln!(
                        "Warning: ROOT_CMD contains extra arguments '{}'. Only '{}' will be used.",
                        cmd, first_word
                    );
                }
                return vec![first_word.to_string()];
            } else {
                eprintln!(
                    "Warning: ROOT_CMD='{}' is not in the allowed whitelist {:?}. Ignoring.",
                    first_word, whitelist
                );
            }
        }
    }

    // Use preferred from config (if set and available in PATH)
    if !preferred.is_empty() {
        if let Ok(_path) = which::which(preferred) {
            return vec![preferred.to_string()];
        }
    }

    // Auto-detect: find first available command from whitelist
    for candidate in &whitelist {
        if let Ok(_path) = which::which(candidate) {
            return vec![candidate.to_string()];
        }
    }

    // Fallback to sudo (legacy behavior)
    vec!["sudo".to_string()]
}

fn get_cleanup_path() -> &'static Mutex<Option<PathBuf>> {
    CLEANUP_PATH.get_or_init(|| Mutex::new(None))
}

fn get_cleanup_mapper() -> &'static Mutex<Option<String>> {
    CLEANUP_MAPPER.get_or_init(|| Mutex::new(None))
}

fn register_cleanup_mapper(name: String) {
    if let Ok(mut guard) = get_cleanup_mapper().lock() {
        *guard = Some(name);
    }
}

fn clear_cleanup_mapper() {
    if let Ok(mut guard) = get_cleanup_mapper().lock() {
        *guard = None;
    }
}

fn register_cleanup_path(path: PathBuf) {
    if let Ok(mut guard) = get_cleanup_path().lock() {
        *guard = Some(path);
    }
}

fn clear_cleanup_path() {
    if let Ok(mut guard) = get_cleanup_path().lock() {
        *guard = None;
    }
}

fn cleanup_on_interrupt() {
    // 1. Close mapper if exists (must happen BEFORE file removal)
    if let Ok(guard) = get_cleanup_mapper().lock() {
        if let Some(mapper) = guard.as_ref() {
            eprintln!("\nInterrupted! Closing LUKS mapper: {}", mapper);
            
            // Critical: Kill child processes (mksquashfs) which might be holding the device open
            let my_pid = process::id();
            let _ = process::Command::new("pkill")
                .arg("-P")
                .arg(my_pid.to_string())
                .status();

            // Give a moment for the kernel/device release
            std::thread::sleep(Duration::from_millis(200));

            let root_cmds = get_effective_root_cmd();
            let mut args = root_cmds.clone();
            args.extend(vec!["cryptsetup".to_string(), "close".to_string(), mapper.clone()]);
            
            // We use standard process::Command here because we are in a signal handler context
            // and don't have access to the executor trait.
            let prog = args.remove(0);
            let _ = process::Command::new(prog).args(args).status();
        }
    }

    // 2. Remove file
    if let Ok(guard) = get_cleanup_path().lock() {
        if let Some(path) = guard.as_ref() {
            if path.exists() {
                eprintln!("Interrupted! Cleaning up file: {:?}", path);
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "squash_manager", 
    about = "Manages SquashFS archives", 
    version
)]
pub struct SquashManagerArgs {
    #[command(subcommand)]
    pub command: Commands,
}

const BANNER: &str = r#"
Copyleft 🄯 2026 :: GPL3
github.com/Antony-hash512/Zero-Kelvin-Stazis
     _            _                              
 ___| |_ __ _ ___(_)___        ___ ___  _ __ ___ 
/ __| __/ _` |_  / / __|_____ / __/ _ \| '__/ _ \
\__ \ || (_| |/ /| \__ \_____| (_| (_) | | |  __/
|___/\__\__,_/___|_|___/      \___\___/|_|  \___|
                                                 
aka
 ____                        _      
/ ___|  __ _ _   _  __ _ ___| |__   
\___ \ / _` | | | |/ _` / __| '_ \  
 ___) | (_| | |_| | (_| \__ \ | | | 
|____/ \__, |\__,_|\__,_|___/_| |_| 
          |_|  Manager              
"#;

impl SquashManagerArgs {
    pub fn build_command() -> clap::Command {
        use clap::CommandFactory;
        let cmd = Self::command();
        cmd.after_help(format!("Detailed Command Information:
{0}
  create <INPUT> [OUTPUT] [OPTIONS]
    Convert a directory or an archive into a SquashFS image.
    Arguments:
      INPUT                 Source directory or archive file.
      OUTPUT                (Optional) Path to the resulting image.
    Options:
      -e, --encrypt         Create an encrypted LUKS container (Requires root/sudo).
      -c, --compression N   Zstd compression level (default: {1}).
      --no-progress         Disable progress bar completely.
      --vanilla-progress    Use native mksquashfs progress (explicit, also default).
      --alfa-progress       Use experimental custom progress bar (broken, for testing).

    Supported Input Formats (repacked on-the-fly via pipe):
      - Directory: Standard behavior
      - Tarball:   .tar (requires 'cat')
      - Combos:    .tar.gz, .tgz (requires 'gzip')
                   .tar.bz2, .tbz2 (requires 'bzip2')
                   .tar.xz, .txz (requires 'xz')
                   .tar.zst, .tzst (requires 'zstd')
                   .tar.zip (requires 'unzip')
                   .tar.7z (requires '7z')
                   .tar.rar (requires 'unrar')
      Note: Archive repacking requires 'tar2sqfs' (from squashfs-tools-ng) installed.

  mount <IMAGE> [MOUNT_POINT]
    Mount a SquashFS image as a directory.
    Arguments:
      IMAGE                 Path to the SquashFS image file.
      MOUNT_POINT           (Optional) Manual mount point.
                            Generated if omitted (prefix_timestamp_random).

  umount <TARGET>
    Unmounts a directory or all instances of an image.
    Arguments:
      TARGET                Mount point directory OR path to the image file.
", BANNER, DEFAULT_ZSTD_COMPRESSION))
    }
}

#[derive(Debug, PartialEq)]
enum CompressionMode {
    None,
    Zstd(u32),
}

impl CompressionMode {
    fn from_level(level: u32) -> Self {
        if level == 0 {
            Self::None
        } else {
            Self::Zstd(level)
        }
    }


    fn apply_to_mksquashfs(&self, args: &mut Vec<String>) {
        match self {
            Self::None => {
                args.push("-no-compression".to_string());
            }
            Self::Zstd(level) => {
                args.push("-comp".to_string());
                args.push("zstd".to_string());
                args.push("-Xcompression-level".to_string());
                args.push(level.to_string());
            }
        }
    }

    fn get_tar2sqfs_compressor_flag(&self) -> Result<String, ZksError> {
        match self {
            Self::None => Err(ZksError::CompressionError("Archive repacking does not support uncompressed mode (tar2sqfs limitation)".to_string())),
            Self::Zstd(_) => Ok("-c zstd".to_string()),
        }
    }
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Create a new SquashFS archive from a directory or existing archive
    Create {
        /// Path to the source directory or archive file (tar, zip, etc.)
        #[arg(value_name = "INPUT")]
        input_path: PathBuf,

        /// Path where the resulting SquashFS archive will be saved
        #[arg(value_name = "OUTPUT")]
        output_path: Option<PathBuf>,

        /// Encrypt the archive using LUKS (Requires root/sudo)
        #[arg(short, long)]
        encrypt: bool,

        /// Zstd compression level
        #[arg(short, long, default_value_t = DEFAULT_ZSTD_COMPRESSION)]
        compression: u32,

        /// Disable progress bar completely
        #[arg(long)]
        no_progress: bool,

        /// Use native mksquashfs progress output (explicit, also the default)
        #[arg(long)]
        vanilla_progress: bool,

        /// Use experimental custom progress bar (broken, for testing only)
        #[arg(long, hide = true)]
        alfa_progress: bool,

        /// Overwrite files inside existing archive (Applies to both Plain and LUKS)
        #[arg(long)]
        overwrite_files: bool,

        /// Replace ENTIRE content of LUKS container (Requires LUKS output)
        #[arg(long)]
        overwrite_luks_content: bool,
    },
    /// Mount a SquashFS archive to a directory (using squashfuse)
    Mount {
        /// Path to the SquashFS image file
        #[arg(value_name = "IMAGE")]
        image: PathBuf,
        /// Optional: Manual mount point. If omitted, a directory is created in the current working directory.
        #[arg(value_name = "MOUNT_POINT")]
        mount_point: Option<PathBuf>,
    },
    /// Unmount a previously mounted SquashFS image (using fusermount -u)
    Umount {
        /// Target mount point directory OR path to the source image file
        #[arg(value_name = "TARGET")]
        mount_point: PathBuf,
    },
}

/// Helper to ensure LUKS resources are cleaned up on failure (RAII)
struct LuksTransaction<'a, E: CommandExecutor + ?Sized> {
    executor: &'a E,
    mapper_name: Option<String>,
    output_path: &'a PathBuf,
    success: bool,
}

impl<'a, E: CommandExecutor + ?Sized> LuksTransaction<'a, E> {
    fn new(executor: &'a E, output_path: &'a PathBuf) -> Self {
        // Register for cleanup on Ctrl+C (Global handler)
        register_cleanup_path(output_path.clone());
        Self {
            executor,
            mapper_name: None,
            output_path,
            success: false,
        }
    }

    fn set_mapper(&mut self, name: String) {
        // Register for cleanup on interrupt
        register_cleanup_mapper(name.clone());
        self.mapper_name = Some(name);
    }

    fn set_success(&mut self) {
        self.success = true;
    }
}

impl<'a, E: CommandExecutor + ?Sized> Drop for LuksTransaction<'a, E> {
    fn drop(&mut self) {
        // Clear global cleanup registration
        clear_cleanup_path();
        clear_cleanup_mapper();

        if let Some(mapper) = &self.mapper_name {
            // debug logging
            if std::env::var("RUST_LOG").is_ok() {
                eprintln!("\nDEBUG: LuksTransaction drop. Closing mapper: {}", mapper);
            }

            // Sync and wait for udev to prevent "device busy" from udisks/scanners
            let _ = self.executor.run("sync", &[]);
            let _ = self.executor.run("udevadm", &["settle"]);

            // Always try to close mapper, even on success.
            // Retry loop to handle race conditions where device might still be busy (e.g. mksquashfs just exited)
            let root_cmds = get_effective_root_cmd();
            
            for i in 0..10 {
                let mut close_args = root_cmds.clone();
                close_args.extend(vec!["cryptsetup".to_string(), "close".to_string(), mapper.clone()]);
                let prog = close_args.remove(0);
                let refs: Vec<&str> = close_args.iter().map(|s| s.as_str()).collect();

                let res = self.executor.run(&prog, &refs);
                match res {
                    Ok(output) => {
                         if output.status.success() {
                             if std::env::var("RUST_LOG").is_ok() {
                                 eprintln!("DEBUG: Mapper closed successfully on attempt {}", i+1);
                             }
                             break;
                         } else {
                             if std::env::var("RUST_LOG").is_ok() {
                                 let stderr = String::from_utf8_lossy(&output.stderr);
                                 eprintln!("DEBUG: Attempt {} failed. Status: {}. Stderr: {}", i+1, output.status, stderr);
                             } else if i == 9 {
                                 let stderr = String::from_utf8_lossy(&output.stderr);
                                 eprintln!("\nWarning: Failed to close LUKS mapper '{}': {}", mapper, stderr);
                             }
                         }
                    },
                    Err(e) => {
                        if std::env::var("RUST_LOG").is_ok() {
                            eprintln!("DEBUG: Execution error on attempt {}: {}", i+1, e);
                        }
                    }
                }
                
                // Exponential backoff-ish (up to 500ms)
                std::thread::sleep(Duration::from_millis(std::cmp::min(100 * (i + 1) as u64, 500)));
            }
        }
        
        if !self.success {
             // Remove the file if we failed
             if self.output_path.exists() {
                 let _ = fs::remove_file(self.output_path);
             }
        }
    }
}

/// Helper to ensure output files are cleaned up on failure or interruption (RAII)
/// Used for plain (non-LUKS) archive creation
struct CreateTransaction {
    output_path: PathBuf,
    success: bool,
}

impl CreateTransaction {
    fn new(output_path: PathBuf) -> Self {
        // Register for cleanup on Ctrl+C
        register_cleanup_path(output_path.clone());
        Self {
            output_path,
            success: false,
        }
    }

    fn set_success(&mut self) {
        self.success = true;
    }
}

impl Drop for CreateTransaction {
    fn drop(&mut self) {
        // Clear the global cleanup path first
        clear_cleanup_path();
        
        if !self.success {
            // Remove the incomplete file if we failed
            if self.output_path.exists() {
                eprintln!("\nCleaning up incomplete file: {:?}", self.output_path);
                let _ = fs::remove_file(&self.output_path);
            }
        }
    }
}

fn get_fs_overhead_percentage(path: &PathBuf, executor: &impl CommandExecutor) -> u32 {
    // stat -f -c %T <path>
    // Output:
    // ext2/ext3
    // or
    // tmpfs
    
    // We need to handle the case where path doesn't exist yet (use parent)
    let check_path = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(&PathBuf::from(".")).to_path_buf()
    };
    
    // Run stat -f -c %T
    if let Ok(output) = executor.run("stat", &["-f", "-c", "%T", check_path.to_str().unwrap_or(".")]) {
        if output.status.success() {
             let out_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
             match out_str.as_str() {
                 "ext2/ext3" | "ext4" | "btrfs" | "xfs" | "zfs" | "tmpfs" | "overlay" => return 50,
                 _ => return 10,
             }
        }
    }
    
        // Default fallback
    10
}


fn main() {
    if let Err(e) = run_app() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_app() -> Result<(), ZksError> {
    if std::env::var("RUST_LOG").is_err() {
        // Safe way to set default log level if not present
    }
    env_logger::init();

    // Set up Ctrl+C handler for cleanup
    ctrlc::set_handler(|| {
        cleanup_on_interrupt();
        process::exit(130); // 128 + SIGINT(2) = standard exit code for Ctrl+C
    }).expect("Error setting Ctrl+C handler");

    let args_raw: Vec<String> = std::env::args().collect();

    // 1. No args -> Help + Exit 0
    if args_raw.len() <= 1 {
         SquashManagerArgs::build_command().print_help()?;
         println!();
         return Ok(());
    }

    // Use try_parse_from to catch --help and handle it with build_command if necessary
    // Actually, clap's FromArgMatches trait allows us to map matches back to the struct.
    let matches = match SquashManagerArgs::build_command().try_get_matches() {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind;
            match e.kind() {
                // 2. Invalid subcommand -> Full Help + Exit 2
                ErrorKind::InvalidSubcommand | ErrorKind::UnknownArgument => {
                    if args_raw.len() >= 2 && !args_raw[1].starts_with('-') {
                        eprintln!("Error: {}\n", e);
                        SquashManagerArgs::build_command().print_help()?;
                        println!();
                        std::process::exit(2);
                    }
                }
                // 3. Command specific errors -> Subcommand Help
                ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand | ErrorKind::TooFewValues | ErrorKind::ValueValidation => {
                    if args_raw.len() >= 2 {
                        let sub = &args_raw[1];
                        let mut cmd = SquashManagerArgs::build_command();
                        if let Some(sub_cmd) = cmd.find_subcommand_mut(sub) {
                             eprintln!("Error: {}\n", e);
                             sub_cmd.print_help()?;
                             println!();
                             std::process::exit(e.exit_code());
                        }
                    }
                }
                _ => {}
            }
            e.exit();
        }
    };
    
    use clap::FromArgMatches;
    let args = SquashManagerArgs::from_arg_matches(&matches)
        .map_err(|e| {
            e.exit();
        })
        .unwrap();

    let executor = RealSystem;

    run(args, &executor)
}

/// Helper to determine if we need sudo/doas


/// Check if the image file is a LUKS container
/// Note: cryptsetup isLuks only reads the file header and doesn't require root privileges
fn is_luks_image(image_path: &PathBuf, executor: &impl CommandExecutor) -> bool {
    let img_str = match image_path.to_str() {
        Some(s) => s,
        None => return false,
    };
    
    // Run cryptsetup isLuks directly (no sudo needed - just reads file header)
    if let Ok(output) = executor.run("cryptsetup", &["isLuks", img_str]) {
        output.status.success()
    } else {
        false
    }
}


/// Generate mapper name from image basename (sanitized).
/// Checks /dev/mapper for collisions and appends a numeric suffix if needed.
fn generate_mapper_name(image_path: &PathBuf) -> String {
    let basename = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Sanitize: replace dots with underscores, keep alphanumeric and underscore
    let sanitized: String = basename
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();

    let base = format!("sq_{}", sanitized);

    // Check for collision: if /dev/mapper/<base> already exists, append suffix
    if !PathBuf::from(format!("/dev/mapper/{}", base)).exists() {
        return base;
    }

    for i in 2..=99 {
        let candidate = format!("{}_{}", base, i);
        if !PathBuf::from(format!("/dev/mapper/{}", candidate)).exists() {
            return candidate;
        }
    }

    // Fallback: use timestamp + random to guarantee uniqueness
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let rnd: u32 = rand::rng().random_range(1000..9999);
    format!("{}_{}_{}", base, ts, rnd)
}


/// Main logic entry point with dependency injection
pub fn run(args: SquashManagerArgs, executor: &impl CommandExecutor) -> Result<(), ZksError> {
    match args.command {
        Commands::Create {
            input_path,
            output_path,
            encrypt,
            compression,
            no_progress,
            vanilla_progress,
            alfa_progress,
            overwrite_files,
            overwrite_luks_content,
        } => {
            // Check Privilege for LUKS
            if encrypt {
                #[cfg(not(test))]
                {
                    let euid = unsafe { libc::geteuid() };
                    if euid != 0 {
                        return Err(ZksError::OperationFailed("LUKS creation requires root privileges: must be run as root".to_string()));
                    }
                }
            }

            // Define compression strategy
            let comp_mode = CompressionMode::from_level(compression);

            // 0. Handle Output Path (Auto-generation if directory or omitted)
            // Logic:
            // If output_path is None -> Error (or current dir? Spec says "Output path required" in table, but zks passes it)
            // Wait, old table for SM said "Output path required" for empty input.
            // But new requirement: "squash_manager-rs create <src> <existing_dir>" -> Auto-gen filename
            // So we need to handle output_path.
            
            let final_output = match &output_path {
                Some(p) => {
                    if p.is_dir() {
                        // Auto-generate filename inside this directory
                        // Format: prefix_unixtime_random.extension
                        // prefix = input dir name
                        let prefix = input_path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("archive");
                        
                        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                        let rnd: u32 = rand::rng().random_range(100000..999999);
                        
                        let ext = if encrypt { "sqfs_luks.img" } else { "sqfs" };
                        let filename = format!("{}_{}_{}.{}", prefix, timestamp, rnd, ext);
                        
                        let final_path = p.join(filename);
                        println!("Auto-generated output filename: {}", final_path.display());
                        final_path
                    } else {
                        // It's a file path (existing or not)
                        p.to_path_buf()
                    }
                },
                None => return Err(ZksError::MissingTarget("Output path required".to_string())),
            };
            
            // 0.1 Check for Existing Output
            if final_output.exists() {
                let is_luks = is_luks_image(&final_output, executor);
                // Check valid SquashFS signature (magic number)
                let is_sqfs = if let Ok(output) = executor.run("file", &[final_output.to_str().ok_or(ZksError::InvalidPath(final_output.clone()))?]) {
                     String::from_utf8_lossy(&output.stdout).contains("Squashfs")
                } else { false };

                if !overwrite_files && !overwrite_luks_content {
                     return Err(ZksError::OperationFailed("Output file exists.\nUse --overwrite-files to update content (append).\nUse --overwrite-luks-content to replace LUKS container payload.".to_string()));
                }
                
                if overwrite_files {
                    // Supported for Plain SQFS and LUKS
                    if !is_luks && !is_sqfs {
                         return Err(ZksError::OperationFailed("Target exists but is not a valid SquashFS or LUKS container. Cannot update.".to_string()));
                    }
                    // Logic continues below...
                    // For LUKS: we open it and then append.
                    // For Plain: we just run mksquashfs (default is append).
                }
                
                if overwrite_luks_content {
                    if !is_luks {
                         return Err(ZksError::LuksError("--overwrite-luks-content requires a valid LUKS container target.".to_string()));
                    }
                    // Logic continues below...
                    // We open LUKS, then run mksquashfs with -noappend.
                }
            }

            if encrypt {
                // ENCRYPTED FLOW
                // ...
                // CRITICAL CHANGE: Disable archive support for LUKS due to persistent I/O errors
                if !input_path.is_dir() {
                    return Err(ZksError::OperationFailed("Encrypted mode (-e) currently supports only DIRECTORIES.\nPlease extract the archive first and point to the directory.".to_string()));
                }

                // Determine raw size (now strictly for directories)
                // du -sb
                let raw_size_bytes = if let Ok(output) = executor.run("du", &["-sb", input_path.to_str().ok_or(ZksError::InvalidPath(input_path.clone()))?]) {
                    if output.status.success() {
                        let out_str = String::from_utf8_lossy(&output.stdout);
                        out_str.split_whitespace().next().unwrap_or("0").parse::<u64>().unwrap_or(0)
                    } else {
                        0
                    }
                } else { 0 };

                if raw_size_bytes == 0 {
                    return Err(ZksError::OperationFailed("Could not determine input directory size or empty input".to_string()));
                }

                let output_buf = &final_output; // Use resolved path
                
                // If appending/replacing, we don't recreate the container file
                // UNLESS --overwrite-luks-content? No, that replaces CONTENT, not container.
                // Actually, if we want to replace *container*, user should delete it.
                // The flag is --overwrite-luks-content (payload).
                
                // If file exists, skip creation/formatting
                if !final_output.exists() { 
                    // ... Normal creation logic ...
                    
                    // Overhead calc
                    let fs_overhead = get_fs_overhead_percentage(output_buf, executor);
                    let overhead_percent = fs_overhead;
                    
                    let overhead_bytes = (raw_size_bytes as f64 * (overhead_percent as f64 / 100.0)) as u64;
                    let luks_header_bytes = LUKS_HEADER_SIZE;
                    let safety_buffer = LUKS_SAFETY_BUFFER;
                    
                    let unaligned_size = raw_size_bytes + overhead_bytes + luks_header_bytes + safety_buffer;
                    
                    // Align to 1MB (1024*1024) to ensure loop device creates exactly this size
                    // (avoiding partial sectors which could be dropped by kernel)
                    let align_size = 1024 * 1024;
                    let container_size = (unaligned_size + align_size - 1) / align_size * align_size;

                    if std::env::var("RUST_LOG").is_ok() {
                        eprintln!("DEBUG: Encrypting directory. Input: {} bytes. Overhead: {}%. Allocating: {} bytes.", 
                            raw_size_bytes, overhead_percent, container_size);
                    }

                    // 1. Create container file with actual allocated space
                    // Using fallocate instead of sparse file (set_len) because:
                    // - Loop devices may fail to write to unallocated sparse regions
                    // - Some filesystems don't support sparse writes through loop
                    // fallocate -l <size> <file>
                    let size_str = container_size.to_string();
                    let output_str_create = output_buf.to_str().ok_or(ZksError::InvalidPath(output_buf.clone()))?;
                    
                    
                    // Fallback to dd if fallocate failed (non-success status) OR if we fell through above
                    // Re-check fallocate success? 
                    // Refactoring for clarity:
                    
                    let mut created = false;
                    let mut fallocate_stderr = String::new();
                    
                    let fallocate_res = executor.run("fallocate", &["-l", &size_str, output_str_create]);
                    
                    if let Ok(out) = fallocate_res {
                        if out.status.success() {
                            created = true;
                        } else {
                            fallocate_stderr = String::from_utf8_lossy(&out.stderr).to_string();
                            if std::env::var("RUST_LOG").is_ok() {
                                eprintln!("DEBUG: fallocate failed, try dd. Stderr: {}", fallocate_stderr);
                            }
                        }
                    } else if let Err(e) = fallocate_res {
                         fallocate_stderr = e.to_string();
                    }
                    
                    if !created {
                        let count = (container_size / (1024 * 1024)) + 1;
                        let dd_output = executor.run("dd", &[
                            "if=/dev/zero",
                            &format!("of={}", output_str_create),
                            "bs=1M",
                            &format!("count={}", count),
                            "status=none"
                        ])?;
                        
                        if !dd_output.status.success() {
                            let dd_err = String::from_utf8_lossy(&dd_output.stderr);
                            return Err(ZksError::OperationFailed(format!("Failed to create container file. fallocate error: '{}'. dd error: '{}'", fallocate_stderr.trim(), dd_err.trim())));
                        }
                    }
                
                } // End if !exists

                // Start Transaction for cleanup
                let mut transaction = LuksTransaction::new(executor, output_buf);

                let output_str = output_buf.to_str().ok_or(ZksError::InvalidPath(output_buf.clone()))?;
                
                // 2. Format LUKS (Only if new)
                let root_cmd = get_effective_root_cmd();

                if !final_output.exists() || (!overwrite_files && !overwrite_luks_content) {
                    // Original Creation Logic
                    println!("Initializing LUKS container...");
                    // Construct command: [sudo] cryptsetup luksFormat -q output
                    let mut luks_args = root_cmd.clone();
                    luks_args.extend(vec!["cryptsetup".to_string(), "luksFormat".to_string(), "-q".to_string(), output_str.to_string()]);
                    
                    let prog = luks_args.remove(0);
                    let args_refs: Vec<&str> = luks_args.iter().map(|s| s.as_str()).collect();

                    let status = executor.run_interactive(&prog, &args_refs)
                        .map_err(|e| ZksError::IoError(e))?;

                    if !status.success() {
                        return Err(ZksError::LuksError("luksFormat failed".to_string()));
                    }
                } else {
                     println!("Opening existing LUKS container for update...");
                }

                // 3. Open
                let mapper_name = generate_mapper_name(&output_buf);
                
                println!("Opening LUKS container...");
                let mut open_args = root_cmd.clone();
                open_args.extend(vec!["cryptsetup".to_string(), "open".to_string(), output_str.to_string(), mapper_name.clone()]);
                
                let prog_open = open_args.remove(0);
                let args_open_refs: Vec<&str> = open_args.iter().map(|s| s.as_str()).collect();

                let status_open = executor.run_interactive(&prog_open, &args_open_refs)
                    .map_err(|e| ZksError::IoError(e))?;
                
                if !status_open.success() {
                    return Err(ZksError::LuksError("cryptsetup open failed".to_string()));
                }
                
                transaction.set_mapper(mapper_name.clone());
                let mapper_path = format!("/dev/mapper/{}", mapper_name);

                // 4. Pack Data
                // Execute mksquashfs to mapper_path
                let pack_result = {
                    let mut cmd_args = vec![
                         input_path.to_str().ok_or(ZksError::InvalidPath(input_path.clone()))?.to_string(),
                         mapper_path.clone(),
                         "-no-recovery".to_string(),
                    ];
                    
                    // Logic for -noappend usage in LUKS:
                    // - Brand new file: Use -noappend (standard)
                    // - overwrite-luks-content: Use -noappend (overwrite internal FS)
                    // - overwrite-files: Do NOT use -noappend (append mode)
                    
                    let is_new_file = !final_output.exists() || (!overwrite_files && !overwrite_luks_content);
                    // Actually, if we just created it (is_new_file logic above in block 1), it is new.
                    // If we opened existing, we only append if overwrite_files.
                    
                    if is_new_file || overwrite_luks_content {
                         cmd_args.push("-noappend".to_string());
                    }
                    // Else if overwrite_files, we omit -noappend to allow appending
                    if no_progress { cmd_args.push("-no-progress".to_string()); }
                    let level_str = compression.to_string();
                    
                    // Helper to adapt Vec<String> to Vec<&str> API of CompressionMode
                    // We need to temporarily hold the strings?
                    // compression mode just pushes &str literals usually.
                    // But wait, `apply_to_mksquashfs` implementation in `lib.rs` takes `&mut Vec<&str>`.
                    // We have `Vec<String>`.
                    // We should change `apply_to_mksquashfs` to simple push logic OR handle manual push here.
                    // Or... convert our Vec<String> to Vec<&str> first? No, we can't push to Vec<&str> if backing string is new.
                    // Simpler: Apply args manually or refactor `apply_to_mksquashfs` is generic?
                    // "comp_mode" implementation is simple.
                    // Let's just manually apply logic here since `apply_to...` is restrictive for String owner.
                    match comp_mode {
                         CompressionMode::None => cmd_args.push("-no-compression".to_string()),
                         CompressionMode::Zstd(_) => {
                              cmd_args.push("-comp".to_string());
                              cmd_args.push("zstd".to_string());
                              cmd_args.push("-Xcompression-level".to_string());
                              cmd_args.push(level_str.to_string());
                         },
                         // other modes...
                    }
                    
                    // Construct: [sudo] mksquashfs ...
                    let mut mk_args = root_cmd.clone();
                    mk_args.extend(vec!["mksquashfs".to_string()]);
                    mk_args.extend(cmd_args);
                    
                    let mk_prog = mk_args.remove(0);
                    let mk_refs: Vec<&str> = mk_args.iter().map(|s| s.as_str()).collect();

                    // Progress bar logic based on flags
                    let output = if no_progress {
                        // No progress at all - just run silently
                        executor.run(&mk_prog, &mk_refs)?
                    } else if alfa_progress {
                        // EXPERIMENTAL: Custom progress bar - parse stdout for percentages (currently broken)
                        // Get directory size for display
                        let dir_size = if let Ok(du_output) = executor.run("du", &["-sb", input_path.to_str().ok_or(ZksError::InvalidPath(input_path.clone()))?]) {
                            if du_output.status.success() {
                                let out_str = String::from_utf8_lossy(&du_output.stdout);
                                out_str.split_whitespace().next().unwrap_or("0").parse::<u64>().unwrap_or(0)
                            } else { 0 }
                        } else { 0 };
                        let dir_size_mb = dir_size as f64 / 1024.0 / 1024.0;
                        
                        let pb = ProgressBar::new(100);
                        pb.set_style(
                            ProgressStyle::with_template(
                                "{spinner:.cyan} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}% {msg}"
                            )
                            .unwrap()
                            .progress_chars("█▓▒░  ")
                        );
                        pb.set_message("Encrypting → SquashFS+LUKS");
                        pb.enable_steady_tick(Duration::from_millis(100));
                        
                        let result = executor.run_with_stdout_progress(&mk_prog, &mk_refs, &pb)?;
                        
                        if result.status.success() {
                            pb.finish_with_message(format!("✓ Encrypted {:.1} MB", dir_size_mb));
                        } else {
                            pb.finish_with_message("✗ Failed");
                        }
                        result
                    } else {
                        // DEFAULT: Use mksquashfs native progress (interactive mode)
                        let status = executor.run_interactive(&mk_prog, &mk_refs)?;
                        std::process::Output {
                            status,
                            stdout: vec![],
                            stderr: vec![],
                        }
                    };

                    if !output.status.success() {
                         Err(ZksError::OperationFailed(format!("mksquashfs failed: {}", String::from_utf8_lossy(&output.stderr))))
                    } else { Ok(()) }
                };

                if let Err(e) = pack_result {
                    return Err(e);
                }

                // 5. Trim logic
                // Need unsquashfs (sudo usually not needed for read, but reading from /dev/mapper requires root)
                let mut trim_size: Option<u64> = None;
                
                // Get FS Size - we're already root in LUKS context, run directly
                // unsquashfs -s /dev/mapper/...
                match executor.run("unsquashfs", &["-s", &mapper_path]) {
                    Ok(out) => {
                        let out_str = String::from_utf8_lossy(&out.stdout);
                        
                        // unsquashfs -s output format:
                        // "Filesystem size 248 bytes (0.24 Kbytes / 0.00 Mbytes)"
                        // parts[0]="Filesystem" parts[1]="size" parts[2]="248" parts[3]="bytes"
                        // We need to find line where parts[3] == "bytes" and parts[2] is an integer
                        let mut fs_bytes: Option<u64> = None;
                        for line in out_str.lines() {
                            if line.contains("Filesystem size") && line.contains(" bytes ") {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                // parts[0]="Filesystem" parts[1]="size" parts[2]="248" parts[3]="bytes"
                                if parts.len() >= 4 && parts[3] == "bytes" {
                                    // Only accept if parts[2] is a pure integer (not "0.24")
                                    if let Ok(bytes) = parts[2].parse::<u64>() {
                                        fs_bytes = Some(bytes);
                                        break;
                                    }
                                }
                            }
                        }
                        
                        if let Some(bytes) = fs_bytes {
                            // Get Offset - we're already root
                            match executor.run("cryptsetup", &["luksDump", output_str]) {
                                Ok(dump) => {
                                    let dump_str = String::from_utf8_lossy(&dump.stdout);
                                    let mut offset: u64 = 0;
                                    // LUKS2: "offset: 16777216 [bytes]"
                                    for line in dump_str.lines() {
                                        if line.trim().starts_with("offset:") && line.contains("bytes") {
                                            if let Some(val_str) = line.split_whitespace().nth(1) {
                                                if let Ok(val) = val_str.parse::<u64>() {
                                                    offset = val;
                                                    break;
                                                }
                                            }
                                        }
                                        // LUKS1: "Payload offset: 4096" (sectors)
                                        if line.trim().starts_with("Payload offset:") {
                                            if let Some(val_str) = line.split_whitespace().nth(2) {
                                                if let Ok(sect) = val_str.parse::<u64>() {
                                                    offset = sect * 512;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    
                                    if offset > 0 {
                                        // Calc total
                                        let raw_trim = bytes + offset + 1024*1024; // +1MB safety margin
                                        // Align to 4096
                                        let aligned = ((raw_trim + 4095) / 4096) * 4096;
                                        trim_size = Some(aligned);
                                    }
                                },
                                Err(_) => {}, // luksDump failed, skip trim
                            }
                        }
                    },
                    Err(_) => {}, // unsquashfs failed, skip trim
                }

                // 6. Close and Finish Transaction
                // We set success (preventing file deletion) and drop the transaction to trigger correct mapper closing
                // This uses the robust logic in LuksTransaction::drop (sync, settle, retries, root rights)
                transaction.set_success();
                drop(transaction);
                
                // 7. Truncate (Safe now that mapper is closed)
                if let Some(size) = trim_size {
                    let container_file = fs::File::options().write(true).open(output_buf).map_err(|e| ZksError::IoError(e))?;
                    let current_len = container_file.metadata()?.len();
                    if size < current_len {
                        println!(" Optimizing container size: {:.1}MB -> {:.1}MB", 
                            current_len as f64 / 1024.0 / 1024.0, size as f64 / 1024.0 / 1024.0);
                        container_file.set_len(size)?;
                    }
                }


                return Ok(());
            }


            // 1. Check if input exists
            if !input_path.exists() {
                return Err(ZksError::InvalidPath(input_path.clone()));
            }

            // 2. Archive Repacking (File -> SquashFS)
            if input_path.is_file() {
                let input_str = input_path.to_str().ok_or(ZksError::InvalidPath(input_path.clone()))?;
                // Use final_output resolved earlier
                let output_buf = &final_output;
                let output_str = output_buf.to_str().ok_or(ZksError::InvalidPath(final_output.clone()))?;

                // Determine decompressor
                let file_name = input_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let decompressor = if file_name.ends_with(".tar") {
                    "cat"
                } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
                    "gzip -dc"
                } else if file_name.ends_with(".tar.bz2") || file_name.ends_with(".tbz2") {
                     "bzip2 -dc"
                } else if file_name.ends_with(".tar.xz") || file_name.ends_with(".txz") {
                    "xz -dc"
                } else if file_name.ends_with(".tar.zst") || file_name.ends_with(".tzst") {
                    "zstd -dc"
                } else if file_name.ends_with(".tar.zip") {
                    "unzip -p"
                } else if file_name.ends_with(".tar.7z") {
                    "7z x -so"
                } else if file_name.ends_with(".tar.rar") {
                    "unrar p -inul"
                } else {
                    return Err(ZksError::CompressionError(format!("Unsupported archive format: {}", file_name)));
                };

                // Determine compressor flag for tar2sqfs
                let compressor_flag = comp_mode.get_tar2sqfs_compressor_flag()?;

                // Construct pipeline: decompressor input | tar2sqfs options output
                // Using explicit quoting for paths to handle spaces safely in sh -c
                // Fixed: Do not pass compression level to -j (threads), use -c <compressor>
                // SECURITY: all interpolated values are shell-quoted.
                // compressor_flag is currently hardcoded but quoted defensively
                // to prevent injection if it ever becomes configurable.
                let cmd = format!(
                    "{decompressor} '{input}' | tar2sqfs --quiet --no-skip --force {flag} '{output}'",
                    decompressor = decompressor,
                    input = input_str.replace("'", "'\\''"),
                    flag = compressor_flag.replace("'", "'\\''"),
                    output = output_str.replace("'", "'\\''")
                );

                if std::env::var("RUST_LOG").is_ok() {
                    eprintln!("DEBUG: Executing pipeline: {}", cmd);
                }

                // Get input file size for display
                let input_size = fs::metadata(&input_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                let input_size_mb = input_size as f64 / 1024.0 / 1024.0;

                // Create transaction for cleanup on failure
                let output_buf = &final_output;
                let mut transaction = CreateTransaction::new(output_buf.clone());

                // Use 'set -o pipefail' so that if decompressor fails, the whole pipeline fails
                let full_cmd = format!("set -o pipefail; {}", cmd);
                
                if no_progress {
                    // Silent mode
                    let output = executor.run("sh", &["-c", &full_cmd])?;
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(ZksError::OperationFailed(format!("Archive repack failed: {}", stderr)));
                    }
                } else {
                    // Progress mode: show filling progress bar
                    let pb = ProgressBar::new(input_size);
                    pb.set_style(
                        ProgressStyle::with_template(
                            "{spinner:.cyan} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}) {msg}"
                        )
                        .unwrap()
                        .progress_chars("█▓▒░  ")
                    );
                    pb.set_message("Repacking archive → SquashFS");
                    pb.enable_steady_tick(Duration::from_millis(100));

                    let output = executor.run_with_file_progress(
                        "sh",
                        &["-c", &full_cmd],
                        output_buf,
                        &pb,
                        Duration::from_millis(100),
                    )?;
                    
                    if output.status.success() {
                        pb.finish_with_message(format!(
                            "✓ Repacked {:.1} MB successfully",
                            input_size_mb
                        ));
                    } else {
                        pb.finish_with_message("✗ Failed");
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(ZksError::OperationFailed(format!("Archive repack failed: {}", stderr)));
                    }
                }
                
                transaction.set_success();
                return Ok(());
            }

            // 3. Standard Directory Packing (Directory -> SquashFS)
            {
                let output_buf = &final_output; // Use resolved path
                let output_str = output_buf.to_str().ok_or(ZksError::InvalidPath(output_buf.clone()))?;
                let input_str = input_path.to_str().ok_or(ZksError::InvalidPath(input_path.clone()))?;
                
                // Transaction for cleanup
                let mut transaction = CreateTransaction::new(output_buf.clone());

                // 1. Pack Directory
                let mk_result = {
                    let mut cmd_args = vec![input_str, output_str];
                    
                    if no_progress {
                        cmd_args.push("-no-progress");
                    }
                    
                    
                    let mut mksquashfs_args: Vec<String> = cmd_args.iter().map(|s: &&str| s.to_string()).collect();
                    
                    
                    if !final_output.exists() {
                         mksquashfs_args.push("-noappend".to_string());
                    }
                    // Else if existing (and we are here, meaning overwrite_files is true), we omit -noappend (default is append).

                    
                    // Compression
                    comp_mode.apply_to_mksquashfs(&mut mksquashfs_args);
                    
                    // Convert back to Vec<&str> for execution args
                    // This is a bit clumsy but safer given we modified Vec<String>
                    // We need to pass &str to executor
                    
                    let mk_prog = "mksquashfs";
                    
                    // Helper to run with progress
                    let run_with_progress = |args: &[String]| -> Result<(), ZksError> {
                         let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                         
                        if no_progress {
                             let output = executor.run(mk_prog, &refs)?;
                             if !output.status.success() { 
                                 let stderr = String::from_utf8_lossy(&output.stderr);
                                 return Err(ZksError::OperationFailed(format!("mksquashfs failed: {}", stderr))); 
                             }
                        } else if vanilla_progress {
                             let status = executor.run_interactive(mk_prog, &refs)?;
                             if !status.success() { return Err(ZksError::OperationFailed("mksquashfs failed".to_string())); }
                        } else if alfa_progress {
                             // Fallback
                             let output = executor.run_interactive(mk_prog, &refs)?;
                             if !output.success() { return Err(ZksError::OperationFailed("mksquashfs failed".to_string())); }
                        } else {
                             // Default Custom Progress
                             // Get directory size
                             let dir_size = if let Ok(output) = executor.run("du", &["-sb", input_str]) {
                                if output.status.success() {
                                    let out_str = String::from_utf8_lossy(&output.stdout);
                                    out_str.split_whitespace().next().unwrap_or("0").parse::<u64>().unwrap_or(0)
                                } else { 0 }
                            } else { 0 };
                            let dir_size_mb = dir_size as f64 / 1024.0 / 1024.0;
                            
                            let pb = ProgressBar::new(dir_size);
                            pb.set_style(
                                ProgressStyle::with_template(
                                    "{spinner:.cyan} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}) {msg}"
                                )
                                .unwrap()
                                .progress_chars("█▓▒░  ")
                            );
                            pb.set_message("Packing directory → SquashFS");
                            pb.enable_steady_tick(Duration::from_millis(100));
                            
                            let output = executor.run_with_file_progress(
                                mk_prog,
                                &refs,
                                output_buf,
                                &pb,
                                Duration::from_millis(100),
                            )?;
                            
                            if output.status.success() {
                                pb.finish_with_message(format!(
                                    "✓ Packed {:.1} MB successfully",
                                    dir_size_mb
                                ));
                            } else {
                                pb.finish_with_message("✗ Failed");
                                let stderr = String::from_utf8_lossy(&output.stderr);
                                return Err(ZksError::OperationFailed(format!("mksquashfs failed: {}", stderr)));
                            }
                        }
                        Ok(())
                    };
                    
                    run_with_progress(&mksquashfs_args)
                }; // block result
                
                if let Err(e) = mk_result {
                    return Err(e);
                }

                transaction.set_success();
                Ok(())
            }
        } // End Create
        Commands::Mount { image, mount_point } => {
            if !image.exists() {
                return Err(ZksError::InvalidPath(image));
            }
            // Always use absolute path to ensure losetup/detection works reliably
            let image = fs::canonicalize(image).map_err(|e| ZksError::IoError(e))?;

            let target_mount_point = match mount_point {
                Some(path) => path,
                None => {
                    // Auto-generate mount point
                    let prefix = image.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("sqfs_image");
                    
                    let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    
                    let random_suffix: u32 = rand::rng().random_range(100000..999999);
                    let dir_name = format!("mount_{}_{}_{}", prefix, timestamp, random_suffix);
                    
                    // Use /tmp/stazis-<uid> for reliability (avoids FUSE-on-FUSE/Network issues)
                    let stazis_tmp = zero_kelvin_stazis::utils::get_stazis_temp_dir()
                        .unwrap_or_else(|_| env::temp_dir()); 
                    
                    let path = stazis_tmp.join(dir_name);
                    
                    println!("No mount point specified. Using secure local path for stability: {}", path.display());
                    path
                }
            };
            
            fs::create_dir_all(&target_mount_point).map_err(|e| ZksError::IoError(e))?;
            
            // Check if this is a LUKS container
            if is_luks_image(&image, executor) {
                println!("Detected LUKS container. Opening encrypted image...");
                
                let mapper_name = generate_mapper_name(&image);
                let mapper_path = format!("/dev/mapper/{}", mapper_name);
                let root_cmd = get_effective_root_cmd();
                
                // Check if mapper already exists
                if PathBuf::from(&mapper_path).exists() {
                    println!("Mapper device already exists. Attempting to mount...");
                    
                    // Try mounting existing mapper
                    let mut mount_args = root_cmd.clone();
                    mount_args.extend(vec![
                        "mount".to_string(),
                        "-t".to_string(),
                        "squashfs".to_string(),
                        mapper_path.clone(),
                        target_mount_point.to_str().ok_or(ZksError::InvalidPath(target_mount_point.clone()))?.to_string(),
                    ]);
                    
                    let prog = mount_args.remove(0);
                    let args_refs: Vec<&str> = mount_args.iter().map(|s| s.as_str()).collect();
                    
                    if let Ok(output) = executor.run(&prog, &args_refs) {
                        if output.status.success() {
                            println!("Mounted at {}", target_mount_point.display());
                            return Ok(());
                        }
                    }
                    
                    // Stale mapper - close and retry
                    println!("Mount failed (stale mapper?). Closing and retrying...");
                    let mut close_args = root_cmd.clone();
                    close_args.extend(vec!["cryptsetup".to_string(), "close".to_string(), mapper_name.clone()]);
                    
                    let close_prog = close_args.remove(0);
                    let close_refs: Vec<&str> = close_args.iter().map(|s| s.as_str()).collect();
                    let _ = executor.run(&close_prog, &close_refs);
                }
                
                // Open LUKS container (interactive - will ask for password)
                println!("Opening encrypted container (password required)...");
                let mut open_args = root_cmd.clone();
                open_args.extend(vec![
                    "cryptsetup".to_string(),
                    "open".to_string(),
                    image.to_str().ok_or(ZksError::InvalidPath(image.clone()))?.to_string(),
                    mapper_name.clone(),
                ]);
                
                let open_prog = open_args.remove(0);
                let open_refs: Vec<&str> = open_args.iter().map(|s| s.as_str()).collect();
                
                let status = executor.run_interactive(&open_prog, &open_refs)
                    .map_err(|e| ZksError::IoError(e))?;
                
                if !status.success() {
                    return Err(ZksError::LuksError("Failed to open encrypted container".to_string()));
                }
                
                // Mount the mapper device
                let mut mount_args = root_cmd.clone();
                mount_args.extend(vec![
                    "mount".to_string(),
                    "-t".to_string(),
                    "squashfs".to_string(),
                    mapper_path.clone(),
                    target_mount_point.to_str().ok_or(ZksError::InvalidPath(target_mount_point.clone()))?.to_string(),
                ]);
                
                let mount_prog = mount_args.remove(0);
                let mount_refs: Vec<&str> = mount_args.iter().map(|s| s.as_str()).collect();
                
                let output = executor.run(&mount_prog, &mount_refs)?;
                
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    // Cleanup: close the mapper we just opened
                    let mut close_args = root_cmd.clone();
                    close_args.extend(vec!["cryptsetup".to_string(), "close".to_string(), mapper_name]);
                    let close_prog = close_args.remove(0);
                    let close_refs: Vec<&str> = close_args.iter().map(|s| s.as_str()).collect();
                    let _ = executor.run(&close_prog, &close_refs);
                    
                    return Err(ZksError::OperationFailed(format!("Mount failed: {}", stderr)));
                }
                
                println!("Mounted at {}", target_mount_point.display());
                return Ok(());
            }
            
            // Plain SquashFS - use squashfuse (no root required)
            let mp_str = target_mount_point.to_str().ok_or(ZksError::InvalidPath(target_mount_point.clone()))?;
            let img_str = image.to_str().ok_or(ZksError::InvalidPath(image.clone()))?;
            
            // Added -o nonempty to allow mounting over non-empty directories
            let output = executor.run("squashfuse", &["-o", "nonempty", img_str, mp_str])?;
            
             if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ZksError::OperationFailed(format!("squashfuse failed: {}", stderr)));
            }
            
            Ok(())
        },


        Commands::Umount { mount_point } => {
            let path = &mount_point;
            let root_cmd = get_effective_root_cmd();
            
            if !path.exists() {
                return Err(ZksError::InvalidPath(path.clone()));
            }

            let mut targets = Vec::new();

            if path.is_dir() {
                targets.push(path.clone());
            } else if path.is_file() {
                // It's an image file. Find matching squashfuse processes.
                let abs_path = fs::canonicalize(path)
                    .map_err(|e| ZksError::IoError(e))?;
                let abs_path_str = abs_path.to_str().unwrap_or("");
                
                if std::env::var("RUST_LOG").is_ok() {
                    eprintln!("DEBUG: Scanning processes for image: '{}'", abs_path_str);
                }

                // Iterate over /proc
                let proc_dir = fs::read_dir("/proc").map_err(|e| ZksError::IoError(e))?;
                
                for entry in proc_dir {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        let file_name_str = file_name.to_str().unwrap_or("");
                        
                        // Check if it's a PID (all digits)
                        if file_name_str.chars().all(|c| c.is_ascii_digit()) {
                             let cmdline_path = entry.path().join("cmdline");
                             if let Ok(cmdline) = fs::read_to_string(cmdline_path) {
                                 // cmdline is null-separated
                                 let args: Vec<&str> = cmdline.split('\0').collect();
                                 
                                 if args.is_empty() { continue; }
                                 
                                 // Check if process name contains squashfuse
                                 let prog_name = args[0];
                                 if prog_name.contains("squashfuse") {
                                     // Look for the image path in arguments
                                     // squashfuse [options] IMAGE MOUNTPOINT
                                     
                                     for (i, arg) in args.iter().enumerate() {
                                         // Skip empty args and options
                                         if arg.is_empty() || arg.starts_with('-') {
                                             continue;
                                         }
                                         
                                         // Try to canonicalize the argument to handle:
                                         // 1. Relative paths (./image.sqfs vs /full/path/image.sqfs)
                                         // 2. Symlinks (/home/user vs /home/share/user)
                                         let arg_path = PathBuf::from(arg);
                                         let matches = if let Ok(arg_canonical) = fs::canonicalize(&arg_path) {
                                             arg_canonical == abs_path
                                         } else {
                                             // If canonicalize fails, fall back to string comparison
                                             *arg == abs_path_str
                                         };
                                         
                                         if matches {
                                             if i + 1 < args.len() {
                                                 let potential_mount = args[i+1];
                                                 if !potential_mount.starts_with('-') && !potential_mount.is_empty() {
                                                     if std::env::var("RUST_LOG").is_ok() {
                                                         eprintln!("DEBUG: Found match! pid {} mountpoint '{}'", file_name_str, potential_mount);
                                                     }
                                                     targets.push(PathBuf::from(potential_mount));
                                                 }
                                             }
                                         }
                                     }
                                 }
                             }
                        }
                    }
                }
                
                // If no squashfuse found, check for LUKS mounts
                // LUKS images are mounted via loop device -> cryptsetup -> /dev/mapper/sq_* -> mount
                if targets.is_empty() {
                    if std::env::var("RUST_LOG").is_ok() {
                        eprintln!("DEBUG: No squashfuse found, checking for LUKS mounts...");
                    }
                    
                    // Find loop device(s) associated with this file
                    // losetup -j <file> shows: /dev/loop0: []: (<file>)
                    // We try regular user first, then root if needed
                    let mut losetup_output = executor.run("losetup", &["-j", abs_path_str]);
                    
                    // Fallback to root only if failed (permission denied), not if just empty (no loops found)
                    if let Ok(ref out) = losetup_output {
                        if !out.status.success() {
                            let mut args = root_cmd.clone();
                            args.extend(vec!["losetup".to_string(), "-j".to_string(), abs_path_str.to_string()]);
                            let prog = args.remove(0);
                            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                            losetup_output = executor.run(&prog, &refs);
                        }
                    }

                    if let Ok(output) = losetup_output {
                        if output.status.success() {
                            let out_str = String::from_utf8_lossy(&output.stdout);
                            for line in out_str.lines() {
                                // Parse /dev/loopX from the output
                                if let Some(loop_dev) = line.split(':').next() {
                                    let loop_dev = loop_dev.trim();
                                    if std::env::var("RUST_LOG").is_ok() {
                                        eprintln!("DEBUG: Found loop device: {}", loop_dev);
                                    }
                                    
                                    // Now find mounts from /dev/mapper/sq_* that use this loop device
                                    // Read /proc/mounts to find mount points for sq_* mappers
                                    if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
                                        for mount_line in mounts.lines() {
                                            let parts: Vec<&str> = mount_line.split_whitespace().collect();
                                            if parts.len() >= 2 {
                                                let source = parts[0];
                                                let mount_point = parts[1];
                                                
                                                // Check if it's a sq_* mapper
                                                if source.starts_with("/dev/mapper/sq_") {
                                                    // Verify this mapper uses our loop device
                                                    // dmsetup table sq_* shows the backing device
                                                    let mapper_name = source.trim_start_matches("/dev/mapper/");
                                                    
                                                    // Try dmsetup (user -> root fallback)
                                                    let mut dm_output = executor.run("dmsetup", &["deps", "-o", "devname", mapper_name]);
                                                    
                                                    if let Ok(ref out) = dm_output {
                                                        if !out.status.success() {
                                                             let mut args = root_cmd.clone();
                                                             args.extend(vec!["dmsetup".to_string(), "deps".to_string(), "-o".to_string(), "devname".to_string(), mapper_name.to_string()]);
                                                             let prog = args.remove(0);
                                                             let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                                                             dm_output = executor.run(&prog, &refs);
                                                        }
                                                    }

                                                    if let Ok(dm_output) = dm_output {
                                                        if dm_output.status.success() {
                                                            let dm_str = String::from_utf8_lossy(&dm_output.stdout);
                                                            // Output like: 1 dependencies  : (loop0)
                                                            let loop_name = loop_dev.trim_start_matches("/dev/");
                                                            if dm_str.contains(loop_name) {
                                                                if std::env::var("RUST_LOG").is_ok() {
                                                                    eprintln!("DEBUG: Found LUKS mount: {} at {}", source, mount_point);
                                                                }
                                                                targets.push(PathBuf::from(mount_point));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                
                if targets.is_empty() {
                    return Err(ZksError::OperationFailed(format!("Image is not mounted (no squashfuse or LUKS mount found): {:?}", path)));
                }
            } else {
                 return Err(ZksError::InvalidPath(path.clone()));
            }
            
            for target in targets {
                let target_str = target.to_str().ok_or(ZksError::InvalidPath(target.clone()))?;
                
                // Detect source device using findmnt (doesn't need root - just reads /proc/mounts)
                let mut source_device: Option<String> = None;
                
                if let Ok(output) = executor.run("findmnt", &["-n", "-o", "SOURCE", target_str]) {
                    if output.status.success() {
                        source_device = Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
                    }
                }
                
                // Get root_cmd only if needed (for LUKS unmount operations)
                // root_cmd is now retrieved at function scope
                // let root_cmd = get_effective_root_cmd();

                
                // Determine unmount method based on source device
                let is_luks_mapper = source_device.as_ref()
                    .map(|dev| dev.starts_with("/dev/mapper/sq_"))
                    .unwrap_or(false);
                
                if is_luks_mapper {
                    // LUKS mount - use sudo umount
                    println!("Unmounting LUKS mapper...");
                    let mut umount_args = root_cmd.clone();
                    umount_args.extend(vec!["umount".to_string(), target_str.to_string()]);
                    
                    let prog = umount_args.remove(0);
                    let args_refs: Vec<&str> = umount_args.iter().map(|s| s.as_str()).collect();
                    
                    let output = executor.run(&prog, &args_refs)?;
                    
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(ZksError::OperationFailed(format!("umount failed for {:?}: {}", target, stderr)));
                    }
                    
                    // Close LUKS mapper
                    if let Some(dev) = source_device {
                        let mapper_name = dev.trim_start_matches("/dev/mapper/");
                        println!("Closing LUKS container {}...", mapper_name);
                        
                        let mut close_args = root_cmd.clone();
                        close_args.extend(vec!["cryptsetup".to_string(), "close".to_string(), mapper_name.to_string()]);
                        
                        let close_prog = close_args.remove(0);
                        let close_refs: Vec<&str> = close_args.iter().map(|s| s.as_str()).collect();
                        
                        let output = executor.run(&close_prog, &close_refs)?;
                        
                        if !output.status.success() {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            eprintln!("Warning: Failed to close LUKS mapper: {}", stderr);
                        }
                    }
                } else {
                    // Plain squashfuse mount - use fusermount -u
                    let output = executor.run("fusermount", &["-u", target_str])?;
                                        if !output.status.success() {
                          let stderr = String::from_utf8_lossy(&output.stderr);
                          return Err(ZksError::OperationFailed(format!("fusermount failed for {:?}: {}", target, stderr)));
                     }
                }
                
                // Post-unmount cleanup: remove directory if empty
                let _ = fs::remove_dir(&target);
            }

            Ok(())
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Output;
    // use zero_kelvin_stazis::executor::MockCommandExecutor; // Not visible/available
    use mockall::predicate::*;
    use mockall::mock;

    // Define the mock locally for the binary tests
    mock! {
        pub CommandExecutor {}
        impl CommandExecutor for CommandExecutor {
            fn run<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<Output>;
            fn run_interactive<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<std::process::ExitStatus>;
            fn run_with_file_progress<'a>(
                &self,
                program: &str,
                args: &[&'a str],
                output_file: &std::path::Path,
                progress_bar: &indicatif::ProgressBar,
                poll_interval: std::time::Duration,
            ) -> std::io::Result<Output>;
            fn run_with_stdout_progress<'a>(
                &self,
                program: &str,
                args: &[&'a str],
                progress_bar: &indicatif::ProgressBar,
            ) -> std::io::Result<Output>;
            fn run_and_capture_error<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<(std::process::ExitStatus, String)>;
        }
    }


    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        SquashManagerArgs::command().debug_assert();
    }

    #[test]
    fn test_create_plain_archive() {
        // Create a temp directory so input_path.exists() passes
        let temp_dir = tempfile::tempdir().unwrap();
        let input_path = temp_dir.path().to_path_buf();
        let input_path_str = input_path.to_str().unwrap();
        let input_path_check = input_path_str.to_string();

        let mut mock = MockCommandExecutor::new();
        // Expectation: mksquashfs input_dir output.sqfs -no-progress -comp zstd -Xcompression-level <DEFAULT_ZSTD_COMPRESSION>
        mock.expect_run()
            .withf(move |program, args| {
                 program == "mksquashfs" &&
                 args.len() == 8 &&
                 args[0] == input_path_check &&
                 args[1] == "output.sqfs" &&
                 args[2] == "-no-progress" &&
                 args[3] == "-noappend" &&
                 args[4] == "-comp" &&
                 args[5] == "zstd" &&
                 args[6] == "-Xcompression-level" &&
                 args[7] == DEFAULT_ZSTD_COMPRESSION.to_string()
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path: input_path,
                output_path: Some(PathBuf::from("output.sqfs")),
                encrypt: false,
                compression: DEFAULT_ZSTD_COMPRESSION,
                no_progress: true,
                vanilla_progress: false,
                alfa_progress: false,
                overwrite_files: false,
                overwrite_luks_content: false,
            },
        };

        run(args, &mock).unwrap();
    }

    #[test]
    fn test_create_encrypted_flow() {
        // Setup
        let temp_dir = tempfile::tempdir().unwrap();
        let input_path = temp_dir.path().join("input_dir");
        fs::create_dir(&input_path).unwrap();
        let input_str = input_path.to_str().unwrap().to_string();
        
        // Output path
        let output_path = temp_dir.path().join("encrypted.sqfs");
        let output_str = output_path.to_str().unwrap().to_string();

        let mut mock = MockCommandExecutor::new();
        
        // 1. du -sb (Size calc)
        let input_str_1 = input_str.clone();
        mock.expect_run()
            .withf(move |program, args| {
                program == "du" && args == vec!["-sb", input_str_1.as_str()]
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"1048576\tinput_dir\n".to_vec(),
                stderr: vec![],
            }));

        // 2. stat -f -c %T (Overhead calc)
        let parent = temp_dir.path().to_str().unwrap().to_string();
        mock.expect_run()
            .withf(move |program, args| {
                program == "stat" && args == vec!["-f", "-c", "%T", parent.as_str()]
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"ext2/ext3\n".to_vec(),
                stderr: vec![],
            }));

        // 2.5. fallocate (Container creation)
        // Need to capture output_path to create the file in the returning closure

        mock.expect_run()
            .withf(|program, args| {
                program == "fallocate" && args.len() == 3 && args[0] == "-l"
            })
            .times(1)
            .returning(move |_, args| {
                // Create the file that fallocate would create
                let file_path = args[2];
                let _ = fs::File::create(file_path);
                Ok(Output {
                    status: std::process::ExitStatus::from_raw(0),
                    stdout: vec![],
                    stderr: vec![],
                })
            });

        // 3. luksFormat
        let output_str_3 = output_str.clone();
        mock.expect_run_interactive()
            .withf(move |program, args| {
                 // Check if program is a known runner or direct call
                 let is_runner = ["sudo", "doas", "run0"].contains(&program);
                 let is_direct = program == "cryptsetup";
                 
                 if is_direct {
                     args == vec!["luksFormat", "-q", output_str_3.as_str()]
                 } else if is_runner {
                     args == vec!["cryptsetup", "luksFormat", "-q", output_str_3.as_str()]
                 } else {
                     false
                 }
            })
            .times(1)
            .returning(|_, _| Ok(std::process::ExitStatus::from_raw(0)));

        // 4. open
        let output_str_4 = output_str.clone();
        mock.expect_run_interactive()
            .withf(move |program, args| {
                let is_runner = ["sudo", "doas", "run0"].contains(&program);
                let is_direct = program == "cryptsetup";
                
                let check_args = |a: &&[&str]| a.contains(&"open") && a.contains(&output_str_4.as_str());

                if is_direct {
                    check_args(&args)
                } else if is_runner {
                    // Args should contain cryptsetup, open, path... 
                    // But args to runner are ["cryptsetup", "open", ...]
                    args.contains(&"cryptsetup") && args.contains(&"open") && args.contains(&output_str_4.as_str())
                } else {
                    false
                }
            })
            .times(1)
            .returning(|_, _| Ok(std::process::ExitStatus::from_raw(0)));

        // 5. mksquashfs
        // output to /dev/mapper/...
        mock.expect_run()
            .withf(move |program, args| {
                 let is_runner = ["sudo", "doas", "run0"].contains(&program);
                 let is_direct = program == "mksquashfs";
                 
                 if is_direct {
                     args.iter().any(|s| s.starts_with("/dev/mapper/sq_"))
                 } else if is_runner {
                     args.iter().any(|s| s.starts_with("/dev/mapper/sq_"))
                 } else {
                     false
                 }
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));
            
        // 6. unsquashfs -s (Trim size) - called directly without sudo
        mock.expect_run()
            .withf(|program, args| program == "unsquashfs" && args.contains(&"-s"))
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"Filesystem size 500000 bytes (488.28 Kbytes / 0.48 Mbytes)\n".to_vec(),
                stderr: vec![],
            }));
            
        // 7. luksDump (Offset) - called directly without sudo
        mock.expect_run()
            .withf(|program, args| program == "cryptsetup" && args.contains(&"luksDump"))
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"offset: 16777216 [bytes]\n".to_vec(),
                stderr: vec![],
            }));
            
        // 8. Transaction Drop Sequence
        // 8.1 Sync
        mock.expect_run()
            .withf(|program, _args| program == "sync")
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        // 8.2 udevadm settle
        mock.expect_run()
            .withf(|program, args| program == "udevadm" && args.contains(&"settle"))
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        // 8.3 close (from LuksTransaction drop)
        mock.expect_run()
            .withf(|program, args| {
                let is_runner = ["sudo", "doas", "run0"].contains(&program);
                let is_direct = program == "cryptsetup";
                
                if is_direct {
                    args.contains(&"close")
                } else if is_runner {
                    args.contains(&"cryptsetup") && args.contains(&"close")
                } else {
                    false
                }
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path,
                output_path: Some(output_path),
                encrypt: true,
                compression: DEFAULT_ZSTD_COMPRESSION,
                no_progress: true,
                vanilla_progress: false,
                alfa_progress: false,
                overwrite_files: false,
                overwrite_luks_content: false,
            },
        };

        run(args, &mock).unwrap();
    }
    #[test]
    fn test_mount_auto_gen_path() {
        // We can't easily mock env::current_dir or SystemTime in this simple setup without more refactoring/creates.
        // But we can verify that the logic *would* generate a path if mount_point is None.
        // Actually, we can test `run` with `mount_point: None` and a mock executor.
        
        // Use a real file for image to pass .exists() check
        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("test.sqfs");
        fs::write(&image_path, "dummy data").unwrap();
        let image_path_str = image_path.to_str().unwrap().to_string();

        let mut mock = MockCommandExecutor::new();
        
        // 0. cryptsetup isLuks (LUKS detection) - returns failure (not LUKS)
        mock.expect_run()
            .withf(|program, args| {
                program == "cryptsetup" && args.len() == 2 && args[0] == "isLuks"
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(256), // exit code 1 = not LUKS
                stdout: vec![],
                stderr: vec![],
            }));
        
        // 1. squashfuse (for plain SquashFS)
        mock.expect_run()
            .withf(move |program, args| {
                program == "squashfuse" &&
                args.len() == 4 && // -o nonempty image mountpoint
                args[0] == "-o" &&
                args[1] == "nonempty" &&
                args[2] == image_path_str
                // args[3] is the auto-generated path, hard to match exact string due to randomness/time
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));
            
        let args = SquashManagerArgs {
            command: Commands::Mount {
                image: image_path,
                mount_point: None,
            },
        };
        
        // This will create a directory in CWD. We should clean it up?
        // The integration tests handle this better. 
        // For unit test, we might dirty the CWD if we are not careful.
        // Let's rely on integration tests for the side-effects (dir creation) 
        // OR refactor `run` to take a "PathGenerator" trait? 
        // Overkill for now. 
        
        // Let's skip dirtying CWD in unit test by running it in a temp CWD?
        // Valid strategy: change CWD for the test.
        let orig_cwd = env::current_dir().unwrap();
        let test_cwd = tempfile::tempdir().unwrap();
        env::set_current_dir(&test_cwd).unwrap();
        
        let result = run(args, &mock);
        
        // Restore CWD
        env::set_current_dir(&orig_cwd).unwrap();
        
        assert!(result.is_ok());
    }


    #[test]
    fn test_compression_mode_logic() {
        // Test None
        let mode_none = CompressionMode::from_level(0);
        assert_eq!(mode_none, CompressionMode::None);
        
        let mut args = vec![];
        mode_none.apply_to_mksquashfs(&mut args);
        assert_eq!(args, vec!["-no-compression"]);

        assert!(mode_none.get_tar2sqfs_compressor_flag().is_err());

        // Test Zstd
        let mode_zstd = CompressionMode::from_level(15);
        assert_eq!(mode_zstd, CompressionMode::Zstd(15));
        
        let mut args2 = vec![];
        mode_zstd.apply_to_mksquashfs(&mut args2);
        assert_eq!(args2, vec!["-comp", "zstd", "-Xcompression-level", "15"]);
        assert_eq!(mode_zstd.get_tar2sqfs_compressor_flag().unwrap(), "-c zstd");
    }

    #[test]
    fn test_create_directory_with_no_compression() {
        let temp_dir = tempfile::tempdir().unwrap();
        let input_path = temp_dir.path().to_path_buf();
        let input_path_check = input_path.to_str().unwrap().to_string();

        let mut mock = MockCommandExecutor::new();
        // Expectation: mksquashfs input output -no-progress -no-compression
        mock.expect_run()
            .withf(move |program, args| {
                 program == "mksquashfs" &&
                 args.len() == 5 && // input, output, -no-progress, -noappend, -no-compression
                 args[0] == input_path_check &&
                 args[1] == "output_no_comp.sqfs" &&
                 args[2] == "-no-progress" &&
                 args[3] == "-noappend" &&
                 args[4] == "-no-compression"
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path,
                output_path: Some(PathBuf::from("output_no_comp.sqfs")),
                encrypt: false,
                compression: 0,
                no_progress: true,
                vanilla_progress: false,
                alfa_progress: false,
                overwrite_files: false,
                overwrite_luks_content: false,
            },
        };

        run(args, &mock).unwrap();
    }
}
