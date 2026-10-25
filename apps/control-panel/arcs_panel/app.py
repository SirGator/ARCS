"""GTK-Desktop-Oberfläche. Host und Treiber bleiben eigenständige Prozesse."""
from __future__ import annotations

from datetime import datetime
import json
from pathlib import Path
import queue
import time
from zoneinfo import ZoneInfo

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk, GdkPixbuf, Gio, GLib, Gtk, Pango

from .controller import Configuration, PROJECT_ROOT, SessionController, read_configuration
from .input_page import InputPage

ASSETS = Path(__file__).resolve().parent
ROLES = ["input", "observe", "request", "transform", "reason", "act", "output"]
ROLE_NAMES = {role: role.capitalize() for role in ROLES}


def label(text="", css=None, xalign=0):
    widget = Gtk.Label(label=text, xalign=xalign)
    if css:
        for name in css.split():
            widget.get_style_context().add_class(name)
    return widget


def box(vertical=True, spacing=0, css=None, margin=0):
    widget = Gtk.Box(orientation=Gtk.Orientation.VERTICAL if vertical else Gtk.Orientation.HORIZONTAL, spacing=spacing)
    if css:
        widget.get_style_context().add_class(css)
    for side in ["top", "bottom", "start", "end"]:
        getattr(widget, "set_margin_" + side)(margin)
    return widget


def button(text, icon=None, css=None, callback=None):
    widget = Gtk.Button()
    content = box(False, 7)
    if icon:
        content.pack_start(Gtk.Image.new_from_icon_name(icon, Gtk.IconSize.BUTTON), False, False, 0)
    content.pack_start(label(text), False, False, 0)
    widget.add(content)
    if css:
        for name in css.split():
            widget.get_style_context().add_class(name)
    if callback:
        widget.connect("clicked", callback)
    return widget


class ControlWindow(Gtk.ApplicationWindow):
    def __init__(self, application=None, configuration=None, root=PROJECT_ROOT, persist_settings=True):
        super().__init__(application=application, title="ARCS Control")
        self.root = root.resolve()
        self.persist_settings = persist_settings
        self._configuration_valid = False
        self.controller = SessionController(self.root)
        self.set_default_size(1260, 850)
        self.set_size_request(1050, 730)
        self.set_position(Gtk.WindowPosition.CENTER)
        self.set_icon_from_file(str(ASSETS / "assets/arcs.svg"))
        self.state = "idle"
        self.catalog = []
        self.snapshot = {}
        self.recent = []
        self.log_lines = []
        self._last_artifacts = None
        self._last_ready = None
        self._last_goals = {}
        self._closing = False
        self._updating = False
        self._stopping_since = None
        self._source_id = None
        self.presets = [
            ("Aufgaben per Text · Beispiel", Configuration(self.root / "apps/adapter-host/config/text-tasks/host.json", self.root / "apps/adapter-host/config/text-tasks/runtime.json")),
            ("Mehrere Adapter", Configuration(self.root / "apps/adapter-host/config/multiple-adapters/host.json")),
            ("Aufgaben & Reasoning", Configuration(self.root / "apps/adapter-host/config/roles/host.json", self.root / "apps/adapter-host/config/roles/runtime.json")),
            ("Zielablauf · Lüftertest", Configuration(self.root / "apps/adapter-host/config/goals/host.json", self.root / "apps/adapter-host/config/goals/runtime.json")),
            ("Texteingabe", Configuration(self.root / "apps/adapter-host/config/text-input/host.json")),
            ("Sensor & Lüfter", Configuration(self.root / "apps/adapter-host/config/host.json")),
            ("Aufgaben per Text · LLM-API", Configuration(self.root / "apps/adapter-host/config/llm-tasks/host.json", self.root / "apps/adapter-host/config/llm-tasks/runtime.json")),
        ]
        provider = Gtk.CssProvider()
        provider.load_from_path(str(ASSETS / "style.css"))
        Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        settings = Gtk.Settings.get_default()
        settings.set_property("gtk-application-prefer-dark-theme", True)
        self._build()
        self.connect("delete-event", self._on_close)
        self.connect("destroy", self._on_destroy)
        self.show_all()
        self.show_page("input")
        self.force_button.hide()
        self.banner_row.hide()
        if configuration is None:
            configuration = self._load_settings() or self.presets[0][1]
        self.select_configuration(configuration)
        self._source_id = GLib.timeout_add(100, self._poll_events)

    def _build(self):
        header = Gtk.HeaderBar(show_close_button=True)
        header.set_title("ARCS Control")
        header.set_subtitle("Lokale Steuerzentrale")
        self.set_titlebar(header)
        shell = box(False)
        self.add(shell)
        sidebar = box(css="sidebar", margin=0)
        sidebar.set_size_request(215, -1)
        shell.pack_start(sidebar, False, False, 0)
        brand = box(False, 12, margin=25)
        brand.pack_start(Gtk.Image.new_from_pixbuf(GdkPixbuf.Pixbuf.new_from_file_at_scale(str(ASSETS / "assets/arcs.svg"), 38, 38, True)), False, False, 0)
        brand_text = box(spacing=3)
        brand_text.pack_start(label("ARCS", "brand"), False, False, 0)
        brand_text.pack_start(label("CONTROL PANEL", "brand-subtitle"), False, False, 0)
        brand.pack_start(brand_text, False, False, 0)
        sidebar.pack_start(brand, False, False, 0)
        nav = box(spacing=7, margin=15)
        kicker = label("ARBEITSBEREICH", "section-kicker")
        kicker.set_margin_start(14)
        kicker.set_margin_bottom(8)
        nav.pack_start(kicker, False, False, 0)
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.NONE)
        self.nav_buttons = {}
        for key, title, icon in [("overview", "Übersicht", "view-grid-symbolic"), ("input", "Eingabe", "mail-message-new-symbolic"), ("adapters", "Adapter", "network-workgroup-symbolic"), ("logs", "Protokolle", "utilities-terminal-symbolic")]:
            item = button(title, icon, "nav", lambda _, name=key: self.show_page(name))
            self.nav_buttons[key] = item
            nav.pack_start(item, False, False, 0)
        sidebar.pack_start(nav, False, False, 0)
        sidebar.pack_start(box(), True, True, 0)
        side_footer = box(spacing=9, margin=25)
        side_footer.pack_start(label("LOKALE SITZUNG", "section-kicker"), False, False, 0)
        self.side_status = label("●  Startbereit", "muted")
        side_footer.pack_start(self.side_status, False, False, 0)
        side_footer.pack_start(label("ARCS Control · 0.1", "footer"), False, False, 0)
        sidebar.pack_start(side_footer, False, False, 0)
        main = box(spacing=20, margin=28)
        shell.pack_start(main, True, True, 0)
        top = box(False, 12)
        titles = box(spacing=6)
        titles.pack_start(label("Kontrollzentrum", "page-title"), False, False, 0)
        titles.pack_start(label("Starte ARCS und behalte seine Komponenten im Blick.", "subtitle"), False, False, 0)
        top.pack_start(titles, True, True, 0)
        self.build_button = button("Bauen", "system-run-symbolic", callback=self._on_build)
        self.build_button.set_tooltip_text("ARCS und seine allgemeinen Adapter lokal bauen")
        self.stop_button = button("Stoppen", "media-playback-stop-symbolic", callback=self._on_stop)
        self.start_button = button("ARCS starten", "media-playback-start-symbolic", "primary", self._on_start)
        for widget in [self.build_button, self.stop_button, self.start_button]:
            widget.set_valign(Gtk.Align.CENTER)
            top.pack_start(widget, False, False, 0)
        main.pack_start(top, False, False, 0)
        self.config_card = box(spacing=12, css="card", margin=0)
        config_inner = box(spacing=12, margin=17)
        self.config_card.add(config_inner)
        config_top = box(False, 12)
        config_top.pack_start(label("STARTKONFIGURATION", "metric-title"), True, True, 0)
        self.preset_combo = Gtk.ComboBoxText()
        for name, _ in self.presets:
            self.preset_combo.append_text(name)
        self.preset_combo.append_text("Eigene Konfiguration")
        self.preset_combo.connect("changed", self._on_preset)
        config_top.pack_start(self.preset_combo, False, False, 0)
        config_inner.pack_start(config_top, False, False, 0)
        self.host_entry = Gtk.Entry(editable=False)
        self.runtime_entry = Gtk.Entry(editable=False)
        self.runtime_entry.set_placeholder_text("Keine Aufgaben-Runtime ausgewählt")
        grid = Gtk.Grid(column_spacing=10, row_spacing=9)
        self.config_buttons = []
        for row, title, entry, callback in [(0, "Host", self.host_entry, self._choose_host), (1, "Aufgaben-Runtime", self.runtime_entry, self._choose_runtime)]:
            grid.attach(label(title, "config-label"), 0, row, 1, 1)
            entry.set_hexpand(True)
            grid.attach(entry, 1, row, 1, 1)
            browse = button("Auswählen", "folder-open-symbolic", callback=callback)
            self.config_buttons.append(browse)
            grid.attach(browse, 2, row, 1, 1)
        self.clear_runtime = button("Entfernen", css="flat", callback=self._clear_runtime)
        grid.attach(self.clear_runtime, 3, 1, 1, 1)
        self.connection_settings = Gtk.Expander(label="Verbindungen konfigurieren", expanded=False)
        grid.set_margin_top(10)
        self.connection_settings.add(grid)
        config_inner.pack_start(self.connection_settings, False, False, 0)
        main.pack_start(self.config_card, False, False, 0)
        self.banner_row = box(False, 10)
        self.banner = box(False, 12, css="banner")
        self.banner_label = label()
        self.banner_label.set_line_wrap(True)
        self.banner.pack_start(self.banner_label, True, True, 0)
        self.force_button = button("Sofort beenden", css="danger", callback=self._on_force)
        self.banner.pack_start(self.force_button, False, False, 0)
        self.banner_row.pack_start(self.banner, True, True, 0)
        main.pack_start(self.banner_row, False, False, 0)
        self.stack.set_vexpand(True)
        main.pack_start(self.stack, True, True, 0)
        self._build_overview()
        self.input_page = InputPage(self.controller.submit_input)
        self.stack.add_named(self.input_page, "input")
        self._build_adapters()
        self._build_logs()
        footer = box(False)
        self.footer_status = label("Startbereit", "footer")
        self.uptime_label = label("Sitzung noch nicht gestartet", "footer")
        footer.pack_start(self.footer_status, True, True, 0)
        footer.pack_end(self.uptime_label, False, False, 0)
        main.pack_end(footer, False, False, 0)
        self.show_page("overview")

    def _build_overview(self):
        overview = box(spacing=20)
        metrics = box(False, 13)
        metrics.set_homogeneous(True)
        self.metrics = {}
        for key, title, value, subtitle in [("system", "SYSTEM", "Bereit", "Wartet auf den Start"), ("adapters", "ADAPTER", "—", "Konfiguration laden"), ("artifacts", "ARTEFAKTE", "—", "Persistierte Daten"), ("review", "ZUR PRÜFUNG", "—", "Aufgaben-Runtime")]:
            card = box(spacing=7, css="card")
            inner = box(spacing=7, margin=17)
            inner.pack_start(label(title, "metric-title"), False, False, 0)
            number = label(value, "metric-value" + (" green" if key == "system" else ""))
            sub = label(subtitle, "muted small")
            inner.pack_start(number, False, False, 0)
            inner.pack_start(sub, False, False, 0)
            card.add(inner)
            metrics.pack_start(card, True, True, 0)
            self.metrics[key] = (number, sub)
        overview.pack_start(metrics, False, False, 0)
        lower = box(False, 18)
        lower.set_vexpand(True)
        adapters_card = box(css="card")
        head = box(False, 10, margin=17)
        head.pack_start(label("Adapter", "card-title"), True, True, 0)
        self.adapter_badge = label("KONFIGURIERT", "badge")
        head.pack_end(self.adapter_badge, False, False, 0)
        adapters_card.pack_start(head, False, False, 0)
        self.overview_store, self.overview_tree = self._adapter_table(compact=True)
        adapters_card.pack_start(self._scroll(self.overview_tree), True, True, 0)
        lower.pack_start(adapters_card, True, True, 0)
        activity_card = box(css="card")
        activity_card.set_size_request(285, -1)
        activity_inner = box(spacing=9, margin=17)
        activity_inner.pack_start(label("Letzte Aktivität", "card-title"), False, False, 0)
        activity_inner.pack_start(label("Aus dem ARCS-Artefaktlog", "muted small"), False, False, 0)
        self.activity_box = box(css="activity")
        activity_inner.pack_start(self._scroll(self.activity_box), True, True, 0)
        activity_card.pack_start(activity_inner, True, True, 0)
        lower.pack_end(activity_card, False, False, 0)
        overview.pack_start(lower, True, True, 0)
        self.stack.add_named(overview, "overview")
        self._render_activity()

    def _build_adapters(self):
        page = box(spacing=13)
        tools = box(False, 12)
        tools.pack_start(label("Angeschlossene Komponenten", "card-title"), True, True, 0)
        self.search = Gtk.SearchEntry(placeholder_text="Adapter suchen")
        self.search.connect("search-changed", lambda _: self._render_catalog())
        tools.pack_start(self.search, False, False, 0)
        self.role_combo = Gtk.ComboBoxText()
        self.role_combo.append("all", "Alle Rollen")
        for role in ROLES:
            self.role_combo.append(role, ROLE_NAMES[role])
        self.role_combo.set_active(0)
        self.role_combo.connect("changed", lambda _: self._render_catalog())
        tools.pack_end(self.role_combo, False, False, 0)
        page.pack_start(tools, False, False, 0)
        pane = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        self.adapter_store, self.adapter_tree = self._adapter_table()
        pane.pack1(self._scroll(self.adapter_tree), True, False)
        self.details = Gtk.TextView(editable=False, cursor_visible=False, wrap_mode=Gtk.WrapMode.WORD_CHAR,
            left_margin=18, right_margin=18, top_margin=16, bottom_margin=16)
        self.details.set_size_request(285, -1)
        self.details.get_buffer().set_text("Adapter auswählen\n\nWähle eine Komponente aus der Liste, um ihre Rollen und Fähigkeiten zu sehen.")
        pane.pack2(self._scroll(self.details), True, False)
        pane.set_position(575)
        page.pack_start(pane, True, True, 0)
        page.pack_start(label("Fähigkeiten werden pro Installation und Betreiber-Grant ausgewiesen.", "muted small"), False, False, 0)
        self.stack.add_named(page, "adapters")

    def _build_logs(self):
        page = box(spacing=13)
        head = box(False, 10)
        head.pack_start(label("Sitzungsprotokoll", "card-title"), True, True, 0)
        head.pack_start(button("Exportieren", "document-save-symbolic", callback=self._export_logs), False, False, 0)
        head.pack_end(button("Leeren", css="flat", callback=self._clear_logs), False, False, 0)
        page.pack_start(head, False, False, 0)
        self.log_view = Gtk.TextView(editable=False, cursor_visible=False, monospace=True, wrap_mode=Gtk.WrapMode.WORD_CHAR,
            left_margin=17, right_margin=17, top_margin=17, bottom_margin=17)
        self.log_scroll = self._scroll(self.log_view)
        page.pack_start(self.log_scroll, True, True, 0)
        self.stack.add_named(page, "logs")

    def _scroll(self, child):
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        scroll.add(child)
        return scroll

    def _adapter_table(self, compact=False):
        store = Gtk.ListStore(str, str, str, str)
        tree = Gtk.TreeView(model=store, enable_search=False)
        tree.set_tooltip_column(0)
        tree.set_grid_lines(Gtk.TreeViewGridLines.NONE)
        for index, title in enumerate(["KOMPONENTE", "ROLLEN", "STATUS"]):
            if compact and index == 1:
                continue
            renderer = Gtk.CellRendererText(xpad=13, ypad=12, ellipsize=Pango.EllipsizeMode.END)
            column = Gtk.TreeViewColumn(title, renderer, text=index)
            if index == 0:
                column.set_expand(True)
                column.set_min_width(175)
            if index == 2:
                column.add_attribute(renderer, "foreground", 3)
                column.set_min_width(115)
            tree.append_column(column)
        tree.get_selection().connect("changed", self._on_adapter_selected)
        return store, tree

    def show_page(self, name):
        # Keep inactive native child windows hidden as well. This also makes
        # keyboard focus and GTK offscreen rendering follow the selected page.
        selected = self.stack.get_child_by_name(name)
        for child in self.stack.get_children():
            child.set_visible(child == selected)
        self.stack.set_visible_child_name(name)
        for key, item in self.nav_buttons.items():
            context = item.get_style_context()
            (context.add_class if key == name else context.remove_class)("active")

    def current_configuration(self):
        host = self.host_entry.get_text()
        if not host:
            raise ValueError("Bitte eine Host-Konfiguration auswählen.")
        runtime = self.runtime_entry.get_text()
        return Configuration(Path(host), Path(runtime) if runtime else None)

    def select_configuration(self, configuration):
        if self.controller.busy:
            return
        self._updating = True
        index = next((index for index, (_, config) in enumerate(self.presets) if config == configuration), len(self.presets))
        self.preset_combo.set_active(index)
        self.host_entry.set_text(str(configuration.host.expanduser().resolve()))
        self.runtime_entry.set_text(str(configuration.runtime.expanduser().resolve()) if configuration.runtime else "")
        self._updating = False
        self.catalog = []
        self._configuration_valid = False
        self.snapshot = {}
        self.recent = []
        self.input_page.reset()
        try:
            preview = read_configuration(configuration)
            self.catalog = preview["adapters"]
            self._configuration_valid = True
            self.metrics["adapters"][0].set_text(str(len(self.catalog)))
            self.metrics["adapters"][1].set_text("konfiguriert")
            self.metrics["artifacts"][0].set_text("—")
            self.metrics["review"][0].set_text("—")
            self.banner_row.hide()
            self._set_state("idle")
            self._save_settings(configuration)
        except (OSError, ValueError, TypeError) as error:
            self._show_error(str(error))
        self._render_catalog()
        self._render_activity()

    def _on_preset(self, combo):
        if not self._updating and 0 <= combo.get_active() < len(self.presets):
            self.select_configuration(self.presets[combo.get_active()][1])

    def _chooser(self, title, save=False):
        dialog = Gtk.FileChooserDialog(title=title, transient_for=self,
            action=Gtk.FileChooserAction.SAVE if save else Gtk.FileChooserAction.OPEN)
        dialog.add_buttons("Abbrechen", Gtk.ResponseType.CANCEL, "Speichern" if save else "Auswählen", Gtk.ResponseType.OK)
        if save:
            dialog.set_current_name("arcs-session.log")
            dialog.set_do_overwrite_confirmation(True)
        else:
            file_filter = Gtk.FileFilter()
            file_filter.set_name("JSON-Konfigurationen")
            file_filter.add_pattern("*.json")
            dialog.add_filter(file_filter)
            dialog.set_current_folder(str(self.root / "apps/adapter-host/config"))
        result = dialog.run()
        path = Path(dialog.get_filename()) if result == Gtk.ResponseType.OK else None
        dialog.destroy()
        return path

    def _choose_host(self, _):
        path = self._chooser("Host-Konfiguration auswählen")
        if path:
            self.select_configuration(Configuration(path))

    def _choose_runtime(self, _):
        path = self._chooser("Aufgaben-Runtime auswählen")
        if path:
            self.select_configuration(Configuration(Path(self.host_entry.get_text()), path))

    def _clear_runtime(self, _):
        self.select_configuration(Configuration(Path(self.host_entry.get_text())))

    def _on_start(self, _=None):
        try:
            configuration = self.current_configuration()
            self.controller.start(configuration)
            self.banner_row.hide()
            self._last_artifacts = None
            self._last_ready = None
            self._last_goals = {}
            self.input_page.reset()
            self.snapshot = {}
            self.metrics["artifacts"][0].set_text("—")
            self.metrics["review"][0].set_text("—")
            self.uptime_label.set_text("Sitzung startet …")
            self._set_state("starting")
            self._log("Start angefordert: " + configuration.host.name)
        except (OSError, ValueError, RuntimeError) as error:
            self._show_error(str(error))

    def _on_build(self, _):
        try:
            self.controller.build()
            self.banner_row.hide()
            self._set_state("building")
            self._log("Lokaler Build gestartet.")
        except RuntimeError as error:
            self._show_error(str(error))

    def _on_stop(self, _=None):
        if self.controller.busy:
            self.controller.request_stop()
            self._stopping_since = time.monotonic()
            self._set_state("stopping")
            self._show_banner("ARCS beendet die aktuelle Runde und schließt seine Adapter.")
            self._log("Geordnetes Beenden angefordert.")

    def _on_force(self, _):
        self.controller.force_stop()
        self.force_button.set_sensitive(False)
        self._log("Sitzung und zugehörige Kindprozesse werden sofort beendet.")

    def _set_state(self, state):
        self.state = state
        self.input_page.set_running(state == "running")
        text, sub = {
            "idle": ("Bereit", "Wartet auf den Start"), "building": ("Build", "Programme werden gebaut"),
            "starting": ("Startet", "Adapter werden verbunden"), "running": ("Läuft", "Verarbeitung ist aktiv"),
            "stopping": ("Stoppt", "Aktuelle Runde abschließen"), "error": ("Fehler", "Protokoll prüfen"),
        }[state]
        self.metrics["system"][0].set_text(text)
        self.metrics["system"][1].set_text(sub)
        self.side_status.set_text("●  " + text)
        self.footer_status.set_text(sub)
        busy = self.controller.busy
        for widget in [self.start_button, self.build_button, self.preset_combo, self.clear_runtime, *self.config_buttons]:
            widget.set_sensitive(not busy)
        self.start_button.set_sensitive(not busy and self._configuration_valid)
        self.stop_button.set_sensitive(busy and state != "stopping")
        if state != "stopping":
            self.force_button.hide()
            self.force_button.set_sensitive(True)
        self.adapter_badge.set_text("VERBUNDEN" if state in ["running", "stopping"] else "KONFIGURIERT")
        context = self.adapter_badge.get_style_context()
        (context.add_class if state in ["running", "stopping"] else context.remove_class)("live")
        self._render_catalog()

    def _render_catalog(self):
        if not hasattr(self, "adapter_store"):
            return
        query = self.search.get_text().casefold()
        selected_role = self.role_combo.get_active_id() or "all"
        self.overview_store.clear()
        self.adapter_store.clear()
        connected = self.state in ["running", "stopping"]
        for adapter in sorted(self.catalog, key=lambda item: item["adapter_id"]):
            roles = sorted({cap["role"] for cap in adapter.get("capabilities", [])}, key=lambda role: ROLES.index(role) if role in ROLES else 99)
            status = "Verbunden" if connected else "Konfiguriert"
            row = [adapter["adapter_id"], " · ".join(ROLE_NAMES.get(role, role) for role in roles) or "—", status, "#86dfc2" if connected else "#8795a9"]
            self.overview_store.append(row)
            if (selected_role == "all" or selected_role in roles) and query in (adapter["adapter_id"] + " " + " ".join(roles)).casefold():
                self.adapter_store.append(row)

    def _on_adapter_selected(self, selection):
        model, iterator = selection.get_selected()
        if iterator is None or not hasattr(self, "details"):
            return
        adapter = next((item for item in self.catalog if item["adapter_id"] == model[iterator][0]), None)
        if not adapter:
            return
        lines = [adapter["adapter_id"], "", "Installation", "Version: " + adapter.get("adapter_version", "nach dem Start verfügbar"), "Klasse: " + adapter.get("producer_class", "nach dem Start verfügbar"), "", "Fähigkeiten"]
        for cap in adapter.get("capabilities", []):
            lines += ["", cap["capability"]["capability_id"], "Rolle: " + ROLE_NAMES.get(cap["role"], cap["role"])]
            if cap.get("accepts"):
                lines += ["Eingaben: " + ", ".join(cap["accepts"])]
            lines += ["Ausgaben: " + ", ".join(cap.get("emits", []))]
            if cap.get("reasoning_methods"):
                lines += ["Verfahren: " + ", ".join(cap["reasoning_methods"])]
        if not adapter.get("capabilities"):
            lines += ["", "Das Manifest wird beim Start vom Adapter abgefragt."]
        self.details.get_buffer().set_text("\n".join(lines))

    def _render_activity(self):
        for child in self.activity_box.get_children():
            self.activity_box.remove(child)
        if not self.recent:
            empty = label("Nach dem Start erscheinen hier\ndie letzten gespeicherten Artefakte.", "empty")
            empty.set_line_wrap(True)
            self.activity_box.pack_start(empty, False, False, 0)
        for item in list(reversed(self.recent))[:6]:
            row = box(False, 10, css="activity-row")
            row.pack_start(label("•", "activity-dot"), False, False, 0)
            content = box(spacing=5)
            schema = item["schema_id"].removeprefix("arcs.")
            heading = label(schema, "activity-schema")
            heading.set_ellipsize(Pango.EllipsizeMode.END)
            heading.set_max_width_chars(28)
            heading.set_tooltip_text(item["version_id"])
            content.pack_start(heading, False, False, 0)
            source = label(item["producer_id"], "activity-source")
            source.set_ellipsize(Pango.EllipsizeMode.END)
            source.set_max_width_chars(28)
            content.pack_start(source, False, False, 0)
            row.pack_start(content, True, True, 0)
            self.activity_box.pack_start(row, False, False, 0)
        self.activity_box.show_all()

    def _poll_events(self):
        for _ in range(250):
            try:
                event = self.controller.events.get_nowait()
            except queue.Empty:
                break
            kind = event["event"]
            if kind == "state":
                if self.state != "stopping":
                    self._set_state(event["state"])
            elif kind == "process":
                self.footer_status.set_text(f"Lokaler Prozess · PID {event['pid']}")
            elif kind == "ready":
                self.catalog = event["catalog"]
                self.metrics["adapters"][0].set_text(str(len(self.catalog)))
                self.metrics["adapters"][1].set_text("angeschlossen")
                if self.state != "stopping":
                    self._set_state("running")
                self._log(f"{len(self.catalog)} Adapter verbunden. " + ("Aufgaben-Runtime aktiv." if event["has_runtime"] else "Beobachtungsbetrieb aktiv."))
            elif kind == "snapshot":
                self.snapshot = event
                self.metrics["artifacts"][0].set_text(f"{event['stored_artifacts']:,}".replace(",", "."))
                runtime = event.get("runtime")
                self.input_page.goal_progress(runtime.get("goals", []) if runtime else [])
                self.metrics["review"][0].set_text(str(runtime["ready_for_review"] + runtime.get("awaiting_approval", 0)) if runtime else "—")
                self.metrics["review"][1].set_text(f"{runtime['pending_jobs']} offen · {runtime['failed_jobs']} fehlgeschlagen" if runtime else "Keine Aufgaben-Runtime")
                if runtime and runtime.get("goals"):
                    self.metrics["review"][1].set_text(f"{runtime['completed_goals']} Ziele fertig · {runtime['awaiting_approval']} Freigaben offen")
                    for goal in runtime["goals"]:
                        key = (runtime["runtime_id"], goal["sequence"])
                        progress = (goal["state"], goal["round"])
                        if self._last_goals.get(key) != progress:
                            self._log(f"Ziel {goal['sequence']} · {goal['state']} · Schritt {goal['round']}" + (f" · {goal['detail']}" if goal['detail'] else ""))
                            self._last_goals[key] = progress
                seconds = int(event["uptime_ms"] / 1000)
                self.uptime_label.set_text(f"Laufzeit {seconds // 3600:02}:{seconds // 60 % 60:02}:{seconds % 60:02}  ·  Runde {event['ticks']}")
                if self._last_artifacts != event["stored_artifacts"]:
                    self._log(f"{event['stored_artifacts']} Artefakte gespeichert · World-Revision {event['world_revision']}.")
                    self._last_artifacts = event["stored_artifacts"]
                if runtime and self._last_ready != runtime["ready_for_review"]:
                    self._log(f"{runtime['ready_for_review']} Aufgaben zur Prüfung bereit.")
                    self._last_ready = runtime["ready_for_review"]
                self.recent = (self.recent + event.get("recent_artifacts", []))[-20:]
                self._render_activity()
            elif kind == "inputs":
                self.input_page.connect_targets(event["targets"], event["has_runtime"])
            elif kind in ["input_queued", "input_error"]:
                self.input_page.result(event)
                if kind == "input_queued":
                    self._log(f"Eingabe {event['id']} im Input-Adapter {event['adapter_id']} gespeichert.")
                else:
                    self._log(f"Eingabe {event['id']}: {event['message']}")
            elif kind == "log":
                self._log(event["message"])
            elif kind == "error":
                self._show_error(event["message"])
            elif kind == "finished":
                error = bool(event.get("message")) or (event["code"] != 0 and not event["requested"])
                self._set_state("error" if error else "idle")
                self.metrics["adapters"][1].set_text("konfiguriert")
                if not error:
                    self.banner_row.hide()
                    self._log("Build abgebrochen." if event["operation"] == "build" and event["requested"] else "Build abgeschlossen." if event["operation"] == "build" else "Sitzung sofort beendet." if event.get("forced") else "Sitzung beendet. Alle Adapter wurden geschlossen.")
                self._stopping_since = None
                if self._closing:
                    self.destroy()
                    return False
        if self._stopping_since and time.monotonic() - self._stopping_since > 8 and self.controller.busy:
            self.force_button.show_all()
        return True

    def _show_banner(self, message, error=False):
        self.banner_label.set_text(message)
        context = self.banner.get_style_context()
        (context.add_class if error else context.remove_class)("error")
        self.banner.show()
        self.banner_row.show()

    def _show_error(self, message):
        self._log("Fehler: " + message)
        self._show_banner(message, True)
        if not self.controller.busy:
            self._set_state("error")

    def _log(self, message):
        clock = datetime.now(ZoneInfo("Europe/Berlin")).strftime("%H:%M:%S")
        self.log_lines.append(f"{clock}  {message}")
        self.log_lines = self.log_lines[-1500:]
        buffer = self.log_view.get_buffer()
        buffer.set_text("\n".join(self.log_lines))
        mark = buffer.create_mark(None, buffer.get_end_iter(), False)
        self.log_view.scroll_mark_onscreen(mark)
        buffer.delete_mark(mark)

    def _clear_logs(self, _):
        self.log_lines.clear()
        self.log_view.get_buffer().set_text("")

    def _export_logs(self, _):
        path = self._chooser("Sitzungsprotokoll speichern", save=True)
        if path:
            try:
                path.write_text("\n".join(self.log_lines) + "\n", encoding="utf-8")
            except OSError as error:
                self._show_error(str(error))

    def _load_settings(self):
        if not self.persist_settings:
            return None
        try:
            values = json.loads((self.root / "var/control-panel/settings.json").read_text(encoding="utf-8"))
            configuration = Configuration(Path(values["host"]), Path(values["runtime"]) if values.get("runtime") else None)
            # Older versions saved the structured multi-adapter demo as their
            # default. Move that default to text tasks once, while preserving
            # custom configurations and subsequent deliberate selections.
            if values.get("settings_version", 0) == 0 and configuration == self.presets[1][1]:
                return self.presets[0][1]
            return configuration
        except (OSError, ValueError, KeyError, TypeError):
            return None

    def _save_settings(self, configuration):
        if not self.persist_settings:
            return
        try:
            path = self.root / "var/control-panel/settings.json"
            path.parent.mkdir(parents=True, exist_ok=True)
            temporary = path.with_suffix(".tmp")
            temporary.write_text(json.dumps({"settings_version": 1, "host": str(configuration.host), "runtime": str(configuration.runtime) if configuration.runtime else None}), encoding="utf-8")
            temporary.replace(path)
        except OSError as error:
            self._log("Auswahl konnte nicht gespeichert werden: " + str(error))

    def _on_close(self, *_):
        if self.controller.busy:
            self._closing = True
            self._on_stop()
            return True
        return False

    def _on_destroy(self, *_):
        if self.controller.busy:
            self.controller.force_stop()
        if self._source_id:
            GLib.source_remove(self._source_id)
            self._source_id = None


def launch(host: Path | None = None, runtime: Path | None = None) -> int:
    if not Gtk.init_check()[0]:
        print("Kein erreichbarer Desktop-Bildschirm. ARCS Control bitte in deiner grafischen Sitzung starten.", file=__import__("sys").stderr)
        return 1
    GLib.set_application_name("ARCS Control")
    GLib.set_prgname("arcs-control")
    application = Gtk.Application(application_id="org.arcs.Control", flags=Gio.ApplicationFlags.NON_UNIQUE)
    def activate(app):
        window = ControlWindow(app, Configuration(host, runtime) if host else None)
        window.present()
    application.connect("activate", activate)
    return application.run([])
