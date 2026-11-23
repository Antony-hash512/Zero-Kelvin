use thiserror::Error;
use std::path::PathBuf;

#[derive(Error, Debug)]
pub enum ZkError {
    #[error("Manifest error: {0}")]
    ManifestError(#[from] serde_yaml::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("LUKS error: {0}")]
    LuksError(String),

    #[error("Staging error: {0}")]
    StagingError(String),
    
    #[error("Operation failed: {0}")]
    OperationFailed(String),

    #[error("Unknown error: {0}")]
    Unknown(#[from] anyhow::Error),
    
    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),
    
    #[error("Missing target: {0}")]
    MissingTarget(String),

    #[error("Optional dependency '{tool}' is not installed (required for {mode}).\nInstall '{package_hint}' to use this mode.")]
    MissingOptionalDependency {
        tool: String,
        mode: String,
        package_hint: String,
    },

    #[error("Required tool '{tool}' not found in PATH.\nInstall package '{package_hint}' to use this command.")]
    MissingTool {
        tool: String,
        package_hint: String,
    },

    /// CLI argument parsing resulted in an error that was already printed.
    /// Carries the desired process exit code (e.g. 2 for invalid subcommand).
    #[error("")]
    CliExit(u8),
}

impl ZkError {
    pub fn exit_code(&self) -> u8 {
        match self {
            ZkError::CliExit(code) => *code,
            ZkError::MissingOptionalDependency { .. } | ZkError::MissingTool { .. } => {
                crate::constants::EXIT_CODE_MISSING_DEPENDENCY
            }
            _ => 1,
        }
    }

    pub fn friendly_message(&self) -> Option<String> {
        match self {
            ZkError::IoError(e) => {
                // ENOSPC (28) -> No space left on device
                if let Some(code) = e.raw_os_error() {
                    if code == 28 {
                        return Some("Disk is full. Please free up space and try again.".to_string());
                    }
                }
                None
            },
            ZkError::LuksError(msg) | ZkError::OperationFailed(msg) => {
                // Common cryptsetup/luks errors
                // Note: cryptsetup usually prints to stderr, but if we captured it in msg:
                if msg.to_lowercase().contains("no key available with this passphrase") {
                    return Some("Incorrect passphrase provided.".to_string());
                }
                None
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::EXIT_CODE_MISSING_DEPENDENCY;

    #[test]
    fn test_missing_optional_dependency_exit_code_and_message() {
        let err = ZkError::MissingOptionalDependency {
            tool: "tar2sqfs".to_string(),
            mode: "archive repacking mode".to_string(),
            package_hint: "squashfs-tools-ng".to_string(),
        };

        assert_eq!(err.exit_code(), EXIT_CODE_MISSING_DEPENDENCY);
        assert_eq!(err.exit_code(), 127);

        let msg = err.to_string();
        assert!(msg.contains("tar2sqfs"));
        assert!(msg.contains("archive repacking mode"));
        assert!(msg.contains("squashfs-tools-ng"));
    }

    #[test]
    fn test_default_and_cli_exit_codes() {
        assert_eq!(ZkError::CliExit(2).exit_code(), 2);
        assert_eq!(ZkError::OperationFailed("boom".to_string()).exit_code(), 1);
    }

    #[test]
    fn test_missing_tool_exit_code_and_message() {
        let err = ZkError::MissingTool {
            tool: "squashfuse".to_string(),
            package_hint: "squashfuse".to_string(),
        };

        assert_eq!(err.exit_code(), EXIT_CODE_MISSING_DEPENDENCY);
        assert_eq!(err.exit_code(), 127);

        let msg = err.to_string();
        assert!(msg.contains("squashfuse"));
        assert!(msg.contains("not found in PATH"));
    }
}
