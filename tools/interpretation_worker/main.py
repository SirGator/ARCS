from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import urlparse


@dataclass(frozen=True)
class WorkerConfig:
    host: str = "127.0.0.1"
    port: int = 8090
    config_path: Path = Path("config/arcs.yaml")


def parse_simple_yaml(path: Path) -> dict[str, str]:
    data: dict[str, str] = {}
    if not path.exists():
        return data

    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        key = key.strip()
        value = value.strip().strip('"').strip("'")
        data[key] = value
    return data


def load_prompt_text(config_dir: Path, config: dict[str, str]) -> str:
    prompt_file = config.get("prompt_file", "")
    if not prompt_file:
        return "You are the ARCS interpretation worker. Return structured JSON only."

    path = Path(prompt_file)
    if not path.is_absolute():
        path = (config_dir / path).resolve()

    if not path.exists():
        return "You are the ARCS interpretation worker. Return structured JSON only."

    return path.read_text(encoding="utf-8")


def parse_key_value_input(text: str) -> dict[str, str]:
    result: dict[str, str] = {}
    for token in text.split():
        if "=" not in token:
            continue
        key, value = token.split("=", 1)
        if key:
            result[key.strip()] = value.strip()
    return result


def parse_control_fields(text: str) -> dict[str, str]:
    parsed = parse_key_value_input(text)
    lowered = text.lower()

    patterns = {
        "approval": r"approval[^a-z0-9]+(yes|no|true|false)",
        "permission": r"permission[^a-z0-9]+(yes|no|true|false)",
        "policy_drift": r"policy(?:_|\s)?drift[^a-z0-9]+(yes|no|true|false)",
    }
    for key, pattern in patterns.items():
        if key in parsed:
            continue
        match = re.search(pattern, lowered)
        if match:
            parsed[key] = match.group(1)

    return parsed


def json_response(ok: bool, payload: Any = None, error: str = "") -> bytes:
    body = {"ok": ok, "payload": payload, "error": error}
    return json.dumps(body, ensure_ascii=True).encode("utf-8")


class InterpretationHandler(BaseHTTPRequestHandler):
    server_version = "ARCSInterpretationWorker/1.0"

    def do_GET(self) -> None:  # noqa: N802
        route = urlparse(self.path).path
        if route == "/health":
            self._send_json(HTTPStatus.OK, {"ok": True})
            return
        self._send_json(HTTPStatus.NOT_FOUND, {"ok": False, "error": "not found"})

    def do_POST(self) -> None:  # noqa: N802
        route = urlparse(self.path).path
        body = self._read_json_body()
        if body is None:
            self._send_json(HTTPStatus.BAD_REQUEST, {"ok": False, "error": "invalid json"})
            return

        if route == "/input":
            self._handle_input(body)
            return
        if route == "/schema":
            self._handle_schema(body)
            return
        if route == "/prompt":
            self._handle_prompt(body)
            return
        if route == "/output":
            self._handle_output(body)
            return

        self._send_json(HTTPStatus.NOT_FOUND, {"ok": False, "error": "not found"})

    def _handle_input(self, body: dict[str, Any]) -> None:
        raw_input = str(body.get("raw_input", "")).strip()
        if not raw_input:
            self._send_json(HTTPStatus.BAD_REQUEST, {"ok": False, "error": "raw_input missing"})
            return

        parsed = parse_control_fields(raw_input)
        payload = {
            "type": "ingress_event",
            "request_id": body.get("request_id", ""),
            "source_kind": body.get("source_kind", "chat"),
            "source_ref": body.get("source_ref", "external"),
            "raw_input": raw_input,
            "schema_id": "interpretation/input/v1",
            "parsed": parsed,
        }
        self._send_json(HTTPStatus.OK, {"ok": True, "payload": payload, "error": ""})

    def _handle_schema(self, body: dict[str, Any]) -> None:
        schema_id = str(body.get("schema_id", "")).strip() or "interpretation/input/v1"
        payload = schema_registry().get(schema_id)
        if payload is None:
            self._send_json(HTTPStatus.NOT_FOUND, {"ok": False, "error": f"unknown schema: {schema_id}"})
            return
        self._send_json(HTTPStatus.OK, {"ok": True, "payload": payload, "error": ""})

    def _handle_prompt(self, body: dict[str, Any]) -> None:
        prompt_id = str(body.get("prompt_id", "")).strip() or "default"
        prompt = self.server.worker_prompt if prompt_id == "default" else None
        if prompt is None:
            self._send_json(HTTPStatus.NOT_FOUND, {"ok": False, "error": f"unknown prompt: {prompt_id}"})
            return
        self._send_json(HTTPStatus.OK, {"ok": True, "payload": {"prompt_id": prompt_id, "prompt": prompt}, "error": ""})

    def _handle_output(self, body: dict[str, Any]) -> None:
        request_id = str(body.get("request_id", "")).strip()
        interpreted_payload = body.get("interpreted_payload")
        if not request_id:
            self._send_json(HTTPStatus.BAD_REQUEST, {"ok": False, "error": "request_id missing"})
            return
        if interpreted_payload is None:
            self._send_json(HTTPStatus.BAD_REQUEST, {"ok": False, "error": "interpreted_payload missing"})
            return
        payload = {
            "type": "interpreted_output",
            "request_id": request_id,
            "accepted": True,
            "interpreted_payload": interpreted_payload,
        }
        self._send_json(HTTPStatus.OK, {"ok": True, "payload": payload, "error": ""})

    def _read_json_body(self) -> dict[str, Any] | None:
        content_length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(content_length) if content_length > 0 else b"{}"
        try:
            decoded = raw.decode("utf-8")
            value = json.loads(decoded)
            return value if isinstance(value, dict) else None
        except (UnicodeDecodeError, json.JSONDecodeError):
            return None

    def _send_json(self, status: HTTPStatus, payload: dict[str, Any]) -> None:
        body = json.dumps(payload, ensure_ascii=True).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt: str, *args: Any) -> None:
        return


def schema_registry() -> dict[str, dict[str, Any]]:
    return {
        "interpretation/input/v1": {
            "type": "object",
            "required": ["type", "request_id", "source_kind", "source_ref", "raw_input"],
            "properties": {
                "type": {"const": "ingress_event"},
                "request_id": {"type": "string"},
                "source_kind": {"type": "string"},
                "source_ref": {"type": "string"},
                "raw_input": {"type": "string"},
                "schema_id": {"type": "string"},
                "parsed": {"type": "object"},
            },
        },
        "interpretation/output/v1": {
            "type": "object",
            "required": ["type", "request_id", "accepted", "interpreted_payload"],
            "properties": {
                "type": {"const": "interpreted_output"},
                "request_id": {"type": "string"},
                "accepted": {"type": "boolean"},
                "interpreted_payload": {},
            },
        },
    }


def build_server(cfg: WorkerConfig) -> ThreadingHTTPServer:
    config_data = parse_simple_yaml(cfg.config_path)
    prompt_text = load_prompt_text(cfg.config_path.parent, config_data)
    server = ThreadingHTTPServer((cfg.host, cfg.port), InterpretationHandler)
    server.worker_prompt = prompt_text  # type: ignore[attr-defined]
    return server


def parse_args() -> WorkerConfig:
    parser = argparse.ArgumentParser(description="ARCS interpretation worker")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default=8090, type=int)
    parser.add_argument("--config", default="config/arcs.yaml")
    args = parser.parse_args()
    return WorkerConfig(host=args.host, port=args.port, config_path=Path(args.config))


def main() -> int:
    cfg = parse_args()
    server = build_server(cfg)
    print(f"ARCS interpretation worker listening on http://{cfg.host}:{cfg.port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
