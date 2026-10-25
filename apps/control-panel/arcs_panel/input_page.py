"""Native Eingabeformulare aus dem tatsächlichen Input-Vertrag der Sitzung."""
import json

from gi.repository import Gtk

from .input_bridge import parse_json


def text_label(text, css=None):
    widget = Gtk.Label(label=text, xalign=0)
    widget.set_line_wrap(True)
    if css:
        for name in css.split():
            widget.get_style_context().add_class(name)
    return widget


def scroll(widget):
    area = Gtk.ScrolledWindow()
    area.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
    area.add(widget)
    return area


def editor(monospace=False):
    return Gtk.TextView(monospace=monospace, wrap_mode=Gtk.WrapMode.WORD_CHAR,
        left_margin=14, right_margin=14, top_margin=12, bottom_margin=12)


def contents(widget):
    buffer = widget.get_buffer()
    return buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), False)


class InputPage(Gtk.Box):
    def __init__(self, submit):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=13)
        self.submit = submit
        self.targets = []
        self.fields = {}
        self._extra_fields = {}
        self.running = False
        self.pending = False
        self.history = []
        self._last_submission = None
        self._loading = False
        self.pack_start(text_label("Aufgabe eingeben", "card-title"), False, False, 0)
        self.pack_start(text_label("Beschreibe dein Ziel. Die angeschlossenen Reasoner übernehmen die fachliche Bearbeitung.", "muted"), False, False, 0)
        tools = Gtk.Box(spacing=12)
        tools.pack_start(text_label("Eingang"), False, False, 0)
        self.target_combo = Gtk.ComboBoxText()
        self.target_combo.set_hexpand(True)
        self.target_combo.connect("changed", self._select_target)
        tools.pack_start(self.target_combo, True, True, 0)
        self.mode_combo = Gtk.ComboBoxText()
        self.mode_combo.append("form", "Formular")
        self.mode_combo.append("json", "Erweitert: JSON")
        self.mode_combo.set_active_id("form")
        self.mode_combo.connect("changed", self._change_mode)
        tools.pack_end(self.mode_combo, False, False, 0)
        self.pack_start(tools, False, False, 0)
        self.description = text_label("ARCS starten, um verfügbare Eingänge zu verbinden.", "muted small")
        self.description.set_selectable(True)
        self.pack_start(self.description, False, False, 0)
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.NONE)
        self.form = Gtk.Grid(column_spacing=18, row_spacing=14, margin=18)
        self.stack.add_named(scroll(self.form), "form")
        self.json_editor = editor(monospace=True)
        self.stack.add_named(scroll(self.json_editor), "json")
        self.pack_start(self.stack, True, True, 0)
        footer = Gtk.Box(spacing=15)
        self.status = text_label("Noch keine Eingabe gesendet.", "muted")
        footer.pack_start(self.status, True, True, 0)
        self.send_button = Gtk.Button(label="Aufgabe senden")
        self.send_button.get_style_context().add_class("primary")
        self.send_button.connect("clicked", self._send)
        footer.pack_end(self.send_button, False, False, 0)
        self.pack_start(footer, False, False, 0)
        self.receipts = text_label("", "muted small")
        self.receipts.set_selectable(True)
        self.pack_start(self.receipts, False, False, 0)
        self._update_controls()

    @property
    def target(self):
        key = self.target_combo.get_active_id()
        return next((target for target in self.targets if target.key == key), None)

    def connect_targets(self, targets, has_runtime):
        self.targets = targets
        self._loading = True
        self.target_combo.remove_all()
        for target in targets:
            self.target_combo.append(target.key, f"{target.adapter_id}  ·  {target.capability_id}")
        self.target_combo.set_active(0 if targets else -1)
        self._loading = False
        self._select_target()
        if targets:
            self.status.set_text("Aufgaben folgen den konfigurierten Runtime-Routen." if has_runtime else "Eingaben werden gespeichert. Für eine Bearbeitung muss eine passende Reasoning-Route angeschlossen sein.")
        else:
            self.description.set_text("Kein beschreibbarer Eingang verbunden. Die GUI unterstützt derzeit den JSON-Input-Adapter mit seinem --enqueue-Eingang.")

    def set_running(self, running):
        self.running = running
        self._update_controls()

    def reset(self):
        self.running = False
        self.pending = False
        self.connect_targets([], False)
        self.description.set_text("ARCS starten, um verfügbare Eingänge zu verbinden.")
        self.status.set_text("Noch keine Eingabe gesendet.")

    def _update_controls(self):
        active = self.running and bool(self.targets) and not self.pending
        self.target_combo.set_sensitive(active)
        self.mode_combo.set_sensitive(active)
        self.stack.set_sensitive(active)
        self.send_button.set_sensitive(active)

    def _select_target(self, *_):
        if self._loading:
            return
        for child in self.form.get_children():
            self.form.remove(child)
        self.fields = {}
        self._extra_fields = {}
        target = self.target
        if target is None:
            self.json_editor.get_buffer().set_text("")
            self._update_controls()
            return
        schema = target.schema
        self.description.set_text(f"{target.schema_id}  ·  bis {target.max_payload_bytes:,} Bytes\nDie Eingabe geht in die Warteschlange dieses Adapters.")
        properties = schema.get("properties", {})
        initial = {}
        for row, name in enumerate(schema.get("required", [])):
            field = properties.get(name, {})
            kind = field.get("type")
            value = target.template.get(name, field.get("const", field.get("enum", [None])[0]))
            if value is None:
                value = {"string":"", "boolean":False, "integer":0, "number":0, "array":[], "object":{}}.get(kind)
            initial[name] = value
            caption = text_label(field.get("title", "Deine Aufgabe" if name == "raw_text" else name))
            caption.set_valign(Gtk.Align.START)
            caption.set_margin_top(10)
            self.form.attach(caption, 0, row * 2, 2 if name == "raw_text" else 1, 1)
            if kind == "boolean":
                widget = Gtk.CheckButton(label="Aktiv")
                widget.set_active(bool(value))
                container = widget
            elif kind == "string" and name == "raw_text":
                widget = editor()
                widget.get_style_context().add_class("input-text")
                widget.set_vexpand(True)
                widget.get_buffer().set_text(value if isinstance(value, str) else "")
                area = scroll(widget)
                area.set_min_content_height(120)
                container = Gtk.Frame()
                container.get_style_context().add_class("input-editor")
                container.add(area)
            else:
                widget = Gtk.Entry()
                widget.set_text(value if kind == "string" and isinstance(value, str) else json.dumps(value, ensure_ascii=False))
                if kind in ["array", "object"]:
                    widget.set_tooltip_text("Als JSON eingeben, zum Beispiel [1, 2, 4].")
                container = widget
            container.set_hexpand(True)
            self.form.attach(container, 0 if name == "raw_text" else 1, row * 2 + (1 if name == "raw_text" else 0), 2 if name == "raw_text" else 1, 1)
            self.fields[name] = (kind, widget)
        self.mode_combo.get_model().set_value(self.mode_combo.get_model().get_iter_first(), 0, "Text" if list(self.fields) == ["raw_text"] else "Formular")
        self._loading = True
        self.stack.set_visible_child_name("form")
        if schema.get("type") != "object" or not self.fields:
            self.mode_combo.set_active_id("json")
            initial = target.template or None
        else:
            self.mode_combo.set_active_id("form")
        self.json_editor.get_buffer().set_text(json.dumps(initial, indent=2, ensure_ascii=False))
        self._loading = False
        self.form.show_all()
        self._change_mode()
        self._update_controls()

    def _form_payload(self):
        payload = dict(self._extra_fields)
        for name, (kind, widget) in self.fields.items():
            if kind == "boolean":
                payload[name] = widget.get_active()
            elif isinstance(widget, Gtk.TextView):
                payload[name] = contents(widget)
            elif kind == "string":
                payload[name] = widget.get_text()
            else:
                try:
                    payload[name] = parse_json(widget.get_text())
                except ValueError as error:
                    raise ValueError(f"{name}: {error}") from error
        return payload

    def _change_mode(self, *_):
        if self._loading:
            return
        mode = self.mode_combo.get_active_id() or "form"
        if mode == "form" and not self.fields:
            self.mode_combo.set_active_id("json")
            return
        if mode == "json" and self.fields and self.stack.get_visible_child_name() == "form":
            try:
                self.json_editor.get_buffer().set_text(json.dumps(self._form_payload(), indent=2, ensure_ascii=False))
            except ValueError as error:
                self.status.set_text(str(error))
                self.mode_combo.set_active_id("form")
                return
        elif mode == "form" and self.fields and self.stack.get_visible_child_name() == "json":
            try:
                payload = parse_json(contents(self.json_editor))
                if not isinstance(payload, dict) or any(name not in payload for name in self.fields):
                    raise ValueError("Das Formular benötigt ein Objekt mit allen Pflichtfeldern.")
                for name, (kind, widget) in self.fields.items():
                    value = payload[name]
                    if kind == "boolean":
                        if not isinstance(value, bool):
                            raise ValueError(f"{name}: true oder false erwartet.")
                        widget.set_active(value)
                    elif kind == "string":
                        if not isinstance(value, str):
                            raise ValueError(f"{name}: Text erwartet.")
                        if isinstance(widget, Gtk.TextView):
                            widget.get_buffer().set_text(value)
                        else:
                            widget.set_text(value)
                    else:
                        widget.set_text(json.dumps(value, ensure_ascii=False))
                self._extra_fields = {name:value for name,value in payload.items() if name not in self.fields}
            except ValueError as error:
                self.status.set_text(str(error))
                self.mode_combo.set_active_id("json")
                return
        for child in self.stack.get_children():
            child.set_visible(self.stack.child_get_property(child, "name") == mode)
        self.stack.set_visible_child_name(mode)

    def _send(self, *_):
        target = self.target
        if target is None or not self.running or self.pending:
            return
        try:
            payload = parse_json(contents(self.json_editor)) if self.mode_combo.get_active_id() == "json" else self._form_payload()
            identity = self.submit(target.key, payload)
        except (ValueError, RuntimeError, OSError) as error:
            self.status.set_text(str(error))
            return
        self.pending = True
        key = (target.adapter_id, identity)
        self._last_submission = key
        record = next((item for item in self.history if (item['adapter_id'], item['id']) == key), None)
        if record is None:
            self.history.append({'adapter_id':target.adapter_id, 'id':identity, 'receipt':'sending'})
            self.history = self.history[-3:]
        self.status.set_text("Eingabe wird geprüft und an den Adapter übertragen …")
        self._update_controls()

    def result(self, event):
        self.pending = False
        key = (event['adapter_id'], event['id'])
        record = next((item for item in self.history if (item['adapter_id'], item['id']) == key), None)
        if record is None:
            record = {'adapter_id':event['adapter_id'], 'id':event['id']}
            self.history.append(record)
            self.history = self.history[-3:]
        if event["event"] == "input_queued":
            record['receipt'] = 'queued'
        else:
            record['receipt'] = 'error'
            record['error'] = event['message']
        self._render_history()
        self._update_controls()

    def goal_progress(self, goals):
        changed = False
        for goal in goals:
            source = goal.get('input_source', {})
            for record in self.history:
                if source.get('adapter_id') == record['adapter_id'] and source.get('reference') == 'gui:' + record['id']:
                    progress = (goal['state'], goal.get('detail', ''))
                    routes = record.setdefault('goals', {})
                    if routes.get(goal['route_id']) != progress:
                        routes[goal['route_id']] = progress
                        changed = True
        if changed:
            self._render_history()

    def _render_history(self):
        labels = {'acquiring':'Daten erfassen', 'checking':'Aufgabe prüfen', 'planning':'Nächsten Schritt planen',
            'verifying':'Vorschlag prüfen', 'awaiting_approval':'Wartet auf Freigabe', 'executing':'Aktion ausführen',
            'observing':'Wirkung beobachten', 'assessing':'Ziel bewerten', 'completed':'Ziel erreicht',
            'blocked':'Aufgabe angehalten', 'rejected':'Abgelehnt', 'budget_exhausted':'Schrittbudget erreicht', 'cancelled':'Abgebrochen'}
        lines = []
        for record in reversed(self.history):
            summary = ''
            if record.get('goals'):
                if len(record['goals']) == 1:
                    state, detail = next(iter(record['goals'].values()))
                    message = labels.get(state, state) + (': ' + detail[:240] if detail else '')
                    summary = labels.get(state, state)
                else:
                    message = '; '.join(f"{route}: {labels.get(state, state)}" for route,(state,_) in record['goals'].items())
                    summary = message
            elif record.get('receipt') == 'queued':
                message = 'Im Input-Adapter gespeichert. ARCS übernimmt die Eingabe beim nächsten Abruf.'
                summary = 'im Adapter gespeichert'
            elif record.get('receipt') == 'error':
                message = record['error'] + '\nBei unveränderter Eingabe verwendet Wiederholen dieselbe Kennung.'
                summary = 'Übertragung fehlgeschlagen'
            else:
                message = 'Aufgabe wird übertragen …'
                summary = message
            lines.append(f"{record['adapter_id']} · {record['id'][-8:]} · {summary[:120]}")
            if (record['adapter_id'], record['id']) == self._last_submission:
                self.status.set_text(message)
        self.receipts.set_text('\n'.join(lines))
