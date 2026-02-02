use anyhow::{Context, Result};
use std::process::{Command, Output, Stdio};

/// Abstraction for running system commands.
#[cfg_attr(test, mockall::automock)]
pub trait CommandExecutor {
    /// Runs a command synchronously and captures output.
    fn run<'a>(&self, program: &str, args: &[&'a str]) -> Result<Output>;

    /// Runs a command interactively (inherits stdio).
    fn run_interactive<'a>(&self, program: &str, args: &[&'a str]) -> Result<std::process::ExitStatus>;
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
