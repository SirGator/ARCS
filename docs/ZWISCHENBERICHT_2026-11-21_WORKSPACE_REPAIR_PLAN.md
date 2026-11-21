# Zwischenbericht vom 21. November 2026

Ein Plan kann eine freigegebene Korrektur ausführen und anschließend
genau den entstandenen Git-Commit prüfen. Nach dem erfolgreichen
Assessment bindet die Runtime die unveränderliche Edit-Quittung als
Abhängigkeit. Der nächste Reasoner erhält nur das exportierte
`result`-Feld. Der Check-Kandidat verweist auf die Quittung und
durchläuft eine eigene Verification und Freigabe.

Die neue Exportoption ist fachneutral. Mit
`goal.export_execution_schema_id` kann der Betreiber ein Goal auf
seine erfolgreich bewertete Ausführungsquittung als Ergebnis
festlegen. Die Runtime prüft das Schema, den ausführenden Adapter, die
Beziehung zur freigegebenen Aktion und den Nachweis im Assessment. Planvorlagen
begrenzen die exportierbaren Felder weiterhin.

## Nachweise

- Der Integrationstest startet echte Adapter. Die Korrektur erzeugt
  einen neuen Commit. Nach Neustart mit einem um 30 Tage
  vorgerückten Testzeitpunkt wird dieser Commit in den Prüfkandidaten
  übernommen. Inzwischen zeigt die Arbeitsreferenz auf einen
  neueren fehlerhaften Nachfolger. Nach einer eigenen Freigabe
  besteht der Test weiterhin auf dem Commit aus der Quittung.
- Ein Konflikt der Edit-Basis blockiert den Prüfschritt. Eine
  unzulässige Feldauswahl blockiert den Plan vor der ersten Action.

Die Beispielaufgabe enthält bereits die gewünschte Textkorrektur.
Die automatische Diagnose eines unbekannten Fehlers und die Auswahl
einer Korrektur liegen außerhalb dieses Ablaufs.

[Adapteranleitung](../apps/adapter-host/examples/workspace-check.md)
