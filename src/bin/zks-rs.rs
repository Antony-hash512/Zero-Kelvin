use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "zks", 
    about = "Zero Kelvin Stazis - Cold Storage Utility", 
    version
)]
pub struct ZksArgs {
    #[command(subcommand)]
    pub command: Commands,
}

const BANNER: &str = r#"
 _____                _  __    _       _         ____  _            _     
|__  /___ _ __ ___   | |/ /___| |_   _(_)_ __   / ___|| |_ __ _ ___(_)___ 
  / // _ \ '__/ _ \  | ' // _ \ \ \ / / | '_ \  \___ \| __/ _` |_  / / __|
 / /|  __/ | | (_) | | . \  __/ |\ V /| | | | |  ___) | || (_| |/ /| \__ \
/____\___|_|  \___/  |_|\_\___|_| \_/ |_|_| |_| |____/ \__\__,_/___|_|___/
"#;

impl ZksArgs {
    pub fn build_command() -> clap::Command {
        use clap::CommandFactory;
        let cmd = Self::command();
        cmd.after_help(format!("Detailed Command Information:
{0}
  freeze [TARGETS...] [ARCHIVE_PATH] [OPTIONS]
    Offload data to a SquashFS archive (frozen state).
    Arguments:
      TARGETS...            Files or directories to freeze.
      ARCHIVE_PATH          Destination .sqfs archive path.
    Options:
      -e, --encrypt         Encrypt the archive using LUKS (via squash_manager).
      -r, --read <FILE>     Read list of targets from a file.
      -c, --compression N   Zstd compression level (default: {1}).

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
      --force-delete        Delete local files if they match the archive (Destructive!).
", BANNER, DEFAULT_ZSTD_COMPRESSION))
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Freeze data into a SquashFS archive
    #[command(
        arg_required_else_help = true,
        override_usage = "zks-rs freeze [OPTIONS] [TARGETS]... [ARCHIVE_PATH]",
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
        force_delete: bool,
    },
}

use anyhow::{Result, anyhow, Context};
use zero_kelvin_stazis::engine::{self, FreezeOptions, UnfreezeOptions, CheckOptions};
use zero_kelvin_stazis::constants::DEFAULT_ZSTD_COMPRESSION;
use zero_kelvin_stazis::executor::RealSystem;
use std::fs;

fn main() -> Result<()> {
    let args_raw: Vec<String> = std::env::args().collect();

    // 1. No args -> Help + Exit 0
    if args_raw.len() <= 1 {
         ZksArgs::build_command().print_help().unwrap_or_default();
         println!();
         return Ok(());
    }

    let matches = match ZksArgs::build_command().try_get_matches() {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind;
            match e.kind() {
                // 2. Invalid subcommand -> Full Help + Exit 2
                ErrorKind::InvalidSubcommand | ErrorKind::UnknownArgument => {
                    if args_raw.len() >= 2 && !args_raw[1].starts_with('-') {
                        eprintln!("Error: {}\n", e);
                        ZksArgs::build_command().print_help().unwrap_or_default();
                        println!();
                        std::process::exit(2);
                    }
                }
                // 3. Command specific errors -> Subcommand Help
                ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand | ErrorKind::TooFewValues | ErrorKind::ValueValidation => {
                    if args_raw.len() >= 2 {
                        let sub = &args_raw[1];
                        let mut cmd = ZksArgs::build_command();
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
    let args = ZksArgs::from_arg_matches(&matches)
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
        } => {
            let (targets, output) = resolve_freeze_args(args, read)?;
            let executor = RealSystem;

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
            };
            
            // Log info
            // println!("Freezing {:?} to {:?}", targets, options.output);
            
            engine::freeze(&targets, &options, &executor)?;
            println!("Successfully created archive: {:?}", options.output);
        }
        Commands::Unfreeze { archive_path, overwrite, skip_existing } => {
            let options = UnfreezeOptions {
                overwrite,
                skip_existing,
            };
            let executor = RealSystem;
            engine::unfreeze(&archive_path, &options, &executor)?;
            println!("Unfreeze completed successfully.");
        }
        Commands::Check { archive_path, use_cmp, force_delete } => {
            let executor = RealSystem;
            let options = engine::CheckOptions {
                use_cmp,
                force_delete,
            };
            engine::check(&archive_path, &options, &executor)?;
            println!("Check completed successfully.");
        }
    }
    
    Ok(())
}

fn resolve_freeze_args(mut args: Vec<PathBuf>, read_file: Option<PathBuf>) -> Result<(Vec<PathBuf>, PathBuf)> {
    // Logic: 
    // Last argument is Output Path (Archive).
    // Preceding arguments are Targets.
    // If -r file provided, read lines and add to Targets.
    
    // 1. Determine Output Path
    if args.is_empty() {
        return Err(anyhow!("Destination archive path is required"));
    }
    
    let output_path = args.pop().unwrap(); // Last one
    
    // 2. Collect Targets
    let mut targets = args; // The rest are targets
    
    // 3. Read from file if provided
    if let Some(path) = read_file {
        let content = fs::read_to_string(&path)
            .context(format!("Failed to read target list file {:?}", path))?;
            
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                targets.push(PathBuf::from(trimmed));
            }
        }
    }
    
    if targets.is_empty() {
        return Err(anyhow!("No targets specified to freeze"));
    }
    
    // 4. Handle Output Directory case
    // squash_manager-rs now handles directory selection/autonaming.
    
    Ok((targets, output_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        ZksArgs::command().debug_assert();
    }

    #[test]
    fn test_parse_freeze_args() {
        let args = ZksArgs::parse_from(&[
            "zks",
            "freeze",
            "/home/user/data",
            "/mnt/backup/data.sqfs",
            "/mnt/backup/data.sqfs",
            "-e",
            "--read",
            "/tmp/list.txt",
            "-c", "19",
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
            }
            _ => panic!("Expected Freeze command"),
        }
    }

    #[test]
    fn test_parse_freeze_progress_flags() {
        // Test vanilla-progress
        let args = ZksArgs::parse_from(&[
            "zks", "freeze", "target", "out.sqfs", "--vanilla-progress"
        ]);
        if let Commands::Freeze { vanilla_progress, no_progress, alfa_progress, compression, .. } = args.command {
            assert!(vanilla_progress);
            assert!(!no_progress);
            assert!(!alfa_progress);
            assert_eq!(compression, None);
        } else { panic!("Wrong command"); }

        // Test no-progress
        let args = ZksArgs::parse_from(&[
            "zks", "freeze", "target", "out.sqfs", "--no-progress"
        ]);
        if let Commands::Freeze { vanilla_progress, no_progress, .. } = args.command {
            assert!(no_progress);
            assert!(!vanilla_progress);
        } else { panic!("Wrong command"); }
    }

    #[test]
    fn test_parse_check_args() {
        let args = ZksArgs::parse_from(&[
            "zks",
            "check",
            "archive.sqfs",
            "--use-cmp",
            "--force-delete",
        ]);
        match args.command {
            Commands::Check {
                archive_path,
                use_cmp,
                force_delete,
            } => {
                assert_eq!(archive_path, PathBuf::from("archive.sqfs"));
                assert!(use_cmp);
                assert!(force_delete);
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
}
