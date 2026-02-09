/// Default zstd compression level for SquashFS
pub const DEFAULT_ZSTD_COMPRESSION: u32 = 19;

/// Application name for directory naming (XDG_CACHE_HOME, etc.)
pub const APP_NAME: &str = "0k";
pub const APP_NAME_FOR_CONFIG: &str = "0k";
pub const APP_NAME_FOR_CACHE: &str = "0k-cache";

/// Size of the LUKS2 header in bytes
pub const LUKS_HEADER_SIZE: u64 = 32 * 1024 * 1024; // 32MB for LUKS2 header

/// Safety buffer size in bytes to avoid truncation
pub const LUKS_SAFETY_BUFFER: u64 = 128 * 1024 * 1024; // 128MB safety buffer to avoid truncation

/// Whitelist of allowed privilege escalation commands.
/// Only these binaries are accepted via ROOT_CMD env var or config file.
pub const ALLOWED_ROOT_CMDS: &[&str] = &["sudo", "doas", "sudo-rs", "run0", "pkexec", "please"];

/// Prefix for LUKS mapper device names in /dev/mapper/
pub const LUKS_MAPPER_PREFIX: &str = "sq_";

/// Maximum number of processes to scan in /proc during umount (DoS protection)
pub const PROC_SCAN_LIMIT: usize = 10000;

/// Maximum size of manifest file (list.yaml) in bytes (10MB, YAML-bomb protection)
pub const MANIFEST_MAX_SIZE: u64 = 10 * 1024 * 1024;

/// Directory for application logs under XDG_STATE_HOME
pub const LOG_DIR_NAME: &str = "logs";
