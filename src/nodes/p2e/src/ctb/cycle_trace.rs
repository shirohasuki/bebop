use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const RECORD_PREFIX: &str = "@BCT";
const MAX_TRACE_PATH_DEPTH: usize = 4;

#[derive(Debug)]
pub struct CycleTraceCollector {
    cycle_dir: PathBuf,
    uart_lines: HashMap<u32, Vec<u8>>,
    first_start: Option<u64>,
    last_end: Option<u64>,
    elapsed_sum: u64,
    trace_count: u64,
}

impl CycleTraceCollector {
    pub fn new(log_dir: &Path) -> Result<Self, String> {
        let cycle_dir = log_dir.join("trace/cycle");
        fs::create_dir_all(&cycle_dir)
            .map_err(|e| format!("failed to create cycle trace directory {}: {e}", cycle_dir.display()))?;
        for entry in fs::read_dir(&cycle_dir)
            .map_err(|e| format!("failed to scan cycle trace directory {}: {e}", cycle_dir.display()))?
        {
            let entry = entry.map_err(|e| format!("failed to read cycle trace directory entry: {e}"))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "summary.txt" || (name.starts_with("trace-") && name.ends_with(".txt")) {
                fs::remove_file(entry.path())
                    .map_err(|e| format!("failed to remove stale cycle trace {}: {e}", entry.path().display()))?;
            }
        }

        Ok(Self {
            cycle_dir,
            uart_lines: HashMap::new(),
            first_start: None,
            last_end: None,
            elapsed_sum: 0,
            trace_count: 0,
        })
    }

    pub fn push_uart_byte(&mut self, hart_id: u32, byte: u8) -> Result<(), String> {
        let line = self.uart_lines.entry(hart_id).or_default();
        line.push(byte);
        if byte != b'\n' {
            return Ok(());
        }

        let line = std::mem::take(line);
        self.consume_line(&line)
    }

    pub fn finish(self) -> Result<(), String> {
        if self.trace_count == 0 {
            return Ok(());
        }

        let first_start = self.first_start.expect("trace count requires first start");
        let last_end = self.last_end.expect("trace count requires last end");
        let summary = format!(
            "first_start {first_start}\nlast_end {last_end}\ntrace_span {}\ntraced_cycle_sum {}\ntrace_count {}\n",
            last_end.saturating_sub(first_start),
            self.elapsed_sum,
            self.trace_count,
        );
        let summary_path = self.cycle_dir.join("summary.txt");
        fs::write(&summary_path, summary)
            .map_err(|e| format!("failed to write cycle trace summary {}: {e}", summary_path.display()))
    }

    fn consume_line(&mut self, bytes: &[u8]) -> Result<(), String> {
        let line = std::str::from_utf8(bytes)
            .map_err(|e| format!("cycle trace UART record is not UTF-8: {e}"))?
            .trim_end_matches(['\r', '\n']);
        if !line.starts_with(RECORD_PREFIX) {
            return Ok(());
        }

        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != 8 || fields[0] != RECORD_PREFIX {
            return Err(format!("invalid cycle trace UART record: {line}"));
        }

        let depth = parse_usize(fields[1], "depth", line)?;
        if !(1..=MAX_TRACE_PATH_DEPTH).contains(&depth) {
            return Err(format!(
                "cycle trace path depth {depth} is out of range in record: {line}"
            ));
        }

        let path = [
            parse_i64(fields[2], "path0", line)?,
            parse_i64(fields[3], "path1", line)?,
            parse_i64(fields[4], "path2", line)?,
            parse_i64(fields[5], "path3", line)?,
        ];
        if path[..depth].iter().any(|component| *component < 0) {
            return Err(format!("cycle trace path has a negative component in record: {line}"));
        }

        let start = parse_u64(fields[6], "start", line)?;
        let end = parse_u64(fields[7], "end", line)?;
        if end < start {
            return Err(format!("cycle trace end precedes start in record: {line}"));
        }

        let path_key = path[..depth]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("-");
        let elapsed = end - start;
        let trace_path = self.cycle_dir.join(format!("trace-{path_key}.txt"));
        fs::write(&trace_path, format!("start {start}\nend {end}\nelapsed {elapsed}\n"))
            .map_err(|e| format!("failed to write cycle trace {}: {e}", trace_path.display()))?;

        self.first_start = Some(self.first_start.map_or(start, |current| current.min(start)));
        self.last_end = Some(self.last_end.map_or(end, |current| current.max(end)));
        self.elapsed_sum = self.elapsed_sum.saturating_add(elapsed);
        self.trace_count = self.trace_count.saturating_add(1);
        Ok(())
    }
}

fn parse_usize(value: &str, field: &str, line: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|e| format!("invalid cycle trace {field} in record {line}: {e}"))
}

fn parse_i64(value: &str, field: &str, line: &str) -> Result<i64, String> {
    value
        .parse()
        .map_err(|e| format!("invalid cycle trace {field} in record {line}: {e}"))
}

fn parse_u64(value: &str, field: &str, line: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|e| format!("invalid cycle trace {field} in record {line}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::CycleTraceCollector;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_bemu_compatible_cycle_files_from_uart_records() {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("bebop-p2e-cycle-trace-{}-{nonce}", std::process::id()));
        let mut collector = CycleTraceCollector::new(&dir).unwrap();

        for byte in b"normal UART output\n@BCT,2,7,9,-1,-1,100,160\n" {
            collector.push_uart_byte(0, *byte).unwrap();
        }
        collector.finish().unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("trace/cycle/trace-7-9.txt")).unwrap(),
            "start 100\nend 160\nelapsed 60\n"
        );
        assert_eq!(
            fs::read_to_string(dir.join("trace/cycle/summary.txt")).unwrap(),
            "first_start 100\nlast_end 160\ntrace_span 60\ntraced_cycle_sum 60\ntrace_count 1\n"
        );

        fs::remove_dir_all(dir).unwrap();
    }
}
