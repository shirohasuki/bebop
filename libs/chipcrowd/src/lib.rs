//! Standalone inference service primitives for Bebop.
//!
//! This crate deliberately does not depend on the `bebop` CLI crate.  It is a
//! small host-side API gateway which can later be connected to a P2E/UART,
//! PCIe, or Ethernet transport without changing its HTTP API.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapability {
    pub id: String,
    pub version: String,
    pub task: String,
    pub input: Value,
    pub output: Value,
    pub streaming: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ModelRegistry {
    models: Vec<ModelCapability>,
}

impl ModelRegistry {
    pub fn new(models: impl IntoIterator<Item = ModelCapability>) -> Self {
        Self { models: models.into_iter().collect() }
    }

    pub fn models(&self) -> &[ModelCapability] {
        &self.models
    }

    pub fn contains(&self, model: &str) -> bool {
        self.models.iter().any(|candidate| candidate.id == model)
    }

    pub fn default_registry() -> Self {
        Self::new([
            ModelCapability {
                id: "bb-lenet".into(),
                version: "0.1.0".into(),
                task: "image-classification".into(),
                input: json!({"type": "image", "formats": ["png", "jpeg", "bmp"], "shape": [1, 1, 28, 28]}),
                output: json!({"type": "topk", "max_k": 10}),
                streaming: false,
            },
            ModelCapability {
                id: "bb-mobilenetv3".into(),
                version: "0.1.0".into(),
                task: "image-classification".into(),
                input: json!({"type": "image", "formats": ["png", "jpeg", "bmp"], "shape": [1, 3, 224, 224]}),
                output: json!({"type": "topk", "max_k": 1000}),
                streaming: false,
            },
            ModelCapability {
                id: "bb-resnet18".into(),
                version: "0.1.0".into(),
                task: "image-classification".into(),
                input: json!({"type": "image", "formats": ["png", "jpeg", "bmp"], "shape": [1, 3, 224, 224]}),
                output: json!({"type": "topk", "max_k": 1000}),
                streaming: false,
            },
            ModelCapability {
                id: "bb-yolo26n".into(),
                version: "0.1.0".into(),
                task: "object-detection".into(),
                input: json!({"type": "image", "formats": ["png", "jpeg", "bmp"]}),
                output: json!({"type": "detections", "box_format": "xyxy"}),
                streaming: false,
            },
            ModelCapability {
                id: "bb-qwen3-0.6b".into(),
                version: "0.1.0".into(),
                task: "text-generation".into(),
                input: json!({"type": "messages", "max_context_tokens": 4096}),
                output: json!({"type": "text", "stream_format": "sse"}),
                streaming: true,
            },
        ])
    }
}

#[derive(Debug, Clone)]
pub enum InferenceRequest {
    ImageClassification { model: String, input: Value },
    Chat { model: String, input: Value, stream: bool },
}

#[derive(Debug, Clone, Serialize)]
pub struct InferenceResult {
    pub model: String,
    pub output: Value,
    pub latency_ms: u64,
    pub fpga_cycles: Option<u64>,
}

pub trait InferenceTransport: Send + Sync {
    fn infer(&self, request: InferenceRequest) -> Result<InferenceResult, String>;
}

/// Development transport. It validates the service path without requiring an FPGA.
#[derive(Debug, Default)]
pub struct MockTransport;

impl InferenceTransport for MockTransport {
    fn infer(&self, request: InferenceRequest) -> Result<InferenceResult, String> {
        let (model, output) = match request {
            InferenceRequest::ImageClassification { model, input } => (
                model,
                json!({
                    "results": [{"label": "mock-class", "class_id": 0, "score": 1.0}],
                    "input_bytes": input.get("input_base64").and_then(Value::as_str).map_or(0, str::len)
                }),
            ),
            InferenceRequest::Chat { model, input, stream } => (
                model,
                json!({
                    "id": "mock-chat-completion",
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": "mock response"}, "text": "mock response"}],
                    "stream": stream,
                    "input": input
                }),
            ),
        };
        Ok(InferenceResult { model, output, latency_ms: 0, fpga_cycles: None })
    }
}

/// Placeholder for the future UART/PCIe/Ethernet implementation.
#[derive(Debug, Default)]
pub struct FpgaTransport;

impl InferenceTransport for FpgaTransport {
    fn infer(&self, _request: InferenceRequest) -> Result<InferenceResult, String> {
        Err("FPGA transport is not connected yet; use --transport mock".into())
    }
}

pub struct Service<T: InferenceTransport + 'static> {
    registry: ModelRegistry,
    api_key: String,
    transport: Arc<T>,
}

impl<T: InferenceTransport + 'static> Service<T> {
    pub fn new(registry: ModelRegistry, api_key: impl Into<String>, transport: T) -> Self {
        Self { registry, api_key: api_key.into(), transport: Arc::new(transport) }
    }

    pub fn serve(self, listen: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(listen)?;
        eprintln!("chipcrowd listening on http://{listen}");
        let service = Arc::new(self);
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let service = Arc::clone(&service);
                    thread::spawn(move || {
                        if let Err(error) = service.handle_connection(stream) {
                            eprintln!("request failed: {error}");
                        }
                    });
                }
                Err(error) => eprintln!("accept failed: {error}"),
            }
        }
        Ok(())
    }

    fn handle_connection(&self, mut stream: TcpStream) -> std::io::Result<()> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end;
        loop {
            let count = stream.read(&mut chunk)?;
            if count == 0 { return Ok(()); }
            buffer.extend_from_slice(&chunk[..count]);
            if let Some(index) = find_bytes(&buffer, b"\r\n\r\n") {
                header_end = index + 4;
                break;
            }
            if buffer.len() > 64 * 1024 { return write_response(&mut stream, 413, json!({"error": "headers too large"})); }
        }
        let header_text = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
        let mut lines = header_text.split("\r\n");
        let request_line = lines.next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let mut content_length = 0_usize;
        let mut authorization = String::new();
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") { content_length = value.trim().parse().unwrap_or(0); }
                if name.eq_ignore_ascii_case("authorization") { authorization = value.trim().to_string(); }
            }
        }
        while buffer.len() - header_end < content_length {
            let count = stream.read(&mut chunk)?;
            if count == 0 { break; }
            buffer.extend_from_slice(&chunk[..count]);
        }
        let body = &buffer[header_end..buffer.len().min(header_end + content_length)];
        let response = self.route(&method, &path, &authorization, body);
        match response {
            Ok((status, value)) => write_response(&mut stream, status, value),
            Err((status, value)) => write_response(&mut stream, status, value),
        }
    }

    fn route(&self, method: &str, path: &str, authorization: &str, body: &[u8]) -> Result<(u16, Value), (u16, Value)> {
        if path == "/healthz" && method == "GET" { return Ok((200, json!({"status": "ok"}))); }
        if authorization != format!("Bearer {}", self.api_key) { return Err((401, json!({"error": "invalid api key"}))); }
        if path == "/v1/models" && method == "GET" { return Ok((200, json!({"data": self.registry.models()}))); }
        let input: Value = serde_json::from_slice(body).map_err(|_| (400, json!({"error": "invalid JSON body"})))?;
        let model = input.get("model").and_then(Value::as_str).ok_or((400, json!({"error": "model is required"})))?;
        if !self.registry.contains(model) { return Err((404, json!({"error": format!("model is not registered: {model}")}))); }
        let request = match (method, path) {
            ("POST", "/v1/vision/classify") => InferenceRequest::ImageClassification { model: model.into(), input },
            ("POST", "/v1/chat/completions") => InferenceRequest::Chat { model: model.into(), stream: input.get("stream").and_then(Value::as_bool).unwrap_or(false), input },
            _ => return Err((404, json!({"error": "route not found"}))),
        };
        self.transport.infer(request).map(|result| (200, serde_json::to_value(result).unwrap_or_else(|_| json!({"error": "serialization failure"})))).map_err(|error| (502, json!({"error": error})))
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn write_response(stream: &mut TcpStream, status: u16, body: Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(&body).unwrap_or_else(|_| b"{\"error\":\"serialization failure\"}".to_vec());
    let reason = match status { 200 => "OK", 400 => "Bad Request", 401 => "Unauthorized", 404 => "Not Found", 413 => "Payload Too Large", 502 => "Bad Gateway", _ => "Error" };
    write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_initial_models() {
        let registry = ModelRegistry::default_registry();
        assert!(registry.contains("bb-mobilenetv3"));
        assert!(registry.contains("bb-qwen3-0.6b"));
    }

    #[test]
    fn mock_transport_returns_result() {
        let result = MockTransport.infer(InferenceRequest::Chat { model: "bb-qwen3-0.6b".into(), input: json!({}), stream: true }).unwrap();
        assert_eq!(result.model, "bb-qwen3-0.6b");
    }
}
