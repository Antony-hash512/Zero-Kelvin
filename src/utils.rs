use anyhow::{Result, anyhow};
use std::fs;
use log::warn;

// Stub implementation for TDD phase

pub fn get_current_uid() -> Result<u32> {
    let content = fs::read_to_string("/proc/self/status")?;
    parse_uid_from_status(&content)
}

pub fn is_root() -> Result<bool> {
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

pub fn check_root_or_get_runner(reason: &str) -> Result<Option<String>> {
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
    
    Err(anyhow::anyhow!("Root privileges required but no elevation tool (sudo, doas, etc.) found."))
}

// Helpers for testing (not exposed)
fn parse_uid_from_status(content: &str) -> Result<u32> {
    for line in content.lines() {
        if line.starts_with("Uid:") {
            // Format: Uid: Puid Euid Suid Fsuid
            // Split by whitespace
            let parts: Vec<&str> = line.split_whitespace().collect();
            // parts[0] is "Uid:", parts[1] is RW, parts[2] is EUID
            if parts.len() >= 3 {
                return parts[2].parse().map_err(|e| anyhow!("Failed to parse UID: {}", e));
            }
        }
    }
    Err(anyhow!("Uid field not found in status"))
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

pub fn check_read_permissions(paths: &[PathBuf]) -> Result<bool> {
    for path in paths {
        // If path doesn't exist, we can't read it. But usually this should be checked before.
        // If it doesn't exist, returning error or false? 
        // Logic: "analyze permissions to targets". If target missing, freeze should fail.
        // Return error if missing.
        
        let metadata = fs::metadata(path).map_err(|e| anyhow!("Failed to access {:?}: {}", path, e))?;

        if metadata.is_dir() {
            if let Err(e) = fs::read_dir(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Ok(false);
                }
                return Err(anyhow!("Failed to read directory {:?}: {}", path, e));
            }
        } else {
            if let Err(e) = fs::File::open(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Ok(false);
                }
                return Err(anyhow!("Failed to open file {:?}: {}", path, e));
            }
        }
    }
    Ok(true)
}
