# DEVELOPMENT

## Build

### Voraussetzungen

- CMake >= 3.20
- C++20 Compiler (GCC, Clang, MSVC)
- Kein manuelles Installieren von Dependencies nötig — werden via FetchContent geladen

### Dependencies (automatisch)

| Library | Zweck | Quelle |
|---------|-------|--------|
| nlohmann/json 3.11.3 | JSON-SerDe | System oder FetchContent |
| valijson 1.1.0 | JSON Schema Validierung (Draft 7) | GitHub |
| GoogleTest 1.16.0 | Unit-Tests | GitHub |

### Bauen

```bash
cmake -B build -DCMAKE_BUILD_TYPE=Debug
cmake --build build
```

### Tests

```bash
cmake --build build --target test
# oder
ctest --test-dir build --output-on-failure
```

### LLM-Konfiguration

Die CLI liest optional `config/arcs.yaml` aus dem Projektroot. Starte am besten mit `cp config/arcs.yaml.example config/arcs.yaml`.

Die vier externen APIs und die Prompt-Datei werden ausschließlich über `config/arcs.yaml` konfiguriert.

### text-to-json-parser als externer Interpretation-Service

ARCS erwartet, dass der text-to-json-parser (sibling repository
`../text-to-json-parser/`) als eigener FastAPI-Service läuft. Der
`interpretation_worker` delegiert dorthin.

Setup:

```bash
# 1. Parser-Service starten
cd ../text-to-json-parser
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt    # siehe ../text-to-json-parser/README.md
uvicorn main:app --host 127.0.0.1 --port 8000

# 2. ARCS interpretation_worker starten (eigene Shell)
cd ../ARCS
python -m venv .venv && source .venv/bin/activate
pip install -r tools/requirements.txt
python -m tools.interpretation_worker --config config/arcs.yaml
```

Der ARCS Core ruft `POST /interpret` auf `127.0.0.1:8090` auf. Der
Worker mappt das intern auf `POST /generate-json` auf
`127.0.0.1:8000`. Stoppe den Parser, schlägt `/interpret` mit HTTP 502
fehl — fail-closed.

### Nur die Bridge-Tests laufen lassen (kein C++-Build nötig)

```bash
cd ARCS
python -m venv .venv && source .venv/bin/activate
pip install httpx
python -m unittest tools.interpretation_worker.tests.test_parser_bridge -v
```

### Full-Stack-E2E-Test der Interpretation laufen lassen

Der Test startet den echten Parser mit `LLM_BACKEND=echo`, startet den
echten Worker und ruft danach `arcs_app` gegen diesen Stack auf.

```bash
cmake -S . -B build
cmake --build build --target arcs_app
ctest --test-dir build -R arcs_full_stack_e2e_tests --output-on-failure
```

### Einzelnen Test ausführen

```bash
./build/tests/test_happy_path
./build/tests/test_store_memory
```

## Projektstruktur

```
ARCS/
├── CMakeLists.txt              # Root-CMake, Dependencies
├── README.md                   # Projektüberblick
├── docs/                       # Diese Dokumente
├── app/                        # CLI-Applikation (Entry Point)
│
├── src/
│   ├── artifact/               # Kern-Datentypen: ArtifactVersion, ActorRef, Provenance, IDs
│   ├── event/                  # Event-Typen und JSON-SerDe
│   ├── store/                  # IStore, StoreMemory, Head-Tracking, Optimistic Lock
│   ├── schema/                 # SchemaRegistry, Loader, Validator
│   ├── reducer/                # IReducer<TState>, TaskState, ApprovalState, Permissions
│   ├── verification/           # IVerifier, VerificationEngine, 6 Core-Verifier
│   ├── approval/               # IApprovalGate, ApprovalGate
│   ├── execution/              # IExecutor, Materializer, AdapterRegistry, Executors
│   ├── policy/                 # Policy, PermissionGrant
│   ├── core/                   # Orchestrierung: run_text_flow()
│   └── adapters/               # Input (CLI), externe API-Verträge
│
├── tests/                      # GoogleTest-basierte Tests
└── schemas/                    # JSON Schema-Dateien
```

## Architektur-Patterns

### Neues Modul hinzufügen

1. **Verzeichnis erstellen:** `src/my_module/`
2. **CMakeLists.txt** nach Vorbild eines bestehenden Moduls
3. **Interfaces implementieren:**
   - Neue Artefakt-Typen → Schema unter `schemas/` registrieren
   - Neuer Step-Kind → Manifest mit `step_kinds[]`
   - Neuer Executor → `IExecutor` implementieren, in `AdapterRegistry` registrieren
   - Neuer Verifier → `IVerifier` implementieren
4. **In Root-CMake eintragen:** `add_subdirectory(src/my_module)`

### Ein bestehendes Interface implementieren

Beispiel: Neuer Executor für `send_reply`:

```cpp
class SendReplyExecutor : public IExecutor {
public:
    ExecutionResult execute(const Action& action, const ExecutionContext& ctx) override {
        // action.params auslesen
        // Reply senden
        // ExecutionResult zurückgeben (mit Idempotenz-Check)
    }

    std::string handles_action_type() const override {
        return "send_reply";
    }
};
```

Registrieren:

```cpp
registry.register_executor(std::make_unique<SendReplyExecutor>());
```

## Code-Konventionen

- **Namespace:** `arcs::` + Modulname (z.B. `arcs::artifact`, `arcs::execution`)
- **Dateien:** `.hpp` für Header, `.cpp` für Implementation
- **Header-Guards:** `#pragma once`
- **Naming:**
  - Klassen: `PascalCase` (`ArtifactVersion`, `VerificationEngine`)
  - Interfaces: `I`-Prefix (`IStore`, `IVerifier`, `IExecutor`)
  - Methoden/Funktionen: `snake_case` (`run_all`, `has_artifact`)
  - Member: `snake_case` mit `_`-Suffix nicht nötig (direkt benannt)
  - Enums: `PascalCase` für Typ, `PascalCase` für Werte (`CheckStatus::Pass`)
- **JSON-SerDe:** `to_json` / `from_json` im selben Namespace wie der Typ
- **Const-Correctness:** `const`-Referenzen für Input, `const`-Methoden wo möglich

## Tests

### Test-Dateien

| Test | Was er prüft |
|------|-------------|
| `test_happy_path.cpp` | Vollständiger Flow von Ingress bis Execution |
| `test_unknown_blocks.cpp` | unknown in Verifier blockt die Pipeline |
| `test_policy_drift.cpp` | Policy-Wechsel nach Approval → Re-Verify |
| `test_idempotency.cpp` | Doppelte action_id → nur ein Resultat |
| `test_revocation.cpp` | Approval-Revocation während Execution |
| `test_replay.cpp` | Event Log → Reducer → identischer State |
| `test_llm_isolation.cpp` | LLM-Output mit falschem Schema → rejected |
| `test_approval.cpp` | Approval-Decisionen und State-Übergänge |
| `test_verification_engine.cpp` | Verifier-Aggregation und Report-Erstellung |
| `test_authority_verifier.cpp` | Permission-Checks für Policy-Änderungen |
| `test_store_memory.cpp` | Store-Operationen, Optimistic Lock, Head-Tracking |
| `test_schema_registry.cpp` | Schema-Validierung |
| `test_materializer.cpp` | Option → Action Materialisierung |
| `test_task_state_reducer.cpp` | Task-Status-Reduktion |
| `test_approval_state_reducer.cpp` | Approval-Status-Reduktion |
| `test_permission_ttl.cpp` | Permission TTL und Gültigkeit |
| `test_policy_change_control.cpp` | Governance für Policy-Änderungen |
| `phase1_tests.cpp` | Phase-1-Integrationstests |
| `tools/interpretation_worker/tests/test_parser_bridge.py` | Worker-Bridge isoliert ohne C++-Build |
| `tools/interpretation_worker/tests/test_full_stack_e2e.py` | Echter Stack: Parser + Worker + `arcs_app` |

### Test-Prinzipien

- `StoreMemory` für Tests — keine persistente DB nötig
- `MockTimeSource` für deterministische Zeitstempel
- Jeder Test committet isoliert — keine Side-Effects zwischen Tests
