# Allgemeine Adapter zuerst

Die nächste Entwicklungsphase baut allgemeine Adapterfähigkeiten aus.
Anschließend wird versucht, mit diesen Fähigkeiten die simulierte Drohne
zu steuern. Diese Reihenfolge ist verbindlich; die Drohnensuche bleibt das
erste große Anwendungsziel.

ARCS stellt zunächst den allgemeinen Harness für verlässliche Daten,
nachvollziehbare Zusammenhänge und passenden Reasoning-Kontext bereit.
Wahrnehmung und Lösungsverfahren werden über austauschbare Komponenten
angeschlossen. Vor weitergehender Autonomie steht eine durchgängige
Datengrundlage mit klarer Bedeutung, zeitlicher Zuordnung, Herkunft und
erkennbaren Unsicherheiten. Das verbindliche [Zielbild](../README.md#zielbild)
gilt unabhängig von der Drohnenanwendung.

## Vorhandene Grundlage

| Baustein | Stand |
| --- | --- |
| Gemeinsame IDs, Manifeste, Capability-Verträge und Nachrichten | `crates/arcs-adapter-protocol` |
| Grants, Schema-Prüfung und kontrollierte Ausführung | `crates/arcs-core` |
| Konfigurierbarer Host für mehrere externe Adapter | `apps/adapter-host` |
| Gemeinsame Adapter-Session für alle sieben Rollen | `crates/arcs-adapter-sdk` |
| Externe Eingaben mit dauerhaften Empfangsbelegen und eigenem ACK | `InputService::ingest_once` und `adapters/json-input` |
| Dauerhafte Input-/Observe-zu-Reason-Verarbeitung mit Wiederaufnahme | `arcs_host::task_runtime`, persistente Queue und Review-Liste |
| Allgemeine Aufgabensteuerung bis Outcome und erneuter Planung | Optionale [Goal-Routen](../apps/adapter-host/examples/goal-runtime.md), externe Planner und Bewertungs-Reasoner, explizite Betreiberfreigaben |
| Allgemeiner externer Reasoning-V2-Port | `GeneralReasoningService`, `Host::reason` und `adapters/example-reasoner` |
| Deterministische Verarbeitung mit Provenienz, Trust-Grenze und Replay | `TransformService` und `adapters/normalize-number` |
| Referenzierte Binärdaten und begrenzter Abruf unter separatem Grant | Protokoll, SDK, `DataService` und `Host::copy_data` |
| Explizite Binäreingaben für Reasoning V2 | `InputDataService`, `Host::reason_with_data`; getrennte Quell- und Empfangsrechte |
| Allgemeine Datenquelle mit persistenten Snapshots und Metadaten-ACK | `adapters/file-data` |
| Kontrollierte Ausgabe mit zielgebundener Freigabe und Empfangsbeleg | `OutputService` und `adapters/file-output` |
| Mehrstufige Observe → Transform → Output-Kette | Allgemeiner `arcs_host::pipeline`-Runner mit Definition, Checkpoints und Cursor; Beispielpolicy außerhalb des Runners |
| Externer Anwendungsnachweis mit simulierter Drohne | Eigenständiges Nachbarprojekt `../drone-sim`, angeschlossen über Observe/Request/Act |
| Geräteunabhängiger Kontext für Sensoraufnahmen | `CaptureContext` im Protokoll: Quelle, Uhr, Zustandsbezug, Koordinaten und Qualität |
| Externer Kamera-Nachweis mit gespeicherten Bildbytes und zugeordneter Pose | Konkreter Adapter und Renderer ausschließlich im Nachbarprojekt `../drone-sim` |
| Verschiedene Nutzer desselben SDK | Sensor, Lüfter, Dateiquelle, Zahlen-Normalisierer, JSON-Bericht und Dateiausgabe unter `adapters/`; Drohne im externen Projekt |

Das SDK übernimmt die wiederkehrende Protokollarbeit. Ein Treiber ergänzt
sein Manifest und die fachlichen Operationen. Derselbe Adapter kann mehrere
Rollen anbieten, etwa Beobachtungen, gezielte Abfragen und Actions.
Eine Fähigkeit benötigt deshalb keinen eigenen Prozess pro Rolle.

## Mehrere angeschlossene Komponenten pro Rolle

Wie bei mehreren an ein Betriebssystem angeschlossenen Geräten kann ARCS
mehrere eigenständige Installationen gleichzeitig verwalten. Dazu gehören
auch Softwaredienste, mathematische Solver, Regeln, LLMs und andere KI-Verfahren.
Keine der sieben Rollen ist ein Singleton. Eine Installation kann wiederum
mehrere Rollen anbieten.

Die Anwendung adressiert immer `AdapterId` plus lokaler `CapabilityId`.
`Host::adapter_catalog` zeigt alle angeschlossenen externen Installationen;
`Host::capabilities(AdapterRole)` findet sämtliche freigeschalteten Ziele einer
Rolle. Grants, Datenzugriff und Freigaben gelten jeweils für die konkrete
Installation. Der Katalog trifft keine Orchestrierungsentscheidung und gibt
keine zusätzliche Handlungsbefugnis.

Der [Mehrfachanschluss-Nachweis](../apps/adapter-host/examples/multiple-adapters.md)
verbindet elf echte Adapterprozesse und deckt jede Rolle mehrfach ab.
Gleiche lokale Gerätenamen und Fähigkeiten bleiben getrennt. Anschlüsse
werden aktuell beim Host-Start konfiguriert; dynamisches Hotplug und eine
allgemeine parallele Orchestrierung sind weitere Schritte.

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
echte Modellanbieter sind noch anzubinden. Die
[Aufgaben-Runtime](../apps/adapter-host/examples/task-runtime.md) verbindet
Input/Observe mit Reasoning über eine dauerhafte Queue und Review-Übergabe.
Der Drohnen-Bewegungstest bleibt ein vorhandener Anwendungsnachweis.

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
   Modell-Testadapter. Die allgemeine Runtime unterstützt inzwischen Dauerbetrieb
   mit serieller Verarbeitung; echte Modellanbieter folgen noch.
3. Referenzierte Binärdaten sind als allgemeiner Vertrag implementiert.
   Der blockweise Abruf berücksichtigt Frame-Limits und gesonderte
   Datenlimits des Grants. Die Dateiquelle und das Exportbeispiel weisen
   den vollständigen Prozessweg nach. Die externe simulierte Kamera
   verwendet denselben Vertrag für persistente Bildsnapshots. Ein externer
   Rules-Reasoner erhält über den allgemeinen Binäreingabe-Transport
   ausgewählte Bildbytes und einen getrennten Farbauftrag. Semantische
   Objekterkennung und Videostreaming fehlen noch.
4. Die vorhandenen Verträge sind inzwischen an einer einfachen Raum- und
   Drohnensimulation erprobt. Der Adapter liefert Pose und Bewegungsfähigkeit
   über Observe, Request und Act. Die Anwendung weist eine freigegebene
   Bewegung und deren separat beobachtete Wirkung nach. Die Fachverträge
   liegen im eigenständigen Projekt `../drone-sim`, außerhalb des ARCS-
   Repositorys und -Workspace. Kameradaten sind über Request und
   den bestehenden Datenabruf verfügbar. Der erste Pixel-Suchnachweis liegt
   ebenfalls extern; der Core enthält keine Bildauswertung.

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
[Drohnen-Bewegungsnachweis](../../drone-sim/apps/drone-demo/README.md) verbindet nun
Telemetrie und eine freigegebene Bewegung im einfachen Simulator.
Der [Kamera-Nachweis](../../drone-sim/adapters/simulated-drone/CAMERA.md) ergänzt inzwischen
Bildbytes und zugeordnete Aufnahmemetadaten. Der
[Pixel-Suchnachweis](../../drone-sim/adapters/image-reasoner/README.md)
verbindet die zielunabhängige Quelle mit einem separaten Rules-Reasoner.
Die Fachverträge bleiben bei den Komponenten.
