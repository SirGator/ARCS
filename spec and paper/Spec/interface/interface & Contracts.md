## 6. Interfaces and Contracts

Interfaces define how ARCS components communicate with each other.

An interface does not describe how a component is implemented internally. It describes what other components are allowed to send to it, what they can expect to receive from it, which side effects are allowed, and which responsibilities are explicitly forbidden.

In ARCS, interfaces are treated as contracts. A component may change its internal implementation as long as it continues to satisfy its interface contract.

The purpose of this section is to make the system boundaries explicit. Every major component must have a clearly defined interface so that responsibility, data ownership, artifact flow, and side effects remain traceable.

---

### 6.1 Interface Rules

Every ARCS interface must define:

* **Purpose** — why the interface exists.
* **Input** — which artifacts or data structures the component may receive.
* **Output** — which artifacts or data structures the component may produce.
* **Allowed Side Effects** — what the component is allowed to change outside itself.
* **Forbidden Side Effects** — what the component must never do.
* **Errors** — which failure cases can occur.
* **Invariants** — rules that must always remain true.
* **Example Flow** — a simple example of how the interface is used.

A component must not depend on hidden state from another component. If information is required, it must be passed explicitly through an artifact, event, or approved interface.

A component must not silently create authoritative system state unless its interface explicitly allows it.

---

### 6.2 General Interface Template

Each interface should follow this structure:

```text
Interface Name

Purpose:
- What is this interface responsible for?

Input:
- Which artifacts, events, or data structures can it receive?

Output:
- Which artifacts, events, or data structures can it produce?

Allowed Side Effects:
- What is this component allowed to modify, trigger, persist, or emit?

Forbidden Side Effects:
- What must this component never do?

Errors:
- What can fail?
- How are failures represented?

Invariants:
- Which rules must always remain true?

Example:
- A short example of a valid interaction.
```

---

### 6.3 Input Adapter Interface

The Input Adapter Interface defines how external input enters ARCS.

Input adapters are boundary components. They translate external signals into internal ARCS ingress artifacts.

They do not decide whether a request is valid, safe, useful, or executable. These decisions belong to the ARCS core.

#### Purpose

The purpose of an input adapter is to receive an external signal and convert it into a normalized internal artifact.

External signals may come from systems such as:

* user messages
* API requests
* Discord messages
* file events
* scheduled events
* webhook events
* CLI input
* approval responses
* permission responses

Input in ARCS means externally initiated, discrete signals that actively enter the system.
This includes human requests, webhooks, callbacks, approvals, and permission replies.

High-frequency world updates are treated separately through the External State Adapter Interface.

If semantic translation is required, raw or weakly structured input MAY later be passed into a separate Interpretation Adapter.

#### Input

An input adapter may receive:

* raw external input
* source metadata
* timestamp metadata
* authentication context, if available
* channel-specific metadata

#### Output

An input adapter produces:

* `IngressArtifact`

The `IngressArtifact` must preserve:

* original source
* original timestamp
* original content
* adapter identity
* normalized representation
* attached metadata

#### Allowed Side Effects

An input adapter is allowed to:

* receive external input
* normalize input format
* attach metadata
* create an ingress artifact
* forward the ingress artifact into the ARCS core

#### Forbidden Side Effects

An input adapter must not:

* decide whether the request is valid
* decide whether the request is safe
* decide whether the request is useful
* execute tools
* modify memory directly
* create final responses
* choose plans
* approve actions
* reject requests based on its own reasoning
* create authoritative system state outside the approved artifact flow

#### Invariants

* The adapter may transform representation, but not meaning.
* The adapter must preserve the original input.
* The adapter must not hide or rewrite relevant metadata.
* Every external input must enter ARCS as an explicit artifact.

#### Example

```text
External Discord Message
        ↓
Discord Input Adapter
        ↓
IngressArtifact
        ↓
Core Validation Flow
```

---

### 6.4 Output Adapter Interface

The Output Adapter Interface defines how approved ARCS output leaves the system.

Output adapters are boundary components. They translate approved internal output artifacts into external formats.

They do not create new decisions. They do not invent system state. They only deliver, display, serialize, or forward output that has already been approved by the ARCS core.

#### Purpose

The purpose of an output adapter is to convert an approved internal output artifact into the format required by an external system.

#### Input

An output adapter may receive:

* `ApprovedOutputArtifact`
* destination metadata
* formatting metadata
* delivery metadata

#### Output

An output adapter may produce:

* external message
* API response
* file output
* UI display
* webhook response
* serialized payload

#### Allowed Side Effects

An output adapter is allowed to:

* format output
* serialize output
* send output
* display output
* forward output to an external system
* attach delivery metadata

#### Forbidden Side Effects

An output adapter must not:

* create new decisions
* modify the meaning of the approved output
* execute unapproved actions
* alter the final answer based on external context
* modify memory directly
* create new authoritative system state
* bypass the verifier or policy layer

#### Invariants

* Only approved output artifacts may leave the system.
* The adapter may transform representation, but not meaning.
* The adapter must preserve the intent of the approved output.
* Delivery metadata must remain traceable.

#### Example

```text
ApprovedOutputArtifact
        ↓
Discord Output Adapter
        ↓
Discord Message
```

---

### 6.5 External State Adapter Interface

The External State Adapter Interface defines how ARCS receives or polls continuous updates about the outside world.

This interface exists because world state is not the same thing as active user or system requests.
External state updates may arrive every second or faster, may be polled continuously, and often represent observations rather than requests.

#### Purpose

The purpose of an external state adapter is to collect, normalize, and forward high-frequency world-state information without turning it into hidden authority.

#### Input

An external state adapter may receive:

* device status
* software status
* health checks
* telemetry streams
* polled sensor values
* external environment snapshots

#### Output

An external state adapter may produce:

* `StateObservationArtifact`
* `IngressArtifact`
* `EnvironmentSnapshotArtifact`

#### Allowed Side Effects

An external state adapter is allowed to:

* poll external systems
* receive high-frequency updates
* normalize observation payloads
* attach timestamps and source metadata
* submit observations into the ARCS flow

#### Forbidden Side Effects

An external state adapter must not:

* invent missing measurements
* silently discard relevant state transitions
* treat world observations as approval or permission authority
* execute external actions merely because a state changed

#### Invariants

* Observations must remain traceable to source and time.
* Frequent updates must not become hidden mutable state outside artifacts.
* External state is observation, not authority.

#### Example

```text
Device Health Poll
        ↓
External State Adapter
        ↓
StateObservationArtifact
        ↓
Core Validation Flow
```

---

### 6.6 Interpretation Adapter Interface

The Interpretation Adapter Interface defines how ARCS translates raw or weakly structured input into structured proposal material.

Interpretation is not the same thing as input transport.
Input gets data into ARCS.
Interpretation converts ambiguous text, speech, or mixed user signals into a proposal that the Core can validate and process.

#### Purpose

The purpose of an interpretation adapter is to convert external expressions into traceable structured interpretation proposals.

#### Input

An interpretation adapter may receive:

* raw text
* normalized user messages
* speech-to-text output
* mixed structured/unstructured request payloads
* language-specific external input

#### Output

An interpretation adapter may produce:

* `InterpretationProposalArtifact`
* `StructuredIntentDraft`
* `InterpretationErrorArtifact`

#### Allowed Side Effects

An interpretation adapter is allowed to:

* translate text into structured proposals
* call parsers, translators, or LLM-backed services
* attach interpretation metadata
* expose uncertainty or missing fields

#### Forbidden Side Effects

An interpretation adapter must not:

* approve actions
* create authoritative state directly
* invent permissions
* treat interpreted meaning as verified truth

#### Invariants

* Interpretation output is only a proposal.
* Original input must remain traceable.
* Interpretation must not bypass Core validation.

#### Example

```text
Raw User Text
        ↓
Interpretation Adapter
        ↓
InterpretationProposalArtifact
        ↓
Core Validation / Verification Flow
```

---

### 6.7 Reasoning Interface

The Reasoning Interface defines how ARCS creates options, plans, or analyses from already structured inputs, context, and state.

The reasoning component proposes. It does not execute and it does not approve.

#### Purpose

The purpose of reasoning is to create one or more possible next-step proposals based on the current request, context, memory, and system state.

#### Input

The reasoning component may receive:

* `ValidatedRequestArtifact`
* `ContextArtifact`
* `MemorySnapshotArtifact`
* `SystemStateArtifact`
* `PolicyContextArtifact`

#### Output

The reasoning component may produce:

* `PlanCandidateArtifact`
* `PlanCandidateArtifact[]`
* `ReasoningReportArtifact`
* `NoPlanFoundArtifact`

Each plan candidate must include:

* proposed action
* required tools
* required data
* expected result
* assumptions
* uncertainty
* reasoning metadata

#### Allowed Side Effects

The reasoning component is allowed to:

* generate plan candidates
* request additional context through approved interfaces
* attach reasoning metadata
* mark uncertainty
* declare that no valid plan was found

#### Forbidden Side Effects

The reasoning component must not:

* execute tools
* approve its own plan
* modify memory directly
* send output to the user
* bypass verification
* hide assumptions
* create final decisions

#### Invariants

* A reasoning result is only a proposal.
* Every resulting plan or option must be verifiable.
* Every reasoning path should expose assumptions when available.
* The reasoning component must not perform external actions.

#### Example

```text
ValidatedRequestArtifact
        ↓
Reasoning Component
        ↓
PlanCandidateArtifact[]
        ↓
Verifier
```

---

### 6.8 LLM Adapter Interface

The LLM Adapter Interface defines how ARCS talks to language models as external software capabilities.

An LLM Adapter is not a reasoning authority by itself. It is a boundary module around model transport, prompt packaging, and response retrieval.

#### Purpose

The purpose of the LLM adapter is to call a model and return a traceable model response in a controlled format.

#### Input

An LLM adapter may receive:

* prompt text
* schema constraints
* runtime context
* model selection metadata

#### Output

An LLM adapter may produce:

* `ModelResponseArtifact`
* `StructuredOutputDraft`
* `ModelErrorArtifact`

#### Allowed Side Effects

An LLM adapter is allowed to:

* package model requests
* call a model service
* normalize model responses
* surface parser or schema-related failures

#### Forbidden Side Effects

An LLM adapter must not:

* approve actions
* grant permissions
* commit authoritative system state directly
* execute actions

#### Invariants

* Model output is never authority by itself.
* LLM responses must remain distinguishable from verified ARCS state.
* Structured model output must still pass normal Core controls.

#### Example

```text
Prompt + Schema + Context
        ↓
LLM Adapter
        ↓
StructuredOutputDraft
        ↓
Reasoning / Core Validation Flow
```

---

### 6.9 Verifier Interface

The Verifier Interface defines how ARCS checks proposed plans, actions, and outputs.

The verifier does not create the original plan. It evaluates whether a plan or output satisfies system rules, safety rules, policy constraints, and task requirements.

#### Purpose

The purpose of the verifier is to decide whether an artifact is acceptable for the next stage of the system flow.

#### Input

The verifier may receive:

* `PlanCandidateArtifact`
* `ExecutionResultArtifact`
* `OutputDraftArtifact`
* `PolicyContextArtifact`
* `TraceArtifact`
* `RequirementArtifact`

#### Output

The verifier may produce:

* `VerificationResultArtifact`
* `ApprovalArtifact`
* `RejectionArtifact`
* `RevisionRequestArtifact`

A verification result must include:

* status
* reason
* checked constraints
* failed constraints, if any
* confidence level
* required correction, if any

#### Allowed Side Effects

The verifier is allowed to:

* approve artifacts
* reject artifacts
* request revisions
* attach verification metadata
* create trace information
* escalate uncertain cases

#### Forbidden Side Effects

The verifier must not:

* execute tools
* silently modify the plan
* silently modify the output
* create new goals
* bypass policy rules
* approve artifacts without traceable reasons

#### Invariants

* Every approval must be traceable.
* Every rejection must include a reason.
* The verifier must distinguish between failure, uncertainty, and missing information.
* The verifier must not act as the executor.

#### Example

```text
PlanCandidateArtifact
        ↓
Verifier
        ↓
ApprovalArtifact or RejectionArtifact
```

---

### 6.7 Executor Interface

The Executor Interface defines how approved actions are performed.

The executor only acts on approved execution artifacts. It does not decide what should be done. It only performs actions that have passed the required planning and verification stages.

#### Purpose

The purpose of the executor is to perform approved actions and return execution results.

#### Input

The executor may receive:

* `ApprovedActionArtifact`
* `ToolCallArtifact`
* `ExecutionContextArtifact`
* `PermissionArtifact`

#### Output

The executor may produce:

* `ExecutionResultArtifact`
* `ToolResultArtifact`
* `ExecutionErrorArtifact`

#### Allowed Side Effects

The executor is allowed to:

* call approved tools
* perform approved system actions
* read approved resources
* write approved outputs
* return execution results
* report execution errors

#### Forbidden Side Effects

The executor must not:

* execute unapproved actions
* create its own plan
* bypass permissions
* modify memory directly unless explicitly allowed
* hide tool failures
* retry dangerous actions without approval
* change the goal of the task

#### Invariants

* Every execution must be based on an approved action artifact.
* Every external action must be traceable.
* Every tool result must be represented as an artifact.
* Execution failure must not be hidden.

#### Example

```text
ApprovedActionArtifact
        ↓
Executor
        ↓
ToolResultArtifact
        ↓
Verifier / Memory / Output Flow
```

---

### 6.8 Memory Interface

The Memory Interface defines how ARCS reads from and writes to memory.

Memory is not a passive database. It is part of the system state and must therefore be accessed through controlled interfaces.

#### Purpose

The purpose of the memory interface is to provide controlled access to stored information, prior artifacts, traces, user context, system context, and long-term knowledge.

#### Input

The memory system may receive:

* `MemoryQueryArtifact`
* `MemoryWriteArtifact`
* `TraceArtifact`
* `ExecutionResultArtifact`
* `ValidatedKnowledgeArtifact`

#### Output

The memory system may produce:

* `MemorySnapshotArtifact`
* `RetrievedContextArtifact`
* `MemoryWriteResultArtifact`
* `MemoryErrorArtifact`

#### Allowed Side Effects

The memory system is allowed to:

* retrieve relevant context
* store approved information
* update approved records
* attach provenance metadata
* return memory snapshots
* reject invalid memory writes

#### Forbidden Side Effects

The memory system must not:

* store unverified information as authoritative truth
* overwrite important state without trace
* hide provenance
* execute tools
* create plans
* approve actions
* modify artifacts outside its ownership

#### Invariants

* Every memory write must have provenance.
* Retrieved memory must be distinguishable from current input.
* Uncertain information must be marked as uncertain.
* Memory must not become an untraceable source of truth.

#### Example

```text
MemoryQueryArtifact
        ↓
Memory
        ↓
MemorySnapshotArtifact
        ↓
Planner / Verifier
```

---

### 6.9 Artifact Interface

The Artifact Interface defines the minimum requirements for all ARCS artifacts.

Artifacts are the primary units of information flow inside ARCS. Components do not pass vague internal state to each other. They pass explicit artifacts.

#### Purpose

The purpose of the artifact interface is to make every relevant piece of system information traceable, inspectable, and verifiable.

#### Required Fields

Every artifact must include:

* artifact id
* artifact type
* creator
* creation timestamp
* source
* version
* content
* metadata
* provenance
* validity status

Depending on the artifact type, it may also include:

* confidence
* assumptions
* dependencies
* expiration time
* permissions
* verification status

#### Allowed Side Effects

An artifact may be:

* created
* read
* validated
* rejected
* superseded
* archived

#### Forbidden Side Effects

An artifact must not be:

* silently modified
* overwritten without trace
* used outside its allowed lifecycle
* treated as verified if it has not been verified
* disconnected from its source

#### Invariants

* Every artifact must have a source.
* Every artifact must have an owner or creator.
* Every artifact must have a lifecycle state.
* Changes to important artifacts must be traceable.
* Artifacts are the only authoritative carriers of internal system information.

#### Example

```text
User Input
        ↓
IngressArtifact
        ↓
ValidatedRequestArtifact
        ↓
PlanCandidateArtifact
        ↓
ApprovalArtifact
        ↓
ExecutionResultArtifact
        ↓
ApprovedOutputArtifact
```

---

### 6.10 Core Interface

The Core Interface defines how the central ARCS flow coordinates components.

The core does not replace individual modules. It controls the movement of artifacts through the system.

#### Purpose

The purpose of the core is to coordinate validation, planning, verification, execution, memory access, and output generation.

#### Input

The core may receive:

* `IngressArtifact`
* `SystemEventArtifact`
* `ScheduledTaskArtifact`
* `InternalRequestArtifact`

#### Output

The core may produce:

* `ValidatedRequestArtifact`
* `RejectedRequestArtifact`
* `PlanCandidateArtifact`
* `ApprovalArtifact`
* `ExecutionResultArtifact`
* `ApprovedOutputArtifact`
* `TraceArtifact`

#### Allowed Side Effects

The core is allowed to:

* route artifacts
* trigger approved stages
* manage lifecycle transitions
* create trace records
* enforce system-level invariants
* stop invalid flows

#### Forbidden Side Effects

The core must not:

* execute arbitrary external input directly
* bypass verification
* allow untraceable state transitions
* allow adapters to make core decisions
* allow tools to modify system state without artifacts

#### Invariants

* All meaningful state transitions must be represented as artifacts.
* Every request must pass through validation before planning.
* Every executable action must pass through verification before execution.
* Every external output must be approved before leaving the system.
* The core must preserve traceability across the full flow.

#### Example

```text
IngressArtifact
        ↓
Core Validation
        ↓
Planning
        ↓
Verification
        ↓
Execution
        ↓
Result Verification
        ↓
Output Approval
        ↓
Output Adapter
```

---

### 6.11 Interface Boundary Rule

No component may take over the responsibility of another component unless the spec explicitly defines that behavior.

For example:

* An adapter may normalize input, but it may not validate the request.
* A planner may propose actions, but it may not execute them.
* A verifier may approve or reject plans, but it may not silently rewrite them.
* An executor may execute approved actions, but it may not create its own goals.
* Memory may store approved information, but it may not become an unverified source of truth.

This rule exists to keep ARCS understandable, auditable, and modular.

If a component needs information from another component, it must request it through an approved interface.

If a component produces information that another component needs, it must produce an explicit artifact.

Hidden state, implicit ownership, and undocumented side effects are not allowed.
