"""Gespeicherte Verbindungen und Anschlusskonfiguration, ohne Anbieteraufrufe."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
from urllib.parse import urlsplit
import uuid

from .controller import Configuration
from .credentials import Credentials


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, allow_nan=False, separators=(",", ":"))


def private_directory(path):
    path.mkdir(parents=True, exist_ok=True, mode=0o700)
    path.chmod(0o700)


def write_text(path, value):
    private_directory(path.parent)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent, delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(value)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
    finally:
        if temporary:
            temporary.unlink(missing_ok=True)


def write_json(path, value):
    write_text(path, canonical(value) + "\n")


def normalize_connection(value):
    allowed = {"id", "name", "provider", "model", "endpoint", "requires_key", "remember_key", "provider_options"}
    if not isinstance(value, dict) or not allowed.issubset(value) or set(value) - (allowed | {"kind"}):
        raise ValueError("Ungültige gespeicherte Modellverbindung.")
    result = dict(value)
    result.setdefault("kind", "llm")
    if result["kind"] not in ("llm", "vision"):
        raise ValueError("Ungültiger Modelltyp.")
    try:
        if uuid.UUID(result["id"]).hex != result["id"]:
            raise ValueError()
    except (ValueError, TypeError, AttributeError):
        raise ValueError("Ungültige Verbindungs-ID.") from None
    for field, maximum in (("name", 160), ("provider", 80), ("model", 512), ("endpoint", 2048)):
        text = result[field]
        if not isinstance(text, str) or not text.strip() or len(text) > maximum or any(ord(char) < 32 or ord(char) == 127 for char in text):
            raise ValueError("Bitte Name, Anbieter, Modell und API-Adresse vollständig und gültig eintragen.")
        result[field] = text.strip()
    if type(result["requires_key"]) is not bool or type(result["remember_key"]) is not bool:
        raise ValueError("Ungültige Zugangsoptionen.")
    if not result["requires_key"]:
        result["remember_key"] = False
    endpoint = urlsplit(result["endpoint"].replace("{model}", "model"))
    if not endpoint.hostname or endpoint.username or endpoint.password or endpoint.query or endpoint.fragment:
        raise ValueError("API-Adresse bitte ohne Zugangsdaten, Suchparameter oder Fragment eintragen.")
    if endpoint.scheme != "https" and not (endpoint.scheme == "http" and endpoint.hostname in ("127.0.0.1", "localhost", "::1")):
        raise ValueError("API-Adresse benötigt HTTPS. HTTP ist für lokale Verbindungen erlaubt.")
    options = result["provider_options"]
    if not isinstance(options, dict) or set(options) - {"token_field", "json_mode", "api_version"}:
        raise ValueError("Ungültige Anbieteroptionen.")
    if ("token_field" in options and options["token_field"] not in ("max_tokens", "max_completion_tokens")) or ("json_mode" in options and type(options["json_mode"]) is not bool):
        raise ValueError("Ungültige Chat-Optionen.")
    if "api_version" in options and (not isinstance(options["api_version"], str) or not options["api_version"] or len(options["api_version"]) > 80 or any(ord(char) < 32 for char in options["api_version"])):
        raise ValueError("Ungültige API-Version.")
    return result


class AdapterSetup:
    def __init__(self, root):
        self.program = root.parent / "llm-api/arcs-llm-api"
        self.vision_program = root.parent / "model-services/vision-api"
        self._lock = threading.Lock()
        self._processes = set()
        self._closed = False

    def run(self, option, packet=None, environment=None, program=None):
        program = program or self.program
        if not program.is_file():
            raise ValueError("Die externe LLM-Anbindung fehlt. Erwartet wird llm-api neben dem ARCS-Projekt.")
        with self._lock:
            if self._closed:
                raise ValueError("Modelleinrichtung wurde geschlossen.")
            process = subprocess.Popen([str(program), option], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL, env={**os.environ, **(environment or {})}, start_new_session=True)
            self._processes.add(process)
        try:
            output, _ = process.communicate(canonical(packet).encode() if packet is not None else b"", timeout=20)
            if len(output) > 65536:
                raise ValueError("Ungültige Antwort der externen Modelleinrichtung.")
            try:
                result = json.loads(output)
            except (ValueError, UnicodeError):
                raise ValueError("Die externe Modelleinrichtung hat keine gültige Antwort geliefert.") from None
            if not isinstance(result, dict):
                raise ValueError("Ungültige Antwort der externen Modelleinrichtung.")
            if process.returncode or result.get("ok") is False:
                message = result.get("error", "Verbindung konnte nicht geprüft werden.")
                raise ValueError(str(message)[:400])
            return result
        except subprocess.TimeoutExpired:
            raise ValueError("Verbindungstest hat das Zeitlimit erreicht.") from None
        finally:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            process.wait()
            for stream in (process.stdin, process.stdout):
                if stream:
                    stream.close()
            with self._lock:
                self._processes.discard(process)

    def describe(self):
        result = self.run("--describe")
        if self.vision_program.is_file():
            result["providers"] += self.run("--describe", program=self.vision_program)["providers"]
        if result.get("setup_version") != 1 or not isinstance(result.get("providers"), list) or not result["providers"]:
            raise ValueError("Die externe Anbindung unterstützt diese Modelleinrichtung noch nicht.")
        for item in result["providers"]:
            if not isinstance(item, dict) or not {"id", "label", "endpoint", "requires_key"}.issubset(item) or set(item) - {"id", "label", "endpoint", "requires_key", "kind"}:
                raise ValueError("Ungültige Anbieterbeschreibung.")
            if any(not isinstance(item[name], str) or not item[name].strip() for name in ("id", "label", "endpoint")) or type(item["requires_key"]) is not bool:
                raise ValueError("Ungültige Anbieterbeschreibung.")
        if len({item["id"] for item in result["providers"]}) != len(result["providers"]):
            raise ValueError("Doppelte Anbieterbeschreibung.")
        return result["providers"]

    def probe(self, record, key):
        record = normalize_connection(record)
        variable = "ARCS_PANEL_PROBE_KEY" if record["requires_key"] else None
        if variable and not key:
            raise ValueError("Bitte zuerst einen API-Schlüssel eintragen.")
        packet = {name: record[name] for name in ("provider", "model", "endpoint", "provider_options")}
        packet["api_key_env"] = variable
        return self.run("--probe", packet, {variable: key} if variable else {}, self.vision_program if record["kind"] == "vision" else None)

    def close(self):
        with self._lock:
            self._closed = True
            for process in self._processes:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass


class ModelConnections:
    def __init__(self, root, directory=None, credentials=None):
        self.root = root.resolve()
        self.directory = (directory or self.root / "var/control-panel/models").resolve()
        self.credentials = credentials or Credentials()
        self.setup = AdapterSetup(self.root)
        self._lock = threading.RLock()
        self.records = []
        self.assignments = {"planner": None, "assessor": None}
        self.load_error = None
        try:
            self.reload()
        except (ValueError, OSError):
            self.load_error = "Gespeicherte Modelle konnten nicht geladen werden. Die Datei bleibt erhalten; bitte unter Erweitert prüfen."

    def _read(self):
        path = self.directory / "connections.json"
        if not path.exists():
            return {"version": 1, "connections": [], "assignments": {"planner": None, "assessor": None}}
        return json.loads(path.read_text(encoding="utf-8"))

    def reload(self):
        with self._lock:
            values = self._read()
            if not isinstance(values, dict) or set(values) != {"version", "connections", "assignments"} or values["version"] != 1:
                raise ValueError("Ungültige Verbindungsdatei.")
            if not isinstance(values["connections"], list):
                raise ValueError("Ungültige Verbindungsliste.")
            records = [normalize_connection(record) for record in values["connections"]]
            ids = {record["id"] for record in records}
            assignments = values["assignments"]
            if len(ids) != len(records) or not isinstance(assignments, dict) or not {"planner", "assessor"}.issubset(assignments) or set(assignments) - {"planner", "assessor", "vision"} or any(identifier is not None and identifier not in ids for identifier in assignments.values()):
                raise ValueError("Ungültige Modellzuordnung.")
            self.records, self.assignments = records, assignments
            self._revision = canonical(values)
            self.load_error = None

    def _mutate(self, operation):
        with self._lock:
            if self.load_error:
                raise ValueError(self.load_error)
            private_directory(self.directory)
            with (self.directory / "connections.lock").open("a+") as lock:
                os.fchmod(lock.fileno(), 0o600)
                fcntl.flock(lock, fcntl.LOCK_EX)
                values = self._read()
                if canonical(values) != self._revision:
                    raise ValueError("Modelle wurden in einem anderen Fenster geändert. Bitte Neu laden drücken.")
                operation(values)
                write_json(self.directory / "connections.json", values)
                self.reload()

    def save(self, record, key=None):
        record = normalize_connection(record)
        def operation(values):
            previous = next((item for item in values["connections"] if item["id"] == record["id"]), None)
            old_remembered = bool(previous and previous["remember_key"])
            if record["requires_key"]:
                if previous and (previous["provider"], previous["endpoint"]) != (record["provider"], record["endpoint"]) and not key:
                    raise ValueError("Nach einem Anbieter- oder Adresswechsel bitte den API-Schlüssel erneut eingeben.")
                selected_key = key or self.credentials.get(record["id"], old_remembered)
                if not selected_key:
                    raise ValueError("Bitte einen API-Schlüssel eintragen oder die Schlüsselanforderung für einen lokalen Server deaktivieren.")
                self.credentials.put(record["id"], selected_key, record["remember_key"], old_remembered)
            elif previous:
                self.credentials.remove(record["id"], old_remembered)
            values["connections"] = [item for item in values["connections"] if item["id"] != record["id"]] + [record]
        self._mutate(operation)
        return record

    def remove(self, identifier):
        def operation(values):
            if identifier in values["assignments"].values():
                raise ValueError("Diese Verbindung ist noch zugeordnet. Bitte zuerst eine andere Zuordnung wählen und übernehmen.")
            previous = next((item for item in values["connections"] if item["id"] == identifier), None)
            if previous:
                self.credentials.remove(identifier, previous["remember_key"])
            values["connections"] = [item for item in values["connections"] if item["id"] != identifier]
        self._mutate(operation)

    def assign(self, planner, assessor, vision=None):
        def operation(values):
            ids = {item["id"] for item in values["connections"]}
            if planner not in ids or assessor not in ids:
                raise ValueError("Bitte ein gespeichertes Modell für Planung und Prüfung auswählen.")
            records = {item["id"]: item for item in values["connections"]}
            if any(records[identifier].get("kind", "llm") != "llm" for identifier in (planner, assessor)):
                raise ValueError("Planung und Wirkungsprüfung benötigen ein Sprachmodell; YOLO gehört zur Bildauswertung.")
            values["assignments"] = {"planner": planner, "assessor": assessor}
            if vision:
                if vision not in records or records[vision].get("kind") != "vision":
                    raise ValueError("Bitte einen gespeicherten Bilddienst auswählen.")
                values["assignments"]["vision"] = vision
        self._mutate(operation)

    def key_for(self, record, entered=None):
        if not record["requires_key"]:
            return None
        if entered:
            if any(ord(char) < 32 or ord(char) == 127 for char in entered):
                raise ValueError("Ungültiger API-Schlüssel.")
            return entered
        previous = next((item for item in self.records if item["id"] == record["id"]), None)
        if previous and (previous["provider"], previous["endpoint"]) != (record["provider"], record["endpoint"]):
            raise ValueError("Nach einem Anbieter- oder Adresswechsel bitte den API-Schlüssel erneut eingeben.")
        return self.credentials.get(record["id"], bool(previous and previous["remember_key"]))

    def materialize(self):
        """Unveränderliches Bundle mit getrennten Journals pro Modellkonfiguration."""
        with self._lock:
            if self.load_error:
                raise ValueError(self.load_error)
            chosen = {role: next((item for item in self.records if item["id"] == self.assignments[role]), None) for role in ("planner", "assessor")}
            if any(record is None for record in chosen.values()):
                raise ValueError("Bitte zuerst Modelle für Planung und Prüfung zuordnen.")
            source = self.root / "apps/adapter-host/config/llm-tasks"
            host = json.loads((source / "host.json").read_text())
            runtime = json.loads((source / "runtime.json").read_text())
            files = {}
            for path in source.glob("*.json"):
                if path.name not in ("host.json", "runtime.json"):
                    files[path.name] = json.loads(path.read_text())
            schema_files = []
            for index, relative in enumerate(host["schemas"]):
                name = f"host-schema-{index}.json"
                files[name] = json.loads((source / relative).read_text())
                schema_files.append(name)
            host["schemas"] = schema_files
            for adapter in host["adapters"]:
                adapter["program"] = str((source / adapter["program"]).resolve())
                original = (source / adapter["profile"]).resolve()
                role = next((role for role in chosen if original.name == role + ".json"), None)
                if role:
                    record = chosen[role]
                    profile = json.loads(original.read_text())
                    profile.update({field: record[field] for field in ("provider", "model", "endpoint", "provider_options")})
                    profile["api_key_env"] = "ARCS_PANEL_KEY_" + record["id"].upper() if record["requires_key"] else None
                    for schema_id, relative in profile["target_schemas"].items():
                        name = role + "-" + Path(relative).name
                        files[name] = json.loads((original.parent / relative).read_text())
                        profile["target_schemas"][schema_id] = name
                    instructions = (original.parent / profile["instructions_file"]).read_text(encoding="utf-8")
                    profile["instructions_file"] = role + "-instructions.md"
                    files[profile["instructions_file"]] = instructions
                    adapter["profile"] = role + ".json"
                    files[adapter["profile"]] = profile
                else:
                    adapter["profile"] = original.name
                for argument in adapter.get("args", []):
                    if isinstance(argument, dict):
                        argument["path"] = Path(argument["path"]).name
            vision = next((item for item in self.records if item["id"] == self.assignments.get("vision")), None)
            if vision:
                external = self.root.parent / "model-services"
                program = external / "adapter/target/debug/arcs-vision-api"
                if not program.is_file():
                    raise ValueError("Der externe Bild-API-Adapter ist noch nicht gebaut.")
                files["vision-profile.json"] = {"adapter_id": "panel.vision", "endpoint": vision["endpoint"], "model": vision["model"]}
                files["vision-schema.json"] = json.loads((external / "vision/detections.schema.json").read_text())
                resources = {"max_context_items": 4, "max_context_bytes": 32768, "max_output_bytes": 60000, "max_candidates": 1,
                    "max_runtime_ms": 15000, "max_iterations": None, "max_output_tokens": None}
                files["vision-grant.json"] = {"adapter_id": "panel.vision", "producer_class": "model", "enabled_capabilities": ["image.detect"],
                    "granted_permissions": ["reason.compute", "data.receive"], "assigned_trust": "medium", "ingress_source_kind": None,
                    "max_payload_bytes": 55000, "max_external_reference_bytes": 256,
                    "reasoning_limits_v2": {"methods": ["learned"], "resources": resources},
                    "input_data_limits": {"max_total_bytes": 8388608, "max_objects": 1}}
                host["schemas"].append("vision-schema.json")
                host["adapters"].append({"program": str(program), "profile": "vision-profile.json", "args": [{"path": "vision.sqlite"}],
                    "grant_file": "vision-grant.json", "timeout_ms": 20000})
            binding = {"version": 1, "host": host, "runtime": runtime, "files": files}
            digest = hashlib.sha256(canonical(binding).encode()).hexdigest()[:24]
            directory = self.directory / "sessions" / digest
            private_directory(directory)
            host["database"] = "core.sqlite"
            runtime["journal"] = "runtime.sqlite"
            for name, document in files.items():
                path = directory / name
                text = document if name.endswith(".md") else canonical(document) + "\n"
                if path.exists():
                    if path.read_text(encoding="utf-8") != text:
                        raise ValueError("Erzeugte Modellkonfiguration wurde manuell geändert. Bitte als eigene Startkonfiguration verwenden oder eine neue Modellkonfiguration anlegen.")
                else:
                    write_text(path, text)
            manifest = {"version": 1, "credentials": [{"connection_id": record["id"], "env": "ARCS_PANEL_KEY_" + record["id"].upper(),
                         "remember_key": record["remember_key"]} for record in chosen.values() if record["requires_key"]]}
            write_json(directory / "credentials.json", manifest)
            for name, document in (("host.json", host), ("runtime.json", runtime)):
                path = directory / name
                if path.exists() and path.read_text(encoding="utf-8") != canonical(document) + "\n":
                    raise ValueError("Erzeugte Modellkonfiguration wurde manuell geändert. Bitte als eigene Startkonfiguration verwenden oder eine neue Modellkonfiguration anlegen.")
                if not path.exists():
                    write_json(path, document)
            return Configuration(directory / "host.json", directory / "runtime.json")

    def environment_for(self, configuration):
        path = configuration.host.expanduser().resolve()
        if not path.is_relative_to((self.directory / "sessions").resolve()):
            return {}
        manifest = json.loads((path.parent / "credentials.json").read_text())
        if manifest.get("version") != 1:
            raise ValueError("Ungültige Zugangskonfiguration.")
        result = {}
        for item in manifest["credentials"]:
            if not isinstance(item, dict) or set(item) != {"connection_id", "env", "remember_key"} or type(item["remember_key"]) is not bool:
                raise ValueError("Ungültige Schlüsselreferenz.")
            try:
                identifier = uuid.UUID(item["connection_id"]).hex
            except (ValueError, TypeError, AttributeError):
                raise ValueError("Ungültige Schlüsselreferenz.") from None
            if item["env"] != "ARCS_PANEL_KEY_" + identifier.upper():
                raise ValueError("Ungültige Schlüsselreferenz.")
            key = self.credentials.get(item["connection_id"], item["remember_key"])
            if not key:
                raise ValueError("API-Schlüssel fehlt für diese Sitzung. Unter Modelle die Verbindung öffnen, Schlüssel eintragen und speichern.")
            result[item["env"]] = key
        return result

    def close(self):
        self.setup.close()
        self.credentials.close()
