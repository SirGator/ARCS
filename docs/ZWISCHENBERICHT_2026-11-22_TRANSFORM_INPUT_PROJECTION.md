# ARCS-Zwischenbericht · 22. November 2026

## Feldbeschränkter Transform-Eingang

Die Pipeline kann mit `input_projection` ausgewählte Felder einer
Observation für Transform freigeben. Der Core validiert die Originalversion
und prüft die kopierten Felder gegen ein registriertes
`arcs.projection.*`-Schema. Er speichert eine eigene unveränderliche
Artefaktversion. Ihre Trust-Einstufung bleibt innerhalb der Grenzen der
Quelle, und `derived_from` verweist auf die Originalversion.

Die Transform-Kette und die automatische Pfadsuche verwenden das
Projektionsschema. Der vorhandene Transform-Port erhält nur die
Projektionsversion und ihren Payload. Die Zustellung nennt Original- und
Projektionsversion. Dieselbe Feldauswahl und Quellversion führen über
Neustarts hinweg zur selben Projektion.

## Verifikation

Der Prozess-Test exportiert `value` und `unit` aus einem Artefakt mit weiteren
Feldern. Der echte Berichtsadapter erhält und rendert diese beiden Felder.
Der Test weist ein Steuerschema vor dem Polling ab und blockiert den ersten
Aufruf durch ein zu kleines Bytebudget. Nach Neustart verwendet ARCS die
gespeicherte Projektion wieder und liefert genau eine freigegebene Datei.

268 Core-Tests und 16 Pipeline-Prozesstests bestanden. Der Workspace
bestand die Prüfung aller Targets.

## Delegierte Dokumentation

- **Erhalten** – Bonsai 2 27B erhielt den Metaprompt, die Feldnamen,
  die Schema- und Verarbeitungsregeln sowie `docs/METAPROMPTS.md`.
- **Erledigt** – Kurzen Text und JSON-Beispiel entworfen. Der Hauptagent hat
  den Entwurf geprüft und in die Pipeline-Anleitung übernommen.
- **Geprüft** – Feldnamen im Entwurf mit der Vorgabe abgeglichen.
- **Status** – `SUCCESS`.

## Delegierte Core-Tests

- **Erhalten** – GPT‑5.6 Luna erhielt den Core-Einstiegspunkt, die vorhandene
  Fixture und den Auftrag, Feldauswahl, Trust, Replay und ungültige Optionen
  zu prüfen. Schreibumfang waren die Testdatei und dieser Bericht.
- **Erledigt** – Zwei Regressionstests in
  `crates/arcs-core/src/io/transform/tests.rs` ergänzt. Der Hauptagent
  ergänzte Prüfungen für `derived_from` und einen unpassenden Payload.
- **Geprüft** – 12 gezielte Core-Tests zu Transform bestanden.
  `git diff --check` war erfolgreich.
- **Status** – `SUCCESS`.

## Offener Integrationspunkt

Plan-Abhängigkeiten brauchen eine eigene Zuordnung zwischen erlaubten
Exportfeldern, Projektionsschema und freigegebener Transform-Capability.
Die Pipeline-Option gilt für Observation-Eingaben.
