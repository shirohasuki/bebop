use clap::Parser;
use std::path::PathBuf;

/// Environment variables for bbdev-driven regression runs
/// (nextest does not allow custom CLI args after `--`, so we use envs).
const ENV_WORKLOAD_TOML: &str = "BEBOP_WORKLOAD_TOML";
const ENV_BB_TESTS_ROOT: &str = "BEBOP_BB_TESTS_ROOT";
#[cfg(feature = "p2e")]
const ENV_P2E_BITSTREAM: &str = "BEBOP_P2E_BITSTREAM";
#[cfg(feature = "p2e")]
const ENV_P2E_BUILD_DIR: &str = "BEBOP_P2E_BUILD_DIR";

#[derive(Parser, Debug, Clone)]
#[command(name = "elf-regression")]
#[command(about = "ELF regression test harness for bebop")]
pub struct RegressionArgs {
    #[arg(long, value_name = "PATTERN")]
    pub filter: Option<String>,

    #[arg(long, value_name = "FILE")]
    pub case_list: Option<PathBuf>,

    #[arg(long)]
    pub clean_before: bool,

    #[arg(long, short = 'j', value_name = "N", default_value = "1")]
    pub jobs: usize,

    #[arg(long, short = 'v')]
    pub verbose: bool,

    #[arg(long, hide = true)]
    pub list: bool,

    #[arg(long, hide = true)]
    pub format: Option<String>,

    #[arg(long, hide = true)]
    pub ignored: bool,

    #[arg(long, hide = true)]
    pub exact: bool,

    #[arg(long, hide = true)]
    pub nocapture: bool,

    #[arg(long, hide = true)]
    pub bench: bool,

    #[arg(long, hide = true)]
    pub show_output: bool,

    #[arg(trailing_var_arg = true)]
    pub test_args: Vec<String>,
}

impl RegressionArgs {
    /// Workload toml path read from BEBOP_WORKLOAD_TOML env var.
    pub fn workload_toml(&self) -> Option<PathBuf> {
        std::env::var_os(ENV_WORKLOAD_TOML).map(PathBuf::from)
    }

    /// Root directory that `search_path` in workloads.toml is resolved against.
    /// Read from BEBOP_BB_TESTS_ROOT; defaults to `../bb-tests/output` relative
    /// to the bebop crate (compatible with the pre-bbdev developer workflow).
    pub fn bb_tests_root(&self) -> PathBuf {
        std::env::var_os(ENV_BB_TESTS_ROOT)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../bb-tests/output"))
    }

    /// P2E bitstream path read from BEBOP_P2E_BITSTREAM env var.
    #[cfg(feature = "p2e")]
    pub fn p2e_bitstream(&self) -> Option<PathBuf> {
        std::env::var_os(ENV_P2E_BITSTREAM).map(PathBuf::from)
    }

    /// P2E build dir read from BEBOP_P2E_BUILD_DIR env var.
    #[cfg(feature = "p2e")]
    pub fn p2e_build_dir(&self) -> Option<PathBuf> {
        std::env::var_os(ENV_P2E_BUILD_DIR).map(PathBuf::from)
    }

    pub fn libtest_forward_flags(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.exact {
            out.push("--exact".to_string());
        }
        if self.nocapture {
            out.push("--nocapture".to_string());
        }
        if self.show_output {
            out.push("--show-output".to_string());
        }
        if self.bench {
            out.push("--bench".to_string());
        }
        out
    }
}
