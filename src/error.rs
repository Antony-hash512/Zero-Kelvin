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
