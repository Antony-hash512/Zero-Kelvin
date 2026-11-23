use crate::error::ZkError;
use crate::executor::CommandExecutor;
use log::warn;
use std::fs;
use std::path::Path;

pub enum ArchiveType {
    Tar,
    Gzip,
    Bzip2,
    Xz,
    Zstd,
    Zip,
    SevenZ,
    Rar,
    Squashfs,
    Unknown,
}

pub fn get_file_type(path: &Path) -> Result<ArchiveType, ZkError> {
    if !path.exists() {
        return Err(ZkError::InvalidPath(path.to_path_buf()));
    }
    
    // Check using infer (magic numbers)
    // We read the first few bytes
    let kind = infer::get_from_path(path)
        .map_err(|e| ZkError::IoError(e))?;
        
    match kind {
        Some(k) => {
            match k.mime_type() {
                "application/x-tar" => Ok(ArchiveType::Tar),
                "application/gzip" => Ok(ArchiveType::Gzip),
                "application/x-bzip2" => Ok(ArchiveType::Bzip2),
                "application/x-xz" => Ok(ArchiveType::Xz),
                "application/zstd" => Ok(ArchiveType::Zstd),
                "application/zip" => Ok(ArchiveType::Zip),
                "application/x-7z-compressed" => Ok(ArchiveType::SevenZ),
                "application/vnd.rar" => Ok(ArchiveType::Rar),
                // "application/vnd.squashfs" ?? infer 0.15 might not have it, let's check extension if mime unknown for sqfs 
                // OR custom check. infer supports squashfs since recent versions.
                // mime type for squashfs is often just "application/octet-stream" or specialized.
                // Let's check k.extension() just in case for squashfs
                _ => {
                     if k.extension() == "sqsh" || k.mime_type().contains("squashfs") {
                         Ok(ArchiveType::Squashfs)
                     } else {
                         Ok(ArchiveType::Unknown)
                     }
                }
            }
        },
        None => Ok(ArchiveType::Unknown),
    }
}

pub fn is_luks_image(image_path: &Path, executor: &impl CommandExecutor) -> bool {
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

// Stub implementation for TDD phase

pub fn get_current_uid() -> Result<u32, ZkError> {
    let content = fs::read_to_string("/proc/self/status").map_err(ZkError::IoError)?;
    parse_uid_from_status(&content)
}

pub fn is_root() -> Result<bool, ZkError> {
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

pub fn check_root_or_get_runner(reason: &str) -> Result<Option<String>, ZkError> {
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

    Err(ZkError::OperationFailed(
        "Root privileges required but no elevation tool (sudo, doas, etc.) found.".to_string(),
    ))
}

pub fn is_permission_denied(err: &ZkError) -> bool {
    match err {
        ZkError::IoError(e) => e.kind() == std::io::ErrorKind::PermissionDenied,
        ZkError::OperationFailed(msg) => {
            let msg_lower = msg.to_lowercase();
            msg_lower.contains("permission denied")
                || msg_lower.contains("operation not permitted")
                || msg_lower.contains("cannot initialize device-mapper")
                || msg_lower.contains("must be run as root")
                || msg_lower.contains("insufficient read permissions")
        }
        _ => false,
    }
}

fn current_executable_path() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(s) = exe.to_str() {
            return s.to_string();
        }
    }
    std::env::args().next().unwrap_or_default()
}

pub fn re_exec_with_runner(runner: &str) -> Result<(), ZkError> {
    use std::os::unix::process::CommandExt;

    let args: Vec<String> = std::env::args().collect();
    let program = current_executable_path();
    let cmd_args = args.get(1..).unwrap_or(&[]);

    // Construct command: runner /abs/path/to/0k args...
    let err = std::process::Command::new(runner)
        .arg(&program)
        .args(cmd_args)
        .exec();

    // exec replaces the process, so we only return if it fails
    Err(ZkError::OperationFailed(format!(
        "Failed to re-execute with {}: {}",
        runner, err
    )))
}

pub fn re_exec_with_runner_custom_args(runner: &str, new_args: &[String]) -> Result<(), ZkError> {
    use std::os::unix::process::CommandExt;

    let program = current_executable_path();

    // Construct command: runner /abs/path/to/0k new_args...
    let err = std::process::Command::new(runner)
        .arg(&program)
        .args(new_args)
        .exec();

    Err(ZkError::OperationFailed(format!(
        "Failed to re-execute with {} and custom args: {}",
        runner, err
    )))
}

fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
}

/// Resolves a sibling binary (e.g. `0k-core`) located next to `exe_dir`.
/// Falls back to bare `bin_name` (PATH lookup) if no executable sibling is found.
pub fn resolve_sibling_binary_in(exe_dir: Option<&Path>, bin_name: &str) -> String {
    let Some(dir) = exe_dir else {
        return bin_name.to_string();
    };
    let candidate = dir.join(bin_name);
    if is_executable_file(&candidate) {
        if let Some(path_str) = candidate.to_str() {
            return path_str.to_string();
        }
    }
    bin_name.to_string()
}

/// Resolves a sibling binary located in the same directory as the currently running executable.
/// Falls back to `bin_name` (standard PATH lookup) if not found next to the executable.
pub fn resolve_sibling_binary(bin_name: &str) -> String {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    resolve_sibling_binary_in(exe_dir.as_deref(), bin_name)
}

// Helpers for testing (not exposed)
fn parse_uid_from_status(content: &str) -> Result<u32, ZkError> {
    for line in content.lines() {
        if line.starts_with("Uid:") {
            // Format: Uid: Puid Euid Suid Fsuid
            // Split by whitespace
            let parts: Vec<&str> = line.split_whitespace().collect();
            // parts[0] is "Uid:", parts[1] is RW, parts[2] is EUID
            if parts.len() >= 3 {
                return parts[2]
                    .parse()
                    .map_err(|e| ZkError::OperationFailed(format!("Failed to parse UID: {}", e)));
            }
        }
    }
    Err(ZkError::OperationFailed(
        "Uid field not found in status".to_string(),
    ))
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

pub fn check_read_permissions(paths: &[PathBuf]) -> Result<bool, ZkError> {
    for path in paths {
        // If path doesn't exist, we can't read it. But usually this should be checked before.
        // If it doesn't exist, returning error or false?
        // Logic: "analyze permissions to targets". If target missing, freeze should fail.
        // Return error if missing.

        let metadata = fs::metadata(path).map_err(|_| ZkError::InvalidPath(path.clone()))?;

        if metadata.is_dir() {
            if let Err(e) = fs::read_dir(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Ok(false);
                }
                return Err(ZkError::IoError(e));
            }
        } else {
            if let Err(e) = fs::File::open(path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    return Ok(false);
                }
                return Err(ZkError::IoError(e));
            }
        }
    }
    Ok(true)
}

pub fn ensure_read_permissions(paths: &[PathBuf]) -> Result<(), ZkError> {
    if !check_read_permissions(paths)? {
        return Err(ZkError::OperationFailed(
            "Insufficient read permissions for one or more freeze targets".to_string(),
        ));
    }
    Ok(())
}

/// Returns the path to $TMPDIR/0k-cache-<uid> (or /tmp/0k-cache-<uid> if TMPDIR not set)
/// without ensuring it exists.
pub fn get_0k_temp_dir_path() -> Result<PathBuf, ZkError> {
    let uid = get_current_uid()?;
    let tmp_base = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
    Ok(PathBuf::from(format!("{}/0k-cache-{}", tmp_base, uid)))
}

/// Returns the path to /tmp/0k-cache-<uid> and ensures it exists with 0700 permissions.
/// Uses atomic mkdir + ownership verification to prevent symlink attacks (TOCTOU).
pub fn get_0k_temp_dir() -> Result<PathBuf, ZkError> {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    let path = get_0k_temp_dir_path()?;
    let uid = get_current_uid()?;

    // Attempt atomic create (not create_dir_all — that follows symlinks).
    match fs::create_dir(&path) {
        Ok(()) => {
            // We just created it — set permissions.
            let mut perms = fs::metadata(&path)
                .map_err(ZkError::IoError)?
                .permissions();
            perms.set_mode(0o700);
            fs::set_permissions(&path, perms).map_err(ZkError::IoError)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Directory already exists — verify it's safe to use:
            // 1. Must be a real directory (not a symlink)
            // 2. Must be owned by us
            // 3. Must have 0700 permissions
            let meta = fs::symlink_metadata(&path).map_err(ZkError::IoError)?;

            if meta.file_type().is_symlink() {
                return Err(ZkError::StagingError(format!(
                    "Security: {:?} is a symlink (possible attack). Refusing to use.",
                    path
                )));
            }
            if !meta.is_dir() {
                return Err(ZkError::StagingError(format!(
                    "Security: {:?} exists but is not a directory.",
                    path
                )));
            }
            if meta.uid() != uid {
                return Err(ZkError::StagingError(format!(
                    "Security: {:?} is owned by uid {} but we are uid {}. Refusing to use.",
                    path,
                    meta.uid(),
                    uid
                )));
            }

            // Fix permissions if needed
            let mut perms = meta.permissions();
            if perms.mode() & 0o777 != 0o700 {
                perms.set_mode(0o700);
                fs::set_permissions(&path, perms).map_err(ZkError::IoError)?;
            }
        }
        Err(e) => return Err(ZkError::IoError(e)),
    }

    Ok(path)
}

/// Unescape octal sequences in /proc/self/mountinfo and /proc/mounts paths (e.g., \040 → space).
pub fn unescape_mountinfo_octal(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            let d1 = bytes[i + 1];
            let d2 = bytes[i + 2];
            let d3 = bytes[i + 3];
            if (b'0'..=b'7').contains(&d1)
                && (b'0'..=b'7').contains(&d2)
                && (b'0'..=b'7').contains(&d3)
            {
                let val = (d1 - b'0') * 64 + (d2 - b'0') * 8 + (d3 - b'0');
                result.push(val);
                i += 4;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
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
        assert_eq!(
            expanded,
            PathBuf::from(format!("{}/Documents/file.txt", home))
        );
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

    #[test]
    fn test_resolve_sibling_binary_finds_executable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let core_bin = dir.path().join("0k-core");
        fs::write(&core_bin, "#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&core_bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&core_bin, perms).unwrap();

        let resolved = resolve_sibling_binary_in(Some(dir.path()), "0k-core");
        assert_eq!(resolved, core_bin.to_str().unwrap());
    }

    #[test]
    fn test_resolve_sibling_binary_ignores_non_executable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let core_bin = dir.path().join("0k-core");
        fs::write(&core_bin, "not executable").unwrap();
        let mut perms = fs::metadata(&core_bin).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(&core_bin, perms).unwrap();

        let resolved = resolve_sibling_binary_in(Some(dir.path()), "0k-core");
        assert_eq!(resolved, "0k-core");
    }

    #[test]
    fn test_resolve_sibling_binary_fallback_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(resolve_sibling_binary_in(Some(dir.path()), "0k-core"), "0k-core");
        assert_eq!(resolve_sibling_binary_in(None, "0k-core"), "0k-core");
    }
}
