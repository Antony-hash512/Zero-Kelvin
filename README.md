# 🧊 Zero-Kelvin Stazis (Rust)

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE.GPLv3)
[![Rust](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)

**Zero-Kelvin Stazis (zks-rs)** is a high-performance, identity-preserving data offloading utility. It's the Rust port of the original `zero-kelvin-store` Fish functions, designed for "freezing" projects and data into compressed, mountable SquashFS archives to free up disk space while maintaining instant accessibility.

---

## 🚀 Key Features

- **Offloading, not just Backup:** Conceptually move data to cold storage with safe verification and optional deletion.
- **Rootless Freeze:** Uses User Namespaces (`unshare`) to archive sensitive files without requiring `sudo` whenever possible.
- **Instant Access:** Archives are SquashFS images. Mount them instantly to browse files without full extraction.
- **Zero-Knowledge Privacy:** Supports LUKS-encrypted containers (via `squash_manager-rs`) for secure "black box" archiving.
- **Contextual Sets:** Bundle logical projects spread across your filesystem into a single atomic archive.
- **Prescriptive Restore:** Manifest-driven restoration remembers exactly where files belong.

---

## 🛠 Installation

Currently in development. To build from source:

```bash
cargo build --release
```

The build produces two main binaries:
- `zks-rs`: The primary high-level orchestrator.
- `squash_manager-rs`: Low-level tool for SquashFS and LUKS management.

---

## 📖 Usage

### Freeze (Archive)
Move logicially grouped paths into a "frozen" state.

```bash
# Basic freeze
zks-rs freeze ~/projects/old-work /mnt/nas/archives/old-work.sqfs

# Encrypted freeze
zks-rs freeze --encrypt ~/secret-data /mnt/nas/archives/secure.sqfs_luks.img

# Freeze multiple targets
zks-rs freeze /etc/nginx/sites-available /var/www/html /mnt/nas/backup/web-server.sqfs
```

### Unfreeze (Restore)
Return data to its original location instantly.

```bash
zks-rs unfreeze /mnt/nas/archives/old-work.sqfs
```

### Check (Verify)
Compare an archive against the live system.

```bash
# Verify integrity
zks-rs check /mnt/nas/archives/old-work.sqfs

# Verify and safely delete local files that match the archive (Offloading)
zks-rs check --force-delete /mnt/nas/archives/old-work.sqfs
```

---

## 🔧 Core Philosophy

1. **Low Friction:** Restoration should be as simple as `Ctrl+Z`.
2. **KISS & Native:** Uses standard Linux tools (`rsync`, `mksquashfs`, `cryptsetup`) so your data is never locked into a proprietary format.
3. **User-Space Friendly:** Prefers FUSE (`squashfuse`) and namespaces over root privileges.

---

## 📜 License

This project is licensed under the **GPLv3 License**. See `LICENSE.GPLv3` for details.
