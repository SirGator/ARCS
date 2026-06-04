"""HTTP-Client für die text-to-json-parser FastAPI-Anwendung.

ARCS spricht nicht direkt mit dem Parser. Dieser Client ist die Brücke:
- ARCS sendet ein InterpretationRequest mit raw_input, schema_id, schema, context.
- Wir übersetzen das in einen text-to-json-parser Aufruf (`POST /generate-json`).
- Wir nehmen das `data` Feld der Parser-Antwort und packen es als ARCS-Payload
  (interpretation_proposal) zurück.

Damit bleibt ARCS von der Parser-Implementierung entkoppelt. Der Parser selbst
kann ausgetauscht werden, solange er dieselbe Schnittstelle anbietet.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any
from urllib.parse import urljoin

import httpx


@dataclass(frozen=True)
class ParserClientConfig:
    """Konfiguration für den Parser-Client.

    Attributes:
        base_url: Basis-URL des text-to-json-parser (z.B. http://127.0.0.1:8000).
        generate_path: Pfad zum generate-json Endpoint.
        timeout_seconds: HTTP-Timeout in Sekunden.
    """

    base_url: str = "http://127.0.0.1:8000"
    generate_path: str = "/generate-json"
    timeout_seconds: float = 120.0

    def generate_url(self) -> str:
        return urljoin(self.base_url.rstrip("/") + "/", self.generate_path.lstrip("/"))


@dataclass
class ParserResult:
    """Ergebnis eines Parser-Aufrufs, normalisiert für ARCS.

    Attributes:
        ok: True wenn der Parser valides JSON geliefert hat.
        data: Das vom Parser extrahierte JSON (sollte schema-konform sein).
        error_code: Maschinenlesbarer Fehlercode (nur bei ok=False).
        error_message: Menschenlesbare Fehlermeldung (nur bei ok=False).
        error_details: Optionale Detail-Liste (z.B. Validation-Errors).
    """

    ok: bool
    data: dict[str, Any] | None = None
    error_code: str = ""
    error_message: str = ""
    error_details: list[str] = field(default_factory=list)


class ParserClient:
    """Spricht mit dem text-to-json-parser Service.

    Keine ARCS-Logik. Nur HTTP-Adapter für den Parser. Tests können einen
    Fake-Client mit der gleichen `interpret()`-Signatur einsetzen.
    """

    def __init__(self, config: ParserClientConfig | None = None) -> None:
        self._config = config or ParserClientConfig()

    @property
    def config(self) -> ParserClientConfig:
        return self._config

    def interpret(
        self,
        text: str,
        schema: dict[str, Any],
        context: dict[str, Any] | None = None,
        prompt: str | None = None,
    ) -> ParserResult:
        """Ruft den Parser und gibt das normalisierte Ergebnis zurück."""
        payload: dict[str, Any] = {
            "text": text,
            "schema": schema,
        }
        if context is not None:
            payload["context"] = context
        if prompt:
            payload["prompt"] = prompt

        try:
            response = httpx.post(
                self._config.generate_url(),
                json=payload,
                timeout=self._config.timeout_seconds,
            )
        except httpx.HTTPError as exc:
            return ParserResult(
                ok=False,
                error_code="parser_unreachable",
                error_message=f"text-to-json-parser nicht erreichbar: {exc}",
            )

        if response.status_code >= 500:
            return ParserResult(
                ok=False,
                error_code="parser_error",
                error_message=f"Parser lieferte HTTP {response.status_code}",
            )

        try:
            raw: Any = response.json()
        except json.JSONDecodeError as exc:
            return ParserResult(
                ok=False,
                error_code="parser_invalid_response",
                error_message=f"Parser-Antwort ist kein JSON: {exc}",
            )

        if not isinstance(raw, dict):
            return ParserResult(
                ok=False,
                error_code="parser_invalid_response",
                error_message="Parser-Antwort ist kein Objekt",
            )

        if response.status_code >= 400 or raw.get("ok") is False:
            error = raw.get("error") if isinstance(raw.get("error"), dict) else {}
            return ParserResult(
                ok=False,
                error_code=str(error.get("code") or "parser_failed"),
                error_message=str(error.get("message") or "Parser lieferte ok=false"),
                error_details=[str(d) for d in (error.get("details") or [])],
            )

        data = raw.get("data")
        if not isinstance(data, dict):
            return ParserResult(
                ok=False,
                error_code="parser_invalid_data",
                error_message="Parser-Daten sind kein Objekt",
            )

        return ParserResult(ok=True, data=data)
