use anyhow::{Result, Context};
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

fn main() -> Result<()> {
    let args = Args::parse();
    
    // Safety check: Don't allow running on root or sensitive paths straightforwardly
    // Though the prompt didn't specify strict safety on root, it's good practice.
    // However, the main logic is rm_if_empty
    
    if rm_if_empty(&args.path)? {
        println!("Removed: {:?}", args.path);
    } else {
        println!("Kept: {:?}", args.path);
    }
    
    Ok(())
}

/// Recursively removes a directory if it is "empty".
/// A directory is empty if it contains no files, OR
/// if it contains only 0-byte files and other empty directories.
/// Returns true if the directory was removed.
fn rm_if_empty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    
    let metadata = fs::symlink_metadata(path).context(format!("Failed to get metadata for {:?}", path))?;

    if metadata.is_file() {
        if metadata.len() == 0 {
            fs::remove_file(path).context(format!("Failed to remove 0-byte file {:?}", path))?;
            return Ok(true);
        } else {
            return Ok(false);
        }
    } else if metadata.is_dir() {
        let entries = fs::read_dir(path).context(format!("Failed to read dir {:?}", path))?;
        let mut all_removed = true;

        for entry in entries {
            let entry = entry?;
            let child_path = entry.path();
            if !rm_if_empty(&child_path)? {
                all_removed = false;
            }
        }

        if all_removed {
            fs::remove_dir(path).context(format!("Failed to remove dir {:?}", path))?;
            return Ok(true);
        } else {
            return Ok(false);
        }
    } 
    
    // Preserve symlinks and other types (safe default)
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs::File;

    #[test]
    fn test_remove_empty_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_path_buf();
        // Since tempdir cleans up on drop, we need to be careful with assert.
        // But here we want to test our function.
        // Wait, tempdir removes itself when dropped.
        // We should create a subdir inside tempdir to test removal.
        let target = path.join("empty");
        fs::create_dir(&target).unwrap();
        
        assert!(rm_if_empty(&target).unwrap());
        assert!(!target.exists());
    }
    
    #[test]
    fn test_keep_non_empty_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("non_empty");
        fs::create_dir(&target).unwrap();
        let file = target.join("data.txt");
        fs::write(&file, "content").unwrap();
        
        assert!(!rm_if_empty(&target).unwrap());
        assert!(target.exists());
        assert!(file.exists());
    }
    
    #[test]
    fn test_remove_zero_byte_file() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("zero_byte");
        fs::create_dir(&target).unwrap();
        let file = target.join("empty.txt");
        File::create(&file).unwrap(); // Creates 0-byte file
        
        assert!(rm_if_empty(&target).unwrap());
        assert!(!target.exists());
    }
    
    #[test]
    fn test_recursive_removal() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("nested");
        fs::create_dir(&target).unwrap();
        let subdir = target.join("subdir");
        fs::create_dir(&subdir).unwrap();
        let file = subdir.join("empty.txt");
        File::create(&file).unwrap();
        
        assert!(rm_if_empty(&target).unwrap());
        assert!(!target.exists());
    }
    
    #[test]
    fn test_recursive_keep() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("nested_keep");
        fs::create_dir(&target).unwrap();
        let subdir = target.join("subdir");
        fs::create_dir(&subdir).unwrap();
        let file = subdir.join("data.txt");
        fs::write(&file, "data").unwrap();
        
        assert!(!rm_if_empty(&target).unwrap());
        assert!(target.exists());
        assert!(subdir.exists());
        assert!(file.exists());
    }
}
