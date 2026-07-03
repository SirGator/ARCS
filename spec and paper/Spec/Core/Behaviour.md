# Core

## Behaviour

This section defines the logical behaviour of the ARCS Core.

It does not describe classes, functions, files, frameworks, or implementation details. It defines what the Core receives, what it produces, what it is responsible for, and what it is explicitly forbidden to do.

The Core is the coordination and governance layer of ARCS. Its primary behaviour is to enforce valid transitions between schema-valid artifacts, committed events, reducer-derived state, verification results, approvals, actions, execution results, and external outputs.

The Core MUST NOT act directly on raw external input, model output, module output, tool output, or hidden runtime state.

---

### Input

The Core receives information required to coordinate the current request lifecycle.

Input MAY include:

* external signals normalized by Input Adapters,
* committed artifacts,
* committed events,
* reducer-derived state,
* active policy references,
* effective permissions,
* schema definitions,
* module manifests,
* verifier results,
* approval state,
* executor results,
* runtime metadata,
* trace and provenance metadata.

The Core MUST distinguish between authoritative and non-authoritative input.

Authoritative input consists only of:

* schema-valid committed artifacts,
* committed events,
* reducer-derived state,
* active policy artifacts,
* effective permission artifacts,
* approved configuration artifacts.

Runtime-local data, temporary buffers, model outputs, tool responses, UI state, and context views are not authoritative by themselves.

The Core MUST NOT assume hidden state that is not represented by committed artifacts, committed events, reducer-derived state, or explicitly provided runtime input.

---

### Output

The Core produces controlled state transitions and routing outcomes.

Core outputs MAY include:

* accepted `ingress_event` artifacts,
* derived `task` artifacts,
* committed `option` artifacts,
* committed `verification_report` artifacts,
* committed `approval` artifacts when required by policy,
* committed `action` artifacts,
* committed `execution_result` artifacts,
* blocked or failed derived task states,
* external response payloads through Output Adapters,
* provenance and audit records.

If an output affects authoritative system state, it MUST be represented as a schema-valid artifact or event and committed through the Store.

The Core MUST NOT create implicit state that cannot be validated, traced, replayed, or audited.

The Core MUST NOT treat temporary runtime output as authoritative unless it has been converted into a schema-valid artifact and committed.

---

### Responsibilities

The Core is responsible for enforcing the ARCS lifecycle.

The Core MUST:

1. receive normalized external signals from Input Adapters,
2. ensure that `ingress_event` artifacts pass schema validation before commit,
3. coordinate task extraction from committed ingress events,
4. ensure that `task` artifacts are schema-valid before commit,
5. derive task state only through Reducers,
6. assemble runtime context only from committed facts and reducer-derived state,
7. route generated options through schema validation,
8. ensure that options are committed before verification when required by the flow,
9. invoke or coordinate Verification Engines,
10. commit `verification_report` artifacts,
11. block paths with `fail` or `unknown` verification results,
12. enforce approval requirements defined by policy,
13. ensure that required approvals are committed before materialization,
14. coordinate deterministic action materialization,
15. ensure that actions are schema-valid, scoped, permission-bound, and idempotent,
16. require action verification before execution,
17. dispatch only verified actions to Executors,
18. commit execution results after actual execution,
19. update derived state through Reducers,
20. produce controlled external outputs through Output Adapters,
21. preserve replayability, provenance, and auditability.

The Core is responsible for enforcing separation between lifecycle stages.

The Core MUST ensure that:

* an `ingress_event` is not treated as a `task`,
* a `task` is not treated as an `option`,
* an `option` is not treated as an executable command,
* a `verification_report` is not treated as approval,
* an `approval` is not treated as an action,
* an `action` is not treated as an execution result,
* an `execution_result` is not automatically treated as memory,
* runtime context is not treated as source of truth,
* model output is not treated as authority,
* module output is not trusted until verified,
* tool output is not authoritative until represented as a committed artifact.

---

### State

The Core MAY use temporary runtime state during request processing.

Temporary runtime state MAY include:

* routing metadata,
* context assembly buffers,
* validation results before commit,
* transient verifier outputs before artifact creation,
* temporary selection buffers,
* executor dispatch metadata,
* output formatting buffers.

Temporary runtime state MUST NOT become authoritative state by itself.

Long-term or authoritative state MUST be represented as committed artifacts and committed events in the Store.

Reducer-derived state MUST be reconstructable from committed artifacts and events.

The Core MUST treat divergence between live state and replayed state as a consistency failure.

The Core MUST NOT maintain hidden mutable state that affects future decisions without being represented in committed artifacts or events.

---

### Decisions

The Core MAY make lifecycle and routing decisions within its authority.

Core-level decisions MAY include:

* whether an artifact passes schema validation,
* whether a lifecycle path may continue,
* whether verification is required,
* whether approval is required by policy,
* whether an action may be materialized,
* whether execution may be dispatched,
* whether a task must enter `blocked`, `failed`, `waiting_for_approval`, `waiting_for_input`, `deferred`, `cancelled`, or `completed`.

The Core MUST NOT make semantic decisions that belong to specialized modules unless those decisions are represented through the appropriate artifact flow.

The Core MUST NOT decide that an unsafe or unknown path is acceptable.

The Core MUST NOT convert model confidence into authority.

Any decision that affects execution, approval, safety, memory, permissions, policy, external side effects, or authoritative state MUST be explicit, traceable, and replayable.

If option selection or ranking affects execution, the selection rationale MUST be persisted as provenance or as a schema-valid selection artifact.

---

### Constraints

The Core MUST follow all active schemas, policies, permissions, module manifests, verifier requirements, and store consistency rules.

The Core MUST NOT:

* execute raw external input,
* execute model output directly,
* execute module output directly,
* bypass schema validation,
* bypass verification,
* bypass approval when approval is required by policy,
* bypass action verification,
* mutate committed artifacts outside the Store,
* create authoritative state through hidden memory,
* dispatch unverified actions,
* silently ignore missing permissions,
* silently ignore invalid schemas,
* silently repair hard blockers,
* treat `unknown` as `pass`,
* treat failed verification as recoverable without explicit remediation,
* allow policy drift between approval and execution,
* allow runtime context to override committed artifacts,
* allow modules or adapters to bypass the Core lifecycle.

If required information is missing, invalid, contradictory, or unverifiable, the Core MUST block the affected path or transition the task into a controlled failure or waiting state.

The Core MUST NOT invent missing information.

---

### Failure Behaviour

Core failures MUST be explicit.

A Core failure MAY be represented through:

* `verification_report`,
* `risk`,
* `conflict`,
* blocked derived state,
* failed derived state,
* `execution_result` if execution had already started,
* `incident_report`,
* another specific schema-valid artifact.

The Core MUST distinguish recoverable failures from hard blockers.

Recoverable failures MAY include:

* temporary executor failure,
* unavailable non-critical adapter,
* missing optional context,
* retryable external system failure,
* transient runtime failure.

Hard blockers include:

* invalid schema,
* missing permission,
* policy violation,
* unknown verification result,
* unsafe action,
* expired approval,
* policy drift,
* corrupted reducer state,
* replay mismatch,
* unauthorized Core mutation,
* unscoped external side effect,
* contradictory required artifacts.

Hard blockers MUST NOT be repaired silently.

If the Core cannot safely continue, it MUST transition the affected task or path to a controlled state such as:

* `blocked`,
* `failed`,
* `waiting_for_approval`,
* `waiting_for_input`,
* `deferred`,
* `cancelled`.

A failure representation SHOULD include:

* what failed,
* where it failed,
* which artifact or lifecycle stage was affected,
* which input caused or contributed to the failure,
* whether the failure is recoverable,
* which policy, schema, verifier, or permission blocked continuation,
* which component should handle the next step.

The Core MUST NOT hide failures behind default behaviour if the default behaviour could change authoritative state, execution eligibility, safety, auditability, or replayability.

---

### Traceability

Every Core-relevant output MUST be traceable to its input artifacts, committed events, active policies, effective permissions, verifier results, approvals, and lifecycle stage.

The Core MUST make it possible to answer:

* Which external signal started the lifecycle?
* Which `ingress_event` was created?
* Which `task` was derived?
* Which artifacts were committed?
* Which reducer-derived state was used?
* Which runtime context was assembled?
* Which options existed?
* Which options were rejected or blocked?
* Which verification reports were produced?
* Which approvals were required?
* Which approvals were granted, denied, expired, or missing?
* Which action was materialized?
* Which action verification result allowed or blocked execution?
* Which executor ran?
* Which execution result was produced?
* Which side effects occurred?
* Which final state was derived?
* Can the lifecycle be replayed?

Traceability is REQUIRED so that ARCS can be debugged, verified, audited, reproduced, and safely extended.

Final rule:

**If the Core cannot explain and replay a transition from committed artifacts and events, that transition is not authoritative.**
