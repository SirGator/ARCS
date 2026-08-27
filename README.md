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

## Zielbild

ARCS betrachtet Agentenverhalten als Optimierungsproblem. Neben dem
Aufgabennutzen gehören Rechenkosten, Latenz und Risiko ausdrücklich zur
Zielfunktion:

$$
\pi^* = \arg\max_\pi
\mathbb{E}_\pi\left[
\sum_{t=0}^{T}
\left(R_t-\lambda_C C_t-\lambda_L L_t-\lambda_R \operatorname{Risk}_t\right)
\right]
$$

Dabei bezeichnet $R_t$ den erwarteten Nutzen, $C_t$ die Rechenkosten, $L_t$
die Latenz und $\operatorname{Risk}_t$ das erwartete Fehlerrisiko. Die
Gewichte $\lambda_C$, $\lambda_L$ und $\lambda_R$ machen die gewünschte
Abwägung explizit und konfigurierbar.

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
| `decision` | Verification und explizite Approval-Entscheidungen |
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
Zustandsslot:

$$
k(o) = \bigl(\operatorname{EntityId}(\operatorname{subject}(o)),
              \operatorname{SchemaId}(o)\bigr)
$$

Der deterministische Latest-Observation-Reducer ist definiert als:

$$
B(b,o)[k(o)] = \operatorname{estimate}(o)
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

```bash
cargo run --quiet
```

Die Demo in [`src/main.rs`](src/main.rs) registriert einen simulierten
Chat-Adapter samt Grant, nimmt eine Observation entgegen, schreibt sie in
einen flüchtigen SQLite-Store und reduziert sie in einen neuen World-State.
Anschließend gibt sie das gespeicherte Artefakt als JSON aus. Für persistente
Daten steht `SqliteArtifactStore::open` zur Verfügung.

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
cargo test --locked --all-targets

# Formatierung prüfen
cargo fmt --all -- --check

# Statische Analyse
cargo clippy --all-targets --all-features

# API-Dokumentation bauen
cargo doc --no-deps
```

Die wichtigsten Einstiegspunkte sind:

- [`src/lib.rs`](src/lib.rs) für die öffentliche Modulstruktur,
- [`src/world`](src/world) für das aktuelle World Model,
- [`src/runtime`](src/runtime) für die Agentenorchestrierung,
- [`src/reasoning`](src/reasoning) für Fast Path und Reasoning-Fallback und
- [`src/main.rs`](src/main.rs) für ein minimales ausführbares Beispiel.

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
   -\lambda_C C(r)-\lambda_L L(r)-\lambda_R\operatorname{Risk}(r)
   \right]
   $$

4. **Lernen:** Erfolgswahrscheinlichkeiten und Skill-Gewichte mit
   nachvollziehbaren, begrenzten Updates lernen; Exploration und
   Nichtstationarität ausdrücklich behandeln.
5. **Belief-Ausbau:** Kalibrierte Beobachtungsmodelle, Evidenzfusion und bei
   Bedarf einen probabilistischen POMDP-Belief ergänzen.

Free-Energy-Modelle oder STDP-artige Lernregeln bleiben mögliche spätere
Experimente. Sie sind bewusst nicht das Fundament des ersten ARCS-Kerns.
