# ARCS System Flow

Status: Draft
Spec Level: Normative
Scope: Core Runtime Flow

---

## Purpose

This document defines the normative system flow of ARCS.

ARCS MUST NOT execute external input directly. Every authoritative state transition MUST be represented by schema-valid artifacts and committed events. Runtime objects MAY be used for computation, routing, planning, formatting, or temporary context assembly, but runtime objects MUST NOT become authoritative state unless converted into schema-valid artifacts and committed through the Store.

The core rule is:

**Committed artifacts and committed events are the source of truth. Runtime objects are derived views.**

No component MAY create authoritative state through hidden mutable memory, uncommitted runtime state, implicit side effects, or non-replayable local state.

---

## Normative Language

The following terms are used as defined in RFC-style specifications:

* `MUST` means the rule is mandatory.
* `MUST NOT` means the behavior is forbidden.
* `SHOULD` means the behavior is strongly recommended unless a justified exception exists.
* `MAY` means the behavior is allowed but optional.
* `REQUIRED` means a condition must be satisfied for the flow to continue.

---

## High-Level Flow

```text
External Input
    ↓
Input Adapter
    ↓
ingress_event
    ↓
Schema Gate
    ↓
Store Commit
    ↓
Task Extractor
    ↓
task
    ↓
Schema Gate
    ↓
Store Commit
    ↓
Reducers derive TaskState
    ↓
Context Assembly
    ↓
Option Generator
    ↓
option
    ↓
Schema Gate
    ↓
Store Commit
    ↓
Verification Engine
    ↓
verification_report
    ↓
Store Commit
    ↓
fail / unknown → blocked
    ↓
pass
    ↓
Action Materializer
    ↓
action_candidate
    ↓
Schema Gate
    ↓
Store Commit
    ↓
Approval Gate, if required by policy
    ├── approval required → approval_request → Schema Gate → Store Commit → approval → Schema Gate → Store Commit
    └── approval not required → continue with policy-derived rationale
    ↓
Action Promotion
    ↓
action
    ↓
Schema Gate
    ↓
Store Commit
    ↓
Action Verification
    ↓
verification_report
    ↓
Store Commit
    ↓
fail / unknown → blocked
    ↓
pass
    ↓
Executor
    ↓
execution_result
    ↓
Schema Gate
    ↓
Store Commit
    ↓
Reducers update derived state
    ↓
Output Adapter
    ↓
External Output
```

This flow MUST prevent direct planner-to-executor execution.

A proposed operation MUST first become an explicit `option`, pass schema validation, pass option verification, be deterministically materialized into an `action_candidate`, satisfy approval requirements if required by policy against that concrete candidate, be promoted into an `action`, pass action verification, and only then reach an Executor.

---

## Request Lifecycle

A request lifecycle begins when an external signal enters ARCS through an Input Adapter.

External signals MAY originate from:

* user interfaces,
* APIs,
* file events,
* scheduled tasks,
* tool callbacks,
* sensor events,
* watchers,
* internal system events.

The Input Adapter MUST create an `ingress_event`.

The Input Adapter MUST NOT decide how ARCS should respond. Its responsibility is limited to normalization, source attribution, trust assignment, and construction of a schema-valid input artifact.

The `ingress_event` MUST pass the Schema Gate before it MAY enter the Store.

Input that fails schema validation, source validation, trust validation, or policy-defined ingress constraints MUST NOT be extracted into a task.

After the `ingress_event` has been committed, the Task Extractor MAY derive a `task` artifact from it.

A `task` represents an interpreted goal, obligation, or system-relevant event. A `task` MUST NOT contain execution authority.

Reducers MUST derive `TaskState` from committed artifacts and committed events. `TaskState` MUST NOT be manually mutated by planners, adapters, tools, executors, modules, user interfaces, or runtime workers.

Context Assembly MAY build a temporary runtime context from committed system facts and reducer-derived state.

Runtime context MAY include:

* task state,
* active policy references,
* effective permissions,
* memory artifact references,
* claim references,
* evidence references,
* world state references,
* available modules,
* available tools,
* unresolved risks,
* conflicts,
* prior artifacts,
* active goals,
* autonomy constraints.

Runtime context MUST NOT introduce new authoritative facts.

The Option Generator MAY create one or more `option` artifacts.

An `option` is a proposal. An `option` MUST NOT be executed.

Each `option` MUST pass schema validation and option verification. Verification MUST produce a `verification_report`.

ARCS MUST follow the blocking rule:

**fail = blocked**
**unknown = blocked**

Only an option with a `pass` verification result MAY continue to action-candidate materialization and approval evaluation.

If policy requires approval, the Approval Gate MUST create an `approval_request` and then an `approval` artifact bound to the concrete `action_candidate` before an executable `action` MAY be created.

If policy does not require approval, ARCS MAY continue without an approval artifact, but the reason MUST be derivable from committed policy, permission, verification, and action-candidate artifacts.

The Action Materializer MUST convert a verified option into an `action_candidate`.

The Action Promotion step MUST convert an approved-if-required `action_candidate` into an executable `action`.

The Action Materializer and Action Promotion step MUST be deterministic. They MUST NOT call an LLM, parse natural language, infer missing execution parameters, perform I/O, or invent execution details.

The `action` MUST pass action verification before execution.

The Executor MUST run only verified actions. After execution, the Executor MUST produce an `execution_result`.

An `execution_result` records actual execution. If execution was blocked before the Executor started, ARCS MUST represent that state through a `verification_report`, `risk`, `conflict`, blocked derived state, or another specific governance artifact. ARCS MUST NOT create a fake `execution_result` for an action that never executed.

---

## Core Responsibilities

The ARCS Core is responsible for enforcing valid state transitions between schema-valid artifacts.

The Core MUST:

1. validate incoming artifacts,
2. commit valid artifacts and corresponding events atomically,
3. derive state through Reducers,
4. assemble runtime context only from committed facts and reducer-derived state,
5. route generated options through schema validation and verification,
6. verify options,
7. enforce approval requirements,
8. materialize actions deterministically,
9. verify actions before execution,
10. dispatch only verified actions to Executors,
11. commit execution results,
12. update derived state through Reducers,
13. produce an external output or controlled failure response.

The Core MUST enforce separation between stages:

* An `ingress_event` is not a task.
* A `task` is not a plan.
* An `option` is not an executable command.
* A `verification_report` is not approval.
* An `approval` is not an action.
* An `action` is not an execution result.
* An `execution_result` is not automatically memory.
* A runtime context is not a source of truth.
* A model output is not authority.
* A module output is not trusted until verified.
* A tool result is not authoritative until represented as a committed artifact.

If live state and replayed state diverge, ARCS MUST treat this as a consistency failure.

---

## Artifact Flow

ARCS MUST represent state-relevant information as explicit artifacts.

The primary artifact sequence is:

```text
ingress_event
    ↓
task
    ↓
option
    ↓
verification_report for option
    ↓
action_candidate
    ↓
approval_request, if required by policy
    ↓
approval, if required by policy
    ↓
action
    ↓
verification_report for action
    ↓
execution_result
```

---

### ingress_event

An `ingress_event` is created by the Input Adapter.

It represents a normalized external or internal signal.

It MUST record:

* source,
* channel,
* actor identity if available,
* trust metadata,
* timestamp metadata,
* raw payload reference or normalized payload.

An `ingress_event` MUST NOT contain execution authority.

---

### task

A `task` is created by the Task Extractor.

It represents the interpreted goal, obligation, or system-relevant event.

A `task` defines what must be handled. It MUST NOT define how it will be executed.

A `task` MUST NOT directly encode executable tool calls, shell commands, API calls, or file operations.

---

### TaskState

`TaskState` is derived by Reducers.

`TaskState` represents the current derived state of a task.

`TaskState` MUST be reconstructable from committed artifacts and committed events.

`TaskState` MUST NOT be manually mutated.

---

### option

An `option` is created by the Option Generator.

It represents a possible next step, plan candidate, or controlled proposal.

An `option` has no execution authority.

An `option` MAY contain:

* typed steps,
* required capabilities,
* risk metadata,
* human-readable summary,
* estimated cost,
* reversibility,
* safety level,
* expected impact.

An invalid option MUST be rejected by the Schema Gate or blocked by Verification.

Blocked options and their verification reports SHOULD remain audit-visible unless the option was rejected before commit due to schema invalidity.

---

### verification_report

A `verification_report` is created by the Verification Engine.

A `verification_report` MUST specify:

* target artifact,
* target type,
* verification stage,
* status,
* individual checks,
* blockers,
* relevant recommendations.

Allowed statuses are:

```text
pass
fail
unknown
```

Rules:

```text
pass    → may continue to the next gate
fail    → blocked
unknown → blocked
```

A `verification_report` for an option and a `verification_report` for an action MUST be distinguishable.

Example stages:

```text
option_verification
action_verification
module_activation_verification
policy_change_verification
```

---

### approval

An `approval` is created by the Approval Gate when required by policy.

An `approval` records that a verified option MAY become an action under a specific policy version.

An approval MUST bind to:

* target option,
* policy reference,
* approving actor,
* decision,
* timestamp,
* expiry,
* reason.

If policy changes between approval and execution, ARCS MUST block the affected path and require re-verification.

---

### action

An `action` is created by the Action Materializer.

An `action` represents the deterministic executable form of a verified and approved-if-required option.

An action MUST be:

* typed,
* schema-valid,
* scoped,
* permission-bound,
* idempotent.

An action MUST NOT contain unresolved natural language instructions.

An action MUST NOT be created directly from user input, model output, runtime context, or unverified module output.

---

### execution_result

An `execution_result` is created by the Executor after actual execution.

It MUST record:

* referenced action,
* executor identity,
* execution status,
* produced outputs,
* side effects,
* logs or log references,
* failure information if execution failed,
* timeout or cancellation information if applicable.

A blocked action MUST NOT create an `execution_result` unless execution had already started and was then cancelled or aborted.

---

## Decision Flow

A decision in ARCS is not a direct model output. It is a controlled artifact transition.

The decision flow is:

1. Reducers derive the current `TaskState`.
2. Context Assembly builds runtime context from committed facts.
3. The Option Generator creates one or more `option` artifacts.
4. Each option passes the Schema Gate.
5. The Verification Engine creates a `verification_report`.
6. Options with `fail` or `unknown` are blocked.
7. Passing options MAY be ranked by explicit ranking criteria.
8. If ranking affects option selection, the ranking rationale MUST be persisted.
9. Required approval is obtained through the Approval Gate.
10. The Action Materializer creates an `action`.
11. The action passes action verification.
12. The Executor runs the action.
13. The Executor produces an `execution_result`.
14. The result is committed to the Store.
15. Reducers update derived state.

The Core MUST be able to explain every action through committed artifacts, events, and provenance.

For every executed action, ARCS MUST be able to answer:

* Which task caused it?
* Which options existed?
* Which options were rejected or blocked?
* Which verification reports were produced?
* Which option was selected?
* Why was it selected?
* Was approval required?
* Who or what approved it?
* Which policy version was active?
* How was the action materialized?
* Which action was executed?
* Which executor executed it?
* What result was produced?
* What side effects occurred?

Core rule:

**No action may exist without a verified option, deterministic materialization, action verification, and approval if required by policy.**

---

## Option Selection and Ranking

If multiple options pass verification, ARCS MAY select one based on explicit ranking criteria.

Allowed ranking criteria include:

* policy priority,
* safety level,
* required permissions,
* scope,
* goal relevance,
* cost,
* expected duration,
* confidence,
* reversibility,
* user preference,
* resource usage,
* dependency risk,
* prior success rate,
* current system state.

Ranking MUST NOT override verification, approval, permissions, policy, or scope restrictions.

If ranking determines system behavior, ARCS MUST persist enough information to reconstruct why the selected option was chosen.

The ranking record MAY be represented as:

* provenance on the selected option,
* an `option_selection_report` artifact,
* a structured field in a committed workflow artifact.

A lower-risk option SHOULD be preferred when multiple options satisfy the same goal with comparable expected usefulness.

---

## Error and Recovery Flow

ARCS MUST treat failures as explicit artifacts or derived state transitions.

A component MUST NOT swallow, hide, or silently repair a failure if the failure affects authoritative state, verification, approval, execution, replay, security, or auditability.

The detecting component MUST produce a traceable representation, such as:

* `verification_report`,
* `risk`,
* `conflict`,
* `execution_result`,
* `incident_report`,
* or another specific schema-valid artifact.

V1 SHOULD avoid a generic `error` artifact unless its schema and semantics are explicitly defined. Failures SHOULD be represented by the most specific artifact type available.

ARCS separates recoverable failures from hard blockers.

Recoverable failures MAY include:

* temporary tool failure,
* missing optional context,
* unavailable non-critical adapter,
* incomplete non-critical memory result,
* retryable execution failure,
* rejected low-risk option,
* transient runtime failure.

Hard blockers include:

* missing permission,
* policy violation,
* invalid schema,
* unsafe action,
* ambiguous authority,
* expired approval,
* corrupted task state,
* replay mismatch,
* policy drift,
* contradictory required artifacts,
* unknown verification result,
* invalid module manifest,
* unscoped file access,
* unauthorized network access,
* attempted Core mutation.

Hard blockers MUST NOT be repaired silently.

For recoverable failures, ARCS MAY use:

* retry,
* repair,
* fallback,
* downgrade,
* replan,
* defer,
* request clarification,
* abort.

Every recovery attempt MUST record:

* original failure,
* affected artifact,
* detecting component,
* selected recovery strategy,
* reason for selection,
* recovery result,
* final state.

If recovery fails or cannot be performed safely, the task MUST enter a controlled `blocked` or `failed` state.

---

## Blocking Semantics

`blocked` is a derived state indicating that a specific artifact path is not eligible to progress toward materialization, execution, activation, or completion under the current policy, verification result, approval state, permission state, or consistency state.

Blocked paths MUST NOT continue toward execution.

A blocked path MUST record:

* target artifact,
* blocking status,
* blocking verifier or component,
* reason,
* policy reference,
* required remediation,
* whether re-submit is allowed.

Examples:

```text
invalid schema        → reject before commit or blocked if already committed
missing permission    → blocked
unknown scope         → blocked
policy drift          → blocked and re-verify required
expired approval      → blocked
unsafe action         → blocked
replay mismatch       → consistency failure
```

Blocked does not mean deleted.

Blocked artifacts remain part of the audit trail unless they were rejected before commit by the Schema Gate.

---

## Partial Success

Partial success is allowed only when the action or workflow has explicitly defined substeps and each relevant side effect can be audited.

A partial success result MUST include:

* completed substeps,
* failed substeps,
* side effects already produced,
* remaining work,
* whether retry is safe,
* whether rollback is possible,
* affected artifacts,
* affected external systems,
* recommended recovery path.

Partial success MUST NOT be used as a vague success state.

If side effects cannot be reconstructed, bounded, or explained, the result MUST be treated as a failure or incident.

---

## End State

A request lifecycle is complete only if exactly one terminal or waiting state can be derived from committed artifacts and committed events.

Terminal states include:

* `completed`
* `partially_completed`
* `failed`
* `cancelled`
* `revoked`

Waiting states include:

* `blocked`
* `waiting_for_approval`
* `waiting_for_input`
* `deferred`

A valid end state requires:

1. committed final artifacts or explicit rejection before commit,
2. updated Event Log,
3. reducer-derived final state,
4. recorded verification results,
5. recorded approval state if approval was required,
6. committed execution results if execution occurred,
7. discarded or cleaned temporary runtime views,
8. external output or controlled failure response.

The Output Adapter converts the final internal state into an external response.

The Output Adapter MAY format, filter, summarize, or translate the result.

The Output Adapter MUST NOT create new authoritative state.

Memory updates are not automatic.

An execution result MAY become persistent memory only if memory policy accepts it and a corresponding memory artifact is created, validated, and committed.

Final rule:

**If ARCS cannot replay and explain how a state was reached, that state is not authoritative.**
