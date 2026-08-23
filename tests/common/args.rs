use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "elf-regression")]
#[command(about = "ELF regression test harness for bebop")]
pub struct RegressionArgs {
    #[arg(long, value_name = "PATH")]
    pub workload_toml: Option<PathBuf>,

    #[arg(long, value_name = "DIR")]
    pub bb_tests_root: Option<PathBuf>,

    #[cfg(feature = "p2e")]
    #[arg(long, value_name = "PATH")]
    pub p2e_bitstream: Option<PathBuf>,

    #[arg(long, value_name = "PATTERN")]
    pub filter: Option<String>,

    #[arg(long, value_name = "FILE")]
    pub case_list: Option<PathBuf>,

    #[arg(long)]
    pub clean_before: bool,

    #[arg(long, short = 'j', value_name = "N")]
    pub jobs: Option<usize>,

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
    pub fn bb_tests_root(&self) -> PathBuf {
        self.bb_tests_root.clone().expect("--bb-tests-root is required")
    }

    #[cfg(feature = "p2e")]
    pub fn p2e_bitstream(&self) -> PathBuf {
        self.p2e_bitstream
            .clone()
            .expect("--p2e-bitstream is required for test_p2e")
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
        if let Some(jobs) = self.jobs {
            out.push("--test-threads".to_string());
            out.push(jobs.to_string());
        }
        out
    }
}
