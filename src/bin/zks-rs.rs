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

impl ZksArgs {
    pub fn build_command() -> clap::Command {
        use clap::CommandFactory;
        let cmd = Self::command();
        cmd.after_help("Detailed Command Information:

  freeze [TARGETS...] [ARCHIVE_PATH] [OPTIONS]
    Offload data to a SquashFS archive (frozen state).
    Arguments:
      TARGETS...            Files or directories to freeze.
      ARCHIVE_PATH          Destination .sqfs archive path.
    Options:
      -e, --encrypt         Encrypt the archive using LUKS (via squash_manager).
      -r, --read <FILE>     Read list of targets from a file.

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
")
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Freeze data into a SquashFS archive
    Freeze {
        /// Files or directories to include in the archive
        #[arg(value_name = "TARGETS", num_args = 0..)]
        args: Vec<PathBuf>,
        
        /// Encrypt the archive using LUKS
        #[arg(short, long)]
        encrypt: bool,
        
        /// Read the list of target paths from a file
        #[arg(short, long, value_name = "FILE")]
        read: Option<PathBuf>,
    },
    /// Unfreeze (restore) data from a SquashFS archive
    Unfreeze {
        /// Path to the SquashFS archive
        #[arg(value_name = "ARCHIVE_PATH")]
        archive_path: PathBuf,
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

fn main() {
    let args_raw: Vec<String> = std::env::args().collect();

    // 1. No args -> Help + Exit 0
    if args_raw.len() <= 1 {
         ZksArgs::build_command().print_help().unwrap_or_default();
         println!();
         return;
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

    println!("ZKS started: {:?}", args);
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
            "-e",
            "--read",
            "/tmp/list.txt",
        ]);

        match args.command {
            Commands::Freeze {
                args,
                encrypt,
                read,
            } => {
                assert_eq!(args[0], PathBuf::from("/home/user/data"));
                assert_eq!(args[1], PathBuf::from("/mnt/backup/data.sqfs"));
                assert!(encrypt);
                assert_eq!(read, Some(PathBuf::from("/tmp/list.txt")));
            }
            _ => panic!("Expected Freeze command"),
        }
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
}
