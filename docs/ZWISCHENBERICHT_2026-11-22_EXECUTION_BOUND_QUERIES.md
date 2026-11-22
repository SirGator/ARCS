# Zwischenbericht vom 22. November 2026

Nach einer Aktion kann ARCS einen Request an ihre gespeicherte
Ausführungsquittung binden. Die Route gibt Quittungsschema, ausgewähltes
Feld und Zielparameter an. Die Runtime prüft die Zugehörigkeit zur
Aktion und speichert die Herkunft am Request. Der Nachher-Request kann
damit genau die bestätigte Zustandsrevision anfordern.

## Nachweise

- Ein Prozesstest unterbrach den Runtime-Commit nach der ersten
  Nachher-Abfrage. Beim Neustart blieben Request und Quittung identisch.
  Die erste Aktion wurde kein zweites Mal ausgeführt.
- Ein unzulässiger Feldexport wurde vor der Aufnahme abgewiesen.
  Ein Wert mit falschem Typ erreichte den Request-Adapter nicht.
- Im lokalen Versuch mit dem separaten Drohnensimulator forderte
  `camera.capture` nach zwei genehmigten Drehungen exakt die Revisionen
  1 und 2 an. Der externe Test-Reasoner erhielt die PPM-Bildbytes.
  Er meldete den anfangs unsichtbaren Farbmarker erst in der zweiten
  Aufnahme als gefunden. Der Durchlauf mit einer anderen Zielfarbe
  endete am konfigurierten Rundenbudget. Ein erneuter Host-Start zeigte
  in beiden Fällen dieselben Ausführungsquittungen.

Für den Simulatorversuch wurde eine temporäre Testkonfiguration mit
externer Regelstrategie verwendet. Der ARCS-Host enthält weiterhin nur
den allgemeinen Request-Vertrag.

[Konfiguration und Datenvertrag](../apps/adapter-host/examples/goal-runtime.md#nachher-abfragen-an-die-ausführungsquittung-binden)
