# Zwischenbericht vom 21. November 2026

Ein Ersatzplan kann eine freigegebene Fehlerquittung in einen
nachfolgenden Reparaturschritt übernehmen. Der Plan speichert den
Bezug als `failure_evidence`. Die Vorlage begrenzt, welche Felder
aus dem Rückblick verwendet werden dürfen. Für diese Weitergabe
braucht die Planroute eine eigene Betreiberfreigabe.

Der Reparaturschritt ruft anhand der Testquittung den Quelltext des
fehlgeschlagenen Commits ab. Die Modellinstanz im Nachbarprojekt
`llm-api` erhält die gespeicherte Datei und den Testfehler. Sie
erzeugt einen Kandidaten für die Textkorrektur. Verification und
Freigabe bleiben im Host, die Änderung und die Prüfung gehen über
die vorhandenen Act-Fähigkeiten.

## Nachweis mit echter Inferenz

- Im ersten Lauf berechnete das lokale Modell die richtige
  Änderung und wählte den älteren Basis-Commit aus der Edit-Quittung.
  Der Test stoppte vor der neuen Freigabe.
- Danach wurde die Revisionsquelle direkt an die Fehlerquittung
  gebunden. Der Ersatzschritt erhält den betroffenen Commit aus der
  Prüfung und den exakt dazu abgerufenen Quelltext.
- Im zweiten Lauf erzeugte `qwen2.5:14b` über das lokale Ollama-API
  nach einem Neustart und 30 simulierten Tagen den korrekten
  Reparaturkandidaten mit 137 Ausgabetokens. Nach den getrennten
  Testfreigaben wurde der neue Commit erzeugt und die echte
  Rechnerprüfung bestand.
- Die Prozess-Tests prüfen, dass bei fehlender Betreiberfreigabe
  keine neue Generation mit Fehlerbeleg startet.

Der Modelltest verwendet ein isoliertes Python-Rechnerprojekt.
Seine automatisierten Freigaben setzen die erwartete minimale
Rechnerkorrektur und den passenden Basis-Commit voraus. Der
Nachweis gilt für diesen Fehlerfall und belegt die Verbindung aus
Neuplanung, Modellvorschlag und geprüfter Ausführung.

[Reparaturtest und Modellprofil](../apps/adapter-host/examples/workspace-check.md#einen-lokalen-modellvorschlag-für-die-reparatur-prüfen)
