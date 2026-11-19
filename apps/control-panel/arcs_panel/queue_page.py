"""Pages of operational task metadata from the live Rust host."""
from datetime import datetime

from gi.repository import Gtk, Pango

from .control_widgets import STATE_NAMES, local_operator, reason_text, revision_command, text_label


WAIT_LABELS = {
    "time": "Wartet auf Termin", "event": "Wartet auf Ereignis",
    "dependency": "Wartet auf Voraussetzungen", "approval": "Wartet auf Freigabe",
}


def state_text(task):
    if not task["can_control"]:
        return STATE_NAMES.get(task["state"], task["state"])
    if task["paused"]:
        return "Pausiert" if task["locally_paused"] else "Durch Plan oder Serie pausiert"
    state = task.get("goal_state") or task["state"]
    return WAIT_LABELS.get(task.get("waiting_for"), STATE_NAMES.get(state, state))


def local_time(unix_ms):
    try:
        return datetime.fromtimestamp(unix_ms / 1000).strftime("%d.%m.%Y, %H:%M")
    except (ValueError, OSError, OverflowError):
        return "außerhalb des darstellbaren Zeitraums"


class QueuePage(Gtk.Box):
    def __init__(self, submit):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.submit = submit
        self.operator = local_operator()
        self.page_size = None
        self.cancel_available = False
        self.page = None
        self.cursors = (0,)
        self.pending = None
        self.initial_requested = False
        self.tasks = {}

        top = Gtk.Box(spacing=8)
        top.pack_start(text_label("Aufgabenwarteschlange", "card-title"), True, True, 0)
        self.scope = Gtk.ComboBoxText()
        self.scope.append("open", "Offene Aufgaben")
        self.scope.append("all", "Alle Aufgaben")
        self.scope.set_active_id("open")
        self.scope.connect("changed", self._reload)
        top.pack_start(self.scope, False, False, 0)
        self.reload_button = Gtk.Button(label="Aktualisieren")
        self.reload_button.connect("clicked", self._reload)
        top.pack_start(self.reload_button, False, False, 0)
        self.pack_start(top, False, False, 0)

        self.rows = Gtk.ListStore(str, str, str, str)
        self.tree = Gtk.TreeView(model=self.rows)
        for index, name in enumerate(("Aufgabe", "Route", "Zustand", "Priorität")):
            renderer = Gtk.CellRendererText(ellipsize=Pango.EllipsizeMode.END)
            column = Gtk.TreeViewColumn(name, renderer, text=index)
            column.set_expand(index == 1)
            column.set_resizable(index in (1, 2))
            if index == 2:
                column.set_min_width(190)
            self.tree.append_column(column)
        self.tree.get_selection().connect("changed", self._selected_changed)
        viewport = Gtk.ScrolledWindow(vexpand=True, min_content_height=100)
        viewport.add(self.tree)
        self.pack_start(viewport, True, True, 0)

        nav = Gtk.Box(spacing=8)
        self.previous_button = Gtk.Button(label="Zurück")
        self.previous_button.connect("clicked", self._previous)
        self.next_button = Gtk.Button(label="Weiter")
        self.next_button.connect("clicked", self._next)
        self.page_number = text_label(css="muted")
        nav.pack_start(self.previous_button, False, False, 0)
        nav.pack_start(self.next_button, False, False, 0)
        nav.pack_start(self.page_number, False, False, 0)
        self.pack_start(nav, False, False, 0)

        self.detail = text_label(css="muted")
        self.detail.set_selectable(True)
        self.pack_start(self.detail, False, False, 0)
        actions = Gtk.Box(spacing=8)
        self.reason = Gtk.Entry(placeholder_text="Begründung für den Befehl")
        self.reason.connect("changed", lambda *_: self._refresh())
        self.pause_button = Gtk.Button(label="Pausieren")
        self.pause_button.connect("clicked", self._change_task)
        self.cancel_button = Gtk.Button(label="Abbrechen", no_show_all=True)
        self.cancel_button.get_style_context().add_class("danger")
        self.cancel_button.set_tooltip_text(
            "Ein Plan-Hauptauftrag wird samt offenen Schritten beendet. "
            "Bei einer Prüfserie endet der ausgewählte Durchlauf."
        )
        self.cancel_button.connect("clicked", self._change_task, True)
        actions.pack_start(self.reason, True, True, 0)
        actions.pack_start(self.pause_button, False, False, 0)
        actions.pack_start(self.cancel_button, False, False, 0)
        self.pack_start(actions, False, False, 0)
        self.status = text_label(css="muted")
        self.status.set_selectable(True)
        self.pack_start(self.status, False, False, 0)
        self._refresh()

    def set_connected(self, max_page_size, cancel_available=False):
        self.page_size = min(max_page_size, 25) if max_page_size else None
        self.cancel_available = bool(self.page_size and cancel_available)
        self._refresh()

    def reset(self):
        self.page_size = None
        self.cancel_available = False
        self.page = None
        self.cursors = (0,)
        self.pending = None
        self.initial_requested = False
        self.tasks.clear()
        self.rows.clear()
        self.reason.set_text("")
        self.status.set_text("")
        self._refresh()

    def load_if_needed(self):
        if self.page_size and not self.initial_requested:
            self._reload()

    def _reload(self, *_):
        if self.page and self.page["scope"] != self.scope.get_active_id():
            self.page = None
            self.tasks.clear()
            self.rows.clear()
        self._load((0,), None)

    def _load(self, cursors, through_sequence):
        if not self.page_size or self.pending:
            return
        self.initial_requested = True
        command = {"scope": self.scope.get_active_id(), "limit": self.page_size,
                   "after_sequence": cursors[-1], "through_sequence": through_sequence}
        self._send("queue", command, cursors)

    def _previous(self, *_):
        if self.previous_button.get_sensitive():
            self._load(self.cursors[:-1], self.page["through_sequence"])

    def _next(self, *_):
        if self.next_button.get_sensitive():
            self._load((*self.cursors, self.page["next_after_sequence"]), self.page["through_sequence"])

    def _selected(self):
        model, row = self.tree.get_selection().get_selected()
        return self.tasks.get(int(model[row][0])) if row is not None else None

    def _selected_changed(self, *_):
        self.reason.set_text("")
        self._refresh()

    def _change_task(self, button, cancel=False):
        if not button.get_sensitive():
            return
        task = self._selected()
        reason = reason_text(self.reason)
        if task is None or not reason:
            return
        change = "resume" if task["locally_paused"] else "pause"
        if cancel:
            change = "cancel"
        command = revision_command(self.operator, task["sequence"], task["control_revision"], reason, {"command": change})
        self._send("task", command, None)

    def _send(self, kind, command, cursors):
        try:
            self.pending = self.submit(kind, command), cursors
        except (OSError, RuntimeError, ValueError) as error:
            self.status.set_text(str(error))
        else:
            self.status.set_text("Lade Aufgaben …" if kind == "queue" else "Warte auf Rückmeldung von ARCS …")
        self._refresh()

    def result(self, event):
        if self.pending is None or event.get("request_id") != self.pending[0]:
            return
        _, cursors = self.pending
        self.pending = None
        if not event.get("ok"):
            self.status.set_text(event.get("error", {}).get("message", "ARCS hat die Anfrage abgewiesen."))
        elif cursors is None:
            self.reason.set_text("")
            if self.page_size:
                self._load(self.cursors, self.page["through_sequence"])
            else:
                self.status.set_text("Befehl bestätigt.")
        else:
            self._show_page(event["receipt"], cursors)
        self._refresh()

    def _show_page(self, page, cursors):
        selected = self._selected()
        self.page, self.cursors = page, cursors
        self.tasks = {task["sequence"]: task for task in page["tasks"]}
        self.rows.clear()
        for task in page["tasks"]:
            row = self.rows.append((str(task["sequence"]), task["route_id"], state_text(task), str(task["priority"])))
            if selected and selected["sequence"] == task["sequence"]:
                self.tree.get_selection().select_iter(row)
        self.status.set_text("" if page["tasks"] else "Auf dieser Seite sind keine Aufgaben verfügbar.")

    def _refresh(self):
        task = self._selected()
        ready = bool(self.page_size and self.pending is None)
        matches_scope = self.page and self.page["scope"] == self.scope.get_active_id()
        self.scope.set_sensitive(ready)
        self.reload_button.set_sensitive(ready)
        self.previous_button.set_sensitive(bool(ready and matches_scope and len(self.cursors) > 1))
        self.next_button.set_sensitive(bool(ready and matches_scope and self.page["next_after_sequence"] is not None))
        self.page_number.set_text(f"Seite {len(self.cursors)}" if matches_scope else "")
        recover = bool(task and task["locally_paused"] and task["action_recovery_required"])
        adjustable = bool(ready and matches_scope and task and task["can_control"])
        can_pause = adjustable and not recover
        can_cancel = adjustable and self.cancel_available
        has_reason = reason_text(self.reason) is not None
        self.reason.set_sensitive(can_pause or can_cancel)
        self.pause_button.set_label("Fortsetzen" if task and task["locally_paused"] else "Pausieren")
        self.pause_button.set_sensitive(can_pause and has_reason)
        self.cancel_button.set_visible(self.cancel_available)
        self.cancel_button.set_sensitive(can_cancel and has_reason)
        if not self.page_size:
            detail = "Starte einen Host mit Aufgaben-Runtime und Unterstützung für die Warteschlange."
        elif not task:
            detail = "Wähle eine Aufgabe aus."
        else:
            lines = [state_text(task), f"Eingabe {task['input_version_id']}"]
            if task.get("not_before_unix_ms") is not None:
                lines.append("Frühestens " + local_time(task["not_before_unix_ms"]))
            if task.get("deadline_unix_ms") is not None:
                lines.append("Frist " + local_time(task["deadline_unix_ms"]))
            if task.get("parent_plan_sequence") is not None:
                lines.append(f"Plan {task['parent_plan_sequence']}")
            if task.get("series_sequence") is not None:
                lines.append(f"Prüfserie {task['series_sequence']}")
            if recover:
                lines.append("Die Fortsetzung erfordert die ausdrückliche Wiederaufnahme des Aktorauftrags über die CLI.")
            detail = " · ".join(lines)
        self.detail.set_text(detail)
