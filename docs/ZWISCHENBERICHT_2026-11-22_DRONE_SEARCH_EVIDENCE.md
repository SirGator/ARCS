# Zwischenbericht vom 22. November 2026

Die externe Drohnenanwendung kann einen Fund anhand der ursprünglichen
Kameraaufnahme erneut prüfen. Sie verfolgt die gespeicherte Verknüpfung
zwischen Auftrag, Rundenbeleg und Abschlussbewertung. Nach einer Bewegung
gleicht sie die Pose der Aufnahme mit der Ausführungsquittung ab. Der
lokale Bericht enthält Versions-IDs, Pose, Pixelzahl und Bildbereich.
Eine ausdrücklich angeforderte PPM-Datei enthält die Originalbytes.

Die Anwendung kann außerdem einen lokalen Datei-Output konfigurieren.
Der ARCS-Host bereitet die Abschlussbewertung als Bericht mit ihren
genauen Evidenzversionen vor. Die Zustellung verwendet den vorhandenen
Freigabe- und Wiederaufnahmevertrag.

## Nachweise

- Ein Fund nach zwei Drehungen exportierte die Aufnahme aus Revision 2.
  Ein zweiter Export auf denselben Pfad ließ die erste Datei erhalten.
- Bei fehlender Zielfarbe meldete der Bericht `budget_exhausted`. Eine
  angeforderte Funddatei wurde abgewiesen. Bei einem sofort sichtbaren
  Ziel stützte sich der Beleg auf die erste Aufnahme ohne Ausführungsquittung.
- Die Vorschau und ein Neustart ließen das Zustellverzeichnis leer.
  Erst die Freigabe erzeugte die Textdatei. Dieselbe Entscheidung und
  weitere Neustarts hielten die Zustellung bei einer Datei.

33 Rust-Tests im Simulator, drei Such-Prozesstests und fünf
Output-Integrationstests in ARCS bestanden.

Der Quellstand des Simulators liegt im lokalen Commit `fbd0441` in
`drone-sim`. Für dieses Repository ist bislang kein Git-Remote eingerichtet.

[Aufruf und Freigabeverfahren](../../drone-sim/apps/drone-demo/README.md#fundbeleg-und-lokale-bilddatei)
