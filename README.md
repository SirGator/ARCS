# ARCS

**ARCS – Artifact Reasoning and Control System** ist ein experimentelles
Agenten-Framework in Rust. Es soll einen möglichst allgemeinen, fähigen und
günstigen Agenten ermöglichen, ohne Reasoning direkt mit Handlungsbefugnis zu
verwechseln.

Der Kernansatz ist einfach: Externe Daten werden zuerst validiert und als
unveränderliche Artefakte gespeichert. Daraus baut ARCS einen nachvollziehbaren
Weltzustand auf, versucht bekannte Aufgaben deterministisch zu lösen und nutzt
teureres Reasoning nur als kontrollierten Fallback. Vorschläge müssen danach
weiterhin verifiziert, freigegeben und als eigene Actions ausgeführt werden.

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
│   └── file-output/                Idempotente Ausgabe mit Journal und JSON-Berichten
├── apps/
│   ├── adapter-host/               Allgemeiner Host mit JSON-Konfiguration
│   │   ├── src/                    CLI und Bibliothek ohne Geräteabhängigkeit
│   │   ├── config/                 Programme, Profile, Schemas, Grants und Abfragen
│   │   └── tests/                  Prozess- und Konfigurationstests
│   └── demo-host/                  Beispielanwendungen, Regeln und System-Freigabe
│       ├── src/                    Minimale Demo sowie Sensor- und Lüfter-Host
│       ├── config/                 Betreiber-Grants
│       └── tests/                  Tests mit echten externen Adapterprozessen
└── config/                         Historische C++/Worker-Beispiele; nicht geladen
```

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
anderen Binärdaten; eine Kamera oder Bildauswertung ist noch nicht enthalten.
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

**Universalität ist eine verbindliche Architekturvorgabe.** ARCS bleibt ein
allgemeines Agenten-Framework. Die Drohnensuche dient als erster großer
Anwendungs- und Bewährungsfall. Allgemeine Zielverfolgung, Beobachtung,
Planung und kontrollierte Ausführung sollen auch für andere Aufgaben nutzbar
sein; Gerätewissen und aufgabenspezifische Regeln bleiben austauschbar.

Das erste große Anwendungsziel ist: **ARCS steuert eine simulierte Drohne
durch einen simulierten Raum und findet darin ein vorgegebenes Zielobjekt
mithilfe der Drohnenkamera.**

Die Drohne soll dafür wiederholt beobachten, eine Suchentscheidung treffen,
eine freigegebene Bewegung ausführen und das Ergebnis erneut beobachten.
Ein Fund muss auf Kamerabildern beruhen und mit gespeicherter Evidenz
nachvollziehbar sein. Drohnenadapter und Simulation bleiben außerhalb des
Core. Aktuell sind die allgemeine Adapteranbindung und die kontrollierte
Action-Pipeline vorhanden; Drohne, Kamera und Suchsteuerung sind noch nicht
implementiert.

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
| Gemeinsames Adapter-SDK für Observe, Request, Transform, Output und Act | Implementiert, mehrere unabhängige Adapter |
| Transform mit geprüften Ein-/Ausgaben, Provenienz und persistentem Replay | Implementiert, allgemeiner Zahlen-Normalisierer als Beispiel |
| Output mit gebundenem Auftrag, Verification, Approval und Beleg | Implementiert, Dateiausgabe mit Journal und Recovery |
| Mehrstufige Observe → Transform → Output-Pipelines | Allgemeiner Host-Runner mit gebundener Definition, Checkpoints und Cursor; Freigabepolicy in der Anwendung |
| Referenzierte Binärdaten mit begrenztem Abruf und separaten Grants | Implementiert, allgemeine Dateiquelle als Prozessnachweis |
| Allgemeiner Host mit JSON-Konfiguration und mehreren Adapterprozessen | Implementiert, einschließlich konfigurierter Datenabfragen |
| Externer stdio-Adapter mit Geräteprofil, ACK und persistentem Replay | Erster Observation-Slice implementiert |
| Request und freigegebene Action über stdio | Implementiert, inklusive persistentem Lüfter-Simulator |
| Deterministische Host-Vorschläge mit Regel und Evidenz | Implementiert, separate Verification und Approval |
| Observation → Entity → StateEstimate → WorldState | Implementiert |
| Append-only SQLite-Store und deterministisches Replay | Implementiert |
| Artifact-Netz als bekannter Fast Path | Implementiert |
| Kuratiertes Reasoning als Fallback | Implementiert |
| Verification → Approval → Action → Execution → Outcome | Implementiert |
| Explizite Goals und maschinenprüfbare Zielbedingungen | Geplant |
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
