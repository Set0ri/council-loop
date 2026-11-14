"""The other two surfaces of the Rust binary, checked against the Python reference:

- `council mcp`: the MCP server. A real MCP client (the Python SDK) connects to both servers, lists
  what they offer and drives a whole Lite run through the tools. Both must offer the same tools with
  the same schemas and give the same answers.
- `council hook`: the Claude Code hook that denies edits to locked files. Both must decide the same
  for the same event.

Skipped when the binary is not built (`cargo build --release`) or the `mcp` package is missing.
"""
import asyncio
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

import sim
from test_parity import (ASK, BIN, ROOT, World, approve_pool, brief, compare, control, fill_fuse, same, seats, shop)

pytestmark = pytest.mark.skipif(not BIN.exists(), reason="the Rust binary is not built: cargo build --release")
SERVER = ROOT / "mcp" / "server.py"
HOOK = ROOT / "hooks" / "protect_locked_files.py"


# ----------------------------------------------------------------------------- MCP
def field(obj, *names):
    """A field of an SDK object, whichever of its spellings this SDK version uses."""
    for name in names:
        if hasattr(obj, name):
            return getattr(obj, name)
    raise AttributeError(names[0])


async def play_mcp(w: World) -> dict:
    """Connect to one implementation's server and drive it. Returns what it offers and every answer."""
    from mcp import ClientSession, StdioServerParameters
    from mcp.client.stdio import stdio_client

    env = w.env({"COUNCIL_NOW": "2026-11-13T12:00:00"})          # one clock for the whole session
    command, args = (sys.executable, [str(SERVER)]) if w.impl == "py" else (str(BIN), ["mcp"])
    log: list[dict] = []
    async with stdio_client(StdioServerParameters(command=command, args=args, env=env, cwd=str(w.root))) as (r, wr):
        async with ClientSession(r, wr) as s:
            init = await s.initialize()

            async def call(name: str, **kw) -> dict:
                out = await s.call_tool(name, kw)
                failed = bool(field(out, "is_error", "isError"))
                text = out.content[0].text
                answer = None if failed else json.loads(text)      # a refused call is worded by each SDK in its own way
                if not failed:
                    structured = field(out, "structured_content", "structuredContent")
                    assert structured is None or structured == answer, f"{name}: the text and the structured answer differ"
                log.append({"cmd": f"{name} {json.dumps(kw, sort_keys=True)[:90]}", "rc": int(failed),
                            "out": w.norm(json.dumps(answer, indent=1, ensure_ascii=False)), "err": ""})
                return answer if isinstance(answer, dict) else {}

            tools = field(await s.list_tools(), "tools")
            resources = field(await s.list_resources(), "resources")
            templates = field(await s.list_resource_templates(), "resource_templates", "resourceTemplates")
            prompts = field(await s.list_prompts(), "prompts")
            offered = {"protocol": field(init, "protocol_version", "protocolVersion"),
                       "tools": [t.model_dump(mode="json", by_alias=True, exclude_none=True) for t in tools],
                       "resources": [x.model_dump(mode="json", by_alias=True, exclude_none=True) for x in resources],
                       "templates": [x.model_dump(mode="json", by_alias=True, exclude_none=True) for x in templates],
                       "prompts": [x.model_dump(mode="json", by_alias=True, exclude_none=True) for x in prompts]}
            read = {}
            for uri in ("skill://council-loop/SKILL.md", "council://templates/ideate", "council://templates/brief.yaml",
                        "council://reference/protocol", "council://reference/protocol.md", "council://models/lock",
                        "council://templates/no-such-thing", "council://reference/..%2F..%2FSKILL", "council://templates/packets/vote",
                        "council://templates/..", "council://nothing/here"):
                try:
                    read[uri] = field(await s.read_resource(uri), "contents")[0].text
                except Exception as err:  # noqa: BLE001 - the SDK's own error type differs between its versions
                    read[uri] = f"refused: {type(err).__name__}"
            prompt = await s.get_prompt("council_loop", {"task": ASK, "tier": "lite", "mode": "fanout:claude"})
            read["prompt"] = w.norm(prompt.messages[0].content.text)

            root = str(w.root)
            # -- the building blocks
            await call("council_doctor")
            await call("council_models", mode="fanout:claude", profile="max")
            await call("council_models", mode="nope")
            await call("council_plan", task=ASK, tier="lite", mode="fanout:claude", research=1)
            await call("council_plan", task=ASK, tier="lite", mode="fanout:claude", project_dir=root, research=1, mapper="always",
                       paths=["db/migrations/001.sql"], loops=2, research_budget=5)
            await call("council_plan", task=ASK, project_dir="relative/path")
            cards = [{"id": f"c{i}", "title": f"Idea {i}", "body": " ".join(["word"] * (100 + i)), "eli5": f"simple {i}", "author": f"L{i + 1}"}
                     for i in range(4)]
            blind = await call("council_blind", cards=cards, seed=7)
            votes = [{"voter": f"L{i + 1}", "ranking": list(blind["mapping"])[i:] + list(blind["mapping"])[:i], "best_on": {"R1": "A"}}
                     for i in range(4)]
            await call("council_tally", votes=votes, mapping=blind["mapping"])
            await call("council_tally", votes=votes, mapping=blind["mapping"], keep=3, checks={"A": "fail"}, scores={"B": {"R2": 9}})
            await call("council_blind", cards=cards[:2], max_words=50)
            await call("council_check_run", run={"tier": "lite", "loops": [{}], "phase": "done"})
            await call("council_verify_locks")
            await call("council_elder_review", packet="### Candidate A: x\nbody\nELI5: y", profile="max", providers=["claude", "gemini"])
            await call("council_config", project_dir=root)
            await call("council_memory_status", project_dir=root)
            await call("council_memory_add", project_dir=root, source="https://example.org/a",
                       claim="Idempotency keys stop double charges when a queue retries", tags="queues, billing", confidence="high")
            await call("council_memory_add", project_dir=root, source="", claim="A claim with no source at all")
            await call("council_memory_search", project_dir=root, query="idempotency keys")
            await call("council_memory_search", project_dir=root, query="idempotency", tag="billing", limit=1)
            await call("council_memory_setup", project_dir=root, files_dir="notes/memory", for_project=True)
            await call("council_memory_setup", project_dir=root, obsidian_vault=str(w.base / "no-such-vault"))
            await call("council_memory_status", project_dir=root)
            for action in ("skeleton", "check", "validate", "packet", "accept", "install", "bogus"):
                await call("council_map", project_dir=root, action=action)
            w.write("map.md", sim.codebase_map(f"skeleton made by a script: `{w.root / '.council' / 'map' / 'skeleton.md'}`"))
            await call("council_map", project_dir=root, action="install", file="map.md")
            await call("council_map", project_dir=root, action="check")
            # -- calls that cannot be answered
            await call("council_next")
            await call("council_blind", cards="not a list")
            await call("council_status", project_dir=root)
            await call("council_next", project_dir="relative/path")
            # -- a whole Lite run through the tools
            await call("council_init", ask=ASK, project_dir=root, tier="lite", mode="fanout:claude", research=1, mapper="never",
                       paths=["shop/core.py"], stakes="medium", reversible="yes", research_budget=4, research_later="never", focus_tokens=300)
            await call("council_ingest", project_dir=root)       # the brief is not written yet
            await call("council_status", project_dir=root)
            await call("council_note", project_dir=root, text="A note before the brief is accepted")
            brief(w)
            await call("council_ingest", project_dir=root)
            seats(w, "loop-1/research")
            await call("council_note", project_dir=root, text="Does any queue keep order across partitions?", to="research")
            await call("council_dispatch", project_dir=root, dry_run=True)
            await call("council_ingest", project_dir=root)
            seats(w, "loop-1/ideate", only=("L1", "L2"))
            control(w)
            await call("council_next", project_dir=root, run_id=w.run()["id"])
            await call("council_ingest", project_dir=root)       # L3 has not answered
            await call("council_ingest", project_dir=root, drop="L3", reason="the seat never came back")
            approve_pool(w, 1)
            await call("council_ingest", project_dir=root)
            seats(w, "loop-1/redteam")
            await call("council_ingest", project_dir=root)
            fill_fuse(w, 1)
            await call("council_ingest", project_dir=root)
            seats(w, "loop-1/rank", only=("L1", "L2"))
            await call("council_ingest", project_dir=root, drop="L3", reason="the seat never came back")
            seats(w, "loop-1/elders")
            await call("council_ingest", project_dir=root)
            await call("council_deviation", project_dir=root, text="seat L3 was unreachable for the whole run")
            await call("council_cut", project_dir=root, eli5="")
            await call("council_cut", project_dir=root, eli5="two experts and one elder picked a queue", changed="keys on every consumer")
            d = w.run_dir()
            final = d / "final.md"
            final.write_text(final.read_text(encoding="utf-8").replace("TODO(chair)", "done"), encoding="utf-8")
            await call("council_ingest", project_dir=root)
            retro = d / "retro.md"
            retro.write_text(retro.read_text(encoding="utf-8").replace("TODO(chair)", "done"), encoding="utf-8")
            await call("council_ingest", project_dir=root)
            await call("council_status", project_dir=root)
            await call("council_audit", project_dir=root)
            await call("council_check_run", run=w.run())
            assert w.run()["phase"] == "done", log[-4]["out"]
    w.log = log
    return {"offered": offered, "read": read}


def test_both_mcp_servers_offer_and_answer_the_same(tmp_path):
    pytest.importorskip("mcp")
    worlds, seen = [], []
    for impl in ("py", "rs"):
        w = World(impl, tmp_path / impl)
        shop(w)
        seen.append(asyncio.run(play_mcp(w)))
        worlds.append(w)
    py, rs = seen
    assert py["offered"]["protocol"] == rs["offered"]["protocol"]
    assert len(py["offered"]["tools"]) == 23
    for part in ("tools", "resources", "templates", "prompts"):
        same(py["offered"][part], rs["offered"][part], f"what the servers offer: {part}")
    for uri in py["read"]:
        same(py["read"][uri], rs["read"][uri], f"resource {uri}")
    assert "name: council-loop" in rs["read"]["skill://council-loop/SKILL.md"] and "IDEATE" in rs["read"]["council://templates/ideate"]
    assert rs["read"]["council://reference/protocol"] == rs["read"]["council://reference/protocol.md"] != "(missing: protocol.md)"
    assert rs["read"]["council://reference/..%2F..%2FSKILL"].startswith("refused")        # a resource never leaves its folder
    compare(*worlds)


def test_the_mcp_server_survives_bad_input(tmp_path):
    """Lines that are not requests get a JSON-RPC error or silence, never a crash; and nothing but
    protocol messages reaches standard output."""
    lines = ['{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, '
             '"clientInfo": {"name": "t", "version": "0"}}}',
             '{"jsonrpc": "2.0", "method": "notifications/initialized"}',
             "this is not json", "", "[]", "5",
             '[{"jsonrpc": "2.0", "id": 2, "method": "ping"}, {"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {}}]',
             '{"jsonrpc": "2.0", "id": 3, "method": "no/such/method"}',
             '{"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "council_status", "arguments": {"project_dir": "%s"}}}'
             % str(tmp_path).replace("\\", "\\\\"),
             '{"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "council_doctor", "arguments": {"probe": false}}}',
             '{"jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": {"uri": "file:///etc/passwd"}}',
             '{"jsonrpc": "2.0", "id": "seven", "method": "prompts/get", "params": {"name": "council_loop", "arguments": {"task": "Pick a queue for billing"}}}']
    env = {k: v for k, v in os.environ.items() if not k.startswith("COUNCIL_")}
    env.update(COUNCIL_HOME=str(tmp_path / "home"), CLAUDECODE="1")
    p = subprocess.run([str(BIN), "mcp"], input="\n".join(lines) + "\n", capture_output=True, encoding="utf-8", env=env, cwd=tmp_path, timeout=120)
    assert p.returncode == 0, p.stderr
    replies = [json.loads(line) for line in p.stdout.splitlines()]                  # every line is one JSON message
    by_id = {r["id"]: r for r in replies if isinstance(r, dict)}
    assert by_id[1]["result"]["protocolVersion"] == "2025-06-18" and by_id[1]["result"]["serverInfo"]["name"] == "council-loop"
    assert by_id[None]["error"]["code"] in (-32700, -32600)                         # the line that was not JSON, or not a request
    assert [r for r in replies if isinstance(r, list)] == [[{"jsonrpc": "2.0", "id": 2, "result": {}}]]      # a batch: one reply, no reply to the notification
    assert by_id[3]["error"]["code"] == -32601
    status = by_id[4]["result"]
    assert status["isError"] is False and status["structuredContent"]["ok"] is False and "no current run" in status["structuredContent"]["output"]
    assert "recommended_mode" in by_id[5]["result"]["structuredContent"]
    assert by_id[6]["error"]["code"] == -32002
    assert "GATE tier=" in by_id["seven"]["result"]["messages"][0]["content"]["text"]
    assert len(replies) == 10


# ----------------------------------------------------------------------------- the hook
def hook(impl: str, project: Path, event, env_extra: dict | None = None, raw: bytes | None = None, cwd: Path | None = None):
    env = {k: v for k, v in os.environ.items() if k not in ("COUNCIL_ALLOW_LOCKED", "CLAUDE_PROJECT_DIR")}
    env.update({"CLAUDE_PROJECT_DIR": str(project), **(env_extra or {})})
    for key in [k for k, v in env.items() if v is None]:
        del env[key]
    cmd = [sys.executable, str(HOOK)] if impl == "py" else [str(BIN), "hook"]
    data = raw if raw is not None else json.dumps(event).encode("utf-8")
    p = subprocess.run(cmd, input=data, capture_output=True, env=env, cwd=cwd or project)
    return p.returncode, p.stdout.decode("utf-8"), p.stderr.decode("utf-8")


def test_both_hooks_decide_the_same(tmp_path):
    project = tmp_path / "proj"
    (project / ".council").mkdir(parents=True)
    (project / "tests").mkdir()
    (project / "tests" / "test_x.py").write_text("def test_x():\n    assert True\n", encoding="utf-8")
    (project / ".council" / "locks.json").write_text(json.dumps({"tests/test_x.py": "abc", "db/migrations/001.sql": "def", "café/test.py": "x"}),
                                                    encoding="utf-8")
    edit = lambda tool, **inp: {"tool_name": tool, "tool_input": inp}              # noqa: E731
    events = [
        edit("Edit", file_path=str(project / "tests" / "test_x.py")), edit("Edit", file_path="tests/test_x.py"),
        edit("Write", file_path="tests\\test_x.py"), edit("MultiEdit", file_path="./tests/../tests/test_x.py"),
        edit("NotebookEdit", notebook_path="db/migrations/001.sql"), edit("Edit", path=".council/locks.json"),
        edit("Edit", file_path="café/test.py"), edit("Edit", file_path=str(project / "src" / "app.py")), edit("Edit"),
        edit("Read", file_path="tests/test_x.py"), edit("Edit", file_path=""), edit("Edit", file_path=None),
        edit("Bash", command="sed -i 's/1/2/' tests/test_x.py"), edit("Bash", command="cat tests/test_x.py"),
        edit("Bash", command="echo x > tests/test_x.py"), edit("Bash", command="echo x >> db/migrations/001.sql && rm tests/test_x.py"),
        edit("Bash", command="Remove-Item tests\\test_x.py"), edit("Bash", command="GIT  CHECKOUT -- tests/test_x.py"),
        edit("Bash", command="git restore .council/locks.json"), edit("Bash", command="pytest -q tests/test_x.py"),
        edit("Bash", command="rm src/app.py"), edit("Bash", command="cp tests/test_x.py /tmp/copy.py"), edit("Bash"),
        edit("Bash", command="mv café/test.py elsewhere"), {"tool_name": "Edit"}, {"tool_name": "Edit", "tool_input": None},
        {"tool_input": {"file_path": "tests/test_x.py"}}, {}, [], "text", 5,
    ]
    denied = 0
    for event in events:
        answers = [hook(impl, project, event) for impl in ("py", "rs")]
        assert answers[0] == answers[1], f"{event}\npython: {answers[0]}\nrust:   {answers[1]}"
        assert answers[1][0] == 0
        denied += bool(answers[1][1])
    assert denied == 15
    out = json.loads(hook("rs", project, events[0])[1])
    assert out["hookSpecificOutput"]["permissionDecision"] == "deny" and "tests/test_x.py is locked" in out["hookSpecificOutput"]["permissionDecisionReason"]
    first = events[0]
    variants = [dict(env_extra={"COUNCIL_ALLOW_LOCKED": "1"}), dict(env_extra={"COUNCIL_ALLOW_LOCKED": "yes"}),
                dict(raw=b"\xef\xbb\xbf" + json.dumps(first).encode()), dict(raw=b"not json"), dict(raw=b""),
                dict(raw=json.dumps(first).encode("utf-16")),
                dict(env_extra={"CLAUDE_PROJECT_DIR": ""}, raw=json.dumps({**first, "cwd": str(project)}).encode()),
                dict(env_extra={"CLAUDE_PROJECT_DIR": ""}), dict(env_extra={"CLAUDE_PROJECT_DIR": str(tmp_path / "nowhere")})]
    for kw in variants:
        answers = [hook(impl, project, first, **kw) for impl in ("py", "rs")]
        assert answers[0] == answers[1], f"{kw}\npython: {answers[0]}\nrust:   {answers[1]}"
    (project / ".council" / "locks.json").write_text("[1, 2", encoding="utf-8")       # a manifest that is not JSON: let it pass
    assert hook("py", project, first) == hook("rs", project, first) == (0, "", "")
    (project / ".council" / "locks.json").write_text(json.dumps(["tests/test_x.py"]), encoding="utf-8")   # a list of paths is read too
    assert hook("py", project, first) == hook("rs", project, first) and hook("rs", project, first)[1]


# ----------------------------------------------------------------------------- the launchers
@pytest.mark.skipif(os.name != "nt", reason="bin\\council.cmd is the launcher for cmd and PowerShell")
def test_the_windows_launcher_runs_the_binary_when_there_is_one_and_python_when_not(tmp_path):
    project = tmp_path / "proj"
    (project / ".council").mkdir(parents=True)
    (project / ".council" / "locks.json").write_text(json.dumps({"tests/t.py": "x"}), encoding="utf-8")
    base = {k: v for k, v in os.environ.items() if not k.startswith("COUNCIL_")}
    base.update(COUNCIL_HOME=str(tmp_path / "home"), CLAUDECODE="1", CLAUDE_PROJECT_DIR=str(project))
    event = json.dumps({"tool_name": "Edit", "tool_input": {"file_path": "tests/t.py"}})

    def run(*args, stdin=None, **env):
        p = subprocess.run(["cmd", "/c", str(ROOT / "bin" / "council.cmd"), *args], capture_output=True, encoding="utf-8", cwd=project,
                           input=stdin, env={**base, **env}, timeout=120)
        return p.returncode, p.stdout, p.stderr

    for extra in ({"COUNCIL_BIN": str(BIN)}, {"COUNCIL_PYTHON": sys.executable}):       # the binary; the Python reference
        assert run("--version", **extra)[:2] == (0, "council 0.4.0\n")
        assert run("status", **extra)[0] == 2                                           # the exit code passes through
        code, out, _ = run("plan", "Pick (a) queue for billing & audit", "--stakes", "low", **extra)
        assert code == 0 and out.startswith("GATE tier=direct")                         # brackets and an ampersand in an argument
        code, out, _ = run("hook", stdin=event, **extra)
        assert code == 0 and json.loads(out)["hookSpecificOutput"]["permissionDecision"] == "deny"
    def which(**env) -> str:
        out = run("doctor", **env)[1]
        report = json.loads(out[out.index("{"):])
        return "rust" if "council" in report else "python"

    assert which(COUNCIL_BIN=str(BIN)) == "rust" and which(COUNCIL_PYTHON=sys.executable) == "python"


@pytest.mark.skipif(os.name == "nt", reason="the POSIX launcher; Windows has bin/council.cmd")
def test_the_launcher_runs_the_binary_when_there_is_one_and_python_when_not(tmp_path):
    """skills/council-loop/scripts/council is what the skill, the plugin hook and the MCP entry call.
    It must find the binary (COUNCIL_BIN, a build in the checkout, PATH) without ever calling itself
    in a circle, and fall back to the Python reference."""
    import shutil
    kit = tmp_path / "kit"                                       # a plugin folder without a built binary
    shutil.copytree(ROOT / "skills", kit / "skills", ignore=shutil.ignore_patterns("__pycache__"))
    for part in ("bin", "mcp", "hooks"):
        shutil.copytree(ROOT / part, kit / part, ignore=shutil.ignore_patterns("__pycache__"))
    installed = tmp_path / "installed"                           # a folder on PATH that holds the binary
    installed.mkdir()
    shutil.copy(BIN, installed / "council")
    project = tmp_path / "proj"
    (project / ".council").mkdir(parents=True)
    (project / ".council" / "locks.json").write_text(json.dumps({"tests/t.py": "x"}), encoding="utf-8")
    base = {k: v for k, v in os.environ.items() if not k.startswith("COUNCIL_")}
    base.update(COUNCIL_HOME=str(tmp_path / "home"), CLAUDECODE="1", CLAUDE_PROJECT_DIR=str(project))
    clean = os.pathsep.join(p for p in base["PATH"].split(os.pathsep) if not (Path(p) / "council").exists())

    def run(launcher, *args, path=clean, stdin=None, shell="sh", **env):
        p = subprocess.run([shell, str(launcher), *args], capture_output=True, encoding="utf-8", cwd=project, input=stdin,
                           env={**base, "PATH": path, **env}, timeout=120)
        return p.returncode, p.stdout, p.stderr

    def which(launcher, **kw) -> str:
        code, out, err = run(launcher, "doctor", **kw)
        assert code == 0, err
        report = json.loads(out[out.index("{"):])
        return "rust" if "council" in report else "python"

    plugin, skill = kit / "bin" / "council", kit / "skills" / "council-loop" / "scripts" / "council"
    assert which(plugin) == which(skill) == "python"                                     # nothing built, nothing installed
    assert which(plugin, COUNCIL_BIN=str(BIN)) == "rust"
    assert which(plugin, path=f"{installed}{os.pathsep}{clean}") == "rust"               # installed on PATH
    assert which(plugin, path=f"{kit / 'bin'}{os.pathsep}{installed}{os.pathsep}{clean}") == "rust"   # the launcher itself comes first on PATH
    assert which(plugin, path=f"{kit / 'bin'}{os.pathsep}{clean}") == "python"           # only the launcher on PATH: no circle
    assert which(ROOT / "bin" / "council") == "rust"                                     # a checkout with target/release
    if shutil.which("dash"):
        assert which(plugin, shell="dash", path=f"{installed}{os.pathsep}{clean}") == "rust"
    # exit codes and standard input pass through, in both modes
    event = json.dumps({"tool_name": "Edit", "tool_input": {"file_path": "tests/t.py"}})
    for extra in ({}, {"COUNCIL_BIN": str(BIN)}):
        assert run(plugin, "status", **extra)[0] == 2
        code, out, _ = run(plugin, "hook", stdin=event, **extra)
        assert code == 0 and json.loads(out)["hookSpecificOutput"]["permissionDecision"] == "deny"
    # a `python3` that is only the Microsoft Store's placeholder (Windows) is passed over for a real interpreter
    store, real = tmp_path / "WindowsApps", tmp_path / "real"
    store.mkdir()
    real.mkdir()
    (store / "python3").write_text("#!/bin/sh\necho 'Python was not found; run without arguments to install from the Microsoft Store' >&2\nexit 9\n",
                                   encoding="utf-8")
    (store / "python3").chmod(0o755)
    (real / "python").symlink_to(sys.executable)
    assert which(plugin, path=f"{store}{os.pathsep}{real}{os.pathsep}{clean}") == "python"
    code, _, err = run(plugin, "doctor", COUNCIL_PYTHON="no-such-python")
    assert code == 127 and "no Python found" in err
    # the plugin's hook line, run the way Claude Code runs it (sh -c on this system): it denies an edit to a
    # locked file, and it can never fail, because a hook that fails may block every tool call
    hooks = json.loads((ROOT / "hooks" / "hooks.json").read_text(encoding="utf-8"))
    command = hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
    assert command == 'sh "${CLAUDE_PLUGIN_ROOT}/bin/council" hook; exit 0'
    for root, verdict in ((kit, "deny"), (tmp_path / "no such plugin", None)):
        p = subprocess.run(["sh", "-c", command.replace("${CLAUDE_PLUGIN_ROOT}", str(root))], capture_output=True, encoding="utf-8",
                           cwd=project, input=event, env={**base, "PATH": clean}, timeout=120)
        assert p.returncode == 0, p.stderr
        assert (json.loads(p.stdout)["hookSpecificOutput"]["permissionDecision"] if p.stdout else None) == verdict
    # the MCP entry starts the binary itself: no shell is involved, so it is the same on every system
    server = json.loads((ROOT / ".mcp.json").read_text(encoding="utf-8"))["mcpServers"]["council-loop"]
    assert server == {"command": "council", "args": ["mcp"]}
    # a skill folder on its own (no plugin around it) still runs the loop, and says what it cannot serve
    alone = tmp_path / "alone" / "council-loop"
    shutil.copytree(ROOT / "skills" / "council-loop", alone, ignore=shutil.ignore_patterns("__pycache__"))
    assert which(alone / "scripts" / "council") == "python"
    code, _, err = run(alone / "scripts" / "council", "mcp")
    assert code == 127 and "Install the council binary" in err
