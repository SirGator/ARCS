# Zwischenbericht vom 22. November 2026

ARCS kann ausgewählte Prüfungen über `check.revision.isolated`
mit Bubblewrap ausführen. Das Profil bindet die Git-Kopie
schreibgeschützt ein. Der Prozess erhält ein frisches temporäres
Verzeichnis sowie getrennte Netzwerk- und PID-Namespaces. Ein
Sandbox-Start ohne gültigen Exitbeleg liefert einen
Ausführungsfehler. Isolierte Prüfnamen werden durch die übrigen
Prüffähigkeiten desselben Adapterprofils abgewiesen.

## Nachweise

- Der Prozess-Test nutzte Bubblewrap 0.9.0 aus einem lokal
  entpackten Testpaket. Der Test konnte den Git-Quelltext lesen
  und in das temporäre Verzeichnis schreiben. Schreibzugriffe auf
  die Git-Kopie scheiterten. Die Host-Testdatei, der Host-Prozess
  und dessen Loopback-Listener waren aus dem Test nicht erreichbar.
  Der Adapter unterschied bestandene Prüfung, Fehlcode und Timeout.
- Ein Testlauncher ohne Sandbox-Exitbeleg lieferte `runner_error`.
  Auch der Aufruf über den gewöhnlichen Prüfpfad wurde abgewiesen.
  Der Marker für einen ausgeführten Prüfbefehl blieb in beiden
  Fällen aus.
- Der Reparaturplan lief über beide isolierten Prüfungen durch.
  Nach Neustart und getrennten Testfreigaben bestand der neue
  Git-Commit. Dieser Lauf verwendete den deterministischen
  Test-Reasoner.

Die Abnahme umfasst den Python-Reparaturablauf und die genannten
Namespace-Grenzen. Eigene CPU-, RAM- und Prozessquoten bleiben
außerhalb dieses Profils.

[Profil, Grants und nachprüfbare Testbefehle](../apps/adapter-host/examples/workspace-check.md#eine-revision-mit-bubblewrap-isoliert-prüfen)
