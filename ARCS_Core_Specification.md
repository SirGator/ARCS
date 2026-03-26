**ARCS**

Core Platform Specification

Core Platform Specification

2026

+-----------------------------------------------------------------------+
| **Core Prinzipien**                                                   |
|                                                                       |
| Der Core definiert nur Regeln, nicht Funktionen.                      |
|                                                                       |
| Fail-Closed: unknown = BLOCK                                          |
|                                                                       |
| LLM ist nie Autoritüt | Immutable Logs | Schema First               |
+=======================================================================+
+-----------------------------------------------------------------------+

*Alles andere ist nur Erweiterung.*

# Inhaltsverzeichnis

Inhaltsverzeichnis

Teil I: Was ist ARCS Core

1. Die 7 Kern-Komponenten

Teil II: Die 7 Kern-Komponenten im Detail

1. Artifact System

1.1 Was ist ein Artefakt?

1.2 Artefakt Basis-Schema

1.3 Provenance / Lineage

1.4 Core Artefakt-Typen

1.5 Standard-Enums (V1, verbindlich)

2. Schema Registry

2.1 Interface

2.2 Schema-IDs und Namespace

2.3 Migrations-Regeln

3. Store

3.1 Interface

3.2 Implementierungen

3.3 Head-Semantik

3.4 Commit-Boundary (hart)

3.5 Concurrency: Optimistic Lock

4. Reducer

4.1 Definition (hart)

4.2 Reducer vs. Verifier vs. Store

4.3 Core Reducer (V1)

4.4 Wichtig: Keine Zeit-Abhüngigkeit

4.5 Event-Log als Grundlage

5. Verification Engine

5.1 Tri-State (nicht verhandelbar)

5.2 Verifier-Interface

5.3 Core Verifier

5.4 Hard vs. Soft Checks

5.5 Wann entsteht \'unknown\'?

6. Approval System

6.1 Approval Artefakt

6.2 Policy Drift / TOCTOU-Schutz

6.3 Clock-Ownership für expires_at

7. Execution Engine

7.1 Executor-Vorbedingungen (alle drei müssen erfüllt sein)

7.2 Action Materializer

7.3 Option Steps als Typed DSL

7.4 Action Schema (streng)

7.5 Executor-Idempotenz (hart)

7.6 Executor-Typen (V1)

7.7 Option Status-Maschine

Teil III: LLM-Integration

8. LLM als Option Generator

8.1 Position im Fluss

8.2 LLM Interface

8.3 LLM-Commit-Regeln

8.4 LLM-Provenance (Pflicht)

8.5 Verschiedene LLM-Rollen pro Modul

Teil IV: Erweiterungsmodell

9. Module-Manifest

9.1 Was ein Modul registriert

9.2 Manifest-Schema

9.3 Isolation zwischen Modulen

9.4 Beispiel: Support-AI Modul

Teil V: Sicherheitsmodell & Policy

10. Trust-Modell

10.1 Trust Levels

11. Policy & Permissions

11.1 Trennung

11.2 Policy-Struktur (V1)

11.3 Change Control

Teil VI: Gesamtfluss & MVP

12. Vollstündiger Systemfluss

13. MVP-Scope V1

13.1 MVP-Flow (verbindlich)

13.2 V1-Constraints

13.3 Minimale Artefakt-Typen V1

13.4 Integration-Tests (die ARCS echt machen)

Teil VII: Code-Interfaces & Roadmap

14. Modul-Interfaces (vollstündig)

15. Roadmap V1 bis V3

Anhang: Drei harte Betriebsregeln

# Teil I: Was ist ARCS Core

ARCS ist kein Produkt. ARCS ist eine Plattform - das Betriebssystem für Entscheidungen. Anwendungen sind Module auf diesem Betriebssystem. Der Core definiert nur Regeln, nicht Funktionen.

## 1. Die 7 Kern-Komponenten

ARCS Core besteht aus exakt 7 Komponenten. Alles andere ist Erweiterung.

  ------------------------------------------------------------------------------------------------------
  **\#**   **Komponente**        **Rolle im System**                                   **Analogie**
  -------- --------------------- ----------------------------------------------------- -----------------
  1        Artifact System       Kommunikation im System - alles ist ein Artefakt      Sprache

  2        Schema Registry       Regeln für Daten - was ist ein gültiges Artefakt      Grammatik

  3        Store                 Persistenter Zustand - append-only, replaybar         Gedüchtnis

  4        Reducer               State-Projektion - deterministisch aus Log ableiten   Verstand

  5        Verification Engine   Policy Enforcement - ist diese Option erlaubt?        Gewissen

  6        Approval System       Human-in-the-loop - kritische Entscheidungen          Zustimmung

  7        Execution Engine      Wirkung in der Welt - typisiert, idempotent, sicher   Hünde
  ------------------------------------------------------------------------------------------------------

# Teil II: Die 7 Kern-Komponenten im Detail

## 1. Artifact System

Das wichtigste Konzept in ARCS. Alles im System ist ein Artefakt. Artefakte sind die Sprache von ARCS - sie sind das einzige Kommunikationsmedium zwischen allen Komponenten.

### 1.1 Was ist ein Artefakt?

  -----------------------------------------------------------------------------------------------------------------------
  **Eigenschaft**    **Bedeutung**                                          **Konseqünz**
  ------------------ ------------------------------------------------------ ---------------------------------------------
  immutable          Einmal committed, nie veründert                        Append-only Store, neü Version statt Update

  versioniert        Jede Ünderung = neü Version mit stabiler artifact_id   Vollstündige History, Rollback müglich

  schema-validiert   Payload entspricht registriertem JSON Schema           Kein untyped Payload nach Ingress

  auditierbar        Provenance: woher kam es, wer hat es erzeugt           Vollstündiger Audit-Trail
  -----------------------------------------------------------------------------------------------------------------------

### 1.2 Artefakt Basis-Schema

```
{

"artifact_id": "a_01H...", // stabile Identitüt

"version_id": "v_01H...", // diese spezifische Version

"version": 1, // Versionsnummer

"type": "task", // Artefakt-Typ

"schema_id": "arcs.task.v1", // Schema-Registry Referenz

"schema_version": 1,

"created_at": "2026-02-09T10:00:00Z",

"created_by": { "actor_type": "human|system|model|executor", "id": "..." },

"source": { "kind": "chat|file|api|sensor|timer|internal", "ref": "..." },

"trust": { "level": "high|medium|low", "source_class": "human|system|model|external" },

"stream_key": "task_id:t_01H...", // für Reducer (Aggregate-Grenze)

"tags": [],

"payload": { } // schema-validiert via schema_id

}
```

**1.3 Provenance / Lineage**

Jedes abgeleitete Artefakt referenziert vollstündig seine Eingaben. Das ist die Grundlage für Audit und Replay.

```
{

"provenance": {

"parents": ["a_task\_...", "a_policy\_..."],

"rules_applied": ["rule_permission_check", "rule_scope"],

"models_used": [{

"name": "model-name",

"prompt_hash": "sha256:...",

"inputs": ["a_task\_...", "a_world_state\_..."],

"temperature": 0.2,

"raw_output_hash": "sha256:..."

}],

"transform": "extract | match | reason | verify | plan | materialize"

}

}
```

### 1.4 Core Artefakt-Typen

Die folgenden Typen sind im Core definiert. Module künnen weitere registrieren (siehe Teil IV).

  ------------------------------------------------------------------------------------------------
  **Gruppe**    **Typ**               **Zweck**
  ------------- --------------------- ------------------------------------------------------------
  Input         ingress_event         Roher eingehender Event aus beliebiger Qülle

  Input         task                  User/System will X erreichen

  Input         claim                 Wissensbehauptung (keine Fakten - immer mit source)

  Input         evidence              Datei/URL/API-Response Snapshot mit Metadaten

  State         world_state           Assertions über aktüllen Zustand (scope + validity)

  State         assumption_set        Explizite Annahmen für einen Reasoner-Lauf

  Engine        match_result          Deterministic Pattern hat etwas gefunden

  Engine        risk                  Risiko-Objekt mit severity + rationale

  Engine        conflict              Widerspruch zwischen Artefakten

  Planning      option                Alternative mit typed steps + human_summary

  Governance    policy                Regeln + Verifier-Config + Action-Allowlist

  Governance    permission_grant      Principal -\> Capability (scope + TTL)

  Governance    verification_report   Ergebnis der Verifier (pass|fail|unknown)

  Governance    approval              Menschliche Entscheidung (approve|reject|modify|revoke)

  Execution     action                Typisierte, idempotente Operation

  Execution     execution_result      Outcome inkl. Logs, exit_code, action_id
  ------------------------------------------------------------------------------------------------

### 1.5 Standard-Enums (V1, verbindlich)

-   actor_type: human | system | model | executor

-   source.kind: chat | file | api | sensor | timer | internal

-   trust.level: low | medium | high

-   trust.source_class: human | system | model | external

-   verification.status: pass | fail | unknown

-   approval.decision: approve | reject | modify | revoke

-   action.type: file_write | file_read | api_call | shell_cmd | report_emit

-   execution_result.status: success | fail | cancelled | timeout

## 2. Schema Registry

Die Schema Registry ist das Regelwerk für alle Artefakte. Kein Artefakt betritt das System ohne gültiges Schema.

### 2.1 Interface

```
interface ISchemaRegistry {

load_schemas(): void

validate(artifact: Artifact): ValidationResult

get_schema(schema_id: string): JsonSchema

register(schema_id: string, schema: JsonSchema): void // nur für Module

}
```

### 2.2 Schema-IDs und Namespace

```
// Core schemas (unarcsenderlich)

arcs.artifact_base.v1

arcs.ingress_event.v1

arcs.task.v1

arcs.option.v1

arcs.verification_report.v1

arcs.approval.v1

arcs.action.v1

arcs.execution_result.v1

arcs.policy.v1

arcs.permission_grant.v1

// Modul-Schemas (registriert via Manifest)

{module_name}.{type}.v{n}

// Beispiele:

robotics.sensor_reading.v1

robotics.motor_command.v1

support.reply_option.v1
```

### 2.3 Migrations-Regeln

-   Jede Breaking-Ünderung = schema_version++

-   Migrationen sind pure functions: migrate(artifact@v1) -\> artifact@v2

-   Event-Log speichert immer die Original-Version

-   Schema-Migrationen unterliegen demselben Change-Control-Pfad wie Policy-Ünderungen

+-------------------------------------------------------------------------------------------------+
| **Kill-Kriterium**                                                                              |
|                                                                                                 |
| Wenn Migrationen nicht sauber funktionieren, ist \'alles Artefakte\' langfristig nicht wartbar. |
|                                                                                                 |
| Schema-Migrationen sind policy-relevante Ünderungen und brauchen AuthorityVerifier + Approval.  |
+=================================================================================================+
+-------------------------------------------------------------------------------------------------+

## 3. Store

Der Store ist das Gedüchtnis von ARCS. Er speichert alle Artefakte append-only und macht das System vollstündig replaybar.

### 3.1 Interface

```
interface IStore {

append(artifact: ArtifactVersion): void // append-only

get(artifact_id: string): ArtifactVersion // current head

get_version(version_id: string): ArtifactVersion // spezifische Version

list(type: string, stream_key?: string): ArtifactVersion[]

commit(versions: ArtifactVersion[], events: Event[]): void // ATOMAR

}
```

### 3.2 Implementierungen

  -----------------------------------------------------------------------------------------
  **Implementierung**   **Einsatz**          **Eigenschaften**
  --------------------- -------------------- ----------------------------------------------
  StoreMemory           Tests / Unit Tests   In-Memory, kein Persistence, schnell

  StoreSQLite           V1 / Entwicklung     Lokale Datei, einfach, keine Infrastruktur

  StorePostgres         V1 Production        ACID, concurrent-safe, Optimistic Lock

  StoreLog              Production / Audit   Append-only Log + Index, manipulationssicher
  -----------------------------------------------------------------------------------------

### 3.3 Head-Semantik

artifact_head ist nicht \'latest row in table\'. Es ist das Ergebnis eines deterministischen Reducers über den Event Log.

```
// Definition: artifact_head = latest version die:

// 1. schema-valid ist

// 2. erforderliche Verifier bestanden hat (policy-gesteürt)

// 3. nicht revoked ist

// 4. durch explizites head_advanced Event bestütigt wurde

// Head-Wechsel ist selbst ein Event:

{

"event_type": "head_advanced",

"refs": [{ "artifact_id": "a\_...", "version_id": "v_NEW", "role": "target" }]

}
```

### 3.4 Commit-Boundary (hart)

+-------------------------------------------------------------------------------------------+
| **Betriebsregel 1: Commit-Boundary**                                                      |
|                                                                                           |
| Alles was zusammengehürt wird in EINER Transaktion committed: artifact_versions + events. |
|                                                                                           |
| Kein Halb-Commit. Kein Event ohne Artefakt. Kein Artefakt ohne Event.                     |
|                                                                                           |
| Konseqünz: Replay ist immer konsistent.                                                   |
+===========================================================================================+
+-------------------------------------------------------------------------------------------+

### 3.5 Concurrency: Optimistic Lock

```
// Jeder Commit trügt expected_head_version_id

{

"artifact_id": "a_task\_...",

"expected_head_version_id": "v_01H...", // muss mit aktüllem Head übereinstimmen

"new_version": { ... }

}

// Wenn expected != current head -\> COMMIT REJECT

// Branching/Merging: V2
```

## 4. Reducer

Reducer sind deterministische Projektionen vom Artifact-Log auf abgeleitete Zustünde (Views/Projections), die für Entscheidungen gebraucht werden.

### 4.1 Definition (hart)

  ------------------------------------------------------------------------------------------------------
  **Eigenschaft**       **Regel**
  --------------------- --------------------------------------------------------------------------------
  Input                 vector\<Artifact\> für einen stream_key (z.B. alle Artefakte für task_id)

  Output                DerivedState (z.B. TaskState, ApprovalState, ExecutionPlan)

  Determinismus         Keine Side-Effects, keine Uhrzeit-Abhüngigkeit, keine Randomness

  Sortierung            Nach Log-Reihenfolge (append index) oder causal links - NICHT nach occurred_at

  Replay                Derselbe Input ergibt immer denselben Output - in Production und in Tests
  ------------------------------------------------------------------------------------------------------

### 4.2 Reducer vs. Verifier vs. Store

  -------------------------------------------------------------------------------------------
  **Komponente**     **Frage**                   **Charakteristik**
  ------------------ --------------------------- --------------------------------------------
  Store              Wo sind die Artefakte?      Persistenz, append-only, Source of Truth

  Reducer            Was ist der Zustand?        Projektion, deterministisch, pure function

  Verifier           Ist diese Option erlaubt?   Policy Enforcement über Zustand + Option
  -------------------------------------------------------------------------------------------

### 4.3 Core Reducer (V1)

```
// TaskStateReducer

// Input: alle Artefakte mit stream_key = "task_id:t\_..."

// Output: TaskState { status, current_options, active_verifications, pending_approvals }

// ApprovalStateReducer

// Input: alle approval Artefakte für eine option

// Output: ApprovalState { decision, policy_ref, expires_at, is_valid }

// ExecutionPlanReducer

// Input: approved option + policy + selected executor

// Output: ExecutionPlan { actions[], required_permissions[], safety_level }

// PermissionReducer

// Input: alle permission_grant Artefakte für einen principal

// Output: EffectivePermissions { capabilities[], scopes[], ttl }

interface IReducer\<TState\> {

reduce(artifacts: Artifact[]): TState

// Pure function - kein this-State, kein IO, keine Zeit

}
```

### 4.4 Wichtig: Keine Zeit-Abhüngigkeit

+--------------------------------------------------------------------------------------------------------------------------+
| **Replay-Invariante**                                                                                                    |
|                                                                                                                          |
| Reducer dürfen NICHT nach occurred_at sortieren - nur nach Log-Reihenfolge (append index) oder causal links (parent_id). |
|                                                                                                                          |
| occurred_at ist nur für Audit/UI - niemals für Logik.                                                                    |
|                                                                                                                          |
| Bei Replay gilt: \'validity at time of event\' aus dem Event-Log, nicht aktüller Zeitstempel.                            |
|                                                                                                                          |
| TimeSource ist ein injiziertes Interface - nicht Date.now() direkt.                                                      |
+==========================================================================================================================+
+--------------------------------------------------------------------------------------------------------------------------+

### 4.5 Event-Log als Grundlage

```
// Minimales Event-Schema (V1)

{

"event_id": "e_01H...",

"event_type": "artifact_committed | head_advanced | approval_recorded | action_materialized | execution_recorded",

"ts": "2026-02-09T10:00:00Z", // nur für Audit - NICHT für Logik

"actor": { "actor_type": "...", "id": "..." },

"refs": [

{ "artifact_id": "a\_...", "version_id": "v\_...", "role": "target|parent|policy|action|result" }

],

"stream_key": "task_id:t_01H...", // Aggregate-Grenze für Reducer

"payload": {}, // klein, deterministisch, kein Blob

"prev_hash": "..." // V1 optional, V2 mandatory

}
```

## 5. Verification Engine

Hier werden alle Optionen, Artefakt-Updates und Aktionen geprüft. Keine externe Wirkung ohne pass. Das ist das Herzstuck der Governance.

### 5.1 Tri-State (nicht verhandelbar)

  ----------------------------------------------------------------------------------------------
  **Status**    **Bedeutung**                              **Konseqünz**
  ------------- ------------------------------------------ -------------------------------------
  **pass**      Alle relevanten Checks bestanden           Weiter zum nüchsten Gate

  **fail**      Mind. ein Hard-Check fehlgeschlagen        Immer blocken - keine Ausnahmen

  **unknown**   Check nicht deterministisch entscheidbar   Immer blocken (Fail-Closed Prinzip)
  ----------------------------------------------------------------------------------------------

### 5.2 Verifier-Interface

```
interface IVerifier {

check(target: Artifact, context: VerificationContext): VerificationReport

// context enthült: policy (current head), permissions, world_state

}

// VerificationReport Artefakt:

{

"type": "verification_report",

"payload": {

"target": { "artifact_id": "a\_...", "version_id": "v\_..." },

"status": "pass | fail | unknown",

"checks": [

{ "name": "schema", "status": "pass", "detail": "..." },

{ "name": "permission", "status": "fail", "detail": "capability exec:shell fehlt" },

{ "name": "scope", "status": "unknown", "detail": "scope nicht eindeutig auflüsbar" }

],

"blockers": ["permission fehlt: exec:shell"],

"recommendations": ["alternative option ohne shell verfügbar"]

}

}
```

### 5.3 Core Verifier

  -------------------------------------------------------------------------------------------------------
  **Verifier**                 **Prüft**                                            **Hard/Soft**
  ---------------------------- ---------------------------------------------------- ---------------------
  SchemaVerifier               Payload entspricht schema_id                         Hard

  PermissionVerifier           Principal hat erforderliche capability               Hard

  ScopeVerifier                Aktion im erlaubten Scope (project/task/namespace)   Hard

  SafetyVerifier               Geführliche Ops, safety_level \> policy-Limit        Hard

  AuthorityVerifier            Wer darf policy/permission_grant ündern              Hard

  ReferenceIntegrityVerifier   Alle artifact_ids/version_ids existieren im Store    Hard - blockt immer

  ConsistencyVerifier          Widersprüche zwischen Artefakten im stream_key       Hard

  ApprovalVerifier             Approval gültig, nicht expired, policy-ref stimmt    Hard
  -------------------------------------------------------------------------------------------------------

### 5.4 Hard vs. Soft Checks

  -----------------------------------------------------------------------------------
  **Hard Checks (blocken immer)**          **Soft Checks (nur Empfehlungen)**
  ---------------------------------------- ------------------------------------------
  Schema invalid                           Risiko hoch, aber erlaubt (mit Approval)

  Permission fehlt                         Bessere Option existiert

  Action ausserhalb Allowlist              Konfidenz niedrig

  Referenz auf nicht-existente Artefakte   Stale world_state (validity window)

  Policy-Version mismatch                  

  Approval expired oder revoked            
  -----------------------------------------------------------------------------------

### 5.5 Wann entsteht \'unknown\'?

unknown entsteht exakt dann, wenn ein Check nicht deterministisch entschieden werden kann UND das Policy-Level Block verlangt.

-   Shell command enthült variable Pfade / nicht resolvable

-   Domain ist nicht eindeutig parsebar für Allowlist-Check

-   Ambige Permission scope (mehrere mügliche Interpretationen)

-   Fehlender Evidence-Snapshot wenn Policy ihn verlangt

-   Zeitfenster nicht eindeutig bestimmbar (fehlende TimeSource)

+-------------------------------------------------------------------------------+
| **Recovery-Pfad für unknown (V1)**                                            |
|                                                                               |
| unknown blockt immer - das Artefakt bleibt im Status \'blocked\'.             |
|                                                                               |
| V1: Manüller Re-Submit nach Klürung des Grundes (explizite V1-Einschrünkung). |
|                                                                               |
| V2: Automatischer Eskalations-Pfad mit Notification und Override-Mechanismus. |
+===============================================================================+
+-------------------------------------------------------------------------------+

## 6. Approval System

Wenn eine Option kritisch ist - d.h. externe Wirkung hat oder eine hohe safety_level-Schwelle überschreitet - ist menschliche Zustimmung erforderlich. Ohne Approval keine Execution.

### 6.1 Approval Artefakt

```
{

"type": "approval",

"payload": {

"target_option": { "artifact_id": "a_option\_...", "version_id": "v\_..." },

"policy_ref": { "artifact_id": "a_policy\_...", "version_id": "v_policy\_..." },

"decision": "approve | reject | modify | revoke",

"reason": "Geprüft, Risiken akzeptabel",

"actor": { "actor_type": "human", "id": "user:simon" },

"timestamp": "2026-02-09T10:00:00Z",

"expires_at": "2026-02-09T11:00:00Z" // absolute UTC

}

}
```

### 6.2 Policy Drift / TOCTOU-Schutz

+-------------------------------------------------------------------------------------------------------+
| **V1-Regel (hart)**                                                                                   |
|                                                                                                       |
| Approval ist an eine konkrete policy_ref (artifact_id + version_id) gebunden.                         |
|                                                                                                       |
| Wenn Policy oder Constraints zwischen Approval und Ausführung wechseln: blocken und neu verifizieren. |
|                                                                                                       |
| Dies verhindert dass nach einer Approval heimlich andere Actions entstehen.                           |
|                                                                                                       |
| V2-Hürtung: Approval bindet zusützlich an materializer_version + constraint_hash.                     |
+=======================================================================================================+
+-------------------------------------------------------------------------------------------------------+

### 6.3 Clock-Ownership für expires_at

Die Gültigkeitsprüfung sitzt an einem einzigen, definierten Ort.

  ------------------------------------------------------------------------------------------
  **Modell**   **Owner**             **Problem**                         **Empfehlung**
  ------------ --------------------- ----------------------------------- -------------------
  Modell 1     ApprovalVerifier      Keins                               Verwenden

  Modell 2     Approval Gate         TOCTOU zwischen Gate und Executor   Nicht ausreichend

  Modell 3     Executor              Governance fragmentiert             Nicht verwenden
  ------------------------------------------------------------------------------------------

+-----------------------------------------------------------------------+
| **Entscheidung (V1, verbindlich)**                                    |
|                                                                       |
| Clock-Ownership liegt beim Core (ApprovalVerifier).                   |
|                                                                       |
| Executor macht nur einen finalen \'expired =\> abort\' Guard.         |
|                                                                       |
| expires_at ist absolute UTC time.                                     |
|                                                                       |
| Bei Replay: \'validity at time of event\' - nicht aktüller Stand.     |
|                                                                       |
| TimeSource ist ein injiziertes Interface (kein Date.now() direkt).    |
+=======================================================================+
+-----------------------------------------------------------------------+

## 7. Execution Engine

Wenn Verification pass und Approval erteilt: Option -\> Action -\> Executor -\> ExecutionResult. Kein Executor ohne vollstündige Vorbedingungen.

### 7.1 Executor-Vorbedingungen (alle drei müssen erfüllt sein)

1.  Approval vorhanden, gültig, nicht expired, policy_ref stimmt

2.  VerificationReport mit status = pass für diese Action

3.  Permission Token für erforderliche capabilities vorhanden

### 7.2 Action Materializer

Der Materializer erzeugt typisierte Actions aus approved Options. Er ist vollstündig deterministisch und verwendet kein LLM.

```
// Input: approved option + policy (current head) + selected_executor

// Output: 1..n action Artefakte

// Regel:

// - nimmt nur step.kind + step.params

// - validiert gegen Policy-Constraints + Action-Schema

// - kein Text-Parsing, kein LLM, keine Heuristiken

// - schreibt Event "action_materialized_from_option"

// - danach: separater Verifier-Pfad über die action

interface IMaterializer {

materialize(option: OptionArtifact, policy: PolicyArtifact): Action[]

// Pure function - deterministisch, kein IO

}
```

+---------------------------------------------------------------------------------------------------------------------+
| **Kill-Kriterium**                                                                                                  |
|                                                                                                                     |
| Sobald im Materializer Parsing-Heuristiken auftauchen (\'wenn Text enthült...\'), ist das Modell wieder Autoritüt. |
|                                                                                                                     |
| option.steps[] muss ein Union-Typ (typed DSL) sein - kein Freitext.                                               |
|                                                                                                                     |
| Nach Materialisierung lüuft ein separater Verifier-Pfad über die action.                                            |
+=====================================================================================================================+
+---------------------------------------------------------------------------------------------------------------------+

### 7.3 Option Steps als Typed DSL

```
// option.steps[] ist ein Union-Typ - kein Freitext

type Step =

| { kind: "emit_report"; params: { format: "pdf"|"json"; sections: string[] } }

| { kind: "write_file"; params: { path: string; content_artifact_id: string } }

| { kind: "api_call"; params: { endpoint: string; method: string; body_artifact_id: string } }

// Module registrieren weitere step.kind Varianten

// option Artefakt:

{

"type": "option",

"payload": {

"title": "Verification Report generieren",

"human_summary": "Erstellt einen PDF-Report der aktüllen Prüfergebnisse. (lesbar für Approval-UI)",

"steps": [

{ "kind": "emit_report", "params": { "format": "pdf", "sections": ["summary", "risks"] } }

],

"requires_permissions": ["exec:report_emit"],

"safety_level": "low"

}

}
```

### 7.4 Action Schema (streng)

```
{

"type": "action",

"payload": {

"action_id": "x_01H...",

"schema_id": "actions/report_emit@v1", // pro type ein eigenes Schema

"type": "report_emit",

"params": { "format": "pdf", "sections": ["summary"] },

"required_permissions": ["exec:report_emit"],

"safety_level": "low",

"idempotency_key": "x_01H..." // = action_id

}

}
```

### 7.5 Executor-Idempotenz (hart)

+----------------------------------------------------------------------------------------------+
| **Betriebsregel 3: At-least-once + Idempotenz**                                              |
|                                                                                              |
| action_id ist der Idempotency Key.                                                           |
|                                                                                              |
| Jeder Executor speichert action_id persistent (oder schlügt im Store nach).                  |
|                                                                                              |
| Doppelte Ausführung derselben action_id: no-op ODER safe replay (gleiches execution_result). |
|                                                                                              |
| Voraussetzung für Retries, Crash-Recovery und Revocation.                                    |
+==============================================================================================+
+----------------------------------------------------------------------------------------------+

### 7.6 Executor-Typen (V1)

  ------------------------------------------------------------------------------------------------
  **Executor**           **V1-Status**         **Constraints**
  ---------------------- --------------------- ---------------------------------------------------
  ReportEmitExecutor     Default / MVP         Keine Nebenwirkungen, safe, empfohlen für MVP

  FileExecutor           Optional              Allowlist read/write roots, kein Netz

  ApiExecutor            Mit Einschrünkungen   Allowlist domains via Proxy, rate limited

  ShellExecutor          Default verboten      Nur mit Sandbox + Allowlist + Timeout + Namespace

  DroneExecutor          V2+                   Nur über kontrollierten Action-Bus
  ------------------------------------------------------------------------------------------------

### 7.7 Option Status-Maschine

```
option:draft

|

v (verification)

+\-- fail/unknown \--\> option:blocked (Recovery: manüller Re-Submit)

|

v pass \--\> option:verified

|

v (human approval)

+\-- reject \--\> option:rejected

+\-- modify \--\> neü option-version \--\> zurück zu draft

|

v approve \--\> option:approved

|

v (materialization)

\--\> option:action_dispatched

|

v (execution)

+\-- fail/timeout \--\> option:failed

+\-- revoke \--\> option:revoked

|

v success \--\> option:executed
```

# Teil III: LLM-Integration

LLMs sind ein Werkzeug in ARCS - kein Entscheider. Sie sitzen als Option Generator im Fluss und sind vollstündig vom Execution-Pfad getrennt.

## 8. LLM als Option Generator

### 8.1 Position im Fluss

```
// LLM sitzt hier:

TaskState + Kontext

|

v

[LLM Option Generator]

|

v

OptionArtifact(s) \<\-- schema-validiert, trust.level = low

|

v

[Verifier] \<\-- ab hier: normale Pipeline

|

v

[Approval Gate]

|

v

[Execution Engine]
```

### 8.2 LLM Interface

```
interface ILLMOptionGenerator {

generate(

state: TaskState,

context: LLMContext

): OptionArtifact[]

// LLM Output wird immer als trust.level = low committed

// Immer schema-validiert bevor commit

// Immer mit model_provenance im artifact.provenance

}
```

### 8.3 LLM-Commit-Regeln

  ------------------------------------------------------------------------------
  **LLM darf committen**                     **LLM darf NICHT committen**
  ------------------------------------------ -----------------------------------
  claim (als claim, nicht fact)              policy

  option (als proposal, trust.level=low)     permission_grant

  match_result (wenn deterministic engine)   approval

  assumption_set (als Vorschlag)             action

                                             execution_result
  ------------------------------------------------------------------------------

### 8.4 LLM-Provenance (Pflicht)

Für jeden LLM-involvierten Commit muss Model-Provenance gespeichert werden. Ziel: nicht reproduzieren, sondern beweisen.

```
{

"provenance": {

"models_used": [{

"name": "model-identifier",

"prompt_hash": "sha256:...",

"inputs": ["a_task\_...", "a_world_state\_..."],

"temperature": 0.2,

"raw_output_hash": "sha256:..."

}]

}

// Du musst nicht reproduzieren künnen.

// Du musst beweisen künnen, warum du etwas übernommen hast.

}
```

### 8.5 Verschiedene LLM-Rollen pro Modul

Verschiedene Anwendungen brauchen verschiedene LLM-Rollen. Jede Rolle hat eigene Trust-Anforderungen und Failure-Modi.

  ---------------------------------------------------------------------------------------------------------
  **LLM-Rolle**         **Trust-Level**   **Output**              **Failure-Modus**
  --------------------- ----------------- ----------------------- -----------------------------------------
  Option Generator      low               OptionArtifact[]      Schlechte Option -\> Verifier blockt

  Extractor             low -\> medium    task, claim, evidence   Falscher Typ -\> Schema rejected

  Pattern Suggester     low               match_result (model)    Falsch-Positiv -\> Verifier filtert

  Anomaly Detector      low -\> medium    risk Artefakt           False alarm -\> human prüft

  Sensor Interpreter    medium (V2)       world_state assertion   Falsche Assertion -\> Conflict Verifier
  ---------------------------------------------------------------------------------------------------------

# Teil IV: Erweiterungsmodell

ARCS Core ist eine Plattform. Module registrieren sich mit einem Manifest und erweitern den Core ohne ihn zu veründern.

## 9. Module-Manifest

### 9.1 Was ein Modul registriert

  ---------------------------------------------------------------------------------------------------------------
  **\#**   **Was**                                 **Pflicht**             **Beispiel**
  -------- --------------------------------------- ----------------------- --------------------------------------
  1        Schemas (Artefakt-Typen + Schema-IDs)   Ja                      robotics.sensor_reading.v1

  2        Reducers (optional, core reichen oft)   Nein                    SensorStateReducer

  3        Verifiers (für option/action types)     Ja (wenn neü types)     PhysicalSafetyVerifier

  4        Executors (für action types)            Ja (wenn neü actions)   MotorCommandExecutor

  5        LLM Roles/Policies                      Nein                    SensorInterpreter mit model + prompt

  6        Step-Kinds (für option DSL)             Ja (wenn neü steps)     motor_command step.kind
  ---------------------------------------------------------------------------------------------------------------

### 9.2 Manifest-Schema

```
{

"module_id": "robotics",

"version": "1.0.0",

"display_name": "Robotics Control Module",

"requires_core": "\>=1.0.0",

"schemas": [

{ "schema_id": "robotics.sensor_reading.v1", "path": "./schemas/sensor_reading.v1.json" },

{ "schema_id": "robotics.motor_command.v1", "path": "./schemas/motor_command.v1.json" }

],

"step_kinds": [

{ "kind": "motor_command", "schema_id": "robotics.motor_command.v1" }

],

"verifiers": [

{ "name": "PhysicalSafetyVerifier", "applies_to": ["option", "action"], "priority": "hard" }

],

"executors": [

{ "name": "MotorCommandExecutor", "handles_action_type": "motor_command" }

],

"llm_roles": [

{

"role": "sensor_interpreter",

"model": "model-identifier",

"trust_level": "medium",

"output_type": "world_state"

}

],

"capabilities": [

"exec:motor_command",

"exec:sensor_read"

]

}
```

### 9.3 Isolation zwischen Modulen

-   scope in world_state und permission_grant ist mandatory für Modul-Artefakte

-   Artefakte eines Moduls sind nur in dessen scope sichtbar (kein Cross-Modul-Zugriff ohne explizite Permission)

-   Schema-Namespaces verhindern ID-Kollisionen ({module_id}.{type}.v{n})

-   Ein Modul kann den Core nicht umgehen - alle Aktionen laufen durch dieselbe Verification Pipeline

### 9.4 Beispiel: Support-AI Modul

```
// support modul manifest

{

"module_id": "support",

"schemas": [

{ "schema_id": "support.reply_option.v1" },

{ "schema_id": "support.ticket_task.v1" }

],

"step_kinds": [

{ "kind": "send_reply", "schema_id": "support.reply_option.v1" }

],

"executors": [

{ "name": "SendReplyExecutor", "handles_action_type": "send_reply" }

],

"capabilities": ["exec:send_reply"]

}

// Fluss:

// User message -\> ingress_event -\> support.ticket_task

// -\> LLM Option Generator -\> support.reply_option

// -\> Verifier (policy: Antwortkategorien)

// -\> optional Approval (bei kritischen Kategorien)

// -\> send_reply action -\> SendReplyExecutor

// -\> execution_result
```

# Teil V: Sicherheitsmodell & Policy

## 10. Trust-Modell

### 10.1 Trust Levels

  --------------------------------------------------------------------------------------------------------------------------
  **Level**    **source_class**   **Typische Qüllen**                      **Konseqünz**
  ------------ ------------------ ---------------------------------------- -------------------------------------------------
  **high**     human              Menschliche Freigabe, signierte Qüllen   Kann Policy ündern, Approval erteilen

  **medium**   system             Bekannte interne Systeme                 Kann Artefakte committen, keine Policy-Ünderung

  **low**      model / external   LLM-Output, Web, unbekannte Externe      Nur Vorschlüge, immer Verifier-Prüfung
  --------------------------------------------------------------------------------------------------------------------------

## 11. Policy & Permissions

### 11.1 Trennung

  --------------------------------------------------------------------------------------------------------------
                     **policy**                                               **permission_grant**
  ------------------ -------------------------------------------------------- ----------------------------------
  Inhalt             Regeln, Constraints, Action-Allowlist, Verifier-Config   Principal -\> Capability Mapping

  Ünderungsfreqünz   Selten                                                   Hüufiger

  Authority          policy:edit capability + second approver (V2)            perm:grant capability + approval
  --------------------------------------------------------------------------------------------------------------

### 11.2 Policy-Struktur (V1)

```
{

"type": "policy",

"payload": {

"capabilities": ["exec:report_emit", "exec:file_write", "approve:high_risk"],

"constraints": {

"shell": { "allow_cmd": ["git", "python"] },

"net": { "allow_domains": ["api.example.com"] },

"file": { "allow_roots": ["/sandbox/work"] }

},

"verifier_rules": {

"hard_checks": ["schema", "permission", "scope", "reference_integrity", "approval"],

"soft_checks": ["risk_level", "confidence"]

},

"approval_required_for": ["safety_level:high", "exec:shell", "exec:file_write"]

}

}
```

### 11.3 Change Control

Ünderungen an policy und permission_grant sind selbst Actions und brauchen denselben Governance-Pfad:

4.  Ünderungs-Option erstellen

5.  AuthorityVerifier prüft: Actor hat policy:edit capability

6.  VerificationReport: pass

7.  Human Approval (mandatory)

8.  Action: policy_update

9.  Executor: PolicyUpdateExecutor

10. Neüs policy Artefakt committed

# Teil VI: Gesamtfluss & MVP

## 12. Vollstündiger Systemfluss

```
[Externe Qülle: chat | file | api | sensor | timer]

|

v

[Ingress Layer]

\- normalisieren, typisieren, signieren

\- IngressRouter, Normalizer, Validator

\- Quarantine bei fail/unknown

|

v

[Artifact Extractor]

\- IngressEvent -\> typisierte Artefakte

\- LLM: Kandidaten erzeugen (trust.level=low)

\- Schema-Validierung

\- commit() -\> Store + Event Log

|

v

[Store + Event Log] \<\-- append-only, Source of Truth

|

v

[Reducer] \<\-- deterministisch, pure function

\- TaskStateReducer

\- PermissionReducer

\- ApprovalStateReducer

|

v

[Pattern Engine] (optional, parallel)

\- Deterministic Matcher -\> match_result

\- Conflict Detection -\> conflict

\- Risk Assessment -\> risk

|

v

[Reasoner / LLM Option Generator]

\- TaskState + Kontext -\> OptionArtifact[]

\- trust.level = low, schema-validiert

|

v

[Verification Engine]

\- pass | fail | unknown

\- fail/unknown -\> option:blocked

|

v

[Approval Gate] (wenn policy es verlangt)

\- Human review

\- approve | reject | modify | revoke

|

v

[Action Materializer]

\- approved option + policy -\> Action[]

\- deterministisch, kein LLM

\- separater Verifier-Pfad

|

v

[Execution Engine]

\- Executor (idempotent, at-least-once safe)

\- ExecutionResult

|

v

[Store + Event Log] \<\-- alles committed

|

v

[Observability / Audit / Replay]
```

## 13. MVP-Scope V1

### 13.1 MVP-Flow (verbindlich)

```
ingress_event

-\> task

-\> option (typed steps: emit_report)

-\> verification_report (pass)

-\> approval (approve)

-\> action (report_emit)

-\> execution_result
```

### 13.2 V1-Constraints

+----------------------------------------------------------------------------+
| **Maximal Safe V1**                                                        |
|                                                                            |
| Kein ShellExecutor.                                                        |
|                                                                            |
| Kein direkter Netzwerkzugriff.                                             |
|                                                                            |
| Kein file_write (optional - wenn maximal safe gewünscht).                  |
|                                                                            |
| Nur report_emit: testet den kompletten Gate-/Verifier-/Audit-/Replay-Loop. |
|                                                                            |
| StoreSQLite für V1, StorePostgres für Production.                          |
+============================================================================+
+----------------------------------------------------------------------------+

### 13.3 Minimale Artefakt-Typen V1

-   task, claim, risk

-   option (typed steps + human_summary)

-   verification_report, approval

-   action, execution_result

-   policy, permission_grant

### 13.4 Integration-Tests (die ARCS echt machen)

Wenn diese Tests grün sind, ist das System nicht mehr nur Design, sondern System:

  -----------------------------------------------------------------------------------------------------
  **Test**                **Was er prüft**
  ----------------------- -----------------------------------------------------------------------------
  Happy Path              Vollstündiger Flow von ingress_event bis execution_result

  unknown blockt          unknown in Verifier -\> keine Action materialisiert, option:blocked

  Policy Drift            Approval an P1, policy head wechselt auf P2 -\> block + Re-Verify erzwungen

  Idempotenz              Gleiche action_id zweimal -\> nur ein Resultat, safe replay

  Concurrent Commit       expected_head mismatch -\> einer wird rejected

  Revocation              Approval erteilt -\> action dispatched -\> revoke -\> Executor stoppt

  Replay                  Event Log -\> Reducer -\> identischer DerivedState wie Live

  LLM-Isolation           LLM-Output mit falschem Schema -\> commit rejected, keine Action
  -----------------------------------------------------------------------------------------------------

# Teil VII: Code-Interfaces & Roadmap

## 14. Modul-Interfaces (vollstündig)

```
// Core Interfaces

interface IIngressSource { emit(): IngressEvent }

interface IExtractor { extract(event: IngressEvent): Artifact[] }

interface ISchemaRegistry { validate(a: Artifact): ValidationResult; get(id: string): JsonSchema }

interface IStore { append(v: ArtifactVersion): void; commit(vs: ArtifactVersion[], es: Event[]): void }

interface IReducer\<T\> { reduce(artifacts: Artifact[]): T }

interface IVerifier { check(target: Artifact, ctx: VerificationContext): VerificationReport }

interface IApprovalGate { submit(decision: ApprovalDecision): ApprovalArtifact }

interface IMaterializer { materialize(option: OptionArtifact, policy: PolicyArtifact): Action[] }

interface IExecutor { execute(action: Action): ExecutionResult }

interface ITimeSource { now(): UTCTimestamp } // injiziert - nie Date.now() direkt

// Erweiterung durch Module

interface IModuleManifest {

module_id: string

schemas: SchemaRegistration[]

step_kinds: StepKindRegistration[]

verifiers: VerifierRegistration[]

executors: ExecutorRegistration[]

llm_roles: LLMRoleRegistration[]

capabilities: string[]

}
```

## 15. Roadmap V1 bis V3

  -----------------------------------------------------------------------------------------------------------------------------
  **Version**   **Feature**                                      **Prioritüt**   **Begründung**
  ------------- ------------------------------------------------ --------------- ----------------------------------------------
  V1            Typed Steps (Union DSL) + human_summary          Kritisch        Herzstuck Materializer - vor Code definieren

  V1            ApprovalVerifier owns time validity              Kritisch        Clock-Ownership - vor Code definieren

  V1            ITimeSource als injiziertes Interface            Kritisch        Test + Replay-Sauberkeit

  V1            step.kind Vokabular geschlossen (2-3 Kinds)      Hoch            Keine versteckte Komplexitüt

  V1            StoreSQLite + StoreMemory (für Tests)            Hoch            MVP lauffühig

  V1            Module-Manifest System (minimal)                 Mittel          Erweiterbarkeit von Anfang an

  V2            Approval bindet an constraint_hash               Hoch            Manipulationsschutz nach Approval

  V2            prev_hash im Event Log                           Hoch            Tamper-Detection

  V2            action_plan Artefakt (Option-\>Plan-\>Actions)   Mittel          Wenn Materializer komplex wird

  V2            Versionierte Capabilities (statt Freitext)       Mittel          Skalierbarkeit Governance

  V2            world_state Conflict Resolution Strategy         Mittel          Automatische Auflüsungsregeln

  V3            Sensor Inputs / Robotik Module                   Geplant         Echtzeit-Constraints, PhysicalSafetyVerifier

  V3            DroneExecutor über Action-Bus                    Geplant         Externe Systeme sicher ansteürn

  V3            Distributed Store                                Geplant         Multi-Node, Sharding
  -----------------------------------------------------------------------------------------------------------------------------

# Anhang: Drei harte Betriebsregeln

Diese Regeln sind keine Features. Sie sind Stabilitütsvoraussetzungen. Wenn sie code-seitig erzwungen werden, ist der Core echt.

+-------------------------------------------------------------------------------------------+
| **Regel 1: Commit-Boundary (Transaktion)**                                                |
|                                                                                           |
| Alles was zusammengehürt wird in EINER Transaktion committed: artifact_versions + events. |
|                                                                                           |
| Kein Halb-Commit. Kein Event ohne Artefakt. Kein Artefakt ohne Event.                     |
|                                                                                           |
| Konseqünz: Replay ist immer konsistent.                                                   |
+===========================================================================================+
+-------------------------------------------------------------------------------------------+

+----------------------------------------------------------------------------------------------+
| **Regel 2: Reducer-Regel für Head**                                                          |
|                                                                                              |
| Head ist nicht \'latest row in table\', sondern Ergebnis von: events -\> reduce() -\> heads. |
|                                                                                              |
| Head-Wechsel ist selbst ein explizites Event.                                                |
|                                                                                              |
| Kein implizites \'latest version wins\'.                                                     |
|                                                                                              |
| Live und Replay verwenden dieselbe Reducer-Regel.                                            |
+==============================================================================================+
+----------------------------------------------------------------------------------------------+

+-----------------------------------------------------------------------------+
| **Regel 3: At-least-once Execution + Idempotenz**                           |
|                                                                             |
| action_id ist der Idempotency Key.                                          |
|                                                                             |
| execution_result muss die referenzierte action_id/Action-Version enthalten. |
|                                                                             |
| Executor darf mehrfach aufgerufen werden - darf nichts doppelt tun.         |
|                                                                             |
| Voraussetzung: jede Action muss idempotent ausführbar sein.                 |
+=============================================================================+
+-----------------------------------------------------------------------------+

*ARCS Core - Version 1.0 - 2026*

*Der Core definiert nur Regeln, nicht Funktionen. Alles andere ist Erweiterung.*
