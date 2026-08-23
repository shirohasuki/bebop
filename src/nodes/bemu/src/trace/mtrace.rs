use super::trace::with_current_trace;

pub struct MTraceEvent {
    pub is_write: bool,
    pub addr: u64,
    pub rows: u64,
    pub line_bytes: u32,
    pub row_stride: u64,
    pub vbank_id: u32,
    pub pbank_id: u32,
    pub group_id: u32,
}

pub fn mtrace(event: MTraceEvent) {
    with_current_trace(|trace| {
        let event_name = if event.is_write { "write" } else { "read" };
        let json = format!(
            r#"{{"type":"mtrace","clk":{},"event":"{}","addr":"0x{:016x}","rows":{},"line_bytes":{},"row_stride":{},"vbank_id":{},"pbank_id":{},"group_id":{}}}"#,
            trace.bemu_clk(),
            event_name,
            event.addr,
            event.rows,
            event.line_bytes,
            event.row_stride,
            event.vbank_id,
            event.pbank_id,
            event.group_id
        );

        trace.write_mtrace(&json);
    });
}
