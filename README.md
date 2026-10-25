# ARCS

**ARCS – Artifact Reasoning and Control System** ist ein experimentelles
Agenten-Framework in Rust. Es soll einen möglichst allgemeinen, fähigen und
günstigen Agenten ermöglichen, ohne Reasoning direkt mit Handlungsbefugnis zu
verwechseln.

ARCS bildet den allgemeinen Harness: Er nimmt externe Daten entgegen,
validiert und speichert sie als unveränderliche Artefakte, ordnet ihre
Zusammenhänge und stellt passenden Kontext für Reasoning bereit. Analysen,
Planung und Lösungsvorschläge entstehen durch angeschlossene Solver, Modelle
und andere Reasoning-Verfahren. Bekannte Ergebnisse können wiederverwendet
werden. Vorschläge werden vor einer Wirkung verifiziert, freigegeben und
über kontrollierte Actions oder Outputs umgesetzt.

> **Projektstatus:** frühe Entwicklung (`0.1.0`). Der erste vertikale
> World-Model-Slice und die kontrollierte Proposal-to-Action-Pipeline sind
> implementiert. Goals, eigenständige Skills und ein gelerntes
> kostenoptimierendes Routing sind noch Roadmap.

## Projektstruktur

ARCS ist ein Cargo-Workspace mit getrennten Paketen:

```text
ARCS/
├── Cargo.toml                       Workspace und gemeinsame Abhängigkeitsversionen
├── crates/
│   ├── arcs-core/                   Allgemeine Logik, Store und Kontrolle
│   │   ├── src/                     World, Reasoning, Decision, Action, Runtime …
│   │   └── schemas/                 Versionierte allgemeine Artefakt-Schemas
│   ├── arcs-adapter-protocol/       IDs, Manifeste, Nachrichten und JSON-Framing
│   └── arcs-adapter-sdk/            Gemeinsame externe Adapter-Session
├── adapters/
│   ├── simulated-sensor/           Eigenes Cargo-Paket und ausführbarer Adapter
│   │   ├── src/
│   │   ├── config/profile.json
│   │   └── schemas/
│   ├── simulated-fan/              Eigenes Cargo-Paket mit persistentem Simulator
│   │   ├── src/
│   │   ├── config/profile.json
│   │   └── schemas/
│   ├── file-data/                  Allgemeine Dateiquelle mit persistenten Snapshots
│   │   ├── src/
│   │   └── config/profile.json
│   ├── normalize-number/           Deterministischer Transform mit eigenen Schemas
│   ├── json-report/                Schemaunabhängige JSON-Darstellung als Bericht
│   ├── json-input/                 Externe JSON-Ereignisse mit dauerhafter ACK-Outbox
│   ├── example-reasoner/           Mathematischer Solver und separater Modell-Testadapter
│   └── file-output/                Idempotente Ausgabe mit Journal und JSON-Berichten
├── apps/
│   ├── adapter-host/               Allgemeiner Host mit JSON-Konfiguration
│   ├── control-panel/              Natives Desktop-Kontrollpanel für Start, Status und Adapter
│   │   ├── src/                    CLI und Bibliothek ohne Geräteabhängigkeit
│   │   ├── config/                 Programme, Profile, Schemas, Grants und Abfragen
│   │   └── tests/                  Prozess- und Konfigurationstests
│   └── demo-host/                  Beispielanwendungen, Regeln und System-Freigabe
│       ├── src/                    Minimale Demo sowie Sensor- und Lüfter-Host
│       ├── config/                 Betreiber-Grants
│       └── tests/                  Tests mit echten externen Adapterprozessen
└── config/                         Historische C++/Worker-Beispiele; nicht geladen
```

Kamera, Drohne und Raum-Simulation gehören zum getrennten lokalen Nachbarprojekt
[`../drone-sim`](../drone-sim/README.md), mit eigenem Git-Repository und
Cargo-Workspace. ARCS baut und testet unabhängig davon. Der allgemeine Host
schließt dessen ausführbaren Adapter ausschließlich per stdio und
Betreiberkonfiguration an; auch die fachlichen Schemas liegen beim externen
Projekt. Im ARCS-Protokoll bleiben nur die geräteunabhängigen Verträge.

Auch die Anbindung externer LLM-APIs liegt im getrennten Nachbarprojekt
[`../llm-api`](../llm-api/README.md). Der Host schließt dessen Planner- und
Bewertungsinstanzen über denselben Reasoning-V2-Vertrag an. Ein
[Textaufgaben-Beispiel](apps/adapter-host/examples/llm-tasks.md) verbindet die
Modellvorschläge mit der vorhandenen kontrollierten Aufgabensteuerung.

Sensor und Lüfter nutzen das gemeinsame Protokoll, das Adapter-SDK und ihre
eigenen Bibliotheken. Der Lüfter nutzt SQLite für seinen simulierten
Gerätezustand. Kein Adapter hängt von `arcs-core` ab. Auch der Core hat keine
Abhängigkeit von konkreten Adaptern. Das Protokoll benötigt nur Serde/JSON.

`crates/arcs-core/src/adapters/` enthält die **Core-seitige Anschlussstelle**:
Registry, Betreiber-Grants, Ports und den Host-Transport. Dort liegt kein
konkreter Geräteadapter. Die öffentliche Rust-Bibliothek des Pakets
`arcs-core` heißt weiterhin `arcs`; vorhandene Modulpfade bleiben erhalten.
Gemeinsame Nachrichten werden dafür aus `arcs-adapter-protocol` reexportiert.

`crates/arcs-adapter-sdk` übernimmt auf der Adapterseite Anmeldung,
Nachrichtenaustausch, Routing und Fehlerantworten. Sensor und Lüfter nutzen
dieselbe Implementierung. Das SDK ist vom Core und von konkreten Geräten
unabhängig; Treiber ergänzen ihre fachlichen Operationen.

Die Anwendung `apps/demo-host` verbindet die Pakete. Sie benutzt die
fachlichen Schema-/Profiltypen der Adapter-Bibliotheken; deren eigentliche
Treiber laufen weiterhin in separaten Prozessen. Schwellenwerte,
Verifikation und Freigabepolicy des Beispiels werden vom Demo-Host angewendet.

`apps/adapter-host` ergänzt den allgemeinen Host. Er hängt von keinem
konkreten Adapter-Paket ab und lädt die gesamte Anschlusskonfiguration aus
JSON. Profile bleiben beim jeweiligen Adapter; fachliche Steuerungsregeln
und die Erteilung von Approvals bleiben in der Anwendung.

`adapters/file-data` ergänzt eine allgemeine Quelle für beliebige Dateiinhalte.
Der Adapter speichert unveränderliche Snapshots, veröffentlicht kleine
Metadaten-Referenzen und liefert Inhalte blockweise. Der Host benötigt dafür
eine gesonderte Datenfreigabe. Das funktioniert mit Bildern ebenso wie mit
anderen Binärdaten. Konkrete Kamera und Bildauswertung liegen im externen
Nachbarprojekt `drone-sim`.
Start und Export stehen in der [Dateiquellen-Dokumentation](adapters/file-data/README.md).

`adapters/normalize-number` demonstriert den allgemeinen Transform-Port:
Eine persistierte Eingabe wird extern verarbeitet, das Ergebnis validiert
und mit seiner Herkunft gespeichert. Parameter und Rechenregel bleiben
außerhalb des Core. Die [Transform-Dokumentation](adapters/normalize-number/README.md)
beschreibt Start, Grants und Replay.

`adapters/file-output` ergänzt eine kontrollierte Ausgabesenke. Der allgemeine
Core bindet Inhalt und Ziel an einen Ausgabeauftrag, prüft Verification und
Approval und persistiert den Empfangsbeleg. Der Adapter übernimmt die konkrete
Dateiveröffentlichung und Wiederherstellung nach verlorenen Antworten.
Die [Dateiausgabe-Dokumentation](adapters/file-output/README.md) enthält einen
vollständigen Nachweis mit ausdrücklich benannter lokaler Beispielpolicy.

Das [Pipeline-Beispiel](apps/adapter-host/examples/observe-report/README.md)
verbindet drei echte Prozesse: Sensorbeobachtung → JSON-Bericht → freigegebene
Dateiausgabe. Quelle, Transform, Ziel und Schema-Übergänge stehen in JSON;
die Exportpolicy bleibt in der Anwendung. Wiederholte Läufe verwenden die
gespeicherten Checkpoints und verarbeiten nur ausstehende Beobachtungen.
Der allgemeine Runner `arcs_host::pipeline::run_pipeline` unterstützt mehrere
explizite Transform-Schritte, prüft alle Schema-Übergänge vor dem Polling
und setzt unterbrochene Eingaben über die dauerhaften Core-Aufrufe fort.
Der Adapter `adapters/json-report` kann auch andere deklarierte JSON-Verträge
darstellen und enthält kein Sensor- oder Drohnenwissen.

Das [Input-/Reasoning-Beispiel](apps/adapter-host/examples/reason-task.md)
verbindet eine persistente JSON-Eingabe mit zwei unabhängigen Reasoner-Prozessen:
einem mathematischen Solver und einem ausdrücklich aufgezeichneten Modelltest.
Es zeigt Kontextauswahl, unterschiedliche Trust-Herkunft und Wiederaufnahme
anhand derselben Artefakte. Ein echter Modellanbieter ist noch nicht angebunden.

Die [Aufgaben-Runtime](apps/adapter-host/examples/task-runtime.md) verbindet
Input oder Observe dauerhaft mit konfigurierten Reasonern. Sie hält eine
persistente Aufgabenqueue, setzt nach einem Neustart fort und stellt fertige
Kandidaten zur Prüfung bereit. Neue JSON-Aufgaben lassen sich während des
laufenden Hosts einreihen. Mit der optionalen [Zielsteuerung](apps/adapter-host/examples/goal-runtime.md)
koordiniert sie außerdem Vorschlag, Verification, manuelle oder konfigurierte
Freigabe, Aktion, frische Wirkungsdaten und erneute externe Planung bis zum
Zielabschluss oder Budgetende. Die fachlichen Regeln bleiben in den Komponenten.

Der erste [externe Drohnen-Bewegungsnachweis](../drone-sim/apps/drone-demo/README.md) verbindet
eine separate Raum-Simulation mit einem externen Drohnenadapter und den
bestehenden allgemeinen Kontrollmechanismen. Er liest die Pose, führt eine
begrenzt freigegebene Bewegung aus und prüft die neue Position anhand einer
separaten Abfrage und einer gespeicherten Observation. Zustand, Outbox und
Bewegungsjournal überstehen Neustarts ohne zweite Bewegung. Die autonome
Suche ist noch nicht implementiert.

Der [externe Kamera-Nachweis](../drone-sim/adapters/simulated-drone/CAMERA.md) liefert inzwischen
perspektivische Bilder mit geometrischer Verdeckung. Aufnahmekontext, Pose,
Kameraparameter und Zustandsrevision gehören zu einem atomar gespeicherten
Snapshot; Bildbytes bleiben nach Bewegungen und Neustarts abrufbar. Der
allgemeine Aufnahmekontext liegt im Protokoll; Kamera, Szene und
Simulationscode liegen im eigenständigen Projekt außerhalb von ARCS. Ein
[separater Pixel-Reasoner](../drone-sim/adapters/image-reasoner/README.md)
erhält Bildbytes und einen getrennten Suchauftrag über den allgemeinen
Binäreingabe-Vertrag. Die Kamera kennt das Suchziel nicht.

Die Komponenten lassen sich innerhalb des Workspace getrennt bauen:

```bash
cargo build --locked -p arcs-simulated-sensor
cargo build --locked -p arcs-simulated-fan
cargo test --locked -p arcs-core
```

Ein neuer Adapter kann ein weiteres Paket unter `adapters/` oder ein externes
Programm in einer anderen Sprache sein. Er implementiert die JSON-Verträge;
eine Änderung am Core ist dafür nicht erforderlich.

## Zielbild

**Langfristiges Ziel ist ein vielseitiges Agentensystem, das unterschiedliche
Aufgaben mit austauschbarer Wahrnehmung, Reasoning und ausführbaren Fähigkeiten
bearbeiten kann.** ARCS liefert dafür den Harness zur Datenzusammenführung,
Kontextbereitstellung, Koordination und kontrollierten Ausführung.

**Leistungsfähigkeit und einfache Bedienung sind gleichrangige Ziele.**
Eine gemeinsame Desktop-Oberfläche soll Aufgaben, angeschlossene Komponenten,
Fortschritt, nötige Freigaben und Ergebnisse verständlich zugänglich machen.
Der Nutzer beschreibt sein Ziel; ARCS organisiert den Datenaustausch, die
Zusammenarbeit der externen Verfahren und den weiteren Aufgabenablauf.
Die Oberfläche soll die Einrichtung, den Start und die Verwaltung der
Anbindungen übernehmen. Für die normale Nutzung sollen verständliche
Voreinstellungen und die Oberfläche genügen. Eigene Programme, Adapter oder
manuell bearbeitete Konfigurationsdateien sollen dafür nicht erforderlich sein.

**Erweiterung und Anpassung bleiben optional zugänglich.** Erfahrene Nutzer
sollen eigene Adapter und Reasoning-Verfahren anschließen, Abläufe und
Freigaberegeln konfigurieren sowie Komponenten austauschen können. Zusätzliche
Expertenansichten und dokumentierte Schnittstellen erschließen diese
Möglichkeiten, ohne sie zur Voraussetzung für die normale Bedienung zu machen.

**Selbstständiger Fähigkeitsausbau ist ein langfristiges Ziel.** Wenn für eine
Aufgabe eine passende Fähigkeit fehlt, soll ARCS externe Reasoner mit dem
Entwurf eigener Werkzeuge, Adapter, Programme oder Abläufe beauftragen können.
ARCS soll deren Erstellung in einer isolierten Arbeitsumgebung koordinieren,
Builds und Tests über angeschlossene Fähigkeiten ausführen lassen und geprüfte
Ergebnisse versioniert einbinden können. Später können auch vorgeschlagene
Änderungen an ARCS selbst diesen Ablauf durchlaufen.

Erzeugter Code und neue Fähigkeiten bleiben Vorschläge, bis die konfigurierten
Prüfungen und Freigaberegeln erfüllt sind. Die Betreiberkonfiguration bestimmt,
welche Schritte automatisch erlaubt sind und welche eine manuelle Freigabe
benötigen. Aktivierungen sollen nachvollziehbar und rücknehmbar sein. Entwurf
und fachliche Bewertung bleiben bei externen Komponenten; ARCS organisiert den
Ablauf. Auch dieser Fähigkeitsausbau soll über die normale Oberfläche nutzbar
sein. Die allgemeine Build-, Test- und Einbindungskette ist noch umzusetzen.

Modelle, APIs, Datenquellen und Geräte sollen sich über diese Oberfläche
anschließen lassen. Bei einem Modelldienst gehören dazu Anbieter beziehungsweise
API-Format, Endpunkt, Modell und die geschützte Zugangskonfiguration. Die
Übersetzung und das fachliche Reasoning bleiben bei austauschbaren externen
Komponenten, auch wenn ARCS ihre Einrichtung und Prozesse im Hintergrund
verwaltet. Die Seite **Modelle** im Kontrollpanel bietet inzwischen die
Einrichtung, den Verbindungstest und getrennte Zuordnung für Planung,
Wirkungsprüfung und optional Bildauswertung. Die lokale Umgebung
[model-services](../model-services/README.md) betreibt Ollama und YOLO als
eigene API-Dienste; Modellgewichte und Inferenz bleiben außerhalb von ARCS.
Schlüssel bleiben im geöffneten Fenster oder werden optional
im Systemschlüsselbund gespeichert. Eigene Profile und Umgebungsvariablen
bleiben zusätzlich verfügbar.

**Aufgaben sollen hauptsächlich als Text eingegeben werden.** ARCS bewahrt die
Zielbeschreibung und reicht sie mit ausgewähltem Kontext an externe Reasoner
weiter. Interpretation und fachliche Planung bleiben dort; Formulare und JSON
sind zusätzliche technische Eingänge. Der [Textaufgaben-Nachweis](apps/adapter-host/examples/text-tasks.md)
verbindet dies bereits mit einem begrenzten externen Rules-Reasoner.

**Die verlässliche Datengrundlage hat zunächst Vorrang.** Daten sollen fachlich
verständlich, zeitlich zuordenbar und mit nachvollziehbarer Herkunft,
Qualität und Unsicherheit verfügbar sein. Fehlende, widersprüchliche oder
veraltete Informationen müssen erkennbar bleiben. Die Schema-Prüfung ist
dabei ein Teil der Datenqualität. Der Harness stellt den angeschlossenen
Reasonern den für die Aufgabe erforderlichen Kontext bereit; konkrete
Wahrnehmung und Lösungsverfahren bleiben austauschbare Komponenten.

**Universalität ist eine verbindliche Architekturvorgabe.** ARCS bleibt ein
allgemeines Agenten-Framework. Die Drohnensuche dient als erster großer
Anwendungs- und Bewährungsfall. Allgemeine Zielverfolgung, Beobachtung,
Planung und kontrollierte Ausführung sollen auch für andere Aufgaben nutzbar
sein; Gerätewissen und aufgabenspezifische Regeln bleiben austauschbar.

Das gilt ausdrücklich auch für Reasoning: Mehrere mathematische Solver,
Regelverfahren, LLMs und andere KI-Modelle sollen nebeneinander anschließbar
sein. Reasoning ist die allgemeine Lösungsrolle; die neue
V2-Schnittstelle unterstützt diese Verfahren. Ein mathematischer Solver
und ein Modell-Testadapter weisen sie nach. Das externe Projekt `llm-api`
ergänzt konfigurierbare Anbindungen für lokale Modelle und entfernte
Modelldienste. Der Prozessweg ist mit lokalen API-Testservern geprüft;
ein Test mit einem echten Modell steht nach Einrichtung des Zugangs noch aus.
Den verbindlichen Ausbau beschreibt der
[Reasoning-Entwurf](docs/REASONING.md).

Wie bei mehreren an ein Betriebssystem angeschlossenen Geräten können alle
sieben Adapter-Rollen mehrfach vertreten sein. Anschlüsse umfassen Geräte,
Datenquellen und unterschiedliche Software-/Reasoning-Verfahren. Jede
Installation hat eine eigene Adapter-ID und eigene Grants; lokale
Capability-Namen dürfen sich wiederholen. Der
[Anschlusskatalog und Prozessnachweis](apps/adapter-host/examples/multiple-adapters.md)
zeigt diese Auswahl mit mehreren Instanzen jeder Rolle.

Das erste große Anwendungsziel ist: **ARCS steuert eine simulierte Drohne
durch einen anfangs unbekannten simulierten Raum ohne vorgegebene Karte und
findet darin ein vorgegebenes Zielobjekt mithilfe der Drohnenkamera.**

Die Drohne soll dafür wiederholt beobachten, eine Suchentscheidung treffen,
eine freigegebene Bewegung ausführen und das Ergebnis erneut beobachten.
Ein Fund muss auf Kamerabildern beruhen und mit gespeicherter Evidenz
nachvollziehbar sein. Drohnenadapter, Kamera, Simulation und fachliche Demo
liegen im eigenständigen Nachbarprojekt `drone-sim`, außerhalb des
ARCS-Repositorys und -Workspace. Aktuell sind die allgemeine Adapteranbindung
und die kontrollierte Action-Pipeline sowie eine einfache Drohnenbewegung in einer Raumsimulation
vorhanden. Eine optionale simulierte Kamera liefert zugeordnete Bildsnapshots.
Ein externer Rules-Reasoner weist sichtbare und fehlende Farbsignale anhand
der übertragenen Pixel nach. Semantische Objekterkennung und autonome
Suchsteuerung sind noch nicht implementiert.

Etappen, Zuständigkeiten und Erfolgskriterien stehen im
[Plan für die Drohnensuche](docs/DRONE_SEARCH.md).
Die Entwicklungsreihenfolge lautet: zuerst die
[allgemeinen Adapterfähigkeiten](docs/GENERAL_ADAPTERS.md) ausbauen und
nachweisen, anschließend damit die Drohne anbinden.

ARCS betrachtet Agentenverhalten als Optimierungsproblem. Neben dem
Aufgabennutzen gehören Rechenkosten, Latenz und Risiko ausdrücklich zur
Zielfunktion:

$$
\pi^* = \arg\max_\pi
\mathbb{E}_\pi\left[
\sum_{t=0}^{T}
\left(R_t-\lambda_C C_t-\lambda_L L_t-\lambda_R \rho_t\right)
\right]
$$

Dabei bezeichnet $R_t$ den erwarteten Nutzen, $C_t$ die Rechenkosten, $L_t$
die Latenz und $\rho_t$ das erwartete Fehlerrisiko. Die Gewichte $\lambda_C$,
$\lambda_L$ und $\lambda_R$ machen die gewünschte Abwägung explizit und
konfigurierbar.

Die angestrebte Agentenschleife lautet:

```text
Observation → Belief Update → Goal → Memory/Skill Retrieval
            → Reasoner Selection → Planning → Verification
            → Approval → Action → Outcome Update
```

## Was heute implementiert ist

| Bereich | Stand |
| --- | --- |
| Versionierte Artefakte, Schemas, Provenance und Trust | Implementiert |
| Adapter-Manifeste und getrennte Betreiber-Grants | Implementiert |
| Mehrere Installationen aller sieben Rollen und Anschlusskatalog | `Host::adapter_catalog`, `Host::capabilities` und CLI `--list-adapters`; Prozess- und Neustartnachweis |
| Gemeinsames Adapter-SDK für Input, Observe, Request, Transform, Reason, Act und Output | Implementiert, einschließlich neutralem Reasoning-V2-Port |
| Transform mit geprüften Ein-/Ausgaben, Provenienz und persistentem Replay | Implementiert, allgemeiner Zahlen-Normalisierer als Beispiel |
| Output mit gebundenem Auftrag, Verification, Approval und Beleg | Implementiert, Dateiausgabe mit Journal und Recovery |
| Mehrstufige Observe → Transform → Output-Pipelines | Allgemeiner Host-Runner mit gebundener Definition, Checkpoints und Cursor; Freigabepolicy in der Anwendung |
| Externer Raum- und Drohnennachweis | Im getrennten Projekt `../drone-sim`: Bewegung, Kollisionsprüfung und freigegebene Action mit Wirkungsprüfung |
| Referenzierte Binärdaten und explizite Reasoning-V2-Eingaben | Begrenzter Abruf und Empfang unter getrennten Grants; generischer Transport und externer Pixel-Suchnachweis |
| Allgemeiner Host mit JSON-Konfiguration und mehreren Adapterprozessen | Implementiert, einschließlich konfigurierter Datenabfragen |
| Externer stdio-Adapter mit Geräteprofil, ACK und persistentem Replay | Erster Observation-Slice implementiert |
| Request und freigegebene Action über stdio | Implementiert, inklusive persistentem Lüfter-Simulator |
| Deterministische Host-Vorschläge mit Regel und Evidenz | Implementiert, separate Verification und Approval |
| Observation → Entity → StateEstimate → WorldState | Implementiert |
| Append-only SQLite-Store und deterministisches Replay | Implementiert |
| Artifact-Netz als bekannter Fast Path | Implementiert |
| Externe Kamera und Pixel-Suche | In `../drone-sim`: zielunabhängige Kamera und separater Rules-Reasoner mit ausgewählten Bildbytes; noch keine autonome Suche |
| Dauerhafte Aufgabenverarbeitung | Konfigurierbare Input-/Observe-zu-Reason-Routen, persistente Queue, Wiederaufnahme und Review-Übergabe; serielles Polling |
| Allgemeine Zielschleife | Optional pro Route: externe Planung und Bewertung, kontrollierte Aktionen, frische Quellen, Wiederaufnahme und Rundenbudget; Lüftertest nachgewiesen |
| Kuratiertes Reasoning | V1-Modellfall und allgemeiner V2-Port; externe Solver und Modell-Testadapter, noch kein echter Modellanbieter |
| Verification → Approval → Action → Execution → Outcome | Implementiert |
| Explizite Goals und maschinenprüfbare Zielbedingungen | Aufgabensteuerung vorhanden; Zielbedeutung und Wirkungsprüfung über konfigurierte externe Reasoner, kein universeller Beweisprüfer |
| Episodisches Memory und wiederverwendbare Skills | Geplant |
| Gelerntes Kosten-/Latenz-/Risiko-Routing | Geplant |
| Kalibrierte Sensorfusion und probabilistischer Belief State | Geplant |

## Architektur

```text
externe Welt
    │
    ▼
Adapter + Grant ──→ Observation ──→ Entity ──→ StateEstimate ──→ WorldState
                         │                                      │
                         ▼                                      ▼
                 Artifact Store/Network ──→ Routing ──→ Reasoning
                                                    │          │
                                                    └────┬─────┘
                                                         ▼
                                      Verification → Approval → Action
                                                                    │
                                                                    ▼
                                                        Execution → Outcome
                                                                    │
                                                                    ▼
                                                           Store / Learning
```

Die öffentlichen Rust-Module teilen diese Verantwortung auf:

| Modul | Verantwortung |
| --- | --- |
| `core` | Vertrauenswürdige Typen, Schemata, Artefakte und Validierung |
| `adapters` | Verträge und Berechtigungsgrenze für externe Integrationen |
| `io` | Kontrollierte Kommunikation mit externen Systemen |
| `world` | Observations, Entities, Beliefs, WorldState und Reducer |
| `store` | Append-only Persistenz, Relationen und Artifact-Netz |
| `reasoning` | Validierte, aber noch nicht autorisierte Vorschläge |
| `decision` | Deterministische Vorschläge, Verification und explizite Approval-Entscheidungen |
| `action` | Materialisierung, Ausführung und Outcomes |
| `runtime` | Orchestrierung von Routing, Reasoning und Agentenzyklen |
| `learning` | Kontrollierte lokale Gewichtsänderungen im Artifact-Netz |

## Mathematischer World-State

ARCS behandelt den Weltzustand als Belief State $b_t$, nicht als wahre und
vollständig bekannte Welt:

$$
b_t = B(b_{t-1}, o_t)
$$

Im aktuellen ersten Slice bestimmt jede persistierte Observation genau einen
Zustandsslot. Dabei bildet $e(o)$ das namespaced Subject auf eine `EntityId`
ab und $\sigma(o)$ bezeichnet die versionierte `SchemaId`:

$$
k(o) = \bigl(e(o),\sigma(o)\bigr)
$$

Mit $\hat{s}(o)$ als dem aus der Observation erzeugten `StateEstimate` ist der
deterministische Latest-Observation-Reducer definiert als:

$$
B(b,o)[k(o)] = \hat{s}(o)
$$

Für alle anderen Schlüssel $k' \neq k(o)$ gilt
$B(b,o)[k']=b[k']$. Die Commit-Reihenfolge ist explizit; Zeitstempel dienen
nicht als versteckte Konfliktheuristik.

Eine Observation darf den World-State erst verändern, nachdem sie validiert
und im Store committed wurde. Der dabei vergebene monotone
`ObservationCursor` ermöglicht:

- idempotentes Wiederholen derselben Observation,
- sichtbare Fehler bei veralteter oder widersprüchlicher Reihenfolge,
- deterministischen Neuaufbau des World-State nach einem Neustart und
- atomare Updates auf Objektebene: Bei einem Fehler bleibt der Zustand gleich.

Trust und quantitative Confidence sind getrennte Konzepte. Da im ersten Slice
noch kein kalibriertes Beobachtungsmodell existiert, erzeugt der Reducer
bewusst `EstimateConfidence::Unknown`. Er erfindet keine Präzision aus dem
Trust-Level einer Quelle.

## Kontroll- und Sicherheitsgrenzen

ARCS setzt Autorität an mehreren Stellen ausdrücklich nicht voraus:

- Ein Adapter-Manifest beschreibt Fähigkeiten, schaltet sie aber nicht frei.
  Erst ein separater Betreiber-Grant autorisiert konkrete Capabilities,
  Permissions, Größenlimits und Trust.
- Daten von außen gelten an der Systemgrenze als untrusted. Der Core setzt
  IDs, Zeit, Actor, Trust und Provenance selbst.
- Ein Reasoner erzeugt nur validierte Kandidaten. Er darf keine Action direkt
  ausführen.
- Verification und Approval sind eigene Stufen vor der Materialisierung einer
  Action.
- Fehler werden sichtbar weitergereicht. Fehlkonfiguration oder Store-Fehler
  werden nicht als „kein Treffer“ umgedeutet und lösen dadurch keinen
  unbeabsichtigten teuren Fallback aus.
- Der Artifact-Store bleibt der unveränderliche Audit-Trail; der World-State
  ist eine daraus reproduzierbare aktuelle Sicht.

## Schnellstart

Für ein eigenes Desktop-Fenster:

```bash
./arcs-control
```

Das [Kontrollpanel](apps/control-panel/README.md) bietet Start und Stopp,
Konfigurationsauswahl, Adapterkatalog mit Rollenfiltern, aktuelle Statusdaten
und Protokolle. Unter **Eingabe** lassen sich Texte, Formulare und JSON-Aufgaben
an verbundene JSON-Input-Adapter senden. **Aufgaben per Text · Beispiel**
verbindet den Originaltext mit externen Reasonern und der Zielsteuerung.
Die zusätzliche Konfiguration **Texteingabe** empfängt Nachrichten ohne
Bearbeitungsroute. `./arcs-control --install-launcher` ergänzt einen Eintrag im
persönlichen Anwendungsmenü.

Voraussetzung ist eine Rust-Toolchain mit Unterstützung für Edition 2024.
SQLite wird über die `bundled`-Option von `rusqlite` eingebunden.

Der allgemeine Host verbindet beide Simulatoren ausschließlich über
Konfiguration:

```bash
cargo build --locked --workspace --bins
cargo run --locked -p arcs-host -- \
  --config apps/adapter-host/config/host.json
```

Die Ausgabe enthält Sensormessungen, abgefragte Lüfterdaten und den
World-State. Dieser Host führt standardmäßig einen begrenzten Lauf aus und
schaltet den Lüfter nicht selbstständig ein. Vollständige Konfiguration,
Pfadregeln, Request-IDs und Bibliotheks-API stehen in
[apps/adapter-host/README.md](apps/adapter-host/README.md).

Das minimale Beispiel ohne externe Prozesse bleibt separat verfügbar:

```bash
cargo run --locked -p arcs-demo-host --bin arcs --quiet
```

Die Demo in [`apps/demo-host/src/main.rs`](apps/demo-host/src/main.rs) registriert einen simulierten
Chat-Adapter samt Grant, nimmt eine Observation entgegen, schreibt sie in
einen flüchtigen SQLite-Store und reduziert sie in einen neuen World-State.
Anschließend gibt sie das gespeicherte Artefakt als JSON aus. Für persistente
Daten steht `SqliteArtifactStore::open` zur Verfügung.

Ein zweites Beispiel verbindet einen eigenständigen Sensor-Adapter über stdio
mit einem persistenten Store und dem World-State. Geräteprofil und
Temperatur-Schema liegen außerhalb des Cores. Startbefehle, Nachrichtenformate
und Wiederholungsverhalten stehen in [ADAPTER_PROTOCOL.md](ADAPTER_PROTOCOL.md).

Die Lüfter-Demo ergänzt den vollständigen Regelkreis über denselben Transport:
gezielte Temperaturabfrage, deterministischer Vorschlag, Verification,
System-Freigabe für den Simulator, Action, Rückabfrage, Outcome und World-State.
Sie verwendet einen eigenen persistenten Adapterzustand und schützt auch
nach Prozessneustarts vor wiederholten Wirkungen. Start und Grenzen sind im
[Abschnitt zum Lüfter-Zyklus](ADAPTER_PROTOCOL.md#vollständigen-lüfter-zyklus-starten)
beschrieben.

Der zentrale World-State-Schritt sieht nach erfolgreichem Ingest so aus:

```rust
let recorded = observation
    .ingest_recorded(&adapter_id, message)?;

let mut world = WorldState::new();
let reduction = WorldReducer::new()
    .reduce(&mut world, &recorded)?;
```

## Entwicklung

```bash
# Gesamte Testsuite
cargo test --locked --workspace --all-targets

# Formatierung prüfen
cargo fmt --all -- --check

# Statische Analyse
cargo clippy --locked --workspace --all-targets --all-features

# API-Dokumentation bauen
cargo doc --locked --workspace --no-deps
```

Die wichtigsten Einstiegspunkte sind:

- [`crates/arcs-core/src/lib.rs`](crates/arcs-core/src/lib.rs) für die öffentliche Modulstruktur,
- [`crates/arcs-core/src/world`](crates/arcs-core/src/world) für das aktuelle World Model,
- [`crates/arcs-adapter-protocol/src`](crates/arcs-adapter-protocol/src) für die unabhängigen Kommunikationsverträge,
- [`adapters`](adapters) für konkrete Adapter und ihre Geräteprofile und
- [`apps/demo-host/src`](apps/demo-host/src) für ausführbare Beispiele und die Lüfter-Regel.

Der konfigurierbare Host startet in
[`apps/adapter-host/src`](apps/adapter-host/src); seine Beispielkonfiguration
liegt unter [`apps/adapter-host/config`](apps/adapter-host/config).

Die Integrationstests bauen die Adapter-Binaries einmal pro Testsuite separat,
da Cargo `CARGO_BIN_EXE` nur für Programme des eigenen Pakets bereitstellt.
Anschließend starten sie die echten ausführbaren Adapter direkt über stdio.

## Nächste formale Schritte

1. **Goals:** Ziele als versionierte Artefakte mit prüfbarer
   Erfüllungsbedingung, Budget, Deadline und Risikogrenze modellieren.
2. **Memory und Skills:** Episoden von ausführbaren, versionierten Skills
   trennen und Retrieval nur oberhalb einer expliziten Ähnlichkeits- und
   Vertrauensschwelle erlauben.
3. **Kostenbewusstes Routing:** Reasoner anhand des erwarteten Nettowerts
   auswählen:

   $$
   r^*=\arg\max_r\left[
   P(\mathrm{success}\mid r,x)V
   -\lambda_C C(r)-\lambda_L L(r)-\lambda_R\rho(r)
   \right]
   $$

4. **Lernen:** Erfolgswahrscheinlichkeiten und Skill-Gewichte mit
   nachvollziehbaren, begrenzten Updates lernen; Exploration und
   Nichtstationarität ausdrücklich behandeln.
5. **Belief-Ausbau:** Kalibrierte Beobachtungsmodelle, Evidenzfusion und bei
   Bedarf einen probabilistischen POMDP-Belief ergänzen.

Free-Energy-Modelle oder STDP-artige Lernregeln bleiben mögliche spätere
Experimente. Sie sind bewusst nicht das Fundament des ersten ARCS-Kerns.
