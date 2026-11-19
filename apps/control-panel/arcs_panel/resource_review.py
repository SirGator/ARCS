"""Review exact Core evidence before confirming a reservation handover."""
import json

from gi.repository import Gtk

from .control_widgets import reason_text, text_label


class ResourceReview(Gtk.Dialog):
    def __init__(self, parent, record, recent, operator, submit):
        super().__init__(title=f"Ressourcen von Ziel {record['sequence']} klären", transient_for=parent, modal=True)
        self.record = record
        self.operator = operator
        self.submit = submit
        self.pending = None
        self.inspected = ()
        self.versions = [item["version_id"] for item in record["reservations"]]
        self.set_default_size(690, 570)
        self.add_button("Schließen", Gtk.ResponseType.CANCEL)
        self.add_button("Übergabe bestätigen", Gtk.ResponseType.ACCEPT)
        self.set_default_response(Gtk.ResponseType.CANCEL)

        content = self.get_content_area()
        content.set_spacing(8)
        content.set_border_width(16)
        summary = [f"{item['resource']} · {item['version_id']}" for item in record["reservations"]]
        if record.get("execution"):
            summary.append(f"Ausführung {record['execution']}")
        if record.get("outcome"):
            summary.append(f"Wirkungsbewertung {record['outcome']}")
        summary_view = Gtk.ScrolledWindow(min_content_height=62, max_content_height=120, propagate_natural_height=True)
        revisions = text_label("\n".join(summary))
        revisions.set_selectable(True)
        summary_view.add(revisions)
        content.pack_start(summary_view, False, False, 0)

        if recent:
            picker = Gtk.ComboBoxText()
            for item in reversed(recent):
                picker.append(item["version_id"], f"{item['schema_id']} · {item['producer_id']} · {item['created_at']}")
            picker.insert(0, "", "Kürzlich gespeicherten Beleg auswählen")
            picker.set_active(0)
            picker.connect("changed", self._pick_evidence)
            content.pack_start(picker, False, False, 0)

        heading = text_label("Belegversionen, eine pro Zeile")
        content.pack_start(heading, False, False, 0)
        self.evidence = Gtk.TextView(monospace=True, wrap_mode=Gtk.WrapMode.CHAR)
        heading.set_mnemonic_widget(self.evidence)
        self.evidence.get_buffer().connect("changed", self._evidence_changed)
        evidence_view = Gtk.ScrolledWindow(min_content_height=62)
        evidence_view.add(self.evidence)
        content.pack_start(evidence_view, False, False, 0)
        self.inspect = Gtk.Button(label="Belege anzeigen", halign=Gtk.Align.START)
        self.inspect.connect("clicked", self._inspect)
        content.pack_start(self.inspect, False, False, 0)

        preview_view = Gtk.ScrolledWindow(vexpand=True, min_content_height=110)
        self.preview = Gtk.TextView(editable=False, monospace=True, wrap_mode=Gtk.WrapMode.CHAR)
        preview_view.add(self.preview)
        content.pack_start(preview_view, True, True, 0)
        self.status = text_label("Bestätige die Übergabe nach Prüfung des Gerätezustands und der Belege.", "muted")
        content.pack_start(self.status, False, False, 0)
        self.reason = Gtk.Entry(placeholder_text="Begründung der Übergabe")
        self.reason.connect("changed", lambda *_: self._refresh())
        content.pack_start(self.reason, False, False, 0)
        self._refresh()

    def _evidence_versions(self):
        buffer = self.evidence.get_buffer()
        raw = buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), True)
        return tuple(line.strip() for line in raw.splitlines() if line.strip())

    def _pick_evidence(self, picker):
        version = picker.get_active_id()
        if version:
            versions = self._evidence_versions()
            if version not in versions:
                self.evidence.get_buffer().set_text("\n".join((*versions, version)))
            picker.set_active(0)

    def _evidence_changed(self, *_):
        self.inspected = ()
        self.preview.get_buffer().set_text("")
        self._refresh()

    def _refresh(self):
        versions = self._evidence_versions()
        self.inspect.set_sensitive(self.pending is None and 1 <= len(versions) <= 32)
        self.set_response_sensitive(Gtk.ResponseType.ACCEPT, self.release_command() is not None)

    def _inspect(self, *_):
        if not self.inspect.get_sensitive():
            return
        versions = self._evidence_versions()
        command = {"command": "inspect_resource_evidence", "sequence": self.record["sequence"],
                   "reservation_versions": self.versions, "evidence_versions": versions}
        self.inspected = ()
        self.preview.get_buffer().set_text("")
        try:
            self.pending = self.submit("goal", command), versions
        except (OSError, RuntimeError, ValueError) as error:
            self.status.set_text(str(error))
        else:
            self.status.set_text("Lade Belege …")
        self._refresh()

    def result(self, event):
        if self.pending is None or event.get("request_id") != self.pending[0]:
            return
        _, expected = self.pending
        self.pending = None
        if not event.get("ok"):
            self.status.set_text(event.get("error", {}).get("message", "ARCS hat die Anfrage abgewiesen."))
        else:
            receipt = event.get("receipt") or {}
            artifacts = receipt.get("artifacts", [])
            if (receipt.get("sequence") == self.record["sequence"]
                    and receipt.get("reservation_versions") == self.versions
                    and tuple(item["version_id"] for item in artifacts) == expected == self._evidence_versions()):
                self.inspected = expected
                self.preview.get_buffer().set_text(json.dumps(artifacts, ensure_ascii=True, indent=2))
                self.status.set_text("Bestätige die Übergabe nach Prüfung des Gerätezustands und der Belege.")
            else:
                self.status.set_text("Die Antwort passt nicht zu den ausgewählten Belegversionen.")
        self._refresh()

    def release_command(self):
        reason = reason_text(self.reason)
        if self.pending is None and reason and self.inspected and self.inspected == self._evidence_versions():
            return {"command": "release_resources", "sequence": self.record["sequence"], "operator_id": self.operator,
                    "reason": reason, "reservation_versions": self.versions, "evidence_versions": self.inspected}
        return None
