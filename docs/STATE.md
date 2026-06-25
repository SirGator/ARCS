# ARCS — Projektstand (Inhaltsverzeichnis)

> Snapshot vom 7. Juni 2026. Letzter erfolgreicher Build: 7. Juni 2026.
> Build-Verzeichnis: `build/` (CMake + FetchContent)
> Code-Umfang: ~7.500 LOC (C++/Header) — nach Phase 0
> Test-Stand: C++-Suite grün, Python-Bridge- und Full-Stack-E2E-Tests vorhanden

Dieses Dokument ist die **Wurzel**. Detaillierte Inhalte sind in Phasen aufgeteilt.

---

## Phasen-Index

| # | Phase | Datei | Inhalt |
|---|-------|-------|--------|
| 0 | **Foundation** | [docs/phases/phase-0-foundation.md](docs/phases/phase-0-foundation.md) | Artifact, Event, Schema, Store, Config — die unveränderlichen Bausteine |
| 1 | **Determinism** | [docs/phases/phase-1-determinism.md](docs/phases/phase-1-determinism.md) | Reducer + ITimeSource — der berechenbare Zustand |
| 2 | **Governance** | [docs/phases/phase-2-governance.md](docs/phases/phase-2-governance.md) | Policy, Permission, Approval, Verifier — die Regelwächter |
| 3 | **Execution** | [docs/phases/phase-3-execution.md](docs/phases/phase-3-execution.md) | Materializer, Executors, Idempotenz — die Wirkung |
| 4 | **Interpretation** | [docs/phases/phase-4-interpretation.md](docs/phases/phase-4-interpretation.md) | Ingress + Worker + Parser — die LLM-Brücke |
| 5 | **V2 & Tooling** | [docs/phases/phase-5-v2-tooling.md](docs/phases/phase-5-v2-tooling.md) | Schema-Migration, Second-Approver, `prev_hash`, CI, Lint |

Lese-Reihenfolge: `phase-0 → 1 → 2 → 3 → 4 → 5`. Jede Phase baut auf der vorherigen auf.

---

## Schnellüberblick (was funktioniert heute)

### 7 Core-Module (C++20) — alle ✅

| # | Modul | Pfad |
|---|-------|------|
| 1 | Artifact System | `src/artifact/` |
| 2 | Schema Registry (valijson) | `src/schema/` |
| 3 | Store (`IStore` + `StoreMemory`) | `src/store/` |
| 4 | Reducer (Task / Approval / Permission) | `src/reducer/` |
| 5 | Verification Engine (6 Verifier) | `src/verification/` |
| 6 | Approval Gate | `src/approval/` |
| 7 | Execution (Materializer, Adapters, Idempotenz) | `src/execution/` |

### Interpretation-Pipeline (3-stufig) — alle ✅

```
ARCS Core (C++)  →  interpretation_worker (Python stdlib HTTP)
                       →  text-to-json-parser (FastAPI, sibling)
```

### Tests

- **25 GTest-Binaries** in `build/tests/` (C++) — **25/25 grün**
- **Python-Bridge-Test** (`tools/interpretation_worker/tests/test_parser_bridge.py`)
- **Python-Full-Stack-E2E-Test** (`tools/interpretation_worker/tests/test_full_stack_e2e.py`)
- 12 JSON-Schemas in `schemas/v1/`

### Dokumentation

| Datei | Zweck |
|-------|-------|
| `README.md` | Projektüberblick |
| `docs/ARCHITECTURE.md` | System-Architektur, Modul-Struktur, Interfaces |
| `docs/SPECIFICATION.md` | Kern-Prinzipien, Artefakt-Typen, MVP-Scope V1 |
| `docs/GOVERNANCE.md` | Trust-Modell, LLM-Regeln, Change-Control |
| `docs/DEVELOPMENT.md` | Build, Tests, Konventionen |
| `docs/phases/*.md` | **dieser Phasen-Breakdown** |

---

## Aktueller MVP-Scope (V1)

**Funktioniert (in Code + Tests):**

```
ingress_event → task (optional) → option (emit_report)
  → verification_report (pass) → approval (approve)
  → action (report_emit) → execution_result
```

**Constraints V1:** Kein Shell, kein Netzwerk, kein `file_write`. Nur `report_emit`. Kein DB — In-Memory-Store.

---

## Die 5 wichtigsten offenen Items (Details in Phase-Files)

| Prio | Item | Phase | Stand 7.6.2026 |
|------|------|-------|----------------|
| 1 | `StoreSQLite` implementieren (V1-Voraussetzung) | 0 | ✅ erledigt |
| 2 | `WriteFileStep` / `ApiCallStep` Executors fehlen | 3 | offen |
| 3 | Doppelte Header-Layouts in `src/execution/include/` aufräumen | 3 | offen |
| 4 | End-to-End-Integrationstest der Interpretation-Pipeline | 4 | ✅ erledigt |
| 5 | CI + clang-format/clang-tidy einrichten | 5 | offen |

---

## Schnellreferenz (Befehle)

| | |
|---|---|
| Build | `cmake -S . -B build && cmake --build build` |
| Tests | `ctest --test-dir build --output-on-failure` |
| App | `./build/app/arcs_app` |
| Worker | `python -m tools.interpretation_worker --config config/arcs.yaml` |
| Parser (separat, sibling) | `uvicorn main:app --port 8000` in `../text-to-json-parser/` |
| Config | `config/arcs.yaml` (Vorlage: `arcs.yaml.example`) |
