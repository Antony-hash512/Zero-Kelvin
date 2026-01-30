use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "squash_manager", about = "Manages SquashFS archives", version)]
pub struct SquashManagerArgs {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Create {
        #[arg(value_name = "INPUT")]
        input_path: PathBuf,
        #[arg(value_name = "OUTPUT")]
        output_path: Option<PathBuf>,
        #[arg(short, long)]
        encrypt: bool,
        #[arg(short, long, default_value_t = 15)]
        compression: u32,
        #[arg(long)]
        no_progress: bool,
    },
    Mount {
        image: PathBuf,
        mount_point: PathBuf,
    },
    Umount {
        mount_point: PathBuf,
    },
}

fn main() {
    let args = SquashManagerArgs::parse();
    // Logic will go here later
    // For now we just print debug info to satisfy "skeleton" phase requirements
    // std::env::set_var("RUST_LOG", "info"); // Unsafe in 2024 edition, relying on external env var layer
    env_logger::init();
    
    log::info!("Started with args: {:?}", args);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        SquashManagerArgs::command().debug_assert();
    }

    #[test]
    fn test_parse_create_args() {
        let args = SquashManagerArgs::parse_from(&[
            "squash_manager", "create", 
            "input_dir", 
            "output_archive.sqfs", 
            "-e", 
            "-c", "22"
        ]);

        match args.command {
            Commands::Create { input_path, output_path, encrypt, compression, no_progress } => {
                assert_eq!(input_path, PathBuf::from("input_dir"));
                assert_eq!(output_path, Some(PathBuf::from("output_archive.sqfs")));
                assert!(encrypt);
                assert_eq!(compression, 22);
                assert!(!no_progress);
            },
            _ => panic!("Expected Create command"),
        }
    }
}
