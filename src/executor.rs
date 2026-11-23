use indicatif::ProgressBar;
use regex::Regex;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use std::thread;
use std::fs;

/// Abstraction for running system commands.
#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
pub trait CommandExecutor {
    /// Checks whether an external executable is available in PATH.
    fn command_exists(&self, program: &str) -> bool;

    /// Runs a command synchronously and captures output.
    fn run<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<Output>;

    /// Runs a command interactively (inherits stdio).
    /// Runs a command interactively (inherits stdio).
    /// Note: Cannot capture stderr for friendly error messages easily while inheriting.
    /// To support friendly errors, we might need a mode that pipes stderr but streams stdout.
    fn run_interactive<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<std::process::ExitStatus>;

    /// Runs a command, piping stdout/stderr to parent but CAPTURING stderr if it fails.
    /// Returns (ExitStatus, Option<StderrString>)
    fn run_and_capture_error<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<(std::process::ExitStatus, String)>;

    /// Runs a command while monitoring output file size for progress.
    /// Updates the progress bar with the current size of the output file.
    fn run_with_file_progress<'a>(
        &self,
        program: &str,
        args: &[&'a str],
        output_file: &Path,
        progress_bar: &ProgressBar,
        poll_interval: Duration,
    ) -> std::io::Result<Output>;

    /// Runs a command while parsing stdout for progress percentage.
    /// Looks for patterns like "45%" in stdout and updates the progress bar (0-100 scale).
    fn run_with_stdout_progress<'a>(
        &self,
        program: &str,
        args: &[&'a str],
        progress_bar: &ProgressBar,
    ) -> std::io::Result<Output>;

    /// Runs a native two-stage pipeline (`stage1 | stage2`) without a shell.
    /// Returns a non-zero exit status if either stage fails (portable `pipefail`).
    fn run_pipeline<'a>(
        &self,
        stage1_prog: &str,
        stage1_args: &[&'a str],
        stage2_prog: &str,
        stage2_args: &[&'a str],
    ) -> std::io::Result<Output>;

    /// Runs a native two-stage pipeline (`stage1 | stage2`) while polling `output_file` for progress.
    /// Returns a non-zero exit status if either stage fails (portable `pipefail`).
    fn run_pipeline_with_file_progress<'a>(
        &self,
        stage1_prog: &str,
        stage1_args: &[&'a str],
        stage2_prog: &str,
        stage2_args: &[&'a str],
        output_file: &Path,
        progress_bar: &ProgressBar,
        poll_interval: Duration,
    ) -> std::io::Result<Output>;
}

fn execute_native_pipeline(
    stage1_prog: &str,
    stage1_args: &[&str],
    stage2_prog: &str,
    stage2_args: &[&str],
    progress: Option<(&Path, &ProgressBar, Duration)>,
) -> std::io::Result<Output> {
    use std::io::Read;

    let mut stage1 = Command::new(stage1_prog)
        .args(stage1_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to spawn {}: {}", stage1_prog, e),
            )
        })?;

    let stage1_stdout = stage1.stdout.take().ok_or_else(|| {
        let _ = stage1.kill();
        let _ = stage1.wait();
        std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stage1 stdout")
    })?;

    let stage1_stderr_pipe = stage1.stderr.take();
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = stage1_stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let mut stage2 = match Command::new(stage2_prog)
        .args(stage2_args)
        .stdin(Stdio::from(stage1_stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = stage1.kill();
            let _ = stage1.wait();
            let _ = stderr_thread.join();
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to spawn {}: {}", stage2_prog, e),
            ));
        }
    };

    if let Some((output_file, progress_bar, poll_interval)) = progress {
        loop {
            match stage2.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if let Ok(meta) = fs::metadata(output_file) {
                        progress_bar.set_position(meta.len());
                    }
                    thread::sleep(poll_interval);
                }
                Err(e) => {
                    let _ = stage1.kill();
                    let _ = stage1.wait();
                    let _ = stderr_thread.join();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Error waiting for {}: {}", stage2_prog, e),
                    ));
                }
            }
        }
        if let Ok(meta) = fs::metadata(output_file) {
            progress_bar.set_position(meta.len());
        }
    }

    let stage2_output = stage2.wait_with_output().map_err(|e| {
        let _ = stage1.kill();
        let _ = stage1.wait();
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to wait for {}: {}", stage2_prog, e),
        )
    })?;

    let stage1_status = stage1.wait().map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to wait for {}: {}", stage1_prog, e),
        )
    })?;

    let mut combined_stderr = stderr_thread.join().unwrap_or_default();
    combined_stderr.extend_from_slice(&stage2_output.stderr);

    let final_status = if !stage2_output.status.success() {
        stage2_output.status
    } else if !stage1_status.success() {
        stage1_status
    } else {
        stage2_output.status
    };

    Ok(Output {
        status: final_status,
        stdout: stage2_output.stdout,
        stderr: combined_stderr,
    })
}

/// Real system executor using std::process::Command.
pub struct RealSystem;

impl CommandExecutor for RealSystem {
    fn command_exists(&self, program: &str) -> bool {
        which::which(program).is_ok()
    }

    fn run<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<Output> {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to execute command: {} {:?}: {}", program, args, e)))
    }

    fn run_interactive<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<std::process::ExitStatus> {
        Command::new(program)
            .args(args)
            .status()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to execute interactive command: {} {:?}: {}", program, args, e)))
    }

    fn run_and_capture_error<'a>(&self, program: &str, args: &[&'a str]) -> std::io::Result<(std::process::ExitStatus, String)> {
        // We want user to see output LIVE. 
        // But we also want to capture stderr if it fails.
        // Tapping into the stream is hard without complex threading.
        // 
        // Compromise: Use `Output` capture if we suspect it might fail? No, interactive commands like LUKS need stdin/stdout.
        //
        // If we strictly need to catch "Incorrect password" from cryptsetup, it prints to stderr.
        // If we redirect stderr to Pipe, we hide it from user (unless we reprint).
        
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::inherit())  // Allow password input
            .stdout(Stdio::inherit()) // Show progress
            .stderr(Stdio::piped())   // Capture stderr
            .spawn()?;

        let stderr_pipe = child.stderr.take()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stderr pipe"))?;
        
        // We need to read stderr in a thread or loop to avoid blocking? 
        // Or just read to string since stderr volume is usually low for prompts?
        // But if we block reading stderr, we might block the process if it writes too much.
        // Better: Use a thread to tee stderr to user + string.
        
        let (tx, rx) = std::sync::mpsc::channel();
        
        let t = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut reader = BufReader::new(stderr_pipe);
            let mut buffer = [0; 1024];
            let mut captured = Vec::new();
            
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let chunk = &buffer[0..n];
                        // Passthrough to real stderr
                        let _ = std::io::stderr().write_all(chunk);
                        captured.extend_from_slice(chunk);
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(captured);
        });

        let status = child.wait()?;
        let _ = t.join(); // Wait for thread
        let captured_bytes = rx.recv().unwrap_or_default();
        let captured_string = String::from_utf8_lossy(&captured_bytes).to_string();
        
        Ok((status, captured_string))
    }

    fn run_with_file_progress<'a>(
        &self,
        program: &str,
        args: &[&'a str],
        output_file: &Path,
        progress_bar: &ProgressBar,
        poll_interval: Duration,
    ) -> std::io::Result<Output> {
        // Spawn the command asynchronously
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to spawn command: {} {:?}: {}", program, args, e)))?;

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
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, format!("Error waiting for process: {}", e)));
                }
            }
        }

        // Final position update
        if let Ok(meta) = fs::metadata(output_file) {
            progress_bar.set_position(meta.len());
        }

        // Get the output
        let output = child.wait_with_output()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to get output from command: {} {:?}: {}", program, args, e)))?;
        
        Ok(output)
    }

    fn run_with_stdout_progress<'a>(
        &self,
        program: &str,
        args: &[&'a str],
        progress_bar: &ProgressBar,
    ) -> std::io::Result<Output> {
        // Regex to find percentage like "45%" or "100%"
        let percent_re = Regex::new(r"(\d+)%")
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Regex error: {}", e)))?;
        
        // Spawn the command with piped stdout
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to spawn command: {} {:?}: {}", program, args, e)))?;

        // Take stdout handle for reading
        let stdout = child.stdout.take()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "Failed to capture stdout"))?;
        
        let reader = BufReader::new(stdout);
        
        // Read stdout line by line, parse percentage
        for line in reader.lines() {
            if let Ok(line_str) = line {
                // Find last percentage in line (mksquashfs outputs "[===...] 1/2 50%")
                if let Some(caps) = percent_re.captures_iter(&line_str).last() {
                    if let Some(pct_match) = caps.get(1) {
                        if let Ok(pct) = pct_match.as_str().parse::<u64>() {
                            progress_bar.set_position(pct);
                        }
                    }
                }
            }
        }

        // Wait for process to finish and collect stderr
        let output = child.wait_with_output()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to get output from command: {} {:?}: {}", program, args, e)))?;
        
        // Final update to 100% if successful
        if output.status.success() {
            progress_bar.set_position(100);
        }
        
        Ok(output)
    }

    fn run_pipeline<'a>(
        &self,
        stage1_prog: &str,
        stage1_args: &[&'a str],
        stage2_prog: &str,
        stage2_args: &[&'a str],
    ) -> std::io::Result<Output> {
        execute_native_pipeline(stage1_prog, stage1_args, stage2_prog, stage2_args, None)
    }

    fn run_pipeline_with_file_progress<'a>(
        &self,
        stage1_prog: &str,
        stage1_args: &[&'a str],
        stage2_prog: &str,
        stage2_args: &[&'a str],
        output_file: &Path,
        progress_bar: &ProgressBar,
        poll_interval: Duration,
    ) -> std::io::Result<Output> {
        execute_native_pipeline(
            stage1_prog,
            stage1_args,
            stage2_prog,
            stage2_args,
            Some((output_file, progress_bar, poll_interval)),
        )
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
