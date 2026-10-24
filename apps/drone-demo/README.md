# Erster Drohnennachweis: beobachten, freigeben, bewegen, Wirkung prüfen

Diese Anwendung führt einen einzelnen ausdrücklich konfigurierten
Bewegungszyklus durch den allgemeinen Host aus. Sie enthält die fachliche
Policy; der Core und der allgemeine Host enthalten kein Drohnenwissen.
Eine Kamera und die autonome Suche sind noch nicht implementiert.

## Start

Aus dem Cargo-Workspace:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-drone-demo -- \
  apps/drone-demo/config/host.json apps/drone-demo/config/flight.json
```

Die Drohne startet bei **(1, 1)** und bewegt sich nach Freigabe auf
**(1,5, 1)** Meter. Höhe und Blickrichtung bleiben bei 1 Meter und 0 Grad.
Die Revision steigt von 0 auf 1. Der JSON-Bericht enthält `status: success`,
Vorher-/Nachher-Telemetrie und alle beteiligten Artefaktversionen.
Die anfängliche Pose und die Pose nach der Bewegung liegen im World-State-
Log; der aktuelle Estimate enthält die neue Position.

Die getrennten Datenbanken `var/drone-core.sqlite` und
`var/drone-adapter.sqlite` sind ignorierte Laufzeitdaten. Absolute Pfade zu
beiden Konfigurationsdateien erlauben den Start aus anderen Verzeichnissen.
Die Dateipfade der Hostkonfiguration beziehen sich auf deren Verzeichnis.

## Ablauf und Zuständigkeiten

```text
persistierter Zyklus mit Bewegung und Policy
    → initiale Pose beobachten und gezielt abfragen
    → Candidate mit Pose-Revision und Evidenz
    → Verification → explizites System-Approval
    → materialisierte Action → externer Drohnenadapter → Simulation
    → separate Pose-Abfrage und gespeicherte neue Observation
    → Outcome mit Referenzen auf beide Wirkungsbelege
```

`config/flight.json` nennt Drohne, Adapter, Cycle-ID, Verschiebung,
Drehung, `approve` und die Grenzen für automatische Freigaben. Der Start
der Anwendung aktiviert ausdrücklich `demo.drone.bounded-move.v1`.
Die Beispielpolicy erlaubt höchstens die konfigurierten 0,75 Meter und
45 Grad, prüft Ziel, Ausgangsrevision und persistierte Evidenz und erteilt
bei `approve: true` ein System-Approval. Sie behauptet keine menschliche
Freigabe. `approve: false` erzeugt eine Ablehnung ohne Action.

Die Policy liest weder die Hinderniskarte noch einen versteckten Zielzustand.
Wände und Hindernisse werden vom Adapter beziehungsweise der Simulation
geprüft. Ein innerhalb der Policy liegender Befehl kann deshalb an einer
Kollision scheitern. Dann endet der Aufruf mit Fehler, ohne Bewegung oder
Erfolgsmeldung. Eine zu große Bewegung wird bereits bei der Verification
abgelehnt und erhält den Status `verification_failed`.

Das Outcome vergleicht die verlangte Positionsänderung mit dem
Ausführungsbeleg, einer separaten Request-Antwort und einer persistierten
Observation nach der Bewegung. Eine bloße Befehlsannahme genügt nicht.
Das ist eine Prüfung innerhalb der Simulation, keine unabhängige Messung
eines realen Fluggeräts.

Die normale `arcs-host`-CLI mit `config/host.json` liest ausschließlich
ausstehende Telemetrie. Ein Grant für `drone.move` löst allein keine Bewegung
aus. Die Anwendung nutzt die bestehenden allgemeinen Dienste für Proposal,
Verification, Approval, Action, Execution und Outcome.

## Wiederholung und neue Bewegungen

Eine Cycle-ID bezeichnet einen dauerhaften Auftrag. Dieselbe Konfiguration
verwendet nach einem Neustart dieselben Query-, Candidate-, Approval-,
Action-, Ergebnis- und Outcome-Versionen. Es gibt keine zweite Bewegung.
Die Request-Antworten sind dabei historische Snapshots dieses Zyklus;
ein Replay ist keine frische Positionsmessung.

Für eine weitere Bewegung wird `cycle_id` geändert, zum Beispiel auf
`move-2`. Damit erzeugt die Anwendung neue Vorher-/Nachher-Abfragen, übernimmt
die aktuelle Revision und führt eine neue Freigabekette aus. Geänderte
Bewegungs- oder Policyparameter unter derselben Cycle-ID werden als
Identitätskonflikt abgewiesen. Eine einmalige Ablehnung wird nicht durch
eine geänderte Konfiguration unter derselben ID ersetzt.

Der Adapter schützt neue Befehle zusätzlich mit `expected_revision` vor
veralteten Ausgangszuständen. Ein bereits erfolgreich ausgeführter Auftrag
bleibt über sein Journal wiederholbar, auch wenn die aktuelle Revision
weiter fortgeschritten ist. Grants prüft der Core auch vor einem Replay.
Nach Adapterfehlern ist ein neuer Hostlauf erforderlich; automatische
Reconnects und Retries sind nicht Teil dieses begrenzten Beispiels.

## Projektordnung und Grenzen

| Ort | Verantwortung |
| --- | --- |
| `simulation/room` | Reine Geometrie und deterministische Bewegung |
| `adapters/simulated-drone` | Fachverträge, stdio-SDK, persistenter Zustand, Outbox und Journal |
| `apps/drone-demo` | Bewegungsauftrag, lokale Freigabepolicy und Wirkungsprüfung |
| `apps/adapter-host` | Allgemeiner Prozesszugang, einschließlich `request_version` für gespeicherte Query-Eingaben |
| `crates/arcs-core` | Bestehende allgemeine Kontroll- und Speicherdienste, unverändert |

Die Simulation arbeitet ohne grafische Anzeige, Zeitachse und realistische
Flugphysik in einer festen Höhe. Bilder, Objekterkennung und Suchplanung
fehlen noch. Der nächste fachliche Schritt ist eine simulierte Kamera mit
Bilddaten über den vorhandenen allgemeinen Datenreferenzvertrag.

```bash
cargo test --locked -p arcs-drone-demo
cargo test --locked -p arcs-room-sim -p arcs-simulated-drone
```

Die Prozess-Tests prüfen erfolgreiche Bewegung und Wirkungsbelege, Neustart
ohne zweite Bewegung, wirksame Ablehnungen und Policygrenzen, Kollisionen,
geänderte Cycle-Identitäten, neue Positionsabfragen sowie entzogene
Capabilities. Simulation und Adapter besitzen zusätzliche Geometrie-,
Journal- und Prozessabbruchtests.
