---
name: codify-workflow
description: Ground coding work in the local Codify graph, spec, evidence, and handoff lifecycle.
---

<!-- codify-owned: portable-agent-skill v1 -->
# Codify workflow

1. Run `cg brief`, then `cg spec next` and `cg spec start <id>`.
2. Use `cg survey` and `cg context` before broad reads.
3. Implement only the claimed task; record durable decisions with `cg remember`.
4. Run `cg review`, `cg guard`, and the task verification.
5. Snapshot with `cg commit`, qualify with `cg spec done`, and prove with `cg spec trace`.
Never force completion or treat a snapshot, declaration, or heartbeat as qualification.
