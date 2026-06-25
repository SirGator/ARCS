# SPECIFICATION

## Rolle dieser Datei

Diese Datei beschreibt den Kernvertrag von ARCS.

Sie ist absichtlich knapper und stabiler als Status- oder Phase-Dokumente.

Die angrenzenden Dokumente haben andere Rollen:

- [`README.md`](../README.md): Einstieg und aktueller Scope
- [`ARCHITECTURE.md`](ARCHITECTURE.md): Struktur und Modulaufteilung
- [`SCHEMA_CONTRACT.md`](SCHEMA_CONTRACT.md): harte Schema- und Commit-Gates
- [`MVP_FLOW.md`](MVP_FLOW.md): aktueller echter V1-Flow
- [`TEST_MATRIX.md`](TEST_MATRIX.md): welcher Test welche Garantie belegt

Diese Datei beantwortet vor allem die Frage:

```text
Was muss ARCS als Core garantieren?
```

## Kern-Prinzipien

| Prinzip | Bedeutung |
|---------|-----------|
| **Rules, not Features** | Der Core definiert nur Regeln, nicht Funktionen |
| **Fail-Closed** | unknown = BLOCK — keine Ausnahmen |
| **LLM ist nie Autorität** | LLM-Output = trust.level low, immer verifiziert |
| **Immutable Logs** | Append-only, Replay-garantiert |
| **Schema First** | Kein untypisierter Payload nach Ingress und kein relevanter Commit ohne bekannte `schema_id` |

## V1-Lesart dieser Prinzipien

Fuer V1 bedeuten die Prinzipien konkret:

- `Schema First` ist nicht nur Stil, sondern ein Commit-Gate
- `Fail-Closed` gilt fuer Verification und Interpretation
- LLMs duerfen Vorschlaege erzeugen, aber keine Autoritaetsartefakte direkt committen
- Replay und Reducer duerfen keine verdeckte Laufzeitlogik voraussetzen

## Die 7 Kern-Komponenten

| # | Komponente | Rolle | Analogie |
|---|-----------|-------|----------|
| 1 | **Artifact System** | Alles im System ist ein Artefakt | Sprache |
| 2 | **Schema Registry** | Regeln für Datenstruktur | Grammatik |
| 3 | **Store** | Append-only Persistenz | Gedächtnis |
| 4 | **Reducer** | Deterministische State-Projektion | Verstand |
| 5 | **Verification Engine** | Policy Enforcement | Gewissen |
| 6 | **Approval System** | Human-in-the-Loop | Zustimmung |
| 7 | **Execution Engine** | Wirkung in der Welt | Hände |

## External APIs

Rohtext wird nicht mehr intern interpretiert. ARCS stellt einen einzelnen `/interpret`-Auftrag und verarbeitet das zurückgegebene Interpretation-Proposal.

Wichtig fuer V1:

- der Interpretationspfad ist extern und entkoppelt
- Interpretation erzeugt maximal Vorschlaege
- Action, Approval, Policy und ExecutionResult bleiben ausserhalb des LLM-Vertrauenspfads

## Core Artefakt-Typen

| Gruppe | Typ | Zweck |
|--------|-----|-------|
| Input | ingress_event | Roher eingehender Event |
| Input | raw input / config | Externe Schnittstellen steuern die Verarbeitung |
| Input | task | User/System will X erreichen |
| Input | claim | Wissensbehauptung (mit source) |
| Input | evidence | Datei/URL/API-Snapshot |
| State | world_state | Assertions über aktuellen Zustand |
| Engine | match_result | Deterministic Pattern Match |
| Engine | risk | Risiko mit severity + rationale |
| Engine | conflict | Widerspruch zwischen Artefakten |
| Planning | option | Alternative mit typed steps |
| Governance | policy | Regeln + Verifier-Config |
| Governance | permission_grant | Principal → Capability |
| Governance | verification_report | pass / fail / unknown |
| Governance | approval | approve / reject / modify / revoke |
| Execution | action | Typisierte, idempotente Operation |
| Execution | execution_result | Outcome mit exit_code, Logs |

## Standard-Enums (V1)

| Enum | Werte |
|------|-------|
| actor_type | human, system, model, executor |
| source.kind | chat, file, api, sensor, timer, internal |
| trust.level | low, medium, high |
| trust.source_class | human, system, model, external |
| verification.status | pass, fail, unknown |
| approval.decision | approve, reject, modify, revoke |
| action.type | file_write, file_read, api_call, shell_cmd, report_emit |
| execution_result.status | success, fail, cancelled, timeout |

## Option Status-Maschine

```
option:draft
  │
  ▼ (verification)
  +-- fail/unknown --> option:blocked
  │
  ▼ pass --> option:verified
  │
  ▼ (human approval)
  +-- reject --> option:rejected
  +-- modify --> neue Option-Version --> zurück zu draft
  │
  ▼ approve --> option:approved
  │
  ▼ (materialization)
  --> option:action_dispatched
  │
  ▼ (execution)
  +-- fail/timeout --> option:failed
  +-- revoke --> option:revoked
  │
  ▼ success --> option:executed
```

## MVP-Scope V1

**Flow:** `ingress_event → task (optional) → option (emit_report) → verification_report (pass) → approval (approve) → action (report_emit) → execution_result`

**External contract:** input, schema, prompt, output

**Constraints:** Kein Shell, kein Netzwerk, kein file_write. Nur `report_emit`.

**Store:** StoreMemory für Tests, StoreSQLite für V1, StorePostgres für Production.

Der aktuelle implementierte und getestete Flow ist in [`MVP_FLOW.md`](MVP_FLOW.md) beschrieben.

Die Tests, die diesen Scope heute belegen oder noch nicht voll belegen, stehen in [`TEST_MATRIX.md`](TEST_MATRIX.md).

## Harte V1-Regeln

### Regel 0: Schema- und Commit-Gate

Kein relevantes Artefakt darf committed werden, wenn:

- `schema_id` fehlt
- `schema_id` unbekannt ist
- das Payload das referenzierte Schema nicht erfuellt
- die zugehoerigen Governance-Gates nicht greifen

Die praezise Vertragsform steht in [`SCHEMA_CONTRACT.md`](SCHEMA_CONTRACT.md).

## Drei harte Betriebsregeln

### Regel 1: Commit-Boundary
Alles was zusammengehört in EINER Transaktion: artifact_versions + events. Kein Halb-Commit. Kein Event ohne Artefakt. Kein Artefakt ohne Event.

### Regel 2: Reducer-Regel für Head
Head ist nicht "latest row". Head ist das Ergebnis eines deterministischen Reducers über den Event Log. Reducer dürfen NICHT nach occurred_at sortieren — nur nach Log-Reihenfolge oder causal links.

### Regel 3: At-least-once + Idempotenz
action_id ist der Idempotency-Key. Doppelte Ausführung = no-op oder safe replay.

## Nicht Teil dieser Datei

Diese Datei sagt nicht automatisch, dass jede Regel bereits vollstaendig im
heutigen Repo erzwungen wird.

Dafuer gelten die anderen Dokumente:

- `SCHEMA_CONTRACT.md`: Zielvertrag plus aktueller Durchsetzungsstand
- `MVP_FLOW.md`: echter Ist-Flow
- `TEST_MATRIX.md`: Testbeweis oder Luecke
