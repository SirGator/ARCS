# ARCS — Artifact Reasoning and Control System

ARCS is a governance-first control architecture for AI-assisted systems.

It turns inputs into typed artifacts, verifies possible actions against policy, requires approval when needed, and executes only safe, idempotent actions.

## Core Idea

ARCS is not a single assistant or application.  
ARCS is a platform for controlled decision-making.

The core defines rules, not product features:

- everything is represented as an artifact
- every artifact is schema-validated
- every decision is replayable
- unknown states block by default
- LLMs may propose options, but they are never authority
- interpretation can suggest tasks, but it is never authority
- execution only happens after verification, approval, and permission checks

## Key Features

- **Artifact-based architecture**  
  All system communication happens through immutable, typed artifacts.

- **Schema-first validation**  
  No untyped payload enters the core after ingress.

- **Append-only event log**  
  System state can be audited, replayed, and reconstructed.

- **Deterministic reducers**  
  Derived state is calculated from the log, not hidden runtime state.

- **Fail-closed verification**  
  Verification returns `pass`, `fail`, or `unknown`.  
  `fail` and `unknown` always block.

- **Human approval system**  
  Critical actions require explicit approval bound to a concrete policy version.

- **Safe execution model**  
  Actions are typed, permission-checked, and idempotent.

- **LLM isolation**  
  LLMs can generate proposals, but cannot approve, execute, or change policy.

## Quick Start

### Build

```bash
cmake -S . -B build
cmake --build build
```

### Run

```bash
./build/app/arcs_app
```

Setup tip: copy `arcs.yaml.example` to `arcs.yaml` and adjust the API URL if you use the local worker.

Set `ARCS_INTERPRETATION_API_URL` or `ARCS_INTERPRETER_API_URL` (and optional `ARCS_INTERPRETATION_API_KEY`, `ARCS_INTERPRETATION_MODEL`) to route free-text interpretation through an OpenAI-kompatible LLM API.

You can also put the same values into `arcs.yaml` in the project root or pass them as CLI flags like `--interpretation-api-url ...`.

For the local worker, start:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r tools/requirements.txt
uvicorn tools.interpretation_worker.main:app --host 127.0.0.1 --port 8090
```

The worker returns `interpretation_proposal` results and the core will either create a safe task or skip it.

### Run Tests

```bash
ctest --test-dir build
```

## Architecture Overview

ARCS Core consists of seven core components:

| # | Component | Role |
|---|---|---|
| 1 | Artifact System | Common language of the system |
| 2 | Schema Registry | Validates artifact structure |
| 3 | Store | Append-only memory and event log |
| 4 | Reducer | Derives deterministic state |
| 5 | Verification Engine | Enforces policy and safety rules |
| 6 | Approval System | Handles human-in-the-loop decisions |
| 7 | Execution Engine | Executes typed, idempotent actions |

## MVP Flow

The minimal ARCS V1 flow is:

```text
ingress_event
  -> interpretation_proposal
  -> task (optional)
  -> option
  -> verification_report
  -> approval
  -> action
  -> execution_result
```

Every step is stored as an artifact and can be audited or replayed.
The interpretation step is a proposal only; unsupported inputs are ingested without task creation.

## Safety Principles

ARCS follows a fail-closed security model:

```text
unknown = block
fail    = block
pass    = continue
```

No external action may execute unless all required gates pass:

1. valid schema
2. valid permissions
3. verification result is `pass`
4. approval exists when required
5. approval still matches the current policy
6. action is idempotent

## LLM Role

LLMs are used only as proposal generators.

They may create:

- claims
- assumptions
- option proposals

They may not create:

- approvals
- policies
- permission grants
- actions
- execution results

This keeps the model out of the trusted execution path.

## Project Structure

```text
ARCS/
├── app/                  # Demo entry point
├── src/                  # Core implementation
├── schemas/              # JSON schemas
│   └── v1/
├── tests/                # Unit and integration tests
├── docs/                 # Project documentation
│   ├── ARCHITECTURE.md
│   ├── DEVELOPMENT.md
│   ├── GOVERNANCE.md
│   └── SPECIFICATION.md
└── CMakeLists.txt
```

## Development Principle

Build vertical slices.

Do not build large isolated components without a working end-to-end path.  
Each phase should end with a running demo and tests that prove the slice works.

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Development Guide](docs/DEVELOPMENT.md)
- [Governance Model](docs/GOVERNANCE.md)
- [Core Specification](docs/SPECIFICATION.md)

## Status

ARCS is currently in active development.

The current goal is a complete V1 MVP flow:

```text
input -> ingress_event -> interpretation -> task -> option -> verification -> approval -> action -> execution_result
```

## License

TBD
