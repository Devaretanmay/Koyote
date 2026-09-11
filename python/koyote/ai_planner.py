"""AI maintenance reasoning: deep repository + change context, exact-match application.

The model reasons over codebase context, change context, verified history, and
known failures; Koyote applies the resulting AI-authored SEARCH/REPLACE edits
by exact match only, verifies them in the sandbox, and quarantines on failure.
AI-first on top, exact-match execution underneath. No regex fixers, no rewrite
rules, and no deterministic fallback ever author source changes here.
"""

from __future__ import annotations

import difflib
import os
import re
from typing import Any, Dict, List

from koyote.graph import build_dependency_graph
from koyote.autopatch import ScanConfig, scan_callsites
from koyote.knowledge import lookup as kb_lookup
from koyote.llm import LLMClient, resolve_llm_config
from koyote.patch_writer import PatchResult
from koyote.test_runner import _detect_test_command


_BLOCK_REGEX = re.compile(
    r"<<<<<<< SEARCH\s*\n(.*?)\n=======\s*\n(.*?)\n>>>>>>> REPLACE",
    re.DOTALL,
)


def parse_search_replace_blocks(text: str) -> list[tuple[str, str]]:
    """Extract search and replace pairs from model output."""
    matches = _BLOCK_REGEX.findall(text)
    return [(search, replace) for search, replace in matches]


_CONFIDENCE_REGEX = re.compile(r"confidence\s*:\s*(high|medium|low)", re.IGNORECASE)


def parse_confidence(text: str) -> str:
    """Extract a trailing Confidence: high|medium|low line. Unknown when absent."""
    match = _CONFIDENCE_REGEX.search(text.replace("*", ""))
    return match.group(1).lower() if match else "unknown"


MAX_PROMPT_FILE_CHARS = 12000


def bound_file_content(content: str, limit: int = MAX_PROMPT_FILE_CHARS) -> tuple[str, bool]:
    """Cap file text sent to the model. Returns (text, truncated)."""
    if len(content) <= limit:
        return (content, False)
    return (content[:limit], True)


def ai_followup_for_missed(
    repo_dir: str,
    provider_name: str,
    from_version: str,
    to_version: str,
    touched_abs_paths: list[str],
    impact_files: list[str],
    migration_details: str = "",
    changelog_url: str = "",
    dry_run: bool = False,
) -> tuple[list[PatchResult], "AIPatchPlanner" | None]:
    """AI reasoning for affected files the evidence pass surfaced but left unpatched.

    Returns ([], None) when nothing is missed or no provider is configured —
    callers keep their honest refusal path untouched.
    """
    touched = set(touched_abs_paths or [])
    missed = [
        f for f in (impact_files or [])
        if os.path.isfile(os.path.join(repo_dir, f))
        and os.path.abspath(os.path.join(repo_dir, f)) not in touched
    ]
    if not missed:
        return ([], None)
    planner = AIPatchPlanner.from_env()
    if planner is None:
        return ([], None)
    context = build_reasoning_context(
        repo_dir, provider_name, from_version, to_version,
        migration_details, changelog_url)
    results = planner.plan_and_apply(
        repo_dir=repo_dir,
        affected_files=missed,
        provider_name=provider_name,
        from_version=from_version,
        to_version=to_version,
        migration_details=migration_details,
        dry_run=dry_run,
        context=context,
        changelog_url=changelog_url,
    )
    return (results, planner)


def build_reasoning_context(
    repo_dir: str,
    provider_name: str,
    from_version: str,
    to_version: str,
    migration_details: str = "",
    changelog_url: str = "",
    max_callsites: int = 40,
) -> dict[str, Any]:
    """Assemble what the model needs to reason like a maintainer, not a rewriter.

    Codebase context (wrappers, callsites, test command) + change context
    (migration, changelog) + maintenance memory (verified patterns to reuse,
    failed patterns to avoid). Best-effort throughout: missing pieces yield
    empty strings, never exceptions. Zero tokens to build — all static.
    """
    ctx: dict[str, Any] = {
        "test_command": "", "wrappers": [], "callsites": [],
        "verified_patterns": [], "failed_patterns": [],
    }
    try:
        ctx["test_command"] = _detect_test_command(repo_dir) or ""
    except Exception:
        pass
    try:
        graph = build_dependency_graph(repo_dir) or {}
        prov = provider_name.lower()
        for w in graph.get("wrappers", []) or []:
            hay = f"{w.get('wrapper_file', '')} {w.get('wraps_provider', '')}".lower()
            if prov and prov in hay and w.get("wrapper_file"):
                ctx["wrappers"].append(w["wrapper_file"])
    except Exception:
        pass
    try:
        res = scan_callsites(repo_dir, ScanConfig(sdk_names=[provider_name])) or {}
        for c in (res.get("callsites", []) or [])[:max_callsites]:
            line = str(c.get("line_content", "")).strip()[:200]
            ctx["callsites"].append(
                f"{c.get('file_path')}:{c.get('line_number')} [{c.get('kind')}] {line}")
    except Exception:
        pass
    try:
        entry = kb_lookup(repo_dir, provider_name, from_version, to_version)
        if entry:
            ctx["verified_patterns"] = [p.description for p in entry.patterns[:10] if p.description]
            ctx["failed_patterns"] = [f.get("description", "") for f in (entry.failed_patterns or [])[:5]]
            if not ctx["test_command"] and entry.test_recipe.get("test_command"):
                ctx["test_command"] = entry.test_recipe["test_command"]
    except Exception:
        pass
    ctx["migration_details"] = migration_details
    ctx["changelog_url"] = changelog_url
    return ctx


class AIPatchPlanner:
    """Generates surgical code patches using LLMs with self-repair support."""

    def __init__(self, client: LLMClient | None = None):
        self.client = client

    @classmethod
    def from_env(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        base_url: str | None = None,
    ) -> "AIPatchPlanner" | None:
        cfg = resolve_llm_config(api_key=api_key, model=model, base_url=base_url)
        if not cfg:
            return None
        return cls(client=LLMClient(cfg))

    def plan_and_apply(
        self,
        repo_dir: str,
        affected_files: list[str],
        provider_name: str,
        from_version: str,
        to_version: str,
        migration_details: str = "",
        test_error: str | None = None,
        dry_run: bool = False,
        context: Dict[str, Any] | None = None,
        changelog_url: str = "",
    ) -> list[PatchResult]:
        """Reason over deep context, then generate and apply AI patches."""
        if not self.client:
            return []

        if context is None:
            context = build_reasoning_context(
                repo_dir, provider_name, from_version, to_version,
                migration_details, changelog_url)

        results = []
        for file_path in affected_files:
            abs_path = file_path if os.path.isabs(file_path) else os.path.join(repo_dir, file_path)
            if not os.path.isfile(abs_path):
                continue

            with open(abs_path, "r", encoding="utf-8", errors="replace") as f:
                original_content = f.read()

            patch_res = self._patch_file(
                abs_path=abs_path,
                repo_dir=repo_dir,
                original_content=original_content,
                provider_name=provider_name,
                from_version=from_version,
                to_version=to_version,
                migration_details=migration_details,
                test_error=test_error,
                dry_run=dry_run,
                context=context,
            )
            if patch_res:
                results.append(patch_res)

        return results

    def assess(
        self,
        repo_dir: str,
        provider_name: str,
        from_version: str,
        to_version: str,
        migration_details: str = "",
        changelog_url: str = "",
        affected_files: List[str] | None = None,
        context: Dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Reason about impact without touching the worktree.

        Returns {"body", "affected_files", "confidence"}. Read-only by
        construction: the only tool used is the LLM completion call.
        """
        if not self.client:
            return {"body": "", "affected_files": affected_files or [], "confidence": "unknown"}

        if context is None:
            context = build_reasoning_context(
                repo_dir, provider_name, from_version, to_version,
                migration_details, changelog_url)

        prompt = (
            "Assess, do not modify. Answer in markdown addressing these exact questions:\n"
            "1. What changed upstream and why it matters in this repository?\n"
            "2. Which callsites are truly affected (file:line), and which inherit the fix via a wrapper?\n"
            "3. Which apparently similar callsites are actually unaffected?\n"
            "4. What must NOT be touched?\n"
            "5. What evidence supports this affected-set determination?\n"
            "6. What existing repository conventions and test commands constrain the solution?\n"
            "7. Is this impact provisional or confirmed, and what additional evidence "
            "would confirm or invalidate it?\n"
            "8. Does the current source state still represent the same change, or has "
            "it evolved since the observed push?\n"
            "9. Strict Rule: Never invent identifiers, APIs, or configuration. Reference only existing symbols.\n"
            'End with exactly one line: "Confidence: high|medium|low".'
        )
        messages = [{"role": "user", "content": prompt + "\n\n" + self._context_text(
            repo_dir, provider_name, from_version, to_version, context)}]
        try:
            resp = self.client.complete(messages=messages, system_prompt=(
                "You are Howl, Koyote's autonomous software maintenance reasoning agent. "
                "Diagnose the semantic impact of a dependency or contract change. "
                "You have read authority only. Never output code patches; explain only. "
                "Never hallucinate or invent nonexistent identifiers or APIs."))
        except Exception as exc:
            return {"body": "", "affected_files": affected_files or [],
                    "confidence": "unknown", "error": f"LLM call failed: {exc}"}
        return {"body": resp.content, "affected_files": affected_files or [],
                "confidence": parse_confidence(resp.content)}

    def _context_text(
        self,
        repo_dir: str,
        provider_name: str,
        from_version: str,
        to_version: str,
        context: dict[str, Any],
    ) -> str:
        """Render the shared reasoning context both assess and repair use."""
        sections = [
            f"System: {provider_name}",
            f"Change: {from_version} -> {to_version}",
        ]
        if context.get("migration_details"):
            sections.append(f"Change details: {context['migration_details']}")
        if context.get("changelog_url"):
            sections.append(f"Vendor guide: {context['changelog_url']}")
        if context.get("test_command"):
            sections.append(f"Repo verification: `{context['test_command']}` must keep passing")
        if context.get("wrappers"):
            sections.append("Wrappers to consider first:\n" + "\n".join(f"- {w}" for w in context["wrappers"][:10]))
        if context.get("callsites"):
            sections.append("Known usage across the repo:\n" + "\n".join(f"- {c}" for c in context["callsites"][:40]))
        if context.get("verified_patterns"):
            sections.append("Previously verified repairs (prefer these shapes):\n" + "\n".join(f"- {p}" for p in context["verified_patterns"][:10]))
        if context.get("failed_patterns"):
            sections.append("Known-bad approaches (do NOT repeat):\n" + "\n".join(f"- {p}" for p in context["failed_patterns"][:5] if p))
        return "\n".join(sections)

    def _patch_file(
        self,
        abs_path: str,
        repo_dir: str,
        original_content: str,
        provider_name: str,
        from_version: str,
        to_version: str,
        migration_details: str,
        test_error: str | None,
        dry_run: bool,
        context: Dict[str, Any] | None = None,
    ) -> PatchResult | None:
        context = context or {}
        system_prompt = (
            "You are Hunt, Koyote's autonomous software maintenance repair agent. "
            "A system your software depends on changed; determine what must change "
            "in this repository and repair exactly that — nothing more.\n"
            "Reason first about impact: which callsites are truly affected, which "
            "abstractions or wrappers must change first, which occurrences inherit "
            "the fix, and which unrelated code must NOT be touched.\n"
            "Strict Invariants:\n"
            "1. Only modify lines directly affected by the change.\n"
            "2. Preserve exact formatting, indentation, and unrelated logic.\n"
            "3. NEVER invent identifiers, files, symbols, APIs, or configuration.\n"
            "4. A nonexistent identifier is a reasoning failure.\n"
            "5. Minimal code only (YAGNI): no new abstractions, helpers, "
            "config options, or speculative generality. The smallest diff "
            "that fixes the affected lines wins; extra code is a defect.\n"
            "6. Do not refactor, rename, or 'improve' surrounding code.\n"
            "Emit surgical updates as search-and-replace blocks:\n"
            "<<<<<<< SEARCH\n"
            "exact lines to replace\n"
            "=======\n"
            "replacement lines\n"
            ">>>>>>> REPLACE\n"
            "Do not include markdown commentary outside the blocks."
        )

        if migration_details and migration_details != context.get("migration_details"):
            context = {**context, "migration_details": migration_details}
        sections = [
            f"File: {os.path.relpath(abs_path, repo_dir)}",
            self._context_text(repo_dir, provider_name, from_version, to_version, context),
        ]
        shown_content, truncated = bound_file_content(original_content)
        if truncated:
            sections.append(
                f"[File truncated to first {MAX_PROMPT_FILE_CHARS} chars "
                f"of {len(original_content)} — reason only over shown lines.]")
        user_content = "\n".join(sections) + f"\n\nFile Content:\n```\n{shown_content}\n```\n"

        if test_error:
            user_content += (
                f"\nNOTE: A previous patch attempt caused test failure:\n"
                f"```\n{test_error[:1500]}\n```\n"
                f"Please fix the code to resolve this test failure."
            )

        messages = [{"role": "user", "content": user_content}]
        try:
            resp = self.client.complete(messages=messages, system_prompt=system_prompt)
        except Exception as exc:
            return PatchResult(
                file_path=abs_path,
                success=False,
                lines_changed=0,
                unified_diff="",
                error=f"LLM call failed: {exc}",
            )

        blocks = parse_search_replace_blocks(resp.content)
        if not blocks:
            return None

        current = original_content
        applied_rules = []
        for search, replace in blocks:
            if search in current:
                current = current.replace(search, replace, 1)
                applied_rules.append(f"AI migration for {provider_name}")

        if current == original_content:
            return None

        orig_lines = original_content.splitlines(keepends=True)
        new_lines = current.splitlines(keepends=True)
        rel_path = os.path.relpath(abs_path, repo_dir)

        diff = "".join(difflib.unified_diff(
            orig_lines,
            new_lines,
            fromfile=f"a/{rel_path}",
            tofile=f"b/{rel_path}",
            lineterm="",
        ))

        lines_changed = sum(
            1 for line in diff.splitlines()
            if line.startswith(("+", "-")) and not line.startswith(("+++", "---"))
        )

        if not dry_run:
            with open(abs_path, "w", encoding="utf-8") as f:
                f.write(current)

        return PatchResult(
            file_path=abs_path,
            success=True,
            lines_changed=lines_changed,
            unified_diff=diff,
            rules_applied=applied_rules,
        )
