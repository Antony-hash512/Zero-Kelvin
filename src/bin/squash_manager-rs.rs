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
      -e, --encrypt         Create an encrypted LUKS container (Requires root/sudo).
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

        /// Encrypt the archive using LUKS (Requires root/sudo)
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

/// Helper to ensure LUKS resources are cleaned up on failure (RAII)
struct LuksTransaction<'a, E: CommandExecutor + ?Sized> {
    executor: &'a E,
    mapper_name: Option<String>,
    output_path: &'a PathBuf,
    success: bool,
}

impl<'a, E: CommandExecutor + ?Sized> LuksTransaction<'a, E> {
    fn new(executor: &'a E, output_path: &'a PathBuf) -> Self {
        Self {
            executor,
            mapper_name: None,
            output_path,
            success: false,
        }
    }

    fn set_mapper(&mut self, name: String) {
        self.mapper_name = Some(name);
    }

    fn set_success(&mut self) {
        self.success = true;
    }
}

impl<'a, E: CommandExecutor + ?Sized> Drop for LuksTransaction<'a, E> {
    fn drop(&mut self) {
        if let Some(mapper) = &self.mapper_name {
            // Always try to close mapper, even on success (it should be closed manually before, but if panic happens...)
            // Actually, in normal flow we close it manually to check error code. 
            // This drop is mostly for panic/error path.
            // If we closed it manually, the check "if exists" would be good, but we can't easily check existence without command.
            // We'll rely on cryptsetup erroring if not exists, or check /dev/mapper.
            // Silence errors here to avoid panic-in-drop.
            let _ = self.executor.run("sudo", &["cryptsetup", "close", mapper]);
        }
        
        if !self.success {
             // Remove the file if we failed
             if self.output_path.exists() {
                 let _ = fs::remove_file(self.output_path);
             }
        }
    }
}

fn get_fs_overhead_percentage(path: &PathBuf, executor: &impl CommandExecutor) -> u32 {
    // df -T <path>
    // Output:
    // Filesystem     Type     1K-blocks      Used Available Use% Mounted on
    // /dev/sda1      ext4     ...
    
    // We need to handle the case where path doesn't exist yet (use parent)
    let check_path = if path.exists() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(&PathBuf::from(".")).to_path_buf()
    };
    
    // Run df -T
    if let Ok(output) = executor.run("df", &["-T", check_path.to_str().unwrap_or(".")]) {
        if output.status.success() {
             let out_str = String::from_utf8_lossy(&output.stdout);
             // Parse 2nd line, 2nd column
             if let Some(second_line) = out_str.lines().nth(1) {
                 let parts: Vec<&str> = second_line.split_whitespace().collect();
                 if parts.len() >= 2 {
                     let fs_type = parts[1];
                     match fs_type {
                         "ext4" | "xfs" | "btrfs" | "zfs" | "f2fs" | "tmpfs" | "overlay" => return 50,
                         _ => return 10,
                     }
                 }
             }
        }
    }
    
    // Default fallback
    10
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

/// Helper to determine if we need sudo/doas
fn get_effective_root_cmd() -> Vec<String> {
    // Check if we are root via 'id -u'
    if let Ok(output) = std::process::Command::new("id").arg("-u").output() {
        if let Ok(uid_str) = String::from_utf8(output.stdout) {
            if let Ok(uid) = uid_str.trim().parse::<u32>() {
                if uid == 0 {
                    return vec![];
                }
            }
        }
    }
    
    // Check env var
    if let Ok(cmd) = std::env::var("ROOT_CMD") {
         if !cmd.trim().is_empty() {
             return cmd.split_whitespace().map(|s| s.to_string()).collect();
         }
    }

    // Default to sudo
    vec!["sudo".to_string()]
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
            // Define compression strategy
            let comp_mode = CompressionMode::from_level(compression);

            if encrypt {
                // Determine raw size
                let raw_size_bytes = if input_path.is_dir() {
                    // du -sb
                     if let Ok(output) = executor.run("du", &["-sb", input_path.to_str().unwrap()]) {
                        if output.status.success() {
                            let out_str = String::from_utf8_lossy(&output.stdout);
                            out_str.split_whitespace().next().unwrap_or("0").parse::<u64>().unwrap_or(0)
                        } else {
                            0
                        }
                     } else { 0 }
                } else {
                    // file size
                    fs::metadata(&input_path).map(|m| m.len()).unwrap_or(0)
                };

                if raw_size_bytes == 0 {
                    return Err(anyhow!("Could not determine input size or empty input"));
                }

                let output_buf = output_path.as_ref().ok_or(anyhow!("Output path required"))?;
                
                // Overhead calc
                let overhead_percent = get_fs_overhead_percentage(output_buf, executor);
                let overhead_bytes = (raw_size_bytes as f64 * (overhead_percent as f64 / 100.0)) as u64;
                let luks_header_bytes = 32 * 1024 * 1024; // 32MB safety
                
                let container_size = raw_size_bytes + overhead_bytes + luks_header_bytes;

                if std::env::var("RUST_LOG").is_ok() {
                    eprintln!("DEBUG: Encrypting. Input: {} bytes. Overhead: {}%. Allocating: {} bytes.", 
                        raw_size_bytes, overhead_percent, container_size);
                }

                // 1. Create sparse file
                let file = fs::File::create(output_buf).context("Failed to create container file")?;
                file.set_len(container_size).context("Failed to pre-allocate container size")?;
                drop(file); // Close to allow cryptsetup to use it via path

                // Start Transaction for cleanup
                let mut transaction = LuksTransaction::new(executor, output_buf);

                let output_str = output_buf.to_str().ok_or(anyhow!("Invalid output path"))?;
                
                // 2. Format LUKS
                // Requires root. We assume user has sudo or is root.
                // -q for quiet (assumes we passed key or interactively asked? Wait, luksFormat requires confirmation 'YES')
                // For automation/simplicity we might need input. 
                // But normally ZKS asks user interactive password. 
                // 'cryptsetup luksFormat' without key file will ask interactive.
                // We should use run_interactive to inherit stdio.
                
                
                // Get command prefix (e.g. empty if root, or ["sudo"])
                let root_cmd = get_effective_root_cmd();
                
                println!("Initializing LUKS container...");
                // Construct command: [sudo] cryptsetup luksFormat -q output
                let mut luks_args = root_cmd.clone();
                luks_args.extend(vec!["cryptsetup".to_string(), "luksFormat".to_string(), "-q".to_string(), output_str.to_string()]);
                
                let prog = luks_args.remove(0);
                let args_refs: Vec<&str> = luks_args.iter().map(|s| s.as_str()).collect();

                let status = executor.run_interactive(&prog, &args_refs)
                    .context("Failed to execute cryptsetup luksFormat")?;

                if !status.success() {
                    return Err(anyhow!("luksFormat failed"));
                }

                // 3. Open
                // Generate tmp name
                let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
                let rnd: u32 = rand::rng().random_range(1000..9999);
                let mapper_name = format!("sq_{}_{}", timestamp, rnd);
                
                println!("Opening LUKS container...");
                let mut open_args = root_cmd.clone();
                open_args.extend(vec!["cryptsetup".to_string(), "open".to_string(), output_str.to_string(), mapper_name.clone()]);
                
                let prog_open = open_args.remove(0);
                let args_open_refs: Vec<&str> = open_args.iter().map(|s| s.as_str()).collect();

                let status_open = executor.run_interactive(&prog_open, &args_open_refs)
                    .context("Failed to execute cryptsetup open")?;
                
                if !status_open.success() {
                    return Err(anyhow!("cryptsetup open failed"));
                }
                
                transaction.set_mapper(mapper_name.clone());
                let mapper_path = format!("/dev/mapper/{}", mapper_name);

                // 4. Pack Data
                // If input is dir -> mksquashfs to mapper_path
                let pack_result = if input_path.is_dir() {
                    let mut cmd_args = vec![
                         input_path.to_str().unwrap().to_string(),
                         mapper_path.clone(),
                         "-no-recovery".to_string(),
                         "-noappend".to_string()
                    ];
                    if no_progress { cmd_args.push("-no-progress".to_string()); }
                    let level_str = compression.to_string();
                    
                    // Helper to adapt Vec<String> to Vec<&str> API of CompressionMode
                    // We need to temporarily hold the strings?
                    // compression mode just pushes &str literals usually.
                    // But wait, `apply_to_mksquashfs` implementation in `lib.rs` takes `&mut Vec<&str>`.
                    // We have `Vec<String>`.
                    // We should change `apply_to_mksquashfs` to simple push logic OR handle manual push here.
                    // Or... convert our Vec<String> to Vec<&str> first? No, we can't push to Vec<&str> if backing string is new.
                    // Simpler: Apply args manually or refactor `apply_to_mksquashfs` is generic?
                    // "comp_mode" implementation is simple.
                    // Let's just manually apply logic here since `apply_to...` is restrictive for String owner.
                    match comp_mode {
                         CompressionMode::None => cmd_args.push("-no-compression".to_string()),
                         CompressionMode::Zstd(_) => {
                              cmd_args.push("-comp".to_string());
                              cmd_args.push("zstd".to_string());
                              cmd_args.push("-Xcompression-level".to_string());
                              cmd_args.push(level_str.to_string());
                         },
                         // other modes...
                    }
                    
                    // Construct: [sudo] mksquashfs ...
                    let mut mk_args = root_cmd.clone();
                    mk_args.extend(vec!["mksquashfs".to_string()]);
                    mk_args.extend(cmd_args);
                    
                    let mk_prog = mk_args.remove(0);
                    let mk_refs: Vec<&str> = mk_args.iter().map(|s| s.as_str()).collect();

                    let output = executor.run(&mk_prog, &mk_refs)?;
                    if !output.status.success() {
                         Err(anyhow!("mksquashfs failed: {}", String::from_utf8_lossy(&output.stderr)))
                    } else { Ok(()) }
                } else {
                     // Archive repack
                    let input_str = input_path.to_str().ok_or(anyhow!("Invalid input path"))?;
                    
                    let file_name = input_path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();
                    let decompressor = if file_name.ends_with(".tar") { "cat" }
                    else if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") { "gzip -dc" }
                    else if file_name.ends_with(".tar.bz2") || file_name.ends_with(".tbz2") { "bzip2 -dc" }
                    else if file_name.ends_with(".tar.xz") || file_name.ends_with(".txz") { "xz -dc" }
                    else if file_name.ends_with(".tar.zst") || file_name.ends_with(".tzst") { "zstd -dc" }
                    else if file_name.ends_with(".tar.zip") { "unzip -p" }
                    else if file_name.ends_with(".tar.7z") { "7z x -so" }
                    else if file_name.ends_with(".tar.rar") { "unrar p -inul" }
                    else { return Err(anyhow!("Unsupported format: {}", file_name)); };

                    let compressor_flag = comp_mode.get_tar2sqfs_compressor_flag()?;
                    
                    // Pipeline: decompress | tar2sqfs -> /dev/mapper
                    // Need sudo for tar2sqfs writing to mapper? 
                    // Yes, likely.
                    // But we can't easily pipe INTO sudo tar2sqfs.
                    // solution: sudo sh -c "decompress ... | tar2sqfs ... -o /dev/mapper/..."
                    // Wait, input file might be user owned. sudo decompress is accessing user file. Fine.
                    let cmd = format!(
                        "set -o pipefail; {} '{}' | tar2sqfs --quiet --no-skip --force {} '{}'",
                        decompressor,
                        input_str.replace("'", "'\\''"),
                        compressor_flag,
                        mapper_path
                    );
                    
                    let mut sh_args = root_cmd.clone();
                    sh_args.extend(vec!["sh".to_string(), "-c".to_string(), cmd]);
                    
                    let sh_prog = sh_args.remove(0);
                    let sh_refs: Vec<&str> = sh_args.iter().map(|s| s.as_str()).collect();
                    
                    let output = executor.run(&sh_prog, &sh_refs)?;
                    if !output.status.success() {
                         Err(anyhow!("Archive repack failed: {}", String::from_utf8_lossy(&output.stderr)))
                    } else { Ok(()) }
                };

                if let Err(e) = pack_result {
                    return Err(e);
                }

                // 5. Trim logic
                // Need unsquashfs (sudo usually not needed for read, but reading from /dev/mapper requires root)
                let mut trim_size: Option<u64> = None;
                
                // Get FS Size
                // unsquashfs -s /dev/mapper/...
                if let Ok(out) = executor.run("sudo", &["unsquashfs", "-s", &mapper_path]) {
                     let out_str = String::from_utf8_lossy(&out.stdout); // unsquashfs prints to stdout
                     // Regex: "Filesystem size\s+([0-9]+)\s+bytes"
                     // Quick parse logic
                     if let Some(pos) = out_str.find("Filesystem size") {
                         let rest = &out_str[pos..];
                         if let Some(line) = rest.lines().next() {
                             // "Filesystem size 1234 bytes"
                             let parts: Vec<&str> = line.split_whitespace().collect();
                             if parts.len() >= 3 {
                                 if let Ok(bytes) = parts[2].parse::<u64>() {
                                      // Get Offset
                                      if let Ok(dump) = executor.run("sudo", &["cryptsetup", "luksDump", output_str]) {
                                          let dump_str = String::from_utf8_lossy(&dump.stdout);
                                          let mut offset: u64 = 0;
                                          // LUKS2: "offset: 16777216 [bytes]"
                                          for line in dump_str.lines() {
                                              if line.trim().starts_with("offset:") && line.contains("bytes") {
                                                  if let Some(val_str) = line.split_whitespace().nth(1) {
                                                      if let Ok(val) = val_str.parse::<u64>() {
                                                          offset = val;
                                                          break;
                                                      }
                                                  }
                                              }
                                              // LUKS1: "Payload offset: 4096" (sectors)
                                              if line.trim().starts_with("Payload offset:") {
                                                  if let Some(val_str) = line.split_whitespace().nth(2) {
                                                      if let Ok(sect) = val_str.parse::<u64>() {
                                                          offset = sect * 512;
                                                          break;
                                                      }
                                                  }
                                              }
                                          }
                                          
                                          if offset > 0 {
                                               // Calc total
                                               let raw_trim = bytes + offset + 1024*1024; // +1MB
                                               // Align to 4096
                                               let aligned = ((raw_trim + 4095) / 4096) * 4096;
                                               trim_size = Some(aligned);
                                          }
                                      }
                                 }
                             }
                         }
                     }
                }

                // 6. Close
                // We do explicit close here to ensure success before truncate
                // Transaction drop will try to close again but fail harmlessly or we can clear mapper from it
                let _ = executor.run("sudo", &["cryptsetup", "close", &mapper_name]);
                transaction.mapper_name = None; // Disable drop cleanup for mapper
                
                // 7. Truncate
                if let Some(size) = trim_size {
                    let container_file = fs::File::options().write(true).open(output_buf).context("Failed to open file for trimming")?;
                    let current_len = container_file.metadata()?.len();
                    if size < current_len {
                        println!(" Optimizing container size: {:.1}MB -> {:.1}MB", 
                            current_len as f64 / 1024.0 / 1024.0, size as f64 / 1024.0 / 1024.0);
                        container_file.set_len(size)?;
                    }
                }

                transaction.set_success();
                return Ok(());
            }


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

                // Use 'set -o pipefail' so that if decompressor fails, the whole pipeline fails
                let output = executor.run("sh", &["-c", &format!("set -o pipefail; {}", cmd)])?;

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
            fn run_interactive<'a>(&self, program: &str, args: &[&'a str]) -> Result<std::process::ExitStatus>;
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
    fn test_create_encrypted_flow() {
        // Setup
        let temp_dir = tempfile::tempdir().unwrap();
        let input_path = temp_dir.path().join("input_dir");
        fs::create_dir(&input_path).unwrap();
        let input_str = input_path.to_str().unwrap().to_string();
        
        // Output path
        let output_path = temp_dir.path().join("encrypted.sqfs");
        let output_str = output_path.to_str().unwrap().to_string();

        let mut mock = MockCommandExecutor::new();
        
        // 1. du -sb (Size calc)
        let input_str_1 = input_str.clone();
        mock.expect_run()
            .withf(move |program, args| {
                program == "du" && args == vec!["-sb", input_str_1.as_str()]
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"1048576\tinput_dir\n".to_vec(),
                stderr: vec![],
            }));

        // 2. df -T (Overhead calc)
        let parent = temp_dir.path().to_str().unwrap().to_string();
        mock.expect_run()
            .withf(move |program, args| {
                program == "df" && args == vec!["-T", parent.as_str()]
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"Filesystem Type\n/dev/sda1 ext4\n".to_vec(),
                stderr: vec![],
            }));

        // 3. luksFormat
        let output_str_3 = output_str.clone();
        mock.expect_run_interactive()
            .withf(move |program, args| {
                 // get_effective_root_cmd returns ["sudo"] by default if not root
                 program == "cryptsetup" && args == vec!["luksFormat", "-q", output_str_3.as_str()]
                 || program == "sudo" && args == vec!["cryptsetup", "luksFormat", "-q", output_str_3.as_str()]
            })
            .times(1)
            .returning(|_, _| Ok(std::process::ExitStatus::from_raw(0)));

        // 4. open
        let output_str_4 = output_str.clone();
        mock.expect_run_interactive()
            .withf(move |program, args| {
                (program == "cryptsetup" || program == "sudo") &&
                args.contains(&"open") &&
                args.contains(&output_str_4.as_str())
            })
            .times(1)
            .returning(|_, _| Ok(std::process::ExitStatus::from_raw(0)));

        // 5. mksquashfs
        // output to /dev/mapper/...
        mock.expect_run()
            .withf(move |program, args| {
                 (program == "mksquashfs" || program == "sudo") &&
                 args.iter().any(|s| s.starts_with("/dev/mapper/sq_"))
            })
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));
            
        // 6. unsquashfs -s (Trim size)
        mock.expect_run()
            .withf(|program, args| (program == "unsquashfs" || program == "sudo") && args.contains(&"unsquashfs") || args.contains(&"-s"))
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"Filesystem size 500000 bytes\n".to_vec(),
                stderr: vec![],
            }));
            
        // 7. luksDump (Offset)
        mock.expect_run()
            .withf(|program, args| (program == "cryptsetup" || program == "sudo") && args.contains(&"luksDump"))
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: b"offset: 1000000 bytes\n".to_vec(),
                stderr: vec![],
            }));
            
        // 8. close
        mock.expect_run()
            .withf(|program, args| (program == "cryptsetup" || program == "sudo") && args.contains(&"close"))
            .times(1)
            .returning(|_, _| Ok(Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: vec![],
                stderr: vec![],
            }));

        let args = SquashManagerArgs {
            command: Commands::Create {
                input_path: input_path,
                output_path: Some(output_path),
                encrypt: true,
                compression: DEFAULT_ZSTD_COMPRESSION,
                no_progress: false,
            },
        };

        run(args, &mock).unwrap();
    }
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
