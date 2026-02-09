# Zero-Kelvin Security & Quality Audit v0.2

**Date:** 2026-02-09
**Version audited:** 0.2.16 (branch `dev`, commit `8a6a97e`)
**Auditor:** Claude Opus 4.6
**Scope:** Full source audit of `src/`, `tests/`, binaries `0k`, `0k-core`, `0k-safe-rm`

---

## Executive Summary

17 issues were identified in the initial audit, and 8 additional issues (R1-R8) were found during the post-fix re-audit. All have been resolved:
- **16 initial + 8 re-audit = 24 fixed**
- **1 informational / deferred** (issues 13, 15)

All 71 unit tests pass. Zero `.unwrap()`/`.expect()` in production code. Zero `process::exit()` calls.

---

## Issue Index

| # | Severity | Status | Title |
|---|----------|--------|-------|
| 1 | Critical | **Fixed** | `compare_files` false mismatches on FUSE mounts |
| 2 | Critical | **Fixed** | `.unwrap()` / `.expect()` in production code |
| 3 | Critical | **Fixed** | Non-UTF8 filename silent corruption via `to_string_lossy` |
| 4 | Medium | **Fixed** | No validation of zstd compression level |
| 5 | Medium | **Fixed** | TOCTOU race in `generate_mapper_name` |
| 6 | Medium | **Fixed** | GC skips directories without `.lock` file |
| 7 | Medium | **Fixed** | `process::exit()` bypasses destructors |
| 8 | Medium | **Fixed** | Silent sudo fallback without warning |
| 9 | Medium | **Fixed** | rsync: blind sudo retry on any failure |
| 10 | Medium | **Fixed** | `name.contains("..")` false positives in validation |
| 11 | Low | **Fixed** | Test data races with `set_var` |
| 12 | Low | **Fixed** | No post-freeze verification of output |
| 13 | Low | Deferred | No `--delete` confirmation prompt |
| 14 | Low | **Fixed** | `0k-safe-rm` doesn't check for bind mounts |
| 15 | Low | Info | `freeze.sh` left on disk in staging area |
| 16 | Low | **Fixed** | No hostname mismatch warning |
| 17 | Low | **Fixed** | Duplicate mock implementation in `0k-core.rs` |

---

## Fixed Issues

### Issue 1 (Critical): `compare_files` false mismatches on FUSE mounts

**File:** `src/engine.rs:552-594`

**Problem:** The original `compare_files` used a single `read()` call per iteration. FUSE filesystems (e.g., `squashfuse`) may return fewer bytes than requested, even when more data is available. This caused false content mismatches during `check --use-cmp`, because the local file's `read()` would return 8192 bytes while the FUSE mount's `read()` returned fewer (e.g., 4096), making identical files appear different.

**Fix:** Introduced a `read_full()` helper that loops until the buffer is completely filled or EOF is reached, handling `EINTR` gracefully:

```rust
fn read_full(reader: &mut impl std::io::Read, buf: &mut [u8]) -> Result<usize, std::io::Error> {
    let mut total = 0;
    while total < buf.len() {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}
```

`compare_files` now calls `read_full` instead of raw `read()`, ensuring both buffers are filled to the same degree before comparison.

**Tests added:** `test_compare_files_identical`, `test_compare_files_different_size`, `test_compare_files_different_content`, `test_compare_files_empty`, `test_read_full_fills_buffer`

---

### Issue 2 (Critical): `.unwrap()` / `.expect()` in production code

**Files:** `src/engine.rs`, `src/bin/0k-core.rs`

**Problem:** Multiple `.unwrap()` and `.expect()` calls in non-test code violate the project's safety policy. Any of these could cause a panic and crash in production.

**Fixes applied:**

| Location | Before | After |
|----------|--------|-------|
| `engine.rs:76` | `entry.name.as_ref().unwrap()` | `.ok_or_else(\|\| ZkError::StagingError(...))? ` |
| `engine.rs:471` | `read_link().unwrap()` with `is_err()` | `match (&live_target, &mount_target)` pattern |
| `0k-core.rs` | `.expect("Error setting Ctrl+C handler")` | `.map_err(\|e\| ZkError::OperationFailed(...))?` |
| `0k-core.rs` | `SystemTime...unwrap()` | `.unwrap_or_default()` |
| `0k-core.rs` (x3) | `ProgressStyle...unwrap()` | `.map_err(\|e\| ZkError::OperationFailed(...))?` |

**Note:** Remaining `.unwrap_or(...)` and `.unwrap_or_default()` calls are safe (they provide fallback values, not panics).

---

### Issue 3 (Critical): Non-UTF8 filename silent corruption via `to_string_lossy`

**File:** `src/manifest.rs:39-87`

**Problem:** `FileEntry::from_path()` used `to_string_lossy().into_owned()` when extracting `name` and `restore_path` from paths. This silently replaces non-UTF8 bytes with the Unicode replacement character `U+FFFD` (`\ufffd`), corrupting filenames. The archive would contain a different name than the original file, making restoration impossible.

**Fix:** Replaced `to_string_lossy()` with explicit `to_str().ok_or_else(...)` that returns a clear error:

```rust
let name = abs_path.file_name()
    .ok_or_else(|| ZkError::InvalidPath(path.to_path_buf()))?
    .to_str()
    .ok_or_else(|| ZkError::OperationFailed(format!(
        "Path contains non-UTF8 characters: {:?}. Non-UTF8 filenames are not supported.",
        path
    )))?
    .to_string();
```

Same pattern applied to `restore_path`. Users now get a clear error message instead of silent data corruption.

---

### Issue 4 (Medium): No validation of zstd compression level

**Files:** `src/bin/0k.rs`, `src/bin/0k-core.rs`

**Problem:** `mksquashfs` with zstd supports compression levels 0-22. Values > 22 would be passed through, causing `mksquashfs` to fail with a cryptic error or fall back to a default.

**Fix:** Added validation in both binaries before any processing begins:

```rust
if compression > 22 {
    return Err(ZkError::CompressionError(format!(
        "Invalid compression level: {}. Zstd supports levels 0-22 (0 = no compression).",
        compression
    )));
}
```

---

### Issue 8 (Medium): Silent sudo fallback without warning

**File:** `src/bin/0k-core.rs`

**Problem:** When no privilege escalation tool is found, `0k-core` silently falls back to `"sudo"` as a hardcoded default. If `sudo` is also absent, the user gets a confusing error from the OS rather than from the application.

**Fix:** Added a warning message before the fallback:

```rust
eprintln!(
    "Warning: No privilege escalation tool (sudo, doas, run0, pkexec, please) found in PATH.\n\
     Falling back to 'sudo', which will likely fail."
);
```

---

### Issue 9 (Medium): rsync blind sudo retry on any failure

**File:** `src/engine.rs:822-878`

**Problem:** The `unfreeze` function retried every failed `rsync` call with `sudo`, regardless of the failure reason. A typo in a path or a network error would trigger an unnecessary sudo prompt, confusing the user.

**Fix:** Now checks rsync's exit code to determine if the failure is permission-related before retrying:

```rust
let rsync_exit_code = rsync_status.as_ref().ok().and_then(|s| s.code());
let is_likely_permission_error = matches!(rsync_exit_code, Some(23) | Some(11));

if privilege_mode_requires_root || is_likely_permission_error {
    // Retry with sudo
} else {
    // Report failure directly — no sudo retry
    return Err(ZkError::OperationFailed(format!(
        "rsync failed (exit code: {:?}) while restoring {:?}",
        rsync_exit_code, dest_path
    )));
}
```

Exit code 23 = partial transfer (permission errors), 11 = file I/O error.

---

### Issue 10 (Medium): `name.contains("..")` false positives in validation

**File:** `src/manifest.rs:89-111`

**Problem:** The old validation `name.contains("..")` would reject legitimate filenames like `backup..2024.tar` or `config...bak`. The check was overly broad.

**Fix:** Changed to exact match for path traversal patterns:

```rust
if name == ".." || name == "." || name.contains('/') || name.contains('\0') {
    return Err(ZkError::ManifestError(serde_yaml::Error::custom(format!(
        "Invalid name: '{}'. Names cannot be '.', '..', or contain '/' or null bytes.", name
    ))));
}
```

- `name == ".."` and `name == "."` catch exact path traversal.
- `name.contains('/')` catches embedded path separators.
- `name.contains('\0')` catches null byte injection.
- Filenames like `backup..2024.tar` now pass validation correctly.

**Tests added:** `backup..2024.tar` (valid), `..` (rejected), `.` (rejected)

---

### Issue 12 (Low): No post-freeze verification of output

**File:** `src/engine.rs:962-1007`

**Problem:** After the `unshare` + `mksquashfs` pipeline completed, there was no verification that the output file was actually created and valid. A silent failure (e.g., out of disk space) could produce an empty or missing file.

**Fix:** Added three-level verification after `unshare` succeeds:

1. **Existence check:** `options.output.exists()`
2. **Size check:** File size > 0
3. **Format check:**
   - Plain archives: `unsquashfs -s` validates SquashFS superblock
   - Encrypted archives: `is_luks_image()` validates LUKS header

```rust
if !options.encrypt {
    match executor.run("unsquashfs", &["-s", output_str]) {
        Ok(verify_out) if verify_out.status.success() => {
            info!("Post-freeze verification: archive is a valid SquashFS image");
        }
        Ok(verify_out) => {
            return Err(ZkError::OperationFailed(format!(
                "Post-freeze verification failed: output is not a valid SquashFS image: {}",
                verify_err.trim()
            )));
        }
        Err(e) => {
            warn!("Post-freeze verification skipped (unsquashfs not available?): {}", e);
        }
    }
} else {
    if !utils::is_luks_image(&options.output, executor) {
        return Err(ZkError::OperationFailed(
            "Post-freeze verification failed: output is not a valid LUKS container".to_string(),
        ));
    }
}
```

---

### Issue 16 (Low): No hostname mismatch warning

**File:** `src/engine.rs:271-280`

**Problem:** When running `check` on an archive created on a different host, the restore paths might not exist. The user received confusing "MISSING" outputs without understanding why.

**Fix:** Added a non-blocking warning after manifest validation in `check()`:

```rust
if let Ok(current_host) = get_hostname() {
    if manifest.metadata.host != current_host {
        eprintln!(
            "Warning: This archive was created on host '{}', but current host is '{}'.\n\
             Restore paths may not exist or may differ on this system.",
            manifest.metadata.host, current_host
        );
    }
}
```

No interactive prompt. Warning only, continues execution.

---

### Issue 5 (Medium): TOCTOU race in `generate_mapper_name`

**File:** `src/bin/0k-core.rs`

**Problem:** `generate_mapper_name()` checked if `/dev/mapper/sq_<name>` exists with `.exists()`, and if so, appended `_N` to find a free name. Between the check and the `cryptsetup open` call, another process could claim the same name.

**Fix:** Separated the function into two parts:

1. `generate_mapper_name()` — now a pure sanitizer, generates the base name without any `.exists()` check
2. `open_luks_container()` — new function that tries `cryptsetup open` and retries atomically on exit code 5 (name collision)

The retry sequence tries: base name, `_2`, `_3`, ..., `_10`, then a timestamp+random fallback.

```rust
fn open_luks_container(
    executor: &impl CommandExecutor,
    root_cmd: &[String],
    image_path_str: &str,
    base_mapper_name: &str,
) -> Result<String, ZkError> {
    for (i, mapper_name) in candidates.iter().enumerate() {
        // ... build cryptsetup open command ...
        if status.success() { return Ok(mapper_name.clone()); }
        if status.code() == Some(5) { continue; } // name taken, retry
        return Err(...); // real error — stop
    }
}
```

Both call sites (create and mount) now use `open_luks_container()`.

**Tests added:** `test_generate_mapper_name_sanitization`, `test_open_luks_container_success_first_try`, `test_open_luks_container_retry_on_name_collision`, `test_open_luks_container_real_error_no_retry`

---

### Issue 6 (Medium): GC skips directories without `.lock` file

**File:** `src/engine.rs`

**Problem:** `try_gc_staging()` only cleaned directories that have a `.lock` file. If a process crashed before creating the lock file, or if the lock file mechanism was added after old directories were created, stale `build_*` directories accumulated indefinitely.

**Fix:** Rewrote `try_gc_staging()` with two-branch logic:

1. **With `.lock`:** Try non-blocking `flock(LOCK_EX)`. If lock acquired, the owning process is dead — safe to remove.
2. **Without `.lock`:** Age-based heuristic. If directory's mtime is older than 24 hours (`GC_MAX_AGE_SECS`), it's almost certainly stale.

Before any deletion, an additional safety check reads `/proc/self/mountinfo` to verify no active mount points exist inside the directory (belt-and-suspenders, analogous to issue 14):

```rust
fn gc_remove_dir(path: &Path) {
    if has_active_mounts_inside(path) {
        warn!("GC: Skipping {:?} — active mount points detected inside.", path);
        return;
    }
    if let Err(e) = fs::remove_dir_all(path) {
        warn!("GC: Failed to remove {:?}: {}", path, e);
    } else {
        info!("GC: Removed stale staging dir {:?}", path);
    }
}
```

Helper functions added:
- `is_dir_older_than(path, max_age_secs)` — checks mtime against threshold
- `has_active_mounts_inside(path)` — reads `/proc/self/mountinfo`, matches mount points inside path
- `unescape_mountinfo_octal(s)` — handles kernel's octal escaping (`\040` → space)

**Tests added:** `test_unescape_mountinfo_octal_plain`, `test_unescape_mountinfo_octal_space`, `test_unescape_mountinfo_octal_tab`, `test_unescape_mountinfo_octal_backslash`, `test_unescape_mountinfo_octal_trailing_backslash`, `test_is_dir_older_than_fresh`, `test_is_dir_older_than_large_threshold`, `test_is_dir_older_than_nonexistent`, `test_has_active_mounts_inside_clean_dir`, `test_gc_remove_dir_removes_empty`

---

### Issue 7 (Medium): `process::exit()` bypasses destructors

**Files:** `src/bin/0k-core.rs`, `src/bin/0k.rs`, `src/bin/0k-safe-rm.rs`, `src/error.rs`

**Problem:** Multiple `std::process::exit()` calls across all three binaries. This skips `Drop` implementations, which means:
- `LuksTransaction` doesn't close `/dev/mapper/*` entries
- `CreateTransaction` doesn't clean up incomplete output files
- `flock` locks aren't properly released via RAII

Most critically, the Ctrl+C handler in `0k-core.rs` called `process::exit(130)` after manual cleanup, preventing the main thread's RAII guards from running.

**Fix:** All `process::exit()` calls eliminated across the entire codebase:

1. **Ctrl+C handler (`0k-core.rs`):** Replaced `process::exit(130)` with `AtomicBool` flag:
   ```rust
   static INTERRUPTED: AtomicBool = AtomicBool::new(false);

   ctrlc::set_handler(|| {
       INTERRUPTED.store(true, Ordering::SeqCst);
       cleanup_on_interrupt();
       // Do NOT call process::exit() — let the main thread unwind
   })
   ```

2. **`main()` functions:** All three binaries now return `std::process::ExitCode` instead of calling `process::exit()`:
   ```rust
   fn main() -> std::process::ExitCode {
       let result = run_app();
       if INTERRUPTED.load(Ordering::SeqCst) {
           return std::process::ExitCode::from(130);
       }
       match result {
           Ok(()) => ExitCode::SUCCESS,
           Err(ZkError::CliExit(code)) => ExitCode::from(code as u8),
           Err(e) => { eprintln!("Error: {}", e); ExitCode::FAILURE }
       }
   }
   ```

3. **CLI parse errors:** Added `ZkError::CliExit(i32)` variant. Clap errors are now printed via `e.print()` and propagated as `CliExit` with the appropriate exit code, instead of calling `e.exit()` or `process::exit()`.

4. **`0k-safe-rm`:** Changed from `fn main() -> io::Result<()>` with `process::exit(1)` to `fn main() -> ExitCode` with `ExitCode::FAILURE` returns.

**Flow after fix (Ctrl+C during LUKS operation):**
1. Handler sets `INTERRUPTED` flag + kills child processes + manual mapper close
2. Main thread wakes up (child died) → gets error → error propagates up the call stack
3. `LuksTransaction::drop()` runs (harmless retry of already-closed mapper)
4. `main()` sees `INTERRUPTED` flag → returns `ExitCode(130)`

---

### Issue 11 (Low): Test data races with `set_var`

**Files:** `src/engine.rs` (function `prepare_staging` + tests)

**Problem:** Tests used `unsafe { std::env::set_var("XDG_CACHE_HOME", ...) }` to redirect the staging directory. `set_var` is not thread-safe in Rust (marked `unsafe` since 1.66). When `cargo test` runs tests in parallel, two tests modifying environment variables simultaneously can cause undefined behavior.

**Fix:** Added `staging_root_override: Option<&Path>` parameter to `prepare_staging()`:

```rust
pub fn prepare_staging(
    targets: &[PathBuf],
    dereference: bool,
    staging_root_override: Option<&Path>,
) -> Result<(PathBuf, String, std::fs::File), ZkError> {
    let staging_root = match staging_root_override {
        Some(root) => { fs::create_dir_all(root)?; root.to_path_buf() }
        None => utils::get_0k_temp_dir()?,
    };
    // ... rest unchanged
}
```

- Production code passes `None` (uses default `/tmp/0k-cache-<uid>/`)
- Tests pass `Some(temp_dir)` directly — no `unsafe`, no environment mutation
- Both `unsafe { set_var(...) }` blocks removed from tests

---

### Issue 14 (Low): `0k-safe-rm` doesn't check for active bind mounts

**File:** `src/bin/0k-safe-rm.rs`

**Problem:** `0k-safe-rm` checks that all files in a staging directory are 0-byte stubs before deleting. However, if a bind mount from a crashed `unshare` namespace is still active inside the directory, `remove_dir_all` would follow the bind mount and **delete the original user data**, not just the empty stubs.

While the kernel normally auto-cleans bind mounts when a namespace exits, edge cases exist:
- The namespace process may have been `SIGKILL`ed without proper cleanup
- Mounts created outside the namespace (e.g., by a `freeze.sh` script that failed after bind mounts but before `mksquashfs`)

**Fix:** Added `check_no_active_mounts()` that reads `/proc/self/mountinfo` before any deletion. If any mount point is found inside the target directory, the operation is aborted with a clear error message telling the user how to unmount:

```rust
fn check_no_active_mounts(path: &Path) -> io::Result<()> {
    let canonical = path.canonicalize()?;
    let target_prefix = canonical.to_string_lossy().to_string();
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;

    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 { continue; }
        let mount_point = unescape_mountinfo(fields[4]);
        if mount_point.starts_with(&target_prefix) && mount_point.len() > target_prefix.len() {
            return Err(io::Error::new(io::ErrorKind::Other, format!(
                "Active mount point detected inside target: '{}'. \
                 Please unmount it first.", mount_point
            )));
        }
    }
    Ok(())
}
```

Also added `unescape_mountinfo()` to handle kernel's octal escaping in paths (e.g., `\040` for spaces).

**Tests added:** `test_unescape_mountinfo_plain`, `test_unescape_mountinfo_space`, `test_unescape_mountinfo_tab`, `test_unescape_mountinfo_no_octal`, `test_check_no_active_mounts_clean_dir`

---

### Issue 17 (Low): Duplicate mock implementation in `0k-core.rs`

**Files:** `Cargo.toml`, `src/executor.rs`, `src/bin/0k-core.rs`, `Justfile`

**Problem:** `0k-core.rs` contained a manual 22-line `mock! { CommandExecutor {} ... }` block that duplicated the entire `CommandExecutor` trait definition. This existed because `MockCommandExecutor` generated by `#[cfg_attr(test, mockall::automock)]` in the library is only available within the library's own test context. When the library is compiled as a dependency for binary tests, `cfg(test)` is not set, so the mock doesn't exist.

This duplication means any change to the `CommandExecutor` trait requires updating both the trait definition AND the manual mock — a maintenance burden and source of bugs.

**Fix:** Introduced a Cargo feature flag `testing` that activates `mockall::automock` generation in the library even when used as a dependency:

1. **`Cargo.toml`:** Added `mockall` as optional dependency + `testing` feature:
   ```toml
   [dependencies]
   mockall = { version = "0.14.0", optional = true }

   [features]
   testing = ["dep:mockall"]

   [dev-dependencies]
   mockall = "0.14.0"
   ```

2. **`executor.rs`:** Extended cfg condition:
   ```rust
   #[cfg_attr(any(test, feature = "testing"), mockall::automock)]
   pub trait CommandExecutor { ... }
   ```

3. **`0k-core.rs`:** Replaced 22-line `mock! {}` block with a single import:
   ```rust
   use zero_kelvin::executor::MockCommandExecutor;
   ```

4. **`Justfile`:** Updated `unit-tests` to pass `--features testing`:
   ```
   cargo test --locked --features testing
   ```

**How it works:**
- Library tests: `cfg(test)` generates the mock (as before)
- Binary tests: `--features testing` activates mock generation via the feature flag
- Production builds: `mockall` is not compiled (optional, feature not active)

---

## Informational Issues (No Action Required)

### Issue 13: No `--delete` confirmation prompt

The `check --delete` flag can remove files without asking for confirmation. This is by design (scripts/automation), but could be dangerous for interactive use. A future `--interactive` flag or `-i` could add confirmation. Low priority.

### Issue 15: `freeze.sh` left on disk in staging area

The generated shell script is written to `build_dir/freeze.sh` before being passed to `unshare sh`. The script contains paths and flags but no sensitive data (passwords are prompted interactively by `cryptsetup`). Cleaned up during post-freeze staging removal. No security impact.

---

## Re-audit (Post-fix Review)

A second pass identified 8 additional issues (including regressions from the initial fixes):

| # | Severity | Status | Description |
|---|----------|--------|-------------|
| R1 | Critical | **Fixed** | `unescape_mountinfo_octal` accepted digits 8-9 (not valid octal), causing `u8` overflow/panic |
| R2 | Medium | **Fixed** | `has_active_mounts_inside` returned `false` on error → could allow GC deletion of mounted dirs |
| R3 | Medium | **Fixed** | `.unwrap()` in `executor.rs:83` (`child.stderr.take()`) — policy violation |
| R4 | Medium | **Fixed** | `.expect()` in `executor.rs:178` (`Regex::new`) — policy violation |
| R5 | Low | **Fixed** | `.unwrap()` in `0k.rs:441` (`args.pop()`) — policy violation |
| R6 | Low | **Fixed** | `CliExit(i32)` → `u8` truncation: changed to `CliExit(u8)` |
| R7 | Low | **Fixed** | `unwrap_or("unknown")` in `engine.rs` silently constructed wrong paths |
| R8 | Low | **Fixed** | `/proc/mounts` parsing in `0k-core.rs` didn't decode octal escapes (paths with spaces) |

### R1: Octal digit overflow (engine.rs, 0k-safe-rm.rs)

**Problem:** `unescape_mountinfo_octal` used `is_ascii_digit()` which accepts 0-9. Octal only uses 0-7. Input like `\899` would compute `(8-0)*64 = 512`, overflowing `u8` — panic in debug, silent corruption in release.

**Fix:** Changed validation from `is_ascii_digit()` to `(b'0'..=b'7').contains()` in both `engine.rs` and `0k-safe-rm.rs`.

### R2: `has_active_mounts_inside` fail-open (engine.rs)

**Problem:** When `canonicalize()` or reading `/proc/self/mountinfo` failed, the function returned `false` ("no mounts found"), allowing `gc_remove_dir` to proceed with deletion even when mount state was unknown.

**Fix:** Changed both error branches to return `true` ("assume unsafe, skip deletion").

### R3-R5: Remaining `.unwrap()`/`.expect()` policy violations

**Fix:** Replaced with `?`-propagating error handling:
- `executor.rs:83`: `.unwrap()` → `.ok_or_else(|| io::Error::new(...))?`
- `executor.rs:178`: `.expect()` → `.map_err(|e| io::Error::new(...))?`
- `0k.rs:441`: `.unwrap()` → `.ok_or_else(|| ZkError::MissingTarget(...))?`

### R6: CliExit type narrowing

**Fix:** Changed `ZkError::CliExit(i32)` to `ZkError::CliExit(u8)`. All call sites now cast `e.exit_code() as u8` at the point of creation.

### R7: Silent fallback to "unknown" entry name

**Fix:** Replaced `.unwrap_or("unknown")` with `.ok_or_else(|| ZkError::OperationFailed(...))` in both `check` and `restore_from_mount` functions. Missing entry names now produce clear error messages instead of silently constructing wrong paths.

### R8: /proc/mounts octal escapes in 0k-core.rs

**Problem:** The LUKS umount flow parsed `/proc/mounts` without decoding kernel octal escapes (`\040` = space). Mount points with spaces in their path would not be found.

**Fix:** Made `unescape_mountinfo_octal()` public in the library and applied it to `mount_point` in the `/proc/mounts` parsing loop in `0k-core.rs`.

---

## Test Summary

After all fixes (initial + re-audit):

```
Running unittests src/lib.rs:      42 passed (+11 new: GC helpers, mountinfo, octal edge cases)
Running unittests src/bin/0k.rs:   10 passed
Running unittests src/bin/0k-core: 11 passed (+4 new: mapper sanitization, LUKS open retry)
Running unittests src/bin/0k-safe: 7 passed  (+5 new: mount check, unescape)
Running tests/file_type_detection: 1 passed
Total: 71 passed, 0 failed
```

---

## Appendix: Files Modified

| File | Changes |
|------|---------|
| `src/engine.rs` | Issues 1, 2, 6, 9, 11, 12, 16, R1, R2, R7 + 16 new tests |
| `src/manifest.rs` | Issues 3, 10 + 3 new test cases |
| `src/executor.rs` | Issues 17, R3, R4: `cfg_attr`, `.unwrap()` fix, `.expect()` fix |
| `src/error.rs` | Issues 7, R6: `CliExit(u8)` |
| `src/bin/0k-core.rs` | Issues 2, 4, 5, 7, 8, 17, R8 + 4 new tests, octal decode in /proc/mounts |
| `src/bin/0k-safe-rm.rs` | Issues 7, 14, R1: mount check + `ExitCode` + octal fix + 5 new tests |
| `src/bin/0k.rs` | Issues 4, 7, R5: `ExitCode` + `.unwrap()` fix |
| `Cargo.toml` | Issue 17: `testing` feature + optional `mockall` dep |
| `Justfile` | Issue 17: `--features testing` in unit-tests |
