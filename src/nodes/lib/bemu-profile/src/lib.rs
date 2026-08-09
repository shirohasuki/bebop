//! Host-time accounting for BEMU runs.

use std::fmt::Write;
use std::time::{Duration, Instant};

const FUNCT7_COUNT: usize = 128;

#[derive(Clone, Copy, Default)]
struct OperationCounter {
    calls: u64,
    elapsed: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationProfile {
    pub funct7: u8,
    pub calls: u64,
    pub elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct BemuProfileReport {
    total: Duration,
    spike_step: Duration,
    npu: Duration,
    operations: Vec<OperationProfile>,
}

impl BemuProfileReport {
    pub fn total(&self) -> Duration {
        self.total
    }

    pub fn npu(&self) -> Duration {
        self.npu
    }

    pub fn spike_guest(&self) -> Duration {
        self.spike_step.saturating_sub(self.npu)
    }

    pub fn bemu_glue(&self) -> Duration {
        self.total.saturating_sub(self.spike_step)
    }

    pub fn operations(&self) -> &[OperationProfile] {
        &self.operations
    }
}

pub struct BemuProfile {
    enabled: bool,
    npu: Duration,
    operations: [OperationCounter; FUNCT7_COUNT],
}

impl BemuProfile {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            npu: Duration::ZERO,
            operations: [OperationCounter::default(); FUNCT7_COUNT],
        }
    }

    pub fn begin_npu(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    pub fn end_npu(&mut self, funct7: u8, started: Option<Instant>) {
        let Some(started) = started else {
            return;
        };
        let elapsed = started.elapsed();
        self.npu = self.npu.saturating_add(elapsed);
        let counter = &mut self.operations[funct7 as usize];
        counter.calls = counter.calls.saturating_add(1);
        counter.elapsed = counter.elapsed.saturating_add(elapsed);
    }

    pub fn report(&self, total: Duration, spike_step: Duration) -> Option<BemuProfileReport> {
        if !self.enabled {
            return None;
        }

        let mut operations = self
            .operations
            .iter()
            .enumerate()
            .filter_map(|(funct7, counter)| {
                (counter.calls != 0).then_some(OperationProfile {
                    funct7: funct7 as u8,
                    calls: counter.calls,
                    elapsed: counter.elapsed,
                })
            })
            .collect::<Vec<_>>();
        operations.sort_unstable_by(|a, b| b.elapsed.cmp(&a.elapsed));

        let spike_step = spike_step.min(total);
        Some(BemuProfileReport {
            total,
            spike_step,
            npu: self.npu.min(spike_step),
            operations,
        })
    }
}

pub fn format_report(report: &BemuProfileReport) -> String {
    let total = report.total();
    let npu = report.npu();
    let spike_guest = report.spike_guest();
    let bemu_glue = report.bemu_glue();

    let mut output = String::new();
    writeln!(output, "[INFO] BEMU host profile").unwrap();
    write_bucket(&mut output, "Total loop wall time", total, total);
    write_bucket(&mut output, "Spike guest instruction execution", spike_guest, total);
    write_bucket(&mut output, "NPU functional model", npu, total);
    write_bucket(&mut output, "BEMU wrapper + syscall + loop", bemu_glue, total);

    writeln!(output, "[INFO] Top NPU operations by host time:").unwrap();
    for operation in report.operations().iter().take(10) {
        writeln!(
            output,
            "[INFO]   funct7={:<3} {:>10} {:>9} calls",
            operation.funct7,
            format_duration(operation.elapsed),
            operation.calls,
        )
        .unwrap();
    }
    output
}

pub fn print_report(report: &BemuProfileReport) {
    print!("{}", format_report(report));
}

fn write_bucket(output: &mut String, name: &str, elapsed: Duration, total: Duration) {
    let percentage = if total.is_zero() {
        0.0
    } else {
        elapsed.as_secs_f64() / total.as_secs_f64() * 100.0
    };
    writeln!(
        output,
        "[INFO]   {name:<28} {:>10} {:>5.1}%",
        format_duration(elapsed),
        percentage,
    )
    .unwrap();
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}s", duration.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_profile_has_no_report() {
        assert!(BemuProfile::new(false)
            .report(Duration::from_secs(1), Duration::from_secs(1))
            .is_none());
    }

    #[test]
    fn report_sorts_operations_by_elapsed_time() {
        let mut profile = BemuProfile::new(true);
        profile.npu = Duration::from_millis(8);
        profile.operations[1] = OperationCounter {
            calls: 2,
            elapsed: Duration::from_millis(3),
        };
        profile.operations[2] = OperationCounter {
            calls: 1,
            elapsed: Duration::from_millis(5),
        };

        let report = profile
            .report(Duration::from_millis(10), Duration::from_millis(9))
            .unwrap();
        assert_eq!(report.spike_guest(), Duration::from_millis(1));
        assert_eq!(report.bemu_glue(), Duration::from_millis(1));
        assert_eq!(report.operations()[0].funct7, 2);
        assert_eq!(report.operations()[1].funct7, 1);
    }
}
