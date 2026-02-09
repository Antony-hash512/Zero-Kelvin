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
    
    // We want to inline subcommand help. 
    // clap_mangen doesn't have an easy "inline subcommands without links" option exposed cleanly.
    // So we will construct a custom description that includes the subcommand help
    // and then remove the subcommands from the command struct so clap_mangen doesn't generate the default section.
    
    // We can't clear subcommands from an iterator. 
    // Instead of reusing 'cmd', let's build a fresh one with the new description and NO subcommands.
    // We duplicate the logic from `zk::Args::build_command()` regarding after_help.
    
    let after_help = cmd.get_after_help().map(|s| s.to_string()).unwrap_or_default();

    // Strip ASCII art for man page (search for start of detailed help)
    let clean_help = if let Some(idx) = after_help.find("  freeze [TARGETS...]") {
        format!("Detailed Command Information:\n\n{}", &after_help[idx..])
    } else {
        after_help
    };

    let man_cmd = clap::Command::new("0k")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Zero Kelvin - Cold Storage Utility") 
        .after_help(clean_help) 
        .author("Copyleft 🄯 2026 :: GPL3 github.com/Antony-hash512/Zero-Kelvin");

    let man = clap_mangen::Man::new(man_cmd);
    // We don't need .render_subcommands_section(false) because there are no subcommands now.
    
    let mut buffer: Vec<u8> = Default::default();
    man.render(&mut buffer)?;
    fs::write(out_dir.join("0k.1"), buffer)?;

    // Repeat for 0k-core
    let core_cmd = core_cli::Args::build_command();
    
    // Extract the after_help which contains the banner and detailed subcommands list
    let core_after_help = core_cmd.get_after_help().map(|s| s.to_string()).unwrap_or_default();
    
    // Strip ASCII art for man page (search for start of detailed help)
    let core_clean_help = if let Some(idx) = core_after_help.find("  create <INPUT>") {
        format!("Detailed Command Information:\n\n{}", &core_after_help[idx..])
    } else {
        core_after_help
    };
    
    // Create new command without subcommands
    let man_core_cmd = clap::Command::new("0k-core")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Manages SquashFS archives")
        .after_help(core_clean_help);
    
    let man_core = clap_mangen::Man::new(man_core_cmd);
    let mut buffer: Vec<u8> = Default::default();
    man_core.render(&mut buffer)?;
    fs::write(out_dir.join("0k-core.1"), buffer)?;

    println!("cargo:rerun-if-changed=src/cli/zk.rs");
    println!("cargo:rerun-if-changed=src/cli/core.rs");
    println!("cargo:rerun-if-changed=src/constants.rs");

    Ok(())
}
