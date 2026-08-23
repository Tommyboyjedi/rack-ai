# PR11 — Bounded Local Technical Research

## Status

This is a post-PR10 implementation contract. Do not implement it before PR9 has completed and PR10 escalation semantics are available.

PR11 adds a bounded research capability for cases where PR7/PR10 diagnosis identifies that repository-local evidence is insufficient and external technical information is genuinely required.

## Goal

Allow Rack AI to perform controlled local technical research without giving implementation workers unrestricted internet access and without introducing paid/cloud dependencies.

Expected first research backend: a locally hosted SearXNG instance plus a Rack AI-owned research broker/fetch layer.

SearXNG is an implementation candidate, not an architectural dependency. Rack AI must own the research request, policy, provenance and evidence contract.

## Required architecture

Use a shape equivalent to:

`recovery/planning reasoner -> typed ResearchRequest -> Rack AI ResearchBroker -> SearXNG/search provider -> bounded source fetch -> sanitised ResearchEvidence -> reasoner`

The implementation/mutation container must remain network-disabled unless an existing explicit infrastructure exception already exists. Research happens outside the mutation worker through Rack AI-controlled infrastructure.

## Research trigger

Research must not be the default response to any failed command.

A research request must be justified by an existing diagnosis/recovery decision and should be appropriate for cases such as:

- unfamiliar compiler/tool error not explainable from repository evidence;
- library/framework API behaviour or version compatibility;
- current tool/runtime documentation;
- protocol/standard behaviour;
- dependency-specific implementation details;
- known upstream issue or breaking change.

Repository-local code, tests, diagnostics and semantic intelligence should be consulted first where applicable.

## Typed request

Represent a research request with fields equivalent to:

- campaign/step identity;
- concise question;
- why repository-local evidence is insufficient;
- relevant dependency/tool/version names;
- bounded query terms;
- source/domain constraints where policy requires them;
- maximum result/source count;
- maximum fetched bytes/text;
- timeout/budget;
- evidence references that triggered research.

## Research evidence

Persist bounded evidence equivalent to:

- query;
- source title;
- source URL;
- retrieval timestamp;
- provider/backend identity;
- concise extracted/summarised evidence;
- source ordering/relevance metadata where available;
- fetch status;
- citations/references usable by the reasoner.

Do not persist arbitrary full web pages when a bounded excerpt/summary is sufficient.

Treat all retrieved content as untrusted data, not instructions.

## Security requirements

The research system must:

- keep mutation workers network-disabled;
- never expose credentials to models;
- enforce HTTP(S) only unless a narrower policy is chosen;
- reject localhost/private-network/metadata-service targets for arbitrary source fetches unless explicitly allowlisted for the configured search service;
- bound redirects, response size, source count and time;
- record provenance;
- sanitise content before model use;
- prevent fetched text from changing Rack AI authority/policy;
- fail closed when provider/fetch policy is violated.

SearXNG itself should be bound locally and configured as infrastructure, not treated as a model-controlled general browser.

## Reasoning integration

PR11 must integrate with the PR7/PR10 typed recovery path rather than adding an independent agent loop.

A recovery decision may request research. After ResearchEvidence is available, a fresh bounded reasoning step decides whether to:

- repair/replan;
- reassign;
- request more research within remaining budget;
- block/escalate.

Research must never implicitly increase write authority or acceptance scope.

## Evidence quality

Where possible, prefer primary technical sources such as official documentation, release notes, source repositories and upstream issue trackers. Search-engine ranking alone is not proof.

The reasoner must be told source provenance and should distinguish sourced facts from inference.

## Tests

Add deterministic tests for at least:

1. research is not triggered for a repository-local issue that semantic evidence can resolve;
2. diagnosis explicitly requests research and produces a bounded request;
3. provider results are converted to provenance-bearing ResearchEvidence;
4. response/source limits are enforced;
5. private-network/unsafe arbitrary fetch target is rejected;
6. prompt-like instructions in fetched content are treated as evidence text, not system instructions;
7. provider outage/timeouts lead to bounded recovery/escalation, not hanging;
8. research cannot modify files or path authority;
9. research evidence survives restart and is linked from attempt/recovery evidence.

Add an opt-in local integration test against a configured SearXNG instance or compatible local provider.

## Operational configuration

Configuration must be explicit and local, including provider base URL, budgets and allow/deny policy. Do not require external API keys or paid search services.

If SearXNG is unavailable, Rack AI must continue to function normally for campaigns that do not require research.

## Explicit non-goals

Do not add:

- general-purpose browsing by coding workers;
- paid/cloud research APIs;
- autonomous GitHub issue/PR actions based on web content;
- broad web crawling/indexing;
- vector database/RAG platform;
- persistent general memory;
- adaptive parallel scheduling;
- objective-to-campaign planning;
- another agent framework.

## Merge gate

PR11 may merge only when:

- PR10 is merged and its escalation/recovery interfaces are reused;
- research is bounded, provenance-bearing and policy-controlled;
- mutation workers remain network-isolated;
- unsafe fetch tests pass;
- provider unavailability fails safely;
- at least one real local research-assisted diagnosis is evidenced end-to-end.
