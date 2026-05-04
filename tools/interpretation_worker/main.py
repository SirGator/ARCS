import json
import os
import urllib.error
import urllib.request

UPSTREAM_URL = os.getenv("ARCS_INTERPRETATION_UPSTREAM_URL", "http://127.0.0.1:11434/api/generate")
MODEL = os.getenv("ARCS_INTERPRETATION_WORKER_MODEL", os.getenv("ARCS_INTERPRETATION_MODEL", "nuextract:latest"))


def extract_raw_text(body: dict) -> str:
    if isinstance(body.get("raw_text"), str):
        return body["raw_text"]

    messages = body.get("messages")
    if isinstance(messages, list):
        for message in reversed(messages):
            if message.get("role") == "user" and isinstance(message.get("content"), str):
                return message["content"]

    return ""


def extract_json(text: str) -> dict:
    start = text.find("{")
    end = text.rfind("}")

    if start == -1 or end == -1 or end <= start:
        return {}

    try:
        return json.loads(text[start : end + 1])
    except Exception:
        return {}


def normalize(value) -> str:
    if value is None:
        return ""
    return str(value).lower().strip()


def detect_format(raw_text: str, extracted: dict) -> str:
    joined = " ".join([
        raw_text,
        str(extracted.get("requested_format", "")),
        str(extracted.get("requested_object", "")),
    ]).lower()

    if "json" in joined:
        return "json"
    if "pdf" in joined:
        return "pdf"
    if "text" in joined or "txt" in joined:
        return "text"

    return "unknown"


def is_question_request(joined: str) -> bool:
    return any(token in joined for token in ["frage", "warum", "wie ", "was ist", "who is", "what is", "?"])


def map_to_interpretation(raw_text: str, extracted: dict) -> dict:
    action = normalize(extracted.get("requested_action", ""))
    obj = normalize(extracted.get("requested_object", ""))
    fmt = detect_format(raw_text, extracted)

    joined = f"{raw_text} {action} {obj}".lower()

    if any(token in joined for token in ["termin", "kalender", "meeting", "uhr"]):
        return {
            "status": "unknown",
            "intent": "unknown",
            "target": raw_text,
            "format": "unknown",
            "confidence": 0.0,
            "raw_text": raw_text,
            "reason": "calendar scheduling is not supported by current ARCS interpretation worker",
        }

    if (
        ("bericht" in joined or "report" in joined or "prüfergebnis" in joined or "pruefergebnis" in joined or "auswertung" in joined or "zusammenfassung" in joined)
        and ("erstelle" in joined or "erstell" in joined or "generiere" in joined or "mach" in joined or "schreib" in joined)
    ):
        return {
            "status": "parsed",
            "intent": "create_report",
            "target": extracted.get("requested_object") or raw_text,
            "format": fmt if fmt != "unknown" else "text",
            "confidence": 0.80,
            "raw_text": raw_text,
            "reason": "worker extracted report request",
        }

    if is_question_request(joined):
        return {
            "status": "parsed",
            "intent": "ask_question",
            "target": extracted.get("requested_object") or raw_text,
            "format": "text",
            "confidence": 0.70,
            "raw_text": raw_text,
            "reason": "worker extracted question request",
        }

    return {
        "status": "unknown",
        "intent": "unknown",
        "target": raw_text,
        "format": "unknown",
        "confidence": 0.0,
        "raw_text": raw_text,
        "reason": "no supported ARCS intent detected",
    }


def build_prompt(raw_text: str) -> str:
    return f"""
### Template:
{{
  "requested_action": "",
  "requested_object": "",
  "requested_format": "",
  "time_expression": ""
}}

### Text:
{raw_text}
"""


def call_upstream(raw_text: str) -> dict:
    request_payload = {
        "model": MODEL,
        "prompt": build_prompt(raw_text),
        "stream": False,
        "options": {"temperature": 0},
    }

    request = urllib.request.Request(
        UPSTREAM_URL,
        data=json.dumps(request_payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    with urllib.request.urlopen(request, timeout=30) as response:
        response_text = response.read().decode("utf-8")

    model_text = json.loads(response_text).get("response", "")
    extracted = extract_json(model_text)

    if not extracted:
        return {
            "status": "unknown",
            "intent": "unknown",
            "target": raw_text,
            "format": "unknown",
            "confidence": 0.0,
            "raw_text": raw_text,
            "reason": "upstream returned no valid json",
        }

    return map_to_interpretation(raw_text, extracted)


def json_response(status: int, payload: dict):
    body = json.dumps(payload).encode("utf-8")
    headers = [
        (b"content-type", b"application/json"),
        (b"content-length", str(len(body)).encode("ascii")),
    ]
    return status, headers, body


async def read_body(receive):
    chunks = []
    while True:
        message = await receive()
        if message["type"] != "http.request":
            continue

        chunk = message.get("body", b"")
        if chunk:
            chunks.append(chunk)

        if not message.get("more_body", False):
            break

    return b"".join(chunks)


async def app(scope, receive, send):
    if scope["type"] != "http":
        return

    path = scope.get("path", "")
    method = scope.get("method", "GET").upper()

    if method == "GET" and path == "/health":
        status, headers, body = json_response(200, {
            "ok": True,
            "model": MODEL,
            "upstream_url": UPSTREAM_URL,
        })
    elif method == "POST" and path == "/interpret":
        raw_body = await read_body(receive)

        try:
            request_body = json.loads(raw_body.decode("utf-8") or "{}")
        except Exception:
            request_body = {}

        raw_text = extract_raw_text(request_body)

        if not raw_text:
            status, headers, body = json_response(200, {
                "status": "failed",
                "intent": "unknown",
                "target": "",
                "format": "unknown",
                "confidence": 0.0,
                "raw_text": "",
                "reason": "request contained no raw_text or user message",
            })
        else:
            try:
                payload = call_upstream(raw_text)
            except urllib.error.URLError as e:
                payload = {
                    "status": "failed",
                    "intent": "unknown",
                    "target": raw_text,
                    "format": "unknown",
                    "confidence": 0.0,
                    "raw_text": raw_text,
                    "reason": f"worker request failed: {e}",
                }
            except Exception as e:
                payload = {
                    "status": "failed",
                    "intent": "unknown",
                    "target": raw_text,
                    "format": "unknown",
                    "confidence": 0.0,
                    "raw_text": raw_text,
                    "reason": f"worker request failed: {e}",
                }

            status, headers, body = json_response(200, payload)
    else:
        status, headers, body = json_response(404, {"detail": "not found"})

    await send({"type": "http.response.start", "status": status, "headers": headers})
    await send({"type": "http.response.body", "body": body})
