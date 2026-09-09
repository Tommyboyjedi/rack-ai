# Application Coding Principles

These principles apply to all application-owned production code unless an explicit exception is approved by the user and documented in an application-level Markdown file.

The intent is to keep code small, explicit, object-oriented, testable, easy to reason about, and resistant to architectural drift without introducing unnecessary ceremony.

## 1. Class size

Application-owned classes must not exceed **100 lines of executable code**, excluding comments and docstrings.

If a class would exceed that limit, split responsibilities into smaller collaborating objects before extending it further.

An exception is allowed only when:

1. the user explicitly agrees;
2. the reason is documented; and
3. the exception is recorded in an application-level Markdown file.

Large coordinator, manager, service, processor, or utility classes are a design smell and should be decomposed rather than extended by adding more branches.

## 2. Method and constructor parameters

A constructor or method must accept **no more than two input parameters**, excluding `self`.

If more than two related values are required, create a dedicated input/parameter object representing the concept being passed.

Prefer:

```python
@dataclass(frozen=True)
class FailureContext:
    contract: BehaviorContract
    observation: FailureObservation
    run_state: BehaviorContractRunState

router.route(context)
```

instead of:

```python
router.route(contract, observation, run_state, cycle, phase, work_unit)
```

The same rule applies to constructors.

## 3. Parameter and data objects

For Python, simple data-transfer, parameter, state, and value objects should normally use `dataclass`.

Prefer immutable objects using:

```python
@dataclass(frozen=True)
```

when mutation is unnecessary.

Do **not** use Pydantic for ordinary application-domain objects or parameter bags. Framework-owned model types may use framework mechanisms when the framework requires them.

For non-Python code, use the language's ordinary lightweight typed value/record/struct mechanism to achieve the same purpose.

## 4. Composition over inheritance

Application-owned classes must not inherit from other application-owned classes.

Prefer composition and explicit collaboration between objects.

Inheritance is permitted only when inheriting from an already-developed external framework or library where that is the intended extension mechanism.

Do not create application inheritance hierarchies such as `BaseCoordinator -> TddCoordinator -> AdaptiveTddCoordinator`.

## 5. Small classes with one responsibility

Prefer multiple small collaborating classes over large classes that perform planning, execution, persistence, classification, recovery, and state mutation together.

Each class should have one clearly describable responsibility.

A useful test is:

> Can the class's purpose be described accurately in one short sentence without using "and"?

If not, the class probably owns too much.

## 6. Small methods

Methods should perform one coherent operation.

Do not disguise an oversized class by moving a large method into many private helper methods while leaving all responsibilities in the same object. Extract responsibilities into collaborating objects when they represent distinct behavior.

## 7. No magic numbers

Meaningful numeric policy values must not be embedded directly in application logic.

Use typed configuration or clearly named constants.

Examples include retry limits, timeouts, resource limits, maximum depths, thresholds, page sizes, and capacity limits.

Stable mathematical/protocol constants may remain named constants when true configuration would add no value.

## 8. No magic strings

Meaningful domain states, classifications, route names, statuses, executor types, feature names, policy names, and repeated identifiers must not be scattered as raw strings through application logic.

Prefer enums, typed value objects, named constants, or typed configuration.

Strings may exist at serialization, configuration, database, or external API boundaries, but should be converted into typed application values when entering core logic.

## 9. Configuration over embedded policy

Behavior that may reasonably vary between deployments, projects, environments, or operational modes belongs in configuration rather than being buried in implementation logic.

Examples include retry limits, timeouts, resource limits, executor backend, trusted roots, model/resource policy, and feature limits.

Configuration should be typed and validated when loaded.

Do not turn every stable constant into configuration merely to satisfy this rule.

## 10. SQL before application-side data processing

When a relational SQL database is part of the application, operations that are naturally and efficiently expressible in SQL should be performed by the database rather than by loading unnecessary data into application memory and processing it there.

Prefer SQL/database operations for filtering, joins, grouping, aggregation, existence checks, ordering, pagination, set operations, and suitable bulk updates.

Do not move business meaning into obscure SQL merely to satisfy this principle. Business rules remain explicit in the application/domain layer; data selection and set processing should happen as close to the database as practical.

## 11. Explicit domain types

Important application concepts should have explicit types rather than being represented by groups of unrelated primitives, loose dictionaries, booleans, or strings.

Examples include `FailureContext`, `DependencyDecision`, `ExecutionRequest`, `TrustedRevision`, `ProjectEnvironment`, `ReviewResult`, and similar domain concepts.

Avoid primitive obsession.

## 12. Avoid generic dictionaries in core logic

Raw dictionaries are acceptable at serialization, configuration, persistence, and external API boundaries.

Inside core application/domain logic, convert them to typed dataclasses, structs, records, enums, or value objects.

Core code should not depend on remembering undocumented dictionary keys.

## 13. Dependency direction

Higher-level application and domain logic should depend on abstractions representing capabilities, not directly on infrastructure details.

Framework, database, filesystem, HTTP, Rack AI, model-provider, and network details belong at infrastructure/adaptor boundaries.

Prefer explicit capability interfaces such as `ReasoningGateway`, `ExecutionGateway`, or repositories over direct infrastructure calls from domain logic.

## 14. Separate decisions from effects

Where practical, the object deciding **what should happen** should not also perform every external side effect required to make it happen.

Prefer explicit separation such as:

- classifier -> classification;
- planner -> plan/decision;
- router -> transition decision;
- repository -> persistence;
- executor -> execution;
- coordinator -> coordination between these objects.

This keeps decision logic independently testable and makes state transitions easier to reason about.

## 15. Explicit state transitions

Complex workflows and state machines should use typed states and explicit transition/result objects.

A transition should make clear:

- current state;
- triggering event/evidence;
- chosen action;
- next state;
- required persisted effects.

Avoid giant nested `if`/`elif` or `match` structures that implicitly encode the application's state machine in one coordinator.

## 16. Fail closed

For autonomous execution, security, trust, revision management, orchestration, and evidence handling, uncertainty must not be converted into success.

If the system cannot prove an action is valid, trusted, within policy, or correctly evidenced, it should stop, reject, defer, or request explicit replanning rather than infer success.

## 17. Preserve evidence and traceability

Decisions that change application state, trusted revision, plan structure, retries, dependencies, or recovery path must retain enough evidence to explain why the transition happened.

This is especially important for autonomous systems.

## 18. Tests follow object boundaries

Tests should primarily exercise observable behavior through public object interfaces.

Avoid tests that are tightly coupled to private implementation details solely because responsibilities have become tangled inside a large object.

Small classes and explicit boundaries should make focused tests straightforward.

## 19. Refactor before extending an overgrown object

If a new feature would cause an existing class, constructor, method, or responsibility boundary to violate these principles, refactor the structure first.

Do not repeatedly append another conditional branch to a large coordinator because that is the fastest local patch.

## 20. Exceptions

These principles are strong defaults, not an excuse to create unnecessary abstraction.

If following a rule would materially make the design less clear, less safe, or substantially more complex, an exception may be proposed.

For exceptions to class size, parameter count, inheritance, or another architectural rule:

1. explain why following the rule would make the design worse;
2. obtain explicit user approval before implementation; and
3. document the approved exception and rationale in an application-level Markdown file.

Unapproved or undocumented architectural exceptions are not accepted.

## Governing philosophy

Prefer **small objects, explicit domain concepts, composition, constrained interfaces, typed state, database work in the database, configuration instead of hidden policy, and understandable state transitions**.

Apply SOLID and object-oriented design principles pragmatically. Avoid both monolithic procedural code and needless abstraction. The desired outcome is boring, explicit, maintainable code whose responsibilities and state transitions can be understood without reconstructing the entire application in the reader's head.
