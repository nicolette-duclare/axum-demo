# axum-demo

Ready-to-use Axum demo app that returns **JSON for every API endpoint**.

## Endpoints

- `GET /` — hello endpoint
- `GET /healthz` — health/status endpoint with uptime, IP address, hostname, PID, service name, and version
- `GET /proxy?url=<http(s)-json-endpoint>` — simple JSON proxy to another endpoint
- `GET /openapi.json` — generated OpenAPI specification in JSON
- `GET /docs` — Swagger UI for interactive API docs

## Notes about `/proxy`

- only `http` and `https` URLs are allowed
- localhost-style targets are blocked by default
- upstream response is expected to be JSON

## Run

```bash
cargo run
```

Server listens on:

```text
0.0.0.0:3000
```

## API documentation

After starting the server, open:

- Swagger UI: `http://127.0.0.1:3000/docs`
- OpenAPI JSON: `http://127.0.0.1:3000/openapi.json`

## Example requests

```bash
curl http://127.0.0.1:3000/
curl http://127.0.0.1:3000/healthz
curl http://127.0.0.1:3000/openapi.json
curl 'http://127.0.0.1:3000/proxy?url=https%3A%2F%2Fhttpbin.org%2Fjson'
```

## Test

```bash
cargo test
```

## Example response: `/`

```json
{
  "ok": true,
  "data": {
    "message": "hello from axum-demo",
    "service": "axum-demo",
    "version": "0.1.0"
  }
}
```

## Example response: `/healthz`

```json
{
  "ok": true,
  "data": {
    "status": "ok",
    "uptime_seconds": 12,
    "ip_address": "192.168.1.10",
    "hostname": "demo-host",
    "process_id": 12345,
    "rust_env": "development",
    "service": "axum-demo",
    "version": "0.1.0"
  }
}
```
