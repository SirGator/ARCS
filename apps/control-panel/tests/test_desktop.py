import json
from pathlib import Path
import sys
import tempfile
import time
import unittest

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


if __name__=='__main__':
    unittest.main()
