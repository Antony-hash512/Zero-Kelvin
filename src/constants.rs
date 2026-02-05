/// Default zstd compression level for SquashFS
pub const DEFAULT_ZSTD_COMPRESSION: u32 = 19;

/// Application name for directory naming (XDG_CACHE_HOME, etc.)
pub const APP_NAME: &str = "zero-kelvin-stazis";

/// Filename for the manifest inside the SquashFS image
pub const MANIFEST_FILENAME: &str = "list.yaml";

/// Directory name inside the SquashFS image where files are stored
pub const RESTORE_DIR_NAME: &str = "to_restore";

/// Size of the LUKS2 header in bytes
pub const LUKS_HEADER_SIZE: u64 = 32 * 1024 * 1024; // 32MB for LUKS2 header

/// Safety buffer size in bytes to avoid truncation
pub const LUKS_SAFETY_BUFFER: u64 = 128 * 1024 * 1024; // 128MB safety buffer to avoid truncation
