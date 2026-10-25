"""API-Schlüssel im Speicher oder optional im Systemschlüsselbund."""
import threading


class Credentials:
    def __init__(self, backend=None):
        self._keys = {}
        self._lock = threading.RLock()
        self.backend = backend
        if backend is None:
            try:
                import gi
                gi.require_version("Secret", "1")
                from gi.repository import Secret
                self.backend = Secret
            except (ImportError, ValueError):
                pass
        self.schema = self.backend.Schema.new("org.arcs.Control.Model", self.backend.SchemaFlags.NONE,
            {"connection_id": self.backend.SchemaAttributeType.STRING}) if self.backend else None

    @property
    def persistent_available(self):
        return bool(self.backend)

    def has_session_key(self, identifier):
        with self._lock:
            return identifier in self._keys

    def get(self, identifier, remembered=False):
        with self._lock:
            if identifier in self._keys:
                return self._keys[identifier]
        if not remembered:
            return None
        if not self.backend:
            raise ValueError("Systemschlüsselbund nicht verfügbar. Bitte den Schlüssel für diese Sitzung erneut eingeben.")
        try:
            key = self.backend.password_lookup_sync(self.schema, {"connection_id": identifier}, None)
        except Exception:
            raise ValueError("Schlüsselbund nicht erreichbar oder gesperrt. Bitte entsperren oder den Schlüssel erneut eingeben.") from None
        if key:
            with self._lock:
                self._keys[identifier] = key
        return key

    def put(self, identifier, key, remember=False, previously_remembered=False):
        if not isinstance(key, str) or not key or any(ord(char) < 32 or ord(char) == 127 for char in key):
            raise ValueError("Bitte einen gültigen API-Schlüssel eintragen.")
        if remember:
            if not self.backend:
                raise ValueError("Ein Systemschlüsselbund ist nicht verfügbar. Der Schlüssel kann nur für diese Sitzung genutzt werden.")
            try:
                if not self.backend.password_store_sync(self.schema, {"connection_id": identifier},
                    self.backend.COLLECTION_DEFAULT, "ARCS Modellzugang", key, None):
                    raise RuntimeError()
            except Exception:
                raise ValueError("Schlüssel konnte nicht im Systemschlüsselbund gespeichert werden. Nutze bei Bedarf die Speicherung nur für diese Sitzung.") from None
        elif previously_remembered:
            self._clear_persistent(identifier)
        with self._lock:
            self._keys[identifier] = key

    def _clear_persistent(self, identifier):
        if not self.backend:
            raise ValueError("Der gespeicherte Schlüssel kann ohne Systemschlüsselbund nicht entfernt werden.")
        try:
            self.backend.password_clear_sync(self.schema, {"connection_id": identifier}, None)
        except Exception:
            raise ValueError("Der gespeicherte Schlüssel konnte nicht aus dem Systemschlüsselbund entfernt werden.") from None

    def remove(self, identifier, remembered=False):
        if remembered:
            self._clear_persistent(identifier)
        with self._lock:
            self._keys.pop(identifier, None)

    def close(self):
        with self._lock:
            self._keys.clear()
