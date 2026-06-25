# ARCS Docs Update

**Stand:** 25. Juni 2026  
**Ziel:** Die Dokumentation soll nicht mehr nur das Zielbild beschreiben, sondern den echten Engineering-Stand des Repos sauber abbilden.

---

## 1. Warum dieses Docs-Update nötig ist

ARCS hat inzwischen genug Code, Tests und Struktur, dass die Dokumentation nicht mehr nur Konzept-Spec sein darf. Die Docs müssen jetzt drei Dinge trennen:

1. **Zielbild**  
   Was ARCS langfristig garantieren soll.

2. **V1-Vertrag**  
   Was der aktuelle Core in Version 1 wirklich leisten muss.

3. **Aktueller Repo-Stand**  
   Was im Code bereits funktioniert, was nur Demo ist und was noch offen ist.

Aktuell vermischen sich diese Ebenen teilweise. Dadurch entsteht das Risiko, dass eine Phase als „fertig“ wirkt, obwohl sie nur teilweise implementiert oder nur als Demo-Flow vorhanden ist.

Seit diesem Update-Entwurf wurden bereits erste Korrekturen im Repo gemacht:

- `STATE.md`, `README.md`, `docs/DEVELOPMENT.md` und `docs/phases/*` wurden auf den neuen Full-Stack-E2E-Test abgeglichen.
- Die Interpretation-Pipeline hat jetzt einen automatisierten Vollketten-Test.
- Die Grundkritik dieser Datei bleibt aber bestehen: Die Doku ist noch nicht konsequent als Engineering-Vertrag aufgebaut.

---

## 2. Hauptproblem der aktuellen Docs

Die Architektur ist stark, aber die Dokumentation ist an mehreren Stellen nicht hart genug an den Code gekoppelt.

### Kritische Inkonsistenzen

| Bereich | Problem |
|---|---|
| `STATE.md` | Snapshot und Status müssen weiter stärker zwischen „stabil“, „teilweise stabil“ und „Demo“ trennen. |
| `docs/phases/README.md` | Zahlen wurden angeglichen, aber der Phasenindex ist noch kein echter Test-/Garantie-Überblick. |
| `docs/SPECIFICATION.md` | Beschreibt harte Prinzipien, aber nicht klar genug, wo der Code diese schon erzwingt. |
| V1-Flow | Der Interpretationspfad ist jetzt E2E-getestet, aber Schema-Gates und Commit-Garantien sind noch nicht sauber als Vertrag dokumentiert. |
| Schema First | Muss als zwingendes Commit-Gate dokumentiert werden, nicht nur als Prinzip. |

Der wichtigste Satz für die neue Doku:

> ARCS ist erst dann V1-stabil, wenn kein Artefakt committed werden kann, das nicht gegen sein Schema und seine Governance-Regeln validiert wurde.

---

## 3. Neue Dokumentationsstruktur

Die Docs sollten ab jetzt wie ein Engineering-System aufgebaut sein, nicht wie ein Ideenpapier.

Empfohlene Struktur:

```text
README.md
STATE.md
/docs
  ARCHITECTURE.md
  SPECIFICATION.md
  GOVERNANCE.md
  DEVELOPMENT.md
  SCHEMA_CONTRACT.md
  MVP_FLOW.md
  TEST_MATRIX.md
  DOCS_UPDATE.md
  /phases
    README.md
    phase-0-foundation.md
    phase-1-determinism.md
    phase-2-governance.md
    phase-3-execution.md
    phase-4-interpretation.md
    phase-5-v2-tooling.md
```

---

## 4. Neue Rollen der wichtigsten Docs

### `README.md`

Zweck: Einstieg für Menschen, die das Projekt öffnen.

Muss enthalten:

- Was ist ARCS in 5 Sätzen?
- Was ist der aktuelle V1-Scope?
- Wie baut man das Projekt frisch?
- Wie laufen Tests?
- Welche Dinge sind bewusst nicht in V1?

Nicht enthalten:

- Tiefe Architekturdetails
- Lange Philosophie
- Vollständige Spec

---

### `STATE.md`

Zweck: Ein ehrlicher Snapshot des aktuellen Stands.

Muss enthalten:

```text
Datum:
Commit / Snapshot:
Build-Status:
Test-Status:
Stabile Komponenten:
Teilweise stabile Komponenten:
Demo-Komponenten:
Offene Blocker:
Nächster Engineering-Schritt:
```

Wichtig: `STATE.md` darf nicht optimistisch formuliert sein. Wenn etwas nur im Demo-Flow funktioniert, muss dort „Demo“ stehen. Wenn etwas E2E-getestet ist, aber noch kein hartes Commit-Gate besitzt, muss das ebenfalls explizit genannt werden.

Beispiel:

```text
Execution:
- ReportEmitExecutor vorhanden
- Idempotency-Tests vorhanden
- file_write/api_call/shell bewusst nicht V1
- Interpretations-Flow ist Full-Stack getestet
- Commit-/Schema-Gate für Artefakt-Erzeugung noch nicht hart genug dokumentiert
```

---

### `docs/SPECIFICATION.md`

Zweck: Der Core-Vertrag.

Diese Datei sollte nicht ständig geändert werden. Sie beschreibt, was ARCS garantieren muss.

Muss klarer trennen:

```text
Prinzip:
Schema First

V1-Regel:
Kein Commit ohne bekannte schema_id und gültigen Payload.

Code-Anforderung:
Store/Commit-Pfad muss SchemaRegistry zwingend verwenden.

Test-Beweis:
Invalid task / invalid option / invalid action wird rejected und erzeugt keine Head-Änderung.
```

---

### `docs/SCHEMA_CONTRACT.md`

Zweck: Der wichtigste neue Vertrag.

Diese Datei sollte neu erstellt werden.

Inhalt:

```text
# SCHEMA_CONTRACT

## Grundregel
Kein Artefakt darf committed werden, wenn es nicht vollständig schema-valide ist.

## Commit-Voraussetzungen
Ein Artefakt darf nur committed werden, wenn:

1. `schema_id` existiert.
2. Base Artifact Schema gültig ist.
3. Payload gegen spezifisches Schema gültig ist.
4. `stream_key` vorhanden ist.
5. `created_by` gültig ist.
6. `trust` gültig ist.
7. abgeleitete Artefakte `provenance.parents` haben.
8. referenzierte Artefakte existieren.
9. Commit-Bundle Artefakte und Events atomar enthält.

## Verboten
- Kein loser JSON-Blob nach Ingress.
- Kein Commit mit unbekannter `schema_id`.
- Kein Demo-Bypass.
- Kein Executor darf Actions aus Freitext ableiten.
- Kein LLM darf `action`, `approval`, `policy` oder `execution_result` committen.
```

---

### `docs/MVP_FLOW.md`

Zweck: Der eine echte V1-Flow.

Diese Datei sollte den Flow nicht abstrakt, sondern testbar beschreiben.

```text
ingress_event
  -> task
  -> option { step.kind = emit_report }
  -> verification_report { status = pass }
  -> approval { decision = approve }
  -> action { type = report_emit }
  -> execution_result { status = success }
```

Für jeden Schritt muss dokumentiert werden:

| Schritt | Input | Output | Gate | Test |
|---|---|---|---|---|
| Ingress | raw input | ingress_event | schema | ingress test |
| Task | ingress_event | task | schema | task schema test |
| Option | task | option | schema + permission | option test |
| Verification | option | verification_report | verifier engine | unknown blocks |
| Approval | verification pass | approval | policy_ref + expires_at | approval test |
| Materializer | approved option | action | typed DSL only | materializer test |
| Execution | action | execution_result | final guards | happy path |

---

### `docs/TEST_MATRIX.md`

Zweck: Tests beweisen Status. Keine Phase gilt ohne Tests als fertig.

Muss enthalten:

| Test | Status | Beweist | Phase |
|---|---|---|---|
| Schema invalid rejected | offen/ok | Schema First | 0/2 |
| Concurrent Commit | ok/offen | Optimistic Lock | 0 |
| Replay deterministic | ok/offen | Reducer-Regel | 1 |
| Unknown blocks | ok/offen | Fail-Closed | 2 |
| Policy Drift | ok/offen | TOCTOU-Schutz | 2 |
| Approval expired | ok/offen | TimeSource/ApprovalVerifier | 2 |
| Idempotency | ok/offen | Safe retry | 3 |
| Revocation | ok/offen | Execution stoppt | 3 |
| LLM invalid schema rejected | offen/ok | LLM-Isolation | 4 |
| Fresh clone build | offen | Repo-Sauberkeit | 5 |

Wichtig: „Test existiert“ und „Test beweist den echten Flow“ sind nicht dasselbe. Das muss in der Matrix getrennt werden.

---

## 5. Phasen neu bewerten

Die aktuelle Phasenstruktur 0–5 ist gut. Sie sollte bleiben.

Aber jede Phase braucht harte Done-Kriterien.

---

### Phase 0 — Foundation

**Ziel:** Artifact, Event, Schema, Store, Commit-Boundary.

Fertig nur wenn:

- Fresh build funktioniert.
- `build/` wird nicht als Wahrheit betrachtet.
- SchemaRegistry lädt alle V1-Schemas.
- Store kann atomare Bundles committen.
- Kein Event ohne Artefakt.
- Kein Artefakt ohne Event.
- Unknown schema_id wird rejected.
- Concurrent Commit Test ist grün.

Status: **größtenteils vorhanden, aber Build-/Snapshot-Doku bereinigen.**

---

### Phase 1 — Determinism

**Ziel:** Reducer und Replay.

Fertig nur wenn:

- Reducer sind pure functions.
- Keine Sortierung nach `occurred_at`.
- Keine direkte Systemzeit in Reducern.
- ITimeSource ist injiziert.
- Replay ergibt denselben DerivedState wie Live.

Status: **gut fortgeschritten.**

---

### Phase 2 — Governance

**Ziel:** Verification, Policy, Permission, Approval.

Fertig nur wenn:

- `pass/fail/unknown` vollständig umgesetzt ist.
- `unknown` blockt immer.
- Permission fehlt -> fail.
- Scope unklar -> unknown oder fail.
- Approval ist an konkrete `policy_ref` gebunden.
- Policy Drift blockt.
- Approval expired blockt.
- Policy/Permission nicht per Demo-Flag simuliert werden.

Status: **teilweise vorhanden, aber noch härter im Flow verbinden.**

---

### Phase 3 — Execution

**Ziel:** Option -> Action -> Executor -> ExecutionResult.

Fertig nur wenn:

- Materializer akzeptiert nur typed steps.
- Kein Freitext-Parsing im Materializer.
- Action wird separat verifiziert.
- Executor prüft final guards.
- `action_id` ist Idempotency-Key.
- doppelte Action führt zu no-op oder safe replay.
- ReportEmitExecutor ist der einzige V1-Executor.

Status: **teilweise vorhanden, V1 bewusst eng halten.**

---

### Phase 4 — Interpretation

**Ziel:** Input/Parser/LLM-Brücke ohne Autoritätsbruch.

Fertig nur wenn:

- Raw input wird zu `interpretation_proposal`.
- Proposal ist schema-valide.
- Proposal darf keine Action direkt erzeugen.
- LLM/Parser Output hat `trust.level = low`.
- Ungültiger Output wird rejected.
- Interpretation erzeugt maximal Vorschläge, keine Exekution.

Status: **E2E testbar vorhanden, Isolation und Vertragsklarheit weiter härten.**

---

### Phase 5 — V2 & Tooling

**Ziel:** Härtung und Produktionsdisziplin.

Fertig nur wenn:

- Fresh clone build dokumentiert und getestet ist.
- CI läuft.
- clang-format/clang-tidy oder vergleichbare Checks laufen.
- `build/`, `.venv/`, Logs und Artefakt-Ausgaben sind nicht Teil des sauberen Source-Snapshots.
- Schema-Migrationen sind dokumentiert.
- `prev_hash` im Event Log ist eingeführt oder bewusst V2-offen markiert.
- Second-Approver für kritische Policy-Änderungen ist definiert.

Status: **offen.**

---

## 6. Wichtigste Docs-Änderungen sofort

Stand heute:

- Ein Teil dieser Liste ist bereits erledigt.
- Der nächste Fokus sollte nicht mehr auf Snapshot-Kosmetik liegen, sondern auf fehlenden Vertragsdokumenten und auf der Kopplung von Code, Test und Garantie.

### Sofort ändern

```text
STATE.md
- weiter schärfen: „stabil“ vs „teilweise stabil“ vs „Demo“
- echte Blocker und fehlende Garantien explizit nennen
```

```text
docs/phases/README.md
- Phasenstatus stärker an Tests und Garantien koppeln
```

```text
docs/SPECIFICATION.md
- Schema First als Commit-Gate ergänzen
- LLM-Regeln als harte Verbote formulieren
- V1 Scope klarer abgrenzen
```

### Neu erstellen

```text
docs/SCHEMA_CONTRACT.md
docs/MVP_FLOW.md
docs/TEST_MATRIX.md
```

---

## 7. Was bewusst NICHT gemacht werden sollte

Nicht nochmal eine komplett neue große Spec schreiben.

Nicht neue Features dokumentieren, bevor der aktuelle Flow hart ist.

Nicht zu früh `file_write`, `api_call`, Shell oder echte externe Systeme als V1 verkaufen.

Nicht „Phase fertig“ schreiben, wenn nur ein Demo-Test existiert.

Nicht `build/` oder `.venv/` als normalen Projektbestandteil behandeln.

---

## 8. Neuer V1-Fokus

Der nächste echte Meilenstein ist nicht ein neues Modul.

Der nächste echte Meilenstein ist:

```text
Schema-validierter MVP-Flow mit nachweisbaren Commit-Gates
```

Definition:

```text
Ein vollständiger Lauf von ingress_event bis execution_result,
bei dem jedes Artefakt vor Commit gegen seine schema_id validiert wird,
jede relevante Policy/Permission/Approval-Regel greift,
die Interpretation zwar E2E funktioniert, aber keine Autorität erhält,
und Replay denselben DerivedState ergibt.
```

Erst wenn das grün ist, sollte ARCS breiter werden.

---

## 9. Empfohlene Reihenfolge ab jetzt

1. `SCHEMA_CONTRACT.md` schreiben.
2. `MVP_FLOW.md` schreiben und gegen den echten Flow abgleichen.
3. `TEST_MATRIX.md` einführen.
4. `docs/SPECIFICATION.md` auf harte Commit-/Schema-Gates zuspitzen.
5. Commit-Pfad so härten, dass SchemaRegistry nicht umgangen werden kann.
6. Demo-Flow brechen lassen, falls Artefakte invalid sind.
7. Schemas oder Artefakt-Erzeugung korrigieren.
8. Fresh build ohne mitgelieferten `build/`-Ordner testen.
9. CI einrichten.
10. Erst danach neue Step-Kinds oder Domain-Adapter bauen.

---

## 10. Harte Abschlussdefinition für „ARCS V1 Core stabil“

ARCS V1 Core ist erst stabil, wenn alle Punkte erfüllt sind:

```text
[ ] Fresh clone build funktioniert.
[ ] Alle Tests laufen aus frischem Build.
[ ] Kein Commit ohne Schema-Validierung.
[ ] Ungültige Artefakte werden rejected.
[ ] Store-Commit ist atomar.
[ ] Replay ist deterministisch.
[ ] unknown blockt immer.
[ ] Policy Drift blockt.
[ ] Approval expired blockt.
[ ] Materializer nutzt nur typed DSL.
[ ] Action wird separat verifiziert.
[ ] Executor ist idempotent.
[ ] LLM/Parser Output kann keine Action direkt auslösen.
[ ] Interpretationspfad ist E2E-getestet und bleibt fail-closed.
[ ] STATE.md, TEST_MATRIX.md und MVP_FLOW.md stimmen mit dem Code überein.
```

---

## 11. Kurzfazit

Die Docs müssen nicht neu erfunden werden.

Sie müssen härter werden.

Der neue Doku-Stil sollte sein:

```text
Nicht: Was soll ARCS irgendwann sein?
Sondern: Was garantiert ARCS jetzt, wodurch, und welcher Test beweist es?
```

Wenn die Dokumentation das sauber abbildet, wird sie wieder nützlich für die Implementierung und verhindert, dass Konzept, Code und Tests auseinanderlaufen.
