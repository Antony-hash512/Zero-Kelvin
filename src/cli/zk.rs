use clap::{Parser, Subcommand};
use std::path::PathBuf;
use crate::constants::DEFAULT_ZSTD_COMPRESSION;

const BANNER: &str = concat!(
    r#"
Copyleft 🄯 2026 :: GPL3
github.com/Antony-hash512/Zero-Kelvin
   __  __  _
  /  \|  |/ /  Zero Kelvin Offload Tool v"#,env!("CARGO_PKG_VERSION"),r#"
 ( 0  │  K <   [ Freeze your data. Free your space. ]
  \__/|__|\_\  Blazed by Rust
"#
);

#[derive(Parser, Debug)]
#[command(
    name = "0k",
    about = "Zero Kelvin - Cold Storage Utility",
    long_version = concat!("\rZero Kelvin Offload Tool\na.k.a. `0k` ", env!("CARGO_PKG_VERSION"))
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

impl Args {
    pub fn build_command() -> clap::Command {
        use clap::CommandFactory;
        let cmd = Self::command();
        cmd.after_help(format!(
            "Detailed Command Information:
{0}
  freeze [TARGETS...] [ARCHIVE_PATH] [OPTIONS]
    Offload data to a SquashFS archive (frozen state).
    Arguments:
      TARGETS...            Files or directories to freeze.
      ARCHIVE_PATH          Destination .sqfs archive path.
    Options:
      -e, --encrypt         Encrypt the archive using LUKS (via 0k-core).
      -r, --read <FILE>     Read list of targets from a file.
      -c, --compression N   Zstd compression level (default: {1}) 0 = no compression.
          --no-progress     Disable progress bar.
          --prefix <NAME>   Prefix for auto-generated filename
                            (when ARCHIVE_PATH is a directory).
                            If omitted, you will be prompted interactively.

  unfreeze <ARCHIVE_PATH> [OPTIONS]
    Restore data from a frozen archive to its original locations.
    Arguments:
      ARCHIVE_PATH          Path to the .sqfs archive to restore.
    Options:
      --overwrite           Overwrite existing files.
      --skip-existing       Skip files that already exist.
      --force-unfreeze      Force unfreeze even if hostname mismatches.
      --verify              Verify archive integrity before restoring.

  check <ARCHIVE_PATH> [OPTIONS]
    Verify archive integrity against the live system.
    Arguments:
      ARCHIVE_PATH          Path to the .sqfs archive to check.
    Options:
      --use-cmp             Verify file content (byte-by-byte) in addition to size/mtime.
      --delete              Delete local files if they match the archive (Destructive!).
      -D, --force-delete    Modifier for --delete: also delete files newer than archive.
                            (Useful for cleaning up already restored/unfrozen files).

Full help for a specific command can be obtained via:
  zero-kelvin <command> --help
  0k help <command>
",
            BANNER, DEFAULT_ZSTD_COMPRESSION
        ))
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Freeze data into a SquashFS archive
    #[command(
        arg_required_else_help = true,
        override_usage = "0k freeze [OPTIONS] [TARGETS]... [ARCHIVE_PATH]",
        about = "Offload data to a SquashFS archive (frozen state)",
        long_about = "Offload data to a SquashFS archive (frozen state).\n\n\
                     Arguments:\n  \
                       TARGETS...            Files or directories to freeze.\n  \
                       ARCHIVE_PATH          Destination .sqfs archive path.\n\n\
                     Note: If [ARCHIVE_PATH] is a directory, the utility will prompt for a filename.\n\
                     The last positional argument is always treated as the destination ARCHIVE_PATH."
    )]
    Freeze {
        /// Files/directories to freeze followed by the destination ARCHIVE_PATH
        #[arg(value_name = "TARGETS]... [ARCHIVE_PATH", num_args = 1..)]
        args: Vec<PathBuf>,

        /// Encrypt the archive using LUKS
        #[arg(short, long)]
        encrypt: bool,

        /// Read the list of target paths from a file
        #[arg(short, long, value_name = "FILE")]
        read: Option<PathBuf>,

        /// Overwrite files inside existing archive (Applies to both Plain and LUKS)
        #[arg(long)]
        overwrite_files: bool,

        /// Replace ENTIRE content of LUKS container (Requires LUKS output)
        #[arg(long)]
        overwrite_luks_content: bool,

        /// Disable progress bar
        #[arg(long, group = "progress")]
        no_progress: bool,

        /// Use standard mksquashfs progress bar (Default behavior)
        #[arg(long, group = "progress")]
        vanilla_progress: bool,

        /// Use experimental Alfa progress bar
        #[arg(long, group = "progress")]
        alfa_progress: bool,

        /// Zstd compression level (0 = none, default: see help)
        #[arg(short = 'c', long, value_name = "LEVEL")]
        compression: Option<u32>,

        /// Dereference symlinks (store their content instead of the link)
        #[arg(short = 'L', long)]
        dereference: bool,

        /// Prefix for auto-generated filename (when ARCHIVE_PATH is a directory).
        /// Skips the interactive prompt.
        #[arg(long, value_name = "NAME")]
        prefix: Option<String>,
    },
    /// Unfreeze (restore) data from a SquashFS archive
    Unfreeze {
        /// Path to the SquashFS archive
        #[arg(value_name = "ARCHIVE_PATH")]
        archive_path: PathBuf,

        /// Overwrite existing files
        #[arg(long)]
        overwrite: bool,

        /// Skip existing files (conflicts)
        #[arg(long)]
        skip_existing: bool,

        /// Skip hostname mismatch check (non-interactive mode)
        #[arg(long)]
        force_unfreeze: bool,
        
        /// Verify archive integrity before restoring (pre-flight check)
        #[arg(long)]
        verify: bool,
    },
    /// Check integrity of an archive against the original files
    Check {
        /// Path to the SquashFS archive
        #[arg(value_name = "ARCHIVE_PATH")]
        archive_path: PathBuf,

        /// Perform byte-by-byte comparison
        #[arg(long)]
        use_cmp: bool,

        /// Delete local files if they match the archive content
        #[arg(long)]
        delete: bool,

        /// Force delete even if local file is newer (ignoring mtime).
        /// This is a modifier for --delete. Useful if you want to clean up
        /// files that were already restored (unfrozen) as they often have newer mtime.
        #[arg(short = 'D', long, requires = "delete")]
        force_delete: bool,
    },
}
