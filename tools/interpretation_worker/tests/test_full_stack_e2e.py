from __future__ import annotations

import os
import json
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


ARCS_ROOT = Path(__file__).resolve().parents[3]
WORKER_ROOT = ARCS_ROOT
PARSER_ROOT = ARCS_ROOT.parent / "text-to-json-parser"


def _reserve_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        sock.listen(1)
        return int(sock.getsockname()[1])


def _preferred_python(root: Path) -> str:
    candidate = root / ".venv" / "bin" / "python"
    if candidate.exists() and os.access(candidate, os.X_OK):
        try:
            probe = subprocess.run(
                [str(candidate), "--version"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=5,
                check=False,
            )
            if probe.returncode == 0:
                return str(candidate)
        except (OSError, subprocess.SubprocessError):
            pass
    return sys.executable


def _module_available(python_executable: str, module_name: str) -> bool:
    try:
        probe = subprocess.run(
            [python_executable, "-c", f"import {module_name}"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return False
    return probe.returncode == 0


def _wait_for_health(url: str, process: subprocess.Popen[str], timeout: float = 20.0) -> None:
    deadline = time.time() + timeout
    last_error = "service did not become healthy"
    while time.time() < deadline:
        if process.poll() is not None:
            stderr = process.stderr.read() if process.stderr else ""
            raise RuntimeError(f"process exited early: {stderr.strip()}")
        try:
            with urllib.request.urlopen(url, timeout=1) as response:
                if response.status == 200:
                    return
        except urllib.error.URLError as exc:
            last_error = str(exc)
        time.sleep(0.2)

    raise RuntimeError(last_error)


class FullStackEndToEndTests(unittest.TestCase):
    def setUp(self) -> None:
        app_path = os.environ.get("ARCS_APP_PATH", "").strip()
        if not app_path:
            self.skipTest("ARCS_APP_PATH is not set")
        self.arcs_app_path = Path(app_path)
        if not self.arcs_app_path.exists():
            self.skipTest(f"arcs_app not found: {self.arcs_app_path}")

        self._processes: list[subprocess.Popen[str]] = []
        self._tempdir = tempfile.TemporaryDirectory(prefix="arcs-e2e-")
        self.addCleanup(self._cleanup)

    def _cleanup(self) -> None:
        for process in reversed(self._processes):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
        self._tempdir.cleanup()

    def _start_parser(self) -> int:
        if not PARSER_ROOT.exists():
            self.skipTest(f"text-to-json-parser not found: {PARSER_ROOT}")
        parser_port = _reserve_port()
        parser_python = _preferred_python(PARSER_ROOT)
        if not _module_available(parser_python, "uvicorn"):
            self.skipTest(f"uvicorn not available for parser interpreter: {parser_python}")
        env = os.environ.copy()
        env["LLM_BACKEND"] = "echo"
        process = subprocess.Popen(
            [
                parser_python,
                "-m",
                "uvicorn",
                "main:app",
                "--host",
                "127.0.0.1",
                "--port",
                str(parser_port),
            ],
            cwd=PARSER_ROOT,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self._processes.append(process)
        _wait_for_health(f"http://127.0.0.1:{parser_port}/health", process)
        return parser_port

    def _start_worker(self, parser_url: str) -> int:
        worker_port = _reserve_port()
        worker_python = _preferred_python(WORKER_ROOT)
        if not _module_available(worker_python, "httpx"):
            self.skipTest(f"httpx not available for worker interpreter: {worker_python}")
        worker_config = Path(self._tempdir.name) / "worker.yaml"
        worker_config.write_text("", encoding="utf-8")
        env = os.environ.copy()
        env["PYTHONPATH"] = str(ARCS_ROOT)
        process = subprocess.Popen(
            [
                worker_python,
                "-m",
                "tools.interpretation_worker",
                "--host",
                "127.0.0.1",
                "--port",
                str(worker_port),
                "--config",
                str(worker_config),
                "--parser-url",
                parser_url,
            ],
            cwd=ARCS_ROOT,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self._processes.append(process)
        _wait_for_health(f"http://127.0.0.1:{worker_port}/health", process)
        return worker_port

    def _run_arcs_app(self, worker_port: int, user_input: str) -> subprocess.CompletedProcess[str]:
        app_config = Path(self._tempdir.name) / "arcs.yaml"
        app_config.write_text(
            f"interpret_api_url: http://127.0.0.1:{worker_port}/interpret\n",
            encoding="utf-8",
        )
        return subprocess.run(
            [str(self.arcs_app_path), "--config", str(app_config)],
            cwd=ARCS_ROOT,
            input=user_input + "\n",
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )

    def _start_fake_worker(self, payload: dict[str, object]) -> int:
        worker_port = _reserve_port()

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                if self.path != "/interpret":
                    self.send_error(404)
                    return

                content_length = int(self.headers.get("Content-Length", "0"))
                if content_length:
                    self.rfile.read(content_length)

                body = json.dumps(
                    {
                        "ok": True,
                        "request_id": "req_fake",
                        "schema_id": "arcs.interpretation_proposal.v1",
                        "payload": payload,
                    }
                ).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, format: str, *args: object) -> None:
                return

        server = ThreadingHTTPServer(("127.0.0.1", worker_port), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()

        def cleanup() -> None:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

        self.addCleanup(cleanup)
        return worker_port

    def test_free_text_round_trip_reaches_core_and_executes(self) -> None:
        parser_port = self._start_parser()
        worker_port = self._start_worker(f"http://127.0.0.1:{parser_port}")

        result = self._run_arcs_app(
            worker_port,
            "bitte erstelle einen bericht als json ueber die letzten pruefergebnisse",
        )

        self.assertEqual(result.returncode, 0, msg=result.stderr)
        self.assertIn("step: interpret -> OK", result.stdout)
        self.assertIn("step: interpretation artifact -> OK", result.stdout)
        self.assertIn("step: interpretation_verification_report -> OK | pass", result.stdout)
        self.assertIn("interpretation: external worker accepted", result.stdout)
        self.assertIn("step: verification_report -> FAIL", result.stdout)
        self.assertIn("decision: blocked", result.stdout)

    def test_free_text_blocks_when_parser_is_unreachable(self) -> None:
        worker_port = self._start_worker("http://127.0.0.1:1")

        result = self._run_arcs_app(
            worker_port,
            "bitte erstelle einen bericht als json ueber die letzten pruefergebnisse",
        )

        self.assertEqual(result.returncode, 0, msg=result.stderr)
        self.assertIn("step: interpret -> FAIL", result.stdout)
        self.assertIn("decision: blocked", result.stdout)
        self.assertIn("reason: free text interpretation unavailable", result.stdout)

    def test_free_text_does_not_treat_interpretation_flags_as_authority(self) -> None:
        worker_port = self._start_fake_worker(
            {
                "status": "ok",
                "intent": {
                    "name": "generate_report",
                    "category": "reporting",
                    "description": "Generate a report",
                },
                "confidence": 0.91,
                "slots": {},
                "missing_required_fields": [],
                "next_step": "ask",
                "parsed": {
                    "approval": True,
                    "permission": True,
                    "policy_drift": False,
                }
            }
        )

        result = self._run_arcs_app(
            worker_port,
            "bitte erstelle einen bericht als json ueber die letzten pruefergebnisse",
        )

        self.assertEqual(result.returncode, 0, msg=result.stderr)
        self.assertIn("step: interpret -> OK", result.stdout)
        self.assertIn("step: interpretation artifact -> OK", result.stdout)
        self.assertIn("step: interpretation_verification_report -> OK | pass", result.stdout)
        self.assertIn("decision: blocked", result.stdout)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        os.environ["ARCS_APP_PATH"] = sys.argv[1]
    unittest.main(argv=[sys.argv[0]])
