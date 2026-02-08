use clap::{Parser, Subcommand};
use rand::Rng;
use std::path::{Path, PathBuf};

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

const BANNER: &str = concat!(
    r#"
Copyleft 🄯 2026 :: GPL3
github.com/Antony-hash512/Zero-Kelvin-Stazis
   __  __  _
  /  \|  |/ /  Zero Kelvin Offload Tool v"#,env!("CARGO_PKG_VERSION"),r#"
 ( 0  │  K <   [ Freeze your data. Free your space. ]
  \__/|__|\_\  Blazed by Rust
"#
);

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
      -c, --compression N   Zstd compression level (default: {1}).
          --prefix <NAME>   Prefix for auto-generated filename
                            (when ARCHIVE_PATH is a directory).
                            If omitted, you will be prompted interactively.

  unfreeze <ARCHIVE_PATH>
    Restore data from a frozen archive to its original locations.
    Arguments:
      ARCHIVE_PATH          Path to the .sqfs archive to restore.

  check <ARCHIVE_PATH> [OPTIONS]
    Verify archive integrity against the live system.
    Arguments:
      ARCHIVE_PATH          Path to the .sqfs archive to check.
    Options:
      --use-cmp             Verify file content (byte-by-byte) in addition to size/mtime.
      --delete              Delete local files if they match the archive (Destructive!).
      -D, --force-delete    Modifier for --delete: also delete files newer than archive.
                            (Useful for cleaning up already restored/unfrozen files).
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
        // #[arg(short = 'p', long, value_name = "NAME")]
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

use std::fs;
use zero_kelvin::constants::DEFAULT_ZSTD_COMPRESSION;
use zero_kelvin::engine::{self, FreezeOptions, UnfreezeOptions};
use zero_kelvin::error::ZkError;
use zero_kelvin::executor::RealSystem;
use zero_kelvin::utils;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if let Err(e) = run_app() {
        if let Some(friendly) = e.friendly_message() {
            eprintln!("Suggestion: {}", friendly);
        }
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_app() -> Result<(), ZkError> {
    let args_raw: Vec<String> = std::env::args().collect();

    // 1. No args -> Help + Exit 0
    if args_raw.len() <= 1 {
        Args::build_command().print_help().unwrap_or_default();
        println!();
        return Ok(());
    }

    let matches = match Args::build_command().try_get_matches() {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind;
            match e.kind() {
                // 2. Invalid subcommand -> Full Help + Exit 2
                ErrorKind::InvalidSubcommand | ErrorKind::UnknownArgument => {
                    if args_raw.len() >= 2 && !args_raw[1].starts_with('-') {
                        eprintln!("Error: {}\n", e);
                        Args::build_command().print_help().unwrap_or_default();
                        println!();
                        std::process::exit(2);
                    }
                }
                // 3. Command specific errors -> Subcommand Help
                ErrorKind::MissingRequiredArgument
                | ErrorKind::MissingSubcommand
                | ErrorKind::TooFewValues
                | ErrorKind::ValueValidation => {
                    if args_raw.len() >= 2 {
                        let sub = &args_raw[1];
                        let mut cmd = Args::build_command();
                        if let Some(sub_cmd) = cmd.find_subcommand_mut(sub) {
                            eprintln!("Error: {}\n", e);
                            sub_cmd.print_help().unwrap_or_default();
                            println!();
                            std::process::exit(e.exit_code());
                        }
                    }
                }
                _ => {}
            }
            e.exit();
        }
    };

    use clap::FromArgMatches;
    let args = Args::from_arg_matches(&matches)
        .map_err(|e| {
            e.exit();
        })
        .unwrap();

    match args.command {
        Commands::Freeze {
            args,
            encrypt,
            read,
            overwrite_files,
            overwrite_luks_content,
            no_progress,
            vanilla_progress: _vanilla_progress,
            alfa_progress,
            compression,
            dereference,
            prefix,
        } => {
            let (targets, output) = resolve_freeze_args(args, read)?;
            let executor = RealSystem;

            // If output is a directory, resolve to a full file path
            let output = if output.is_dir() {
                resolve_directory_output(&output, prefix, encrypt)?
            } else {
                output
            };

            let progress_mode = if no_progress {
                engine::ProgressMode::None
            } else if alfa_progress {
                engine::ProgressMode::Alfa
            } else {
                // Default is Vanilla (even if vanilla_progress is false but others are false too)
                engine::ProgressMode::Vanilla
            };

            let options = FreezeOptions {
                encrypt,
                output,
                overwrite_files,
                overwrite_luks_content,
                progress_mode,
                compression,
                dereference,
            };

            // Log info
            // println!("Freezing {:?} to {:?}", targets, options.output);

            // engine::freeze(&targets, &options, &executor)?;
            if let Err(e) = engine::freeze(&targets, &options, &executor) {
                if utils::is_permission_denied(&e) {
                    if let Some(runner) = utils::check_root_or_get_runner(
                        "Permission denied during freeze. Retrying with elevation...",
                    )? {
                        return utils::re_exec_with_runner(&runner);
                    }
                }
                return Err(e);
            }
            println!("Successfully created archive: {:?}", options.output);
        }
        Commands::Unfreeze {
            archive_path,
            overwrite,
            skip_existing,
        } => {
            let options = UnfreezeOptions {
                overwrite,
                skip_existing,
            };
            let executor = RealSystem;
            // engine::unfreeze(&archive_path, &options, &executor)?;
            if let Err(e) = engine::unfreeze(&archive_path, &options, &executor) {
                if utils::is_permission_denied(&e) {
                    if let Some(runner) = utils::check_root_or_get_runner(
                        "Permission denied during unfreeze. Retrying with elevation...",
                    )? {
                        return utils::re_exec_with_runner(&runner);
                    }
                }
                return Err(e);
            }
            println!("Unfreeze completed successfully.");
        }
        Commands::Check {
            archive_path,
            use_cmp,
            delete,
            force_delete,
        } => {
            let executor = RealSystem;
            let options = engine::CheckOptions {
                use_cmp,
                delete,
                force_delete,
            };
            // engine::check(&archive_path, &options, &executor)?;
            if let Err(e) = engine::check(&archive_path, &options, &executor) {
                if utils::is_permission_denied(&e) {
                    if let Some(runner) = utils::check_root_or_get_runner(
                        "Permission denied during check. Retrying with elevation...",
                    )? {
                        return utils::re_exec_with_runner(&runner);
                    }
                }
                return Err(e);
            }
            println!("Check completed successfully.");
        }
    }

    Ok(())
}

/// Resolve output directory to a full file path with auto-generated name.
/// If `prefix` is Some, uses it directly. Otherwise, prompts the user interactively.
fn resolve_directory_output(
    dir: &Path,
    prefix: Option<String>,
    encrypt: bool,
) -> Result<PathBuf, ZkError> {
    let prefix = match prefix {
        Some(p) => p,
        None => prompt_for_prefix()?,
    };

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| ZkError::OperationFailed(format!("Time error: {}", e)))?
        .as_secs();
    let rnd: u32 = rand::rng().random_range(100000..999999u32);
    let ext = if encrypt { "sqfs_luks.img" } else { "sqfs" };
    let filename = format!("{}_{}_{}.{}", prefix, timestamp, rnd, ext);

    let final_path = dir.join(filename);
    eprintln!("Auto-generated output filename: {}", final_path.display());
    Ok(final_path)
}

/// Prompt user interactively via stderr/stdin to enter a prefix for the output filename.
fn prompt_for_prefix() -> Result<String, ZkError> {
    use std::io::{self, BufRead, Write};

    eprint!("Output is a directory. Enter a prefix for the archive filename: ");
    io::stderr().flush().map_err(ZkError::IoError)?;

    let stdin = io::stdin();
    let line = stdin
        .lock()
        .lines()
        .next()
        .ok_or_else(|| {
            ZkError::OperationFailed("Failed to read prefix from stdin (no input)".into())
        })?
        .map_err(ZkError::IoError)?;

    let trimmed = line.trim().to_string();
    if trimmed.is_empty() {
        return Err(ZkError::OperationFailed("Prefix cannot be empty".into()));
    }
    // Sanitize: disallow path separators and other problematic characters
    if trimmed.contains('/') || trimmed.contains('\0') {
        return Err(ZkError::OperationFailed(
            "Prefix cannot contain '/' or null characters".into(),
        ));
    }
    Ok(trimmed)
}

fn resolve_freeze_args(
    mut args: Vec<PathBuf>,
    read_file: Option<PathBuf>,
) -> Result<(Vec<PathBuf>, PathBuf), ZkError> {
    // Logic:
    // Last argument is Output Path (Archive).
    // Preceding arguments are Targets.
    // If -r file provided, read lines and add to Targets.

    // 1. Determine Output Path
    if args.is_empty() {
        return Err(ZkError::MissingTarget(
            "Destination archive path is required".into(),
        ));
    }

    let output_path = args.pop().unwrap(); // Last one

    // 2. Collect Targets
    let mut targets = args; // The rest are targets

    // 3. Read from file if provided
    if let Some(path) = read_file {
        let content = fs::read_to_string(&path).map_err(|e| ZkError::IoError(e))?;

        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // Fix: Expand tilde manually
                let expanded = zero_kelvin::utils::expand_tilde(trimmed);
                targets.push(expanded);
            }
        }
    }

    if targets.is_empty() {
        return Err(ZkError::MissingTarget(
            "No targets specified to freeze".into(),
        ));
    }

    // 4. Handle Output Directory case
    // 0k-core now handles directory selection/autonaming.

    Ok((targets, output_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Args::command().debug_assert();
    }

    #[test]
    fn test_parse_freeze_args() {
        let args = Args::parse_from(&[
            "0k",
            "freeze",
            "/home/user/data",
            "/mnt/backup/data.sqfs",
            "/mnt/backup/data.sqfs",
            "-e",
            "--read",
            "/tmp/list.txt",
            "-c",
            "19",
        ]);

        match args.command {
            Commands::Freeze {
                args,
                encrypt,
                read,
                overwrite_files,
                overwrite_luks_content,
                no_progress,
                vanilla_progress,
                alfa_progress,
                compression,
                dereference,
                prefix,
            } => {
                assert_eq!(args[0], PathBuf::from("/home/user/data"));
                assert_eq!(args[1], PathBuf::from("/mnt/backup/data.sqfs"));
                assert!(encrypt);
                assert_eq!(read, Some(PathBuf::from("/tmp/list.txt")));
                assert!(!overwrite_files);
                assert!(!overwrite_luks_content);
                assert!(!no_progress); // not passed
                assert!(!vanilla_progress); // not passed
                assert!(!alfa_progress); // not passed
                assert_eq!(compression, Some(19));
                assert!(!dereference);
                assert_eq!(prefix, None); // not passed
            }
            _ => panic!("Expected Freeze command"),
        }
    }

    #[test]
    fn test_parse_freeze_progress_flags() {
        // Test vanilla-progress
        let args =
            Args::parse_from(&["0k", "freeze", "target", "out.sqfs", "--vanilla-progress"]);
        if let Commands::Freeze {
            vanilla_progress,
            no_progress,
            alfa_progress,
            compression,
            ..
        } = args.command
        {
            assert!(vanilla_progress);
            assert!(!no_progress);
            assert!(!alfa_progress);
            assert_eq!(compression, None);
        } else {
            panic!("Wrong command");
        }

        // Test no-progress
        let args = Args::parse_from(&["0k", "freeze", "target", "out.sqfs", "--no-progress"]);
        if let Commands::Freeze {
            vanilla_progress,
            no_progress,
            ..
        } = args.command
        {
            assert!(no_progress);
            assert!(!vanilla_progress);
        } else {
            panic!("Wrong command");
        }
    }

    #[test]
    fn test_parse_check_args() {
        let args = Args::parse_from(&["0k", "check", "archive.sqfs", "--use-cmp", "--delete"]);
        match args.command {
            Commands::Check {
                archive_path,
                use_cmp,
                delete,
                force_delete,
            } => {
                assert_eq!(archive_path, PathBuf::from("archive.sqfs"));
                assert!(use_cmp);
                assert!(delete);
                assert!(!force_delete);
            }
            _ => panic!("Expected Check command"),
        }
    }

    #[test]
    fn test_resolve_freeze_args_basic() {
        let args = vec![
            PathBuf::from("t1"),
            PathBuf::from("t2"),
            PathBuf::from("out.sqfs"),
        ];
        let (targets, out) = super::resolve_freeze_args(args, None).unwrap();
        assert_eq!(targets, vec![PathBuf::from("t1"), PathBuf::from("t2")]);
        assert_eq!(out, PathBuf::from("out.sqfs"));
    }

    #[test]
    fn test_resolve_freeze_args_with_file() {
        use std::io::Write;
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "file_from_list").unwrap();
        writeln!(tmp, " # comment").unwrap();
        writeln!(tmp, "file2_from_list").unwrap();

        let file_path = tmp.path().to_path_buf();
        let args = vec![PathBuf::from("cli_target"), PathBuf::from("out.sqfs")];

        let (targets, out) = super::resolve_freeze_args(args, Some(file_path)).unwrap();
        assert_eq!(out, PathBuf::from("out.sqfs"));
        assert_eq!(targets.len(), 3);
        assert!(targets.contains(&PathBuf::from("cli_target")));
        assert!(targets.contains(&PathBuf::from("file_from_list")));
        assert!(targets.contains(&PathBuf::from("file2_from_list")));
    }

    #[test]
    fn test_resolve_freeze_args_no_output() {
        let args = vec![];
        let res = super::resolve_freeze_args(args, None);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_freeze_with_prefix() {
        let args =
            Args::parse_from(&["0k", "freeze", "target", "out_dir", "--prefix", "mybackup"]);
        if let Commands::Freeze { prefix, .. } = args.command {
            assert_eq!(prefix, Some("mybackup".to_string()));
        } else {
            panic!("Wrong command");
        }
    }

    #[test]
    fn test_resolve_directory_output_with_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let result =
            super::resolve_directory_output(dir.path(), Some("myprefix".into()), false).unwrap();
        let filename = result.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("myprefix_"));
        assert!(filename.ends_with(".sqfs"));
        assert_eq!(result.parent().unwrap(), dir.path());
    }

    #[test]
    fn test_resolve_directory_output_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let result =
            super::resolve_directory_output(dir.path(), Some("secret".into()), true).unwrap();
        let filename = result.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("secret_"));
        assert!(filename.ends_with(".sqfs_luks.img"));
    }
}
