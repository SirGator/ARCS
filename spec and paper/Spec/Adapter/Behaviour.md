# Adapter

## Behaviour

Adapters are the boundary layer between ARCS and the external world.

An Adapter translates between external systems and the internal ARCS artifact flow. It MUST NOT own authoritative system state, make final decisions, approve actions, verify safety, materialize actions, or execute arbitrary external input directly.

Adapters transform representation, not authority.

---

## 1. Role

An Adapter exists to make external signals compatible with ARCS.

An Input Adapter receives an external signal, extracts relevant content and metadata, normalizes the representation, and creates an `ingress_event` draft or equivalent ingress artifact draft accepted by the Core.

An Output Adapter receives final output instructions or final state-derived output from the Core or Output Coordinator and converts it into a format suitable for an external consumer.

Adapters MUST NOT decide whether a request is valid, safe, useful, executable, approved, or policy-compliant. These decisions belong to the Core lifecycle and its schema validation, Store commit, Reducers, Verification, Approval, Action Materialization, Action Verification, and Execution controls.

---

## 2. Input Adapter Behaviour

An Input Adapter MAY receive external signals from:

* user interfaces,
* API requests,
* file events,
* scheduled events,
* system signals,
* webhooks,
* tool callbacks,
* watcher events,
* sensor events,
* platform events,
* external services.

The Input Adapter MUST extract and preserve relevant metadata.

Relevant metadata SHOULD include:

* source,
* channel,
* timestamp,
* actor or identity information if available,
* authentication or trust metadata if available,
* permission context if available,
* raw payload reference if required,
* normalized payload,
* external correlation identifiers,
* platform-specific source identifiers,
* adapter identity and version.

The Input Adapter MAY perform syntactic validation, normalization, decoding, parsing, rate-limit checks, source checks, and format checks before submitting input to the Core.

Adapter-level validation MUST NOT replace Core schema validation, policy validation, permission checks, verification, approval, or execution controls.

The Input Adapter MUST NOT silently reinterpret user intent. If interpretation is required, the Adapter SHOULD preserve the raw input and submit the normalized signal for Core-controlled task extraction.

The Input Adapter MUST NOT invent missing facts. If information is missing, the missing information MUST remain explicit or be omitted according to schema and policy.

---

## 3. Output Adapter Behaviour

An Output Adapter translates final ARCS output into an external representation.

An Output Adapter MAY:

* format output,
* serialize output,
* summarize output,
* translate output,
* display output,
* forward output to a response channel,
* report delivery status.

The Output Adapter MUST derive its output only from committed artifacts, reducer-derived state, or explicit output instructions received from the Core or Output Coordinator.

The Output Adapter MUST NOT create authoritative state.

The Output Adapter MUST NOT hide safety-relevant failure states unless policy explicitly permits redaction or filtering.

If output delivery creates a meaningful delivery status, failure, or external observation, the Adapter MUST report it back to the Core as an external signal or delivery result so it can be represented through the normal artifact/event flow.

The Adapter MUST NOT directly commit authoritative delivery state unless the architecture explicitly defines a Store interface for that Adapter role. By default, Store commits are coordinated by the Core.

---

## 4. Core Trust Boundary

The Core treats Adapters as boundary components.

Even if an Adapter receives input from a trusted external source, the resulting internal artifact MUST still pass through the normal ARCS lifecycle.

Trusted source does not mean trusted artifact.

Adapter output becomes authoritative only after:

1. schema validation,
2. Store commit,
3. event recording,
4. and, where applicable, reducer processing.

Adapters MUST NOT bypass:

* Schema Gate,
* Store commit,
* Reducers,
* Verification,
* Approval when required by policy,
* Action Materialization,
* Action Verification,
* Execution control,
* Event logging,
* Audit requirements,
* Replay requirements.

---

## 5. Input Lifecycle

The normal Input Adapter lifecycle is:

```text
External Signal
    ↓
Adapter receives signal
    ↓
Adapter extracts raw content and metadata
    ↓
Adapter normalizes representation
    ↓
Adapter creates ingress_event draft
    ↓
Adapter submits draft to Core
    ↓
Schema Gate validates structure
    ↓
Store commits artifact and event
    ↓
Core processing begins
```

The Adapter does not decide the task, plan, verification result, approval state, action, or execution result.

---

## 6. Output Lifecycle

The normal Output Adapter lifecycle is:

```text
Final Core state or output instruction
    ↓
Output Adapter receives output payload
    ↓
Adapter formats result for external target
    ↓
Adapter sends, displays, or forwards result
    ↓
Adapter reports delivery status if required
    ↓
Delivery status re-enters ARCS as signal or result
    ↓
Core decides whether it becomes committed state
```

Output delivery MUST NOT be confused with arbitrary external action execution.

If an operation affects external systems beyond response delivery, user-visible output, or status reporting, it SHOULD be modeled as an `action` and executed through the Execution Engine, not as simple Output Adapter formatting.

Examples of operations that SHOULD be modeled as actions include:

* sending an email as a task result,
* modifying a file,
* calling a third-party API with side effects,
* changing permissions,
* deploying code,
* creating calendar events,
* writing to an external database.

---

## 7. Adapter Rules

All Adapter behaviour MUST follow these rules:

1. External input MUST NOT be executed directly.
2. External input MUST NOT become authoritative state without Core validation and Store commit.
3. Every state-relevant external signal MUST become an explicit artifact, event, rejection record, or traceable runtime rejection according to policy.
4. Adapters MAY normalize data, but MUST NOT silently reinterpret meaning.
5. Adapters MAY attach metadata, but MUST NOT invent missing facts.
6. Adapters MAY reject malformed input before it enters the Core.
7. Adapters MUST NOT approve actions.
8. Adapters MUST NOT verify their own output as safe.
9. Adapters MUST NOT mutate committed artifacts.
10. Adapters MUST NOT bypass schema validation, permission checks, verification, approval, execution control, or logging.
11. Adapter actions MUST be traceable.
12. Adapter failures MUST be explicit when they affect reliability, auditability, security, or state.

---

## 8. Failure Behaviour

Adapter failures MUST be explicit if they affect ingress, output delivery, security, auditability, replayability, or user-visible behaviour.

Adapter failures MAY include:

* malformed external input,
* unsupported external format,
* missing required metadata,
* invalid source identity,
* failed decoding,
* rate-limit rejection,
* unavailable external channel,
* output delivery failure,
* timeout,
* partial delivery,
* duplicate external signal,
* adapter dependency failure.

An Adapter MUST NOT silently drop important input.

An Adapter MUST NOT retry indefinitely without trace.

An Adapter MUST NOT hide partial delivery failure from the Core if the delivery status affects user-visible state, auditability, or follow-up behaviour.

A failure report SHOULD include:

* adapter identity,
* external source,
* lifecycle stage,
* raw payload reference if available,
* normalized payload reference if available,
* failure reason,
* recoverability,
* retry status,
* affected request or correlation ID,
* recommended next handling component.

If the failure is relevant to ARCS state, the Adapter MUST report it back to the Core so it can be represented through the normal artifact/event flow.

---

## 9. Traceability

Every Adapter-produced ingress artifact or output delivery report MUST be traceable.

For input, ARCS SHOULD be able to answer:

* Which external signal was received?
* Which Adapter received it?
* Which source and channel did it come from?
* Which actor or identity was attached?
* Which metadata was preserved?
* Which raw payload was referenced?
* Which normalized artifact draft was created?
* Was the artifact accepted or rejected by the Core?

For output, ARCS SHOULD be able to answer:

* Which internal result caused the output?
* Which Output Adapter handled it?
* Which external target received it?
* Was the output delivered, failed, delayed, cancelled, or partially delivered?
* Did the delivery status re-enter ARCS as a signal or result?

Traceability is REQUIRED so that boundary interactions can be debugged, audited, replayed, and secured.

---

## 10. Final Rule

Adapters translate external reality into ARCS-compatible representations and translate final ARCS results back into external representations.

They do not govern, verify, approve, materialize, or execute ARCS decisions.

In short:

**Adapters translate at the boundary. The Core governs the lifecycle. Executors perform verified actions.**
