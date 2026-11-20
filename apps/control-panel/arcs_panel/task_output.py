"""Preview and decide a version-bound report for a completed task."""
from gi.repository import Gtk

from .control_widgets import local_operator, reason_text, text_label


class TaskOutputDialog(Gtk.Dialog):
    def __init__(self, parent, submit, sequence):
        super().__init__(title=f"Aufgabe {sequence} ausgeben", transient_for=parent, modal=True)
        self.set_default_size(660, 430)
        self.add_button("Schließen", Gtk.ResponseType.CLOSE)
        self.connect("response", lambda *_: self.destroy())
        self.submit = submit
        self.sequence = sequence
        self.pending = None
        self.options = None
        self.preview = None
        self.review = None
        self.delivery = None

        content = self.get_content_area()
        content.set_border_width(16)
        content.set_spacing(10)
        selectors = Gtk.Grid(column_spacing=10, row_spacing=8)
        self.result_choice = Gtk.ComboBoxText(hexpand=True)
        self.destination_choice = Gtk.ComboBoxText(hexpand=True)
        for row, title, widget in (
            (0, "Ergebnisversion", self.result_choice),
            (1, "Zieladapter", self.destination_choice),
        ):
            selectors.attach(text_label(title), 0, row, 1, 1)
            selectors.attach(widget, 1, row, 1, 1)
            widget.connect("changed", self._selection_changed)
        content.pack_start(selectors, False, False, 0)

        self.preview_button = Gtk.Button(label="Bericht anzeigen")
        self.preview_button.connect("clicked", self._load_preview)
        content.pack_start(self.preview_button, False, False, 0)
        viewport = Gtk.ScrolledWindow(vexpand=True, min_content_height=160)
        self.report = Gtk.TextView(editable=False, cursor_visible=False, wrap_mode=Gtk.WrapMode.WORD_CHAR)
        self.report.set_monospace(True)
        viewport.add(self.report)
        content.pack_start(viewport, True, True, 0)

        actions = Gtk.Box(spacing=8)
        self.reason = Gtk.Entry(placeholder_text="Begründung")
        self.reason.connect("changed", lambda *_: self._refresh())
        actions.pack_start(self.reason, True, True, 0)
        self.reject_button = Gtk.Button(label="Ablehnen")
        self.approve_button = Gtk.Button(label="Zustellung freigeben")
        self.reject_button.connect("clicked", self._decide, "rejected")
        self.approve_button.connect("clicked", self._decide, "approved")
        actions.pack_start(self.reject_button, False, False, 0)
        actions.pack_start(self.approve_button, False, False, 0)
        content.pack_start(actions, False, False, 0)
        self.status = text_label(css="muted")
        self.status.set_selectable(True)
        content.pack_start(self.status, False, False, 0)
        self.show_all()
        self._send("task_output", "options", {"task_sequence": sequence})

    def _send(self, kind, command, fields):
        if self.pending:
            return
        try:
            self.pending = self.submit(kind, {"command": command, **fields}), kind, command
        except (OSError, RuntimeError, ValueError) as error:
            self.status.set_text(str(error))
        else:
            self.status.set_text("Warte auf ARCS …")
        self._refresh()

    def _selection_changed(self, *_):
        self.preview = self.review = self.delivery = None
        self.report.get_buffer().set_text("")
        self.reason.set_text("")
        self._refresh()

    def _load_preview(self, _):
        version = self.result_choice.get_active_id()
        index = self.destination_choice.get_active()
        if not self.options or version is None or index < 0:
            return
        target = {**self.options["destinations"][index],
                  "task_sequence": self.sequence, "result_version_id": version}
        self._send("task_output", "preview", {"target": target})

    def _decide(self, _, decision):
        reason = reason_text(self.reason)
        if self.preview is None or self.review is not None or reason is None:
            return
        review = {key: self.preview[key] for key in ("target", "request_version_id", "verification_version_id")}
        self._send("task_output", "review", {"review": {**review, "decision": decision,
                                                        "operator_id": local_operator(), "reason": reason}})

    def result(self, event):
        if not self.pending or self.pending[0] != event.get("request_id"):
            return
        _, kind, command = self.pending
        self.pending = None
        if not event.get("ok"):
            self.status.set_text(event.get("error", {}).get("message", "ARCS hat die Anfrage abgewiesen."))
            self._refresh()
            return
        receipt = event["receipt"]
        if kind == "output":
            self.delivery = receipt
            self._show_status()
        elif command == "options":
            self.options = receipt
            for version in receipt["result_versions"]:
                self.result_choice.append(version, version)
            for index, target in enumerate(receipt["destinations"]):
                capability = target["destination"]
                self.destination_choice.append(
                    str(index),
                    f"{capability['adapter_id']} · {capability['capability_id']} · {target['receipt_schema_id']}",
                )
            self.result_choice.set_active(0)
            self.destination_choice.set_active(0)
            self.status.set_text("" if receipt["destinations"] else "Kein passender Output-Adapter verbunden.")
        elif command == "preview":
            self.preview = receipt
            self.review = receipt.get("review")
            self.delivery = None
            payload = receipt["payload"]
            self.report.get_buffer().set_text(f"{payload['title']}\n\n{payload['body']}")
            self._show_status()
        else:
            self.review = receipt
            self._show_status()
        self._refresh()

    def snapshot(self, runtime):
        if self.review is None or self.pending:
            return
        if self.review["state"] == "pending":
            self._send("task_output", "inspect", {"request_version_id": self.review["request_version_id"]})
        elif self.review["state"] == "queued":
            sequence = self.review["delivery_sequence"]
            self.delivery = next((item for item in runtime.get("output_deliveries", [])
                                  if item["sequence"] == sequence), self.delivery)
            if self.delivery is None:
                self._send("output", "inspect", {"sequence": sequence})
            else:
                self._show_status()

    def _show_status(self):
        if self.review is None:
            message = "Prüfe den angezeigten Bericht vor der Freigabe."
        elif self.review["state"] == "rejected":
            message = "Ausgabe abgelehnt."
        elif self.review["state"] == "failed":
            message = self.review["error"]
        elif self.delivery and self.delivery["state"] == "delivered":
            message = f"Zugestellt · Quittung {self.delivery['receipt_version_id']}"
        elif self.delivery and self.delivery["state"] == "failed":
            message = self.delivery["error"]
        elif self.review["state"] == "queued":
            message = f"Zustellung {self.review['delivery_sequence']} in der Warteschlange."
        else:
            message = "Entscheidung gespeichert."
        self.status.set_text(message)

    def _refresh(self):
        idle = self.pending is None
        self.result_choice.set_sensitive(idle and bool(self.options))
        self.destination_choice.set_sensitive(idle and bool(self.options))
        self.preview_button.set_sensitive(idle and self.result_choice.get_active() >= 0
                                          and self.destination_choice.get_active() >= 0)
        can_decide = idle and self.preview is not None and self.review is None
        self.reason.set_sensitive(can_decide)
        for button in (self.approve_button, self.reject_button):
            button.set_sensitive(can_decide and reason_text(self.reason) is not None)
