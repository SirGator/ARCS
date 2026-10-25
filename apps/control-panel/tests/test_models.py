import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from arcs_panel.credentials import Credentials
from arcs_panel.controller import PROJECT_ROOT, read_configuration
from arcs_panel.model_connections import ModelConnections, normalize_connection


def connection(model="local-test", requires_key=False):
    return {"id": uuid.uuid4().hex, "name": model, "provider": "ollama", "model": model,
            "endpoint": "http://127.0.0.1:11434/api/chat", "requires_key": requires_key,
            "remember_key": False, "provider_options": {}}


class FakeSecrets:
    class Schema:
        @staticmethod
        def new(*_args):
            return object()
    class SchemaFlags:
        NONE = 0
    class SchemaAttributeType:
        STRING = 0
    COLLECTION_DEFAULT = "default"

    def __init__(self):
        self.keys = {}
        self.fail = False

    def password_store_sync(self, _schema, attributes, _collection, _label, key, _cancel):
        if self.fail:
            raise ValueError("backend might include sensitive text")
        self.keys[attributes["connection_id"]] = key
        return True

    def password_lookup_sync(self, _schema, attributes, _cancel):
        return self.keys.get(attributes["connection_id"])

    def password_clear_sync(self, _schema, attributes, _cancel):
        return self.keys.pop(attributes["connection_id"], None) is not None


class ModelTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="arcs-model-settings-")
        self.directory = Path(self.temporary.name) / "models"
        self.credentials = Credentials(False)
        self.store = ModelConnections(PROJECT_ROOT, self.directory, self.credentials)

    def tearDown(self):
        self.store.close()
        self.temporary.cleanup()

    def test_multiple_connections_assignments_and_settings_survive_restart_without_secrets(self):
        first, second = connection("planner"), connection("assessor")
        for record in (first, second):
            self.store.save(record)
        self.store.assign(first["id"], second["id"])
        restored = ModelConnections(PROJECT_ROOT, self.directory, Credentials(False))
        try:
            self.assertEqual(len(restored.records), 2)
            self.assertEqual(restored.assignments, {"planner": first["id"], "assessor": second["id"]})
            config = restored.materialize()
            preview = read_configuration(config)
            self.assertEqual(len(preview["adapters"]), 4)
            self.assertTrue(all(path.is_file() for path in preview["programs"]))
            host = json.loads(config.host.read_text())
            planner = json.loads((config.host.parent / host["adapters"][2]["profile"]).read_text())
            assessor = json.loads((config.host.parent / host["adapters"][3]["profile"]).read_text())
            self.assertEqual(planner["model"], "planner")
            self.assertEqual(assessor["model"], "assessor")
            self.assertIsNone(planner["api_key_env"])
            self.assertEqual(restored.environment_for(config), {})
        finally:
            restored.close()

    def test_api_keys_are_session_only_and_never_written_into_generated_files(self):
        record = connection(requires_key=True)
        self.store.save(record, "test-super-secret")
        self.store.assign(record["id"], record["id"])
        config = self.store.materialize()
        environment = self.store.environment_for(config)
        self.assertEqual(set(environment.values()), {"test-super-secret"})
        for path in self.directory.rglob("*"):
            if path.is_file():
                self.assertNotIn(b"test-super-secret", path.read_bytes())
                self.assertEqual(path.stat().st_mode & 0o077, 0)
        restored = ModelConnections(PROJECT_ROOT, self.directory, Credentials(False))
        try:
            with self.assertRaisesRegex(ValueError, "API-Schlüssel fehlt"):
                restored.environment_for(config)
        finally:
            restored.close()

    def test_model_changes_create_separate_journals_while_key_rotation_and_names_preserve_them(self):
        record = connection(requires_key=True)
        self.store.save(record, "first-secret")
        self.store.assign(record["id"], record["id"])
        original = self.store.materialize()
        record["name"] = "Renamed connection"
        self.store.save(record, "rotated-secret")
        self.assertEqual(self.store.materialize(), original)
        self.assertEqual(set(self.store.environment_for(original).values()), {"rotated-secret"})
        record["model"] = "other-model"
        self.store.save(record)
        changed = self.store.materialize()
        self.assertNotEqual(changed.host.parent, original.host.parent)
        old_profile = json.loads((original.host.parent / "planner.json").read_text())
        self.assertEqual(old_profile["model"], "local-test")

    def test_changed_endpoint_does_not_reuse_a_previous_secret(self):
        record = connection(requires_key=True)
        self.store.save(record, "old-secret")
        record["endpoint"] = "https://different.example/api/chat"
        with self.assertRaisesRegex(ValueError, "Adresswechsel"):
            self.store.save(record)
        with self.assertRaisesRegex(ValueError, "Adresswechsel"):
            self.store.key_for(record)

    def test_persistent_credentials_use_only_the_keyring_and_can_be_forgotten(self):
        backend = FakeSecrets()
        first = Credentials(backend)
        identifier = uuid.uuid4().hex
        first.put(identifier, "keyring-test-secret", remember=True)
        first.close()
        restored = Credentials(backend)
        self.assertEqual(restored.get(identifier, remembered=True), "keyring-test-secret")
        restored.put(identifier, "session-secret", remember=False, previously_remembered=True)
        self.assertFalse(backend.keys)
        self.assertEqual(restored.get(identifier), "session-secret")
        backend.fail = True
        with self.assertRaisesRegex(ValueError, "nicht im Systemschlüsselbund") as failure:
            restored.put(identifier, "sensitive", remember=True)
        self.assertNotIn("sensitive", str(failure.exception))
        self.assertNotIn("backend might", str(failure.exception))
        restored.close()

    def test_corrupt_settings_are_not_overwritten_and_concurrent_edits_are_rejected(self):
        other = ModelConnections(PROJECT_ROOT, self.directory, Credentials(False))
        try:
            self.store.save(connection())
            with self.assertRaisesRegex(ValueError, "anderen Fenster"):
                other.save(connection("other"))
            other.reload()
            other.save(connection("other"))
            self.store.reload()
            self.assertEqual(len(self.store.records), 2)
        finally:
            other.close()
        path = self.directory / "connections.json"
        path.write_text("broken-json")
        broken = ModelConnections(PROJECT_ROOT, self.directory, Credentials(False))
        try:
            self.assertIsNotNone(broken.load_error)
            with self.assertRaises(ValueError):
                broken.save(connection())
            self.assertEqual(path.read_text(), "broken-json")
        finally:
            broken.close()

    def test_assigned_connections_cannot_be_deleted_until_reassigned(self):
        first, second = connection("first"), connection("second")
        for record in (first, second):
            self.store.save(record)
        self.store.assign(first["id"], first["id"])
        with self.assertRaisesRegex(ValueError, "zugeordnet"):
            self.store.remove(first["id"])
        self.store.assign(second["id"], second["id"])
        self.store.remove(first["id"])
        self.assertEqual([record["id"] for record in self.store.records], [second["id"]])

    def test_credentials_manifest_cannot_override_other_environment_variables(self):
        record = connection(requires_key=True)
        self.store.save(record, "test-secret")
        self.store.assign(record["id"], record["id"])
        config = self.store.materialize()
        path = config.host.parent / "credentials.json"
        manifest = json.loads(path.read_text())
        manifest["credentials"][0]["env"] = "PATH"
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "Schlüsselreferenz"):
            self.store.environment_for(config)

    def test_endpoint_credentials_and_invalid_options_are_rejected(self):
        record = connection()
        for endpoint in ("https://name:password@example.com", "https://example.com?key=secret", "http://example.com"):
            record["endpoint"] = endpoint
            with self.assertRaises(ValueError):
                normalize_connection(record)

    def test_manually_edited_generated_files_are_never_overwritten(self):
        record = connection()
        self.store.save(record)
        self.store.assign(record["id"], record["id"])
        config = self.store.materialize()
        path = config.host.parent / "planner.json"
        changed = json.loads(path.read_text())
        changed["model"] = "manually-configured-model"
        path.write_text(json.dumps(changed))
        with self.assertRaisesRegex(ValueError, "manuell geändert"):
            self.store.materialize()
        self.assertEqual(json.loads(path.read_text())["model"], "manually-configured-model")

    def test_vision_models_cannot_be_assigned_as_language_planners(self):
        record = connection("yolo11n")
        record.update(provider="yolo_http", kind="vision", endpoint="http://127.0.0.1:11435/v1/detect")
        self.store.save(record)
        with self.assertRaisesRegex(ValueError, "Sprachmodell"):
            self.store.assign(record["id"], record["id"])

    @unittest.skipUnless((PROJECT_ROOT.parent/'model-services/adapter/target/debug/arcs-vision-api').is_file(), 'Externer Bildadapter nicht installiert')
    def test_language_and_vision_services_are_materialized_as_separate_adapter_instances(self):
        language = connection("qwen2.5:1.5b")
        vision = connection("yolo11n")
        vision.update(provider="yolo_http", kind="vision", endpoint="http://127.0.0.1:11435/v1/detect")
        for record in (language, vision):
            self.store.save(record)
        self.store.assign(language["id"], language["id"], vision["id"])
        config = self.store.materialize()
        preview = read_configuration(config)
        self.assertEqual(len(preview['adapters']), 5)
        grant = json.loads((config.host.parent/'vision-grant.json').read_text())
        self.assertEqual(grant['reasoning_limits_v2']['methods'], ['learned'])
        self.assertEqual(grant['granted_permissions'], ['reason.compute', 'data.receive'])
        profile = json.loads((config.host.parent/'vision-profile.json').read_text())
        self.assertEqual(profile['model'], 'yolo11n')


if __name__ == "__main__":
    unittest.main()
