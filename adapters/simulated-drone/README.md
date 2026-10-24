# Persistenter Drohnenadapter für die Raumsimulation

`arcs-simulated-drone` verwendet das allgemeine Adapter-SDK und die separate
Bibliothek `simulation/room`. Das Paket hat keine Core-Abhängigkeit. Alle
fachlichen Schemas liegen unter `schemas/`.

```bash
cargo build --locked -p arcs-simulated-drone
./target/debug/arcs-simulated-drone \
  adapters/simulated-drone/config/profile.json /tmp/drone-state.sqlite
```

Das Programm spricht JSON-Lines über stdin/stdout und wird normalerweise
vom allgemeinen Host gestartet. Das Elternverzeichnis der Adapterdatenbank
muss existieren. Logs gehen nach stderr.

## Fähigkeiten

| Capability | Port | Vertrag | Permission |
| --- | --- | --- | --- |
| `pose.observe` | Observe | `arcs.state.drone_pose.v1` | `drone.read` |
| `pose.read` | Request | `arcs.request.drone_pose.v1` → Pose | `drone.read` |
| `drone.move` | Act | `arcs.drone_move_candidate.v1` → `arcs.execution_result.drone.v1` | `drone.move` |

Die Telemetrie enthält Drohnen-ID, Pose, feste Höhe und `revision`.
Initial ist die Revision 0. Jede angenommene Bewegung erhöht sie genau um 1.
Die Initialpose und jede neue Pose werden als Observation bis zum ACK
aufbewahrt. Ein wiederholtes ACK ist zulässig; unbekannte IDs werden abgewiesen.

Ein Bewegungsauftrag nennt die Drohnen-ID, `expected_revision` und eine
Verschiebung samt Drehung. Der Adapter akzeptiert nur einen materialisierten
Action-Payload mit passenden Ziel-, Approval-, Capability- und Schema-
Referenzen. Der Core prüft Grants und die Freigabekette vor dem Aufruf;
das SDK prüft Routing und Korrelation. Eine neue Invocation gegen eine
inzwischen überholte Pose wird abgewiesen. Die Bibliothek prüft zusätzlich
Raumgrenzen, Hindernisse und die gesamte Bewegungsstrecke.

Die Standardkonfiguration erlaubt höchstens 1 Meter und 90 Grad pro Befehl
in einem 6 × 4 Meter großen Raum bei 1 Meter Höhe. Kollisionen werden mit
0,2 Meter konservativem Sicherheitsabstand geprüft. Das Profil enthält ein
rechteckiges Hindernis. Geometrie und Vereinfachungen stehen in der
[Simulationsdokumentation](../../simulation/room/README.md).

## Dauerhaftes Verhalten

SQLite speichert das vollständige Profil, den Zustand, ein Invocation-Journal
und die Observation-Outbox. Zustand, Antwort und neue Observation einer
Bewegung committen atomar unter `synchronous=FULL`. Verliert der Host die
Antwort, liefert dieselbe Invocation nach Neustart dieselbe Rückgabe;
die Drohne bewegt sich nicht erneut. Geänderte Auftragsdaten unter derselben
Invocation-ID werden abgewiesen. Der Cache wird vor der Prüfung einer
inzwischen alten `expected_revision` gelesen.

Requests sind ebenfalls unveränderliche Snapshots. Dieselbe Invocation liefert
den damaligen Zustand; eine neue Messung braucht eine neue persistierte
Request-Eingabe. Requests erzeugen keine zusätzliche Observation. Nur der
initiale Zustand und erfolgreiche Bewegungen füllen die Outbox.

Ein geändertes Profil an einer bestehenden Adapterdatenbank wird als
Identitätskonflikt abgewiesen. Ein neues Szenario braucht eine eigene
Installation mit passenden Adapter-/Core-Daten und einer eigenen Adapter-ID,
wenn es denselben Core-Store verwendet. Auftrags- und Observation-IDs einer
Installation dürfen nicht mit einer zurückgesetzten Datenbank neu belegt
werden. Core- und Adapterdatenbank gehören zum selben Versuch.

Der allgemeine stdio-Host beendet die Verbindung nach einer Fehlerantwort.
Ein neuer Hostlauf ist dann erforderlich; der Adapter hat keine versteckte
Retry- oder Reconnect-Schleife. Eine Kollisionsablehnung bewegt die Drohne
nicht und erzeugt keine Erfolgsantwort.

## Nachweis

```bash
cargo test --locked -p arcs-simulated-drone
```

Tests prüfen ACK/Replay, veraltete Revisionen, Identitätskonflikte,
unzulässige Bewegungen, Hindernisse zwischen zwei freien Endpunkten und
persistente Request-Snapshots. Ein Prozess-Test beendet den Adapter nach
dem Bewegungs-Commit, ohne die Antwort zu lesen, und weist anschließend
genau eine Bewegung und die erhaltene Outbox nach.

Der vollständige Freigabe- und Wirkungsnachweis steht unter
[apps/drone-demo](../../apps/drone-demo/README.md).
