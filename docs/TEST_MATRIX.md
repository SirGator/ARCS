# TEST_MATRIX

## Zweck

Diese Matrix verbindet drei Dinge:

1. welche Garantie ARCS behauptet
2. welcher Test diese Garantie heute belegt
3. ob der Beleg vollstaendig, teilweise oder noch offen ist

Statuswerte in dieser Datei:

- `belegt`: durch vorhandene Tests direkt nachweisbar
- `teilweise belegt`: es gibt relevante Tests, aber die volle Systemgarantie ist noch nicht sauber bewiesen
- `offen`: aktuell kein ausreichender Testbeweis im Repo

---

## Matrix

| Garantie | Status | Beweis / Luecke | Test / Referenz | Phase |
|---|---|---|---|---|
| Schema-Registry lehnt leere `schema_id` bei Registrierung ab | belegt | direkte Registry-Regel | `tests/test_schema_registry.cpp` | 0 |
| Doppelte Schema-Registrierung wird abgelehnt | belegt | direkte Registry-Regel | `tests/test_schema_registry.cpp` | 0 |
| Commit ohne Versions wird abgelehnt | belegt | Store-Gate vorhanden | `src/store/src/store_memory.cpp`, `src/store/src/store_sqlite.cpp` | 0 |
| Commit ohne Events wird abgelehnt | belegt | Store-Gate vorhanden | `src/store/src/store_memory.cpp`, `src/store/src/store_sqlite.cpp` | 0 |
| CommitBundle blockt doppelte IDs | belegt | lokale Bundle-Konsistenz im Store | `src/store/src/store_memory.cpp`, `src/store/src/store_sqlite.cpp` | 0 |
| Unbekannte `schema_id` wird vor jedem relevanten Commit rejected | offen | Ziel im Vertrag, aber kein zentraler End-to-End-Testbeweis | `docs/SCHEMA_CONTRACT.md` | 0 |
| Invalides Payload wird vor jedem relevanten Commit rejected | offen | noch kein harter Gesamtbeweis fuer alle Commit-Pfade | `docs/SCHEMA_CONTRACT.md` | 0 |
| Store-Commit ist atomar | teilweise belegt | Architektur und Implementierung sprechen dafuer, aber expliziter Atomaritaetsfehlschlag-Test fehlt | `docs/SPECIFICATION.md`, `src/store/` | 0 |
| Replay ergibt denselben DerivedState wie Live | belegt | gleicher Log -> gleicher State | `tests/test_replay.cpp` | 1 |
| Reducer respektieren Log-Reihenfolge | belegt | Order-sensitive Replay-Test vorhanden | `tests/test_replay.cpp` | 1 |
| Expired Permission wird ignoriert | belegt | TTL-Logik vorhanden | `tests/test_permission_ttl.cpp` | 1/2 |
| Active Permission bleibt wirksam | belegt | TTL-Logik vorhanden | `tests/test_permission_ttl.cpp` | 1/2 |
| `unknown` blockt immer | belegt | Aggregation und Scope-Fall getestet | `tests/test_unknown_blocks.cpp` | 2 |
| `fail` gewinnt gegen `unknown` | belegt | Verifier-Aggregation getestet | `tests/test_unknown_blocks.cpp` | 2 |
| Fehlende Permission blockt den Flow | belegt | Text-Flow blockt sauber | `tests/test_text_flow.cpp` | 2 |
| Fehlende Approval blockt den Flow | belegt | Text-Flow blockt sauber | `tests/test_text_flow.cpp` | 2 |
| Policy Drift blockt | belegt | isolierter Drift-Test plus Text-Flow-Test | `tests/test_policy_drift.cpp`, `tests/test_text_flow.cpp` | 2 |
| Approval expiry blockt Execution | belegt | Revocation-/Expiry-Verhalten im Executor getestet | `tests/test_revocation.cpp` | 2/3 |
| Materializer erzeugt `report_emit` Action aus typisiertem Step | belegt | Happy Materialization getestet | `tests/test_materializer.cpp` | 3 |
| Materializer nutzt deterministische Idempotency-Keys | belegt | wiederholte Materialization wird verglichen | `tests/test_materializer.cpp` | 3 |
| Materializer rejectet kaputte Optionen robust | teilweise belegt | einzelne Negativfaelle getestet, aber nicht vollstaendig | `tests/test_materializer.cpp` | 3 |
| Idempotenz: gleiche `action_id` liefert gespeichertes Resultat | belegt | direkter Executor-Test | `tests/test_idempotency.cpp` | 3 |
| Revocation / ungueltige Approval stoppt Execution | belegt | direkter Executor-Test | `tests/test_revocation.cpp` | 3 |
| Happy Path `report_emit` erzeugt `execution_result=success` | belegt | direkter Executor-Test | `tests/test_happy_path.cpp` | 3 |
| Freitext ohne externe Interpretation blockt fail-closed | belegt | Core-Flow-Test vorhanden | `tests/test_text_flow.cpp` | 4 |
| Worker ↔ Parser Bridge funktioniert isoliert | belegt | Bridge-Test ohne C++-Build | `tools/interpretation_worker/tests/test_parser_bridge.py` | 4 |
| Vollstaendige Interpretationskette `Core -> Worker -> Parser -> Core` funktioniert | belegt | echter Full-Stack-E2E-Test vorhanden | `tools/interpretation_worker/tests/test_full_stack_e2e.py` | 4 |
| Parser-Ausfall fuehrt zu fail-closed Verhalten | belegt | Full-Stack-E2E prueft 502/blocked-Pfad | `tools/interpretation_worker/tests/test_full_stack_e2e.py` | 4 |
| Ungueltiger LLM-Kandidat wird vor Execution rejected | teilweise belegt | Test existiert, aber noch eher Regeltest als echter End-to-End-Beweis | `tests/test_llm_isolation.cpp` | 4 |
| LLM-Provenance wird hart erzwungen | offen | in Governance gefordert, aktuell nicht workerseitig erzwungen | `docs/GOVERNANCE.md` | 4 |
| Fresh clone build funktioniert | offen | bisher kein expliziter Fresh-Clone-Test im Repo | `docs/docs_update.md` | 5 |
| Alle Tests laufen aus frischem Build | teilweise belegt | lokal nachvollziehbar, aber kein CI- oder Fresh-Clone-Beweis | `docs/DEVELOPMENT.md` | 5 |
| CI prueft Build und Test automatisch | offen | keine CI-Konfiguration als Beweis | `docs/phases/phase-5-v2-tooling.md` | 5 |

---

## Wichtigste aktuelle Luecken

Die groessten offenen oder nur teilweise belegten Garantien sind aktuell:

1. harter Schema-Gate vor jedem relevanten Commit
2. invalides Payload wird systemweit vor Commit rejected
3. LLM-Provenance wird erzwungen
4. Fresh-clone- und CI-Beweis fuer Build/Test-Disziplin

---

## Wie diese Matrix genutzt werden soll

- `STATE.md` soll die wichtigsten roten oder gelben Punkte zusammenfassen.
- `SCHEMA_CONTRACT.md` beschreibt die gewollten Gates.
- `MVP_FLOW.md` beschreibt den echten Ist-Flow.
- Diese Matrix beantwortet die Frage: Welche Garantie ist schon testbar belegt?

---

## Naechste direkte Anschlussarbeit

1. `SPECIFICATION.md` auf `SCHEMA_CONTRACT.md`, `MVP_FLOW.md` und diese Matrix verweisen.
2. Offene Garantien aus Phase 0 und 4 in Code oder Tests haerten.
3. CI einfuehren, damit `belegt` nicht nur lokal gilt.
