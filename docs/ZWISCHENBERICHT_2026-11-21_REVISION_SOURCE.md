# Zwischenbericht vom 21. November 2026

ARCS kann freigegebenen Quelltext aus einem konkreten Git-Commit
als Request-Artefakt abrufen. Das Adapterprofil legt Referenz und
zulässige Dateien fest. Die Runtime kann aus einem gebundenen
Vorgängerexport den Commit in die Request-Eingabe übernehmen und
die Herkunft im gemeinsamen Artifact-Netz festhalten.

## Nachweise

- Der Adapter liest die exakten Bytes einer freigegebenen
  UTF-8-Datei. Pfade außerhalb der Freigabe, Symlinks und
  unerreichbare Commits werden abgewiesen. Eine bereits
  gespeicherte Antwort bleibt beim Neustart erhalten, auch
  nach dem Zurücksetzen der lokalen Referenz.
- Im Reparaturtest führt eine echte Fehlermeldung zur Neuplanung.
  Nach Neustart und 30 simulierten Tagen liest der Nachfolger
  den Quelltext genau des fehlgeschlagenen Stands. Eine
  fallbezogene Regel leitet daraus die Codeänderung ab.
  Nach den getrennten Freigaben wird der neue Commit erfolgreich
  geprüft.
- Ein Request kann keine Felder außerhalb des freigegebenen
  Vorgängerexports lesen.

Der Reparaturtest verwendet eine deterministische Regel für
einen bekannten Python-Fehler. Eine allgemeine Diagnose durch
ein produktives Sprachmodell ist mit diesem Test noch offen.

[Adapter- und Request-Konfiguration](../apps/adapter-host/examples/workspace-check.md#quelltext-einer-revision-abrufen)
