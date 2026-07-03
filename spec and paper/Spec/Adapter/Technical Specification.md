# Adapter Technical Specification

Status: Draft
Spec Level: Normative
Scope: Adapter Layer

---

## 1. Purpose

Adapters are boundary components between ARCS and external systems.

Their purpose is to translate external signals into ARCS-compatible ingress drafts and to translate final ARCS output into external representations.

Adapters MUST NOT own authoritative ARCS state.
Adapters MUST NOT decide what ARCS should do.
Adapters MUST NOT approve, verify, materialize, or execute ARCS decisions unless they are explicitly registered as Executor backends and are invoked by the Execution Engine with a verified action.

The core principle is:

**Adapters translate across boundaries. The Core governs the lifecycle. Executors perform verified actions.**

All authoritative state transitions MUST pass through the ARCS Core, Schema Gate, Store Commit, Reducers, Verification, Approval when required by policy, Action Materialization, Action Verification, Execution control, and replayable event logging.

Adapter-local data, logs, queues, buffers, cached responses, and external protocol messages are not authoritative ARCS state unless converted into schema-valid artifacts or events and committed through the Store.

---

## 2. Adapter Responsibilities

Adapters MAY perform boundary responsibilities.

Allowed responsibilities include:

1. receiving external input,
2. preserving raw input or raw input references,
3. extracting source metadata,
4. extracting actor or identity metadata if available,
5. normalizing external data into ARCS-compatible structures,
6. attaching channel, trust, context, correlation, and trace metadata,
7. creating `ingress_event` drafts,
8. submitting ingress drafts to the Core,
9. receiving final output instructions from the Core or Output Coordinator,
10. formatting output for external systems,
11. delivering output to external targets,
12. reporting delivery status back to the Core when required,
13. acting as Executor backends only when registered and dispatched by the Execution Engine,
14. converting external tool responses into `execution_result` drafts when acting as Executor backends,
15. emitting structured adapter failures or delivery reports when required by policy.

Adapters MUST NOT:

1. create authoritative state directly,
2. mutate committed artifacts,
3. bypass schema validation,
4. bypass Store commit,
5. bypass Verification,
6. bypass Approval when required by policy,
7. bypass Action Materialization,
8. bypass Action Verification,
9. execute raw external input,
10. execute model output directly,
11. execute unverified module output,
12. execute unverified actions,
13. approve actions,
14. verify their own output as safe,
15. silently reinterpret user intent,
16. invent missing facts,
17. silently discard state-relevant input,
18. treat logs as replay authority,
19. create hidden long-term memory,
20. decide final system behaviour.

Adapters transform representation, not authority.

---

## 3. Adapter Types

ARCS MAY use multiple adapter types depending on the external boundary.

Adapter types MUST remain separated by responsibility.

---

### 3.1 Input Adapter

An Input Adapter receives an external signal and converts it into an `ingress_event` draft.

Examples:

* user message adapter,
* API request adapter,
* file event adapter,
* scheduled task adapter,
* system signal adapter,
* webhook adapter,
* watcher adapter,
* sensor adapter.

Primary input:

```text
ExternalSignal
```

Primary output:

```text
IngressEventDraft
```

Rules:

* The Input Adapter MUST preserve the original input or a raw input reference when required by policy.
* The Input Adapter MAY normalize representation.
* The Input Adapter MUST NOT silently reinterpret meaning.
* The Input Adapter MUST NOT create a committed `ingress_event` directly unless the architecture explicitly grants it Store access through the Core commit interface.
* By default, the Input Adapter submits an ingress draft to the Core, and the Core coordinates validation and commit.

---

### 3.2 Output Adapter

An Output Adapter converts final ARCS output into an external representation.

Examples:

* chat response adapter,
* API response adapter,
* notification adapter,
* report delivery adapter,
* UI display adapter.

Primary input:

```text
FinalOutputPayload
```

Primary output:

```text
ExternalOutputDelivery
```

Rules:

* The Output Adapter MUST derive output only from committed artifacts, reducer-derived state, or explicit output instructions from the Core or Output Coordinator.
* The Output Adapter MUST NOT create authoritative state.
* The Output Adapter MUST NOT hide safety-relevant failure states unless policy explicitly permits redaction.
* If delivery status is state-relevant, the Adapter MUST report it back to the Core as a signal or delivery result.

Output delivery MUST NOT be confused with arbitrary external action execution.

If an operation changes an external system beyond normal response delivery, display, or status reporting, it SHOULD be modeled as an `action` and executed through the Execution Engine.

Examples of operations that SHOULD be modeled as actions:

* sending an email as a task result,
* modifying a file,
* calling a third-party API with side effects,
* creating a calendar event,
* deploying code,
* changing permissions,
* writing to an external database.

---

### 3.3 Executor Backend / Tool Driver

An Executor Backend connects the Execution Engine to an external tool, service, protocol, or system.

Examples:

* web search executor,
* database query executor,
* code execution executor,
* calendar executor,
* email executor,
* file system executor,
* shell executor if explicitly allowed by policy.

Primary input:

```text
ArtifactRef<action>
```

Primary raw output:

```text
ExternalToolResponse
```

Primary ARCS output:

```text
ExecutionResultDraft
```

Rules:

* An Executor Backend MAY execute external side effects only after the Execution Engine dispatches a verified `action`.
* The Executor Backend MUST NOT decide whether the action should execute.
* The Executor Backend MUST NOT accept raw user input as an executable command.
* The Executor Backend MUST enforce local scope checks and idempotency.
* The Executor Backend MUST convert raw external responses into `execution_result` drafts.
* The resulting `execution_result` becomes authoritative only after schema validation and Store commit through the Core lifecycle.

An Executor Backend is allowed to communicate with external systems, but it is not allowed to govern the ARCS lifecycle.

---

### 3.4 Store Backend

A Store Backend connects the ARCS Store to physical persistence infrastructure.

Examples:

* SQL backend,
* object store backend,
* local file backend,
* event log backend,
* vector index backend,
* graph persistence backend.

A Store Backend is infrastructure, not a normal boundary Adapter.

Rules:

* The Store contract defines authoritative commit semantics.
* The Store Backend MUST NOT decide which artifacts are authoritative.
* The Store Backend MUST NOT bypass Store versioning, event logging, commit boundaries, or replay rules.
* Physical persistence does not define logical authority. Logical authority is defined by committed artifacts and committed events.

---

### 3.5 Scheduler Adapter

A Scheduler Adapter receives time-based, recurring, delayed, or condition-triggered signals and converts them into ingress drafts.

Examples:

* delayed task trigger,
* recurring task trigger,
* system timer,
* periodic background check,
* watcher trigger.

Primary output:

```text
IngressEventDraft
```

Rules:

* The Scheduler Adapter MUST include scheduling metadata.
* Scheduling metadata SHOULD include trigger time, recurrence rule, scheduler identity, task identity if available, and correlation identifiers.
* The Scheduler Adapter MUST NOT execute the scheduled task directly.
* The scheduled trigger MUST enter the Core lifecycle as ingress.

---

## 4. Adapter Interfaces

Adapters SHOULD implement interfaces according to their type.

There is no single universal adapter interface that grants all capabilities to all adapters.

---

### 4.1 Input Adapter Interface

```text
InputAdapter
├── initialize(config)
├── receive_external_signal() -> ExternalSignal
├── normalize_signal(signal: ExternalSignal) -> IngressEventDraft
├── validate_local(draft: IngressEventDraft) -> LocalValidationResult
├── submit_ingress(draft: IngressEventDraft) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* `validate_local` MAY reject malformed external input before Core submission.
* Local validation MUST NOT replace Core schema validation.
* `submit_ingress` submits to the Core. It MUST NOT imply that the artifact is committed.

---

### 4.2 Output Adapter Interface

```text
OutputAdapter
├── initialize(config)
├── format_output(payload: FinalOutputPayload) -> ExternalPayload
├── deliver_output(payload: ExternalPayload) -> DeliveryReport
├── report_delivery(report: DeliveryReport) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* `format_output` MUST NOT create authoritative state.
* `deliver_output` MAY produce external delivery side effects.
* If delivery status is relevant, `report_delivery` MUST submit it back to the Core.

---

### 4.3 Executor Backend Interface

```text
ExecutorBackend
├── initialize(config)
├── can_execute(action_ref: ArtifactRef<action>) -> CapabilityCheckResult
├── execute_verified_action(action_ref: ArtifactRef<action>) -> ExternalToolResponse
├── convert_response(response: ExternalToolResponse) -> ExecutionResultDraft
├── submit_execution_result(result: ExecutionResultDraft) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* `execute_verified_action` MUST only be called by the Execution Engine or Execution Dispatcher.
* The backend MUST reject actions that are outside its declared capability or permission scope.
* The backend MUST NOT execute actions that do not carry a valid action reference and dispatch context.
* Raw external responses are not authoritative. Only committed `execution_result` artifacts are authoritative.

---

### 4.4 Store Backend Interface

```text
StoreBackend
├── initialize(config)
├── begin_transaction() -> StoreTransaction
├── write_artifact_version(...)
├── write_event(...)
├── advance_store_head(...)
├── commit_transaction(...)
├── rollback_transaction(...)
└── shutdown()
```

Rules:

* Store Backend interfaces are governed by the Store specification, not the Adapter lifecycle.
* Store Backend MUST NOT expose hidden write paths that bypass Store commit rules.

---

## 5. Adapter Lifecycle

---

### 5.1 Initialization

During initialization, the Adapter loads its configuration and registers its declared capabilities.

Required initialization data:

```text
adapter_id
adapter_type
adapter_version
capabilities
permission_scope
input_schema_refs
output_schema_refs
failure_policy
configuration_version
```

Rules:

* The Adapter MUST fail closed if required configuration is missing.
* The Adapter MUST NOT enable capabilities not declared in its configuration.
* Adapter configuration MUST be versioned.

---

### 5.2 Receive Phase

The Adapter receives raw external input or external protocol data.

Examples:

```text
HTTP request
user message
file change event
tool callback
scheduled trigger
system signal
webhook payload
external tool response
```

Rules:

* Raw input MUST be preserved or referenced when required for traceability, audit, security, or replay analysis.
* The Adapter MUST NOT treat raw input as executable authority.

---

### 5.3 Normalization Phase

The Adapter converts external representation into an internal draft format.

A normalized ingress draft SHOULD include:

```text
source
channel
timestamp
raw_reference
actor_identity
authentication_context
permission_context
normalized_content
metadata
correlation_id
adapter_id
adapter_version
```

Rules:

* The Adapter MAY clean formatting, decode transport-specific structures, or map external fields into internal fields.
* The Adapter MUST NOT silently reinterpret intent.
* The Adapter MUST NOT invent missing facts.
* Missing information MUST remain explicit or be omitted according to schema and policy.

---

### 5.4 Local Validation Phase

The Adapter MAY perform local validation before Core submission.

Local validation MAY check:

```text
required fields
data types
size limits
encoding format
source identity
malformed input
unsupported content
rate limits
basic permission metadata
```

Rules:

* Invalid external input MAY be rejected before Core submission.
* Adapter-level validation MUST NOT replace Core schema validation, policy validation, permission checks, Verification, Approval, or Execution control.
* If local rejection is relevant to audit, rate limiting, security, or incident analysis, the rejection SHOULD be reported through a traceable record according to policy.

---

### 5.5 Ingress Submission Phase

After successful normalization and local validation, the Input Adapter submits an `ingress_event` draft to the Core.

Example:

```text
External Signal
    ↓
Input Adapter
    ↓
IngressEventDraft
    ↓
Core Submission
    ↓
Schema Gate
    ↓
Store Commit
    ↓
Committed ingress_event
```

Rules:

* The Adapter submits a draft.
* The Core validates and commits.
* The Adapter MUST NOT directly create tasks, options, approvals, actions, execution results, or memory entries.

---

### 5.6 Execution Backend Phase

This phase applies only to registered Executor Backends.

When the Execution Engine dispatches a verified `action`, the Executor Backend MAY translate the typed action into an external protocol call.

Examples:

```text
send message
call API
write file
execute tool
create notification
update external system
```

Rules:

* The backend MUST verify that the action target matches its declared capability.
* The backend MUST enforce local scope checks.
* The backend MUST enforce idempotency.
* The backend MUST NOT treat approval alone as sufficient for execution.
* The backend MUST NOT execute actions that failed action verification.
* The backend MUST NOT execute unsupported action types.

---

### 5.7 Result Reporting Phase

After actual execution, the Executor Backend converts the external response into an `execution_result` draft and submits it to the Core.

The result draft SHOULD include:

```text
action_id
adapter_id
executor_id
status
output_reference
error_reference
started_at
finished_at
external_reference
idempotency_key
correlation_id
trace_id
side_effects
```

Rules:

* The Executor Backend submits an `execution_result` draft.
* The Core validates and commits the `execution_result`.
* The result is not authoritative until committed.
* A blocked action MUST NOT create an `execution_result` unless execution had already started and was then cancelled or aborted.

---

### 5.8 Output Delivery Phase

The Output Adapter receives final output payload from the Core or Output Coordinator and delivers it to an external target.

Example:

```text
Final Core state or output instruction
    ↓
Output Adapter
    ↓
External Payload
    ↓
External Delivery
    ↓
DeliveryReport, if required
    ↓
Core Submission, if state-relevant
```

Rules:

* Output delivery MUST be traceable.
* Delivery status MUST be reported if it affects user-visible state, auditability, retry behaviour, or follow-up behaviour.
* Output Adapter delivery MUST NOT be used to bypass the Execution Engine for state-changing operations.

---

### 5.9 Shutdown Phase

During shutdown, an Adapter MUST close external connections, flush operational logs, and release resources.

Rules:

* The Adapter MUST NOT lose unreported execution results when recoverable recovery data exists.
* If graceful shutdown is not possible, the Adapter SHOULD emit or submit a structured failure or recovery report on restart if policy requires it.
* Shutdown MUST NOT create hidden authoritative state.

---

## 6. Adapter Event Structure

Adapter-originated events are not authoritative until accepted, schema-validated, and committed by the Core or Store.

A boundary event draft SHOULD follow a stable structure:

```json
{
  "event_id": "string",
  "event_type": "string",
  "adapter_id": "string",
  "adapter_type": "string",
  "adapter_version": "string",
  "source": "string",
  "channel": "string",
  "timestamp": "datetime",
  "correlation_id": "string",
  "trace_id": "string",
  "actor": {
    "id": "string",
    "type": "string",
    "auth_context": {},
    "permission_context": {}
  },
  "payload_schema_version": "string",
  "payload": {},
  "metadata": {}
}
```

Rules:

* The payload schema depends on adapter type.
* Adapter event drafts MUST NOT be treated as committed events.
* Event authority is created only by Store commit.

---

## 7. Action Dispatch Structure

Executor Backends receive dispatched verified actions, not merely approved actions.

A dispatched action context SHOULD contain:

```json
{
  "action_ref": {
    "artifact_id": "string",
    "artifact_type": "action",
    "version": "string",
    "schema_version": "string",
    "store_head": "string"
  },
  "dispatch_id": "string",
  "target_executor": "string",
  "action_type": "string",
  "verification_report_ref": "string",
  "approval_ref": "string|null",
  "policy_ref": "string",
  "permission_refs": [],
  "idempotency_key": "string",
  "constraints": {},
  "correlation_id": "string",
  "trace_id": "string"
}
```

An Executor Backend MUST reject dispatch if:

```text
target_executor does not match
action_ref is missing
action type is unsupported
action verification is missing or invalid
permission scope is insufficient
policy reference is missing
required approval is missing or expired
idempotency key is missing for side-effecting action
constraints cannot be satisfied
```

Rules:

* Approval does not replace action verification.
* Dispatch context does not replace committed artifacts.
* The Executor Backend MUST NOT invent missing dispatch data.

---

## 8. Execution Result Structure

An Executor Backend returns an `execution_result` draft.

```json
{
  "action_id": "string",
  "execution_id": "string",
  "executor_id": "string",
  "adapter_id": "string",
  "status": "success|failure|timeout|cancelled|aborted|partial_success",
  "output_reference": "string|null",
  "error_reference": "string|null",
  "side_effects": [],
  "started_at": "datetime",
  "finished_at": "datetime",
  "external_reference": "string|null",
  "idempotency_key": "string",
  "correlation_id": "string",
  "trace_id": "string"
}
```

Rules:

* The `execution_result` draft MUST reference the executed action.
* It MUST include executor identity.
* It MUST describe side effects when side effects occurred.
* It MUST NOT claim success if side effects are unknown.
* It becomes authoritative only after Core validation and Store commit.

---

## 9. Error Handling

Adapters MUST emit or submit structured errors instead of throwing untracked failures.

Adapter errors SHOULD include:

```text
error_id
adapter_id
adapter_type
error_type
severity
message
recoverable
raw_reference
affected_stage
correlation_id
trace_id
timestamp
```

Common error types:

```text
validation_error
permission_error
unsupported_input
external_service_error
timeout_error
rate_limit_error
serialization_error
execution_error
configuration_error
delivery_error
duplicate_signal_error
```

Rules:

* Adapters MUST NOT silently ignore failed input.
* Adapters MUST NOT silently ignore failed delivery.
* Executor Backends MUST NOT silently ignore failed actions.
* Errors affecting authoritative state, auditability, security, or user-visible behaviour MUST be reported back to the Core.

---

## 10. Retry Behaviour

Retries MUST be explicit and controlled.

An Adapter or Executor Backend MAY retry external operations only if:

1. the operation is safe to retry,
2. an idempotency key exists,
3. the retry policy allows retry,
4. retry metadata is recorded.

Retry metadata SHOULD include:

```text
attempt_number
max_attempts
retry_reason
backoff_strategy
idempotency_key
first_attempt_at
last_attempt_at
```

Rules:

* Non-idempotent actions MUST NOT be retried automatically unless the Core explicitly allows it through policy and dispatch context.
* Infinite retry loops are forbidden.
* Retry attempts MUST be traceable.

---

## 11. Idempotency

Executor Backends SHOULD support idempotency for external side effects.

An idempotency key SHOULD be derived from:

```text
action_id
target_executor
external_target
correlation_id
```

Rules:

* Repeated execution with the same idempotency key MUST NOT create duplicate side effects when the backend can prevent it.
* If idempotency cannot be guaranteed, the backend MUST report this limitation.
* The Execution Engine MUST treat non-idempotent operations as higher risk.

Examples:

```text
Do not send the same email twice.
Do not write the same output file twice.
Do not execute the same external transaction twice.
Do not create the same calendar event twice.
```

---

## 12. Permissions

Every Adapter and Executor Backend MUST declare its permission scope.

Example permission scopes:

```text
receive_user_input
receive_webhook
read_external_state
write_output
deliver_notification
execute_tool
read_storage
write_storage
send_external_message
modify_external_state
read_file
write_file
network_access
```

Rules:

* The Core MUST check whether a requested operation is allowed for the target Adapter or Executor Backend.
* Executor Backends MUST perform local scope checks before execution.
* Permission failure MUST produce a structured `permission_error`.
* Local permission checks do not replace Core permission checks.

---

## 13. Logging and Traceability

Adapters MUST produce traceable operational logs for relevant boundary interactions.

Logs are not the source of truth. Replayability MUST be based on committed artifacts and committed events, not logs.

At minimum, operational logs SHOULD include:

```text
received input
normalization result
local validation result
submitted ingress draft
core submission result
received output instruction
delivery result
received dispatched action
local permission check
execution attempt
execution response
submitted execution_result draft
errors
retry attempts
```

Logs SHOULD include:

```text
trace_id
correlation_id
adapter_id
adapter_version
external_reference
affected_stage
```

ARCS SHOULD be able to answer:

```text
What entered the system?
Where did it come from?
Which Adapter handled it?
Which ingress draft was submitted?
Was the ingress accepted or rejected?
Which action was dispatched?
Which Executor Backend handled it?
What external side effect happened?
Which execution_result was submitted?
Was the result committed?
```

---

## 14. Security Requirements

Adapters are trust boundaries.

Every Adapter MUST assume that external input may be malformed, incomplete, adversarial, duplicated, delayed, or unsafe.

Security requirements:

```text
validate external input locally where possible
enforce size limits
enforce declared permission scopes
sanitize transport-specific data
preserve raw input references when required
never execute raw input
never execute unverified actions
never expose internal secrets
never create hidden authoritative state
never bypass the Core lifecycle
never treat external trust as internal authority
```

External credentials MUST NOT be hardcoded inside Adapter logic. They MUST be provided through the configured secret management mechanism.

Adapters MUST NOT leak internal artifact contents, policy data, secrets, or system metadata to external targets unless policy allows it.

---

## 15. Adapter Configuration

Each Adapter SHOULD be configured through a declarative configuration object.

Example:

```yaml
adapter_id: chat_input_adapter
adapter_type: input
enabled: true
version: 0.1.0

schemas:
  input: schemas/chat_input.schema.json
  ingress: schemas/ingress_event.schema.json

permissions:
  - receive_user_input

limits:
  max_payload_size: 100000
  timeout_ms: 5000

logging:
  trace_enabled: true
  raw_input_reference: true

failure_policy:
  on_validation_error: reject
  on_timeout: report_failure
  on_permission_error: reject
```

Configuration MUST be versioned.

Rules:

* Missing required configuration MUST fail closed.
* Disabled Adapters MUST NOT receive or submit input.
* Permission scope MUST be explicit.
* Adapter capabilities MUST be declared before use.

---

## 16. Adapter Invariants

The following invariants MUST always hold:

1. An Adapter MUST NOT create authoritative state directly.
2. An Input Adapter MUST submit ingress drafts, not committed authority.
3. An Output Adapter MUST NOT create authority through formatting or delivery.
4. An Executor Backend MUST NOT execute without a verified dispatched action.
5. Approval alone MUST NOT be treated as sufficient for execution.
6. An Adapter MUST NOT bypass schema validation.
7. An Adapter MUST NOT bypass the Core lifecycle.
8. An Adapter MUST NOT silently discard relevant input.
9. An Adapter MUST NOT mutate committed artifacts.
10. Every Adapter-submitted signal MUST be traceable.
11. Every side-effecting execution MUST produce an execution result draft or structured failure.
12. Every external side effect MUST be traceable to a verified action.
13. Every Adapter MUST declare its permissions.
14. Every Adapter MUST fail closed when required validation fails.
15. Logs MUST NOT replace committed artifacts or events.

---

## 17. Minimal Input Adapter Example

```text
User message received
    ↓
ChatInputAdapter.receive_external_signal()
    ↓
ChatInputAdapter.normalize_signal()
    ↓
IngressEventDraft created
    ↓
ChatInputAdapter.submit_ingress()
    ↓
Core Schema Gate validates draft
    ↓
Core commits ingress_event and event
    ↓
Core processing begins
```

This flow ensures that the Adapter only translates boundary input while the Core remains responsible for validation, commitment, task derivation, verification, approval, action control, and state authority.

---

## 18. Minimal Output Adapter Example

```text
Core derives final output state
    ↓
OutputCoordinator creates output payload
    ↓
ChatOutputAdapter.format_output()
    ↓
ChatOutputAdapter.deliver_output()
    ↓
DeliveryReport created if required
    ↓
DeliveryReport submitted to Core if state-relevant
```

This flow ensures that output delivery does not become hidden authority.

---

## 19. Minimal Executor Backend Example

```text
Core verifies option
    ↓
Approval satisfied if required
    ↓
ActionMaterializer creates action
    ↓
ActionVerifier verifies action
    ↓
ExecutionDispatcher dispatches verified action
    ↓
EmailExecutorBackend.execute_verified_action()
    ↓
External email API call
    ↓
EmailExecutorBackend.convert_response()
    ↓
ExecutionResultDraft submitted to Core
    ↓
Core validates and commits execution_result
```

This flow ensures that external side effects occur only through verified actions and replayable execution results.

---

## 20. Final Rule

Adapters are not decision-making authorities.

They are controlled boundary translators.

Executor Backends may perform external side effects only when dispatched by the Execution Engine with a verified action.

Store Backends provide persistence infrastructure but do not define logical authority.

Final rule:

**Adapters translate external reality into ARCS-compatible representations. The Core governs lifecycle authority. Executors perform verified actions. The Store defines committed truth.**
