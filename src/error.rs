use thiserror::Error;
use std::path::PathBuf;

#[derive(Error, Debug)]
pub enum ZksError {
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
}

impl ZksError {
    pub fn friendly_message(&self) -> Option<String> {
        match self {
            ZksError::IoError(e) => {
                // ENOSPC (28) -> No space left on device
                if let Some(code) = e.raw_os_error() {
                    if code == 28 {
                        return Some("Disk is full. Please free up space and try again.".to_string());
                    }
                }
                None
            },
            ZksError::LuksError(msg) | ZksError::OperationFailed(msg) => {
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
