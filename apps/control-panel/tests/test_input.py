import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arcs_panel.controller import Configuration, PROJECT_ROOT, SessionController
from arcs_panel.input_bridge import make_submission, parse_json
from desktop_helpers import real_configuration
from test_controller import next_event


def wait_until(predicate, timeout=8):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        if predicate(): return
        time.sleep(.03)
    raise AssertionError('Input did not arrive')


class InputTests(unittest.TestCase):
    def setUp(self):
        self.temporary=tempfile.TemporaryDirectory(prefix='arcs input ; $(literal) ')
        self.directory=Path(self.temporary.name)
        self.controller=SessionController(PROJECT_ROOT)

    def tearDown(self):
        if self.controller.busy:
            self.controller.request_stop()
            next_event(self.controller,'finished',timeout=25)
        self.temporary.cleanup()

    def start(self, example='text-input'):
        host=real_configuration(PROJECT_ROOT,self.directory,example)
        self.controller.start(Configuration(host))
        return next_event(self.controller,'inputs')['targets']

    def artifacts(self):
        with sqlite3.connect(self.directory/'state/core.sqlite') as db:
            return [json.loads(row[0]) for row in db.execute('SELECT artifact_json FROM artifact_versions')]

    def test_multiline_text_reaches_the_real_adapter_and_core_and_survives_restart(self):
        target=self.start()[0]
        payload={'raw_text':'Finde das rote Ziel.\n$(touch unwanted); `literal` — Kamera prüfen.'}
        identity=self.controller.submit_input(target.key,payload)
        self.assertEqual(next_event(self.controller,'input_queued')['id'],identity)
        wait_until(lambda:any(a['payload']==payload for a in self.artifacts()))
        matches=[a for a in self.artifacts() if a['payload']==payload]
        self.assertEqual(len(matches),1)
        self.assertEqual(matches[0]['created_by']['id'],'panel.text')
        self.assertNotEqual(matches[0]['trust']['source_class'],'human')
        self.controller.request_stop();next_event(self.controller,'finished')
        self.start()
        wait_until(lambda:len(self.artifacts())>=1)
        self.assertEqual(len([a for a in self.artifacts() if a['payload']==payload]),1)
        with sqlite3.connect(target.journal) as db:
            self.assertEqual(db.execute('SELECT ack FROM events WHERE id=?',(identity,)).fetchone()[0],1)
        self.assertFalse((PROJECT_ROOT/'unwanted').exists())

    def test_invalid_schema_never_enters_the_outbox_and_correcting_integer_type_works(self):
        target=self.start('goals')[0]
        before=None
        with sqlite3.connect(target.journal) as db:
            before=db.execute('SELECT COUNT(*) FROM events').fetchone()[0]
        for payload in [{'device_id':'fan-01','enabled':False}, {'device_id':'fan-01','enabled':False,'minimum_actuations':1.0}]:
            self.controller.submit_input(target.key,payload)
            self.assertIn('Adaptervertrag',next_event(self.controller,'input_error')['message'])
            with sqlite3.connect(target.journal) as db:
                self.assertEqual(db.execute('SELECT COUNT(*) FROM events').fetchone()[0],before)
        payload={'device_id':'fan-01','enabled':False,'minimum_actuations':1}
        self.controller.submit_input(target.key,payload)
        next_event(self.controller,'input_queued')
        wait_until(lambda:any(a['payload']==payload for a in self.artifacts()))
        self.assertTrue(self.controller.busy)

    def test_multiple_input_targets_stay_separate_and_unconnected_targets_are_rejected(self):
        targets=self.start('multiple-adapters')
        self.assertEqual(len(targets),2)
        with self.assertRaises(ValueError): self.controller.submit_input('unknown',{'values':[1],'target':1})
        for index,target in enumerate(targets):
            payload={'values':[100+index,200+index],'target':150+index}
            self.controller.submit_input(target.key,payload);next_event(self.controller,'input_queued')
            wait_until(lambda:any(a['created_by']['id']==target.adapter_id and a['payload']==payload for a in self.artifacts()))
            self.assertEqual(len([a for a in self.artifacts() if a['payload']==payload]),1)

    def test_pending_submission_blocks_double_click_and_retry_reuses_the_same_event_id(self):
        target=self.start()[0]
        original=__import__('arcs_panel.controller',fromlist=['enqueue_submission']).enqueue_submission
        started,release=threading.Event(),threading.Event()
        calls=[]
        def lost_receipt(target,event,validator):
            calls.append(event['id']);original(target,event,validator)
            started.set();release.wait(timeout=3)
            raise RuntimeError('receipt lost after adapter commit')
        payload={'raw_text':'einmal einreihen'}
        with patch('arcs_panel.controller.enqueue_submission',side_effect=lost_receipt):
            first=self.controller.submit_input(target.key,payload)
            self.assertTrue(started.wait(timeout=5))
            with self.assertRaises(RuntimeError): self.controller.submit_input(target.key,payload)
            release.set();next_event(self.controller,'input_error')
        second=self.controller.submit_input(target.key,payload)
        self.assertEqual(first,second);next_event(self.controller,'input_queued')
        wait_until(lambda:any(a['payload']==payload for a in self.artifacts()))
        self.assertEqual(len([a for a in self.artifacts() if a['payload']==payload]),1)
        with sqlite3.connect(target.journal) as db:
            self.assertEqual(db.execute('SELECT COUNT(*) FROM events WHERE id=?',(first,)).fetchone()[0],1)

    def test_stopped_sessions_and_oversized_or_nonfinite_values_cannot_submit(self):
        with self.assertRaises(RuntimeError): self.controller.submit_input('unconnected',{})
        target=self.start()[0]
        with self.assertRaises(ValueError): self.controller.submit_input(target.key,{'raw_text':'x'*40000})
        with self.assertRaises(ValueError): make_submission(target,{'raw_text':float('nan')})
        with self.assertRaises(ValueError): parse_json('{"value":NaN}')
        self.controller.request_stop()
        with self.assertRaises(RuntimeError): self.controller.submit_input(target.key,{'raw_text':'late'})
        next_event(self.controller,'finished')


if __name__=='__main__': unittest.main()
