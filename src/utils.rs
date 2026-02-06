use crate::error::ZksError;
use std::fs;
use log::warn;

// Stub implementation for TDD phase

pub fn get_current_uid() -> Result<u32, ZksError> {
    let content = fs::read_to_string("/proc/self/status").map_err(ZksError::IoError)?;
    parse_uid_from_status(&content)
}

pub fn is_root() -> Result<bool, ZksError> {
    let euid = get_current_uid()?;
    Ok(euid == 0)
}

pub fn get_superuser_command() -> Option<String> {
    let tools = ["sudo", "doas", "run0", "pkexec"];
    for tool in tools {
        if which::which(tool).is_ok() {
            return Some(tool.to_string());
        }
    }
    None
}

pub fn check_root_or_get_runner(reason: &str) -> Result<Option<String>, ZksError> {
    if is_root()? {
        return Ok(None);
    }
    
    // Not root, check for runner
    if let Some(runner) = get_superuser_command() {
        warn!("{}", reason); // Using log::warn as implied by context
        // Also print to stderr for visibility if logger not configured
        eprintln!("Info: {}", reason);
        return Ok(Some(runner));
    }
    
    Err(ZksError::OperationFailed("Root privileges required but no elevation tool (sudo, doas, etc.) found.".to_string()))
}

pub fn is_permission_denied(err: &ZksError) -> bool {
    match err {
        ZksError::IoError(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
        // Also handle when OperationFailed might perform checks, but usually it's IO
        _ => false,
    }
}

pub fn re_exec_with_runner(runner: &str) -> Result<(), ZksError> {
    use std::os::unix::process::CommandExt;
    
    let args: Vec<String> = std::env::args().collect();
    // args[0] is the current binary path
    let program = &args[0];
    let cmd_args = &args[1..];
    
    // Construct command: runner program args...
    // But Runner might be "sudo".
    // We want: sudo /path/to/zks freeze ...
    
    let err = std::process::Command::new(runner)
        .arg(program)
        .args(cmd_args)
        .exec();
        
    // exec replaces the process, so we only return if it fails
    Err(ZksError::OperationFailed(format!("Failed to re-execute with {}: {}", runner, err)))
}

// Helpers for testing (not exposed)
fn parse_uid_from_status(content: &str) -> Result<u32, ZksError> {
    for line in content.lines() {
        if line.starts_with("Uid:") {
            // Format: Uid: Puid Euid Suid Fsuid
            // Split by whitespace
            let parts: Vec<&str> = line.split_whitespace().collect();
            // parts[0] is "Uid:", parts[1] is RW, parts[2] is EUID
            if parts.len() >= 3 {
                return parts[2].parse().map_err(|e| ZksError::OperationFailed(format!("Failed to parse UID: {}", e)));
            }
        }
    }
    Err(ZksError::OperationFailed("Uid field not found in status".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_root() tests ---
    // Since is_root interacts with OS, we test the parsing logic primarily
    
    #[test]
    fn test_parse_uid_effective_root() {
        // "Uid: Real Effective Saved Filesystem"
        // Effective is the 2nd value.
        let status_content = "Name:\tzks\nState:\tR (running)\nUid:\t1000\t0\t1000\t1000\nGid:\t1000\t1000\t1000\t1000";
        let uid = parse_uid_from_status(status_content).unwrap();
        assert_eq!(uid, 0, "Should parse effective UID as 0");
    }

    #[test]
    fn test_parse_uid_real_root_only() {
        // Real=0, Effective=1000. Not root.
        let status_content = "Uid:\t0\t1000\t1000\t1000";
        let uid = parse_uid_from_status(status_content).unwrap();
        assert_eq!(uid, 1000, "Should parse effective UID as 1000");
    }
    
    #[test]
    fn test_parse_uid_standard_user() {
        let status_content = "Uid:\t1000\t1000\t1000\t1000";
        let uid = parse_uid_from_status(status_content).unwrap();
        assert_eq!(uid, 1000);
    }
    
    // --- check_root_or_get_runner tests ---
    // We can't easily mock is_root() and get_superuser_command() here without dependency injection or conditional compilation mocking.
    // For now, we will verify the parser logic as requested in the Prompt. 
    // Testing get_superuser_command() implies testing `which` or system state. 
    
    // We can add a "simulated" test that doesn't rely on system state if we refactor `check_root_or_get_runner` 
    // to take a closure for `is_root_check`. But let's stick to the prompt's request for "unit tests for parser".

    // --- check_read_permissions tests ---
    
    #[test]
    fn test_check_read_permissions_readable() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("readable.txt");
        fs::write(&file, "content").unwrap();
        
        // Pass slice of PathBuf
        let paths = vec![file];
        assert!(check_read_permissions(&paths).unwrap());
    }
    
    #[test]
    fn test_check_read_permissions_unreadable() {
        // Skip this test if we are root, because root can read everything
        if is_root().unwrap() {
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("secret.txt");
        fs::write(&file, "content").unwrap();
        
        // Make unreadable
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_mode(0o000);
        fs::set_permissions(&file, perms).unwrap();
        
        let paths = vec![file];
        assert!(!check_read_permissions(&paths).unwrap());
    }
}

use std::path::PathBuf;

pub fn check_read_permissions(paths: &[PathBuf]) -> Result<bool, ZksError> {
    for path in paths {
        // If path doesn't exist, we can't read it. But usually this should be checked before.
        // If it doesn't exist, returning error or false? 
        // Logic: "analyze permissions to targets". If target missing, freeze should fail.
        // Return error if missing.
        
        let metadata = fs::metadata(path).map_err(|_| ZksError::InvalidPath(path.clone()))?;

        if metadata.is_dir() {
            if let Err(e) = fs::read_dir(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Ok(false);
                }
                return Err(ZksError::IoError(e));
            }
        } else {
            if let Err(e) = fs::File::open(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Ok(false);
                }
                return Err(ZksError::IoError(e));
            }
        }
    }
    Ok(true)
}

pub fn ensure_read_permissions(paths: &[PathBuf]) -> Result<(), ZksError> {
    for path in paths {
        let metadata = fs::metadata(path).map_err(|_| ZksError::InvalidPath(path.clone()))?;

        if metadata.is_dir() {
            if let Err(e) = fs::read_dir(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Err(ZksError::IoError(e));
                }
                return Err(ZksError::IoError(e));
            }
        } else {
            if let Err(e) = fs::File::open(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Err(ZksError::IoError(e));
                }
                return Err(ZksError::IoError(e));
            }
        }
    }
    Ok(())
}


/// Returns the path to /tmp/stazis-<uid> without ensuring it exists.
pub fn get_stazis_temp_dir_path() -> Result<PathBuf, ZksError> {
    let uid = get_current_uid()?;
    Ok(PathBuf::from(format!("/tmp/stazis-{}", uid)))
}

/// Returns the path to /tmp/stazis-<uid> and ensures it exists with 0700 permissions.
pub fn get_stazis_temp_dir() -> Result<PathBuf, ZksError> {
    use std::os::unix::fs::PermissionsExt;
    let path = get_stazis_temp_dir_path()?;
    
    if !path.exists() {
        fs::create_dir_all(&path).map_err(ZksError::IoError)?;
    }
    
    let mut perms = fs::metadata(&path).map_err(ZksError::IoError)?.permissions();
    if perms.mode() & 0o777 != 0o700 {
        perms.set_mode(0o700);
        fs::set_permissions(&path, perms).map_err(ZksError::IoError)?;
    }
    
    Ok(path)
}

/// Expands a tilde (~) at the start of a path to the user's HOME directory.
/// Supports "~/" and "~" (exact). Does NOT support "~user".
/// Returns the original path if HOME is not set or tilde is not present.
pub fn expand_tilde(path_str: &str) -> PathBuf {
    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    } else if path_str == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(path_str)
}

#[cfg(test)]
mod tests_expand {
    use super::*;

    #[test]
    fn test_expand_tilde_home() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let expanded = expand_tilde("~");
        assert_eq!(expanded, PathBuf::from(&home));
    }

    #[test]
    fn test_expand_tilde_path() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let expanded = expand_tilde("~/Documents/file.txt");
        assert_eq!(expanded, PathBuf::from(format!("{}/Documents/file.txt", home)));
    }

    #[test]
    fn test_no_expand_absolute() {
        let path = "/tmp/file";
        assert_eq!(expand_tilde(path), PathBuf::from(path));
    }

    #[test]
    fn test_no_expand_relative() {
        let path = "Documents/file.txt";
        assert_eq!(expand_tilde(path), PathBuf::from(path));
    }
}
