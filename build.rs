use std::fs;
use std::path::Path;

#[allow(dead_code)]
#[path = "src/constants.rs"]
mod constants;

#[allow(dead_code)]
#[path = "src/cli/zk.rs"]
mod zk;

#[allow(dead_code)]
#[path = "src/cli/core.rs"]
mod core_cli;

fn main() -> std::io::Result<()> {
    let out_dir = Path::new("man");
    if !out_dir.exists() {
        fs::create_dir_all(out_dir)?;
    }

    // Generate man page for '0k'
    let cmd = zk::Args::build_command();
    let man = clap_mangen::Man::new(cmd);
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;
    fs::write(out_dir.join("0k.1"), buffer)?;

    // Generate man page for '0k-core'
    let cmd = core_cli::Args::build_command();
    let man = clap_mangen::Man::new(cmd);
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;
    fs::write(out_dir.join("0k-core.1"), buffer)?;

    println!("cargo:rerun-if-changed=src/cli/zk.rs");
    println!("cargo:rerun-if-changed=src/cli/core.rs");
    println!("cargo:rerun-if-changed=src/constants.rs");

    Ok(())
}
