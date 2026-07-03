# Problem / Solution — Getting from ARCS Core to a JARVIS-like Assistant

This document lists the concrete obstacles between the current ARCS Core
(governance-first control architecture) and the long-term product goal
(a modular AI system with a JARVIS-like exterior: voice-driven, conversational,
proactive) — and the possible ways to overcome each one.

## Problem 1 — Personality vs. Governance

**Problem:** A JARVIS-like assistant implies natural, proactive, opinionated
behavior. ARCS Core is deliberately strict: schema-first, fail-closed,
LLM output is never authority. If assistant-like behavior is built directly
into the Core, the governance guarantees erode over time (exceptions creep in
"just for the assistant flow").

**Possible solutions:**
- Keep all personality/conversational behavior in an ingress adapter, never
  in the Core. The adapter may only ever produce `ingress_event` /
  `interpretation_proposal` artifacts — same as any other input source.
- Treat "proactive suggestion" as just another `option` artifact generated
  by a proposal source. It still has to pass verification/approval like any
  other option; proactivity changes *when* an option is generated, not
  *whether* it needs to clear the pipeline.
- Write down an explicit rule (e.g. in `Core/Behaviour.md`) that no future
  feature is allowed to add a bypass path around verification/approval,
  regardless of how "obviously safe" it seems for an assistant UX.

## Problem 2 — Docs / Reality Drift

**Problem:** The top-level `README.md` references a canonical `docs/`
specification tree (`docs/core-rules/`, `docs/current-state/STATE.md`, etc.)
that does not exist in the repository. The actual spec lives under
`spec and paper/Spec/`. This makes the README misleading for anyone (including
future-you) trying to onboard into the project.

**Possible solutions:**
- Decide which location is canonical: either migrate `spec and paper/Spec/`
  into a real `docs/` tree matching the README's references, or rewrite the
  README to point at the actual current locations.
- Add a `docs/current-state/STATE.md`-equivalent file that is kept up to date
  every time a milestone lands, so "current status" is always answerable
  without archaeology.
- Treat README/spec consistency as a checklist item at the end of each
  work session, not an afterthought — drift compounds silently otherwise.

## Problem 3 — Voice / Natural-Language Front End Does Not Exist Yet

**Problem:** The JARVIS vision requires a voice interface and natural
conversational interaction. Currently only a text-to-JSON interpretation
pipeline exists (`text-to-json-parser` + `interpretation_worker`), which is
turn-based and one-directional (text in, structured proposal out).

**Possible solutions:**
- Treat voice as *another ingress adapter* upstream of the existing
  interpretation pipeline: speech-to-text feeds free text into the same
  `/interpret` contract that already exists. No new core concept needed.
- Add a symmetric output path (text-to-speech / spoken response) as an
  adapter that consumes `execution_result` / `option` artifacts and renders
  them as speech — this can be built independently of the Core.
- Start with push-to-talk / turn-based voice before attempting always-on
  wake-word listening — reduces false-trigger risk and keeps scope small
  for a first working slice.

## Problem 4 — Proactivity Requires Long-Running Context

**Problem:** JARVIS acts without being asked (reminds, warns, prepares
things). ARCS's MVP flow is currently reactive: an `ingress_event` starts
every chain. Proactive behavior needs some notion of "notice something on
its own" without becoming a hidden, unauditable planner.

**Possible solutions:**
- Introduce scheduled/triggered `ingress_event`s (e.g. a cron-like source,
  or a monitor that watches external state) — proactivity becomes just
  another producer of `ingress_event`, so it flows through the exact same
  pipeline and is equally auditable.
- Keep any "watching" logic (timers, sensors, calendar polling) outside the
  Core as adapters; the Core still only ever sees artifacts, never the
  underlying monitoring state.
- Start with a small number of explicit triggers (e.g. calendar reminder,
  file-change watch) rather than a general always-on inference loop —
  avoids building an opaque proactive planner before the pattern is proven.

## Problem 5 — Scope: One Vision, Long Timeline

**Problem:** The gap between "governance core with unit tests" and
"JARVIS-like assistant" is large. Without milestones, work risks scattering
across many partially-built subsystems instead of one working path.

**Possible solutions:**
- Finish the MVP flow end-to-end for a single real use case first
  (`ingress_event -> task -> option -> verification -> approval -> action ->
  execution_result`), before adding voice, proactivity, or personality.
- Add each new capability (voice in, voice out, one proactive trigger,
  one "personality" ingress adapter) as its own vertical slice with its own
  test, following the existing "Development Principle: build vertical
  slices" already stated in the README.
- Revisit this document after each milestone and mark solved problems
  instead of letting it become stale.
