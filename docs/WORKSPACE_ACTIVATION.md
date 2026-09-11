# Koyote Workspace Initialization & Agent Execution

## 1. What It Is

When you run `koyote init`, Koyote turns your project directory into a **managed agent workspace**. You launch your favorite agent directly inside an isolated kernel sandbox.

---

## 2. How It Works

```text
koyote init
└── creates .koyote/
    ├── config.yaml    <- workspace compartment policy
    ├── state/         <- runtime state
    ├── snapshots/     <- BLAKE3 worktree diff snapshots
    └── executions/    <- execution records

Direct Execution:
  $ koyote claude      -> Launches Claude Code in kernel sandbox
  $ koyote opencode    -> Launches OpenCode in kernel sandbox
  $ koyote codex       -> Launches Codex in kernel sandbox
  $ koyote cursor      -> Launches Cursor in kernel sandbox
  $ koyote aider       -> Launches Aider in kernel sandbox
```

---

## 3. Running Interactive Coding Agents

```bash
koyote claude
koyote opencode
koyote codex
koyote cursor
koyote aider
```

Each interactive agent runs with:
- Full native TUI support (colors, alternate screen, Ctrl+C, Ctrl+D, window resize).
- Hard OS-level kernel isolation (Seatbelt on macOS / Landlock on Linux).
- Deny-by-default credential protection (`~/.ssh`, `~/.aws`, `~/.config/gcloud` blocked).
- Automatic BLAKE3 pre-execution snapshots for physical instant rollback (`koyote undo`).

---

## 4. Checking Workspace Health

```bash
koyote status
```

```text
================================================================================
                              KOYOTE STATUS
================================================================================

GitHub:             CONNECTED (Devaretanmay)
AI:                 CONNECTED (groq)
Active repo:        acme/checkout-service
Repositories:       3
Repository Key:     kyp_da1358315f6c9d1ad8791cbf8cb9
Howl:               AVAILABLE
Hunt:               AVAILABLE
Status:             READY

================================================================================
```

---

## 5. Running Multi-Agent Workflows

Run a declared workflow DAG:

```bash
koyote --run invoice-pipeline
```

Or run standalone Python agent scripts:

```bash
koyote exec --compartment research -- python3 scraper.py
```
