# ARCS — Artifact Reasoning and Control System

ARCS is a governance-first runtime for AI-assisted decisions and actions.
Models may propose goals, plans and actions, but they cannot authorize or
execute them. Deterministic core components validate every artifact and gate
every external effect.

## Intended flow

```text
ingress adapter
  -> goal artifact
  -> retrieve known successful flows
  -> propose plans when no suitable flow exists
  -> verify schema, policy, permission and risk
  -> request human approval when required
  -> dispatch an idempotent execution command
  -> compare expected and actual results
  -> append the result and learning evidence
```

Learning never bypasses control. Reused flows must pass the same current
policies, permissions and approval gates as newly proposed flows.

## Current milestone

The Rust implementation currently provides one deliberately small input slice:

- a typed Rust artifact envelope
- the embedded `arcs.input.v1` payload schema
- fail-closed payload validation
- consistency checks between schema ID, artifact type and schema version
- explicit lifecycle-event types instead of mutable artifact state
- an append-only SQLite artifact-version store
- protection against duplicate versions and version gaps

The envelope is not yet validated through a separate JSON schema. For this
first slice, Rust types define its structure while `arcs.input.v1` validates
the payload. Input provenance lives only in the envelope's `source` field; the
payload contains only non-empty `raw_text`.

Reasoning, graph retrieval, policy evaluation, approvals, adapter execution and
learning weights are subsequent milestones. Their schemas and runtime
components are not implemented yet.

## Build and test

Rust 2024 edition and Cargo are required.

```bash
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Run the small in-memory example:

```bash
cargo run
```

## Core invariants

1. Artifacts are immutable and versioned.
2. The store is append-only.
3. Missing schemas and unknown verification outcomes block progress.
4. Approval, verification and execution are separate artifacts.
5. An LLM is a proposal source, never an authority.
6. Learned confidence influences retrieval, never authorization.
7. External actions must be permission-checked and idempotent.

## Project layout

```text
schemas/v1/              current input payload contract
src/core/artifact/       immutable artifact envelope
src/core/schema/         embedded schema registry and validator
src/core/validation/     payload and schema-consistency validation
src/core/lifecycle/      append-only lifecycle event types
src/store/database/      SQLite artifact store
src/main.rs              minimal executable example
```

Payload JSON schemas are the external data contracts. Rust structures define
the internal envelope until a dedicated envelope schema is introduced.
