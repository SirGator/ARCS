"""Tests für die ARCS ↔ text-to-json-parser Brücke.

Diese Tests prüfen die Bridge-Logik isoliert vom HTTP-Layer.
Sie verwenden einen FakeParserClient, der die `interpret()` Methode
überschreibt, damit keine echte HTTP-Verbindung nötig ist.
"""

from __future__ import annotations

import json
import sys
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Thread
from typing import Any

# `tools/` als Paket importierbar machen.
_TOOLS_DIR = Path(__file__).resolve().parents[2]
if str(_TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(_TOOLS_DIR))

from interpretation_worker.parser_client import (  # noqa: E402
    ParserClient,
    ParserClientConfig,
    ParserResult,
)
from interpretation_worker.main import (  # noqa: E402
    INTERPRETATION_PROPOSAL_SCHEMA,
    InterpretationHandler,
    WorkerConfig,
    build_server,
    interpret_via_parser,
)


# ---------------------------------------------------------------------------
# Test-Doubles
# ---------------------------------------------------------------------------


class FakeParserClient:
    """Ein Parser-Client, der vordefinierte Ergebnisse zurückgibt.

    Erfüllt die gleiche Schnittstelle wie `ParserClient.interpret`
    (nimmt text, schema, context, prompt entgegen).
    """

    def __init__(self, result: ParserResult) -> None:
        self._result = result
        self.last_call: dict[str, Any] | None = None

    def interpret(
        self,
        text: str,
        schema: dict[str, Any],
        context: dict[str, Any] | None = None,
        prompt: str | None = None,
    ) -> ParserResult:
        self.last_call = {
            "text": text,
            "schema": schema,
            "context": context,
            "prompt": prompt,
        }
        return self._result


# ---------------------------------------------------------------------------
# Bridge-Logik (interpret_via_parser)
# ---------------------------------------------------------------------------


class InterpretViaParserTests(unittest.TestCase):
    def test_returns_error_when_raw_input_missing(self) -> None:
        client = FakeParserClient(ParserResult(ok=True, data={}))
        outcome = interpret_via_parser({}, client)  # type: ignore[arg-type]
        self.assertFalse(outcome.ok)
        self.assertIn("raw_input", outcome.error)

    def test_passes_raw_input_and_schema_to_parser(self) -> None:
        client = FakeParserClient(
            ParserResult(ok=True, data={"status": "ok", "intent": {}})
        )
        request = {
            "request_id": "req-1",
            "raw_input": "approval=yes permission=yes",
            "schema_id": "arcs.interpretation_proposal.v1",
        }
        outcome = interpret_via_parser(request, client)  # type: ignore[arg-type]

        self.assertTrue(outcome.ok)
        self.assertEqual(client.last_call["text"], "approval=yes permission=yes")
        # Fallback-Schema wurde automatisch eingesetzt.
        self.assertEqual(client.last_call["schema"], INTERPRETATION_PROPOSAL_SCHEMA)
        self.assertEqual(outcome.schema_id, "arcs.interpretation_proposal.v1")
        self.assertEqual(outcome.request_id, "req-1")
        self.assertEqual(outcome.payload, {"status": "ok", "intent": {}})

    def test_uses_caller_supplied_schema(self) -> None:
        caller_schema = {
            "type": "object",
            "properties": {"foo": {"type": "string"}},
        }
        client = FakeParserClient(ParserResult(ok=True, data={"foo": "bar"}))
        outcome = interpret_via_parser(
            {"raw_input": "hi", "schema": caller_schema, "schema_id": "custom.v1"},
            client,  # type: ignore[arg-type]
        )
        self.assertTrue(outcome.ok)
        self.assertEqual(client.last_call["schema"], caller_schema)
        self.assertEqual(outcome.schema_id, "custom.v1")

    def test_uses_default_prompt_when_no_prompt_in_request(self) -> None:
        client = FakeParserClient(ParserResult(ok=True, data={}))
        interpret_via_parser(
            {"raw_input": "hi"},
            client,  # type: ignore[arg-type]
            default_prompt="ARCS default",
        )
        self.assertEqual(client.last_call["prompt"], "ARCS default")

    def test_caller_prompt_overrides_default(self) -> None:
        client = FakeParserClient(ParserResult(ok=True, data={}))
        interpret_via_parser(
            {
                "raw_input": "hi",
                "prompt_config": {"prompt": "caller specific"},
            },
            client,  # type: ignore[arg-type]
            default_prompt="ARCS default",
        )
        self.assertEqual(client.last_call["prompt"], "caller specific")

    def test_propagates_parser_error(self) -> None:
        client = FakeParserClient(
            ParserResult(
                ok=False,
                error_code="invalid_output",
                error_message="schema mismatch",
                error_details=["name: missing"],
            )
        )
        outcome = interpret_via_parser(
            {"raw_input": "hi"}, client  # type: ignore[arg-type]
        )
        self.assertFalse(outcome.ok)
        self.assertIn("schema mismatch", outcome.error)
        self.assertIn("name: missing", outcome.error)


# ---------------------------------------------------------------------------
# ParserClient (HTTP-Layer)
# ---------------------------------------------------------------------------


class _OkHandler(BaseHTTPRequestHandler):
    payload: dict[str, Any] = {"ok": True, "data": {"value": 42}}

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        _ = self.rfile.read(length)
        body = json.dumps(self.payload).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt: str, *args: Any) -> None:
        return


class _ErrorHandler(BaseHTTPRequestHandler):
    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        _ = self.rfile.read(length)
        body = json.dumps({
            "ok": False,
            "data": None,
            "error": {
                "code": "validation_failed",
                "message": "schema kaputt",
                "details": ["age: must be integer"],
            },
        }).encode("utf-8")
        self.send_response(400)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt: str, *args: Any) -> None:
        return


def _start_server(handler_cls: type) -> tuple[ThreadingHTTPServer, Thread]:
    server = ThreadingHTTPServer(("127.0.0.1", 0), handler_cls)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


class ParserClientTests(unittest.TestCase):
    def test_successful_call_returns_data(self) -> None:
        server, _ = _start_server(_OkHandler)
        try:
            client = ParserClient(
                ParserClientConfig(
                    base_url=f"http://127.0.0.1:{server.server_address[1]}",
                    timeout_seconds=5,
                )
            )
            result = client.interpret("hi", {"type": "object"})
            self.assertTrue(result.ok)
            self.assertEqual(result.data, {"value": 42})
        finally:
            server.shutdown()
            server.server_close()

    def test_error_response_is_normalized(self) -> None:
        server, _ = _start_server(_ErrorHandler)
        try:
            client = ParserClient(
                ParserClientConfig(
                    base_url=f"http://127.0.0.1:{server.server_address[1]}",
                    timeout_seconds=5,
                )
            )
            result = client.interpret("hi", {"type": "object"})
            self.assertFalse(result.ok)
            self.assertEqual(result.error_code, "validation_failed")
            self.assertIn("schema kaputt", result.error_message)
            self.assertIn("age: must be integer", result.error_details)
        finally:
            server.shutdown()
            server.server_close()

    def test_unreachable_server_returns_parser_unreachable(self) -> None:
        # Port 1 ist reserviert und in der Regel nicht offen.
        client = ParserClient(
            ParserClientConfig(
                base_url="http://127.0.0.1:1",
                timeout_seconds=1,
            )
        )
        result = client.interpret("hi", {"type": "object"})
        self.assertFalse(result.ok)
        self.assertEqual(result.error_code, "parser_unreachable")


# ---------------------------------------------------------------------------
# End-to-End: HTTP /interpret
# ---------------------------------------------------------------------------


class _WorkerEndToEndTests(unittest.TestCase):
    """Startet den ARCS-Worker mit einem Fake-Parser und ruft /interpret per HTTP."""

    def setUp(self) -> None:
        # Worker auf beliebigem freien Port starten.
        self._worker_cfg = WorkerConfig(
            host="127.0.0.1",
            port=0,  # type: ignore[arg-type]
            parser_url="http://unused:8000",
        )

    def _start_worker(self, fake_result: ParserResult) -> ThreadingHTTPServer:
        fake = FakeParserClient(fake_result)
        cfg = WorkerConfig(
            host="127.0.0.1",
            port=0,  # type: ignore[arg-type]
            parser_url="http://unused:8000",
        )
        server = build_server(cfg, parser_client=fake)  # type: ignore[arg-type]
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.shutdown)
        self.addCleanup(server.server_close)
        return server

    def _post(self, port: int, route: str, body: dict[str, Any]) -> tuple[int, dict[str, Any]]:
        import urllib.request

        req = urllib.request.Request(
            f"http://127.0.0.1:{port}{route}",
            data=json.dumps(body).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=5) as resp:
                return resp.status, json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:  # type: ignore[attr-defined]
            return exc.code, json.loads(exc.read().decode("utf-8"))

    def test_interpret_endpoint_returns_parser_payload(self) -> None:
        fake = FakeParserClient(
            ParserResult(
                ok=True,
                data={
                    "status": "ok",
                    "intent": {"name": "report", "category": "exec", "description": "..."},
                    "confidence": 0.91,
                    "slots": {"approval": "yes"},
                    "missing_required_fields": [],
                    "next_step": "execute",
                },
            )
        )
        cfg = WorkerConfig(
            host="127.0.0.1",
            port=0,  # type: ignore[arg-type]
            parser_url="http://unused:8000",
        )
        server = build_server(cfg, parser_client=fake)  # type: ignore[arg-type]
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            port = server.server_address[1]
            status, body = self._post(
                port,
                "/interpret",
                {
                    "request_id": "r-1",
                    "raw_input": "approval=yes permission=yes",
                    "schema_id": "arcs.interpretation_proposal.v1",
                },
            )
            self.assertEqual(status, 200)
            self.assertTrue(body["ok"])
            self.assertEqual(body["request_id"], "r-1")
            self.assertEqual(body["schema_id"], "arcs.interpretation_proposal.v1")
            self.assertEqual(body["payload"]["status"], "ok")
            self.assertEqual(body["payload"]["intent"]["name"], "report")
        finally:
            server.shutdown()
            server.server_close()

    def test_interpret_endpoint_returns_502_on_parser_failure(self) -> None:
        fake = FakeParserClient(
            ParserResult(
                ok=False,
                error_code="parser_unreachable",
                error_message="text-to-json-parser nicht erreichbar",
            )
        )
        cfg = WorkerConfig(
            host="127.0.0.1",
            port=0,  # type: ignore[arg-type]
            parser_url="http://unused:8000",
        )
        server = build_server(cfg, parser_client=fake)  # type: ignore[arg-type]
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            port = server.server_address[1]
            status, body = self._post(port, "/interpret", {"raw_input": "hi"})
            self.assertEqual(status, 502)
            self.assertFalse(body["ok"])
            self.assertIn("text-to-json-parser", body["error"])
        finally:
            server.shutdown()
            server.server_close()

    def test_legacy_input_endpoint_still_works(self) -> None:
        """Rückwärtskompatibilität für /input, /schema, /prompt, /output."""
        cfg = WorkerConfig(
            host="127.0.0.1",
            port=0,  # type: ignore[arg-type]
            parser_url="http://unused:8000",
        )
        # /input braucht keinen Parser, aber build_server verlangt einen Client.
        # Wir geben einen No-Op Fake.
        fake = FakeParserClient(ParserResult(ok=True, data={}))
        server = build_server(cfg, parser_client=fake)  # type: ignore[arg-type]
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            port = server.server_address[1]
            status, body = self._post(
                port,
                "/input",
                {"raw_input": "approval=yes permission=no", "request_id": "x"},
            )
            self.assertEqual(status, 200)
            self.assertTrue(body["ok"])
            self.assertEqual(body["payload"]["parsed"]["approval"], "yes")
            self.assertEqual(body["payload"]["parsed"]["permission"], "no")
        finally:
            server.shutdown()
            server.server_close()

    def test_health_endpoint(self) -> None:
        import urllib.request

        cfg = WorkerConfig(
            host="127.0.0.1",
            port=0,  # type: ignore[arg-type]
            parser_url="http://unused:8000",
        )
        fake = FakeParserClient(ParserResult(ok=True, data={}))
        server = build_server(cfg, parser_client=fake)  # type: ignore[arg-type]
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            port = server.server_address[1]
            with urllib.request.urlopen(
                f"http://127.0.0.1:{port}/health", timeout=5
            ) as resp:
                self.assertEqual(resp.status, 200)
                body = json.loads(resp.read().decode("utf-8"))
                self.assertTrue(body["ok"])
        finally:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    unittest.main()
