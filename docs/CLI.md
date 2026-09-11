# Koyote CLI Reference & User Guide

Koyote is autonomous software maintenance for systems that change. It detects contract drift, repairs the code, verifies against your real test suite, and delivers the evidence as a PR.

> **“Koyote understands the changes the outside world makes to software — and repairs them.”**

---

## The Public CLI Contract

```text
Core Maintenance Commands:
  koyote auth                     Connect & configure BYOK AI provider (needed for AI repair)
  koyote connect [owner/repo]     Connect GitHub account, choose repository, and issue Repository Key
  koyote active [owner/repo]      Show or switch the active repository working context
  koyote status                   Show current workspace and repository connection status
  koyote consult [path]           Consult mode: assess with AI reasoning, file Issue, modify nothing (aliases: howl, @howl)
  koyote work [path] [--provider] Work mode: repair from a finding ID or provider, sandbox-verify, report evidence, open PR (aliases: fix, maintain, update, hunt, @hunt)
  koyote disconnect [owner/repo]  Disconnect repository registration and clear active working context

Diagnostic & Advanced Commands:
  koyote doctor                   Product readiness: GitHub, AI, index, knowledge, tests
  koyote index [path]             Index repository contracts & callsites (AST evidence scan)
  koyote check [path]             Detect contract changes & impact (read-only; needs no AI key)
  koyote reviews [path]           List past maintenance runs from the ledger
  koyote onboard [path]           Guided setup: auth → connect → doctor
  koyote providers                List monitored contract sources & migrations
  koyote app serve                Run GitHub App webhook listener
  koyote pr                       Review a pull request with the contract guard

Legacy / advanced (workflows, sessions, lanes)
  koyote init                     Initialize a Koyote workspace (legacy path; onboarding no longer needs this)
  koyote status                   Show workspace health & active executions
  koyote inspect                  Dump declared compartments & policies

Agents
  koyote claude | opencode | codex | cursor | aider
                                   Run coding agent in governed OS sandbox
  koyote exec -- <cmd>            Run arbitrary command inside a compartment

Workflows
  koyote -w <name>                Create a new workflow branch
  koyote step <workflow> <target> Add a step with auto-inferred properties
  koyote --run <workflow>         Execute declared workflow DAG

Changes
  koyote diff                     Review change sets attributed by agent
  koyote apply                    Promote changes to workspace baseline
  koyote commit -m <msg>          Commit to Git with RFC-5322 metadata trailers
  koyote undo                     Instant physical snapshot rollback
  koyote restore                  Restore from session checkpoint
```

---

## 0. Product Onboarding

```bash
koyote auth              # Connect AI provider (OpenAI / Anthropic / local). Needed only for AI repair.
koyote doctor            # Readiness: GitHub, AI, Indexed, Knowledge Base, Test command, Monitoring
koyote index .           # AST evidence scan → .koyote/graph.json + knowledge test recipes
koyote check .           # Read-only drift & impact audit (works with no AI key configured)
koyote fix .             # Auto-detect provider, repair, sandbox-verify, report evidence
```

`check` never modifies code. `fix` with no safe path and no AI credentials refuses loudly
(`NOT RUN … REFUSED / INCOMPLETE`, zero files touched) instead of faking success.

---

## 1. Workspace Commands (legacy path)

> `init` is the legacy workspace path. Normal onboarding (`auth → index → check → fix`) does not need it.

### `koyote init`
Initializes a `.koyote/` control plane in the current directory:
- Detects installed agents (`claude`, `codex`, `opencode`, `cursor`, `aider`).
- Configures default security compartments (`default`, `research`, `builder`, `network`, `tester`).
- Sets up execution tracking and BLAKE3 snapshot storage.

```bash
koyote init
```

---

### `koyote status`
Shows live workspace health, active agents, recent executions, and security events.

```bash
koyote status
```

---

### `koyote inspect`
Dumps declarative topology, active compartments, filesystem permissions, and network policies.

```bash
koyote inspect
koyote inspect --json
```

---

## 2. Interactive Agent Execution

### Direct Agent Commands (`koyote <agent>`)
Launch any interactive coding agent inside an isolated kernel sandbox with full native terminal TUI fidelity (colors, alternate screen, Ctrl+C, Ctrl+D, window resizing):

```bash
koyote claude
koyote opencode
koyote codex
koyote cursor
koyote aider
```

**Under the Hood:**
1. Resolves genuine binary on system `PATH`.
2. Allocates a pseudo-terminal master/slave pair (`PtySupervisor`).
3. Takes a pre-execution BLAKE3 hash snapshot of the workspace.
4. Applies OS kernel sandboxing (Seatbelt on macOS / Landlock on Linux).
5. Captures file changes upon exit into `koyote diff`.

---

### `koyote exec`
Runs any arbitrary script, tool, or shell command inside an explicitly selected compartment:

```bash
# Run inside default compartment
koyote exec -- python3 script.py

# Run inside 'research' (read-only filesystem, network allowed)
koyote exec --compartment research -- python3 scraper.py

# Run inside 'builder' (read-write filesystem, network restricted)
koyote exec --compartment builder -- pytest tests/
```

---

## 3. Agentic Workflows (Git-Style Pipelines)

### `koyote -w <name>` (or `koyote workflow create <name>`)
Creates a new workflow branch in `workflows/<name>.yaml` or `.koyote/workflows/<name>.yaml`:

```bash
koyote -w invoice-pipeline
```

---

### `koyote step <workflow> <target>`
Adds steps to your workflow branch. Point Koyote at an individual file, a command, or an entire directory:

```bash
# Add a single script with auto-inferred runner & compartment
koyote step invoice-pipeline src/ocr.py

# Ingest an entire directory (scans and auto-chains scripts)
koyote step invoice-pipeline src/

# Add a test or shell command
koyote step invoice-pipeline "pytest tests/" --compartment tester
```

---

### `koyote --run <workflow>` (or `koyote run <workflow>`)
Executes the declared workflow DAG under kernel isolation:

```bash
koyote --run invoice-pipeline
```

- Topologically sorts the execution graph.
- Executes each step in its designated compartment (`research`, `builder`, `tester`, `reviewer`).
- If an upstream step fails, downstream dependent steps are cleanly `SKIPPED` to prevent cascading data corruption. Independent branches continue running.

---

### `koyote workflow show <workflow>`
Inspects and visualizes declared workflow DAG nodes, commands, and dependencies:

```bash
koyote workflow show invoice-pipeline
```

---

## 4. Change Management & Git Provenance

### `koyote diff`
Review change sets attributed by execution ID and agent name:

```bash
koyote diff               # Show all execution change sets
koyote diff --unapplied   # Only show pending changes not yet applied
koyote diff --trailers    # View formatted RFC-5322 Git metadata trailers
```

---

### `koyote apply`
Promotes an execution's recorded change set into the workspace baseline. Detects conflicts if another execution modified the same files.

```bash
koyote apply                         # Apply all pending completed executions
koyote apply --execution exec_101    # Apply a specific execution
koyote apply --force                 # Apply even if changes overlap
```

---

### `koyote commit`
Commits applied agent changes to Git, automatically embedding structured RFC-5322 metadata trailers for auditability and compliance:

```bash
koyote commit -m "feat(auth): implement token verification"
```

*Commit will contain metadata trailers (per the [Agent Provenance Trailers spec](../SPEC.md)):*
```text
Agent-Origin: agent
Agent-Agent: claude
Agent-Execution: exec_1787082469762
Agent-Compartment: builder
Agent-Sandbox: clean
```

---

### `koyote undo`
Physically restores the workspace to its exact state before the execution ran using the pre-execution BLAKE3 hash snapshot (restores in ~2 milliseconds):

```bash
koyote undo                         # Undo latest execution
koyote undo --execution exec_101    # Undo a specific execution
```

---

### `koyote restore [session_id]`
Restores workspace files from an Agent Session snapshot checkpoint:

```bash
koyote restore                       # Restores latest session checkpoint
koyote restore sess_1787082470931    # Restores specific session checkpoint
```

---

## 5. External-Change Intelligence & Autonomous Maintenance

### `koyote auth [--provider … --api-key …] [--status] [--clear]`
Connects a BYOK AI provider, saved to `~/.koyote/credentials.json` (0600), with per-installation
scoping available. Credentials are required only when AI reasoning/generation is actually needed —
`index` and `check` work without them. `--status` shows the masked active provider.

### `koyote doctor`
Prints product readiness: GitHub CONNECTED / NOT CONFIGURED, AI provider, repository Indexed state,
Knowledge Base READY / STALE / MISSING, detected test command, and monitoring ACTIVE / NOT ACTIVE,
with remediation hints.

### `koyote index [path]`
AST evidence scan: callsites, manifests, dependency graph → `.koyote/graph.json`, plus
`index_state.json` (commit SHA + mtimes) for incremental re-indexing and knowledge test recipes.

### `koyote check [path]` (alias: `scan`, `audit`)
Day-0 external-change dependency audit and risk register. Scans manifests, lockfiles, and AST callsites to report at-risk, deprecated, or breaking external integrations:

```bash
koyote check .
koyote check . --format=github-issue    # Markdown for GitHub Issue
koyote check . --format=json            # Machine-readable JSON risk register
koyote check . --write-graph            # Persists .koyote/graph.json
```

---

### `koyote graph [path]`
Queries and inspects the repository's External-Change Dependency Graph (providers, contracts, manifest dependencies, wrapper clients, AST callsites, and active edges):

```bash
koyote graph .
koyote graph . --json
```

---

### `koyote hunt <finding-id>` (aliases: `fix`, `maintain`, `update`, `work`, `@hunt`)
Hunt starts from a finding ID printed by `koyote check` — then rebuilds live
context (branch, exact SHA, active work, callsites, tests, verified memory),
reasons with AI, authors the patch with AI, verifies in an isolated sandbox
worktree, and fails closed with no PR unless the repair verifies green and
scope-clean. One Hunt runs per repository at a time (a second attempt waits
on a repo lock, then refuses loudly instead of overlapping):

```bash
koyote check .                # note the finding ID, e.g. stripe-3a9c79
koyote hunt stripe-3a9c79     # full reasoning → repair → sandbox → verify cycle
koyote hunt stripe-3a9c79 --create-pr --repo owner/repo
```

The provider-driven form runs the same engine with an explicit target:

```bash
koyote work .                       # Auto-detect provider from manifests
koyote work . --provider stripe
koyote work . --provider openai --from v3.28.0 --to v4.0.0
koyote work . --detect              # Detect installed API providers
koyote work . --show-pr             # Preview Developer Trust PR body
koyote work . --create-pr --repo owner/repo
```

All work-family names share one parser (`aliases=[...]`); `args.command`
carries the name actually typed.

### `koyote consult [path] [--repo owner/repo]`
Same AI reasoning as `fix` — codebase context, change context, maintenance memory, impact
analysis — but Consult authority: assess and report only. Files a GitHub Issue with findings,
affected files, inheritance notes, recommendation, and confidence, then declares no code was
modified. Requires AI credentials (refuses loudly without them); requires a resolvable repo
to file the Issue. Start here to build trust before enabling Work:

```bash
koyote consult . --repo owner/repo
```

### `koyote reviews [path]` / `koyote onboard [path]` / `koyote logout`
- `reviews` lists past maintenance runs (provider, versions, outcome) from `.koyote/history.json`.
- `onboard` chains `auth → index → doctor` as one guided setup.
- `logout` clears stored credentials. `auth --status` also reports GitHub identity when a token is present.

On refusal (no safe path, no AI credentials): `Repository Tests: NOT RUN`, zero files modified,
`REFUSED / INCOMPLETE`. Tests are never reported PASSED unless they actually ran and exited 0.

---

### `koyote providers`
Lists the built-in provider contract registry and available breaking-change migration specifications:

```bash
koyote providers
koyote providers --json
```

---

### `koyote app [serve|status]`
Runs the GitHub App continuous webhook listener daemon for automated PR drift detection and verification.
A webhook secret is required (fail-closed); `--no-secret` is local-debugging only:

```bash
koyote app serve --port 8080 --secret $KOYOTE_WEBHOOK_SECRET
```

On installation events Koyote persists the installation record, runs Day-0 indexing where a
checkout is available, and tracks per-repository state (PENDING → INDEXED → READY).

### `koyote pr [number]`
Runs the contract guard against a local checkout of a PR (mergeable only on real green tests).

