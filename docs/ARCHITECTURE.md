# ARCHITECTURE

## System-Übersicht

ARCS ist eine **artifact-zentrische, event-gesourcte Plattform**. Jede Information, jede Entscheidung und jede Aktion wird als Artefakt repräsentiert. Der Flow ist immer:

```
Input → Ingress → Task → Option → Verification → Approval → Action → Execution → Result
```

## Gesamtfluss

```
[Externe Quelle: chat ]
  │
  ▼
┌─────────────────┐
│  Ingress Layer  │  normalisieren, typisieren, validieren
└────────┬────────┘
         ▼
┌─────────────────┐
│ Artifact System │  ingress_event → task → claim → evidence
└────────┬────────┘
         ▼
┌─────────────────┐
│  Schema Registry│  JSON Schema Validierung (Draft 7 via valijson)
└────────┬────────┘
         ▼
┌─────────────────┐
│     Store       │  append-only, atomare Commits (Commit-Boundary)
└────────┬────────┘
         ▼
┌─────────────────┐
│    Reducers     │  deterministische Projektion: TaskState, ApprovalState, Permissions
└────────┬────────┘
         ▼
┌─────────────────┐
│  Verification   │  Tri-State: pass / fail / unknown — unknown blockt immer
│     Engine      │  SchemaVerifier, PermissionVerifier, ScopeVerifier,
│                 │  ApprovalVerifier, AuthorityVerifier, ReferenceIntegrityVerifier
└────────┬────────┘
         ▼
┌─────────────────┐
│  Approval Gate  │  Human-in-the-Loop: approve / reject / modify / revoke
└────────┬────────┘
         ▼
┌─────────────────┐
│  Materializer   │  approved option → typisierte Actions (deterministisch, kein LLM)
└────────┬────────┘
         ▼
┌─────────────────┐
│  Executor       │  idempotent, at-least-once safe, adapter-basiert
└────────┬────────┘
         ▼
┌─────────────────┐
│  Store + Events │  alles committed, replaybar
└─────────────────┘
```

## Modul-Struktur

| Modul | Pfad | Aufgabe |
|-------|------|---------|
| Artifact | `src/artifact/` | Zentrale Datentypen: `ArtifactVersion`, `ActorRef`, `SourceRef`, `TrustInfo`, `Provenance`, ID-Generierung, JSON-SerDe |
| Event | `src/event/` | Event-Typen, JSON-SerDe |
| Store | `src/store/` | `IStore` Interface, `StoreMemory` (In-Memory), Head-Tracking, Optimistic Lock, atomare Commits |
| Schema | `src/schema/` | `SchemaRegistry`, `SchemaLoader`, `Validator` (valijson Draft 7) |
| Reducer | `src/reducer/` | `IReducer<TState>`, `TaskStateReducer`, `ApprovalStateReducer`, `PermissionReducer`, `ITimeSource` |
| Verification | `src/verification/` | `IVerifier`, `VerificationEngine`, 6 Core-Verifier |
| Approval | `src/approval/` | `IApprovalGate`, `ApprovalGate` (erzeugt Approval-Artefakte) |
| Execution | `src/execution/` | `IExecutor`, `ActionMaterializer`, `AdapterRegistry`, `AdapterExecutor`, `ReportEmitExecutor`, `PolicyUpdateExecutor`, Idempotenz |
| Policy | `src/policy/` | `PolicyPayload`, `PermissionGrantPayload` |
| Core | `src/core/` | Orchestrierung (`run_text_flow`), `SystemLogger` |
| Adapters | `src/adapters/` | Input (CLI), externe API-Verträge |

## Key Interfaces

| Interface | Datei | Zweck |
|-----------|-------|-------|
| `IStore` | `store/include/store/store.hpp` | Append-only Store mit atomarem `commit()` |
| `IVerifier` | `verification/include/verification/verifier.hpp` | Single Check — Strategy Pattern |
| `VerificationEngine` | `verification/include/verification/verifier.hpp` | Aggregiert mehrere Verifier |
| `IExecutor` | `execution/include/execution/executor.hpp` | Führt eine Action aus |
| `IExecutionAdapter` | `execution/include/execution/adapter_registry.hpp` | Plugin-Interface für action-type-spezifische Logik |
| `IReducer<TState>` | `reducer/include/reducer/reducer.hpp` | Pure function: `vector<Artifact> → TState` |
| `ITimeSource` | `reducer/include/reducer/time_source.hpp` | Injizierte Zeit (Testbarkeit, Replay) |
| `IMaterializer` | `execution/include/execution/materializer.hpp` | Option → Actions (deterministisch) |
| `IApprovalGate` | `approval/include/approval.hpp` | Approval-Entscheidungen committen |
| `ISchemaRegistry` | `schema/include/schema/schema_registry.hpp` | Schemas registrieren und validieren |
| `External API contract` | `config/arcs.yaml` | input/schema/prompt/output werden extern konfiguriert |

## Datenmodell

### ArtifactVersion (Kernstruktur)

```
artifact_id       – stabile Identität
version_id        – diese spezifische Version
version           – fortlaufende Nummer
type              – Artefakt-Typ (task, option, action, ...)
schema_id         – Referenz auf Schema Registry
created_at        – UTC-Zeitstempel
created_by        – ActorRef { actor_type, id }
source            – SourceRef { kind, ref }
trust             – TrustInfo { level, source_class }
stream_key        – Aggregate-Grenze für Reducer
tags              – frei
payload           – JSON (schema-validiert)
provenance        – parents, models_used, rules_applied, transform
```

### Event

```
event_id          – eindeutige ID
event_type        – artifact_committed, head_advanced, approval_recorded, ...
ts                – Audit-Zeitstempel (NICHT für Logik)
actor             – ActorRef
refs              – EventRef[] { artifact_id, version_id, role }
stream_key        – Aggregate-Grenze
payload           – klein, deterministisch
prev_hash         – V2 mandatory (tamper-detection)
```

### CommitBundle

```
versions[]        – PendingVersion[] (mit expected_head für Optimistic Lock)
events[]          – Event[]
```

> **Betriebsregel:** Kein Event ohne Artefakt. Kein Artefakt ohne Event. Alles in einer Transaktion.

## Design Patterns

| Pattern | Einsatz |
|---------|---------|
| **Event Sourcing** | Alle Zustandsänderungen als Events im append-only Log |
| **Optimistic Concurrency** | `expected_head_version_id` verhindert Lost Updates |
| **Reducer Pattern** | Deterministische Projektion: Log → abgeleiteter Zustand |
| **Strategy Pattern** | `IVerifier`, `IExecutor`, `IExecutionAdapter` — austauschbar |
| **Idempotenz** | `action_id` = Idempotency-Key — doppelte Ausführung = no-op |
| **Policy-as-Artifact** | Policies sind versionierte Artefakte mit eigenem Governance-Pfad |

## Implementierungsstand

### Vollständig implementiert

- `artifact/` — Alle Typen, JSON-SerDe, ID-Generierung, Factory
- `event/` — Event-Typen, JSON-SerDe
- `store/` — `IStore`, `StoreMemory`, Head-Tracking, Optimistic Lock
- `schema/` — Registry, Loader, Validator (valijson)
- `policy/` — Policy- und Permission-Artefakte
- `approval/` — `ApprovalGate`
- `reducer/` — TaskState, ApprovalState, Permission-Reducer, MockTimeSource
- `verification/` — Engine + 6 Verifier (Schema, Permission, Scope, Approval, Authority, ReferenceIntegrity)
- `execution/` — `ReportEmitExecutor`, `PolicyUpdateExecutor`, Materializer, AdapterRegistry, Idempotenz
- `adapters/interpretation/` — C++-Client, der `POST /interpret` an den
  `interpretation_worker` schickt
- `tools/interpretation_worker/` — HTTP-Bridge zum text-to-json-parser
  inkl. `parser_client.py` und Bridge-Tests

### Teilweise / Stub

| Komponente | Status |
|------------|--------|
| `External API contract` | `config/arcs.yaml` | input/schema/prompt/output werden extern konfiguriert |
| `adapter_context.hpp` | Stub — leere Datei |
| `WriteFileStep` | Definiert, kein Executor |
| `ApiCallStep` | Definiert, kein Executor |
| `adapters/ai/` | Leeres Verzeichnis |
| `adapters/data/` | Leeres Verzeichnis |
| `adapters/execution/` | Leeres Verzeichnis |
| `StoreSQLite / StorePostgres` | Nicht implementiert (nur In-Memory) |

## Vollständige Spec

Siehe [`SPECIFICATION.md`](SPECIFICATION.md) für die formale Übersicht und [`README.md`](../README.md) für den Projektüberblick.

## Interpretation-Pipeline (text → interpretation_proposal)

ARCS hält den LLM nie selbst. Die Trennung ist dreistufig:

| Stufe | Komponente | Verantwortung |
|-------|-----------|---------------|
| 1 | `WorkerInterpretationClient` (C++) | Spricht mit dem Worker. Kennt nur `interpret_api_url` und das ARCS-Request-Schema. |
| 2 | `interpretation_worker` (Python, stdlib HTTP) | Brücke. Validiert den Request, leitet an den Parser, mappt das Resultat zurück. |
| 3 | `text-to-json-parser` (Python, FastAPI) | LLM-gestützter Schema-Extraktor. Nimmt `text` + `schema` und liefert schema-konformes JSON. |

Konfiguriert wird das ausschließlich über `config/arcs.yaml`:

- `interpret_api_url` — wohin der C++-Client schickt (Worker, default `http://127.0.0.1:8090/interpret`)
- `parser_url` — wohin der Worker schickt (Parser, default `http://127.0.0.1:8000`)
- `parser_timeout` — HTTP-Timeout für Parser-Aufrufe
- `parser_prompt_file` — optionaler Default-Prompt, falls der ARCS-Caller keinen eigenen mitgibt

Der Worker fällt automatisch auf das eingebaute Schema
`arcs.interpretation_proposal.v1` zurück, wenn der ARCS-Caller keines
mitgibt. Damit ist der C++-Client unabhängig von der Parser-Implementierung
austauschbar.
