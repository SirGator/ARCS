"""Native Einrichtung externer Modelle; alle API-Tests laufen außerhalb der GUI."""
import threading
import uuid

from gi.repository import GLib, Gtk

from .model_connections import ModelConnections, normalize_connection


def label(text, css=None):
    widget = Gtk.Label(label=text, xalign=0, wrap=True)
    if css:
        for name in css.split():
            widget.get_style_context().add_class(name)
    return widget


class ModelsPage(Gtk.Box):
    def __init__(self, root, use_configuration, connections=None):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        self.connections = connections or ModelConnections(root)
        self.use_configuration = use_configuration
        self.providers = []
        self.selected_id = None
        self.pending = False
        self.session_busy = False
        self.closed = False
        self._loading = False
        self._previous_endpoint = None
        self._generation = 0
        self.pack_start(label("Modelle verbinden", "card-title"), False, False, 0)
        self.pack_start(label("Verbinde lokale Modelle oder einen API-Dienst. Planung und Prüfung können unterschiedliche Modelle verwenden.", "muted"), False, False, 0)
        area = Gtk.ScrolledWindow()
        area.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        self.pack_start(area, True, True, 0)
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16, margin=12)
        area.add(content)
        self.controls = []
        saved = Gtk.Box(spacing=10)
        self.saved_combo = Gtk.ComboBoxText(hexpand=True)
        self.saved_combo.connect("changed", self._select_saved)
        saved.pack_start(self.saved_combo, True, True, 0)
        self.new_button = Gtk.Button(label="Neue Verbindung")
        self.new_button.connect("clicked", lambda *_: self.new_connection())
        saved.pack_start(self.new_button, False, False, 0)
        self.reload_button = Gtk.Button(label="Neu laden")
        self.reload_button.connect("clicked", self._reload)
        saved.pack_start(self.reload_button, False, False, 0)
        content.pack_start(saved, False, False, 0)
        self.controls += [self.saved_combo, self.new_button, self.reload_button]
        columns = Gtk.Box(spacing=20)
        content.pack_start(columns, False, False, 0)
        editor = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, hexpand=True)
        columns.pack_start(editor, True, True, 0)
        assignment_card = Gtk.Box(valign=Gtk.Align.START)
        assignment_card.get_style_context().add_class("card")
        assignment = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12, margin=16)
        assignment.set_size_request(265, -1)
        assignment_card.add(assignment)
        columns.pack_start(assignment_card, False, False, 0)
        self.form = Gtk.Grid(column_spacing=18, row_spacing=10)
        self.name_entry = Gtk.Entry(placeholder_text="Name (optional)", hexpand=True)
        self.provider_combo = Gtk.ComboBoxText(hexpand=True)
        self.provider_combo.connect("changed", self._provider_changed)
        self.model_entry = Gtk.Entry(placeholder_text="Modellname beim Dienst", hexpand=True)
        self.endpoint_entry = Gtk.Entry(placeholder_text="Vollständige API-Adresse", hexpand=True)
        self.key_entry = Gtk.Entry(visibility=False, input_purpose=Gtk.InputPurpose.PASSWORD, hexpand=True)
        self.key_entry.set_placeholder_text("API-Schlüssel")
        for row, (name, widget) in enumerate((("Anbieter / Format", self.provider_combo), ("Modell", self.model_entry), ("API-Adresse", self.endpoint_entry), ("API-Schlüssel", self.key_entry))):
            self.form.attach(label(name), 0, row, 1, 1)
            self.form.attach(widget, 1, row, 1, 1)
        editor.pack_start(self.form, False, False, 0)
        self.controls.append(self.form)
        self.remember = Gtk.CheckButton(label="Schlüssel im Systemschlüsselbund für weitere Starts merken")
        self.remember.get_child().set_line_wrap(True)
        self.remember.get_child().set_max_width_chars(48)
        editor.pack_start(self.remember, False, False, 0)
        self.key_hint = label("", "muted small")
        self.key_hint.set_max_width_chars(65)
        editor.pack_start(self.key_hint, False, False, 0)
        advanced = Gtk.Expander(label="Erweitert")
        expert = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, margin=10)
        expert.pack_start(self.name_entry, False, False, 0)
        self.requires_key = Gtk.CheckButton(label="Diese Verbindung benötigt einen API-Schlüssel")
        self.requires_key.connect("toggled", lambda *_: self._update_controls())
        expert.pack_start(self.requires_key, False, False, 0)
        self.json_mode = Gtk.CheckButton(label="JSON-Modus des kompatiblen Chat-Dienstes verwenden")
        self.json_mode.set_active(True)
        expert.pack_start(self.json_mode, False, False, 0)
        token_row = Gtk.Box(spacing=12)
        token_row.pack_start(label("Chat-Tokenparameter"), True, True, 0)
        self.token_field = Gtk.ComboBoxText()
        for value in ("max_completion_tokens", "max_tokens"):
            self.token_field.append(value, value)
        self.token_field.set_active(0)
        token_row.pack_end(self.token_field, False, False, 0)
        expert.pack_start(token_row, False, False, 0)
        self.api_version = Gtk.Entry(placeholder_text="Anthropic API-Version (optional)")
        expert.pack_start(self.api_version, False, False, 0)
        expert.pack_start(label("Eigene Adapter, Profile und Routen bleiben über die Startkonfiguration anschließbar.", "muted small"), False, False, 0)
        advanced.add(expert)
        self.controls.append(expert)
        editor.pack_start(advanced, False, False, 0)
        buttons = Gtk.Box(spacing=10)
        self.test_button = Gtk.Button(label="Verbindung testen")
        self.test_button.set_tooltip_text("Sendet eine kurze Testanfrage an das ausgewählte Modell.")
        self.test_button.connect("clicked", self._test)
        self.save_button = Gtk.Button(label="Speichern")
        self.save_button.get_style_context().add_class("primary")
        self.save_button.connect("clicked", self._save)
        self.delete_button = Gtk.Button(label="Entfernen")
        self.delete_button.connect("clicked", self._delete)
        for widget in (self.test_button, self.save_button, self.delete_button):
            buttons.pack_start(widget, False, False, 0)
        editor.pack_start(buttons, False, False, 0)
        probe_hint = label("Verbindung testen sendet eine kurze Testnachricht an das Modell.", "muted small")
        probe_hint.set_max_width_chars(65)
        editor.pack_start(probe_hint, False, False, 0)
        assignment.pack_start(label("Für Aufgaben verwenden", "card-title"), False, False, 0)
        mapping = Gtk.Grid(column_spacing=18, row_spacing=10)
        self.planner_combo = Gtk.ComboBoxText(hexpand=True)
        self.assessor_combo = Gtk.ComboBoxText(hexpand=True)
        self.vision_combo = Gtk.ComboBoxText(hexpand=True)
        for row, (name, widget) in enumerate((("Planung", self.planner_combo), ("Wirkungsprüfung", self.assessor_combo), ("Bildauswertung (optional)", self.vision_combo))):
            mapping.attach(label(name), 0, row * 2, 1, 1)
            mapping.attach(widget, 0, row * 2 + 1, 1, 1)
        self.controls.append(mapping)
        assignment.pack_start(mapping, False, False, 0)
        self.use_button = Gtk.Button(label="Für Aufgaben übernehmen")
        self.use_button.get_style_context().add_class("primary")
        self.use_button.connect("clicked", self._use)
        assignment.pack_start(self.use_button, False, False, 0)
        task_hint = label("Beide Zuordnungen dürfen dasselbe Modell nutzen. Das Aufgabenbeispiel steuert den simulierten Lüfter.", "muted small")
        task_hint.set_max_width_chars(30)
        assignment.pack_start(task_hint, False, False, 0)
        self.status = label("Anbieter werden aus der externen Anbindung geladen …", "muted")
        self.status.set_selectable(True)
        self.pack_start(self.status, False, False, 0)
        self._work(self.connections.setup.describe, self._loaded_providers)

    def _work(self, operation, success):
        if self.pending or self.closed:
            return
        self.pending = True
        self._generation += 1
        generation = self._generation
        self._update_controls()
        def worker():
            try:
                result, error = operation(), None
            except (ValueError, OSError) as failure:
                result, error = None, str(failure)
            except Exception:
                result, error = None, "Die Modelleinrichtung konnte den Vorgang nicht abschließen."
            def finished():
                if self.closed or generation != self._generation:
                    return False
                self.pending = False
                if error:
                    self.status.set_text(error)
                else:
                    success(result)
                self._update_controls()
                return False
            GLib.idle_add(finished)
        threading.Thread(target=worker, daemon=True).start()

    def _loaded_providers(self, providers):
        self.providers = providers
        self._loading = True
        self.provider_combo.remove_all()
        for item in providers:
            self.provider_combo.append(item["id"], item["label"])
        self._loading = False
        selected = self.connections.assignments["planner"] or (self.connections.records[0]["id"] if self.connections.records else None)
        self._refresh_saved(selected)
        if selected:
            self._select_saved()
        else:
            self.new_connection()
        self.status.set_text(self.connections.load_error or "Verbindung auswählen oder ein neues Modell hinzufügen.")

    def _refresh_saved(self, selected=None):
        self._loading = True
        self.saved_combo.remove_all()
        self.saved_combo.append("new", "Neue Verbindung")
        for record in self.connections.records:
            self.saved_combo.append(record["id"], record["name"])
        self.saved_combo.set_active_id(selected or "new")
        for role, combo in (("planner", self.planner_combo), ("assessor", self.assessor_combo), ("vision", self.vision_combo)):
            previous = combo.get_active_id()
            combo.remove_all()
            if role == "vision":
                combo.append("none", "Kein Bilddienst")
            for record in self.connections.records:
                if (record.get("kind", "llm") == "vision") == (role == "vision"):
                    combo.append(record["id"], record["name"])
            chosen = self.connections.assignments.get(role) or previous
            if chosen:
                combo.set_active_id(chosen)
            if combo.get_active() == -1 and len(combo.get_model()):
                combo.set_active(0)
        self._loading = False

    def new_connection(self):
        self.selected_id = uuid.uuid4().hex
        self._loading = True
        self.saved_combo.set_active_id("new")
        for widget in (self.name_entry, self.model_entry, self.endpoint_entry, self.key_entry, self.api_version):
            widget.set_text("")
        self.key_entry.set_placeholder_text("API-Schlüssel")
        self.remember.set_active(False)
        self.json_mode.set_active(True)
        self.token_field.set_active(0)
        self.provider_combo.set_active(0 if self.providers else -1)
        self._loading = False
        self._previous_endpoint = None
        self._provider_changed()
        self._update_controls()

    def _provider_changed(self, *_):
        if self._loading:
            return
        provider = next((item for item in self.providers if item["id"] == self.provider_combo.get_active_id()), None)
        if provider:
            if not self.endpoint_entry.get_text() or self.endpoint_entry.get_text() == self._previous_endpoint:
                self.endpoint_entry.set_text(provider["endpoint"])
            self._previous_endpoint = provider["endpoint"]
            self.requires_key.set_active(provider["requires_key"])
        self._update_controls()

    def _select_saved(self, *_):
        if self._loading:
            return
        selected = self.saved_combo.get_active_id()
        if selected == "new":
            self.new_connection()
            return
        record = next((item for item in self.connections.records if item["id"] == selected), None)
        if not record:
            return
        self.selected_id = record["id"]
        self._loading = True
        for name, widget in (("name", self.name_entry), ("model", self.model_entry), ("endpoint", self.endpoint_entry)):
            widget.set_text(record[name])
        self.provider_combo.set_active_id(record["provider"])
        self.requires_key.set_active(record["requires_key"])
        self.remember.set_active(record["remember_key"])
        self.json_mode.set_active(record["provider_options"].get("json_mode", True))
        self.token_field.set_active_id(record["provider_options"].get("token_field", "max_completion_tokens"))
        self.api_version.set_text(record["provider_options"].get("api_version", ""))
        self.key_entry.set_text("")
        self.key_entry.set_placeholder_text("Leer lassen, um den vorhandenen Schlüssel beizubehalten")
        self._loading = False
        self._previous_endpoint = next((item["endpoint"] for item in self.providers if item["id"] == record["provider"]), None)
        self._update_controls()

    def _record(self):
        provider = self.provider_combo.get_active_id()
        if provider not in {item["id"] for item in self.providers}:
            raise ValueError("Bitte einen unterstützten Anbieter auswählen.")
        model = self.model_entry.get_text().strip()
        options = {}
        if provider == "openai_chat":
            options = {"token_field": self.token_field.get_active_id(), "json_mode": self.json_mode.get_active()}
        elif provider == "anthropic" and self.api_version.get_text().strip():
            options["api_version"] = self.api_version.get_text().strip()
        return normalize_connection({"id": self.selected_id, "name": self.name_entry.get_text().strip() or model,
            "provider": provider, "model": model, "endpoint": self.endpoint_entry.get_text().strip(),
            "requires_key": self.requires_key.get_active(), "remember_key": self.remember.get_active(), "provider_options": options,
            "kind": next(item.get("kind", "llm") for item in self.providers if item["id"] == provider)})

    def _save(self, *_):
        try:
            record, key = self._record(), self.key_entry.get_text() or None
        except ValueError as failure:
            self.status.set_text(str(failure))
            return
        def saved(result):
            self.key_entry.set_text("")
            self._refresh_saved(result["id"])
            self._select_saved()
            self.status.set_text("Verbindung gespeichert. " + ("Schlüssel im Systemschlüsselbund gespeichert." if result["remember_key"] else "Schlüssel gilt nur für dieses geöffnete Kontrollpanel." if result["requires_key"] else "Kein API-Schlüssel erforderlich."))
        self.status.set_text("Verbindung wird gespeichert …")
        self._work(lambda: self.connections.save(record, key), saved)

    def _test(self, *_):
        try:
            record, entered = self._record(), self.key_entry.get_text() or None
        except ValueError as failure:
            self.status.set_text(str(failure))
            return
        def test():
            key = self.connections.key_for(record, entered)
            return self.connections.setup.probe(record, key)
        self.status.set_text("Kurze Testanfrage an das Modell läuft …")
        def tested(result):
            self.status.set_text(f"Verbindung erfolgreich: Bilddienst {result['model']} hat eine echte Testinferenz ausgeführt." if result.get("kind") == "vision" else f"Verbindung erfolgreich: Modell antwortet mit gültigem JSON ({result['output_tokens']} Ausgabetokens).")
        self._work(test, tested)

    def _delete(self, *_):
        identifier = self.saved_combo.get_active_id()
        def removed(_result):
            self._refresh_saved()
            self.new_connection()
            self.status.set_text("Verbindung entfernt.")
        self._work(lambda: self.connections.remove(identifier), removed)

    def _reload(self, *_):
        def reload():
            self.connections.reload()
            return self.connections.setup.describe()
        self._work(reload, self._loaded_providers)

    def _use(self, *_):
        planner, assessor = self.planner_combo.get_active_id(), self.assessor_combo.get_active_id()
        vision = self.vision_combo.get_active_id()
        def prepare():
            self.connections.assign(planner, assessor, vision if vision != "none" else None)
            configuration = self.connections.materialize()
            self.connections.environment_for(configuration)
            return configuration
        def ready(configuration):
            if self.session_busy:
                self.status.set_text("Zuordnung gespeichert. Bitte die laufende Sitzung stoppen und danach übernehmen.")
                return
            self.use_configuration(configuration)
            self.status.set_text("Modelle zugeordnet. Unter Eingabe ARCS starten und eine Aufgabe schreiben.")
        self.status.set_text("Aufgabenverbindung wird vorbereitet …")
        self._work(prepare, ready)

    def set_session_busy(self, busy):
        self.session_busy = busy
        self._update_controls()

    def _update_controls(self):
        if not hasattr(self, "test_button"):
            return
        enabled = not self.pending and not self.session_busy and bool(self.providers)
        for widget in self.controls:
            widget.set_sensitive(enabled)
        self.reload_button.set_sensitive(not self.pending and not self.session_busy)
        for widget in (self.test_button, self.save_button):
            widget.set_sensitive(enabled)
        self.delete_button.set_sensitive(enabled and self.saved_combo.get_active_id() not in (None, "new"))
        self.use_button.set_sensitive(enabled and bool(self.connections.records))
        self.remember.set_sensitive(enabled and self.requires_key.get_active() and self.connections.credentials.persistent_available)
        self.key_entry.set_sensitive(enabled and self.requires_key.get_active())
        if not self.requires_key.get_active():
            self.key_entry.set_placeholder_text("Für diese Verbindung nicht erforderlich")
        else:
            record = next((item for item in self.connections.records if item["id"] == self.selected_id), None)
            available = record and (record["remember_key"] or self.connections.credentials.has_session_key(record["id"]))
            self.key_entry.set_placeholder_text("Leer lassen, um den vorhandenen Schlüssel beizubehalten" if available else "API-Schlüssel für diese Sitzung eingeben")
        self.json_mode.set_sensitive(enabled and self.provider_combo.get_active_id() == "openai_chat")
        self.token_field.set_sensitive(enabled and self.provider_combo.get_active_id() == "openai_chat")
        self.api_version.set_sensitive(enabled and self.provider_combo.get_active_id() == "anthropic")
        self.key_hint.set_text("Schlüssel bleiben außerhalb der Verbindungsdateien. Dauerhaftes Merken ist über den Systemschlüsselbund möglich." if self.connections.credentials.persistent_available else "Kein Systemschlüsselbund verfügbar: Schlüssel gelten nur für dieses geöffnete Kontrollpanel. Verbindungen und Zuordnungen bleiben gespeichert.")

    def close(self):
        self.closed = True
        self.connections.close()
        self.key_entry.set_text("")
