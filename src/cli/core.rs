use clap::Parser;
use std::path::PathBuf;
use crate::constants::DEFAULT_ZSTD_COMPRESSION;

const BANNER: &str = r#"
Copyleft 🄯 2026 :: GPL3
github.com/Antony-hash512/Zero-Kelvin
  ____  _                            
 / __ \| | __      ___ ___  _ __ ___ 
| | /| | |/ /____ / __/ _ \| '__/ _ \
| |/_| |   <_____| (_| (_) | | |  __/
 \____/|_|\_\     \___\___/|_|  \___|
                                     
also known as
 ____                        _      
/ ___|  __ _ _   _  __ _ ___| |__   
\___ \ / _` | | | |/ _` / __| '_ \  
 ___) | (_| | |_| | (_| \__ \ | | | 
|____/ \__, |\__,_|\__,_|___/_| |_| 
          |_|  Manager           
"#;

#[derive(Parser, Debug)]
#[command(
    name = "0k-core", 
    about = "Manages SquashFS archives", 
    version
)]
pub struct Args {
    #[command(subcommand)]
    pub command: Commands,
}

impl Args {
    pub fn build_command() -> clap::Command {
        use clap::CommandFactory;
        let cmd = Self::command();
        cmd.after_help(format!("Detailed Command Information:
{0}
  create <INPUT> [OUTPUT] [OPTIONS]
    Convert a directory or an archive into a SquashFS image.
    Arguments:
      INPUT                 Source directory or archive file.
      OUTPUT                (Optional) Path to the resulting image.
    Options:
      -e, --encrypt         Create an encrypted LUKS container (Requires root/sudo).
      -c, --compression N   Zstd compression level (default: {1}) 0 = no compression.
      --no-progress         Disable progress bar completely.
      --vanilla-progress    Use native mksquashfs progress (explicit, also default).
      --alfa-progress       Use experimental custom progress bar (not fixed in encryption mode, yet; for testing).

    Supported Input Formats (repacked on-the-fly via pipe):
      - Directory: Standard behavior
      - Tarball:   .tar (requires 'cat')
      - Combos:    .tar.gz, .tgz (requires 'gzip')
                   .tar.bz2, .tbz2 (requires 'bzip2')
                   .tar.xz, .txz (requires 'xz')
                   .tar.zst, .tzst (requires 'zstd')
                   .tar.zip (requires 'unzip')
                   .tar.7z (requires '7z')
                   .tar.rar (requires 'unrar')
      Note: Archive repacking requires 'tar2sqfs' (from squashfs-tools-ng) installed.

  mount <IMAGE> [MOUNT_POINT]
    Mount a SquashFS image as a directory.
    Arguments:
      IMAGE                 Path to the SquashFS image file.
      MOUNT_POINT           (Optional) Manual mount point.
                            Generated if omitted (prefix_timestamp_random).

  umount <TARGET>
    Unmounts a directory or all instances of an image.
    Arguments:
      TARGET                Mount point directory OR path to the image file.
", BANNER, DEFAULT_ZSTD_COMPRESSION))
    }
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Create a new SquashFS archive from a directory or existing tar archive file
    Create {
        /// Path to the source directory or tar archive file
        #[arg(value_name = "INPUT")]
        input_path: PathBuf,

        /// Path where the resulting SquashFS archive will be saved
        #[arg(value_name = "OUTPUT")]
        output_path: Option<PathBuf>,

        /// Encrypt the archive using LUKS (Requires root/sudo)
        #[arg(short, long)]
        encrypt: bool,

        /// Zstd compression level
        #[arg(short, long, default_value_t = DEFAULT_ZSTD_COMPRESSION)]
        compression: u32,

        /// Disable progress bar completely
        #[arg(long)]
        no_progress: bool,

        /// Use native mksquashfs progress output (explicit, also the default)
        #[arg(long)]
        vanilla_progress: bool,

        /// Use experimental custom progress bar (broken in encryption mode, for testing only)
        #[arg(long, hide = true)]
        alfa_progress: bool,

        /// Overwrite files inside existing archive (Applies to both Plain and LUKS)
        #[arg(long)]
        overwrite_files: bool,

        /// Replace ENTIRE content of LUKS container (Requires LUKS output)
        #[arg(long)]
        overwrite_luks_content: bool,
    },
    /// Mount a SquashFS archive to a directory (using squashfuse)
    Mount {
        /// Path to the SquashFS image file
        #[arg(value_name = "IMAGE")]
        image: PathBuf,
        /// Optional: Manual mount point. If omitted, a directory is created in the current working directory.
        #[arg(value_name = "MOUNT_POINT")]
        mount_point: Option<PathBuf>,
    },
    /// Unmount a previously mounted SquashFS image (using fusermount -u)
    Umount {
        /// Target mount point directory OR path to the source image file
        #[arg(value_name = "TARGET")]
        mount_point: PathBuf,
    },
}
