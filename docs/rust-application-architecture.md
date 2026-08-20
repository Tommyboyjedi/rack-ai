# Rack AI Rust Application Architecture

Date: 2026-08-20
Status: planned target architecture

## Purpose

This document defines the target shape of Rack AI as a real Rust application.

The existing Python control plane is now the reference prototype. It discovered
useful domain boundaries and working behavior, but it should not continue to
harden into the long-term architecture.

The long-term system should be a structured Rust application that:
- owns rack capability and execution policy
- persists durable task, run, DAG, and lease state
- schedules work across heterogeneous GPUs and backends
- uses JCode, vLLM, and future services as execution backends
- remains small, testable, typed, and operationally trustworthy

## Engineering rules

These rules are treated as design constraints for the Rust build:
- one primary type per file
- interfaces first
- prefer polymorphism over long switch or match dispatch chains
- no type larger than 100 lines where a smaller composition is viable
- no global mutable state
- no magic strings or magic numbers
- maximum two constructor or method parameters; introduce value objects where needed
- composition over inheritance-style hierarchies
- follow SOLID where doing so does not create verbosity without value
- maintain 100 percent automated test coverage for Rust crates

## Rust interpretation of the rules

Rack AI is being built in Rust, not an OOP language in the Java or C# sense.
The rules above therefore map to Rust as follows:
- "class" means a focused `struct`, `trait`, or small enum-backed type
- "interface" means a `trait` that defines behavior before concrete adapters
- long `match` statements should be replaced with trait objects, strategy types,
  or behavior attached directly to domain types where practical
- constructor parameter limits should push configuration into typed request or
  options structs rather than long primitive argument lists
- shared state should move through owned services and repositories, not globals

## Target repository shape

```text
/srv/rack-ai
  Cargo.toml
  crates/
    rack_ai_domain/
      src/
        task/
        run/
        dag/
        worker/
        resource/
        model/
        lease/
    rack_ai_application/
      src/
        submit/
        schedule/
        execute/
        retry/
        inspect/
    rack_ai_infrastructure/
      src/
        state_store/
        registry_store/
        lease_store/
        backends/
          jcode/
          vllm/
    rack_ai_cli/
      src/
        commands/
    rack_ai_tests/
      tests/
```

## Crate responsibilities

### `rack_ai_domain`

Pure domain model.

Owns:
- `TaskId`
- `RunId`
- `DagNodeId`
- `TaskSpec`
- `RunState`
- `DagState`
- `Worker`
- `Resource`
- `Model`
- `Lease`
- value objects for status, timeout, attempts, and placement

Rules:
- no network calls
- no filesystem calls
- no backend-specific logic
- rich invariants and validation belong here

### `rack_ai_application`

Application services and use cases.

Owns:
- submit task
- admit runnable node
- acquire and release lease
- execute next runnable unit
- update durable run state
- retry or timeout policy
- status inspection use cases

Rules:
- depends on domain traits, not concrete adapters
- orchestrates workflow but does not speak HTTP or shell directly

### `rack_ai_infrastructure`

Concrete adapters.

Owns:
- JSON or database-backed repositories
- vLLM endpoint probes
- JCode command execution adapters
- filesystem-backed lease store
- future API server or daemon persistence adapters

Rules:
- implements traits defined elsewhere
- isolates external process and I/O behavior

### `rack_ai_cli`

Thin operator surface.

Owns:
- `rack-ai submit`
- `rack-ai run-next`
- `rack-ai status`
- `rack-ai retry`
- `rack-ai cancel`
- `rack-ai healthcheck`

Rules:
- argument parsing only
- delegate to application services
- no business logic in command handlers

### `rack_ai_tests`

Integration and contract testing.

Owns:
- durable queue progression tests
- DAG scheduling tests
- admission and lease contention tests
- backend contract tests against live rack services where appropriate

## First Rust scope

The first Rust milestone should reach parity with the current Python prototype
for the durable execution backbone only.

That scope is:
1. registry loading
2. durable queue submission
3. run-state persistence
4. resource lease handling
5. dependency-aware DAG advancement
6. status inspection CLI

This deliberately excludes:
- UI work
- HTTP server work
- advanced multi-run scheduling fairness
- additional agent intelligence
- replacing JCode itself

## Python disposition

The current Python code should be treated as:
- behavioral reference implementation
- migration oracle
- temporary fallback while Rust reaches parity

It should not receive major new architecture beyond what is needed to support
migration or live rack operations.

## Testing stance

The Rust build should move away from shell-smoke-led confidence and toward:
- unit tests for domain invariants
- application service tests with fake repositories and fake backends
- integration tests for filesystem-backed state transitions
- explicit coverage reporting as a build gate

The 100 percent coverage target should apply to Rust crates and be enforced in
CI or local release checks once the toolchain is installed.

## Immediate next actions

1. install Rust toolchain on `gpurack`
2. create Cargo workspace and crates
3. port domain model first
4. port durable repositories and lease store
5. port queue runner and DAG advancement
6. add Rust tests that reproduce current Python smoke coverage
7. retire Python command entrypoints after parity
