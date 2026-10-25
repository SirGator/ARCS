"""Eingaben über die Outbox des JSON-Input-Adapters; keine Core-Schreibzugriffe."""
from __future__ import annotations

from dataclasses import dataclass, replace
import json
from pathlib import Path
import subprocess
import tempfile
import uuid


@dataclass(frozen=True)
class InputTarget:
    adapter_id: str
    capability_id: str
    schema_id: str
    program: Path
    profile: Path
    journal: Path
    working_directory: Path
    template: dict
    schema: dict | None = None
    max_payload_bytes: int = 0
    max_reference_bytes: int = 0

    @property
    def key(self):
        return json.dumps([self.adapter_id, self.capability_id], separators=(",", ":"))


def configured_inputs(host: Path, adapters: list[dict]) -> list[InputTarget]:
    """Lokale --enqueue-Brücke nur für die bekannte JSON-Input-Implementierung.

    Verbundene Input-Capability und Schema werden später aus dem Handshake
    gebunden. Andere Input-Implementierungen erhalten dadurch keine Push-API.
    """
    targets = []
    for adapter in adapters:
        program = (host.parent / adapter["program"]).resolve()
        if program.name != "arcs-json-input" or not isinstance(adapter.get("profile"), str):
            continue
        args = adapter.get("args", [])
        if len(args) != 1:
            continue
        working = (host.parent / (adapter.get("working_directory") or ".")).resolve()
        argument = args[0]
        if isinstance(argument, dict) and set(argument) == {"path"}:
            journal = (host.parent / argument["path"]).resolve()
        elif isinstance(argument, str):
            journal = (working / argument).resolve()
        else:
            continue
        profile = (host.parent / adapter["profile"]).resolve()
        data = json.loads(profile.read_text(encoding="utf-8"))
        if not isinstance(data, dict) or not all(isinstance(data.get(field), str) for field in ["adapter_id", "capability_id", "schema_id"]):
            continue
        events = data.get("events", [])
        example = events[0].get("payload", {}) if events and isinstance(events[0], dict) else {}
        targets.append(InputTarget(data["adapter_id"], data["capability_id"], data["schema_id"], program, profile, journal, working,
            example if isinstance(example, dict) else {}))
    return sorted(targets, key=lambda target: (target.schema_id != "arcs.input.v1", target.adapter_id, target.capability_id))


def connected_inputs(candidates: list[InputTarget], contracts: list[dict]) -> list[InputTarget]:
    targets = []
    for candidate in candidates:
        for contract in contracts:
            capability = contract.get("capability", {})
            schema = contract.get("schema", {})
            if capability == {"adapter_id": candidate.adapter_id, "capability_id": candidate.capability_id} and schema.get("$id") == candidate.schema_id:
                targets.append(replace(candidate, schema=schema, max_payload_bytes=contract["max_payload_bytes"], max_reference_bytes=contract["max_external_reference_bytes"]))
    return targets


def parse_json(text: str):
    def invalid(value):
        raise ValueError(f"{value} ist keine gültige JSON-Zahl.")
    return json.loads(text, parse_constant=invalid)


def make_submission(target: InputTarget, payload) -> dict:
    if target.schema is None:
        raise ValueError("Der Input-Adapter ist noch nicht verbunden.")
    encoded = json.dumps(payload, ensure_ascii=False, allow_nan=False, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > min(target.max_payload_bytes, 60 * 1024):
        raise ValueError(f"Die Eingabe überschreitet das Adapterlimit von {target.max_payload_bytes} Bytes.")
    identity = "gui-" + uuid.uuid4().hex
    reference = "gui:" + identity
    if len(reference.encode("utf-8")) > target.max_reference_bytes:
        raise ValueError("Das Referenzlimit dieses Adapters ist für GUI-Eingaben zu klein.")
    event = {"id": identity, "subject": identity, "reference": reference, "payload": parse_json(encoded)}
    # The adapter bounds the complete message as well as the Core payload grant.
    message = {"capability_id": target.capability_id, "external_subject": identity, "external_reference": reference, "payload": event["payload"]}
    if len(json.dumps(message, ensure_ascii=False, separators=(",", ":")).encode("utf-8")) > 60 * 1024:
        raise ValueError("Die vollständige Eingabe überschreitet 60 KiB.")
    return event


def enqueue_submission(target: InputTarget, event: dict, validator: Path) -> None:
    """Eine unveränderliche Ereignis-ID; ein Retry benutzt dieselbe ID."""
    with tempfile.TemporaryDirectory(prefix="arcs-gui-input-") as directory:
        path = Path(directory) / "event.json"
        validation = Path(directory) / "validation.json"
        with validation.open("w", encoding="utf-8") as writer:
            validation.chmod(0o600)
            json.dump({"schema":target.schema, "payload":event["payload"]}, writer, ensure_ascii=False, allow_nan=False)
        check = subprocess.run([str(validator), "--validate-input", str(validation)], stdin=subprocess.DEVNULL,
            capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=10)
        if check.returncode:
            raise ValueError(check.stderr.strip()[:1500] or "Die Eingabe passt nicht zum Adaptervertrag.")
        if json.loads(check.stdout) != {"valid": True}:
            raise RuntimeError("Keine gültige Bestätigung der Eingabeprüfung.")
        with path.open("w", encoding="utf-8") as writer:
            path.chmod(0o600)
            json.dump(event, writer, ensure_ascii=False, allow_nan=False)
        result = subprocess.run([str(target.program), "--enqueue", str(target.profile), str(target.journal), str(path)],
            cwd=target.working_directory, stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=10)
        if result.returncode:
            raise RuntimeError(result.stderr.strip()[:1500] or f"Input-Adapter beendet mit Status {result.returncode}.")
        try:
            receipt = json.loads(result.stdout)
        except ValueError as error:
            raise RuntimeError("Keine gültige Empfangsbestätigung vom Input-Adapter.") from error
        if not isinstance(receipt, dict) or receipt.get("queued") != event["id"]:
            raise RuntimeError("Die Empfangsbestätigung gehört nicht zu dieser Eingabe.")
