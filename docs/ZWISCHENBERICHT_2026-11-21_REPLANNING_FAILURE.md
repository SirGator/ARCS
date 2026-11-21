# Zwischenbericht vom 21. November 2026

ARCS kann bei der Neuplanung eine konkrete Fehlerquittung aus
dem vorherigen Plan berücksichtigen. Eine Planvorlage legt fest,
welche Felder der Ersatzplaner erhält. Assessment und Outcome
müssen den Fehlschlag derselben Ausführung belegen. Der
eingefrorene Kontext hält die Quittung samt Versionsbezug im
Artifact-Netz fest. Ein fehlerhafter Schritt bleibt blockiert.

## Nachweise

- Eine erste Korrektur erzeugt einen Commit, auf dem der echte
  Rechner-Test fehlschlägt. Nach Neustart und simuliertem
  Monatswechsel verarbeitet der Ersatzplaner diese Quittung.
  Die zweite Korrektur beruht auf dem fehlgeschlagenen Commit.
  ARCS prüft den neu erzeugten Commit und erreicht das Ziel nach
  eigenen Freigaben für beide neuen Aktionen.
- Ohne Exportfreigabe erhält der Ersatzplaner keine Fehlerquittung
  aus dem Schritt. Ein nachträglich veränderter Feldausschnitt
  wird vor Veröffentlichung des Rückblicks und vor dem
  Reasoner-Aufruf abgewiesen.

Der Test-Reasoner erhält die zweite Textkorrektur als Vorgabe. Der
Rückblick liefert Teststatus, Exitcode und Commit, ohne ein
Testprotokoll zu erfassen. Damit ist die eigenständige Diagnose
eines unbekannten Quellcodefehlers weiterhin offen.

[Konfiguration und Grenzen der Neuplanung](../apps/adapter-host/examples/plan-replanning.md)
