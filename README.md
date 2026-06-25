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
- free text is interpreted through one external `/interpret` contract
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

Setup tip: copy `config/arcs.yaml.example` to `config/arcs.yaml`.

ARCS uses a two-stage interpretation pipeline:

1. **text-to-json-parser** (FastAPI, Python) — translates free text into
   schema-conformant JSON. Lives in the sibling directory
   `../text-to-json-parser/` and is started as its own service.
2. **interpretation_worker** (stdlib HTTP, Python) — bridges the ARCS Core
   (C++) and the parser. Exposes the `/interpret` endpoint the C++ client
   calls, and forwards the request to the parser.

### Start both services

```bash
# 1. Parser (in ../text-to-json-parser)
cd ../text-to-json-parser
uvicorn main:app --host 127.0.0.1 --port 8000

# 2. ARCS interpretation worker (in this directory)
python -m tools.interpretation_worker --config config/arcs.yaml
```

The worker reads `parser_url` (default `http://127.0.0.1:8000`) and
`interpret_api_url` (default `http://127.0.0.1:8090/interpret`) from
`config/arcs.yaml`. Both can be overridden via environment variables
(`ARCS_PARSER_URL`, `ARCS_PARSER_TIMEOUT`,
`ARCS_PARSER_PROMPT_FILE`).

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
  -> task
  -> task (optional)
  -> option
  -> verification_report
  -> approval
  -> action
  -> execution_result
```

Every step is stored as an artifact and can be audited or replayed.
Unsupported free text is handled by the external interpretation contract.

### Free-text interpretation pipeline

When the input is free text instead of a typed control line, ARCS routes it
through the external interpretation contract:

```text
ARCS Core (C++)
    │  POST /interpret
    ▼
interpretation_worker (Python, stdlib HTTP)
    │  POST /generate-json
    ▼
text-to-json-parser (Python, FastAPI)
    │  Ollama / OpenAI / etc.
    ▼
schema-conformant JSON (interpretation_proposal)
    ▲
    │  response
    └── ARCS Core interprets the proposal
```

- ARCS never talks to the parser directly. The C++ client only knows
  `interpret_api_url` (the worker).
- The worker is the single place that knows the parser contract
  (`/generate-json`, `{text, schema, context, prompt}`).
- The worker falls back to the built-in `arcs.interpretation_proposal.v1`
  schema if the ARCS caller doesn't pass one.

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
├── tests/                # C++ unit and integration tests
├── tools/
│   └── interpretation_worker/
│       ├── main.py              # HTTP server + bridge to text-to-json-parser
│       ├── parser_client.py     # HTTP client for the parser service
│       └── tests/
│           ├── test_parser_bridge.py  # bridge unit tests
│           └── test_full_stack_e2e.py # parser + worker + arcs_app E2E
├── docs/                 # Project documentation
│   ├── ARCHITECTURE.md
│   ├── DEVELOPMENT.md
│   ├── GOVERNANCE.md
│   └── SPECIFICATION.md
└── CMakeLists.txt
```

The external text-to-json-parser lives in the sibling directory
`../text-to-json-parser/`. It is its own Python project and a separate
Git repository. ARCS depends on it at runtime over HTTP, not via a
submodule.

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
input -> ingress_event -> task -> option -> verification -> approval -> action -> execution_result
```

## License

TBD
