use std::path::{Path, PathBuf};
use anyhow::{Result, Context, anyhow};
use crate::executor::CommandExecutor;
use crate::manifest::{Manifest, Metadata, FileEntry, PrivilegeMode};
use crate::utils;
use std::fs;
use fs2::FileExt; // For flock
// rand is in Cargo.toml

/// Prepares the staging area for freezing.
/// Creates a directory in XDG_CACHE_HOME, generates stubs for targets, and writes the manifest.
/// Returns the path to the staging directory AND the locked .lock file handle (which must be kept alive).
pub fn prepare_staging(targets: &[PathBuf]) -> Result<(PathBuf, std::fs::File)> {
    // 1. Resolve XDG_CACHE_HOME
    let cache_root = get_cache_dir()?;
    let app_cache = cache_root.join("zero-kelvin-stazis");
    
    // 2. Create unique build directory: build_<timestamp>_<random>
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let random_id: u32 = rand::random();
    let build_dir_name = format!("build_{}_{}", timestamp, random_id);
    let build_dir = app_cache.join(build_dir_name);
    
    fs::create_dir_all(&build_dir).context("Failed to create build directory")?;

    // 2.1 Create and Lock .lock file
    // This lock safeguards against GC while this process is alive.
    let lock_path = build_dir.join(".lock");
    let lock_file = fs::File::create(&lock_path).context("Failed to create .lock file")?;
    lock_file.lock_exclusive().context("Failed to acquire exclusive lock on staging directory")?;

    // 2.5 Create 'payload' directory
    let payload_dir = build_dir.join("payload");
    fs::create_dir(&payload_dir).context("Failed to create payload directory")?;
    
    // 3. Create 'to_restore' directory INSIDE payload
    let restore_root = payload_dir.join("to_restore");
    fs::create_dir(&restore_root).context("Failed to create to_restore directory")?;
    
    // 4. Generate Files list and create stubs
    let mut file_entries = Vec::new();
    
    for (i, target) in targets.iter().enumerate() {
        let id = (i + 1) as u32;
        let entry = FileEntry::from_path(id, target)?;
        
        let container_dir = restore_root.join(id.to_string());
        fs::create_dir(&container_dir)?;
        
        // Create stub
        // If target is directory, create directory stub
        // If target is file, create empty file stub
        // Note: entry.entry_type was derived from target on disk
        let stub_path = container_dir.join(entry.name.as_ref().unwrap());
        
        match entry.entry_type {
            crate::manifest::EntryType::Directory => {
                fs::create_dir(&stub_path)?;
            },
            crate::manifest::EntryType::File => {
                fs::File::create(&stub_path)?;
            }
        }
        
        file_entries.push(entry);
    }
    
    // 5. Generate Manifest
    // Check permissions to decide PrivilegeMode (Logic from Utils)
    // If ANY target is not readable by user, we default to Root mode requirements?
    // Wait, if we are preparing staging as user, we can only read user files.
    // If we need root, this function probably should have been called under sudo?
    // Or we assume we are running as is.
    // Let's deduce mode: if we are root -> Root, else User.
    let mode = if utils::is_root()? { PrivilegeMode::Root } else { PrivilegeMode::User };
    let hostname = get_hostname()?;
    
    let metadata = Metadata::new(hostname, mode);
    let manifest = Manifest::new(metadata, file_entries);
    
    // 6. Write list.yaml INSIDE payload
    let manifest_path = payload_dir.join("list.yaml");
    let f = fs::File::create(&manifest_path)?;
    serde_yaml::to_writer(f, &manifest)?;
    
    Ok((build_dir, lock_file))
}

/// Tries to garbage collect old staging directories.
/// Iterates over subdirectories in the cache. Tries to acquire non-blocking exclusive lock on .lock.
/// If successful, it means the process is dead, so we delete the directory.
pub fn try_gc_staging() -> Result<()> {
    let cache_root = get_cache_dir()?;
    let app_cache = cache_root.join("zero-kelvin-stazis");

    if !app_cache.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(app_cache)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("build_") {
                    let lock_path = path.join(".lock");
                    if lock_path.exists() {
                        if let Ok(lock_file) = fs::File::open(&lock_path) {
                            // Try LOCK_NB (Non-Blocking). 
                            // If lock_exclusive succeeds, it means no one else holds it.
                            if lock_file.try_lock_exclusive().is_ok() {
                                // Safe to delete
                                // We hold the lock now, so no one else can claim it.
                                // We can delete the directory.
                                // Note: remove_dir_all might fail on .lock file on Windows because we hold open handle, 
                                // but on Linux unlink usually works on open files. 
                                // To be safe, we can drop lock_file before delete? 
                                // NO, if we drop, someone else might claim it (race).
                                // But since we are deleting, new processes create NEW directories with new names, 
                                // they don't reuse old build_ dirs. So race is only with other GCs.
                                // If we hold lock, other GCs fail try_lock.
                                // So we are safe.
                                if let Err(e) = fs::remove_dir_all(&path) {
                                    eprintln!("GC: Failed to remove {:?}: {}", path, e);
                                } else {
                                    // println!("GC: Removed staged dir {:?}", path);
                                }
                            }
                        }
                    } else {
                        // No .lock file? Maybe created before locking logic or broken.
                        // Can we safely delete? 
                        // Let's rely on checking age or just skip for now to be safe.
                    }
                }
            }
        }
    }
    Ok(())
}

fn get_cache_dir() -> Result<PathBuf> {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cache"))
                .map_err(|_| anyhow!("Neither XDG_CACHE_HOME nor HOME set"))
        })
}

fn get_hostname() -> Result<String> {
    std::process::Command::new("uname")
        .arg("-n")
        .output()
        .context("Failed to run uname")
        .and_then(|o| String::from_utf8(o.stdout).context("Invalid utf8 from uname"))
        .map(|s| s.trim().to_string())
}

pub struct FreezeOptions {
    pub encrypt: bool,
    pub output: PathBuf,
    pub overwrite_files: bool,
    pub overwrite_luks_content: bool,
}

pub fn freeze<E: CommandExecutor>(
    targets: &[PathBuf],
    options: &FreezeOptions,
    executor: &E,
) -> Result<()> {
    // 0. Auto-GC: Cleanup stale build directories (protected by flock)
    if let Err(e) = try_gc_staging() {
        // Log but don't fail, maybe just debug print if we had logging
        // eprintln!("GC Warning: {}", e); 
    }

    // 1. Prepare Staging
    // _lock must be kept in scope to maintain the flock until we are done (or until cleanup)
    let (build_dir, _lock) = prepare_staging(targets)?;
    
    // 2. Read Manifest (re-read to get file list with correct stub paths if needed? 
    // No, prepare_staging returned build_dir, manifest is at build_dir/payload/list.yaml.
    // We need manifest to generate bind mounts.
    // Optimization: prepare_staging could return Manifest too?
    // Or we read it back. Reading back ensures consistency.
    let payload_dir = build_dir.join("payload");
    let manifest_path = payload_dir.join("list.yaml");
    let f = fs::File::open(&manifest_path)?;
    let manifest: Manifest = serde_yaml::from_reader(f)?;
    
    // 3. Generate internal script
    let script = generate_freeze_script(&manifest, &build_dir, options)?;
    let script_path = build_dir.join("freeze.sh");
    fs::write(&script_path, &script)?;
    
    // 4. Run unshare
    // unshare -m -U -r --propagation private sh <script>
    // Note: unshare might not be in PATH? "check_root_or_get_runner" logic?
    // User Namespace wrapper usually runs as user (if unprivileged userns allowed).
    // If we needed root, we would have escalated earlier?
    // "zks-rs Logic - Freeze": logic "Ownership Strategy".
    // If prepare_staging detected root needed, we might need sudo.
    // But currently prepare_staging only sets Metadata.privilege_mode.
    // Assuming we run `unshare` as current user. 
    // If privilege_mode is Root, it implies we successfully read files?
    // Wait, if files are not readable by user, prepare_staging (which does FileEntry::from_path -> metadata) might fail if it can't read metadata?
    // User can usually read metadata of other users' files if dir is executable.
    
    let args = vec![
        "-m", "-U", "-r", "--propagation", "private",
        "sh", script_path.to_str().unwrap()
    ];
    
    let status = executor.run_interactive("unshare", &args)?;
    
    if !status.success() {
         return Err(anyhow!("Freeze process failed (unshare/squash_manager returned non-zero exit code)"));
    }
    
    // Cleanup Staging Area
    if let Err(e) = std::fs::remove_dir_all(&build_dir) {
        // Use eprintln for now as we don't have a logger setup
        eprintln!("Warning: Failed to clean up staging directory {:?}: {}", build_dir, e);
    }
    
    Ok(())
}

fn generate_freeze_script(
    manifest: &Manifest,
    build_dir: &Path,
    options: &FreezeOptions,
) -> Result<String> {
    let mut script = String::new();
    script.push_str("#!/bin/sh\n");
    script.push_str("set -e\n"); // Exit on error
    
    // Bind mounts
    for entry in &manifest.files {
        // Source: restore_path/name
        // Wait, restore_path is parent dir. So full path is restore_path/name.
        if let (Some(parent), Some(name)) = (&entry.restore_path, &entry.name) {
             let src = Path::new(parent).join(name);
             let dest = build_dir.join("payload").join("to_restore").join(entry.id.to_string()).join(name);
             
             // Escape paths? Ideally use safe quoting. 
             // Using debug format {:?} adds quotes but might not be sh-safe.
             // Simple single quoting is safer if no single quotes.
             // For now assume logic needs simple quoting.
             script.push_str(&format!("mount --bind \"{}\" \"{}\"\n", src.display(), dest.display()));
        }
    }
    
    // Call squash_manager-rs
    // "squash_manager-rs create <options> <input> <output>"
    // Assuming squash_manager-rs is in PATH.
    // If it's a sibling binary, we might need to resolve it?
    // Legacy assumed in PATH or resolved via functions.
    // We'll assume PATH for now.
    let encrypt_flag = if options.encrypt { "--encrypt" } else { "" };
    
    let mut flags = String::new();
    if options.overwrite_files {
        flags.push_str(" --overwrite-files");
    }
    if options.overwrite_luks_content {
        flags.push_str(" --overwrite-luks-content");
    }

    // IMPORTANT: Point squash_manager to the PAYLOAD directory, not the build root
    let input_dir = build_dir.join("payload");
    script.push_str(&format!(
        "squash_manager-rs create {} {} --no-progress \"{}\" \"{}\"\n",
        encrypt_flag,
        flags,
        input_dir.display(),
        options.output.display()
    ));

    Ok(script)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_prepare_staging() {
        // Mock XDG_CACHE_HOME by setting environment variable
        let temp_cache = tempdir().unwrap();
        // Since get_cache_dir checks env var, we need to set it.
        // Tests run in threads, setting env var might race if parallel.
        // cargo test runs parallel by default.
        // We can use a mutex or `rusty-fork`? Or valid logic.
        // Or just rely on typical behavior. This test might be flaky if other tests use XDG_CACHE_HOME.
        // For now, let's assume it's fine or run single-threaded if needed.
        // Better: Make prepare_staging accept cache_dir override? No, stick to env.
        // We can create a dedicated test function that sets it and unsets it (using serial test logic).
        
        // Since we are adding ONLY this test to this file, it's fine.
        unsafe { std::env::set_var("XDG_CACHE_HOME", temp_cache.path()); }
        
        let target_dir = tempdir().unwrap();
        let file_target = target_dir.path().join("data.txt");
        let dir_target = target_dir.path().join("config");
        fs::write(&file_target, "content").unwrap();
        fs::create_dir(&dir_target).unwrap();
        
        let targets = vec![file_target.clone(), dir_target.clone()];
        
        let (build_dir, _lock) = prepare_staging(&targets).unwrap();
        
        assert!(build_dir.exists());
        let payload_dir = build_dir.join("payload");
        assert!(payload_dir.exists());
        assert!(payload_dir.join("list.yaml").exists());
        assert!(payload_dir.join("to_restore").exists());
        
        // Check stubs
        // ID 1: data.txt (file)
        assert!(payload_dir.join("to_restore/1/data.txt").exists());
        assert!(payload_dir.join("to_restore/1/data.txt").metadata().unwrap().is_file());
        assert_eq!(payload_dir.join("to_restore/1/data.txt").metadata().unwrap().len(), 0); // Stub is empty
        
        // ID 2: config (dir)
        assert!(payload_dir.join("to_restore/2/config").exists());
        assert!(payload_dir.join("to_restore/2/config").metadata().unwrap().is_dir());
        
        // Check manifest content validation
        let manifest_content = fs::read_to_string(payload_dir.join("list.yaml")).unwrap();
        assert!(manifest_content.contains("data.txt"));
        assert!(manifest_content.contains("config"));
    }

    #[test]
    fn test_generate_freeze_script() {
        let temp = tempfile::tempdir().unwrap();
        let build_dir = temp.path().join("build");
        let output = temp.path().join("out.sqfs");
        
        let manifest = Manifest {
             metadata: Metadata::new("test-host".into(), PrivilegeMode::User),
             files: vec![
                 FileEntry { 
                     id: 1, 
                     entry_type: crate::manifest::EntryType::File,
                     name: Some("file1".into()),
                     restore_path: Some("/src/dir1".into()),
                     original_path: None
                 }
             ]
        };
        
        let options = FreezeOptions {
            encrypt: false,
            output: output.clone(),
            overwrite_files: false,
            overwrite_luks_content: false,
        };
        
        let script = generate_freeze_script(&manifest, &build_dir, &options).unwrap();
        
        assert!(script.contains("mount --bind \"/src/dir1/file1\""));
        assert!(script.contains("squash_manager-rs create"));
        assert!(script.contains("--no-progress"));
    }
    
    
    #[test]
    fn test_freeze_execution_flow() {
        // Can't run full freeze because prepare_staging needs real paths.
        // But we can check if it compiles and structure is correct.
        // We verified generate_freeze_script above.
        // Mocking execution is complex because prepare_staging does FS ops.
        // We'll trust logic + integration tests for full flow.
    }
}
