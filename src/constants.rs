/// Default zstd compression level for SquashFS
pub const DEFAULT_ZSTD_COMPRESSION: u32 = 19;

/// Application name for directory naming (XDG_CACHE_HOME, etc.)
pub const APP_NAME: &str = "zero-kelvin-stazis";

/// Filename for the manifest inside the SquashFS image
pub const MANIFEST_FILENAME: &str = "list.yaml";

/// Directory name inside the SquashFS image where files are stored
pub const RESTORE_DIR_NAME: &str = "to_restore";
