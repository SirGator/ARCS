"""Operator controls for work reported by the running host."""
from __future__ import annotations

from datetime import datetime
import json

from gi.repository import Gtk

from .control_widgets import STATE_NAMES, local_operator, reason_text, revision_command, text_label


GOAL_ENDED = {"completed", "failed", "blocked", "rejected", "budget_exhausted", "cancelled"}
PLAN_OPEN = {"pending", "running", "replanning", "failed"}
KIND_NAMES = {"goal": "Ziel", "series": "Prüfserie", "plan": "Plan"}


class ControlPage(Gtk.Box):
    def __init__(self, submit):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        self.submit = submit
        self.operator = local_operator()
        self.records = {}
        self.task_controls = {}
        self.options = []
        self.connected = False
        self.review_available = False
        self.pending = None
        self.waiting_for_snapshot = False
        self.review = None
        self.pack_start(text_label("Laufende Aufgaben steuern", "card-title"), False, False, 0)
        self.selection = Gtk.ComboBoxText()
        self.selection.connect("changed", lambda *_: self._refresh())
        self.pack_start(self.selection, False, False, 0)
        self.detail = text_label(css="muted")
        self.pack_start(self.detail, False, False, 0)
        self.reason = Gtk.Entry(placeholder_text="Begründung für Pause, Fortsetzung oder Abbruch")
        self.reason.connect("changed", lambda *_: self._refresh())
        self.pack_start(self.reason, False, False, 0)
        buttons = Gtk.Box(spacing=8)
        self.inspect_button = Gtk.Button(label="Vorschlag prüfen")
        self.inspect_button.get_style_context().add_class("primary")
        self.inspect_button.connect("clicked", self._inspect)
        self.pause_button = Gtk.Button(label="Pausieren")
        self.pause_button.connect("clicked", self._pause)
        self.cancel_button = Gtk.Button(label="Abbrechen")
        self.cancel_button.get_style_context().add_class("danger")
        self.cancel_button.connect("clicked", self._cancel)
        for button in (self.inspect_button, self.pause_button, self.cancel_button):
            button.set_no_show_all(True)
            buttons.pack_start(button, False, False, 0)
        self.pack_start(buttons, False, False, 0)
        self.status = text_label(css="muted")
        self.status.set_selectable(True)
        self.pack_start(self.status, False, False, 0)
        self._refresh()

    def set_connected(self, connected, review_available=False):
        self.connected = connected
        self.review_available = review_available
        if not connected:
            self._dismiss_review()
        self._refresh()

    def reset(self):
        self.pending = None
        self.status.set_text("")
        self.reason.set_text("")
        self.set_connected(False)
        self.snapshot({})

    def snapshot(self, runtime):
        self.waiting_for_snapshot = False
        self.records = {
            f"{kind}:{entry['sequence']}": (kind, entry)
            for kind, entries in (("goal", runtime.get("goals", [])), ("series", runtime.get("recurring_checks", [])),
                                  ("plan", runtime.get("planning_jobs", []) + runtime.get("plans", [])))
            for entry in entries
        }
        self.task_controls = {row["sequence"]: row for row in runtime.get("task_controls", [])}
        if self.review and not self._still_reviewable(self.review[1]):
            self._dismiss_review()
            self.status.set_text("Der angezeigte Vorschlag wartet nicht mehr auf eine Entscheidung.")
        options = [(key, f"{KIND_NAMES[kind]} {row['sequence']} · {row['route_id']}") for key, (kind, row) in self.records.items()]
        if options != self.options:
            previous = self.selection.get_active_id()
            self.options = options
            self.selection.remove_all()
            for key, title in options:
                self.selection.append(key, title)
            if previous in self.records:
                self.selection.set_active_id(previous)
            else:
                self.selection.set_active(0)
        self._refresh()

    def _selected(self):
        return self.records.get(self.selection.get_active_id(), (None, {}))

    def _refresh(self):
        kind, row = self._selected()
        ready = self.connected and self.pending is None and not self.waiting_for_snapshot and self.review is None
        has_reason = reason_text(self.reason) is not None
        state = row.get("state")
        controls = self.task_controls.get(row.get("sequence"), {}) if kind != "series" else {}
        open_task = (kind == "goal" and state not in GOAL_ENDED) or (kind == "plan" and state in PLAN_OPEN)
        actionable = (kind == "series" and state in {"active", "paused"}) or open_task
        paused = state == "paused" or controls.get("paused", False)
        effective_pause = paused or bool(controls.get("paused_by"))
        self.selection.set_sensitive(bool(self.records))
        if not row:
            detail = "In dieser Sitzung sind noch keine Ziele, Prüfserien oder Pläne sichtbar."
        else:
            detail = "Pausiert" if effective_pause and actionable else STATE_NAMES.get(state, state)
            if row.get("next_due_unix_ms"):
                try:
                    due = datetime.fromtimestamp(row["next_due_unix_ms"] / 1000).strftime("%d.%m.%Y, %H:%M")
                    detail += (" · Geplanter Termin " if paused else " · Nächster Termin ") + due
                except (ValueError, OSError, OverflowError):
                    pass
            if kind == "goal":
                detail += f" · Runde {row['round']}"
        if not self.connected:
            detail = "Steuerung verfügbar, sobald eine unterstützte ARCS-Runtime läuft."
        self.detail.set_text(detail)
        self.reason.set_sensitive(ready and actionable)
        self.pause_button.set_visible(actionable)
        self.pause_button.set_label("Fortsetzen" if paused else "Pausieren")
        self.pause_button.set_sensitive(ready and actionable and has_reason and not (paused and controls.get("action_recovery_required")))
        if paused and controls.get("action_recovery_required"):
            self.pause_button.set_tooltip_text("Der offene Aktorauftrag muss über die CLI ausdrücklich wiederaufgenommen werden.")
        else:
            self.pause_button.set_tooltip_text(None)
        self.cancel_button.set_visible(actionable)
        self.cancel_button.set_sensitive(ready and actionable and has_reason)
        may_review = self.review_available and kind == "goal" and state == "awaiting_approval" and bool(row.get("candidate"))
        self.inspect_button.set_visible(may_review)
        self.inspect_button.set_sensitive(ready and may_review and not effective_pause)

    def _send(self, kind, payload, review_key=None):
        if not self.connected or self.pending or self.waiting_for_snapshot:
            return
        try:
            self.pending = self.submit(kind, payload), review_key
        except (OSError, RuntimeError, ValueError) as error:
            self.status.set_text(str(error))
        else:
            self.status.set_text("Warte auf Rückmeldung von ARCS …")
        self._refresh()

    def _pause(self, *_):
        kind, row = self._selected()
        reason = reason_text(self.reason)
        if not self.pause_button.get_sensitive() or not reason:
            return
        sequence = row["sequence"]
        if kind == "series":
            change = "resume" if row["state"] == "paused" else "pause"
            self._send("series", revision_command(self.operator, sequence, row["control_revision"], reason, change))
        else:
            controls = self.task_controls.get(sequence, {})
            change = "resume" if controls.get("paused") else "pause"
            self._send("task", revision_command(self.operator, sequence, controls.get("revision", 0), reason, {"command": change}))

    def _cancel(self, *_):
        kind, row = self._selected()
        reason = reason_text(self.reason)
        if not self.cancel_button.get_sensitive() or not reason:
            return
        sequence = row["sequence"]
        command = (revision_command(self.operator, sequence, row["control_revision"], reason, "cancel") if kind == "series"
                   else {"command": "cancel", "sequence": sequence, "reason": reason})
        self._send(kind, command)

    def _inspect(self, *_):
        _, row = self._selected()
        if not self.inspect_button.get_sensitive():
            return
        expected = (row["sequence"], row["candidate"], row.get("verification"), row.get("capability"))
        self._send("goal", {"command": "inspect", "sequence": expected[0], "candidate_version_id": expected[1]}, expected)

    def _still_reviewable(self, expected):
        sequence, candidate, verification, capability = expected
        _, current = self.records.get(f"goal:{sequence}", (None, {}))
        controls = self.task_controls.get(sequence, {})
        return (self.connected and self.review_available and current.get("state") == "awaiting_approval" and not controls.get("paused_by")
                and current.get("candidate") == candidate and current.get("verification") == verification
                and current.get("capability") == capability)

    def result(self, event):
        if self.pending is None or event.get("request_id") != self.pending[0]:
            return
        _, expected = self.pending
        self.pending = None
        if not event.get("ok"):
            self.status.set_text(event.get("error", {}).get("message", "ARCS hat den Befehl abgewiesen."))
        elif expected is None:
            self.waiting_for_snapshot = True
            self.reason.set_text("")
            self.status.set_text("Befehl bestätigt.")
        else:
            receipt = event.get("receipt") or {}
            received = (receipt.get("sequence"), receipt.get("candidate_version_id"), receipt.get("verification_version_id"), receipt.get("capability"))
            if received == expected and "payload" in receipt and self._still_reviewable(expected):
                self.status.set_text("")
                self._show_review(receipt, expected)
            else:
                self.status.set_text("Der aktuelle Vorschlag konnte nicht zur Prüfung geöffnet werden.")
        self._refresh()

    def _show_review(self, receipt, expected):
        sequence, candidate, verification, capability = expected
        dialog = Gtk.Dialog(title=f"Aktion für Ziel {sequence} prüfen", transient_for=self.get_toplevel(), modal=True)
        dialog.set_default_size(590, 430)
        dialog.add_button("Schließen", Gtk.ResponseType.CANCEL)
        dialog.add_button("Ablehnen", Gtk.ResponseType.REJECT)
        dialog.add_button("Freigeben", Gtk.ResponseType.ACCEPT)
        dialog.set_default_response(Gtk.ResponseType.CANCEL)
        content = dialog.get_content_area()
        content.set_spacing(10)
        content.set_border_width(16)
        summary = text_label(f"{capability['adapter_id']} · {capability['capability_id']}\nVorschlag {candidate}\nPrüfbeleg {verification}")
        summary.set_selectable(True)
        content.pack_start(summary, False, False, 0)
        payload = Gtk.TextView(editable=False, monospace=True, wrap_mode=Gtk.WrapMode.CHAR)
        payload.get_buffer().set_text(json.dumps(receipt["payload"], indent=2, ensure_ascii=True, sort_keys=True))
        viewport = Gtk.ScrolledWindow()
        viewport.add(payload)
        content.pack_start(viewport, True, True, 0)
        reason = Gtk.Entry(placeholder_text="Begründung der Entscheidung")
        content.pack_start(reason, False, False, 0)
        def update_decisions(*_):
            for response in (Gtk.ResponseType.ACCEPT, Gtk.ResponseType.REJECT):
                dialog.set_response_sensitive(response, reason_text(reason) is not None)
        reason.connect("changed", update_decisions)
        dialog.connect("response", self._decision, expected, reason)
        self.review = dialog, expected, reason
        update_decisions()
        dialog.show_all()

    def _dismiss_review(self):
        if self.review is not None:
            dialog = self.review[0]
            self.review = None
            dialog.destroy()

    def _decision(self, dialog, response, expected, reason_entry):
        if self.review is None or self.review[0] is not dialog:
            return
        reason = reason_text(reason_entry)
        if response in (Gtk.ResponseType.ACCEPT, Gtk.ResponseType.REJECT) and reason and self._still_reviewable(expected):
            self._dismiss_review()
            self._send("goal", {"command": "review", "sequence": expected[0], "candidate_version_id": expected[1],
                       "decision": "approved" if response == Gtk.ResponseType.ACCEPT else "rejected",
                       "operator_id": self.operator, "reason": reason})
        else:
            self._dismiss_review()
            self._refresh()
