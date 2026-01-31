use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "zks", about = "Zero Kelvin Store", version)]
pub struct ZksArgs {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Freeze {
        #[arg(value_name = "ARGS", num_args = 1..)]
        args: Vec<PathBuf>,
        #[arg(short, long)]
        encrypt: bool,
        #[arg(short, long)]
        read: Option<PathBuf>,
    },
    Unfreeze {
        archive_path: PathBuf,
    },
    Check {
        archive_path: PathBuf,
        #[arg(long)]
        use_cmp: bool,
        #[arg(long)]
        force_delete: bool,
    },
}

fn main() {
    let args = ZksArgs::parse();

    // Skeleton logic
    // env_logger::init(); // removed for now to avoid unsafe in tests or if strictly needed I'd put it in a function.
    // For now simple println is enough for skeleton

    // log::info!("Started zks with args: {:?}", args);
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
