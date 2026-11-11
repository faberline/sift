# ADR 0003: Core-owned operations

- **Status:** Proposed.
- **Date:** 2026-11-11.
- **Applies to:** ROADMAP.md and the code named in its "Core-owned operations" section.

## Context

Sift uses 22 core crates at tag `v0.4.14`. lumen, tape and defer already moved
their operations to core-owned crates. Sift has no plan for the same move yet. The DDD layout that the move builds on is
[ADR 0002](0002-ddd-p2.md).

Core ADR 0002 defers Sift until the engine has a profile for its shape
(faberline/core#148). The next core tag is faberline/core#147.

## Decision

**Sift keeps:**

- the domain data and the domain model of every bounded context;
- the snapshot payload format;
- the archive record format. The persisted bytes do not change: WAL, segments,
  Raft snapshots, archive manifests and digests stay as they are.

**Core owns** the five rows of the "Core-owned operations" table in ROADMAP.md:
the backup set and restore ledger, the operator and CRD status primitives, the
directory generation swap, the ephemeral object store, and nothing for
`raft-core`, which is removed.

A move changes no persisted byte and no public CLI or API contract. Each row
moves in one pull request behind its own issue.

## Consequences

- Sift's code shrinks by about 2,800 lines (934 + 1,393 + 304 + 177).
- A row waits for its core dependency. The dependency bump (#66) comes first.
- When a fact in a row turns out to be wrong, the pull request that starts the
  row corrects the ROADMAP table.
