# Core

## Technical Specification

Status: Draft
Spec Level: Normative
Scope: Core Technical Implementation

---

## 1. Purpose

This section defines the technical implementation contract of the ARCS Core.

It specifies the concrete components, interfaces, data structures, events, state handling rules, algorithms, concurrency behaviour, error handling, security constraints, dependencies, and test requirements required to implement the Core behaviour defined in the previous section.

The Core implementation MUST enforce the ARCS lifecycle:

```text
ingress_event
→ task
→ option
→ verification_report
→ approval, if required by policy
→ action
→ action verification
→ execution_result
→ reducer-derived state
→ external output
```

The Core implementation MUST NOT introduce an alternative request model that bypasses committed artifacts, schema validation, Store commits, Reducers, verification, approval, action materialization, or replayable logging.

---

## 2. Components

The Core is implemented as a set of narrowly scoped components.

Each component MUST have one clearly defined responsibility and MUST NOT take over the responsibility of unrelated lifecycle stages.

---

### 2.1 CoreRuntime

Responsibility:

* Coordinates the request lifecycle.
* Routes artifacts through validation, Store commits, Reducers, verification, approval, materialization, execution, and output.
* Enforces lifecycle ordering.

Uses:

* SchemaGate
* StoreCommitCoordinator
* ReducerRunner
* RuntimeContextAssembler
* OptionRouter
* VerificationCoordinator
* ApprovalCoordinator
* ActionMaterializer
* ExecutionDispatcher
* OutputCoordinator

Must not:

* Execute actions directly.
* Treat runtime context as authoritative state.
* Bypass Store commits.
* Bypass verification.
* Bypass approval when required by policy.

---

### 2.2 SchemaGate

Responsibility:

* Validates artifacts against registered schemas.
* Rejects invalid artifacts before Store commit.
* Records schema validation failures.

Uses:

* SchemaRegistry
* Artifact metadata
* Schema version metadata

Must not:

* Repair invalid artifacts silently.
* Accept unknown schema versions unless policy explicitly allows compatibility mode.
* Convert invalid input into authoritative state.

---

### 2.3 StoreCommitCoordinator

Responsibility:

* Commits valid artifacts and corresponding events atomically.
* Enforces append-only Store rules.
* Enforces optimistic concurrency and head advancement rules.

Uses:

* ArtifactStore
* EventLog
* CommitTransaction
* StoreHead

Must not:

* Commit artifacts without events.
* Commit events without artifacts when an artifact change is required.
* Mutate committed artifacts.
* Allow partial commits.

---

### 2.4 ReducerRunner

Responsibility:

* Runs deterministic Reducers over committed artifacts and events.
* Produces reducer-derived state such as `TaskState`.
* Supports replay and consistency checking.

Uses:

* ReducerRegistry
* EventLog
* ArtifactStore
* StoreHead

Must not:

* Perform I/O inside Reducers.
* Use wall-clock time, randomness, or hidden mutable state inside Reducers.
* Mutate committed artifacts.
* Produce non-replayable state.

---

### 2.5 RuntimeContextAssembler

Responsibility:

* Builds temporary runtime context from committed artifacts, committed events, reducer-derived state, active policy, effective permissions, and explicit runtime metadata.
* Provides context to option generation, verification, approval evaluation, materialization, and output formatting.

Uses:

* ArtifactStore
* ReducerRunner
* PolicyResolver
* PermissionResolver
* ModuleRegistry

Must not:

* Introduce new authoritative facts.
* Override committed artifacts.
* Treat model output or tool output as authority.
* Persist runtime context as authoritative state unless converted into a schema-valid artifact and committed.

---

### 2.6 OptionRouter

Responsibility:

* Receives options from Option Generators.
* Ensures options pass schema validation.
* Routes valid options to verification.
* Ensures invalid options are rejected or blocked according to lifecycle rules.

Uses:

* SchemaGate
* StoreCommitCoordinator
* VerificationCoordinator

Must not:

* Treat an option as an action.
* Execute options.
* Select unsafe options.
* Bypass verification.

---

### 2.7 VerificationCoordinator

Responsibility:

* Coordinates verification of options, actions, module activations, policy changes, or other verifiable artifacts.
* Produces `verification_report` artifacts.
* Enforces `fail = blocked` and `unknown = blocked`.

Uses:

* VerifierRegistry
* PolicyResolver
* PermissionResolver
* RiskRules
* SchemaGate
* StoreCommitCoordinator

Must not:

* Treat unknown as pass.
* Hide failed checks.
* Convert model confidence into verification authority.
* Allow execution to continue after failed or unknown verification.

---

### 2.8 ApprovalCoordinator

Responsibility:

* Determines whether approval is required by active policy.
* Creates approval requests when required.
* Validates received approvals.
* Ensures approvals bind to policy version, actor, target option, expiry, and decision.
* Detects policy drift between approval and execution.

Uses:

* PolicyResolver
* PermissionResolver
* ApprovalStore
* StoreCommitCoordinator

Must not:

* Create approval for invalid or unverified options.
* Treat missing approval as approval.
* Allow expired approval.
* Allow approval under an outdated policy version.

---

### 2.9 ActionMaterializer

Responsibility:

* Converts a verified and approved-if-required option into a deterministic `action` artifact.
* Ensures actions are typed, scoped, permission-bound, schema-valid, and idempotent.

Uses:

* MaterializationRules
* SchemaGate
* PermissionResolver
* StoreCommitCoordinator

Must not:

* Call an LLM.
* Parse natural language at execution time.
* Invent missing parameters.
* Perform external I/O.
* Create an action directly from user input, model output, or runtime context.
* Materialize an action from an unverified option.

---

### 2.10 ActionVerifier

Responsibility:

* Verifies materialized actions before execution.
* Produces a `verification_report` for action verification.
* Blocks unsafe, unscoped, unauthorized, invalid, or unknown actions.

Uses:

* VerificationCoordinator
* PolicyResolver
* PermissionResolver
* CapabilityRegistry

Must not:

* Dispatch actions to Executors.
* Treat prior option verification as sufficient for execution.
* Bypass permission checks.

---

### 2.11 ExecutionDispatcher

Responsibility:

* Dispatches verified actions to the correct Executor.
* Enforces idempotency.
* Handles timeouts, cancellation, and executor errors.
* Ensures actual execution produces an `execution_result`.

Uses:

* ExecutorRegistry
* IdempotencyRegistry
* StoreCommitCoordinator
* TimeoutController

Must not:

* Execute unverified actions.
* Execute blocked actions.
* Execute free-form natural language instructions.
* Create fake execution results for actions that did not run.
* Retry non-idempotent actions.

---

### 2.12 OutputCoordinator

Responsibility:

* Converts final internal state into external output through Output Adapters.
* Formats, filters, summarizes, or translates final state for external consumers.

Uses:

* OutputAdapterRegistry
* ArtifactStore
* ReducerRunner
* PolicyResolver

Must not:

* Create authoritative state.
* Modify committed artifacts.
* Hide safety-relevant failure state.
* Present uncommitted runtime data as authoritative output.

---

### 2.13 ConsistencyMonitor

Responsibility:

* Compares live state with replay-derived state.
* Detects replay mismatch, reducer divergence, commit inconsistency, or corrupted derived state.
* Emits consistency failures.

Uses:

* ArtifactStore
* EventLog
* ReducerRunner
* StoreHead

Must not:

* Repair consistency failures silently.
* Continue execution after unreconciled replay mismatch.
* Mark inconsistent state as authoritative.

---

## 3. Public Interfaces

The Core exposes stable interfaces for lifecycle coordination.

Internal implementation details MUST NOT be used by modules, adapters, tools, or executors.

---

### 3.1 Accept Ingress Event

```text
accept_ingress_event(signal: AdapterSignal) -> CommitResult<ArtifactRef<ingress_event>>
```

Input:

* `AdapterSignal`

Output:

* committed `ingress_event` reference

Errors:

* `SchemaValidationError`
* `IngressPolicyError`
* `StoreCommitError`

Side effects:

* yes, commits artifact and event if valid

Rules:

* Raw external input MUST be normalized by an Input Adapter before this interface is called.
* The resulting `ingress_event` MUST pass schema validation before commit.

---

### 3.2 Derive Task

```text
derive_task(ingress_event_ref: ArtifactRef<ingress_event>) -> CommitResult<ArtifactRef<task>>
```

Input:

* committed `ingress_event` reference

Output:

* committed `task` reference

Errors:

* `MissingArtifactError`
* `TaskExtractionError`
* `SchemaValidationError`
* `StoreCommitError`

Side effects:

* yes, commits task artifact and event if valid

Rules:

* The task MUST NOT contain execution authority.
* The task MUST describe what must be handled, not how it will be executed.

---

### 3.3 Derive Task State

```text
derive_task_state(task_ref: ArtifactRef<task>, store_head: StoreHead) -> TaskState
```

Input:

* committed `task` reference
* Store head

Output:

* reducer-derived `TaskState`

Errors:

* `ReducerError`
* `ReplayMismatchError`
* `MissingArtifactError`

Side effects:

* no authoritative side effects

Rules:

* `TaskState` MUST be derived only from committed artifacts and events.
* `TaskState` MUST be replayable.

---

### 3.4 Assemble Runtime Context

```text
assemble_runtime_context(task_ref: ArtifactRef<task>, store_head: StoreHead) -> RuntimeContextView
```

Input:

* committed task reference
* Store head

Output:

* temporary `RuntimeContextView`

Errors:

* `MissingPolicyError`
* `MissingPermissionError`
* `ContextAssemblyError`

Side effects:

* no authoritative side effects

Rules:

* `RuntimeContextView` MUST NOT be treated as source of truth.
* Runtime context MUST be derived from committed facts, reducer-derived state, active policy, effective permissions, and explicit runtime metadata.

---

### 3.5 Submit Option

```text
submit_option(option: OptionDraft, task_ref: ArtifactRef<task>) -> CommitResult<ArtifactRef<option>>
```

Input:

* `OptionDraft`
* task reference

Output:

* committed `option` reference

Errors:

* `SchemaValidationError`
* `InvalidOptionError`
* `StoreCommitError`

Side effects:

* yes, commits option artifact and event if valid

Rules:

* An option MUST NOT be executable.
* An option MUST be verified before it may continue toward action materialization.

---

### 3.6 Verify Artifact

```text
verify_artifact(target_ref: ArtifactRef, stage: VerificationStage) -> CommitResult<ArtifactRef<verification_report>>
```

Input:

* target artifact reference
* verification stage

Output:

* committed `verification_report`

Errors:

* `VerifierError`
* `MissingArtifactError`
* `StoreCommitError`

Side effects:

* yes, commits verification report and event

Rules:

* Allowed verification statuses are `pass`, `fail`, and `unknown`.
* `fail` and `unknown` MUST block continuation.
* Verification reports for options and actions MUST be distinguishable.

---

### 3.7 Evaluate Approval Requirement

```text
evaluate_approval_requirement(option_ref: ArtifactRef<option>, policy_ref: ArtifactRef<policy>) -> ApprovalRequirement
```

Input:

* verified option reference
* active policy reference

Output:

* approval requirement result

Errors:

* `MissingPolicyError`
* `PolicyEvaluationError`

Side effects:

* no authoritative side effects unless an approval request artifact is created by a separate call

Rules:

* If approval is required by policy, no action may be materialized before approval is committed.
* If approval is not required, the rationale MUST be derivable from committed policy and verification artifacts.

---

### 3.8 Commit Approval

```text
commit_approval(approval: ApprovalDraft) -> CommitResult<ArtifactRef<approval>>
```

Input:

* approval draft

Output:

* committed approval artifact

Errors:

* `SchemaValidationError`
* `ApprovalExpiredError`
* `PolicyDriftError`
* `UnauthorizedApproverError`
* `StoreCommitError`

Side effects:

* yes, commits approval artifact and event

Rules:

* Approval MUST bind to target option, policy reference, actor, decision, timestamp, expiry, and reason.
* Policy drift MUST block continuation.

---

### 3.9 Materialize Action

```text
materialize_action(option_ref: ArtifactRef<option>) -> CommitResult<ArtifactRef<action>>
```

Input:

* verified and approved-if-required option reference

Output:

* committed action artifact

Errors:

* `MissingVerificationError`
* `MissingApprovalError`
* `PolicyDriftError`
* `MaterializationError`
* `SchemaValidationError`
* `StoreCommitError`

Side effects:

* yes, commits action artifact and event

Rules:

* Materialization MUST be deterministic.
* The Materializer MUST NOT call an LLM or perform external I/O.
* The action MUST be typed, scoped, permission-bound, and idempotent.

---

### 3.10 Dispatch Action

```text
dispatch_action(action_ref: ArtifactRef<action>) -> CommitResult<ArtifactRef<execution_result>>
```

Input:

* verified action reference

Output:

* committed execution result artifact

Errors:

* `MissingActionVerificationError`
* `ExecutorUnavailableError`
* `ExecutionTimeoutError`
* `ExecutionFailedError`
* `IdempotencyError`
* `StoreCommitError`

Side effects:

* yes, may trigger external side effects

Rules:

* Only verified actions MAY be dispatched.
* Idempotency MUST be enforced.
* Executor output MUST be represented as an `execution_result`.

---

### 3.11 Produce External Output

```text
produce_external_output(task_ref: ArtifactRef<task>, output_target: OutputTarget) -> ExternalOutput
```

Input:

* task reference
* output target

Output:

* external response payload

Errors:

* `OutputFormattingError`
* `PolicyFilteringError`

Side effects:

* external output only

Rules:

* Output formatting MUST NOT create authoritative state.
* External output MUST reflect committed artifacts and reducer-derived final state.

---

## 4. Core Data Structures

---

### 4.1 ArtifactRef

```text
ArtifactRef:
- artifact_id: ArtifactId
- artifact_type: ArtifactType
- version: ArtifactVersion
- schema_version: SchemaVersion
- store_head: StoreHead
```

Rules:

* An `ArtifactRef` MUST point to a committed artifact.
* Runtime drafts MUST NOT be represented as committed artifact references.

---

### 4.2 CommitResult

```text
CommitResult<T>:
- status: committed | rejected | failed
- artifact_ref: T | null
- event_ref: EventRef | null
- errors: ErrorRecord[]
- store_head_before: StoreHead
- store_head_after: StoreHead | null
```

Rules:

* `committed` requires both artifact and event.
* `rejected` means the artifact was not committed.
* `failed` means commit failed after validation or during Store operation.

---

### 4.3 RuntimeContextView

```text
RuntimeContextView:
- task_ref: ArtifactRef<task>
- task_state: TaskState
- policy_refs: ArtifactRef<policy>[]
- permission_refs: ArtifactRef<permission_grant>[]
- memory_refs: ArtifactRef[]
- claim_refs: ArtifactRef<claim>[]
- evidence_refs: ArtifactRef<evidence>[]
- risk_refs: ArtifactRef<risk>[]
- conflict_refs: ArtifactRef<conflict>[]
- available_modules: ModuleDescriptor[]
- available_tools: ToolDescriptor[]
- autonomy_constraints: ConstraintRef[]
- store_head: StoreHead
```

Rules:

* `RuntimeContextView` MUST NOT be authoritative.
* It MUST be reconstructable from committed artifacts, events, reducer-derived state, and explicit runtime metadata.
* It MUST NOT contain invented facts.

---

### 4.4 VerificationStage

```text
VerificationStage:
- option_verification
- action_verification
- module_activation_verification
- policy_change_verification
```

Rules:

* Verification reports MUST declare their stage.
* Option verification MUST NOT substitute action verification.

---

### 4.5 ApprovalRequirement

```text
ApprovalRequirement:
- required: boolean
- policy_ref: ArtifactRef<policy>
- reason: string
- required_actor_role: string | null
- expiry_required: boolean
```

Rules:

* If `required = true`, materialization MUST NOT continue until valid approval is committed.
* If `required = false`, the reason MUST be derivable from policy and verification artifacts.

---

## 5. Events

The Core MUST emit events for all authoritative lifecycle transitions.

Required event metadata:

```text
Event:
- event_id
- event_type
- artifact_id
- artifact_type
- artifact_version
- request_id
- task_id
- timestamp
- source_component
- payload_schema_version
- causation_id
- correlation_id
- store_head_before
- store_head_after
```

---

### 5.1 Emitted Events

```text
core.ingress.accepted
core.ingress.rejected

core.task.derived
core.task.blocked
core.task.failed
core.task.completed
core.task.waiting_for_approval
core.task.waiting_for_input
core.task.deferred
core.task.cancelled
core.task.revoked

core.option.committed
core.option.rejected
core.option.blocked
core.option.selected

core.verification.started
core.verification.completed
core.verification.failed
core.verification.unknown
core.verification.blocked

core.approval.required
core.approval.not_required
core.approval.granted
core.approval.denied
core.approval.expired
core.approval.policy_drift_detected

core.action.materialized
core.action.materialization_failed
core.action.verified
core.action.blocked
core.action.dispatched

core.execution.started
core.execution.completed
core.execution.failed
core.execution.timeout
core.execution.cancelled
core.execution.aborted

core.reducer.completed
core.reducer.failed
core.replay.mismatch
core.consistency.failure

core.output.produced
core.output.failed
```

---

### 5.2 Consumed Events

The Core MAY consume:

```text
adapter.signal.received
module.option.proposed
approval.response.received
executor.result.available
policy.updated
permission.revoked
module.registered
module.revoked
watcher.triggered
scheduler.triggered
```

Consumed events MUST be converted into schema-valid artifacts or used only as non-authoritative runtime triggers.

---

## 6. State Handling

The Core does not own hidden long-term state.

State model:

```text
owns hidden long-term state: no
reads committed artifacts: yes
writes committed artifacts: yes, through Store only
reads committed events: yes
writes committed events: yes, through Store only
uses temporary request-local state: yes
uses reducer-derived state: yes
```

Rules:

* Long-term state MUST live in committed artifacts and events.
* Derived state MUST be produced by Reducers.
* Temporary request-local state MUST NOT affect future decisions unless represented as committed artifacts or events.
* Live state and replayed state MUST converge.
* Divergence MUST be treated as a consistency failure.

---

## 7. Algorithms

---

### 7.1 Main Request Lifecycle Algorithm

```text
1. Receive normalized external signal from Input Adapter.
2. Create ingress_event draft.
3. Validate ingress_event against schema.
4. Commit ingress_event and event atomically.
5. Derive task from committed ingress_event.
6. Validate task against schema.
7. Commit task and event atomically.
8. Run Reducers to derive TaskState.
9. Assemble RuntimeContextView.
10. Route Option Generator output into option draft.
11. Validate option against schema.
12. Commit option and event atomically.
13. Verify option.
14. Commit verification_report and event atomically.
15. If verification status is fail or unknown, derive blocked state and stop executable path.
16. Evaluate approval requirement.
17. If approval is required, wait for approval artifact.
18. Validate approval.
19. If approval is missing, denied, expired, or policy-drifted, block path.
20. Materialize action deterministically.
21. Validate action against schema.
22. Commit action and event atomically.
23. Verify action.
24. Commit action verification_report and event atomically.
25. If action verification is fail or unknown, block path.
26. Dispatch verified action to Executor.
27. Receive executor result.
28. Validate execution_result against schema.
29. Commit execution_result and event atomically.
30. Run Reducers to update derived state.
31. Produce external output from committed artifacts and derived final state.
```

---

### 7.2 Blocking Algorithm

```text
1. Receive blocking condition from SchemaGate, VerificationCoordinator, ApprovalCoordinator, ActionVerifier, Store, Reducer, or Executor.
2. Determine affected artifact path.
3. Determine whether the blocker is recoverable.
4. If recoverable, create explicit recovery path or waiting state.
5. If hard blocker, stop executable path.
6. Commit specific artifact or event representing the blocker.
7. Derive blocked or failed TaskState through Reducers.
8. Produce controlled external output if required.
```

Rules:

* `fail` MUST block.
* `unknown` MUST block.
* Missing required approval MUST block.
* Policy drift MUST block.
* Replay mismatch MUST create consistency failure.

---

### 7.3 Replay Algorithm

```text
1. Load committed events in canonical order.
2. Load referenced committed artifact versions.
3. Apply Reducers deterministically.
4. Reconstruct derived state.
5. Compare replay-derived state with live derived state.
6. If states match, replay is valid.
7. If states diverge, emit consistency failure and block affected execution paths.
```

Rules:

* Reducers MUST be deterministic.
* Reducers MUST NOT perform I/O.
* Reducers MUST NOT depend on wall-clock time, randomness, hidden memory, or external services.

---

## 8. Concurrency and Async Behaviour

The Core MUST be safe for parallel request execution.

Concurrency rules:

* Request-local data MUST NOT leak between requests.
* Shared state MUST be accessed through controlled Store interfaces.
* Store commits MUST use optimistic concurrency or equivalent conflict detection.
* Store head advancement MUST be atomic.
* Reducer execution MUST be deterministic regardless of parallel scheduling.
* Executor dispatch MUST enforce idempotency.
* Approval state MUST be rechecked before materialization and execution.
* Policy references MUST be checked for drift before execution.

Async behaviour:

* Long-running execution MUST expose timeout behaviour.
* Long-running execution SHOULD expose cancellation behaviour.
* Waiting states MUST be represented explicitly.
* Background work MUST NOT create authoritative state without Store commit.
* Queue messages MUST NOT be treated as authoritative unless converted into committed artifacts or events.

---

## 9. Error Handling

The Core MUST fail explicitly.

Common error categories:

```text
SchemaValidationError
IngressPolicyError
MissingArtifactError
StoreCommitError
ReducerError
ReplayMismatchError
ContextAssemblyError
InvalidOptionError
VerifierError
MissingVerificationError
MissingApprovalError
ApprovalExpiredError
PolicyDriftError
UnauthorizedApproverError
MaterializationError
MissingActionVerificationError
ExecutorUnavailableError
ExecutionTimeoutError
ExecutionFailedError
IdempotencyError
OutputFormattingError
ConsistencyFailure
UnknownCoreError
```

Each error record SHOULD include:

```text
ErrorRecord:
- error_id
- error_type
- lifecycle_stage
- affected_artifact_ref
- cause
- recoverable: boolean
- policy_ref: optional
- verifier_ref: optional
- component
- timestamp
```

Rules:

* Silent failure is forbidden.
* Hard blockers MUST NOT be repaired silently.
* Unknown verification MUST NOT be treated as pass.
* Missing permissions MUST NOT be ignored.
* Invalid schemas MUST NOT be coerced into valid artifacts without explicit repair flow.
* Unknown internal errors SHOULD transition the affected path to `failed` or `blocked`.

---

## 10. Logging and Traceability

Logs support debugging and operations. Logs are not the source of truth.

Committed artifacts and events remain the source of truth.

Trace requirements:

* input artifact references,
* output artifact references,
* causation IDs,
* correlation IDs,
* decision points,
* verification results,
* approval decisions,
* materialization references,
* action references,
* execution result references,
* error cases,
* execution duration,
* Store head before and after commit.

The Core MUST make every executed action traceable to:

```text
ingress_event
→ task
→ option
→ verification_report
→ approval, if required
→ action
→ action verification
→ execution_result
```

If a transition cannot be traced and replayed, it is not authoritative.

---

## 11. Security and Permissions

The Core MUST enforce active policies and permissions.

Required permission checks include:

* ingress source trust,
* option capability requirements,
* action scope,
* action actor authority,
* executor capability,
* module permission boundaries,
* approval authority,
* file access scope,
* network access scope,
* external side-effect scope.

Forbidden actions:

* execute raw external input,
* execute model output directly,
* execute unverified module output,
* execute unverified actions,
* bypass schema validation,
* bypass verification,
* bypass required approval,
* mutate committed artifacts outside Store,
* create hidden authoritative memory,
* materialize action with missing parameters,
* allow unscoped external side effects,
* allow unauthorized Core mutation.

Policy checks:

* approval requirement,
* safety level,
* permission scope,
* actor authority,
* policy drift,
* revocation state,
* executor allowlist,
* module capability limits.

---

## 12. Dependencies

Internal dependencies:

* Artifact System
* Schema Registry
* Store
* Event Log
* Reducer Registry
* Verification Engine
* Approval System
* Permission System
* Policy Resolver
* Action Materializer
* Execution Engine
* Module Registry
* Output Adapter Registry
* Observability System

External dependencies:

* none required for Core correctness

Optional dependencies:

* persistent database backend
* distributed queue
* metrics backend
* tracing backend
* external executors
* module sandbox runtime

The Core SHOULD depend on interfaces, not concrete implementations.

The Core MUST remain correct if optional observability or metrics dependencies are unavailable.

---

## 13. Test Requirements

The Core is valid only if the following tests pass.

Required tests:

* ingress schema validation test,
* invalid ingress rejection test,
* task derivation test,
* reducer determinism test,
* Store atomic commit test,
* concurrent commit conflict test,
* option schema test,
* option verification pass test,
* option verification fail blocks test,
* option verification unknown blocks test,
* approval required test,
* approval not required test,
* approval denied blocks test,
* approval expired blocks test,
* policy drift blocks test,
* deterministic materialization test,
* materializer does not call LLM test,
* action schema test,
* action verification fail blocks test,
* executor dispatch only after action verification test,
* idempotency test,
* execution result commit test,
* replay equivalence test,
* replay mismatch failure test,
* permission revocation test,
* runtime context is not authoritative test,
* output adapter does not create authority test,
* no hidden mutable state test,
* traceability chain test.

Each test SHOULD verify one clear behaviour.

Tests MUST NOT rely on hidden global state.

Replay tests MUST reconstruct final state from committed artifacts and events.

---

## 14. Implementation Notes

Implementation notes are non-normative and MAY change without changing the logical contract of the Core.

Recommended implementation approach:

* Keep Core orchestration small.
* Keep policy, permission, verification, storage, reduction, and execution behind interfaces.
* Use typed artifact references instead of raw IDs where possible.
* Treat Store commits as the hard boundary between runtime computation and authoritative state.
* Keep Reducers deterministic and side-effect free.
* Keep Action Materialization separate from Execution.
* Treat Output Adapters as formatting layers, not authority layers.
* Treat logs as operational traces, not as authoritative state.
* Prefer explicit failure states over implicit default behaviour.

Final rule:

**The Core implementation is correct only if every authoritative state transition can be validated, committed, replayed, explained, and blocked when required.**
