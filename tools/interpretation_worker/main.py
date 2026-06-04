"""ARCS Interpretation Worker.

Dieser Worker ist die Brücke zwischen ARCS Core (C++) und dem
text-to-json-parser (Python/FastAPI).

ARCS ruft einheitlich `POST /interpret` mit raw_input + Ziel-Schema auf.
Der Worker leitet den Aufruf intern an den text-to-json-parser weiter
und gibt das schema-konforme Proposal als ARCS-Payload zurück.

Zusätzlich exponiert der Worker die historischen Endpoints
`/input`, `/schema`, `/prompt`, `/output` für Tests und Inspektion.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass, field
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

from .parser_client import ParserClient, ParserClientConfig, ParserResult


# ---------------------------------------------------------------------------
# Konfiguration
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class WorkerConfig:
    host: str = "127.0.0.1"
    port: int = 8090
    config_path: Path = Path("config/arcs.yaml")
    # URL des text-to-json-parser (FastAPI). Standard: localhost:8000.
    parser_url: str = "http://127.0.0.1:8000"
    # Timeout für Parser-Aufrufe in Sekunden.
    parser_timeout: float = 120.0
    # Optionaler Default-Prompt, der an den Parser weitergereicht wird,
    # falls der ARCS-Aufrufer keinen eigenen Prompt mitgibt.
    default_parser_prompt: str = ""


# ---------------------------------------------------------------------------
# YAML-Light-Parser (reicht für arcs.yaml)
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# Freitext-Parsing-Helfer (für /input Endpoint und Fallbacks)
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# Schema-Registry (für /schema und Default-interpretation_proposal)
# ---------------------------------------------------------------------------


INTERPRETATION_PROPOSAL_SCHEMA: dict[str, Any] = {
    "$id": "arcs.interpretation_proposal.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "title": "ARCS Interpretation Proposal",
    "type": "object",
    "required": [
        "status",
        "intent",
        "confidence",
        "slots",
        "missing_required_fields",
        "next_step",
    ],
    "properties": {
        "status": {"type": "string"},
        "intent": {
            "type": "object",
            "required": ["name", "category", "description"],
            "properties": {
                "name": {"type": "string"},
                "category": {"type": "string"},
                "description": {"type": "string"},
            },
            "additionalProperties": True,
        },
        "confidence": {"type": "number"},
        "slots": {"type": "object"},
        "missing_required_fields": {
            "type": "array",
            "items": {"type": "string"},
        },
        "next_step": {"type": "string"},
    },
    "additionalProperties": True,
}


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
        "arcs.interpretation_proposal.v1": INTERPRETATION_PROPOSAL_SCHEMA,
    }


# ---------------------------------------------------------------------------
# Bridge: ARCS Interpretation → text-to-json-parser
# ---------------------------------------------------------------------------


@dataclass
class InterpretOutcome:
    """Ergebnis der Bridge-Logik, normalisiert für den HTTP-Layer."""

    ok: bool
    request_id: str = ""
    schema_id: str = ""
    payload: dict[str, Any] = field(default_factory=dict)
    error: str = ""


def interpret_via_parser(
    request_body: dict[str, Any],
    parser_client: ParserClient,
    default_prompt: str = "",
) -> InterpretOutcome:
    """Übersetzt einen ARCS-Interpret-Aufruf an den text-to-json-parser.

    Erwartet mindestens `raw_input`. Optional: `schema`, `schema_id`,
    `context`, `prompt_config`. Wenn kein Schema mitgeschickt wird, fällt
    der Worker auf das eingebaute `arcs.interpretation_proposal.v1` zurück.
    """
    raw_input = str(request_body.get("raw_input", "")).strip()
    if not raw_input:
        return InterpretOutcome(ok=False, error="raw_input missing")

    request_id = str(request_body.get("request_id", ""))
    schema_id = str(request_body.get("schema_id", "")).strip()
    schema = request_body.get("schema")
    context = request_body.get("context")
    prompt_config = request_body.get("prompt_config")

    if not isinstance(schema, dict) or not schema:
        # Fallback: interpretation_proposal Schema
        schema = INTERPRETATION_PROPOSAL_SCHEMA
        if not schema_id:
            schema_id = "arcs.interpretation_proposal.v1"
    elif not schema_id:
        schema_id = "arcs.interpretation_proposal.v1"

    if not isinstance(context, dict):
        context = None

    prompt = ""
    if isinstance(prompt_config, dict):
        prompt = str(prompt_config.get("prompt") or "")
    if not prompt:
        prompt = default_prompt

    result: ParserResult = parser_client.interpret(
        text=raw_input,
        schema=schema,
        context=context,
        prompt=prompt or None,
    )

    if not result.ok:
        details = "; ".join(result.error_details) if result.error_details else ""
        message = result.error_message
        if details:
            message = f"{message} ({details})"
        return InterpretOutcome(
            ok=False,
            request_id=request_id,
            schema_id=schema_id,
            error=message or "parser failed",
        )

    return InterpretOutcome(
        ok=True,
        request_id=request_id,
        schema_id=schema_id,
        payload=result.data or {},
    )


# ---------------------------------------------------------------------------
# HTTP-Handler
# ---------------------------------------------------------------------------


class InterpretationHandler(BaseHTTPRequestHandler):
    server_version = "ARCSInterpretationWorker/1.0"

    # Wird beim Bauen des Servers gesetzt (siehe build_server).
    parser_client: ParserClient = None  # type: ignore[assignment]
    default_parser_prompt: str = ""

    # --- Routing --------------------------------------------------------

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

        if route == "/interpret":
            self._handle_interpret(body)
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

    # --- Handler --------------------------------------------------------

    def _handle_interpret(self, body: dict[str, Any]) -> None:
        """ARCS-Interpretationsaufruf. Delegiert an text-to-json-parser."""
        if self.parser_client is None:
            self._send_json(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                {"ok": False, "error": "parser client not configured"},
            )
            return

        outcome = interpret_via_parser(
            body,
            self.parser_client,
            default_prompt=self.default_parser_prompt,
        )

        response: dict[str, Any] = {
            "ok": outcome.ok,
            "request_id": outcome.request_id,
            "schema_id": outcome.schema_id,
            "payload": outcome.payload,
            "error": outcome.error,
        }
        if not outcome.ok and not response["error"]:
            response["error"] = "interpret failed"

        status = HTTPStatus.OK if outcome.ok else HTTPStatus.BAD_GATEWAY
        self._send_json(status, response)

    def _handle_input(self, body: dict[str, Any]) -> None:
        raw_input = str(body.get("raw_input", "")).strip()
        if not raw_input:
            self._send_json(
                HTTPStatus.BAD_REQUEST, {"ok": False, "error": "raw_input missing"}
            )
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
            self._send_json(
                HTTPStatus.NOT_FOUND,
                {"ok": False, "error": f"unknown schema: {schema_id}"},
            )
            return
        self._send_json(HTTPStatus.OK, {"ok": True, "payload": payload, "error": ""})

    def _handle_prompt(self, body: dict[str, Any]) -> None:
        prompt_id = str(body.get("prompt_id", "")).strip() or "default"
        prompt = self.default_parser_prompt if prompt_id == "default" else None
        if prompt is None:
            self._send_json(
                HTTPStatus.NOT_FOUND,
                {"ok": False, "error": f"unknown prompt: {prompt_id}"},
            )
            return
        self._send_json(
            HTTPStatus.OK,
            {"ok": True, "payload": {"prompt_id": prompt_id, "prompt": prompt}, "error": ""},
        )

    def _handle_output(self, body: dict[str, Any]) -> None:
        request_id = str(body.get("request_id", "")).strip()
        interpreted_payload = body.get("interpreted_payload")
        if not request_id:
            self._send_json(
                HTTPStatus.BAD_REQUEST, {"ok": False, "error": "request_id missing"}
            )
            return
        if interpreted_payload is None:
            self._send_json(
                HTTPStatus.BAD_REQUEST,
                {"ok": False, "error": "interpreted_payload missing"},
            )
            return
        payload = {
            "type": "interpreted_output",
            "request_id": request_id,
            "accepted": True,
            "interpreted_payload": interpreted_payload,
        }
        self._send_json(HTTPStatus.OK, {"ok": True, "payload": payload, "error": ""})

    # --- HTTP-Utilities -------------------------------------------------

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


# ---------------------------------------------------------------------------
# Server-Bau und CLI
# ---------------------------------------------------------------------------


def build_server(
    cfg: WorkerConfig,
    parser_client: ParserClient | None = None,
) -> ThreadingHTTPServer:
    config_data = parse_simple_yaml(cfg.config_path)
    prompt_text = load_prompt_text(cfg.config_path.parent, config_data)

    # Env-Overrides haben Vorrang vor YAML.
    parser_url = os.getenv("ARCS_PARSER_URL") or config_data.get("parser_url") or cfg.parser_url
    parser_timeout_raw = os.getenv("ARCS_PARSER_TIMEOUT") or config_data.get("parser_timeout")
    try:
        parser_timeout = float(parser_timeout_raw) if parser_timeout_raw else cfg.parser_timeout
    except ValueError:
        parser_timeout = cfg.parser_timeout

    default_prompt_path = os.getenv("ARCS_PARSER_PROMPT_FILE") or config_data.get("parser_prompt_file")
    default_prompt = cfg.default_parser_prompt
    if default_prompt_path:
        ppath = Path(default_prompt_path)
        if not ppath.is_absolute():
            ppath = (cfg.config_path.parent / ppath).resolve()
        if ppath.exists():
            default_prompt = ppath.read_text(encoding="utf-8")
    elif prompt_text:
        default_prompt = prompt_text

    if parser_client is None:
        parser_client = ParserClient(
            ParserClientConfig(
                base_url=parser_url,
                timeout_seconds=parser_timeout,
            )
        )

    server = ThreadingHTTPServer((cfg.host, cfg.port), InterpretationHandler)
    InterpretationHandler.parser_client = parser_client
    InterpretationHandler.default_parser_prompt = default_prompt
    return server


def parse_args() -> WorkerConfig:
    parser = argparse.ArgumentParser(description="ARCS interpretation worker")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default=8090, type=int)
    parser.add_argument("--config", default="config/arcs.yaml")
    parser.add_argument(
        "--parser-url",
        default=os.getenv("ARCS_PARSER_URL", "http://127.0.0.1:8000"),
        help="URL des text-to-json-parser Service",
    )
    parser.add_argument(
        "--parser-timeout",
        default=float(os.getenv("ARCS_PARSER_TIMEOUT", "120")),
        type=float,
        help="HTTP-Timeout für Parser-Aufrufe in Sekunden",
    )
    args = parser.parse_args()
    return WorkerConfig(
        host=args.host,
        port=args.port,
        config_path=Path(args.config),
        parser_url=args.parser_url,
        parser_timeout=args.parser_timeout,
    )


def main() -> int:
    cfg = parse_args()
    server = build_server(cfg)
    print(
        f"ARCS interpretation worker listening on http://{cfg.host}:{cfg.port}\n"
        f"  -> text-to-json-parser: {cfg.parser_url}",
        file=sys.stderr,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
