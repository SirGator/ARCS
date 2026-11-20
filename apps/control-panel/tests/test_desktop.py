import json
import sqlite3
import subprocess
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from arcs_panel.controller import Configuration,PROJECT_ROOT
from desktop_helpers import real_configuration,render_window

try:
    from arcs_panel.app import ControlWindow,Gtk,GLib
    DISPLAY=Gtk.init_check()[0]
except (ImportError,ValueError):
    DISPLAY=False


def pump_until(predicate,timeout=15):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        while Gtk.events_pending():
            Gtk.main_iteration_do(False)
        if predicate():
            return
        time.sleep(.025)
    raise AssertionError('Desktop condition timed out')


@unittest.skipUnless(DISPLAY,'Kein erreichbarer GTK-Desktop')
class DesktopTests(unittest.TestCase):
    def setUp(self):
        self.temporary=tempfile.TemporaryDirectory(prefix='arcs-desktop-')
        self.directory=Path(self.temporary.name)
        self.host=real_configuration(PROJECT_ROOT,self.directory)
        self.window=ControlWindow(configuration=Configuration(self.host),persist_settings=False)

    def tearDown(self):
        if self.window.controller.busy:
            self.window.controller.force_stop()
            pump_until(lambda:not self.window.controller.busy)
        self.window.destroy()
        self.temporary.cleanup()

    def test_real_start_filter_details_stop_and_restart(self):
        window=self.window
        window.show_page('overview')
        self.assertEqual(window.metrics['adapters'][0].get_text(),'11')
        self.assertFalse((self.directory/'state').exists())
        window.start_button.emit('clicked')
        pump_until(lambda:window.state=='running' and bool(window.snapshot))
        self.assertEqual(len(window.catalog),11)
        self.assertFalse(window.start_button.get_sensitive())
        self.assertFalse(window.preset_combo.get_sensitive())
        self.assertGreater(window.snapshot['stored_artifacts'],0)
        shots=PROJECT_ROOT/'var/control-panel/screenshots'
        pump_until(lambda:window.get_mapped())
        render_window(window,shots/'overview.png')
        window.show_page('adapters');window.role_combo.set_active_id('reason')
        pump_until(lambda:len(window.adapter_store)==2)
        window.adapter_tree.get_selection().select_path(0)
        buffer=window.details.get_buffer()
        self.assertIn('solution.propose',buffer.get_text(buffer.get_start_iter(),buffer.get_end_iter(),False))
        # Let the GTK stack transition and layout finish before rendering.
        pump_until(lambda:not window.stack.get_transition_running())
        render_window(window,shots/'adapters.png')
        window.search.set_text('solver')
        pump_until(lambda:len(window.adapter_store)==1)
        window.show_page('logs')
        pump_until(lambda:not window.stack.get_transition_running())
        self.assertTrue(any('verbunden' in line for line in window.log_lines))
        render_window(window,shots/'logs.png')
        count=window.snapshot['stored_artifacts']
        window.stop_button.emit('clicked')
        pump_until(lambda:not window.controller.busy and window.state=='idle')
        self.assertTrue(window.start_button.get_sensitive())
        window.start_button.emit('clicked')
        pump_until(lambda:window.state=='running' and window.snapshot.get('ticks',0)>0)
        self.assertEqual(window.snapshot['stored_artifacts'],count)
        # Closing the desktop requests a normal backend stop first.
        self.assertTrue(window._on_close())
        pump_until(lambda:not window.controller.busy)

    def test_error_is_visible_without_starting_a_host(self):
        self.window.select_configuration(Configuration(self.directory/'missing.json'))
        self.assertEqual(self.window.state,'error')
        self.assertTrue(self.window.banner.get_visible())
        self.assertFalse(self.window.start_button.get_sensitive())
        self.assertFalse(self.window.controller.busy)

    def test_legacy_default_migrates_to_text_without_overriding_later_or_custom_choices(self):
        window=self.window
        settings=self.directory/'var/control-panel/settings.json'
        settings.parent.mkdir(parents=True)
        structured=window.presets[1][1]
        text=window.presets[0][1]
        custom=Configuration(self.host)
        with patch.object(window,'root',self.directory), patch.object(window,'persist_settings',True):
            for version,configuration,expected in [(None,structured,text),(1,structured,structured),(None,custom,custom)]:
                with self.subTest(version=version,configuration=configuration):
                    values={'host':str(configuration.host),'runtime':None}
                    if version is not None:
                        values['settings_version']=version
                    settings.write_text(json.dumps(values))
                    self.assertEqual(window._load_settings(),expected)
            window._save_settings(text)
            self.assertEqual(json.loads(settings.read_text())['settings_version'],1)
            self.assertEqual(window._load_settings(),text)

    def test_runtime_review_counts_are_displayed_from_real_backend(self):
        runtime=json.loads((PROJECT_ROOT/'apps/adapter-host/config/roles/runtime.json').read_text())
        runtime['runtime_id']='desktop-test'
        runtime['journal']='state/runtime.sqlite'
        runtime['routes'][0]['source']['adapter_id']='multi.input.left'
        runtime['routes'][0]['reasoner']['adapter_id']='multi.reasoner.solver'
        path=self.directory/'runtime.json'
        path.write_text(json.dumps(runtime))
        self.window.select_configuration(Configuration(self.host,path))
        self.window.start_button.emit('clicked')
        pump_until(lambda:self.window.snapshot.get('runtime') is not None and self.window.snapshot['runtime']['ready_for_review']==1)
        self.assertEqual(self.window.metrics['review'][0].get_text(),'1')
        self.assertIn('0 offen',self.window.metrics['review'][1].get_text())
        self.window.stop_button.emit('clicked')
        pump_until(lambda:not self.window.controller.busy and self.window.state=='idle')

    def test_scheduled_tasks_are_visible_and_still_wait_after_restart(self):
        runtime=json.loads((PROJECT_ROOT/'apps/adapter-host/config/roles/runtime.json').read_text())
        runtime.update(runtime_id='desktop-scheduled',journal='state/scheduled.sqlite')
        runtime['routes'][0]['source']['adapter_id']='multi.input.left'
        runtime['routes'][0]['reasoner']['adapter_id']='multi.reasoner.solver'
        runtime['routes'][0]['activation']={'kind':'at_time','unix_ms':4102444800000}
        path=self.directory/'scheduled.json'
        path.write_text(json.dumps(runtime))
        self.window.select_configuration(Configuration(self.host,path))
        for _ in range(2):
            self.window.start_button.emit('clicked')
            pump_until(lambda:self.window.state=='running' and self.window.snapshot.get('runtime',{}).get('waiting_time')==1)
            self.assertEqual(self.window.metrics['review'][0].get_text(),'0')
            self.assertIn('1 warten auf Termin/Ereignis',self.window.metrics['review'][1].get_text())
            self.window.stop_button.emit('clicked')
            pump_until(lambda:not self.window.controller.busy and self.window.state=='idle')

    def test_goal_loop_completion_and_progress_are_displayed_from_real_backend(self):
        host=real_configuration(PROJECT_ROOT,self.directory,'goals')
        runtime=json.loads((self.directory/'runtime.json').read_text())
        runtime['journal']='state/goal-runtime.sqlite'
        runtime['interval_ms']=10
        path=self.directory/'runtime.json'
        path.write_text(json.dumps(runtime))
        self.window.select_configuration(Configuration(host,path))
        self.window.start_button.emit('clicked')
        pump_until(lambda:self.window.snapshot.get('runtime') is not None
                   and self.window.snapshot['runtime']['completed_goals']==1, timeout=30)
        report=self.window.snapshot['runtime']
        self.assertEqual(report['goals'][0]['state'],'completed')
        self.assertEqual(report['goals'][0]['round'],2)
        self.assertEqual(self.window.metrics['review'][0].get_text(),'0')
        self.assertIn('1 Ziele fertig',self.window.metrics['review'][1].get_text())
        self.assertTrue(any('completed' in line for line in self.window.log_lines))
        self.window.show_page('input')
        page=self.window.input_page
        pump_until(lambda:len(page.targets)==1)
        page.fields['minimum_actuations'][1].set_text('4')
        page.send_button.emit('clicked')
        pump_until(lambda:not page.pending and bool(page.history))
        pump_until(lambda:self.window.snapshot['runtime']['completed_goals']==2, timeout=30)
        self.assertIn('2 Ziele fertig',self.window.metrics['review'][1].get_text())
        render_window(self.window,PROJECT_ROOT/'var/control-panel/screenshots/input-form.png')
        self.window.stop_button.emit('clicked')
        pump_until(lambda:not self.window.controller.busy and self.window.state=='idle')

    def test_series_controls_keep_the_same_running_host_and_persist_after_restart(self):
        runtime = json.loads((PROJECT_ROOT / 'apps/adapter-host/config/roles/runtime.json').read_text())
        runtime.update(runtime_id='desktop-recurrence', journal='state/recurring.sqlite', interval_ms=25)
        route = runtime['routes'][0]
        route['source']['adapter_id'] = 'multi.input.left'
        route['reasoner']['adapter_id'] = 'multi.reasoner.solver'
        route['recurrence'] = {'every_ms': 3600000, 'max_occurrences': 5}
        path = self.directory / 'recurring.json'
        path.write_text(json.dumps(runtime))
        window = self.window
        window.select_configuration(Configuration(self.host, path))
        window.show_page('control')
        page = window.control_page

        def series_state():
            rows = (window.snapshot.get('runtime') or {}).get('recurring_checks', [])
            return rows[0]['state'] if rows else None

        window.start_button.emit('clicked')
        pump_until(lambda: page.connected and series_state() == 'active' and bool(page.records))
        control = window.controller._process.pid
        page.reason.set_text('Wartungsfenster')
        self.assertTrue(page.pause_button.get_sensitive())
        page.pause_button.emit('clicked')
        pump_until(lambda: series_state() == 'paused' and page.pending is None)
        self.assertEqual(page.pause_button.get_label(), 'Fortsetzen')
        self.assertEqual(window.controller._process.pid, control)
        render_window(window, PROJECT_ROOT / 'var/control-panel/screenshots/live-series.png')
        window.stop_button.emit('clicked')
        pump_until(lambda: not window.controller.busy and window.state == 'idle')
        window.start_button.emit('clicked')
        pump_until(lambda: page.connected and series_state() == 'paused' and bool(page.records))
        page.reason.set_text('Wartung abgeschlossen')
        page.pause_button.emit('clicked')
        pump_until(lambda: series_state() == 'active' and page.pending is None)
        page.reason.set_text('Prüfserie abgeschlossen')
        page.cancel_button.emit('clicked')
        pump_until(lambda: series_state() == 'cancelled' and page.pending is None)
        self.assertFalse(page.cancel_button.get_visible())
        self.assertEqual(window.snapshot['runtime']['recurring_checks'][0]['control_revision'], 3)
        self.assertEqual(window.snapshot['runtime']['ready_for_review'], 1)

    def test_goal_review_panel_displays_exact_parameters_before_approval(self):
        host = real_configuration(PROJECT_ROOT, self.directory, 'goals')
        path = self.directory / 'runtime.json'
        runtime = json.loads(path.read_text())
        runtime.update(journal='state/manual-runtime.sqlite', interval_ms=15)
        runtime['routes'][0]['goal']['approval'] = {'mode': 'manual'}
        path.write_text(json.dumps(runtime))
        source = self.directory / 'input-profile.json'
        inputs = json.loads(source.read_text())
        inputs['events'][0]['payload'] = {'device_id': 'fan-01', 'enabled': True, 'minimum_actuations': 1}
        source.write_text(json.dumps(inputs))
        window = self.window
        window.select_configuration(Configuration(host, path))
        window.show_page('control')
        window.start_button.emit('clicked')
        page = window.control_page
        pump_until(lambda: page.inspect_button.get_sensitive())

        def actuations():
            with sqlite3.connect(f"file:{self.directory / 'state/fan.sqlite'}?mode=ro", uri=True) as db:
                return db.execute('SELECT actuation_count FROM fan_state').fetchone()[0]

        self.assertEqual(actuations(), 0)
        page.inspect_button.emit('clicked')
        pump_until(lambda: page.review is not None)
        dialog, expected, entry = page.review
        self.assertFalse(dialog.get_widget_for_response(Gtk.ResponseType.ACCEPT).get_sensitive())
        view = next(widget.get_child() for widget in dialog.get_content_area().get_children() if isinstance(widget, Gtk.ScrolledWindow))
        buffer = view.get_buffer()
        self.assertEqual(json.loads(buffer.get_text(buffer.get_start_iter(), buffer.get_end_iter(), True)),
                         {'device_id': 'fan-01', 'enabled': True})
        self.assertEqual(expected[1], window.snapshot['runtime']['goals'][0]['candidate'])
        self.assertEqual(actuations(), 0)
        render_window(dialog, PROJECT_ROOT / 'var/control-panel/screenshots/goal-review.png')
        dialog.response(Gtk.ResponseType.CANCEL)
        self.assertIsNone(page.review)
        self.assertEqual(actuations(), 0)
        page.inspect_button.emit('clicked')
        pump_until(lambda: page.review is not None)
        dialog, _, entry = page.review
        entry.set_text('Testfreigabe für den lokalen Lüftersimulator')
        self.assertTrue(dialog.get_widget_for_response(Gtk.ResponseType.ACCEPT).get_sensitive())
        dialog.response(Gtk.ResponseType.ACCEPT)
        pump_until(lambda: window.snapshot['runtime']['completed_goals'] == 1)
        self.assertEqual(actuations(), 1)

    def test_plan_root_pause_and_cancel_use_the_running_scheduler(self):
        host = real_configuration(PROJECT_ROOT, self.directory, 'dynamic-plans')
        source = PROJECT_ROOT / 'apps/adapter-host/config/dynamic-plans'
        host_config = json.loads(host.read_text())
        # References to other example profiles stay in the repository.
        for adapter in host_config['adapters']:
            for field in ('profile', 'grant_file'):
                if adapter[field].startswith('../'):
                    adapter[field] = str((source / adapter[field]).resolve())
        host.write_text(json.dumps(host_config))
        path = self.directory / 'runtime.json'
        runtime = json.loads(path.read_text())
        runtime.update(journal='state/plan-runtime.sqlite', interval_ms=20)
        for template in runtime['routes'][0]['plan']['templates']:
            template['route']['activation'] = {'kind': 'at_time', 'unix_ms': 4102444800000}
        path.write_text(json.dumps(runtime))

        window = self.window
        window.select_configuration(Configuration(host, path))
        window.show_page('control')
        window.start_button.emit('clicked')
        page = window.control_page
        pump_until(lambda: bool(page.records.get('plan:1', (None, {}))[1].get('steps')))
        page.reason.set_text('Untersuchung pausieren')
        page.pause_button.emit('clicked')
        pump_until(lambda: window.snapshot['runtime']['paused_jobs'] > 1)
        window.show_page('tasks')
        queue = window.queue_page
        pump_until(lambda: bool(queue.page and len(queue.tasks) > 1))
        inherited = [job for job in queue.tasks.values() if job['parent_plan_sequence'] == 1]
        self.assertTrue(inherited)
        self.assertTrue(all(job['paused'] and not job['locally_paused'] for job in inherited))
        window.show_page('control')
        page.reason.set_text('Untersuchung abbrechen')
        page.cancel_button.emit('clicked')
        pump_until(lambda: page.records['plan:1'][1]['state'] == 'cancelled')
        self.assertTrue(all(step['state'] == 'cancelled' for step in page.records['plan:1'][1]['steps']))
        self.assertFalse(page.cancel_button.get_visible())

    def test_resource_review_displays_evidence_and_releases_the_simulated_reservation(self):
        host = real_configuration(PROJECT_ROOT, self.directory, 'goals')
        path = self.directory / 'runtime.json'
        runtime = json.loads(path.read_text())
        runtime.update(journal='state/resource-runtime.sqlite', interval_ms=1000)
        goal_config = runtime['routes'][0]['goal']
        goal_config['approval'] = {'mode': 'manual'}
        goal_config['exclusive_resources'] = ['device.fan-01']
        path.write_text(json.dumps(runtime))
        window = self.window
        page = window.control_page
        window.select_configuration(Configuration(host, path))
        window.show_page('control')
        window.start_button.emit('clicked')
        pump_until(lambda: page.inspect_button.get_sensitive())
        self.assertNotIn('resource:1', page.records)
        page.inspect_button.emit('clicked')
        pump_until(lambda: page.review is not None)
        dialog, _, reason = page.review
        reason.set_text('Simulierten Lüfterauftrag prüfen und freigeben')
        dialog.response(Gtk.ResponseType.ACCEPT)
        pump_until(lambda: page.records['goal:1'][1]['dispatch_cursor'] is not None)
        page.reason.set_text('Simulierten Auftrag nach Auslösung beenden')
        page.cancel_button.emit('clicked')
        pump_until(lambda: 'resource:1' in page.records and page.pending is None
                   and page.records['resource:1'][1]['execution'] is not None)

        page.selection.set_active_id('resource:1')
        self.assertIn('device.fan-01', page.detail.get_text())
        page.resource_button.emit('clicked')
        review = page.resource_review
        accept = review.get_widget_for_response(Gtk.ResponseType.ACCEPT)
        evidence = review.record['execution']
        reservation = review.versions[0]
        review.reason.set_text('Simulierten Gerätezustand geprüft')
        review.evidence.get_buffer().set_text(evidence)
        self.assertFalse(accept.get_sensitive())
        review.inspect.emit('clicked')
        pump_until(lambda: review.inspected == (evidence,))
        preview = review.preview.get_buffer()
        artifacts = json.loads(preview.get_text(preview.get_start_iter(), preview.get_end_iter(), True))
        self.assertEqual(artifacts[0]['version_id'], evidence)
        self.assertTrue(artifacts[0]['payload'])
        self.assertTrue(accept.get_sensitive())
        review.evidence.get_buffer().set_text('missing-evidence')
        self.assertFalse(accept.get_sensitive())
        review.inspect.emit('clicked')
        pump_until(lambda: review.pending is None)
        self.assertFalse(accept.get_sensitive())
        review.evidence.get_buffer().set_text(evidence)
        review.inspect.emit('clicked')
        pump_until(lambda: accept.get_sensitive())
        render_window(review, PROJECT_ROOT / 'var/control-panel/screenshots/resource-review.png')
        review.response(Gtk.ResponseType.ACCEPT)
        pump_until(lambda: 'resource:1' not in page.records and page.pending is None)
        released = window.snapshot['resource_reservations'][0]
        self.assertEqual(released['state'], 'released')
        self.assertNotEqual(released['version_id'], reservation)
        self.assertEqual(released['evidence'], [evidence])
        window.stop_button.emit('clicked')
        pump_until(lambda: not window.controller.busy and window.state == 'idle')
        window.start_button.emit('clicked')
        pump_until(lambda: window.snapshot.get('resource_reservations'))
        self.assertEqual(window.snapshot['resource_reservations'][0], released)
        self.assertNotIn('resource:1', page.records)
        with sqlite3.connect(self.directory / 'state/fan.sqlite') as db:
            self.assertEqual(db.execute('SELECT actuation_count FROM fan_state').fetchone()[0], 1)

    def test_queue_pages_and_controls_a_scheduled_reasoning_task(self):
        runtime = json.loads((PROJECT_ROOT / 'apps/adapter-host/config/roles/runtime.json').read_text())
        runtime.update(runtime_id='desktop-task-queue', journal='state/task-queue.sqlite', interval_ms=20, max_pending_jobs=30)
        template = runtime['routes'][0]
        template['source']['adapter_id'] = 'multi.input.left'
        template['reasoner']['adapter_id'] = 'multi.reasoner.solver'
        template['activation'] = {'kind': 'at_time', 'unix_ms': 4102444800000}
        runtime['routes'] = [{**template, 'id': f'waiting-{index}'} for index in range(26)]
        path = self.directory / 'task-queue.json'
        path.write_text(json.dumps(runtime))
        profile_path = self.directory / 'input-left.json'
        profile = json.loads(profile_path.read_text())
        event_path = self.directory / 'late-queue-input.json'
        event_path.write_text(json.dumps(profile['events'][0]))
        profile['events'] = []
        profile_path.write_text(json.dumps(profile))

        window = self.window
        window.select_configuration(Configuration(self.host, path))
        window.show_page('tasks')
        window.start_button.emit('clicked')
        page = window.queue_page
        pump_until(lambda: page.page is not None and page.page['through_sequence'] == 0)
        self.assertFalse(page.tasks)
        result = subprocess.run(
            [PROJECT_ROOT / 'target/debug/arcs-json-input', '--enqueue', profile_path,
             self.directory / 'state/input-left.sqlite', event_path],
            capture_output=True, text=True, timeout=15,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        pump_until(lambda: bool(page.page and len(page.tasks) == 25))
        self.assertEqual(page.page['through_sequence'], 26)
        self.assertEqual(page.page_number.get_text(), 'Seite 1')
        self.assertEqual(page.rows[0][2], 'Wartet auf Termin')
        self.assertTrue(page.next_button.get_sensitive())
        page.next_button.emit('clicked')
        pump_until(lambda: set(page.tasks) == {26})
        self.assertEqual(page.page_number.get_text(), 'Seite 2')
        self.assertFalse(page.next_button.get_sensitive())
        page.tree.get_selection().select_path(0)
        page.reason.set_text('Testaufgabe pausieren')
        page.pause_button.emit('clicked')
        pump_until(lambda: page.tasks[26]['locally_paused'] and page.pending is None)
        self.assertEqual(page.pause_button.get_label(), 'Fortsetzen')
        self.assertEqual(page.rows[0][2], 'Pausiert')
        self.assertIn('Frühestens', page.detail.get_text())
        render_window(window, PROJECT_ROOT / 'var/control-panel/screenshots/task-queue.png')
        page.previous_button.emit('clicked')
        pump_until(lambda: len(page.tasks) == 25 and page.pending is None)
        self.assertEqual(page.page_number.get_text(), 'Seite 1')
        page.next_button.emit('clicked')
        pump_until(lambda: set(page.tasks) == {26} and page.pending is None)
        self.assertEqual(page.tasks[26]['control_revision'], 1)
        page.tree.get_selection().select_path(0)
        page.reason.set_text('Testaufgabe fortsetzen')
        page.pause_button.emit('clicked')
        pump_until(lambda: page.tasks[26]['control_revision'] == 2 and page.pending is None)
        self.assertFalse(page.tasks[26]['paused'])
        self.assertTrue(page.cancel_button.get_visible())
        self.assertFalse(page.cancel_button.get_sensitive())
        page.reason.set_text('Geplante Aufgabe abbrechen')
        self.assertTrue(page.cancel_button.get_sensitive())
        page.cancel_button.emit('clicked')
        pump_until(lambda: not page.tasks and page.pending is None)
        page.scope.set_active_id('all')
        pump_until(lambda: page.page and page.page['scope'] == 'all' and page.pending is None)
        self.assertEqual(page.page_number.get_text(), 'Seite 1')
        page.next_button.emit('clicked')
        pump_until(lambda: set(page.tasks) == {26} and page.pending is None)
        self.assertEqual(page.tasks[26]['control_revision'], 3)
        self.assertEqual(page.rows[0][2], 'Abgebrochen')
        page.tree.get_selection().select_path(0)
        self.assertFalse(page.reason.get_sensitive())
        self.assertFalse(page.cancel_button.get_sensitive())
        self.assertEqual(window.snapshot['runtime']['ready_for_review'], 0)

    def test_text_input_and_json_modes_send_through_the_real_adapter_and_report_invalid_payloads(self):
        host=real_configuration(PROJECT_ROOT,self.directory,'text-input')
        self.window.select_configuration(Configuration(host))
        self.window.show_page('input')
        page=self.window.input_page
        self.assertFalse(page.send_button.get_sensitive())
        self.window.start_button.emit('clicked')
        pump_until(lambda:len(page.targets)==1 and page.send_button.get_sensitive())
        text='Finde das rote Ziel.\nNutze die angeschlossene Kamera.'
        page.fields['raw_text'][1].get_buffer().set_text(text)
        page.mode_combo.set_active_id('json')
        buffer=page.json_editor.get_buffer()
        self.assertEqual(json.loads(buffer.get_text(buffer.get_start_iter(),buffer.get_end_iter(),False)),{'raw_text':text})
        buffer.set_text('{"raw_text": 42}')
        page.send_button.emit('clicked')
        pump_until(lambda:not page.pending and 'Adaptervertrag' in page.status.get_text())
        self.assertEqual(self.window.state,'running')
        buffer.set_text(json.dumps({'raw_text':text},ensure_ascii=False))
        page.mode_combo.set_active_id('form')
        self.assertEqual(page.mode_combo.get_active_id(),'form')
        page.send_button.emit('clicked')
        pump_until(lambda:not page.pending and 'Im Input-Adapter gespeichert' in page.status.get_text())
        def received():
            with sqlite3.connect(self.directory/'state/core.sqlite') as db:
                artifacts=[json.loads(row[0]) for row in db.execute('SELECT artifact_json FROM artifact_versions')]
            return [a for a in artifacts if a['payload']=={'raw_text':text}]
        pump_until(lambda:bool(received()))
        self.assertEqual(received()[0]['created_by']['id'],'panel.text')
        self.assertEqual(len(received()),1)
        render_window(self.window,PROJECT_ROOT/'var/control-panel/screenshots/input-text.png')
        self.window.stop_button.emit('clicked')
        pump_until(lambda:not self.window.controller.busy and self.window.state=='idle')
        self.assertFalse(page.send_button.get_sensitive())


    def test_primary_text_tasks_are_reasoned_executed_and_reported_in_the_same_text_page(self):
        host=real_configuration(PROJECT_ROOT,self.directory,'text-tasks')
        runtime=json.loads((self.directory/'runtime.json').read_text())
        runtime['journal']='state/text-runtime.sqlite'
        runtime['interval_ms']=10
        path=self.directory/'runtime.json';path.write_text(json.dumps(runtime))
        self.window.select_configuration(Configuration(host,path))
        self.window.show_page('input')
        self.assertIn('Text',self.window.presets[0][0])
        page=self.window.input_page
        self.window.start_button.emit('clicked')
        pump_until(lambda:len(page.targets)==1 and page.send_button.get_sensitive())
        self.assertEqual(set(page.fields),{'raw_text'})
        for text,completed,blocked in [('Schalte den Lüfter ein.',1,0),('Schalte den Lüfter ein.',2,0),('Finde die rote Kiste mit der Drohne.',2,1),('Schalte den Lüfter aus.',3,1)]:
            page.fields['raw_text'][1].get_buffer().set_text(text)
            page.send_button.emit('clicked')
            pump_until(lambda:not page.pending)
            pump_until(lambda:self.window.snapshot.get('runtime') is not None
                       and self.window.snapshot['runtime']['completed_goals']==completed
                       and self.window.snapshot['runtime']['blocked_goals']==blocked, timeout=30)
            if 'Drohne' in text:
                self.assertIn('versteht diese Aufgabe nicht',page.status.get_text())
            else:
                self.assertIn('Ziel erreicht',page.status.get_text())
        with sqlite3.connect(self.directory/'state/fan.sqlite') as db:
            self.assertEqual(db.execute('SELECT enabled,actuation_count FROM fan_state').fetchone(),(0,2))
        with sqlite3.connect(self.directory/'state/core.sqlite') as db:
            self.assertEqual(db.execute("SELECT COUNT(*) FROM artifact_versions WHERE schema_id='arcs.action.v1'").fetchone()[0],2)
        render_window(self.window,PROJECT_ROOT/'var/control-panel/screenshots/text-tasks.png')
        self.window.stop_button.emit('clicked')
        pump_until(lambda:not self.window.controller.busy and self.window.state=='idle')

if __name__=='__main__':
    unittest.main()
