# ARCS-Zwischenbericht · 22. November 2026

## Automatischer Transform-Pfad für Pipelines

Die bestehende Pipeline kann mit `transform_routing` den kürzesten Pfad
zwischen der Eingabe und einem vorgegebenen Zielschema suchen. Die
Betreiberliste begrenzt die zulässigen Transform-Capabilities. Der Core
prüft Grants, Rollen und Ein-/Ausgabeschemas. Identische Start- und
Zielschemas benötigen keinen Transform-Schritt.

Die gewählten Schritte und Manifeste gehören zur gespeicherten
Pipeline-Definition. Beim Wiederanlauf prüft das Journal seine Zwischenstände
gegen die aufgelöste Schrittliste. Ein bereits gespeichertes Ergebnis wird
über den vorhandenen Transform-Dienst wiederverwendet. Die Anwendung erhält
die ausgewählten Verträge für ihre Verification und Freigabe der Ausgabe.

Die Suche gilt für Pipelines mit vollständigen Eingabeartefakten. Für
feldbeschränkte Plan-Abhängigkeiten fehlt noch ein eigener Eingabevertrag
für Transform-Adapter.

## Verifikation

15 Pipeline-Prozesstests und 10 gezielte Core-Tests zur Transform-Funktion
bestanden. Der neue Pipeline-Test weist einen zu kurzen Pfad vor dem Polling
ab, findet anschließend zwei kompatible Adapter und unterbricht die
Ausführung am zweiten Schritt. Nach dem Neustart bleibt die erste
Ergebnisversion erhalten. Die freigegebene Ausgabe wird genau einmal
zugestellt. Ein zweiter Test bestätigt die Freigabe der Originalversion ohne
Transform-Schritt. Der Workspace bestand die Prüfung aller Standard-Targets.

## Delegierte Dokumentation

- **Erhalten** – GPT-5.6 Luna erhielt den Auftrag, die Observe-Report-README um `transform_routing` zu ergänzen, mit `output_schema_id`, 0 bis 64 `allowed_capabilities` und `max_steps` von 1 bis 32.
- **Erledigt** – `apps/adapter-host/examples/observe-report/README.md` um Konfiguration, Demo-Pfad, deterministisches Routing sowie Journal-, Topologie- und Freigaberegeln ergänzt.
- **Geprüft** – `git diff --check` erfolgreich.
- **Status** – `SUCCESS`.

## Delegierte Core-Tests

- **Erhalten** – GPT-5.6 Luna erhielt den Einstiegspunkt `TransformRouting::resolve`, die Regeln für deterministische kürzeste Pfade, Zero-Hop, fehlende Grants und den Testbefehl ohne Produktionscodeänderungen.
- **Erledigt** – `crates/arcs-core/src/io/transform/tests.rs` um Tests für umgekehrte Allowlists, kürzeste Pfade, Grenzen, Zero-Hop und fehlende Grants ergänzt.
- **Geprüft** – `cargo test -p arcs-core --locked --offline --lib io::transform` mit 10 erfolgreichen Tests und `git diff --check` erfolgreich.
- **Status** – `SUCCESS`.
