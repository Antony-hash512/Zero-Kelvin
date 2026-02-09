use rand::Rng;
use std::path::{Path, PathBuf};
use std::fs;
use zero_kelvin::cli::zk::{Args, Commands};
use zero_kelvin::engine::{self, FreezeOptions, UnfreezeOptions};
use zero_kelvin::error::ZkError;
use zero_kelvin::executor::RealSystem;
use zero_kelvin::logging;
use zero_kelvin::utils;

fn main() -> std::process::ExitCode {
    // Initialize tracing with file rotation (guard must be kept alive)
    let _log_guard = logging::init_logging();

    match run_app() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(ZkError::CliExit(code)) => std::process::ExitCode::from(code),
        Err(e) => {
            if let Some(friendly) = e.friendly_message() {
                eprintln!("Suggestion: {}", friendly);
            }
            eprintln!("Error: {}", e);
            std::process::ExitCode::FAILURE
        }
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
                        return Err(ZkError::CliExit(2));
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
                            return Err(ZkError::CliExit(e.exit_code() as u8));
                        }
                    }
                }
                // --help, --version: clap prints output, return OK
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                    let _ = e.print();
                    return Ok(());
                }
                _ => {}
            }
            // Fallback: print and return with clap's exit code
            let code = e.exit_code() as u8;
            let _ = e.print();
            return Err(ZkError::CliExit(code));
        }
    };

    use clap::FromArgMatches;
    let args = Args::from_arg_matches(&matches).map_err(|e| {
        let code = e.exit_code() as u8;
        let _ = e.print();
        ZkError::CliExit(code)
    })?;

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

            // Validate compression level
            if let Some(level) = compression {
                if level > 22 {
                    return Err(ZkError::CompressionError(format!(
                        "Invalid compression level: {}. Zstd supports levels 0-22 (0 = no compression).",
                        level
                    )));
                }
            }

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
            force_unfreeze,
            verify,
        } => {
            let options = UnfreezeOptions {
                overwrite,
                skip_existing,
                force_unfreeze,
                verify,
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

    let output_path = args.pop().ok_or_else(|| {
        ZkError::MissingTarget("Destination archive path is required".into())
    })?;

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
