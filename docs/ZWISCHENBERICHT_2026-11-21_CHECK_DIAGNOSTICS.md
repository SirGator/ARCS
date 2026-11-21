# Zwischenbericht vom 21. November 2026

Eine freigegebene revisionsgenaue Prüfung kann jetzt begrenzte
Standardausgabe und Fehlerausgabe als Diagnose speichern. Die
Capability `check.revision.diagnostic` gilt für ausdrücklich
konfigurierte Prüfungen. Das optionale Feld `diagnostics` des
v2-Ausführungsartefakts lässt sich für die Neuplanung unabhängig
von den Prüfmetadaten freigeben.

## Nachweise

- Der native Prüfadapter verarbeitet große Ausgaben, hält die
  ersten 512 Eingabebytes je Strom fest und markiert Kürzungen.
  Auch ein Hintergrundprozess mit offenen Ausgabeströmen hält
  das Ergebnis nicht auf. Bestandene Prüfungen speichern
  keine Ausgabetexte.
- Nach einem simulierten Verlust der Core-Quittung übernimmt
  ARCS die gespeicherte Diagnose aus dem Adapterjournal.
  Der Prüfprozess startet dabei nur einmal.
- Der Ende-zu-Ende-Test übergibt dem Ersatzplaner nach
  Neustart und 30 simulierten Tagen die echte Meldung
  `AssertionError: 6 != 5` und den Commit. Der Planer verlangt
  den Befund, bevor er den Ersatzplan vorschlägt. Der Text bleibt
  bei einem Export von nur `result` außerhalb des Planerkontexts.
  Die zweite Korrektur verwendet weiterhin eine vorgegebene
  Textänderung.

[Adapterkonfiguration und Quittungsschema](../apps/adapter-host/examples/workspace-check.md#diagnoseausgaben-einer-revision-erfassen)
