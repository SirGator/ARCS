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
import json,os,pathlib,subprocess,sys,time
arguments=sys.argv[1:]
stop=pathlib.Path(arguments[arguments.index('--stop-file')+1])
config=pathlib.Path(arguments[arguments.index('--config')+1])
config.with_suffix('.environment').write_text(json.dumps({'session':os.environ.get('ARCS_TEST_KEY_A')=='provided-by-session','configured':os.environ.get('ARCS_TEST_CONFIG')=='configured-value','other':'ARCS_TEST_UNRELATED' in os.environ,'other_key':'ARCS_TEST_KEY_B' in os.environ,'path':'PATH' in os.environ}))
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

    def test_host_receives_only_declared_ambient_variables_and_selected_credentials(self):
        self.document['adapters'][0]['inherit_env'] = ['ARCS_TEST_CONFIG', 'ARCS_TEST_KEY_A']
        self.host.write_text(json.dumps(self.document))
        controller = self.controller()
        with patch.dict(os.environ, {'ARCS_TEST_CONFIG': 'configured-value', 'ARCS_TEST_KEY_A': 'ambient-old-value',
                'ARCS_TEST_KEY_B': 'other-model-secret', 'ARCS_TEST_UNRELATED': 'unrelated-secret'}):
            controller.start(self.configuration, credentials=lambda _configuration: {'ARCS_TEST_KEY_A': 'provided-by-session'})
            next_event(controller, 'ready')
            self.assertEqual(json.loads(self.host.with_suffix('.environment').read_text()),
                {'session': True, 'configured': True, 'other': False, 'other_key': False, 'path': True})
            controller.request_stop()
            self.assertEqual(next_event(controller, 'finished')['code'], 0)

    def test_environment_limits_apply_per_adapter_in_a_multi_adapter_host(self):
        (self.root / 'second-grant.json').write_text(json.dumps({'adapter_id': 'source.right'}))
        self.document['adapters'][0]['inherit_env'] = [f'ARCS_LEFT_{i}' for i in range(40)]
        self.document['adapters'].append({'grant_file': 'second-grant.json',
            'program': str(self.root / 'target/debug/arcs-host'),
            'inherit_env': [f'ARCS_RIGHT_{i}' for i in range(40)]})
        self.host.write_text(json.dumps(self.document))
        self.assertEqual(len(read_configuration(self.configuration)['inherit_env']), 80)
        controller = self.controller()
        controller.start(self.configuration)
        next_event(controller, 'ready')
        controller.request_stop()
        self.assertEqual(next_event(controller, 'finished')['code'], 0)

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

    def test_control_commands_use_the_owned_pipe_and_report_a_missing_reply(self):
        binary = self.root / 'target/debug/arcs-host'
        binary.write_text('''#!/usr/bin/python3
import json,pathlib,sys,time
args=sys.argv[1:]
if args == ['--help']:
    print('--control-stdin')
    sys.exit(0)
assert '--control-stdin' in args
stop=pathlib.Path(args[args.index('--stop-file')+1])
state=pathlib.Path(args[args.index('--config')+1]).with_suffix('.command')
print(json.dumps({'event':'ready','catalog':[],'has_runtime':True,'control':{'transport':'stdin_jsonl','protocol_version':1}}),flush=True)
for line in sys.stdin:
    request=json.loads(line)
    state.write_text(line)
    while not stop.exists(): time.sleep(.02)
    break
print(json.dumps({'event':'stopped'}),flush=True)
''')
        runtime = self.root / 'runtime.json'
        runtime.write_text(json.dumps({'config_version': 1}))
        controller = self.controller()
        with self.assertRaises(RuntimeError):
            controller.submit_control('series', {})
        controller.start(Configuration(self.host, runtime))
        next_event(controller, 'ready')
        command = {'sequence': 1, 'expected_revision': 0, 'command_id': 'pause', 'change': 'pause'}
        request_id = controller.submit_control('series', command)
        with self.assertRaises(RuntimeError):
            controller.submit_control('series', command)
        path = self.host.with_suffix('.command')
        deadline = time.monotonic() + 5
        while not path.exists() or not path.stat().st_size:
            self.assertLess(time.monotonic(), deadline)
            time.sleep(.02)
        self.assertEqual(json.loads(path.read_text()),
                         {'request_id': request_id, 'control': {'kind': 'series', 'command': command}})
        controller.request_stop()
        result = next_event(controller, 'control_result')
        self.assertEqual(result['request_id'], request_id)
        self.assertFalse(result['ok'])
        self.assertIn('offen', result['error']['message'])
        self.assertEqual(next_event(controller, 'finished')['code'], 0)


if __name__ == '__main__':
    unittest.main()
