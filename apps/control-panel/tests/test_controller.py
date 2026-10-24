import json
import os
from pathlib import Path
import queue
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arcs_panel.controller import Configuration, SessionController, read_configuration


FAKE_HOST = '''#!/usr/bin/python3
import json,pathlib,subprocess,sys,time
arguments=sys.argv[1:]
stop=pathlib.Path(arguments[arguments.index('--stop-file')+1])
config=pathlib.Path(arguments[arguments.index('--config')+1])
child=subprocess.Popen([sys.executable,'-c','import time; time.sleep(120)'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
config.with_suffix('.child').write_text(str(child.pid))
print(json.dumps({'event':'ready','catalog':[],'has_runtime':False}),flush=True)
while not stop.exists(): time.sleep(.02)
child.terminate();child.wait(timeout=2)
print(json.dumps({'event':'stopped'}),flush=True)
'''


def next_event(controller, kind, timeout=8):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            event = controller.events.get(timeout=.1)
        except queue.Empty:
            continue
        if event['event'] == kind:
            return event
    raise AssertionError(f"missing {kind}; busy={controller.busy}")


def is_running(pid):
    try:
        return Path(f'/proc/{pid}/stat').read_text().split()[2] != 'Z'
    except FileNotFoundError:
        return False


class ControllerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='arcs panel test ')
        self.root = Path(self.temporary.name)
        executable = self.root / 'target/debug/arcs-host'
        executable.parent.mkdir(parents=True)
        executable.write_text(FAKE_HOST)
        executable.chmod(0o755)
        self.host = self.root / 'config ; $(touch unwanted).json'
        (self.root / 'grant.json').write_text(json.dumps({'adapter_id':'source.left'}))
        self.document = {'config_version':1,'database':'state/core.sqlite',
            'adapters':[{'grant_file':'grant.json','program':str(executable)}]}
        self.host.write_text(json.dumps(self.document))
        self.configuration = Configuration(self.host)
        self.controllers = []

    def tearDown(self):
        for controller in self.controllers:
            if controller.busy:
                controller.force_stop()
                next_event(controller, 'finished')
        self.temporary.cleanup()

    def controller(self):
        controller = SessionController(self.root)
        self.controllers.append(controller)
        return controller

    def test_stop_restart_and_paths_with_shell_characters(self):
        controller = self.controller()
        for _ in range(2):
            controller.start(self.configuration)
            next_event(controller, 'ready')
            child = int(self.host.with_suffix('.child').read_text())
            self.assertTrue(is_running(child))
            with self.assertRaises(RuntimeError):
                controller.start(self.configuration)
            controller.request_stop()
            result = next_event(controller, 'finished')
            self.assertEqual(result['code'], 0)
            self.assertTrue(result['requested'])
            self.assertFalse(controller.busy)
            self.assertFalse(is_running(child))
            self.assertFalse((self.root/'unwanted').exists())

    def test_database_lock_does_not_stop_the_existing_session(self):
        first, second = self.controller(), self.controller()
        first.start(self.configuration)
        next_event(first, 'ready')
        second.start(self.configuration)
        error = next_event(second, 'error')
        self.assertIn('anderen Kontrollpanel', error['message'])
        next_event(second, 'finished')
        self.assertTrue(first.busy)
        first.request_stop()
        self.assertEqual(next_event(first, 'finished')['code'], 0)

    def test_force_stop_ends_the_owned_process_group(self):
        controller = self.controller()
        controller.start(self.configuration)
        next_event(controller, 'ready')
        child = int(self.host.with_suffix('.child').read_text())
        controller.force_stop()
        result = next_event(controller, 'finished')
        self.assertTrue(result['requested'])
        self.assertTrue(result['forced'])
        self.assertFalse(is_running(child))

    def test_build_can_be_cancelled_without_launching_a_host(self):
        cargo = self.root / 'fake cargo'
        cargo.write_text('#!/usr/bin/python3\nimport time\nprint("building",flush=True)\ntime.sleep(30)\n')
        cargo.chmod(0o755)
        controller = self.controller()
        with patch('arcs_panel.controller.shutil.which', return_value=str(cargo)):
            controller.build()
            next_event(controller, 'process')
            controller.request_stop()
            result = next_event(controller, 'finished')
        self.assertTrue(result['requested'])
        self.assertEqual(result['message'], '')
        self.assertFalse(self.host.with_suffix('.child').exists())

    def test_invalid_configuration_does_not_start_a_process(self):
        for document in [[], {}, {'config_version':2}, {**self.document,'adapters':[]},
                {**self.document,'adapters':self.document['adapters']*2}]:
            self.host.write_text(json.dumps(document))
            controller = self.controller()
            with self.assertRaises(ValueError):
                controller.start(self.configuration)
            self.assertFalse(controller.busy)
        self.host.write_text(json.dumps(self.document))
        (self.root/'grant.json').write_text('[]')
        with self.assertRaises(ValueError):
            read_configuration(self.configuration)

    def test_child_start_error_is_reported_and_can_be_retried(self):
        binary=self.root/'target/debug/arcs-host'
        binary.chmod(0o644)
        controller=self.controller()
        controller.start(self.configuration)
        self.assertIn('Permission', next_event(controller,'error')['message'])
        self.assertNotEqual(next_event(controller,'finished')['code'],0)
        binary.chmod(0o755)
        controller.start(self.configuration)
        next_event(controller,'ready')
        controller.request_stop()
        self.assertEqual(next_event(controller,'finished')['code'],0)


if __name__ == '__main__':
    unittest.main()
