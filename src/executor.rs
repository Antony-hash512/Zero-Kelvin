use anyhow::{Context, Result};
use indicatif::ProgressBar;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use std::thread;
use std::fs;

/// Abstraction for running system commands.
#[cfg_attr(test, mockall::automock)]
pub trait CommandExecutor {
    /// Runs a command synchronously and captures output.
    fn run<'a>(&self, program: &str, args: &[&'a str]) -> Result<Output>;

    /// Runs a command interactively (inherits stdio).
    fn run_interactive<'a>(&self, program: &str, args: &[&'a str]) -> Result<std::process::ExitStatus>;

    /// Runs a command while monitoring output file size for progress.
    /// Updates the progress bar with the current size of the output file.
    fn run_with_file_progress<'a>(
        &self,
        program: &str,
        args: &[&'a str],
        output_file: &Path,
        progress_bar: &ProgressBar,
        poll_interval: Duration,
    ) -> Result<Output>;
}

/// Real system executor using std::process::Command.
pub struct RealSystem;

impl CommandExecutor for RealSystem {
    fn run<'a>(&self, program: &str, args: &[&'a str]) -> Result<Output> {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("Failed to execute command: {} {:?}", program, args))
    }

    fn run_interactive<'a>(&self, program: &str, args: &[&'a str]) -> Result<std::process::ExitStatus> {
        Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("Failed to execute interactive command: {} {:?}", program, args))
    }

    fn run_with_file_progress<'a>(
        &self,
        program: &str,
        args: &[&'a str],
        output_file: &Path,
        progress_bar: &ProgressBar,
        poll_interval: Duration,
    ) -> Result<Output> {
        // Spawn the command asynchronously
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn command: {} {:?}", program, args))?;

        // Monitor file size in a loop until process exits
        loop {
            // Check if process has exited
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Process finished, get final output
                    break;
                }
                Ok(None) => {
                    // Still running, update progress
                    if let Ok(meta) = fs::metadata(output_file) {
                        progress_bar.set_position(meta.len());
                    }
                    thread::sleep(poll_interval);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Error waiting for process: {}", e));
                }
            }
        }

        // Final position update
        if let Ok(meta) = fs::metadata(output_file) {
            progress_bar.set_position(meta.len());
        }

        // Get the output
        let output = child.wait_with_output()
            .with_context(|| format!("Failed to get output from command: {} {:?}", program, args))?;
        
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn test_mock_system_strict_args() {
        let mut mock = MockCommandExecutor::new();

        // Expect: mksquashfs /source /target -comp zstd
        mock.expect_run()
            .withf(|program, args| {
                program == "mksquashfs" && args == &["/source", "/target", "-comp", "zstd"]
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"OK".to_vec(),
                stderr: b"".to_vec(),
            }));

        // Test implementation usage
        let res = mock
            .run("mksquashfs", &["/source", "/target", "-comp", "zstd"])
            .unwrap();
        assert!(res.status.success());
        assert_eq!(res.stdout, b"OK");
    }

    #[test]
    #[should_panic]
    fn test_mock_system_wrong_args() {
        let mut mock = MockCommandExecutor::new();
        
        mock.expect_run()
            .withf(|program, args| {
                 program == "ls" && args == &["-la"]
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        // Should panic because args don't match (expected -la, got -l)
        let _ = mock.run("ls", &["-l"]);
    }
}
