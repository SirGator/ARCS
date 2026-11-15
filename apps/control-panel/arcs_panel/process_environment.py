"""Gemeinsame Basis und ausdrücklich freigegebene Prozessvariablen."""
import json
import os
from pathlib import Path
import re


BASE_NAMES = tuple(json.loads((Path(__file__).resolve().parents[3] /
    "apps/adapter-host/config/environment-base.json").read_text()))


def validate_names(names):
    if (not isinstance(names, list) or len(names) > 64 or
            any(not isinstance(name, str) or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]{0,127}", name) for name in names) or
            len(names) != len(set(names))):
        raise ValueError("inherit_env benötigt höchstens 64 eindeutige Umgebungsnamen.")
    return names


def process_environment(extra=None):
    environment = {name: os.environ[name] for name in BASE_NAMES if name in os.environ}
    environment.update(extra or {})
    return environment


def inherited_environment(names):
    # Die Vereinigung mehrerer gültiger Adapterlisten kann mehr als 64 Namen
    # enthalten. Das Limit gilt pro Adapter, nicht für den gesamten Host.
    for name in names:
        validate_names([name])
    return {name: os.environ[name] for name in names if name in os.environ}
