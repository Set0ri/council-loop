#!/usr/bin/env python3
"""Council Loop MCP server - serves the skill and the council tools to any MCP client.

ELI5: an MCP server is a plug. Any AI app that speaks MCP (Claude Code, Claude Desktop,
Cursor, Codex, Gemini CLI, ...) can plug in and get (1) the Council Loop instructions and
(2) the bookkeeping tools, so every app runs the same process the same way.

It is a thin wrapper: all logic lives in skills/council-loop/scripts/council.py, so there is
one enforcement path for the CLI, the plugin hooks, CI and MCP.

Run:  python3 mcp/server.py            (stdio; needs `pip install "mcp[cli]" pyyaml`)
      uv run --with "mcp[cli]" --with pyyaml python mcp/server.py
"""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKILL = ROOT / "skills" / "council-loop"
sys.path.insert(0, str(SKILL / "scripts"))

import council  # noqa: E402  (single source of truth)

try:  # MCP Python SDK v2
    from mcp.server import MCPServer as _Server
except ImportError:  # SDK v1.x
    from mcp.server.fastmcp import FastMCP as _Server

mcp = _Server("council-loop")


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8") if path.exists() else f"(missing: {path.name})"


# ---------------------------------------------------------------- resources (read-only context)
@mcp.resource("skill://council-loop/SKILL.md")
def skill_md() -> str:
    """The Council Loop skill (Agent Skills format). Read this first."""
    return _read(SKILL / "SKILL.md")


@mcp.resource("council://reference/{name}")
def reference(name: str) -> str:
    """Reference docs: protocol, context-and-memory, model-modes, research, commands."""
    safe = Path(name).name.removesuffix(".md")
    return _read(SKILL / "reference" / f"{safe}.md")


@mcp.resource("council://templates/{name}")
def template(name: str) -> str:
    """Stage templates: intent-brief, idea-cards, candidate-card, ledger, elder-packet, final-writeup, retro."""
    safe = Path(name).name
    path = SKILL / "templates" / safe
    return _read(path if path.suffix else path.with_suffix(".md"))


@mcp.resource("council://models/lock")
def models_lock() -> str:
    """Current model picks for every mode x profile (generated from Artificial Analysis data)."""
    return _read(SKILL / "models" / "lock.yaml")


# ---------------------------------------------------------------- prompts (slash commands)
@mcp.prompt()
def council_loop(task: str, tier: str = "auto", mode: str = "crosslab", profile: str = "balanced") -> str:
    """Run the Council Loop on a task. tier: auto|direct|lite|full. mode: solo|crosslab|fanout:<family>.
    profile: max|balanced|efficient."""
    gate = council.plan(task, tier, mode, profile)["gate"]
    return (_read(SKILL / "SKILL.md") + "\n\n---\n"
            f"TASK (Logan's words, verbatim): {task}\n{gate}\n"
            "Start by printing the GATE line, then follow the skill for that tier.")


# ---------------------------------------------------------------- tools (the bookkeeping)
@mcp.tool()
def council_plan(task: str, tier: str = "auto", mode: str = "crosslab", profile: str = "balanced",
                 loops: int = 3, paths: list[str] | None = None) -> dict:
    """Dry-run the gate: tier (tripwires can only raise it), models per role, cut schedule, checks.
    Spends no model tokens."""
    return council.plan(task, tier, mode, profile, loops, paths)


@mcp.tool()
def council_models(mode: str = "crosslab", profile: str = "balanced") -> dict:
    """Model picks per role for a mode (solo, crosslab, fanout:claude|gemini|openai|openrouter|open|mix)
    and profile (max, balanced, efficient)."""
    return council.models_for(mode, profile)


@mcp.tool()
def council_blind(cards: list[dict], seed: int = 0, max_words: int = 120) -> dict:
    """Anonymise + shuffle candidate cards ({id, author, title, body, eli5}) into a judge packet.
    Keep the returned mapping away from judges."""
    return council.blind(cards, seed, max_words)


@mcp.tool()
def council_tally(votes: list[dict], mapping: dict, scores: dict | None = None,
                  checks: dict | None = None, keep: int | None = None) -> dict:
    """Borda tally with self-vote exclusion; failed checks rank last; optional cut to `keep` with one
    Pareto wildcard."""
    return council.tally(votes, mapping, scores, checks, keep)


@mcp.tool()
def council_check_run(run: dict) -> dict:
    """Validate a run ledger against the protocol (idea counts, caps, ELI5s, loops, red team, elders)."""
    return council.check_run(run)


@mcp.tool()
def council_verify_locks() -> dict:
    """Report locked files (tests, migrations, ...) that changed. CI should run the same check."""
    return council.verify_locks()


@mcp.tool()
def council_elder_review(packet: str, profile: str = "balanced", providers: list[str] | None = None,
                         dry_run: bool = True) -> dict:
    """Send one blinded packet to the cross-lab elders (claude, openai, gemini, openrouter) in parallel.
    Defaults to dry_run=True: set False to actually call the CLIs/APIs."""
    import providers as prov
    return prov.elder_review(packet, profile, providers, dry_run=dry_run)


if __name__ == "__main__":
    mcp.run()
