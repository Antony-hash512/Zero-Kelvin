//! Logging configuration for Zero-Kelvin
//!
//! Provides dual-output logging:
//! - Console: respects RUST_LOG env var
//! - File: writes to ~/.local/state/zero-kelvin/logs/
//!
//! Log files are rotated daily with automatic cleanup of old files.

use crate::constants::{APP_NAME, LOG_DIR_NAME};
use std::fs;
use std::path::PathBuf;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
    Layer,
};

/// Returns the log directory path: $XDG_STATE_HOME/zero-kelvin/logs/
/// Falls back to ~/.local/state/zero-kelvin/logs/
pub fn get_log_dir() -> PathBuf {
    let state_home = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/.local/state", home)
        });
    
    PathBuf::from(state_home).join(APP_NAME).join(LOG_DIR_NAME)
}

/// Initialize logging with dual output:
/// - Console (stderr): INFO level by default, respects RUST_LOG
/// - File: DEBUG level, rotates daily
///
/// Returns a guard that must be kept alive for the file appender to work.
/// When the guard is dropped, pending logs are flushed.
pub fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = get_log_dir();
    
    // Try to create log directory
    let file_guard = if fs::create_dir_all(&log_dir).is_ok() {
        // File appender with daily rotation
        let file_appender = RollingFileAppender::new(
            Rotation::DAILY,
            &log_dir,
            "0k.log",
        );
        
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        
        // File layer - DEBUG level for detailed logs
        let file_layer = fmt::layer()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(true)
            .with_line_number(true);
        
        // Console layer - respects RUST_LOG or defaults to INFO
        let console_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false);
        
        // Environment filter for console (file always gets DEBUG)
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"));
        
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .with(file_layer.with_filter(EnvFilter::new("debug")))
            .init();
        
        Some(guard)
    } else {
        // Fallback to console-only if log dir creation fails
        let console_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(false);
        
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"));
        
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .init();
        
        None
    };
    
    file_guard
}

/// Log a security-relevant event (failed access, privilege escalation, etc.)
#[macro_export]
macro_rules! security_event {
    ($($arg:tt)*) => {
        tracing::warn!(target: "security", $($arg)*)
    };
}

/// Log a security error (failed authentication, denied access, etc.)
#[macro_export]
macro_rules! security_error {
    ($($arg:tt)*) => {
        tracing::error!(target: "security", $($arg)*)
    };
}
