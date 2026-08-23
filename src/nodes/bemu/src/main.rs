use bebop_bemu::{BemuInstance, TraceConfig};
use std::path::PathBuf;

#[derive(Default)]
struct Args {
    elf: Option<PathBuf>,
    log_dir: Option<PathBuf>,
    pk: bool,
    itrace: bool,
    mtrace: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1))?;
    let elf = args.elf.ok_or_else(|| "missing required argument --elf".to_string())?;
    let log_dir = args
        .log_dir
        .ok_or_else(|| "missing required argument --log-dir".to_string())?;

    let trace_config = TraceConfig::new(args.itrace, args.mtrace);
    let mut bemu = BemuInstance::new(&log_dir, trace_config, false, false).map_err(|e| e.to_string())?;
    bemu.load_elf(&elf).map_err(|e| e.to_string())?;
    bemu.init_hart(args.pk).map_err(|e| e.to_string())?;
    while !bemu.finished() {
        bemu.step().map_err(|e| e.to_string())?;
    }

    let exit_code = bemu.exit_code().unwrap_or(0);
    if exit_code != 0 {
        return Err(format!("bemu exited with code {exit_code}"));
    }
    Ok(())
}

fn parse_args<I>(args: I) -> Result<Args, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args: Vec<_> = args.into_iter().collect();
    if args.first().map(String::as_str) == Some("run") && args.get(1).map(String::as_str) == Some("bemu") {
        args.drain(0..2);
    }

    let mut parsed = Args::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--elf" => {
                i += 1;
                parsed.elf = Some(PathBuf::from(
                    args.get(i).ok_or_else(|| "--elf needs a value".to_string())?,
                ));
            }
            "--log-dir" => {
                i += 1;
                parsed.log_dir = Some(PathBuf::from(
                    args.get(i).ok_or_else(|| "--log-dir needs a value".to_string())?,
                ));
            }
            "--pk" => parsed.pk = true,
            "--itrace" => parsed.itrace = true,
            "--mtrace" => parsed.mtrace = true,
            other => return Err(format!("unknown argument {other}")),
        }
        i += 1;
    }

    Ok(parsed)
}
