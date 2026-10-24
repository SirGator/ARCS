# Allgemeine Adapter zuerst

Die nächste Entwicklungsphase baut allgemeine Adapterfähigkeiten aus.
Anschließend wird versucht, mit diesen Fähigkeiten die simulierte Drohne
zu steuern. Diese Reihenfolge ist verbindlich; die Drohnensuche bleibt das
erste große Anwendungsziel.

## Vorhandene Grundlage

| Baustein | Stand |
| --- | --- |
| Gemeinsame IDs, Manifeste, Capability-Verträge und Nachrichten | `crates/arcs-adapter-protocol` |
| Grants, Schema-Prüfung und kontrollierte Ausführung | `crates/arcs-core` |
| Konfigurierbarer Host für mehrere externe Adapter | `apps/adapter-host` |
| Gemeinsame Adapter-Session für alle sieben Rollen | `crates/arcs-adapter-sdk` |
| Externe Eingaben mit dauerhaften Empfangsbelegen und eigenem ACK | `InputService::ingest_once` und `adapters/json-input` |
| Allgemeiner externer Reasoning-V2-Port | `GeneralReasoningService`, `Host::reason` und `adapters/example-reasoner` |
| Deterministische Verarbeitung mit Provenienz, Trust-Grenze und Replay | `TransformService` und `adapters/normalize-number` |
| Referenzierte Binärdaten und begrenzter Abruf unter separatem Grant | Protokoll, SDK, `DataService` und `Host::copy_data` |
| Allgemeine Datenquelle mit persistenten Snapshots und Metadaten-ACK | `adapters/file-data` |
| Kontrollierte Ausgabe mit zielgebundener Freigabe und Empfangsbeleg | `OutputService` und `adapters/file-output` |
| Mehrstufige Observe → Transform → Output-Kette | Allgemeiner `arcs_host::pipeline`-Runner mit Definition, Checkpoints und Cursor; Beispielpolicy außerhalb des Runners |
| Anwendungsnachweis mit simulierter Drohne | `simulation/room`, `adapters/simulated-drone` und `apps/drone-demo` verwenden die bestehenden Observe-/Request-/Act-Verträge |
| Verschiedene Nutzer desselben SDK | Sensor, Lüfter, Dateiquelle, Zahlen-Normalisierer, JSON-Bericht, Dateiausgabe und simulierte Drohne unter `adapters/` |

Das SDK übernimmt die wiederkehrende Protokollarbeit. Ein Treiber ergänzt
sein Manifest und die fachlichen Operationen. Derselbe Adapter kann mehrere
Rollen anbieten, etwa Beobachtungen, gezielte Abfragen und Actions.
Eine Fähigkeit benötigt deshalb keinen eigenen Prozess pro Rolle.

Geräteabhängige Übersetzung, Persistenz und Idempotenz bleiben explizite
Treiberaufgaben. Grants und Freigaben kommen vom Host/Core. Das SDK
erzeugt keine Autorität und prüft keine Suchstrategie.

## Vorgabe für Reasoning

Reasoning soll mehrere mathematische Solver, regelbasierte Verfahren,
LLMs und andere KI-Verfahren über einen gemeinsamen Anschluss unterstützen.
Ein Modellanbieter ist nur eine mögliche Implementierung. Der neue V2-Port
hat dafür neutrale Ressourcen- und Herkunftsverträge; V1 bleibt für den
bisherigen Modellfall erhalten. Stand und Nachweise stehen im
[Reasoning-Entwurf](REASONING.md).

Input und Reasoning sind jetzt ebenfalls über stdio angeschlossen. Ein
mathematischer Solver und ein Modell-Testadapter weisen den V2-Port nach;
echte Modellanbieter sind noch anzubinden. Vor weiterer Automatisierung
folgt die dauerhaft laufende Orchestrierung. Der Drohnen-Bewegungstest
bleibt ein vorhandener Anwendungsnachweis.

## Weitere Reihenfolge

1. Die gemeinsame Basis mit den bestehenden Sensor- und Lüfterfällen
   nachweisen: Observe mit ACK und Replay, Request und freigegebene Act-Aufträge,
   korrelierte Fehler sowie abgewiesene falsche Capabilities.
2. Transform ist inzwischen durchgängig implementiert: persistierte Eingabe,
   externer Aufruf, geprüfte Ausgabe, Provenienz und dauerhaftes Replay.
   Der erste Nachweis normalisiert Zahlen mit unterschiedlichen Parametern
   und Einheiten. Output ist ebenfalls durchgängig umgesetzt: gebundener
   Ausgabeauftrag, separate Prüfung und Freigabe, externer Treiber und
   dauerhafter Beleg. Die Dateiausgabe weist Idempotenz und Recovery nach.
   Eine konfigurierte Beispielanwendung verbindet inzwischen drei echte
   Prozesse durchgängig: Sensorbeobachtung, allgemeine JSON-Berichtsdarstellung
   und freigegebene Dateiausgabe. Zwei Eingabeschemas, niedriger Eingangstrust,
   Quellenauswahl, Neustart und wirksame Ablehnung sind getestet. Die
   Exportpolicy liegt ausdrücklich in der Anwendung, nicht im Host/Core.
   Der Runner unterstützt inzwischen mehrere geordnete Transform-Schritte,
   prüft die gesamte Kette vor dem Polling und bindet deren Manifeste an
   eine feste Pipeline-ID. Persistierte Checkpoints und ein Cursor ermöglichen
   Wiederaufnahme und Verarbeitung neuer Beobachtungen. Ein Fehler im zweiten
   Transform mit späterer Wiederaufnahme ist als Prozess-Test nachgewiesen.
   Input und Reason sind inzwischen auch über stdio umgesetzt. Input besitzt
   getrennte ACKs und dauerhafte Empfangsbelege ohne World-Update. Reasoning
   bietet einen neutralen V2-Port und einen Nachweis mit externem Solver und
   Modell-Testadapter. Echte Modellanbieter und Dauerbetrieb folgen noch.
3. Referenzierte Binärdaten sind als allgemeiner Vertrag implementiert.
   Der blockweise Abruf berücksichtigt Frame-Limits und gesonderte
   Datenlimits des Grants. Die Dateiquelle und das Exportbeispiel weisen
   den vollständigen Prozessweg nach. Eine spätere Kameraquelle kann
   denselben Vertrag verwenden; Bildanalyse und Videostreaming fehlen noch.
4. Die vorhandenen Verträge sind inzwischen an einer einfachen Raum- und
   Drohnensimulation erprobt. Der Adapter liefert Pose und Bewegungsfähigkeit
   über Observe, Request und Act. Die Anwendung weist eine freigegebene
   Bewegung und deren separat beobachtete Wirkung nach. Die Fachverträge
   bleiben außerhalb des Core. Kameradaten und Wahrnehmung folgen später
   über die allgemeinen Daten- und Transform-Verträge.

Die Reihenfolge weiterer Rollen richtet sich nach einem konkreten Bedarf;
der Beginn der Drohnenerprobung erfordert nicht die Implementierung jeder
denkbaren Adapterrolle. Die bereits benötigten Rollen müssen allgemein
nutzbar und unabhängig nachgewiesen sein.

## Abnahme der gemeinsamen Basis

- SDK und Protokoll besitzen keine Core-, Sensor-, Lüfter- oder
  Drohnenabhängigkeit.
- Die gleichen Session- und Routingmechanismen bedienen Sensor und Lüfter.
- Ein neuer fachlicher Adapter implementiert Treiberoperationen; er benötigt
  keine Kopie der Protokollschleife und keine Gerätefallunterscheidung im Core.
- Bestehende Grants, Approval-Grenzen und die Wiederholungssemantik bleiben
  wirksam. Die vorhandenen Prozess- und Neustarttests müssen weiterhin bestehen.

Details zur Implementierung stehen in der
[SDK-Dokumentation](../crates/arcs-adapter-sdk/README.md), der langfristige
Anwendungsnachweis im [Plan für die Drohnensuche](DRONE_SEARCH.md).
Der [Transform-Nachweis](../adapters/normalize-number/README.md) beschreibt
die neue allgemeine Verarbeitungsgrenze und ihre aktuellen Einschränkungen.
Die [Pipeline-Anleitung](../apps/adapter-host/examples/observe-report/README.md)
zeigt die vollständige Kette. Der
[Drohnen-Bewegungsnachweis](../apps/drone-demo/README.md) verbindet nun
Telemetrie und eine freigegebene Bewegung im einfachen Simulator.
Kamera und Wahrnehmung folgen danach; ihre Fachverträge bleiben bei den Komponenten.
