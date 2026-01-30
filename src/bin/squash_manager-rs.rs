use clap::Parser;
use std::path::PathBuf;
use anyhow::{Result, anyhow};
use zero_kelvin_stazis::executor::{CommandExecutor, RealSystem};

#[derive(Parser, Debug)]
#[command(name = "squash_manager", about = "Manages SquashFS archives", version)]
pub struct SquashManagerArgs {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(clap::Subcommand, Debug)]
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

fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        // Safe way to set default log level if not present, avoiding unsafe set_var if possible, 
        // but for CLI tool usually we rely on env_logger default behavior or just init.
        // For now just init.
    }
    env_logger::init();
    
    let args = SquashManagerArgs::parse();
    let executor = RealSystem;
    
    run(args, &executor)
}

/// Main logic entry point with dependency injection
pub fn run(args: SquashManagerArgs, executor: &impl CommandExecutor) -> Result<()> {
    match args.command {
        Commands::Create { input_path, output_path, encrypt, compression, no_progress } => {
            if encrypt {
                return Err(anyhow!("Encryption support will be added in Stage 4"));
            }

            if !input_path.exists() {
                return Err(anyhow!("Input path does not exist: {:?}", input_path));
            }

            let mut cmd_args = vec![
                input_path.to_str().ok_or(anyhow!("Invalid input path"))?,
                output_path.as_ref().ok_or(anyhow!("Output path required"))?.to_str().ok_or(anyhow!("Invalid output path"))?,
            ];

            if no_progress {
                cmd_args.push("-no-progress");
            }
            
            cmd_args.push("-comp");
            cmd_args.push("zstd");
            
            cmd_args.push("-Xcompression-level");
            let comp_level_str = compression.to_string();
            cmd_args.push(&comp_level_str);

            let output = executor.run("mksquashfs", &cmd_args)?;
            
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("mksquashfs failed: {}", stderr));
            }

            Ok(())
        },
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zero_kelvin_stazis::executor::MockSystem;
    use std::process::Output;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        SquashManagerArgs::command().debug_assert();
    }

    #[test]
    fn test_create_plain_archive() {
        let mut mock = MockSystem::new();
        // Expectation: mksquashfs input_dir output.sqfs -no-progress -comp zstd -Xcompression-level 15
        mock.expect("mksquashfs", &[
            "input_dir", 
            "output.sqfs", 
            "-no-progress",
            "-comp", "zstd", 
            "-Xcompression-level", "15"
        ]).returns(Output {
            status: std::process::ExitStatus::from_raw(0),
            stdout: vec![],
            stderr: vec![],
        });

        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path: PathBuf::from("input_dir"),
                output_path: Some(PathBuf::from("output.sqfs")),
                encrypt: false,
                compression: 15,
                no_progress: true,
            }
        };

        run(args, &mock).unwrap();
        
        mock.verify_complete();
    }

    #[test]
    fn test_create_with_encryption_flag_fails() {
        let mock = MockSystem::new();
        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path: PathBuf::from("input_dir"),
                output_path: Some(PathBuf::from("output.sqfs")),
                encrypt: true,
                compression: 15,
                no_progress: false,
            }
        };

        // This should fail
        let result = run(args, &mock);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Encryption support will be added in Stage 4");
    }
}
