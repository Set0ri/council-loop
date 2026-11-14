#!/usr/bin/env python3
"""Council Loop MCP server - serves the skill and the council tools to any MCP client.

ELI5: an MCP server is a plug. Any AI app that speaks MCP (Claude Code, Claude Desktop,
Cursor, Codex, Gemini CLI, ...) can plug in and get (1) the Council Loop instructions and
(2) the bookkeeping tools, so every app runs the same process the same way.

It is a thin wrapper: all logic lives in skills/council-loop/scripts/ (council.py and run.py), so
there is one enforcement path for the CLI, the plugin hooks, CI and MCP.

The run-loop tools (council_init, council_next, council_ingest, ...) are the CLI verbs of the same
name. They take `project_dir`, the absolute path of the project the run belongs to, because a
server process does not share the client's working directory. They return the text the CLI would
print, which is the chair's exact next step.

Run:  python3 mcp/server.py            (stdio; needs `pip install "mcp[cli]" pyyaml`)
      uv run --with "mcp[cli]" --with pyyaml python mcp/server.py
"""
from __future__ import annotations

import contextlib
import inspect
import io
import os
import sys
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKILL = ROOT / "skills" / "council-loop"
sys.path.insert(0, str(SKILL / "scripts"))

import codemap  # noqa: E402
import council  # noqa: E402  (single source of truth)
import memory  # noqa: E402
import run as runner  # noqa: E402

try:  # MCP Python SDK v2
    from mcp.server import MCPServer as _Server
except ImportError:  # SDK v1.x
    from mcp.server.fastmcp import FastMCP as _Server

mcp = _Server("council-loop")


def _plain_docs(register):
    """Register a tool, prompt or resource with its docstring as Python 3.13 keeps it: without the
    indentation of its later lines. Python 3.10 to 3.12 keep that indentation, and every client would
    read four spaces before each line but the first."""
    def outer(*args, **kwargs):
        decorate = register(*args, **kwargs)

        def inner(fn):
            if fn.__doc__:
                fn.__doc__ = inspect.cleandoc(fn.__doc__)
            return decorate(fn)
        return inner
    return outer


for _kind in ("tool", "prompt", "resource"):
    setattr(mcp, _kind, _plain_docs(getattr(mcp, _kind)))


_CWD = threading.Lock()                      # the run loop finds its project from the working directory


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8") if path.exists() else f"(missing: {path.name})"


def _in_project(project_dir: str, fn, *args) -> dict:
    """Run one CLI verb inside a project and return what it would have printed."""
    root = Path(project_dir).expanduser()
    if not root.is_absolute() or not root.is_dir():
        return {"ok": False, "output": f"project_dir must be an existing absolute directory (got {project_dir!r})"}
    buf = io.StringIO()
    with _CWD:
        before = os.getcwd()
        try:
            os.chdir(root)
            with contextlib.redirect_stdout(buf):
                code = fn(*args)
            return {"ok": code == 0, "output": buf.getvalue().strip()}
        except (runner.RunError, memory.MemoryError_) as err:    # not a crash: the tool says what is still owed
            return {"ok": False, "output": (buf.getvalue() + f"\nerror: {err}").strip()}
        finally:
            os.chdir(before)


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
    """Templates: brief.yaml, final-writeup.md, run.example.json, and the seat packets ideate, vote,
    redteam, elder and control (e.g. council://templates/ideate)."""
    safe = Path(name).name
    for path in (SKILL / "templates" / safe, SKILL / "templates" / f"{safe}.md",
                 SKILL / "templates" / "packets" / safe, SKILL / "templates" / "packets" / f"{safe}.md"):
        if path.is_file():
            return path.read_text(encoding="utf-8")
    return f"(missing: {safe})"


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
            f"TASK (the requester's words, verbatim): {task}\n{gate}\n"
            "Start by showing the GATE line, then follow the skill for that tier. Without a shell, use this "
            "server's tools in place of the commands: council_init, then council_next and council_ingest in a "
            "loop, council_cut at the end of each loop, council_status after a context reset.")


# ---------------------------------------------------------------- tools: the run loop
@mcp.tool()
def council_doctor(probe: bool = False) -> dict:
    """What can be called from the machine this server runs on (CLIs, keys, model-data age) and which
    mode fits. probe=True sends one tiny test prompt to each installed CLI (an installed CLI may be
    logged out)."""
    return council.doctor(probe=probe)


@mcp.tool()
def council_init(ask: str, project_dir: str, tier: str = "auto", mode: str | None = None, profile: str = "balanced",
                 loops: int | None = None, stakes: str | None = None, reversible: str | None = None,
                 override: str | None = None, paths: list[str] | None = None, research: int | None = None,
                 mapper: str | None = None, obsidian: bool = False, research_budget: int | None = None,
                 research_later: str | None = None, focus_tokens: int | None = None) -> dict:
    """Start a run in project_dir. `ask` is the requester's own words, verbatim. Returns the GATE line,
    the MEMBERS line and the first step. stakes: low|medium|high. reversible: yes|no. override: the
    requester's own words when they set the tier themselves.
    Support members (leave unset to use the project's config): research = how many researchers, 0 to 4;
    research_budget = searches per researcher; research_later = on_request|always|never; mapper =
    stale|always|never (when the codebase map is rewritten); focus_tokens = size of the ranked code list
    in packets. obsidian=True keeps the run's shared memory in the user's Obsidian vault (set once with
    council_memory_setup)."""
    return _in_project(project_dir, runner.cmd_init, ask, tier, mode, profile, loops, paths or [], stakes,
                       reversible, override, research, mapper, obsidian, research_budget, research_later, focus_tokens)


@mcp.tool()
def council_next(project_dir: str, run_id: str | None = None, without_control: str | None = None) -> dict:
    """Prepare the current step (writes the seats' prompt files) and return exactly what to do now.
    Safe to call again: after a restart it lists only the seats that still owe an output."""
    return _in_project(project_dir, runner.cmd_next, run_id, without_control)


@mcp.tool()
def council_ingest(project_dir: str, run_id: str | None = None, drop: str | None = None,
                   reason: str | None = None) -> dict:
    """Check what came back for the current step; advance when it is all valid, else say what is owed.
    drop: seat ids to go on without (e.g. "L3" or "EB,EC"), with a reason. Never drops a seat you did not name."""
    return _in_project(project_dir, runner.cmd_ingest, run_id, drop, reason)


@mcp.tool()
def council_dispatch(project_dir: str, run_id: str | None = None, seat: str | None = None,
                     dry_run: bool = False) -> dict:
    """Run the seats of the current step that live behind a CLI or API (codex, gemini, claude -p,
    OpenRouter). Sends prompts to those vendors. dry_run=True only shows the commands."""
    return _in_project(project_dir, runner.cmd_dispatch, run_id, seat, dry_run)


@mcp.tool()
def council_cut(project_dir: str, eli5: str, changed: str = "", stop: str | None = None, asked: str | None = None,
                unattended: str | None = None, run_id: str | None = None) -> dict:
    """End the loop: apply the vote, checks and elder verdicts, write the ledger, start the next loop or
    the final. eli5: one line on what happened. At a judgment point pass asked ("question -> answer") or
    unattended (the safer option taken)."""
    return _in_project(project_dir, runner.cmd_cut, run_id, eli5, changed, stop, asked, unattended)


@mcp.tool()
def council_status(project_dir: str, run_id: str | None = None) -> dict:
    """One-screen digest of the run and what the current step still owes. Use it after a context reset."""
    return _in_project(project_dir, runner.cmd_status, run_id)


@mcp.tool()
def council_deviation(project_dir: str, text: str, run_id: str | None = None) -> dict:
    """Record that the run left the protocol, and why. It reaches the ledger, the final and the retro."""
    return _in_project(project_dir, runner.cmd_deviation, run_id, text)


@mcp.tool()
def council_audit(project_dir: str, run_id: str | None = None) -> dict:
    """Re-count every vote of a run from the ballots on disk and compare with the record."""
    return _in_project(project_dir, runner.cmd_audit, run_id)


# ---------------------------------------------------------------- tools: shared memory and the codebase map
@mcp.tool()
def council_note(project_dir: str, text: str, to: str = "all", run_id: str | None = None) -> dict:
    """Post to the current run's commons as the chair. to: "all" (members read it unsigned at the next
    ideation or red-team step), "research" (a question the researchers take up before the next loop)
    or "chair" (kept for the record)."""
    return _in_project(project_dir, runner.cmd_note, run_id, text, to)


@mcp.tool()
def council_memory_search(project_dir: str, query: str, tag: str | None = None, limit: int = 10) -> dict:
    """Find facts in the project's research memory by keyword. Each fact has a source and a confidence."""
    return _in_project(project_dir, memory.cmd_search, query, tag, limit)


@mcp.tool()
def council_memory_add(project_dir: str, claim: str, source: str, quote: str = "", kind: str = "other",
                       confidence: str = "medium", tags: str = "", date: str = "", why: str = "") -> dict:
    """Add one fact to the research memory by hand. source: a web address you opened, a file in the
    project, or "test: <command>". A fact without a source is refused."""
    return _in_project(project_dir, memory.cmd_add, claim, source, quote, kind, confidence, tags, date, why, "chair")


@mcp.tool()
def council_memory_status(project_dir: str) -> dict:
    """Where the shared memory lives (project files or an Obsidian vault) and what it holds."""
    return _in_project(project_dir, memory.cmd_status)


@mcp.tool()
def council_memory_setup(project_dir: str, obsidian_vault: str | None = None, folder: str | None = None,
                         files_dir: str | None = None, for_project: bool = False) -> dict:
    """Choose where the shared memory lives. obsidian_vault: the path of the user's Obsidian vault on the
    machine this server runs on (saved in the user's own settings unless for_project). Without it:
    plain files, in files_dir or the project's .council/memory."""
    return _in_project(project_dir, memory.cmd_setup, bool(obsidian_vault), obsidian_vault, folder,
                       not obsidian_vault, files_dir, for_project)


@mcp.tool()
def council_map(project_dir: str, action: str = "skeleton", file: str | None = None) -> dict:
    """The codebase map (docs/ARCHITECTURE.md). action: "skeleton" writes the script-made skeleton and
    says whether the map is behind the code; "check" fails when it is; "validate" checks the written
    map (sections, and that every path it names exists); "packet" writes the mapper's packet so one
    fresh agent can refresh the map; "install" validates `file` and puts it in place; "accept" records
    that the map describes the code as it is now."""
    if action == "packet":
        return _in_project(project_dir, runner.cmd_map_packet)
    if action not in ("skeleton", "check", "validate", "accept", "install"):
        return {"ok": False, "output": "action must be skeleton, check, validate, packet, install or accept"}
    if action == "install" and not file:
        return {"ok": False, "output": "install needs `file`: the Markdown document to install"}
    return _in_project(project_dir, codemap.cmd_map, action == "check", action == "accept", action == "validate",
                       None, 420, file if action == "install" else None)


# ---------------------------------------------------------------- tools: building blocks
@mcp.tool()
def council_plan(task: str, tier: str = "auto", mode: str = "crosslab", profile: str = "balanced",
                 loops: int | None = None, paths: list[str] | None = None, project_dir: str | None = None,
                 research: int | None = None, mapper: str | None = None, research_budget: int | None = None) -> dict:
    """Dry-run the gate: tier (tripwires can only raise it), models per role, cut schedule, the support
    members (research = how many researchers, 0 to 4; mapper = stale|always|never) and the estimated
    seat calls and tokens. Spends no model tokens. Pass project_dir to use that project's config and to
    count the mapper when its map is missing or behind its code."""
    root = Path(project_dir).expanduser() if project_dir else None
    if root is None or not root.is_absolute() or not root.is_dir():
        return council.plan(task, tier, mode, profile, loops, paths, research=research, mapper="never",
                            research_budget=research_budget)
    with _CWD:
        before = os.getcwd()
        try:
            os.chdir(root)
            return council.plan(task, tier, mode, profile, loops, paths, research=research, mapper=mapper,
                                research_budget=research_budget)
        finally:
            os.chdir(before)


@mcp.tool()
def council_config(project_dir: str) -> dict:
    """Every setting for the researchers, the mapper and the shared memory in this project, and who set
    it (a flag, the project's .council/config.yaml, the user's ~/.council/config.yaml, or the default)."""
    def show() -> int:
        root = council.project_root()
        print("\n".join(council.members_report(council.members(root), memory.settings(root), root)))
        return 0
    return _in_project(project_dir, show)


@mcp.tool()
def council_models(mode: str = "crosslab", profile: str = "balanced") -> dict:
    """Model picks per role for a mode (solo, crosslab, fanout:claude|gemini|openai|openrouter|open|mix)
    and profile (max, balanced, efficient)."""
    return council.models_for(mode, profile)


@mcp.tool()
def council_blind(cards: list[dict], seed: int = 0, max_words: int = 120) -> dict:
    """Anonymise + shuffle candidate cards ({id, authors, title, body, eli5}) into a judge packet, and
    refuse cards that say who wrote them. Keep the returned mapping away from judges."""
    return council.blind(cards, seed, max_words)


@mcp.tool()
def council_tally(votes: list[dict], mapping: dict, scores: dict | None = None,
                  checks: dict | None = None, keep: int | None = None) -> dict:
    """Count blind ballots as head-to-heads weighted by each voter's stake in the cards (a voter's own
    card earns nothing from that voter). Failed checks rank last. Reports `thin` pairs the vote could not
    judge. Optional cut to `keep` with one Pareto wildcard."""
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
