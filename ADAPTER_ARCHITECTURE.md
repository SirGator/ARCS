# ARCS Adapter Architecture

ARCS bleibt domänenneutral. Smart Home, Softwarewerkzeuge, Roboter, Sensoren
und Sprachmodelle werden ausschließlich durch externe Adapter spezialisiert.

```text
Observation/Input adapter
  -> capability + grant + schema check
  -> Core-owned Artifact envelope
  -> append-only history + Current-State pointer
  -> transient ArtifactNetwork activation
       -> known DataRequest -> correlated DataAdapter -> new Current-State
       -> known response    -> no model call
       -> no known response -> curated ReasoningRequest -> model candidate
  -> validated candidate commit
  -> correlated OutputAdapter
  -> Result Artifact
```

The folders follow the same vertical boundaries:

```text
src/adapters/<port>/          serializable external contracts
src/adapters/gateway/<case>   authorization, validation, and atomic commit
src/runtime/routing/          deterministic fast path plus fallback decision
src/runtime/agent_cycle/      explicit phases of one host-controlled cycle
src/store/network/            weighted activation only
src/store/relations/          typed audit relationships only
src/store/database/           private SQLite mechanics
```

## Manifest is not authority

An `AdapterManifest` declares capabilities and their schema contracts. It never
grants those capabilities. A separately configured `AdapterGrant` controls:

- enabled capabilities;
- permissions granted by the operator;
- the producer class (`Adapter`, `Model`, `System`, or `Executor`);
- assigned trust and the observation source kind;
- allowed Subject namespaces for every enabled Observe capability;
- maximum payload and source-reference sizes;
- absolute context, output, token, and candidate limits for reasoning.

A capability can only be used when it is declared by the manifest, enabled by
the grant, and all declared permission requirements are covered by that grant.

Every `Act` and `Output` capability is conservatively treated as an
external-effect boundary. It must declare an idempotency contract and require
at least one explicit permission. The current slice deliberately has no action
dispatcher.

`Data`, `Reason`, and `Output` are correlated ports. They cannot be installed
through the generic registration method, because that would bypass their
endpoint- and producer-specific checks. Each must use its dedicated
registration slice.

## Boundary submissions

Only an enabled `Observe` capability may push data without a prior Core
invocation. `Transform` and `Act` output is rejected on this path; future
response paths must correlate it with a Core-generated invocation or execution
token.

An observation adapter may submit only:

- the capability it is using;
- the desired payload schema;
- the domain-defined subject of the current-state slot;
- an external source reference;
- the payload.

It cannot choose another installed adapter by name: registration returns an
opaque `AdapterSession`, which is bound to one installation. A future
out-of-process transport must create that handle only after authenticating the
connection.

The operator grant also binds every Observe capability to one or more Subject
namespaces. A namespace matches itself or slash-delimited children. Therefore
an adapter granted `server-01` can update `server-01/cpu`, but cannot replace
`server-010/cpu` or another adapter's `server-02` Current-State slot.

The adapter cannot submit an accepted `Artifact`. The gateway creates all
authoritative fields after validation:

- artifact and version IDs;
- artifact type and schema version;
- timestamp;
- actor, producer, source, and trust classification;
- stream key, tags, and provenance.

`SqliteArtifactStore::append` is crate-internal, so external adapter crates
cannot bypass this boundary.

The trusted host may create a schema-validated Core event through
`record_internal`, for example a known DataRequest template. That DTO still
cannot choose IDs, time, actor, source class, or trust and is not exposed to an
adapter process.

## Persistent view, transient activation

Every accepted observation is immutable and remains in the append-only
history. In the same SQLite transaction, `current_artifacts` moves the pointer
for the exact `(subject, schema_id)` slot to the new version. A host can
therefore reload the current environment view after a restart without erasing
the earlier observations. `history(subject, schema_id)` returns every state in
stable commit order, and Observation stream keys remain stable across those
versions.

Numeric activations are deliberately not stored. On every evaluation, the
runtime selects the relevant current Artifact versions and passes them as
`ActiveSource` values. This distinction avoids stale neural state while keeping
the environment view durable:

```text
durable:   Artifact history, Current-State pointers, weighted edges, relations
transient: source activations, contributions, aggregate target activation
```

Weighted `NetworkEdge` values are restricted to `-1.0..=1.0`. They alone
participate in activation. Semantic relations such as `fulfills`,
`caused_by`, `supported_by`, `generated_by`, and `result_of` are a separate
audit graph and never change network activation.

## Correlated Data and Output

A DataAdapter receives a Core-generated invocation for one already persisted
request version. The gateway verifies the exact `CapabilityRef`, accepted and
emitted schemas, payload/reference limits, response correlation, and subject.
Artifact, Current-State pointer, `fulfills`, and `caused_by` are committed
atomically.

An OutputAdapter receives one already persisted candidate version. Output
capabilities require an `Executor` grant, an explicit permission, and an
idempotency promise. A successful correlated receipt becomes a Result Artifact
with a persistent `result_of` relation. Deterministic invocation IDs let the
external adapter deduplicate retries. All three correlated wire DTOs
(`DataInvocation`, `ReasoningInvocation`, and `OutputInvocation`) carry the
exact authorized `CapabilityRef`, so one adapter endpoint can safely implement
multiple capabilities.

## External schemas

Adapter schema packages are installed atomically. If one schema or the
manifest/grant combination fails, none of the package is registered.

Every accepted schema document is additionally stored in SQLite as canonical
JSON. A schema ID is immutable: after restart, registering a different document
under an already bound `$id` fails with `SchemaDrift`. Historical stores that
contain artifacts without such a binding fail closed instead of guessing which
former contract applied. Compatible evolution therefore always uses a new
`.v<n>` ID.

The current dependency-free validator intentionally supports a strict JSON
Schema Draft 2020-12 subset:

- `type`, `properties`, `required`, `additionalProperties`;
- `items`, `minItems`;
- `minLength`;
- `minimum`, `maximum`;
- `enum`, `const`;
- the `date-time` format.

Unsupported keywords such as `$ref`, `$defs`, `oneOf`, `anyOf`, `allOf`,
`pattern`, or `maxLength` are rejected during registration. They are never
silently ignored. Adapter-provided IDs currently follow:

```text
arcs.<artifact_type>.<domain-or-adapter-namespace>.v<version>
```

IDs are at most 256 bytes; every name segment is non-empty and contains only
lowercase ASCII letters, digits, `_`, or `-`. Versions are canonical positive
integers, so aliases such as `v01` are rejected.

## Reasoning fallback

`ReasoningAdapter` is the internal port to an external LLM, planner, or solver.
The trait is useful for dependency injection and tests; it is not a stable
plugin ABI or a security sandbox. Production adapters should run in separate
processes behind authenticated HTTP, stdio, or IPC transports.

Before the external call, ARCS stores a closed `arcs.reasoning_request.v1`
audit Artifact containing only the objective and sorted context version IDs.
Even a transport failure therefore leaves evidence that a model call was
prepared.

The reasoner receives no Store, Network, Gateway, credentials, source
references, creator metadata, or tags. The Core selects stored artifact
versions and whitelists individual top-level payload fields. It also enforces:

- request limits below immutable operator ceilings;
- context item and byte limits;
- output byte and candidate limits, plus a token ceiling in the adapter
  invocation contract;
- an exact target schema;
- Candidate-only schema types (`candidate` or `*_candidate`);
- a finite, control-free, and size-bounded model trace;
- fully qualified `CapabilityRef { adapter_id, capability_id }` values;
- candidate references limited to the supplied context.

Reasoner output is always untrusted. A valid response becomes a
`ValidatedProposal`, not an action. If committed, it is stored as
`ActorType::Model` with low trust and adapter-reported model provenance. Those
reported hashes are audit metadata, not a Core trust proof. Committing stores
`supported_by` links only to context versions explicitly referenced by the
candidate, while `generated_by` points to the exact ReasoningRequest. Merely
visible context is not mislabeled as evidence. It does not dispatch an executor
or change network edges. A validated proposal can be committed only once per
ReasoningRequest version and candidate index within the running gateway;
cloned/replayed proposal values are rejected.

## Fast path versus fallback

`HybridRouter` calls `ArtifactNetwork::propagate_many` first:

1. Weighted contributions from all active sources are aggregated per target.
2. A target above the threshold is accepted only when its exact schema and
   trust level satisfy the Core-owned `KnownRoutePolicy`.
3. An eligible known candidate is returned without calling a model.
4. A successful lookup without an eligible target may invoke the
   `ReasoningAdapter`.
5. An empty fallback result is represented explicitly as `Unresolved`.
6. A network, validation, or persistence error is returned as an error and
   never disguised as an unknown situation.

The route policy itself must contain at least one unique, registered schema.
An empty, duplicate, or unknown policy fails before network evaluation and
before any model call; configuration errors can therefore never manufacture an
expensive fallback.

This keeps normal agent operation deterministic and cheap while reserving LLM
cost for genuinely unknown or complex situations.

`AgentCycle` exposes these steps as separate, domain-neutral phases:

1. `evaluate_network`;
2. `acquire_data`;
3. `resolve_with_fallback`;
4. `commit_proposal`;
5. `deliver_output`.

There is intentionally no server-specific `run` method. The included
end-to-end test specializes the same ports with CPU, process, chat, and model
adapters and proves that Input plus current state activates a DataRequest,
while the model is called exactly once only after the enriched network still
has no known response.

## Deliberately not implemented yet

- Action dispatch and authorization tokens;
- policy, risk, and human-approval services;
- persistent adapter transport/authentication;
- a scheduler or daemon loop for periodically invoking observation adapters;
- credential isolation and OS sandboxing;
- asynchronous timeouts and cancellation;
- schema migrations and adapter uninstall/upgrade;
- a durable invocation/proposal completion journal for Core-side replay
  suppression across process restarts (external Output adapters must already
  deduplicate the deterministic invocation ID);
- durable globally unique ID generation (the included sequence generator is
  for a single process/demo);
- Core-computed cryptographic invocation/response hashes;
- automatic edge creation, weight learning, STDP, decay, or recursive ticks.
- automatic projection of a new Current-State version onto stable concept
  nodes or edges associated with its predecessor. Network edges intentionally
  bind exact `VersionId` values today. A long-running host must currently
  activate and connect the newly selected Current version explicitly; a future
  deterministic adapter transform or subject/schema edge policy should provide
  that bridge without involving an LLM.

These features belong above or outside the completed adapter boundary and must
not weaken its current invariants.
