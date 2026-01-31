use anyhow::{Result, anyhow};
use clap::Parser;
use std::path::PathBuf;
use zero_kelvin_stazis::constants::DEFAULT_ZSTD_COMPRESSION;
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
        #[arg(short, long, default_value_t = DEFAULT_ZSTD_COMPRESSION)]
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
        Commands::Create {
            input_path,
            output_path,
            encrypt,
            compression,
            no_progress,
        } => {
            if encrypt {
                return Err(anyhow!("Encryption support will be added in Stage 4"));
            }

            if !input_path.exists() {
                return Err(anyhow!("Input path does not exist: {:?}", input_path));
            }

            let mut cmd_args = vec![
                input_path.to_str().ok_or(anyhow!("Invalid input path"))?,
                output_path
                    .as_ref()
                    .ok_or(anyhow!("Output path required"))?
                    .to_str()
                    .ok_or(anyhow!("Invalid output path"))?,
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
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Output;
    // use zero_kelvin_stazis::executor::MockCommandExecutor; // Not visible/available
    use mockall::predicate::*;
    use mockall::mock;

    // Define the mock locally for the binary tests
    mock! {
        pub CommandExecutor {}
        impl CommandExecutor for CommandExecutor {
            fn run<'a>(&self, program: &str, args: &[&'a str]) -> Result<Output>;
        }
    }


    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        SquashManagerArgs::command().debug_assert();
    }

    #[test]
    fn test_create_plain_archive() {
        // Create a temp directory so input_path.exists() passes
        let temp_dir = tempfile::tempdir().unwrap();
        let input_path = temp_dir.path().to_path_buf();
        let input_path_str = input_path.to_str().unwrap();
        let input_path_check = input_path_str.to_string();

        let mut mock = MockCommandExecutor::new();
        // Expectation: mksquashfs input_dir output.sqfs -no-progress -comp zstd -Xcompression-level <DEFAULT_ZSTD_COMPRESSION>
        mock.expect_run()
            .withf(move |program, args| {
                 program == "mksquashfs" &&
                 args.len() == 7 &&
                 args[0] == input_path_check &&
                 args[1] == "output.sqfs" &&
                 args[2] == "-no-progress" &&
                 args[3] == "-comp" &&
                 args[4] == "zstd" &&
                 args[5] == "-Xcompression-level" &&
                 args[6] == DEFAULT_ZSTD_COMPRESSION.to_string()
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path: input_path,
                output_path: Some(PathBuf::from("output.sqfs")),
                encrypt: false,
                compression: DEFAULT_ZSTD_COMPRESSION,
                no_progress: true,
            },
        };

        run(args, &mock).unwrap();
    }

    #[test]
    fn test_create_with_encryption_flag_fails() {
        let mock = MockCommandExecutor::new();
        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path: PathBuf::from("input_dir"),
                output_path: Some(PathBuf::from("output.sqfs")),
                encrypt: true,
                compression: DEFAULT_ZSTD_COMPRESSION,
                no_progress: false,
            },
        };

        // This should fail
        let result = run(args, &mock);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Encryption support will be added in Stage 4"
        );
    }
}
