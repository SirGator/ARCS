# ARCS-Zwischenbericht · 22. November 2026

## Parallele Transform-Aufrufe

`Host::poll_transform` führt freigegebene Transform-Aufrufe in begrenzten
Workern aus. Der Host reserviert die Adapter-Sitzung und geprüfte Kapazitäten
aus benannten Pools. Der Worker kommuniziert mit dem Adapter. Der Host
validiert und speichert das Ergebnis samt Provenienz und Invocation-Status.

Bereits gespeicherte Ergebnisse können auch während einer belegten
Adapter-Sitzung gelesen werden. Ein ausstehender Aufruf wird nach einem
unterbrochenen Host-Lauf anhand derselben Invocation-ID wieder aufgenommen.
Die synchrone Pipeline verwendet dafür dieselbe Core-Validierung.

## Verifikation

Der Prozess-Test hält einen Adapter mit einem Dateisignal an. Ein zweiter
Adapter mit demselben Pool wartet. Ein unabhängiger Adapter liefert sein
Ergebnis währenddessen. Der Test prüft Worker-Limit, exklusive Sitzung,
Replay bei belegter Sitzung, Wiederanlauf einer abgebrochenen Anfrage
und Freigabe des Pools nach Timeout.

269 Core-Tests und 54 Host-Prozesstests bestanden. Der Workspace
bestand die Prüfung aller Targets.

## Delegierte Dokumentation

- **Erhalten** – Bonsai 2 27B erhielt `docs/METAPROMPTS.md`, die Host-API,
  ihre Rückgabewerte und die Regeln für Replay, Worker und Pools.
- **Erledigt** – Kurzabschnitt für die Pipeline-Anleitung entworfen.
  Der Hauptagent hat die Formulierung geprüft und übernommen.
- **Geprüft** – Rückgabewerte und Anwendungsgrenzen mit dem Auftrag abgeglichen.
- **Status** – `SUCCESS`.

## Delegierte Core-Tests

- **Erhalten** – GPT‑5.6 Luna erhielt die vorhandene Fixture, den Auftrag
  zum Test von `prepare_for_worker`, `finish` und synchronem Replay
  sowie als Schreibumfang die Testdatei und diesen Bericht.
- **Erledigt** – Einen Regressionstest in
  `crates/arcs-core/src/io/transform/tests.rs` ergänzt. Er prüft,
  wann der Adapter aufgerufen und das Ergebnis gespeichert wird.
- **Geprüft** – 13 gezielte Core-Tests zu Transform und `git diff --check`
  bestanden.
- **Status** – `SUCCESS`.

## Offener Integrationspunkt

Plan-Abhängigkeiten müssen den Transform-Auftrag und dessen Feldauswahl
an die Wartestufe der abhängigen Aufgabe binden. Die Worker-Schnittstelle
steht dafür bereit.
