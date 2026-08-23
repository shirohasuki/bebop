use chipcrowd::{FpgaTransport, MockTransport, ModelRegistry, Service};
use std::env;

fn main() {
    let mut listen = "127.0.0.1:8080".to_string();
    let mut api_key = "bbk-dev".to_string();
    let mut transport = "mock".to_string();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--listen" => listen = args.next().unwrap_or_else(|| usage("--listen requires an address")),
            "--api-key" => api_key = args.next().unwrap_or_else(|| usage("--api-key requires a value")),
            "--transport" => transport = args.next().unwrap_or_else(|| usage("--transport requires mock or fpga")),
            "--help" | "-h" => usage(""),
            other => usage(&format!("unknown argument: {other}")),
        }
    }
    let registry = ModelRegistry::default_registry();
    let result = match transport.as_str() {
        "mock" => Service::new(registry, api_key, MockTransport).serve(&listen),
        "fpga" => Service::new(registry, api_key, FpgaTransport).serve(&listen),
        _ => usage("--transport must be mock or fpga"),
    };
    if let Err(error) = result { eprintln!("chipcrowd failed: {error}"); std::process::exit(1); }
}

fn usage(message: &str) -> ! {
    if !message.is_empty() { eprintln!("error: {message}"); }
    eprintln!("usage: chipcrowd [--listen HOST:PORT] [--api-key KEY] [--transport mock|fpga]");
    std::process::exit(if message.is_empty() { 0 } else { 2 });
}
