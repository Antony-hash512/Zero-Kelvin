use anyhow::{Result, anyhow, Context};
use clap::Parser;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use rand::Rng;
use zero_kelvin_stazis::constants::DEFAULT_ZSTD_COMPRESSION;
use zero_kelvin_stazis::executor::{CommandExecutor, RealSystem};

#[derive(Parser, Debug)]
#[command(
    name = "squash_manager", 
    about = "Manages SquashFS archives", 
    version
)]
pub struct SquashManagerArgs {
    #[command(subcommand)]
    pub command: Commands,
}

impl SquashManagerArgs {
    pub fn build_command() -> clap::Command {
        use clap::CommandFactory;
        let cmd = Self::command();
        cmd.after_help(format!("Detailed Command Information:

  create <INPUT> [OUTPUT] [OPTIONS]
    Convert a directory or an archive into a SquashFS image.
    Arguments:
      INPUT                 Source directory or archive file.
      OUTPUT                (Optional) Path to the resulting image.
    Options:
      -e, --encrypt         Create an encrypted LUKS container.
      -c, --compression N   Zstd compression level (default: {0}).
      --no-progress         Disable variable progress bar.

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
", DEFAULT_ZSTD_COMPRESSION))
    }
}

#[derive(Debug, PartialEq)]
enum CompressionMode {
    None,
    Zstd(u32),
}

impl CompressionMode {
    fn from_level(level: u32) -> Self {
        if level == 0 {
            Self::None
        } else {
            Self::Zstd(level)
        }
    }

    fn apply_to_mksquashfs<'a>(&self, args: &mut Vec<&'a str>, temp_level: &'a str) {
        match self {
            Self::None => {
                args.push("-no-compression");
            }
            Self::Zstd(_) => {
                args.push("-comp");
                args.push("zstd");
                args.push("-Xcompression-level");
                args.push(temp_level);
            }
        }
    }

    fn get_tar2sqfs_compressor_flag(&self) -> Result<String> {
        match self {
            Self::None => Err(anyhow!("Archive repacking does not support uncompressed mode (tar2sqfs limitation)")),
            Self::Zstd(_) => Ok("-c zstd".to_string()),
        }
    }
}

#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Create a new SquashFS archive from a directory or existing archive
    Create {
        /// Path to the source directory or archive file (tar, zip, etc.)
        #[arg(value_name = "INPUT")]
        input_path: PathBuf,

        /// Path where the resulting SquashFS archive will be saved
        #[arg(value_name = "OUTPUT")]
        output_path: Option<PathBuf>,

        /// Encrypt the archive using LUKS (Not yet implemented)
        #[arg(short, long)]
        encrypt: bool,

        /// Zstd compression level
        #[arg(short, long, default_value_t = DEFAULT_ZSTD_COMPRESSION)]
        compression: u32,

        /// Disable variable progress bar
        #[arg(long)]
        no_progress: bool,
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

fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        // Safe way to set default log level if not present
    }
    env_logger::init();

    let args_raw: Vec<String> = std::env::args().collect();

    // 1. No args -> Help + Exit 0
    if args_raw.len() <= 1 {
         SquashManagerArgs::build_command().print_help()?;
         println!();
         return Ok(());
    }

    // Use try_parse_from to catch --help and handle it with build_command if necessary
    // Actually, clap's FromArgMatches trait allows us to map matches back to the struct.
    let matches = match SquashManagerArgs::build_command().try_get_matches() {
        Ok(m) => m,
        Err(e) => {
            use clap::error::ErrorKind;
            match e.kind() {
                // 2. Invalid subcommand -> Full Help + Exit 2
                ErrorKind::InvalidSubcommand | ErrorKind::UnknownArgument => {
                    if args_raw.len() >= 2 && !args_raw[1].starts_with('-') {
                        eprintln!("Error: {}\n", e);
                        SquashManagerArgs::build_command().print_help()?;
                        println!();
                        std::process::exit(2);
                    }
                }
                // 3. Command specific errors -> Subcommand Help
                ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand | ErrorKind::TooFewValues | ErrorKind::ValueValidation => {
                    if args_raw.len() >= 2 {
                        let sub = &args_raw[1];
                        let mut cmd = SquashManagerArgs::build_command();
                        if let Some(sub_cmd) = cmd.find_subcommand_mut(sub) {
                             eprintln!("Error: {}\n", e);
                             sub_cmd.print_help()?;
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
    let args = SquashManagerArgs::from_arg_matches(&matches)
        .map_err(|e| {
            e.exit();
        })
        .unwrap();

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

            // Define compression strategy
            let comp_mode = CompressionMode::from_level(compression);


            // 1. Check if input exists
            if !input_path.exists() {
                return Err(anyhow!("Input path does not exist: {:?}", input_path));
            }

            // 2. Archive Repacking (File -> SquashFS)
            if input_path.is_file() {
                let input_str = input_path.to_str().ok_or(anyhow!("Invalid input path"))?;
                let output_str = output_path
                    .as_ref()
                    .ok_or(anyhow!("Output path required"))?
                    .to_str()
                    .ok_or(anyhow!("Invalid output path"))?;

                // Determine decompressor
                let file_name = input_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let decompressor = if file_name.ends_with(".tar") {
                    "cat"
                } else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
                    "gzip -dc"
                } else if file_name.ends_with(".tar.bz2") || file_name.ends_with(".tbz2") {
                     "bzip2 -dc"
                } else if file_name.ends_with(".tar.xz") || file_name.ends_with(".txz") {
                    "xz -dc"
                } else if file_name.ends_with(".tar.zst") || file_name.ends_with(".tzst") {
                    "zstd -dc"
                } else if file_name.ends_with(".tar.zip") {
                    "unzip -p"
                } else if file_name.ends_with(".tar.7z") {
                    "7z x -so"
                } else if file_name.ends_with(".tar.rar") {
                    "unrar p -inul"
                } else {
                    return Err(anyhow!("Unsupported archive format: {}", file_name));
                };

                // Determine compressor flag for tar2sqfs
                let compressor_flag = comp_mode.get_tar2sqfs_compressor_flag()?;

                // Construct pipeline: decompressor input | tar2sqfs options output
                // Using explicit quoting for paths to handle spaces safely in sh -c
                // Fixed: Do not pass compression level to -j (threads), use -c <compressor>
                let cmd = format!(
                    "{} '{}' | tar2sqfs --quiet --no-skip --force {} '{}'",
                    decompressor,
                    input_str.replace("'", "'\\''"), // Escape single quotes in path
                    compressor_flag,
                    output_str.replace("'", "'\\''")
                );

                if std::env::var("RUST_LOG").is_ok() {
                    eprintln!("DEBUG: Executing pipeline: {}", cmd);
                }

                let output = executor.run("sh", &["-c", &cmd])?;

                if !output.status.success() {
                     let stderr = String::from_utf8_lossy(&output.stderr);
                     return Err(anyhow!("Archive repack failed: {}", stderr));
                }
                
                return Ok(());
            }

            // 3. Standard Directory Packing (Directory -> SquashFS)
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

            let comp_level_str = compression.to_string();
            comp_mode.apply_to_mksquashfs(&mut cmd_args, &comp_level_str);

            let output = executor.run("mksquashfs", &cmd_args)?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("mksquashfs failed: {}", stderr));
            }

            Ok(())
        }
        Commands::Mount { image, mount_point } => {
            if !image.exists() {
                return Err(anyhow!("Image file does not exist: {:?}", image));
            }

            let target_mount_point = match mount_point {
                Some(path) => path,
                None => {
                    // Auto-generate mount point
                    let prefix = image.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("sqfs_image");
                    
                    let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    
                    // Simple random suffix to avoid collisions
                    // rand 0.9 usage
                    let random_suffix: u32 = rand::rng().random_range(100000..999999);
                    
                    let dir_name = format!("{}_{}_{}", prefix, timestamp, random_suffix);
                    let path = env::current_dir()?.join(dir_name);
                    
                    println!("No mount point specified. Using auto-generated path: {}", path.display());
                    path
                }
            };
            
            fs::create_dir_all(&target_mount_point).context("Failed to create mount point")?;
            
            let mp_str = target_mount_point.to_str().ok_or(anyhow!("Invalid mount point path"))?;
            let img_str = image.to_str().ok_or(anyhow!("Invalid image path"))?;
            
            // Added -o nonempty to allow mounting over non-empty directories (if user desires/auto-gen collision)
            // This fixes BATS tests where we test "keep dir" scenarios or if auto-gen collides (rarely).
            let output = executor.run("squashfuse", &["-o", "nonempty", img_str, mp_str])?;
            
             if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("squashfuse failed: {}", stderr));
            }
            
            Ok(())
        },

        Commands::Umount { mount_point } => {
            let path = &mount_point;
            
            if !path.exists() {
                 // return Err(anyhow!("Path does not exist: {:?}", path));
                 // Relax check: if user passed a file path that *used* to exist but maybe was deleted?
                 // But requirements say "path to image". If image doesn't exist, we can't be sure what they meant.
                 return Err(anyhow!("Path does not exist: {:?}", path));
            }

            let mut targets = Vec::new();

            if path.is_dir() {
                targets.push(path.clone());
            } else if path.is_file() {
                // It's an image file. Find matching squashfuse processes.
                let abs_path = fs::canonicalize(path)
                    .context("Failed to canonicalize image path")?;
                let abs_path_str = abs_path.to_str().unwrap_or("");
                
                if std::env::var("RUST_LOG").is_ok() {
                    eprintln!("DEBUG: Scanning processes for image: '{}'", abs_path_str);
                }

                // Iterate over /proc
                let proc_dir = fs::read_dir("/proc").context("Failed to read /proc")?;
                
                for entry in proc_dir {
                    if let Ok(entry) = entry {
                        let file_name = entry.file_name();
                        let file_name_str = file_name.to_str().unwrap_or("");
                        
                        // Check if it's a PID (all digits)
                        if file_name_str.chars().all(|c| c.is_ascii_digit()) {
                             let cmdline_path = entry.path().join("cmdline");
                             if let Ok(cmdline) = fs::read_to_string(cmdline_path) {
                                 // cmdline is null-separated
                                 let args: Vec<&str> = cmdline.split('\0').collect();
                                 
                                 if args.is_empty() { continue; }
                                 
                                 // Check if process name contains squashfuse
                                 let prog_name = args[0];
                                 if prog_name.contains("squashfuse") {
                                     // Look for the image path in arguments
                                     // squashfuse [options] IMAGE MOUNTPOINT
                                     
                                     for (i, arg) in args.iter().enumerate() {
                                         if *arg == abs_path_str {
                                             if i + 1 < args.len() {
                                                 let potential_mount = args[i+1];
                                                 if !potential_mount.starts_with('-') && !potential_mount.is_empty() {
                                                     if std::env::var("RUST_LOG").is_ok() {
                                                         eprintln!("DEBUG: Found match! pid {} mountpoint '{}'", file_name_str, potential_mount);
                                                     }
                                                     targets.push(PathBuf::from(potential_mount));
                                                 }
                                             }
                                         }
                                     }
                                 }
                             }
                        }
                    }
                }
                
                if targets.is_empty() {
                    return Err(anyhow!("Image is not mounted (no squashfuse process found): {:?}", path));
                }
            } else {
                 return Err(anyhow!("Path is neither file nor directory: {:?}", path));
            }
            
            for target in targets {
                let target_str = target.to_str().ok_or(anyhow!("Invalid target path"))?;
                
                let output = executor.run("fusermount", &["-u", target_str])?;
                
                if !output.status.success() {
                     let stderr = String::from_utf8_lossy(&output.stderr);
                     return Err(anyhow!("fusermount failed for {:?}: {}", target, stderr));
                }
                
                // Post-unmount cleanup: remove directory if empty
                let _ = fs::remove_dir(&target);
            }

            Ok(())
        }
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
    } // Added closing brace
    #[test]
    fn test_mount_auto_gen_path() {
        // We can't easily mock env::current_dir or SystemTime in this simple setup without more refactoring/creates.
        // But we can verify that the logic *would* generate a path if mount_point is None.
        // Actually, we can test `run` with `mount_point: None` and a mock executor.
        
        // Use a real file for image to pass .exists() check
        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("test.sqfs");
        fs::write(&image_path, "dummy data").unwrap();
        let image_path_str = image_path.to_str().unwrap().to_string();

        let mut mock = MockCommandExecutor::new();
        
        mock.expect_run()
            .withf(move |program, args| {
                program == "squashfuse" &&
                args.len() == 4 && // -o nonempty image mountpoint
                args[0] == "-o" &&
                args[1] == "nonempty" &&
                args[2] == image_path_str
                // args[3] is the auto-generated path, hard to match exact string due to randomness/time
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));
            
        let args = SquashManagerArgs {
            command: Commands::Mount {
                image: image_path,
                mount_point: None,
            },
        };
        
        // This will create a directory in CWD. We should clean it up?
        // The integration tests handle this better. 
        // For unit test, we might dirty the CWD if we are not careful.
        // Let's rely on integration tests for the side-effects (dir creation) 
        // OR refactor `run` to take a "PathGenerator" trait? 
        // Overkill for now. 
        
        // Let's skip dirtying CWD in unit test by running it in a temp CWD?
        // Valid strategy: change CWD for the test.
        let orig_cwd = env::current_dir().unwrap();
        let test_cwd = tempfile::tempdir().unwrap();
        env::set_current_dir(&test_cwd).unwrap();
        
        let result = run(args, &mock);
        
        // Restore CWD
        env::set_current_dir(&orig_cwd).unwrap();
        
        assert!(result.is_ok());
    }


    #[test]
    fn test_compression_mode_logic() {
        // Test None
        let mode_none = CompressionMode::from_level(0);
        assert_eq!(mode_none, CompressionMode::None);
        
        let mut args = vec![];
        let dummy_level = "0";
        mode_none.apply_to_mksquashfs(&mut args, dummy_level);
        assert_eq!(args, vec!["-no-compression"]);

        assert!(mode_none.get_tar2sqfs_compressor_flag().is_err());

        // Test Zstd
        let mode_zstd = CompressionMode::from_level(15);
        assert_eq!(mode_zstd, CompressionMode::Zstd(15));
        
        let mut args2 = vec![];
        let level_str = "15";
        mode_zstd.apply_to_mksquashfs(&mut args2, level_str);
        assert_eq!(args2, vec!["-comp", "zstd", "-Xcompression-level", "15"]);

        assert_eq!(mode_zstd.get_tar2sqfs_compressor_flag().unwrap(), "-c zstd");
    }

    #[test]
    fn test_create_directory_with_no_compression() {
        let temp_dir = tempfile::tempdir().unwrap();
        let input_path = temp_dir.path().to_path_buf();
        let input_path_check = input_path.to_str().unwrap().to_string();

        let mut mock = MockCommandExecutor::new();
        // Expectation: mksquashfs input output -no-progress -no-compression
        mock.expect_run()
            .withf(move |program, args| {
                 program == "mksquashfs" &&
                 args.len() == 4 && // input, output, -no-progress, -no-compression
                 args[0] == input_path_check &&
                 args[1] == "output_no_comp.sqfs" &&
                 args[2] == "-no-progress" &&
                 args[3] == "-no-compression"
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
                output_path: Some(PathBuf::from("output_no_comp.sqfs")),
                encrypt: false,
                compression: 0,
                no_progress: true,
            },
        };

        run(args, &mock).unwrap();
    }
}
