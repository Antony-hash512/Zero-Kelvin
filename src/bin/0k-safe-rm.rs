use std::path::{Path, PathBuf};
use std::fs;
use clap::Parser;
use std::io;

#[derive(Parser, Debug)]
#[command(version, about = "Safely removes empty directories recursively")]
struct Args {
    /// Directory to clean
    #[arg(required = true)]
    path: PathBuf,
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();

    // Safety check: basic sanity check
    if !args.path.exists() {
        return std::process::ExitCode::SUCCESS;
    }

    // Atomic Operation:
    // 1. Scan: Ensure entire tree contains ONLY empty files (0 bytes) or directories.
    // 2. Delete: If scan ok, remove everything.

    // Safety check: ensure no active mount points exist inside the target
    if let Err(e) = check_no_active_mounts(&args.path) {
        eprintln!("Operation aborted: {}", e);
        return std::process::ExitCode::FAILURE;
    }

    match scan_for_non_empty(&args.path) {
        Ok(_) => {
            // All clear.
            let result = if args.path.is_file() {
                fs::remove_file(&args.path)
            } else {
                fs::remove_dir_all(&args.path)
            };
            if let Err(e) = result {
                eprintln!("Failed to remove {:?}: {}", args.path, e);
                return std::process::ExitCode::FAILURE;
            }
        },
        Err(e) => {
            // Found non-empty content or error. Abort.
            eprintln!("Operation aborted: {}", e);
            return std::process::ExitCode::FAILURE;
        }
    }

    std::process::ExitCode::SUCCESS
}

/// Checks that no active mount points exist within the given path.
/// Reads /proc/self/mountinfo (Linux-specific) to find all current mount points
/// and verifies none of them are inside our target directory.
/// This prevents catastrophic data loss if a bind mount from a crashed namespace
/// is still active — remove_dir_all would follow the mount and delete real data.
fn check_no_active_mounts(path: &Path) -> io::Result<()> {
    let canonical = path.canonicalize().map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("Cannot resolve path {:?}: {}", path, e))
    })?;
    let target_prefix = canonical.to_string_lossy().to_string();

    let mountinfo = match fs::read_to_string("/proc/self/mountinfo") {
        Ok(content) => content,
        Err(_) => {
            // If /proc is unavailable (container, exotic setup), skip the check
            // but warn the user
            eprintln!("Warning: Cannot read /proc/self/mountinfo. Skipping mount point safety check.");
            return Ok(());
        }
    };

    // mountinfo format (fields separated by spaces):
    // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
    // Field index 4 (0-based) is the mount point.
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let mount_point = unescape_mountinfo(fields[4]);
        // Check if this mount point is inside our target directory (or is the target itself)
        if mount_point.starts_with(&target_prefix) && mount_point.len() > target_prefix.len() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "Active mount point detected inside target: '{}'. \
                     This likely means a bind mount from a previous 0k session is still active. \
                     Please unmount it first (e.g., 'umount {}' or 'fusermount -u {}').",
                    mount_point, mount_point, mount_point
                ),
            ));
        }
    }

    Ok(())
}

/// Unescapes octal escape sequences in mountinfo paths.
/// The kernel escapes spaces as \040, tabs as \011, newlines as \012, etc.
fn unescape_mountinfo(s: &str) -> String {
    zero_kelvin::utils::unescape_mountinfo_octal(s)
}

/// Scans the path recursively. Returns Ok(()) if safe to delete (all empty).
/// Returns Err if any non-empty item found.
fn scan_for_non_empty(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to get metadata for {:?}: {}", path, e)))?;

    if metadata.is_file() {
        if metadata.len() > 0 {
             return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Found non-empty file: {:?} (size: {})", path, metadata.len())));
        }
        return Ok(());
    } else if metadata.is_dir() {
        let entries = fs::read_dir(path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to read dir {:?}: {}", path, e)))?;
        for entry in entries {
            let entry = entry?;
            scan_for_non_empty(&entry.path())?;
        }
        return Ok(());
    } else {
        // Symlinks or other types: Conservative approach.
        // If it's a symlink, even if it points to empty, the symlink itself is "content" in this context?
        // Or if user wants to delete structure with broken symlinks?
        // Let's assume symlink counts as "non-empty" content for now unless specified otherwise.
        // Actually, user said: "if directory contains ... only 0-byte files".
        // It implies we delete structure.
        // Let's count symlink as non-empty to be safe (it's not a 0-byte file).
        return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Found special file/symlink: {:?}", path)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_scan_ok_empty_structure() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let target = path.join("empty_struct");
        fs::create_dir_all(target.join("nest/nest2")).unwrap();
        File::create(target.join("zero.txt")).unwrap();
        File::create(target.join("nest/zero2.txt")).unwrap();
        
        assert!(scan_for_non_empty(&target).is_ok());
    }
    
    #[test]
    fn test_scan_fail_non_empty_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("data_struct");
        fs::create_dir_all(target.join("nest")).unwrap();
        File::create(target.join("zero.txt")).unwrap();
        fs::write(target.join("nest/data.txt"), "data").unwrap();
        
        assert!(scan_for_non_empty(&target).is_err());
    }
    
    // We can't test main directly easily without extensive mocking or separate binary test.
    // The integration tests in BATS will cover the full binary behavior (exit codes etc).

    #[test]
    fn test_unescape_mountinfo_plain() {
        assert_eq!(unescape_mountinfo("/tmp/0k-cache-1000"), "/tmp/0k-cache-1000");
    }

    #[test]
    fn test_unescape_mountinfo_space() {
        // Space is encoded as \040
        assert_eq!(unescape_mountinfo("/tmp/my\\040dir"), "/tmp/my dir");
    }

    #[test]
    fn test_unescape_mountinfo_tab() {
        // Tab is encoded as \011
        assert_eq!(unescape_mountinfo("/tmp/a\\011b"), "/tmp/a\tb");
    }

    #[test]
    fn test_unescape_mountinfo_no_octal() {
        // Backslash not followed by 3 digits should be kept as-is
        assert_eq!(unescape_mountinfo("/tmp/a\\bc"), "/tmp/a\\bc");
    }

    #[test]
    fn test_check_no_active_mounts_clean_dir() {
        let dir = tempdir().unwrap();
        // No mounts inside a fresh temp dir
        assert!(check_no_active_mounts(dir.path()).is_ok());
    }
}
