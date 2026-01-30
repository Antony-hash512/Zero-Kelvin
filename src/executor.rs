use anyhow::{Result, Context};
use std::process::{Command, Output, Stdio};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Abstraction for running system commands.
pub trait CommandExecutor {
    /// Runs a command synchronously and captures output.
    fn run(&self, program: &str, args: &[&str]) -> Result<Output>;
}

/// Real system executor using std::process::Command.
pub struct RealSystem;

impl CommandExecutor for RealSystem {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
         Command::new(program)
            .args(args)
            .stdin(Stdio::null()) 
            .output()
            .with_context(|| format!("Failed to execute command: {} {:?}", program, args))
    }
}

/// Mock entry for storing expectations.
#[derive(Clone, Debug)]
struct ExpectedCommand {
    program: String,
    args: Vec<String>,
    output: Output,
}

/// Mock system for testing.
#[derive(Clone, Default)]
pub struct MockSystem {
    expectations: Arc<Mutex<VecDeque<ExpectedCommand>>>,
}

impl MockSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// sets up an expectation for a command call.
    pub fn expect(&mut self, program: &str, args: &[&str]) -> MockExpectationBuilder {
        MockExpectationBuilder {
            parent: self.clone(),
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }
}

pub struct MockExpectationBuilder {
    parent: MockSystem,
    program: String,
    args: Vec<String>,
}

impl MockExpectationBuilder {
    pub fn returns(self, output: Output) {
        let mut queues = self.parent.expectations.lock().unwrap();
        queues.push_back(ExpectedCommand {
            program: self.program,
            args: self.args,
            output,
        });
    }
}

impl CommandExecutor for MockSystem {
    fn run(&self, program: &str, args: &[&str]) -> Result<Output> {
        let mut queues = self.expectations.lock().unwrap();
        
        if let Some(expected) = queues.pop_front() {
            // Strict checking of program name
            if expected.program != program {
                panic!(
                    "MockSystem: Unexpected program. Expected '{}', got '{}'",
                    expected.program, program
                );
            }
            // Strict checking of arguments
            let current_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            if expected.args != current_args {
                panic!(
                    "MockSystem: Unexpected args for '{}'. Expected {:?}, got {:?}",
                    program, expected.args, current_args
                );
            }
            
            Ok(expected.output)
        } else {
             panic!("MockSystem: Unexpected command call (queue empty): {} {:?}", program, args);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn test_mock_system_strict_args() {
        let mut mock = MockSystem::new();
        // Expect: mksquashfs /source /target -comp zstd
        mock.expect("mksquashfs", &["/source", "/target", "-comp", "zstd"])
            .returns(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"OK".to_vec(),
                stderr: b"".to_vec(),
            });

        // Test implementation usage
        let res = mock.run("mksquashfs", &["/source", "/target", "-comp", "zstd"]).unwrap();
        assert!(res.status.success());
        assert_eq!(res.stdout, b"OK");
    }
    
    #[test]
    #[should_panic(expected = "Unexpected args")]
    fn test_mock_system_wrong_args() {
        let mut mock = MockSystem::new();
        mock.expect("ls", &["-la"]).returns(Output {
             status: std::process::ExitStatus::from_raw(0),
             stdout: vec![],
             stderr: vec![],
        });
        
        // Should panic because args don't match
        let _ = mock.run("ls", &["-l"]); 
    }
}
