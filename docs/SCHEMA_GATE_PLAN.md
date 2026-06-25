# SCHEMA_GATE_PLAN

## Zweck

Diese Datei haelt den aktuellen Stand rund um Schema- und Commit-Gates fest
und definiert die naechsten konkreten Schritte, um spaeter direkt
weiterarbeiten zu koennen.

---

## Stand jetzt

Bereits erledigt:

- `docs/docs_update.md` auf aktuellen Repo-Stand geschaerft
- `docs/SCHEMA_CONTRACT.md` erstellt
- `docs/MVP_FLOW.md` erstellt
- `docs/TEST_MATRIX.md` erstellt
- `docs/SPECIFICATION.md` auf das neue Doku-Modell umgestellt
- Full-Stack-E2E-Test fuer die Interpretationskette vorhanden
- minimaler zentraler Commit-Gate im Store eingebaut

Der aktuelle Commit-Gate prueft in beiden Stores:

- `StoreMemory::commit()`
- `StoreSqlite::commit()`

Aktuell erzwungen:

- Artefakt muss gegen `arcs.artifact_base.v1` valide sein
- leere `schema_id` wird rejected
- leerer `stream_key` wird rejected
- fehlende Basisfelder werden rejected

Bereits gruen verifiziert:

- `arcs_store_memory_tests`
- `arcs_store_sqlite_tests`
- `arcs_text_flow_tests`

---

## Warum noch kein voller typspezifischer Schema-Gate aktiv ist

Die im Core erzeugten Artefakte passen noch nicht sauber zu den vorhandenen
Payload-Schemas. Ein sofortiger globaler Gate gegen jedes typspezifische Schema
wuerde den aktuellen MVP-Flow brechen.

---

## Konkrete Gaps

### 1. `task`

Code erzeugt aktuell:

- `title`
- `description`
- `approval`
- `permission`
- `policy_drift`

Schema kennt aktuell nur:

- `title`
- `description`
- `scope`
- `priority`

Folge:

- typspezifische Payload-Validierung wuerde `task` aktuell rejecten

### 2. `option`

Code erzeugt aktuell:

- `title`
- `request`
- `policy_ref`
- `requires_permissions`
- `required_scopes`
- `steps`

Schema verlangt aktuell:

- `title`
- `human_summary`
- `steps`
- `requires_permissions`
- `safety_level`

Folge:

- `option` ist aktuell der groesste Schema-Bruch im MVP-Flow

### 3. `decision`

Code committed:

- `schema_id = arcs.decision.v1`

Aber:

- in `schemas/v1/` existiert kein `decision`-Schema

Folge:

- ein vollstaendiger typspezifischer Commit-Gate ist unmoeglich, solange
  `decision` schema-los bleibt

### 4. `execution_result`

Code erzeugt aktuell Payload mit:

- `action_ref`
- `status`
- `exit_code`
- `error_message`
- `logs`

Schema erwartet aktuell:

- `action_id`
- `status`
- optional `output`

Folge:

- klares Mismatch zwischen echtem Artefakt und Schema

### 5. `action`

Aktuell inkonsistent auf drei Ebenen:

- Artefakt-`schema_id = arcs.action.v1`
- Payload-intern `schema_id = actions/report_emit@v1`
- vorhandenes Schema: `arcs.action.report_emit.v1`

Folge:

- vor globaler Validierung muss die Benennung vereinheitlicht werden

---

## Empfohlene Reihenfolge

### Phase A: MVP-Payload-Schemas an den echten Flow anpassen

1. `schemas/v1/task.schema.json` harmonisieren
2. `schemas/v1/option.schema.json` harmonisieren
3. neues `schemas/v1/decision.schema.json` anlegen

Ziel:

- der heutige Core-Flow fuer `task`, `option`, `decision` wird formal schema-faehig

### Phase B: Execution-Artefakte harmonisieren

4. `schemas/v1/execution_result.schema.json` an echten Payload anpassen
5. Action-Schema-Namen vereinheitlichen:
   - entweder Artefakt auf `arcs.action.report_emit.v1`
   - oder Schema auf die aktuelle Struktur anpassen

Ziel:

- auch `action` und `execution_result` werden sauber validierbar

### Phase C: Typspezifischen Commit-Gate scharf schalten

6. im Store-Commit zusaetzlich zur Basis-Validierung das referenzierte Schema pruefen
7. neue Negativ-Tests ergaenzen:
   - unbekannte `schema_id`
   - invalides `task` payload
   - invalides `option` payload
   - invalides `decision` payload

Ziel:

- kein relevantes Artefakt kann mehr ohne typspezifische Schema-Validierung committed werden

---

## Konkrete naechste minimale Aufgabe

Empfohlener naechster PR- oder Arbeitsschritt:

1. `task.schema.json` aktualisieren
2. `option.schema.json` aktualisieren
3. `decision.schema.json` neu anlegen
4. passende Schema-/Store-Tests ergaenzen

Warum genau dieser Schritt:

- klein genug fuer eine saubere Einheit
- direkt im MVP-Core-Pfad wirksam
- noetig, bevor der strenge typspezifische Gate aktiviert werden kann

---

## Relevante Dateien

Code:

- `src/core/src/flow.cpp`
- `src/approval/src/approval_gate.cpp`
- `src/verification/src/verification_engine.cpp`
- `src/execution/src/action_materializer.cpp`
- `src/store/src/store_memory.cpp`
- `src/store/src/store_sqlite.cpp`

Schemas:

- `schemas/v1/task.schema.json`
- `schemas/v1/option.schema.json`
- `schemas/v1/policy.schema.json`
- `schemas/v1/approval.schema.json`
- `schemas/v1/verification_report.schema.json`
- `schemas/v1/execution_result.schema.json`
- `schemas/v1/action.report_emit.schema.json`

Docs:

- `docs/SCHEMA_CONTRACT.md`
- `docs/MVP_FLOW.md`
- `docs/TEST_MATRIX.md`
- `docs/SPECIFICATION.md`

Tests:

- `tests/test_store_memory.cpp`
- `tests/test_store_sqlite.cpp`
- `tests/test_text_flow.cpp`
- `tests/test_materializer.cpp`
- `tools/interpretation_worker/tests/test_full_stack_e2e.py`

---

## Definition of Done fuer den naechsten Schritt

Der naechste Schritt ist fertig, wenn:

- `task`, `option` und `decision` je ein passendes Schema haben
- die aktuellen MVP-Artefakte gegen diese Schemas valide waeren
- neue oder angepasste Tests gruen sind
- die Doku nicht mehr behauptet, dass diese Artefakte schema-seitig offen sind
