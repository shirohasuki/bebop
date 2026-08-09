use crate::{BuildCommand, BuildTarget};
use duct::cmd;
use snafu::{FromString, ResultExt, Whatever};

#[cfg(feature = "p2e")]
use bebop_p2e::BitstreamBuilder;

pub fn build(command: BuildCommand) -> Result<(), Whatever> {
    match command.target {
        BuildTarget::Verilator {
            rtl_dir,
            out_dir,
            diff,
            fast,
        } => {
            if !rtl_dir.is_dir() {
                let message = format!("RTL directory does not exist: {}", rtl_dir.display());
                return Err(Whatever::without_source(message));
            }
            if fast {
                return Err(Whatever::without_source(
                    "Verilator fast build is not supported yet".to_string(),
                ));
            }
            let rtl_dir = rtl_dir
                .canonicalize()
                .whatever_context("failed to canonicalize RTL directory")?;
            std::fs::create_dir_all(&out_dir).whatever_context("failed to create output directory")?;

            let features = if diff { "verilator,bemu" } else { "verilator" };
            println!("Building {features}: {} -> {}", rtl_dir.display(), out_dir.display());
            cmd!("cargo", "build", "--release", "--bin", "bebop", "--features", features)
                .env("VSRC_PATH", &rtl_dir)
                .run()
                .whatever_context("failed to build bebop")?;

            // copy the built executable to the output directory
            let dest = out_dir.join("bebop-verilator");
            std::fs::copy("target/release/bebop", &dest).whatever_context("failed to copy built executable")?;
            println!("Built executable: {}", dest.display());
            Ok(())
        }
        BuildTarget::P2e { rtl_dir, out_dir } => {
            if !rtl_dir.is_dir() {
                let message = format!("RTL directory does not exist: {}", rtl_dir.display());
                return Err(Whatever::without_source(message));
            }
            let rtl_dir = rtl_dir
                .canonicalize()
                .whatever_context("failed to canonicalize RTL directory")?;
            std::fs::create_dir_all(&out_dir).whatever_context("failed to create output directory")?;
            println!("Building p2e: {} -> {}", rtl_dir.display(), out_dir.display());
            let features = "p2e";
            cmd!("cargo", "build", "--release", "--bin", "bebop", "--features", features)
                .env("VSRC_PATH", &rtl_dir)
                .env("OUT_PATH", &out_dir)
                .run()
                .whatever_context("failed to build p2e")?;

            #[cfg(feature = "p2e")]
            {
                // bbdev's P2E runworkload flow rebuilds the host runtime in the
                // bitstream case before every run.  It sets this internal flag so
                // the existing bitstream is never regenerated.
                let runtime_only = std::env::var_os("BEBOP_P2E_RUNTIME_ONLY").is_some();
                if !runtime_only {
                    BitstreamBuilder::new(out_dir.clone())
                        .build()
                        .map_err(Whatever::without_source)?;
                }

                // copy the built executable to the output directory
                let dest = out_dir.join("bebop-p2e");
                std::fs::copy("target/release/bebop", &dest).whatever_context("failed to copy built executable")?;
                println!("Built P2E runtime: {}", dest.display());
                Ok(())
            }
            #[cfg(not(feature = "p2e"))]
            {
                Err(Whatever::without_source(
                    "p2e builder is not compiled into this executable".to_string(),
                ))
            }
        }
    }
}
