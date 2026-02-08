use std::path::{Path, PathBuf};
use std::fs;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about = "Safely removes empty directories recursively")]
struct Args {
    /// Directory to clean
    #[arg(required = true)]
    path: PathBuf,
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();
    
    // Safety check: basic sanity check
    if !args.path.exists() {
        return Ok(());
    }

    // Atomic Operation:
    // 1. Scan: Ensure entire tree contains ONLY empty files (0 bytes) or directories.
    // 2. Delete: If scan ok, remove everything.

    match scan_for_non_empty(&args.path) {
        Ok(_) => {
            // All clear.
            if args.path.is_file() {
                 fs::remove_file(&args.path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to remove file: {}", e)))?;
            } else {
                 fs::remove_dir_all(&args.path).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to remove directory tree: {}", e)))?;
            }
            // println!("Removed: {:?}", args.path); // Quiet by default? Or log?
        },
        Err(e) => {
            // Found non-empty content or error. Abort.
            eprintln!("Operation aborted: {}", e);
            std::process::exit(1);
        }
    }
    
    Ok(())
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
}
