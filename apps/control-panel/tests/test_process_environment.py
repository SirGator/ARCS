import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arcs_panel.model_connections import AdapterSetup
from arcs_panel.input_bridge import InputTarget, enqueue_submission


class ProcessEnvironmentTests(unittest.TestCase):
    def test_probe_gets_only_the_explicit_key_and_describe_gets_none(self):
        with tempfile.TemporaryDirectory(prefix="arcs-probe-env-") as directory:
            program = Path(directory) / "probe.py"
            program.write_text("""#!/usr/bin/python3
import json,os
print(json.dumps({'ok':True,'probe_key':os.environ.get('ARCS_PANEL_PROBE_KEY')=='synthetic-probe-key','other':'ARCS_TEST_UNRELATED' in os.environ,'ambient_model_key':'OPENAI_API_KEY' in os.environ,'path':'PATH' in os.environ}))
""")
            program.chmod(0o700)
            setup = AdapterSetup(Path(directory))
            try:
                with patch.dict(os.environ, {"ARCS_TEST_UNRELATED": "ambient-secret", "OPENAI_API_KEY": "ambient-model-key"}):
                    described = setup.run("--describe", program=program)
                    probed = setup.run("--probe", {}, {"ARCS_PANEL_PROBE_KEY": "synthetic-probe-key"}, program)
                self.assertEqual(described, {"ok": True, "probe_key": False, "other": False, "ambient_model_key": False, "path": True})
                self.assertEqual(probed, {"ok": True, "probe_key": True, "other": False, "ambient_model_key": False, "path": True})
            finally:
                setup.close()

    def test_validation_and_enqueue_do_not_inherit_ambient_keys(self):
        with tempfile.TemporaryDirectory(prefix="arcs-input-env-") as name:
            directory = Path(name)
            program = directory / "arcs-json-input"
            program.write_text("""#!/usr/bin/python3
import json,os,pathlib,sys
root=pathlib.Path(__file__).parent
mode=sys.argv[1]
(root/(mode[2:]+'.environment')).write_text(json.dumps({'ambient':'ARCS_TEST_UNRELATED' in os.environ,'model':'OPENAI_API_KEY' in os.environ,'path':'PATH' in os.environ}))
if mode=='--validate-input': print(json.dumps({'valid':True}))
else: print(json.dumps({'queued':json.loads(pathlib.Path(sys.argv[4]).read_text())['id']}))
""")
            program.chmod(0o700)
            target = InputTarget("input", "submit", "arcs.input.v1", program, directory / "profile.json", directory / "journal.sqlite", directory, {}, schema={})
            event = {"id": "event-1", "payload": {"raw_text": "test"}}
            with patch.dict(os.environ, {"ARCS_TEST_UNRELATED": "ambient-secret", "OPENAI_API_KEY": "ambient-model-key"}):
                enqueue_submission(target, event, program)
            for mode in ("validate-input", "enqueue"):
                self.assertEqual(json.loads((directory / (mode + ".environment")).read_text()), {"ambient": False, "model": False, "path": True})


if __name__ == "__main__":
    unittest.main()
