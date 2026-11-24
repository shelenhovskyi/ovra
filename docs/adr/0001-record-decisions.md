# 0001. Record architecture decisions as ADRs

- Status: accepted
- Date: 2026-11-24

## Context

Most code is written by agents that start every session without memory.
Decisions that live only in chat get lost and are re-litigated.

## Decision

Every architecture decision is an ADR in `docs/adr/`. Agents read relevant
ADRs before changing code; a new ADR or a deviation needs human approval.

## Consequences

Decisions are cheap to find. Adding one costs a short document and a review.
