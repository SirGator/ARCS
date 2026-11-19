"""Shared labels and revision-bound operator commands for GTK controls."""
import os
import pwd
from uuid import uuid4

from gi.repository import Gtk

STATE_NAMES = {
    "active": "Aktiv", "paused": "Pausiert", "cancelled": "Abgebrochen", "exhausted": "Beendet",
    "acquiring": "Erfasst Daten", "checking": "Prüft Ausgangslage", "planning": "Plant",
    "verifying": "Prüft Vorschlag", "awaiting_approval": "Wartet auf Freigabe",
    "executing": "Führt aus", "observing": "Beobachtet Wirkung", "assessing": "Bewertet Wirkung",
    "waiting_time": "Wartet auf Termin", "waiting_event": "Wartet auf Ereignis", "completed": "Abgeschlossen",
    "failed": "Fehlgeschlagen", "blocked": "Blockiert", "rejected": "Abgelehnt", "budget_exhausted": "Budget erschöpft",
    "running": "Läuft", "pending": "Wartet", "replanning": "Plant neu", "ready_for_review": "Zur Prüfung bereit",
}

def text_label(text="", css=None):
    label = Gtk.Label(label=text, xalign=0, wrap=True)
    if css:
        label.get_style_context().add_class(css)
    return label


def reason_text(entry):
    reason = entry.get_text().strip()
    return reason if 0 < len(reason.encode("utf-8")) <= 2000 else None


def local_operator():
    uid = os.getuid()
    try:
        return pwd.getpwuid(uid).pw_name
    except KeyError:
        return str(uid)


def revision_command(operator, sequence, revision, reason, change):
    return {"sequence": sequence, "expected_revision": revision, "command_id": uuid4().hex,
            "operator": {"actor_type": "human", "id": operator}, "reason": reason, "change": change}
