"""GUI-unabhängige Prozesssteuerung. Ausschließlich eigene Kindprozesse."""
from __future__ import annotations

import fcntl
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import signal
import subprocess
import tempfile
import threading
from dataclasses import dataclass

from .input_bridge import configured_inputs, connected_inputs, make_submission, enqueue_submission


PROJECT_ROOT = Path(__file__).resolve().parents[3]


@dataclass(frozen=True)
class Configuration:
    host: Path
    runtime: Path | None = None


def read_configuration(configuration: Configuration) -> dict:
    """Nur Vorschau. Die verbindliche Prüfung bleibt beim Rust-Host."""
    host = configuration.host.expanduser().resolve()
    with host.open(encoding="utf-8") as reader:
        document = json.load(reader)
    if not isinstance(document, dict) or document.get("config_version") != 1:
        raise ValueError("Die Host-Konfiguration benötigt config_version 1.")
    adapters = document.get("adapters")
    database = document.get("database")
    if not isinstance(adapters, list) or not adapters:
        raise ValueError("Die Konfiguration enthält keine Adapter.")
    if not isinstance(database, str) or not database.strip():
        raise ValueError("In der Host-Konfiguration fehlt der Datenbankpfad.")
    preview, programs, names = [], [], set()
    for adapter in adapters:
        if not isinstance(adapter, dict) or not isinstance(adapter.get("grant_file"), str):
            raise ValueError("Ein Adapter hat keine gültige Grant-Datei.")
        grant_path = host.parent / adapter["grant_file"]
        with grant_path.open(encoding="utf-8") as reader:
            grant = json.load(reader)
        if not isinstance(grant, dict):
            raise ValueError("Die Grant-Datei muss ein JSON-Objekt enthalten.")
        adapter_id = grant.get("adapter_id")
        if not isinstance(adapter_id, str) or not adapter_id or adapter_id in names:
            raise ValueError("Adapter benötigen unterschiedliche, nicht leere IDs.")
        names.add(adapter_id)
        preview.append({"adapter_id": adapter_id, "capabilities": [], "configured": True})
        program = adapter.get("program")
        if not isinstance(program, str) or not program:
            raise ValueError("Ein Adapter hat keinen Programmpfad.")
        programs.append((host.parent / program).resolve())
    if configuration.runtime:
        with configuration.runtime.expanduser().resolve().open(encoding="utf-8") as reader:
            runtime = json.load(reader)
        if not isinstance(runtime, dict) or runtime.get("config_version") != 1:
            raise ValueError("Die Aufgaben-Runtime benötigt config_version 1.")
    return {"database": (host.parent / database).resolve(), "adapters": preview, "programs": programs,
        "input_candidates": configured_inputs(host, adapters)}


class SessionController:
    def __init__(self, root: Path = PROJECT_ROOT):
        self.root = root.resolve()
        self.events: queue.SimpleQueue = queue.SimpleQueue()
        self._lock = threading.Lock()
        self._process: subprocess.Popen | None = None
        self._stop = threading.Event()
        self._forced = threading.Event()
        self._stop_file: Path | None = None
        self._busy = False
        self._input_candidates = []
        self._input_targets = []
        self._accepting_input = False
        self._submission_thread = None
        self._submission_pending = False
        self._retry_submission = None

    @property
    def busy(self) -> bool:
        with self._lock:
            return self._busy

    @property
    def submission_pending(self) -> bool:
        with self._lock:
            return self._submission_pending

    def submit_input(self, key: str, payload) -> str:
        with self._lock:
            if not self._accepting_input or self._stop.is_set():
                raise RuntimeError("Bitte ARCS starten und einen verbundenen Eingang auswählen.")
            if self._submission_pending:
                raise RuntimeError("Eine Eingabe wird bereits übertragen.")
            target = next((target for target in self._input_targets if target.key == key), None)
            if target is None:
                raise ValueError("Dieser Eingang gehört nicht zur laufenden Sitzung.")
            event = make_submission(target, payload)
            if self._retry_submission:
                previous_target, previous_event = self._retry_submission
                if previous_target == target and json.dumps(previous_event["payload"], sort_keys=True, allow_nan=False) == json.dumps(event["payload"], sort_keys=True, allow_nan=False):
                    event = previous_event
            self._submission_pending = True
            self._submission_thread = threading.Thread(target=self._submit_worker, args=(target, event), daemon=True)
            self._submission_thread.start()
        return event["id"]

    def _submit_worker(self, target, event):
        try:
            enqueue_submission(target, event, self.root / "target/debug/arcs-host")
        except Exception as error:
            with self._lock:
                self._retry_submission = (target, event)
                self._submission_pending = False
            self._push("input_error", adapter_id=target.adapter_id, id=event["id"], message=str(error))
        else:
            with self._lock:
                self._retry_submission = None
                self._submission_pending = False
            self._push("input_queued", adapter_id=target.adapter_id, id=event["id"])

    def start(self, configuration: Configuration, credentials=None) -> None:
        read_configuration(configuration)
        self._begin(configuration, credentials)

    def build(self) -> None:
        self._begin(None)

    def _begin(self, configuration: Configuration | None, credentials=None) -> None:
        with self._lock:
            if self._busy:
                raise RuntimeError("Es läuft bereits ein Vorgang.")
            self._busy = True
            self._stop.clear()
            self._forced.clear()
            self._accepting_input = False
            self._input_targets = []
        threading.Thread(target=self._worker, args=(configuration, credentials), daemon=True).start()

    def _push(self, event: str, **values) -> None:
        self.events.put({"event": event, **values})

    def _run_process(self, command: list[str], json_events: bool, environment=None) -> int:
        if self._stop.is_set():
            return 0
        process = subprocess.Popen(command, cwd=self.root, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8",
            errors="replace", start_new_session=True, bufsize=1,
            env={**os.environ, **environment} if environment is not None else None)
        with self._lock:
            self._process = process
            if self._stop.is_set() and not json_events and process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
        self._push("process", pid=process.pid)

        def read_stderr():
            for line in iter(lambda: process.stderr.readline(65536), ""):
                self._push("log", message=line.rstrip(), level="stderr")

        stderr = threading.Thread(target=read_stderr, daemon=True)
        stderr.start()
        try:
            for line in iter(lambda: process.stdout.readline(1048576), ""):
                if json_events:
                    try:
                        event = json.loads(line)
                        if not isinstance(event, dict) or "event" not in event:
                            raise ValueError("Ungültige Statusnachricht")
                        if event["event"] == "ready":
                            targets = connected_inputs(self._input_candidates, event.get("input_contracts", []))
                            with self._lock:
                                self._input_targets = targets
                                self._accepting_input = not self._stop.is_set()
                        self.events.put(event)
                        if event["event"] == "ready":
                            self._push("inputs", targets=targets, has_runtime=event["has_runtime"])
                    except (ValueError, json.JSONDecodeError):
                        self._push("log", message=line.rstrip()[:4000], level="stdout")
                else:
                    self._push("log", message=line.rstrip(), level="build")
            result = process.wait()
            stderr.join(timeout=2)
            return result
        finally:
            process.stdout.close()
            process.stderr.close()
            with self._lock:
                self._process = None

    def _worker(self, configuration: Configuration | None, credentials=None) -> None:
        lock_file = None
        result, message = 0, ""
        operation = "build" if configuration is None else "host"
        try:
            if configuration:
                preview = read_configuration(configuration)
                self._input_candidates = preview["input_candidates"]
                lock_dir = self.root / "var/control-panel"
                lock_dir.mkdir(parents=True, exist_ok=True)
                name = hashlib.sha256(str(preview["database"]).encode()).hexdigest()[:24]
                lock_file = (lock_dir / f"session-{name}.lock").open("a+")
                try:
                    fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                except BlockingIOError as error:
                    raise RuntimeError("Diese Datenbank wird bereits von einem anderen Kontrollpanel verwendet.") from error
            binary = self.root / "target/debug/arcs-host"
            if configuration is None or not binary.is_file() or any(not path.is_file() for path in preview["programs"]):
                self._push("state", state="building")
                cargo = shutil.which("cargo")
                if cargo is None:
                    candidate = Path.home() / ".cargo/bin/cargo"
                    cargo = str(candidate) if candidate.is_file() else None
                if cargo is None:
                    raise RuntimeError("Cargo wurde nicht gefunden. Bitte Rust installieren oder Cargo in PATH aufnehmen.")
                result = self._run_process([cargo, "build", "--locked", "--offline", "--workspace", "--bins"], False)
                if result and not self._stop.is_set():
                    raise RuntimeError("Der Build ist fehlgeschlagen. Details stehen im Protokoll.")
            if configuration and not self._stop.is_set():
                with tempfile.TemporaryDirectory(prefix="arcs-panel-") as directory:
                    stop_file = Path(directory) / "stop"
                    with self._lock:
                        self._stop_file = stop_file
                    self._push("state", state="starting")
                    command = [str(binary), "--config", str(configuration.host.expanduser().resolve()),
                        "--monitor", "--stop-file", str(stop_file)]
                    if configuration.runtime:
                        command += ["--runtime", str(configuration.runtime.expanduser().resolve())]
                    environment = credentials(configuration) if credentials else None
                    result = self._run_process(command, True, environment)
                    if result and not self._stop.is_set():
                        raise RuntimeError(f"ARCS wurde mit Status {result} beendet. Details stehen im Protokoll.")
        except Exception as error:
            result, message = 1, str(error)
            self._push("error", message=message)
        finally:
            with self._lock:
                self._accepting_input = False
                submission = self._submission_thread
            if submission:
                submission.join(timeout=22)
            if lock_file:
                lock_file.close()
            with self._lock:
                self._stop_file = None
                self._busy = False
                self._input_targets = []
            self._push("finished", code=result, operation=operation, requested=self._stop.is_set(), forced=self._forced.is_set(), message=message)

    def request_stop(self) -> None:
        self._stop.set()
        with self._lock:
            self._accepting_input = False
            if self._stop_file is not None:
                try:
                    self._stop_file.touch(exist_ok=True)
                except FileNotFoundError:
                    pass  # The host has already finished and removed its temporary directory.
            # A build has no host stop-file protocol.
            elif self._process is not None and self._process.poll() is None:
                try:
                    os.killpg(self._process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass

    def force_stop(self) -> None:
        self._stop.set()
        self._forced.set()
        with self._lock:
            if self._process is not None and self._process.poll() is None:
                try:
                    os.killpg(self._process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
