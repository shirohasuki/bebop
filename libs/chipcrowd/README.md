# ChipCrowd

This is intentionally a standalone crate. It is not wired into the `bebop`
CLI yet.

Run a local API server with the development transport:

```bash
cargo run --manifest-path bebop/libs/chipcrowd/Cargo.toml -- \
  --listen 127.0.0.1:8080 --api-key bbk-dev --transport mock
```

Check health and models:

```bash
curl http://127.0.0.1:8080/healthz
curl -H 'Authorization: Bearer bbk-dev' \
  http://127.0.0.1:8080/v1/models
```

Call the mock inference endpoints:

```bash
curl -H 'Authorization: Bearer bbk-dev' \
  -H 'Content-Type: application/json' \
  -d '{"model":"bb-mobilenetv3","input_base64":"...","top_k":5}' \
  http://127.0.0.1:8080/v1/vision/classify

curl -H 'Authorization: Bearer bbk-dev' \
  -H 'Content-Type: application/json' \
  -d '{"model":"bb-qwen3-0.6b","messages":[{"role":"user","content":"hello"}],"stream":false}' \
  http://127.0.0.1:8080/v1/chat/completions
```

`--transport fpga` is deliberately a placeholder until the P2E/UART framed
transport is implemented. It returns a clear `502` rather than pretending to
have connected to an FPGA.
