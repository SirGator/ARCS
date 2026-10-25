import json
import sqlite3
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

    def test_goal_loop_completion_and_progress_are_displayed_from_real_backend(self):
        host=real_configuration(PROJECT_ROOT,self.directory,'goals')
        runtime=json.loads((self.directory/'runtime.json').read_text())
        runtime['journal']='state/goal-runtime.sqlite'
        runtime['interval_ms']=10
        path=self.directory/'runtime.json'
        path.write_text(json.dumps(runtime))
        self.window.select_configuration(Configuration(host,path))
        self.window.start_button.emit('clicked')
        pump_until(lambda:self.window.snapshot.get('runtime') is not None and self.window.snapshot['runtime']['completed_goals']==1)
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
        pump_until(lambda:self.window.snapshot['runtime']['completed_goals']==2)
        self.assertIn('2 Ziele fertig',self.window.metrics['review'][1].get_text())
        render_window(self.window,PROJECT_ROOT/'var/control-panel/screenshots/input-form.png')
        self.window.stop_button.emit('clicked')
        pump_until(lambda:not self.window.controller.busy and self.window.state=='idle')

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
            pump_until(lambda:self.window.snapshot.get('runtime') is not None and self.window.snapshot['runtime']['completed_goals']==completed and self.window.snapshot['runtime']['blocked_goals']==blocked)
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
