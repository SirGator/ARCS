# Core

## Purpose

The Core is the central coordination and governance layer of ARCS.

Its purpose is to enforce the controlled lifecycle through which external signals, tasks, options, approvals, actions, execution results, and derived state transitions must pass before ARCS may change authoritative state or trigger an external side effect.

The Core does not represent a single algorithm, planner, model, or executor. It defines and enforces the rules that govern artifact creation, schema validation, store commits, reducer-derived state, verification, approval when required by policy, deterministic action materialization, action verification, execution, and replayable logging.

The Core exists to ensure that ARCS never acts directly on raw external input, model output, module output, tool output, or hidden runtime state. Instead, every authoritative transition must be represented by schema-valid artifacts and committed events.

External input is first represented as an `ingress_event`. Interpreted intent or obligation is represented as a `task`. Possible next steps are represented as `option` artifacts. Verified and approved-if-required options may be materialized into `action` artifacts. Executors may only run verified actions and must produce `execution_result` artifacts after actual execution.

The Core is responsible for maintaining the separation between:

* external input,
* runtime context,
* model output,
* module output,
* committed artifacts,
* reducer-derived state,
* verified options,
* approvals,
* executable actions,
* execution results,
* external outputs.

Runtime objects, including context views, planner state, temporary buffers, UI state, and tool responses, are not authoritative by themselves. They may influence computation only if their effects are converted into schema-valid artifacts and committed through the Store.

The Core acts as the system boundary that prevents hidden mutable runtime objects from becoming authoritative state and prevents unverified proposals from becoming executable actions.

In short, the Core exists to make ARCS controlled, traceable, verifiable, replayable, and safe to extend.
