use crate::error::ZkError;
use crate::executor::CommandExecutor;
use crate::manifest::{FileEntry, Manifest, Metadata, PrivilegeMode};
use crate::utils;
use fs2::FileExt;
use std::fs;
use std::path::{Path, PathBuf}; // For flock
// rand is in Cargo.toml
use log::{info, warn};
use tempfile;

/// Prepares the staging area for freezing.
/// Creates a directory in XDG_CACHE_HOME, generates stubs for targets, and writes the manifest.
/// Returns the path to the staging directory AND the locked .lock file handle (which must be kept alive).
pub fn prepare_staging(
    targets: &[PathBuf],
    dereference: bool,
) -> Result<(PathBuf, String, std::fs::File), ZkError> {
    // 1. Resolve Staging Root: /tmp/0k-cache-<uid>
    let staging_root = utils::get_0k_temp_dir()?;

    // 2. Create unique build directory: /tmp/0k-cache-<uid>/build_<timestamp>_<random>
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| ZkError::OperationFailed(format!("Time error: {}", e)))?
        .as_secs();
    let random_id: u32 = rand::random();
    let build_dir_name = format!("build_{}_{}", timestamp, random_id);
    let build_dir = staging_root.join(build_dir_name);

    fs::create_dir_all(&build_dir).map_err(|e| {
        ZkError::StagingError(format!(
            "Failed to create build dir: {:?} ({})",
            build_dir, e
        ))
    })?;

    // 2.1 Create and Lock .lock file
    let lock_path = build_dir.join(".lock");
    let lock_file = fs::File::create(&lock_path)
        .map_err(|e| ZkError::StagingError(format!("Failed to create .lock file: {}", e)))?;
    lock_file.lock_exclusive().map_err(|e| {
        ZkError::StagingError(format!(
            "Failed to acquire exclusive lock on staging directory: {}",
            e
        ))
    })?;

    // 2.5 Create payload directory (fixed name)
    // The payload directory always uses the name "payload".
    // Output filename prefix is handled by 0k via --prefix flag or interactive prompt.
    let payload_name = "payload".to_string();

    let payload_dir = build_dir.join(&payload_name);
    fs::create_dir(&payload_dir).map_err(|e| {
        ZkError::StagingError(format!("Failed to create payload directory: {}", e))
    })?;

    // 3. Create 'to_restore' directory INSIDE payload
    let restore_root = payload_dir.join("to_restore");
    fs::create_dir(&restore_root).map_err(|e| {
        ZkError::StagingError(format!("Failed to create to_restore directory: {}", e))
    })?;

    // 4. Generate Files list and create stubs
    let mut file_entries = Vec::new();

    for (i, target) in targets.iter().enumerate() {
        let id = (i + 1) as u32;
        let entry = FileEntry::from_path(id, target, dereference)?;

        let container_dir = restore_root.join(id.to_string());
        fs::create_dir(&container_dir)?;

        // Create stub
        let stub_path = container_dir.join(entry.name.as_ref().unwrap());

        match entry.entry_type {
            crate::manifest::EntryType::Directory => {
                fs::create_dir(&stub_path)?;
            }
            crate::manifest::EntryType::File => {
                fs::File::create(&stub_path)?;
            }
            crate::manifest::EntryType::Symlink => {
                let link_target = fs::read_link(target).map_err(|e| ZkError::IoError(e))?;
                std::os::unix::fs::symlink(&link_target, &stub_path)?;
            }
        }

        file_entries.push(entry);
    }

    // 5. Generate Manifest
    let mode = if utils::is_root()? {
        PrivilegeMode::Root
    } else {
        PrivilegeMode::User
    };
    let hostname = get_hostname()?;

    let metadata = Metadata::new(hostname, mode);
    let manifest = Manifest::new(metadata, file_entries);

    // 6. Write list.yaml INSIDE payload
    let manifest_path = payload_dir.join("list.yaml");
    let f = fs::File::create(&manifest_path)?;
    serde_yaml::to_writer(f, &manifest)?;

    Ok((build_dir, payload_name, lock_file))
}

/// Tries to garbage collect old staging directories.
/// Iterates over subdirectories in the cache. Tries to acquire non-blocking exclusive lock on .lock.
/// If successful, it means the process is dead, so we delete the directory.
pub fn try_gc_staging() -> Result<(), ZkError> {
    let staging_root = utils::get_0k_temp_dir_path()?;

    if !staging_root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&staging_root).map_err(ZkError::IoError)? {
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
                                    warn!("GC: Failed to remove {:?}: {}", path, e);
                                } else {
                                    info!("GC: Removed stale staging dir {:?}", path);
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

fn get_hostname() -> Result<String, ZkError> {
    std::process::Command::new("uname")
        .arg("-n")
        .output()
        .map_err(|e| ZkError::OperationFailed(format!("Failed to run uname: {}", e)))
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .map_err(|e| ZkError::OperationFailed(format!("Invalid utf8 from uname: {}", e)))
        })
        .map(|s| s.trim().to_string())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProgressMode {
    None,
    Vanilla, // Standard mksquashfs bar
    Alfa,    // Placeholder for future advanced bar
}

pub struct FreezeOptions {
    pub encrypt: bool,
    pub output: PathBuf,
    pub overwrite_files: bool,
    pub overwrite_luks_content: bool,
    pub progress_mode: ProgressMode,
    pub compression: Option<u32>,
    pub dereference: bool,
}

pub struct UnfreezeOptions {
    pub overwrite: bool,
    pub skip_existing: bool,
}

pub struct CheckOptions {
    pub use_cmp: bool,
    pub delete: bool,
}

pub fn check<E: CommandExecutor>(
    archive_path: &Path,
    options: &CheckOptions,
    executor: &E,
) -> Result<(), ZkError> {
    // 0. Check for LUKS (requires Root to mount)
    // If it is LUKS and we are not root, fail early to trigger elevation retry in 0k
    if utils::is_luks_image(archive_path, executor) {
        if !utils::is_root().unwrap_or(false) {
             return Err(ZkError::OperationFailed("Permission denied: Checking LUKS archive requires root privileges to mount.".to_string()));
        }
    }

    // 1. Mount Archive
    let mount_dir = tempfile::tempdir().map_err(|e| {
        ZkError::OperationFailed(format!("Failed to create temporary mount directory: {}", e))
    })?;
    let mount_point = mount_dir.path();

    let status = executor
        .run_interactive(
            "0k-core",
            &[
                "mount",
                archive_path
                    .to_str()
                    .ok_or(ZkError::InvalidPath(archive_path.to_path_buf()))?,
                mount_point
                    .to_str()
                    .ok_or(ZkError::InvalidPath(mount_point.to_path_buf()))?,
            ],
        )
        .map_err(|e| {
            ZkError::OperationFailed(format!("Failed to execute mount command: {}", e))
        })?;

    if !status.success() {
        return Err(ZkError::OperationFailed("Failed to mount archive".into()));
    }

    // Ensure unmount
    struct UnmountGuard<'a, E: CommandExecutor>(&'a E, &'a Path);
    impl<'a, E: CommandExecutor> Drop for UnmountGuard<'a, E> {
        fn drop(&mut self) {
            if let Some(s) = self.1.to_str() {
                let _ = self.0.run("0k-core", &["umount", s]);
            }
        }
    }
    let _guard = UnmountGuard(executor, mount_point);

    // 2. Read Manifest
    let manifest_path = mount_point.join("list.yaml");
    if !manifest_path.exists() {
        return Err(ZkError::OperationFailed(
            "Archive missing list.yaml - invalid format".into(),
        ));
    }
    let f = fs::File::open(&manifest_path).map_err(ZkError::IoError)?;
    let manifest: Manifest = serde_yaml::from_reader(f).map_err(ZkError::ManifestError)?;
    manifest.validate()?;

    // 3. Perform Check
    println!("Checking {} files from archive...", manifest.files.len());

    let mut stats_files_matched = 0;
    let mut stats_dirs_matched = 0;
    let mut stats_mismatch = 0;
    let mut stats_missing = 0;
    let mut stats_skipped = 0;
    let mut stats_files_deleted = 0;
    let mut stats_dirs_deleted = 0;
    let mut stats_links_matched = 0;
    let mut stats_links_deleted = 0;

    for entry in &manifest.files {
        // ... (Path resolution logic is same)
        let live_root = if let (Some(parent), Some(name)) = (&entry.restore_path, &entry.name) {
            PathBuf::from(parent).join(name)
        } else if let Some(orig) = &entry.original_path {
            PathBuf::from(orig)
        } else {
            println!("SKIPPED (Invalid Entry {}): Missing path info", entry.id);
            continue;
        };

        // Construct source path in mount
        let entry_name_in_mount = entry
            .name
            .as_deref()
            .or(live_root.file_name().and_then(|n| n.to_str()))
            .unwrap_or("unknown");

        let mount_root = mount_point
            .join("to_restore")
            .join(entry.id.to_string())
            .join(entry_name_in_mount);

        if fs::symlink_metadata(&mount_root).is_err() {
            println!(
                "ERROR: Archive corrupted, missing internal root for id {}",
                entry.id
            );
            continue;
        }

        if entry.entry_type == crate::manifest::EntryType::File
            || entry.entry_type == crate::manifest::EntryType::Symlink
        {
            // Check single item
            check_item(
                &live_root,
                &mount_root,
                options,
                &mut stats_files_matched,
                &mut stats_dirs_matched,
                &mut stats_links_matched,
                &mut stats_files_deleted,
                &mut stats_dirs_deleted,
                &mut stats_links_deleted,
                &mut stats_mismatch,
                &mut stats_missing,
                &mut stats_skipped,
            )?;
        } else {
            // Directory: Use Walker
            let walker = walkdir::WalkDir::new(&mount_root).contents_first(true);
            for item in walker {
                let item = match item {
                    Ok(i) => i,
                    Err(e) => {
                        println!("WALK ERROR: {}", e);
                        continue;
                    }
                };
                let mount_path = item.path();
                let rel_path = match mount_path.strip_prefix(&mount_root) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                let live_path = live_root.join(rel_path);

                check_item(
                    &live_path,
                    mount_path,
                    options,
                    &mut stats_files_matched,
                    &mut stats_dirs_matched,
                    &mut stats_links_matched,
                    &mut stats_files_deleted,
                    &mut stats_dirs_deleted,
                    &mut stats_links_deleted,
                    &mut stats_mismatch,
                    &mut stats_missing,
                    &mut stats_skipped,
                )?;
            }
        }
    }

    println!("---------------------------------------------------");
    println!("Indexed Paths: {}", manifest.files.len());
    println!(
        "Files Matched: {}, Dirs Matched: {}, Links Matched: {}",
        stats_files_matched, stats_dirs_matched, stats_links_matched
    );
    println!(
        "Files Deleted: {}, Dirs Deleted: {}, Links Deleted: {}",
        stats_files_deleted, stats_dirs_deleted, stats_links_deleted
    );
    println!(
        "Mismatched: {}, Missing: {}, Skipped (Newer): {}",
        stats_mismatch, stats_missing, stats_skipped
    );

    Ok(())
}

fn check_item(
    live_path: &Path,
    mount_path: &Path,
    options: &CheckOptions,
    stats_files_matched: &mut u32,
    stats_dirs_matched: &mut u32,
    stats_links_matched: &mut u32,
    stats_files_deleted: &mut u32,
    stats_dirs_deleted: &mut u32,
    stats_links_deleted: &mut u32,
    stats_mismatch: &mut u32,
    stats_missing: &mut u32,
    stats_skipped: &mut u32,
) -> Result<(), ZkError> {
    let display_name = live_path.display().to_string();

    // MISSING check
    let live_meta = match fs::symlink_metadata(live_path) {
        Ok(m) => m,
        Err(_) => {
            println!("MISSING: {}", display_name);
            *stats_missing += 1;
            return Ok(());
        }
    };

    let mount_meta = match fs::symlink_metadata(mount_path) {
        Ok(m) => m,
        Err(_) => return Ok(()), // Should not happen if walker is correct
    };

    // Check Type
    if live_meta.file_type().is_dir() != mount_meta.file_type().is_dir()
        || live_meta.file_type().is_file() != mount_meta.file_type().is_file()
        || live_meta.file_type().is_symlink() != mount_meta.file_type().is_symlink()
    {
        println!("MISMATCH (Type): {}", display_name);
        *stats_mismatch += 1;
        return Ok(());
    }

    if live_meta.is_dir() {
        if options.delete {
            if let Err(e) = fs::remove_dir(live_path) {
                if e.kind() == std::io::ErrorKind::DirectoryNotEmpty || e.raw_os_error() == Some(39)
                {
                    println!("MATCH (Dir): {}", display_name);
                    *stats_dirs_matched += 1;
                } else {
                    println!("ERROR: Failed to delete dir {}: {}", display_name, e);
                }
            } else {
                println!("DELETED (Dir): {}", display_name);
                *stats_dirs_deleted += 1;
            }
        } else {
            println!("MATCH (Dir): {}", display_name);
            *stats_dirs_matched += 1;
        }
        return Ok(());
    }

    if live_meta.is_symlink() {
        let live_target = fs::read_link(live_path);
        let mount_target = fs::read_link(mount_path);

        if live_target.is_err()
            || mount_target.is_err()
            || live_target.as_ref().unwrap() != mount_target.as_ref().unwrap()
        {
            println!(
                "MISMATCH (Link Target): {} ({:?} vs {:?})",
                display_name, live_target, mount_target
            );
            *stats_mismatch += 1;
            return Ok(());
        }
    } else {
        if live_meta.len() != mount_meta.len() {
            println!(
                "MISMATCH (Size): {} (Live: {}, Archive: {})",
                display_name,
                live_meta.len(),
                mount_meta.len()
            );
            *stats_mismatch += 1;
            return Ok(());
        }

        if options.use_cmp {
            let matches = compare_files(live_path, mount_path).unwrap_or(false);
            if !matches {
                println!("MISMATCH (Content): {}", display_name);
                *stats_mismatch += 1;
                return Ok(());
            }
        }
    }

    // Match found
    if options.delete {
        let live_mtime = live_meta
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let archive_mtime = mount_meta
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Safety Gate: Do not delete if Live file is NEWER than Archive
        // Exception: If use_cmp is enabled, we verified content is identical.
        // So even if mtime is newer (e.g. touched), data is safe to delete (it is backed up).
        if !options.use_cmp {
            if live_mtime > archive_mtime {
                println!("SKIPPED (Newer): {} (Live mtime > Archive)", display_name);
                *stats_skipped += 1;
                return Ok(());
            }
        }

        if let Err(e) = fs::remove_file(live_path) {
            println!("ERROR: Failed to delete {}: {}", display_name, e);
        } else {
            println!("DELETED: {}", display_name);
            if live_meta.is_symlink() {
                *stats_links_deleted += 1;
            } else {
                *stats_files_deleted += 1;
            }
        }
    } else {
        println!("MATCH: {}", display_name);
        if live_meta.is_symlink() {
            *stats_links_matched += 1;
        } else {
            *stats_files_matched += 1;
        }
    }

    Ok(())
}

fn compare_files(p1: &Path, p2: &Path) -> Result<bool, ZkError> {
    use std::io::Read;
    let f1 = fs::File::open(p1).map_err(ZkError::IoError)?;
    let f2 = fs::File::open(p2).map_err(ZkError::IoError)?;

    // Use BufReader logic
    let mut b1 = std::io::BufReader::new(f1);
    let mut b2 = std::io::BufReader::new(f2);

    let mut buf1 = [0; 8192];
    let mut buf2 = [0; 8192];

    loop {
        let n1 = b1.read(&mut buf1)?;
        let n2 = b2.read(&mut buf2)?;

        if n1 != n2 {
            return Ok(false);
        }
        if n1 == 0 {
            return Ok(true);
        } // EOF reached for both

        if buf1[..n1] != buf2[..n2] {
            return Ok(false);
        }
    }
}

pub fn unfreeze<E: CommandExecutor>(
    archive_path: &Path,
    options: &UnfreezeOptions,
    executor: &E,
) -> Result<(), ZkError> {
    // 0. Check for LUKS (requires Root to mount)
    // If it is LUKS and we are not root, fail early to trigger elevation retry in 0k
    if utils::is_luks_image(archive_path, executor) {
        if !utils::is_root().unwrap_or(false) {
             return Err(ZkError::OperationFailed("Permission denied: Unfreezing LUKS archive requires root privileges.".to_string()));
        }
    }

    // 1. Create temporary mount point
    let mount_dir = tempfile::tempdir().map_err(|e| {
        ZkError::OperationFailed(format!("Failed to create temporary mount directory: {}", e))
    })?;
    let mount_point = mount_dir.path();

    // 2. Mount Archive
    let status = executor
        .run_interactive(
            "0k-core",
            &[
                "mount",
                archive_path
                    .to_str()
                    .ok_or(ZkError::InvalidPath(archive_path.to_path_buf()))?,
                mount_point
                    .to_str()
                    .ok_or(ZkError::InvalidPath(mount_point.to_path_buf()))?,
            ],
        )
        .map_err(|e| {
            ZkError::OperationFailed(format!("Failed to execute mount command: {}", e))
        })?;

    if !status.success() {
        return Err(ZkError::OperationFailed("Failed to mount archive".into()));
    }

    // Ensure we unmount even if errors occur later
    struct UnmountGuard<'a, E: CommandExecutor>(&'a E, &'a Path);
    impl<'a, E: CommandExecutor> Drop for UnmountGuard<'a, E> {
        fn drop(&mut self) {
            if let Some(s) = self.1.to_str() {
                let _ = self.0.run("0k-core", &["umount", s]);
            }
        }
    }
    let _guard = UnmountGuard(executor, mount_point);

    restore_from_mount(mount_point, options, executor)
}

/// SECURITY: Verify that none of the existing ancestor components of `path`
/// are symlinks. This prevents symlink-based redirect attacks during restore
/// (e.g. attacker creates /home/user/docs -> /etc, then restore overwrites
/// /etc/passwd). Only existing path components are checked — if a parent
/// directory does not exist yet (we create it ourselves), it cannot be a symlink.
fn validate_no_symlinks_in_ancestors(path: &Path) -> Result<(), ZkError> {
    let mut checked = PathBuf::new();
    for component in path.components() {
        checked.push(component);
        match fs::symlink_metadata(&checked) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(ZkError::OperationFailed(format!(
                        "Security: restore path component {:?} is a symlink. \
                         This could redirect writes to unintended locations. Aborting.",
                        checked
                    )));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Component doesn't exist yet — will be created by us, safe to proceed
                break;
            }
            Err(e) => {
                return Err(ZkError::IoError(e));
            }
        }
    }
    Ok(())
}

fn restore_from_mount<E: CommandExecutor>(
    mount_point: &Path,
    options: &UnfreezeOptions,
    executor: &E,
) -> Result<(), ZkError> {
    // 3. Read Manifest
    let manifest_path = mount_point.join("list.yaml");
    if !manifest_path.exists() {
        return Err(ZkError::OperationFailed(
            "Archive missing list.yaml - invalid format".into(),
        ));
    }

    let f = fs::File::open(&manifest_path)?;
    let manifest: Manifest = serde_yaml::from_reader(f)?;

    // 4. Validate manifest (paths)
    manifest.validate()?;

    println!("Restoring {} files from archive...", manifest.files.len());

    // 5. Restore Loop
    for entry in &manifest.files {
        // Determine destination path (handle Legacy vs New format)
        let (dest_path, restore_parent) =
            if let (Some(parent), Some(name)) = (&entry.restore_path, &entry.name) {
                let p = PathBuf::from(parent);
                (p.join(name), p)
            } else if let Some(orig) = &entry.original_path {
                let p = PathBuf::from(orig);
                let parent = p.parent().unwrap_or(Path::new("/")).to_path_buf();
                (p, parent)
            } else {
                return Err(ZkError::OperationFailed(format!(
                    "Invalid entry {}: missing path info",
                    entry.id
                )));
            };

        // Derive name if missing (Legacy)
        let entry_name = entry
            .name
            .as_deref()
            .or(dest_path.file_name().and_then(|n| n.to_str()))
            .unwrap_or("unknown");

        // Construct source path in mount
        // Structure: mount_point/to_restore/<id>/<name>
        let src_path = mount_point
            .join("to_restore")
            .join(entry.id.to_string())
            .join(entry_name);

        println!("Restoring: {:?} -> {:?}", entry_name, dest_path);

        // SECURITY: verify no symlinks in the restore destination path.
        // Prevents attacker from creating e.g. /home/user/docs -> /etc
        // to redirect restore writes to system directories.
        validate_no_symlinks_in_ancestors(&dest_path)?;

        // Conflict Check
        let mut extra_rsync_flags = Vec::new();

        if dest_path.exists() {
            if options.skip_existing {
                if dest_path.is_dir() {
                    println!(
                        "Merging into existing directory (skipping conflicts): {:?}",
                        dest_path
                    );
                    extra_rsync_flags.push("--ignore-existing");
                } else {
                    println!("Skipping existing file: {:?}", dest_path);
                    continue;
                }
            } else if !options.overwrite {
                return Err(ZkError::OperationFailed(format!(
                    "File exists: {:?}. Use --overwrite to replace/merge.",
                    dest_path
                )));
            }
        }

        // Ensure parent directory exists
        if !restore_parent.exists() {
            if let Err(_) = fs::create_dir_all(&restore_parent) {
                // Fallback to sudo mkdir -p
                if let Some(runner) =
                    utils::check_root_or_get_runner("Parent directory creation requires root")?
                {
                    let status = executor.run_interactive(
                        &runner,
                        &[
                            "mkdir",
                            "-p",
                            restore_parent
                                .to_str()
                                .ok_or(ZkError::InvalidPath(restore_parent.clone()))?,
                        ],
                    )?;
                    if !status.success() {
                        return Err(ZkError::OperationFailed(format!(
                            "Failed to create directory {:?}",
                            restore_parent
                        )));
                    }
                } else {
                    return Err(ZkError::OperationFailed(format!(
                        "Failed to create directory {:?}",
                        restore_parent
                    )));
                }
            }
        }

        let src_str = src_path
            .to_str()
            .ok_or(ZkError::InvalidPath(src_path.clone()))?;
        let dest_str = dest_path
            .to_str()
            .ok_or(ZkError::InvalidPath(dest_path.clone()))?;

        println!(
            "Restoring {} -> {}",
            src_path.display(),
            dest_path.display()
        );

        let mut final_src = src_str.to_string();
        if entry.entry_type == crate::manifest::EntryType::Directory {
            final_src.push('/');
        }

        // Use user rsync by default
        let mut args = vec!["-a", "--info=progress2", &final_src, dest_str];
        // Insert flags before source/dest
        for flag in &extra_rsync_flags {
            args.insert(2, flag);
        }

        let rsync_status = executor.run_interactive("rsync", &args);

        let need_sudo = if let Ok(s) = rsync_status {
            !s.success()
        } else {
            true
        };

        if need_sudo
            || (manifest.metadata.privilege_mode == Some(PrivilegeMode::Root)
                && !utils::is_root().unwrap_or(false))
        {
            if let Some(runner) =
                utils::check_root_or_get_runner("Restoration requires elevated privileges")?
            {
                println!("Retrying with {}", runner);

                let mut sudo_args = vec!["rsync", "-a", "--info=progress2", &final_src, dest_str];
                for flag in &extra_rsync_flags {
                    sudo_args.insert(2, flag);
                }

                let status = executor.run_interactive(runner.as_str(), &sudo_args)?;
                if !status.success() {
                    return Err(ZkError::OperationFailed(format!(
                        "Failed to restore {:?}: rsync failed even with sudo",
                        dest_path
                    )));
                }
            } else {
                // Already tried as root or no runner, and failed
                return Err(ZkError::OperationFailed(format!(
                    "Failed to restore {:?}",
                    dest_path
                )));
            }
        }
    }

    Ok(())
}

pub fn freeze<E: CommandExecutor>(
    targets: &[PathBuf],
    options: &FreezeOptions,
    executor: &E,
) -> Result<(), ZkError> {
    // 0. Ensure we can read targets (triggers escalation if needed)
    utils::ensure_read_permissions(targets)?;

    // 0. Auto-GC: Cleanup stale build directories (protected by flock)
    if let Err(e) = try_gc_staging() {
        warn!("GC Error: {}", e);
    }

    // 1. Prepare Staging
    // _lock must be kept in scope to maintain the flock until we are done (or until cleanup)
    let (build_dir, payload_name, _lock) = prepare_staging(targets, options.dereference)?;

    // 2. Read Manifest
    let payload_dir = build_dir.join(&payload_name);
    let manifest_path = payload_dir.join("list.yaml");
    let f = fs::File::open(&manifest_path).map_err(ZkError::IoError)?;
    let manifest: Manifest = serde_yaml::from_reader(f).map_err(ZkError::ManifestError)?;

    // 3. Generate internal script
    let script = generate_freeze_script(&manifest, &build_dir, &payload_name, options)?;
    let script_path = build_dir.join("freeze.sh");
    fs::write(&script_path, &script)?;

    // 4. Run unshare
    // ADAPTIVE STRATEGY:
    // - Encrypted (-e): We MUST be real root. We use only Mount Namespace (-m). (User NS breaks LUKS)
    // - Plain: We can be User or Root.
    //   - If Root: Use only Mount Namespace (-m).
    //   - If User: Use User+Mount Namespace (-m -U -r).

    let mut unshare_args = Vec::new();

    if options.encrypt {
        // Enforce Root
        if !utils::is_root().unwrap_or(false) {
            return Err(ZkError::OperationFailed(
                "Encrypted freeze (-e) must be run as root (for LUKS). Please run with sudo."
                    .to_string(),
            ));
        }
        // Root + Encrypt -> Mount NS only
        unshare_args.extend_from_slice(&["-m", "--propagation", "private"]);
    } else {
        // Plain mode
        if utils::is_root().unwrap_or(false) {
            // Root + Plain -> Mount NS only (simpler, cleaner)
            unshare_args.extend_from_slice(&["-m", "--propagation", "private"]);
        } else {
            // User + Plain -> User + Mount NS (Rootless)
            unshare_args.extend_from_slice(&["-m", "-U", "-r", "--propagation", "private"]);
        }
    }

    // Append command
    unshare_args.push("sh");
    unshare_args.push(
        script_path
            .to_str()
            .ok_or(ZkError::InvalidPath(script_path.clone()))?,
    );

    // Use run_and_capture_error to get stderr for friendly messages
    let (status, stderr) = executor
        .run_and_capture_error("unshare", &unshare_args)
        .map_err(|e| ZkError::OperationFailed(format!("Failed to execute unshare: {}", e)))?;

    if !status.success() {
        return Err(ZkError::OperationFailed(format!(
            "Freeze process failed: {}",
            stderr
        )));
    }

    // Cleanup Staging Area
    if let Err(e) = std::fs::remove_dir_all(&build_dir) {
        warn!(
            "Failed to clean up staging directory {:?}: {}",
            build_dir, e
        );
    }

    Ok(())
}

/// Escape a string for safe use inside single quotes in POSIX shell.
/// Single quotes prevent ALL interpretation ($, `, \, etc.).
/// The only character that needs escaping is `'` itself: `'` -> `'\''`
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn generate_freeze_script(
    manifest: &Manifest,
    build_dir: &Path,
    payload_name: &str,
    options: &FreezeOptions,
) -> Result<String, ZkError> {
    let mut script = String::new();
    script.push_str("#!/bin/sh\n");
    script.push_str("set -e\n"); // Exit on error

    // Bind mounts
    for entry in &manifest.files {
        if entry.entry_type == crate::manifest::EntryType::Symlink {
            continue; // Already staged as symlink, no bind mount needed
        }
        if let (Some(parent), Some(name)) = (&entry.restore_path, &entry.name) {
            let src = Path::new(parent).join(name);
            let dest = build_dir
                .join(payload_name)
                .join("to_restore")
                .join(entry.id.to_string())
                .join(name);

            let src_quoted = shell_quote(&src.display().to_string());
            let dest_quoted = shell_quote(&dest.display().to_string());
            script.push_str(&format!("mount --bind {} {}\n", src_quoted, dest_quoted));
        }
    }

    let encrypt_flag = if options.encrypt { "--encrypt" } else { "" };

    let mut flags = String::new();
    if options.overwrite_files {
        flags.push_str(" --overwrite-files");
    }
    if options.overwrite_luks_content {
        flags.push_str(" --overwrite-luks-content");
    }
    if let Some(level) = options.compression {
        flags.push_str(&format!(" --compression {}", level));
    }

    // IMPORTANT: Point squash_manager to the PAYLOAD directory, not the build root
    let input_dir = build_dir.join(payload_name);

    let progress_flag = match options.progress_mode {
        ProgressMode::None => "--no-progress",
        ProgressMode::Vanilla => "--vanilla-progress",
        ProgressMode::Alfa => "--alfa-progress",
    };

    // 5. Generate command to run 0k-core
    // IMPORTANT: Point 0k-core to the PAYLOAD directory, not the build root
    // because build root contains freeze.sh itself which we don't want in the archive.
    let create_flags = encrypt_flag; // This is the --encrypt flag
    let tar_flags = flags; // This contains --overwrite-files, --overwrite-luks-content, --compression
    let exclusions = ""; // No exclusions for now
    let payload_dir_quoted = shell_quote(&input_dir.display().to_string()); // INPUT: the payload directory with bind mounts
    let dest_quoted = shell_quote(&options.output.display().to_string()); // OUTPUT: standard destination

    script.push_str(&format!(
        "0k-core create {} {} {} {} {} {}\n",
        create_flags,
        progress_flag,
        tar_flags,
        exclusions,
        payload_dir_quoted,
        dest_quoted
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

        // Since we are adding ONLY this test to this file, it's fine.
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", temp_cache.path());
        }

        let target_dir = tempdir().unwrap();
        let file_target = target_dir.path().join("data.txt");
        let dir_target = target_dir.path().join("config");
        fs::write(&file_target, "content").unwrap();
        fs::create_dir(&dir_target).unwrap();

        let targets = vec![file_target.clone(), dir_target.clone()];

        // Return tuple now includes payload_name
        let (build_dir, payload_name, _lock) = prepare_staging(&targets, false).unwrap();

        assert_eq!(payload_name, "payload"); // Always "payload"

        assert!(build_dir.exists());
        let payload_dir = build_dir.join(&payload_name);
        assert!(payload_dir.exists());
        assert!(payload_dir.join("list.yaml").exists());
        assert!(payload_dir.join("to_restore").exists());

        // Check stubs
        // ID 1: data.txt (file)
        assert!(payload_dir.join("to_restore/1/data.txt").exists());
        assert!(
            payload_dir
                .join("to_restore/1/data.txt")
                .metadata()
                .unwrap()
                .is_file()
        );
        assert_eq!(
            payload_dir
                .join("to_restore/1/data.txt")
                .metadata()
                .unwrap()
                .len(),
            0
        ); // Stub is empty

        // ID 2: config (dir)
        assert!(payload_dir.join("to_restore/2/config").exists());
        assert!(
            payload_dir
                .join("to_restore/2/config")
                .metadata()
                .unwrap()
                .is_dir()
        );

        // Check manifest content validation
        let manifest_content = fs::read_to_string(payload_dir.join("list.yaml")).unwrap();
        assert!(manifest_content.contains("data.txt"));
        assert!(manifest_content.contains("config"));
    }

    #[test]
    fn test_prepare_staging_symlinks() {
        use std::os::unix::fs::symlink;
        // Mock XDG_CACHE_HOME by setting environment variable
        let temp_cache = tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_CACHE_HOME", temp_cache.path());
        }

        let target_dir = tempdir().unwrap();
        let target_file = target_dir.path().join("real_file");
        fs::write(&target_file, "real content").unwrap();

        let symlink_path = target_dir.path().join("my_link");
        symlink(&target_file, &symlink_path).unwrap();

        // Test 1: No Dereference (default) -> Should preserve symlink
        let targets = vec![symlink_path.clone()];
        let (build_dir, payload_name, _lock) = prepare_staging(&targets, false).unwrap();

        let payload_dir = build_dir.join(&payload_name);
        let link_in_staging = payload_dir.join("to_restore/1/my_link");

        assert!(link_in_staging.exists() || link_in_staging.is_symlink()); // is_symlink is sufficient check
        assert!(fs::symlink_metadata(&link_in_staging).unwrap().is_symlink());

        let target = fs::read_link(&link_in_staging).unwrap();
        assert_eq!(target, target_file); // Should point to original target absolute path

        // Clean up lock to allow GC (not critical for test but good practice)
        drop(_lock);

        // Test 2: Dereference -> Should be a file stub
        let (build_dir_2, payload_name_2, _lock_2) = prepare_staging(&targets, true).unwrap();
        let payload_dir_2 = build_dir_2.join(&payload_name_2);
        let stub_in_staging = payload_dir_2.join("to_restore/1/my_link");

        assert!(stub_in_staging.exists());
        assert!(fs::metadata(&stub_in_staging).unwrap().is_file()); // It's a file stub now
        assert!(!fs::symlink_metadata(&stub_in_staging).unwrap().is_symlink());
    }

    #[test]
    fn test_generate_freeze_script() {
        let temp = tempfile::tempdir().unwrap();
        let build_dir = temp.path().join("build");
        let output = temp.path().join("out.sqfs");

        let manifest = Manifest {
            metadata: Metadata::new("test-host".into(), PrivilegeMode::User),
            files: vec![FileEntry {
                id: 1,
                entry_type: crate::manifest::EntryType::File,
                name: Some("file1".into()),
                restore_path: Some("/src/dir1".into()),
                original_path: None,
            }],
        };

        let options = FreezeOptions {
            encrypt: false,
            output: output.clone(),
            overwrite_files: false,
            overwrite_luks_content: false,
            progress_mode: ProgressMode::None,
            compression: None,
            dereference: false,
        };

        let payload_name = "test_payload";
        let script = generate_freeze_script(&manifest, &build_dir, payload_name, &options).unwrap();

        assert!(script.contains("mount --bind '/src/dir1/file1'"));
        assert!(script.contains("to_restore/1/file1'"));
        assert!(script.contains("0k-core create"));
        assert!(script.contains("build/test_payload'"));
        assert!(script.contains("--no-progress"));
    }

    #[test]
    fn test_shell_quote() {
        // Normal string
        assert_eq!(shell_quote("hello"), "'hello'");
        // String with single quote
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        // Dangerous characters that double-quotes would NOT protect from
        assert_eq!(shell_quote("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(shell_quote("`malicious`"), "'`malicious`'");
        assert_eq!(shell_quote("path with $VAR"), "'path with $VAR'");
        assert_eq!(shell_quote("back\\slash"), "'back\\slash'");
    }

    #[test]
    fn test_generate_freeze_script_injection_safe() {
        let temp = tempfile::tempdir().unwrap();
        let build_dir = temp.path().join("build");

        let manifest = Manifest {
            metadata: Metadata::new("test-host".into(), PrivilegeMode::User),
            files: vec![FileEntry {
                id: 1,
                entry_type: crate::manifest::EntryType::Directory,
                name: Some("$(whoami)".into()),
                restore_path: Some("/tmp/`id`".into()),
                original_path: None,
            }],
        };

        let options = FreezeOptions {
            encrypt: false,
            output: PathBuf::from("/tmp/out $HOME.sqfs"),
            overwrite_files: false,
            overwrite_luks_content: false,
            progress_mode: ProgressMode::None,
            compression: None,
            dereference: false,
        };

        let script = generate_freeze_script(&manifest, &build_dir, "payload", &options).unwrap();
        // All dangerous chars must be inside single quotes (neutralized)
        assert!(script.contains("'/tmp/`id`/$(whoami)'"));
        assert!(script.contains("'/tmp/out $HOME.sqfs'"));
        // Must NOT contain unquoted dangerous patterns
        assert!(!script.contains("\"$("));
        assert!(!script.contains("\"`"));
    }

    #[test]
    fn test_freeze_execution_flow() {
        // Can't run full freeze because prepare_staging needs real paths.
        // But we can check if it compiles and structure is correct.
        // We verified generate_freeze_script above.
        // Mocking execution is complex because prepare_staging does FS ops.
        // We'll trust logic + integration tests for full flow.
    }

    #[test]
    fn test_restore_from_mount() {
        use crate::executor::MockCommandExecutor;
        use std::os::unix::process::ExitStatusExt;

        let mount = tempfile::tempdir().unwrap();
        let mount_path = mount.path();

        // 1. Create file structure in mount
        let restore_subdir = mount_path.join("to_restore").join("1");
        fs::create_dir_all(&restore_subdir).unwrap();
        fs::write(restore_subdir.join("myfile.txt"), "content").unwrap();

        // 2. Create destination
        let dest = tempfile::tempdir().unwrap();
        let dest_path_str = dest.path().to_str().unwrap().to_string();

        // 3. Create manifest
        let manifest = Manifest {
            metadata: Metadata::new("host".into(), PrivilegeMode::User),
            files: vec![FileEntry {
                id: 1,
                entry_type: crate::manifest::EntryType::File,
                name: Some("myfile.txt".into()),
                restore_path: Some(dest_path_str.clone()),
                original_path: None,
            }],
        };
        let f = fs::File::create(mount_path.join("list.yaml")).unwrap();
        serde_yaml::to_writer(f, &manifest).unwrap();

        // 4. Mock Executor
        let mut mock = MockCommandExecutor::new();

        let src_check = restore_subdir
            .join("myfile.txt")
            .to_str()
            .unwrap()
            .to_string();
        let dest_check = dest.path().join("myfile.txt").to_str().unwrap().to_string();

        mock.expect_run_interactive()
            .withf(move |program, args| {
                program == "rsync" &&
                 args.contains(&"-a") &&
                 args.contains(&src_check.as_str()) && // Check source
                 args.contains(&dest_check.as_str()) // Check dest
            })
            .times(1)
            .returning(|_, _| Ok(std::process::ExitStatus::from_raw(0)));

        let options = UnfreezeOptions {
            overwrite: false,
            skip_existing: false,
        };

        restore_from_mount(mount_path, &options, &mock).unwrap();
    }

    #[test]
    fn test_restore_from_mount_legacy() {
        use crate::executor::MockCommandExecutor;
        use std::os::unix::process::ExitStatusExt;

        let mount = tempfile::tempdir().unwrap();
        let mount_path = mount.path();

        // 1. Create file structure in mount (using derived name from original path)
        let restore_subdir = mount_path.join("to_restore").join("1");
        fs::create_dir_all(&restore_subdir).unwrap();
        fs::write(restore_subdir.join("legacy.txt"), "legacy content").unwrap();

        // 2. Create destination
        let dest = tempfile::tempdir().unwrap();
        let dest_path_str = dest.path().join("legacy.txt").to_str().unwrap().to_string();

        // 3. Create Legacy Manifest (no name, no restore_path, only original_path)
        let manifest = Manifest {
            metadata: Metadata::new("host".into(), PrivilegeMode::User),
            files: vec![FileEntry {
                id: 1,
                entry_type: crate::manifest::EntryType::File,
                name: None,         // Missing in legacy
                restore_path: None, // Missing in legacy
                original_path: Some(dest_path_str.clone()),
            }],
        };
        let f = fs::File::create(mount_path.join("list.yaml")).unwrap();
        serde_yaml::to_writer(f, &manifest).unwrap();

        // 4. Mock Executor
        let mut mock = MockCommandExecutor::new();

        let src_check = restore_subdir
            .join("legacy.txt")
            .to_str()
            .unwrap()
            .to_string(); // Name derived from filename
        let dest_check = dest_path_str.clone();

        mock.expect_run_interactive()
            .withf(move |program, args| {
                program == "rsync"
                    && args.contains(&src_check.as_str())
                    && args.contains(&dest_check.as_str())
            })
            .times(1)
            .returning(|_, _| Ok(std::process::ExitStatus::from_raw(0)));

        let options = UnfreezeOptions {
            overwrite: false,
            skip_existing: false,
        };

        restore_from_mount(mount_path, &options, &mock).unwrap();
    }
}
