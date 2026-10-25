import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arcs_panel.controller import Configuration, PROJECT_ROOT
from arcs_panel.credentials import Credentials
from arcs_panel.model_connections import ModelConnections
from desktop_helpers import render_window

try:
    from arcs_panel.app import ControlWindow, Gtk
    DISPLAY = Gtk.init_check()[0]
except (ImportError, ValueError):
    DISPLAY = False


def pump_until(predicate, timeout=20):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)
        if predicate():
            return
        time.sleep(.02)
    raise AssertionError("Model desktop condition timed out")


class ModelServer:
    def __init__(self):
        self.requests = []
        self.failure = False
        owner = self
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                owner.requests.append((dict(self.headers), body))
                if owner.failure:
                    self.send_response(401); self.end_headers()
                    self.wfile.write(b"do-not-show: ui-test-secret")
                    return
                content = body["messages"][1]["content"]
                if content.startswith("Reply with JSON"):
                    result = {"ok": True}
                else:
                    inv = json.loads(content)
                    goal = inv["constraints"]["goal_runtime"]
                    versions = {item["version_id"]: item for item in inv["context"]}
                    if inv["target_schema"]["$id"] == "arcs.fan_candidate.v1":
                        payload, capabilities = {"device_id": "fan-01", "enabled": True}, inv["allowed_capabilities"]
                    else:
                        current = goal["after"] if goal["phase"] == "assess" else goal["before"]
                        enabled = versions[current["fan-state"]]["payload"]["enabled"]
                        payload = {"goal_status": "achieved" if enabled else "continue", "effect": "success" if goal["phase"] == "assess" else "unknown",
                                   "detail": "Der Lüfter läuft, der frische Zustand bestätigt die Wirkung.", "evidence_version_ids": list(current.values())}
                        capabilities = []
                    result = {"status": "solved", "explanation": "GUI-Verbindungstest", "candidates": [{"payload": payload, "required_capabilities": capabilities, "referenced_versions": list(versions)}]}
                text = json.dumps(result)
                if body["model"] == "assessor-local":
                    response = {"done": True, "done_reason": "stop", "message": {"content": text}, "eval_count": 40}
                else:
                    response = {"choices": [{"finish_reason": "stop", "message": {"content": text}}], "usage": {"completion_tokens": 40}}
                raw = json.dumps(response).encode()
                self.send_response(200); self.send_header("Content-Length", str(len(raw))); self.end_headers(); self.wfile.write(raw)
            def log_message(self, *_args):
                pass
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={"poll_interval": .02}, daemon=True)
        self.thread.start()
        self.endpoint = f"http://127.0.0.1:{self.server.server_port}/api/chat"

    def close(self):
        self.server.shutdown(); self.server.server_close(); self.thread.join()


@unittest.skipUnless(DISPLAY and (PROJECT_ROOT.parent / "llm-api/arcs-llm-api").is_file(), "GTK-Desktop oder externe Modellanbindung fehlt")
class ModelDesktopTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="arcs-model-desktop-")
        self.directory = Path(self.temporary.name)
        self.connections = ModelConnections(PROJECT_ROOT, self.directory / "models", Credentials(False))
        self.window = ControlWindow(configuration=Configuration(PROJECT_ROOT / "apps/adapter-host/config/text-input/host.json"),
            persist_settings=False, model_connections=self.connections)
        self.page = self.window.models_page
        self.window.show_page("models")
        self.server = ModelServer()
        pump_until(lambda: not self.page.pending)
        self.assertGreaterEqual(len(self.page.providers), 5)

    def tearDown(self):
        if self.window.controller.busy:
            self.window.controller.force_stop()
            pump_until(lambda: not self.window.controller.busy)
        self.window.destroy()
        self.server.close()
        self.temporary.cleanup()

    def fill(self, provider, model, key=None):
        self.page.new_button.emit("clicked")
        self.page.provider_combo.set_active_id(provider)
        self.page.name_entry.set_text(model)
        self.page.model_entry.set_text(model)
        self.page.endpoint_entry.set_text(self.server.endpoint)
        if key:
            self.page.key_entry.set_text(key)

    def test_connect_test_save_assign_and_complete_a_text_task_with_two_different_models(self):
        self.fill("openai_chat", "planner-api", "ui-test-secret")
        self.page.test_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        self.assertIn("Verbindung erfolgreich", self.page.status.get_text())
        self.page.save_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        self.assertIn("Verbindung gespeichert", self.page.status.get_text())
        self.assertEqual(self.page.key_entry.get_text(), "")
        planner = self.connections.records[0]["id"]
        self.fill("ollama", "assessor-local")
        self.assertFalse(self.page.requires_key.get_active())
        self.page.test_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        self.assertIn("Verbindung erfolgreich", self.page.status.get_text())
        self.page.save_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        self.assertEqual(len(self.connections.records), 2)
        assessor = next(item["id"] for item in self.connections.records if item["model"] == "assessor-local")
        self.page.planner_combo.set_active_id(planner)
        self.page.assessor_combo.set_active_id(assessor)
        render_window(self.window, PROJECT_ROOT / "var/control-panel/screenshots/models.png")
        self.page.use_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        self.assertEqual(self.window.stack.get_visible_child_name(), "input")
        self.assertEqual(self.window.preset_combo.get_active_text(), "Meine Modelle · Textaufgaben")
        config = self.window.current_configuration()
        self.window.start_button.emit("clicked")
        pump_until(lambda: self.window.state == "running" and bool(self.window.input_page.fields))
        self.assertFalse(self.page.use_button.get_sensitive())
        self.window.input_page.fields["raw_text"][1].get_buffer().set_text("Sorge bitte dafür, dass der Lüfter läuft.")
        self.window.input_page.send_button.emit("clicked")
        pump_until(lambda: self.window.snapshot.get("runtime", {}).get("completed_goals") == 1)
        self.assertIn("Ziel erreicht", self.window.input_page.status.get_text())
        with sqlite3.connect(config.host.parent / "fan.sqlite") as database:
            self.assertEqual(database.execute("SELECT enabled,actuation_count FROM fan_state").fetchone(), (1, 1))
        self.assertEqual(len(self.server.requests), 5)
        for headers, body in self.server.requests:
            if body["model"] == "planner-api":
                self.assertEqual(headers["Authorization"], "Bearer ui-test-secret")
            else:
                self.assertNotIn("Authorization", headers)
        for path in (self.directory / "models").rglob("*"):
            if path.is_file() and path.suffix != ".sqlite":
                self.assertNotIn(b"ui-test-secret", path.read_bytes())
        self.assertFalse(any("ui-test-secret" in line for line in self.window.log_lines))
        render_window(self.window, PROJECT_ROOT / "var/control-panel/screenshots/models-task.png")
        self.window.stop_button.emit("clicked")
        pump_until(lambda: not self.window.controller.busy and self.window.state == "idle")

    def test_model_test_failure_is_visible_without_starting_the_host_or_disclosing_key(self):
        self.fill("openai_chat", "planner-api", "ui-test-secret")
        self.server.failure = True
        self.page.test_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        self.assertIn("HTTP 401", self.page.status.get_text())
        self.assertNotIn("ui-test-secret", self.page.status.get_text())
        self.assertFalse(self.window.controller.busy)
        self.assertFalse((self.directory / "models/connections.json").exists())

    def test_reopening_saved_settings_requires_a_new_session_key_and_preserves_model_choices(self):
        self.fill("openai_chat", "planner-api", "ui-test-secret")
        self.page.save_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        identifier = self.connections.records[0]["id"]
        self.connections.assign(identifier, identifier)
        config = self.connections.materialize()
        self.window.destroy()
        restored = ModelConnections(PROJECT_ROOT, self.directory / "models", Credentials(False))
        self.window = ControlWindow(configuration=config, persist_settings=False, model_connections=restored)
        self.page = self.window.models_page
        self.window.show_page("models")
        pump_until(lambda: not self.page.pending)
        self.page.saved_combo.set_active_id(identifier)
        self.assertEqual(self.page.model_entry.get_text(), "planner-api")
        self.assertEqual(self.page.key_entry.get_text(), "")
        self.assertIn("nur für dieses geöffnete Kontrollpanel", self.page.key_hint.get_text())
        self.page.use_button.emit("clicked")
        pump_until(lambda: not self.page.pending)
        self.assertIn("API-Schlüssel fehlt", self.page.status.get_text())
        self.assertEqual(self.window.stack.get_visible_child_name(), "models")


if __name__ == "__main__":
    unittest.main()
