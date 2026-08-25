use bebop_bemu::{format_profile_report, print_profile_report, BemuInstance, TraceConfig};
use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "bebop-bemu")]
struct Args {
    #[arg(long)]
    elf: PathBuf,
    #[arg(long)]
    log_dir: PathBuf,
    #[arg(long)]
    pk: bool,
    #[arg(long)]
    disasm: bool,
    #[arg(long = "tool-profile")]
    tool_profile: bool,
    #[arg(long)]
    itrace: bool,
    #[arg(long)]
    mtrace: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let mut bemu = BemuInstance::new(
        &args.log_dir,
        TraceConfig::new(args.itrace, args.mtrace),
        args.disasm,
        args.tool_profile,
    )
    .map_err(|e| e.to_string())?;
    bemu.load_elf(&args.elf).map_err(|e| e.to_string())?;
    bemu.init_hart(args.pk).map_err(|e| e.to_string())?;
    let started = args.tool_profile.then(Instant::now);
    while !bemu.finished() {
        bemu.step().map_err(|e| e.to_string())?;
    }
    if let Some(started) = started {
        let report = bemu
            .profile_report(started.elapsed())
            .ok_or_else(|| "tool-profile enabled but no profile report".to_string())?;
        print_profile_report(&report);
        let profile_path = args.log_dir.join("tool-profile.txt");
        std::fs::write(&profile_path, format_profile_report(&report))
            .map_err(|e| format!("failed to write tool profile {}: {e}", profile_path.display()))?;
    }

    let exit_code = bemu
        .exit_code()
        .ok_or_else(|| "bemu finished without an exit code".to_string())?;
    if exit_code != 0 {
        return Err(format!("bemu exited with code {exit_code}"));
    }
    Ok(())
}
