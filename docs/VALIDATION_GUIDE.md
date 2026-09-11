# Product Validation Guide: Testing Koyote with Your AI Agents

This guide provides a quick test suite for validating Koyote as the audit and control layer for your AI agents and codebase dependencies.

---

## The Core Validation Model

```text
WITHOUT KOYOTE
Agent -> Tools -> OS / Credentials / Network (Unmonitored)

WITH KOYOTE
Agent -> KOYOTE (Topology, Policies, Proxy, Snapshots) -> OS (Kernel Enforced)
```

---

## 1. Test Scenario 1: CLI Coding Agent (Claude Code / Shell Agent)

Validate that an AI agent reading your repo and executing bash commands is blocked from reading `~/.ssh` or `~/.aws` credentials while executing workspace tasks cleanly.

```bash
koyote claude
# or arbitrary command execution:
koyote exec -- cat ~/.ssh/id_rsa
```

### What You Observe:
- Host SSH credential access is blocked by the OS kernel.
- Workspace file modifications are tracked with BLAKE3 file diffs.
- `koyote diff` isolates agent modifications.
- `koyote undo` restores workspace state in 2ms.

---

## 2. Test Scenario 2: Day-0 External Dependency Audit & Risk Register

Audit your entire codebase for upstream breaking changes, deprecated API callsites, and auto-repairable integrations.

```bash
koyote check .
koyote check . --format=github-issue
```

### What You Observe:
- AST parser maps all external SDK and API callsites across TypeScript, Python, and Go.
- Categorizes dependencies into At-Risk, Watchlist, and Healthy.
- Generates formatted risk register markdown ready for GitHub Issues.

---

## 3. Test Scenario 3: Autonomous Repair Loop

Detect upstream API drift, then repair from a finding ID with AI-authored,
sandbox-verified patches against breaking changes.

```bash
koyote check .                # note the finding ID, e.g. stripe-3a9c79
koyote hunt stripe-3a9c79     # full reasoning → repair → sandbox → verify cycle
```

### What You Observe:
- Scans manifests and callsites against provider contracts.
- AI reasons about impact, authors the repair, and verifies it in an isolated sandbox worktree with the real test command.
- Validates blast-radius constraints; refusals are loud with zero files touched, and only sealed green repairs may proceed to a Developer Trust PR.

---

## The Validation Question

After running these validation scenarios on your codebase:

> **"Would you run your coding agents without Koyote?"**
