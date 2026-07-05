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

ARCS uses a small number of broad adapter classes.

The goal is not to create a separate top-level adapter type for every external concern.
The goal is to group extensions by responsibility while keeping the model simple.

The canonical adapter classes are:

1. `input`
2. `external_state`
3. `interpretation`
4. `output`
5. `database`
6. `reasoning`
7. `llm`

Approval replies, permission replies, admin commands, webhook callbacks, and similar externally-originated decisions are treated as `input` unless they are part of a continuously refreshed world-state feed.

---

### 3.1 Input Adapter

An Input Adapter receives externally initiated signals and converts them into an `ingress_event` draft.

Examples:

* user message adapter,
* API request adapter,
* CLI adapter,
* webhook adapter,
* approval response adapter,
* permission response adapter,
* scheduler trigger adapter.

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
* Approval or permission information arriving from outside ARCS is still `input`; it does not gain authority merely by entering through an adapter.
* If semantic translation is required, the Input Adapter SHOULD hand off to a separate Interpretation Adapter instead of silently performing interpretation itself.

---

### 3.2 External State Adapter

An External State Adapter receives or polls high-frequency world-state updates and converts them into traceable ARCS-compatible state observations.

Examples:

* device status adapter,
* software health adapter,
* sensor adapter,
* watcher adapter,
* telemetry adapter,
* periodic environment snapshot adapter.

Primary input:

```text
ExternalStateSignal | PolledWorldState
```

Primary output:

```text
IngressEventDraft | StateObservationDraft
```

Rules:

* The External State Adapter exists because world-state updates may be frequent, continuous, and latency-sensitive.
* It MUST preserve source identity, timestamp, and observation context.
* It MUST NOT silently collapse distinct state transitions that matter for audit or control.
* It MUST NOT treat observed state as permission or approval authority by itself.
* It MUST NOT execute state-changing operations merely because it observes state changes.

---

### 3.3 Interpretation Adapter

An Interpretation Adapter converts raw or weakly structured input into a structured interpretation proposal.

Examples:

* text-to-JSON bridge adapter,
* speech-to-structured-intent adapter,
* domain translation adapter,
* schema-constrained interpretation service.

Primary input:

```text
RawInput | NormalizedInputSignal
```

Primary output:

```text
InterpretationProposalDraft
```

Rules:

* The Interpretation Adapter performs semantic translation, not authority decisions.
* It MAY call LLMs, parsers, translators, or other proposal-generating systems.
* It MUST preserve enough linkage to the original input for audit and replay analysis.
* It MUST NOT create authoritative `task`, `approval`, `policy`, `permission_grant`, `action`, or `execution_result` artifacts directly.
* Its output remains a proposal until validated and committed through the Core lifecycle.

---

### 3.4 Output Adapter

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

---

### 3.5 Database Adapter

A Database Adapter connects ARCS to structured persistence or query systems that are not the ARCS Store itself.

Examples:

* PostgreSQL adapter,
* MySQL adapter,
* SQLite application database adapter,
* document database adapter,
* cache/query adapter when used as structured data access.

Primary input:

```text
VerifiedAction | QueryRequest | ReadRequest
```

Primary output:

```text
QueryResultDraft | ExecutionResultDraft | RetrievedStateDraft
```

Rules:

* The Database Adapter MUST NOT be confused with the ARCS Store backend.
* The ARCS Store defines committed truth for ARCS itself; a Database Adapter connects to some other data system.
* Read-only database access MAY be used to retrieve context or state.
* State-changing database operations MUST be modeled as verified actions when they have external side effects.
* The adapter MUST NOT bypass ARCS verification, approval, or execution rules.

---

### 3.6 Reasoning Adapter

A Reasoning Adapter connects ARCS to software that proposes plans, options, analyses, or structured reasoning outputs from already structured inputs.

Examples:

* planner service,
* rule engine,
* symbolic reasoning module,
* retrieval-assisted proposal generator,
* domain-specific recommendation engine.

Primary input:

```text
ReasoningRequest
```

Primary output:

```text
InterpretationProposalDraft | OptionProposalDraft | ReasoningReportDraft
```

Rules:

* A Reasoning Adapter may propose, analyze, rank, classify, or explain.
* It MUST NOT approve actions.
* It MUST NOT issue permission grants.
* It MUST NOT create authoritative `action`, `approval`, `policy`, or `execution_result` artifacts directly.
* Its outputs remain proposals until the Core validates, verifies, and commits the resulting artifacts.
* Interpretation and Reasoning MAY be combined in one deployed service, but they are distinct responsibilities in the architecture.

---

### 3.7 LLM Adapter

An LLM Adapter is a specialized adapter for calling a language model or model-serving API.

Examples:

* Ollama adapter,
* OpenAI-compatible adapter,
* local inference adapter,
* structured-output model adapter.

Primary input:

```text
ModelRequest
```

Primary output:

```text
ModelResponseDraft
```

Rules:

* The LLM Adapter is a transport and formatting boundary around model inference.
* It MUST NOT be treated as an authority layer.
* It MUST NOT approve actions, grant permissions, or mutate committed state.
* If model output is used for planning or interpretation, it MUST flow back into ARCS as proposals subject to normal controls.

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

### 4.2 External State Adapter Interface

```text
ExternalStateAdapter
├── initialize(config)
├── poll_or_receive_state() -> ExternalStateSignal
├── normalize_state(signal: ExternalStateSignal) -> StateObservationDraft
├── validate_local(draft: StateObservationDraft) -> LocalValidationResult
├── submit_state(draft: StateObservationDraft) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* The adapter MAY be push-based, poll-based, or hybrid.
* It MUST preserve observation timestamps and source identity.
* It MUST NOT treat frequent updates as permission to mutate external systems.
* It MUST NOT replace the normal verification path for downstream decisions.

---

### 4.3 Interpretation Adapter Interface

```text
InterpretationAdapter
├── initialize(config)
├── receive_interpretation_request(input) -> InterpretationRequest
├── translate_to_proposal(request: InterpretationRequest) -> InterpretationProposalDraft
├── validate_local(draft: InterpretationProposalDraft) -> LocalValidationResult
├── submit_proposal(draft: InterpretationProposalDraft) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* The Interpretation Adapter transforms raw or weakly structured input into a structured proposal.
* It MUST preserve traceability back to the original input.
* It MUST NOT treat interpreted output as authority.

---

### 4.4 Output Adapter Interface

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

### 4.5 Database Adapter Interface

```text
DatabaseAdapter
├── initialize(config)
├── can_read(request) -> CapabilityCheckResult
├── read(request) -> RetrievedStateDraft
├── can_write(action_ref: ArtifactRef<action>) -> CapabilityCheckResult
├── execute_verified_write(action_ref: ArtifactRef<action>) -> ExternalToolResponse
├── convert_response(response) -> ExecutionResultDraft | QueryResultDraft
├── submit_result(result) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* Read access MAY be used for context retrieval.
* Write access MUST follow the verified action path.
* The adapter MUST distinguish clearly between read-only queries and side-effecting writes.

---

### 4.6 Reasoning Adapter Interface

```text
ReasoningAdapter
├── initialize(config)
├── receive_reasoning_request(request) -> ReasoningRequest
├── generate_proposal(request) -> InterpretationProposalDraft | OptionProposalDraft | ReasoningReportDraft
├── validate_local(draft) -> LocalValidationResult
├── submit_proposal(draft) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* A Reasoning Adapter generates proposals, not authority.
* It MUST expose uncertainty and assumptions when available.
* It MUST NOT bypass Core verification.

---

### 4.7 LLM Adapter Interface

```text
LlmAdapter
├── initialize(config)
├── build_model_request(prompt, schema, context) -> ModelRequest
├── call_model(request: ModelRequest) -> ModelResponseDraft
├── validate_local(response: ModelResponseDraft) -> LocalValidationResult
├── submit_response(response: ModelResponseDraft) -> CoreSubmissionResult
└── shutdown()
```

Rules:

* The LLM Adapter manages model transport and response conversion.
* It MUST NOT directly create authoritative ARCS state.
* If an LLM-backed capability is intended to reason, the resulting output still enters ARCS as proposal material.

---

### 4.8 Executor Backend Interface

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

### 4.9 Store Backend Interface

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

### 4.10 Adapter Management Layer

Adapter interfaces alone are not sufficient for a real system.
ARCS SHOULD provide a dedicated Adapter Management Layer that is responsible for registration, lifecycle control, lookup, health tracking, and capability-aware routing.

The Adapter Management Layer is not itself an authority source.
It does not approve actions, grant permissions, or validate policy correctness.
Its role is operational coordination of Adapter instances.

#### Purpose

The purpose of the Adapter Management Layer is to make Adapter usage explicit, inspectable, and controllable at runtime.

It answers questions such as:

* which adapters are available,
* which adapter class each one belongs to,
* whether an adapter is enabled,
* whether an adapter initialized successfully,
* which adapter should handle a given request,
* whether an adapter is healthy enough to be used.

#### Core Responsibilities

The Adapter Management Layer SHOULD support at least:

1. Adapter registration,
2. Adapter descriptor storage,
3. lifecycle management,
4. typed lookup by adapter class,
5. lookup by adapter id,
6. capability-aware routing,
7. enabled/disabled state,
8. health state tracking,
9. last-error tracking,
10. configuration binding.

#### Recommended Components

ARCS SHOULD model the management layer using components such as:

* `AdapterDescriptor`
* `AdapterRuntimeState`
* `AdapterManager`
* typed registries or typed lookup views
* `AdapterResolver`

#### AdapterDescriptor

An `AdapterDescriptor` SHOULD describe the static identity and declared capabilities of an adapter.

Recommended fields:

```text
adapter_id
adapter_class
adapter_version
display_name
capabilities
supported_kinds
permission_scope
default_enabled
configuration_version
```

Rules:

* The descriptor MUST NOT claim capabilities the adapter does not actually implement.
* The descriptor SHOULD be inspectable without executing the adapter's external side effects.

#### AdapterRuntimeState

An `AdapterRuntimeState` SHOULD capture the operational state of an adapter instance.

Recommended fields:

```text
initialized
enabled
healthy
last_error
last_started_at
last_stopped_at
last_health_check_at
last_success_at
```

Rules:

* Runtime state is operational metadata, not authoritative ARCS business state.
* Runtime state MUST NOT replace committed artifacts or committed events.

#### AdapterManager

The `AdapterManager` SHOULD be the central runtime registry for adapter instances.

Responsibilities:

* register adapter instances,
* reject duplicate adapter ids unless replacement is explicitly allowed,
* initialize adapters with bound configuration,
* stop adapters on shutdown,
* expose descriptor and runtime state for inspection,
* provide typed and untyped lookup methods.

Minimal conceptual interface:

```text
AdapterManager
├── register_adapter(adapter)
├── initialize_adapter(adapter_id, config)
├── initialize_all()
├── shutdown_adapter(adapter_id)
├── shutdown_all()
├── get_descriptor(adapter_id)
├── get_runtime_state(adapter_id)
├── list_adapters(adapter_class?)
├── find_adapter(adapter_id)
└── resolve_adapter(request_context)
```

Rules:

* The AdapterManager MUST fail closed when required adapter configuration is missing.
* The AdapterManager MUST NOT route work to disabled adapters.
* The AdapterManager MUST NOT route work to adapters known to be unhealthy when policy forbids degraded execution.

#### Typed Registries

ARCS MAY implement either:

* one central `AdapterManager` with typed lookup views, or
* separate typed registries such as:
  * `InputAdapterRegistry`
  * `ExternalStateAdapterRegistry`
  * `InterpretationAdapterRegistry`
  * `OutputAdapterRegistry`
  * `DatabaseAdapterRegistry`
  * `ReasoningAdapterRegistry`
  * `LlmAdapterRegistry`

The exact implementation is flexible.
What matters is that adapter class, identity, capabilities, and runtime state remain explicit.

#### AdapterResolver

An `AdapterResolver` SHOULD choose the correct adapter for a concrete runtime need.

Examples:

* which Interpretation Adapter handles this language or format,
* which Output Adapter delivers to this destination,
* which Database Adapter owns this external data source,
* which LLM Adapter serves this model class,
* which External State Adapter owns this polling target.

Selection MAY depend on:

* adapter class,
* adapter id,
* declared capabilities,
* target system,
* schema support,
* destination metadata,
* policy restrictions,
* health state.

Rules:

* Resolution MUST be deterministic for the same configuration and request context unless policy explicitly allows fallback behaviour.
* Fallback behaviour MUST be explicit and traceable.

#### Health and Failure Handling

The management layer SHOULD support adapter health inspection.

Health state MAY include:

```text
healthy
degraded
unavailable
disabled
misconfigured
```

Rules:

* Health state MUST NOT silently bypass Core controls.
* Health state SHOULD be visible to operational tooling.
* Health failures SHOULD be traceable through logs or explicit runtime diagnostics.

#### Scope of This Layer

The Adapter Management Layer manages Adapter instances.
It does not replace the Core lifecycle.

It MUST NOT:

* validate business correctness,
* approve actions,
* grant permissions,
* commit artifacts by itself,
* redefine which state is authoritative.

Final rule:

**Adapter interfaces define what adapters can do. The Adapter Management Layer defines which adapters exist, whether they are usable, and which one is selected. The Core remains the authority over lifecycle and committed truth.**

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
