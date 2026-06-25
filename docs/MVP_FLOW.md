# MVP_FLOW

## Zweck

Diese Datei beschreibt den einen echten ARCS-V1-MVP-Flow so, wie er heute im
Repo implementiert und getestet ist.

Sie ist kein Zielbild fuer spaeter, sondern eine Beschreibung des aktuellen
Pfads durch den Core.

---

## Geltungsbereich

Der dokumentierte V1-Flow ist absichtlich eng:

- Input: CLI-Text
- Planung: genau eine `option`
- Step-Kind: `emit_report`
- Action-Typ: `report_emit`
- Execution: `ReportEmitExecutor`
- Fail-Closed bei fehlender Permission, fehlender Approval oder Policy Drift

Nicht Teil dieses MVP-Flows:

- `file_write`
- `api_call`
- Shell-Ausfuehrung
- weitere Ingress-Quellen

---

## Flow in Kurzform

```text
input
  -> ingress_event
  -> task
  -> option { step.kind = emit_report }
  -> verification_report
  -> approval
  -> action { type = report_emit }
  -> execution_result
  -> decision
```

Es gibt zwei Input-Varianten:

1. direktes `key=value`-Input, z. B. `approval=yes permission=yes`
2. Freitext, der erst ueber `Core -> Worker -> Parser -> Core` in ein
   `interpretation_proposal` uebersetzt wird

---

## Echte Referenz im Code

- Orchestrierung: `src/core/src/flow.cpp`
- Einstieg: `app/main.cpp`
- C++ API: `src/core/include/core/flow.hpp`

Der MVP-Flow wird aktuell zentral durch
`arcs::core::run_text_flow(...)` beschrieben.

---

## Schritt fuer Schritt

| Schritt | Input | Output | Gate | Referenz | Test |
|---|---|---|---|---|---|
| Parse Input | CLI-Text | `ParsedInput` oder Freitext-Routing | leeres Input blockt | `src/core/src/flow.cpp` | `tests/test_text_flow.cpp` |
| Interpretation | Freitext | `interpretation_proposal` | externer Worker muss antworten, sonst block | `src/core/src/flow.cpp`, `tools/interpretation_worker/` | `tools/interpretation_worker/tests/test_full_stack_e2e.py` |
| Ingress | Raw Input | `ingress_event` | Ingress-Validierung / Quarantaene | `src/ingress/` | `tests/test_text_flow.cpp` |
| Task-Ableitung | `ingress_event` + `ParsedInput` | `task` | kein separates hartes Gate im Flow | `src/core/src/flow.cpp` | `tests/test_text_flow.cpp` |
| Option-Ableitung | `task` + `policy_ref` | `option` mit `emit_report` | Policy-Bindung und Required Permissions werden gesetzt | `src/core/src/flow.cpp` | `tests/test_text_flow.cpp`, `tests/test_materializer.cpp` |
| Verification | `option` + effektive Permissions | `verification_report` | Permission, Scope, Approval, Policy Drift | `src/verification/`, `src/core/src/flow.cpp` | `tests/test_verification_engine.cpp`, `tests/test_unknown_blocks.cpp`, `tests/test_text_flow.cpp` |
| Approval | Verification `pass` | `approval` | Approval nur nach erfolgreicher Verification | `src/approval/`, `src/core/src/flow.cpp` | `tests/test_text_flow.cpp` |
| Materialization | `option` + `policy` | `action` | nur typed steps, kein Freitext | `src/execution/src/action_materializer.cpp` | `tests/test_materializer.cpp` |
| Execution | `action` + `ExecutionContext` | `execution_result` | Approval/Verification/Permission muessen passen, Idempotenz gilt | `src/execution/` | `tests/test_happy_path.cpp`, `tests/test_idempotency.cpp`, `tests/test_revocation.cpp` |
| Commit | erzeugte Artefakte | Store-Write | Bundle atomar, keine doppelten IDs, Optimistic Lock | `src/store/` | Store-Tests |
| Decision | Gesamtzustand | `decision: blocked` oder `decision: not blocked` | fail-closed | `src/core/src/flow.cpp` | `tests/test_text_flow.cpp` |

---

## Happy Path

Der kleinste erfolgreiche V1-Lauf ist heute:

```text
approval=yes permission=yes
```

Erwartetes Ergebnis:

```text
step: ingress_event -> OK
step: task -> OK
step: option -> OK
step: verification_report -> OK | pass
step: approval -> OK
decision: not blocked
reason: approval=yes and permission=yes
```

Beweis:

- `tests/test_text_flow.cpp`

---

## Fail-Closed-Faelle

Der MVP-Flow ist nur dann korrekt, wenn er nicht nur den Happy Path kann,
sondern definierte Blockaden liefert.

### Fehlende Permission

Input:

```text
approval=yes permission=no
```

Erwartung:

- Verification faellt auf `FAIL`
- Entscheidung wird `blocked`

Beweis:

- `tests/test_text_flow.cpp`

### Fehlende Approval

Input:

```text
approval=no permission=yes
```

Erwartung:

- Approval-Check faellt auf `FAIL`
- Entscheidung wird `blocked`

Beweis:

- `tests/test_text_flow.cpp`

### Policy Drift

Input:

```text
approval=yes permission=yes policy_drift=yes
```

Erwartung:

- Policy-Bindung ist stale
- Verification blockt

Beweis:

- `tests/test_text_flow.cpp`

### Freitext ohne verfuegbare Interpretation

Input:

```text
bitte erstelle einen bericht als json ueber die letzten pruefergebnisse
```

Erwartung:

- wenn kein Worker/Parser verfuegbar ist: `decision: blocked`
- Grund: `free text interpretation unavailable`

Beweis:

- `tests/test_text_flow.cpp`
- `tools/interpretation_worker/tests/test_full_stack_e2e.py`

---

## Full-Stack-Freitextpfad

Wenn die externe Interpretationskette verfuegbar ist, verlaeuft der Freitextpfad
heute so:

```text
raw text
  -> ARCS Core
  -> POST /interpret
  -> interpretation_worker
  -> POST /generate-json
  -> text-to-json-parser
  -> interpretation_proposal
  -> ARCS Core
  -> task/option/verification/approval/action/execution_result
```

Beweis:

- `tools/interpretation_worker/tests/test_full_stack_e2e.py`

---

## Was dieser MVP-Flow heute noch nicht beweist

- dass jeder Artefakt-Commit zentral und hart ueber einen einheitlichen Schema-Gate laeuft
- dass alle negativen Artefaktfaelle vor Commit abgefangen werden
- dass `LLM-Provenance` bereits durch den Worker erzwungen wird
- dass der Flow ueber den aktuellen `report_emit`-Scope hinaus stabil ist

Das ist bewusst nicht Aufgabe dieser Datei. Diese Luecken muessen in
`SCHEMA_CONTRACT.md` und spaeter in `TEST_MATRIX.md` sichtbar gemacht werden.

---

## Direkte Anschlussarbeit

1. `TEST_MATRIX.md` schreiben.
2. `SPECIFICATION.md` auf `SCHEMA_CONTRACT.md` und diese Datei referenzieren.
3. Commit-Gates im Code auf einen zentralen Schema-Validierungspfad pruefen.
